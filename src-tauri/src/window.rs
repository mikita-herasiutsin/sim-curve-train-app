//! Sizes the main window to the screen it opens on.

use tauri::{LogicalSize, Manager};

/// Smallest window the UI supports; keep in sync with `minWidth`/`minHeight` in `tauri.conf.json`.
const MIN_SIZE: (f64, f64) = (960.0, 600.0);
/// Largest default window; bigger screens don't need a bigger window.
const MAX_SIZE: (f64, f64) = (1600.0, 1000.0);
/// Share of the monitor's work area the window takes.
const SCREEN_SHARE: f64 = 0.9;

/// Window size in logical pixels for a work area of `work_area` (logical pixels): about 90%
/// of it, capped at `max`, but never below `min`.
fn fit_window(work_area: (f64, f64), min: (f64, f64), max: (f64, f64)) -> (f64, f64) {
    let fit = |area: f64, min: f64, max: f64| (area * SCREEN_SHARE).min(max).max(min).round();
    (
        fit(work_area.0, min.0, max.0),
        fit(work_area.1, min.1, max.1),
    )
}

/// Resizes the main window to fit its monitor and centers it. Keeps the configured size if no
/// monitor can be found.
pub fn fit_main_window(app: &tauri::App) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let scale = monitor.scale_factor();
    if scale <= 0.0 {
        return;
    }
    let area = monitor.work_area().size;
    let (width, height) = fit_window(
        (
            f64::from(area.width) / scale,
            f64::from(area.height) / scale,
        ),
        MIN_SIZE,
        MAX_SIZE,
    );
    if window.set_size(LogicalSize::new(width, height)).is_ok() {
        let _ = window.center();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_90_percent_of_a_small_work_area() {
        // 1280x720 laptop minus taskbar: the height floor wins.
        assert_eq!(
            fit_window((1280.0, 680.0), MIN_SIZE, MAX_SIZE),
            (1152.0, 612.0)
        );
    }

    #[test]
    fn caps_on_large_screens() {
        assert_eq!(
            fit_window((3840.0, 2100.0), MIN_SIZE, MAX_SIZE),
            (1600.0, 1000.0)
        );
    }

    #[test]
    fn never_goes_below_the_minimum() {
        assert_eq!(
            fit_window((800.0, 500.0), MIN_SIZE, MAX_SIZE),
            (960.0, 600.0)
        );
    }
}
