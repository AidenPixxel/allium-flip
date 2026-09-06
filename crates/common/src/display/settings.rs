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
    /// How far night mode cuts green and blue, 0 (no tint) to 100 (the full warm shift).
    #[serde(default = "default_night_mode_strength")]
    pub night_mode_warmth: u8,
    /// How far night mode dims the panel, 0 (no dimming) to 100 (the full dim).
    /// Independent of warmth, since wanting one without the other is common.
    #[serde(default = "default_night_mode_strength")]
    pub night_mode_dimness: u8,
}

/// Both strength sliders default to full, so night mode out of the box matches the tuned look
/// the constants describe.
pub const DEFAULT_NIGHT_MODE_STRENGTH: u8 = 100;

fn default_night_mode_strength() -> u8 {
    DEFAULT_NIGHT_MODE_STRENGTH
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

        // Eases a scale between 1.0 (slider at 0) and `full` (slider at 100)
        fn strength(full: f32, percent: u8) -> f32 {
            1.0 - (1.0 - full) * f32::from(percent.min(100)) / 100.0
        }

        fn scale(value: u8, factor: f32, floor: u8) -> u8 {
            ((f32::from(value) * factor).round() as u8).max(floor)
        }

        let warmth = self.night_mode_warmth;
        Self {
            luminance: scale(
                self.luminance,
                strength(NIGHT_MODE_LUMINANCE_SCALE, self.night_mode_dimness),
                1,
            ),
            // Cutting green and blue while leaving red alone is what warms the panel. The floor
            // keeps both above the threshold at which the platform gives up and flattens all
            // three channels back to neutral grey, which would cancel the tint outright.
            g: scale(
                self.g,
                strength(NIGHT_MODE_GREEN_SCALE, warmth),
                NIGHT_MODE_CHANNEL_FLOOR,
            ),
            b: scale(
                self.b,
                strength(NIGHT_MODE_BLUE_SCALE, warmth),
                NIGHT_MODE_CHANNEL_FLOOR,
            ),
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
            night_mode_warmth: DEFAULT_NIGHT_MODE_STRENGTH,
            night_mode_dimness: DEFAULT_NIGHT_MODE_STRENGTH,
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
    fn night_mode_can_never_trip_the_platform_flatten() {
        // MiyooPlatform::set_display_settings resets r/g/b to 15 when *all three* fall below 15,
        // which silently cancels the tint. The floor makes that unreachable -- including from a
        // base whose red is itself under the threshold, which is the case an earlier floor of 8
        // did not cover.
        for (r, g, b) in [(20, 20, 20), (10, 50, 50), (1, 1, 1)] {
            let night = DisplaySettings {
                r,
                g,
                b,
                night_mode: true,
                ..DisplaySettings::new()
            }
            .effective();

            assert!(night.g >= NIGHT_MODE_CHANNEL_FLOOR, "green floored");
            assert!(night.b >= NIGHT_MODE_CHANNEL_FLOOR, "blue floored");
            assert!(
                !(night.r < 15 && night.g < 15 && night.b < 15),
                "flatten would fire for base ({r}, {g}, {b})"
            );
            assert!(night.g >= night.b, "green is never cut below blue");
        }
    }

    #[test]
    fn strength_sliders_scale_the_effect() {
        let base = DisplaySettings {
            night_mode: true,
            ..DisplaySettings::new()
        };

        // At 0 the sliders are a no-op even with night mode enabled
        let off = DisplaySettings {
            night_mode_warmth: 0,
            night_mode_dimness: 0,
            ..base.clone()
        }
        .effective();
        assert_eq!(off.luminance, base.luminance);
        assert_eq!((off.g, off.b), (base.g, base.b));

        // At 100 it matches the tuned constants
        let full = base.effective();
        assert!(full.luminance < base.luminance);
        assert!(full.b < full.g && full.g < full.r);

        // Half strength sits between the two
        let half = DisplaySettings {
            night_mode_warmth: 50,
            night_mode_dimness: 50,
            ..base.clone()
        }
        .effective();
        assert!(half.luminance > full.luminance && half.luminance < base.luminance);
        assert!(half.b > full.b && half.b < base.b);

        // The two are independent: dimness alone must not touch colour
        let dim_only = DisplaySettings {
            night_mode_warmth: 0,
            night_mode_dimness: 100,
            ..base.clone()
        }
        .effective();
        assert_eq!((dim_only.g, dim_only.b), (base.g, base.b));
        assert_eq!(dim_only.luminance, full.luminance);
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
        let legacy =
            r#"{"luminance":40,"hue":50,"saturation":50,"contrast":50,"r":50,"g":50,"b":50}"#;
        let parsed: DisplaySettings = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.luminance, 40);
        assert!(!parsed.night_mode);
        // Missing strength fields must default to full, not to zero, or night mode would come
        // back from an upgrade doing nothing at all
        assert_eq!(parsed.night_mode_warmth, DEFAULT_NIGHT_MODE_STRENGTH);
        assert_eq!(parsed.night_mode_dimness, DEFAULT_NIGHT_MODE_STRENGTH);
    }
}
