use std::fs::{self, File};
use std::io::Write;

use anyhow::{Context, Result};
use log::warn;
use sysfs_gpio::{Direction, Pin};

/// The panel's power line: what u-boot raises before the logo, and what MinUI drives low to sleep
const PANEL_POWER_GPIO: u64 = 4;

pub fn get_brightness() -> Result<u8> {
    Ok(
        fs::read_to_string("/sys/devices/soc0/soc/1f003400.pwm/pwm/pwmchip0/pwm0/duty_cycle")?
            .trim()
            .parse()?,
    )
}

pub fn set_brightness(brightness: u8) -> Result<()> {
    File::create("/sys/devices/soc0/soc/1f003400.pwm/pwm/pwmchip0/pwm0/duty_cycle")
        .context("failed to open pwm/duty_cycle")?
        .write_all(format!("{}", brightness.max(3)).as_bytes())?;
    Ok(())
}

/// Cuts power to the backlight, or restores it.
///
/// Something `set_brightness` cannot express: it floors its argument at 3 so the brightness slider
/// can never leave the user looking at a black screen, which means writing 0 there still leaves the
/// backlight burning. Suspend wants it genuinely off.
///
/// Addressed through `/sys/class` rather than the `/sys/devices/soc0/...` path used above. They are
/// two views of the same channel, but `/sys/class` is the spelling the boot script already writes
/// this exact node with, which is the only evidence available that the write lands.
pub fn set_backlight(on: bool) -> Result<()> {
    File::create("/sys/class/pwm/pwmchip0/pwm0/enable")
        .context("failed to open pwm/enable")?
        .write_all(if on { b"1" } else { b"0" })?;
    Ok(())
}

/// Powers the panel down, or back up.
///
/// `set_backlight(false)` stops the PWM, which is the lamp's dimming signal; the panel and its
/// driver stay powered behind it. This is the rest of the standby saving. The wake sequence is
/// MinUI's: line high, pin released, then the PWM re-latched by switching it off and on again --
/// the order it has been running on this hardware with.
///
/// The line is exported for the duration and released on wake rather than held, so a second
/// suspend's export does not fail on a pin that is already exported.
pub fn set_panel_power(on: bool) -> Result<()> {
    let pin = Pin::new(PANEL_POWER_GPIO);
    if on {
        pin.set_value(1)
            .context("failed to raise the panel power line")?;
        if let Err(err) = pin.unexport() {
            warn!("could not release the panel power line: {err}");
        }
        set_backlight(false)?;
        set_backlight(true)?;
    } else {
        pin.export()
            .context("failed to export the panel power line")?;
        pin.set_direction(Direction::Out)
            .context("failed to set the panel power line as an output")?;
        pin.set_value(0)
            .context("failed to lower the panel power line")?;
    }
    Ok(())
}

pub fn blank(blank: bool) -> Result<()> {
    File::create("/proc/mi_modules/fb/mi_fb0")
        .context("failed to open mi_fb0")?
        .write_all(if blank {
            b"GUI_SHOW 0 off"
        } else {
            b"GUI_SHOW 0 on"
        })?;
    Ok(())
}
