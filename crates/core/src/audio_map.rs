/// Share of the tolerance an in-band pedal may drift past the band before it counts as out
/// again. The hysteresis keeps the sound from fluttering when the pedal rests on the band edge.
pub const BAND_HYSTERESIS: f32 = 0.1;

/// Beeps per second right at the band edge.
pub const PULSE_RATE_MIN_HZ: f32 = 3.0;

/// Beeps per second at [`PULSE_SIZE_SPAN_PCT`] or more past the band edge.
pub const PULSE_RATE_MAX_HZ: f32 = 11.0;

/// Distance past the band edge, in percentage points of pedal travel, at which the beeps
/// reach their top rate.
pub const PULSE_SIZE_SPAN_PCT: f32 = 30.0;

/// Curvature of the rate curve: below 1 the rate rises quickly just past the band edge.
const PULSE_CURVE_EXPONENT: f32 = 0.6;

/// The beep rate (pulses per second) for a pedal `error_pct` percentage points from the target
/// with a band of ±`tolerance_pct`, parking-sensor style: [`PULSE_RATE_MIN_HZ`] at the band edge,
/// rising with the distance past it to [`PULSE_RATE_MAX_HZ`] at [`PULSE_SIZE_SPAN_PCT`] and
/// beyond. Over- and undershoot sound the same. Non-finite input gives `0.0` (silence).
#[must_use]
pub fn pulse_rate_hz(error_pct: f32, tolerance_pct: f32) -> f32 {
    if !error_pct.is_finite() || !tolerance_pct.is_finite() {
        return 0.0;
    }
    let distance = (error_pct.abs() - tolerance_pct).max(0.0);
    let k = (distance / PULSE_SIZE_SPAN_PCT)
        .min(1.0)
        .powf(PULSE_CURVE_EXPONENT);
    PULSE_RATE_MIN_HZ + (PULSE_RATE_MAX_HZ - PULSE_RATE_MIN_HZ) * k
}

/// Turns a running drill's pedal samples into the beep rate, one call per sample. Silent
/// (rate `0.0`) while the pedal is in the band.
///
/// The pedal enters the band at `|error| <= tolerance`, as in scoring, and leaves it only past
/// `tolerance * (1 + BAND_HYSTERESIS)`; the rate itself is measured from the band edge
/// (`tolerance`). Hold and Trace drills behave the same. The state resets whenever no rep is
/// active.
#[derive(Debug, Default)]
pub struct ToneTracker {
    in_band: bool,
}

