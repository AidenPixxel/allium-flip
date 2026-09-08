use anyhow::Result;
use serde::{Deserialize, Serialize};
use strum::FromRepr;

use crate::constants::ALLIUM_POWER_SETTINGS;
use crate::state_file;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerSettings {
    pub power_button_action: PowerButtonAction,
    pub lid_close_action: PowerButtonAction,
    pub auto_sleep_when_charging: bool,
    pub auto_sleep_duration_minutes: i32,
    #[serde(default)]
    pub volume_on_startup: VolumeOnStartup,
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize, FromRepr, Default)]
pub enum PowerButtonAction {
    #[default]
    Suspend,
    Shutdown,
    Nothing,
}

/// What the volume is set to when the device powers on.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, FromRepr, Default)]
pub enum VolumeOnStartup {
    /// Restore the volume the device was turned off at.
    #[default]
    Restore,
    Muted,
}

impl PowerButtonAction {
    pub fn suspend() -> Self {
        PowerButtonAction::Suspend
    }
    pub fn shutdown() -> Self {
        PowerButtonAction::Shutdown
    }
    pub fn nothing() -> Self {
        PowerButtonAction::Nothing
    }
}

impl Default for PowerSettings {
    fn default() -> Self {
        Self {
            lid_close_action: PowerButtonAction::Shutdown,
            power_button_action: PowerButtonAction::Suspend,
            auto_sleep_when_charging: true,
            auto_sleep_duration_minutes: 5,
            volume_on_startup: VolumeOnStartup::Restore,
        }
    }
}

impl PowerSettings {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn load() -> Result<Self> {
        Ok(state_file::load(ALLIUM_POWER_SETTINGS.as_path(), "power").unwrap_or_else(Self::new))
    }

    pub fn save(&self) -> Result<()> {
        state_file::save(ALLIUM_POWER_SETTINGS.as_path(), self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_power_files_still_parse() {
        // A parse failure resets every power setting to its default (and used to delete the file
        // outright), so a file written by another build has to keep deserializing -- including
        // one carrying keys this build knows nothing about.
        let legacy = r#"{
            "power_button_action": "Suspend",
            "lid_close_action": "Shutdown",
            "auto_sleep_when_charging": false,
            "auto_sleep_duration_minutes": 15
        }"#;

        let parsed: PowerSettings = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.auto_sleep_duration_minutes, 15);
        assert!(!parsed.auto_sleep_when_charging);

        let with_unknown_key = legacy.replace(
            r#""auto_sleep_duration_minutes": 15"#,
            r#""auto_sleep_duration_minutes": 15, "performance_mode": "Max""#,
        );
        let parsed: PowerSettings = serde_json::from_str(&with_unknown_key).unwrap();
        assert_eq!(parsed.auto_sleep_duration_minutes, 15);
    }
}
