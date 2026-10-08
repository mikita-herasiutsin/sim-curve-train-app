//! Extraction of pedal zones into validated preset JSON drills.

use sct_core::preset::{Drill, DrillKind, Pedal, Preset, SCHEMA_VERSION};
use std::path::Path;

use crate::csv::LapTelemetry;
use crate::simplify::{detect_plateau, process_trace_segment};
use crate::zones::{detect_brake_zones, detect_lift_zones, detect_throttle_exit_zones};

/// Sanitizes an arbitrary string into a valid preset/drill ID matching `[a-z0-9-]+`.
#[must_use]
pub fn sanitize_id(s: &str) -> String {
    let mut result = String::new();
    let mut last_was_dash = false;

    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            result.push(c.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            result.push('-');
            last_was_dash = true;
        }
    }

    let trimmed = result.trim_matches('-');
    if trimmed.is_empty() {
        "drill".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Extracts a clean track slug for drill IDs, e.g. "Road Atlanta (Full Course)" -> "road-atlanta".
#[must_use]
pub fn track_slug(track: &str) -> String {
    let ascii_track = track
        .replace(['ä', 'Ä'], "ae")
        .replace(['ö', 'Ö'], "oe")
        .replace(['ü', 'Ü'], "ue")
        .replace('ß', "ss");
    let base = if let Some(idx) = ascii_track.find('(') {
        let prefix = ascii_track[..idx].trim();
        if prefix.is_empty() {
            &ascii_track
        } else {
            prefix
        }
    } else {
        &ascii_track
    };
    let slug = sanitize_id(base);
    if slug.is_empty() {
        "track".to_string()
    } else {
        slug
    }
}

/// Options configuring preset extraction.
#[derive(Clone, Debug)]
pub struct ExtractOptions {
    /// Preset identifier (must conform to `[a-z0-9-]+`).
    pub preset_id: Option<String>,
    /// Human-readable preset name.
    pub preset_name: Option<String>,
    /// Target file path to write output JSON to.
    pub out_path: Option<String>,
    /// Permissible tolerance override in percent (omitted if None, applying D-17 default).
    pub tolerance: Option<f32>,
    /// Maximum number of drills to output (defaults to 12).
    pub max_drills: usize,
}

impl Default for ExtractOptions {
    fn default() -> Self {
        Self {
            preset_id: None,
            preset_name: None,
            out_path: None,
            tolerance: None,
            max_drills: 12,
        }
    }
}

#[derive(Clone, Debug)]
enum DecelZone {
    Brake(crate::zones::BrakeZone),
    Lift(crate::zones::LiftZone),
}

impl DecelZone {
    fn onset_idx(&self) -> usize {
        match self {
            Self::Brake(b) => b.onset_idx,
            Self::Lift(l) => l.onset_idx,
        }
    }

    fn metric(&self) -> f64 {
        match self {
            Self::Brake(b) => f64::from(b.peak_pct),
            Self::Lift(l) => f64::from(l.min_pct),
        }
    }

    fn priority_score(&self) -> (f64, f64) {
        match self {
            Self::Brake(b) => (f64::from(b.peak_pct), b.duration_s),
            Self::Lift(l) => (100.0 - f64::from(l.min_pct), l.duration_s),
        }
    }
}

#[derive(Clone, Debug)]
struct CornerCandidate {
    lap_idx: usize,
    onset_pct: f32,
    decel: DecelZone,
    throttle: Option<crate::zones::ThrottleExitZone>,
}

fn circular_dist(a: f32, b: f32) -> f32 {
    let diff = (a - b).abs();
    diff.min(1.0 - diff)
}

struct ChosenCorner {
    representative_lap: usize,
    mean_onset_pct: f32,
    decel: DecelZone,
    throttle: Option<crate::zones::ThrottleExitZone>,
    priority_score: (f64, f64),
    corner_num: usize,
    trace_only: bool,
}

/// Maximum distance in normalized lap distance (0.01 = 1% of lap) to cluster zones into the same corner.
pub const CORNER_CLUSTER_EPSILON: f32 = 0.01;

/// A throttle-hold whose target is this close (in percentage points) to the same corner's
/// lift-hold is the same plateau found twice, and is dropped.
pub const DUPLICATE_PLATEAU_PCT: f32 = 3.0;

/// Extracts grouped corner drills from multiple laps and compiles them into a validated [`Preset`].
///
/// # Errors
///
/// Returns an error string if preset validation fails or no valid drills could be extracted.
#[expect(
    clippy::too_many_lines,
    reason = "multi-lap corner clustering, representative selection, and drill generation"
)]
pub fn extract_preset_from_laps(
    laps: &[LapTelemetry],
    options: &ExtractOptions,
) -> Result<Preset, String> {
    if laps.is_empty() {
        return Err("no telemetry laps provided for extraction".to_string());
    }

    // 1. Ingest candidates across all laps
    let mut all_candidates: Vec<CornerCandidate> = Vec::new();

    for (lap_num, lap) in laps.iter().enumerate() {
        let b_zones = detect_brake_zones(&lap.brake);
        let l_zones = detect_lift_zones(&lap.throttle, &lap.brake);
        let t_zones = detect_throttle_exit_zones(&lap.throttle, &b_zones, &l_zones);

        let mut decel_list: Vec<DecelZone> = Vec::with_capacity(b_zones.len() + l_zones.len());
        for b in b_zones {
            decel_list.push(DecelZone::Brake(b));
        }
        for l in l_zones {
            decel_list.push(DecelZone::Lift(l));
        }
        decel_list.sort_by_key(DecelZone::onset_idx);

        for (k, decel) in decel_list.iter().enumerate() {
            let onset_idx = decel.onset_idx();
            let onset_pct = lap.lap_dist_pct.get(onset_idx).copied().unwrap_or(0.0);

            let next_onset = if k + 1 < decel_list.len() {
                decel_list[k + 1].onset_idx()
            } else {
                lap.throttle.len()
            };

            let matching_tz = t_zones
                .iter()
                .find(|tz| tz.onset_idx >= onset_idx && tz.onset_idx < next_onset)
                .cloned();

            all_candidates.push(CornerCandidate {
                lap_idx: lap_num,
                onset_pct,
                decel: decel.clone(),
                throttle: matching_tz,
            });
        }
    }

    if all_candidates.is_empty() {
        return Err("no drills could be extracted from telemetry data".to_string());
    }

    // 2. Group zones by corner across laps using LapDistPct
    all_candidates.sort_by(|a, b| {
        a.onset_pct
            .partial_cmp(&b.onset_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut clusters: Vec<Vec<CornerCandidate>> = Vec::new();

    for cand in all_candidates {
        let mut matched_idx = None;
        for (c_idx, cluster) in clusters.iter().enumerate() {
            let same_type = matches!(
                (&cluster[0].decel, &cand.decel),
                (DecelZone::Brake(_), DecelZone::Brake(_))
                    | (DecelZone::Lift(_), DecelZone::Lift(_))
            );
            let has_lap = cluster.iter().any(|c| c.lap_idx == cand.lap_idx);
            let close = cluster
                .iter()
                .any(|c| circular_dist(c.onset_pct, cand.onset_pct) <= CORNER_CLUSTER_EPSILON);

            if close && same_type && !has_lap {
                matched_idx = Some(c_idx);
                break;
            }
        }

        if let Some(c_idx) = matched_idx {
            clusters[c_idx].push(cand);
        } else {
            clusters.push(vec![cand]);
        }
    }

    // Merge wrap-around across 0/1 boundary
    if clusters.len() > 1 {
        let first_onset = clusters[0][0].onset_pct;
        let same_type = matches!(
            (
                &clusters[0][0].decel,
                &clusters[clusters.len() - 1][0].decel
            ),
            (DecelZone::Brake(_), DecelZone::Brake(_)) | (DecelZone::Lift(_), DecelZone::Lift(_))
        );
        let has_lap_overlap = clusters[0].iter().any(|c1| {
            clusters[clusters.len() - 1]
                .iter()
                .any(|c2| c1.lap_idx == c2.lap_idx)
        });
        let last_close = clusters[clusters.len() - 1]
            .iter()
            .any(|c| circular_dist(c.onset_pct, first_onset) <= CORNER_CLUSTER_EPSILON);

        if last_close && same_type && !has_lap_overlap {
            let last_cluster = clusters.pop().unwrap();
            clusters[0].extend(last_cluster);
        }
    }

    // 3. Keep only corners seen in at least half the laps
    let total_laps = laps.len();
    let min_laps_required = total_laps.div_ceil(2);

    let mut kept_clusters: Vec<Vec<CornerCandidate>> = Vec::new();
    for cluster in clusters {
        let mut seen_laps = std::collections::HashSet::new();
        for c in &cluster {
            seen_laps.insert(c.lap_idx);
        }
        if seen_laps.len() >= min_laps_required {
            kept_clusters.push(cluster);
        }
    }

    if kept_clusters.is_empty() {
        return Err("no corners seen in at least half the laps".to_string());
    }

    // 4. For each corner pick the zone from the lap with the median zone metric
    let mut chosen_corners: Vec<ChosenCorner> = Vec::new();

    for mut cluster in kept_clusters {
        // Keep at most 1 representative per lap
        cluster.sort_by(|a, b| {
            a.lap_idx
                .cmp(&b.lap_idx)
                .then_with(|| b.decel.metric().partial_cmp(&a.decel.metric()).unwrap())
        });
        cluster.dedup_by_key(|c| c.lap_idx);

        // Sort by metric to select median
        cluster.sort_by(|a, b| {
            a.decel
                .metric()
                .partial_cmp(&b.decel.metric())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let median_idx = cluster.len() / 2;
        let median_cand = &cluster[median_idx];

        #[expect(
            clippy::cast_precision_loss,
            reason = "cluster member count safely converts to f32"
        )]
        let count_f32 = cluster.len() as f32;
        let mut sum_sin = 0.0_f32;
        let mut sum_cos = 0.0_f32;
        for c in &cluster {
            let theta = c.onset_pct * 2.0 * std::f32::consts::PI;
            sum_sin += theta.sin();
            sum_cos += theta.cos();
        }
        let mut avg_onset =
            (sum_sin / count_f32).atan2(sum_cos / count_f32) / (2.0 * std::f32::consts::PI);
        if avg_onset < 0.0 {
            avg_onset += 1.0;
        }

        chosen_corners.push(ChosenCorner {
            representative_lap: median_cand.lap_idx,
            mean_onset_pct: avg_onset,
            decel: median_cand.decel.clone(),
            throttle: median_cand.throttle.clone(),
            priority_score: median_cand.decel.priority_score(),
            corner_num: 0,
            trace_only: false,
        });
    }

    // 4.5 Number all kept clusters by onset along the lap
    chosen_corners.sort_by(|a, b| {
        a.mean_onset_pct
            .partial_cmp(&b.mean_onset_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for (i, corner) in chosen_corners.iter_mut().enumerate() {
        corner.corner_num = i + 1;
    }

    // 5. Prioritize corners with the highest brake peak / longest zones to fit within max_drills
    chosen_corners.sort_by(|a, b| {
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "priority score fits in u32"
        )]
        let p_a = a.priority_score.0.round() as u32;
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "priority score fits in u32"
        )]
        let p_b = b.priority_score.0.round() as u32;
        p_b.cmp(&p_a).then_with(|| {
            b.priority_score
                .1
                .partial_cmp(&a.priority_score.1)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });

    let max_drills = options.max_drills.max(1);

    // Filter corners so drill budget is respected
    let mut selected_corners = Vec::new();
    let mut drill_budget = 0;

    for mut corner in chosen_corners {
        let rep_lap = &laps[corner.representative_lap];
        let mut count_for_corner = 1; // decel trace
        match &corner.decel {
            DecelZone::Brake(b) => {
                if detect_plateau(&rep_lap.brake, b.onset_idx, b.release_idx).is_some() {
                    count_for_corner += 1;
                }
            }
            DecelZone::Lift(l) => {
                if detect_plateau(&rep_lap.throttle, l.onset_idx, l.recovery_idx).is_some() {
                    count_for_corner += 1;
                }
            }
        }
        if let Some(ref tz) = corner.throttle {
            count_for_corner += 1; // throttle trace
            if detect_plateau(&rep_lap.throttle, tz.onset_idx, tz.full_idx).is_some() {
                count_for_corner += 1;
            }
        }

        if drill_budget + count_for_corner <= max_drills || selected_corners.is_empty() {
            drill_budget += count_for_corner;
            corner.trace_only = false;
            selected_corners.push(corner);
        } else if drill_budget < max_drills {
            // Include corner if trace drills fit
            let trace_only = if corner.throttle.is_some() { 2 } else { 1 };
            if drill_budget + trace_only <= max_drills {
                drill_budget += trace_only;
                corner.trace_only = true;
                selected_corners.push(corner);
            }
        }
    }

    // 6. Stable, deterministic ordering by LapDistPct
    selected_corners.sort_by(|a, b| {
        a.mean_onset_pct
            .partial_cmp(&b.mean_onset_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let track_name = laps[0]
        .metadata
        .as_ref()
        .map_or("track", |m| m.track.as_str());
    let t_slug = track_slug(track_name);

    let mut drills = Vec::new();

    for corner in &selected_corners {
        let corner_num = corner.corner_num;
        let rep_lap = &laps[corner.representative_lap];
        // Target of this corner's lift-hold. A lift zone runs until the throttle recovers, so it
        // overlaps the throttle exit and can find the same plateau again.
        let mut lift_hold_target: Option<f32> = None;

        match &corner.decel {
            DecelZone::Brake(bz) => {
                let trace_pts = process_trace_segment(&rep_lap.brake, bz.onset_idx, bz.release_idx);
                if trace_pts.len() >= 2 && drills.len() < max_drills {
                    drills.push(Drill {
                        id: sanitize_id(&format!("{t_slug}-c{corner_num:02}-brake")),
                        name: format!("Turn {corner_num} brake ({:.0}%)", bz.peak_pct),
                        pedal: Pedal::Brake,
                        reps: 5,
                        lead_in_ms: 2000,
                        tolerance: options.tolerance,
                        kind: DrillKind::Trace { points: trace_pts },
                    });
                }

                if !corner.trace_only
                    && let Some(plateau) =
                        detect_plateau(&rep_lap.brake, bz.onset_idx, bz.release_idx)
                    && drills.len() < max_drills
                {
                    drills.push(Drill {
                        id: sanitize_id(&format!("{t_slug}-c{corner_num:02}-brake-hold")),
                        name: format!("Turn {corner_num} brake hold ({:.0}%)", plateau.target),
                        pedal: Pedal::Brake,
                        reps: 5,
                        lead_in_ms: 2000,
                        tolerance: options.tolerance,
                        kind: DrillKind::Hold {
                            target: plateau.target,
                            hold_ms: plateau.hold_ms,
                        },
                    });
                }
            }
            DecelZone::Lift(lz) => {
                let trace_pts =
                    process_trace_segment(&rep_lap.throttle, lz.onset_idx, lz.recovery_idx);
                if trace_pts.len() >= 2 && drills.len() < max_drills {
                    drills.push(Drill {
                        id: sanitize_id(&format!("{t_slug}-c{corner_num:02}-lift")),
                        name: format!("Turn {corner_num} lift ({:.0}%)", lz.min_pct),
                        pedal: Pedal::Throttle,
                        reps: 5,
                        lead_in_ms: 2000,
                        tolerance: options.tolerance,
                        kind: DrillKind::Trace { points: trace_pts },
                    });
                }

                if !corner.trace_only
                    && let Some(plateau) =
                        detect_plateau(&rep_lap.throttle, lz.onset_idx, lz.recovery_idx)
                    && drills.len() < max_drills
                {
                    lift_hold_target = Some(plateau.target);
                    drills.push(Drill {
                        id: sanitize_id(&format!("{t_slug}-c{corner_num:02}-lift-hold")),
                        name: format!("Turn {corner_num} lift hold ({:.0}%)", plateau.target),
                        pedal: Pedal::Throttle,
                        reps: 5,
                        lead_in_ms: 2000,
                        tolerance: options.tolerance,
                        kind: DrillKind::Hold {
                            target: plateau.target,
                            hold_ms: plateau.hold_ms,
                        },
                    });
                }
            }
        }

        if let Some(ref tz) = corner.throttle {
            let trace_pts = process_trace_segment(&rep_lap.throttle, tz.onset_idx, tz.full_idx);
            if trace_pts.len() >= 2 && drills.len() < max_drills {
                drills.push(Drill {
                    id: sanitize_id(&format!("{t_slug}-c{corner_num:02}-throttle")),
                    name: format!("Turn {corner_num} throttle"),
                    pedal: Pedal::Throttle,
                    reps: 5,
                    lead_in_ms: 2000,
                    tolerance: options.tolerance,
                    kind: DrillKind::Trace { points: trace_pts },
                });
            }

            if !corner.trace_only
                && let Some(plateau) = detect_plateau(&rep_lap.throttle, tz.onset_idx, tz.full_idx)
                && !lift_hold_target
                    .is_some_and(|t| (t - plateau.target).abs() <= DUPLICATE_PLATEAU_PCT)
                && drills.len() < max_drills
            {
                drills.push(Drill {
                    id: sanitize_id(&format!("{t_slug}-c{corner_num:02}-throttle-hold")),
                    name: format!("Turn {corner_num} throttle hold ({:.0}%)", plateau.target),
                    pedal: Pedal::Throttle,
                    reps: 5,
                    lead_in_ms: 2000,
                    tolerance: options.tolerance,
                    kind: DrillKind::Hold {
                        target: plateau.target,
                        hold_ms: plateau.hold_ms,
                    },
                });
            }
        }
    }

    if drills.is_empty() {
        return Err("no drills could be extracted from telemetry data".to_string());
    }

    let mut valid_drills = Vec::new();
    for drill in drills {
        let dummy_preset = Preset {
            schema_version: SCHEMA_VERSION,
            id: "dummy".to_string(),
            name: "dummy".to_string(),
            description: String::new(),
            drills: vec![drill.clone()],
        };
        if let Err(e) = dummy_preset.validate() {
            eprintln!("warning: dropping invalid drill '{}': {e}", drill.id);
        } else {
            valid_drills.push(drill);
        }
    }

    if valid_drills.is_empty() {
        return Err("all extracted drills failed validation".to_string());
    }

    let default_car = laps[0].car_name();
    let preset_id = options.preset_id.as_deref().map_or_else(
        || sanitize_id(&format!("{default_car}-extracted")),
        sanitize_id,
    );

    let preset_name = options
        .preset_name
        .clone()
        .unwrap_or_else(|| format!("{default_car} Extracted Drills"));

    let preset = Preset {
        schema_version: SCHEMA_VERSION,
        id: preset_id,
        name: preset_name,
        description: format!(
            "Pedal practice drills extracted from Garage 61 telemetry for {default_car}."
        ),
        drills: valid_drills,
    };

    // Strict validation via sct-core
    preset
        .validate()
        .map_err(|e| format!("preset validation failed: {e}"))?;

    // Verify round-trip parsing matches sct-core expectations
    let json = serde_json::to_string_pretty(&preset)
        .map_err(|e| format!("JSON serialization error: {e}"))?;
    sct_core::preset::parse_preset(&json)
        .map_err(|e| format!("sct-core parse_preset verification failed: {e}"))?;

    Ok(preset)
}

/// Runs the extraction command and writes output to file or stdout.
///
/// # Errors
///
/// Returns an error string if extraction or writing fails.
pub fn run_extract(laps: &[LapTelemetry], options: &ExtractOptions) -> Result<(), String> {
    let preset = extract_preset_from_laps(laps, options)?;
    let json = serde_json::to_string_pretty(&preset)
        .map_err(|e| format!("JSON serialization error: {e}"))?;

    if let Some(ref out_path) = options.out_path {
        std::fs::write(Path::new(out_path), &json)
            .map_err(|e| format!("failed to write preset JSON to '{out_path}': {e}"))?;
        println!(
            "Successfully extracted {n} drills to '{out_path}'.",
            n = preset.drills.len()
        );
    } else {
        println!("{json}");
    }

    Ok(())
}
