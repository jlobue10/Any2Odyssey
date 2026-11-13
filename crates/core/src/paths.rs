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
    #[cfg(unix)]
    {
        match dirs::data_local_dir() {
            Some(base) => base.join("any2odyssey"),
            None => homeless_data_dir(),
        }
    }
    #[cfg(not(unix))]
    {
        let base = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir);
        if cfg!(windows) {
            base.join("Any2Odyssey")
        } else {
            base.join("any2odyssey")
        }
    }
}

/// Where the data goes on Unix when no home folder can be found (no `$HOME`
/// and no entry in the user database). Helpers are run from the data folder
/// and the depth model, the capture token and the instance socket live
/// there, so it is never a name in the shared temporary folder, which
/// another user could have made first: it is a folder of our own in the
/// session's runtime folder, or nowhere at all.
#[cfg(unix)]
fn homeless_data_dir() -> PathBuf {
    // Worked out once: the answer does not change while the program runs,
    // and finding it makes a folder and looks at who owns it.
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        private_runtime_dir(std::env::var_os("XDG_RUNTIME_DIR")).unwrap_or_else(|| {
            // Nothing can be made under a file. Whoever reads or writes
            // there gets an error that names this path, which every caller
            // already handles (a data folder that cannot be made).
            PathBuf::from("/dev/null/any2odyssey-has-no-home-or-runtime-folder")
        })
    })
    .clone()
}

/// `any2odyssey` in the runtime folder `runtime` (`$XDG_RUNTIME_DIR`), made
/// for this user only if it is not there. `None` unless both folders belong
/// to this user and nobody else can write to them.
#[cfg(unix)]
fn private_runtime_dir(runtime: Option<std::ffi::OsString>) -> Option<PathBuf> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    let runtime = PathBuf::from(runtime.filter(|dir| !dir.is_empty())?);
    if !runtime.is_absolute() {
        return None;
    }
    let dir = runtime.join("any2odyssey");
    match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return None,
    }
    // A file made here belongs to this user, which is how the user's id is
    // learnt without calling into the C library. It has no name and is gone
    // when this function returns.
    let me = tempfile::tempfile_in(&dir).ok()?.metadata().ok()?.uid();
    let private = |metadata: std::fs::Metadata| {
        metadata.is_dir() && metadata.uid() == me && metadata.mode() & 0o022 == 0
    };
    // The folder itself must be a real folder, not a link put in its place.
    let ours = std::fs::metadata(&runtime).is_ok_and(private)
        && std::fs::symlink_metadata(&dir).is_ok_and(private);
    ours.then_some(dir)
}

#[cfg(all(test, unix))]
mod homeless_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &std::path::Path) -> u32 {
        std::fs::symlink_metadata(path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    }

    fn set_mode(path: &std::path::Path, mode: u32) {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    #[test]
    fn without_a_home_the_data_goes_to_a_private_folder_in_the_runtime_folder() {
        let runtime = tempfile::tempdir().unwrap();
        set_mode(runtime.path(), 0o700);
        let dir = private_runtime_dir(Some(runtime.path().into())).unwrap();
        assert_eq!(dir, runtime.path().join("any2odyssey"));
        assert_eq!(mode(&dir), 0o700, "made for this user only");
        // The second time it is found as it was left.
        std::fs::write(dir.join("settings.json"), b"{}").unwrap();
        assert_eq!(
            private_runtime_dir(Some(runtime.path().into())),
            Some(dir.clone())
        );
        assert!(dir.join("settings.json").exists());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "no probe left");
    }

