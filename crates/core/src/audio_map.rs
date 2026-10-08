use crate::preset::DrillKind;

/// Share of the tolerance an in-band pedal may drift past the band before it counts as out
/// again. The hysteresis keeps the cues from fluttering when the pedal rests on the band edge.
pub const BAND_HYSTERESIS: f32 = 0.1;

/// How long the pedal must stay in the band before the lock chime plays, and before leaving the
/// band counts as a miss (100 ms).
pub const CHIME_DWELL_US: u64 = 100_000;

/// At most one miss cue per this long, measured from the previous miss cue (1 s).
pub const MISS_COOLDOWN_US: u64 = 1_000_000;

/// The sound cues for one sample, from [`ToneTracker::step`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToneStep {
    /// The pedal has just been held in the band long enough: play the lock chime.
    pub chime: bool,
    /// The pedal has just left the band after settling in it: play the soft miss cue.
    pub miss: bool,
}

/// Turns a running drill's pedal samples into sound cues, one call per sample. Nothing sounds
/// while the pedal is out of the band.
///
/// The pedal enters the band at `|error| <= tolerance`, as in scoring, and leaves it only past
/// `tolerance * (1 + BAND_HYSTERESIS)`. Hold drills play the lock chime once per rep, after the
/// pedal has stayed in the band for [`CHIME_DWELL_US`]. Both drill kinds play the miss cue on
/// the sample where a pedal that had settled in the band (for at least [`CHIME_DWELL_US`])
/// leaves it, at most once per [`MISS_COOLDOWN_US`]. Approaching the target from outside, or
/// brushing through the band briefly, never plays it. The state resets whenever no rep is active.
#[derive(Debug, Default)]
pub struct ToneTracker {
    in_band: bool,
    /// When the pedal last entered the band.
    entered_us: u64,
    /// The chime already played in this rep.
    chimed: bool,
    /// When the last miss cue played in this rep.
    last_miss_us: Option<u64>,
}

