#[cfg(feature = "miyoo")]
use std::fs::{self, File};
#[cfg(feature = "miyoo")]
use std::io::Write;
#[cfg(feature = "miyoo")]
use tokio::process::Command;

use anyhow::Result;
use log::{info, warn};
use serde::{Deserialize, Serialize};

use crate::constants::ALLIUM_WIFI_SETTINGS;
use crate::state_file;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WiFiSettings {
    pub wifi: bool,
    pub ssid: String,
    pub password: String,
    pub ntp: bool,
    pub web_file_browser: bool,
    pub telnet: bool,
    #[serde(default)]
    pub ssh: bool,
    pub ftp: bool,
    pub syncthing: bool,
    pub scraper: bool,
}

impl WiFiSettings {
    pub fn new() -> Self {
        Self {
            wifi: false,
            ssid: String::new(),
            password: String::new(),
            ntp: false,
            web_file_browser: false,
            telnet: false,
            ssh: false,
            ftp: false,
            syncthing: false,
            scraper: false,
        }
    }

    pub fn load() -> Result<Self> {
        Ok(state_file::load(ALLIUM_WIFI_SETTINGS.as_path(), "wifi")
            .or_else(Self::load_wpa_supplicant_conf)
            .unwrap_or_default())
    }

    pub fn init(&self) -> Result<()> {
        if self.wifi {
            wifi_on()?;
            telnet_off()?;
            ssh_off()?;
            if self.ntp {
                info!("Starting NTP...");
                ntp_sync()?;
            }
            if self.telnet {
                info!("Starting Telnet...");
                telnet_on()?;
            }
            if self.ssh {
                info!("Starting SSH...");
                ssh_on()?;
            }
            if self.ftp {
                info!("Starting FTP...");
                ftp_on()?;
            }
            if self.web_file_browser {
                info!("Starting Web File Browser...");
                web_file_browser_on()?;
            }
            if self.syncthing {
                info!("Starting Syncthing...");
                syncthing_on()?;
            }
            if self.scraper {
                info!("Starting Box Art Scraper...");
                scraper_on()?;
            }
        }
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        state_file::save(ALLIUM_WIFI_SETTINGS.as_path(), self)?;
        if let Err(e) = self.update_wpa_supplicant_conf() {
            warn!("failed to update wpa_supplicant.conf: {}", e);
        }
        Ok(())
    }

    fn load_wpa_supplicant_conf() -> Option<Self> {
        #[cfg(feature = "miyoo")]
        {
            let data = fs::read_to_string("/appconfigs/wpa_supplicant.conf").ok()?;

            let ssid_index = data.find("ssid=\"")?;
            let ssid = &data[ssid_index + 6..];
            let ssid_end = ssid.find('"')?;
            let ssid = &ssid[..ssid_end];

            let psk_index = data.find("psk=\"")?;
            let psk = &data[psk_index + 5..];
            let psk_end = psk.find('"')?;
            let psk = &psk[..psk_end];

            Some(Self {
                ssid: ssid.to_string(),
                password: psk.to_string(),
                ..Default::default()
            })
        }

        #[cfg(not(feature = "miyoo"))]
        Some(Self::new())
    }

    fn update_wpa_supplicant_conf(&self) -> Result<()> {
        #[cfg(feature = "miyoo")]
        {
            // An empty password means an open network, and `psk=""` is not "no key" to
            // wpa_supplicant -- it is an invalid passphrase, and it rejects the whole file for it.
            // `scan_ssid=1` probes for the network by name, which is what finds a hidden one.
            let security = if self.password.is_empty() {
                "\tkey_mgmt=NONE\n".to_string()
            } else {
                format!("\tpsk=\"{}\"\n", escape(&self.password))
            };
            let mut file = File::create("/appconfigs/wpa_supplicant.conf")?;
            write!(
                file,
                "ctrl_interface=/var/run/wpa_supplicant\nupdate_config=1\n\nnetwork={{\n\tssid=\"{ssid}\"\n\tscan_ssid=1\n{security}}}\n",
                ssid = escape(&self.ssid),
            )?;
        }
        Ok(())
    }

    pub fn set_wifi(&mut self, enabled: bool) -> Result<()> {
        self.wifi = enabled;
        if self.wifi {
            wifi_on()?;
            let telnet = self.telnet;
            let ssh = self.ssh;
            let ftp = self.ftp;
            tokio::spawn(async move {
                if wait_for_wifi().await.is_ok() {
                    if telnet {
                        telnet_on().ok();
                    }
                    if ssh {
                        ssh_on().ok();
                    }
                    if ftp {
                        ftp_on().ok();
                    }
                }
            });
        } else {
            wifi_off()?;
            if self.telnet {
                telnet_off().ok();
            }
            if self.ssh {
                ssh_off().ok();
            }
            if self.ftp {
                ftp_off().ok();
            }
        }
        Ok(())
    }

