//! Depth and pop-out sliders work in hundredths; this is the mapping between
//! the stored floating-point preference, the integer slider position and the
//! two-decimal readout. Port of the invariants checked by Any2Odd's
//! `SliderValidation`.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SliderSpec {
    pub min: i32,
    pub max: i32,
    pub tick: i32,
}

pub const DEPTH: SliderSpec = SliderSpec {
    min: 0,
    max: 200,
    tick: 25,
};
pub const POPOUT: SliderSpec = SliderSpec {
    min: -100,
    max: 100,
    tick: 25,
};

impl SliderSpec {
    /// Preference value (e.g. 0.9) to slider position (90), clamped.
    pub fn position(&self, value: f64) -> i32 {
        let v = if value.is_finite() { value } else { 0.0 };
        ((v * 100.0).round() as i32).clamp(self.min, self.max)
    }
    /// Slider position to preference value.
    pub fn value(&self, position: i32) -> f64 {
        position.clamp(self.min, self.max) as f64 / 100.0
    }
    pub fn readout(&self, position: i32) -> String {
        format!("{:.2}", self.value(position))
    }
}

/// The preference ranges and the slider ranges must agree.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::prefs::{DEPTH_MAX, DEPTH_MIN, POPOUT_MAX, POPOUT_MIN};

    #[test]
    fn ranges_match_preferences() {
        assert_eq!(DEPTH.value(DEPTH.min), DEPTH_MIN);
        assert_eq!(DEPTH.value(DEPTH.max), DEPTH_MAX);
        assert_eq!(POPOUT.value(POPOUT.min), POPOUT_MIN);
        assert_eq!(POPOUT.value(POPOUT.max), POPOUT_MAX);
    }

    #[test]
    fn position_value_readout_round_trip() {
        assert_eq!(DEPTH.position(0.9), 90);
        assert_eq!(DEPTH.readout(90), "0.90");
        assert_eq!(DEPTH.position(2.7), 200);
        assert_eq!(DEPTH.position(f64::NAN), 0);
        assert_eq!(POPOUT.position(-0.25), -25);
        assert_eq!(POPOUT.readout(-25), "-0.25");
        assert_eq!(POPOUT.readout(0), "0.00");
        for p in DEPTH.min..=DEPTH.max {
            assert_eq!(DEPTH.position(DEPTH.value(p)), p);
        }
        for p in POPOUT.min..=POPOUT.max {
            assert_eq!(POPOUT.position(POPOUT.value(p)), p);
        }
    }
}
