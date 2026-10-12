mod audio;
mod drill_thread;
mod input;
#[cfg(debug_assertions)]
mod sim_pedals;
mod window;

use std::collections::HashMap;

use audio::AudioFeedback;
use input::InputService;
use sct_core::attempts::{Attempt, AttemptStore, NewAttempt, NewWarmUpRun, WarmUpRun};
use sct_core::axis_detect::Detection;
use sct_core::calibration::{AxisCalibration, RangeCapture};
use sct_core::device::DevicesSnapshot;
use sct_core::drill_engine::DrillEvent;
use sct_core::preset::{
    Preset, bundled_presets, find_drill, find_warm_up_drill, load_with_overrides,
};
use sct_core::profile::{DeviceProfile, ProfileStore};
use sct_core::stream::SampleBatch;
use sct_core::{AppInfo, BuildInfo};
use tauri::Manager;
use tauri::ipc::Channel;

fn current_app_info(app: &tauri::AppHandle) -> AppInfo {
    let mut info = AppInfo::new(app.package_info().version.to_string());
    info.build = build_info();
    info.test_data = data_dir_override().is_some();
    #[cfg(debug_assertions)]
    {
        info.sim_pedals = sim_pedals::enabled_by_env();
    }
    info
}

/// Returns the app name and version for the UI header.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn app_info(app: tauri::AppHandle) -> AppInfo {
    current_app_info(&app)
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

/// Streams samples of one device to `on_batch` (about every 8 ms) until `stop_stream`
/// is called with the returned token. `raw` selects whether batches carry raw samples,
/// which only the device setup screens need.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn start_stream(
    device_id: u32,
    raw: bool,
    on_batch: Channel<SampleBatch>,
    input: tauri::State<'_, InputService>,
) -> Result<u64, String> {
    input.start_stream(device_id, raw, on_batch)
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

/// Dev only: sets the simulated pedals (fractions 0 to 1: throttle, brake, clutch) or turns on
/// their automatic cycle.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn set_sim_pedals(
    values: [f32; 3],
    auto: bool,
    input: tauri::State<'_, InputService>,
) -> Result<(), String> {
    input.set_sim_pedals(values, auto)
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

/// Returns the highest total score per drill of a preset, keyed by drill id.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn best_totals(
    preset_id: String,
    attempts: tauri::State<'_, AttemptsService>,
) -> Result<HashMap<String, f32>, String> {
    let guard = lock_attempts(&attempts.store);
    let store = guard
        .as_ref()
        .ok_or_else(|| "attempts store is unavailable".to_string())?;
    store.best_totals(&preset_id).map_err(|e| e.to_string())
}

/// Saves a completed warm-up run and its step results.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn save_warm_up_run(
    run: NewWarmUpRun,
    attempts: tauri::State<'_, AttemptsService>,
) -> Result<i64, String> {
    let guard = lock_attempts(&attempts.store);
    let store = guard
        .as_ref()
        .ok_or_else(|| "attempts store is unavailable".to_string())?;
    store.save_warm_up_run(&run).map_err(|e| e.to_string())
}

/// Lists recorded warm-up runs for a preset, ordered from newest to oldest.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn list_warm_up_runs(
    preset_id: String,
    limit: u32,
    attempts: tauri::State<'_, AttemptsService>,
) -> Result<Vec<WarmUpRun>, String> {
    let guard = lock_attempts(&attempts.store);
    let store = guard
        .as_ref()
        .ok_or_else(|| "attempts store is unavailable".to_string())?;
    store
        .list_warm_up_runs(&preset_id, limit)
        .map_err(|e| e.to_string())
}

/// Loads the presets compiled into the binary. An optional `presets/` folder in the
/// resource dir (next to the exe) replaces presets by id or adds new ones.
fn load_presets(app: &tauri::AppHandle) -> Result<Vec<Preset>, String> {
    let dir = match app.path().resource_dir() {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("Presets: no resource dir ({e}), using the bundled presets");
            return bundled_presets().map_err(|e| format!("Failed to load presets: {e}"));
        }
    };
    load_with_overrides(&dir.join("presets")).map_err(|e| format!("Failed to load presets: {e}"))
}

