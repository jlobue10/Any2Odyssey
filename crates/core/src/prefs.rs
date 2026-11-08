//! Conversion preferences (`settings.json`) and desktop UI settings
//! (`ui-settings.json`). Keys keep Any2Odd's PascalCase names so a one-time
//! import of an existing Any2Odd `settings.json` is a plain load.

use crate::fsutil::write_json_atomic;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

/// What is captured and fed to the converter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
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
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e),
        };
        let mut prefs: Self = serde_json::from_str(&text)
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
        write_json_atomic(path, self)
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
    pub fn load_from(path: &Path) -> UiSettings {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }
    pub fn load() -> UiSettings {
        Self::load_from(&crate::paths::ui_settings_file())
    }
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        write_json_atomic(path, self)
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
        assert!(!UiSettings::load_from(&path).dark_mode);
        UiSettings { dark_mode: true }.save_to(&path).unwrap();
        assert!(UiSettings::load_from(&path).dark_mode);
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("\"DarkMode\": true"));
    }
}
