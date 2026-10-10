//! Drill preset schema, validation, and loading.
//!
//! Presets define a collection of sim-racing pedal drills (e.g. threshold braking,
//! trail-off traces, or throttle modulation) with configurable tolerance and repetitions.

use std::collections::{HashSet, VecDeque};
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Current schema version for preset files.
pub const SCHEMA_VERSION: u32 = 1;

pub const DEFAULT_REPS: u32 = 5;
pub const DEFAULT_LEAD_IN_MS: u32 = 3000;
pub const DEFAULT_TOLERANCE: f32 = 10.0;

/// Shortest allowed lead-in in milliseconds: the countdown shows GO for its last second.
pub const MIN_LEAD_IN_MS: u32 = 1000;
/// Longest allowed lead-in in milliseconds.
pub const MAX_LEAD_IN_MS: u32 = 10000;
/// Shortest allowed trace duration in milliseconds, so the ±150 ms timing window stays small
/// next to the trace.
pub const MIN_TRACE_MS: u32 = 500;
/// Longest allowed trace duration in milliseconds: it fits the UI's 20 s pedal history with
/// the lead-in.
pub const MAX_TRACE_MS: u32 = 15000;
/// Most points a trace may have.
pub const MAX_TRACE_POINTS: usize = 64;

const fn default_reps() -> u32 {
    DEFAULT_REPS
}

const fn default_lead_in() -> u32 {
    DEFAULT_LEAD_IN_MS
}

/// Pedal targeted by a drill (the same type as in device profiles).
pub use crate::profile::Pedal;

/// Specific variant and target profile of a drill.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DrillKind {
    /// Hold pedal at a target percentage for a given duration.
    #[serde(rename_all = "camelCase")]
    Hold {
        /// Target pedal travel in percent (`0.0..=100.0`).
        target: f32,
        /// Duration to hold the target in milliseconds (`200..=60000`).
        hold_ms: u32,
    },
    /// Follow a time-varying pedal trace curve.
    #[serde(rename_all = "camelCase")]
    Trace {
        /// Sequence of `(timestamp_ms, target_percent)` points.
        points: Vec<(u32, f32)>,
    },
}

/// A single practice drill within a preset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Drill {
    /// Unique identifier for this drill within the preset (`[a-z0-9-]+`).
    pub id: String,
    /// Human-readable name shown in the UI.
    pub name: String,
    /// Target pedal for this drill.
    pub pedal: Pedal,
    /// Number of repetitions (`1..=50`, defaults to 5).
    #[serde(default = "default_reps")]
    pub reps: u32,
    /// Lead-in preparation countdown before each rep in milliseconds (`1000..=10000`, defaults to 3000).
    #[serde(default = "default_lead_in")]
    pub lead_in_ms: u32,
    /// Permissible error tolerance in percent (`0.5..=50.0`).
    /// `None` (omitted, or an explicit `null`) means unset: the drill uses 10.0 (D-17).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tolerance: Option<f32>,
    /// Display precision of percentages for this drill (0 or 1 decimal places).
    /// `None` (omitted) means 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u8>,
    /// Drill type and parameters (flattened in JSON).
    #[serde(flatten)]
    pub kind: DrillKind,
}

impl Drill {
    /// Returns the target curve if this is a [`DrillKind::Trace`] drill.
    #[must_use]
    pub fn trace_curve(&self) -> Option<TraceCurve> {
        match &self.kind {
            DrillKind::Trace { points } => Some(TraceCurve::from_points(points)),
            DrillKind::Hold { .. } => None,
        }
    }

    /// Returns the target pedal position as a fraction in `[0.0, 1.0]` if this is a [`DrillKind::Hold`] drill.
    #[must_use]
    pub fn target_fraction(&self) -> Option<f32> {
        match &self.kind {
            DrillKind::Hold { target, .. } => Some(*target / 100.0),
            DrillKind::Trace { .. } => None,
        }
    }

    /// Returns the permissible tolerance as a fraction in `[0.0, 1.0]`.
    #[must_use]
    pub fn tolerance_fraction(&self) -> f32 {
        self.tolerance.unwrap_or(DEFAULT_TOLERANCE) / 100.0
    }

    /// Returns the duration of a single repetition in milliseconds.
    ///
    /// For [`DrillKind::Hold`], this is `hold_ms`. For [`DrillKind::Trace`], this is the
    /// timestamp of the final curve point (or `0` if empty).
    #[must_use]
    pub fn rep_ms(&self) -> u32 {
        match &self.kind {
            DrillKind::Hold { hold_ms, .. } => *hold_ms,
            DrillKind::Trace { points } => points.last().map_or(0, |(t, _)| *t),
        }
    }

    /// Returns the estimated duration of a set of `reps` repetitions in milliseconds.
    ///
    /// Calculated as `lead_in_ms + reps * rep_ms() + (reps - 1) * DEFAULT_REST_MS`
    /// using saturating arithmetic. For [`DrillKind::Trace`] drills, each rep also adds
    /// [`crate::drill_engine::TRACE_LAG_MARGIN_MS`], because the engine ends a trace rep that
    /// many milliseconds after its last point. When `reps` is 0, this returns `lead_in_ms`.
    #[must_use]
    pub fn set_ms(&self, reps: u32) -> u32 {
        if reps == 0 {
            return self.lead_in_ms;
        }
        let reps_duration = reps.saturating_mul(self.rep_ms());
        let rest_duration = reps
            .saturating_sub(1)
            .saturating_mul(crate::drill_engine::DEFAULT_REST_MS);
        let margin_duration = if matches!(self.kind, DrillKind::Trace { .. }) {
            reps.saturating_mul(crate::drill_engine::TRACE_LAG_MARGIN_MS)
        } else {
            0
        };
        self.lead_in_ms
            .saturating_add(reps_duration)
            .saturating_add(rest_duration)
            .saturating_add(margin_duration)
    }
}

/// Shortest allowed estimated warm-up length in ms (D-25).
pub const WARM_UP_MIN_MS: u32 = 180_000;
/// Longest allowed estimated warm-up length in ms (D-25).
pub const WARM_UP_MAX_MS: u32 = 300_000;

/// A single step in a preset's pre-race warm-up sequence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WarmUpStep {
    /// Identifier of the drill to run for this step.
    pub drill: String,
    /// Number of repetitions to perform for this drill during the warm-up (`1..=50`).
    pub reps: u32,
}

/// An ordered pre-race warm-up routine chaining drills from the preset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WarmUp {
    /// Ordered list of drill steps that make up the warm-up.
    pub steps: Vec<WarmUpStep>,
}

/// A drill preset containing metadata and one or more drills.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preset {
    /// Format schema version, must equal [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Unique identifier for this preset (`[a-z0-9-]+`).
    pub id: String,
    /// Human-readable preset name shown in the UI.
    pub name: String,
    /// Optional description of the preset's purpose.
    #[serde(default)]
    pub description: String,
    /// Drills included in this preset.
    pub drills: Vec<Drill>,
    /// Optional pre-race warm-up routine chaining drills from this preset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warm_up: Option<WarmUp>,
}

