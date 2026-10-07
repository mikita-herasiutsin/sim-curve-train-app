//! Extraction of pedal zones into validated preset JSON drills.

use sct_core::preset::{Drill, DrillKind, Pedal, Preset, SCHEMA_VERSION};
use std::path::Path;

use crate::csv::LapTelemetry;
use crate::simplify::{detect_plateau, process_trace_segment};
use crate::zones::{detect_brake_zones, detect_throttle_exit_zones};

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

/// Options configuring preset extraction.
#[derive(Clone, Debug)]
pub struct ExtractOptions {
    /// Preset identifier (must conform to `[a-z0-9-]+`).
    pub preset_id: Option<String>,
    /// Human-readable preset name.
    pub preset_name: Option<String>,
    /// Target file path to write output JSON to.
    pub out_path: Option<String>,
}

fn collect_brake_drills(
    lap: &LapTelemetry,
    brake_zones: &[crate::zones::BrakeZone],
    lap_idx: usize,
    multi_lap: bool,
    drills: &mut Vec<Drill>,
) {
    for (b_num, b_zone) in brake_zones.iter().enumerate() {
        let b_idx = b_num + 1;
        let trace_pts = process_trace_segment(&lap.brake, b_zone.onset_idx, b_zone.release_idx);
        if trace_pts.len() >= 2 {
            let drill_id = if multi_lap {
                sanitize_id(&format!("lap-{lap_idx}-brake-zone-{b_idx}"))
            } else {
                sanitize_id(&format!("brake-zone-{b_idx}"))
            };

            let drill_name = if multi_lap {
                format!("Lap {lap_idx} Brake Zone {b_idx} ({:.0}%)", b_zone.peak_pct)
            } else {
                format!("Brake Zone {b_idx} ({:.0}%)", b_zone.peak_pct)
            };

            drills.push(Drill {
                id: drill_id,
                name: drill_name,
                pedal: Pedal::Brake,
                reps: 5,
                lead_in_ms: 2000,
                tolerance: 6.0,
                kind: DrillKind::Trace { points: trace_pts },
            });

            if let Some(plateau) = detect_plateau(&lap.brake, b_zone.onset_idx, b_zone.release_idx)
            {
                let hold_id = if multi_lap {
                    sanitize_id(&format!("lap-{lap_idx}-brake-hold-{b_idx}"))
                } else {
                    sanitize_id(&format!("brake-hold-{b_idx}"))
                };

                let hold_name = if multi_lap {
                    format!("Lap {lap_idx} Brake Hold {b_idx} ({:.0}%)", plateau.target)
                } else {
                    format!("Brake Hold {b_idx} ({:.0}%)", plateau.target)
                };

                drills.push(Drill {
                    id: hold_id,
                    name: hold_name,
                    pedal: Pedal::Brake,
                    reps: 5,
                    lead_in_ms: 2000,
                    tolerance: 5.0,
                    kind: DrillKind::Hold {
                        target: plateau.target,
                        hold_ms: plateau.hold_ms,
                    },
                });
            }
        }
    }
}

fn collect_throttle_drills(
    lap: &LapTelemetry,
    throttle_zones: &[crate::zones::ThrottleExitZone],
    lap_idx: usize,
    multi_lap: bool,
    drills: &mut Vec<Drill>,
) {
    for (t_num, t_zone) in throttle_zones.iter().enumerate() {
        let t_idx = t_num + 1;
        let trace_pts = process_trace_segment(&lap.throttle, t_zone.onset_idx, t_zone.full_idx);
        if trace_pts.len() >= 2 {
            let drill_id = if multi_lap {
                sanitize_id(&format!("lap-{lap_idx}-throttle-exit-{t_idx}"))
            } else {
                sanitize_id(&format!("throttle-exit-{t_idx}"))
            };

            let drill_name = if multi_lap {
                format!("Lap {lap_idx} Throttle Exit {t_idx}")
            } else {
                format!("Throttle Exit {t_idx}")
            };

            drills.push(Drill {
                id: drill_id,
                name: drill_name,
                pedal: Pedal::Throttle,
                reps: 5,
                lead_in_ms: 2000,
                tolerance: 6.0,
                kind: DrillKind::Trace { points: trace_pts },
            });

            if let Some(plateau) = detect_plateau(&lap.throttle, t_zone.onset_idx, t_zone.full_idx)
            {
                let hold_id = if multi_lap {
                    sanitize_id(&format!("lap-{lap_idx}-throttle-hold-{t_idx}"))
                } else {
                    sanitize_id(&format!("throttle-hold-{t_idx}"))
                };

                let hold_name = if multi_lap {
                    format!(
                        "Lap {lap_idx} Throttle Hold {t_idx} ({:.0}%)",
                        plateau.target
                    )
                } else {
                    format!("Throttle Hold {t_idx} ({:.0}%)", plateau.target)
                };

                drills.push(Drill {
                    id: hold_id,
                    name: hold_name,
                    pedal: Pedal::Throttle,
                    reps: 5,
                    lead_in_ms: 2000,
                    tolerance: 5.0,
                    kind: DrillKind::Hold {
                        target: plateau.target,
                        hold_ms: plateau.hold_ms,
                    },
                });
            }
        }
    }
}

/// Extracts drills from multiple laps and compiles them into a validated [`Preset`].
///
/// # Errors
///
/// Returns an error string if preset validation fails or no valid drills could be extracted.
pub fn extract_preset_from_laps(
    laps: &[LapTelemetry],
    options: &ExtractOptions,
) -> Result<Preset, String> {
    if laps.is_empty() {
        return Err("no telemetry laps provided for extraction".to_string());
    }

    let mut drills = Vec::new();
    let multi_lap = laps.len() > 1;

    for (lap_num, lap) in laps.iter().enumerate() {
        let lap_idx = lap_num + 1;
        let brake_zones = detect_brake_zones(&lap.brake);
        let throttle_zones = detect_throttle_exit_zones(&lap.throttle, &brake_zones);

        collect_brake_drills(lap, &brake_zones, lap_idx, multi_lap, &mut drills);
        collect_throttle_drills(lap, &throttle_zones, lap_idx, multi_lap, &mut drills);
    }

    if drills.is_empty() {
        return Err("no drills could be extracted from telemetry data".to_string());
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
        drills,
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
