//! Drill engine state machine driving practice sessions and scoring.
//!
//! The engine runs a sequence of repetitions for a [`Drill`], driven solely by sample
//! timestamps (`t_us`) and never by wall-clock time. Each rep consists of a lead-in / rest
//! countdown, an active practice window, and rep evaluation ([`score_hold`] or [`score_trace`]).
//!
//! A set summary ([`summarize_set`]) is produced upon set completion or early abort.

use serde::Serialize;

use crate::preset::{Drill, DrillKind};
use crate::scoring::{Grade, HoldParams, HoldScore, ValueSample, score_hold};
use crate::set_summary::{SetSummary, summarize_set};
use crate::trace_scoring::{TraceParams, TraceScore, score_trace};

/// Default rest pause in milliseconds between repetitions.
pub const DEFAULT_REST_MS: u32 = 2000;

/// Extra recording margin in milliseconds after a trace rep ends for reaction lag estimation.
pub const TRACE_LAG_MARGIN_MS: u32 = 300;

/// Evaluated repetition score for either a hold or trace drill.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RepScore {
    /// Score breakdown for a hold drill repetition.
    Hold(HoldScore),
    /// Score breakdown for a trace drill repetition.
    Trace(TraceScore),
}

impl RepScore {
    /// Returns the overall composite repetition score in `0.0..=100.0`.
    #[must_use]
    pub fn total(&self) -> f32 {
        match self {
            Self::Hold(score) => score.total,
            Self::Trace(score) => score.total,
        }
    }

    /// Returns the letter grade awarded for this repetition.
    #[must_use]
    pub fn grade(&self) -> Grade {
        match self {
            Self::Hold(score) => score.grade,
            Self::Trace(score) => score.grade,
        }
    }
}

/// Lifecycle phase of the drill engine state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "phase", rename_all = "camelCase")]
pub enum Phase {
    /// Engine initialized, awaiting [`DrillRun::start`].
    Idle,
    /// Preparation countdown before a repetition begins.
    #[serde(rename_all = "camelCase")]
    Countdown {
        /// Zero-based repetition index.
        rep: u32,
        /// Monotonic timestamp in microseconds when countdown expires.
        ends_us: u64,
    },
    /// Repetition actively underway.
    #[serde(rename_all = "camelCase")]
    Active {
        /// Zero-based repetition index.
        rep: u32,
        /// Monotonic timestamp in microseconds when active repetition started.
        start_us: u64,
        /// Monotonic timestamp in microseconds when active window closes.
        ends_us: u64,
    },
    /// Trace-only lag margin recording phase after active window ends.
    Scoring {
        /// Zero-based repetition index.
        rep: u32,
    },
    /// All repetitions finished and set completed.
    Finished,
    /// Drill set aborted early by caller.
    Aborted,
}

/// Events emitted during state machine transitions.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "camelCase")]
pub enum DrillEvent {
    /// A preparation countdown has begun for a repetition.
    #[serde(rename_all = "camelCase")]
    CountdownStarted {
        /// Zero-based repetition index.
        rep: u32,
        /// Monotonic timestamp in microseconds when countdown expires.
        ends_us: u64,
    },
    /// A repetition has transitioned from countdown to active.
    #[serde(rename_all = "camelCase")]
    RepStarted {
        /// Zero-based repetition index.
        rep: u32,
        /// Monotonic timestamp in microseconds when the rep started.
        start_us: u64,
    },
    /// A repetition completed and was successfully scored.
    RepScored {
        /// Zero-based repetition index.
        rep: u32,
        /// Score breakdown for the repetition.
        score: RepScore,
    },
    /// A repetition completed but scoring returned `None` (e.g. stalled input).
    RepFailed {
        /// Zero-based repetition index.
        rep: u32,
    },
    /// Drill set concluded and at least one repetition was scored.
    SetFinished {
        /// Performance summary across all scored repetitions.
        summary: SetSummary,
    },
}

/// State machine executing a multi-repetition pedal practice drill.
#[derive(Debug, Clone)]
pub struct DrillRun {
    drill: Drill,
    rest_ms: u32,
    phase: Phase,
    countdown_start_us: u64,
    current_rep_start_us: u64,
    current_rep_active_ends_us: u64,
    buffer: Vec<ValueSample>,
    results: Vec<Option<RepScore>>,
}