impl Preset {
    /// Validates all fields and drills of this preset against schema rules.
    ///
    /// # Errors
    ///
    /// Returns [`PresetError::Invalid`] if any field violates validation constraints.
    pub fn validate(&self) -> Result<(), PresetError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(PresetError::Invalid {
                drill: None,
                message: format!(
                    "unsupported schemaVersion {}, expected {SCHEMA_VERSION}",
                    self.schema_version
                ),
            });
        }

        if !is_valid_id(&self.id) {
            return Err(PresetError::Invalid {
                drill: None,
                message: format!(
                    "field 'id' ('{}') must be non-empty and contain only lowercase alphanumeric characters and hyphens ([a-z0-9-]+)",
                    self.id
                ),
            });
        }

        if self.name.is_empty() {
            return Err(PresetError::Invalid {
                drill: None,
                message: "field 'name' must be non-empty".to_string(),
            });
        }

        if self.drills.is_empty() {
            return Err(PresetError::Invalid {
                drill: None,
                message: "field 'drills' must contain at least one drill".to_string(),
            });
        }

        let mut seen_ids = HashSet::new();
        for drill in &self.drills {
            if !seen_ids.insert(&drill.id) {
                return Err(PresetError::Invalid {
                    drill: Some(drill.id.clone()),
                    message: format!("duplicate drill id '{}'", drill.id),
                });
            }

            validate_drill(drill)?;
        }

        if let Some(warm_up) = &self.warm_up {
            self.validate_warm_up(warm_up)?;
        }

        Ok(())
    }

    fn validate_warm_up(&self, warm_up: &WarmUp) -> Result<(), PresetError> {
        if warm_up.steps.is_empty() {
            return Err(PresetError::Invalid {
                drill: None,
                message: "warmUp: steps must be non-empty".to_string(),
            });
        }

        for step in &warm_up.steps {
            if !self.drills.iter().any(|d| d.id == step.drill) {
                return Err(PresetError::Invalid {
                    drill: None,
                    message: format!("warmUp: drill '{}' not found in preset", step.drill),
                });
            }
        }

        let mut seen_warm_up_drills = HashSet::new();
        for step in &warm_up.steps {
            if !seen_warm_up_drills.insert(&step.drill) {
                return Err(PresetError::Invalid {
                    drill: None,
                    message: format!("warmUp: duplicate drill '{}'", step.drill),
                });
            }
        }

        for step in &warm_up.steps {
            if !(1..=50).contains(&step.reps) {
                return Err(PresetError::Invalid {
                    drill: None,
                    message: format!(
                        "warmUp: drill '{}' reps ({}) must be between 1 and 50",
                        step.drill, step.reps
                    ),
                });
            }
        }

        for step in &warm_up.steps {
            if let Some(drill) = self.drills.iter().find(|d| d.id == step.drill)
                && drill.pedal == Pedal::Clutch
            {
                return Err(PresetError::Invalid {
                    drill: None,
                    message: format!(
                        "warmUp: drill '{}' targets clutch pedal, but warm-up does not support clutch",
                        step.drill
                    ),
                });
            }
        }

        let estimate_ms = self.warm_up_estimate_ms().unwrap_or(0);
        if !(WARM_UP_MIN_MS..=WARM_UP_MAX_MS).contains(&estimate_ms) {
            let duration_sec = f64::from(estimate_ms) / 1000.0;
            let min_s = WARM_UP_MIN_MS / 1000;
            let max_s = WARM_UP_MAX_MS / 1000;
            return Err(PresetError::Invalid {
                drill: None,
                message: format!(
                    "warmUp: estimated duration {duration_sec:.1}s ({estimate_ms} ms) must be between {min_s}s and {max_s}s ({WARM_UP_MIN_MS}..={WARM_UP_MAX_MS} ms)"
                ),
            });
        }

        Ok(())
    }

    /// Returns the estimated total duration in milliseconds for this preset's warm-up routine,
    /// or `None` if the preset does not define a warm-up.
    ///
    /// The estimate is calculated by summing [`Drill::set_ms`] across all warm-up steps
    /// using each step's rep count. A step naming a missing drill adds `0`.
    #[must_use]
    pub fn warm_up_estimate_ms(&self) -> Option<u32> {
        let warm_up = self.warm_up.as_ref()?;
        let mut total: u32 = 0;
        for step in &warm_up.steps {
            if let Some(drill) = self.drills.iter().find(|d| d.id == step.drill) {
                total = total.saturating_add(drill.set_ms(step.reps));
            }
        }
        Some(total)
    }
}

fn validate_drill(drill: &Drill) -> Result<(), PresetError> {
    let drill_id = &drill.id;
    let drill_ctx = Some(drill_id.clone());

    if !is_valid_id(drill_id) {
        return Err(PresetError::Invalid {
            drill: drill_ctx,
            message: format!(
                "field 'id' ('{drill_id}') must be non-empty and contain only lowercase alphanumeric characters and hyphens ([a-z0-9-]+)"
            ),
        });
    }

    if drill.name.is_empty() {
        return Err(PresetError::Invalid {
            drill: drill_ctx,
            message: "field 'name' must be non-empty".to_string(),
        });
    }

    if !(1..=50).contains(&drill.reps) {
        return Err(PresetError::Invalid {
            drill: drill_ctx,
            message: format!("field 'reps' ({}) must be between 1 and 50", drill.reps),
        });
    }

    if !(MIN_LEAD_IN_MS..=MAX_LEAD_IN_MS).contains(&drill.lead_in_ms) {
        return Err(PresetError::Invalid {
            drill: drill_ctx,
            message: format!(
                "field 'leadInMs' ({}) must be between {MIN_LEAD_IN_MS} and {MAX_LEAD_IN_MS}",
                drill.lead_in_ms
            ),
        });
    }

    if let Some(tol) = drill.tolerance
        && (!tol.is_finite() || !(0.5..=50.0).contains(&tol))
    {
        return Err(PresetError::Invalid {
            drill: drill_ctx,
            message: format!("field 'tolerance' ({tol}) must be finite and between 0.5 and 50"),
        });
    }

    if let Some(decimals) = drill.decimals
        && decimals > 1
    {
        return Err(PresetError::Invalid {
            drill: drill_ctx,
            message: format!("field 'decimals' ({decimals}) must be 0 or 1"),
        });
    }

    match &drill.kind {
        DrillKind::Hold { target, hold_ms } => {
            if !target.is_finite() || !(0.0..=100.0).contains(target) {
                return Err(PresetError::Invalid {
                    drill: drill_ctx,
                    message: format!(
                        "field 'target' ({target}) must be finite and between 0 and 100"
                    ),
                });
            }
            if !(200..=60000).contains(hold_ms) {
                return Err(PresetError::Invalid {
                    drill: drill_ctx,
                    message: format!("field 'holdMs' ({hold_ms}) must be between 200 and 60000"),
                });
            }
        }
        DrillKind::Trace { points } => {
            validate_trace_points(points, drill_id)?;
        }
    }

    Ok(())
}

fn validate_trace_points(points: &[(u32, f32)], drill_id: &str) -> Result<(), PresetError> {
    let drill_ctx = Some(drill_id.to_string());

    if points.len() < 2 {
        return Err(PresetError::Invalid {
            drill: drill_ctx,
            message: format!(
                "field 'points' must contain at least 2 points, found {}",
                points.len()
            ),
        });
    }

    if points.len() > MAX_TRACE_POINTS {
        return Err(PresetError::Invalid {
            drill: drill_ctx,
            message: format!(
                "field 'points' must contain at most {MAX_TRACE_POINTS} points, found {}",
                points.len()
            ),
        });
    }

    if points[0].0 != 0 {
        return Err(PresetError::Invalid {
            drill: drill_ctx,
            message: format!(
                "field 'points': first point timestamp must be 0, found {}",
                points[0].0
            ),
        });
    }

    for window in points.windows(2) {
        let (p0, p1) = (&window[0], &window[1]);
        if p1.0 <= p0.0 {
            return Err(PresetError::Invalid {
                drill: drill_ctx,
                message: format!(
                    "field 'points': timestamps must be strictly increasing, but {} <= {}",
                    p1.0, p0.0
                ),
            });
        }
    }

    for (t, val) in points {
        if !val.is_finite() || !(0.0..=100.0).contains(val) {
            return Err(PresetError::Invalid {
                drill: drill_ctx,
                message: format!(
                    "field 'points': value {val} at t={t} must be finite and between 0 and 100"
                ),
            });
        }
    }

    let duration = points[points.len() - 1].0;
    if !(MIN_TRACE_MS..=MAX_TRACE_MS).contains(&duration) {
        return Err(PresetError::Invalid {
            drill: drill_ctx,
            message: format!(
                "field 'points': duration ({duration} ms) must be between {MIN_TRACE_MS} and {MAX_TRACE_MS} ms"
            ),
        });
    }

    Ok(())
}

fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A validated target curve expressed in normalized fractional pedal positions `[0.0, 1.0]`.
#[derive(Clone, Debug)]
pub struct TraceCurve {
    points: Vec<(u32, f32)>,
    /// Final Fritsch–Carlson tangent at each point, in fraction per ms.
    tangents: Vec<f64>,
}

/// The tangents follow from the points, so two curves are equal when their points are.
impl PartialEq for TraceCurve {
    fn eq(&self, other: &Self) -> bool {
        self.points == other.points
    }
}

impl TraceCurve {
    /// Constructs a `TraceCurve` by converting percentage values (`0.0..=100.0`) to fractions (`0.0..=1.0`).
    #[must_use]
    pub fn from_points(points: &[(u32, f32)]) -> Self {
        let points: Vec<(u32, f32)> = points.iter().map(|&(t, pct)| (t, pct / 100.0)).collect();
        let tangents = fritsch_carlson_tangents(&points);
        Self { points, tangents }
    }

    /// Returns the duration of the trace in milliseconds (timestamp of the final point).
    #[must_use]
    pub fn duration_ms(&self) -> u32 {
        self.points.last().map_or(0, |p| p.0)
    }

    /// Returns the points of the curve as `(timestamp_ms, target_fraction)`.
    #[must_use]
    pub fn points(&self) -> &[(u32, f32)] {
        &self.points
    }

