//! CSV ingestion and metadata parsing for Garage 61 telemetry exports.

use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Telemetry recording frequency in Hz. Garage 61 rows are sampled at 60 Hz.
pub const TELEMETRY_HZ: f64 = 60.0;

/// Metadata extracted from a Garage 61 CSV export filename.
///
/// Filenames follow the pattern:
/// `Garage 61 - <Driver> - <Car> - <Track> - <mm.ss.mmm> - <id>.csv`
#[derive(Clone, Debug, PartialEq)]
pub struct FileMetadata {
    /// Driver name.
    pub driver: String,
    /// Car name or model.
    pub car: String,
    /// Track name and layout.
    pub track: String,
    /// Lap time in seconds parsed from the filename.
    pub lap_time_s: f64,
    /// Raw lap time string (e.g. `01.03.799`).
    pub lap_time_str: String,
    /// Session or lap unique ID.
    pub id: String,
}

/// Errors during CSV parsing and reading.
#[derive(Debug)]
pub enum CsvError {
    /// I/O error reading the file.
    Io(std::io::Error),
    /// Missing required header columns.
    MissingColumn(String),
    /// File is empty or has no header.
    EmptyFile,
    /// Row count duration differs from filename lap time by > 2%.
    DurationMismatch {
        row_duration_s: f64,
        meta_lap_time_s: f64,
        filename: String,
    },
}

impl fmt::Display for CsvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::MissingColumn(col) => write!(f, "missing required CSV column: {col}"),
            Self::EmptyFile => write!(f, "CSV file is empty"),
            Self::DurationMismatch {
                row_duration_s,
                meta_lap_time_s,
                filename,
            } => write!(
                f,
                "'{filename}': row duration ({row_duration_s:.3}s at 60 Hz) differs from filename lap time ({meta_lap_time_s:.3}s) by > 2%"
            ),
        }
    }
}

impl std::error::Error for CsvError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::MissingColumn(_) | Self::EmptyFile | Self::DurationMismatch { .. } => None,
        }
    }
}

impl From<std::io::Error> for CsvError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

fn parse_fraction(s: &str) -> Option<f64> {
    if s.is_empty() || !s.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let val: f64 = s.parse().ok()?;
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        reason = "digit count of fraction string is small"
    )]
    let divisor = 10_f64.powi(s.len() as i32);
    Some(val / divisor)
}

/// Parses a lap time string formatted as `mm.ss.mmm` (or `m.ss.mmm` / `mm:ss.mmm`) into seconds.
#[must_use]
pub fn parse_lap_time_str(s: &str) -> Option<f64> {
    let s = s.trim();
    // Normalize colon separators to dots (e.g. "01:03.799" -> "01.03.799")
    let normalized = s.replace(':', ".");
    let parts: Vec<&str> = normalized.split('.').collect();
    if parts.len() == 3 {
        let mins: f64 = parts[0].parse().ok()?;
        let secs: f64 = parts[1].parse().ok()?;
        let frac = parse_fraction(parts[2])?;
        Some(mins * 60.0 + secs + frac)
    } else if parts.len() == 2 {
        let secs: f64 = parts[0].parse().ok()?;
        let frac = parse_fraction(parts[1])?;
        Some(secs + frac)
    } else {
        None
    }
}

/// Parses metadata from a Garage 61 export filename.
///
/// Expected pattern:
/// `Garage 61 - <Driver> - <Car> - <Track> - <mm.ss.mmm> - <id>.csv`
#[must_use]
pub fn parse_filename_metadata(path: &Path) -> Option<FileMetadata> {
    let file_stem = path.file_stem()?.to_str()?;
    let parts: Vec<&str> = file_stem.split(" - ").collect();
    if parts.len() < 6 {
        return None;
    }

    let driver = parts[1].trim().to_string();
    let lap_time_str = parts[parts.len() - 2].trim().to_string();
    let id = parts[parts.len() - 1].trim().to_string();
    let lap_time_s = parse_lap_time_str(&lap_time_str)?;

    let (car, track) = if parts.len() == 6 {
        (parts[2].trim().to_string(), parts[3].trim().to_string())
    } else {
        // If track name contained " - ", car is parts[2] and track is remainder
        let car = parts[2].trim().to_string();
        let track = parts[3..parts.len() - 2].join(" - ");
        (car, track)
    };

    Some(FileMetadata {
        driver,
        car,
        track,
        lap_time_s,
        lap_time_str,
        id,
    })
}

/// Ingested single-lap telemetry data.
#[derive(Clone, Debug)]
pub struct LapTelemetry {
    /// File path.
    #[expect(dead_code, reason = "file path retained for diagnostics")]
    pub path: PathBuf,
    /// Extracted file metadata, if filename matched expected pattern.
    pub metadata: Option<FileMetadata>,
    /// Brake pedal positions in `0.0..=1.0` at 60 Hz.
    pub brake: Vec<f32>,
    /// Throttle pedal positions in `0.0..=1.0` at 60 Hz.
    pub throttle: Vec<f32>,
    /// Vehicle forward speed in m/s at 60 Hz.
    #[expect(dead_code, reason = "speed channel preserved from telemetry")]
    pub speed: Vec<f32>,
    /// Lap distance percentage in `0.0..=1.0` at 60 Hz.
    pub lap_dist_pct: Vec<f32>,
}

