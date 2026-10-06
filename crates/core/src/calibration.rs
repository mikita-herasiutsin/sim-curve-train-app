//! Pedal axis calibration and normalisation.

use serde::{Deserialize, Serialize};

/// Minimum valid raw span (`max - min`) required for calibration (approx. 10% of `i16` range).
pub const MIN_SPAN: i32 = 6554;

/// Full 16-bit signed range calibration with zero deadzones.
pub const FULL_RANGE: AxisCalibration = AxisCalibration {
    min: i16::MIN,
    max: i16::MAX,
    invert: false,
    deadzone_low: 0.0,
    deadzone_high: 0.0,
};

/// Calibration configuration for a single pedal axis.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AxisCalibration {
    /// Raw sensor value at minimum pedal travel.
    pub min: i16,
    /// Raw sensor value at maximum pedal travel.
    pub max: i16,
    /// Whether the axis direction should be inverted.
    pub invert: bool,
    /// Deadzone at the lower end of travel, expressed as a fraction in `[0.0, 0.5)`.
    pub deadzone_low: f32,
    /// Deadzone at the upper end of travel, expressed as a fraction in `[0.0, 0.5)`.
    pub deadzone_high: f32,
}

impl Default for AxisCalibration {
    fn default() -> Self {
        FULL_RANGE
    }
}

/// Errors that can occur when validating an [`AxisCalibration`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CalibrationError {
    /// `min` is not strictly less than `max`.
    MinNotLessThanMax {
        /// The configured minimum raw value.
        min: i16,
        /// The configured maximum raw value.
        max: i16,
    },
    /// The travel span (`max - min`) is smaller than [`MIN_SPAN`].
    SpanTooSmall {
        /// The measured or computed span.
        span: i32,
        /// The minimum required span.
        min_span: i32,
    },
    /// `deadzone_low` is not finite or outside `[0.0, 0.5)`.
    InvalidDeadzoneLow(f32),
    /// `deadzone_high` is not finite or outside `[0.0, 0.5)`.
    InvalidDeadzoneHigh(f32),
}

impl std::fmt::Display for CalibrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::MinNotLessThanMax { min, max } => {
                write!(
                    f,
                    "minimum raw value ({min}) must be strictly less than maximum ({max})"
                )
            }
            Self::SpanTooSmall { span, min_span } => {
                write!(
                    f,
                    "travel span ({span}) is less than minimum required span ({min_span})"
                )
            }
            Self::InvalidDeadzoneLow(dz) => {
                write!(
                    f,
                    "low deadzone ({dz}) must be finite and in the range [0.0, 0.5)"
                )
            }
            Self::InvalidDeadzoneHigh(dz) => {
                write!(
                    f,
                    "high deadzone ({dz}) must be finite and in the range [0.0, 0.5)"
                )
            }
        }
    }
}

impl std::error::Error for CalibrationError {}

impl AxisCalibration {
    /// Normalises a raw sensor reading to the range `0.0..=1.0`.
    ///
    /// The input is clamped to `[min, max]`, inverted if [`invert`](Self::invert) is set,
    /// and scaled according to deadzones. Never returns `NaN`, returning `0.0` if `min >= max`.
    #[must_use]
    pub fn normalise(&self, raw: i16) -> f32 {
        if self.min >= self.max {
            return 0.0;
        }

        let min_f = f64::from(self.min);
        let max_f = f64::from(self.max);
        let raw_f = f64::from(raw);
        let span = max_f - min_f;
        if span <= 0.0 {
            return 0.0;
        }

        let mut t = ((raw_f - min_f) / span).clamp(0.0, 1.0);
        if self.invert {
            t = 1.0 - t;
        }

        let dz_low = f64::from(self.deadzone_low);
        let dz_high = f64::from(self.deadzone_high);

        if !dz_low.is_finite() || !dz_high.is_finite() {
            return 0.0;
        }

        if t <= dz_low {
            0.0
        } else if t >= 1.0 - dz_high {
            1.0
        } else {
            let denom = 1.0 - dz_low - dz_high;
            if denom <= 0.0 {
                return 0.0;
            }
            let norm = ((t - dz_low) / denom).clamp(0.0, 1.0);
            #[expect(
                clippy::cast_possible_truncation,
                reason = "norm is in 0.0..=1.0 and fits within f32 range"
            )]
            let norm_f32 = norm as f32;
            if norm_f32.is_nan() { 0.0 } else { norm_f32 }
        }
    }

    /// Returns the normalised percentage in `0.0..=100.0`.
    #[must_use]
    pub fn percent(&self, raw: i16) -> f32 {
        self.normalise(raw) * 100.0
    }

    /// Validates the calibration parameters.
    ///
    /// # Errors
    ///
    /// Returns [`CalibrationError`] if `min >= max`, `max - min < MIN_SPAN`, or either
    /// deadzone is not finite or outside `[0.0, 0.5)`.
    pub fn validate(&self) -> Result<(), CalibrationError> {
        if self.min >= self.max {
            return Err(CalibrationError::MinNotLessThanMax {
                min: self.min,
                max: self.max,
            });
        }
        let span = i32::from(self.max) - i32::from(self.min);
        if span < MIN_SPAN {
            return Err(CalibrationError::SpanTooSmall {
                span,
                min_span: MIN_SPAN,
            });
        }
        if !self.deadzone_low.is_finite() || self.deadzone_low < 0.0 || self.deadzone_low >= 0.5 {
            return Err(CalibrationError::InvalidDeadzoneLow(self.deadzone_low));
        }
        if !self.deadzone_high.is_finite() || self.deadzone_high < 0.0 || self.deadzone_high >= 0.5
        {
            return Err(CalibrationError::InvalidDeadzoneHigh(self.deadzone_high));
        }
        Ok(())
    }
}

