//! Drill engine state machine driving practice sessions and scoring.
//!
//! The engine runs a sequence of repetitions for a [`Drill`], driven solely by sample
//! timestamps (`t_us`) and never by wall-clock time. Each rep consists of a lead-in / rest
//! countdown, an active practice window, and rep evaluation ([`score_hold`] or [`score_trace`]).
//!
//! A set summary ([`summarize_set`]) is produced upon set completion or early abort.
//!
//! A brake drill with a throttle lead-in (SCT-037) adds two phases between the countdown and
//! the rep: [`Phase::ThrottleWait`] until the throttle reaches the level, then
//! [`Phase::ThrottleHold`] for the hold time. The rep start is the cue to lift. Each rep of
//! such a drill reports an [`Overlap`] of the two pedals.

use serde::Serialize;

use crate::preset::{Drill, DrillKind, EnvelopeTable, TraceCurve};
use crate::scoring::{Grade, HoldParams, HoldScore, ValueSample, score_hold};
use crate::set_summary::{SetSummary, summarize_set};
use crate::trace_scoring::{RAMP_WINDOW_MS, TraceParams, TraceScore, score_trace};

/// Default rest pause in milliseconds between repetitions.
pub const DEFAULT_REST_MS: u32 = 2000;

/// Extra recording margin in milliseconds after a trace rep ends for reaction lag estimation.
pub const TRACE_LAG_MARGIN_MS: u32 = 300;

/// The throttle arms the hold at `level - THROTTLE_ARM_MARGIN` or above.
pub const THROTTLE_ARM_MARGIN: f32 = 0.10;
/// A started hold restarts only when the throttle falls below `level - THROTTLE_DROP_MARGIN`.
/// The gap to [`THROTTLE_ARM_MARGIN`] keeps a throttle resting on the arm edge from restarting
/// the hold on every sample.
pub const THROTTLE_DROP_MARGIN: f32 = 0.15;
/// A pedal counts as pressed for overlap above this fraction.
pub const OVERLAP_THRESHOLD: f32 = 0.05;
/// Longest gap between two samples counted toward overlap, so a stalled stream doesn't inflate it.
const MAX_OVERLAP_GAP_US: u64 = 100_000;

/// Pedal overlap during one rep of a throttle lead-in drill, measured over the throttle hold
/// and the active window.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overlap {
    /// Total time both pedals were above [`OVERLAP_THRESHOLD`], in ms.
    pub overlap_ms: f32,
    /// Highest throttle (fraction 0..=1) seen while both pedals were above
    /// [`OVERLAP_THRESHOLD`]; 0 if none.
    pub peak_throttle: f32,
}

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
    /// Lead-in drills only: countdown done, waiting for the throttle to reach the level.
    ///
    /// There is no timeout: the engine waits rather than failing the rep, and the user can
    /// abort.
    ThrottleWait {
        /// Zero-based repetition index.
        rep: u32,
    },
    /// Lead-in drills only: throttle held; the rep starts (lift cue) at `ends_us`.
    #[serde(rename_all = "camelCase")]
    ThrottleHold {
        /// Zero-based repetition index.
        rep: u32,
        /// Monotonic timestamp in microseconds when the throttle reached the level.
        start_us: u64,
        /// Monotonic timestamp in microseconds of the lift cue (the rep start).
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
        /// Monotonic timestamp in microseconds when countdown began.
        start_us: u64,
        /// Monotonic timestamp in microseconds when countdown expires.
        ends_us: u64,
    },
    /// Lead-in drills: the engine waits for the throttle (after the countdown, or after it dropped).
    #[serde(rename_all = "camelCase")]
    ThrottleWait {
        /// Zero-based repetition index.
        rep: u32,
        /// Monotonic timestamp in microseconds since when the engine waits.
        since_us: u64,
    },
    /// Lead-in drills: the throttle reached the level; the lift cue comes at `ends_us`.
    #[serde(rename_all = "camelCase")]
    ThrottleHoldStarted {
        /// Zero-based repetition index.
        rep: u32,
        /// Monotonic timestamp in microseconds when the throttle reached the level.
        start_us: u64,
        /// Monotonic timestamp in microseconds of the lift cue (the rep start).
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
        /// Pedal overlap of the rep; `None` for drills without a throttle lead-in.
        #[serde(skip_serializing_if = "Option::is_none")]
        overlap: Option<Overlap>,
    },
    /// A repetition completed but scoring returned `None` (e.g. stalled input).
    RepFailed {
        /// Zero-based repetition index.
        rep: u32,
        /// Pedal overlap of the rep; `None` for drills without a throttle lead-in.
        #[serde(skip_serializing_if = "Option::is_none")]
        overlap: Option<Overlap>,
    },
    /// Drill set concluded (all repetitions completed).
    #[serde(rename_all = "camelCase")]
    SetFinished {
        /// Performance summary across all scored repetitions, or `None` if zero repetitions were scored.
        summary: Option<SetSummary>,
    },
}

/// State machine executing a multi-repetition pedal practice drill.
#[derive(Debug, Clone)]
pub struct DrillRun {
    drill: Drill,
    curve: Option<TraceCurve>,
    /// Envelope of `curve` within ±[`RAMP_WINDOW_MS`], so [`Self::band_at`] is a lookup.
    band: Option<EnvelopeTable>,
    rest_ms: u32,
    phase: Phase,
    countdown_start_us: u64,
    current_rep_start_us: u64,
    current_rep_active_ends_us: u64,
    buffer: Vec<ValueSample>,
    results: Vec<Option<RepScore>>,
    /// Throttle level fraction of the lead-in; `None` for drills without one.
    throttle_level: Option<f32>,
    /// Throttle hold of the lead-in in microseconds.
    throttle_hold_us: u64,
    /// Overlap time accumulated in the current rep, in microseconds.
    overlap_us: u64,
    /// Highest throttle seen with both pedals pressed in the current rep.
    overlap_peak: f32,
    /// Previous sample of the overlap window: `(t_us, brake, throttle)`.
    overlap_prev: Option<(u64, f32, f32)>,
}