    /// Computes the target value at `t_ms` by monotone cubic (Fritsch–Carlson) interpolation
    /// with zero slope at both ends, so the curve is rounded and never overshoots its points.
    ///
    /// Timestamps outside `[0, duration]` are clamped to the first or last point value.
    #[must_use]
    pub fn value_at(&self, t_ms: f64) -> f32 {
        if self.points.is_empty() {
            return 0.0;
        }
        if self.points.len() == 1 {
            return self.points[0].1;
        }

        let first_t = f64::from(self.points[0].0);
        if !t_ms.is_finite() || t_ms <= first_t {
            return self.points[0].1;
        }

        let last_t = f64::from(self.duration_ms());
        if t_ms >= last_t {
            return self.points[self.points.len() - 1].1;
        }

        let idx = self.points.partition_point(|p| f64::from(p.0) <= t_ms);
        let (t0_u32, v0_f32) = self.points[idx - 1];
        let (t1_u32, v1_f32) = self.points[idx];

        let t0 = f64::from(t0_u32);
        let t1 = f64::from(t1_u32);
        let h = t1 - t0;
        if h <= 0.0 {
            return v0_f32;
        }

        let (m0, m1) = (self.tangents[idx - 1], self.tangents[idx]);
        let s = (t_ms - t0) / h;
        let s2 = s * s;
        let s3 = s2 * s;
        let v0 = f64::from(v0_f32);
        let v1 = f64::from(v1_f32);
        let val = (2.0 * s3 - 3.0 * s2 + 1.0) * v0
            + (s3 - 2.0 * s2 + s) * h * m0
            + (-2.0 * s3 + 3.0 * s2) * v1
            + (s3 - s2) * h * m1;

        #[expect(
            clippy::cast_possible_truncation,
            reason = "interpolated fraction fits within f32 range [0.0, 1.0]"
        )]
        let result = val as f32;
        result
    }

    /// Returns the minimum and maximum of [`Self::value_at`] over `t_ms - window_ms ..= t_ms +
    /// window_ms` in 1 ms steps. Outside `[0, duration]` the end values extend the curve.
    #[must_use]
    pub fn envelope_at(&self, t_ms: f64, window_ms: u32) -> (f32, f32) {
        let window = i64::from(window_ms);
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for s in -window..=window {
            #[expect(
                clippy::cast_precision_loss,
                reason = "window offset in milliseconds is small"
            )]
            let v = self.value_at(t_ms + s as f64);
            lo = lo.min(v);
            hi = hi.max(v);
        }
        (lo, hi)
    }

    /// Envelope `[lo, hi]` of `value_at` over `t ± window_ms` for every integer `t` in
    /// `-window_ms ..= duration + window_ms`, so index `i` is `t = i - window_ms`.
    ///
    /// Each entry equals [`Self::envelope_at`] at that integer t. Building it is O(duration);
    /// a lookup with [`EnvelopeTable::at`] is O(1), for the input thread.
    #[must_use]
    pub fn envelope_table(&self, window_ms: u32) -> EnvelopeTable {
        let w = i64::from(window_ms);
        let grid: Vec<f32> = (-w..=i64::from(self.duration_ms()) + w)
            .map(|t| {
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "table times in milliseconds are small"
                )]
                let t = t as f64;
                self.value_at(t)
            })
            .collect();
        let window = window_ms as usize;
        // Past both ends of the grid the curve is constant at the end values, which the grid's
        // first and last window_ms + 1 entries already hold, so clamping the window is exact.
        EnvelopeTable {
            window_ms,
            lo: sliding_extreme(&grid, window, |back, new| back >= new),
            hi: sliding_extreme(&grid, window, |back, new| back <= new),
        }
    }
}

/// Precomputed [`TraceCurve::envelope_at`] at every integer millisecond, built by
/// [`TraceCurve::envelope_table`].
#[derive(Clone, Debug, PartialEq)]
pub struct EnvelopeTable {
    window_ms: u32,
    lo: Vec<f32>,
    hi: Vec<f32>,
}

impl EnvelopeTable {
    /// Envelope `(lo, hi)` at `t_ms` rounded to the nearest millisecond. Before the table it is
    /// `(v, v)` with `v` the first point value, after it the last point value.
    #[must_use]
    pub fn at(&self, t_ms: f64) -> (f32, f32) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the index saturates and is clamped to the table below"
        )]
        let i = (t_ms.round() as i64).saturating_add(i64::from(self.window_ms));
        let last = self.lo.len().saturating_sub(1);
        // The first and last entries are the end values (see `TraceCurve::envelope_table`).
        let i = usize::try_from(i.max(0)).map_or(last, |i| i.min(last));
        (self.lo[i], self.hi[i])
    }
}

/// Extreme of `grid` over `i - window ..= i + window` (clamped to the grid) for every index
/// `i`, with a monotonic deque in O(len). `drop(back, new)` says whether the deque's back
/// entry can never be the extreme again once `new` has entered the window.
fn sliding_extreme(grid: &[f32], window: usize, drop: fn(f32, f32) -> bool) -> Vec<f32> {
    let mut out = Vec::with_capacity(grid.len());
    let mut deque: VecDeque<usize> = VecDeque::new();
    let mut next = 0;
    for i in 0..grid.len() {
        let end = (i + window).min(grid.len() - 1);
        while next <= end {
            while deque.back().is_some_and(|&b| drop(grid[b], grid[next])) {
                deque.pop_back();
            }
            deque.push_back(next);
            next += 1;
        }
        while deque.front().is_some_and(|&f| f + window < i) {
            deque.pop_front();
        }
        out.push(grid[deque[0]]);
    }
    out
}

/// Final Fritsch–Carlson tangents at each point of `points`, in fraction per ms, with zero
/// slope at both ends. One sequential pass: limiting segment `j` may lower the tangents at
/// both of its ends, and after it `m_j` is final.
fn fritsch_carlson_tangents(points: &[(u32, f32)]) -> Vec<f64> {
    let n = points.len();
    if n < 2 {
        return vec![0.0; n];
    }
    let slope = |j: usize| {
        let (ta, ya) = points[j];
        let (tb, yb) = points[j + 1];
        (f64::from(yb) - f64::from(ya)) / (f64::from(tb) - f64::from(ta))
    };

    let mut tangents = Vec::with_capacity(n);
    let mut m_cur = 0.0_f64; // m_0
    for j in 0..n - 1 {
        let d_j = slope(j);
        let mut m_next = if j + 1 == n - 1 {
            0.0
        } else {
            let d_next = slope(j + 1);
            if d_j * d_next <= 0.0 {
                0.0
            } else {
                f64::midpoint(d_j, d_next)
            }
        };
        if d_j == 0.0 {
            m_cur = 0.0;
            m_next = 0.0;
        } else {
            let a = m_cur / d_j;
            let b = m_next / d_j;
            let r2 = a * a + b * b;
            if r2 > 9.0 {
                let tau = 3.0 / r2.sqrt();
                m_cur = tau * a * d_j;
                m_next = tau * b * d_j;
            }
        }
        // m_cur is now the final m_j: later steps never touch it.
        tangents.push(m_cur);
        m_cur = m_next;
    }
    // m_cur is the final last tangent.
    tangents.push(m_cur);
    tangents
}

/// Errors that can occur when parsing or validating preset files.
#[derive(Debug)]
pub enum PresetError {
    /// JSON syntax or deserialization error.
    Json(serde_json::Error),
    /// Schema or business rule validation error.
    Invalid {
        /// Identifier of the drill that caused the error, or `None` if preset-level.
        drill: Option<String>,
        /// Human-readable error description.
        message: String,
    },
    /// I/O error reading from disk.
    Io {
        /// Path associated with the I/O failure.
        path: PathBuf,
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// An error wrapped with the file path where it occurred.
    InFile {
        /// File path where the error occurred.
        path: PathBuf,
        /// Underlying preset error.
        source: Box<PresetError>,
    },
}

impl fmt::Display for PresetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(err) => write!(f, "{err}"),
            Self::Invalid { drill, message } => match drill {
                Some(id) => write!(f, "drill '{id}': {message}"),
                None => write!(f, "{message}"),
            },
            Self::Io { path, source } => {
                write!(f, "failed to read '{}': {source}", path.display())
            }
            Self::InFile { path, source } => {
                write!(f, "in '{}': {source}", path.display())
            }
        }
    }
}

impl std::error::Error for PresetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(err) => Some(err),
            Self::Io { source, .. } => Some(source),
            Self::InFile { source, .. } => Some(source.as_ref()),
            Self::Invalid { .. } => None,
        }
    }
}

