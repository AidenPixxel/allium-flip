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
    #[serde(default)]
    pub charging_boot_action: ChargingBootAction,
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
            lid_close_action: PowerButtonAction::Suspend,
            power_button_action: PowerButtonAction::Suspend,
            auto_sleep_when_charging: true,
            auto_sleep_duration_minutes: 5,
            volume_on_startup: VolumeOnStartup::Restore,
            charging_boot_action: ChargingBootAction::ChargeScreen,
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
    fn power_files_from_other_builds_still_parse() {
        // A parse failure resets every power setting to its default (and used to delete the file
        // outright), so both directions have to keep deserializing: a file from before a field
        // existed, and one carrying a field that has since been removed.
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
        // Five, not zero: zero means Never, which would quietly take away the shutdown that this
        // file's device has been doing after five minutes all along
        assert_eq!(parsed.suspend_shutdown_minutes, 5);

        // Written while the CPU speed setting existed. The key is now unknown, and it must be
        // ignored rather than rejected -- a rejection here resets the power button, the lid and
        // the charging behaviour along with it.
        let with_performance_mode = legacy.replace(
            r#""auto_sleep_duration_minutes": 15"#,
            r#""auto_sleep_duration_minutes": 15, "performance_mode": "Max""#,
        );
        let parsed: PowerSettings = serde_json::from_str(&with_performance_mode).unwrap();
        assert_eq!(parsed.auto_sleep_duration_minutes, 15);
        assert!(matches!(
            parsed.power_button_action,
            PowerButtonAction::Suspend
        ));
    }

    #[test]
    fn a_fresh_install_sleeps_when_the_lid_shuts() {
        // A clamshell that powers off whenever it is closed makes every lid close a reboot on the
        // way back. Sleep is what a lid is for. Existing files carry their own value and are not
        // touched by this.
        assert!(matches!(
            PowerSettings::default().lid_close_action,
            PowerButtonAction::Suspend
        ));
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
