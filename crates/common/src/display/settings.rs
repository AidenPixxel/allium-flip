use std::{
    fs::{self, File},
    io::Write,
};

use anyhow::Result;
use log::{debug, warn};
use serde::{Deserialize, Serialize};

use crate::constants::{
    ALLIUM_DISPLAY_SETTINGS, MAX_BRIGHTNESS, MIN_BRIGHTNESS, PANEL_FLATTEN_THRESHOLD,
    WARMTH_BLUE_SCALE, WARMTH_GREEN_SCALE,
};

/// How many profiles are kept. The rotate hotkey cycles all of them, so keep it small enough to
/// get back to where you started without thinking about it.
pub const PROFILE_COUNT: usize = 3;
/// Names are shown in a list cell and in the on-screen indicator, neither of which scrolls, so a
/// long one would simply be cut off.
pub const MAX_PROFILE_NAME_LEN: usize = 12;
/// The platform raises anything lower, so never store a value the panel will not show.
pub const MIN_CONTRAST: u8 = 10;
/// The backlight a profile gets when it says nothing about one. Resolves to the same duty cycle
/// the slider's old default did, so an upgrade does not change how bright the device looks.
pub const DEFAULT_BRIGHTNESS: u8 = 85;
/// ...and what the shipped Night profile uses: as dim as the panel will go while still lighting.
pub const NIGHT_BRIGHTNESS: u8 = MIN_BRIGHTNESS;

/// One full set of panel values, under a name the user chooses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "RawDisplayProfile")]
pub struct DisplayProfile {
    pub name: String,
    pub luminance: u8,
    pub hue: u8,
    pub saturation: u8,
    pub contrast: u8,
    pub r: u8,
    pub g: u8,
    pub b: u8,
    /// How far to cut green and blue, 0 (untouched) to 100 (the full warm shift).
    pub warmth: u8,
    /// The backlight this profile runs at, on the slider's own 0..=100 scale. Applied whenever
    /// the profile becomes active, which is what makes switching to Night actually dim the lamp
    /// and switching back to Day restore it.
    pub brightness: u8,
}

impl DisplayProfile {
    fn neutral(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            luminance: 50,
            hue: 50,
            saturation: 50,
            contrast: 50,
            r: 50,
            g: 50,
            b: 50,
            warmth: 0,
            brightness: DEFAULT_BRIGHTNESS,
        }
    }

    /// The values actually written to the panel: this profile's own values with its warmth folded
    /// in. Never persisted, so turning the slider back down restores exactly what was stored.
    ///
    /// Only the colour channels move. Dimming belongs to `brightness` and the backlight: scaling
    /// `luminance` instead, as this used to, lowers peak white while the lamp keeps lighting the
    /// black level just as hard -- less contrast for no less light, which is the opposite of what
    /// a dark room wants.
    pub fn effective(&self) -> Self {
        // Eases a scale between 1.0 (slider at 0) and `full` (slider at 100)
        fn strength(full: f32, percent: u8) -> f32 {
            1.0 - (1.0 - full) * f32::from(percent.min(100)) / 100.0
        }

        fn scale(value: u8, factor: f32) -> u8 {
            (f32::from(value) * factor).round() as u8
        }

        // Cutting green and blue while leaving red alone is what warms the panel
        let mut warmed = Self {
            g: scale(self.g, strength(WARMTH_GREEN_SCALE, self.warmth)),
            b: scale(self.b, strength(WARMTH_BLUE_SCALE, self.warmth)),
            ..self.clone()
        };

        // The platform resets all three channels to neutral grey when every one of them is under
        // the threshold, cancelling the tint. Only a profile whose own red is that low can get
        // there. Lift the set until its largest channel clears the threshold, keeping the ratio
        // between them: clamping each channel instead -- what this did before -- would flatten
        // the tint here rather than in the platform, and would stop an ordinary profile from
        // reaching a deep amber at all.
        let max = warmed.r.max(warmed.g).max(warmed.b);
        if max < PANEL_FLATTEN_THRESHOLD {
            if max == 0 {
                // No ratio to preserve; any lift is as good as another
                warmed.r = PANEL_FLATTEN_THRESHOLD;
                warmed.g = PANEL_FLATTEN_THRESHOLD;
                warmed.b = PANEL_FLATTEN_THRESHOLD;
            } else {
                let lift = f32::from(PANEL_FLATTEN_THRESHOLD) / f32::from(max);
                warmed.r = scale(warmed.r, lift);
                warmed.g = scale(warmed.g, lift);
                warmed.b = scale(warmed.b, lift);
            }
        }
        warmed
    }
}

