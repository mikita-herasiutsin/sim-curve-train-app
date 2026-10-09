//! Detection of braking zones and corner exit throttle zones.

use crate::csv::TELEMETRY_HZ;

/// Hysteresis onset threshold for braking (5% pedal travel).
pub const BRAKE_ONSET_THRESHOLD: f32 = 0.05;

/// Hysteresis release threshold for braking (2% pedal travel).
pub const BRAKE_RELEASE_THRESHOLD: f32 = 0.02;

/// Minimum duration in seconds for a valid braking zone (100 ms).
pub const BRAKE_MIN_DURATION_S: f64 = 0.10;

/// Minimum peak pedal percentage for a valid braking zone (20%).
pub const BRAKE_MIN_PEAK: f32 = 0.20;

/// Maximum gap between braking pulses in samples (~250 ms at 60 Hz) to merge.
pub const BRAKE_MERGE_GAP_SAMPLES: usize = 15;

/// Threshold considered "full throttle" (98%).
pub const THROTTLE_FULL_THRESHOLD: f32 = 0.98;

/// Minimum consecutive frames at or above 98% throttle to consider it sustained.
pub const THROTTLE_SUSTAINED_FRAMES: usize = 5;

/// Minimum duration in seconds for a corner exit throttle zone (150 ms).
pub const THROTTLE_MIN_DURATION_S: f64 = 0.15;

/// Threshold above which throttle is considered high/full before a lift (95%).
pub const LIFT_ENTRY_THRESHOLD: f32 = 0.95;

/// Threshold below which throttle must drop to qualify as a lift zone (85%).
pub const LIFT_THRESHOLD: f32 = 0.85;

/// Threshold considered recovery from lift (98%).
pub const LIFT_RECOVERY_THRESHOLD: f32 = 0.98;

/// Minimum duration in seconds for a valid lift zone (150 ms).
pub const LIFT_MIN_DURATION_S: f64 = 0.15;

/// Maximum gap between lift pulses in samples (~250 ms at 60 Hz) to merge.
pub const LIFT_MERGE_GAP_SAMPLES: usize = 15;

/// Detected braking event.
#[derive(Clone, Debug, PartialEq)]
pub struct BrakeZone {
    /// Frame index where braking begins (crosses `BRAKE_ONSET_THRESHOLD`).
    pub onset_idx: usize,
    /// Frame index where braking ends (drops below `BRAKE_RELEASE_THRESHOLD`).
    pub release_idx: usize,
    /// Frame index where peak braking occurred.
    pub peak_idx: usize,
    /// Peak brake value in percent `[0.0, 100.0]`.
    pub peak_pct: f32,
    /// Zone duration in seconds from onset to release.
    pub duration_s: f64,
}

/// Detected throttle lift event entering a corner without braking.
#[derive(Clone, Debug, PartialEq)]
pub struct LiftZone {
    /// Frame index where throttle drops below 95%.
    pub onset_idx: usize,
    /// Frame index where throttle reaches minimum in this zone.
    pub min_idx: usize,
    /// Frame index where throttle recovers to >= 98%.
    pub recovery_idx: usize,
    /// Minimum throttle percentage reached in the zone `[0.0, 100.0]`.
    pub min_pct: f32,
    /// Zone duration in seconds from onset to recovery.
    pub duration_s: f64,
}

/// Detected throttle exit acceleration event following a braking zone or lift zone.
#[derive(Clone, Debug, PartialEq)]
pub struct ThrottleExitZone {
    /// Frame index of the throttle minimum prior to application.
    pub min_idx: usize,
    /// Frame index where throttle application begins towards corner exit.
    pub onset_idx: usize,
    /// Frame index where throttle first reaches $\ge 98\%$ and stays sustained.
    pub full_idx: usize,
    /// Duration in seconds from onset to full throttle.
    pub duration_s: f64,
}

