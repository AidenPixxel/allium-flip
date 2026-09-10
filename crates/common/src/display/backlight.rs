//! Turning the brightness slider into a backlight PWM duty cycle.
//!
//! Two things have to be right, and the slider got neither for a long time.
//!
//! It has to cover the panel's range. A 0-100 settings percentage was once written straight into
//! the duty cycle, against a period of 800 -- so Allium used an eighth of what the backlight can
//! do, and at the dim end there were barely any integers left to land on: 20% and 25% came out as
//! the same duty. The curve now spans `DUTY_MIN` to the period itself.
//!
//! And its steps have to be even. Duty maps to emitted light roughly linearly, but what the eye
//! reads as one step is a ratio, not a difference, so a linear slider spends its top half on
//! changes nobody can see. These space the steps by ratio: every press is the same multiple of the
//! last, which over 3..800 is about x1.32 per five -- comfortably more than the x1.33 it takes for
//! consecutive integers to stay distinct from 3 upward.

use log::warn;

/// The dimmest duty the panel actually lights. Below it the screen goes black rather than dim, so
/// there would be nothing to choose between down there.
const DUTY_MIN: f32 = 3.0;

/// The PWM period the boot script sets, and the fallback when it cannot be read back.
///
/// Prefer the value read from the hardware: this exists because `static/.tmp_update/updater` and
/// this file would otherwise agree only by coincidence, which is how the range came to be capped
/// at an eighth of the panel in the first place.
pub const NOMINAL_PERIOD: u32 = 800;

/// Guards against a period so small the curve would invert.
fn usable_period(period: u32) -> f32 {
    if period as f32 > DUTY_MIN {
        period as f32
    } else {
        warn!("PWM period {period} is below the panel's floor; using {NOMINAL_PERIOD}");
        NOMINAL_PERIOD as f32
    }
}

/// The duty cycle for a slider position: 0 gives the dimmest lit setting, 100 gives full output.
pub fn duty_for(brightness: u8, period: u32) -> u32 {
    let max = usable_period(period);
    let t = f32::from(brightness.min(100)) / 100.0;
    (DUTY_MIN * (max / DUTY_MIN).powf(t)).round() as u32
}

/// The slider position `duty_for` would turn back into `duty`.
///
/// Reading the panel back is how brightness survives suspend, so the two have to agree: if this
/// disagreed with `duty_for`, every suspend would resume at a different brightness than the last.
pub fn brightness_for(duty: u32, period: u32) -> u8 {
    let max = usable_period(period);
    let duty = (duty as f32).clamp(DUTY_MIN, max);
    ((duty / DUTY_MIN).ln() / (max / DUTY_MIN).ln() * 100.0).round() as u8
}

/// The slider position that now gives the light an older one used to.
///
/// Before this the curve ran over duty 1..100 rather than 3..period, so the same number is far
/// more light than it was. Stored profiles are put through here once, on load, so a device comes
/// back from the update looking exactly as it did going in. Uses [`NOMINAL_PERIOD`] rather than
/// the live period: it is undoing a conversion that was itself written against that value.
pub fn rescale_from_legacy(old: u8) -> u8 {
    let legacy_duty = 100f32.powf(f32::from(old.min(100)) / 100.0).round() as u32;
    brightness_for(legacy_duty, NOMINAL_PERIOD)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PERIOD: u32 = NOMINAL_PERIOD;

    #[test]
    fn the_ends_are_the_panel_ends() {
        assert_eq!(duty_for(0, PERIOD), 3, "dimmest the panel will light");
        assert_eq!(
            duty_for(100, PERIOD),
            PERIOD,
            "the whole range, not an eighth"
        );
    }

    #[test]
    fn every_step_of_five_changes_the_duty() {
        // The regression this module exists for: 20% and 25% used to be the same setting, and the
        // old round-trip test tolerated it. Nothing may collapse now.
        let mut previous = duty_for(0, PERIOD);
        for brightness in (5..=100u8).step_by(5) {
            let duty = duty_for(brightness, PERIOD);
            assert!(
                duty > previous,
                "{brightness}% gave {duty}, same as the step below"
            );
            previous = duty;
        }
    }

    #[test]
    fn brighter_is_never_dimmer() {
        let mut previous = 0;
        for brightness in 0..=100u8 {
            let duty = duty_for(brightness, PERIOD);
            assert!(
                duty >= previous,
                "{brightness} gave {duty} after {previous}"
            );
            previous = duty;
        }
    }

    #[test]
    fn out_of_range_is_clamped_not_wrapped() {
        assert_eq!(duty_for(u8::MAX, PERIOD), duty_for(100, PERIOD));
        assert_eq!(brightness_for(0, PERIOD), 0, "below the dimmest lit duty");
        assert_eq!(brightness_for(u32::MAX, PERIOD), 100, "above full output");
    }

    #[test]
    fn a_duty_round_trips_so_suspend_cannot_drift() {
        for brightness in 0..=100u8 {
            let duty = duty_for(brightness, PERIOD);
            let recovered = brightness_for(duty, PERIOD);
            assert_eq!(
                duty_for(recovered, PERIOD),
                duty,
                "{brightness}% -> duty {duty} -> {recovered}% changed the duty"
            );
        }
    }

    #[test]
    fn the_curve_follows_whatever_period_the_hardware_reports() {
        for period in [400u32, 800, 4000] {
            assert_eq!(duty_for(100, period), period);
            assert_eq!(duty_for(0, period), 3);
            let mut previous = duty_for(0, period);
            for brightness in (5..=100u8).step_by(5) {
                let duty = duty_for(brightness, period);
                assert!(duty > previous, "period {period}: {brightness}% collapsed");
                previous = duty;
            }
        }
        // A nonsensical period falls back rather than inverting the curve
        assert_eq!(duty_for(100, 1), duty_for(100, NOMINAL_PERIOD));
    }

    #[test]
    fn the_rescale_keeps_the_light_where_it_was() {
        // The old curve was duty = 100^(b/100) over 1..100, floored at 3 by the last release.
        assert_eq!(rescale_from_legacy(20), 0, "the old floor, duty 3");
        assert_eq!(rescale_from_legacy(85), 50, "the old default, duty 50");
        assert_eq!(rescale_from_legacy(100), 63, "the old maximum, duty 100");

        let mut previous = 0;
        for old in 0..=100u8 {
            let new = rescale_from_legacy(old);
            assert!(new >= previous, "{old} rescaled below the step under it");
            previous = new;
        }
        assert!(
            rescale_from_legacy(100) < 100,
            "the old maximum has to leave headroom above it, or nothing was gained"
        );
    }
}
