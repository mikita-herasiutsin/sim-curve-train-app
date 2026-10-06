mod input;

use input::InputService;
use sct_core::AppInfo;
use sct_core::device::DevicesSnapshot;
use sct_core::stream::SampleBatch;
use tauri::Manager;
use tauri::ipc::Channel;

/// Returns the app name and version for the UI header.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn app_info(app: tauri::AppHandle) -> AppInfo {
    AppInfo::new(app.package_info().version.to_string())
}

/// Returns the connected game controllers. Changes arrive via the `devices-changed` event.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn list_devices(input: tauri::State<'_, InputService>) -> DevicesSnapshot {
    input.snapshot()
}

/// Streams raw samples of one device to `on_batch` (about every 8 ms) until `stop_stream`.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn start_stream(
    device_id: u32,
    on_batch: Channel<SampleBatch>,
    input: tauri::State<'_, InputService>,
) -> Result<(), String> {
    input.start_stream(device_id, on_batch)
}

/// Stops the active sample stream, if any.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn stop_stream(input: tauri::State<'_, InputService>) -> Result<(), String> {
    input.stop_stream()
}

/// Builds and runs the Tauri application.
///
/// # Panics
///
/// Panics if the Tauri runtime fails to start (e.g. `WebView2` is missing).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            app.manage(InputService::spawn(app.handle().clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            list_devices,
            start_stream,
            stop_stream
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