impl DrillRun {
    /// Creates a new drill run for the given [`Drill`] with specified rest duration between reps.
    #[must_use]
    pub fn new(drill: Drill, rest_ms: u32) -> Self {
        Self {
            drill,
            rest_ms,
            phase: Phase::Idle,
            countdown_start_us: 0,
            current_rep_start_us: 0,
            current_rep_active_ends_us: 0,
            buffer: Vec::new(),
            results: Vec::new(),
        }
    }

    /// Starts the drill run from monotonic microsecond timestamp `now_us`.
    ///
    /// Transitions from [`Phase::Idle`] to [`Phase::Countdown`] for rep 0 ending at
    /// `now_us + lead_in_ms * 1000`, emitting [`DrillEvent::CountdownStarted`]. Calling this
    /// in any other phase is a no-op and returns an empty event list.
    pub fn start(&mut self, now_us: u64) -> Vec<DrillEvent> {
        if self.phase != Phase::Idle {
            return Vec::new();
        }

        if self.drill.reps == 0 {
            self.phase = Phase::Finished;
            return Vec::new();
        }

        let lead_in_us = u64::from(self.drill.lead_in_ms) * 1_000;
        let ends_us = now_us.saturating_add(lead_in_us);

        self.countdown_start_us = now_us;
        self.phase = Phase::Countdown { rep: 0, ends_us };

        vec![DrillEvent::CountdownStarted { rep: 0, ends_us }]
    }

    /// Advances the state machine with a new timestamped pedal sample.
    ///
    /// If timestamps jump across multiple phase boundaries, transitions and events
    /// are evaluated sequentially in order until current phase catches up to `sample.t_us`.
    pub fn push(&mut self, sample: ValueSample) -> Vec<DrillEvent> {
        if matches!(self.phase, Phase::Idle | Phase::Finished | Phase::Aborted) {
            return Vec::new();
        }

        let mut events = Vec::new();

        loop {
            match self.phase {
                Phase::Idle | Phase::Finished | Phase::Aborted => {
                    break;
                }
                Phase::Countdown { rep, ends_us } => {
                    if sample.t_us < ends_us {
                        if matches!(self.drill.kind, DrillKind::Trace { .. }) {
                            let margin_us = u64::from(TRACE_LAG_MARGIN_MS) * 1_000;
                            if sample.t_us >= ends_us.saturating_sub(margin_us) {
                                self.buffer.push(sample);
                            }
                        }
                        break;
                    }

                    let d_ms = match &self.drill.kind {
                        DrillKind::Hold { hold_ms, .. } => *hold_ms,
                        DrillKind::Trace { points } => points.last().map_or(0, |p| p.0),
                    };
                    let duration_us = u64::from(d_ms) * 1_000;
                    let start_us = ends_us;
                    let active_ends_us = start_us.saturating_add(duration_us);

                    self.current_rep_start_us = start_us;
                    self.current_rep_active_ends_us = active_ends_us;
                    self.phase = Phase::Active {
                        rep,
                        start_us,
                        ends_us: active_ends_us,
                    };
                    events.push(DrillEvent::RepStarted { rep, start_us });
                }
                Phase::Active {
                    rep,
                    start_us,
                    ends_us,
                } => {
                    if sample.t_us < ends_us {
                        self.buffer.push(sample);
                        break;
                    }

                    match self.drill.kind {
                        DrillKind::Hold { .. } => {
                            self.score_rep_and_advance(rep, start_us, ends_us, &mut events);
                        }
                        DrillKind::Trace { .. } => {
                            self.phase = Phase::Scoring { rep };
                        }
                    }
                }
                Phase::Scoring { rep } => {
                    let margin_us = u64::from(TRACE_LAG_MARGIN_MS) * 1_000;
                    let scoring_ends_us = self.current_rep_active_ends_us.saturating_add(margin_us);

                    if sample.t_us <= scoring_ends_us {
                        self.buffer.push(sample);
                    }

                    if sample.t_us < scoring_ends_us {
                        break;
                    }

                    self.score_rep_and_advance(
                        rep,
                        self.current_rep_start_us,
                        scoring_ends_us,
                        &mut events,
                    );
                }
            }
        }

        events
    }

