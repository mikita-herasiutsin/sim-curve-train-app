//! Scoring model for "trace" drill repetitions on full-rate pedal samples.
//!
//! A trace drill presents the driver with a target pedal curve that varies over time
//! (e.g. initial threshold braking followed by a smooth trail-off into the apex).
//! The driver attempts to follow this target curve as closely and smoothly as possible.
//!
//! # Scoring Model
//!
//! Repetition scoring evaluates three distinct pillars, combining them into a total
//! score in `0.0..=100.0` and assigning a letter [`Grade`]:
//!
//! The target is the rounded curve of [`TraceCurve::value_at`] (monotone cubic through the
//! preset points), the same curve the screen draws.
//!
//! - **Accuracy (50% weight):** Measures pedal position against the band the screen and the
//!   audio cue show, at the same instant and with no lag shift, so the result matches what
//!   the driver saw and heard. At each grid point the target band is the range of target
//!   values within ±[`RAMP_WINDOW_MS`] (150 ms), widened by the tolerance: a timing window
//!   that widens the band on ramps and leaves flat parts at ±tolerance. A reaction up to
//!   150 ms late or early therefore stays in the band; a later one leaves it. Accuracy
//!   combines the fraction of resampled duration spent within that band
//!   ([`ACCURACY_BAND_WEIGHT`], 50%) and an exponential decay on the root-mean-square
//!   distance from the windowed target range ([`ACCURACY_RMSE_WEIGHT`], 50%, scaled by
//!   [`ACCURACY_RMSE_SCALE`]). The estimated lag feeds only the timing score.
//! - **Timing (25% weight):** Evaluates reaction latency via zero-mean normalized (Pearson)
//!   cross-correlation over [`LAG_SEARCH_WINDOW_MS`] (±300 ms) with parabolic sub-sample
//!   peak interpolation. An asymmetric timing curve provides a dead zone
//!   (`-30 ms..=+10 ms`) for excellent reactions, with forgiving decay for early inputs
//!   ([`TIMING_HALF_EARLY_MS`], 140 ms half-point) and stricter decay for late reactions
//!   ([`TIMING_HALF_LATE_MS`], 80 ms half-point).
//! - **Smoothness (25% weight):** Evaluates pedal control quality without penalizing
//!   intentional corners in the target curve. Both user and target curves are filtered
//!   with a zero-phase 4th-order Butterworth low-pass filter at [`FILTER_CUTOFF_HZ`] (12 Hz).
//!   Jerk is computed via three successive central differences and evaluated using
//!   Log Dimensionless Jerk (LDLJ) over the release window. Jerk smoothness is scored
//!   relative to the target curve's intrinsic roughness ([`SMOOTHNESS_JERK_WEIGHT`], 70%),
//!   so following the target exactly earns full marks. Excess peak pedal pressure
//!   is penalized via exponential decay ([`SMOOTHNESS_OVERSHOOT_WEIGHT`], 30%,
//!   scaled by [`SMOOTHNESS_OVERSHOOT_SCALE`]).

use serde::Serialize;

use crate::dsp::{butterworth_lowpass_filtfilt, central_difference};
use crate::preset::TraceCurve;
use crate::scoring::{Grade, ValueSample};

/// Resampling grid rate in Hz (1 kHz grid).
pub const RESAMPLE_RATE_HZ: f64 = 1000.0;

/// Discrete time step in seconds (1 ms = 0.001 s) for differentiation.
pub const DT_SECONDS: f64 = 0.001;

/// Maximum lag search offset in milliseconds (±300 ms).
pub const LAG_SEARCH_WINDOW_MS: i32 = 300;

/// Timing window in milliseconds (±150 ms): a trace sample counts as in the band if it is
/// within the tolerance of any target value within ±150 ms. This widens the band on ramps
/// and leaves flat parts at ±tolerance.
pub const RAMP_WINDOW_MS: u32 = 150;

/// Early boundary of the timing dead zone in milliseconds (-30 ms).
pub const TIMING_DEADZONE_EARLY_MS: f32 = 30.0;

/// Late boundary of the timing dead zone in milliseconds (+10 ms).
pub const TIMING_DEADZONE_LATE_MS: f32 = 10.0;

/// Half-score characteristic latency when reacting late (80 ms).
pub const TIMING_HALF_LATE_MS: f32 = 80.0;

/// Half-score characteristic latency when reacting early (140 ms).
pub const TIMING_HALF_EARLY_MS: f32 = 140.0;

/// Characteristic scale (0.06 = 6%) for exponential decay of RMSE accuracy.
pub const ACCURACY_RMSE_SCALE: f32 = 0.06;

/// Cutoff frequency (12 Hz) for the Butterworth low-pass filter used in smoothness evaluation.
pub const FILTER_CUTOFF_HZ: f64 = 12.0;

/// Minimum drill duration in milliseconds (50 ms) required for valid scoring.
pub const MIN_DURATION_MS: u32 = 50;

/// Minimum sample count (50 samples) for the release window fallback.
pub const MIN_RELEASE_WINDOW_SAMPLES: usize = 50;

/// Standard deviation threshold below which a target curve is considered flat.
pub const FLAT_TARGET_STD_THRESHOLD: f64 = 0.01;

/// Normalized target value threshold (1%) marking the completion of the release window.
pub const RELEASE_END_THRESHOLD: f32 = 0.01;

/// Scaling denominator (6.0) for excess LDLJ roughness in smoothness scoring.
pub const SMOOTHNESS_JERK_EXCESS_SCALE: f32 = 6.0;

