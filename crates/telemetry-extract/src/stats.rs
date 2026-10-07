//! Statistical analysis and metric aggregation for pedal zones.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::csv::{LapTelemetry, TELEMETRY_HZ};
use crate::zones::{BrakeZone, ThrottleExitZone};

/// Rate threshold defining a fast throttle step in %/s (300 %/s).
pub const FAST_STEP_RATE_THRESHOLD: f64 = 300.0;

/// Median and Interquartile Range (IQR) for a metric series.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct MetricSummary {
    /// Median (50th percentile).
    pub median: f64,
    /// Interquartile Range (75th percentile minus 25th percentile).
    pub iqr: f64,
}

impl MetricSummary {
    /// Computes median and IQR from a slice of values using linear interpolation.
    #[must_use]
    pub fn compute(values: &[f64]) -> Self {
        if values.is_empty() {
            return Self {
                median: 0.0,
                iqr: 0.0,
            };
        }

        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let median = percentile_sorted(&sorted, 0.50);
        let q1 = percentile_sorted(&sorted, 0.25);
        let q3 = percentile_sorted(&sorted, 0.75);
        let iqr = q3 - q1;

        Self { median, iqr }
    }
}

fn percentile_sorted(sorted: &[f64], p: f64) -> f64 {
    let n = sorted.len();
    if n == 0 {
        return 0.0;
    }
    if n == 1 {
        return sorted[0];
    }

    #[expect(
        clippy::cast_precision_loss,
        reason = "slice length safely converts to f64"
    )]
    let pos = p * (n - 1) as f64;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "percentile rank fits in usize"
    )]
    let idx = pos.floor() as usize;
    let frac = pos - pos.floor();

    if idx + 1 < n {
        sorted[idx] + frac * (sorted[idx + 1] - sorted[idx])
    } else {
        sorted[idx]
    }
}

/// Raw metrics measured from a single throttle corner exit.
#[derive(Clone, Debug, PartialEq)]
pub struct ThrottleExitMetrics {
    /// Throttle percentage reached ~150 ms after onset (`0.0..=100.0`).
    pub initial_stab_level_pct: f64,
    /// Duration from onset to the end of the initial stab in seconds.
    pub time_to_stab_s: f64,
    /// Number of fast steps (rate >= 300 %/s) before the progressive phase.
    pub fast_steps: usize,
    /// Progressive ramp rate from end of stab to 98% in %/s.
    pub progressive_ramp_rate_pct_s: f64,
    /// Total duration from onset to 98% full throttle in seconds.
    pub time_to_full_s: f64,
}

/// Analyzes an individual throttle exit zone.
#[must_use]
pub fn analyze_throttle_exit(throttles: &[f32], zone: &ThrottleExitZone) -> ThrottleExitMetrics {
    let onset = zone.onset_idx;
    let full = zone.full_idx;

    #[expect(
        clippy::cast_precision_loss,
        reason = "sample count safely converts to f64"
    )]
    let time_to_full_s = (full - onset) as f64 / TELEMETRY_HZ;

    // 1. Initial stab level: throttle reached ~150 ms after onset (9 samples at 60 Hz)
    let idx_150ms = (onset + 9).min(full);
    let initial_stab_level_pct = f64::from(throttles[idx_150ms]) * 100.0;

    // 2. Compute derivative rate (%/s) across exit frames
    let num_frames = full - onset;
    let mut rates = Vec::with_capacity(num_frames);
    for i in onset..full {
        let dt = 1.0 / TELEMETRY_HZ;
        let dy = f64::from(throttles[i + 1] - throttles[i]) * 100.0;
        rates.push(dy / dt);
    }

    // 3. Identify fast steps (contiguous blocks with rate >= 300 %/s)
    let mut fast_step_ranges: Vec<(usize, usize)> = Vec::new();
    let mut in_step = false;
    let mut step_start = 0;

    for (k, &r) in rates.iter().enumerate() {
        if r >= FAST_STEP_RATE_THRESHOLD {
            if !in_step {
                in_step = true;
                step_start = k;
            }
        } else if in_step {
            in_step = false;
            fast_step_ranges.push((step_start, k));
        }
    }
    if in_step {
        fast_step_ranges.push((step_start, rates.len()));
    }

    let (time_to_stab_s, end_of_stab_frame, fast_steps) = if fast_step_ranges.is_empty() {
        // No fast steps: driver progressively rolled on from onset
        (0.0, onset, 0)
    } else {
        // The first fast step represents the initial stab
        let first_step_end = onset + fast_step_ranges[0].1;
        #[expect(
            clippy::cast_precision_loss,
            reason = "sample count safely converts to f64"
        )]
        let t_stab = (first_step_end - onset) as f64 / TELEMETRY_HZ;

        // Steps before the progressive phase: count all fast steps
        let steps_count = fast_step_ranges.len();
        (t_stab, first_step_end, steps_count)
    };

    // 4. Progressive ramp rate (%/s) from end of stab to 98%
    let progressive_ramp_rate_pct_s = if full > end_of_stab_frame {
        #[expect(
            clippy::cast_precision_loss,
            reason = "sample count safely converts to f64"
        )]
        let dt = (full - end_of_stab_frame) as f64 / TELEMETRY_HZ;
        let dy = f64::from(throttles[full] - throttles[end_of_stab_frame]) * 100.0;
        if dt > 1e-6 { dy / dt } else { 0.0 }
    } else if time_to_full_s > 1e-6 {
        // Entire zone was a fast stab all the way to full
        let dy = f64::from(throttles[full] - throttles[onset]) * 100.0;
        dy / time_to_full_s
    } else {
        0.0
    };

    ThrottleExitMetrics {
        initial_stab_level_pct,
        time_to_stab_s,
        fast_steps,
        progressive_ramp_rate_pct_s,
        time_to_full_s,
    }
}