    /// Evaluates scoring for a completed rep, records the result, and transitions to
    /// either countdown for the next rep or set completion.
    fn score_rep_and_advance(
        &mut self,
        rep: u32,
        start_us: u64,
        boundary_time: u64,
        events: &mut Vec<DrillEvent>,
    ) {
        let rep_score = match &self.drill.kind {
            DrillKind::Hold { hold_ms, .. } => {
                let target = self.drill.target_fraction().unwrap_or(0.0);
                let tolerance = self.drill.tolerance_fraction();
                let params = HoldParams::new(target, tolerance, *hold_ms);
                score_hold(&self.buffer, start_us, &params).map(RepScore::Hold)
            }
            DrillKind::Trace { .. } => {
                let curve = self.drill.trace_curve();
                let tolerance = self.drill.tolerance_fraction();
                curve.and_then(|c| {
                    let params = TraceParams::new(&c, tolerance);
                    score_trace(&self.buffer, start_us, &params).map(RepScore::Trace)
                })
            }
        };

        if let Some(score) = &rep_score {
            events.push(DrillEvent::RepScored {
                rep,
                score: score.clone(),
            });
        } else {
            events.push(DrillEvent::RepFailed { rep });
        }

        self.results.push(rep_score);
        self.buffer.clear();

        let next_rep = rep.saturating_add(1);
        if next_rep < self.drill.reps {
            let rest_us = u64::from(self.rest_ms) * 1_000;
            let next_ends_us = boundary_time.saturating_add(rest_us);

            self.countdown_start_us = boundary_time;
            self.phase = Phase::Countdown {
                rep: next_rep,
                ends_us: next_ends_us,
            };
            events.push(DrillEvent::CountdownStarted {
                rep: next_rep,
                ends_us: next_ends_us,
            });
        } else {
            self.phase = Phase::Finished;
            let scored_totals: Vec<f32> = self
                .results
                .iter()
                .filter_map(|r| r.as_ref().map(RepScore::total))
                .collect();
            if let Some(summary) = summarize_set(&scored_totals) {
                events.push(DrillEvent::SetFinished { summary });
            }
        }
    }

    /// Aborts the drill set early, transitioning to [`Phase::Aborted`].
    ///
    /// Returns a [`SetSummary`] summarizing all repetitions scored before the abort,
    /// or `None` if zero repetitions were completed. Further calls to [`Self::push`]
    /// become no-ops.
    pub fn abort(&mut self) -> Option<SetSummary> {
        self.phase = Phase::Aborted;
        self.buffer.clear();
        let scored_totals: Vec<f32> = self
            .results
            .iter()
            .filter_map(|r| r.as_ref().map(RepScore::total))
            .collect();
        summarize_set(&scored_totals)
    }

    /// Current lifecycle phase of the drill engine.
    #[must_use]
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Repetition results in order of completion, with `None` representing failed repetitions.
    #[must_use]
    pub fn results(&self) -> &[Option<RepScore>] {
        &self.results
    }

    /// Reference to the practice drill configuration.
    #[must_use]
    pub fn drill(&self) -> &Drill {
        &self.drill
    }

