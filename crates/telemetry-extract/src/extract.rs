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
    /// Reserve drill slots for at least this many brake corners, top-ranked first (defaults to 0).
    pub min_brake: usize,
    /// Reserve drill slots for at least this many lift corners, top-ranked first (defaults to 0).
    pub min_lift: usize,
}

impl ExtractOptions {
    /// Checks that the reserved slots fit in the drill budget.
    ///
    /// # Errors
    ///
    /// Returns an error string when `max_drills` is 0 or `min_brake + min_lift` exceeds it.
    pub fn validate(&self) -> Result<(), String> {
        if self.max_drills == 0 {
            return Err("--max-drills must be a positive integer".to_string());
        }
        if self
            .min_brake
            .checked_add(self.min_lift)
            .is_none_or(|sum| sum > self.max_drills)
        {
            return Err(format!(
                "--min-brake {} plus --min-lift {} must not exceed --max-drills {}",
                self.min_brake, self.min_lift, self.max_drills
            ));
        }
        Ok(())
    }
}

impl Default for ExtractOptions {
    fn default() -> Self {
        Self {
            preset_id: None,
            preset_name: None,
            out_path: None,
            tolerance: None,
            max_drills: 12,
            min_brake: 0,
            min_lift: 0,
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

    fn is_brake(&self) -> bool {
        matches!(self, Self::Brake(_))
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

/// Circular mean of the candidates' onsets, in `0.0..1.0`.
fn circular_mean(cands: &[CornerCandidate]) -> f32 {
    let (sin, cos) = cands.iter().fold((0.0_f32, 0.0_f32), |(s, c), cand| {
        let theta = cand.onset_pct * std::f32::consts::TAU;
        (s + theta.sin(), c + theta.cos())
    });
    (sin.atan2(cos) / std::f32::consts::TAU).rem_euclid(1.0)
}

/// The drills one corner yields, in lap order. With `trace_only` the hold drills are left out.
fn corner_drills(
    corner: &ChosenCorner,
    rep_lap: &LapTelemetry,
    t_slug: &str,
    options: &ExtractOptions,
    trace_only: bool,
) -> Vec<Drill> {
    let n = corner.corner_num;
    let drill = |suffix: &str, name: String, pedal: Pedal, kind: DrillKind| Drill {
        id: sanitize_id(&format!("{t_slug}-c{n:02}-{suffix}")),
        name,
        pedal,
        reps: 5,
        lead_in_ms: 2000,
        tolerance: options.tolerance,
        decimals: None,
        kind,
    };
    let trace = |pedal: &[f32], start: usize, end: usize| {
        let points = process_trace_segment(pedal, start, end);
        (points.len() >= 2).then_some(DrillKind::Trace { points })
    };
    let hold = |pedal: &[f32], start: usize, end: usize| {
        if trace_only {
            None
        } else {
            detect_plateau(pedal, start, end)
        }
    };

    let mut drills = Vec::new();
    // Target of this corner's lift-hold. A lift zone runs until the throttle recovers, so it
    // overlaps the throttle exit and can find the same plateau again.
    let mut lift_hold_target: Option<f32> = None;

    match &corner.decel {
        DecelZone::Brake(bz) => {
            if let Some(kind) = trace(&rep_lap.brake, bz.onset_idx, bz.release_idx) {
                let name = format!("Turn {n} brake ({:.0}%)", bz.peak_pct);
                drills.push(drill("brake", name, Pedal::Brake, kind));
            }
            if let Some(p) = hold(&rep_lap.brake, bz.onset_idx, bz.release_idx) {
                let name = format!("Turn {n} brake hold ({:.0}%)", p.target);
                let kind = DrillKind::Hold {
                    target: p.target,
                    hold_ms: p.hold_ms,
                };
                drills.push(drill("brake-hold", name, Pedal::Brake, kind));
            }
        }
        DecelZone::Lift(lz) => {
            if let Some(kind) = trace(&rep_lap.throttle, lz.onset_idx, lz.recovery_idx) {
                let name = format!("Turn {n} lift ({:.0}%)", lz.min_pct);
                drills.push(drill("lift", name, Pedal::Throttle, kind));
            }
            if let Some(p) = hold(&rep_lap.throttle, lz.onset_idx, lz.recovery_idx) {
                lift_hold_target = Some(p.target);
                let name = format!("Turn {n} lift hold ({:.0}%)", p.target);
                let kind = DrillKind::Hold {
                    target: p.target,
                    hold_ms: p.hold_ms,
                };
                drills.push(drill("lift-hold", name, Pedal::Throttle, kind));
            }
        }
    }

    if let Some(tz) = &corner.throttle {
        if let Some(kind) = trace(&rep_lap.throttle, tz.onset_idx, tz.full_idx) {
            drills.push(drill(
                "throttle",
                format!("Turn {n} throttle"),
                Pedal::Throttle,
                kind,
            ));
        }
        if let Some(p) = hold(&rep_lap.throttle, tz.onset_idx, tz.full_idx)
            && !lift_hold_target.is_some_and(|t| (t - p.target).abs() <= DUPLICATE_PLATEAU_PCT)
        {
            let name = format!("Turn {n} throttle hold ({:.0}%)", p.target);
            let kind = DrillKind::Hold {
                target: p.target,
                hold_ms: p.hold_ms,
            };
            drills.push(drill("throttle-hold", name, Pedal::Throttle, kind));
        }
    }

    drills
}

struct ChosenCorner {
    representative_lap: usize,
    mean_onset_pct: f32,
    decel: DecelZone,
    throttle: Option<crate::zones::ThrottleExitZone>,
    priority_score: (f64, f64),
    corner_num: usize,
}

/// Maximum distance in normalized lap distance (0.01 = 1% of lap) to cluster zones into the same corner.
pub const CORNER_CLUSTER_EPSILON: f32 = 0.01;

/// A throttle-hold whose target is this close (in percentage points) to the same corner's
/// lift-hold is the same plateau found twice, and is dropped.
pub const DUPLICATE_PLATEAU_PCT: f32 = 3.0;

/// Fills the drill budget from corners sorted by rank. The first `min_brake` brake corners and
/// first `min_lift` lift corners are placed first, each leaving room for the reserved corners
/// still to come. The other corners then fill what is left in rank order. Returns the drills per
/// corner with the corner's mean onset, and warning lines.
fn select_drills(
    chosen_corners: &[ChosenCorner],
    laps: &[LapTelemetry],
    t_slug: &str,
    options: &ExtractOptions,
) -> (Vec<(f32, Vec<Drill>)>, Vec<String>) {
    let max_drills = options.max_drills.max(1);
    let mut warnings = Vec::new();

    let mut reserved = vec![false; chosen_corners.len()];
    for (want, is_brake, flag) in [
        (options.min_brake, true, "--min-brake"),
        (options.min_lift, false, "--min-lift"),
    ] {
        let found: Vec<usize> = chosen_corners
            .iter()
            .enumerate()
            .filter(|(_, c)| c.decel.is_brake() == is_brake)
            .map(|(i, _)| i)
            .take(want)
            .collect();
        if found.len() < want {
            let kind = if is_brake { "brake" } else { "lift" };
            warnings.push(format!(
                "warning: {flag} {want} asked, only {} {kind} corners found",
                found.len()
            ));
        }
        for i in found {
            reserved[i] = true;
        }
    }

    let mut selected: Vec<(f32, Vec<Drill>)> = Vec::new();
    let mut drill_budget = 0;

    // Pass 1: reserved corners. Each leaves one slot for every reserved corner after it.
    let mut to_place = reserved.iter().filter(|&&r| r).count();
    for (corner, _) in chosen_corners.iter().zip(&reserved).filter(|(_, r)| **r) {
        to_place -= 1;
        let room = max_drills.saturating_sub(drill_budget + to_place);
        let rep_lap = &laps[corner.representative_lap];
        let full = corner_drills(corner, rep_lap, t_slug, options, false);
        let chosen = if full.len() <= room {
            full
        } else {
            let mut traces = corner_drills(corner, rep_lap, t_slug, options, true);
            if traces.len() > room {
                traces.truncate(1);
            }
            traces
        };
        let (kind, suffixes) = if corner.decel.is_brake() {
            ("brake", ["-brake", "-brake-hold"])
        } else {
            ("lift", ["-lift", "-lift-hold"])
        };
        if !chosen
            .iter()
            .any(|d| suffixes.iter().any(|s| d.id.ends_with(s)))
        {
            warnings.push(format!(
                "warning: reserved {kind} corner {} yielded no {kind} drill",
                corner.corner_num
            ));
        }
        if chosen.is_empty() {
            continue;
        }
        drill_budget += chosen.len();
        selected.push((corner.mean_onset_pct, chosen));
    }

    // Pass 2: the other corners. Charge each corner what it really emits: holds that turn out
    // empty or duplicate are dropped before the budget sees them. A corner that does not fit
    // whole may still fit as traces only.
    for (corner, _) in chosen_corners.iter().zip(&reserved).filter(|(_, r)| !**r) {
        let rep_lap = &laps[corner.representative_lap];
        let full = corner_drills(corner, rep_lap, t_slug, options, false);
        let chosen = if drill_budget + full.len() <= max_drills {
            full
        } else {
            let traces = corner_drills(corner, rep_lap, t_slug, options, true);
            if drill_budget + traces.len() <= max_drills {
                traces
            } else if selected.is_empty() {
                // The top corner alone is over budget: keep its first drills.
                traces.into_iter().take(max_drills).collect()
            } else {
                continue;
            }
        };
        if chosen.is_empty() {
            continue;
        }
        drill_budget += chosen.len();
        selected.push((corner.mean_onset_pct, chosen));
    }

    (selected, warnings)
}

/// Extracts grouped corner drills from multiple laps and compiles them into a validated [`Preset`].
///
/// # Errors
///
/// Returns an error string if preset validation fails or no valid drills could be extracted.
pub fn extract_preset_from_laps(
    laps: &[LapTelemetry],
    options: &ExtractOptions,
) -> Result<Preset, String> {
    let (preset, warnings) = extract_with_warnings(laps, options)?;
    for w in &warnings {
        eprintln!("{w}");
    }
    Ok(preset)
}

/// Like [`extract_preset_from_laps`], but returns the warning lines instead of printing them.
///
/// # Errors
///
/// Returns an error string if preset validation fails or no valid drills could be extracted.
#[expect(
    clippy::too_many_lines,
    reason = "multi-lap corner clustering, representative selection, and drill generation"
)]
pub(crate) fn extract_with_warnings(
    laps: &[LapTelemetry],
    options: &ExtractOptions,
) -> Result<(Preset, Vec<String>), String> {
    options.validate()?;
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

    // Each candidate joins the nearest cluster (by circular mean onset) of the same type that has
    // no zone from its lap yet. Nearest, not first-fit, so a short stab just before a corner's
    // main zone cannot pull that main zone away from its own cluster. `circular_dist` lets a
    // corner straddling the 0/1 line form a single cluster.
    let mut clusters: Vec<Vec<CornerCandidate>> = Vec::new();

    for cand in all_candidates {
        let nearest = clusters
            .iter()
            .enumerate()
            .filter(|(_, cluster)| {
                cluster[0].decel.is_brake() == cand.decel.is_brake()
                    && !cluster.iter().any(|c| c.lap_idx == cand.lap_idx)
            })
            .map(|(c_idx, cluster)| (c_idx, circular_dist(circular_mean(cluster), cand.onset_pct)))
            .filter(|&(_, dist)| dist <= CORNER_CLUSTER_EPSILON)
            .min_by(|a, b| a.1.total_cmp(&b.1));

        if let Some((c_idx, _)) = nearest {
            clusters[c_idx].push(cand);
        } else {
            clusters.push(vec![cand]);
        }
    }

    // A corner braked in some laps and lifted in others forms a brake and a lift cluster with
    // disjoint laps. Merge each lift cluster into the nearest brake cluster within the epsilon
    // that shares no lap with it, each brake cluster taking at most one. Lift clusters go in onset
    // order. A lift cluster that overlaps in laps is a separate corner and stays alone.
    let lift_order = {
        let mut v: Vec<usize> = (0..clusters.len())
            .filter(|&i| !clusters[i][0].decel.is_brake())
            .collect();
        v.sort_by(|&a, &b| circular_mean(&clusters[a]).total_cmp(&circular_mean(&clusters[b])));
        v
    };
    let mut merged_away = vec![false; clusters.len()];
    let mut has_merge = vec![false; clusters.len()];
    for l in lift_order {
        let lift_mean = circular_mean(&clusters[l]);
        let nearest = (0..clusters.len())
            .filter(|&b| !merged_away[b] && clusters[b][0].decel.is_brake() && !has_merge[b])
            .filter(|&b| {
                !clusters[b]
                    .iter()
                    .any(|c| clusters[l].iter().any(|d| d.lap_idx == c.lap_idx))
            })
            .map(|b| (b, circular_dist(circular_mean(&clusters[b]), lift_mean)))
            .filter(|&(_, dist)| dist <= CORNER_CLUSTER_EPSILON)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((b, _)) = nearest {
            let lifted = std::mem::take(&mut clusters[l]);
            clusters[b].extend(lifted);
            merged_away[l] = true;
            has_merge[b] = true;
        }
    }
    let clusters: Vec<Vec<CornerCandidate>> = clusters
        .into_iter()
        .zip(merged_away)
        .filter_map(|(c, gone)| (!gone).then_some(c))
        .collect();

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

    // 4. For each corner take the majority kind (a tie goes to brake) and pick the zone from the
    // lap with the median metric among zones of that kind. Clustering already keeps at most one
    // zone per lap.
    let mut chosen_corners: Vec<ChosenCorner> = Vec::new();

    for cluster in kept_clusters {
        let brakes = cluster.iter().filter(|c| c.decel.is_brake()).count();
        let majority_is_brake = brakes * 2 >= cluster.len();
        let mut of_kind: Vec<&CornerCandidate> = cluster
            .iter()
            .filter(|c| c.decel.is_brake() == majority_is_brake)
            .collect();
        of_kind.sort_by(|a, b| a.decel.metric().total_cmp(&b.decel.metric()));
        let median_cand = of_kind[of_kind.len() / 2];

        chosen_corners.push(ChosenCorner {
            representative_lap: median_cand.lap_idx,
            mean_onset_pct: circular_mean(&cluster),
            decel: median_cand.decel.clone(),
            throttle: median_cand.throttle.clone(),
            priority_score: median_cand.decel.priority_score(),
            corner_num: 0,
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

    let track_name = laps[0]
        .metadata
        .as_ref()
        .map_or("track", |m| m.track.as_str());
    let t_slug = track_slug(track_name);

    let (mut selected, mut warnings) = select_drills(&chosen_corners, laps, &t_slug, options);

    // 6. Stable, deterministic ordering by LapDistPct
    selected.sort_by(|a, b| a.0.total_cmp(&b.0));
    let drills: Vec<Drill> = selected.into_iter().flat_map(|(_, d)| d).collect();

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
            warm_up: None,
        };
        if let Err(e) = dummy_preset.validate() {
            warnings.push(format!(
                "warning: dropping invalid drill '{}': {e}",
                drill.id
            ));
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
        warm_up: None,
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

    Ok((preset, warnings))
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
