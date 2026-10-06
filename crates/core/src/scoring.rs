//! Scoring model for "hold" drill repetitions on full-rate pedal samples.
//!
//! A hold drill presents the driver with a target pedal position (e.g. 70% brake)
//! and a tolerance band (e.g. ±5%). The driver is tasked with quickly pressing to
//! the target band and holding the pedal steady for the duration of the drill.
//!
//! # Scoring Model
//!
//! Repetition scoring evaluates three distinct pillars, combining them into a total
//! score in `0.0..=100.0` and assigning a letter [`Grade`]:
//!
//! - **Accuracy (50% weight):** Evaluated over the settled part of the window (from the
//!   first in-band sample to the window end). It combines the time-weighted fraction of
//!   time spent within the tolerance band ([`ACCURACY_IN_BAND_WEIGHT`], 60%) and the
//!   root-mean-square error ([`ACCURACY_RMSE_WEIGHT`], 40%) normalized against
//!   [`RMSE_TOLERANCE_FACTOR`] times tolerance. If the band is never entered, accuracy is
//!   evaluated over the whole window with zero in-band fraction.
//! - **Timing (25% weight):** Measures how quickly the pedal first enters the tolerance band
//!   relative to `start_us`. Entering within [`TIMING_FAST_THRESHOLD_MS`] (150 ms) earns
//!   a perfect 100 score, decaying linearly down to 0 at [`TIMING_SLOW_THRESHOLD_MS`] (1500 ms).
//!   If the band is never entered, timing is 0.
//! - **Smoothness (25% weight):** Evaluated over the settled part. Penalizes excursions beyond
//!   the band ([`OVERSHOOT_PENALTY_FACTOR`]) and high-frequency jitter ([`JITTER_PENALTY_FACTOR`])
//!   relative to a 50 ms time-weighted moving average. If the band is never entered, smoothness
//!   defaults to 100 because accuracy and timing already penalize the rep.
//!
//! All averages use sample-to-sample elapsed time weights (`dt`) to handle irregular sampling.

use serde::Serialize;

/// Fast timing threshold (150 ms) for a full timing score of 100.
pub const TIMING_FAST_THRESHOLD_MS: f32 = 150.0;

/// Slow timing threshold (1500 ms) where the timing score decays to 0.
pub const TIMING_SLOW_THRESHOLD_MS: f32 = 1500.0;

/// Weight of in-band time fraction in the accuracy score (60%).
pub const ACCURACY_IN_BAND_WEIGHT: f32 = 0.6;

/// Weight of RMSE in the accuracy score (40%).
pub const ACCURACY_RMSE_WEIGHT: f32 = 0.4;

/// Tolerance multiplier defining the ceiling for RMSE normalization (3× tolerance).
pub const RMSE_TOLERANCE_FACTOR: f32 = 3.0;

/// Time window (50 ms) for the moving average baseline used in jitter calculation.
pub const JITTER_WINDOW_MS: f32 = 50.0;

/// Half-window (25 ms) in microseconds for the moving average baseline.
pub const JITTER_HALF_WINDOW_US: u64 = 25_000;

/// Scaling factor (25) for overshoot penalty relative to tolerance.
pub const OVERSHOOT_PENALTY_FACTOR: f32 = 25.0;

/// Scaling factor (100) for jitter penalty relative to tolerance.
pub const JITTER_PENALTY_FACTOR: f32 = 100.0;

/// Maximum penalty points (50) that can be deducted for overshoot or jitter individually.
pub const MAX_SMOOTHNESS_PENALTY: f32 = 50.0;

/// Weight of the accuracy score in the total score (50%).
pub const ACCURACY_TOTAL_WEIGHT: f32 = 0.5;

/// Weight of the timing score in the total score (25%).
pub const TIMING_TOTAL_WEIGHT: f32 = 0.25;

/// Weight of the smoothness score in the total score (25%).
pub const SMOOTHNESS_TOTAL_WEIGHT: f32 = 0.25;

