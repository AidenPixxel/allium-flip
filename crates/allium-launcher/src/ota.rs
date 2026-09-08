use anyhow::{Context, Result};
use common::constants::{ALLIUM_SD_ROOT, ALLIUM_UPDATE_SETTINGS};
use common::state_file;
use const_hex::ToHexExt;
use log::info;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use std::time::Instant;
use tokio::sync::mpsc;

/// This fork's own releases. Pointing at upstream offered an "update" that would restore every core
/// trimmed from this build and replace alliumd and the launcher with stock binaries.
const GITHUB_REPOSITORY: &str = "AidenPixxel/allium-flip";
const RELEASE_FILE: &str = "allium-armv7-unknown-linux-gnueabihf.zip";
const USER_AGENT: &str = "Allium-OTA-Updater";

static UPDATE_FILE_PATH: LazyLock<PathBuf> =
    LazyLock::new(|| ALLIUM_SD_ROOT.join("allium-ota.zip"));

/// The device has no system certificate store, so the Mozilla root set is compiled in instead of
/// going through reqwest's platform verifier.
static TLS_CONFIG: LazyLock<rustls::ClientConfig> = LazyLock::new(|| {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("ring supports the default protocol versions")
    .with_root_certificates(roots)
    .with_no_client_auth();

    // A preconfigured config bypasses reqwest's own ALPN setup, and http2 is not compiled in.
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    config
});

fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .tls_backend_preconfigured(TLS_CONFIG.clone())
        .build()
        .context("Failed to build HTTP client")
}

/// Whether the System Update screen contacts GitHub at all.
///
/// Two states, not three: this fork's CI never marks a release as a prerelease, so a separate
/// "nightly" channel could only ever fail its lookup. Discriminant order matters -- the settings
/// screen indexes its Select by `channel as usize`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum UpdateChannel {
    /// Never contact GitHub
    Off,
    /// Offer the latest release. The old `Stable` and `Nightly` values read as this, so an
    /// update.json written before the channels were collapsed keeps updates on.
    #[default]
    #[serde(alias = "Stable", alias = "Nightly")]
    On,
}

/// Update settings that are persisted to disk
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateSettings {
    pub channel: UpdateChannel,
}

impl UpdateSettings {
    pub fn load() -> Result<Self> {
        Ok(state_file::load_or_default(
            ALLIUM_UPDATE_SETTINGS.as_path(),
            "update",
        ))
    }

    pub fn save(&self) -> Result<()> {
        state_file::save(ALLIUM_UPDATE_SETTINGS.as_path(), self)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubRelease {
    pub tag_name: String,
    pub assets: Vec<GitHubAsset>,
    #[serde(default)]
    pub prerelease: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub digest: Option<String>,
}

/// The release to offer, or `None` when the device is up to date or the channel is off.
pub async fn check_for_update(channel: UpdateChannel) -> Result<Option<GitHubRelease>> {
    let current_version = &*common::constants::ALLIUM_VERSION;
    info!("Current version: {}", current_version);

    let release = match channel {
        // Bail out before any network call
        UpdateChannel::Off => {
            info!("Update channel is off, skipping update check");
            return Ok(None);
        }
        UpdateChannel::On => get_latest_release().await?,
    };
    let latest_version = get_release_version(&release);
    info!("Latest version: {}", latest_version);

    Ok(is_newer(&latest_version, current_version).then_some(release))
}

/// The version a release is known by: its tag, verbatim -- `v1.0.1-flip.14` on this fork.
pub fn get_release_version(release: &GitHubRelease) -> String {
    release.tag_name.clone()
}

/// Whether `latest` should be offered over `current`.
///
/// Comparing the tags for inequality, as this used to, offered a *downgrade* whenever the device
/// ran anything but the newest build -- a side-branch artifact, say. Both are parsed and compared
/// numerically. If either does not parse (a build with no version.txt reads `unknown`), inequality
/// is the fallback: offering an update is the safe direction for a device whose version cannot be
/// established.
fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => latest != current,
    }
}

