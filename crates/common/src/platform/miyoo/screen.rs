use std::fs::{self, File};
use std::io::Write;
use std::sync::LazyLock;

use anyhow::{Context, Result};
use log::warn;

use crate::display::backlight;

const PWM: &str = "/sys/devices/soc0/soc/1f003400.pwm/pwm/pwmchip0/pwm0";

/// The PWM period, read once from the hardware.
///
/// The boot script sets it and the curve has to span it, so reading it back is what keeps the two
/// in step. Assuming the boot script's value instead is how the slider ended up covering an eighth
/// of the panel: the two were written by different commits and nothing ever compared them.
static PERIOD: LazyLock<u32> = LazyLock::new(|| {
    match fs::read_to_string(format!("{PWM}/period")).map(|text| text.trim().parse::<u32>()) {
        Ok(Ok(period)) => period,
        Ok(Err(err)) => {
            warn!("PWM period is not a number ({err}); assuming the boot script's value");
            backlight::NOMINAL_PERIOD
        }
        Err(err) => {
            warn!("could not read the PWM period ({err}); assuming the boot script's value");
            backlight::NOMINAL_PERIOD
        }
    }
});

pub fn get_brightness() -> Result<u8> {
    let duty: u32 = fs::read_to_string(format!("{PWM}/duty_cycle"))?
        .trim()
        .parse()?;
    Ok(backlight::brightness_for(duty, *PERIOD))
}

pub fn set_brightness(brightness: u8) -> Result<()> {
    File::create(format!("{PWM}/duty_cycle"))
        .context("failed to open pwm/duty_cycle")?
        .write_all(format!("{}", backlight::duty_for(brightness, *PERIOD)).as_bytes())?;
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
