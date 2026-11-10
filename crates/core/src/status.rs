//! JSON status files the worker writes and the controls read. Field names stay
//! lower-case like Any2Odd's so log readers and tests can compare them.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerStatus {
    pub state: String,
    pub time: String,
    pub pid: u32,
    /// The engine's output is live. Written only when true, so every other
    /// status keeps the three fields it always had, and a file from a worker
    /// that predates the field reads as not ready. The controls take this,
    /// not the wording of `state`, as the sign that a start or a recovery
    /// attempt succeeded.
    #[serde(default, skip_serializing_if = "is_false")]
    pub ready: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl WorkerStatus {
    pub fn now(state: impl Into<String>, pid: u32) -> WorkerStatus {
        WorkerStatus {
            state: state.into(),
            time: crate::log::now_iso(),
            pid,
            ready: false,
        }
    }

    /// [`WorkerStatus::now`] for the state that announces live output.
    pub fn ready_now(state: impl Into<String>, pid: u32) -> WorkerStatus {
        WorkerStatus {
            ready: true,
            ..WorkerStatus::now(state, pid)
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

/// Worker exit codes. `supervisor::classify` maps them to what the controls
/// say and whether recovery may retry: `ALREADY_RUNNING` and
/// `PREREQUISITES_FAILED` turn conversion off until the user acts, the others
/// are retried.
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
            ready: false,
        };
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"state":"Starting conversion","time":"t","pid":7})
        );
        let back: WorkerStatus = serde_json::from_value(json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn ready_is_written_only_when_true_and_defaults_to_false() {
        let ready = WorkerStatus::ready_now("3D active", 7);
        assert!(ready.ready && ready.pid == 7 && ready.state == "3D active");
        let json = serde_json::to_value(&ready).unwrap();
        assert_eq!(json["ready"], true);
        assert_eq!(serde_json::from_value::<WorkerStatus>(json).unwrap(), ready);
        assert!(!WorkerStatus::now("Starting conversion", 7).ready);
        // A status file written by a worker from before the field existed.
        let old: WorkerStatus =
            serde_json::from_str(r#"{"state":"3D active","time":"t","pid":7}"#).unwrap();
        assert!(!old.ready);
    }
}
