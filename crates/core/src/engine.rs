//! Compatibility pin for Samsung's conversion engine. The worker loads
//! `ConversionWrapper.dll` only when its SHA-256 matches; Samsung updates that
//! change the library need an Any2Odyssey compatibility update.

use sha2::{Digest, Sha256};
use std::path::Path;
use thiserror::Error;

/// SHA-256 of the supported `ConversionWrapper.dll` (Odyssey 3D Hub 1.5.1).
pub const EXPECTED_ENGINE_SHA256: &str =
    "0116b33697ca0192fbe26da96cb9e2592599c0cacfbb1129ae03e445cb3a840b";
/// GPU core-count hint passed to the engine's `Start`. Tested on an RTX 4090.
pub const ENGINE_CORE_HINT: i32 = 16384;
/// How long the worker waits for the engine to report its UI minimized.
pub const ENGINE_READY_TIMEOUT_SECS: u64 = 30;
/// Marker Samsung's `Conversion.log` prints once rendering is live.
pub const ENGINE_READY_MARKER: &str = "First time MinimizeUI";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EngineError {
    #[error("Samsung's conversion engine is missing.")]
    Missing,
    #[error("Samsung's conversion engine could not be read: {0}")]
    Unreadable(String),
    #[error("Samsung's engine changed. This build needs a compatibility update.")]
    HashMismatch { actual: String },
    #[error("The companion worker is not installed.")]
    WorkerMissing,
    #[error("Set Odyssey as the primary display at 3840 x 2160.")]
    WrongResolution,
    #[error("Reconnect the Odyssey USB cable; its 3D camera is unavailable.")]
    CameraUnavailable,
    #[error("Could not check the camera: {0}")]
    CameraCheckFailed(String),
    #[error("Stop the conversion running in Samsung's player, then try again.")]
    SamsungPlayerRunning,
}

pub fn sha256_hex(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Check that the engine library exists and is the supported build.
pub fn verify_engine_file(dll: &Path) -> Result<(), EngineError> {
    if !dll.is_file() {
        return Err(EngineError::Missing);
    }
    let actual = sha256_hex(dll).map_err(|e| EngineError::Unreadable(e.to_string()))?;
    if actual != EXPECTED_ENGINE_SHA256 {
        return Err(EngineError::HashMismatch { actual });
    }
    Ok(())
}

/// Required primary display size.
pub const REQUIRED_WIDTH: i32 = 3840;
pub const REQUIRED_HEIGHT: i32 = 2160;

pub fn check_resolution(width: i32, height: i32) -> Result<(), EngineError> {
    if width == REQUIRED_WIDTH && height == REQUIRED_HEIGHT {
        Ok(())
    } else {
        Err(EngineError::WrongResolution)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_pin_is_lowercase_sha256() {
        assert_eq!(EXPECTED_ENGINE_SHA256.len(), 64);
        assert!(EXPECTED_ENGINE_SHA256
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    }

    #[test]
    fn verify_reports_missing_and_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let dll = dir.path().join("ConversionWrapper.dll");
        assert_eq!(verify_engine_file(&dll), Err(EngineError::Missing));
        std::fs::write(&dll, b"not samsung").unwrap();
        match verify_engine_file(&dll) {
            Err(EngineError::HashMismatch { actual }) => {
                assert_eq!(actual, sha256_hex(&dll).unwrap());
                assert_eq!(actual.len(), 64);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn sha256_of_known_input() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("abc");
        std::fs::write(&f, b"abc").unwrap();
        assert_eq!(
            sha256_hex(&f).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn resolution_check() {
        assert_eq!(check_resolution(3840, 2160), Ok(()));
        assert_eq!(
            check_resolution(2560, 1440),
            Err(EngineError::WrongResolution)
        );
        assert_eq!(
            EngineError::WrongResolution.to_string(),
            "Set Odyssey as the primary display at 3840 x 2160."
        );
    }
}
