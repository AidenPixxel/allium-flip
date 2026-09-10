use std::fs::{self, File};
use std::io::Write;

use anyhow::{Context, Result};

use crate::display::backlight;

pub fn get_brightness() -> Result<u8> {
    let duty: u32 =
        fs::read_to_string("/sys/devices/soc0/soc/1f003400.pwm/pwm/pwmchip0/pwm0/duty_cycle")?
            .trim()
            .parse()?;
    Ok(backlight::brightness_for(duty))
}

pub fn set_brightness(brightness: u8) -> Result<()> {
    File::create("/sys/devices/soc0/soc/1f003400.pwm/pwm/pwmchip0/pwm0/duty_cycle")
        .context("failed to open pwm/duty_cycle")?
        .write_all(format!("{}", backlight::duty_for(brightness)).as_bytes())?;
    Ok(())
}

/// Cuts power to the backlight, or restores it.
///
/// Something `set_brightness` cannot express: its dimmest duty cycle is still lit, so that the
/// slider can never leave the user looking at a black screen. Suspend wants it genuinely off.
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