impl From<serde_json::Error> for PresetError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

const HOLD_KEYS: &[&str] = &[
    "id",
    "name",
    "type",
    "pedal",
    "reps",
    "leadInMs",
    "tolerance",
    "decimals",
    "target",
    "holdMs",
];
const TRACE_KEYS: &[&str] = &[
    "id",
    "name",
    "type",
    "pedal",
    "reps",
    "leadInMs",
    "tolerance",
    "decimals",
    "points",
];

fn validate_drill_unknown_fields(value: &serde_json::Value) -> Result<(), PresetError> {
    let Some(drills) = value.get("drills").and_then(serde_json::Value::as_array) else {
        return Ok(());
    };

    for drill in drills {
        let Some(obj) = drill.as_object() else {
            continue;
        };
        let drill_id = obj
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(ToString::to_string);
        let drill_type = obj.get("type").and_then(serde_json::Value::as_str);

        let allowed_keys = match drill_type {
            Some("hold") => HOLD_KEYS,
            Some("trace") => TRACE_KEYS,
            _ => continue,
        };

        for key in obj.keys() {
            if !allowed_keys.contains(&key.as_str()) {
                return Err(PresetError::Invalid {
                    drill: drill_id,
                    message: format!("unknown field '{key}'"),
                });
            }
        }
    }

    Ok(())
}

/// Parses a preset from a JSON string, ensuring all schema and business rules pass.
///
/// # Errors
///
/// Returns [`PresetError::Json`] if JSON parsing fails or unknown top-level fields exist.
/// Returns [`PresetError::Invalid`] if validation fails or unknown drill fields exist.
pub fn parse_preset(json: &str) -> Result<Preset, PresetError> {
    let value: serde_json::Value = serde_json::from_str(json)?;
    // Before the typed parse, so a typo reports "unknown field" rather than "missing field".
    validate_drill_unknown_fields(&value)?;
    let preset: Preset = serde_json::from_value(value)?;
    preset.validate()?;
    Ok(preset)
}

/// Finds the drill `drill_id` inside the preset `preset_id`.
///
/// # Errors
///
/// Returns a message naming the missing preset or drill.
pub fn find_drill(presets: &[Preset], preset_id: &str, drill_id: &str) -> Result<Drill, String> {
    let preset = presets
        .iter()
        .find(|p| p.id == preset_id)
        .ok_or_else(|| format!("preset '{preset_id}' not found"))?;
    preset
        .drills
        .iter()
        .find(|d| d.id == drill_id)
        .cloned()
        .ok_or_else(|| format!("drill '{drill_id}' not found in preset '{preset_id}'"))
}

/// Finds the drill `drill_id` inside preset `preset_id` configured for its warm-up routine.
///
/// Returns the matching drill cloned with its `reps` count replaced by the warm-up step's
/// rep count.
///
/// # Errors
///
/// Returns an error message naming the missing preset, the preset having no warm-up,
/// or the drill not being present in the warm-up.
pub fn find_warm_up_drill(
    presets: &[Preset],
    preset_id: &str,
    drill_id: &str,
) -> Result<Drill, String> {
    let preset = presets
        .iter()
        .find(|p| p.id == preset_id)
        .ok_or_else(|| format!("preset '{preset_id}' not found"))?;
    let warm_up = preset
        .warm_up
        .as_ref()
        .ok_or_else(|| format!("preset '{preset_id}' has no warm-up"))?;
    let step = warm_up
        .steps
        .iter()
        .find(|s| s.drill == drill_id)
        .ok_or_else(|| {
            format!("drill '{drill_id}' not found in warm-up for preset '{preset_id}'")
        })?;
    let mut drill = preset
        .drills
        .iter()
        .find(|d| d.id == drill_id)
        .cloned()
        .ok_or_else(|| format!("drill '{drill_id}' not found in preset '{preset_id}'"))?;
    drill.reps = step.reps;
    Ok(drill)
}

