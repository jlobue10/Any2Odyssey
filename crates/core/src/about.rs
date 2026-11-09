//! Facts for the About page: the running build, where the executable lives
//! and what the installer recorded when it put it there. No UI here, so the
//! rows can be tested on every OS.

use serde::Deserialize;
use std::path::{Path, PathBuf};

pub const REPOSITORY_URL: &str = "https://github.com/jlobue10/Any2Odyssey";
pub const RELEASES_URL: &str = "https://github.com/jlobue10/Any2Odyssey/releases";
pub const CHANGELOG_URL: &str = "https://github.com/jlobue10/Any2Odyssey/blob/main/CHANGELOG.md";
pub const ISSUES_URL: &str = "https://github.com/jlobue10/Any2Odyssey/issues";
/// Written beside the executable by `installer/install.ps1`.
pub const INSTALL_RECORD_FILE: &str = "version.json";
pub const LICENSE: &str = "GPL-3.0-only";
pub const CREDITS: &str = "Any2Odyssey is a Rust reimplementation of Any2Odd 0.7.3 by Zhandragon, \
released under the GNU General Public License version 3.0 only. \
Copyright (C) 2026 Jonathan LoBue. \
The 3D conversion itself is performed by Samsung's Odyssey 3D Hub engine, which is a separate product and not part of this program. \
The interface uses Libre Baskerville (display face, icon and wordmark) and Fira Sans (text), both under the SIL Open Font License 1.1; \
the fonts egui bundles are listed in THIRD-PARTY-LICENSES.md.";

/// What the installer writes. Field names are the installer's PascalCase;
/// fields it does not know are ignored and missing ones stay empty.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "PascalCase", default)]
pub struct InstallRecord {
    pub installed: Option<String>,
    pub version: Option<String>,
    pub previous_version: Option<String>,
    pub action: Option<String>,
    pub app: Option<String>,
    pub controls_only: bool,
    pub worker: Option<String>,
    pub hash: Option<String>,
}

impl InstallRecord {
    /// `Ok(None)` when the file is absent: a portable or development run.
    pub fn load_from(path: &Path) -> Result<Option<InstallRecord>, String> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("could not read {}: {e}", path.display())),
        };
        // Windows PowerShell writes UTF-8 with a byte-order mark.
        let text = text.trim_start_matches('\u{feff}');
        serde_json::from_str(text)
            .map(Some)
            .map_err(|e| format!("{} is not a valid install record: {e}", path.display()))
    }

    /// The installation time as `YYYY-MM-DD HH:MM` when the installer's
    /// round-trip timestamp is recognized, otherwise the text as written.
    pub fn installed_display(&self) -> Option<String> {
        let text = self.installed.as_deref()?.trim();
        if text.is_empty() {
            return None;
        }
        if text.is_ascii() && text.len() >= 16 && text.as_bytes()[10] == b'T' {
            let mut short = text[..16].to_owned();
            short.replace_range(10..11, " ");
            return Some(short);
        }
        Some(text.to_owned())
    }
}

/// How the running executable got where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Installation {
    /// No record beside the executable.
    NotInstalled,
    /// A record exists but cannot be used.
    Unreadable(String),
    /// Label/value rows from the record.
    Installed(Vec<(String, String)>),
}

/// Everything the About page shows, gathered once at startup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AboutInfo {
    pub version: String,
    /// Commit the build script recorded, when it was built from a checkout.
    pub build: Option<String>,
    pub platform: String,
    pub exe_dir: Option<PathBuf>,
    pub install: Result<Option<InstallRecord>, String>,
}