impl DrillRun {
    /// Creates a new drill run for the given [`Drill`] with specified rest duration between reps.
    ///
    /// For trace drills, `rest_ms` should be at least [`TRACE_LAG_MARGIN_MS`], because the
    /// lag pre-roll is collected during the rest countdown.
    #[must_use]
    pub fn new(drill: Drill, rest_ms: u32) -> Self {
        let curve = drill.trace_curve();
        let band = curve.as_ref().map(|c| c.envelope_table(RAMP_WINDOW_MS));
        let throttle_level = drill.throttle_level_fraction();
        let throttle_hold_us = drill
            .throttle_lead_in
            .as_ref()
            .map_or(0, |l| u64::from(l.hold_ms) * 1_000);
        Self {
            drill,
            curve,
            band,
            rest_ms,
            phase: Phase::Idle,
            countdown_start_us: 0,
            current_rep_start_us: 0,
            current_rep_active_ends_us: 0,
            buffer: Vec::new(),
            results: Vec::new(),
            throttle_level,
            throttle_hold_us,
            overlap_us: 0,
            overlap_peak: 0.0,
            overlap_prev: None,
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

        vec![DrillEvent::CountdownStarted {
            rep: 0,
            start_us: now_us,
            ends_us,
        }]
    }

    /// Advances the state machine with a new timestamped pedal sample, with the throttle released.
    ///
    /// Same as [`Self::push_pedals`] with a throttle of `0.0`. On a drill with a throttle
    /// lead-in the run therefore never leaves [`Phase::ThrottleWait`]; callers must use
    /// [`Self::push_pedals`] for such drills.
    pub fn push(&mut self, sample: ValueSample) -> Vec<DrillEvent> {
        self.push_pedals(sample, 0.0)
    }

    /// Advances the state machine with a new timestamped sample of the drill pedal and the
    /// throttle fraction at the same `t_us`.
    ///
    /// The throttle only matters for drills with a throttle lead-in; other drills ignore it.
    /// If timestamps jump across multiple phase boundaries, transitions and events
    /// are evaluated sequentially in order until current phase catches up to `sample.t_us`.
    pub fn push_pedals(&mut self, sample: ValueSample, throttle: f32) -> Vec<DrillEvent> {
        if matches!(self.phase, Phase::Idle | Phase::Finished | Phase::Aborted) {
            return Vec::new();
        }

        let mut events = Vec::new();
        let is_trace = matches!(self.drill.kind, DrillKind::Trace { .. });
        let margin_us = u64::from(TRACE_LAG_MARGIN_MS) * 1_000;

        loop {
            match self.phase {
                Phase::Idle | Phase::Finished | Phase::Aborted => {
                    break;
                }
                Phase::Countdown { rep, ends_us } => {
                    if sample.t_us < ends_us {
                        if is_trace
                            && self.throttle_level.is_none()
                            && sample.t_us >= ends_us.saturating_sub(margin_us)
                        {
                            self.buffer.push(sample);
                        }
                        break;
                    }

                    if let Some(level) = self.throttle_level {
                        self.leave_countdown_to_throttle(
                            rep,
                            ends_us,
                            sample.t_us,
                            level,
                            throttle,
                            &mut events,
                        );
                    } else {
                        self.enter_active(rep, ends_us, &mut events);
                    }
                }
                Phase::ThrottleWait { rep } => {
                    let level = self.throttle_level.unwrap_or(0.0);
                    if throttle < level - THROTTLE_ARM_MARGIN {
                        break;
                    }
                    self.enter_throttle_hold(rep, sample.t_us, &mut events);
                }
                Phase::ThrottleHold { rep, ends_us, .. } => {
                    if sample.t_us >= ends_us {
                        // The rep starts at the cue, not at this sample's time.
                        self.enter_active(rep, ends_us, &mut events);
                        continue;
                    }

                    self.step_throttle_hold(rep, ends_us, sample, throttle, &mut events);
                    break;
                }
                Phase::Active {
                    rep,
                    start_us,
                    ends_us,
                } => {
                    if self.throttle_level.is_some() {
                        // The boundary sample (t >= ends_us) closes the last gap at ends_us.
                        self.accumulate_overlap(sample, throttle, ends_us);
                    }
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

    /// Starts the active window of `rep` at `start_us` and emits [`DrillEvent::RepStarted`].
    fn enter_active(&mut self, rep: u32, start_us: u64, events: &mut Vec<DrillEvent>) {
        let d_ms = match &self.drill.kind {
            DrillKind::Hold { hold_ms, .. } => *hold_ms,
            DrillKind::Trace { points } => points.last().map_or(0, |p| p.0),
        };
        let duration_us = u64::from(d_ms) * 1_000;
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

    /// Ends the countdown of a lead-in drill at `ends_us`: straight into the throttle hold if
    /// the boundary sample's throttle is at the level, else into [`Phase::ThrottleWait`].
    ///
    /// The hold starts at the boundary sample's time `sample_t_us`, not at `ends_us`, so a late
    /// sample after a stream stall cannot pass a hold the throttle was never seen holding.
    fn leave_countdown_to_throttle(
        &mut self,
        rep: u32,
        ends_us: u64,
        sample_t_us: u64,
        level: f32,
        throttle: f32,
        events: &mut Vec<DrillEvent>,
    ) {
        if throttle >= level - THROTTLE_ARM_MARGIN {
            self.enter_throttle_hold(rep, sample_t_us, events);
        } else {
            self.phase = Phase::ThrottleWait { rep };
            events.push(DrillEvent::ThrottleWait {
                rep,
                since_us: ends_us,
            });
        }
    }

    /// Handles a sample before the lift cue at `ends_us`. A throttle below
    /// `level - THROTTLE_DROP_MARGIN` sends the rep back to [`Phase::ThrottleWait`] and drops
    /// the hold's pre-roll and overlap. Otherwise
    /// the sample counts toward overlap and, for trace drills in the last
    /// [`TRACE_LAG_MARGIN_MS`], is buffered as the lag pre-roll (the role the countdown pre-roll
    /// plays without a lead-in).
    fn step_throttle_hold(
        &mut self,
        rep: u32,
        ends_us: u64,
        sample: ValueSample,
        throttle: f32,
        events: &mut Vec<DrillEvent>,
    ) {
        let level = self.throttle_level.unwrap_or(0.0);
        if throttle < level - THROTTLE_DROP_MARGIN {
            self.phase = Phase::ThrottleWait { rep };
            self.buffer.clear();
            self.reset_overlap();
            events.push(DrillEvent::ThrottleWait {
                rep,
                since_us: sample.t_us,
            });
            return;
        }

        self.accumulate_overlap(sample, throttle, ends_us);
        let margin_us = u64::from(TRACE_LAG_MARGIN_MS) * 1_000;
        if matches!(self.drill.kind, DrillKind::Trace { .. })
            && sample.t_us >= ends_us.saturating_sub(margin_us)
        {
            self.buffer.push(sample);
        }
    }

    /// Starts the throttle hold of `rep` at `start_us` with a fresh overlap window and emits
    /// [`DrillEvent::ThrottleHoldStarted`].
    fn enter_throttle_hold(&mut self, rep: u32, start_us: u64, events: &mut Vec<DrillEvent>) {
        let ends_us = start_us.saturating_add(self.throttle_hold_us);
        self.buffer.clear();
        self.reset_overlap();
        self.phase = Phase::ThrottleHold {
            rep,
            start_us,
            ends_us,
        };
        events.push(DrillEvent::ThrottleHoldStarted {
            rep,
            start_us,
            ends_us,
        });
    }

    fn reset_overlap(&mut self) {
        self.overlap_us = 0;
        self.overlap_peak = 0.0;
        self.overlap_prev = None;
    }

    /// Adds one sample of the overlap window (left-rectangle rule: a sample holds until the
    /// next one, each gap capped at [`MAX_OVERLAP_GAP_US`] and at `window_end_us`). A sample at
    /// or past `window_end_us` only closes the previous gap and is not a peak. O(1), no
    /// allocation.
    fn accumulate_overlap(&mut self, sample: ValueSample, throttle: f32, window_end_us: u64) {
        let brake = sample.value;
        if let Some((prev_t_us, prev_brake, prev_throttle)) = self.overlap_prev
            && prev_brake > OVERLAP_THRESHOLD
            && prev_throttle > OVERLAP_THRESHOLD
        {
            let gap = sample
                .t_us
                .min(window_end_us)
                .saturating_sub(prev_t_us)
                .min(MAX_OVERLAP_GAP_US);
            self.overlap_us = self.overlap_us.saturating_add(gap);
        }
        if sample.t_us < window_end_us && brake > OVERLAP_THRESHOLD && throttle > OVERLAP_THRESHOLD
        {
            self.overlap_peak = self.overlap_peak.max(throttle);
        }
        self.overlap_prev = Some((sample.t_us, brake, throttle));
    }

    /// The overlap of the current rep for lead-in drills, `None` otherwise.
    fn current_overlap(&self) -> Option<Overlap> {
        self.throttle_level?;
        #[expect(
            clippy::cast_precision_loss,
            reason = "overlap time in microseconds fits within f64"
        )]
        let ms = (self.overlap_us as f64) / 1000.0;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "overlap time in ms fits within f32"
        )]
        let overlap_ms = ms as f32;
        Some(Overlap {
            overlap_ms,
            peak_throttle: self.overlap_peak,
        })
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
                let tolerance = self.drill.tolerance_fraction();
                self.curve.as_ref().and_then(|curve| {
                    let params = TraceParams::new(curve, tolerance);
                    score_trace(&self.buffer, start_us, &params).map(RepScore::Trace)
                })
            }
        };

        let overlap = self.current_overlap();
        if let Some(score) = &rep_score {
            events.push(DrillEvent::RepScored {
                rep,
                score: score.clone(),
                overlap,
            });
        } else {
            events.push(DrillEvent::RepFailed { rep, overlap });
        }

        self.results.push(rep_score);
        self.buffer.clear();
        self.reset_overlap();

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
                start_us: boundary_time,
                ends_us: next_ends_us,
            });
        } else {
            self.phase = Phase::Finished;
            let scored_totals: Vec<f32> = self
                .results
                .iter()
                .filter_map(|r| r.as_ref().map(RepScore::total))
                .collect();
            let summary = summarize_set(&scored_totals);
            events.push(DrillEvent::SetFinished { summary });
        }
    }

    /// Aborts the drill set early, transitioning to [`Phase::Aborted`].
    ///
    /// If the phase is [`Phase::Idle`], [`Phase::Finished`], or [`Phase::Aborted`],
    /// this does nothing and returns `None`. Otherwise transitions to [`Phase::Aborted`]
    /// and returns a [`SetSummary`] summarizing all repetitions scored before the abort,
    /// or `None` if zero repetitions were completed. Further calls to [`Self::push`]
    /// become no-ops.
    pub fn abort(&mut self) -> Option<SetSummary> {
        match self.phase {
            Phase::Idle | Phase::Finished | Phase::Aborted => None,
            Phase::Countdown { .. }
            | Phase::ThrottleWait { .. }
            | Phase::ThrottleHold { .. }
            | Phase::Active { .. }
            | Phase::Scoring { .. } => {
                self.phase = Phase::Aborted;
                self.buffer.clear();
                let scored_totals: Vec<f32> = self
                    .results
                    .iter()
                    .filter_map(|r| r.as_ref().map(RepScore::total))
                    .collect();
                summarize_set(&scored_totals)
            }
        }
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
                    let curve = self.curve.as_ref()?;
                    let elapsed_us = t_us.saturating_sub(self.current_rep_start_us);
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "elapsed time in microseconds fits within f64"
                    )]
                    let dt_ms = (elapsed_us as f64) / 1000.0;
                    Some(curve.value_at(dt_ms))
                }
            },
            Phase::Idle
            | Phase::Countdown { .. }
            | Phase::ThrottleWait { .. }
            | Phase::ThrottleHold { .. }
            | Phase::Finished
            | Phase::Aborted => None,
        }
    }

    /// Target band `(lo, hi)` as fractions at timestamp `t_us`, with the same phase rules as
    /// [`Self::target_at`]. A hold drill gives `(target, target)`; a trace drill gives the
    /// range of the curve within ±[`RAMP_WINDOW_MS`], looked up at the nearest millisecond in
    /// a table built once per run (this runs on the input thread). The tolerance is not
    /// included.
    #[must_use]
    pub fn band_at(&self, t_us: u64) -> Option<(f32, f32)> {
        match self.phase {
            Phase::Active { .. } | Phase::Scoring { .. } => match &self.drill.kind {
                DrillKind::Hold { .. } => self.drill.target_fraction().map(|v| (v, v)),
                DrillKind::Trace { .. } => {
                    let band = self.band.as_ref()?;
                    let elapsed_us = t_us.saturating_sub(self.current_rep_start_us);
                    #[expect(
                        clippy::cast_precision_loss,
                        reason = "elapsed time in microseconds fits within f64"
                    )]
                    let dt_ms = (elapsed_us as f64) / 1000.0;
                    Some(band.at(dt_ms))
                }
            },
            Phase::Idle
            | Phase::Countdown { .. }
            | Phase::ThrottleWait { .. }
            | Phase::ThrottleHold { .. }
            | Phase::Finished
            | Phase::Aborted => None,
        }
    }

    /// Throttle level fraction to hold while waiting for or holding the throttle of a lead-in
    /// drill, or `None` in every other phase.
    #[must_use]
    pub fn throttle_target_at(&self) -> Option<f32> {
        match self.phase {
            Phase::ThrottleWait { .. } | Phase::ThrottleHold { .. } => self.throttle_level,
            Phase::Idle
            | Phase::Countdown { .. }
            | Phase::Active { .. }
            | Phase::Scoring { .. }
            | Phase::Finished
            | Phase::Aborted => None,
        }
    }

    /// Normalized progress fraction in `0.0..=1.0` through the current countdown, throttle hold
    /// or active phase (`0.0` while waiting for the throttle, `1.0` during scoring), or `None`
    /// during idle, finished, or aborted states.
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
            Phase::ThrottleWait { .. } => Some(0.0),
            Phase::ThrottleHold {
                start_us, ends_us, ..
            }
            | Phase::Active {
                start_us, ends_us, ..
            } => {
                if ends_us <= start_us {
                    return Some(1.0);
                }
                let total = ends_us - start_us;
                let elapsed = t_us.saturating_sub(start_us);
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "phase duration in microseconds fits within f64"
                )]
                let frac = (elapsed as f64) / (total as f64);
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "progress fraction fits within f32"
                )]
                Some((frac as f32).clamp(0.0, 1.0))
            }
            Phase::Scoring { .. } => Some(1.0),
            Phase::Idle | Phase::Finished | Phase::Aborted => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::{Pedal, parse_preset};
    use crate::trace_scoring::TraceScore;

    fn make_hold_drill(reps: u32, lead_in_ms: u32, hold_ms: u32, target: f32) -> Drill {
        Drill {
            id: "hold-drill".to_string(),
            name: "Hold Drill".to_string(),
            pedal: Pedal::Brake,
            reps,
            lead_in_ms,
            tolerance: Some(5.0),
            decimals: None,
            throttle_lead_in: None,
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
            tolerance: Some(6.0),
            decimals: None,
            throttle_lead_in: None,
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
                start_us: 0,
                ends_us: 1_000_000,
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
                start_us: 0,
                ends_us: 1_000_000,
            }
        );
        assert_eq!(
            events[1],
            DrillEvent::RepStarted {
                rep: 0,
                start_us: 1_000_000,
            }
        );

        match &events[2] {
            DrillEvent::RepScored { rep, score, .. } => {
                assert_eq!(*rep, 0);
                assert!(score.total() >= 98.0, "rep 0 total was {}", score.total());
            }
            other => panic!("expected RepScored(0), got {other:?}"),
        }

        assert_eq!(
            events[3],
            DrillEvent::CountdownStarted {
                rep: 1,
                start_us: 1_500_000,
                ends_us: 1_800_000,
            }
        );
        assert_eq!(
            events[4],
            DrillEvent::RepStarted {
                rep: 1,
                start_us: 1_800_000,
            }
        );

        match &events[5] {
            DrillEvent::RepScored { rep, score, .. } => {
                assert_eq!(*rep, 1);
                assert!(score.total() >= 98.0, "rep 1 total was {}", score.total());
            }
            other => panic!("expected RepScored(1), got {other:?}"),
        }

        match &events[6] {
            DrillEvent::SetFinished { summary } => {
                let summary = summary.as_ref().expect("summary should be present");
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
                start_us: 5_000_000,
                ends_us: 6_000_000,
            }]
        );

        let push_events = run.push(ValueSample::new(6_000_000, 0.70));
        assert_eq!(
            push_events,
            vec![DrillEvent::RepStarted {
                rep: 0,
                start_us: 6_000_000,
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
                start_us: 0,
                ends_us: 1_000_000,
            }]
        );

        // Jump far past rep 0 and rep 1 boundaries
        let jump_events = run.push(ValueSample::new(50_000_000, 0.70));
        assert_eq!(
            jump_events,
            vec![
                DrillEvent::RepStarted {
                    rep: 0,
                    start_us: 1_000_000,
                },
                DrillEvent::RepFailed {
                    rep: 0,
                    overlap: None,
                },
                DrillEvent::CountdownStarted {
                    rep: 1,
                    start_us: 1_500_000,
                    ends_us: 1_800_000,
                },
                DrillEvent::RepStarted {
                    rep: 1,
                    start_us: 1_800_000,
                },
                DrillEvent::RepFailed {
                    rep: 1,
                    overlap: None,
                },
                DrillEvent::SetFinished { summary: None },
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
        assert_eq!(run.progress(1_700_000), Some(1.0));

        feed_1khz(&mut run, 1_601_000, 1_899_000, |_t| 0.0);
        assert_eq!(run.phase(), Phase::Scoring { rep: 0 });

        // Feeding up to 1_900_000 finishes scoring
        let final_events = run.push(ValueSample::new(1_900_000, 0.0));
        assert_eq!(run.phase(), Phase::Finished);

        match &final_events[0] {
            DrillEvent::RepScored { rep, score, .. } => {
                assert_eq!(*rep, 0);
                assert!(score.total() >= 95.0, "total was {}", score.total());
            }
            other => panic!("expected RepScored, got {other:?}"),
        }
        assert!(matches!(
            final_events[1],
            DrillEvent::SetFinished { summary: Some(_) }
        ));
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
    fn band_at_hold_and_trace() {
        let mut hold_run = DrillRun::new(make_hold_drill(1, 1000, 500, 70.0), 300);
        hold_run.start(0);
        assert_eq!(hold_run.band_at(500_000), None);
        hold_run.push(ValueSample::new(1_000_000, 0.70));
        assert_eq!(hold_run.band_at(1_200_000), Some((0.70, 0.70)));

        let points = vec![(0, 0.0), (150, 90.0), (600, 0.0)];
        let curve = TraceCurve::from_points(&points);
        let mut trace_run = DrillRun::new(make_trace_drill(1, 1000, points), 300);
        trace_run.start(0);
        assert_eq!(trace_run.band_at(500_000), None);
        trace_run.push(ValueSample::new(1_000_000, 0.0));

        // On the ramp the band spans the curve from t-150 to t+150 ms.
        let (lo, hi) = trace_run.band_at(1_075_000).unwrap();
        assert_eq!((lo, hi), curve.envelope_at(75.0, RAMP_WINDOW_MS));
        assert!(lo.abs() < 1e-6, "lo was {lo}");
        assert!((hi - 0.90).abs() < 1e-4, "hi was {hi}");
        let target = trace_run.target_at(1_075_000).unwrap();
        assert!(lo <= target && target <= hi);
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
            start_us: 1_000,
            ends_us: 5_000,
        };
        let json_event1 = serde_json::to_value(&event1).unwrap();
        assert_eq!(json_event1["event"], "countdownStarted");
        assert_eq!(json_event1["rep"], 0);
        assert_eq!(json_event1["startUs"], 1_000);
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
            overlap: None,
        };
        let json_event3 = serde_json::to_value(&event3).unwrap();
        assert_eq!(json_event3["event"], "repScored");
        assert_eq!(json_event3["score"]["kind"], "hold");

        let event4 = DrillEvent::RepFailed {
            rep: 2,
            overlap: None,
        };
        let json_event4 = serde_json::to_value(&event4).unwrap();
        assert_eq!(json_event4["event"], "repFailed");
        assert_eq!(json_event4["rep"], 2);

        let summary = summarize_set(&[90.0, 95.0]);
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

    #[test]
    fn test_all_reps_fail_emits_set_finished_none() {
        let drill = make_hold_drill(2, 1000, 500, 70.0);
        let mut run = DrillRun::new(drill, 300);

        let mut events = run.start(0);
        let jump_events = run.push(ValueSample::new(50_000_000, 0.70));
        events.extend(jump_events);

        assert_eq!(run.phase(), Phase::Finished);
        assert_eq!(
            events.last(),
            Some(&DrillEvent::SetFinished { summary: None })
        );
    }

    #[test]
    fn test_abort_while_idle() {
        let drill = make_hold_drill(1, 1000, 500, 70.0);
        let mut run = DrillRun::new(drill, 300);
        assert_eq!(run.phase(), Phase::Idle);
        assert_eq!(run.abort(), None);
        assert_eq!(run.phase(), Phase::Idle);
    }

    #[test]
    fn test_abort_after_finished() {
        let drill = make_hold_drill(1, 1000, 500, 70.0);
        let mut run = DrillRun::new(drill, 300);
        run.start(0);
        feed_1khz(&mut run, 0, 1_500_000, |_t| 0.70);
        assert_eq!(run.phase(), Phase::Finished);
        assert_eq!(run.abort(), None);
        assert_eq!(run.phase(), Phase::Finished);
    }

    #[test]
    fn test_multi_rep_trace_perfect() {
        let points = vec![(0, 0.0), (150, 90.0), (600, 0.0)];
        let drill = make_trace_drill(2, 1000, points);
        let curve = drill.trace_curve().unwrap();
        let mut run = DrillRun::new(drill, 300);

        let mut events = run.start(0);

        // Rep 0: countdown [0..1_000_000), active [1_000_000..1_600_000), scoring [1_600_000..1_900_000]
        // Rep 1: countdown [1_900_000..2_200_000), active [2_200_000..2_800_000), scoring [2_800_000..3_100_000]
        let pushed_events = feed_1khz(&mut run, 0, 3_100_000, |t| {
            if (1_000_000..=1_600_000).contains(&t) {
                #[expect(clippy::cast_precision_loss, reason = "dt fits within f64")]
                let ms = ((t - 1_000_000) as f64) / 1000.0;
                curve.value_at(ms)
            } else if (2_200_000..=2_800_000).contains(&t) {
                #[expect(clippy::cast_precision_loss, reason = "dt fits within f64")]
                let ms = ((t - 2_200_000) as f64) / 1000.0;
                curve.value_at(ms)
            } else {
                0.0
            }
        });
        events.extend(pushed_events);

        assert_eq!(run.phase(), Phase::Finished);
        assert_eq!(events.len(), 7);

        // Assert event order:
        // CountdownStarted(0) -> RepStarted(0) -> RepScored(0) ->
        // CountdownStarted(1) -> RepStarted(1) -> RepScored(1) -> SetFinished(Some)
        assert_eq!(
            events[0],
            DrillEvent::CountdownStarted {
                rep: 0,
                start_us: 0,
                ends_us: 1_000_000,
            }
        );
        assert_eq!(
            events[1],
            DrillEvent::RepStarted {
                rep: 0,
                start_us: 1_000_000,
            }
        );
        let score0 = match &events[2] {
            DrillEvent::RepScored { rep: 0, score, .. } => score.clone(),
            other => panic!("expected RepScored(0), got {other:?}"),
        };
        assert_eq!(
            events[3],
            DrillEvent::CountdownStarted {
                rep: 1,
                start_us: 1_900_000,
                ends_us: 2_200_000,
            }
        );
        assert_eq!(
            events[4],
            DrillEvent::RepStarted {
                rep: 1,
                start_us: 2_200_000,
            }
        );
        let score1 = match &events[5] {
            DrillEvent::RepScored { rep: 1, score, .. } => score.clone(),
            other => panic!("expected RepScored(1), got {other:?}"),
        };
        assert!(matches!(
            events[6],
            DrillEvent::SetFinished { summary: Some(_) }
        ));

        // both scores high (>= 90) and nearly equal
        assert!(score0.total() >= 90.0, "score0 was {}", score0.total());
        assert!(score1.total() >= 90.0, "score1 was {}", score1.total());
        assert!(
            (score0.total() - score1.total()).abs() < 1.0,
            "scores differed: score0={}, score1={}",
            score0.total(),
            score1.total()
        );
    }

    fn with_lead_in(mut drill: Drill, level: f32, hold_ms: u32) -> Drill {
        drill.throttle_lead_in = Some(crate::preset::ThrottleLeadIn { level, hold_ms });
        drill
    }

    /// Feeds 1 kHz samples where `pedals(t)` returns `(brake, throttle)`.
    fn feed_pedals<F>(
        run: &mut DrillRun,
        start_t_us: u64,
        end_t_us: u64,
        mut pedals: F,
    ) -> Vec<DrillEvent>
    where
        F: FnMut(u64) -> (f32, f32),
    {
        let mut all_events = Vec::new();
        let mut t = start_t_us;
        while t <= end_t_us {
            let (brake, throttle) = pedals(t);
            all_events.extend(run.push_pedals(ValueSample::new(t, brake), throttle));
            t = t.saturating_add(1_000);
        }
        all_events
    }

    fn overlap_of(event: &DrillEvent) -> Option<Overlap> {
        match event {
            DrillEvent::RepScored { overlap, .. } | DrillEvent::RepFailed { overlap, .. } => {
                *overlap
            }
            other => panic!("expected RepScored or RepFailed, got {other:?}"),
        }
    }

    fn trace_value(curve: &TraceCurve, t: u64, cue_us: u64) -> f32 {
        if t < cue_us {
            return 0.0;
        }
        #[expect(clippy::cast_precision_loss, reason = "dt fits within f64")]
        let ms = ((t - cue_us) as f64) / 1000.0;
        curve.value_at(ms)
    }

    #[test]
    fn lead_in_regression_throttle_ignored_without_lead_in() {
        // Hold drill: same samples as test_1, through push and push_pedals with the throttle on.
        let drill = make_hold_drill(2, 1000, 500, 70.0);
        let mut plain = DrillRun::new(drill.clone(), 300);
        let mut pedals = DrillRun::new(drill, 300);
        let mut plain_events = plain.start(0);
        let mut pedal_events = pedals.start(0);
        plain_events.extend(feed_1khz(&mut plain, 0, 2_300_000, |_t| 0.70));
        pedal_events.extend(feed_pedals(&mut pedals, 0, 2_300_000, |_t| (0.70, 0.9)));
        assert_eq!(plain_events, pedal_events);
        assert_eq!(plain.results(), pedals.results());
        assert_eq!(pedals.results().len(), 2);
        for result in pedals.results() {
            let total = result.as_ref().unwrap().total();
            assert!(total >= 98.0, "hold total was {total}");
        }

        // Trace drill: same samples as test_multi_rep_trace_perfect.
        let points = vec![(0, 0.0), (150, 90.0), (600, 0.0)];
        let drill = make_trace_drill(2, 1000, points);
        let curve = drill.trace_curve().unwrap();
        let brake = |t: u64| {
            if (1_000_000..=1_600_000).contains(&t) {
                trace_value(&curve, t, 1_000_000)
            } else if (2_200_000..=2_800_000).contains(&t) {
                trace_value(&curve, t, 2_200_000)
            } else {
                0.0
            }
        };
        let mut plain = DrillRun::new(drill.clone(), 300);
        let mut pedals = DrillRun::new(drill, 300);
        let mut plain_events = plain.start(0);
        let mut pedal_events = pedals.start(0);
        plain_events.extend(feed_1khz(&mut plain, 0, 3_100_000, brake));
        pedal_events.extend(feed_pedals(&mut pedals, 0, 3_100_000, |t| (brake(t), 0.9)));
        assert_eq!(plain_events, pedal_events);
        assert_eq!(pedal_events.len(), 7);
        for result in pedals.results() {
            let total = result.as_ref().unwrap().total();
            assert!(total >= 90.0, "trace total was {total}");
        }
        for event in &pedal_events {
            if matches!(event, DrillEvent::RepScored { .. }) {
                assert_eq!(overlap_of(event), None);
            }
        }
    }

    #[test]
    fn lead_in_throttle_at_level_from_start() {
        let drill = with_lead_in(make_hold_drill(1, 1000, 500, 70.0), 80.0, 1000);
        let mut run = DrillRun::new(drill, 300);
        let mut events = run.start(0);

        events.extend(feed_pedals(&mut run, 0, 1_000_000, |_t| (0.0, 0.80)));
        assert_eq!(
            run.phase(),
            Phase::ThrottleHold {
                rep: 0,
                start_us: 1_000_000,
                ends_us: 2_000_000
            }
        );
        assert_eq!(run.throttle_target_at(), Some(0.80));
        assert_eq!(run.target_at(1_500_000), None);
        assert_eq!(run.band_at(1_500_000), None);
        assert_eq!(run.progress(1_500_000), Some(0.5));

        // Lift exactly at the cue and brake after it.
        events.extend(feed_pedals(&mut run, 1_001_000, 2_500_000, |t| {
            if t < 2_000_000 {
                (0.0, 0.80)
            } else {
                (0.70, 0.0)
            }
        }));
        assert_eq!(run.phase(), Phase::Finished);
        assert_eq!(events.len(), 5, "{events:?}");
        assert_eq!(
            events[0],
            DrillEvent::CountdownStarted {
                rep: 0,
                start_us: 0,
                ends_us: 1_000_000
            }
        );
        assert_eq!(
            events[1],
            DrillEvent::ThrottleHoldStarted {
                rep: 0,
                start_us: 1_000_000,
                ends_us: 2_000_000
            }
        );
        assert_eq!(
            events[2],
            DrillEvent::RepStarted {
                rep: 0,
                start_us: 2_000_000
            }
        );
        match &events[3] {
            DrillEvent::RepScored { rep: 0, score, .. } => {
                assert!(score.total() >= 98.0, "total was {}", score.total());
            }
            other => panic!("expected RepScored(0), got {other:?}"),
        }
        assert_eq!(
            overlap_of(&events[3]),
            Some(Overlap {
                overlap_ms: 0.0,
                peak_throttle: 0.0
            })
        );
        assert!(matches!(
            events[4],
            DrillEvent::SetFinished { summary: Some(_) }
        ));
    }

    #[test]
    fn lead_in_waits_for_throttle_after_countdown() {
        let drill = with_lead_in(make_hold_drill(1, 1000, 500, 70.0), 80.0, 1000);
        let mut run = DrillRun::new(drill, 300);
        let mut events = run.start(0);

        events.extend(feed_pedals(&mut run, 0, 1_299_000, |_t| (0.0, 0.0)));
        assert_eq!(run.phase(), Phase::ThrottleWait { rep: 0 });
        assert_eq!(run.progress(1_200_000), Some(0.0));
        assert_eq!(run.throttle_target_at(), Some(0.80));

        events.extend(feed_pedals(&mut run, 1_300_000, 2_800_000, |t| {
            if t < 2_300_000 {
                (0.0, 0.85)
            } else {
                (0.70, 0.0)
            }
        }));
        assert_eq!(
            events[..4],
            [
                DrillEvent::CountdownStarted {
                    rep: 0,
                    start_us: 0,
                    ends_us: 1_000_000
                },
                DrillEvent::ThrottleWait {
                    rep: 0,
                    since_us: 1_000_000
                },
                DrillEvent::ThrottleHoldStarted {
                    rep: 0,
                    start_us: 1_300_000,
                    ends_us: 2_300_000
                },
                DrillEvent::RepStarted {
                    rep: 0,
                    start_us: 2_300_000
                },
            ]
        );
        assert!(matches!(events[4], DrillEvent::RepScored { rep: 0, .. }));
        assert_eq!(run.phase(), Phase::Finished);
    }

    #[test]
    fn lead_in_throttle_drop_restarts_hold() {
        let drill = with_lead_in(make_hold_drill(1, 1000, 500, 70.0), 80.0, 1000);
        let mut run = DrillRun::new(drill, 300);
        let mut events = run.start(0);

        events.extend(feed_pedals(&mut run, 0, 3_000_000, |t| {
            if t < 1_400_000 {
                (0.0, 0.80)
            } else if t < 1_500_000 {
                (0.0, 0.50)
            } else if t < 2_500_000 {
                (0.0, 0.80)
            } else {
                (0.70, 0.0)
            }
        }));
        assert_eq!(
            events[..5],
            [
                DrillEvent::CountdownStarted {
                    rep: 0,
                    start_us: 0,
                    ends_us: 1_000_000
                },
                DrillEvent::ThrottleHoldStarted {
                    rep: 0,
                    start_us: 1_000_000,
                    ends_us: 2_000_000
                },
                DrillEvent::ThrottleWait {
                    rep: 0,
                    since_us: 1_400_000
                },
                DrillEvent::ThrottleHoldStarted {
                    rep: 0,
                    start_us: 1_500_000,
                    ends_us: 2_500_000
                },
                DrillEvent::RepStarted {
                    rep: 0,
                    start_us: 2_500_000
                },
            ]
        );
        assert!(matches!(events[5], DrillEvent::RepScored { rep: 0, .. }));
        assert_eq!(run.phase(), Phase::Finished);
    }

    #[test]
    fn lead_in_overlap_numbers() {
        // Throttle at 30% for the first 200 ms of the active window while braking at 70%.
        let drill = with_lead_in(make_hold_drill(1, 1000, 500, 70.0), 80.0, 1000);
        let mut run = DrillRun::new(drill, 300);
        run.start(0);
        let events = feed_pedals(&mut run, 0, 2_500_000, |t| {
            if t < 2_000_000 {
                (0.0, 0.80)
            } else if t < 2_200_000 {
                (0.70, 0.30)
            } else {
                (0.70, 0.0)
            }
        });
        let scored = events
            .iter()
            .find(|e| matches!(e, DrillEvent::RepScored { .. }))
            .expect("rep scored");
        let overlap = overlap_of(scored).expect("lead-in drill reports overlap");
        assert!(
            (overlap.overlap_ms - 200.0).abs() <= 2.0,
            "overlap_ms was {}",
            overlap.overlap_ms
        );
        assert!(
            (overlap.peak_throttle - 0.30).abs() <= 1e-6,
            "peak was {}",
            overlap.peak_throttle
        );

        // Brake at 10% for the last 500 ms of the hold while holding the throttle.
        let drill = with_lead_in(make_hold_drill(1, 1000, 500, 70.0), 80.0, 1000);
        let mut run = DrillRun::new(drill, 300);
        run.start(0);
        let events = feed_pedals(&mut run, 0, 2_500_000, |t| {
            if t < 1_500_000 {
                (0.0, 0.80)
            } else if t < 2_000_000 {
                (0.10, 0.80)
            } else {
                (0.70, 0.0)
            }
        });
        let scored = events
            .iter()
            .find(|e| matches!(e, DrillEvent::RepScored { .. }))
            .expect("rep scored");
        let overlap = overlap_of(scored).expect("lead-in drill reports overlap");
        assert!(
            (overlap.overlap_ms - 500.0).abs() <= 2.0,
            "overlap_ms was {}",
            overlap.overlap_ms
        );
        assert!(
            (overlap.peak_throttle - 0.80).abs() <= 1e-6,
            "peak was {}",
            overlap.peak_throttle
        );
    }

    #[test]
    fn lead_in_trace_scores_like_plain_trace() {
        let points = vec![(0, 0.0), (150, 90.0), (600, 0.0)];
        let plain_drill = make_trace_drill(1, 1000, points);
        let curve = plain_drill.trace_curve().unwrap();
        let lead_in_drill = with_lead_in(plain_drill.clone(), 80.0, 1000);

        // Plain: cue at 1.0 s, pre-roll from the countdown.
        let mut plain = DrillRun::new(plain_drill, 300);
        plain.start(0);
        feed_1khz(&mut plain, 0, 1_900_000, |t| {
            trace_value(&curve, t, 1_000_000)
        });

        // Lead-in: cue at 2.0 s, pre-roll from the throttle hold.
        let mut lead_in = DrillRun::new(lead_in_drill, 300);
        lead_in.start(0);
        let events = feed_pedals(&mut lead_in, 0, 2_900_000, |t| {
            let throttle = if t < 2_000_000 { 0.80 } else { 0.0 };
            (trace_value(&curve, t, 2_000_000), throttle)
        });

        assert_eq!(plain.phase(), Phase::Finished);
        assert_eq!(lead_in.phase(), Phase::Finished);
        let plain_score = plain.results()[0].as_ref().unwrap();
        let lead_in_score = lead_in.results()[0].as_ref().unwrap();
        assert!(
            plain_score.total() >= 95.0,
            "total was {}",
            plain_score.total()
        );
        assert_eq!(plain_score, lead_in_score);
        assert!(events.contains(&DrillEvent::RepStarted {
            rep: 0,
            start_us: 2_000_000
        }));
    }

    #[test]
    fn lead_in_serialization() {
        let wait = serde_json::to_value(DrillEvent::ThrottleWait {
            rep: 1,
            since_us: 5_000,
        })
        .unwrap();
        assert_eq!(wait["event"], "throttleWait");
        assert_eq!(wait["rep"], 1);
        assert_eq!(wait["sinceUs"], 5_000);

        let hold = serde_json::to_value(DrillEvent::ThrottleHoldStarted {
            rep: 0,
            start_us: 1_000,
            ends_us: 2_000,
        })
        .unwrap();
        assert_eq!(hold["event"], "throttleHoldStarted");
        assert_eq!(hold["startUs"], 1_000);
        assert_eq!(hold["endsUs"], 2_000);

        let score = RepScore::Hold(HoldScore {
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
        });
        let overlap = Overlap {
            overlap_ms: 120.0,
            peak_throttle: 0.25,
        };
        let scored = serde_json::to_value(DrillEvent::RepScored {
            rep: 0,
            score: score.clone(),
            overlap: Some(overlap),
        })
        .unwrap();
        assert_eq!(scored["overlap"]["overlapMs"], 120.0);
        assert_eq!(scored["overlap"]["peakThrottle"], 0.25);

        let failed = serde_json::to_value(DrillEvent::RepFailed {
            rep: 0,
            overlap: Some(overlap),
        })
        .unwrap();
        assert_eq!(failed["event"], "repFailed");
        assert_eq!(failed["overlap"]["overlapMs"], 120.0);

        let plain = serde_json::to_value(DrillEvent::RepScored {
            rep: 0,
            score,
            overlap: None,
        })
        .unwrap();
        assert!(plain.get("overlap").is_none());
        let plain_failed = serde_json::to_value(DrillEvent::RepFailed {
            rep: 0,
            overlap: None,
        })
        .unwrap();
        assert!(plain_failed.get("overlap").is_none());

        let phase = serde_json::to_value(Phase::ThrottleHold {
            rep: 0,
            start_us: 1,
            ends_us: 2,
        })
        .unwrap();
        assert_eq!(phase["phase"], "throttleHold");
        assert_eq!(phase["endsUs"], 2);
        let phase = serde_json::to_value(Phase::ThrottleWait { rep: 0 }).unwrap();
        assert_eq!(phase["phase"], "throttleWait");
    }

    #[test]
    fn lead_in_abort_from_wait_and_hold() {
        // Rep 0: countdown to 1.0 s, hold to 2.0 s, active to 2.5 s; rep 1 countdown to 2.8 s.
        for rehold in [false, true] {
            let drill = with_lead_in(make_hold_drill(2, 1000, 500, 70.0), 80.0, 1000);
            let mut run = DrillRun::new(drill, 300);
            run.start(0);
            feed_pedals(&mut run, 0, 2_900_000, |t| {
                if t < 2_000_000 {
                    (0.0, 0.80)
                } else if t < 2_500_000 {
                    (0.70, 0.0)
                } else if rehold {
                    (0.0, 0.80)
                } else {
                    (0.0, 0.0)
                }
            });
            if rehold {
                assert_eq!(
                    run.phase(),
                    Phase::ThrottleHold {
                        rep: 1,
                        start_us: 2_800_000,
                        ends_us: 3_800_000
                    }
                );
            } else {
                assert_eq!(run.phase(), Phase::ThrottleWait { rep: 1 });
            }

            let summary = run.abort().expect("rep 0 scored so summary exists");
            assert_eq!(summary.rep_totals.len(), 1);
            assert!(summary.rep_totals[0] >= 98.0);
            assert_eq!(run.phase(), Phase::Aborted);
            assert_eq!(run.throttle_target_at(), None);
            assert_eq!(run.push_pedals(ValueSample::new(3_000_000, 0.0), 0.80), []);
        }
    }

    /// Counts the [`DrillEvent::ThrottleWait`] and [`DrillEvent::ThrottleHoldStarted`] events.
    fn throttle_events(events: &[DrillEvent]) -> usize {
        events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    DrillEvent::ThrottleWait { .. } | DrillEvent::ThrottleHoldStarted { .. }
                )
            })
            .count()
    }

    fn first_scored(events: &[DrillEvent]) -> &DrillEvent {
        events
            .iter()
            .find(|e| matches!(e, DrillEvent::RepScored { .. }))
            .expect("rep scored")
    }

    #[test]
    fn no_lead_in_scores_match_main() {
        // Expected values came from main at 6559413 (before SCT-037), printed with `{:?}`.
        const NOISE: [f32; 7] = [0.02, -0.015, 0.01, -0.02, 0.005, 0.015, -0.01];
        let points = vec![(0, 0.0), (150, 90.0), (600, 0.0)];

        // The samples of test_1_perfect_hold_two_reps.
        let mut run = DrillRun::new(make_hold_drill(2, 1000, 500, 70.0), 300);
        run.start(0);
        feed_1khz(&mut run, 0, 2_300_000, |_t| 0.70);
        let perfect_hold = RepScore::Hold(HoldScore {
            total: 100.0,
            grade: Grade::S,
            accuracy: 100.0,
            timing: 100.0,
            smoothness: 100.0,
            time_in_band: 1.0,
            rmse: 0.0,
            time_to_band_ms: Some(0.0),
            overshoot: 0.0,
            jitter: 0.0,
        });
        assert_eq!(
            run.results(),
            &[Some(perfect_hold.clone()), Some(perfect_hold)]
        );

        // The samples of test_multi_rep_trace_perfect.
        let drill = make_trace_drill(2, 1000, points.clone());
        let curve = drill.trace_curve().unwrap();
        let mut run = DrillRun::new(drill, 300);
        run.start(0);
        feed_1khz(&mut run, 0, 3_100_000, |t| {
            if (1_000_000..=1_600_000).contains(&t) {
                trace_value(&curve, t, 1_000_000)
            } else if (2_200_000..=2_800_000).contains(&t) {
                trace_value(&curve, t, 2_200_000)
            } else {
                0.0
            }
        });
        let perfect_trace = RepScore::Trace(TraceScore {
            total: 99.58676,
            grade: Grade::S,
            accuracy: 100.0,
            timing: 100.0,
            smoothness: 98.347_046,
            lag_ms: 3.804_708_3e-13,
            time_in_band: 1.0,
            rmse: 0.0,
            overshoot: 0.002_833_724,
            ldlj_user: -8.036_273,
            ldlj_target: -8.036_273,
        });
        assert_eq!(
            run.results(),
            &[Some(perfect_trace.clone()), Some(perfect_trace)]
        );

        // A noisy hold rep.
        let mut run = DrillRun::new(make_hold_drill(1, 1000, 500, 70.0), 300);
        run.start(0);
        feed_1khz(&mut run, 0, 1_500_000, |t| {
            0.70 + NOISE[usize::try_from((t / 1_000) % 7).unwrap()]
        });
        assert_eq!(
            run.results(),
            &[Some(RepScore::Hold(HoldScore {
                total: 90.7435,
                grade: Grade::A,
                accuracy: 96.127_365,
                timing: 100.0,
                smoothness: 70.71927,
                time_in_band: 1.0,
                rmse: 0.014_522_383,
                time_to_band_ms: Some(0.0),
                overshoot: 0.0,
                jitter: 0.014_640_368,
            }))]
        );

        // A trace rep 100 ms late (the delayed run of test_6).
        let drill = make_trace_drill(1, 1000, points);
        let curve = drill.trace_curve().unwrap();
        let mut run = DrillRun::new(drill, 300);
        run.start(0);
        feed_1khz(&mut run, 0, 1_900_000, |t| {
            if t < 1_000_000 {
                0.0
            } else {
                #[expect(clippy::cast_precision_loss, reason = "dt fits within f64")]
                let ms = ((t - 1_000_000) as f64) / 1000.0 - 100.0;
                curve.value_at(ms)
            }
        });
        assert_eq!(
            run.results(),
            &[Some(RepScore::Trace(TraceScore {
                total: 85.617_645,
                grade: Grade::A,
                accuracy: 100.0,
                timing: 44.13793,
                smoothness: 98.33266,
                lag_ms: 100.0,
                time_in_band: 1.0,
                rmse: 0.0,
                overshoot: 0.002_859_115_6,
                ldlj_user: -7.135_460_4,
                ldlj_target: -8.036_273,
            }))]
        );
    }

    #[test]
    fn lead_in_throttle_at_arm_edge_does_not_chatter() {
        // Level 80 arms at 0.70 and drops below 0.65, so 0.699 keeps a started hold.
        let drill = with_lead_in(make_hold_drill(1, 1000, 500, 70.0), 80.0, 1000);
        let mut run = DrillRun::new(drill, 300);
        run.start(0);
        let mut events = feed_pedals(&mut run, 0, 999_000, |_t| (0.0, 0.0));
        events.extend(feed_pedals(&mut run, 1_000_000, 3_000_000, |t| {
            let throttle = if (t / 1_000) % 2 == 0 { 0.699 } else { 0.701 };
            (0.0, throttle)
        }));
        assert!(
            throttle_events(&events) <= 2,
            "{} throttle events",
            throttle_events(&events)
        );
        assert!(events.contains(&DrillEvent::RepStarted {
            rep: 0,
            start_us: 2_001_000
        }));
    }

    #[test]
    fn lead_in_late_sample_after_countdown_starts_hold_at_sample_time() {
        let drill = with_lead_in(make_hold_drill(1, 1000, 500, 70.0), 80.0, 1000);
        let mut run = DrillRun::new(drill, 300);
        run.start(0);
        // The stream stalls through the countdown; the next sample comes 5 s after its end.
        let events = run.push_pedals(ValueSample::new(6_000_000, 0.0), 0.80);
        assert_eq!(
            events,
            vec![DrillEvent::ThrottleHoldStarted {
                rep: 0,
                start_us: 6_000_000,
                ends_us: 7_000_000
            }]
        );
        assert_eq!(
            run.phase(),
            Phase::ThrottleHold {
                rep: 0,
                start_us: 6_000_000,
                ends_us: 7_000_000
            }
        );
    }

    #[test]
    fn lead_in_jump_across_boundaries_stops_in_the_hold() {
        // Rep 0: hold 1.0-2.0 s, active 2.0-2.5 s; rep 1 countdown 2.5-2.8 s.
        let drill = with_lead_in(make_hold_drill(2, 1000, 500, 70.0), 80.0, 1000);
        let mut run = DrillRun::new(drill, 300);
        run.start(0);
        feed_pedals(&mut run, 0, 2_499_000, |t| {
            if t < 2_000_000 {
                (0.0, 0.80)
            } else {
                (0.70, 0.0)
            }
        });
        let events = run.push_pedals(ValueSample::new(10_000_000, 0.0), 0.80);
        assert!(
            matches!(events[0], DrillEvent::RepScored { rep: 0, .. }),
            "{events:?}"
        );
        assert_eq!(
            events[1..],
            [
                DrillEvent::CountdownStarted {
                    rep: 1,
                    start_us: 2_500_000,
                    ends_us: 2_800_000
                },
                DrillEvent::ThrottleHoldStarted {
                    rep: 1,
                    start_us: 10_000_000,
                    ends_us: 11_000_000
                },
            ]
        );
        assert_eq!(
            run.phase(),
            Phase::ThrottleHold {
                rep: 1,
                start_us: 10_000_000,
                ends_us: 11_000_000
            }
        );
    }

    #[test]
    fn lead_in_trace_hold_restart_clears_pre_roll() {
        let points = vec![(0, 0.0), (150, 90.0), (600, 0.0)];
        let drill = with_lead_in(make_trace_drill(1, 1000, points), 80.0, 1000);
        let curve = drill.trace_curve().unwrap();

        // No drop: hold 1.0-2.0 s, cue at 2.0 s.
        let mut steady = DrillRun::new(drill.clone(), 300);
        steady.start(0);
        let steady_events = feed_pedals(&mut steady, 0, 2_900_000, |t| {
            let throttle = if t < 2_000_000 { 0.80 } else { 0.0 };
            (trace_value(&curve, t, 2_000_000), throttle)
        });

        // Brake at 50 % inside the first pre-roll, drop the throttle at 1.8 s, re-arm at 1.9 s:
        // hold 1.9-2.9 s, cue at 2.9 s.
        let mut dropped = DrillRun::new(drill, 300);
        dropped.start(0);
        let dropped_events = feed_pedals(&mut dropped, 0, 3_800_000, |t| {
            if t < 1_700_000 {
                (0.0, 0.80)
            } else if t < 1_800_000 {
                (0.50, 0.80)
            } else if t < 1_900_000 {
                (0.0, 0.50)
            } else if t < 2_900_000 {
                (0.0, 0.80)
            } else {
                (trace_value(&curve, t, 2_900_000), 0.0)
            }
        });

        assert!(dropped_events.contains(&DrillEvent::RepStarted {
            rep: 0,
            start_us: 2_900_000
        }));
        assert_eq!(steady.phase(), Phase::Finished);
        assert_eq!(dropped.phase(), Phase::Finished);
        assert_eq!(steady.results(), dropped.results());
        assert_eq!(
            overlap_of(first_scored(&steady_events)),
            overlap_of(first_scored(&dropped_events))
        );
    }

    #[test]
    fn lead_in_throttle_noise_is_not_a_peak() {
        let drill = with_lead_in(make_hold_drill(1, 1000, 500, 70.0), 80.0, 1000);
        let mut run = DrillRun::new(drill, 300);
        run.start(0);
        let events = feed_pedals(&mut run, 0, 2_500_000, |t| {
            if t < 2_000_000 {
                (0.0, 0.80)
            } else {
                (0.70, 0.01)
            }
        });
        assert_eq!(
            overlap_of(first_scored(&events)),
            Some(Overlap {
                overlap_ms: 0.0,
                peak_throttle: 0.0
            })
        );
    }

    #[test]
    fn lead_in_overlap_counts_up_to_the_active_end() {
        // Both pedals pressed from 2.3 s to the active end at 2.5 s. The boundary sample comes
        // late, at 2.56 s: the last gap counts only up to 2.5 s.
        let drill = with_lead_in(make_hold_drill(1, 1000, 500, 70.0), 80.0, 1000);
        let mut run = DrillRun::new(drill, 300);
        run.start(0);
        feed_pedals(&mut run, 0, 2_499_000, |t| {
            if t < 2_000_000 {
                (0.0, 0.80)
            } else if t < 2_300_000 {
                (0.70, 0.0)
            } else {
                (0.70, 0.30)
            }
        });
        let events = run.push_pedals(ValueSample::new(2_560_000, 0.70), 0.90);
        let overlap = overlap_of(first_scored(&events)).expect("lead-in drill reports overlap");
        assert!(
            (overlap.overlap_ms - 200.0).abs() < 1.0,
            "overlap_ms was {}",
            overlap.overlap_ms
        );
        // The boundary sample lies past the window, so its throttle is not a peak.
        assert!(
            (overlap.peak_throttle - 0.30).abs() <= 1e-6,
            "peak was {}",
            overlap.peak_throttle
        );
    }

    #[test]
    fn lead_in_overlap_gap_is_capped() {
        let drill = with_lead_in(make_hold_drill(1, 1000, 500, 70.0), 80.0, 1000);
        let mut run = DrillRun::new(drill, 300);
        run.start(0);
        run.push_pedals(ValueSample::new(1_000_000, 0.0), 0.80);
        run.push_pedals(ValueSample::new(1_200_000, 0.30), 0.80);
        run.push_pedals(ValueSample::new(1_700_000, 0.30), 0.80);
        assert_eq!(
            run.current_overlap(),
            Some(Overlap {
                overlap_ms: 100.0,
                peak_throttle: 0.80
            })
        );
    }
}
