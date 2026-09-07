use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use log::{debug, warn};
use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};
use strum::FromRepr;

/// The standard Linux cpufreq interface for the first core. Allium already depends on this being
/// writable: `cores/ffplay/launch_ffplay.sh` echoes `performance` into it before playing a video.
const CPUFREQ_DIR: &str = "/sys/devices/system/cpu/cpu0/cpufreq";

/// A CPU speed preset.
///
/// The discriminants are the on-disk format twice over -- `PowerSettings` serialises the variant
/// name and the database column stores `mode as i64` -- so this list is **append-only**. That is
/// why it is not in speed order: `High` and `Max` hold the slots the original `Balanced` and
/// `Performance` had, and reproduce their behaviour exactly, so a value already stored still means
/// what it did. The two middle tiers went on the end. [`DISPLAY_ORDER`] is what the UI walks.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, FromRepr, Default)]
#[serde(from = "RawPerformanceMode")]
pub enum PerformanceMode {
    /// Leave the governor and the ceiling exactly as the system left them. The default, so
    /// installing this build changes nothing about how the device clocks until it is used.
    #[default]
    System = 0,
    /// `powersave`, which pins the *minimum* frequency rather than scaling down gently. A real
    /// speed cut, not a mild saving -- fine for Game Boy, too slow for GBA.
    Powersave = 1,
    /// Scale with load, up to the device maximum. What a stock kernel normally does.
    High = 2,
    /// Hold the maximum clock for as long as the game runs.
    Max = 3,
    /// Scale with load, but no faster than about half the driver's range.
    Low = 4,
    /// Scale with load, but no faster than about three quarters of the driver's range.
    Medium = 5,
}

/// The presets slowest first, as the UI walks them. Separate from the discriminants because those
/// are stored on disk and can only be appended to.
pub const DISPLAY_ORDER: [PerformanceMode; 6] = [
    PerformanceMode::System,
    PerformanceMode::Powersave,
    PerformanceMode::Low,
    PerformanceMode::Medium,
    PerformanceMode::High,
    PerformanceMode::Max,
];

/// Accepts every shape this field has ever been written in, and anything it has not, so an
/// unrecognised value can never fail the parse.
///
/// That matters because [`crate::power::PowerSettings::load`] *deletes* `power.json` when
/// deserialization fails, taking every other power setting with it -- so renaming a variant here
/// would otherwise reset the user's power button, lid and charging behaviour.
#[derive(Deserialize)]
#[serde(untagged)]
enum RawPerformanceMode {
    Name(String),
    Repr(u8),
    Unknown(IgnoredAny),
}

impl From<RawPerformanceMode> for PerformanceMode {
    fn from(raw: RawPerformanceMode) -> Self {
        match raw {
            RawPerformanceMode::Name(name) => match name.as_str() {
                "System" => Self::System,
                // Battery was this governor under a name that oversold what it does
                "Powersave" | "Battery" => Self::Powersave,
                "Low" => Self::Low,
                "Medium" => Self::Medium,
                // The two presets that always worked, renamed but unchanged
                "High" | "Balanced" => Self::High,
                "Max" | "Performance" => Self::Max,
                _ => Self::default(),
            },
            RawPerformanceMode::Repr(repr) => Self::from_repr(repr as usize).unwrap_or_default(),
            RawPerformanceMode::Unknown(_) => Self::default(),
        }
    }
}