/// Lists all bundled drill presets.
/// `async` so the file reads run off the main thread.
#[tauri::command(async)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn list_presets(app: tauri::AppHandle) -> Result<Vec<Preset>, String> {
    load_presets(&app)
}

/// Starts a drill run for a bundled drill, feeding samples from the active stream.
///
/// When `warm_up` is true, reps are configured from the preset's warm-up routine.
/// The drill is looked up on the Rust side; the webview only names it.
/// `async` so it can wait for the input thread without blocking the main thread.
#[tauri::command(async)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn start_drill_run(
    token: u64,
    preset_id: String,
    drill_id: String,
    warm_up: bool,
    on_event: Channel<DrillEvent>,
    app: tauri::AppHandle,
    input: tauri::State<'_, InputService>,
) -> Result<(), String> {
    let presets = load_presets(&app)?;
    let drill = if warm_up {
        find_warm_up_drill(&presets, &preset_id, &drill_id)?
    } else {
        find_drill(&presets, &preset_id, &drill_id)?
    };
    input.start_drill(token, drill, on_event)
}

/// Aborts the active drill run.
#[tauri::command]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri injects command arguments by value"
)]
fn abort_drill_run(token: u64, input: tauri::State<'_, InputService>) -> Result<(), String> {
    input.abort_drill(token)
}

/// Plays a short test tone. Registered in debug builds only (see `run`).
#[cfg(debug_assertions)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri commands require State by value"
)]
#[tauri::command]
fn audio_test_tone(audio: tauri::State<'_, AudioFeedback>) {
    audio.test_tone();
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri commands require State by value"
)]
#[tauri::command]
fn audio_set_enabled(enabled: bool, audio: tauri::State<'_, AudioFeedback>) {
    audio.set_enabled(enabled);
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "Tauri commands require State by value"
)]
#[tauri::command]
fn audio_set_volume(volume: f32, audio: tauri::State<'_, AudioFeedback>) {
    audio.set_volume(volume);
}

/// The absolute `SCT_DATA_DIR` a debug build uses instead of the app data directory.
#[cfg(debug_assertions)]
fn data_dir_override() -> Option<std::path::PathBuf> {
    static CACHE: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| {
            let dir = std::path::PathBuf::from(std::env::var_os("SCT_DATA_DIR")?);
            if dir.is_absolute() {
                Some(dir)
            } else {
                eprintln!("ignoring SCT_DATA_DIR: it must be an absolute path");
                None
            }
        })
        .clone()
}

#[cfg(not(debug_assertions))]
fn data_dir_override() -> Option<std::path::PathBuf> {
    None
}

/// Arguments that make `git rev-parse` print the worktree path, the full commit hash and the
/// branch, one per line. `rev-parse` prints `--show-toplevel` when it parses it, and
/// `--abbrev-ref` applies to every later revision, so the order matters.
#[cfg(debug_assertions)]
const GIT_BUILD_ARGS: [&str; 5] = [
    "rev-parse",
    "--show-toplevel",
    "HEAD",
    "--abbrev-ref",
    "HEAD",
];

/// Parses the output of `git` with [`GIT_BUILD_ARGS`]: worktree path, full hash, branch.
#[cfg_attr(
    not(debug_assertions),
    expect(dead_code, reason = "release builds report no git state")
)]
fn parse_build_info(output: &str) -> Option<BuildInfo> {
    let lines: Vec<&str> = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let &[toplevel, hash, branch] = lines.as_slice() else {
        return None;
    };
    let commit = hash.get(..7)?;
    let branch = if branch == "HEAD" { "detached" } else { branch };
    let worktree = toplevel
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())?;
    Some(BuildInfo {
        branch: branch.to_owned(),
        commit: commit.to_owned(),
        worktree: worktree.to_owned(),
    })
}

#[cfg(debug_assertions)]
fn build_info() -> Option<BuildInfo> {
    static CACHE: std::sync::OnceLock<Option<BuildInfo>> = std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(env!("CARGO_MANIFEST_DIR"))
                .args(GIT_BUILD_ARGS)
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            parse_build_info(&String::from_utf8(output.stdout).ok()?)
        })
        .clone()
}

#[cfg(not(debug_assertions))]
fn build_info() -> Option<BuildInfo> {
    None
}

