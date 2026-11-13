//! Append-only application log (`companion.log`). Every failure is swallowed:
//! logging must never take the application down.
//!
//! The controls and the worker are separate processes appending to the same
//! file, so each call writes its whole line with a single `write`: an append
//! is atomic per write call, not per line.
//!
//! The file is never truncated. A process that starts while it is larger
//! than [`ROTATE_ABOVE`] renames it to one previous generation
//! (`companion.old.log`, replacing the one before) and the next line starts
//! a new file: see [`Logger::rotate_if_large`].

use std::io::Write;
use std::path::{Path, PathBuf};

pub struct Logger {
    path: PathBuf,
}

/// A log larger than this is moved aside by the next process that starts.
pub const ROTATE_ABOVE: u64 = 4 * 1024 * 1024;

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
    /// Call once when the process starts: a log that has outgrown
    /// [`ROTATE_ABOVE`] becomes the one previous generation.
    ///
    /// Nothing is copied or truncated, so no line can be lost: every call
    /// of [`log`](Self::log) opens the file by name, and a line another
    /// process is writing through a handle it already holds lands in the
    /// renamed file. A rename that fails (on Windows, while a program that
    /// does not share the file for deletion has either file open) leaves
    /// everything as it was and is tried again by the next start.
    pub fn rotate_if_large(&self) {
        self.rotate_above(ROTATE_ABOVE);
    }
    /// Where [`rotate_if_large`](Self::rotate_if_large) moves the log to.
    pub fn previous_path(&self) -> PathBuf {
        self.path.with_extension("old.log")
    }
    fn rotate_above(&self, limit: u64) -> bool {
        let large = std::fs::metadata(&self.path).is_ok_and(|m| m.is_file() && m.len() > limit);
        large && std::fs::rename(&self.path, self.previous_path()).is_ok()
    }
    fn try_log(&self, message: &str) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        // `writeln!` on a `File` issues one write per formatted piece, and
        // another process's line can land between any two of them.
        file.write_all(format_line(&now_iso(), message).as_bytes())
    }
}

/// One log line, newline included. Line breaks inside the message (Windows
/// error texts end with one, a status may quote several lines) become
/// [`LINE_BREAK`], so one call is always exactly one line.
fn format_line(time: &str, message: &str) -> String {
    let mut line = String::with_capacity(time.len() + message.len() + 2);
    line.push_str(time);
    line.push(' ');
    let mut parts = message
        .split(['\n', '\r'])
        .map(str::trim)
        .filter(|part| !part.is_empty());
    if let Some(first) = parts.next() {
        line.push_str(first);
    }
    for part in parts {
        line.push_str(LINE_BREAK);
        line.push_str(part);
    }
    line.push('\n');
    line
}