/// `v1.0.1-flip.14` as `(1, 0, 1, 14)`. A plain `v1.0.1` gets a fourth part of 0, so it sorts
/// before every `-flip.N` build of the same base version.
fn parse_version(tag: &str) -> Option<(u32, u32, u32, u32)> {
    let tag = tag.trim().trim_start_matches('v');
    let (base, build) = match tag.split_once("-flip.") {
        Some((base, build)) => (base, build.parse().ok()?),
        None => (tag, 0),
    };
    let mut parts = base.split('.').map(|part| part.parse::<u32>().ok());
    let major = parts.next()??;
    let minor = parts.next()??;
    let patch = parts.next()??;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch, build))
}

/// Get the latest release from GitHub
async fn get_latest_release() -> Result<GitHubRelease> {
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        GITHUB_REPOSITORY
    );

    let client = client()?;

    let release: GitHubRelease = client
        .get(&url)
        .send()
        .await
        .context("Failed to fetch latest release")?
        .json()
        .await
        .context("Failed to parse release JSON")?;

    Ok(release)
}

/// Download progress information
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
}

impl DownloadProgress {
    pub fn percentage(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            self.downloaded as f32 / self.total as f32 * 100.0
        }
    }
}

/// Download event - either progress or completion/error
#[derive(Debug, Clone)]
pub enum DownloadEvent {
    Progress(DownloadProgress),
    Completed,
    Error(String),
}

/// Download a release to /mnt/SDCARD/allium-ota.zip, reporting the progress to event_tx
pub async fn download_update_with_progress(
    release: &GitHubRelease,
    event_tx: Option<mpsc::UnboundedSender<DownloadEvent>>,
) -> Result<()> {
    // Check if there's enough space (need 300MB)
    #[cfg(feature = "miyoo")]
    {
        let output = std::process::Command::new("df")
            .args(["-m", "/mnt/SDCARD"])
            .output()
            .context("Failed to check disk space")?;

        let output_str = String::from_utf8_lossy(&output.stdout);
        let available_space: i32 = output_str
            .lines()
            .nth(1)
            .and_then(|line| line.split_whitespace().nth(3))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        if available_space < 300 {
            anyhow::bail!(
                "Insufficient disk space. Need 300MB, have {}MB",
                available_space
            );
        }
    }

    info!("Downloading update version {}", release.tag_name);

    // Find the asset with the expected filename
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == RELEASE_FILE)
        .context(format!("Asset '{}' not found in release", RELEASE_FILE))?;

    let expected_hash = asset
        .digest
        .as_ref()
        .and_then(|d| d.strip_prefix("sha256:"))
        .context("SHA256 digest not found for release asset")?;

    info!("Downloading from: {}", asset.browser_download_url);
    let mut response = client()?
        .get(&asset.browser_download_url)
        .send()
        .await
        .context("Failed to download update")?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to download update: HTTP {}", response.status());
    }

    // Get content length for progress reporting
    let total_size = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;

    // Create file and hasher
    info!("Writing update to {}", UPDATE_FILE_PATH.display());
    let file = File::create(&*UPDATE_FILE_PATH).context("Failed to create update file")?;
    let mut writer = BufWriter::new(file);
    let mut hasher = Sha256::new();

    // Stream download to file while calculating hash
    let mut last_progress_update = Instant::now();
    while let Some(chunk) = response.chunk().await.context("Failed to read chunk")? {
        writer
            .write_all(&chunk)
            .context("Failed to write to file")?;
        hasher.update(&chunk);

        downloaded += chunk.len() as u64;

        // Send progress update at most once per second
        if let Some(ref tx) = event_tx {
            let now = Instant::now();
            if now.duration_since(last_progress_update).as_secs() >= 1 {
                last_progress_update = now;
                info!(
                    "Downloaded {}% ({}/{} bytes)",
                    // A server that omits Content-Length leaves this zero, and dividing by it
                    // would panic a second into the download rather than at the start of it
                    if total_size > 0 {
                        downloaded * 100 / total_size
                    } else {
                        0
                    },
                    downloaded,
                    total_size
                );
                let _ = tx.send(DownloadEvent::Progress(DownloadProgress {
                    downloaded,
                    total: total_size,
                }));
            }
        }
    }

    // Send final progress update
    if let Some(ref tx) = event_tx {
        let _ = tx.send(DownloadEvent::Progress(DownloadProgress {
            downloaded,
            total: total_size,
        }));
    }

    writer.flush().context("Failed to flush file")?;

    // Flushing only hands the bytes to the kernel. The checksum below is computed over the
    // downloaded stream, so without forcing them out to the card it attests to what was received
    // rather than to what is on disk -- and the caller reboots the moment this returns, so a
    // write-back that never completed would leave an unverified, truncated archive for the boot
    // script to find.
    writer
        .get_ref()
        .sync_all()
        .context("Failed to sync update file to disk")?;

    // Verify SHA256 checksum
    info!("Verifying SHA256 checksum...");
    let calculated_hash = hasher.finalize().encode_hex();

    if calculated_hash != expected_hash {
        // Delete the file if verification fails
        let _ = std::fs::remove_file(&*UPDATE_FILE_PATH);
        let error_msg = format!(
            "SHA256 checksum mismatch!\nExpected: {}\nCalculated: {}",
            expected_hash, calculated_hash
        );
        if let Some(ref tx) = event_tx {
            let _ = tx.send(DownloadEvent::Error(error_msg.clone()));
        }
        anyhow::bail!(error_msg);
    }

    info!("Update downloaded successfully");
    if let Some(ref tx) = event_tx {
        let _ = tx.send(DownloadEvent::Completed);
    }
    Ok(())
}

