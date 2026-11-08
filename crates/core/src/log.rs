//! Append-only application log (`companion.log`). Every failure is swallowed:
//! logging must never take the application down.

use std::io::Write;
use std::path::{Path, PathBuf};

pub struct Logger {
    path: PathBuf,
}

impl Logger {
    pub fn new(path: PathBuf) -> Logger {
        Logger { path }
    }
    pub fn default_location() -> Logger {
        Logger::new(crate::paths::log_file())
    }
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn log(&self, message: &str) {
        let _ = self.try_log(message);
    }
    fn try_log(&self, message: &str) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(file, "{} {}", now_iso(), message)
    }
}

/// Current local time in RFC 3339 with offset, falling back to UTC.
pub fn now_iso() -> String {
    use time::format_description::well_known::Rfc3339;
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    now.format(&Rfc3339)
        .unwrap_or_else(|_| "unknown-time".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_timestamped_lines_and_creates_parent() {
        let dir = tempfile::tempdir().unwrap();
        let logger = Logger::new(dir.path().join("sub").join("companion.log"));
        logger.log("first");
        logger.log("second");
        let text = std::fs::read_to_string(logger.path()).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with(" first") && lines[1].ends_with(" second"));
        assert!(
            lines[0].starts_with("20"),
            "ISO timestamp expected: {}",
            lines[0]
        );
    }
}
