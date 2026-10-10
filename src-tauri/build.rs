fn main() {
    // Declaring the app's commands makes each one need an explicit `allow-*` permission in
    // `capabilities/`, instead of every command being callable from the webview.
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "app_info",
            "list_devices",
            "start_stream",
            "stop_stream",
            "detect_axis",
            "capture_range",
            "calibrate",
            "load_profile",
            "save_profile",
            "reset_profile",
            "profiled_devices",
            "list_presets",
            "start_drill_run",
            "abort_drill_run",
            "save_attempt",
            "list_attempts",
            "best_total",
            "best_totals",
            "save_warm_up_run",
            "list_warm_up_runs",
            "audio_test_tone",
            "audio_set_enabled",
            "audio_set_volume",
            "set_sim_pedals",
        ]),
    ))
    .expect("failed to run tauri-build");
}
