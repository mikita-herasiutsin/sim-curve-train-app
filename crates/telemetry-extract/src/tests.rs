//! All tests use purely synthetic data generated in-memory.
//! No real telemetry files are used or included in the repository.

#![expect(
    clippy::cast_precision_loss,
    clippy::needless_range_loop,
    reason = "synthetic test fixture generation uses simple loops and float conversions"
)]

use std::path::Path;

use sct_core::preset::{DrillKind, parse_preset};

use crate::csv::{LapTelemetry, parse_filename_metadata, parse_lap_time_str, read_csv_file};
use crate::extract::{ExtractOptions, extract_preset_from_laps};
use crate::simplify::{Point2D, rdp, simplify_adaptive};
use crate::stats::{MetricSummary, StatsCollector, analyze_throttle_exit};
use crate::zones::{BrakeZone, detect_brake_zones, detect_throttle_exit_zones};

/// Helper to generate synthetic CSV file contents.
fn create_synthetic_csv(rows: &[(f32, f32)]) -> String {
    use std::fmt::Write;
    let mut s = String::from(
        "Speed,LapDistPct,Lat,Lon,Brake,Throttle,RPM,SteeringWheelAngle,Gear,Clutch,ABSActive,DRSActive,P2PActive,LatAccel,LongAccel,VertAccel,Yaw,YawRate,PositionType\n",
    );
    for (idx, &(b, t)) in rows.iter().enumerate() {
        let pct = (idx as f64) / (rows.len() as f64);
        let _ = writeln!(
            s,
            "50.0,{pct:.6},36.0,140.0,{b:.4},{t:.4},6000.0,0.0,3,1.0,false,false,false,0.0,0.0,9.8,0.0,0.0,3"
        );
    }
    s
}

#[test]
fn test_filename_parsing_and_timing() {
    let filename = "Garage 61 - Mikita Herasiutsin - BMW M4 GT3 EVO - Road Atlanta (Full Course) - 01.18.593 - 01M3J3QN.csv";
    let meta = parse_filename_metadata(Path::new(filename)).expect("metadata should parse");

    assert_eq!(meta.driver, "Mikita Herasiutsin");
    assert_eq!(meta.car, "BMW M4 GT3 EVO");
    assert_eq!(meta.track, "Road Atlanta (Full Course)");
    assert_eq!(meta.lap_time_str, "01.18.593");
    assert!((meta.lap_time_s - 78.593).abs() < 1e-4);
    assert_eq!(meta.id, "01M3J3QN");

    // Test lap time parsing variations
    assert_eq!(parse_lap_time_str("00.30.996"), Some(30.996));
    assert_eq!(parse_lap_time_str("01:03.799"), Some(63.799));
    assert_eq!(parse_lap_time_str("07.53.117"), Some(473.117));
}

#[test]
fn test_csv_ingestion_and_duration_check() {
    // Generate synthetic 60 Hz rows for a 2-second lap (120 rows)
    let mut rows = Vec::new();
    for _ in 0..120 {
        rows.push((0.0, 1.0));
    }

    let csv_content = create_synthetic_csv(&rows);

    let temp_dir = std::env::temp_dir();
    let temp_file =
        temp_dir.join("Garage 61 - Driver - Test Car - Circuit - 00.02.000 - TESTID.csv");
    std::fs::write(&temp_file, &csv_content).expect("write temp csv");

    let lap = read_csv_file(&temp_file).expect("read synthetic csv");
    let _ = std::fs::remove_file(&temp_file);

    assert_eq!(lap.len(), 120);
    assert!((lap.duration_s() - 2.0).abs() < 1e-4);
    assert_eq!(lap.car_name(), "Test Car");
}