/// The directory for the profile and attempts database. Debug builds honour an absolute
/// `SCT_DATA_DIR`, so automated runs (`npm run smoke`) use a throwaway directory instead of the
/// user's real data. An empty or relative value is ignored.
fn data_dir(app: &tauri::App) -> tauri::Result<std::path::PathBuf> {
    if let Some(dir) = data_dir_override() {
        eprintln!(
            "SCT_DATA_DIR is set: using {} for saved data",
            dir.display()
        );
        return Ok(dir);
    }
    app.path().app_data_dir()
}

/// Opens the profile database in [`data_dir`]. The app still runs without it.
fn open_profile_store(app: &tauri::App) -> Option<ProfileStore> {
    let path = match data_dir(app) {
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

/// Opens the attempts database in [`data_dir`]. The app still runs without it.
fn open_attempt_store(app: &tauri::App) -> Option<AttemptStore> {
    let path = match data_dir(app) {
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
            window::fit_main_window(app);
            if let Some(title) = current_app_info(app.handle()).window_title() {
                if let Some(main_window) = app.get_webview_window("main") {
                    if let Err(error) = main_window.set_title(&title) {
                        eprintln!("failed to set window title to {title:?}: {error}");
                    }
                } else {
                    eprintln!("failed to get main window to set title");
                }
            }
            let store = open_profile_store(app);
            let audio = AudioFeedback::new();
            app.manage(InputService::spawn(
                app.handle().clone(),
                store,
                Some(audio.clone()),
            ));

            let attempt_store = open_attempt_store(app);
            app.manage(AttemptsService::new(attempt_store));
            app.manage(audio);
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
            list_presets,
            start_drill_run,
            abort_drill_run,
            save_attempt,
            list_attempts,
            best_total,
            best_totals,
            save_warm_up_run,
            list_warm_up_runs,
            #[cfg(debug_assertions)]
            audio_test_tone,
            audio_set_enabled,
            audio_set_volume,
            set_sim_pedals
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::parse_build_info;

    const HASH: &str = "0e9d0d84ee53946f1f8074281ef53683ca496265";

    #[test]
    fn parses_branch_commit_and_worktree() {
        let info = parse_build_info(&format!(
            "D:/workspace/projects/sct-wt-ui-polish\n{HASH}\nui-polish\n"
        ))
        .expect("three lines parse");
        assert_eq!(info.branch, "ui-polish");
        assert_eq!(info.commit, "0e9d0d8");
        assert_eq!(info.worktree, "sct-wt-ui-polish");
    }

    #[test]
    fn detached_head_is_named_detached() {
        let info =
            parse_build_info(&format!("D:/workspace/sct-wt-x\n{HASH}\nHEAD\n")).expect("parses");
        assert_eq!(info.branch, "detached");
        assert_eq!(info.commit, "0e9d0d8");
        assert_eq!(info.worktree, "sct-wt-x");
    }

    #[test]
    fn windows_path_with_trailing_separator_uses_last_component() {
        let info =
            parse_build_info(&format!("D:\\a\\sct-wt-x\\\r\n{HASH}\r\nmain\r\n")).expect("parses");
        assert_eq!(info.worktree, "sct-wt-x");
    }

    #[test]
    fn two_lines_are_rejected() {
        assert_eq!(parse_build_info(&format!("D:/a/x\n{HASH}\n")), None);
    }

    /// Runs the real `git` call, so a change to the argument order that breaks the line order
    /// fails here.
    #[cfg(debug_assertions)]
    #[test]
    fn real_git_output_parses() {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(env!("CARGO_MANIFEST_DIR"))
            .args(super::GIT_BUILD_ARGS)
            .output()
            .expect("git runs");
        assert!(output.status.success(), "git rev-parse failed");
        let info = parse_build_info(&String::from_utf8(output.stdout).unwrap()).expect("parses");
        assert_eq!(info.commit.len(), 7);
        assert!(info.commit.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!info.worktree.contains(['/', '\\']));
    }

    #[test]
    fn empty_output_is_rejected() {
        assert_eq!(parse_build_info(""), None);
        assert_eq!(parse_build_info("\n \n"), None);
    }
}
