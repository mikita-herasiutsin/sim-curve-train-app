//! UI-independent core logic for `SimCurveTrainApp`.
//!
//! Everything that can be tested without a window (input processing, drill engine,
//! scoring, storage) lives here; `src-tauri` only wires it to the UI.

pub mod axis_detect;
pub mod calibration;
pub mod device;
pub mod dsp;
pub mod input;
pub mod preset;
pub mod profile;
pub mod ring_buffer;
pub mod scoring;
pub mod set_summary;
pub mod stream;
pub mod trace_scoring;

use serde::Serialize;

/// Human-readable product name, shown in the window title and the UI header.
pub const APP_NAME: &str = "SimCurveTrainApp";

/// Basic application metadata exposed to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

impl AppInfo {
    /// Builds app metadata for the given version (normally the Tauri package version).
    #[must_use]
    pub fn new(version: impl Into<String>) -> Self {
        Self {
            name: APP_NAME.to_owned(),
            version: version.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_uses_product_name_and_given_version() {
        let info = AppInfo::new("1.2.3");
        assert_eq!(info.name, "SimCurveTrainApp");
        assert_eq!(info.version, "1.2.3");
    }

    #[test]
    fn serializes_to_flat_json_object() {
        let json = serde_json::to_value(AppInfo::new("0.1.0")).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "name": "SimCurveTrainApp", "version": "0.1.0" })
        );
    }
}