/// Mirror of every shape a profile has had, so none can be rejected.
///
/// `dimness` is the one that went: it scaled the signal rather than the backlight, so it cost
/// contrast and saved no light. A profile that set it wanted a dark screen, which is now what
/// `brightness` means -- so carry the intent across rather than the number.
#[derive(Deserialize)]
#[serde(default)]
struct RawDisplayProfile {
    name: String,
    luminance: u8,
    hue: u8,
    saturation: u8,
    contrast: u8,
    r: u8,
    g: u8,
    b: u8,
    warmth: u8,
    brightness: Option<u8>,
    dimness: Option<u8>,
}

impl Default for RawDisplayProfile {
    fn default() -> Self {
        let neutral = DisplayProfile::neutral("");
        Self {
            name: neutral.name,
            luminance: neutral.luminance,
            hue: neutral.hue,
            saturation: neutral.saturation,
            contrast: neutral.contrast,
            r: neutral.r,
            g: neutral.g,
            b: neutral.b,
            warmth: neutral.warmth,
            brightness: None,
            dimness: None,
        }
    }
}

impl From<RawDisplayProfile> for DisplayProfile {
    fn from(raw: RawDisplayProfile) -> Self {
        let brightness = raw.brightness.unwrap_or(match raw.dimness {
            Some(dimness) if dimness > 0 => NIGHT_BRIGHTNESS,
            _ => DEFAULT_BRIGHTNESS,
        });
        Self {
            name: raw.name,
            luminance: raw.luminance,
            hue: raw.hue,
            saturation: raw.saturation,
            contrast: raw.contrast,
            r: raw.r,
            g: raw.g,
            b: raw.b,
            warmth: raw.warmth,
            brightness,
        }
    }
}

impl Default for DisplayProfile {
    fn default() -> Self {
        Self::neutral("")
    }
}

/// The stored display configuration: a handful of named profiles and which one is live.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "RawDisplaySettings")]
pub struct DisplaySettings {
    pub active: usize,
    pub profiles: Vec<DisplayProfile>,
}

/// Mirror of every shape `display.json` has had, so neither can be rejected.
///
/// `serde(default)` makes every key optional, and unknown keys are ignored -- which is what lets
/// the pre-profiles `night_mode`, `night_mode_warmth` and `night_mode_dimness` be dropped rather
/// than failing the parse. Only `Deserialize` is routed through here; writes are always the
/// current shape.
#[derive(Default, Deserialize)]
#[serde(default)]
struct RawDisplaySettings {
    active: Option<usize>,
    profiles: Option<Vec<DisplayProfile>>,

    // The flat, single-profile shape
    luminance: Option<u8>,
    hue: Option<u8>,
    saturation: Option<u8>,
    contrast: Option<u8>,
    r: Option<u8>,
    g: Option<u8>,
    b: Option<u8>,
}