impl PerformanceMode {
    /// Governor names to try, best first.
    ///
    /// Nothing guarantees a given kernel was built with any particular governor -- the vendor SDK
    /// has no CPU frequency API at all, so the only evidence any of these exist is that
    /// `launch_ffplay.sh` writes `performance` -- so each preset degrades towards the nearest
    /// thing the driver actually reports.
    fn candidates(self) -> &'static [&'static str] {
        match self {
            PerformanceMode::System => &[],
            PerformanceMode::Powersave => &["powersave", "conservative", "ondemand", "schedutil"],
            PerformanceMode::Low | PerformanceMode::Medium | PerformanceMode::High => {
                &["ondemand", "schedutil", "conservative", "powersave"]
            }
            PerformanceMode::Max => &["performance", "schedutil", "ondemand"],
        }
    }

    /// Where this preset caps the clock, as a percentage of the driver's range, or `None` to let
    /// it run right up to the device maximum.
    fn ceiling_percent(self) -> Option<u32> {
        match self {
            PerformanceMode::Low => Some(50),
            PerformanceMode::Medium => Some(75),
            _ => None,
        }
    }
}

/// Picks the governor to write for `mode` out of the whitespace-separated names the driver
/// reports in `scaling_available_governors`.
fn choose(mode: PerformanceMode, available: &str) -> Option<&'static str> {
    let available: Vec<&str> = available.split_whitespace().collect();
    mode.candidates()
        .iter()
        .copied()
        .find(|candidate| available.contains(candidate))
}

/// The frequency to write to `scaling_max_freq` for `mode`, in kHz.
///
/// `None` leaves the ceiling alone, which happens only for `System` and on a driver that publishes
/// no range. Every other preset writes one -- **including the uncapped ones, which write the device
/// maximum**, because the ceiling persists: without that, switching from Low to Max would leave the
/// old cap in place and Max would silently do nothing.
fn ceiling_for(mode: PerformanceMode, table: &[u64], range: Option<(u64, u64)>) -> Option<u64> {
    if mode == PerformanceMode::System {
        return None;
    }

    let (min, max) = range?;
    let Some(percent) = mode.ceiling_percent() else {
        return Some(max);
    };

    let target = min + (max - min) * u64::from(percent) / 100;

    // Snap down to a frequency the driver actually offers, so the settings label can name the
    // value without writing anything first. Left to itself the kernel rounds to an operating
    // point, and the label and the hardware would then disagree.
    Some(
        table
            .iter()
            .copied()
            .filter(|frequency| *frequency <= target)
            .max()
            .unwrap_or(target)
            .max(min),
    )
}

fn cpufreq(file: &str) -> PathBuf {
    Path::new(CPUFREQ_DIR).join(file)
}

fn read_frequency(file: &str) -> Option<u64> {
    fs::read_to_string(cpufreq(file)).ok()?.trim().parse().ok()
}

/// The governor list, read once. `None` where there is no cpufreq interface at all, which is also
/// the case in the simulator -- hence one warning per process rather than one per launch.
fn available_governors() -> Option<&'static str> {
    static AVAILABLE: OnceLock<Option<String>> = OnceLock::new();
    AVAILABLE
        .get_or_init(
            || match fs::read_to_string(cpufreq("scaling_available_governors")) {
                Ok(available) => {
                    debug!("cpufreq governors available: {}", available.trim());
                    Some(available)
                }
                Err(err) => {
                    warn!("no cpufreq governors, performance mode will do nothing: {err}");
                    None
                }
            },
        )
        .as_deref()
}

/// The operating points the driver publishes, in kHz, ascending. Empty on a driver that does not
/// publish a table, which is why the tiers can also be computed from the range alone.
fn available_frequencies() -> &'static [u64] {
    static TABLE: OnceLock<Vec<u64>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let Ok(raw) = fs::read_to_string(cpufreq("scaling_available_frequencies")) else {
            debug!("no cpufreq frequency table, tiers will come from the range instead");
            return Vec::new();
        };
        let mut frequencies: Vec<u64> = raw
            .split_whitespace()
            .filter_map(|frequency| frequency.parse().ok())
            .collect();
        frequencies.sort_unstable();
        frequencies.dedup();
        debug!("cpufreq frequencies available: {frequencies:?} kHz");
        frequencies
    })
}