#[test]
fn test_zone_detection_with_filtering_and_merging() {
    let mut brakes = vec![0.0; 300];
    let mut throttles = vec![1.0; 300];

    // 1. Noise spike: 3 samples at 0.50 (duration < 100 ms -> should be rejected)
    brakes[20] = 0.50;
    brakes[21] = 0.50;
    brakes[22] = 0.50;

    // 2. Low-force braking: 20 samples at 0.10 (peak < 0.20 -> should be rejected)
    for b in &mut brakes[40..60] {
        *b = 0.10;
    }

    // 3. Real braking zone:
    // Frame 100 to 180 (80 samples = ~1.33 s)
    // Ramps to 0.85, holds, trails off
    for i in 100..180 {
        throttles[i] = 0.0;
        if i < 115 {
            brakes[i] = 0.05 + ((i - 100) as f32 / 15.0) * 0.80; // ramp to 0.85
        } else if i < 145 {
            brakes[i] = 0.85; // plateau at 85%
        } else {
            brakes[i] = 0.85 - ((i - 145) as f32 / 35.0) * 0.84; // trail down to 0.01
        }
    }

    // 4. Corner apex throttle minimum and staged corner exit
    // Coasting / minimum from 180 to 200
    for t in &mut throttles[180..200] {
        *t = 0.0;
    }

    // Throttle exit onset at 200:
    // Fast stab to 60% over 6 frames (200..206)
    for i in 200..206 {
        throttles[i] = ((i - 200) as f32 / 6.0) * 0.60;
    }
    // Hold at 60% for 10 frames (206..216)
    for t in &mut throttles[206..216] {
        *t = 0.60;
    }
    // Progressive ramp to 100% over 30 frames (216..246)
    for i in 216..246 {
        throttles[i] = 0.60 + ((i - 216) as f32 / 30.0) * 0.40;
    }
    for t in &mut throttles[246..300] {
        *t = 1.0;
    }

    let brake_zones = detect_brake_zones(&brakes);
    assert_eq!(brake_zones.len(), 1, "only the real brake zone should pass");

    let bz = &brake_zones[0];
    assert_eq!(bz.onset_idx, 100);
    assert!((bz.peak_pct - 85.0).abs() < 1.0);
    assert!(bz.duration_s >= 1.0);

    let throttle_zones = detect_throttle_exit_zones(&throttles, &brake_zones);
    assert_eq!(
        throttle_zones.len(),
        1,
        "throttle exit zone should be detected"
    );

    let tz = &throttle_zones[0];
    assert!(tz.onset_idx >= 200);
    assert!(tz.full_idx >= 244);
}

#[test]
fn test_rdp_simplification_and_peak_preservation() {
    // Normalised coordinate triangle with sharp apex: (0,0) -> (0.3, 0.95) -> (1.0, 0.0)
    let points = vec![
        Point2D { x: 0.0, y: 0.0 },
        Point2D { x: 0.1, y: 0.3 },
        Point2D { x: 0.2, y: 0.6 },
        Point2D { x: 0.3, y: 0.95 }, // Sharp peak
        Point2D { x: 0.5, y: 0.7 },
        Point2D { x: 0.7, y: 0.4 },
        Point2D { x: 0.9, y: 0.1 },
        Point2D { x: 1.0, y: 0.0 },
    ];

    let simplified = rdp(&points, 0.05);

    // Peak at (0.3, 0.95) must be preserved!
    let has_peak = simplified
        .iter()
        .any(|p| (p.x - 0.3).abs() < 1e-4 && (p.y - 0.95).abs() < 1e-4);
    assert!(has_peak, "RDP must strictly preserve maximum peak vertex");

    // Adaptive simplification target budget
    let adaptive = simplify_adaptive(&points);
    assert!((2..=15).contains(&adaptive.len()));
}

#[test]
fn test_stats_on_known_staged_ramp() {
    let mut throttles = vec![0.0; 120];

    // Onset at frame 10:
    // 1. Initial fast stab: from 0.0 to 0.60 in 6 frames (0.10 s) -> rate = 600 %/s (>300 %/s)
    let onset = 10;
    for i in onset..onset + 6 {
        throttles[i] = ((i - onset) as f32 / 6.0) * 0.60;
    }
    // Hold at 60% for 6 frames (0.10 s)
    for i in onset + 6..onset + 12 {
        throttles[i] = 0.60;
    }
    // 2. Progressive ramp: from 0.60 to 0.98 in 30 frames (0.50 s) -> rate = 76 %/s (<300 %/s)
    let ramp_start = onset + 12;
    let full = ramp_start + 30;
    for i in ramp_start..=full {
        throttles[i] = 0.60 + ((i - ramp_start) as f32 / 30.0) * 0.38;
    }
    for t in &mut throttles[full + 1..] {
        *t = 1.0;
    }

    let exit_zone = crate::zones::ThrottleExitZone {
        min_idx: 0,
        onset_idx: onset,
        full_idx: full,
        duration_s: (full - onset) as f64 / 60.0,
    };

    let metrics = analyze_throttle_exit(&throttles, &exit_zone);

    // 150 ms is 9 frames after onset (frame 19), which is inside the 60% hold
    assert!((metrics.initial_stab_level_pct - 60.0).abs() < 1.0);
    // Time to stab is 6 frames = 0.10 s
    assert!((metrics.time_to_stab_s - 0.10).abs() < 1e-3);
    // 1 fast step before progressive phase
    assert_eq!(metrics.fast_steps, 1);
    // Progressive ramp from 60% to 98% in 36 frames = 0.60 s -> ~63.3 %/s
    assert!(
        metrics.progressive_ramp_rate_pct_s > 40.0 && metrics.progressive_ramp_rate_pct_s < 100.0
    );
    // Time to full is 42 frames = 0.70 s
    assert!((metrics.time_to_full_s - 0.70).abs() < 1e-3);
}