/// Detects all valid braking zones in a telemetry trace using dual-threshold hysteresis,
/// transient filtering, and gap merging.
#[must_use]
pub fn detect_brake_zones(brakes: &[f32]) -> Vec<BrakeZone> {
    if brakes.is_empty() {
        return Vec::new();
    }

    // 1. Raw hysteresis state machine: collect candidate [start, end] intervals.
    let mut raw_intervals: Vec<(usize, usize)> = Vec::new();
    let mut in_brake = false;
    let mut onset = 0;

    for (i, &b) in brakes.iter().enumerate() {
        if !in_brake {
            if b >= BRAKE_ONSET_THRESHOLD {
                in_brake = true;
                onset = i;
            }
        } else if b < BRAKE_RELEASE_THRESHOLD {
            in_brake = false;
            raw_intervals.push((onset, i));
        }
    }

    if in_brake {
        raw_intervals.push((onset, brakes.len().saturating_sub(1)));
    }

    if raw_intervals.is_empty() {
        return Vec::new();
    }

    // 2. Merge adjacent intervals separated by less than BRAKE_MERGE_GAP_SAMPLES.
    let mut merged_intervals: Vec<(usize, usize)> = Vec::new();
    let mut current = raw_intervals[0];

    for &next in &raw_intervals[1..] {
        if next.0.saturating_sub(current.1) < BRAKE_MERGE_GAP_SAMPLES {
            current.1 = next.1;
        } else {
            merged_intervals.push(current);
            current = next;
        }
    }
    merged_intervals.push(current);

    // 3. Filter by duration and peak, and construct BrakeZone.
    let mut zones = Vec::new();
    let len = brakes.len();
    for (start, end) in merged_intervals {
        if end <= start || start == 0 || end == len.saturating_sub(1) {
            continue;
        }

        #[expect(
            clippy::cast_precision_loss,
            reason = "sample count safely converts to f64"
        )]
        let duration_s = (end - start) as f64 / TELEMETRY_HZ;
        if duration_s < BRAKE_MIN_DURATION_S {
            continue;
        }
        if duration_s > 60.0 {
            eprintln!("warning: skipping brake zone > 60s ({duration_s:.1}s)");
            continue;
        }

        // Find peak within [start, end]
        let mut peak_idx = start;
        let mut peak_val = brakes[start];
        for (i, &val) in brakes.iter().enumerate().take(end + 1).skip(start) {
            if val > peak_val {
                peak_val = val;
                peak_idx = i;
            }
        }

        if peak_val < BRAKE_MIN_PEAK {
            continue;
        }

        zones.push(BrakeZone {
            onset_idx: start,
            release_idx: end,
            peak_idx,
            peak_pct: peak_val * 100.0,
            duration_s,
        });
    }

    zones
}

/// Detects valid lift zones where throttle drops from $\ge 95\%$ to below $85\%$
/// and recovers to $\ge 98\%$, without active braking during the lift.
#[must_use]
pub fn detect_lift_zones(throttles: &[f32], brakes: &[f32]) -> Vec<LiftZone> {
    if throttles.is_empty() {
        return Vec::new();
    }

    let mut raw_intervals: Vec<(usize, usize, usize, f32)> = Vec::new();
    let mut in_lift = false;
    let mut onset = 0;
    let mut min_idx = 0;
    let mut min_val = 1.0f32;
    let mut last_full = false;

    for (i, &t) in throttles.iter().enumerate() {
        if t >= 0.95 {
            last_full = true;
        }

        if in_lift {
            if t < min_val {
                min_val = t;
                min_idx = i;
            }
            if t >= LIFT_RECOVERY_THRESHOLD {
                in_lift = false;
                raw_intervals.push((onset, min_idx, i, min_val));
            }
        } else if t < LIFT_ENTRY_THRESHOLD && last_full {
            in_lift = true;
            onset = i;
            min_idx = i;
            min_val = t;
        }
    }

    if in_lift {
        raw_intervals.push((onset, min_idx, throttles.len().saturating_sub(1), min_val));
    }

    // Drop lifts into braking before merging, so a real lift just before one is not merged
    // into it and thrown out with it.
    raw_intervals.retain(|&(start, _, end, _)| {
        let b_end = (end + 1).min(brakes.len());
        brakes[start.min(b_end)..b_end]
            .iter()
            .all(|&b| b < BRAKE_ONSET_THRESHOLD)
    });

    if raw_intervals.is_empty() {
        return Vec::new();
    }

    // Merge adjacent intervals separated by less than LIFT_MERGE_GAP_SAMPLES
    let mut merged: Vec<(usize, usize, usize, f32)> = Vec::new();
    let mut current = raw_intervals[0];

    for &next in &raw_intervals[1..] {
        if next.0.saturating_sub(current.2) < LIFT_MERGE_GAP_SAMPLES {
            if next.3 < current.3 {
                current.1 = next.1;
                current.3 = next.3;
            }
            current.2 = next.2;
        } else {
            merged.push(current);
            current = next;
        }
    }
    merged.push(current);

    let mut zones = Vec::new();
    let len = throttles.len();
    for (start, min_i, end, min_v) in merged {
        if start == 0 || end == len.saturating_sub(1) {
            continue;
        }
        if min_v >= LIFT_THRESHOLD {
            continue;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "sample count safely converts to f64"
        )]
        let duration_s = (end.saturating_sub(start)) as f64 / TELEMETRY_HZ;
        if duration_s < LIFT_MIN_DURATION_S {
            continue;
        }
        if duration_s > 60.0 {
            eprintln!("warning: skipping lift zone > 60s ({duration_s:.1}s)");
            continue;
        }

        zones.push(LiftZone {
            onset_idx: start,
            min_idx: min_i,
            recovery_idx: end,
            min_pct: min_v * 100.0,
            duration_s,
        });
    }

    zones
}