/// Characteristic scale (0.05 = 5%) for exponential decay of overshoot penalty.
pub const SMOOTHNESS_OVERSHOOT_SCALE: f32 = 0.05;

/// Weight of the in-band time fraction in the accuracy score (50 points).
pub const ACCURACY_BAND_WEIGHT: f32 = 50.0;

/// Multiplier for the exponential RMSE score in accuracy (0.5).
pub const ACCURACY_RMSE_WEIGHT: f32 = 0.5;

/// Weight of relative jerk smoothness in the smoothness score (70%).
pub const SMOOTHNESS_JERK_WEIGHT: f32 = 0.7;

/// Weight of overshoot suppression in the smoothness score (30%).
pub const SMOOTHNESS_OVERSHOOT_WEIGHT: f32 = 0.3;

/// Weight of the accuracy score in the total score (50%).
pub const ACCURACY_TOTAL_WEIGHT: f32 = 0.5;

/// Weight of the timing score in the total score (25%).
pub const TIMING_TOTAL_WEIGHT: f32 = 0.25;

/// Weight of the smoothness score in the total score (25%).
pub const SMOOTHNESS_TOTAL_WEIGHT: f32 = 0.25;

/// Parameters configuring trace drill repetition scoring.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TraceParams<'a> {
    /// Reference target pedal curve.
    pub curve: &'a TraceCurve,
    /// Permissible symmetrical error tolerance expressed as a fraction (e.g. 0.06 for ±6%).
    pub tolerance: f32,
}

impl<'a> TraceParams<'a> {
    /// Creates a new parameter configuration for trace scoring.
    #[must_use]
    pub const fn new(curve: &'a TraceCurve, tolerance: f32) -> Self {
        Self { curve, tolerance }
    }
}

/// Detailed evaluation and score breakdown for a trace drill repetition.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceScore {
    /// Overall composite score in `0.0..=100.0`.
    pub total: f32,
    /// Letter grade awarded based on total score.
    pub grade: Grade,
    /// Accuracy sub-score in `0.0..=100.0`, measured against the band without a lag shift.
    pub accuracy: f32,
    /// Latency / timing sub-score in `0.0..=100.0`.
    pub timing: f32,
    /// Smoothness sub-score in `0.0..=100.0`.
    pub smoothness: f32,
    /// Estimated driver reaction lag in milliseconds (positive means late).
    pub lag_ms: f32,
    /// Fraction of drill duration spent inside the tolerance band (`0.0..=1.0`).
    pub time_in_band: f32,
    /// Root-mean-square distance from the windowed target range, without a lag shift.
    pub rmse: f32,
    /// Maximum overshoot beyond target maximum.
    pub overshoot: f32,
    /// Log dimensionless jerk of the user's filtered pedal trace.
    pub ldlj_user: f32,
    /// Log dimensionless jerk of the target curve.
    pub ldlj_target: f32,
}

/// Scores a trace drill repetition from raw timestamped pedal samples.
///
/// Returns `None` if fewer than 2 samples fall within `[start_us, start_us + duration_ms * 1000]`,
/// if the drill duration is less than 50 ms, or if parameters are invalid.
#[must_use]
pub fn score_trace(
    samples: &[ValueSample],
    start_us: u64,
    params: &TraceParams,
) -> Option<TraceScore> {
    if !params.tolerance.is_finite() || params.tolerance <= 0.0 {
        return None;
    }

    let d_ms = params.curve.duration_ms();
    if d_ms < MIN_DURATION_MS {
        return None;
    }

    let duration_us = u64::from(d_ms).checked_mul(1_000)?;
    let end_us = start_us.checked_add(duration_us)?;

    let samples_in_window = samples
        .iter()
        .filter(|s| s.t_us >= start_us && s.t_us <= end_us)
        .count();

    if samples_in_window < 2 {
        return None;
    }

    // Sort samples by monotonic timestamp for interpolation
    let mut sorted_samples = samples.to_vec();
    sorted_samples.sort_by_key(|s| s.t_us);

    // 1. Resample target y[0..=D] and extended user signal u[-300..=D+300]
    let (y, u) = resample_grid(&sorted_samples, start_us, d_ms, params.curve);

    // 2. Estimate latency lag via Pearson cross-correlation
    let lag_ms = estimate_lag(&y, &u);

    // 3. Timing score
    let timing = score_timing(lag_ms);

    // 4. Accuracy against the band as shown live (no lag shift)
    let (accuracy, time_in_band, rmse) = score_accuracy(&y, &u, params.tolerance);

    // 5. Smoothness (relative jerk and overshoot)
    let (smoothness, overshoot, ldlj_user, ldlj_target) = score_smoothness(&y, &u, d_ms);

    // 6. Composite total and Grade
    let total = (ACCURACY_TOTAL_WEIGHT * accuracy
        + TIMING_TOTAL_WEIGHT * timing
        + SMOOTHNESS_TOTAL_WEIGHT * smoothness)
        .clamp(0.0, 100.0);
    let grade = Grade::from_total(total);

    Some(TraceScore {
        total,
        grade,
        accuracy,
        timing,
        smoothness,
        lag_ms,
        time_in_band,
        rmse,
        overshoot,
        ldlj_user,
        ldlj_target,
    })
}