#[test]
fn test_stats_aggregator_median_iqr() {
    let vals = vec![10.0, 20.0, 30.0, 40.0, 50.0];
    let summary = MetricSummary::compute(&vals);

    assert_eq!(summary.median, 30.0);
    assert_eq!(summary.iqr, 20.0); // Q3 (40.0) - Q1 (20.0)

    let mut collector = StatsCollector::default();
    let lap = LapTelemetry {
        path: std::path::PathBuf::from("test.csv"),
        metadata: None,
        brake: vec![0.0; 100],
        throttle: vec![0.0; 100],
        speed: vec![0.0; 100],
    };

    let b_zone = BrakeZone {
        onset_idx: 10,
        release_idx: 70,
        peak_idx: 30,
        peak_pct: 80.0,
        duration_s: 1.0,
    };

    collector.add_lap(&lap, &[b_zone], &[]);
    let report = collector.build_report();

    assert!(report.cars.contains_key("Unknown Car"));
    let car_stats = &report.cars["Unknown Car"];
    assert_eq!(car_stats.brake.zone_count, 1);
    assert_eq!(car_stats.brake.peak_pct.median, 80.0);
}

#[test]
fn test_preset_output_validates_against_sct_core() {
    let mut brakes = vec![0.0; 250];
    let mut throttles = vec![1.0; 250];

    // Heavy brake zone: frame 30 to 120
    for i in 30..120 {
        throttles[i] = 0.0;
        if i < 50 {
            brakes[i] = 0.05 + ((i - 30) as f32 / 20.0) * 0.85;
        } else if i < 85 {
            brakes[i] = 0.90; // Plateau
        } else {
            brakes[i] = 0.90 - ((i - 85) as f32 / 35.0) * 0.89;
        }
    }

    // Throttle exit: frame 130 to 200
    for t in &mut throttles[120..130] {
        *t = 0.0;
    }
    for i in 130..190 {
        throttles[i] = ((i - 130) as f32 / 60.0) * 0.98;
    }
    for t in &mut throttles[190..250] {
        *t = 1.0;
    }

    let lap = LapTelemetry {
        path: std::path::PathBuf::from("test.csv"),
        metadata: None,
        brake: brakes,
        throttle: throttles,
        speed: vec![50.0; 250],
    };

    let options = ExtractOptions {
        preset_id: Some("test-synthetic-preset".to_string()),
        preset_name: Some("Synthetic Test Preset".to_string()),
        out_path: None,
    };

    let preset =
        extract_preset_from_laps(&[lap], &options).expect("preset extraction should succeed");

    // 1. Strict domain validation
    preset
        .validate()
        .expect("extracted preset must pass sct-core validation");

    // 2. Round-trip serialization and loading via parse_preset
    let json = serde_json::to_string_pretty(&preset).expect("json serialize");
    let loaded = parse_preset(&json).expect("parse_preset must load the generated JSON");

    assert_eq!(loaded.id, "test-synthetic-preset");
    assert_eq!(loaded.name, "Synthetic Test Preset");
    assert_ne!(loaded.drills.len(), 0);

    for drill in &loaded.drills {
        assert_eq!(drill.reps, 5);
        assert!(drill.tolerance > 0.0);
        match &drill.kind {
            DrillKind::Trace { points } => {
                assert!(points.len() >= 2);
                assert_eq!(points[0].0, 0, "first point timestamp must be 0 ms");
                for w in points.windows(2) {
                    assert!(w[1].0 > w[0].0, "timestamps must be strictly increasing");
                }
            }
            DrillKind::Hold { target, hold_ms } => {
                assert!(*target >= 0.0 && *target <= 100.0);
                assert!(*hold_ms >= 200 && *hold_ms <= 60000);
            }
        }
    }
}
