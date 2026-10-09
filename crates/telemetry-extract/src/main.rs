//! CLI entrypoint for the Garage 61 telemetry zone extractor and stats tool.

mod csv;
mod extract;
mod simplify;
mod stats;
mod zones;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::csv::LapTelemetry;
use crate::extract::{ExtractOptions, run_extract};
use crate::stats::{StatsCollector, format_stats_table};
use crate::zones::{detect_brake_zones, detect_lift_zones, detect_throttle_exit_zones};

fn print_help() {
    println!(
        r"telemetry-extract: Garage 61 telemetry zone extractor and statistical analyzer.

USAGE:
    telemetry-extract <SUBCOMMAND> [OPTIONS] <INPUT_PATHS...>

SUBCOMMANDS:
    extract    Detect pedal zones, simplify curves, and export validated preset JSON drills.
    stats      Calculate and aggregate pedal metrics per car (median and IQR).

GLOBAL OPTIONS:
    -h, --help       Print help information.

RUN 'telemetry-extract <SUBCOMMAND> --help' FOR MORE INFORMATION ON A SPECIFIC SUBCOMMAND.
"
    );
}

fn print_extract_help() {
    println!(
        r"telemetry-extract extract: Detect pedal zones and export validated preset JSON drills.

USAGE:
    telemetry-extract extract [OPTIONS] <INPUT_PATHS...>

ARGUMENTS:
    <INPUT_PATHS...>       One or more CSV files or directories containing Garage 61 CSV exports.

OPTIONS:
    --out <FILE>           Output file path for the generated preset JSON. If omitted, prints to stdout.
    --car-filter <CAR>     Keep laps whose car name (from the filename) contains CAR, any case.
    --preset-id <ID>       Unique preset identifier (lowercase alphanumeric and hyphens: [a-z0-9-]+).
    --name, --preset-name <NAME>
                           Human-readable preset name.
    --tolerance <PCT>      Permissible error tolerance in percent (e.g. 10.0). If omitted, defaults to app default (10%).
    --max-drills <N>       Maximum number of drills to output (default: 12).
    --allow-mixed          Accept laps from different cars or tracks in one run.
    -h, --help             Print help information.
"
    );
}

fn print_stats_help() {
    println!(
        r"telemetry-extract stats: Aggregate and display pedal control metrics per car.

USAGE:
    telemetry-extract stats [OPTIONS] <INPUT_PATHS...>

ARGUMENTS:
    <INPUT_PATHS...>       One or more CSV files or directories containing Garage 61 CSV exports.

OPTIONS:
    --car-filter <CAR>     Keep laps whose car name (from the filename) contains CAR, any case.
    --json                 Output aggregated statistics as JSON instead of a formatted table.
    --allow-mixed          Accept laps from different cars or tracks in one run.
    -h, --help             Print help information.
"
    );
}

/// Collects all `.csv` files from the specified paths (recursively traversing directories).
fn collect_csv_files(inputs: &[String]) -> Vec<PathBuf> {
    let mut files = Vec::new();

    for input in inputs {
        let path = Path::new(input);
        if path.is_file() {
            if path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("csv"))
            {
                files.push(path.to_path_buf());
            }
        } else if path.is_dir() {
            scan_dir_recursive(path, &mut files);
        } else {
            eprintln!(
                "warning: input path '{}' not found or is neither file nor directory",
                path.display()
            );
        }
    }

    files.sort();
    files
}

fn scan_dir_recursive(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir_recursive(&path, files);
        } else if path.is_file()
            && path
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("csv"))
        {
            files.push(path);
        }
    }
}

/// Ingests and optionally filters laps from discovered files.
fn load_laps(files: &[PathBuf], car_filter: Option<&str>) -> Vec<LapTelemetry> {
    let mut laps = Vec::new();

    for path in files {
        match crate::csv::read_csv_file(path) {
            Ok(lap) => {
                if let Some(filter) = car_filter
                    && !lap
                        .car_name()
                        .to_lowercase()
                        .contains(&filter.to_lowercase())
                {
                    continue;
                }
                laps.push(lap);
            }
            Err(e) => {
                eprintln!("skipping '{}': {e}", path.display());
            }
        }
    }

    laps
}

/// Takes the value after the option at `args[*i]`, advancing `i` past it.
fn option_value(args: &[String], i: &mut usize) -> Result<String, String> {
    let flag = &args[*i];
    match args.get(*i + 1) {
        Some(v) if !v.starts_with("--") => {
            *i += 1;
            Ok(v.clone())
        }
        _ => Err(format!("{flag} needs a value")),
    }
}

/// Parses `--tolerance`, checking the same range sct-core validation enforces.
fn parse_tolerance(v: &str) -> Result<f32, String> {
    v.parse::<f32>()
        .ok()
        .filter(|t| (0.5..=50.0).contains(t))
        .ok_or_else(|| format!("--tolerance must be a number from 0.5 to 50, got '{v}'"))
}

/// Records a positional input path; anything else starting with `-` is an unknown option.
fn positional(arg: &str, inputs: &mut Vec<String>) -> Result<(), String> {
    if arg.starts_with('-') {
        return Err(format!("unknown option '{arg}'"));
    }
    inputs.push(arg.to_string());
    Ok(())
}

