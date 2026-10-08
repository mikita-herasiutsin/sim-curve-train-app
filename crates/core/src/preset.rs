//! Drill preset schema, validation, and loading.
//!
//! Presets define a collection of sim-racing pedal drills (e.g. threshold braking,
//! trail-off traces, or throttle modulation) with configurable tolerance and repetitions.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Current schema version for preset files.
pub const SCHEMA_VERSION: u32 = 1;

const DEFAULT_REPS: u32 = 5;
const DEFAULT_LEAD_IN_MS: u32 = 3000;

const fn default_reps() -> u32 {
    DEFAULT_REPS
}

const fn default_tolerance() -> f32 {
    10.0
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
    /// Lead-in preparation countdown before each rep in milliseconds (`0..=10000`, defaults to 3000).
    #[serde(default = "default_lead_in")]
    pub lead_in_ms: u32,
    /// Permissible error tolerance in percent (`0.5..=50.0`).
    #[serde(default = "default_tolerance")]
    pub tolerance: f32,
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
        self.tolerance / 100.0
    }
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

        Ok(())
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

    if !(0..=10000).contains(&drill.lead_in_ms) {
        return Err(PresetError::Invalid {
            drill: drill_ctx,
            message: format!(
                "field 'leadInMs' ({}) must be between 0 and 10000",
                drill.lead_in_ms
            ),
        });
    }

    if !drill.tolerance.is_finite() || !(0.5..=50.0).contains(&drill.tolerance) {
        return Err(PresetError::Invalid {
            drill: drill_ctx,
            message: format!(
                "field 'tolerance' ({}) must be finite and between 0.5 and 50",
                drill.tolerance
            ),
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
    if duration > 60000 {
        return Err(PresetError::Invalid {
            drill: drill_ctx,
            message: format!("field 'points': duration ({duration} ms) must be at most 60000 ms"),
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TraceCurve {
    points: Vec<(u32, f32)>,
}

impl TraceCurve {
    /// Constructs a `TraceCurve` by converting percentage values (`0.0..=100.0`) to fractions (`0.0..=1.0`).
    #[must_use]
    pub fn from_points(points: &[(u32, f32)]) -> Self {
        let converted = points.iter().map(|&(t, pct)| (t, pct / 100.0)).collect();
        Self { points: converted }
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

    /// Computes the linearly interpolated target value at `t_ms`.
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
        let dt = t1 - t0;
        if dt <= 0.0 {
            return v0_f32;
        }

        let factor = (t_ms - t0) / dt;
        let v0 = f64::from(v0_f32);
        let v1 = f64::from(v1_f32);
        let val = v0 + factor * (v1 - v0);

        #[expect(
            clippy::cast_possible_truncation,
            reason = "interpolated fraction fits within f32 range [0.0, 1.0]"
        )]
        let result = val as f32;
        result
    }
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
            tolerance: 5.0,
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
            tolerance: 6.0,
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
            } if d == "brake-hold-70" && message.contains("field 'leadInMs'")
        ));
    }

    #[test]
    fn validation_tolerance_range() {
        let mut preset = valid_preset();
        preset.drills[0].tolerance = 0.4;
        let json_too_low = serde_json::to_string(&preset).unwrap();
        let err = parse_preset(&json_too_low).unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: Some(d),
                message
            } if d == "brake-hold-70" && message.contains("field 'tolerance'")
        ));

        preset.drills[0].tolerance = 50.1;
        let json_too_high = serde_json::to_string(&preset).unwrap();
        assert!(parse_preset(&json_too_high).is_err());

        preset.drills[0].tolerance = f32::NAN;
        let json_nan = serde_json::to_string(&preset).unwrap();
        assert!(parse_preset(&json_nan).is_err());
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
    fn validation_trace_duration_max() {
        let mut preset = valid_preset();
        preset.drills[1].kind = DrillKind::Trace {
            points: vec![(0, 0.0), (60001, 50.0)],
        };
        let json = serde_json::to_string(&preset).unwrap();
        let err = parse_preset(&json).unwrap_err();
        assert!(matches!(
            err,
            PresetError::Invalid {
                drill: Some(d),
                message
            } if d == "hairpin" && message.contains("duration (60001 ms)")
        ));
    }

    #[test]
    fn error_messages_contain_drill_id() {
        let mut preset = valid_preset();
        preset.drills[0].tolerance = 0.1;
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

        // Midpoint of (150, 0.92) and (600, 0.60) is t=375 -> 0.76
        assert!((curve.value_at(375.0) - 0.76).abs() < 1e-6);

        // Before 0 (clamped to first value)
        assert!((curve.value_at(-100.0) - 0.0).abs() < 1e-6);

        // After the end (clamped to last value)
        assert!((curve.value_at(2000.0) - 0.0).abs() < 1e-6);
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
        assert!((preset.drills[0].tolerance - 10.0).abs() < f32::EPSILON);
    }
}