/// What a line break inside a logged message is written as.
const LINE_BREAK: &str = " | ";

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
        assert!(well_formed(lines[0]), "ISO timestamp expected: {lines:?}");
    }

    #[test]
    fn a_large_log_becomes_the_one_previous_generation() {
        let dir = tempfile::tempdir().unwrap();
        let logger = Logger::new(dir.path().join("companion.log"));
        let previous = dir.path().join("companion.old.log");
        assert_eq!(logger.previous_path(), previous);
        // Nothing to rotate: no file, then a file exactly at the limit.
        assert!(!logger.rotate_above(10));
        std::fs::write(logger.path(), "0123456789").unwrap();
        assert!(!logger.rotate_above(10));
        assert!(!previous.exists());
        // Past the limit it is moved whole, and the next line starts anew.
        logger.log("first generation");
        let first = std::fs::read_to_string(logger.path()).unwrap();
        assert!(logger.rotate_above(10));
        assert!(!logger.path().exists());
        assert_eq!(std::fs::read_to_string(&previous).unwrap(), first);
        logger.log("second generation");
        let second = std::fs::read_to_string(logger.path()).unwrap();
        assert!(second.ends_with(" second generation\n") && !second.contains("first"));
        // Only one previous generation is kept.
        assert!(logger.rotate_above(10));
        assert_eq!(std::fs::read_to_string(&previous).unwrap(), second);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        // The real limit leaves an ordinary log alone.
        logger.log("third");
        logger.rotate_if_large();
        assert!(logger.path().exists());
        // Not a file: left alone.
        let folder = Logger::new(dir.path().join("folder.log"));
        std::fs::create_dir(folder.path()).unwrap();
        assert!(!folder.rotate_above(0));
        assert!(folder.path().is_dir());
    }

    #[test]
    fn a_line_written_through_an_open_handle_survives_the_rotation() {
        // The other process (controls or worker) is in the middle of `log`:
        // it has opened the file as `try_log` does and writes after the
        // rename.
        let dir = tempfile::tempdir().unwrap();
        let logger = Logger::new(dir.path().join("companion.log"));
        logger.log("before");
        let mut held = std::fs::OpenOptions::new()
            .append(true)
            .open(logger.path())
            .unwrap();
        assert!(logger.rotate_above(1));
        held.write_all(b"written during the rotation\n").unwrap();
        drop(held);
        logger.log("after");
        let previous = std::fs::read_to_string(logger.previous_path()).unwrap();
        let lines: Vec<_> = previous.lines().collect();
        assert_eq!(lines.len(), 2, "{previous:?}");
        assert!(lines[0].ends_with(" before"));
        assert_eq!(lines[1], "written during the rotation");
        let current = std::fs::read_to_string(logger.path()).unwrap();
        assert!(current.ends_with(" after\n") && current.lines().count() == 1);
    }

    /// A program that has the log open without sharing it for deletion (an
    /// editor, a `type` in progress) makes Windows refuse the rename.
    #[cfg(windows)]
    #[test]
    fn a_refused_rename_changes_nothing_and_logging_goes_on() {
        use std::os::windows::fs::OpenOptionsExt;
        const SHARE_READ_WRITE: u32 = 1 | 2;
        let hold = |path: &Path| {
            std::fs::OpenOptions::new()
                .read(true)
                .share_mode(SHARE_READ_WRITE)
                .open(path)
                .unwrap()
        };
        let dir = tempfile::tempdir().unwrap();
        let logger = Logger::new(dir.path().join("companion.log"));
        logger.log("before");
        let held = hold(logger.path());
        assert!(!logger.rotate_above(1));
        logger.log("after");
        let text = std::fs::read_to_string(logger.path()).unwrap();
        assert_eq!(text.lines().count(), 2, "{text:?}");
        assert!(!logger.previous_path().exists());
        // Once it is closed, the next start rotates.
        drop(held);
        assert!(logger.rotate_above(1));
        assert_eq!(
            std::fs::read_to_string(logger.previous_path()).unwrap(),
            text
        );
        // The same when it is the previous generation that is held.
        logger.log("next");
        let held = hold(&logger.previous_path());
        assert!(!logger.rotate_above(1));
        drop(held);
        let current = std::fs::read_to_string(logger.path()).unwrap();
        assert!(current.ends_with(" next\n"), "{current:?}");
        assert_eq!(
            std::fs::read_to_string(logger.previous_path()).unwrap(),
            text
        );
    }

    /// `<RFC 3339 time> <message>`: the whole shape of the time is checked,
    /// so a line spliced into another one cannot pass.
    fn well_formed(line: &str) -> bool {
        line.split_once(' ')
            .is_some_and(|(time, message)| is_rfc3339(time) && !message.is_empty())
    }

    /// `YYYY-MM-DDTHH:MM:SS[.fraction]` followed by `Z` or `+HH:MM`/`-HH:MM`.
    fn is_rfc3339(time: &str) -> bool {
        let shaped = |text: &str, shape: &str| {
            text.len() == shape.len()
                && text.bytes().zip(shape.bytes()).all(|(b, s)| match s {
                    b'9' => b.is_ascii_digit(),
                    b'+' => b == b'+' || b == b'-',
                    s => b == s,
                })
        };
        let Some((seconds, rest)) = time.split_at_checked(19) else {
            return false;
        };
        let offset = rest.trim_start_matches(|c: char| c == '.' || c.is_ascii_digit());
        let fraction = &rest[..rest.len() - offset.len()];
        shaped(seconds, "9999-99-99T99:99:99")
            && (fraction.is_empty() || (fraction.len() > 1 && fraction.starts_with('.')))
            && fraction.matches('.').count() <= 1
            && (offset == "Z" || shaped(offset, "+99:99"))
    }

    #[test]
    fn timestamp_shape_check_rejects_spliced_times() {
        for ok in [
            "2026-11-10T12:04:48Z",
            "2026-11-10T12:04:48.043399612Z",
            "2026-11-10T07:04:48.5-05:00",
            "2026-11-10T17:34:48+05:30",
        ] {
            assert!(is_rfc3339(ok), "{ok}");
        }
        for bad in [
            "",
            "unknown-time",
            "2026-11-10T12:04:48",
            "2026-11-10T12:04:48.Z",
            "2026-11-10T12:04:48.1.2Z",
            "2026-11-10T12:04:48.043399612Z2026-11-10T12:04:48.043399669Z",
            "2026-11-10 12:04:48Z",
        ] {
            assert!(!is_rfc3339(bad), "{bad}");
        }
        assert!(is_rfc3339(&now_iso()), "{}", now_iso());
    }

    #[test]
    fn a_message_is_always_one_line() {
        assert_eq!(format_line("T", "plain"), "T plain\n");
        assert_eq!(
            format_line("T", "Access is denied.\r\n"),
            "T Access is denied.\n"
        );
        assert_eq!(
            format_line("T", "first\nsecond\r\n\r\n  third  \rfourth"),
            "T first | second | third | fourth\n"
        );
        assert_eq!(format_line("T", "\n"), "T \n");
        let dir = tempfile::tempdir().unwrap();
        let logger = Logger::new(dir.path().join("companion.log"));
        logger.log("one\ntwo");
        logger.log("three");
        let text = std::fs::read_to_string(logger.path()).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 2, "{text:?}");
        assert!(lines[0].ends_with(" one | two") && lines[1].ends_with(" three"));
    }

    #[test]
    fn concurrent_writers_never_interleave_within_a_line() {
        // Each writer has its own handle on the file, as the controls and
        // the worker do. With one write per formatted piece, most of these
        // lines came out spliced into each other.
        const WRITERS: usize = 8;
        const LINES: usize = 500;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("companion.log");
        std::thread::scope(|scope| {
            for id in 0..WRITERS {
                let logger = Logger::new(path.clone());
                scope.spawn(move || {
                    let message = format!("writer{id}-{}", "x".repeat(60));
                    for _ in 0..LINES {
                        logger.log(&message);
                    }
                });
            }
        });
        let text = std::fs::read_to_string(&path).unwrap();
        let mut seen = [0usize; WRITERS];
        for line in text.lines() {
            assert!(well_formed(line), "{line:?}");
            let message = line.split_once(' ').unwrap().1;
            let id: usize = message
                .strip_prefix("writer")
                .and_then(|rest| rest.strip_suffix(&format!("-{}", "x".repeat(60))))
                .and_then(|id| id.parse().ok())
                .unwrap_or_else(|| panic!("spliced line: {line:?}"));
            seen[id] += 1;
        }
        assert_eq!(seen, [LINES; WRITERS]);
    }
}
