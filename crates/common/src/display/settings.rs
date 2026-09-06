use std::{
    fs::{self, File},
    io::Write,
};

use anyhow::Result;
use log::{debug, warn};
use serde::{Deserialize, Serialize};

use crate::constants::{
    ALLIUM_DISPLAY_SETTINGS, NIGHT_MODE_BLUE_SCALE, NIGHT_MODE_CHANNEL_FLOOR,
    NIGHT_MODE_GREEN_SCALE, NIGHT_MODE_LUMINANCE_SCALE,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplaySettings {
    pub luminance: u8,
    pub hue: u8,
    pub saturation: u8,
    pub contrast: u8,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    /// Warmer, dimmer panel for playing in the dark. Stored alongside the base values, but
    /// never folded into them: see [`DisplaySettings::effective`].
    #[serde(default)]
    pub night_mode: bool,
}

impl DisplaySettings {
    pub fn new() -> Self {
        Self::default()
    }

    /// The values actually written to the panel: the user's base settings with the night mode
    /// warm/dim transform applied on top. Never persisted, so toggling night mode off restores
    /// the base settings exactly.
    pub fn effective(&self) -> Self {
        if !self.night_mode {
            return self.clone();
        }

        fn scale(value: u8, factor: f32, floor: u8) -> u8 {
            ((f32::from(value) * factor).round() as u8).max(floor)
        }

        Self {
            luminance: scale(self.luminance, NIGHT_MODE_LUMINANCE_SCALE, 1),
            // Cutting green and blue while leaving red alone is what warms the panel. The floor
            // keeps red dominant even from a very dim base, where the platform would otherwise
            // flatten all three channels back to neutral grey.
            g: scale(self.g, NIGHT_MODE_GREEN_SCALE, NIGHT_MODE_CHANNEL_FLOOR),
            b: scale(self.b, NIGHT_MODE_BLUE_SCALE, NIGHT_MODE_CHANNEL_FLOOR),
            ..self.clone()
        }
    }

    pub fn load() -> Result<Self> {
        if ALLIUM_DISPLAY_SETTINGS.exists() {
            debug!("found state, loading from file");
            if let Ok(json) = fs::read_to_string(ALLIUM_DISPLAY_SETTINGS.as_path())
                && let Ok(json) = serde_json::from_str(&json)
            {
                return Ok(json);
            }
            warn!("failed to read state file, removing");
            fs::remove_file(ALLIUM_DISPLAY_SETTINGS.as_path())?;
        }
        Ok(Self::new())
    }

    pub fn save(&self) -> Result<()> {
        let json = serde_json::to_string(&self).unwrap();
        File::create(ALLIUM_DISPLAY_SETTINGS.as_path())?.write_all(json.as_bytes())?;
        Ok(())
    }
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            luminance: 50,
            hue: 50,
            saturation: 50,
            contrast: 50,
            r: 50,
            g: 50,
            b: 50,
            night_mode: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_is_a_noop_when_night_mode_is_off() {
        let settings = DisplaySettings::new();
        assert!(!settings.night_mode);
        assert_eq!(settings.effective(), settings);
    }

    #[test]
    fn night_mode_warms_and_dims() {
        let base = DisplaySettings::new();
        let night = DisplaySettings {
            night_mode: true,
            ..base.clone()
        }
        .effective();

        assert!(night.luminance < base.luminance, "night mode should dim");
        assert_eq!(night.r, base.r, "red is left alone");
        assert!(night.b < night.g && night.g < night.r, "warm shift");

        // Untouched knobs stay untouched
        assert_eq!(night.hue, base.hue);
        assert_eq!(night.saturation, base.saturation);
        assert_eq!(night.contrast, base.contrast);
    }

    #[test]
    fn night_mode_from_a_dim_base_keeps_red_dominant() {
        // The platform flattens r/g/b to neutral grey when all three fall below 15, which would
        // silently cancel the tint. The channel floor exists to keep that from happening.
        let night = DisplaySettings {
            r: 20,
            g: 20,
            b: 20,
            night_mode: true,
            ..DisplaySettings::new()
        }
        .effective();

        assert!(night.g >= NIGHT_MODE_CHANNEL_FLOOR);
        assert!(night.b >= NIGHT_MODE_CHANNEL_FLOOR);
        assert!(night.r > night.g && night.g > night.b);
    }

    #[test]
    fn night_mode_round_trips_and_older_files_still_parse() {
        let settings = DisplaySettings {
            night_mode: true,
            ..DisplaySettings::new()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<DisplaySettings>(&json).unwrap(),
            settings
        );

        // A display.json written before night mode existed must still load, or `load` deletes it
        // and resets every display setting.
        let legacy = r#"{"luminance":40,"hue":50,"saturation":50,"contrast":50,"r":50,"g":50,"b":50}"#;
        let parsed: DisplaySettings = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.luminance, 40);
        assert!(!parsed.night_mode);
    }
}
