use crate::preset::DrillKind;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToneTarget {
    pub frequency_hz: f32,
    pub gain: f32,
}

pub const DEFAULT_GAIN: f32 = 0.2;
pub const BASE_FREQ_HZ: f32 = 440.0;

/// Maps pedal error to an audio tone target (frequency and gain).
///
/// * `error_pct` - Signed error in percent (actual - target). Positive means the pedal is pressed too far.
/// * `in_band` - True if the pedal is currently within the acceptable tolerance band.
/// * `drill_kind` - The type of drill currently active.
#[must_use]
pub fn map_tone(error_pct: f32, in_band: bool, drill_kind: &DrillKind) -> ToneTarget {
    if !error_pct.is_finite() || in_band {
        return ToneTarget {
            frequency_hz: BASE_FREQ_HZ,
            gain: 0.0,
        };
    }

    match drill_kind {
        DrillKind::Hold { .. } => {
            // Symmetric log-pitch mapping: one octave per 100 % of error, so the tone spans
            // 220 Hz (pressed 100 % too little) to 880 Hz (100 % too far), 440 Hz on target.
            let clamped_err = error_pct.clamp(-100.0, 100.0);
            let frequency_hz = BASE_FREQ_HZ * (clamped_err / 100.0).exp2();

            ToneTarget {
                frequency_hz,
                gain: DEFAULT_GAIN,
            }
        }
        DrillKind::Trace { .. } => {
            // Trace drills just play a soft constant tone when out of band
            ToneTarget {
                frequency_hz: BASE_FREQ_HZ,
                gain: DEFAULT_GAIN * 0.5,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_hold() -> DrillKind {
        DrillKind::Hold {
            target: 50.0,
            hold_ms: 1000,
        }
    }

    fn dummy_trace() -> DrillKind {
        DrillKind::Trace {
            points: vec![(0, 0.0), (1000, 100.0)],
        }
    }

    #[test]
    fn silent_in_band() {
        let t = map_tone(5.0, true, &dummy_hold());
        assert_eq!(t.frequency_hz, BASE_FREQ_HZ);
        assert_eq!(t.gain, 0.0);
        let t = map_tone(-5.0, true, &dummy_trace());
        assert_eq!(t.frequency_hz, BASE_FREQ_HZ);
        assert_eq!(t.gain, 0.0);
    }

    #[test]
    fn non_finite_error_returns_silence() {
        let t_nan = map_tone(f32::NAN, false, &dummy_hold());
        assert_eq!(t_nan.frequency_hz, BASE_FREQ_HZ);
        assert_eq!(t_nan.gain, 0.0);

        let t_inf = map_tone(f32::INFINITY, false, &dummy_hold());
        assert_eq!(t_inf.frequency_hz, BASE_FREQ_HZ);
        assert_eq!(t_inf.gain, 0.0);

        let t_neg_inf = map_tone(f32::NEG_INFINITY, false, &dummy_trace());
        assert_eq!(t_neg_inf.frequency_hz, BASE_FREQ_HZ);
        assert_eq!(t_neg_inf.gain, 0.0);
    }

    #[test]
    fn trace_out_of_band_constant_soft_tone() {
        let t1 = map_tone(10.0, false, &dummy_trace());
        let t2 = map_tone(-10.0, false, &dummy_trace());
        assert_eq!(t1.frequency_hz, BASE_FREQ_HZ);
        assert_eq!(t2.frequency_hz, BASE_FREQ_HZ);
        assert!(t1.gain > 0.0 && t1.gain < DEFAULT_GAIN); // Softer than default
    }

    #[test]
    fn hold_monotonic_pitch_vs_error() {
        let t_low = map_tone(-20.0, false, &dummy_hold());
        let t_high = map_tone(20.0, false, &dummy_hold());

        assert!(t_low.frequency_hz < BASE_FREQ_HZ);
        assert!(t_high.frequency_hz > BASE_FREQ_HZ);
        assert_eq!(t_low.gain, DEFAULT_GAIN);
        assert_eq!(t_high.gain, DEFAULT_GAIN);

        let t_very_high = map_tone(50.0, false, &dummy_hold());
        assert!(t_very_high.frequency_hz > t_high.frequency_hz);
    }

    #[test]
    fn log_pitch_is_symmetric_octaves() {
        let on = map_tone(0.0001, false, &dummy_hold()).frequency_hz;
        assert!((on - 440.0).abs() < 0.5);
        let hi = map_tone(100.0, false, &dummy_hold()).frequency_hz;
        let lo = map_tone(-100.0, false, &dummy_hold()).frequency_hz;
        assert!((hi - 880.0).abs() < 0.01);
        assert!((lo - 220.0).abs() < 0.01);
        let hi50 = map_tone(50.0, false, &dummy_hold()).frequency_hz;
        let lo50 = map_tone(-50.0, false, &dummy_hold()).frequency_hz;
        assert!((hi50 / 440.0 - 440.0 / lo50).abs() < 1e-4);
    }

    #[test]
    fn clamped_ranges() {
        let t1 = map_tone(150.0, false, &dummy_hold());
        let t2 = map_tone(100.0, false, &dummy_hold());
        assert_eq!(t1.frequency_hz, t2.frequency_hz);

        let t3 = map_tone(-150.0, false, &dummy_hold());
        let t4 = map_tone(-100.0, false, &dummy_hold());
        assert_eq!(t3.frequency_hz, t4.frequency_hz);
    }
}