/// Performance grade awarded based on total score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Grade {
    /// Outstanding performance (total ≥ 95).
    S,
    /// Excellent performance (total ≥ 85).
    A,
    /// Good performance (total ≥ 70).
    B,
    /// Adequate performance (total ≥ 55).
    C,
    /// Needs improvement (total < 55).
    D,
}

impl Grade {
    /// Determines the grade from a total score.
    #[must_use]
    pub fn from_total(total: f32) -> Self {
        if total >= 95.0 {
            Self::S
        } else if total >= 85.0 {
            Self::A
        } else if total >= 70.0 {
            Self::B
        } else if total >= 55.0 {
            Self::C
        } else {
            Self::D
        }
    }
}

/// A single timestamped pedal position sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ValueSample {
    /// Monotonic timestamp in microseconds.
    pub t_us: u64,
    /// Normalized pedal position in `0.0..=1.0`.
    pub value: f32,
}

impl ValueSample {
    /// Creates a new timestamped value sample.
    #[must_use]
    pub const fn new(t_us: u64, value: f32) -> Self {
        Self { t_us, value }
    }
}

/// Parameters configuring a hold drill repetition.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HoldParams {
    /// Target pedal position fraction in `0.0..=1.0`.
    pub target: f32,
    /// Symmetrical tolerance fraction in `0.0..=1.0`.
    pub tolerance: f32,
    /// Hold drill duration in milliseconds.
    pub hold_ms: u32,
}

impl HoldParams {
    /// Creates hold drill parameters.
    #[must_use]
    pub const fn new(target: f32, tolerance: f32, hold_ms: u32) -> Self {
        Self {
            target,
            tolerance,
            hold_ms,
        }
    }
}

/// Detailed evaluation and score breakdown for a hold drill repetition.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldScore {
    /// Overall score in `0.0..=100.0`.
    pub total: f32,
    /// Assigned letter grade.
    pub grade: Grade,
    /// Accuracy sub-score in `0.0..=100.0`.
    pub accuracy: f32,
    /// Timing sub-score in `0.0..=100.0`.
    pub timing: f32,
    /// Smoothness sub-score in `0.0..=100.0`.
    pub smoothness: f32,
    /// Time-weighted fraction of settled time spent in band (`0.0..=1.0`).
    pub time_in_band: f32,
    /// Root-mean-square error from target during settled portion.
    pub rmse: f32,
    /// Time elapsed in milliseconds from `start_us` until first entering the tolerance band.
    pub time_to_band_ms: Option<f32>,
    /// Maximum excursion beyond the tolerance band during settled portion.
    pub overshoot: f32,
    /// Time-weighted RMS deviation from the 50 ms moving average baseline.
    pub jitter: f32,
}