/// The slowest and fastest the hardware will go, in kHz.
fn frequency_range() -> Option<(u64, u64)> {
    static RANGE: OnceLock<Option<(u64, u64)>> = OnceLock::new();
    *RANGE.get_or_init(|| {
        let min = read_frequency("cpuinfo_min_freq")?;
        let max = read_frequency("cpuinfo_max_freq")?;
        if max <= min {
            warn!("cpu frequency range is {min}-{max} kHz, ignoring it");
            return None;
        }
        debug!("cpu frequency range: {min}-{max} kHz");
        Some((min, max))
    })
}

/// The ceiling `mode` resolves to on this device, in MHz, when it caps below the device maximum.
///
/// `None` for the presets that run at full speed and on a driver with no range to work from -- in
/// both cases there is no number worth putting in front of the user.
pub fn ceiling_mhz(mode: PerformanceMode) -> Option<u64> {
    mode.ceiling_percent()?;
    ceiling_for(mode, available_frequencies(), frequency_range()).map(|khz| khz / 1000)
}

/// The governor the device booted with, read once.
fn original_governor() -> Option<&'static str> {
    static ORIGINAL: OnceLock<Option<String>> = OnceLock::new();
    ORIGINAL
        .get_or_init(|| match fs::read_to_string(cpufreq("scaling_governor")) {
            Ok(governor) => {
                debug!("cpu governor at startup: {}", governor.trim());
                Some(governor)
            }
            Err(err) => {
                debug!("could not read the cpu governor: {err}");
                None
            }
        })
        .as_deref()
}

/// The ceiling the device booted with, read once.
fn original_ceiling() -> Option<u64> {
    static ORIGINAL: OnceLock<Option<u64>> = OnceLock::new();
    *ORIGINAL.get_or_init(|| {
        let ceiling = read_frequency("scaling_max_freq");
        match ceiling {
            Some(khz) => debug!("cpu ceiling at startup: {khz} kHz"),
            None => debug!("could not read the cpu ceiling"),
        }
        ceiling
    })
}

/// Records the governor and ceiling the device booted with, so [`restore`] can put them back.
///
/// Must be called before anything has had a chance to change them -- alliumd does so at startup.
/// If it is never called, `restore` reads whatever is current at that point and writing it back is
/// a harmless no-op.
pub fn capture() {
    let _ = original_governor();
    let _ = original_ceiling();
}

/// Points the CPU at `mode`.
///
/// Never fails: this runs on the game launch and resume paths, where nothing here is worth
/// stopping a game from starting over.
pub fn apply(mode: PerformanceMode) {
    if mode == PerformanceMode::System {
        // Hands the CPU back rather than doing nothing, so switching a running game to System
        // takes effect straight away instead of leaving the previous preset in place until the
        // game is quit. At launch this writes back the values it already has.
        debug!("performance mode is System, handing the cpu back");
        restore();
        return;
    }

    // Ceiling before governor, so raising it out of a capped tier takes effect immediately rather
    // than waiting for the governor to next re-evaluate
    if let Some(khz) = ceiling_for(mode, available_frequencies(), frequency_range()) {
        write_ceiling(khz);
    }

    let Some(available) = available_governors() else {
        return;
    };
    let Some(governor) = choose(mode, available) else {
        warn!(
            "no governor suits {mode:?}, available are: {}",
            available.trim()
        );
        return;
    };

    write_governor(governor);
}

/// Puts back the governor and ceiling the device booted with.
///
/// Unconditional rather than only-after-[`apply`]: `cores/ffplay/launch_ffplay.sh` sets
/// `performance` itself and never restores it, so the governor can be left wrong even when Allium
/// never touched it. Called both when returning to the launcher and by `apply(System)`.
pub fn restore() {
    if let Some(khz) = original_ceiling() {
        write_ceiling(khz);
    }
    if let Some(original) = original_governor() {
        write_governor(original.trim());
    }
}

fn write_governor(governor: &str) {
    match fs::write(cpufreq("scaling_governor"), governor) {
        Ok(()) => debug!("cpu governor set to {governor}"),
        Err(err) => warn!("failed to set the cpu governor to {governor}: {err}"),
    }
}