/// Linearly interpolates a sample at monotonic microsecond timestamp `t`.
fn interpolate_sample(samples: &[ValueSample], t: u64) -> f32 {
    let first = samples.first().expect("at least 2 samples guaranteed");
    let last = samples.last().expect("at least 2 samples guaranteed");

    if t <= first.t_us {
        return first.value;
    }
    if t >= last.t_us {
        return last.value;
    }

    let idx = samples.partition_point(|s| s.t_us <= t);
    let s0 = &samples[idx - 1];
    let s1 = &samples[idx];

    let dt = s1.t_us.saturating_sub(s0.t_us);
    if dt == 0 {
        return s0.value;
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "time delta in microseconds fits within f64"
    )]
    let frac = (t.saturating_sub(s0.t_us) as f64) / (dt as f64);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "interpolated fraction fits in f32"
    )]
    let val = (f64::from(s0.value) + frac * f64::from(s1.value - s0.value)) as f32;
    val
}

/// Resamples target `y[0..=D]` and user `u[-300..=D+300]` onto a 1 kHz grid.
fn resample_grid(
    samples: &[ValueSample],
    start_us: u64,
    d_ms: u32,
    curve: &TraceCurve,
) -> (Vec<f32>, Vec<f32>) {
    #[expect(
        clippy::cast_possible_wrap,
        reason = "drill duration fits comfortably in i32"
    )]
    let d_i32 = d_ms as i32;

    // Resample target y[0..=D]
    let y: Vec<f32> = (0..=d_ms).map(|n| curve.value_at(f64::from(n))).collect();

    // Resample user u[-300..=D+300]
    let min_n = -LAG_SEARCH_WINDOW_MS;
    let max_n = d_i32 + LAG_SEARCH_WINDOW_MS;
    #[expect(
        clippy::cast_sign_loss,
        reason = "max_n - min_n + 1 is always positive"
    )]
    let total_u_len = (max_n - min_n + 1) as usize;
    let mut u = Vec::with_capacity(total_u_len);

    for n in min_n..=max_n {
        let t = if n >= 0 {
            #[expect(clippy::cast_sign_loss, reason = "n is non-negative in this branch")]
            let offset_us = (n as u64) * 1_000;
            start_us.saturating_add(offset_us)
        } else {
            #[expect(clippy::cast_sign_loss, reason = "-n is positive for negative n")]
            let offset_us = ((-n) as u64) * 1_000;
            start_us.saturating_sub(offset_us)
        };

        u.push(interpolate_sample(samples, t));
    }

    (y, u)
}

/// Estimates reaction lag in milliseconds via zero-mean normalized (Pearson) cross-correlation.
fn estimate_lag(y: &[f32], u: &[f32]) -> f32 {
    let d = y.len() - 1;
    #[expect(clippy::cast_precision_loss, reason = "y length fits in f64")]
    let n_f64 = y.len() as f64;

    let sum_y: f64 = y.iter().map(|&val| f64::from(val)).sum();
    let mean_y = sum_y / n_f64;

    let var_y: f64 = y
        .iter()
        .map(|&val| {
            let diff = f64::from(val) - mean_y;
            diff * diff
        })
        .sum::<f64>()
        / n_f64;
    let std_y = var_y.sqrt();

    // Flat target: std(y) < 0.01 => lag = 0
    if std_y < FLAT_TARGET_STD_THRESHOLD {
        return 0.0;
    }

    let dy: Vec<f64> = y.iter().map(|&val| f64::from(val) - mean_y).collect();
    let norm_y: f64 = dy.iter().map(|&val| val * val).sum::<f64>().sqrt();

    let mut correlations = Vec::with_capacity(601);

    for k in -LAG_SEARCH_WINDOW_MS..=LAG_SEARCH_WINDOW_MS {
        // u slice for n in 0..=D shifted by k: index in u is (n + k) - (-300) = n + k + 300
        #[expect(
            clippy::cast_sign_loss,
            reason = "k + LAG_SEARCH_WINDOW_MS is in 0..=600"
        )]
        let u_start = (k + LAG_SEARCH_WINDOW_MS) as usize;
        let u_slice = &u[u_start..=u_start + d];

        let sum_u: f64 = u_slice.iter().map(|&val| f64::from(val)).sum();
        let mean_u = sum_u / n_f64;

        let mut dot = 0.0_f64;
        let mut norm_u_sq = 0.0_f64;

        for (&dy_val, &u_val) in dy.iter().zip(u_slice.iter()) {
            let du = f64::from(u_val) - mean_u;
            dot += dy_val * du;
            norm_u_sq += du * du;
        }

        let norm_u = norm_u_sq.sqrt();
        let r = if norm_u < 1e-12 || norm_y < 1e-12 {
            0.0
        } else {
            dot / (norm_y * norm_u)
        };

        correlations.push(r);
    }

    // Find argmax
    let mut best_idx = 0;
    let mut best_r = f64::NEG_INFINITY;
    for (i, &r) in correlations.iter().enumerate() {
        if r > best_r {
            best_r = r;
            best_idx = i;
        }
    }

    let best_idx_i32 = i32::try_from(best_idx).expect("best_idx in 0..=600 fits in i32");
    let k_best = best_idx_i32 - LAG_SEARCH_WINDOW_MS;

    // Skip parabolic interpolation at edges
    if best_idx == 0 || best_idx == correlations.len() - 1 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "integer lag fits precisely in f32"
        )]
        return k_best as f32;
    }

    // Parabolic interpolation with neighbours
    let y_minus = correlations[best_idx - 1];
    let y_0 = correlations[best_idx];
    let y_plus = correlations[best_idx + 1];

    let denom = 2.0 * (2.0 * y_0 - y_minus - y_plus);
    let delta = if denom.abs() < 1e-12 {
        0.0
    } else {
        (y_plus - y_minus) / denom
    };

    #[expect(
        clippy::cast_possible_truncation,
        reason = "interpolated lag fits in f32"
    )]
    let lag = (f64::from(k_best) + delta) as f32;
    lag
}