/// Scores a hold drill repetition on full-rate samples.
///
/// The evaluation window spans `[start_us, start_us + hold_ms * 1000)`. Only samples
/// inside this interval are included in the scoring. Returns `None` if fewer than 2
/// samples fall within the window, or if parameters are invalid.
#[must_use]
pub fn score_hold(
    samples: &[ValueSample],
    start_us: u64,
    params: &HoldParams,
) -> Option<HoldScore> {
    if !params.target.is_finite()
        || !params.tolerance.is_finite()
        || params.tolerance <= 0.0
        || params.hold_ms == 0
    {
        return None;
    }

    let hold_us = u64::from(params.hold_ms).checked_mul(1_000)?;
    let end_us = start_us.checked_add(hold_us)?;

    let mut window_samples: Vec<ValueSample> = samples
        .iter()
        .copied()
        .filter(|s| s.t_us >= start_us && s.t_us < end_us)
        .collect();

    if window_samples.len() < 2 {
        return None;
    }

    window_samples.sort_by_key(|s| s.t_us);

    let n = window_samples.len();
    let mut dts = Vec::with_capacity(n);
    for i in 0..n {
        let dt = if i + 1 < n {
            window_samples[i + 1]
                .t_us
                .saturating_sub(window_samples[i].t_us)
        } else {
            end_us.saturating_sub(window_samples[i].t_us)
        };
        dts.push(dt);
    }

    let first_in_band_idx = window_samples
        .iter()
        .position(|s| (s.value - params.target).abs() <= params.tolerance);

    let (time_to_band_ms, timing_score) =
        compute_timing(&window_samples, start_us, first_in_band_idx);

    let (settled_samples, settled_dts, entered_band) = match first_in_band_idx {
        Some(idx) => (&window_samples[idx..], &dts[idx..], true),
        None => (&window_samples[..], &dts[..], false),
    };

    let (in_band_fraction, rmse, accuracy, total_settled_weight) =
        compute_accuracy(settled_samples, settled_dts, entered_band, params);

    let (overshoot, jitter, smoothness) = compute_smoothness(
        settled_samples,
        settled_dts,
        &window_samples,
        &dts,
        first_in_band_idx,
        total_settled_weight,
        params,
    );

    let total = (ACCURACY_TOTAL_WEIGHT * accuracy
        + TIMING_TOTAL_WEIGHT * timing_score
        + SMOOTHNESS_TOTAL_WEIGHT * smoothness)
        .clamp(0.0, 100.0);
    let grade = Grade::from_total(total);

    Some(HoldScore {
        total,
        grade,
        accuracy,
        timing: timing_score,
        smoothness,
        time_in_band: in_band_fraction,
        rmse,
        time_to_band_ms,
        overshoot,
        jitter,
    })
}

fn compute_timing(
    window_samples: &[ValueSample],
    start_us: u64,
    first_in_band_idx: Option<usize>,
) -> (Option<f32>, f32) {
    match first_in_band_idx {
        Some(idx) => {
            let t_first = window_samples[idx].t_us;
            let time_to_band_us = t_first.saturating_sub(start_us);
            #[expect(
                clippy::cast_precision_loss,
                reason = "time_to_band_us in microseconds fits within f64"
            )]
            let t_ms_f64 = (time_to_band_us as f64) / 1000.0;
            #[expect(
                clippy::cast_possible_truncation,
                reason = "t_ms_f64 fits comfortably in f32"
            )]
            let t_ms = t_ms_f64 as f32;
            let score = if t_ms <= TIMING_FAST_THRESHOLD_MS {
                100.0
            } else if t_ms >= TIMING_SLOW_THRESHOLD_MS {
                0.0
            } else {
                ((TIMING_SLOW_THRESHOLD_MS - t_ms)
                    / (TIMING_SLOW_THRESHOLD_MS - TIMING_FAST_THRESHOLD_MS))
                    * 100.0
            };
            (Some(t_ms), score.clamp(0.0, 100.0))
        }
        None => (None, 0.0),
    }
}

fn compute_accuracy(
    settled_samples: &[ValueSample],
    settled_dts: &[u64],
    entered_band: bool,
    params: &HoldParams,
) -> (f32, f32, f32, f64) {
    let mut total_settled_weight = 0.0_f64;
    let mut in_band_weight = 0.0_f64;
    let mut weighted_sq_err = 0.0_f64;

    for (sample, &dt) in settled_samples.iter().zip(settled_dts.iter()) {
        #[expect(clippy::cast_precision_loss, reason = "dt in microseconds fits in f64")]
        let w = dt as f64;
        total_settled_weight += w;
        if (sample.value - params.target).abs() <= params.tolerance {
            in_band_weight += w;
        }
        let err = f64::from(sample.value) - f64::from(params.target);
        weighted_sq_err += w * err * err;
    }

    let in_band_fraction = if entered_band && total_settled_weight > 0.0 {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "fraction in 0.0..=1.0 fits in f32"
        )]
        ((in_band_weight / total_settled_weight).clamp(0.0, 1.0) as f32)
    } else {
        0.0
    };

    let rmse = if total_settled_weight > 0.0 {
        #[expect(clippy::cast_possible_truncation, reason = "rmse fits in f32")]
        ((weighted_sq_err / total_settled_weight).sqrt() as f32)
    } else {
        0.0
    };

    let rmse_norm = (1.0 - rmse / (RMSE_TOLERANCE_FACTOR * params.tolerance)).max(0.0);
    let accuracy = (100.0
        * (ACCURACY_IN_BAND_WEIGHT * in_band_fraction + ACCURACY_RMSE_WEIGHT * rmse_norm))
        .clamp(0.0, 100.0);

    (in_band_fraction, rmse, accuracy, total_settled_weight)
}

