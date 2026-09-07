use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use log::{debug, warn};
use serde::{Deserialize, Serialize};
use strum::FromRepr;

/// The standard Linux cpufreq interface for the first core. Allium already depends on this being
/// writable: `cores/ffplay/launch_ffplay.sh` echoes `performance` into it before playing a video.
const CPUFREQ_DIR: &str = "/sys/devices/system/cpu/cpu0/cpufreq";

/// A CPU governor preset.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, FromRepr, Default)]
pub enum PerformanceMode {
    /// Leave the governor exactly as the system left it. The default, so installing this build
    /// changes nothing about how the device clocks until the setting is actually used.
    #[default]
    System,
    /// The slowest clock the driver offers. `powersave` pins the *minimum* frequency rather than
    /// scaling down gently, so this is a real speed cut and not a mild saving.
    Battery,
    /// Scale with load, which is what a stock kernel normally does.
    Balanced,
    /// Hold the maximum clock for as long as the game runs.
    Performance,
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
            PerformanceMode::Battery => &["powersave", "conservative", "ondemand", "schedutil"],
            PerformanceMode::Balanced => &["ondemand", "schedutil", "conservative", "powersave"],
            PerformanceMode::Performance => &["performance", "schedutil", "ondemand"],
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

fn cpufreq(file: &str) -> PathBuf {
    Path::new(CPUFREQ_DIR).join(file)
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

/// Records the governor the device booted with, so [`restore`] can put it back.
///
/// Must be called before anything has had a chance to change it -- alliumd does so at startup. If
/// it is never called, `restore` reads whatever is current at that point and writing it back is a
/// harmless no-op.
pub fn capture() {
    let _ = original_governor();
}

/// Points the CPU governor at `mode`.
///
/// Never fails: this runs on the game launch and resume paths, where nothing here is worth
/// stopping a game from starting over.
pub fn apply(mode: PerformanceMode) {
    if mode == PerformanceMode::System {
        // Hands the governor back rather than doing nothing, so switching a running game to
        // System takes effect straight away instead of leaving the previous mode in place until
        // the game is quit. At launch this is the same value it already has.
        debug!("performance mode is System, handing the cpu governor back");
        restore();
        return;
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

/// Puts back the governor the device booted with.
///
/// Unconditional rather than only-after-[`apply`]: `cores/ffplay/launch_ffplay.sh` sets
/// `performance` itself and never restores it, so the governor can be left wrong even when Allium
/// never touched it. Called both when returning to the launcher and by `apply(System)`.
pub fn restore() {
    let Some(original) = original_governor() else {
        return;
    };
    write_governor(original.trim());
}

fn write_governor(governor: &str) {
    match fs::write(cpufreq("scaling_governor"), governor) {
        Ok(()) => debug!("cpu governor set to {governor}"),
        Err(err) => warn!("failed to set the cpu governor to {governor}: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list a stock kernel with the usual governors would report.
    const TYPICAL: &str = "conservative ondemand userspace powersave performance schedutil\n";

    #[test]
    fn each_preset_picks_its_own_governor() {
        assert_eq!(choose(PerformanceMode::Battery, TYPICAL), Some("powersave"));
        assert_eq!(choose(PerformanceMode::Balanced, TYPICAL), Some("ondemand"));
        assert_eq!(
            choose(PerformanceMode::Performance, TYPICAL),
            Some("performance")
        );
    }

    #[test]
    fn system_never_writes_anything() {
        assert_eq!(choose(PerformanceMode::System, TYPICAL), None);
    }

    #[test]
    fn presets_degrade_to_what_the_driver_has() {
        // A kernel built with only the two extremes: Balanced has to settle for one of them
        // rather than writing a governor that does not exist.
        let minimal = "powersave performance";
        assert_eq!(
            choose(PerformanceMode::Balanced, minimal),
            Some("powersave")
        );
        assert_eq!(
            choose(PerformanceMode::Performance, minimal),
            Some("performance")
        );

        // schedutil-only, as newer kernels ship
        let schedutil = "schedutil";
        assert_eq!(
            choose(PerformanceMode::Battery, schedutil),
            Some("schedutil")
        );
        assert_eq!(
            choose(PerformanceMode::Performance, schedutil),
            Some("schedutil")
        );
    }

    #[test]
    fn nothing_recognisable_writes_nothing() {
        // Better to leave the governor alone than to write a name the driver will reject
        for mode in [
            PerformanceMode::Battery,
            PerformanceMode::Balanced,
            PerformanceMode::Performance,
        ] {
            assert_eq!(choose(mode, ""), None);
            assert_eq!(choose(mode, "interactive vendor_magic"), None);
        }
    }

    #[test]
    fn repr_order_matches_the_settings_select() {
        // The Select row maps its index straight to the discriminant, so this order is part of
        // the on-disk format: changing it silently repoints every saved setting.
        assert_eq!(PerformanceMode::from_repr(0), Some(PerformanceMode::System));
        assert_eq!(
            PerformanceMode::from_repr(1),
            Some(PerformanceMode::Battery)
        );
        assert_eq!(
            PerformanceMode::from_repr(2),
            Some(PerformanceMode::Balanced)
        );
        assert_eq!(
            PerformanceMode::from_repr(3),
            Some(PerformanceMode::Performance)
        );
        assert_eq!(PerformanceMode::from_repr(4), None);
    }
}
