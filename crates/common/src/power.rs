use anyhow::Result;
use serde::{Deserialize, Serialize};
use strum::FromRepr;

use crate::constants::ALLIUM_POWER_SETTINGS;
use crate::performance::PerformanceMode;
use crate::state_file;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerSettings {
    pub power_button_action: PowerButtonAction,
    pub lid_close_action: PowerButtonAction,
    pub auto_sleep_when_charging: bool,
    pub auto_sleep_duration_minutes: i32,
    #[serde(default)]
    pub volume_on_startup: VolumeOnStartup,
    #[serde(default)]
    pub charging_boot_action: ChargingBootAction,
    /// The performance mode used by games that have not been given one of their own.
    #[serde(default)]
    pub performance_mode: PerformanceMode,
    /// How long the device stays suspended before powering off. Zero never powers off.
    ///
    /// A named default rather than `#[serde(default)]`, which would yield zero: every existing
    /// power.json would then read as Never and silently lose the shutdown that used to happen
    /// after five minutes.
    #[serde(default = "PowerSettings::default_suspend_shutdown_minutes")]
    pub suspend_shutdown_minutes: i32,
}

/// What happens when the device powers on only because a charger was plugged in.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, FromRepr, Default)]
pub enum ChargingBootAction {
    /// Announce "Charging", hold the lit screen briefly, then park in the charge screen.
    #[default]
    ChargeScreen,
    /// Park in the charge screen without ever lighting the display.
    ChargeSilently,
    /// Shut back down, so the charger never wakes the device at all.
    PowerOff,
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
            charging_boot_action: ChargingBootAction::ChargeScreen,
            // Leaves the CPU governor exactly as it was, so this build changes nothing about how
            // the device clocks until the setting is actually used
            performance_mode: PerformanceMode::System,
            suspend_shutdown_minutes: PowerSettings::default_suspend_shutdown_minutes(),
        }
    }
}

impl PowerSettings {
    pub fn new() -> Self {
        Default::default()
    }

    /// What the suspend timeout used to be hardcoded to, so upgrading changes nothing.
    fn default_suspend_shutdown_minutes() -> i32 {
        5
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
        // outright), so a file written before `charging_boot_action` existed must still deserialize.
        let legacy = r#"{
            "power_button_action": "Suspend",
            "lid_close_action": "Shutdown",
            "auto_sleep_when_charging": false,
            "auto_sleep_duration_minutes": 15
        }"#;

        let parsed: PowerSettings = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.auto_sleep_duration_minutes, 15);
        assert!(!parsed.auto_sleep_when_charging);
        assert_eq!(
            parsed.charging_boot_action,
            ChargingBootAction::ChargeScreen
        );
        // Nothing touches the CPU governor until this is set deliberately
        assert_eq!(parsed.performance_mode, PerformanceMode::System);
        // Five, not zero: zero means Never, which would quietly take away the shutdown that this
        // file's device has been doing after five minutes all along
        assert_eq!(parsed.suspend_shutdown_minutes, 5);
    }

    #[test]
    fn charging_boot_action_round_trips() {
        let json = serde_json::to_string(&ChargingBootAction::PowerOff).unwrap();
        assert_eq!(
            serde_json::from_str::<ChargingBootAction>(&json).unwrap(),
            ChargingBootAction::PowerOff
        );
    }
}