fn compute_smoothness(
    settled_samples: &[ValueSample],
    settled_dts: &[u64],
    window_samples: &[ValueSample],
    dts: &[u64],
    first_in_band_idx: Option<usize>,
    total_settled_weight: f64,
    params: &HoldParams,
) -> (f32, f32, f32) {
    if let Some(idx) = first_in_band_idx {
        let max_overshoot = settled_samples
            .iter()
            .map(|s| (s.value - params.target).abs() - params.tolerance)
            .fold(0.0_f32, f32::max);

        let moving_avgs = calculate_moving_avg_50ms(window_samples, dts);
        let settled_moving_avgs = &moving_avgs[idx..];

        let mut weighted_jitter_sq = 0.0_f64;
        for (sample, (&dt, &ma)) in settled_samples
            .iter()
            .zip(settled_dts.iter().zip(settled_moving_avgs.iter()))
        {
            #[expect(clippy::cast_precision_loss, reason = "dt in microseconds fits in f64")]
            let w = dt as f64;
            let diff = f64::from(sample.value) - f64::from(ma);
            weighted_jitter_sq += w * diff * diff;
        }

        let jitter_val = if total_settled_weight > 0.0 {
            #[expect(clippy::cast_possible_truncation, reason = "jitter fits in f32")]
            ((weighted_jitter_sq / total_settled_weight).sqrt() as f32)
        } else {
            0.0
        };

        let overshoot_penalty = ((max_overshoot / params.tolerance) * OVERSHOOT_PENALTY_FACTOR)
            .min(MAX_SMOOTHNESS_PENALTY);
        let jitter_penalty =
            ((jitter_val / params.tolerance) * JITTER_PENALTY_FACTOR).min(MAX_SMOOTHNESS_PENALTY);
        let smooth = (100.0 - overshoot_penalty - jitter_penalty).clamp(0.0, 100.0);

        (max_overshoot, jitter_val, smooth)
    } else {
        (0.0, 0.0, 100.0)
    }
}