/// Raw metrics measured from a single braking zone.
#[derive(Clone, Debug, PartialEq)]
pub struct BrakeZoneMetrics {
    /// Maximum brake pressure reached in percent `[0.0, 100.0]`.
    pub peak_pct: f64,
    /// Time elapsed from onset to peak in seconds.
    pub time_to_peak_s: f64,
    /// Duration of the trail-off / release phase from peak to release in seconds.
    pub trail_duration_s: f64,
}

/// Analyzes an individual brake zone.
#[must_use]
pub fn analyze_brake_zone(_brakes: &[f32], zone: &BrakeZone) -> BrakeZoneMetrics {
    #[expect(
        clippy::cast_precision_loss,
        reason = "sample count safely converts to f64"
    )]
    let time_to_peak_s = (zone.peak_idx - zone.onset_idx) as f64 / TELEMETRY_HZ;
    #[expect(
        clippy::cast_precision_loss,
        reason = "sample count safely converts to f64"
    )]
    let trail_duration_s = (zone.release_idx - zone.peak_idx) as f64 / TELEMETRY_HZ;

    BrakeZoneMetrics {
        peak_pct: f64::from(zone.peak_pct),
        time_to_peak_s,
        trail_duration_s,
    }
}

/// Summary statistics for brake zones across laps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarBrakeStats {
    pub zone_count: usize,
    pub peak_pct: MetricSummary,
    pub time_to_peak_s: MetricSummary,
    pub trail_duration_s: MetricSummary,
}

/// Summary statistics for throttle exit zones across laps.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarThrottleStats {
    pub zone_count: usize,
    pub initial_stab_level_pct: MetricSummary,
    pub time_to_stab_s: MetricSummary,
    pub fast_steps: MetricSummary,
    pub progressive_ramp_rate_pct_s: MetricSummary,
    pub time_to_full_s: MetricSummary,
}

/// Aggregated telemetry statistics for a car model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarStats {
    pub car: String,
    pub laps_count: usize,
    pub brake: CarBrakeStats,
    pub throttle: CarThrottleStats,
}

/// High-level container for multi-car telemetry statistics.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TelemetryReport {
    pub cars: BTreeMap<String, CarStats>,
}

/// Collector to accumulate and aggregate metrics across laps.
#[derive(Default)]
pub struct StatsCollector {
    cars: BTreeMap<String, CarAccumulator>,
}

#[derive(Default)]
struct CarAccumulator {
    laps_count: usize,
    // Brake metrics
    brake_peaks: Vec<f64>,
    brake_times_to_peak: Vec<f64>,
    brake_trail_durations: Vec<f64>,
    // Throttle metrics
    throttle_stabs: Vec<f64>,
    throttle_times_to_stab: Vec<f64>,
    throttle_fast_steps: Vec<f64>,
    throttle_ramp_rates: Vec<f64>,
    throttle_times_to_full: Vec<f64>,
}

