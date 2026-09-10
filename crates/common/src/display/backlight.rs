//! Turning the brightness slider into a backlight PWM duty cycle.
//!
//! Duty maps to emitted light roughly linearly, but what the eye reads as one step is a ratio, not
//! a difference. A slider written straight to the duty cycle therefore spends its top half on
//! changes nobody can see and crams every useful step into the bottom few percent -- which is
//! exactly the range a dark room lives in. One press near the old floor, 3 to 8, nearly tripled the
//! light; the same press at the top changed it by a twentieth. These space the steps by ratio, so
//! every press is the same multiple of the last.

/// The dimmest duty cycle the slider reaches, against the period of 800 the boot script sets.
/// Genuinely dim, for a pitch-dark room, but still lit: suspend cuts the backlight outright
/// through `set_backlight` rather than coming through here.
const DUTY_MIN: f32 = 1.0;
/// The brightest, unchanged from what the slider has always produced at 100.
const DUTY_MAX: f32 = 100.0;
/// The top of the slider's own 0..=SLIDER_MAX scale.
const SLIDER_MAX: u8 = 100;

/// The duty cycle for a slider position: 0 gives 1, 50 gives 10, 100 gives 100.
pub fn duty_for(brightness: u8) -> u32 {
    let t = f32::from(brightness.min(SLIDER_MAX)) / f32::from(SLIDER_MAX);
    (DUTY_MIN * (DUTY_MAX / DUTY_MIN).powf(t)).round() as u32
}

/// The slider position `duty_for` would turn back into `duty`.
///
/// Reading the panel back is how brightness survives suspend, so the two have to agree: if this
/// returned the raw duty, as it used to, every suspend would resume dimmer than the last.
pub fn brightness_for(duty: u32) -> u8 {
    let duty = (duty as f32).clamp(DUTY_MIN, DUTY_MAX);
    ((duty / DUTY_MIN).ln() / (DUTY_MAX / DUTY_MIN).ln() * f32::from(SLIDER_MAX)).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ends_are_where_they_should_be() {
        assert_eq!(duty_for(0), 1, "dimmest, but still lit");
        assert_eq!(duty_for(100), 100, "the top of the slider is unchanged");
        assert_eq!(duty_for(50), 10);
        // What the old linear slider's floor of 3 now corresponds to
        assert_eq!(duty_for(20), 3);
    }

    #[test]
    fn brighter_is_never_dimmer() {
        let mut previous = 0;
        for brightness in 0..=100u8 {
            let duty = duty_for(brightness);
            assert!(
                duty >= previous,
                "{brightness} gave {duty} after {previous}"
            );
            previous = duty;
        }
    }

    #[test]
    fn out_of_range_is_clamped_not_wrapped() {
        assert_eq!(duty_for(u8::MAX), duty_for(100));
        assert_eq!(brightness_for(0), 0, "below the dimmest duty");
        assert_eq!(brightness_for(10_000), 100, "above the brightest");
    }

    #[test]
    fn a_duty_round_trips_to_its_own_slider_position() {
        // Suspend saves the duty and restores it through these two; a disagreement would drift
        for brightness in 0..=100u8 {
            let round_tripped = brightness_for(duty_for(brightness));
            let drift = i32::from(round_tripped) - i32::from(brightness);
            // Duty is a small integer, so several slider positions share one at the dim end;
            // landing on the same duty again is what matters, not the exact number
            assert_eq!(
                duty_for(round_tripped),
                duty_for(brightness),
                "{brightness} -> {round_tripped} (drift {drift}) changed the duty"
            );
        }
    }

    #[test]
    fn each_step_is_the_same_multiple() {
        // The whole point: equal presses, equal perceived change. Checked in the upper half,
        // where the duty is large enough that integer rounding does not dominate.
        for brightness in (50..=95u8).step_by(5) {
            let ratio = duty_for(brightness + 5) as f32 / duty_for(brightness) as f32;
            assert!(
                (ratio - 1.26).abs() < 0.08,
                "{brightness} -> {} was a factor of {ratio}",
                brightness + 5
            );
        }
    }
}