    pub fn set_ssid(&mut self, ssid: String) -> Result<()> {
        self.ssid = ssid;
        if self.wifi {
            self.set_wifi(self.wifi)?;
        }
        Ok(())
    }

    pub fn set_password(&mut self, password: String) -> Result<()> {
        // Nothing here can reject it, but the log should say why the network never comes up
        let len = password.chars().count();
        if len > 0 && !(8..=63).contains(&len) {
            warn!(
                "Wi-Fi passphrases must be 8 to 63 characters; wpa_supplicant will refuse this one"
            );
        }
        self.password = password;
        if self.wifi {
            self.set_wifi(self.wifi)?;
        }
        Ok(())
    }

    pub fn toggle_ntp(&mut self, enabled: bool) -> Result<()> {
        self.ntp = enabled;
        if self.ntp {
            ntp_sync()?;
        }
        Ok(())
    }

    pub fn toggle_web_file_browser(&mut self, enabled: bool) -> Result<()> {
        self.web_file_browser = enabled;
        if self.web_file_browser {
            web_file_browser_on()?;
        } else {
            web_file_browser_off()?;
        }
        Ok(())
    }

    pub fn toggle_telnet(&mut self, enabled: bool) -> Result<()> {
        self.telnet = enabled;
        if self.telnet {
            telnet_on()?;
        } else {
            telnet_off()?;
        }
        Ok(())
    }

    pub fn toggle_ssh(&mut self, enabled: bool) -> Result<()> {
        self.ssh = enabled;
        if self.ssh {
            ssh_on()?;
        } else {
            ssh_off()?;
        }
        Ok(())
    }

    pub fn toggle_ftp(&mut self, enabled: bool) -> Result<()> {
        self.ftp = enabled;
        if self.ftp {
            ftp_on()?;
        } else {
            ftp_off()?;
        }
        Ok(())
    }

    pub fn toggle_scraper(&mut self, enabled: bool) -> Result<()> {
        self.scraper = enabled;
        if self.scraper {
            scraper_on()?;
        } else {
            scraper_off()?;
        }
        Ok(())
    }

    pub fn toggle_syncthing(&mut self, enabled: bool) -> Result<()> {
        self.syncthing = enabled;
        if self.syncthing {
            syncthing_on()?;
        } else {
            syncthing_off()?;
        }
        Ok(())
    }
}

impl Default for WiFiSettings {
    fn default() -> Self {
        Self::new()
    }
}

pub fn wifi_on() -> Result<()> {
    #[cfg(feature = "miyoo")]
    tokio::spawn(async {
        Command::new(crate::constants::ALLIUM_SCRIPTS_DIR.join("wifi-on.sh"))
            .spawn()
            .map_err(|e| {
                log::error!("failed to spawn wifi-on.sh: {}", e);
                e
            })
            .unwrap()
            .wait()
            .await
            .map_err(|e| {
                log::error!("wifi-on.sh failed: {}", e);
                e
            })
    });
    Ok(())
}

pub fn wifi_off() -> Result<()> {
    #[cfg(feature = "miyoo")]
    tokio::spawn(async {
        Command::new(crate::constants::ALLIUM_SCRIPTS_DIR.join("wifi-off.sh"))
            .spawn()
            .map_err(|e| {
                log::error!("failed to spawn wifi-off.sh: {}", e);
                e
            })
            .unwrap()
            .wait()
            .await
            .map_err(|e| {
                log::error!("wifi-off.sh failed: {}", e);
                e
            })
    });
    Ok(())
}

pub fn telnet_on() -> Result<()> {
    #[cfg(feature = "miyoo")]
    tokio::spawn(async {
        Command::new(crate::constants::ALLIUM_SCRIPTS_DIR.join("telnet-on.sh"))
            .spawn()
            .map_err(|e| {
                log::error!("failed to spawn telnet-on.sh: {}", e);
                e
            })
            .unwrap()
            .wait()
            .await
            .map_err(|e| {
                log::error!("telnet-on.sh failed: {}", e);
                e
            })
    });
    Ok(())
}