impl ToneTracker {
    /// The cues for the sample at `t_us`.
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
                chime: false,
                miss: false,
            };
        };
        let error = value - target;
        let was_in_band = self.in_band;
        let limit = if was_in_band {
            tolerance * (1.0 + BAND_HYSTERESIS)
        } else {
            tolerance
        };
        let in_band = error.abs() <= limit;
        if in_band && !was_in_band {
            self.entered_us = t_us;
        }
        self.in_band = in_band;
        let settled = t_us.saturating_sub(self.entered_us) >= CHIME_DWELL_US;
        let chime =
            in_band && !self.chimed && matches!(drill_kind, DrillKind::Hold { .. }) && settled;
        self.chimed |= chime;
        let miss = was_in_band
            && !in_band
            && settled
            && self
                .last_miss_us
                .is_none_or(|last| t_us.saturating_sub(last) >= MISS_COOLDOWN_US);
        if miss {
            self.last_miss_us = Some(t_us);
        }
        ToneStep { chime, miss }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Feeds `values` one per millisecond from `t0_us` and returns the times of the samples that
    /// chimed and of those that played the miss cue.
    fn cues(
        tracker: &mut ToneTracker,
        t0_us: u64,
        values: &[f32],
        kind: &DrillKind,
    ) -> (Vec<u64>, Vec<u64>) {
        let (mut chimes, mut misses) = (Vec::new(), Vec::new());
        for (i, &value) in (0u64..).zip(values) {
            let now_us = t0_us + i * 1000;
            let step = tracker.step(now_us, Some(0.5), value, 0.05, kind);
            if step.chime {
                chimes.push(now_us);
            }
            if step.miss {
                misses.push(now_us);
            }
        }
        (chimes, misses)
    }

    fn chimes_at(
        tracker: &mut ToneTracker,
        t0_us: u64,
        values: &[f32],
        kind: &DrillKind,
    ) -> Vec<u64> {
        cues(tracker, t0_us, values, kind).0
    }

    fn misses_at(
        tracker: &mut ToneTracker,
        t0_us: u64,
        values: &[f32],
        kind: &DrillKind,
    ) -> Vec<u64> {
        cues(tracker, t0_us, values, kind).1
    }

    /// `n` samples at `value`.
    fn run(value: f32, n: usize) -> Vec<f32> {
        vec![value; n]
    }

    #[test]
    fn tracker_silent_without_active_rep() {
        let mut tracker = ToneTracker::default();
        let step = tracker.step(0, None, 0.5, 0.05, &dummy_hold());
        assert!(!step.chime && !step.miss);
    }

    #[test]
    fn tracker_chimes_once_after_dwell() {
        let mut tracker = ToneTracker::default();
        // 50 ms out, then 300 ms in the band: one chime, 100 ms after entry.
        let mut values = run(0.2, 50);
        values.extend(run(0.5, 300));
        assert_eq!(
            chimes_at(&mut tracker, 0, &values, &dummy_hold()),
            [50_000 + CHIME_DWELL_US]
        );
    }

    #[test]
    fn tracker_no_chime_for_brief_touch() {
        let mut tracker = ToneTracker::default();
        // In for 50 ms, out, in for 50 ms: never held long enough.
        let mut values = run(0.5, 50);
        values.extend(run(0.3, 20));
        values.extend(run(0.5, 50));
        assert_eq!(
            chimes_at(&mut tracker, 0, &values, &dummy_hold()),
            Vec::<u64>::new()
        );
    }

    #[test]
    fn tracker_edge_dither_neither_misses_nor_rechimes() {
        let mut tracker = ToneTracker::default();
        let kind = dummy_hold();
        // Settle in the band and chime.
        assert_eq!(chimes_at(&mut tracker, 0, &[0.5; 150], &kind).len(), 1);
        // Dither just past the edge, inside the hysteresis: stays in band, no cue at all.
        for i in 0..200u64 {
            let value = if i % 2 == 0 { 0.549 } else { 0.552 };
            let step = tracker.step(150_000 + i * 1000, Some(0.5), value, 0.05, &kind);
            assert!(!step.chime && !step.miss, "sample {i}");
        }
    }

    #[test]
    fn tracker_miss_fires_once_on_leaving_after_settling() {
        let mut tracker = ToneTracker::default();
        // 200 ms in the band, then out for 300 ms: one miss, on the first sample past the limit.
        let mut values = run(0.5, 200);
        values.extend(run(0.6, 300));
        assert_eq!(
            misses_at(&mut tracker, 0, &values, &dummy_hold()),
            [200_000]
        );
    }

    #[test]
    fn tracker_miss_needs_the_hysteresis_limit() {
        let mut tracker = ToneTracker::default();
        let kind = dummy_hold();
        let mut values = run(0.5, 200);
        // 0.552 is past the band but inside the hysteresis limit (0.055).
        values.extend(run(0.552, 50));
        assert_eq!(
            misses_at(&mut tracker, 0, &values, &kind),
            Vec::<u64>::new()
        );
        let step = tracker.step(250_000, Some(0.5), 0.56, 0.05, &kind);
        assert!(step.miss);
    }

    #[test]
    fn tracker_no_miss_when_approaching_from_outside() {
        let mut tracker = ToneTracker::default();
        let mut values = run(0.2, 300);
        values.extend(run(0.5, 50));
        assert_eq!(
            misses_at(&mut tracker, 0, &values, &dummy_hold()),
            Vec::<u64>::new()
        );
    }

    #[test]
    fn tracker_no_miss_after_brief_brush() {
        let mut tracker = ToneTracker::default();
        // Out, 50 ms through the band, out again: not settled, so silent.
        let mut values = run(0.2, 50);
        values.extend(run(0.5, 50));
        values.extend(run(0.8, 200));
        assert_eq!(
            misses_at(&mut tracker, 0, &values, &dummy_hold()),
            Vec::<u64>::new()
        );
    }

    #[test]
    fn tracker_miss_cooldown() {
        let mut tracker = ToneTracker::default();
        let kind = dummy_hold();
        // Settle (0..200 ms), leave at 200 ms: miss.
        let mut values = run(0.5, 200);
        values.extend(run(0.7, 100));
        // Re-settle (300..500 ms) and leave at 500 ms, 300 ms after the last miss: suppressed.
        values.extend(run(0.5, 200));
        values.extend(run(0.7, 100));
        assert_eq!(misses_at(&mut tracker, 0, &values, &kind), [200_000]);
        // Re-settle and leave again at 1.2 s, 1 s after the first miss: allowed.
        let mut later = run(0.5, 600);
        later.extend(run(0.7, 100));
        assert_eq!(misses_at(&mut tracker, 600_000, &later, &kind), [1_200_000]);
    }

    #[test]
    fn tracker_miss_works_on_trace_drills() {
        let mut tracker = ToneTracker::default();
        let mut values = run(0.5, 200);
        values.extend(run(0.7, 50));
        let (chimes, misses) = cues(&mut tracker, 0, &values, &dummy_trace());
        assert_eq!(chimes, Vec::<u64>::new());
        assert_eq!(misses, [200_000]);
    }

    #[test]
    fn tracker_band_edge_is_inclusive_like_scoring() {
        // Exactly representable: |0.625 - 0.5| == 0.125.
        let mut tracker = ToneTracker::default();
        let step = tracker.step(0, Some(0.5), 0.625, 0.125, &dummy_hold());
        assert!(!step.miss);
        // Entered the band at 0 (inclusive edge), settled by 100 ms: leaving now misses.
        let step = tracker.step(CHIME_DWELL_US, Some(0.5), 0.9, 0.125, &dummy_hold());
        assert!(step.miss);
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
    fn tracker_state_resets_between_reps() {
        let mut tracker = ToneTracker::default();
        let kind = dummy_hold();
        // Rep 1: settle and miss.
        let mut values = run(0.5, 200);
        values.extend(run(0.7, 50));
        assert_eq!(misses_at(&mut tracker, 0, &values, &kind).len(), 1);
        // Between reps the cooldown and band state are forgotten.
        tracker.step(300_000, None, 0.5, 0.05, &kind);
        // Rep 2: out of band, settle from 450 ms, leave at 650 ms: only 400 ms after rep 1's
        // miss at 200 ms, yet it plays because the cooldown was reset.
        let mut values = run(0.7, 50);
        values.extend(run(0.5, 200));
        values.extend(run(0.7, 50));
        assert_eq!(misses_at(&mut tracker, 400_000, &values, &kind), [650_000]);
    }

    #[test]
    fn tracker_trace_never_chimes() {
        let mut tracker = ToneTracker::default();
        assert_eq!(
            chimes_at(&mut tracker, 0, &[0.5; 500], &dummy_trace()),
            Vec::<u64>::new()
        );
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
}