fn check_mixed_laps(laps: &[LapTelemetry], allow_mixed: bool) -> Result<(), String> {
    if laps.is_empty() || allow_mixed {
        return Ok(());
    }

    let first_car = laps[0].car_name();
    let first_track = laps[0]
        .metadata
        .as_ref()
        .map_or("Unknown Track", |m| m.track.as_str());

    for lap in &laps[1..] {
        let car = lap.car_name();
        let track = lap
            .metadata
            .as_ref()
            .map_or("Unknown Track", |m| m.track.as_str());
        if car != first_car || track != first_track {
            return Err(format!(
                "mixed input detected. First lap is '{first_car}' at '{first_track}', but found '{car}' at '{track}'. Use --allow-mixed to bypass."
            ));
        }
    }

    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") && args.len() == 1 {
        print_help();
        return ExitCode::SUCCESS;
    }

    let subcommand = &args[0];
    let sub_args = &args[1..];

    if sub_args.iter().any(|a| a == "-h" || a == "--help") {
        match subcommand.as_str() {
            "extract" => print_extract_help(),
            "stats" => print_stats_help(),
            _ => print_help(),
        }
        return ExitCode::SUCCESS;
    }

    match subcommand.as_str() {
        "extract" => handle_extract(sub_args),
        "stats" => handle_stats(sub_args),
        _ => {
            eprintln!("error: unknown subcommand '{subcommand}'\n");
            print_help();
            ExitCode::FAILURE
        }
    }
}

fn handle_extract(args: &[String]) -> ExitCode {
    let mut out_path = None;
    let mut car_filter = None;
    let mut preset_id = None;
    let mut preset_name = None;
    let mut tolerance = None;
    let mut max_drills = 12;
    let mut allow_mixed = false;
    let mut inputs = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let parsed: Result<(), String> = match args[i].as_str() {
            "--out" => option_value(args, &mut i).map(|v| out_path = Some(v)),
            "--car-filter" => option_value(args, &mut i).map(|v| car_filter = Some(v)),
            "--preset-id" => option_value(args, &mut i).map(|v| preset_id = Some(v)),
            "--name" | "--preset-name" => option_value(args, &mut i).map(|v| preset_name = Some(v)),
            "--tolerance" => option_value(args, &mut i)
                .and_then(|v| parse_tolerance(&v))
                .map(|v| tolerance = Some(v)),
            "--max-drills" => option_value(args, &mut i).and_then(|v| {
                v.parse::<usize>()
                    .ok()
                    .filter(|&n| n > 0)
                    .map(|n| max_drills = n)
                    .ok_or_else(|| format!("--max-drills must be a positive integer, got '{v}'"))
            }),
            "--allow-mixed" => {
                allow_mixed = true;
                Ok(())
            }
            other => positional(other, &mut inputs),
        };
        if let Err(e) = parsed {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
        i += 1;
    }

    if inputs.is_empty() {
        eprintln!("error: no input files or directories specified for extraction.");
        return ExitCode::FAILURE;
    }

    let files = collect_csv_files(&inputs);
    if files.is_empty() {
        eprintln!("error: no CSV files found in provided input paths.");
        return ExitCode::FAILURE;
    }

    let laps = load_laps(&files, car_filter.as_deref());
    if laps.is_empty() {
        if let Some(filter) = &car_filter {
            eprintln!("error: no readable laps match --car-filter '{filter}'.");
        } else {
            eprintln!("error: none of the CSV files could be read.");
        }
        return ExitCode::FAILURE;
    }

    if let Err(e) = check_mixed_laps(&laps, allow_mixed) {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }

    let options = ExtractOptions {
        preset_id,
        preset_name,
        out_path,
        tolerance,
        max_drills,
    };

    if let Err(e) = run_extract(&laps, &options) {
        eprintln!("error during extraction: {e}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

fn handle_stats(args: &[String]) -> ExitCode {
    let mut car_filter = None;
    let mut json_output = false;
    let mut allow_mixed = false;
    let mut inputs = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let parsed: Result<(), String> = match args[i].as_str() {
            "--car-filter" => option_value(args, &mut i).map(|v| car_filter = Some(v)),
            "--json" => {
                json_output = true;
                Ok(())
            }
            "--allow-mixed" => {
                allow_mixed = true;
                Ok(())
            }
            other => positional(other, &mut inputs),
        };
        if let Err(e) = parsed {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
        i += 1;
    }

    if inputs.is_empty() {
        eprintln!("error: no input files or directories specified for stats.");
        return ExitCode::FAILURE;
    }

    let files = collect_csv_files(&inputs);
    if files.is_empty() {
        eprintln!("error: no CSV files found in provided input paths.");
        return ExitCode::FAILURE;
    }

    let laps = load_laps(&files, car_filter.as_deref());
    if laps.is_empty() {
        if let Some(filter) = &car_filter {
            eprintln!("error: no readable laps match --car-filter '{filter}'.");
        } else {
            eprintln!("error: none of the CSV files could be read.");
        }
        return ExitCode::FAILURE;
    }

    if let Err(e) = check_mixed_laps(&laps, allow_mixed) {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }

    let mut collector = StatsCollector::default();

    for lap in &laps {
        let brake_zones = detect_brake_zones(&lap.brake);
        let lift_zones = detect_lift_zones(&lap.throttle, &lap.brake);
        let throttle_zones = detect_throttle_exit_zones(&lap.throttle, &brake_zones, &lift_zones);
        collector.add_lap(lap, &brake_zones, &lift_zones, &throttle_zones);
    }

    let report = collector.build_report();

    if json_output {
        match serde_json::to_string_pretty(&report) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("error serializing stats report to JSON: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        print!("{table}", table = format_stats_table(&report));
    }

    ExitCode::SUCCESS
}