    /// Target pedal position fraction in `0.0..=1.0` at timestamp `t_us` during active or
    /// scoring phases, or `None` during countdown, idle, finished, or aborted states.
    #[must_use]
    pub fn target_at(&self, t_us: u64) -> Option<f32> {
        match self.phase {
            Phase::Active { .. } | Phase::Scoring { .. } => match &self.drill.kind {
                DrillKind::Hold { .. } => self.drill.target_fraction(),
                DrillKind::Trace { .. } => {
                    let curve = self.drill.trace_curve()?;
                    let elapsed_us = t_us.saturating_sub(self.current_rep_start_us);
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "elapsed time in microseconds fits within f64"
                    )]
                    let dt_ms = (elapsed_us as f64) / 1000.0;
                    Some(curve.value_at(dt_ms))
                }
            },
            Phase::Idle | Phase::Countdown { .. } | Phase::Finished | Phase::Aborted => None,
        }
    }

    /// Normalized progress fraction in `0.0..=1.0` through the current countdown or active phase,
    /// or `None` during idle, scoring, finished, or aborted states.
    #[must_use]
    pub fn progress(&self, t_us: u64) -> Option<f32> {
        match self.phase {
            Phase::Countdown { ends_us, .. } => {
                if ends_us <= self.countdown_start_us {
                    return Some(1.0);
                }
                let total = ends_us - self.countdown_start_us;
                let elapsed = t_us.saturating_sub(self.countdown_start_us);
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "countdown duration in microseconds fits within f64"
                )]
                let frac = (elapsed as f64) / (total as f64);
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "progress fraction fits within f32"
                )]
                Some((frac as f32).clamp(0.0, 1.0))
            }
            Phase::Active {
                start_us, ends_us, ..
            } => {
                if ends_us <= start_us {
                    return Some(1.0);
                }
                let total = ends_us - start_us;
                let elapsed = t_us.saturating_sub(start_us);
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "active duration in microseconds fits within f64"
                )]
                let frac = (elapsed as f64) / (total as f64);
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "progress fraction fits within f32"
                )]
                Some((frac as f32).clamp(0.0, 1.0))
            }
            Phase::Idle | Phase::Scoring { .. } | Phase::Finished | Phase::Aborted => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::{Pedal, parse_preset};

    fn make_hold_drill(reps: u32, lead_in_ms: u32, hold_ms: u32, target: f32) -> Drill {
        Drill {
            id: "hold-drill".to_string(),
            name: "Hold Drill".to_string(),
            pedal: Pedal::Brake,
            reps,
            lead_in_ms,
            tolerance: 5.0,
            kind: DrillKind::Hold { target, hold_ms },
        }
    }

    fn make_trace_drill(reps: u32, lead_in_ms: u32, points: Vec<(u32, f32)>) -> Drill {
        Drill {
            id: "trace-drill".to_string(),
            name: "Trace Drill".to_string(),
            pedal: Pedal::Brake,
            reps,
            lead_in_ms,
            tolerance: 6.0,
            kind: DrillKind::Trace { points },
        }
    }

    fn feed_1khz<F>(
        run: &mut DrillRun,
        start_t_us: u64,
        end_t_us: u64,
        mut value_fn: F,
    ) -> Vec<DrillEvent>
    where
        F: FnMut(u64) -> f32,
    {
        let mut all_events = Vec::new();
        let mut t = start_t_us;
        while t <= end_t_us {
            let val = value_fn(t);
            let events = run.push(ValueSample::new(t, val));
            all_events.extend(events);
            t = t.saturating_add(1_000);
        }
        all_events
    }

    #[test]
    fn test_1_perfect_hold_two_reps() {
        let drill = make_hold_drill(2, 1000, 500, 70.0);
        let mut run = DrillRun::new(drill, 300);

        let mut events = run.start(0);
        assert_eq!(
            events,
            vec![DrillEvent::CountdownStarted {
                rep: 0,
                ends_us: 1_000_000
            }]
        );

        let pushed_events = feed_1khz(&mut run, 0, 2_300_000, |_t| 0.70);
        events.extend(pushed_events);

        assert_eq!(run.phase(), Phase::Finished);
        assert_eq!(run.results().len(), 2);

        assert_eq!(
            events[0],
            DrillEvent::CountdownStarted {
                rep: 0,
                ends_us: 1_000_000
            }
        );
        assert_eq!(
            events[1],
            DrillEvent::RepStarted {
                rep: 0,
                start_us: 1_000_000
            }
        );

        match &events[2] {
            DrillEvent::RepScored { rep, score } => {
                assert_eq!(*rep, 0);
                assert!(score.total() >= 98.0, "rep 0 total was {}", score.total());
            }
            other => panic!("expected RepScored(0), got {other:?}"),
        }

        assert_eq!(
            events[3],
            DrillEvent::CountdownStarted {
                rep: 1,
                ends_us: 1_800_000
            }
        );
        assert_eq!(
            events[4],
            DrillEvent::RepStarted {
                rep: 1,
                start_us: 1_800_000
            }
        );

        match &events[5] {
            DrillEvent::RepScored { rep, score } => {
                assert_eq!(*rep, 1);
                assert!(score.total() >= 98.0, "rep 1 total was {}", score.total());
            }
            other => panic!("expected RepScored(1), got {other:?}"),
        }

        match &events[6] {
            DrillEvent::SetFinished { summary } => {
                assert_eq!(summary.rep_totals.len(), 2);
                assert!(summary.average >= 98.0);
            }
            other => panic!("expected SetFinished, got {other:?}"),
        }

        assert_eq!(events.len(), 7);
    }

    #[test]
    fn test_2_rep_start_time_from_sample_timestamps() {
        let drill = make_hold_drill(1, 1000, 500, 70.0);
        let mut run = DrillRun::new(drill, 300);

        let start_events = run.start(5_000_000);
        assert_eq!(
            start_events,
            vec![DrillEvent::CountdownStarted {
                rep: 0,
                ends_us: 6_000_000
            }]
        );

        let push_events = run.push(ValueSample::new(6_000_000, 0.70));
        assert_eq!(
            push_events,
            vec![DrillEvent::RepStarted {
                rep: 0,
                start_us: 6_000_000
            }]
        );
    }

    #[test]
    fn test_3_timestamp_jump_fails_reps() {
        let drill = make_hold_drill(2, 1000, 500, 70.0);
        let mut run = DrillRun::new(drill, 300);

        let start_events = run.start(0);
        assert_eq!(
            start_events,
            vec![DrillEvent::CountdownStarted {
                rep: 0,
                ends_us: 1_000_000
            }]
        );

        // Jump far past rep 0 and rep 1 boundaries
        let jump_events = run.push(ValueSample::new(50_000_000, 0.70));
        assert_eq!(
            jump_events,
            vec![
                DrillEvent::RepStarted {
                    rep: 0,
                    start_us: 1_000_000
                },
                DrillEvent::RepFailed { rep: 0 },
                DrillEvent::CountdownStarted {
                    rep: 1,
                    ends_us: 1_800_000
                },
                DrillEvent::RepStarted {
                    rep: 1,
                    start_us: 1_800_000
                },
                DrillEvent::RepFailed { rep: 1 },
            ]
        );

        assert_eq!(run.phase(), Phase::Finished);
        assert_eq!(run.results(), &[None, None]);
    }

    #[test]
    fn test_4_abort_in_middle_of_rep_one() {
        let drill = make_hold_drill(2, 1000, 500, 70.0);
        let mut run = DrillRun::new(drill, 300);

        run.start(0);
        // Feed through rep 0 completion (1_500_000) and into rep 1 countdown
        feed_1khz(&mut run, 0, 1_600_000, |_t| 0.70);

        let summary = run.abort().expect("rep 0 scored so summary exists");
        assert_eq!(summary.rep_totals.len(), 1);
        assert!(summary.rep_totals[0] >= 98.0);
        assert_eq!(run.phase(), Phase::Aborted);

        // Pushing after abort is a no-op
        let events = run.push(ValueSample::new(1_700_000, 0.70));
        assert_eq!(events, []);
        assert_eq!(run.phase(), Phase::Aborted);
    }

    #[test]
    fn test_5_exact_copy_trace_drill() {
        let points = vec![(0, 0.0), (150, 90.0), (600, 0.0)];
        let drill = make_trace_drill(1, 1000, points);
        let curve = drill.trace_curve().unwrap();
        let mut run = DrillRun::new(drill, 300);

        run.start(0);

        // Feed countdown and active up to 1_600_000
        feed_1khz(&mut run, 0, 1_600_000, |t| {
            if t < 1_000_000 {
                0.0
            } else {
                #[expect(clippy::cast_precision_loss, reason = "dt fits within f64")]
                let ms = ((t - 1_000_000) as f64) / 1000.0;
                curve.value_at(ms)
            }
        });

        // Between ends_us (1_600_000) and ends_us + 300_000 (1_900_000), phase is Scoring
        assert_eq!(run.phase(), Phase::Scoring { rep: 0 });

        feed_1khz(&mut run, 1_601_000, 1_899_000, |_t| 0.0);
        assert_eq!(run.phase(), Phase::Scoring { rep: 0 });

        // Feeding up to 1_900_000 finishes scoring
        let final_events = run.push(ValueSample::new(1_900_000, 0.0));
        assert_eq!(run.phase(), Phase::Finished);

        match &final_events[0] {
            DrillEvent::RepScored { rep, score } => {
                assert_eq!(*rep, 0);
                assert!(score.total() >= 95.0, "total was {}", score.total());
            }
            other => panic!("expected RepScored, got {other:?}"),
        }
        assert!(matches!(final_events[1], DrillEvent::SetFinished { .. }));
    }

    #[test]
    fn test_6_delayed_trace_scores_lower_with_lag() {
        let points = vec![(0, 0.0), (150, 90.0), (600, 0.0)];
        let drill = make_trace_drill(1, 1000, points.clone());
        let curve = drill.trace_curve().unwrap();

        // Run exact copy (test 5 reference)
        let mut exact_run = DrillRun::new(drill.clone(), 300);
        exact_run.start(0);
        feed_1khz(&mut exact_run, 0, 1_900_000, |t| {
            if t < 1_000_000 {
                0.0
            } else {
                #[expect(clippy::cast_precision_loss, reason = "dt fits within f64")]
                let ms = ((t - 1_000_000) as f64) / 1000.0;
                curve.value_at(ms)
            }
        });
        let exact_score = exact_run.results()[0].as_ref().unwrap().clone();

        // Run delayed by 100 ms
        let mut delayed_run = DrillRun::new(drill, 300);
        delayed_run.start(0);
        feed_1khz(&mut delayed_run, 0, 1_900_000, |t| {
            if t < 1_000_000 {
                0.0
            } else {
                #[expect(clippy::cast_precision_loss, reason = "dt fits within f64")]
                let ms = ((t - 1_000_000) as f64) / 1000.0 - 100.0;
                curve.value_at(ms)
            }
        });

        let delayed_score = match delayed_run.results()[0].as_ref().unwrap() {
            RepScore::Trace(score) => score,
            RepScore::Hold(_) => unreachable!(),
        };

        assert!(
            (delayed_score.lag_ms - 100.0).abs() <= 5.0,
            "lag_ms was {}",
            delayed_score.lag_ms
        );
        assert!(
            delayed_score.total < exact_score.total(),
            "delayed total {} should be lower than exact {}",
            delayed_score.total,
            exact_score.total()
        );
    }

    #[test]
    fn test_7_target_at_hold_and_trace() {
        // Hold drill
        let hold_drill = make_hold_drill(1, 1000, 500, 70.0);
        let mut hold_run = DrillRun::new(hold_drill, 300);
        hold_run.start(0);

        assert_eq!(hold_run.target_at(500_000), None);

        hold_run.push(ValueSample::new(1_000_000, 0.70));
        assert_eq!(
            hold_run.phase(),
            Phase::Active {
                rep: 0,
                start_us: 1_000_000,
                ends_us: 1_500_000
            }
        );
        assert_eq!(hold_run.target_at(1_200_000), Some(0.70));

        // Trace drill
        let trace_drill = make_trace_drill(1, 1000, vec![(0, 0.0), (150, 90.0), (600, 0.0)]);
        let mut trace_run = DrillRun::new(trace_drill, 300);
        trace_run.start(0);

        assert_eq!(trace_run.target_at(500_000), None);

        trace_run.push(ValueSample::new(1_000_000, 0.0));
        assert_eq!(
            trace_run.phase(),
            Phase::Active {
                rep: 0,
                start_us: 1_000_000,
                ends_us: 1_600_000
            }
        );

        let target_at_peak = trace_run.target_at(1_150_000).unwrap();
        assert!((target_at_peak - 0.90).abs() < 1e-4);

        let target_at_mid = trace_run.target_at(1_075_000).unwrap();
        assert!((target_at_mid - 0.45).abs() < 1e-4);
    }

    #[test]
    fn test_8_progress_across_countdown() {
        let drill = make_hold_drill(1, 1000, 500, 70.0);
        let mut run = DrillRun::new(drill, 300);
        run.start(1_000_000);

        assert_eq!(run.progress(1_000_000), Some(0.0));
        assert_eq!(run.progress(1_500_000), Some(0.5));
        assert_eq!(run.progress(2_000_000), Some(1.0));
        assert_eq!(run.progress(500_000), Some(0.0));
        assert_eq!(run.progress(2_500_000), Some(1.0));
    }

    #[test]
    fn test_9_memory_bound_reps_50() {
        let hold_ms = 500;
        let drill = make_hold_drill(50, 100, hold_ms, 70.0);
        let mut run = DrillRun::new(drill, 100);
        run.start(0);

        let mut t = 0_u64;
        let total_duration_us = 50 * (100 + 500 + 100) * 1_000;

        while t <= total_duration_us && run.phase() != Phase::Finished {
            run.push(ValueSample::new(t, 0.70));
            assert!(
                run.buffer.len() <= (hold_ms as usize) + 400,
                "buffer length {} exceeded limit at t={t}",
                run.buffer.len()
            );
            t = t.saturating_add(1_000);
        }

        assert_eq!(run.phase(), Phase::Finished);
        assert_eq!(run.results().len(), 50);
    }

    #[test]
    fn test_10_start_twice_and_push_before_start() {
        let drill = make_hold_drill(1, 1000, 500, 70.0);
        let mut run = DrillRun::new(drill, 300);

        // Push before start is a no-op
        let events = run.push(ValueSample::new(500, 0.5));
        assert_eq!(events, []);
        assert_eq!(run.phase(), Phase::Idle);

        // First start succeeds
        let start1 = run.start(1_000);
        assert_eq!(start1.len(), 1);
        assert!(matches!(run.phase(), Phase::Countdown { .. }));

        // Second start is a no-op
        let start2 = run.start(2_000);
        assert_eq!(start2, []);
        assert_eq!(
            run.phase(),
            Phase::Countdown {
                rep: 0,
                ends_us: 1_001_000
            }
        );
    }

    #[test]
    fn test_11_serialization_camel_case_and_tags() {
        let hold_score = HoldScore {
            total: 96.0,
            grade: Grade::S,
            accuracy: 95.0,
            timing: 98.0,
            smoothness: 95.0,
            time_in_band: 1.0,
            rmse: 0.01,
            time_to_band_ms: Some(100.0),
            overshoot: 0.0,
            jitter: 0.001,
        };
        let rep_score = RepScore::Hold(hold_score);
        let json_score = serde_json::to_value(&rep_score).unwrap();
        assert_eq!(json_score["kind"], "hold");
        assert_eq!(json_score["total"], 96.0);
        assert_eq!(json_score["timeInBand"], 1.0);

        let event1 = DrillEvent::CountdownStarted {
            rep: 0,
            ends_us: 5_000,
        };
        let json_event1 = serde_json::to_value(&event1).unwrap();
        assert_eq!(json_event1["event"], "countdownStarted");
        assert_eq!(json_event1["rep"], 0);
        assert_eq!(json_event1["endsUs"], 5_000);

        let event2 = DrillEvent::RepStarted {
            rep: 1,
            start_us: 10_000,
        };
        let json_event2 = serde_json::to_value(&event2).unwrap();
        assert_eq!(json_event2["event"], "repStarted");
        assert_eq!(json_event2["startUs"], 10_000);

        let event3 = DrillEvent::RepScored {
            rep: 0,
            score: rep_score,
        };
        let json_event3 = serde_json::to_value(&event3).unwrap();
        assert_eq!(json_event3["event"], "repScored");
        assert_eq!(json_event3["score"]["kind"], "hold");

        let event4 = DrillEvent::RepFailed { rep: 2 };
        let json_event4 = serde_json::to_value(&event4).unwrap();
        assert_eq!(json_event4["event"], "repFailed");
        assert_eq!(json_event4["rep"], 2);

        let summary = summarize_set(&[90.0, 95.0]).unwrap();
        let event5 = DrillEvent::SetFinished { summary };
        let json_event5 = serde_json::to_value(&event5).unwrap();
        assert_eq!(json_event5["event"], "setFinished");
        assert!(json_event5["summary"]["repTotals"].is_array());

        let phase = Phase::Active {
            rep: 0,
            start_us: 1000,
            ends_us: 2000,
        };
        let json_phase = serde_json::to_value(phase).unwrap();
        assert_eq!(json_phase["phase"], "active");
        assert_eq!(json_phase["startUs"], 1000);
        assert_eq!(json_phase["endsUs"], 2000);
    }

    #[test]
    fn test_parse_preset_integration() {
        let json = r#"{
            "schemaVersion": 1,
            "id": "threshold-drill",
            "name": "Threshold Practice",
            "drills": [
                {
                    "id": "hold-80",
                    "name": "Hold 80%",
                    "type": "hold",
                    "pedal": "brake",
                    "reps": 1,
                    "leadInMs": 1000,
                    "tolerance": 5,
                    "target": 80,
                    "holdMs": 1000
                }
            ]
        }"#;
        let preset = parse_preset(json).expect("preset should parse cleanly");
        let drill = preset.drills[0].clone();
        let mut run = DrillRun::new(drill, DEFAULT_REST_MS);
        assert_eq!(run.drill().id, "hold-80");

        run.start(0);
        feed_1khz(&mut run, 0, 2_000_000, |_t| 0.80);
        assert_eq!(run.phase(), Phase::Finished);
        assert!(run.results()[0].as_ref().unwrap().total() >= 98.0);
    }
}
