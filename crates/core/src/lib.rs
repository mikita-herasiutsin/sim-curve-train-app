//! UI-independent core logic for `SimCurveTrainApp`.
//!
//! Everything that can be tested without a window (input processing, drill engine,
//! scoring, storage) lives here; `src-tauri` only wires it to the UI.

pub mod attempts;
pub mod audio_map;
pub mod axis_detect;
pub mod calibration;
pub mod db;
pub mod device;
pub mod drill_engine;
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

/// Git state of a dev build, so several worktree builds can be told apart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    pub branch: String,
    pub commit: String,
    pub worktree: String,
}

/// Basic application metadata exposed to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build: Option<BuildInfo>,
    pub test_data: bool,
    pub sim_pedals: bool,
}

impl AppInfo {
    /// Builds app metadata for the given version (normally the Tauri package version).
    #[must_use]
    pub fn new(version: impl Into<String>) -> Self {
        Self {
            name: APP_NAME.to_owned(),
            version: version.into(),
            build: None,
            test_data: false,
            sim_pedals: false,
        }
    }

    /// Window title for a dev or test run, or `None` for a normal run (the title from tauri.conf.json stays).
    #[must_use]
    pub fn window_title(&self) -> Option<String> {
        let mut parts = vec![self.name.clone()];
        if let Some(build) = &self.build {
            parts.push(format!("{} @ {}", build.branch, build.commit));
        }
        if self.test_data {
            parts.push("TEST DATA".to_string());
        }
        if self.sim_pedals {
            parts.push("SIM PEDALS".to_string());
        }
        if parts.len() == 1 {
            None
        } else {
            Some(parts.join(" · "))
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
            serde_json::json!({
                "name": "SimCurveTrainApp",
                "version": "0.1.0",
                "testData": false,
                "simPedals": false,
            })
        );
    }

    #[test]
    fn serializes_with_build_info() {
        let mut info = AppInfo::new("0.1.0");
        info.build = Some(BuildInfo {
            branch: "main".into(),
            commit: "abc1234".into(),
            worktree: "sct".into(),
        });
        let json = serde_json::to_value(info).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "name": "SimCurveTrainApp",
                "version": "0.1.0",
                "build": {
                    "branch": "main",
                    "commit": "abc1234",
                    "worktree": "sct",
                },
                "testData": false,
                "simPedals": false,
            })
        );
    }

    #[test]
    fn window_title_plain_returns_none() {
        let info = AppInfo::new("0.1.0");
        assert_eq!(info.window_title(), None);
    }

    #[test]
    fn window_title_build_only() {
        let mut info = AppInfo::new("0.1.0");
        info.build = Some(BuildInfo {
            branch: "main".into(),
            commit: "abc1234".into(),
            worktree: "sct".into(),
        });
        assert_eq!(
            info.window_title(),
            Some("SimCurveTrainApp · main @ abc1234".to_string())
        );
    }

    #[test]
    fn window_title_build_test_data_sim_pedals() {
        let mut info = AppInfo::new("0.1.0");
        info.build = Some(BuildInfo {
            branch: "main".into(),
            commit: "abc1234".into(),
            worktree: "sct".into(),
        });
        info.test_data = true;
        info.sim_pedals = true;
        assert_eq!(
            info.window_title(),
            Some("SimCurveTrainApp · main @ abc1234 · TEST DATA · SIM PEDALS".to_string())
        );
    }

    #[test]
    fn window_title_test_data_only() {
        let mut info = AppInfo::new("0.1.0");
        info.test_data = true;
        assert_eq!(
            info.window_title(),
            Some("SimCurveTrainApp · TEST DATA".to_string())
        );
    }
}