impl From<RawDisplaySettings> for DisplaySettings {
    fn from(raw: RawDisplaySettings) -> Self {
        if let Some(profiles) = raw.profiles.filter(|p| !p.is_empty()) {
            return Self {
                active: raw.active.unwrap_or(0),
                profiles,
            }
            .sanitized();
        }

        // A flat file predates profiles, so carry the values the user tuned into the first slot
        // rather than resetting their panel on upgrade. Their night-mode strengths only meant
        // anything when night mode was on, and that toggle is gone, so they are dropped.
        let mut settings = Self::default();
        let first = &mut settings.profiles[0];
        let neutral = DisplayProfile::default();
        first.luminance = raw.luminance.unwrap_or(neutral.luminance);
        first.hue = raw.hue.unwrap_or(neutral.hue);
        first.saturation = raw.saturation.unwrap_or(neutral.saturation);
        first.contrast = raw.contrast.unwrap_or(neutral.contrast);
        first.r = raw.r.unwrap_or(neutral.r);
        first.g = raw.g.unwrap_or(neutral.g);
        first.b = raw.b.unwrap_or(neutral.b);
        settings.sanitized()
    }
}

impl DisplaySettings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Clamps the document to the invariants everything downstream relies on, so no caller has to
    /// bounds-check: always `PROFILE_COUNT` profiles, `active` in range, names short enough to
    /// display, contrast above the platform's floor.
    fn sanitized(mut self) -> Self {
        self.profiles.truncate(PROFILE_COUNT);
        while self.profiles.len() < PROFILE_COUNT {
            let i = self.profiles.len();
            self.profiles.push(Self::default_profile(i));
        }
        for profile in &mut self.profiles {
            profile.contrast = profile.contrast.max(MIN_CONTRAST);
            profile.brightness = profile.brightness.clamp(MIN_BRIGHTNESS, MAX_BRIGHTNESS);
            profile.name = truncate_name(&profile.name);
        }
        self.active = self.active.min(self.profiles.len() - 1);
        self
    }

    fn default_profile(index: usize) -> DisplayProfile {
        match index {
            0 => DisplayProfile::neutral("Day"),
            // Everything a dark room wants: the lamp right down, a deep amber, and colour and
            // contrast eased off so nothing on screen glares. Luminance stays put -- see
            // `effective`.
            1 => DisplayProfile {
                brightness: NIGHT_BRIGHTNESS,
                warmth: 100,
                saturation: 35,
                contrast: 40,
                ..DisplayProfile::neutral("Night")
            },
            _ => DisplayProfile::neutral("Custom"),
        }
    }

    /// The profile currently driving the panel.
    pub fn active(&self) -> &DisplayProfile {
        // `sanitized` keeps `active` in range and `profiles` non-empty
        &self.profiles[self.active]
    }

    pub fn active_mut(&mut self) -> &mut DisplayProfile {
        &mut self.profiles[self.active]
    }

    /// Advances to the next profile, wrapping, and returns its index.
    pub fn rotate(&mut self) -> usize {
        self.active = (self.active + 1) % self.profiles.len();
        self.active
    }

    /// What to call profile `index` on screen. Falls back to its position so an emptied name
    /// still leaves something selectable.
    pub fn name_of(&self, index: usize) -> String {
        match self.profiles.get(index) {
            Some(p) if !p.name.is_empty() => p.name.clone(),
            _ => format!("{}", index + 1),
        }
    }

    pub fn names(&self) -> Vec<String> {
        (0..self.profiles.len()).map(|i| self.name_of(i)).collect()
    }

    /// Never deletes the file. A parse failure is far likelier to be a bad migration or a
    /// half-written file than genuine garbage, and discarding it would silently reset every
    /// panel value the user has tuned -- so keep a copy and carry on with defaults.
    pub fn load() -> Result<Self> {
        let path = ALLIUM_DISPLAY_SETTINGS.as_path();
        if !path.exists() {
            return Ok(Self::new());
        }
        debug!("found state, loading from file");

        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => {
                warn!("could not read display settings: {}", e);
                return Ok(Self::new());
            }
        };

        match serde_json::from_str(&text) {
            Ok(settings) => Ok(settings),
            Err(e) => {
                warn!(
                    "display settings did not parse ({}), keeping a .bak copy",
                    e
                );
                let _ = fs::rename(path, path.with_extension("json.bak"));
                Ok(Self::new())
            }
        }
    }

    /// Writes to a sibling file and renames, so an interrupted write cannot leave truncated JSON
    /// behind -- which is the input that made the old `load` delete the file.
    pub fn save(&self) -> Result<()> {
        let path = ALLIUM_DISPLAY_SETTINGS.as_path();
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string(&self)?;
        {
            let mut file = File::create(&tmp)?;
            file.write_all(json.as_bytes())?;
            file.sync_all()?;
        }
        fs::rename(&tmp, path)?;
        Ok(())
    }
}

