//! Where Any2Odyssey keeps its own files, and where Samsung keeps the engine.
//!
//! This is a clean break from Any2Odd: a new data directory, worker name and
//! IPC names, so both programs can coexist on one machine without touching
//! each other's state.

use std::path::PathBuf;

/// Environment variable that overrides the data directory (used by tests and
/// by the installer's isolated validation).
pub const DATA_DIR_ENV: &str = "ANY2ODYSSEY_DATA_DIR";

/// Per-user data directory: `%LOCALAPPDATA%\Any2Odyssey` on Windows,
/// `$XDG_DATA_HOME/any2odyssey` (or the platform equivalent) elsewhere.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os(DATA_DIR_ENV) {
        return PathBuf::from(dir);
    }
    let base = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir);
    if cfg!(windows) {
        base.join("Any2Odyssey")
    } else {
        base.join("any2odyssey")
    }
}

pub fn settings_file() -> PathBuf {
    data_dir().join("settings.json")
}
pub fn ui_settings_file() -> PathBuf {
    data_dir().join("ui-settings.json")
}
pub fn log_file() -> PathBuf {
    data_dir().join("companion.log")
}
pub fn worker_status_file() -> PathBuf {
    data_dir().join("worker-status.json")
}
pub fn desktop_capture_status_file() -> PathBuf {
    data_dir().join("desktop-capture-status.json")
}
pub fn capture_border_status_file() -> PathBuf {
    data_dir().join("capture-border-status.json")
}
pub fn window_diagnostics_file() -> PathBuf {
    data_dir().join("window-diagnostics.json")
}
pub fn projection_diagnostics_file() -> PathBuf {
    data_dir().join("projection-diagnostics.json")
}
pub fn overlay_backup_file() -> PathBuf {
    data_dir().join("Samsung-OverlaySettings-before.json")
}

/// Samsung's conversion engine folder. Only meaningful on Windows.
pub const ENGINE_DIR: &str = r"C:\Program Files\Odyssey 3D Hub\Conversion";
/// Name of the worker copy installed beside Samsung's engine.
pub const WORKER_EXE: &str = "Any2OdysseyWorker.exe";
/// Samsung's engine library that is loaded by the worker.
pub const ENGINE_DLL: &str = "ConversionWrapper.dll";
/// Fixed DLL search folders explicitly registered by the worker.
pub const ENGINE_DLL_DIRS: &[&str] = &[
    r"C:\Program Files\Odyssey 3D Hub\Conversion",
    r"C:\Program Files\Odyssey 3D Hub\Conversion\CUDA",
    r"C:\Program Files\Odyssey 3D Hub\Conversion\ONNX",
    r"C:\Program Files\LeiaSR\Platform\bin",
    r"C:\Program Files\Odyssey 3D Hub",
];
/// Samsung's separate player process that must not run during conversion.
pub const SAMSUNG_PLAYER_PROCESS: &str = "Odyssey3DPlayer";
/// The monitor's stereo camera as Windows names it.
pub const STEREO_CAMERA_NAME: &str = "3D Stereo WebCam";

pub fn engine_dir() -> PathBuf {
    PathBuf::from(ENGINE_DIR)
}
pub fn engine_dll() -> PathBuf {
    engine_dir().join(ENGINE_DLL)
}
pub fn worker_exe() -> PathBuf {
    engine_dir().join(WORKER_EXE)
}

/// `%USERPROFILE%\AppData\LocalLow\Samsung\Odyssey3DHubUI\...` files.
fn samsung_locallow() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(std::env::temp_dir);
    home.join("AppData")
        .join("LocalLow")
        .join("Samsung")
        .join("Odyssey3DHubUI")
}
pub fn samsung_overlay_settings_file() -> PathBuf {
    samsung_locallow()
        .join("SettingsData")
        .join("OverlaySettings.json")
}
pub fn samsung_conversion_log_file() -> PathBuf {
    samsung_locallow().join("Conversion.log")
}

/// Any2Odd 0.5.0–0.7.3 settings file, read once to import preferences.
pub fn legacy_any2odd_settings_file() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    dirs::data_local_dir().map(|d| d.join("Odyssey3DCompanion").join("settings.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dir_honours_override() {
        // Serialize env mutation inside this test only.
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(DATA_DIR_ENV, dir.path());
        assert_eq!(data_dir(), dir.path());
        assert_eq!(settings_file(), dir.path().join("settings.json"));
        std::env::remove_var(DATA_DIR_ENV);
        assert!(data_dir().ends_with(if cfg!(windows) {
            "Any2Odyssey"
        } else {
            "any2odyssey"
        }));
    }
}
