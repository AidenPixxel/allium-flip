//! The CPU clock, as far as the kernel's cpufreq lets it be steered from sysfs.

use std::fs;

use anyhow::{Context, Result};

/// The same node ffplay's launch script writes; `policy0` is the other spelling of it
const GOVERNOR: &str = "/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor";

/// Drops the CPU to the slowest clock the kernel offers, handing back the governor that was in
/// place so `restore` can put it back.
///
/// The kernel boots with `performance`, which pins 1.2 GHz -- including while everything is
/// SIGSTOPped behind a dark panel, where the clock buys nothing and the standby time pays for it.
/// `powersave` is the driver's floor. The driver reprograms the PLL itself on a governor change,
/// so anything set outside the driver has to be put back separately on the way out.
pub fn floor() -> Result<String> {
    let previous = fs::read_to_string(GOVERNOR)
        .context("failed to read the cpufreq governor")?
        .trim()
        .to_owned();
    fs::write(GOVERNOR, "powersave").context("failed to set the cpufreq governor")?;
    Ok(previous)
}

/// Puts back the governor `floor` replaced
pub fn restore(governor: &str) -> Result<()> {
    fs::write(GOVERNOR, governor).context("failed to restore the cpufreq governor")
}
