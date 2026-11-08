//! JSON status files the worker writes and the controls read. Field names stay
//! lower-case like Any2Odd's so log readers and tests can compare them.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerStatus {
    pub state: String,
    pub time: String,
    pub pid: u32,
}

impl WorkerStatus {
    pub fn now(state: impl Into<String>, pid: u32) -> WorkerStatus {
        WorkerStatus {
            state: state.into(),
            time: crate::log::now_iso(),
            pid,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DesktopCaptureStatus {
    pub pid: u32,
    pub time: String,
    pub backend: String,
    pub frames: u64,
    pub fps: f64,
    #[serde(rename = "averageCaptureMilliseconds")]
    pub average_capture_milliseconds: f64,
    pub bounds: crate::geometry::Rect,
    #[serde(rename = "sourceBounds")]
    pub source_bounds: crate::geometry::Rect,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureBorderStatus {
    pub pid: u32,
    pub time: String,
    pub access: String,
    #[serde(rename = "borderRequired")]
    pub border_required: bool,
    pub error: Option<String>,
}

/// Worker exit codes, shared with the supervisor's messages.
pub mod exit_code {
    pub const READY_AND_STOPPED_CLEANLY: i32 = 0;
    pub const RENDERER_ERROR: i32 = 1;
    pub const ALREADY_RUNNING: i32 = 2;
    pub const PREREQUISITES_FAILED: i32 = 3;
    pub const NEVER_BECAME_READY: i32 = 4;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_status_json_shape() {
        let s = WorkerStatus {
            state: "Starting conversion".into(),
            time: "t".into(),
            pid: 7,
        };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"state":"Starting conversion","time":"t","pid":7})
        );
        let back: WorkerStatus = serde_json::from_value(json).unwrap();
        assert_eq!(back, s);
    }
}