/// Evaluates asymmetric timing score from estimated reaction latency.
fn score_timing(lag_ms: f32) -> f32 {
    let (eff, half) = if lag_ms < -TIMING_DEADZONE_EARLY_MS {
        // Early reaction: eff = |lag_ms| - 30
        ((-lag_ms) - TIMING_DEADZONE_EARLY_MS, TIMING_HALF_EARLY_MS)
    } else if lag_ms > TIMING_DEADZONE_LATE_MS {
        // Late reaction: eff = lag_ms - 10
        (lag_ms - TIMING_DEADZONE_LATE_MS, TIMING_HALF_LATE_MS)
    } else {
        // Dead zone: -30 ms <= lag_ms <= 10 ms
        (0.0, 1.0)
    };

    if eff <= 0.0 {
        100.0
    } else {
        let ratio = eff / half;
        (100.0 / (1.0 + ratio * ratio)).clamp(0.0, 100.0)
    }
}

/// Range `[lo, hi]` of `y` over indices `n - RAMP_WINDOW_MS ..= n + RAMP_WINDOW_MS`
/// clamped to the grid, for every grid index `n` (the target envelope at integer ms).
fn target_envelope(y: &[f32]) -> Vec<(f32, f32)> {
    let window = usize::try_from(RAMP_WINDOW_MS).expect("RAMP_WINDOW_MS fits in usize");
    let last = y.len() - 1;
    (0..=last)
        .map(|n| {
            let span = &y[n.saturating_sub(window)..=(n + window).min(last)];
            span.iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &v| {
                    (lo.min(v), hi.max(v))
                })
        })
        .collect()
}

/// Evaluates accuracy against the timing-window envelope of the target, comparing `u` and
/// `y` at the same instant: the band the screen and the audio cue show.
fn score_accuracy(y: &[f32], u: &[f32], tolerance: f32) -> (f32, f32, f32) {
    let d = y.len() - 1;
    let mut in_band_count = 0_usize;
    let mut sum_sq_err = 0.0_f64;

    // u[LAG_SEARCH_WINDOW_MS] is t = 0.
    let u_zero_idx =
        usize::try_from(LAG_SEARCH_WINDOW_MS).expect("LAG_SEARCH_WINDOW_MS is positive");
    let envelope = target_envelope(y);

    for (&u_val, &(lo, hi)) in u[u_zero_idx..=u_zero_idx + d].iter().zip(&envelope) {
        if lo - tolerance <= u_val && u_val <= hi + tolerance {
            in_band_count += 1;
        }
        // Distance from the windowed target range (0 inside it).
        let err = f64::from((lo - u_val).max(u_val - hi).max(0.0));
        sum_sq_err += err * err;
    }

    #[expect(clippy::cast_precision_loss, reason = "y length fits in f32")]
    let total_points = (d + 1) as f32;
    #[expect(clippy::cast_precision_loss, reason = "count fits in f32")]
    let in_band_f32 = in_band_count as f32;
    let time_in_band = in_band_f32 / total_points;

    #[expect(clippy::cast_precision_loss, reason = "y length fits in f64")]
    let mean_sq_err = sum_sq_err / ((d + 1) as f64);
    #[expect(clippy::cast_possible_truncation, reason = "rmse fits in f32")]
    let rmse = mean_sq_err.sqrt() as f32;

    let s_rmse = 100.0 * (-rmse / ACCURACY_RMSE_SCALE).exp();
    let accuracy =
        (ACCURACY_BAND_WEIGHT * time_in_band + ACCURACY_RMSE_WEIGHT * s_rmse).clamp(0.0, 100.0);

    (accuracy, time_in_band, rmse)
}

/// Evaluates Log Dimensionless Jerk (LDLJ) over the specified window indices.
fn calculate_ldlj(filtered: &[f64], win_start: usize, win_end: usize) -> f32 {
    let v = central_difference(filtered, DT_SECONDS);
    let a = central_difference(&v, DT_SECONDS);
    let j = central_difference(&a, DT_SECONDS);

    let v_peak = v[win_start..=win_end]
        .iter()
        .fold(0.0_f64, |acc, &val| acc.max(val.abs()));

    if v_peak < 1e-6 {
        return 0.0;
    }

    #[expect(clippy::cast_precision_loss, reason = "sample count fits in f64")]
    let t_duration = (win_end.saturating_sub(win_start) as f64) * DT_SECONDS;
    if t_duration <= 0.0 {
        return 0.0;
    }

    let sum_j2_dt: f64 = j[win_start..=win_end]
        .iter()
        .map(|&val| val * val * DT_SECONDS)
        .sum();

    let dj = (t_duration.powi(3) / (v_peak * v_peak)) * sum_j2_dt;
    let ldlj = -(dj.max(1e-12)).ln();

    #[expect(clippy::cast_possible_truncation, reason = "ldlj fits in f32")]
    let ldlj_f32 = ldlj as f32;
    ldlj_f32
}

