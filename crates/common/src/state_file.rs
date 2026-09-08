//! Settings files under `.allium/state`, read leniently and written atomically.
//!
//! Every loader here used to delete its file when the JSON failed to parse. That turned two
//! ordinary events into data loss: a build adding a strict field, and a write cut short by power
//! loss leaving truncated JSON behind. `load` now keeps such a file as `<name>.bak` and falls back
//! to defaults, so nothing is lost and the cause can be inspected; `save` writes through a sibling
//! and renames, so a cut-short write can no longer produce the truncated file in the first place.

use std::ffi::OsString;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use log::{debug, warn};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Reads `path` as JSON. `None` if it does not exist or does not parse -- in which case it is
/// moved aside to `<name>.bak` rather than deleted. `what` names the file in log lines.
pub fn load<T: DeserializeOwned>(path: &Path, what: &str) -> Option<T> {
    if !path.exists() {
        return None;
    }
    debug!("found {what} state, loading from file");
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => {
            warn!("could not read {what} state: {e}");
            return None;
        }
    };
    match serde_json::from_str(&text) {
        Ok(value) => Some(value),
        Err(e) => {
            let backup = sibling(path, ".bak");
            warn!(
                "{what} state did not parse ({e}); keeping it as {}",
                backup.display()
            );
            if let Err(e) = fs::rename(path, &backup) {
                warn!("could not move {what} state aside: {e}");
            }
            None
        }
    }
}

/// `load`, falling back to `T::default()` when there is nothing usable.
pub fn load_or_default<T: DeserializeOwned + Default>(path: &Path, what: &str) -> T {
    load(path, what).unwrap_or_default()
}

/// Writes `value` as JSON through a sibling file and a rename, so a write interrupted by power
/// loss leaves either the old file or the new one -- never a truncated mix.
pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let json = serde_json::to_string(value)?;
    let tmp = sibling(path, ".tmp");
    {
        let mut file = File::create(&tmp).with_context(|| format!("creating {}", tmp.display()))?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path).with_context(|| format!("moving {} into place", tmp.display()))?;
    Ok(())
}

/// `path` with `suffix` appended to its file name: `power.json` gives `power.json.bak`, and an
/// extensionless `current_game` gives `current_game.bak` rather than losing its name.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    struct Fixture {
        n: u32,
        #[serde(default)]
        added_later: bool,
    }

    /// A fresh directory per test; `tempfile` is not in the workspace
    fn scratch() -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "allium-state-file-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn round_trips() {
        let path = scratch().join("fixture.json");
        let value = Fixture {
            n: 7,
            added_later: true,
        };
        save(&path, &value).unwrap();
        assert_eq!(load::<Fixture>(&path, "fixture"), Some(value));
        // Nothing left over from the atomic write
        assert!(!sibling(&path, ".tmp").exists());
    }

    #[test]
    fn missing_file_is_none_and_default() {
        let path = scratch().join("absent.json");
        assert_eq!(load::<Fixture>(&path, "fixture"), None);
        assert_eq!(
            load_or_default::<Fixture>(&path, "fixture"),
            Fixture::default()
        );
    }

    #[test]
    fn garbage_is_kept_as_bak_not_deleted() {
        let path = scratch().join("fixture.json");
        fs::write(&path, "{\"n\": 7, \"trunc").unwrap();
        assert_eq!(load::<Fixture>(&path, "fixture"), None);
        assert!(!path.exists(), "the unreadable file should have moved");
        let backup = path.with_file_name("fixture.json.bak");
        assert_eq!(fs::read_to_string(backup).unwrap(), "{\"n\": 7, \"trunc");
    }

    #[test]
    fn missing_fields_take_defaults() {
        // What an older build wrote, before `added_later` existed
        let path = scratch().join("fixture.json");
        fs::write(&path, "{\"n\": 3}").unwrap();
        assert_eq!(
            load::<Fixture>(&path, "fixture"),
            Some(Fixture {
                n: 3,
                added_later: false
            })
        );
    }

    #[test]
    fn sibling_keeps_the_whole_name() {
        assert_eq!(
            sibling(Path::new("/s/power.json"), ".bak"),
            PathBuf::from("/s/power.json.bak")
        );
        assert_eq!(
            sibling(Path::new("/s/current_game"), ".bak"),
            PathBuf::from("/s/current_game.bak")
        );
    }
}
