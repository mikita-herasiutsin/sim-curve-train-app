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
}

impl fmt::Display for CsvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::MissingColumn(col) => write!(f, "missing required CSV column: {col}"),
            Self::EmptyFile => write!(f, "CSV file is empty"),
        }
    }
}

impl std::error::Error for CsvError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::MissingColumn(_) | Self::EmptyFile => None,
        }
    }
}

impl From<std::io::Error> for CsvError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
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
        let millis: f64 = parts[2].parse().ok()?;
        Some(mins * 60.0 + secs + millis / 1000.0)
    } else if parts.len() == 2 {
        let secs: f64 = parts[0].parse().ok()?;
        let millis: f64 = parts[1].parse().ok()?;
        Some(secs + millis / 1000.0)
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

    let mut brake = Vec::new();
    let mut throttle = Vec::new();
    let mut speed = Vec::new();

    for line_res in lines {
        let line = line_res?;
        if line.trim().is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() <= brake_idx || cols.len() <= throttle_idx {
            continue;
        }

        let b: f32 = cols[brake_idx]
            .trim()
            .parse::<f32>()
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        let t: f32 = cols[throttle_idx]
            .trim()
            .parse::<f32>()
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        let s: f32 = speed_idx
            .and_then(|idx| cols.get(idx))
            .and_then(|c| c.trim().parse().ok())
            .unwrap_or(0.0);

        brake.push(b);
        throttle.push(t);
        speed.push(s);
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
                eprintln!(
                    "warning: '{name}': row duration ({row_duration_s:.3}s at 60 Hz) differs from filename lap time ({lap_time:.3}s) by {diff_pct:.2}% (> 2%)",
                    name = path.file_name().unwrap_or_default().to_string_lossy(),
                    lap_time = meta.lap_time_s,
                );
            }
        }
    }

    Ok(LapTelemetry {
        path: path.to_path_buf(),
        metadata,
        brake,
        throttle,
        speed,
    })
}