pub fn update_file_exists() -> bool {
    UPDATE_FILE_PATH.exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> GitHubRelease {
        GitHubRelease {
            tag_name: tag.to_string(),
            assets: vec![],
            prerelease: false,
        }
    }

    #[test]
    fn version_is_the_tag_verbatim() {
        assert_eq!(
            get_release_version(&release("v1.0.1-flip.14")),
            "v1.0.1-flip.14"
        );
    }

    #[test]
    fn parses_fork_tags_and_plain_semver() {
        assert_eq!(parse_version("v1.0.1-flip.14"), Some((1, 0, 1, 14)));
        assert_eq!(parse_version("v1.0.1"), Some((1, 0, 1, 0)));
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3, 0)));
        assert_eq!(parse_version("unknown"), None);
        assert_eq!(parse_version("v1.0"), None);
        assert_eq!(parse_version("v1.0.1.2"), None);
        assert_eq!(parse_version("v1.0.1-flip.x"), None);
    }

    #[test]
    fn newer_means_numerically_greater() {
        assert!(is_newer("v1.0.1-flip.15", "v1.0.1-flip.14"));
        assert!(
            !is_newer("v1.0.1-flip.14", "v1.0.1-flip.15"),
            "a downgrade is not an update"
        );
        assert!(!is_newer("v1.0.1-flip.14", "v1.0.1-flip.14"));
        // A new base version beats any build of the old one
        assert!(is_newer("v1.0.2", "v1.0.1-flip.99"));
        assert!(is_newer("v1.0.1-flip.1", "v1.0.1"));
        // Build numbers compare as numbers, not strings
        assert!(is_newer("v1.0.1-flip.100", "v1.0.1-flip.99"));
    }

    #[test]
    fn unparseable_versions_fall_back_to_inequality() {
        assert!(is_newer("v1.0.1-flip.14", "unknown"));
        assert!(!is_newer("unknown", "unknown"));
    }

    #[test]
    fn old_channel_names_read_as_on() {
        for old in ["\"Stable\"", "\"Nightly\"", "\"On\""] {
            let channel: UpdateChannel = serde_json::from_str(old).unwrap();
            assert_eq!(channel, UpdateChannel::On, "{old}");
        }
        let channel: UpdateChannel = serde_json::from_str("\"Off\"").unwrap();
        assert_eq!(channel, UpdateChannel::Off);
    }

    #[tokio::test]
    #[ignore] // Requires network access
    async fn test_check_for_update() {
        let result = check_for_update(UpdateChannel::On).await;
        assert!(result.is_ok(), "Failed to check for update: {:?}", result);
    }

    #[tokio::test]
    #[ignore] // Requires network access
    async fn test_get_latest_release() {
        let release = get_latest_release()
            .await
            .expect("Failed to get latest release");
        assert!(!release.tag_name.is_empty());
        assert!(
            parse_version(&release.tag_name).is_some(),
            "tag {} is not a version this fork publishes",
            release.tag_name
        );
        let asset = release
            .assets
            .iter()
            .find(|a| a.name == RELEASE_FILE)
            .expect("release asset");
        assert!(
            asset
                .digest
                .as_deref()
                .is_some_and(|d| d.starts_with("sha256:"))
        );
    }
}