/// Evaluates smoothness including relative jerk and overshoot.
fn score_smoothness(y: &[f32], u: &[f32], d_ms: u32) -> (f32, f32, f32, f32) {
    let u_zero_idx =
        usize::try_from(LAG_SEARCH_WINDOW_MS).expect("LAG_SEARCH_WINDOW_MS is positive");
    let d_usize = usize::try_from(d_ms).expect("duration fits in usize");

    // Filter u[0..=D] (no shift) and y with butterworth low-pass at 12 Hz
    let u_slice = &u[u_zero_idx..=u_zero_idx + d_usize];
    let u_f64: Vec<f64> = u_slice.iter().map(|&val| f64::from(val)).collect();
    let y_f64: Vec<f64> = y.iter().map(|&val| f64::from(val)).collect();

    let u_filtered = butterworth_lowpass_filtfilt(&u_f64, FILTER_CUTOFF_HZ, RESAMPLE_RATE_HZ);
    let y_filtered = butterworth_lowpass_filtfilt(&y_f64, FILTER_CUTOFF_HZ, RESAMPLE_RATE_HZ);

    // Release window determination from target y
    let mut n_peak = 0;
    let mut max_y_val = f32::NEG_INFINITY;
    for (i, &val) in y.iter().enumerate() {
        if val > max_y_val {
            max_y_val = val;
            n_peak = i;
        }
    }

    let mut win_end = d_usize;
    for (i, &val) in y.iter().enumerate().skip(n_peak + 1) {
        if val <= RELEASE_END_THRESHOLD {
            win_end = i;
            break;
        }
    }

    let (win_start, win_end) = if win_end - n_peak + 1 < MIN_RELEASE_WINDOW_SAMPLES {
        (0, d_usize)
    } else {
        (n_peak, win_end)
    };

    let ldlj_user = calculate_ldlj(&u_filtered, win_start, win_end);
    let ldlj_target = calculate_ldlj(&y_filtered, win_start, win_end);

    // Relative jerk roughness score
    let excess_jerk = (ldlj_user.abs() - ldlj_target.abs()).max(0.0);
    let s_jerk = 100.0 * (1.0 - excess_jerk / SMOOTHNESS_JERK_EXCESS_SCALE).clamp(0.0, 1.0);

    // Overshoot penalty
    let max_u_filt = u_filtered
        .iter()
        .fold(f64::NEG_INFINITY, |acc, &val| acc.max(val));
    #[expect(
        clippy::cast_possible_truncation,
        reason = "max filtered user fits in f32"
    )]
    let max_u_f32 = max_u_filt as f32;
    let overshoot = (max_u_f32 - max_y_val).max(0.0);
    let s_os = 100.0 * (-overshoot / SMOOTHNESS_OVERSHOOT_SCALE).exp();

    let smoothness =
        (SMOOTHNESS_JERK_WEIGHT * s_jerk + SMOOTHNESS_OVERSHOOT_WEIGHT * s_os).clamp(0.0, 100.0);

    (smoothness, overshoot, ldlj_user, ldlj_target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn standard_curve() -> TraceCurve {
        TraceCurve::from_points(&[(0, 0.0), (150, 92.0), (600, 60.0), (1500, 0.0)])
    }

    fn standard_params(curve: &TraceCurve) -> TraceParams<'_> {
        TraceParams::new(curve, 0.06)
    }

    const START_US: u64 = 1_000_000;

    /// Generates synthetic 1 kHz user samples covering `start - 500 ms .. end + 500 ms`.
    fn generate_synthetic_samples<F>(
        curve: &TraceCurve,
        start_us: u64,
        delay_ms: f64,
        modify: F,
    ) -> Vec<ValueSample>
    where
        F: Fn(i32, f32) -> f32,
    {
        let d = curve.duration_ms();
        #[expect(clippy::cast_possible_wrap, reason = "duration fits in i32")]
        let d_i32 = d as i32;

        let mut samples = Vec::with_capacity((d + 1001) as usize);

        for n in -500..=d_i32 + 500 {
            let t_us = if n >= 0 {
                #[expect(clippy::cast_sign_loss, reason = "n is non-negative")]
                let offset = (n as u64) * 1_000;
                start_us + offset
            } else {
                #[expect(clippy::cast_sign_loss, reason = "-n is positive")]
                let offset = ((-n) as u64) * 1_000;
                start_us.saturating_sub(offset)
            };

            let target_t_ms = f64::from(n) - delay_ms;
            let val = curve.value_at(target_t_ms);
            let final_val = modify(n, val);

            samples.push(ValueSample::new(t_us, final_val));
        }

        samples
    }

    /// Wall time of `score_trace` for a 3.6 s and a `MAX_TRACE_MS` trace. Run with
    /// `cargo test --release -p sct-core score_trace_timing -- --ignored --nocapture`.
    #[test]
    #[ignore = "timing harness, run by hand in a release build"]
    fn score_trace_timing() {
        use std::time::Instant;
        for d in [3600_u32, crate::preset::MAX_TRACE_MS] {
            let curve = TraceCurve::from_points(&[
                (0, 0.0),
                (d / 20, 95.0),
                (d / 3, 70.0),
                (d * 2 / 3, 30.0),
                (d, 0.0),
            ]);
            let params = standard_params(&curve);
            #[expect(clippy::cast_precision_loss, reason = "test noise")]
            let samples = generate_synthetic_samples(&curve, START_US, 60.0, |n, v| {
                v + 0.01 * ((n as f32) * 0.37).sin()
            });
            let mut times: Vec<f64> = (0..50)
                .map(|_| {
                    let started = Instant::now();
                    let score = score_trace(&samples, START_US, &params);
                    let elapsed = started.elapsed().as_secs_f64() * 1e3;
                    assert!(score.is_some());
                    elapsed
                })
                .collect();
            times.sort_by(f64::total_cmp);
            println!(
                "{d} ms trace, {} samples: min {:.2} ms, median {:.2} ms, max {:.2} ms",
                samples.len(),
                times[0],
                times[25],
                times[49]
            );
        }
    }

    #[test]
    fn test_1_exact_copy() {
        let curve = standard_curve();
        let params = standard_params(&curve);
        let samples = generate_synthetic_samples(&curve, START_US, 0.0, |_n, val| val);

        let score = score_trace(&samples, START_US, &params).expect("score should exist");
        assert!(score.total >= 98.0, "total was {}", score.total);
        assert_eq!(score.grade, Grade::S);
        assert!(score.lag_ms.abs() < 1.0, "lag_ms was {}", score.lag_ms);
    }

    #[test]
    fn test_2_delay_100ms() {
        let curve = standard_curve();
        let params = standard_params(&curve);
        let exact_samples = generate_synthetic_samples(&curve, START_US, 0.0, |_n, val| val);
        let exact_score = score_trace(&exact_samples, START_US, &params).unwrap();

        let delayed_samples = generate_synthetic_samples(&curve, START_US, 100.0, |_n, val| val);
        let score = score_trace(&delayed_samples, START_US, &params).expect("score should exist");

        assert!(
            (score.lag_ms - 100.0).abs() <= 2.0,
            "lag_ms was {}",
            score.lag_ms
        );
        assert!(
            score.total < exact_score.total,
            "delayed total {} should be lower than exact {}",
            score.total,
            exact_score.total
        );
        assert!(score.accuracy >= 90.0, "accuracy was {}", score.accuracy);
    }

    #[test]
    fn test_3_delay_ordering() {
        let curve = standard_curve();
        let params = standard_params(&curve);

        let s50 = generate_synthetic_samples(&curve, START_US, 50.0, |_n, val| val);
        let s100 = generate_synthetic_samples(&curve, START_US, 100.0, |_n, val| val);
        let s200 = generate_synthetic_samples(&curve, START_US, 200.0, |_n, val| val);

        let score50 = score_trace(&s50, START_US, &params).unwrap();
        let score100 = score_trace(&s100, START_US, &params).unwrap();
        let score200 = score_trace(&s200, START_US, &params).unwrap();

        assert!(
            score50.total > score100.total,
            "score50 {} not > score100 {}",
            score50.total,
            score100.total
        );
        assert!(
            score100.total > score200.total,
            "score100 {} not > score200 {}",
            score100.total,
            score200.total
        );
    }

    #[test]
    fn test_4_asymmetric_timing_early_higher_than_late() {
        let curve = standard_curve();
        let params = standard_params(&curve);

        let early_samples = generate_synthetic_samples(&curve, START_US, -50.0, |_n, val| val);
        let late_samples = generate_synthetic_samples(&curve, START_US, 50.0, |_n, val| val);

        let early_score = score_trace(&early_samples, START_US, &params).unwrap();
        let late_score = score_trace(&late_samples, START_US, &params).unwrap();

        assert!(
            early_score.timing > late_score.timing,
            "early timing {} not > late timing {}",
            early_score.timing,
            late_score.timing
        );
        assert!(
            early_score.total > late_score.total,
            "early total {} not > late total {}",
            early_score.total,
            late_score.total
        );
    }

    #[test]
    fn test_5_noisy_exact_copy() {
        let curve = standard_curve();
        let params = standard_params(&curve);
        let exact_samples = generate_synthetic_samples(&curve, START_US, 0.0, |_n, val| val);
        let exact_score = score_trace(&exact_samples, START_US, &params).unwrap();

        let noisy_samples = generate_synthetic_samples(&curve, START_US, 0.0, |n, val| {
            let n_f64 = f64::from(n);
            #[expect(clippy::cast_possible_truncation, reason = "noise fits in f32")]
            let noise = (0.03 * (n_f64 * 1.7).sin() * (n_f64 * 0.37).sin()) as f32;
            val + noise
        });

        let noisy_score = score_trace(&noisy_samples, START_US, &params).unwrap();

        assert!(
            noisy_score.smoothness < exact_score.smoothness,
            "noisy smoothness {} not < exact {}",
            noisy_score.smoothness,
            exact_score.smoothness
        );
        assert!(
            noisy_score.total < exact_score.total,
            "noisy total {} not < exact {}",
            noisy_score.total,
            exact_score.total
        );
    }

    #[test]
    fn test_6_overshoot() {
        let curve = standard_curve();
        let params = standard_params(&curve);
        let exact_samples = generate_synthetic_samples(&curve, START_US, 0.0, |_n, val| val);
        let exact_score = score_trace(&exact_samples, START_US, &params).unwrap();

        let overshoot_samples = generate_synthetic_samples(&curve, START_US, 0.0, |n, val| {
            if (0..=300).contains(&n) {
                // Scale so 92% reaches 100%
                val * (1.00 / 0.92)
            } else {
                val
            }
        });

        let os_score = score_trace(&overshoot_samples, START_US, &params).unwrap();

        assert!(
            os_score.overshoot > 0.05,
            "overshoot was {}",
            os_score.overshoot
        );
        assert!(
            os_score.smoothness < exact_score.smoothness,
            "os smoothness {} not < exact {}",
            os_score.smoothness,
            exact_score.smoothness
        );
        assert!(
            os_score.total < exact_score.total,
            "os total {} not < exact {}",
            os_score.total,
            exact_score.total
        );
    }

    #[test]
    fn test_7_expected_order_of_totals() {
        let curve = standard_curve();
        let params = standard_params(&curve);

        let exact = score_trace(
            &generate_synthetic_samples(&curve, START_US, 0.0, |_n, val| val),
            START_US,
            &params,
        )
        .unwrap();

        let delay100 = score_trace(
            &generate_synthetic_samples(&curve, START_US, 100.0, |_n, val| val),
            START_US,
            &params,
        )
        .unwrap();

        let noisy = score_trace(
            &generate_synthetic_samples(&curve, START_US, 0.0, |n, val| {
                let n_f64 = f64::from(n);
                #[expect(clippy::cast_possible_truncation, reason = "noise fits in f32")]
                let noise = (0.03 * (n_f64 * 1.7).sin() * (n_f64 * 0.37).sin()) as f32;
                val + noise
            }),
            START_US,
            &params,
        )
        .unwrap();

        let overshoot = score_trace(
            &generate_synthetic_samples(&curve, START_US, 0.0, |n, val| {
                if (0..=300).contains(&n) {
                    val * (1.00 / 0.92)
                } else {
                    val
                }
            }),
            START_US,
            &params,
        )
        .unwrap();

        assert!(exact.total > delay100.total);
        assert!(exact.total > noisy.total);
        assert!(exact.total > overshoot.total);
    }

    #[test]
    fn test_8_constant_offset_gives_grade_c_or_d() {
        let curve = standard_curve();
        let params = standard_params(&curve);
        let offset_samples =
            generate_synthetic_samples(&curve, START_US, 0.0, |_n, val| val + 0.15);

        let score = score_trace(&offset_samples, START_US, &params).unwrap();
        assert!(
            score.grade == Grade::C || score.grade == Grade::D,
            "expected grade C or D, got {:?} (total {})",
            score.grade,
            score.total
        );
        assert!(score.total < 70.0, "total was {}", score.total);
    }

    #[test]
    fn test_9_flat_target() {
        let flat_curve = TraceCurve::from_points(&[(0, 50.0), (1000, 50.0)]);
        let params = standard_params(&flat_curve);
        let samples = generate_synthetic_samples(&flat_curve, START_US, 0.0, |_n, val| val);

        let score = score_trace(&samples, START_US, &params).unwrap();
        assert_eq!(score.lag_ms, 0.0);
        assert!(score.total >= 98.0, "flat target total was {}", score.total);
        assert_eq!(score.grade, Grade::S);
    }

    #[test]
    fn test_10_too_few_samples() {
        let curve = standard_curve();
        let params = standard_params(&curve);

        assert_eq!(score_trace(&[], START_US, &params), None);

        let one_sample = vec![ValueSample::new(START_US + 500_000, 0.5)];
        assert_eq!(score_trace(&one_sample, START_US, &params), None);

        let outside_samples = vec![ValueSample::new(100, 0.0), ValueSample::new(200, 0.0)];
        assert_eq!(score_trace(&outside_samples, START_US, &params), None);

        // Curve with duration < 50 ms
        let short_curve = TraceCurve::from_points(&[(0, 0.0), (40, 50.0)]);
        let short_params = standard_params(&short_curve);
        let samples = generate_synthetic_samples(&short_curve, START_US, 0.0, |_n, val| val);
        assert_eq!(score_trace(&samples, START_US, &short_params), None);
    }

    #[test]
    fn test_11_irregular_timestamps() {
        let curve = standard_curve();
        let params = standard_params(&curve);
        let exact_samples = generate_synthetic_samples(&curve, START_US, 0.0, |_n, val| val);
        let exact_score = score_trace(&exact_samples, START_US, &params).unwrap();

        let mut irregular_samples = Vec::new();
        let mut curr_t = START_US.saturating_sub(500_000);
        let end_limit = START_US + 2_000_000;
        let mut toggle = false;

        while curr_t <= end_limit {
            #[expect(clippy::cast_precision_loss, reason = "relative timestamp fits in f64")]
            let t_rel_ms = if curr_t >= START_US {
                ((curr_t - START_US) as f64) / 1000.0
            } else {
                -(((START_US - curr_t) as f64) / 1000.0)
            };
            let val = curve.value_at(t_rel_ms);
            irregular_samples.push(ValueSample::new(curr_t, val));

            let step = if toggle { 3_000 } else { 1_000 };
            toggle = !toggle;
            curr_t += step;
        }

        let irregular_score = score_trace(&irregular_samples, START_US, &params).unwrap();
        assert!(
            (irregular_score.total - exact_score.total).abs() <= 1.0,
            "diff was {}",
            (irregular_score.total - exact_score.total).abs()
        );
    }

    fn second_curve() -> TraceCurve {
        TraceCurve::from_points(&[
            (0, 0.0),
            (880, 79.0),
            (1930, 63.3),
            (2790, 5.0),
            (3700, 0.0),
        ])
    }

    #[test]
    fn test_13_late_ramp_stays_in_band_within_timing_window() {
        let target = second_curve();
        let tolerance = 0.06;
        let d = i32::try_from(target.duration_ms()).unwrap();
        // The grids score_trace builds: y[0..=D], and u[-300..=D+300] a copy 100 ms late.
        let y: Vec<f32> = (0..=d).map(|n| target.value_at(f64::from(n))).collect();
        let u: Vec<f32> = (-LAG_SEARCH_WINDOW_MS..=d + LAG_SEARCH_WINDOW_MS)
            .map(|n| target.value_at(f64::from(n) - 100.0))
            .collect();

        // Without the timing window, the late copy misses target ± tolerance on the ramps.
        let u_now = &u[usize::try_from(LAG_SEARCH_WINDOW_MS).unwrap()..];
        let near_target = y
            .iter()
            .zip(u_now)
            .filter(|&(&yv, &uv)| (uv - yv).abs() <= tolerance)
            .count();
        #[expect(clippy::cast_precision_loss, reason = "counts fit in f32")]
        let near_fraction = near_target as f32 / y.len() as f32;
        assert!(near_fraction < 0.9, "near_fraction was {near_fraction}");

        // With the ±150 ms window it never leaves the band, with no lag shift.
        let (accuracy, time_in_band, rmse) = score_accuracy(&y, &u, tolerance);
        assert!(
            (time_in_band - 1.0).abs() < f32::EPSILON,
            "time_in_band was {time_in_band}"
        );
        assert!(rmse.abs() < f32::EPSILON, "rmse was {rmse}");
        assert!((accuracy - 100.0).abs() < 1e-3, "accuracy was {accuracy}");
    }

    #[test]
    fn test_15_late_beyond_window_leaves_the_band() {
        // The sample preset's hairpin, followed exactly but 250 ms late: the screen and the
        // audio show the pedal outside the band for most of the rep, and so must the score.
        let preset = crate::preset::parse_preset(include_str!("../../../presets/sample.json"))
            .expect("presets/sample.json must be valid");
        let drill = preset
            .drills
            .iter()
            .find(|d| d.id == "hairpin")
            .expect("sample preset has the hairpin trace");
        let curve = drill.trace_curve().unwrap();
        let params = TraceParams::new(&curve, drill.tolerance_fraction());
        let samples = generate_synthetic_samples(&curve, START_US, 250.0, |_n, val| val);

        let score = score_trace(&samples, START_US, &params).unwrap();
        assert!(
            (score.lag_ms - 250.0).abs() <= 5.0,
            "lag_ms was {}",
            score.lag_ms
        );
        assert!(
            score.time_in_band < 0.5,
            "time_in_band was {}",
            score.time_in_band
        );
        assert!(score.accuracy < 90.0, "accuracy was {}", score.accuracy);
    }

    #[test]
    fn bundled_presets_score_a_perfect_follow_and_punish_a_late_one() {
        // Every drill in presets/ loads and scores: an exact follow earns an S, and a trace
        // followed 250 ms late loses at least 5 points.
        use crate::preset::DrillKind;
        use crate::scoring::{HoldParams, score_hold};

        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../presets");
        let presets = crate::preset::load_dir(&dir).expect("presets/ loads");
        let mut checked = Vec::new();
        for preset in &presets {
            for drill in &preset.drills {
                let who = format!("{} / {}", preset.id, drill.id);
                let (total, grade) = match &drill.kind {
                    DrillKind::Trace { .. } => {
                        let curve = drill.trace_curve().unwrap();
                        let params = TraceParams::new(&curve, drill.tolerance_fraction());
                        let exact =
                            generate_synthetic_samples(&curve, START_US, 0.0, |_n, val| val);
                        let score = score_trace(&exact, START_US, &params).unwrap();
                        assert!(score.lag_ms.abs() < 1.0, "{who}: lag was {}", score.lag_ms);
                        let late =
                            generate_synthetic_samples(&curve, START_US, 250.0, |_n, val| val);
                        let late = score_trace(&late, START_US, &params).unwrap();
                        assert!(
                            late.total <= score.total - 5.0,
                            "{who}: 250 ms late scored {} against {}",
                            late.total,
                            score.total
                        );
                        (score.total, score.grade)
                    }
                    DrillKind::Hold { hold_ms, .. } => {
                        let target = drill.target_fraction().unwrap();
                        let params = HoldParams::new(target, drill.tolerance_fraction(), *hold_ms);
                        let samples: Vec<ValueSample> = (0..=u64::from(*hold_ms) + 1000)
                            .map(|ms| ValueSample::new(START_US - 500_000 + ms * 1000, target))
                            .collect();
                        let score = score_hold(&samples, START_US, &params).unwrap();
                        (score.total, score.grade)
                    }
                };
                assert!(
                    total.is_finite() && total >= 95.0 && grade == Grade::S,
                    "{who}: total {total} grade {grade:?}"
                );
                checked.push(format!("{}/{}", preset.id, drill.id));
            }
        }
        assert!(
            checked.iter().filter(|id| id.starts_with("gt3/")).count() == 6,
            "expected the six GT3 drills, checked {checked:?}"
        );
    }

    #[test]
    fn test_14_low_plateau_leaves_the_band() {
        let target = second_curve();
        let params = TraceParams::new(&target, 0.10);
        let samples = generate_synthetic_samples(&target, START_US, 0.0, |n, val| {
            if (880..=1930).contains(&n) {
                val - 0.25
            } else {
                val
            }
        });

        let score = score_trace(&samples, START_US, &params).unwrap();
        assert!(
            score.time_in_band < 0.80,
            "time_in_band was {}",
            score.time_in_band
        );
    }

    #[test]
    fn test_12_serialization_uses_camel_case() {
        let score = TraceScore {
            total: 96.5,
            grade: Grade::S,
            accuracy: 98.0,
            timing: 95.0,
            smoothness: 95.0,
            lag_ms: 2.5,
            time_in_band: 0.95,
            rmse: 0.015,
            overshoot: 0.01,
            ldlj_user: -8.5,
            ldlj_target: -8.2,
        };

        let json = serde_json::to_value(&score).unwrap();
        let obj = json.as_object().unwrap();

        assert!(obj.contains_key("total"));
        assert!(obj.contains_key("grade"));
        assert!(obj.contains_key("accuracy"));
        assert!(obj.contains_key("timing"));
        assert!(obj.contains_key("smoothness"));
        assert!(obj.contains_key("lagMs"));
        assert!(obj.contains_key("timeInBand"));
        assert!(obj.contains_key("rmse"));
        assert!(obj.contains_key("overshoot"));
        assert!(obj.contains_key("ldljUser"));
        assert!(obj.contains_key("ldljTarget"));

        // Verify none of the snake_case keys are present
        assert!(!obj.contains_key("lag_ms"));
        assert!(!obj.contains_key("time_in_band"));
        assert!(!obj.contains_key("ldlj_user"));
        assert!(!obj.contains_key("ldlj_target"));
    }
}