pub fn telnet_off() -> Result<()> {
    #[cfg(feature = "miyoo")]
    tokio::spawn(async {
        Command::new(crate::constants::ALLIUM_SCRIPTS_DIR.join("telnet-off.sh"))
            .spawn()
            .map_err(|e| {
                log::error!("failed to spawn telnet-off.sh: {}", e);
                e
            })
            .unwrap()
            .wait()
            .await
            .map_err(|e| {
                log::error!("telnet-off.sh failed: {}", e);
                e
            })
    });
    Ok(())
}

pub fn ssh_on() -> Result<()> {
    #[cfg(feature = "miyoo")]
    tokio::spawn(async {
        Command::new(crate::constants::ALLIUM_SCRIPTS_DIR.join("ssh-on.sh"))
            .spawn()
            .map_err(|e| {
                log::error!("failed to spawn ssh-on.sh: {}", e);
                e
            })
            .unwrap()
            .wait()
            .await
            .map_err(|e| {
                log::error!("ssh-on.sh failed: {}", e);
                e
            })
    });
    Ok(())
}

pub fn ssh_off() -> Result<()> {
    #[cfg(feature = "miyoo")]
    tokio::spawn(async {
        Command::new(crate::constants::ALLIUM_SCRIPTS_DIR.join("ssh-off.sh"))
            .spawn()
            .map_err(|e| {
                log::error!("failed to spawn ssh-off.sh: {}", e);
                e
            })
            .unwrap()
            .wait()
            .await
            .map_err(|e| {
                log::error!("ssh-off.sh failed: {}", e);
                e
            })
    });
    Ok(())
}

pub fn ftp_on() -> Result<()> {
    #[cfg(feature = "miyoo")]
    tokio::spawn(async {
        Command::new(crate::constants::ALLIUM_SCRIPTS_DIR.join("ftp-on.sh"))
            .spawn()
            .map_err(|e| {
                log::error!("failed to spawn ftp-on.sh: {}", e);
                e
            })
            .unwrap()
            .wait()
            .await
            .map_err(|e| {
                log::error!("ftp-on.sh failed: {}", e);
                e
            })
    });
    Ok(())
}

pub fn ftp_off() -> Result<()> {
    #[cfg(feature = "miyoo")]
    tokio::spawn(async {
        Command::new(crate::constants::ALLIUM_SCRIPTS_DIR.join("ftp-off.sh"))
            .spawn()
            .map_err(|e| {
                log::error!("failed to spawn ftp-off.sh: {}", e);
                e
            })
            .unwrap()
            .wait()
            .await
            .map_err(|e| {
                log::error!("ftp-off.sh failed: {}", e);
                e
            })
    });
    Ok(())
}

pub fn ntp_sync() -> Result<()> {
    #[cfg(feature = "miyoo")]
    tokio::spawn(async {
        Command::new(crate::constants::ALLIUM_SCRIPTS_DIR.join("ntp-sync.sh"))
            .spawn()
            .map_err(|e| {
                log::error!("failed to spawn ntp-sync.sh: {}", e);
                e
            })
            .unwrap()
            .wait()
            .await
            .map_err(|e| {
                log::error!("ntp-sync.sh failed: {}", e);
                e
            })
            .ok();

        // Reset start time if time changed
        match crate::game_info::GameInfo::load() {
            Ok(Some(mut game_info)) => {
                game_info.start_time = chrono::Utc::now();
                game_info
                    .save()
                    .map_err(|e| {
                        log::error!("failed to save game info: {}", e);
                        e
                    })
                    .ok();
            }
            Ok(None) => {}
            Err(e) => {
                log::error!("failed to load game info: {}", e);
            }
        }
    });
    Ok(())
}

pub fn web_file_browser_on() -> Result<()> {
    #[cfg(feature = "miyoo")]
    tokio::spawn(async {
        Command::new(crate::constants::ALLIUM_SCRIPTS_DIR.join("dufs-on.sh"))
            .spawn()
            .map_err(|e| {
                log::error!("failed to spawn dufs-on.sh: {}", e);
                e
            })
            .unwrap()
            .wait()
            .await
            .map_err(|e| {
                log::error!("dufs-on.sh failed: {}", e);
                e
            })
    });
    Ok(())
}

pub fn web_file_browser_off() -> Result<()> {
    #[cfg(feature = "miyoo")]
    tokio::spawn(async {
        Command::new(crate::constants::ALLIUM_SCRIPTS_DIR.join("dufs-off.sh"))
            .spawn()
            .map_err(|e| {
                log::error!("failed to spawn dufs-off.sh: {}", e);
                e
            })
            .unwrap()
            .wait()
            .await
            .map_err(|e| {
                log::error!("dufs-off.sh failed: {}", e);
                e
            })
    });
    Ok(())
}

