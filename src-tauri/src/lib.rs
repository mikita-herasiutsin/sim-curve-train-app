mod input;

use input::InputService;
use sct_core::AppInfo;
use sct_core::attempts::{Attempt, AttemptStore, NewAttempt};
use sct_core::axis_detect::Detection;
use sct_core::calibration::{AxisCalibration, RangeCapture};
use sct_core::device::DevicesSnapshot;
use sct_core::profile::{DeviceProfile, ProfileStore};
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

/// Streams raw samples of one device to `on_batch` (about every 8 ms) until `stop_stream`
/// is called with the returned token.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn start_stream(
    device_id: u32,
    on_batch: Channel<SampleBatch>,
    input: tauri::State<'_, InputService>,
) -> Result<u64, String> {
    input.start_stream(device_id, on_batch)
}

/// Stops the stream started with `token`, unless a newer stream has replaced it.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn stop_stream(token: u64, input: tauri::State<'_, InputService>) -> Result<(), String> {
    input.stop_stream(token)
}

/// Detects which axis moved during a wizard step that started at `since_us` (sample clock).
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn detect_axis(
    since_us: u64,
    exclude: Vec<usize>,
    input: tauri::State<'_, InputService>,
) -> Detection {
    input.detect_axis(since_us, &exclude)
}

/// Returns the raw `(min, max)` of `axis` since `since_us`, for re-sweeping a pedal's range.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn capture_range(
    axis: usize,
    since_us: u64,
    input: tauri::State<'_, InputService>,
) -> Option<(i16, i16)> {
    input.capture_range(axis, since_us)
}

/// Builds a calibration from a pedal sweep: raw `min`/`max` seen and the released `rest` value.
#[tauri::command]
fn calibrate(
    min: i16,
    max: i16,
    rest: i16,
    deadzone_low: f32,
    deadzone_high: f32,
) -> Result<AxisCalibration, String> {
    let mut capture = RangeCapture::new();
    capture.observe(min);
    capture.observe(max);
    capture
        .finish(rest, deadzone_low, deadzone_high)
        .map_err(|e| e.to_string())
}

/// Returns the saved profile of a connected device, if any.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn load_profile(
    device_id: u32,
    input: tauri::State<'_, InputService>,
) -> Result<Option<DeviceProfile>, String> {
    input.load_profile(device_id)
}

/// Saves (and applies) a device's axis assignment and calibration.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn save_profile(
    device_id: u32,
    profile: DeviceProfile,
    input: tauri::State<'_, InputService>,
) -> Result<(), String> {
    input.save_profile(device_id, profile)
}

/// Deletes a device's saved profile. Returns whether one existed.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn reset_profile(device_id: u32, input: tauri::State<'_, InputService>) -> Result<bool, String> {
    input.reset_profile(device_id)
}

/// Ids of connected devices with a saved profile; the live view picks the first one.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn profiled_devices(input: tauri::State<'_, InputService>) -> Vec<u32> {
    input.profiled_devices()
}

/// Shared handle to the attempts database, managed by Tauri.
#[derive(Clone)]
pub struct AttemptsService {
    store: std::sync::Arc<std::sync::Mutex<Option<AttemptStore>>>,
}

impl AttemptsService {
    #[must_use]
    pub fn new(store: Option<AttemptStore>) -> Self {
        Self {
            store: std::sync::Arc::new(std::sync::Mutex::new(store)),
        }
    }
}

fn lock_attempts<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Saves a completed or aborted drill set attempt and each of its reps.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn save_attempt(
    attempt: NewAttempt,
    attempts: tauri::State<'_, AttemptsService>,
) -> Result<i64, String> {
    let guard = lock_attempts(&attempts.store);
    let store = guard
        .as_ref()
        .ok_or_else(|| "attempts store is unavailable".to_string())?;
    store.save_attempt(&attempt).map_err(|e| e.to_string())
}

/// Lists recorded attempts for a drill, ordered from newest to oldest.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn list_attempts(
    drill_id: String,
    limit: u32,
    attempts: tauri::State<'_, AttemptsService>,
) -> Result<Vec<Attempt>, String> {
    let guard = lock_attempts(&attempts.store);
    let store = guard
        .as_ref()
        .ok_or_else(|| "attempts store is unavailable".to_string())?;
    store
        .list_attempts(&drill_id, limit)
        .map_err(|e| e.to_string())
}

/// Returns the highest total score recorded for a drill, or `None` if no attempts exist.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn best_total(
    drill_id: String,
    attempts: tauri::State<'_, AttemptsService>,
) -> Result<Option<f32>, String> {
    let guard = lock_attempts(&attempts.store);
    let store = guard
        .as_ref()
        .ok_or_else(|| "attempts store is unavailable".to_string())?;
    store.best_total(&drill_id).map_err(|e| e.to_string())
}

/// Opens the profile database in the app data directory. The app still runs without it.
fn open_profile_store(app: &tauri::App) -> Option<ProfileStore> {
    let path = match app.path().app_data_dir() {
        Ok(dir) => dir.join("profiles.db"),
        Err(error) => {
            eprintln!("no app data directory, profiles won't be saved: {error}");
            return None;
        }
    };
    ProfileStore::open(&path)
        .inspect_err(|error| eprintln!("failed to open {}: {error}", path.display()))
        .ok()
}

/// Opens the attempts database in the app data directory. The app still runs without it.
fn open_attempt_store(app: &tauri::App) -> Option<AttemptStore> {
    let path = match app.path().app_data_dir() {
        Ok(dir) => dir.join("profiles.db"),
        Err(error) => {
            eprintln!("no app data directory, attempts won't be saved: {error}");
            return None;
        }
    };
    AttemptStore::open(&path)
        .inspect_err(|error| eprintln!("failed to open attempts in {}: {error}", path.display()))
        .ok()
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
            let store = open_profile_store(app);
            app.manage(InputService::spawn(app.handle().clone(), store));

            let attempt_store = open_attempt_store(app);
            app.manage(AttemptsService::new(attempt_store));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            list_devices,
            start_stream,
            stop_stream,
            detect_axis,
            capture_range,
            calibrate,
            load_profile,
            save_profile,
            reset_profile,
            profiled_devices,
            save_attempt,
            list_attempts,
            best_total
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
