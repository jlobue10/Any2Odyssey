//! Conversion preferences (`settings.json`) and desktop UI settings
//! (`ui-settings.json`). Keys keep Any2Odd's PascalCase names so a one-time
//! import of an existing Any2Odd `settings.json` is a plain load.

use crate::fsutil::{read_to_string_limited, write_json_atomic_durable, SMALL_FILE_LIMIT};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

/// What is captured and fed to the converter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Default)]
pub enum CaptureMode {
    /// The whole primary desktop, including wallpaper, icons and menus.
    #[default]
    Desktop,
    /// One selected window, fitted to the output.
    Window,
    /// Whichever eligible window is in the foreground.
    Active,
}

impl CaptureMode {
    pub const ALL: [CaptureMode; 3] = [
        CaptureMode::Desktop,
        CaptureMode::Window,
        CaptureMode::Active,
    ];
    pub fn label(self) -> &'static str {
        match self {
            CaptureMode::Desktop => "Whole desktop — constant 3D",
            CaptureMode::Window => "Selected window",
            CaptureMode::Active => "Follow active window",
        }
    }
    pub fn index(self) -> usize {
        match self {
            CaptureMode::Desktop => 0,
            CaptureMode::Window => 1,
            CaptureMode::Active => 2,
        }
    }
    pub fn from_index(i: usize) -> CaptureMode {
        Self::ALL.get(i).copied().unwrap_or_default()
    }
}

/// Serialized as the variant name; read back from the name or from Any2Odd's
/// integer form. Any2Odd is C#, and both Json.NET and System.Text.Json write
/// enums as their integer value unless told otherwise (`"Mode": 2`); the 0.8.0
/// hardware import succeeded with a file carrying `"Mode": "Active"`, so the
/// name form exists in the wild too. Both must load; we always save the name.
impl<'de> Deserialize<'de> for CaptureMode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{self, Unexpected, Visitor};
        struct ModeVisitor;
        impl Visitor<'_> for ModeVisitor {
            type Value = CaptureMode;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("\"Desktop\", \"Window\", \"Active\" or the index 0, 1 or 2")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<CaptureMode, E> {
                match v {
                    "Desktop" => Ok(CaptureMode::Desktop),
                    "Window" => Ok(CaptureMode::Window),
                    "Active" => Ok(CaptureMode::Active),
                    other => Err(E::unknown_variant(other, &["Desktop", "Window", "Active"])),
                }
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<CaptureMode, E> {
                usize::try_from(v)
                    .ok()
                    .and_then(|i| CaptureMode::ALL.get(i).copied())
                    .ok_or_else(|| E::invalid_value(Unexpected::Unsigned(v), &self))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<CaptureMode, E> {
                match u64::try_from(v) {
                    Ok(v) => self.visit_u64(v),
                    Err(_) => Err(E::invalid_value(Unexpected::Signed(v), &self)),
                }
            }
        }
        deserializer.deserialize_any(ModeVisitor)
    }
}

pub const DEPTH_MIN: f64 = 0.0;
pub const DEPTH_MAX: f64 = 2.0;
pub const DEPTH_DEFAULT: f64 = 0.9;
pub const POPOUT_MIN: f64 = -1.0;
pub const POPOUT_MAX: f64 = 1.0;
pub const POPOUT_DEFAULT: f64 = 0.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct Preferences {
    pub mode: CaptureMode,
    pub side_by_side: bool,
    pub auto_convert: bool,
    pub minimize_to_tray: bool,
    pub recover: bool,
    pub keep_overlay_on_top: bool,
    /// Ask GitHub for the newest release once a day when the controls open.
    pub check_updates: bool,
    pub depth: f64,
    pub popout: f64,
}

impl Default for Preferences {
    fn default() -> Self {
        Preferences {
            mode: CaptureMode::Desktop,
            side_by_side: false,
            auto_convert: true,
            minimize_to_tray: true,
            recover: true,
            keep_overlay_on_top: true,
            check_updates: true,
            depth: DEPTH_DEFAULT,
            popout: POPOUT_DEFAULT,
        }
    }
}