impl LapTelemetry {
    /// Returns the number of telemetry sample rows.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "telemetry row count accessor used in tests")
    )]
    pub fn len(&self) -> usize {
        self.brake.len()
    }

    /// Duration in seconds derived from row count at 60 Hz.
    #[must_use]
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "telemetry duration accessor used in tests")
    )]
    pub fn duration_s(&self) -> f64 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "row count up to ~100k safely converts to f64"
        )]
        let rows = self.len() as f64;
        rows / TELEMETRY_HZ
    }

    /// Car name from metadata, or fallback to `"Unknown Car"`.
    #[must_use]
    pub fn car_name(&self) -> &str {
        self.metadata
            .as_ref()
            .map_or("Unknown Car", |m| m.car.as_str())
    }
}

/// Reads a Garage 61 telemetry CSV file into [`LapTelemetry`].
///
/// Warns to `stderr` if the derived duration (`rows / 60`) differs from
/// the filename lap time by more than 2%.
///
/// # Errors
///
/// Returns [`CsvError`] if file reading fails or required columns (`Brake`, `Throttle`) are missing.
#[expect(
    clippy::too_many_lines,
    reason = "parsing logic is linear and self-contained"
)]
pub fn read_csv_file(path: &Path) -> Result<LapTelemetry, CsvError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut lines = reader.lines();

    let header_line = lines.next().ok_or(CsvError::EmptyFile)??;
    let headers: Vec<&str> = header_line.split(',').map(str::trim).collect();

    let brake_idx = headers
        .iter()
        .position(|&h| h.eq_ignore_ascii_case("Brake"))
        .ok_or_else(|| CsvError::MissingColumn("Brake".to_string()))?;

    let throttle_idx = headers
        .iter()
        .position(|&h| h.eq_ignore_ascii_case("Throttle"))
        .ok_or_else(|| CsvError::MissingColumn("Throttle".to_string()))?;

    let speed_idx = headers
        .iter()
        .position(|&h| h.eq_ignore_ascii_case("Speed"));

    let lap_dist_idx = headers
        .iter()
        .position(|&h| h.eq_ignore_ascii_case("LapDistPct"));

    let mut brake = Vec::new();
    let mut throttle = Vec::new();
    let mut speed = Vec::new();
    let mut lap_dist_pct = Vec::new();

    let mut last_b = 0.0;
    let mut last_t = 0.0;
    let mut last_s = 0.0;
    let mut last_d = -1.0;
    // Short rows still take a 60 Hz slot, forward-filled, so the timeline does not shift.
    let mut short_rows = 0usize;

    for line_res in lines {
        let line = line_res?;
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() <= brake_idx || cols.len() <= throttle_idx {
            short_rows += 1;
        }

        let parse_cell = |s: &str| -> Option<f32> {
            let s = s.trim();
            if s.is_empty() {
                None
            } else {
                s.parse::<f32>().ok().filter(|f| f.is_finite())
            }
        };

        let b = cols
            .get(brake_idx)
            .and_then(|&c| parse_cell(c))
            .unwrap_or(last_b)
            .clamp(0.0, 1.0);
        let t = cols
            .get(throttle_idx)
            .and_then(|&c| parse_cell(c))
            .unwrap_or(last_t)
            .clamp(0.0, 1.0);
        let s = speed_idx
            .and_then(|idx| cols.get(idx))
            .and_then(|&c| parse_cell(c))
            .unwrap_or(last_s);
        let d = lap_dist_idx
            .and_then(|idx| cols.get(idx))
            .and_then(|&c| parse_cell(c))
            .unwrap_or(last_d);

        last_b = b;
        last_t = t;
        last_s = s;
        last_d = d;

        brake.push(b);
        throttle.push(t);
        speed.push(s);
        lap_dist_pct.push(if d >= 0.0 { d.clamp(0.0, 1.0) } else { -1.0 });
    }

    if short_rows > 0 {
        eprintln!(
            "note: '{}': {short_rows} short row(s) filled from the previous sample",
            path.display()
        );
    }

    let total_rows = brake.len();
    for (i, p) in lap_dist_pct.iter_mut().enumerate() {
        if *p < 0.0 {
            #[expect(
                clippy::cast_precision_loss,
                reason = "row index converts safely to f32"
            )]
            let frac = (i as f32) / (total_rows.max(1) as f32);
            *p = frac.clamp(0.0, 1.0);
        }
    }

    let metadata = parse_filename_metadata(path);

    // Validate 60 Hz duration vs filename lap time.
    if let Some(ref meta) = metadata {
        #[expect(
            clippy::cast_precision_loss,
            reason = "sample count safely converts to f64"
        )]
        let row_count = brake.len() as f64;
        let row_duration_s = row_count / TELEMETRY_HZ;
        if meta.lap_time_s > 0.0 {
            let diff_pct = ((row_duration_s - meta.lap_time_s).abs() / meta.lap_time_s) * 100.0;
            if diff_pct > 2.0 {
                let filename = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let err = CsvError::DurationMismatch {
                    row_duration_s,
                    meta_lap_time_s: meta.lap_time_s,
                    filename,
                };
                return Err(err);
            }
        }
    }

    Ok(LapTelemetry {
        path: path.to_path_buf(),
        metadata,
        brake,
        throttle,
        speed,
        lap_dist_pct,
    })
}
