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
    data_dir_from(std::env::var_os(DATA_DIR_ENV))
}

/// [`data_dir`] for an explicit override value, so the rule is testable
/// without mutating the process environment under parallel tests.
fn data_dir_from(override_dir: Option<std::ffi::OsString>) -> PathBuf {
    // The worker runs with the engine folder as its working directory, so a
    // relative override would split the controls' and the worker's files;
    // resolve it here, once, against this process's directory. An empty or
    // blank value means unset rather than "the current directory", and
    // surrounding whitespace is an artefact of how the variable was set, not
    // part of the folder name. A value that is not UTF-8 is used as given.
    let override_dir = override_dir.and_then(|dir| match dir.to_str() {
        Some(text) => {
            let text = text.trim();
            (!text.is_empty()).then(|| PathBuf::from(text))
        }
        None => Some(PathBuf::from(dir)),
    });
    if let Some(dir) = override_dir {
        return std::path::absolute(&dir).unwrap_or(dir);
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
/// The user's own lens numbers for interleaved output (`weave::Lens`).
pub fn lens_file() -> PathBuf {
    data_dir().join("lens.json")
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
        let dir = tempfile::tempdir().unwrap();
        let chosen = data_dir_from(Some(dir.path().as_os_str().to_owned()));
        assert_eq!(chosen, dir.path());
        assert_eq!(
            chosen.join("settings.json"),
            dir.path().join("settings.json")
        );
        assert!(data_dir_from(None).ends_with(if cfg!(windows) {
            "Any2Odyssey"
        } else {
            "any2odyssey"
        }));
    }

    #[test]
    fn relative_and_empty_overrides_do_not_depend_on_the_working_directory() {
        let relative = data_dir_from(Some("data".into()));
        assert!(relative.is_absolute(), "{relative:?}");
        assert!(relative.ends_with("data"));
        assert_eq!(data_dir_from(Some("".into())), data_dir_from(None));
        assert_eq!(data_dir_from(Some("  \t ".into())), data_dir_from(None));
        let padded = data_dir_from(Some("  data \n".into()));
        assert_eq!(padded, relative, "surrounding whitespace is not a name");
    }
}