    #[test]
    fn a_runtime_folder_that_is_missing_relative_or_shared_is_refused() {
        assert_eq!(private_runtime_dir(None), None);
        assert_eq!(private_runtime_dir(Some("".into())), None);
        assert_eq!(private_runtime_dir(Some("run/user/1000".into())), None);
        let parent = tempfile::tempdir().unwrap();
        assert_eq!(
            private_runtime_dir(Some(parent.path().join("missing").into())),
            None
        );

        // Our folder, but others may write to it.
        let runtime = tempfile::tempdir().unwrap();
        set_mode(runtime.path(), 0o700);
        let dir = runtime.path().join("any2odyssey");
        std::fs::create_dir(&dir).unwrap();
        for open in [0o777, 0o770, 0o702] {
            set_mode(&dir, open);
            assert_eq!(private_runtime_dir(Some(runtime.path().into())), None);
        }
        set_mode(&dir, 0o700);
        assert!(private_runtime_dir(Some(runtime.path().into())).is_some());
        // A runtime folder others may write to could have our folder
        // exchanged under us.
        set_mode(runtime.path(), 0o777);
        assert_eq!(private_runtime_dir(Some(runtime.path().into())), None);
        set_mode(runtime.path(), 0o700);

        // A link where the folder should be, even to a folder of ours.
        std::fs::remove_dir(&dir).unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        set_mode(elsewhere.path(), 0o700);
        std::os::unix::fs::symlink(elsewhere.path(), &dir).unwrap();
        assert_eq!(private_runtime_dir(Some(runtime.path().into())), None);
        // And a file.
        std::fs::remove_file(&dir).unwrap();
        std::fs::write(&dir, b"not a folder").unwrap();
        assert_eq!(private_runtime_dir(Some(runtime.path().into())), None);
    }

    #[test]
    fn the_place_of_last_resort_cannot_be_made_by_anyone() {
        let nowhere = PathBuf::from("/dev/null/any2odyssey-has-no-home-or-runtime-folder");
        assert!(std::fs::create_dir_all(&nowhere).is_err());
        assert!(std::fs::write(nowhere.join("settings.json"), b"{}").is_err());
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
/// The desktop portal's token for the screen last chosen for live capture
/// (Linux), so its dialog is not shown at every start.
pub fn capture_token_file() -> PathBuf {
    data_dir().join("capture-token")
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

    #[test]
    fn every_data_file_has_its_own_name_in_the_data_folder() {
        let dir = data_dir();
        assert!(dir.is_absolute(), "{dir:?}");
        let files = [
            (settings_file(), "settings.json"),
            (ui_settings_file(), "ui-settings.json"),
            (lens_file(), "lens.json"),
            (capture_token_file(), "capture-token"),
            (log_file(), "companion.log"),
            (worker_status_file(), "worker-status.json"),
            (desktop_capture_status_file(), "desktop-capture-status.json"),
            (capture_border_status_file(), "capture-border-status.json"),
            (window_diagnostics_file(), "window-diagnostics.json"),
            (projection_diagnostics_file(), "projection-diagnostics.json"),
            (overlay_backup_file(), "Samsung-OverlaySettings-before.json"),
        ];
        for (path, name) in files {
            assert_eq!(path, dir.join(name));
        }
    }

    #[test]
    fn engine_and_samsung_files_are_where_samsung_keeps_them() {
        let engine = std::path::Path::new(ENGINE_DIR);
        assert_eq!(engine_dir(), engine);
        assert_eq!(engine_dll(), engine.join("ConversionWrapper.dll"));
        assert_eq!(worker_exe(), engine.join("Any2OdysseyWorker.exe"));
        let hub: PathBuf = ["AppData", "LocalLow", "Samsung", "Odyssey3DHubUI"]
            .iter()
            .collect();
        let overlay = samsung_overlay_settings_file();
        assert!(overlay.is_absolute(), "{overlay:?}");
        assert!(
            overlay.ends_with(hub.join("SettingsData").join("OverlaySettings.json")),
            "{overlay:?}"
        );
        let log = samsung_conversion_log_file();
        assert!(log.is_absolute(), "{log:?}");
        assert!(log.ends_with(hub.join("Conversion.log")), "{log:?}");
        // Any2Odd only ever ran on Windows: nothing is imported elsewhere.
        let legacy = legacy_any2odd_settings_file();
        assert_eq!(legacy.is_some(), cfg!(windows), "{legacy:?}");
        if let Some(legacy) = legacy {
            assert!(
                legacy.ends_with(std::path::Path::new("Odyssey3DCompanion").join("settings.json")),
                "{legacy:?}"
            );
        }
    }
}