impl Preferences {
    /// Only a missing file uses first-run defaults. Existing settings that
    /// cannot be read or parsed must not silently enable desktop capture.
    pub fn load_from(path: &Path) -> io::Result<Preferences> {
        let text = match read_to_string_limited(path, SMALL_FILE_LIMIT) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e),
        };
        // A file hand-edited in Notepad ("UTF-8 with BOM") or written by
        // Windows PowerShell must not count as damaged.
        let mut prefs: Self = serde_json::from_str(crate::fsutil::strip_utf8_bom_str(&text))
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        prefs.sanitize();
        Ok(prefs)
    }

    pub fn load() -> io::Result<Preferences> {
        Self::load_from(&crate::paths::settings_file())
    }

    /// Clamp numeric ranges. Non-finite values fall back to the defaults.
    pub fn sanitize(&mut self) {
        if !self.depth.is_finite() {
            self.depth = DEPTH_DEFAULT;
        }
        if !self.popout.is_finite() {
            self.popout = POPOUT_DEFAULT;
        }
        self.depth = self.depth.clamp(DEPTH_MIN, DEPTH_MAX);
        self.popout = self.popout.clamp(POPOUT_MIN, POPOUT_MAX);
    }

    pub fn save_to(&mut self, path: &Path) -> std::io::Result<()> {
        self.sanitize();
        write_json_atomic_durable(path, self)
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        self.save_to(&crate::paths::settings_file())
    }

    /// Explicit recovery after a load error: preserve the existing bytes in a
    /// unique backup before replacing the file with settings chosen in the UI.
    pub fn save_recovering_to(&mut self, path: &Path) -> io::Result<Option<PathBuf>> {
        let backup = match std::fs::File::open(path) {
            Ok(mut original) => {
                let parent = path
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));
                let mut backup = tempfile::Builder::new()
                    .prefix("settings-recovery-")
                    .suffix(".json")
                    .tempfile_in(parent)?;
                std::io::copy(&mut original, &mut backup)?;
                backup.as_file().sync_all()?;
                Some(backup.keep().map_err(|e| e.error)?.1)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        self.save_to(path)?;
        Ok(backup)
    }
}

/// Desktop-only presentation settings, saved separately from conversion.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "PascalCase", default)]
pub struct UiSettings {
    pub dark_mode: bool,
    /// Unix seconds of the last completed update check.
    pub last_update_check: Option<i64>,
}