impl ToneTracker {
    /// The beep rate in Hz for this sample, `0.0` for silence.
    ///
    /// * `target` - Target pedal fraction (`0.0..=1.0`) while a rep is active, else `None`.
    /// * `value` - Calibrated pedal fraction (`0.0..=1.0`).
    /// * `tolerance` - Band half-width as a fraction (`0.05` for ±5 %).
    pub fn step(&mut self, target: Option<f32>, value: f32, tolerance: f32) -> f32 {
        let Some(target) = target else {
            *self = Self::default();
            return 0.0;
        };
        let error = value - target;
        let limit = if self.in_band {
            tolerance * (1.0 + BAND_HYSTERESIS)
        } else {
            tolerance
        };
        self.in_band = error.abs() <= limit;
        if self.in_band {
            return 0.0;
        }
        pulse_rate_hz(error * 100.0, tolerance * 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_is_min_at_the_edge_and_max_far_out() {
        assert_eq!(pulse_rate_hz(5.0, 5.0), PULSE_RATE_MIN_HZ);
        assert!((pulse_rate_hz(35.0, 5.0) - PULSE_RATE_MAX_HZ).abs() < 1e-4);
        assert_eq!(pulse_rate_hz(80.0, 5.0), PULSE_RATE_MAX_HZ);
        assert_eq!(pulse_rate_hz(-100.0, 5.0), PULSE_RATE_MAX_HZ);
    }

    #[test]
    fn rate_rises_monotonically_between_edge_and_span() {
        let mut prev = pulse_rate_hz(5.0, 5.0);
        for i in 1..=60_u8 {
            let rate = pulse_rate_hz(5.0 + f32::from(i) * 0.5, 5.0);
            assert!(
                rate > prev || (rate - PULSE_RATE_MAX_HZ).abs() < 1e-4,
                "{i}"
            );
            assert!((PULSE_RATE_MIN_HZ..=PULSE_RATE_MAX_HZ + 1e-4).contains(&rate));
            prev = rate;
        }
    }

    #[test]
    fn rate_is_symmetric_for_over_and_undershoot() {
        for e in [6.0, 10.0, 20.0, 33.0] {
            assert_eq!(pulse_rate_hz(e, 5.0), pulse_rate_hz(-e, 5.0));
        }
    }

    #[test]
    fn rate_non_finite_is_zero() {
        assert_eq!(pulse_rate_hz(f32::NAN, 5.0), 0.0);
        assert_eq!(pulse_rate_hz(f32::INFINITY, 5.0), 0.0);
        assert_eq!(pulse_rate_hz(10.0, f32::NAN), 0.0);
    }

    #[test]
    fn tracker_silent_in_band() {
        let mut tracker = ToneTracker::default();
        assert_eq!(tracker.step(Some(0.5), 0.5, 0.05), 0.0);
        assert_eq!(tracker.step(Some(0.5), 0.54, 0.05), 0.0);
    }

    #[test]
    fn tracker_silent_without_active_rep() {
        let mut tracker = ToneTracker::default();
        assert_eq!(tracker.step(None, 0.9, 0.05), 0.0);
    }

    #[test]
    fn tracker_rate_grows_with_distance() {
        let mut tracker = ToneTracker::default();
        let near = tracker.step(Some(0.5), 0.58, 0.05);
        let far = tracker.step(Some(0.5), 0.8, 0.05);
        assert!(near >= PULSE_RATE_MIN_HZ && far > near);
        let under = tracker.step(Some(0.5), 0.2, 0.05);
        assert!((under - far).abs() < 1e-3);
    }

    #[test]
    fn tracker_band_edge_is_inclusive_like_scoring() {
        // Exactly representable: |0.625 - 0.5| == 0.125.
        let mut tracker = ToneTracker::default();
        assert_eq!(tracker.step(Some(0.5), 0.625, 0.125), 0.0);
        assert!(tracker.step(Some(0.5), 0.9, 0.125) > 0.0);
    }

    #[test]
    fn tracker_edge_dither_stays_out_then_returns_to_silence() {
        let mut tracker = ToneTracker::default();
        // Settle in band, then leave well past the hysteresis limit.
        assert_eq!(tracker.step(Some(0.5), 0.5, 0.05), 0.0);
        assert!(tracker.step(Some(0.5), 0.6, 0.05) > 0.0);
        // Dither between the band edge and the hysteresis limit: still out of band.
        for i in 0..100 {
            let value = if i % 2 == 0 { 0.5 + 0.052 } else { 0.5 + 0.057 };
            assert!(tracker.step(Some(0.5), value, 0.05) > 0.0, "sample {i}");
        }
        // Back within the tolerance: silent.
        assert_eq!(tracker.step(Some(0.5), 0.549, 0.05), 0.0);
        // Inside the hysteresis zone now counts as in band.
        assert_eq!(tracker.step(Some(0.5), 0.553, 0.05), 0.0);
        assert!(tracker.step(Some(0.5), 0.56, 0.05) > 0.0);
    }

    #[test]
    fn tracker_state_resets_between_reps() {
        let mut tracker = ToneTracker::default();
        assert_eq!(tracker.step(Some(0.5), 0.5, 0.05), 0.0);
        // Hysteresis zone while in band: silent.
        assert_eq!(tracker.step(Some(0.5), 0.553, 0.05), 0.0);
        assert_eq!(tracker.step(None, 0.553, 0.05), 0.0);
        // A fresh rep starts out of band at that position.
        assert!(tracker.step(Some(0.5), 0.553, 0.05) > 0.0);
    }
}