impl StatsCollector {
    /// Ingests a lap and its detected zones.
    pub fn add_lap(
        &mut self,
        lap: &LapTelemetry,
        brakes: &[BrakeZone],
        throttles: &[ThrottleExitZone],
    ) {
        let car_name = lap.car_name().to_string();
        let acc = self.cars.entry(car_name).or_default();
        acc.laps_count += 1;

        for b_zone in brakes {
            let m = analyze_brake_zone(&lap.brake, b_zone);
            acc.brake_peaks.push(m.peak_pct);
            acc.brake_times_to_peak.push(m.time_to_peak_s);
            acc.brake_trail_durations.push(m.trail_duration_s);
        }

        for t_zone in throttles {
            let m = analyze_throttle_exit(&lap.throttle, t_zone);
            acc.throttle_stabs.push(m.initial_stab_level_pct);
            acc.throttle_times_to_stab.push(m.time_to_stab_s);
            #[expect(
                clippy::cast_precision_loss,
                reason = "small integer count safely converts to f64"
            )]
            acc.throttle_fast_steps.push(m.fast_steps as f64);
            acc.throttle_ramp_rates.push(m.progressive_ramp_rate_pct_s);
            acc.throttle_times_to_full.push(m.time_to_full_s);
        }
    }

    /// Builds the consolidated report.
    #[must_use]
    pub fn build_report(self) -> TelemetryReport {
        let mut report_cars = BTreeMap::new();

        for (car, acc) in self.cars {
            let brake_count = acc.brake_peaks.len();
            let throttle_count = acc.throttle_stabs.len();

            let brake = CarBrakeStats {
                zone_count: brake_count,
                peak_pct: MetricSummary::compute(&acc.brake_peaks),
                time_to_peak_s: MetricSummary::compute(&acc.brake_times_to_peak),
                trail_duration_s: MetricSummary::compute(&acc.brake_trail_durations),
            };

            let throttle = CarThrottleStats {
                zone_count: throttle_count,
                initial_stab_level_pct: MetricSummary::compute(&acc.throttle_stabs),
                time_to_stab_s: MetricSummary::compute(&acc.throttle_times_to_stab),
                fast_steps: MetricSummary::compute(&acc.throttle_fast_steps),
                progressive_ramp_rate_pct_s: MetricSummary::compute(&acc.throttle_ramp_rates),
                time_to_full_s: MetricSummary::compute(&acc.throttle_times_to_full),
            };

            report_cars.insert(
                car.clone(),
                CarStats {
                    car,
                    laps_count: acc.laps_count,
                    brake,
                    throttle,
                },
            );
        }

        TelemetryReport { cars: report_cars }
    }
}

/// Renders the statistics report as a formatted table string.
#[must_use]
pub fn format_stats_table(report: &TelemetryReport) -> String {
    use std::fmt::Write;

    if report.cars.is_empty() {
        return "No telemetry data found matching criteria.\n".to_string();
    }

    let mut out = String::new();

    let _ = writeln!(
        out,
        "========================================================================================================"
    );
    let _ = writeln!(
        out,
        "                                 TELEMETRY ZONE STATISTICS REPORT (MEDIAN / IQR)                         "
    );
    let _ = writeln!(
        out,
        "========================================================================================================\n"
    );

    for (car_name, stats) in &report.cars {
        let _ = writeln!(
            out,
            "CAR: {car_name} ({laps} lap(s), {bz} brake zone(s), {tz} throttle exit(s))",
            laps = stats.laps_count,
            bz = stats.brake.zone_count,
            tz = stats.throttle.zone_count
        );
        let _ = writeln!(
            out,
            "--------------------------------------------------------------------------------------------------------"
        );
        let _ = writeln!(out, "  BRAKE ZONES:");
        let _ = writeln!(
            out,
            "    Peak Pressure:            {:>6.1}%  (IQR: {:>5.1}%)",
            stats.brake.peak_pct.median, stats.brake.peak_pct.iqr
        );
        let _ = writeln!(
            out,
            "    Time to Peak:             {:>6.3}s  (IQR: {:>5.3}s)",
            stats.brake.time_to_peak_s.median, stats.brake.time_to_peak_s.iqr
        );
        let _ = writeln!(
            out,
            "    Trail/Release Duration:   {:>6.3}s  (IQR: {:>5.3}s)",
            stats.brake.trail_duration_s.median, stats.brake.trail_duration_s.iqr
        );

        let _ = writeln!(out, "  THROTTLE EXITS:");
        let _ = writeln!(
            out,
            "    Initial Stab Level (~150ms): {:>5.1}%  (IQR: {:>5.1}%)",
            stats.throttle.initial_stab_level_pct.median, stats.throttle.initial_stab_level_pct.iqr
        );
        let _ = writeln!(
            out,
            "    Time to Stab:                {:>5.3}s  (IQR: {:>5.3}s)",
            stats.throttle.time_to_stab_s.median, stats.throttle.time_to_stab_s.iqr
        );
        let _ = writeln!(
            out,
            "    Fast Steps (>=300%/s):       {:>5.1}   (IQR: {:>5.1})",
            stats.throttle.fast_steps.median, stats.throttle.fast_steps.iqr
        );
        let _ = writeln!(
            out,
            "    Progressive Ramp Rate:       {:>5.1}%/s (IQR: {:>5.1}%/s)",
            stats.throttle.progressive_ramp_rate_pct_s.median,
            stats.throttle.progressive_ramp_rate_pct_s.iqr
        );
        let _ = writeln!(
            out,
            "    Time to Full (98%):          {:>5.3}s  (IQR: {:>5.3}s)",
            stats.throttle.time_to_full_s.median, stats.throttle.time_to_full_s.iqr
        );
        let _ = writeln!(
            out,
            "--------------------------------------------------------------------------------------------------------\n"
        );
    }

    out
}