/// Loads and validates all preset files from a directory.
///
/// Reads every `*.json` file in `dir` (non-recursively), sorted by file name.
/// Rejects duplicate preset IDs across files.
///
/// # Errors
///
/// Returns [`PresetError::Io`] if reading the directory fails.
/// Returns [`PresetError::InFile`] if reading, parsing, or validating any file fails,
/// or if duplicate preset IDs are found across files.
pub fn load_dir(dir: &Path) -> Result<Vec<Preset>, PresetError> {
    let read_dir = std::fs::read_dir(dir).map_err(|source| PresetError::Io {
        path: dir.to_path_buf(),
        source,
    })?;

    let mut paths = Vec::new();
    for entry_res in read_dir {
        let entry = entry_res.map_err(|source| PresetError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(std::ffi::OsStr::to_str) == Some("json") {
            paths.push(path);
        }
    }

    paths.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

    let mut presets = Vec::with_capacity(paths.len());
    let mut seen_ids = HashSet::new();

    for path in paths {
        let content = std::fs::read_to_string(&path).map_err(|source| PresetError::InFile {
            path: path.clone(),
            source: Box::new(PresetError::Io {
                path: path.clone(),
                source,
            }),
        })?;

        let preset = parse_preset(&content).map_err(|source| PresetError::InFile {
            path: path.clone(),
            source: Box::new(source),
        })?;

        if !seen_ids.insert(preset.id.clone()) {
            return Err(PresetError::InFile {
                path: path.clone(),
                source: Box::new(PresetError::Invalid {
                    drill: None,
                    message: format!("duplicate preset id '{}'", preset.id),
                }),
            });
        }

        presets.push(preset);
    }

    Ok(presets)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typo_in_drill_field_reports_unknown_field() {
        let json = r#"{"schemaVersion":1,"id":"p","name":"P","drills":[{"id":"d","name":"D","type":"hold","pedal":"brake","targt":70,"tolerance":5,"holdMs":2000}]}"#;
        let err = parse_preset(json).unwrap_err().to_string();
        assert_eq!(err, "drill 'd': unknown field 'targt'");
    }

    struct TempDirGuard(PathBuf);

    impl Drop for TempDirGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn valid_hold_drill() -> Drill {
        Drill {
            id: "brake-hold-70".to_string(),
            name: "Brake hold 70%".to_string(),
            pedal: Pedal::Brake,
            reps: 5,
            lead_in_ms: 3000,
            tolerance: Some(5.0),
            decimals: None,
            kind: DrillKind::Hold {
                target: 70.0,
                hold_ms: 2000,
            },
        }
    }

    fn valid_trace_drill() -> Drill {
        Drill {
            id: "hairpin".to_string(),
            name: "Hairpin trace".to_string(),
            pedal: Pedal::Brake,
            reps: 5,
            lead_in_ms: 2000,
            tolerance: Some(6.0),
            decimals: None,
            kind: DrillKind::Trace {
                points: vec![(0, 0.0), (150, 92.0), (600, 60.0), (1500, 0.0)],
            },
        }
    }

    fn valid_preset() -> Preset {
        Preset {
            schema_version: SCHEMA_VERSION,
            id: "gt3".to_string(),
            name: "GT3".to_string(),
            description: "Threshold braking and trail-off for GT3 cars.".to_string(),
            drills: vec![valid_hold_drill(), valid_trace_drill()],
            warm_up: None,
        }
    }

    #[test]
    fn valid_preset_roundtrip() {
        let preset = valid_preset();
        let json = serde_json::to_string_pretty(&preset).unwrap();
        let parsed = parse_preset(&json).expect("valid preset should parse successfully");
        assert_eq!(parsed, preset);
    }

    #[test]
    fn defaults_applied() {
        let json = r#"{
            "schemaVersion": 1,
            "id": "minimal",
            "name": "Minimal",
            "drills": [
                {
                    "id": "drill-1",
                    "name": "Hold",
                    "type": "hold",
                    "pedal": "brake",
                    "target": 50,
                    "tolerance": 5,
                    "holdMs": 1000
                }
            ]
        }"#;
        let parsed = parse_preset(json).expect("defaults should apply cleanly");
        assert_eq!(parsed.description, "");
        assert_eq!(parsed.drills[0].reps, 5);
        assert_eq!(parsed.drills[0].lead_in_ms, 3000);
    }

    #[test]
    fn unknown_field_rejected_at_top_level() {
        let json = r#"{
            "schemaVersion": 1,
            "id": "test",
            "name": "Test",
            "extraKey": "not-allowed",
            "drills": [
                {
                    "id": "drill-1",
                    "name": "Hold",
                    "type": "hold",
                    "pedal": "brake",
                    "target": 50,
                    "tolerance": 5,
                    "holdMs": 1000
                }
            ]
        }"#;
        let err = parse_preset(json).unwrap_err();
        assert!(
            matches!(err, PresetError::Json(_)),
            "expected Json error, got: {err:?}"
        );
    }

    #[test]
    fn unknown_field_rejected_inside_hold_drill() {
        let json = r#"{
            "schemaVersion": 1,
            "id": "test",
            "name": "Test",
            "drills": [
                {
                    "id": "drill-1",
                    "name": "Hold",
                    "type": "hold",
                    "pedal": "brake",
                    "target": 50,
                    "tolerance": 5,
                    "holdMs": 1000,
                    "extraKey": 123
                }
            ]
        }"#;
        let err = parse_preset(json).unwrap_err();
        assert!(matches!(
            &err,
            PresetError::Invalid {
                drill: Some(d),
                message
            } if d == "drill-1" && message.contains("unknown field 'extraKey'")
        ));
        assert!(err.to_string().contains("drill 'drill-1'"));
    }

    #[test]
    fn unknown_field_rejected_inside_trace_drill() {
        let json = r#"{
            "schemaVersion": 1,
            "id": "test",
            "name": "Test",
            "drills": [
                {
                    "id": "trace-1",
                    "name": "Trace",
                    "type": "trace",
                    "pedal": "brake",
                    "tolerance": 5,
                    "points": [[0, 0], [100, 50]],
                    "target": 50
                }
            ]
        }"#;
        let err = parse_preset(json).unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: Some(d),
                message
            } if d == "trace-1" && message.contains("unknown field 'target'")
        ));
    }

    #[test]
    fn wrong_schema_version() {
        let mut preset = valid_preset();
        preset.schema_version = 2;
        let json = serde_json::to_string(&preset).unwrap();
        let err = parse_preset(&json).unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: None,
                message
            } if message.contains("schemaVersion")
        ));
    }

    #[test]
    fn validation_preset_id() {
        let mut preset = valid_preset();
        preset.id = "GT3_Cars".to_string();
        let json = serde_json::to_string(&preset).unwrap();
        let err = parse_preset(&json).unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: None,
                message
            } if message.contains("field 'id'")
        ));

        preset.id = String::new();
        let json_empty = serde_json::to_string(&preset).unwrap();
        assert!(parse_preset(&json_empty).is_err());
    }

    #[test]
    fn validation_preset_name() {
        let mut preset = valid_preset();
        preset.name = String::new();
        let json = serde_json::to_string(&preset).unwrap();
        let err = parse_preset(&json).unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: None,
                message
            } if message.contains("field 'name'")
        ));
    }

    #[test]
    fn validation_preset_drills_empty() {
        let mut preset = valid_preset();
        preset.drills.clear();
        let json = serde_json::to_string(&preset).unwrap();
        let err = parse_preset(&json).unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: None,
                message
            } if message.contains("field 'drills'")
        ));
    }

    #[test]
    fn validation_drill_id() {
        let mut preset = valid_preset();
        preset.drills[0].id = "Bad_ID".to_string();
        let json = serde_json::to_string(&preset).unwrap();
        let err = parse_preset(&json).unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: Some(d),
                message
            } if d == "Bad_ID" && message.contains("field 'id'")
        ));
    }

    #[test]
    fn validation_drill_id_unique() {
        let mut preset = valid_preset();
        preset.drills[1].id = preset.drills[0].id.clone();
        let json = serde_json::to_string(&preset).unwrap();
        let err = parse_preset(&json).unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: Some(d),
                message
            } if d == "brake-hold-70" && message.contains("duplicate drill id")
        ));
    }

    #[test]
    fn validation_drill_name_empty() {
        let mut preset = valid_preset();
        preset.drills[0].name = String::new();
        let json = serde_json::to_string(&preset).unwrap();
        let err = parse_preset(&json).unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: Some(d),
                message
            } if d == "brake-hold-70" && message.contains("field 'name'")
        ));
    }

    #[test]
    fn validation_reps_range() {
        let mut preset = valid_preset();
        preset.drills[0].reps = 0;
        let json_zero = serde_json::to_string(&preset).unwrap();
        let err = parse_preset(&json_zero).unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: Some(d),
                message
            } if d == "brake-hold-70" && message.contains("field 'reps'")
        ));

        preset.drills[0].reps = 51;
        let json_too_high = serde_json::to_string(&preset).unwrap();
        assert!(parse_preset(&json_too_high).is_err());
    }

    #[test]
    fn validation_lead_in_range() {
        let mut preset = valid_preset();
        preset.drills[0].lead_in_ms = 10001;
        let json = serde_json::to_string(&preset).unwrap();
        let err = parse_preset(&json).unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: Some(d),
                message
            } if d == "brake-hold-70" && message.contains("field 'leadInMs' (10001) must be between 1000 and 10000")
        ));

        // GO shows for the last second of the countdown, so shorter lead-ins are rejected.
        preset.drills[0].lead_in_ms = 3000;
        for lead_in_ms in [0, 999] {
            preset.drills[1].lead_in_ms = lead_in_ms;
            let err = preset.validate().unwrap_err();
            assert!(matches!(
                err,
                PresetError::Invalid {
                    drill: Some(d),
                    message
                } if d == "hairpin" && message.contains("field 'leadInMs'")
            ));
        }
        for lead_in_ms in [1000, 10000] {
            preset.drills[1].lead_in_ms = lead_in_ms;
            assert!(preset.validate().is_ok(), "leadInMs {lead_in_ms} rejected");
        }
    }

    #[test]
    fn validation_tolerance_range() {
        let mut preset = valid_preset();
        preset.drills[0].tolerance = Some(0.4);
        let json_too_low = serde_json::to_string(&preset).unwrap();
        let err = parse_preset(&json_too_low).unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: Some(d),
                message
            } if d == "brake-hold-70" && message.contains("field 'tolerance'")
        ));

        preset.drills[0].tolerance = Some(50.1);
        let json_too_high = serde_json::to_string(&preset).unwrap();
        assert!(parse_preset(&json_too_high).is_err());

        preset.drills[0].tolerance = Some(f32::NAN);
        assert!(preset.validate().is_err());
    }

    #[test]
    fn tolerance_omitted_defaults_to_ten() {
        let json = r#"{
            "schemaVersion": 1,
            "id": "test-tol",
            "name": "Test Tol",
            "drills": [
                {
                    "id": "drill-1",
                    "name": "Hold",
                    "type": "hold",
                    "pedal": "brake",
                    "target": 50,
                    "holdMs": 1000
                }
            ]
        }"#;
        let parsed = parse_preset(json).expect("omitted tolerance should parse");
        assert_eq!(parsed.drills[0].tolerance, None);
        assert!((parsed.drills[0].tolerance_fraction() - 0.10).abs() < 1e-6);
    }

    #[test]
    fn validation_hold_target_range() {
        let mut preset = valid_preset();
        preset.drills[0].kind = DrillKind::Hold {
            target: -1.0,
            hold_ms: 2000,
        };
        let json_neg = serde_json::to_string(&preset).unwrap();
        let err = parse_preset(&json_neg).unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: Some(d),
                message
            } if d == "brake-hold-70" && message.contains("field 'target'")
        ));

        preset.drills[0].kind = DrillKind::Hold {
            target: 100.5,
            hold_ms: 2000,
        };
        let json_too_high = serde_json::to_string(&preset).unwrap();
        assert!(parse_preset(&json_too_high).is_err());
    }

    #[test]
    fn validation_hold_ms_range() {
        let mut preset = valid_preset();
        preset.drills[0].kind = DrillKind::Hold {
            target: 70.0,
            hold_ms: 199,
        };
        let json_low = serde_json::to_string(&preset).unwrap();
        let err = parse_preset(&json_low).unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: Some(d),
                message
            } if d == "brake-hold-70" && message.contains("field 'holdMs'")
        ));

        preset.drills[0].kind = DrillKind::Hold {
            target: 70.0,
            hold_ms: 60001,
        };
        let json_high = serde_json::to_string(&preset).unwrap();
        assert!(parse_preset(&json_high).is_err());
    }

    #[test]
    fn validation_trace_points_count() {
        let mut preset = valid_preset();
        preset.drills[1].kind = DrillKind::Trace {
            points: vec![(0, 0.0)],
        };
        let json = serde_json::to_string(&preset).unwrap();
        let err = parse_preset(&json).unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: Some(d),
                message
            } if d == "hairpin" && message.contains("at least 2 points")
        ));
    }

    #[test]
    fn validation_trace_first_point_zero() {
        let mut preset = valid_preset();
        preset.drills[1].kind = DrillKind::Trace {
            points: vec![(10, 0.0), (1000, 50.0)],
        };
        let json = serde_json::to_string(&preset).unwrap();
        let err = parse_preset(&json).unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: Some(d),
                message
            } if d == "hairpin" && message.contains("first point timestamp must be 0")
        ));
    }

    #[test]
    fn validation_trace_timestamps_strictly_increasing() {
        let mut preset = valid_preset();
        preset.drills[1].kind = DrillKind::Trace {
            points: vec![(0, 0.0), (500, 50.0), (500, 80.0)],
        };
        let json_equal = serde_json::to_string(&preset).unwrap();
        let err = parse_preset(&json_equal).unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: Some(d),
                message
            } if d == "hairpin" && message.contains("strictly increasing")
        ));

        preset.drills[1].kind = DrillKind::Trace {
            points: vec![(0, 0.0), (500, 50.0), (400, 80.0)],
        };
        let json_desc = serde_json::to_string(&preset).unwrap();
        assert!(parse_preset(&json_desc).is_err());
    }

    #[test]
    fn validation_trace_values_range() {
        let mut preset = valid_preset();
        preset.drills[1].kind = DrillKind::Trace {
            points: vec![(0, 0.0), (1000, -0.5)],
        };
        let json_neg = serde_json::to_string(&preset).unwrap();
        let err = parse_preset(&json_neg).unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: Some(d),
                message
            } if d == "hairpin" && message.contains("value -0.5")
        ));

        preset.drills[1].kind = DrillKind::Trace {
            points: vec![(0, 0.0), (1000, 105.0)],
        };
        let json_high = serde_json::to_string(&preset).unwrap();
        assert!(parse_preset(&json_high).is_err());
    }

    #[test]
    fn validation_trace_duration_range() {
        let mut preset = valid_preset();
        preset.drills[1].kind = DrillKind::Trace {
            points: vec![(0, 0.0), (15001, 50.0)],
        };
        let json = serde_json::to_string(&preset).unwrap();
        let err = parse_preset(&json).unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: Some(d),
                message
            } if d == "hairpin" && message.contains("duration (15001 ms) must be between 500 and 15000 ms")
        ));

        preset.drills[1].kind = DrillKind::Trace {
            points: vec![(0, 0.0), (499, 50.0)],
        };
        let err = preset.validate().unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid { message, .. } if message.contains("duration (499 ms)")
        ));

        for end in [500, 15000] {
            preset.drills[1].kind = DrillKind::Trace {
                points: vec![(0, 0.0), (end, 50.0)],
            };
            assert!(preset.validate().is_ok(), "duration {end} rejected");
        }
    }

    #[test]
    fn validation_trace_points_max() {
        let mut preset = valid_preset();
        let points = |n: u32| (0..n).map(|i| (i * 100, 50.0)).collect::<Vec<_>>();
        preset.drills[1].kind = DrillKind::Trace { points: points(64) };
        assert!(preset.validate().is_ok(), "64 points rejected");

        preset.drills[1].kind = DrillKind::Trace { points: points(65) };
        let err = preset.validate().unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: Some(d),
                message
            } if d == "hairpin" && message.contains("at most 64 points, found 65")
        ));
    }

    #[test]
    fn error_messages_contain_drill_id() {
        let mut preset = valid_preset();
        preset.drills[0].tolerance = Some(0.1);
        let json = serde_json::to_string(&preset).unwrap();
        let err = parse_preset(&json).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("drill 'brake-hold-70'"),
            "Display should include drill id: {msg}"
        );
    }

    #[test]
    fn trace_curve_value_at() {
        let points = [(0, 0.0), (150, 92.0), (600, 60.0), (1500, 0.0)];
        let curve = TraceCurve::from_points(&points);

        assert_eq!(curve.duration_ms(), 1500);

        // At points
        assert!((curve.value_at(0.0) - 0.0).abs() < 1e-6);
        assert!((curve.value_at(150.0) - 0.92).abs() < 1e-6);
        assert!((curve.value_at(600.0) - 0.60).abs() < 1e-6);
        assert!((curve.value_at(1500.0) - 0.0).abs() < 1e-6);

        // Between points: midpoint of (0, 0.0) and (150, 0.92) is t=75 -> 0.46
        assert!((curve.value_at(75.0) - 0.46).abs() < 1e-6);

        // Midpoint of (150, 0.92) and (600, 0.60) is t=375: the rounded curve sits above the
        // 0.76 chord because the slope at 150 is zero and steepest near 600.
        assert!((curve.value_at(375.0) - 0.798_75).abs() < 1e-4);

        // Before 0 (clamped to first value)
        assert!((curve.value_at(-100.0) - 0.0).abs() < 1e-6);

        // After the end (clamped to last value)
        assert!((curve.value_at(2000.0) - 0.0).abs() < 1e-6);
    }

    fn hairpin_reference() -> TraceCurve {
        TraceCurve::from_points(&[
            (0, 0.0),
            (150, 100.0),
            (300, 95.0),
            (600, 70.0),
            (1000, 40.0),
            (1500, 0.0),
        ])
    }

    fn second_reference() -> TraceCurve {
        TraceCurve::from_points(&[
            (0, 0.0),
            (880, 79.0),
            (1930, 63.3),
            (2790, 5.0),
            (3700, 0.0),
        ])
    }

    /// The scoring timing window (`trace_scoring::RAMP_WINDOW_MS`).
    const RAMP_WINDOW: u32 = crate::trace_scoring::RAMP_WINDOW_MS;

    fn assert_close(actual: f32, expected: f32, what: &str) {
        assert!(
            (actual - expected).abs() < 1e-5,
            "{what}: got {actual}, expected {expected}"
        );
    }

    #[test]
    fn value_at_matches_monotone_cubic_reference() {
        let hairpin = hairpin_reference();
        for (t, v) in [
            (0.0, 0.0),
            (75.0, 0.5),
            (150.0, 1.0),
            (225.0, 0.985_938),
            (450.0, 0.832_812),
            (800.0, 0.549_167),
            (1250.0, 0.151_562),
            (1500.0, 0.0),
        ] {
            assert_close(hairpin.value_at(t), v, &format!("hairpin t={t}"));
        }
        for t in 0..=1500 {
            let v = hairpin.value_at(f64::from(t));
            assert!((0.0..=1.0).contains(&v), "hairpin t={t} left [0, 1]: {v}");
        }

        let second = second_reference();
        for (t, v) in [
            (440.0, 0.395),
            (880.0, 0.79),
            (1400.0, 0.7664),
            (1930.0, 0.633),
            (2360.0, 0.314_745),
            (3245.0, 0.006_25),
        ] {
            assert_close(second.value_at(t), v, &format!("second t={t}"));
        }
    }

    #[test]
    fn envelope_at_matches_reference() {
        let hairpin = hairpin_reference();
        for (t, lo, hi) in [
            (225.0, 0.5, 1.0),
            (450.0, 0.7, 0.95),
            (800.0, 0.438_229, 0.661_042),
            (1250.0, 0.029_2, 0.308_8),
            (1500.0, 0.0, 0.061_988),
        ] {
            let (l, h) = hairpin.envelope_at(t, 150);
            assert_close(l, lo, &format!("hairpin lo t={t}"));
            assert_close(h, hi, &format!("hairpin hi t={t}"));
        }

        let second = second_reference();
        for (t, lo, hi) in [
            (440.0, 0.200_836, 0.589_164),
            (880.0, 0.728_965, 0.79),
            (1400.0, 0.743_817, 0.780_187),
            (1930.0, 0.547_242, 0.687_29),
            (3245.0, 0.001_883, 0.014_693),
        ] {
            let (l, h) = second.envelope_at(t, 150);
            assert_close(l, lo, &format!("second lo t={t}"));
            assert_close(h, hi, &format!("second hi t={t}"));
        }
    }

    #[test]
    fn envelope_table_equals_envelope_at() {
        for curve in [hairpin_reference(), second_reference()] {
            let table = curve.envelope_table(RAMP_WINDOW);
            let end = i32::try_from(curve.duration_ms()).unwrap() + 400;
            for t in -400..=end {
                let t = f64::from(t);
                assert_eq!(table.at(t), curve.envelope_at(t, RAMP_WINDOW), "t={t}");
            }
            // Between grid points the lookup rounds to the nearest millisecond.
            assert_eq!(table.at(74.6), curve.envelope_at(75.0, RAMP_WINDOW));
        }
    }

    #[test]
    fn decimals_accepted_rejected_and_omitted() {
        let json = |decimals: &str| {
            format!(
                r#"{{"schemaVersion":1,"id":"p","name":"P","drills":[{{"id":"d","name":"D","type":"hold","pedal":"brake","target":70,"holdMs":2000{decimals}}}]}}"#
            )
        };
        assert_eq!(parse_preset(&json("")).unwrap().drills[0].decimals, None);
        assert_eq!(
            parse_preset(&json(r#","decimals":0"#)).unwrap().drills[0].decimals,
            Some(0)
        );
        assert_eq!(
            parse_preset(&json(r#","decimals":1"#)).unwrap().drills[0].decimals,
            Some(1)
        );

        let err = parse_preset(&json(r#","decimals":2"#)).unwrap_err();
        assert!(matches!(
            &err,
            PresetError::Invalid {
                drill: Some(d),
                message
            } if d == "d" && message.contains("field 'decimals'")
        ));

        let trace = r#"{"schemaVersion":1,"id":"p","name":"P","drills":[{"id":"t","name":"T","type":"trace","pedal":"brake","decimals":1,"points":[[0,0],[1000,50]]}]}"#;
        assert_eq!(parse_preset(trace).unwrap().drills[0].decimals, Some(1));
    }

    #[test]
    fn drill_helpers() {
        let hold = valid_hold_drill();
        assert_eq!(hold.target_fraction(), Some(0.70));
        assert!((hold.tolerance_fraction() - 0.05).abs() < 1e-6);
        assert_eq!(hold.trace_curve(), None);

        let trace = valid_trace_drill();
        assert_eq!(trace.target_fraction(), None);
        assert!((trace.tolerance_fraction() - 0.06).abs() < 1e-6);
        let curve = trace.trace_curve().unwrap();
        assert_eq!(curve.duration_ms(), 1500);
    }

    #[test]
    fn sample_json_file_is_valid() {
        let json = include_str!("../../../presets/sample.json");
        let preset = parse_preset(json).expect("presets/sample.json must be valid");
        assert_eq!(preset.id, "sample");
        assert_eq!(preset.drills.len(), 4);
        assert_eq!(preset.drills[0].pedal, Pedal::Brake);
        assert_eq!(preset.drills[1].pedal, Pedal::Throttle);
        assert_eq!(preset.drills[2].pedal, Pedal::Brake);
        assert_eq!(preset.drills[3].id, "throttle-rolling-start-35");
        assert_eq!(preset.drills[3].pedal, Pedal::Throttle);
        assert!((preset.drills[3].tolerance_fraction() - 0.02).abs() < f32::EPSILON);
        assert_eq!(preset.drills[3].decimals, Some(1));
        match &preset.drills[3].kind {
            DrillKind::Hold { target, hold_ms } => {
                assert!((target - 35.0).abs() < 1e-6);
                assert_eq!(*hold_ms, 10000);
            }
            DrillKind::Trace { .. } => panic!("Expected DrillKind::Hold for drill 3"),
        }
    }

    #[test]
    fn load_dir_reads_and_validates() {
        let unique_name = format!(
            "sct_preset_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let dir = std::env::temp_dir().join(unique_name);
        std::fs::create_dir_all(&dir).unwrap();
        let _guard = TempDirGuard(dir.clone());

        let mut preset_a = valid_preset();
        preset_a.id = "alpha".to_string();
        let mut preset_b = valid_preset();
        preset_b.id = "beta".to_string();

        let path_b = dir.join("b.json");
        let path_a = dir.join("a.json");
        let path_txt = dir.join("notes.txt");

        std::fs::write(&path_b, serde_json::to_string(&preset_b).unwrap()).unwrap();
        std::fs::write(&path_a, serde_json::to_string(&preset_a).unwrap()).unwrap();
        std::fs::write(&path_txt, "not a json file").unwrap();

        let loaded = load_dir(&dir).expect("should load valid files");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].id, "alpha");
        assert_eq!(loaded[1].id, "beta");

        // Duplicate preset id across files
        let mut preset_c = valid_preset();
        preset_c.id = "alpha".to_string();
        let path_c = dir.join("c.json");
        std::fs::write(&path_c, serde_json::to_string(&preset_c).unwrap()).unwrap();

        let err = load_dir(&dir).unwrap_err();
        assert!(
            matches!(err, PresetError::InFile { source, .. } if matches!(
                source.as_ref(),
                PresetError::Invalid { drill: None, message } if message.contains("duplicate preset id 'alpha'")
            ))
        );
    }

    #[test]
    fn display_and_error_source() {
        use std::error::Error;

        let err_invalid_preset = PresetError::Invalid {
            drill: None,
            message: "invalid preset".to_string(),
        };
        assert_eq!(err_invalid_preset.to_string(), "invalid preset");
        assert!(err_invalid_preset.source().is_none());

        let err_invalid_drill = PresetError::Invalid {
            drill: Some("brake-1".to_string()),
            message: "bad target".to_string(),
        };
        assert_eq!(err_invalid_drill.to_string(), "drill 'brake-1': bad target");
        assert!(err_invalid_drill.source().is_none());

        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let err_io = PresetError::Io {
            path: PathBuf::from("foo.json"),
            source: io_err,
        };
        assert!(err_io.to_string().contains("failed to read 'foo.json'"));
        assert!(err_io.source().is_some());

        let err_in_file = PresetError::InFile {
            path: PathBuf::from("bar.json"),
            source: Box::new(err_invalid_drill),
        };
        assert!(err_in_file.to_string().contains("in 'bar.json'"));
        assert!(err_in_file.source().is_some());
    }

    #[test]
    fn find_drill_looks_up_by_preset_and_drill_id() {
        let preset = Preset {
            schema_version: SCHEMA_VERSION,
            id: "p".to_string(),
            name: "P".to_string(),
            description: String::new(),
            drills: vec![valid_hold_drill(), valid_trace_drill()],
            warm_up: None,
        };
        let presets = [preset];
        assert_eq!(
            find_drill(&presets, "p", "hairpin").unwrap(),
            valid_trace_drill()
        );
        assert_eq!(
            find_drill(&presets, "x", "hairpin").unwrap_err(),
            "preset 'x' not found"
        );
        assert_eq!(
            find_drill(&presets, "p", "nope").unwrap_err(),
            "drill 'nope' not found in preset 'p'"
        );
    }

    #[test]
    fn tolerance_defaults_to_ten_when_omitted() {
        let json = r#"{"schemaVersion":1,"id":"p","name":"P","drills":[{"id":"d","name":"D","type":"hold","pedal":"brake","target":70,"holdMs":2000}]}"#;
        let preset = parse_preset(json).unwrap();
        assert_eq!(preset.drills[0].tolerance, None);
        assert!((preset.drills[0].tolerance_fraction() - 0.10).abs() < f32::EPSILON);
    }

    #[test]
    fn drill_set_ms_hold_and_trace() {
        let hold = valid_hold_drill(); // hold_ms = 2000, lead_in_ms = 3000
        assert_eq!(hold.rep_ms(), 2000);
        assert_eq!(hold.set_ms(0), 3000);
        assert_eq!(hold.set_ms(1), 3000 + 2000);
        // reps 5: 3000 + 5 * 2000 + 4 * 2000 = 21000
        assert_eq!(hold.set_ms(5), 21000);

        let trace = valid_trace_drill(); // duration = 1500, lead_in_ms = 2000
        assert_eq!(trace.rep_ms(), 1500);
        assert_eq!(trace.set_ms(0), 2000);
        assert_eq!(trace.set_ms(1), 2000 + 1500 + 300);
        // reps 4: 2000 + 4 * 1500 + 3 * 2000 + 4 * 300 = 15200
        assert_eq!(trace.set_ms(4), 15200);
    }

    #[test]
    fn preset_without_warm_up_parses_with_none() {
        let json = r#"{
            "schemaVersion": 1,
            "id": "minimal",
            "name": "Minimal",
            "drills": [
                {
                    "id": "drill-1",
                    "name": "Hold",
                    "type": "hold",
                    "pedal": "brake",
                    "target": 50,
                    "tolerance": 5,
                    "holdMs": 1000
                }
            ]
        }"#;
        let preset = parse_preset(json).expect("should parse preset without warmUp");
        assert_eq!(preset.warm_up, None);
        assert_eq!(preset.warm_up_estimate_ms(), None);
    }

    #[test]
    fn valid_warm_up_parses_and_estimate_matches() {
        let json = r#"{
            "schemaVersion": 1,
            "id": "warmup-test",
            "name": "Warm-up Test",
            "drills": [
                {
                    "id": "brake-hold-70",
                    "name": "Brake hold 70%",
                    "type": "hold",
                    "pedal": "brake",
                    "target": 70,
                    "tolerance": 5,
                    "holdMs": 2000,
                    "leadInMs": 3000
                },
                {
                    "id": "hairpin",
                    "name": "Hairpin trace",
                    "type": "trace",
                    "pedal": "brake",
                    "tolerance": 6,
                    "leadInMs": 2000,
                    "points": [[0, 0], [150, 92], [600, 60], [1500, 0]]
                }
            ],
            "warmUp": {
                "steps": [
                    { "drill": "brake-hold-70", "reps": 20 },
                    { "drill": "hairpin", "reps": 30 }
                ]
            }
        }"#;
        let preset = parse_preset(json).expect("valid warm-up should parse");
        assert!(preset.warm_up.is_some());
        // Hand-computed:
        // brake-hold-70: 3000 + 20 * 2000 + 19 * 2000 = 81_000 ms
        // hairpin: 2000 + 30 * 1500 + 29 * 2000 + 30 * 300 = 114_000 ms
        // Total: 81_000 + 114_000 = 195_000 ms
        assert_eq!(preset.warm_up_estimate_ms(), Some(195_000));
    }

    #[test]
    fn warm_up_validation_rule_1_empty_steps() {
        let mut preset = valid_preset();
        preset.warm_up = Some(WarmUp { steps: vec![] });
        let err = preset.validate().unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: None,
                ref message
            } if message.starts_with("warmUp: ") && message.contains("steps must be non-empty")
        ));
    }

    #[test]
    fn warm_up_validation_rule_2_drill_not_found() {
        let mut preset = valid_preset();
        preset.warm_up = Some(WarmUp {
            steps: vec![WarmUpStep {
                drill: "nonexistent-drill".to_string(),
                reps: 10,
            }],
        });
        let err = preset.validate().unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: None,
                ref message
            } if message.starts_with("warmUp: ") && message.contains("drill 'nonexistent-drill' not found in preset")
        ));
    }

    #[test]
    fn warm_up_validation_rule_3_duplicate_drill() {
        let mut preset = valid_preset();
        preset.warm_up = Some(WarmUp {
            steps: vec![
                WarmUpStep {
                    drill: "brake-hold-70".to_string(),
                    reps: 10,
                },
                WarmUpStep {
                    drill: "brake-hold-70".to_string(),
                    reps: 10,
                },
            ],
        });
        let err = preset.validate().unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: None,
                ref message
            } if message.starts_with("warmUp: ") && message.contains("duplicate drill 'brake-hold-70'")
        ));
    }

    #[test]
    fn warm_up_validation_rule_4_reps_range() {
        let mut preset = valid_preset();
        preset.warm_up = Some(WarmUp {
            steps: vec![WarmUpStep {
                drill: "brake-hold-70".to_string(),
                reps: 0,
            }],
        });
        let err = preset.validate().unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: None,
                ref message
            } if message.starts_with("warmUp: ") && message.contains("reps (0) must be between 1 and 50")
        ));

        preset.warm_up = Some(WarmUp {
            steps: vec![WarmUpStep {
                drill: "brake-hold-70".to_string(),
                reps: 51,
            }],
        });
        let err51 = preset.validate().unwrap_err();
        assert!(matches!(
            err51,
            PresetError::Invalid {
                drill: None,
                ref message
            } if message.starts_with("warmUp: ") && message.contains("reps (51) must be between 1 and 50")
        ));
    }

    #[test]
    fn warm_up_validation_rule_5_clutch_drill_rejected() {
        let mut preset = valid_preset();
        let mut clutch_drill = valid_hold_drill();
        clutch_drill.id = "clutch-hold".to_string();
        clutch_drill.pedal = Pedal::Clutch;
        preset.drills.push(clutch_drill);
        preset.warm_up = Some(WarmUp {
            steps: vec![WarmUpStep {
                drill: "clutch-hold".to_string(),
                reps: 10,
            }],
        });
        let err = preset.validate().unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: None,
                ref message
            } if message.starts_with("warmUp: ") && message.contains("targets clutch pedal")
        ));
    }

    #[test]
    fn warm_up_validation_rule_6_estimate_range() {
        // Too short (< 180s = 180_000 ms)
        let mut preset = valid_preset();
        preset.warm_up = Some(WarmUp {
            steps: vec![WarmUpStep {
                drill: "brake-hold-70".to_string(),
                reps: 1,
            }],
        });
        let err = preset.validate().unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: None,
                ref message
            } if message.starts_with("warmUp: ") && message.contains("estimated duration") && message.contains("180s and 300s")
        ));

        // Too long (> 300s = 300_000 ms)
        preset.warm_up = Some(WarmUp {
            steps: vec![
                WarmUpStep {
                    drill: "brake-hold-70".to_string(),
                    reps: 50,
                },
                WarmUpStep {
                    drill: "hairpin".to_string(),
                    reps: 50,
                },
            ],
        });
        let err_long = preset.validate().unwrap_err();
        assert!(matches!(
            err_long,
            PresetError::Invalid {
                drill: None,
                ref message
            } if message.starts_with("warmUp: ") && message.contains("estimated duration") && message.contains("180s and 300s")
        ));
    }

    /// A preset whose only drill is a brake hold with a 1000 ms lead-in, and whose warm-up
    /// runs that drill `reps` times.
    fn single_hold_warm_up(hold_ms: u32, reps: u32) -> Preset {
        let mut preset = valid_preset();
        preset.drills = vec![Drill {
            lead_in_ms: 1000,
            kind: DrillKind::Hold {
                target: 70.0,
                hold_ms,
            },
            ..valid_hold_drill()
        }];
        preset.warm_up = Some(WarmUp {
            steps: vec![WarmUpStep {
                drill: "brake-hold-70".to_string(),
                reps,
            }],
        });
        preset
    }

    #[test]
    fn warm_up_estimate_boundaries_are_inclusive() {
        // Estimate with lead-in 1000 ms: 1000 + reps * (hold_ms + 2000) - 2000.
        let cases = [
            // (reps, hold_ms, estimate_ms, valid)
            (9, 18_111, 179_999, false),
            (4, 43_250, 180_000, true),
            (5, 58_200, 300_000, true),
            (23, 11_087, 300_001, false),
        ];
        for (reps, hold_ms, estimate_ms, valid) in cases {
            let preset = single_hold_warm_up(hold_ms, reps);
            assert_eq!(preset.warm_up_estimate_ms(), Some(estimate_ms));
            let result = preset.validate();
            assert_eq!(result.is_ok(), valid, "estimate {estimate_ms} ms");
            if !valid {
                assert!(matches!(
                    result,
                    Err(PresetError::Invalid { drill: None, ref message })
                        if message.contains("estimated duration")
                ));
            }
        }
    }

    #[test]
    fn warm_up_step_unknown_key_rejected() {
        let json = r#"{
            "schemaVersion": 1,
            "id": "warmup-test",
            "name": "Warm-up Test",
            "drills": [
                {
                    "id": "brake-hold-70",
                    "name": "Brake hold 70%",
                    "type": "hold",
                    "pedal": "brake",
                    "target": 70,
                    "tolerance": 5,
                    "holdMs": 2000
                }
            ],
            "warmUp": {
                "steps": [
                    {
                        "drill": "brake-hold-70",
                        "reps": 8,
                        "extraKey": "not allowed"
                    }
                ]
            }
        }"#;
        let err = parse_preset(json).unwrap_err();
        assert!(matches!(err, PresetError::Json(_)));
        assert!(err.to_string().contains("extraKey"));
    }

    #[test]
    fn find_warm_up_drill_overrides_reps_and_errors() {
        let mut preset = valid_preset();
        preset.warm_up = Some(WarmUp {
            steps: vec![WarmUpStep {
                drill: "brake-hold-70".to_string(),
                reps: 18,
            }],
        });
        let presets = [preset];

        // Found with overridden reps
        let drill = find_warm_up_drill(&presets, "gt3", "brake-hold-70").unwrap();
        assert_eq!(drill.reps, 18);
        assert_eq!(drill.id, "brake-hold-70");

        // Drill outside warm-up
        let err_outside = find_warm_up_drill(&presets, "gt3", "hairpin").unwrap_err();
        assert!(err_outside.contains("drill 'hairpin' not found in warm-up"));

        // Preset without warm-up
        let unconfigured_preset = valid_preset();
        let unconfigured_list = [unconfigured_preset];
        let err_no_wu = find_warm_up_drill(&unconfigured_list, "gt3", "brake-hold-70").unwrap_err();
        assert!(err_no_wu.contains("preset 'gt3' has no warm-up"));

        // Missing preset
        let err_missing =
            find_warm_up_drill(&presets, "unknown-preset", "brake-hold-70").unwrap_err();
        assert!(err_missing.contains("preset 'unknown-preset' not found"));
    }
}