/// First index in `from..end` where the throttle reaches full and stays at or above 95 % for
/// `THROTTLE_SUSTAINED_FRAMES`. The whole window must fit before `end`, so a brief spike at the
/// end of the trace or just before the next corner does not count.
fn find_sustained_full(throttles: &[f32], from: usize, end: usize) -> Option<usize> {
    let last = end
        .min(throttles.len())
        .checked_sub(THROTTLE_SUSTAINED_FRAMES)?;
    (from..=last).find(|&i| {
        throttles[i] >= THROTTLE_FULL_THRESHOLD
            && throttles[i..i + THROTTLE_SUSTAINED_FRAMES]
                .iter()
                .all(|&v| v >= 0.95)
    })
}

enum DecelRef<'a> {
    Brake(&'a BrakeZone),
    Lift(&'a LiftZone),
}

impl DecelRef<'_> {
    fn onset_idx(&self) -> usize {
        match self {
            DecelRef::Brake(b) => b.onset_idx,
            DecelRef::Lift(l) => l.onset_idx,
        }
    }
}

/// Detects throttle exit zones associated with each braking zone or lift zone.
///
/// A corner exit throttle zone starts after either a brake zone or a lift zone.
/// For a brake zone, it starts from the throttle minimum in or following the braking zone.
/// For a lift zone, onset is the throttle minimum in the zone.
/// In both cases, it terminates when throttle reaches and stays sustained at $\ge 98\%$.
#[must_use]
pub fn detect_throttle_exit_zones(
    throttles: &[f32],
    brake_zones: &[BrakeZone],
    lift_zones: &[LiftZone],
) -> Vec<ThrottleExitZone> {
    if throttles.is_empty() || (brake_zones.is_empty() && lift_zones.is_empty()) {
        return Vec::new();
    }

    let mut events: Vec<DecelRef<'_>> = Vec::with_capacity(brake_zones.len() + lift_zones.len());
    for b in brake_zones {
        events.push(DecelRef::Brake(b));
    }
    for l in lift_zones {
        events.push(DecelRef::Lift(l));
    }
    events.sort_by_key(DecelRef::onset_idx);

    let mut exit_zones = Vec::new();

    for (k, event) in events.iter().enumerate() {
        let window_end = if k + 1 < events.len() {
            events[k + 1].onset_idx()
        } else {
            throttles.len()
        };

        match event {
            DecelRef::Brake(brake_zone) => {
                let window_start = brake_zone.peak_idx;
                if window_end <= window_start + 10 {
                    continue;
                }

                // 1. Find throttle minimum in window
                let mut min_idx = window_start;
                let mut min_val = throttles[window_start];
                let search_limit = (brake_zone.release_idx + 120).min(window_end);
                for (i, &t) in throttles
                    .iter()
                    .enumerate()
                    .take(search_limit)
                    .skip(window_start)
                {
                    if t <= min_val {
                        min_val = t;
                        min_idx = i;
                    }
                }

                // 2. Find onset where throttle begins rising from minimum
                let onset_threshold = (min_val + 0.02).max(0.05);
                let mut onset_idx = None;
                for (i, &t) in throttles.iter().enumerate().take(window_end).skip(min_idx) {
                    if t >= onset_threshold {
                        onset_idx = Some(i);
                        break;
                    }
                }

                let Some(onset) = onset_idx else {
                    continue;
                };

                // 3. Find where throttle reaches >= 98% sustained
                let Some(full) = find_sustained_full(throttles, onset, window_end) else {
                    continue;
                };

                if full <= onset {
                    continue;
                }

                #[expect(
                    clippy::cast_precision_loss,
                    reason = "sample count safely converts to f64"
                )]
                let duration_s = (full - onset) as f64 / TELEMETRY_HZ;
                if duration_s < THROTTLE_MIN_DURATION_S {
                    continue;
                }

                exit_zones.push(ThrottleExitZone {
                    min_idx,
                    onset_idx: onset,
                    full_idx: full,
                    duration_s,
                });
            }
            DecelRef::Lift(lift_zone) => {
                // For a lift zone: onset = throttle minimum in the zone
                let onset = lift_zone.min_idx;
                let min_idx = lift_zone.min_idx;

                if window_end <= onset + 5 {
                    continue;
                }

                // Find where throttle reaches >= 98% sustained
                let Some(full) = find_sustained_full(throttles, onset, window_end) else {
                    continue;
                };

                if full <= onset {
                    continue;
                }

                #[expect(
                    clippy::cast_precision_loss,
                    reason = "sample count safely converts to f64"
                )]
                let duration_s = (full - onset) as f64 / TELEMETRY_HZ;
                if duration_s < THROTTLE_MIN_DURATION_S {
                    continue;
                }

                exit_zones.push(ThrottleExitZone {
                    min_idx,
                    onset_idx: onset,
                    full_idx: full,
                    duration_s,
                });
            }
        }
    }

    exit_zones
}
