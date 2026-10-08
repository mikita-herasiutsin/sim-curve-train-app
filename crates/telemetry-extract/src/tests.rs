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
use crate::zones::{BrakeZone, detect_brake_zones, detect_lift_zones, detect_throttle_exit_zones};

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

    let throttle_zones = detect_throttle_exit_zones(&throttles, &brake_zones, &[]);
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
    // 1 fast step before progressive phase (plus no plateau -> 1 step total)
    assert_eq!(metrics.fast_steps, 1);
    // Progressive ramp from 60% to 98%
    assert!(
        metrics.progressive_ramp_rate_pct_s > 40.0 && metrics.progressive_ramp_rate_pct_s < 100.0
    );
    // Time to full is 42 frames = 0.70 s
    assert!((metrics.time_to_full_s - 0.70).abs() < 1e-3);
}

#[test]
fn test_staged_throttle_exit_curve() {
    // Synthetic curve: 0 -> 80% (0.1 s) -> hold 0.3 s -> 98% (0.15 s)
    // Expect 2 steps, plateau ~80%
    let mut throttles = vec![0.0; 60];
    let onset = 10;
    // 1. Fast step: 0 -> 80% over 6 frames (0.1 s at 60 Hz -> 800 %/s > 300 %/s)
    for i in onset..onset + 6 {
        throttles[i] = ((i - onset) as f32 / 6.0) * 0.80;
    }
    // 2. Hold at 80% for 18 frames (0.3 s >= 80 ms, |rate| = 0 < 50 %/s)
    for i in onset + 6..onset + 24 {
        throttles[i] = 0.80;
    }
    // 3. Second step: 80% -> 98% over 3 frames (0.05 s)
    let full = onset + 27;
    for i in onset + 24..=full {
        throttles[i] = 0.80 + ((i - (onset + 24)) as f32 / 3.0) * 0.18;
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
    assert_eq!(
        metrics.fast_steps, 2,
        "expected 2 steps (stab, plateau, second increase)"
    );
    assert_eq!(metrics.plateau_levels_pct.len(), 1);
    assert!(
        (metrics.plateau_levels_pct[0] - 80.0).abs() < 1.0,
        "plateau level should be ~80%, got {:.2}",
        metrics.plateau_levels_pct[0]
    );
}

#[test]
fn test_synthetic_oval_lap() {
    // Synthetic oval test: no brake touched, 100 -> 30 -> 100 throttle
    let n = 300;
    let brakes = vec![0.0; n];
    let mut throttles = vec![1.0; n];

    // Lift zone from sample 60 to 180 (2.0 s):
    // 1. Drop from 1.0 to 0.30 over 30 samples (0.5 s)
    for i in 60..90 {
        throttles[i] = 1.0 - ((i - 60) as f32 / 30.0) * 0.70;
    }
    // 2. Minimum hold at 0.30 for 30 samples (0.5 s)
    for i in 90..120 {
        throttles[i] = 0.30;
    }
    // 3. Recovery to 1.0 over 60 samples (1.0 s)
    for i in 120..180 {
        throttles[i] = 0.30 + ((i - 120) as f32 / 60.0) * 0.70;
    }

    let lap_dist_pct: Vec<f32> = (0..n).map(|i| i as f32 / n as f32).collect();
    let path = std::path::PathBuf::from(
        "Garage 61 - Test Driver - Oval Truck - Kansas Speedway - 00.30.000 - OVAL1.csv",
    );
    let metadata = parse_filename_metadata(&path);
    let lap = LapTelemetry {
        path,
        metadata,
        lap_dist_pct,
        brake: brakes,
        throttle: throttles,
        speed: vec![70.0; n],
    };

    let brake_zones = detect_brake_zones(&lap.brake);
    assert!(brake_zones.is_empty(), "oval laps have no brake zones");

    let lift_zones = detect_lift_zones(&lap.throttle, &lap.brake);
    assert_eq!(lift_zones.len(), 1, "oval lap must detect lift zone");
    let lz = &lift_zones[0];
    assert!(
        (lz.min_pct - 30.0).abs() < 1.0,
        "lift depth must match ~30%"
    );
    assert!(
        lz.duration_s >= 1.5,
        "lift duration should span drop through recovery"
    );

    let throttle_zones = detect_throttle_exit_zones(&lap.throttle, &brake_zones, &lift_zones);
    assert_eq!(
        throttle_zones.len(),
        1,
        "throttle exit zone must follow lift"
    );
    let tz = &throttle_zones[0];
    assert_eq!(
        tz.onset_idx, lz.min_idx,
        "exit zone onset must be the lift minimum"
    );
    assert!(tz.full_idx >= 179);

    // Stats collector verification
    let mut collector = StatsCollector::default();
    collector.add_lap(&lap, &brake_zones, &lift_zones, &throttle_zones);
    let report = collector.build_report();

    let car_stats = &report.cars["Oval Truck"];
    assert_eq!(car_stats.brake.zone_count, 0);
    assert_eq!(car_stats.lift.zone_count, 1);
    assert!(
        (car_stats.lift.lift_depth_pct.median - 30.0).abs() < 1.0,
        "stats must report non-zero lift depth (~30%)"
    );
    assert!(
        car_stats.lift.duration_s.median >= 1.5,
        "stats must report non-zero lift duration"
    );
    assert!(
        car_stats.throttle.progressive_ramp_rate_pct_s.median > 0.0,
        "stats must report exit ramp rate for oval"
    );

    // Extraction produces valid drills
    let options = ExtractOptions {
        preset_id: Some("oval-test".to_string()),
        preset_name: Some("Oval Test Preset".to_string()),
        out_path: None,
        tolerance: None,
        max_drills: 12,
    };
    let preset = extract_preset_from_laps(&[lap], &options).expect("extraction should succeed");
    preset
        .validate()
        .expect("extracted oval preset must validate");
    assert!(preset.drills.iter().any(|d| d.id.contains("lift")));
    assert!(preset.drills.iter().any(|d| d.id.contains("throttle")));
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
        lap_dist_pct: (0..100).map(|i| i as f32 / 100.0).collect(),
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

    collector.add_lap(&lap, &[b_zone], &[], &[]);
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
        lap_dist_pct: (0..250).map(|i| i as f32 / 250.0).collect(),
        brake: brakes,
        throttle: throttles,
        speed: vec![50.0; 250],
    };

    let options = ExtractOptions {
        preset_id: Some("test-synthetic-preset".to_string()),
        preset_name: Some("Synthetic Test Preset".to_string()),
        out_path: None,
        tolerance: None,
        max_drills: 12,
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
        // Omitted tolerance defaults to None, resolving to 10%
        assert_eq!(drill.tolerance, None);
        assert!((drill.tolerance_fraction() - 0.10).abs() < 1e-4);
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

#[test]
fn test_corner_grouping_and_options() {
    let n = 200;
    // Build 3 laps with consistent corner at dist ~0.25, wrap corner at ~0.99/0.01, and noise corner at ~0.60
    let mut laps = Vec::new();
    let brake_peaks = [70.0, 85.0, 80.0];

    for (lap_idx, &peak) in brake_peaks.iter().enumerate() {
        let mut b = vec![0.0; n];
        let mut t = vec![1.0; n];
        let dist: Vec<f32> = (0..n).map(|i| i as f32 / n as f32).collect();

        // Corner 1 at dist 0.25 (index 50)
        for i in 45..65 {
            t[i] = 0.0;
            b[i] = peak / 100.0;
        }
        // Corner 1 throttle exit (index 65..85)
        for i in 65..85 {
            t[i] = ((i - 65) as f32 / 20.0) * 0.98;
        }

        // Noise corner at dist 0.60 (index 120) - only on lap 0
        if lap_idx == 0 {
            for i in 115..130 {
                t[i] = 0.0;
                b[i] = 0.60;
            }
        }

        // Wrap corner near lap boundary:
        // Laps 0 & 1 have corner at dist 0.99 / 0.01 (index 195..200 and index 0..10)
        if lap_idx < 2 {
            for i in 195..200 {
                t[i] = 0.0;
                b[i] = 0.75;
            }
        }

        let path = std::path::PathBuf::from(format!(
            "Garage 61 - Driver - Group Car - Track Name - 01.20.{lap_idx:03} - ID{lap_idx}.csv"
        ));
        let metadata = parse_filename_metadata(&path);
        let lap = LapTelemetry {
            path,
            metadata,
            lap_dist_pct: dist,
            brake: b,
            throttle: t,
            speed: vec![50.0; n],
        };
        laps.push(lap);
    }

    // 1. Default tolerance should be None in output drills
    let options_default = ExtractOptions {
        preset_id: Some("group-test".to_string()),
        preset_name: Some("Grouping Test".to_string()),
        out_path: None,
        tolerance: None,
        max_drills: 12,
    };
    let preset = extract_preset_from_laps(&laps, &options_default).expect("extract succeeds");
    preset.validate().expect("extracted preset validates");

    // All drills should omit tolerance (None)
    for drill in &preset.drills {
        assert_eq!(drill.tolerance, None);
        assert!((drill.tolerance_fraction() - 0.10).abs() < 1e-4);
        assert!(drill.id.starts_with("track-name-c"));
    }

    // The noise corner (seen in 1 of 3 laps) must NOT be present
    // Only corner 1 (seen in 3 laps) should be present
    assert_ne!(preset.drills.len(), 0);
    assert!(preset.drills.iter().any(|d| d.id.contains("c01-brake")));

    // 2. Explicit tolerance should be preserved
    let options_tol = ExtractOptions {
        tolerance: Some(15.0),
        ..options_default.clone()
    };
    let preset_tol = extract_preset_from_laps(&laps, &options_tol).expect("extract succeeds");
    for drill in &preset_tol.drills {
        assert_eq!(drill.tolerance, Some(15.0));
        assert!((drill.tolerance_fraction() - 0.15).abs() < 1e-4);
    }

    // 3. Max drills capping
    let options_cap = ExtractOptions {
        max_drills: 1,
        ..options_default
    };
    let preset_cap = extract_preset_from_laps(&laps, &options_cap).expect("extract succeeds");
    assert_eq!(preset_cap.drills.len(), 1);
}

fn mk(n: usize, b: Vec<f32>, t: Vec<f32>, d: Vec<f32>, id: usize) -> LapTelemetry {
    let path = std::path::PathBuf::from(format!(
        "Garage 61 - D - Car - Trk - 01.20.{id:03} - ID{id}.csv"
    ));
    LapTelemetry {
        metadata: parse_filename_metadata(&path),
        path,
        brake: b,
        throttle: t,
        speed: vec![0.0; n],
        lap_dist_pct: d,
    }
}
fn opts(max: usize) -> ExtractOptions {
    ExtractOptions {
        preset_id: Some("p".into()),
        preset_name: Some("p".into()),
        out_path: None,
        tolerance: None,
        max_drills: max,
    }
}
fn brake(b: &mut [f32], t: &mut [f32], s: usize, len: usize, peak: f32) {
    for i in s..s + len {
        b[i] = peak;
        t[i] = 0.0;
    }
}
fn dist(n: usize) -> Vec<f32> {
    (0..n).map(|i| i as f32 / n as f32).collect()
}

#[test]
fn probe_wrap_mean() {
    let n = 3000;
    let mut laps = vec![];
    for k in 0..4 {
        let mut b = vec![0.0; n];
        let mut t = vec![1.0; n];
        brake(&mut b, &mut t, 900, 60, 0.60); // A at 0.30
        brake(&mut b, &mut t, 2100, 60, 0.70); // B at 0.70
        if k < 2 {
            brake(&mut b, &mut t, 2985, 15, 0.90);
        } else {
            brake(&mut b, &mut t, 9, 30, 0.91);
        }
        laps.push(mk(n, b, t, dist(n), k));
    }
    let p = extract_preset_from_laps(&laps, &opts(12)).unwrap();
    let mut onsets = p
        .drills
        .iter()
        .filter_map(|d| {
            let n = d.id.split('-').nth(1)?; // c01, c02
            Some(n)
        })
        .collect::<Vec<_>>();
    onsets.dedup();
    assert_eq!(onsets.len(), 3);

    let wrap_drills = p
        .drills
        .iter()
        .filter(|d| d.id.contains("c03"))
        .collect::<Vec<_>>();
    assert!(!wrap_drills.is_empty(), "Wrap corner should be C03");
}

#[test]
fn probe_chicane() {
    let n = 3000;
    let mut laps = vec![];
    for k in 0..3 {
        let mut b = vec![0.0; n];
        let mut t = vec![1.0; n];
        brake(&mut b, &mut t, 300, 30, 0.90); // onset 0.100
        for i in 330..360 {
            t[i] = 0.5;
        }
        brake(&mut b, &mut t, 360, 30, 0.50); // onset 0.120
        laps.push(mk(n, b, t, dist(n), k));
    }
    let p = extract_preset_from_laps(&laps, &opts(12)).unwrap();
    let mut corners = p
        .drills
        .iter()
        .filter_map(|d| d.id.split('-').nth(1))
        .collect::<Vec<_>>();
    corners.dedup();
    assert_eq!(
        corners.len(),
        2,
        "Two separate brake zones in each lap should yield 2 corners, not 1 mixed cluster"
    );
}

#[test]
fn probe_budget() {
    let n = 3000;
    let mut laps = vec![];
    for k in 0..1 {
        let mut b = vec![0.0; n];
        let t = vec![1.0; n];
        for i in 600..660 {
            b[i] = 0.50; // low priority at 0.2
        }
        for i in 2100..2160 {
            b[i] = 0.95; // high priority at 0.7
        }
        laps.push(mk(n, b, t, dist(n), k));
    }
    let p = extract_preset_from_laps(&laps, &opts(3)).unwrap();
    let hold_drills = p
        .drills
        .iter()
        .filter(|d| matches!(d.kind, sct_core::preset::DrillKind::Hold { .. }))
        .collect::<Vec<_>>();
    assert_eq!(hold_drills.len(), 1);
    assert!(
        hold_drills[0].id.contains("c02"),
        "The top priority corner should keep its hold drill"
    );
}

#[test]
fn probe_oval_steps() {
    let n = 300;
    let brakes = vec![0.0; n];
    let mut throttles = vec![1.0; n];
    for i in 60..90 {
        throttles[i] = 1.0 - ((i - 60) as f32 / 30.0) * 0.70;
    }
    for i in 90..120 {
        throttles[i] = 0.30;
    }
    for i in 120..180 {
        throttles[i] = 0.30 + ((i - 120) as f32 / 60.0) * 0.70;
    }
    let bz = detect_brake_zones(&brakes);
    let lz = detect_lift_zones(&throttles, &brakes);
    let tz = detect_throttle_exit_zones(&throttles, &bz, &lz);
    let m = analyze_throttle_exit(&throttles, &tz[0]);
    assert!(m.fast_steps <= 1);
    assert_eq!(m.plateau_levels_pct.len(), 0);
}

#[test]
fn probe_lap_start_lift() {
    let n = 600;
    let b = vec![0.0; n];
    let mut t = vec![1.0; n];
    for i in 0..30 {
        t[i] = 0.5 + i as f32 / 60.0;
    }
    let lz = detect_lift_zones(&t, &b);
    assert!(
        lz.is_empty(),
        "lift onset requires earlier sample >= 95% in same lap"
    );
}

#[test]
fn probe_nan() {
    let n = 600;
    let mut b = vec![0.0; n];
    let mut t = vec![1.0; n];
    brake(&mut b, &mut t, 200, 60, 0.8);
    b[230] = f32::NAN;
    let lap = mk(n, b, t, dist(n), 0);
    let _r = extract_preset_from_laps(&[lap], &opts(12)).unwrap();
}

#[test]
fn probe_long_zone() {
    let n = 4000;
    let mut b = vec![0.0; n];
    let mut t = vec![1.0; n];
    for i in 100..3800 {
        b[i] = 0.3 + 0.1 * ((i % 7) as f32 / 7.0);
        t[i] = 0.0;
    }
    let bz = detect_brake_zones(&b);
    assert!(bz.is_empty(), "zones > 60s should be skipped");
}

#[test]
fn probe_nan_edge() {
    let n = 600;
    let mut b = vec![0.0; n];
    let mut t = vec![1.0; n];
    brake(&mut b, &mut t, 200, 60, 0.8);
    b[182] = f32::NAN;
    let lap = mk(n, b, t, dist(n), 0);
    let _r = extract_preset_from_laps(&[lap], &opts(12)).unwrap();
}

#[test]
fn probe_median_choice() {
    let mut laps = vec![];
    let n = 1000;
    let peaks = [70.0, 85.0, 80.0];
    for (k, &p) in peaks.iter().enumerate() {
        let mut b = vec![0.0; n];
        let mut t = vec![1.0; n];
        brake(&mut b, &mut t, 200, 100, p / 100.0);
        laps.push(mk(n, b, t, dist(n), k));
    }
    let p = extract_preset_from_laps(&laps, &opts(12)).unwrap();
    let b_drills = p
        .drills
        .iter()
        .filter(|d| matches!(d.kind, sct_core::preset::DrillKind::Hold { .. }))
        .collect::<Vec<_>>();
    assert_eq!(b_drills.len(), 1);
    if let sct_core::preset::DrillKind::Hold { target, .. } = b_drills[0].kind {
        assert!((target - 80.0).abs() < 1e-4, "Target should be median 80.0");
    }
}

#[test]
fn test_parse_lap_time_fraction_by_digit_count() {
    assert_eq!(parse_lap_time_str("01:03.799"), Some(63.799));
    assert_eq!(parse_lap_time_str("01.03.799"), Some(63.799));
    assert_eq!(parse_lap_time_str("63.799"), Some(63.799));
    assert_eq!(parse_lap_time_str("30.5"), Some(30.5));
    assert_eq!(parse_lap_time_str("01:15.50"), Some(75.50));
}

#[test]
fn test_one_frame_throttle_touch_before_braking_is_not_sustained() {
    // 200 frames of synthetic throttle telemetry.
    // BrakeZone 1: onset 10, release 30, peak 20.
    // BrakeZone 2: onset 100, release 120, peak 110.
    // For BrakeZone 1, window_end is 100 (< throttles.len() = 200).
    // Throttle onset starts around frame 60 (0.70).
    // Frame 99 (1 frame before braking onset at 100): 1-frame touch of 0.98.
    let mut throttles = vec![0.0; 200];
    for t in &mut throttles[60..99] {
        *t = 0.70;
    }
    throttles[99] = 0.98; // 1-frame touch right before braking at frame 100

    let bz1 = BrakeZone {
        onset_idx: 10,
        release_idx: 30,
        peak_idx: 20,
        peak_pct: 80.0,
        duration_s: 20.0 / 60.0,
    };
    let bz2 = BrakeZone {
        onset_idx: 100,
        release_idx: 120,
        peak_idx: 110,
        peak_pct: 80.0,
        duration_s: 20.0 / 60.0,
    };

    let exit_zones = detect_throttle_exit_zones(&throttles, &[bz1.clone(), bz2.clone()], &[]);
    assert!(
        exit_zones.is_empty(),
        "a 1-frame 98% touch right before braking should NOT be detected as sustained full throttle"
    );

    // Conversely, if full throttle is sustained for THROTTLE_SUSTAINED_FRAMES (5 frames, 95..100):
    for t in &mut throttles[95..100] {
        *t = 0.98;
    }
    let exit_zones_sustained = detect_throttle_exit_zones(&throttles, &[bz1, bz2], &[]);
    assert_eq!(
        exit_zones_sustained.len(),
        1,
        "5 sustained frames of full throttle before braking should qualify"
    );
    assert_eq!(exit_zones_sustained[0].full_idx, 95);
}

#[test]
fn test_drill_id_format_slug() {
    let n = 200;
    let mut b = vec![0.0; n];
    let mut t = vec![1.0; n];
    for i in 20..50 {
        b[i] = 0.80;
        t[i] = 0.0;
    }
    let path = std::path::PathBuf::from(
        "Garage 61 - Driver - Car - Road Atlanta (Full Course) - 01.20.000 - ID1.csv",
    );
    let lap = LapTelemetry {
        metadata: parse_filename_metadata(&path),
        path,
        brake: b,
        throttle: t,
        speed: vec![50.0; n],
        lap_dist_pct: (0..n).map(|i| i as f32 / n as f32).collect(),
    };
    let opts = ExtractOptions {
        preset_id: Some("test".into()),
        preset_name: Some("test".into()),
        out_path: None,
        tolerance: None,
        max_drills: 12,
    };
    let preset = extract_preset_from_laps(&[lap], &opts).expect("extract succeeds");
    assert_ne!(preset.drills.len(), 0);
    for drill in &preset.drills {
        assert!(
            drill.id.starts_with("road-atlanta-c"),
            "drill ID '{}' should start with 'road-atlanta-c' and not 'troad-atlanta-c'",
            drill.id
        );
        assert!(!drill.id.starts_with("troad-atlanta"));
    }
}
