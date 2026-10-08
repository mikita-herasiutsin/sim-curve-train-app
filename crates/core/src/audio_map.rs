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

/// Silence: the tone the synth fades to between reps and after a drill.
pub const SILENT: ToneTarget = ToneTarget {
    frequency_hz: BASE_FREQ_HZ,
    gain: 0.0,
};

/// Share of the tolerance an in-band pedal may drift past the band before it counts as out
/// again. The hysteresis keeps the tone from fluttering when the pedal rests on the band edge.
pub const BAND_HYSTERESIS: f32 = 0.1;

/// How long the pedal must stay in the band before the lock chime plays (100 ms).
pub const CHIME_DWELL_US: u64 = 100_000;

/// One tone update from [`ToneTracker::step`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToneStep {
    pub tone: ToneTarget,
    /// The pedal has just been held in the band long enough: play the lock chime.
    pub chime: bool,
}

/// Turns a running drill's pedal samples into tone updates, one call per sample.
///
/// The pedal enters the band at `|error| <= tolerance`, as in scoring, and leaves it only past
/// `tolerance * (1 + BAND_HYSTERESIS)`. Hold drills play the lock chime once per rep, after the
/// pedal has stayed in the band for [`CHIME_DWELL_US`]. Trace drills never chime. The state
/// resets whenever no rep is active.
#[derive(Debug, Default)]
pub struct ToneTracker {
    in_band: bool,
    /// When the pedal last entered the band.
    entered_us: u64,
    /// The chime already played in this rep.
    chimed: bool,
}

impl ToneTracker {
    /// The tone for the sample at `t_us`.
    ///
    /// * `target` - Target pedal fraction (`0.0..=1.0`) while a rep is active, else `None`.
    /// * `value` - Calibrated pedal fraction (`0.0..=1.0`).
    /// * `tolerance` - Band half-width as a fraction (`0.05` for ±5 %).
    pub fn step(
        &mut self,
        t_us: u64,
        target: Option<f32>,
        value: f32,
        tolerance: f32,
        drill_kind: &DrillKind,
    ) -> ToneStep {
        let Some(target) = target else {
            *self = Self::default();
            return ToneStep {
                tone: SILENT,
                chime: false,
            };
        };
        let error = value - target;
        let limit = if self.in_band {
            tolerance * (1.0 + BAND_HYSTERESIS)
        } else {
            tolerance
        };
        let in_band = error.abs() <= limit;
        if in_band && !self.in_band {
            self.entered_us = t_us;
        }
        self.in_band = in_band;
        let chime = in_band
            && !self.chimed
            && matches!(drill_kind, DrillKind::Hold { .. })
            && t_us.saturating_sub(self.entered_us) >= CHIME_DWELL_US;
        self.chimed |= chime;
        ToneStep {
            tone: map_tone(error * 100.0, in_band, drill_kind),
            chime,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds `values` one per millisecond from `t0_us` and returns the samples that chimed.
    fn chimes_at(
        tracker: &mut ToneTracker,
        t0_us: u64,
        values: &[f32],
        kind: &DrillKind,
    ) -> Vec<u64> {
        let mut chimes = Vec::new();
        for (i, &value) in (0u64..).zip(values) {
            let now_us = t0_us + i * 1000;
            if tracker.step(now_us, Some(0.5), value, 0.05, kind).chime {
                chimes.push(now_us);
            }
        }
        chimes
    }

    #[test]
    fn tracker_silent_without_active_rep() {
        let mut tracker = ToneTracker::default();
        let step = tracker.step(0, None, 0.5, 0.05, &dummy_hold());
        assert_eq!(step.tone, SILENT);
        assert!(!step.chime);
    }

    #[test]
    fn tracker_chimes_once_after_dwell() {
        let mut tracker = ToneTracker::default();
        // 50 ms out, then 300 ms in the band: one chime, 100 ms after entry.
        let mut values = vec![0.2; 50];
        values.extend([0.5; 300]);
        assert_eq!(
            chimes_at(&mut tracker, 0, &values, &dummy_hold()),
            [50_000 + CHIME_DWELL_US]
        );
    }

    #[test]
    fn tracker_no_chime_for_brief_touch() {
        let mut tracker = ToneTracker::default();
        // In for 50 ms, out, in for 50 ms: never held long enough.
        let mut values = vec![0.5; 50];
        values.extend([0.3; 20]);
        values.extend([0.5; 50]);
        assert_eq!(
            chimes_at(&mut tracker, 0, &values, &dummy_hold()),
            Vec::<u64>::new()
        );
    }

    #[test]
    fn tracker_edge_dither_neither_chatters_nor_rechimes() {
        let mut tracker = ToneTracker::default();
        let kind = dummy_hold();
        // Settle in the band and chime.
        assert_eq!(chimes_at(&mut tracker, 0, &[0.5; 150], &kind).len(), 1);
        // Dither just past the edge, inside the hysteresis: stays in band, silent, no chime.
        for i in 0..200u64 {
            let value = if i % 2 == 0 { 0.549 } else { 0.552 };
            let step = tracker.step(150_000 + i * 1000, Some(0.5), value, 0.05, &kind);
            assert_eq!(step.tone.gain, 0.0, "sample {i}");
            assert!(!step.chime);
        }
    }

    #[test]
    fn tracker_leaves_band_past_hysteresis() {
        let mut tracker = ToneTracker::default();
        let kind = dummy_hold();
        assert_eq!(tracker.step(0, Some(0.5), 0.5, 0.05, &kind).tone.gain, 0.0);
        let out = tracker.step(1000, Some(0.5), 0.56, 0.05, &kind).tone;
        assert!(out.gain > 0.0);
        // Coming back needs the plain band again, not the widened one.
        let edge = tracker.step(2000, Some(0.5), 0.552, 0.05, &kind).tone;
        assert!(edge.gain > 0.0);
    }

    #[test]
    fn tracker_band_edge_is_inclusive_like_scoring() {
        // Exactly representable: |0.625 - 0.5| == 0.125.
        let mut tracker = ToneTracker::default();
        let step = tracker.step(0, Some(0.5), 0.625, 0.125, &dummy_hold());
        assert_eq!(step.tone.gain, 0.0);
    }

    #[test]
    fn tracker_rechimes_in_next_rep() {
        let mut tracker = ToneTracker::default();
        let kind = dummy_hold();
        assert_eq!(chimes_at(&mut tracker, 0, &[0.5; 150], &kind).len(), 1);
        // Rest between reps: no target.
        tracker.step(200_000, None, 0.5, 0.05, &kind);
        assert_eq!(
            chimes_at(&mut tracker, 300_000, &[0.5; 150], &kind).len(),
            1
        );
    }

    #[test]
    fn tracker_trace_never_chimes() {
        let mut tracker = ToneTracker::default();
        assert_eq!(
            chimes_at(&mut tracker, 0, &[0.5; 500], &dummy_trace()),
            Vec::<u64>::new()
        );
    }

    #[test]
    fn tracker_tone_follows_error_out_of_band() {
        let mut tracker = ToneTracker::default();
        let kind = dummy_hold();
        let low = tracker.step(0, Some(0.5), 0.3, 0.05, &kind).tone;
        let high = tracker.step(1000, Some(0.5), 0.7, 0.05, &kind).tone;
        assert!(low.frequency_hz < BASE_FREQ_HZ && low.gain > 0.0);
        assert!(high.frequency_hz > BASE_FREQ_HZ && high.gain > 0.0);
        // 20 % too far is a fifth of an octave up.
        assert!((high.frequency_hz - 440.0 * 0.2f32.exp2()).abs() < 0.01);
    }

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