impl UiSettings {
    /// Presentation settings fall back to defaults when the file is damaged,
    /// unlike `settings.json`; the second value says why, so the launch can
    /// log it before the next save overwrites the file.
    pub fn load_from(path: &Path) -> (UiSettings, Option<String>) {
        let text = match read_to_string_limited(path, SMALL_FILE_LIMIT) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (Self::default(), None),
            Err(e) => return (Self::default(), Some(e.to_string())),
        };
        match serde_json::from_str(crate::fsutil::strip_utf8_bom_str(&text)) {
            Ok(settings) => (settings, None),
            Err(e) => (Self::default(), Some(e.to_string())),
        }
    }
    pub fn load() -> (UiSettings, Option<String>) {
        Self::load_from(&crate::paths::ui_settings_file())
    }
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        write_json_atomic_durable(path, self)
    }
    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&crate::paths::ui_settings_file())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_upstream() {
        let p = Preferences::default();
        assert_eq!(p.mode, CaptureMode::Desktop);
        assert!(
            !p.side_by_side
                && p.auto_convert
                && p.minimize_to_tray
                && p.recover
                && p.keep_overlay_on_top
        );
        assert_eq!(p.depth, 0.9);
        assert_eq!(p.popout, 0.0);
    }

    #[test]
    fn reads_any2odd_settings_json_verbatim() {
        let text = r#"{"Mode":"Active","SideBySide":true,"AutoConvert":false,"MinimizeToTray":false,"Recover":false,"KeepOverlayOnTop":false,"Depth":1.5,"Popout":-0.25}"#;
        let p: Preferences = serde_json::from_str(text).unwrap();
        assert_eq!(p.mode, CaptureMode::Active);
        assert!(
            p.side_by_side
                && !p.auto_convert
                && !p.minimize_to_tray
                && !p.recover
                && !p.keep_overlay_on_top
        );
        assert_eq!((p.depth, p.popout), (1.5, -0.25));
    }

    #[test]
    fn any2odd_integer_modes_are_accepted_and_saved_as_names() {
        // Provenance: Any2Odd is C#, and Json.NET and System.Text.Json both
        // serialize enums as integers by default, so an Any2Odd settings.json
        // may carry "Mode": 2. The 0.8.0 hardware import succeeded with the
        // string form ("Mode": "Active"), so both shapes must load.
        for (json, mode) in [
            (r#"{"Mode":0}"#, CaptureMode::Desktop),
            (r#"{"Mode":1}"#, CaptureMode::Window),
            (r#"{"Mode":2}"#, CaptureMode::Active),
            (r#"{"Mode":"Window"}"#, CaptureMode::Window),
        ] {
            let p: Preferences = serde_json::from_str(json).unwrap();
            assert_eq!(p.mode, mode, "{json}");
        }
        for json in [
            r#"{"Mode":3}"#,
            r#"{"Mode":-1}"#,
            r#"{"Mode":1.5}"#,
            r#"{"Mode":"2"}"#,
            r#"{"Mode":null}"#,
            r#"{"Mode":true}"#,
            r#"{"Mode":18446744073709551615}"#,
        ] {
            assert!(serde_json::from_str::<Preferences>(json).is_err(), "{json}");
        }
        // Our own file always carries the name, whatever form was imported.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut imported: Preferences = serde_json::from_str(r#"{"Mode":2}"#).unwrap();
        imported.save_to(&path).unwrap();
        let json = std::fs::read_to_string(&path).unwrap();
        assert!(json.contains("\"Mode\": \"Active\""), "{json}");
        assert_eq!(
            Preferences::load_from(&path).unwrap().mode,
            CaptureMode::Active
        );
    }

    #[test]
    fn invalid_settings_are_errors_and_valid_values_are_clamped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"Mode":"Sideways","Depth":9,"Popout":-7}"#).unwrap();
        assert!(Preferences::load_from(&path).is_err());
        std::fs::write(&path, r#"{"Depth":9,"Popout":-7}"#).unwrap();
        let p = Preferences::load_from(&path).unwrap();
        assert_eq!((p.depth, p.popout), (2.0, -1.0));
        std::fs::write(&path, "not json").unwrap();
        assert!(Preferences::load_from(&path).is_err());
        std::fs::write(&path, "\u{FEFF}{\"Mode\":\"Window\"}").unwrap();
        assert_eq!(
            Preferences::load_from(&path).unwrap().mode,
            CaptureMode::Window,
            "a byte-order mark is not damage"
        );
        assert_eq!(
            Preferences::load_from(&dir.path().join("missing.json")).unwrap(),
            Preferences::default()
        );
        assert!(Preferences::load_from(dir.path()).is_err());
    }

    #[test]
    fn save_round_trips_and_clamps() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut p = Preferences {
            depth: 3.0,
            popout: f64::NAN,
            ..Default::default()
        };
        p.save_to(&path).unwrap();
        let back = Preferences::load_from(&path).unwrap();
        assert_eq!(back.depth, 2.0);
        assert_eq!(back.popout, 0.0);
        let json = std::fs::read_to_string(&path).unwrap();
        assert!(json.contains("\"Mode\": \"Desktop\""));
    }

    #[test]
    fn explicit_recovery_preserves_the_original_and_uses_unique_backups() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut prefs = Preferences {
            auto_convert: false,
            ..Default::default()
        };
        std::fs::write(&path, b"broken settings\xff").unwrap();
        let first = prefs.save_recovering_to(&path).unwrap().unwrap();
        assert_eq!(std::fs::read(&first).unwrap(), b"broken settings\xff");
        assert!(!Preferences::load_from(&path).unwrap().auto_convert);
        std::fs::write(&path, "another damaged file").unwrap();
        let second = prefs.save_recovering_to(&path).unwrap().unwrap();
        assert_ne!(first, second);
        assert_eq!(std::fs::read(&first).unwrap(), b"broken settings\xff");
        assert_eq!(
            std::fs::read_to_string(second).unwrap(),
            "another damaged file"
        );
        assert!(prefs.save_recovering_to(dir.path()).is_err());
        assert!(dir.path().is_dir());
    }

    #[test]
    fn ui_settings_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui-settings.json");
        let (missing, problem) = UiSettings::load_from(&path);
        assert!(!missing.dark_mode);
        assert_eq!(
            problem, None,
            "a missing file is the first launch, not a problem"
        );
        UiSettings {
            dark_mode: true,
            last_update_check: Some(1_800_000_000),
        }
        .save_to(&path)
        .unwrap();
        let (back, problem) = UiSettings::load_from(&path);
        assert!(back.dark_mode);
        assert_eq!(back.last_update_check, Some(1_800_000_000));
        assert_eq!(problem, None);
        let json = std::fs::read_to_string(&path).unwrap();
        assert!(json.contains("\"DarkMode\": true"));
        assert!(json.contains("\"LastUpdateCheck\": 1800000000"));
        // Files from earlier versions have no check time.
        std::fs::write(&path, "{\"DarkMode\": false}").unwrap();
        assert_eq!(UiSettings::load_from(&path).0.last_update_check, None);
        std::fs::write(&path, "\u{FEFF}{\"DarkMode\": true}").unwrap();
        assert!(UiSettings::load_from(&path).0.dark_mode);
    }

    #[test]
    fn damaged_ui_settings_reset_to_defaults_and_say_why() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui-settings.json");
        std::fs::write(&path, "{\"DarkMode\": tru").unwrap();
        let (settings, problem) = UiSettings::load_from(&path);
        assert_eq!(settings, UiSettings::default());
        assert!(problem.is_some());
        // So does a file that cannot be read at all (here, a folder).
        let (settings, problem) = UiSettings::load_from(dir.path());
        assert_eq!(settings, UiSettings::default());
        assert!(problem.is_some());
    }

    #[test]
    fn a_mode_and_its_place_in_the_list_name_each_other() {
        for (index, mode) in CaptureMode::ALL.into_iter().enumerate() {
            assert_eq!(mode.index(), index);
            assert_eq!(CaptureMode::from_index(index), mode);
        }
        assert_eq!(CaptureMode::from_index(3), CaptureMode::Desktop);
        assert_eq!(CaptureMode::from_index(usize::MAX), CaptureMode::Desktop);
    }

    #[test]
    fn recovery_backs_up_beside_the_settings_and_needs_no_backup_of_a_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("data");
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("settings.json");
        let mut prefs = Preferences {
            recover: false,
            ..Default::default()
        };
        // Nothing to preserve: saved, and no backup is claimed.
        assert_eq!(prefs.save_recovering_to(&path).unwrap(), None);
        assert!(!Preferences::load_from(&path).unwrap().recover);
        std::fs::write(&path, "damaged").unwrap();
        let backup = prefs.save_recovering_to(&path).unwrap().unwrap();
        assert_eq!(backup.parent(), Some(folder.as_path()));
        let name = backup.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            name.starts_with("settings-recovery-") && name.ends_with(".json"),
            "{name}"
        );
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn recovery_does_not_replace_settings_it_could_not_back_up() {
        // A link to itself cannot be opened, and that is not "missing": the
        // rename that publishes new settings would still replace it.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::os::unix::fs::symlink(&path, &path).unwrap();
        let mut prefs = Preferences::default();
        let error = prefs.save_recovering_to(&path).unwrap_err();
        assert_ne!(error.kind(), io::ErrorKind::NotFound, "{error}");
        assert!(std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