/// Computes a 50 ms time-weighted moving average baseline using a two-pointer sliding window.
fn calculate_moving_avg_50ms(samples: &[ValueSample], dts: &[u64]) -> Vec<f32> {
    let n = samples.len();
    let mut result = Vec::with_capacity(n);
    let mut left = 0;
    let mut right = 0;
    let mut sum_weights = 0.0_f64;
    let mut sum_values = 0.0_f64;

    for i in 0..n {
        let t_i = samples[i].t_us;
        let min_t = t_i.saturating_sub(JITTER_HALF_WINDOW_US);
        let max_t = t_i.saturating_add(JITTER_HALF_WINDOW_US);

        while right < n && samples[right].t_us <= max_t {
            #[expect(clippy::cast_precision_loss, reason = "dt in microseconds fits in f64")]
            let w = dts[right] as f64;
            sum_weights += w;
            sum_values += w * f64::from(samples[right].value);
            right += 1;
        }

        while left < right && samples[left].t_us < min_t {
            #[expect(clippy::cast_precision_loss, reason = "dt in microseconds fits in f64")]
            let w = dts[left] as f64;
            sum_weights -= w;
            sum_values -= w * f64::from(samples[left].value);
            left += 1;
        }

        if left == right || sum_weights <= 0.0 {
            sum_weights = 0.0;
            sum_values = 0.0;
            result.push(samples[i].value);
        } else {
            let avg = sum_values / sum_weights;
            #[expect(clippy::cast_possible_truncation, reason = "average value fits in f32")]
            result.push(avg as f32);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_1_perfect_hold() {
        let params = HoldParams {
            target: 0.70,
            tolerance: 0.05,
            hold_ms: 2000,
        };
        let start_us = 0;
        let samples: Vec<ValueSample> = (0_u32..2000)
            .map(|i| ValueSample {
                t_us: u64::from(i) * 1000,
                value: 0.70,
            })
            .collect();
        let score = score_hold(&samples, start_us, &params).expect("score should exist");
        assert!(score.total >= 98.0, "total was {}", score.total);
        assert_eq!(score.grade, Grade::S);
    }

    #[test]
    fn case_2_realistic_hold() {
        let params = HoldParams {
            target: 0.70,
            tolerance: 0.05,
            hold_ms: 2000,
        };
        let start_us = 0;
        let samples: Vec<ValueSample> = (0_u32..2000)
            .map(|i| {
                let val = if i < 150 {
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "ramp fraction fits within f32"
                    )]
                    let r = (0.70 * (f64::from(i) / 150.0)) as f32;
                    r
                } else {
                    0.70
                };
                ValueSample {
                    t_us: u64::from(i) * 1000,
                    value: val,
                }
            })
            .collect();
        let score = score_hold(&samples, start_us, &params).expect("score should exist");
        assert!(score.total >= 95.0, "total was {}", score.total);
    }

    #[test]
    fn case_3_ten_percent_offset() {
        let params = HoldParams {
            target: 0.70,
            tolerance: 0.05,
            hold_ms: 2000,
        };
        let start_us = 0;
        let samples: Vec<ValueSample> = (0_u32..2000)
            .map(|i| ValueSample {
                t_us: u64::from(i) * 1000,
                value: 0.80,
            })
            .collect();
        let score = score_hold(&samples, start_us, &params).expect("score should exist");
        assert!(score.total < 55.0, "total was {}", score.total);
        assert_eq!(score.grade, Grade::D);
    }

    #[test]
    fn case_4_never_entering() {
        let params = HoldParams {
            target: 0.70,
            tolerance: 0.05,
            hold_ms: 2000,
        };
        let start_us = 0;
        let samples: Vec<ValueSample> = (0_u32..2000)
            .map(|i| ValueSample {
                t_us: u64::from(i) * 1000,
                value: 0.0,
            })
            .collect();
        let score = score_hold(&samples, start_us, &params).expect("score should exist");
        assert_eq!(score.timing, 0.0);
        assert_eq!(score.time_to_band_ms, None);
        assert_eq!(score.grade, Grade::D);
    }

    #[test]
    fn case_5_late_entry() {
        let params = HoldParams {
            target: 0.70,
            tolerance: 0.05,
            hold_ms: 2000,
        };
        let start_us = 0;
        let samples_case_2: Vec<ValueSample> = (0_u32..2000)
            .map(|i| {
                let val = if i < 150 {
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "ramp fraction fits within f32"
                    )]
                    let r = (0.70 * (f64::from(i) / 150.0)) as f32;
                    r
                } else {
                    0.70
                };
                ValueSample {
                    t_us: u64::from(i) * 1000,
                    value: val,
                }
            })
            .collect();
        let score_case_2 = score_hold(&samples_case_2, start_us, &params).unwrap();

        let samples_late: Vec<ValueSample> = (0_u32..2000)
            .map(|i| {
                let val = if i < 800 {
                    #[expect(
                        clippy::cast_possible_truncation,
                        reason = "ramp fraction fits within f32"
                    )]
                    let r = (0.65 * (f64::from(i) / 800.0)) as f32;
                    r
                } else {
                    0.70
                };
                ValueSample {
                    t_us: u64::from(i) * 1000,
                    value: val,
                }
            })
            .collect();
        let score_late = score_hold(&samples_late, start_us, &params).unwrap();

        assert!(
            score_late.timing < score_case_2.timing,
            "late timing {} not less than case 2 timing {}",
            score_late.timing,
            score_case_2.timing
        );
        assert!(
            score_late.total < score_case_2.total,
            "late total {} not less than case 2 total {}",
            score_late.total,
            score_case_2.total
        );
    }

    #[test]
    fn case_6_noisy_hold() {
        let params = HoldParams {
            target: 0.70,
            tolerance: 0.05,
            hold_ms: 2000,
        };
        let start_us = 0;
        let perfect_samples: Vec<ValueSample> = (0_u32..2000)
            .map(|i| ValueSample {
                t_us: u64::from(i) * 1000,
                value: 0.70,
            })
            .collect();
        let perfect_score = score_hold(&perfect_samples, start_us, &params).unwrap();

        let noisy_samples: Vec<ValueSample> = (0_u32..2000)
            .map(|i| {
                let angle = f64::from(i) * 0.73;
                #[expect(clippy::cast_possible_truncation, reason = "sin result fits in f32")]
                let noise = (angle.sin() * 0.02) as f32;
                ValueSample {
                    t_us: u64::from(i) * 1000,
                    value: 0.70 + noise,
                }
            })
            .collect();
        let noisy_score = score_hold(&noisy_samples, start_us, &params).unwrap();

        assert!(
            noisy_score.smoothness < 100.0,
            "smoothness was {}",
            noisy_score.smoothness
        );
        assert!(
            noisy_score.jitter > 0.0,
            "jitter was {}",
            noisy_score.jitter
        );
        assert!(
            noisy_score.total < perfect_score.total,
            "noisy total {} not < perfect {}",
            noisy_score.total,
            perfect_score.total
        );
        assert!(
            noisy_score.total >= 70.0,
            "noisy total was {}",
            noisy_score.total
        );
    }

    #[test]
    fn case_7_overshoot_spike() {
        let params = HoldParams {
            target: 0.70,
            tolerance: 0.05,
            hold_ms: 2000,
        };
        let start_us = 0;
        let perfect_samples: Vec<ValueSample> = (0_u32..2000)
            .map(|i| ValueSample {
                t_us: u64::from(i) * 1000,
                value: 0.70,
            })
            .collect();
        let perfect_score = score_hold(&perfect_samples, start_us, &params).unwrap();

        let spike_samples: Vec<ValueSample> = (0_u32..2000)
            .map(|i| {
                let val = if (500..600).contains(&i) { 0.85 } else { 0.70 };
                ValueSample {
                    t_us: u64::from(i) * 1000,
                    value: val,
                }
            })
            .collect();
        let score = score_hold(&spike_samples, start_us, &params).unwrap();

        assert!(
            (score.overshoot - 0.10).abs() < 1e-4,
            "overshoot was {}",
            score.overshoot
        );
        assert!(
            score.smoothness < perfect_score.smoothness,
            "smoothness {} not < {}",
            score.smoothness,
            perfect_score.smoothness
        );
    }

    #[test]
    fn case_8_irregular_timestamps() {
        let params = HoldParams {
            target: 0.70,
            tolerance: 0.05,
            hold_ms: 2000,
        };
        let start_us = 0;
        let perfect_samples: Vec<ValueSample> = (0_u32..2000)
            .map(|i| ValueSample {
                t_us: u64::from(i) * 1000,
                value: 0.70,
            })
            .collect();
        let perfect_score = score_hold(&perfect_samples, start_us, &params).unwrap();

        let mut irregular_samples = Vec::new();
        let mut current_t = 0_u64;
        let mut step_toggle = false;
        while current_t < 2_000_000 {
            irregular_samples.push(ValueSample {
                t_us: current_t,
                value: 0.70,
            });
            let step = if step_toggle { 3000 } else { 1000 };
            step_toggle = !step_toggle;
            current_t += step;
        }
        let irregular_score = score_hold(&irregular_samples, start_us, &params).unwrap();

        assert!(
            (irregular_score.total - perfect_score.total).abs() <= 0.5,
            "diff was {}",
            (irregular_score.total - perfect_score.total).abs()
        );
    }

    #[test]
    fn case_9_samples_outside_window_ignored() {
        let params = HoldParams {
            target: 0.70,
            tolerance: 0.05,
            hold_ms: 2000,
        };
        let start_us = 1_000_000;
        let window_end = start_us + 2_000_000;

        let mut samples = Vec::new();
        samples.push(ValueSample {
            t_us: 100,
            value: 0.0,
        });
        samples.push(ValueSample {
            t_us: 500_000,
            value: 0.1,
        });
        samples.push(ValueSample {
            t_us: 999_999,
            value: 0.9,
        });

        for i in 0_u32..2000 {
            samples.push(ValueSample {
                t_us: start_us + u64::from(i) * 1000,
                value: 0.70,
            });
        }

        samples.push(ValueSample {
            t_us: window_end,
            value: 0.0,
        });
        samples.push(ValueSample {
            t_us: window_end + 10_000,
            value: 0.5,
        });
        samples.push(ValueSample {
            t_us: window_end + 500_000,
            value: 1.0,
        });

        let score = score_hold(&samples, start_us, &params).expect("score should exist");
        assert!(score.total >= 98.0, "total was {}", score.total);
        assert_eq!(score.grade, Grade::S);
    }

    #[test]
    fn case_10_fewer_than_two_samples() {
        let params = HoldParams {
            target: 0.70,
            tolerance: 0.05,
            hold_ms: 2000,
        };
        let start_us = 0;

        assert_eq!(score_hold(&[], start_us, &params), None);

        let outside_samples = vec![
            ValueSample {
                t_us: 2_500_000,
                value: 0.70,
            },
            ValueSample {
                t_us: 3_000_000,
                value: 0.70,
            },
        ];
        assert_eq!(score_hold(&outside_samples, start_us, &params), None);

        let one_sample = vec![ValueSample {
            t_us: 500_000,
            value: 0.70,
        }];
        assert_eq!(score_hold(&one_sample, start_us, &params), None);
    }

    #[test]
    fn case_11_grade_boundaries() {
        assert_eq!(Grade::from_total(100.0), Grade::S);
        assert_eq!(Grade::from_total(95.0), Grade::S);
        assert_eq!(Grade::from_total(94.99), Grade::A);
        assert_eq!(Grade::from_total(85.0), Grade::A);
        assert_eq!(Grade::from_total(84.99), Grade::B);
        assert_eq!(Grade::from_total(70.0), Grade::B);
        assert_eq!(Grade::from_total(69.99), Grade::C);
        assert_eq!(Grade::from_total(55.0), Grade::C);
        assert_eq!(Grade::from_total(54.99), Grade::D);
        assert_eq!(Grade::from_total(0.0), Grade::D);
        assert_eq!(Grade::from_total(-10.0), Grade::D);
    }

    #[test]
    fn case_12_json_serialization_camel_case() {
        let score = HoldScore {
            total: 96.5,
            grade: Grade::S,
            accuracy: 98.0,
            timing: 95.0,
            smoothness: 95.0,
            time_in_band: 1.0,
            rmse: 0.01,
            time_to_band_ms: Some(120.0),
            overshoot: 0.0,
            jitter: 0.002,
        };
        let json = serde_json::to_value(&score).unwrap();
        let obj = json.as_object().unwrap();

        assert!(obj.contains_key("total"));
        assert!(obj.contains_key("grade"));
        assert!(obj.contains_key("accuracy"));
        assert!(obj.contains_key("timing"));
        assert!(obj.contains_key("smoothness"));
        assert!(obj.contains_key("timeInBand"));
        assert!(obj.contains_key("rmse"));
        assert!(obj.contains_key("timeToBandMs"));
        assert!(obj.contains_key("overshoot"));
        assert!(obj.contains_key("jitter"));

        assert_eq!(obj.get("grade").unwrap(), "S");
        assert_eq!(obj.get("timeToBandMs").unwrap(), 120.0);
    }
}