impl AboutInfo {
    pub fn gather(build: Option<&str>) -> AboutInfo {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf));
        let install = match &exe_dir {
            Some(dir) => InstallRecord::load_from(&dir.join(INSTALL_RECORD_FILE)),
            None => Ok(None),
        };
        AboutInfo::assemble(crate::VERSION, build, exe_dir, install)
    }

    pub fn assemble(
        version: &str,
        build: Option<&str>,
        exe_dir: Option<PathBuf>,
        install: Result<Option<InstallRecord>, String>,
    ) -> AboutInfo {
        AboutInfo {
            version: version.to_owned(),
            build: build
                .map(str::trim)
                .filter(|b| !b.is_empty())
                .map(str::to_owned),
            platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            exe_dir,
            install,
        }
    }

    /// Label/value rows about the running build.
    pub fn build_rows(&self) -> Vec<(String, String)> {
        let mut rows = vec![
            ("Version".to_owned(), self.version.clone()),
            (
                "Build".to_owned(),
                self.build
                    .clone()
                    .map(|commit| format!("commit {commit}"))
                    .unwrap_or_else(|| "commit not recorded".to_owned()),
            ),
            ("Platform".to_owned(), self.platform.clone()),
            ("License".to_owned(), LICENSE.to_owned()),
        ];
        if let Some(dir) = &self.exe_dir {
            rows.push(("Location".to_owned(), dir.display().to_string()));
        }
        rows
    }

    pub fn installation(&self) -> Installation {
        let record = match &self.install {
            Err(error) => return Installation::Unreadable(error.clone()),
            Ok(None) => return Installation::NotInstalled,
            Ok(Some(record)) => record,
        };
        let mut rows = Vec::new();
        if let Some(when) = record.installed_display() {
            rows.push(("Installed".to_owned(), when));
        }
        let text = |value: &Option<String>| {
            value
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        };
        if let Some(action) = text(&record.action) {
            rows.push(("Action".to_owned(), action));
        }
        if let Some(version) = text(&record.version) {
            rows.push(("Installed version".to_owned(), version));
        }
        if let Some(previous) = text(&record.previous_version) {
            rows.push(("Replaced".to_owned(), previous));
        }
        rows.push((
            "Package".to_owned(),
            if record.controls_only {
                "Controls only".to_owned()
            } else {
                "Full converter".to_owned()
            },
        ));
        if let Some(worker) = text(&record.worker) {
            rows.push(("Worker".to_owned(), worker));
        }
        Installation::Installed(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shaped like `ConvertTo-Json` output from Windows PowerShell 5.1,
    // byte-order mark included.
    const RECORD: &str = "\u{feff}{\r\n    \"Installed\":  \"2026-11-08T14:10:33.1234567-05:00\",\r\n    \"Version\":  \"0.8.1\",\r\n    \"PreviousVersion\":  \"0.8.0.0\",\r\n    \"Action\":  \"Upgrade\",\r\n    \"App\":  \"C:\\\\Users\\\\J\\\\AppData\\\\Local\\\\Programs\\\\Any2Odyssey\",\r\n    \"ControlsOnly\":  false,\r\n    \"Worker\":  \"C:\\\\Program Files\\\\Odyssey 3D Hub\\\\Conversion\\\\Any2OdysseyWorker.exe\",\r\n    \"Hash\":  \"ABC\",\r\n    \"Future\":  1\r\n}\r\n";

    #[test]
    fn install_record_reads_what_the_installer_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(INSTALL_RECORD_FILE);
        std::fs::write(&path, RECORD).unwrap();
        let record = InstallRecord::load_from(&path).unwrap().unwrap();
        assert_eq!(record.version.as_deref(), Some("0.8.1"));
        assert_eq!(record.previous_version.as_deref(), Some("0.8.0.0"));
        assert_eq!(record.action.as_deref(), Some("Upgrade"));
        assert!(!record.controls_only);
        assert_eq!(
            record.worker.as_deref(),
            Some(r"C:\Program Files\Odyssey 3D Hub\Conversion\Any2OdysseyWorker.exe")
        );
        assert_eq!(
            record.installed_display().as_deref(),
            Some("2026-11-08 14:10")
        );
        assert_eq!(
            InstallRecord::load_from(&dir.path().join("missing.json")),
            Ok(None)
        );
        std::fs::write(&path, "{\"Installed\": [").unwrap();
        assert!(InstallRecord::load_from(&path).is_err());
        assert!(InstallRecord::load_from(dir.path()).is_err());
    }

    #[test]
    fn odd_timestamps_are_shown_as_written() {
        let record = InstallRecord {
            installed: Some(" yesterday ".into()),
            ..Default::default()
        };
        assert_eq!(record.installed_display().as_deref(), Some("yesterday"));
        let record = InstallRecord {
            installed: Some("   ".into()),
            ..Default::default()
        };
        assert_eq!(record.installed_display(), None);
        assert_eq!(InstallRecord::default().installed_display(), None);
    }

    #[test]
    fn about_rows_cover_installed_portable_and_unreadable_runs() {
        let record = InstallRecord {
            installed: Some("2026-11-08T14:10:33.1234567-05:00".into()),
            version: Some("0.8.1".into()),
            previous_version: None,
            action: Some("Install".into()),
            controls_only: true,
            ..Default::default()
        };
        let info = AboutInfo::assemble(
            "0.8.2",
            Some(" abc1234 "),
            Some(PathBuf::from("/opt/any2odyssey")),
            Ok(Some(record)),
        );
        let build = info.build_rows();
        assert_eq!(build[0], ("Version".to_owned(), "0.8.2".to_owned()));
        assert_eq!(build[1].1, "commit abc1234");
        assert_eq!(build[3].1, LICENSE);
        assert_eq!(build[4].0, "Location");
        assert_eq!(
            info.installation(),
            Installation::Installed(vec![
                ("Installed".to_owned(), "2026-11-08 14:10".to_owned()),
                ("Action".to_owned(), "Install".to_owned()),
                ("Installed version".to_owned(), "0.8.1".to_owned()),
                ("Package".to_owned(), "Controls only".to_owned()),
            ])
        );

        let portable = AboutInfo::assemble("0.8.2", None, None, Ok(None));
        assert_eq!(portable.build_rows()[1].1, "commit not recorded");
        assert_eq!(portable.build_rows().len(), 4);
        assert_eq!(portable.installation(), Installation::NotInstalled);

        let broken = AboutInfo::assemble("0.8.2", Some(""), None, Err("bad".into()));
        assert_eq!(broken.build, None);
        assert_eq!(
            broken.installation(),
            Installation::Unreadable("bad".to_owned())
        );
    }

    #[test]
    fn gather_reports_the_package_version_and_names_both_typefaces() {
        let info = AboutInfo::gather(None);
        assert_eq!(info.version, crate::VERSION);
        assert!(info.platform.contains(std::env::consts::OS));
        assert!(CREDITS.contains("Libre Baskerville") && CREDITS.contains("Fira Sans"));
    }
}