/// Records minimum and maximum raw values during a pedal sweep to produce an [`AxisCalibration`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RangeCapture {
    min: Option<i16>,
    max: Option<i16>,
}

impl RangeCapture {
    /// Creates a new, empty `RangeCapture`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            min: None,
            max: None,
        }
    }

    /// Observes a raw sensor reading during the sweep.
    pub fn observe(&mut self, raw: i16) {
        self.min = Some(self.min.map_or(raw, |m| m.min(raw)));
        self.max = Some(self.max.map_or(raw, |m| m.max(raw)));
    }

    /// Returns the captured `(min, max)` range, or `None` if no samples have been observed.
    #[must_use]
    pub fn range(&self) -> Option<(i16, i16)> {
        match (self.min, self.max) {
            (Some(min), Some(max)) => Some((min, max)),
            _ => None,
        }
    }

    /// Finalizes calibration using the captured range, resting position, and deadzones.
    ///
    /// Sets `invert = true` when `rest` is closer to the captured maximum than to the minimum,
    /// ensuring that a released pedal reads 0% and a pressed pedal reads 100%.
    ///
    /// # Errors
    ///
    /// Returns [`CalibrationError::SpanTooSmall`] if no observations were recorded or if
    /// the sweep span was too small. Returns other [`CalibrationError`] variants if the deadzones
    /// are invalid.
    pub fn finish(
        &self,
        rest: i16,
        deadzone_low: f32,
        deadzone_high: f32,
    ) -> Result<AxisCalibration, CalibrationError> {
        let Some((min, max)) = self.range() else {
            return Err(CalibrationError::SpanTooSmall {
                span: 0,
                min_span: MIN_SPAN,
            });
        };

        let dist_min = (i32::from(rest) - i32::from(min)).abs();
        let dist_max = (i32::from(rest) - i32::from(max)).abs();
        let invert = dist_max < dist_min;

        let cal = AxisCalibration {
            min,
            max,
            invert,
            deadzone_low,
            deadzone_high,
        };
        cal.validate()?;
        Ok(cal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_full_range() {
        assert_eq!(AxisCalibration::default(), FULL_RANGE);
        assert_eq!(FULL_RANGE.min, -32768);
        assert_eq!(FULL_RANGE.max, 32767);
        assert!(!AxisCalibration::default().invert);
        assert_eq!(FULL_RANGE.deadzone_low, 0.0);
        assert_eq!(FULL_RANGE.deadzone_high, 0.0);
    }

    #[test]
    fn released_and_pressed_normal_axis() {
        let cal = AxisCalibration {
            min: -30000,
            max: 30000,
            invert: false,
            deadzone_low: 0.0,
            deadzone_high: 0.0,
        };
        assert_eq!(cal.normalise(-30000), 0.0);
        assert_eq!(cal.percent(-30000), 0.0);

        assert_eq!(cal.normalise(30000), 1.0);
        assert_eq!(cal.percent(30000), 100.0);
    }

    #[test]
    fn released_and_pressed_inverted_axis() {
        let cal = AxisCalibration {
            min: -30000,
            max: 30000,
            invert: true,
            deadzone_low: 0.0,
            deadzone_high: 0.0,
        };
        // For an inverted axis, released pedal is at max (+30000) reading 0%,
        // pressed pedal is at min (-30000) reading 100%.
        assert_eq!(cal.normalise(30000), 0.0);
        assert_eq!(cal.percent(30000), 0.0);

        assert_eq!(cal.normalise(-30000), 1.0);
        assert_eq!(cal.percent(-30000), 100.0);
    }

    #[test]
    fn midpoint() {
        let cal = AxisCalibration {
            min: 0,
            max: 10000,
            invert: false,
            deadzone_low: 0.0,
            deadzone_high: 0.0,
        };
        assert!((cal.normalise(5000) - 0.5).abs() < 1e-6);
        assert!((cal.percent(5000) - 50.0).abs() < 1e-4);

        let cal_inv = AxisCalibration {
            invert: true,
            ..cal
        };
        assert!((cal_inv.normalise(5000) - 0.5).abs() < 1e-6);
        assert!((cal_inv.percent(5000) - 50.0).abs() < 1e-4);
    }

    #[test]
    fn clamping_outside_min_max() {
        let cal = AxisCalibration {
            min: 0,
            max: 10000,
            invert: false,
            deadzone_low: 0.0,
            deadzone_high: 0.0,
        };
        assert_eq!(cal.normalise(-500), 0.0);
        assert_eq!(cal.percent(-500), 0.0);
        assert_eq!(cal.normalise(12000), 1.0);
        assert_eq!(cal.percent(12000), 100.0);

        let cal_inv = AxisCalibration {
            invert: true,
            ..cal
        };
        assert_eq!(cal_inv.normalise(-500), 1.0);
        assert_eq!(cal_inv.percent(-500), 100.0);
        assert_eq!(cal_inv.normalise(12000), 0.0);
        assert_eq!(cal_inv.percent(12000), 0.0);
    }

    #[test]
    fn deadzone_at_edges() {
        let cal = AxisCalibration {
            min: 0,
            max: 10000,
            invert: false,
            deadzone_low: 0.10,
            deadzone_high: 0.10,
        };
        // Exactly at or inside low deadzone (<= 1000)
        assert_eq!(cal.normalise(1000), 0.0);
        assert_eq!(cal.normalise(500), 0.0);

        // Exactly at or inside high deadzone (>= 9000)
        assert_eq!(cal.normalise(9000), 1.0);
        assert_eq!(cal.normalise(9500), 1.0);

        // Midpoint with symmetric deadzones remains 0.5
        assert!((cal.normalise(5000) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn degenerate_min_ge_max_returns_zero_and_no_nan() {
        let cal_eq = AxisCalibration {
            min: 1000,
            max: 1000,
            invert: false,
            deadzone_low: 0.0,
            deadzone_high: 0.0,
        };
        assert_eq!(cal_eq.normalise(1000), 0.0);
        assert_eq!(cal_eq.percent(1000), 0.0);
        assert!(!cal_eq.normalise(1000).is_nan());

        let cal_inverted_bounds = AxisCalibration {
            min: 2000,
            max: 1000,
            invert: false,
            deadzone_low: 0.0,
            deadzone_high: 0.0,
        };
        assert_eq!(cal_inverted_bounds.normalise(1500), 0.0);
        assert!(!cal_inverted_bounds.normalise(1500).is_nan());
    }

    #[test]
    fn validation_errors() {
        // min >= max
        let cal_min_ge_max = AxisCalibration {
            min: 5000,
            max: 5000,
            ..FULL_RANGE
        };
        assert_eq!(
            cal_min_ge_max.validate(),
            Err(CalibrationError::MinNotLessThanMax {
                min: 5000,
                max: 5000
            })
        );

        // span too small
        let cal_small_span = AxisCalibration {
            min: 0,
            max: 6000,
            ..FULL_RANGE
        };
        assert_eq!(
            cal_small_span.validate(),
            Err(CalibrationError::SpanTooSmall {
                span: 6000,
                min_span: MIN_SPAN,
            })
        );

        // invalid deadzone low
        let cal_bad_dz_low = AxisCalibration {
            deadzone_low: -0.01,
            ..FULL_RANGE
        };
        assert_eq!(
            cal_bad_dz_low.validate(),
            Err(CalibrationError::InvalidDeadzoneLow(-0.01))
        );

        let cal_bad_dz_low_high = AxisCalibration {
            deadzone_low: 0.5,
            ..FULL_RANGE
        };
        assert_eq!(
            cal_bad_dz_low_high.validate(),
            Err(CalibrationError::InvalidDeadzoneLow(0.5))
        );

        // invalid deadzone high
        let cal_bad_dz_high = AxisCalibration {
            deadzone_high: 0.6,
            ..FULL_RANGE
        };
        assert_eq!(
            cal_bad_dz_high.validate(),
            Err(CalibrationError::InvalidDeadzoneHigh(0.6))
        );

        let cal_nan_dz = AxisCalibration {
            deadzone_high: f32::NAN,
            ..FULL_RANGE
        };
        match cal_nan_dz.validate() {
            Err(CalibrationError::InvalidDeadzoneHigh(v)) => assert!(v.is_nan()),
            other => panic!("expected InvalidDeadzoneHigh, got {other:?}"),
        }

        // valid calibration passes
        assert!(FULL_RANGE.validate().is_ok());
    }

    #[test]
    fn error_display_messages() {
        let err1 = CalibrationError::MinNotLessThanMax { min: 100, max: 100 };
        assert_eq!(
            err1.to_string(),
            "minimum raw value (100) must be strictly less than maximum (100)"
        );

        let err2 = CalibrationError::SpanTooSmall {
            span: 1000,
            min_span: MIN_SPAN,
        };
        assert_eq!(
            err2.to_string(),
            format!("travel span (1000) is less than minimum required span ({MIN_SPAN})")
        );

        let err3 = CalibrationError::InvalidDeadzoneLow(-0.1);
        assert_eq!(
            err3.to_string(),
            "low deadzone (-0.1) must be finite and in the range [0.0, 0.5)"
        );

        let err4 = CalibrationError::InvalidDeadzoneHigh(0.5);
        assert_eq!(
            err4.to_string(),
            "high deadzone (0.5) must be finite and in the range [0.0, 0.5)"
        );
    }

    #[test]
    fn range_capture_sweeps() {
        // Empty capture returns span error
        let capture_empty = RangeCapture::new();
        assert_eq!(capture_empty.range(), None);
        assert_eq!(
            capture_empty.finish(0, 0.0, 0.0),
            Err(CalibrationError::SpanTooSmall {
                span: 0,
                min_span: MIN_SPAN,
            })
        );

        // Rest at min -> normal (invert = false)
        let mut capture_normal = RangeCapture::default();
        capture_normal.observe(-30000);
        capture_normal.observe(0);
        capture_normal.observe(30000);
        assert_eq!(capture_normal.range(), Some((-30000, 30000)));

        let cal_normal = capture_normal.finish(-30000, 0.02, 0.03).unwrap();
        assert_eq!(cal_normal.min, -30000);
        assert_eq!(cal_normal.max, 30000);
        assert!(!cal_normal.invert);
        assert_eq!(cal_normal.deadzone_low, 0.02);
        assert_eq!(cal_normal.deadzone_high, 0.03);

        // Rest at max -> inverted (invert = true)
        let mut capture_inverted = RangeCapture::new();
        capture_inverted.observe(-30000);
        capture_inverted.observe(30000);
        let cal_inverted = capture_inverted.finish(30000, 0.01, 0.01).unwrap();
        assert_eq!(cal_inverted.min, -30000);
        assert_eq!(cal_inverted.max, 30000);
        assert!(cal_inverted.invert);

        // Sweep too small (< MIN_SPAN)
        let mut capture_small = RangeCapture::new();
        capture_small.observe(1000);
        capture_small.observe(2000);
        assert_eq!(
            capture_small.finish(1000, 0.0, 0.0),
            Err(CalibrationError::SpanTooSmall {
                span: 1000,
                min_span: MIN_SPAN,
            })
        );
    }

    #[test]
    fn serde_json_roundtrip_camel_case() {
        let cal = AxisCalibration {
            min: -32000,
            max: 32000,
            invert: true,
            deadzone_low: 0.125,
            deadzone_high: 0.25,
        };

        let json_value = serde_json::to_value(cal).unwrap();
        assert_eq!(json_value["min"], -32000);
        assert_eq!(json_value["max"], 32000);
        assert_eq!(json_value["invert"], true);
        assert_eq!(json_value["deadzoneLow"], 0.125);
        assert_eq!(json_value["deadzoneHigh"], 0.25);

        let roundtrip: AxisCalibration = serde_json::from_value(json_value).unwrap();
        assert_eq!(cal, roundtrip);
    }
}