pub fn scraper_on() -> Result<()> {
    #[cfg(feature = "miyoo")]
    tokio::spawn(async {
        Command::new(crate::constants::ALLIUM_SCRIPTS_DIR.join("collie-on.sh"))
            .spawn()
            .map_err(|e| {
                log::error!("failed to spawn collie-on.sh: {}", e);
                e
            })
            .unwrap()
            .wait()
            .await
            .map_err(|e| {
                log::error!("collie-on.sh failed: {}", e);
                e
            })
    });
    Ok(())
}

pub fn scraper_off() -> Result<()> {
    #[cfg(feature = "miyoo")]
    tokio::spawn(async {
        Command::new(crate::constants::ALLIUM_SCRIPTS_DIR.join("collie-off.sh"))
            .spawn()
            .map_err(|e| {
                log::error!("failed to spawn collie-off.sh: {}", e);
                e
            })
            .unwrap()
            .wait()
            .await
            .map_err(|e| {
                log::error!("collie-off.sh failed: {}", e);
                e
            })
    });
    Ok(())
}

pub fn syncthing_on() -> Result<()> {
    #[cfg(feature = "miyoo")]
    tokio::spawn(async {
        Command::new(crate::constants::ALLIUM_SCRIPTS_DIR.join("syncthing-on.sh"))
            .spawn()
            .map_err(|e| {
                log::error!("failed to spawn syncthing-on.sh: {}", e);
                e
            })
            .unwrap()
            .wait()
            .await
            .map_err(|e| {
                log::error!("syncthing-on.sh failed: {}", e);
                e
            })
    });
    Ok(())
}

pub fn syncthing_off() -> Result<()> {
    #[cfg(feature = "miyoo")]
    tokio::spawn(async {
        Command::new(crate::constants::ALLIUM_SCRIPTS_DIR.join("syncthing-off.sh"))
            .spawn()
            .map_err(|e| {
                log::error!("failed to spawn syncthing-off.sh: {}", e);
                e
            })
            .unwrap()
            .wait()
            .await
            .map_err(|e| {
                log::error!("syncthing-off.sh failed: {}", e);
                e
            })
    });
    Ok(())
}

pub async fn wait_for_wifi() -> Result<()> {
    #[cfg(feature = "miyoo")]
    Command::new(crate::constants::ALLIUM_SCRIPTS_DIR.join("wait-for-wifi.sh"))
        .spawn()
        .map_err(|e| {
            log::error!("failed to spawn wait-for-wifi.sh: {}", e);
            e
        })
        .ok()
        .unwrap()
        .wait()
        .await
        .ok();
    Ok(())
}

/// Quotes a value for a wpa_supplicant.conf double-quoted string
#[cfg(feature = "miyoo")]
fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

/// wpa_supplicant's `wpa_state` for wlan0 -- `SCANNING`, `ASSOCIATING`, `4WAY_HANDSHAKE`,
/// `COMPLETED` and so on -- or `None` if it cannot be asked. This is what turns "Connecting..."
/// from a label into a diagnosis: how far the attempt gets says whether the network was found,
/// the password accepted, or an address handed out.
pub fn association_state() -> Option<String> {
    #[cfg(not(feature = "miyoo"))]
    {
        None
    }

    #[cfg(feature = "miyoo")]
    {
        let output = std::process::Command::new("/customer/app/wpa_cli")
            .args(["-i", "wlan0", "status"])
            .output()
            .ok()?;
        let output = String::from_utf8(output.stdout).ok()?;
        output
            .lines()
            .find_map(|line| line.strip_prefix("wpa_state="))
            .map(|state| state.trim().to_string())
    }
}

pub fn ip_address() -> Option<String> {
    #[cfg(feature = "miyoo")]
    {
        let output = std::process::Command::new("ip")
            .args(["route", "get", "255.255.255.255"])
            .output()
            .ok()?;
        let output = String::from_utf8(output.stdout).ok()?;
        let ip_address = output.split_whitespace().last().map(|s| s.to_string());

        ip_address.and_then(|addr| {
            addr.split('.')
                .all(|octet| octet.parse::<u8>().is_ok())
                .then_some(addr)
        })
    }

    #[cfg(feature = "simulator")]
    {
        Some("127.0.0.1".to_string())
    }

    #[cfg(not(any(feature = "miyoo", feature = "simulator")))]
    return None;
}