fn write_ceiling(khz: u64) {
    match fs::write(cpufreq("scaling_max_freq"), khz.to_string()) {
        Ok(()) => debug!("cpu ceiling set to {khz} kHz"),
        Err(err) => warn!("failed to set the cpu ceiling to {khz} kHz: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list a stock kernel with the usual governors would report.
    const TYPICAL: &str = "conservative ondemand userspace powersave performance schedutil\n";

    /// A plausible table for this SoC: `cpuclock.txt` was committed as "eggs' overclock cpu
    /// (1600)", so 1.6 GHz is the top of the range here.
    const TABLE: [u64; 7] = [
        400_000, 600_000, 800_000, 1_000_000, 1_200_000, 1_400_000, 1_600_000,
    ];
    const RANGE: Option<(u64, u64)> = Some((400_000, 1_600_000));

    #[test]
    fn each_preset_picks_its_own_governor() {
        assert_eq!(
            choose(PerformanceMode::Powersave, TYPICAL),
            Some("powersave")
        );
        assert_eq!(choose(PerformanceMode::Low, TYPICAL), Some("ondemand"));
        assert_eq!(choose(PerformanceMode::Medium, TYPICAL), Some("ondemand"));
        assert_eq!(choose(PerformanceMode::High, TYPICAL), Some("ondemand"));
        assert_eq!(choose(PerformanceMode::Max, TYPICAL), Some("performance"));
    }

    #[test]
    fn system_never_writes_anything() {
        assert_eq!(choose(PerformanceMode::System, TYPICAL), None);
        assert_eq!(ceiling_for(PerformanceMode::System, &TABLE, RANGE), None);
    }

    #[test]
    fn presets_degrade_to_what_the_driver_has() {
        // A kernel built with only the two extremes: the load-following tiers have to settle for
        // one of them rather than writing a governor that does not exist.
        let minimal = "powersave performance";
        assert_eq!(choose(PerformanceMode::High, minimal), Some("powersave"));
        assert_eq!(choose(PerformanceMode::Max, minimal), Some("performance"));

        // schedutil-only, as newer kernels ship
        let schedutil = "schedutil";
        assert_eq!(
            choose(PerformanceMode::Powersave, schedutil),
            Some("schedutil")
        );
        assert_eq!(choose(PerformanceMode::Max, schedutil), Some("schedutil"));
    }

    #[test]
    fn nothing_recognisable_writes_no_governor() {
        // Better to leave the governor alone than to write a name the driver will reject
        for mode in DISPLAY_ORDER {
            assert_eq!(choose(mode, ""), None);
            assert_eq!(choose(mode, "interactive vendor_magic"), None);
        }
    }

    #[test]
    fn middle_tiers_land_on_real_operating_points() {
        // 50% of 400..1600 MHz is 1000 MHz, 75% is 1300 MHz, which snaps down to 1200
        assert_eq!(
            ceiling_for(PerformanceMode::Low, &TABLE, RANGE),
            Some(1_000_000)
        );
        assert_eq!(
            ceiling_for(PerformanceMode::Medium, &TABLE, RANGE),
            Some(1_200_000)
        );
        assert!(
            ceiling_for(PerformanceMode::Low, &TABLE, RANGE)
                < ceiling_for(PerformanceMode::Medium, &TABLE, RANGE),
            "the tiers have to be a ladder, not two names for one speed"
        );
    }

    #[test]
    fn uncapped_presets_still_write_the_maximum() {
        // The ceiling persists, so anything that does not cap has to put it back up: otherwise
        // going Low -> Max leaves the old cap in place and Max does nothing at all.
        for mode in [
            PerformanceMode::Powersave,
            PerformanceMode::High,
            PerformanceMode::Max,
        ] {
            assert_eq!(ceiling_for(mode, &TABLE, RANGE), Some(1_600_000));
        }
    }

    #[test]
    fn no_table_falls_back_to_the_range() {
        // Some drivers publish no operating points. The computed value stands on its own and the
        // kernel rounds it to whatever it supports.
        assert_eq!(
            ceiling_for(PerformanceMode::Low, &[], RANGE),
            Some(1_000_000)
        );
        assert_eq!(
            ceiling_for(PerformanceMode::Medium, &[], RANGE),
            Some(1_300_000)
        );
    }

    #[test]
    fn a_cap_never_drops_below_the_minimum() {
        // A narrow range where half of it would land under the slowest operating point
        let narrow = Some((1_000_000, 1_100_000));
        assert_eq!(
            ceiling_for(PerformanceMode::Low, &TABLE, narrow),
            Some(1_000_000)
        );
    }

    #[test]
    fn no_frequency_information_caps_nothing() {
        // With no range there is nothing to take a percentage of, so the middle tiers collapse
        // into High rather than guessing a frequency
        for mode in DISPLAY_ORDER {
            assert_eq!(ceiling_for(mode, &TABLE, None), None);
        }
    }

    #[test]
    fn repr_order_is_append_only() {
        // These are stored in the database as integers. System and Powersave keep the slots they
        // had, and High and Max keep the slots the original Balanced and Performance had, whose
        // behaviour they reproduce -- so a stored value still means what it meant.
        assert_eq!(PerformanceMode::from_repr(0), Some(PerformanceMode::System));
        assert_eq!(
            PerformanceMode::from_repr(1),
            Some(PerformanceMode::Powersave)
        );
        assert_eq!(PerformanceMode::from_repr(2), Some(PerformanceMode::High));
        assert_eq!(PerformanceMode::from_repr(3), Some(PerformanceMode::Max));
        assert_eq!(PerformanceMode::from_repr(4), Some(PerformanceMode::Low));
        assert_eq!(PerformanceMode::from_repr(5), Some(PerformanceMode::Medium));
        assert_eq!(PerformanceMode::from_repr(6), None);
    }

    #[test]
    fn the_ladder_is_slowest_first_and_complete() {
        assert_eq!(
            DISPLAY_ORDER,
            [
                PerformanceMode::System,
                PerformanceMode::Powersave,
                PerformanceMode::Low,
                PerformanceMode::Medium,
                PerformanceMode::High,
                PerformanceMode::Max,
            ]
        );
    }

    #[test]
    fn every_shape_of_stored_value_parses() {
        // Nothing here may fail: a parse error makes PowerSettings::load delete power.json and
        // reset the power button, lid and charging settings along with it.
        for (json, expected) in [
            (r#""System""#, PerformanceMode::System),
            (r#""Powersave""#, PerformanceMode::Powersave),
            (r#""Low""#, PerformanceMode::Low),
            (r#""Medium""#, PerformanceMode::Medium),
            (r#""High""#, PerformanceMode::High),
            (r#""Max""#, PerformanceMode::Max),
            // The names the first version of this feature wrote, mapped to the presets whose
            // behaviour they had
            (r#""Battery""#, PerformanceMode::Powersave),
            (r#""Balanced""#, PerformanceMode::High),
            (r#""Performance""#, PerformanceMode::Max),
            // A bare discriminant, as the database column holds
            ("4", PerformanceMode::Low),
            // Values from a build that does not exist yet, and outright junk
            (r#""Turbo""#, PerformanceMode::System),
            ("99", PerformanceMode::System),
            ("true", PerformanceMode::System),
            ("null", PerformanceMode::System),
            ("{}", PerformanceMode::System),
        ] {
            assert_eq!(
                serde_json::from_str::<PerformanceMode>(json).unwrap(),
                expected,
                "{json} should parse as {expected:?}"
            );
        }
    }

    #[test]
    fn names_round_trip() {
        for mode in DISPLAY_ORDER {
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(
                serde_json::from_str::<PerformanceMode>(&json).unwrap(),
                mode
            );
        }
    }
}
