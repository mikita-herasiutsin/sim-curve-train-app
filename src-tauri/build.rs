fn main() {
    // Declaring the app's commands makes each one need an explicit `allow-*` permission in
    // `capabilities/`, instead of every command being callable from the webview.
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&["app_info"])),
    )
    .expect("failed to run tauri-build");
}