fn truncate_name(name: &str) -> String {
    // By character, not byte, so a multi-byte name cannot be cut mid-codepoint
    name.chars().take(MAX_PROFILE_NAME_LEN).collect()
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self {
            active: 0,
            profiles: (0..PROFILE_COUNT).map(Self::default_profile).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_three_named_profiles() {
        let settings = DisplaySettings::new();
        assert_eq!(settings.profiles.len(), PROFILE_COUNT);
        assert_eq!(settings.active, 0);
        assert_eq!(settings.names(), vec!["Day", "Night", "Custom"]);

        // Day leaves the panel alone; Night is the warm, dim one
        let day = &settings.profiles[0];
        assert_eq!(day.warmth, 0);
        assert_eq!(day.brightness, DEFAULT_BRIGHTNESS);
        assert_eq!(day.effective(), day.clone());

        let stored = &settings.profiles[1];
        assert!(
            stored.brightness < day.brightness,
            "Night dims the backlight, which is the whole point"
        );
        assert_eq!(
            stored.luminance, day.luminance,
            "dimming is the lamp's job, not the signal's"
        );

        let night = stored.effective();
        assert!(night.b < night.g && night.g < night.r, "warm, not just dim");
        assert!(
            night.b * 4 < night.r,
            "a deep amber: blue at {} against red at {}",
            night.b,
            night.r
        );
    }

    #[test]
    fn rotate_wraps() {
        let mut settings = DisplaySettings::new();
        assert_eq!(settings.rotate(), 1);
        assert_eq!(settings.rotate(), 2);
        assert_eq!(settings.rotate(), 0);
        assert_eq!(settings.active().name, "Day");
    }

    #[test]
    fn effective_can_never_trip_the_platform_flatten() {
        // MiyooPlatform::set_display_settings resets r/g/b to 15 when *all three* fall below 15,
        // which silently cancels the tint. Reachable only from a base whose red is itself that
        // low, and the lift has to keep it out of reach there too.
        for (r, g, b) in [(20, 20, 20), (10, 50, 50), (1, 1, 1), (0, 0, 0)] {
            let profile = DisplayProfile {
                r,
                g,
                b,
                warmth: 100,
                ..DisplayProfile::neutral("t")
            }
            .effective();

            assert!(
                !(profile.r < PANEL_FLATTEN_THRESHOLD
                    && profile.g < PANEL_FLATTEN_THRESHOLD
                    && profile.b < PANEL_FLATTEN_THRESHOLD),
                "flatten would fire for base ({r}, {g}, {b})"
            );
        }
    }

    #[test]
    fn an_ordinary_profile_reaches_a_deep_amber() {
        // What the old per-channel floor of 15 prevented: blue could never fall below it, so the
        // warmest the panel could get was a mild tint.
        let warm = DisplayProfile {
            warmth: 100,
            ..DisplayProfile::neutral("t")
        }
        .effective();

        assert_eq!(warm.r, 50, "red is never touched");
        assert!(
            warm.b < 10,
            "blue almost gone, was floored at 15: {}",
            warm.b
        );
        assert!(warm.g > warm.b && warm.g < warm.r);
    }

    #[test]
    fn a_dimmed_profile_migrates_to_a_dim_backlight() {
        // dimness scaled the signal and saved no light. A profile that set it wanted a dark
        // screen, so it gets one that actually is dark; one that did not keeps its brightness.
        let dimmed = r#"{"name":"Night","luminance":50,"hue":50,"saturation":50,
            "contrast":50,"r":50,"g":50,"b":50,"warmth":100,"dimness":100}"#;
        let parsed: DisplayProfile = serde_json::from_str(dimmed).unwrap();
        assert_eq!(parsed.brightness, NIGHT_BRIGHTNESS);
        assert_eq!(parsed.warmth, 100, "the rest of the profile is untouched");
        assert_eq!(parsed.luminance, 50, "no longer scaled down");

        let plain = r#"{"name":"Day","luminance":40,"dimness":0}"#;
        let parsed: DisplayProfile = serde_json::from_str(plain).unwrap();
        assert_eq!(parsed.brightness, DEFAULT_BRIGHTNESS);
        assert_eq!(parsed.luminance, 40);

        // A profile written by this build round-trips its brightness rather than re-migrating
        let current = r#"{"name":"X","brightness":42,"dimness":100}"#;
        let parsed: DisplayProfile = serde_json::from_str(current).unwrap();
        assert_eq!(parsed.brightness, 42);
    }

    #[test]
    fn sanitized_enforces_the_invariants() {
        let settings = DisplaySettings {
            active: 99,
            profiles: vec![DisplayProfile {
                name: "a very long profile name".to_owned(),
                contrast: 0,
                ..DisplayProfile::default()
            }],
        }
        .sanitized();

        assert_eq!(settings.profiles.len(), PROFILE_COUNT, "slots filled");
        assert!(settings.active < PROFILE_COUNT, "active clamped");
        assert_eq!(
            settings.profiles[0].name.chars().count(),
            MAX_PROFILE_NAME_LEN
        );
        assert_eq!(
            settings.profiles[0].contrast, MIN_CONTRAST,
            "contrast raised to what the panel will accept"
        );

        // More profiles than slots are dropped, not kept
        let many = DisplaySettings {
            active: 0,
            profiles: vec![DisplayProfile::default(); PROFILE_COUNT + 2],
        }
        .sanitized();
        assert_eq!(many.profiles.len(), PROFILE_COUNT);
    }

    #[test]
    fn a_flat_file_becomes_the_first_profile() {
        // Written by the build before profiles existed. The tuned values must carry over, or
        // upgrading would silently reset the panel.
        let legacy =
            r#"{"luminance":40,"hue":50,"saturation":50,"contrast":50,"r":50,"g":30,"b":20}"#;
        let parsed: DisplaySettings = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.active, 0);
        assert_eq!(parsed.profiles.len(), PROFILE_COUNT);
        assert_eq!(parsed.profiles[0].luminance, 40);
        assert_eq!((parsed.profiles[0].g, parsed.profiles[0].b), (30, 20));

        // The same file from the night-mode build: those keys are now unknown and must be
        // ignored rather than rejected.
        let with_night_mode = r#"{"luminance":40,"hue":50,"saturation":50,"contrast":50,
            "r":50,"g":30,"b":20,"night_mode":true,"night_mode_warmth":80,
            "night_mode_dimness":70}"#;
        let parsed: DisplaySettings = serde_json::from_str(with_night_mode).unwrap();
        assert_eq!(parsed.profiles[0].luminance, 40);
        assert_eq!(parsed.names(), vec!["Day", "Night", "Custom"]);
    }

    #[test]
    fn new_shape_round_trips() {
        let mut settings = DisplaySettings::new();
        settings.active = 2;
        settings.active_mut().name = "Movies".to_owned();
        settings.active_mut().saturation = 70;

        let json = serde_json::to_string(&settings).unwrap();
        let parsed: DisplaySettings = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, settings);
        assert_eq!(parsed.active().name, "Movies");
    }
}
