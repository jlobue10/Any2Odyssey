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
    ///
    /// Processes that start together rotate one at a time, so that the
    /// second cannot move the file the first has just started over the
    /// generation it kept: `companion.rotating` is there for as long as one
    /// of them is at it.
    pub fn rotate_if_large(&self) {
        self.rotate_above(ROTATE_ABOVE);
    }
    /// Where [`rotate_if_large`](Self::rotate_if_large) moves the log to.
    pub fn previous_path(&self) -> PathBuf {
        self.path.with_extension("old.log")
    }
    fn rotate_above(&self, limit: u64) -> bool {
        let large = || std::fs::metadata(&self.path).is_ok_and(|m| m.is_file() && m.len() > limit);
        if !large() {
            return false;
        }
        // The controls and the worker start within a moment of each other,
        // and both could find the log large. One renamed it and logged a
        // line, which started a new file; the other's rename then moved
        // that file of one line over the generation just kept, and it was
        // gone. So one process rotates at a time, and looks again once it
        // is its turn: what the other has rotated already is small now.
        let Some(_turn) = self.take_turn() else {
            return false;
        };
        large() && std::fs::rename(&self.path, self.previous_path()).is_ok()
    }
    /// The file that is there for as long as one process is rotating.
    fn turn_path(&self) -> PathBuf {
        self.path.with_extension("rotating")
    }
    /// `None` while another process has the turn: that one rotates, and
    /// this one has nothing to do. A turn left behind by a process that was
    /// ended in the middle of its own (they last as long as a rename) is
    /// removed once it is [`TURN_STALE_AFTER`] old, and the start after
    /// this one rotates.
    fn take_turn(&self) -> Option<Turn> {
        self.take_turn_left_for(TURN_STALE_AFTER)
    }
    /// [`take_turn`](Self::take_turn), with the age from which what is in
    /// the turn's place counts as left behind.
    fn take_turn_left_for(&self, stale_after: std::time::Duration) -> Option<Turn> {
        let path = self.turn_path();
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(_) => Some(Turn(Some(path))),
            Err(_) => {
                // Whatever the refusal was called (for a folder under the
                // name Windows says "access denied", not "already
                // exists"): is something there, and how old is it? Where
                // nothing is, no turn can be had here at all.
                //
                // It is what has the name that is asked, not what a link
                // there leads to: asked through the link, one that leads
                // nowhere had no age and was never old enough, and neither
                // it nor a folder was removed by the removing of a file.
                // Every start then found "another process" at the
                // rotating, and the log grew without end.
                //
                // A time in the future (the clock was set back) is no age
                // to wait out.
                let stale = std::fs::symlink_metadata(&path)
                    .and_then(|m| m.modified())
                    .is_ok_and(|at| at.elapsed().map_or(true, |age| age >= stale_after));
                if stale
                    && std::fs::remove_file(&path).is_err()
                    && std::fs::remove_dir(&path).is_err()
                {
                    // Old, and it cannot be removed (a folder with
                    // something in it): no process's turn, and nothing
                    // that will ever go. The log is rotated without a
                    // turn, as it was before there were turns.
                    return Some(Turn(None));
                }
                None
            }
        }
    }
    fn try_log(&self, message: &str) -> std::io::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        // The log names window titles like the other files of the data
        // folder, and was the one file there that other users could read:
        // it is made for its owner only (0600), as they are.
        //
        // And it is opened without waiting. A named pipe where the log
        // belongs has no reader, and opening it for writing waits for one:
        // the first line a process logged was as far as it got.
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY);
        }
        let mut file = options.open(&self.path)?;
        // Checked on what was opened, so that a path replaced since cannot
        // make the check wrong: a pipe that does have a reader, or a
        // device, takes no line.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = file.metadata()?;
            if !metadata.is_file() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "{} is not a regular file and was not written to",
                        self.path.display()
                    ),
                ));
            }
            // A log made by 1.0.0 or earlier is still there, readable by
            // everyone, until it is rotated, which may be never.
            //
            // Not where the log's name is a link. The lines go to the
            // file the link leads to, as they always have, but that file
            // is somewhere else and whoever put the link there chose who
            // may read it: it was closed to them at every line, however
            // often they opened it again.
            let through_a_link = || {
                std::fs::symlink_metadata(&self.path).is_ok_and(|at| at.file_type().is_symlink())
            };
            if metadata.permissions().mode() & 0o077 != 0 && !through_a_link() {
                let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
            }
        }
        // `writeln!` on a `File` issues one write per formatted piece, and
        // another process's line can land between any two of them.
        file.write_all(format_line(&now_iso(), message).as_bytes())
    }
}

/// How old the file of a rotation turn must be before it is taken for one
/// that was left behind.
const TURN_STALE_AFTER: std::time::Duration = std::time::Duration::from_secs(60);

/// One process's turn at rotating the log; its file goes when it is over.
/// Without a file where the turn's place is taken for good by something
/// that is no turn (see [`Logger::take_turn_left_for`]).
struct Turn(Option<PathBuf>);

impl Drop for Turn {
    fn drop(&mut self) {
        if let Some(file) = &self.0 {
            let _ = std::fs::remove_file(file);
        }
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

    /// The controls and the worker start together. Both found the log
    /// large; the second rename moved the file the first had just started
    /// over the generation it had kept.
    #[test]
    fn processes_that_start_together_rotate_once_and_keep_the_generation() {
        const STARTS: usize = 8;
        const LIMIT: u64 = 4096;
        for round in 0..200 {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("companion.log");
            let generation = format!("the generation to keep\n{}\n", "x".repeat(LIMIT as usize));
            std::fs::write(&path, &generation).unwrap();
            let together = std::sync::Barrier::new(STARTS);
            std::thread::scope(|scope| {
                for _ in 0..STARTS {
                    scope.spawn(|| {
                        let logger = Logger::new(path.clone());
                        together.wait();
                        logger.rotate_above(LIMIT);
                        logger.log("started");
                    });
                }
            });
            let logger = Logger::new(path.clone());
            let previous = std::fs::read_to_string(logger.previous_path()).unwrap();
            assert!(
                previous.starts_with(&generation),
                "round {round}: the previous generation is {previous:?}"
            );
            // Every line is in one of the two files, and nothing else is
            // left in the folder.
            let current = std::fs::read_to_string(&path).unwrap_or_default();
            let started = |text: &str| text.lines().filter(|l| l.ends_with(" started")).count();
            assert_eq!(started(&previous) + started(&current), STARTS);
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
        }
    }

    #[test]
    fn a_log_is_left_alone_while_another_process_rotates_it() {
        let dir = tempfile::tempdir().unwrap();
        let logger = Logger::new(dir.path().join("companion.log"));
        logger.log("before");
        let whole = std::fs::read_to_string(logger.path()).unwrap();
        // The other process is between its look at the size and its rename.
        let other = logger.take_turn().unwrap();
        let turn = dir.path().join("companion.rotating");
        assert_eq!(other.0.as_deref(), Some(turn.as_path()));
        assert!(!logger.rotate_above(1));
        assert_eq!(std::fs::read_to_string(logger.path()).unwrap(), whole);
        assert!(!logger.previous_path().exists() && turn.exists());
        drop(other);
        assert!(logger.rotate_above(1));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        // A process that was ended in its turn left the file behind. Not
        // old enough to tell: left. Old enough: removed, and the start
        // after that rotates.
        logger.log("next");
        let left_behind = logger.turn_path();
        std::fs::write(&left_behind, b"").unwrap();
        assert!(!logger.rotate_above(1));
        assert!(left_behind.exists());
        let then = std::time::SystemTime::now() - 2 * TURN_STALE_AFTER;
        std::fs::File::options()
            .write(true)
            .open(&left_behind)
            .unwrap()
            .set_modified(then)
            .unwrap();
        assert!(!logger.rotate_above(1));
        assert!(!left_behind.exists() && logger.path().exists());
        assert!(logger.rotate_above(1));
        let previous = std::fs::read_to_string(logger.previous_path()).unwrap();
        assert!(previous.ends_with(" next\n"), "{previous:?}");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        // A small log asks for no turn, so a file left behind is not even
        // looked at.
        std::fs::write(&left_behind, b"").unwrap();
        logger.log("small");
        assert!(!logger.rotate_above(4096));
        assert!(left_behind.exists());
    }

    /// A process leaves an empty file behind and nothing else. Whatever
    /// else has the turn's name must not keep the log from being rotated
    /// for good: a folder there, or a link that leads nowhere, was never
    /// old and never removed, and the log grew at every start.
    #[test]
    fn nothing_in_the_place_of_a_turn_keeps_the_log_from_being_rotated_for_good() {
        let old = std::time::Duration::ZERO;
        let dir = tempfile::tempdir().unwrap();
        let logger = Logger::new(dir.path().join("companion.log"));
        let turn = logger.turn_path();
        // An empty folder goes like a file that was left behind, and the
        // start after that has its turn.
        std::fs::create_dir(&turn).unwrap();
        assert!(logger.take_turn().is_none(), "not old enough to tell");
        assert!(turn.is_dir());
        assert!(logger.take_turn_left_for(old).is_none());
        assert!(!turn.exists());
        assert!(logger.take_turn().is_some_and(|turn| turn.0.is_some()));
        assert!(!turn.exists());
        // A folder with something in it cannot be removed, and is nobody's
        // turn: the log is rotated without one, and the folder is left.
        std::fs::create_dir(&turn).unwrap();
        std::fs::write(turn.join("kept"), b"kept").unwrap();
        let without = logger.take_turn_left_for(old).unwrap();
        assert!(without.0.is_none());
        drop(without);
        assert_eq!(std::fs::read(turn.join("kept")).unwrap(), b"kept");
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            std::fs::remove_dir_all(&turn).unwrap();
            // A link that leads nowhere is removed, by its own age.
            symlink("nothing-there", &turn).unwrap();
            assert!(logger.take_turn().is_none(), "not old enough to tell");
            assert!(turn.is_symlink());
            assert!(logger.take_turn_left_for(old).is_none());
            assert!(!turn.is_symlink());
            // So is a link to a file, and not the file.
            let elsewhere = dir.path().join("elsewhere");
            std::fs::write(&elsewhere, b"kept").unwrap();
            symlink(&elsewhere, &turn).unwrap();
            assert!(logger.take_turn_left_for(old).is_none());
            assert!(!turn.is_symlink());
            assert_eq!(std::fs::read(&elsewhere).unwrap(), b"kept");
            // The folder again, by the clock and through the rotation.
            std::fs::create_dir(&turn).unwrap();
            std::fs::write(turn.join("kept"), b"kept").unwrap();
            let then = std::time::SystemTime::now() - 2 * TURN_STALE_AFTER;
            std::fs::File::open(&turn)
                .unwrap()
                .set_modified(then)
                .unwrap();
            logger.log("a line");
            assert!(logger.rotate_above(1));
            assert!(logger.previous_path().exists() && !logger.path().exists());
            assert!(turn.join("kept").exists());
        }
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

    /// A named pipe at `companion.log` (made by mistake, or by another
    /// program of the same user) used to stop every process at its first
    /// logged line, the self-test included: opening a pipe for writing
    /// waits for a reader.
    #[cfg(unix)]
    #[test]
    fn logging_does_not_wait_on_a_pipe_where_the_log_belongs() {
        use std::os::unix::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let pipe = dir.path().join("companion.log");
        let made = std::process::Command::new("mkfifo").arg(&pipe).status();
        if !made.is_ok_and(|status| status.success()) {
            // (a system without mkfifo: nothing to try it with)
            return;
        }
        let (done, logged) = std::sync::mpsc::channel();
        let path = pipe.clone();
        std::thread::spawn(move || {
            let logger = Logger::new(path);
            let result = logger.try_log("to nobody").map_err(|e| e.kind());
            // The call the rest of the program makes swallows the refusal.
            logger.log("to nobody");
            done.send(result)
        });
        let logged = logged.recv_timeout(std::time::Duration::from_secs(10));
        // Let a logger that does wait go, so that the test ends either way.
        // Kept open: from here on the pipe has a reader.
        let mut reader = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&pipe)
            .unwrap();
        assert!(
            matches!(logged, Ok(Err(_))),
            "logging waited on the pipe, or wrote to it: {logged:?}"
        );
        // With a reader the pipe can be opened, and still takes no line.
        let logger = Logger::new(pipe.clone());
        let refused = logger.try_log("to a reader").unwrap_err();
        assert_eq!(refused.kind(), std::io::ErrorKind::InvalidInput);
        assert!(
            refused.to_string().contains("not a regular file"),
            "{refused}"
        );
        let mut read = [0u8; 16];
        let nothing = std::io::Read::read(&mut reader, &mut read).unwrap_err();
        assert_eq!(nothing.kind(), std::io::ErrorKind::WouldBlock);
        // A folder is no log either. A link to a file is logged through.
        let folder = Logger::new(dir.path().join("folder.log"));
        std::fs::create_dir(folder.path()).unwrap();
        assert!(folder.try_log("to a folder").is_err());
        let link = Logger::new(dir.path().join("link.log"));
        std::os::unix::fs::symlink("real.log", link.path()).unwrap();
        link.log("through the link");
        let text = std::fs::read_to_string(dir.path().join("real.log")).unwrap();
        assert!(text.ends_with(" through the link\n"), "{text:?}");
    }

    /// The log was the one file of the data folder made with the default
    /// permissions (0644 under the usual umask); the rest are 0600.
    #[cfg(unix)]
    #[test]
    fn the_log_is_private_to_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        let dir = tempfile::tempdir().unwrap();
        let logger = Logger::new(dir.path().join("companion.log"));
        logger.log("first");
        assert_eq!(mode(logger.path()), 0o600);
        // One an earlier version made is closed to others by the next line,
        // and keeps what it had.
        std::fs::set_permissions(logger.path(), std::fs::Permissions::from_mode(0o644)).unwrap();
        logger.log("second");
        assert_eq!(mode(logger.path()), 0o600);
        let text = std::fs::read_to_string(logger.path()).unwrap();
        assert_eq!(text.lines().count(), 2, "{text:?}");
        // So is the generation that follows a rotation.
        assert!(logger.rotate_above(1));
        logger.log("third");
        assert_eq!(mode(logger.path()), 0o600);
        assert_eq!(mode(&logger.previous_path()), 0o600);
        // A file somewhere else that the log's name is a link to takes the
        // line, as it always has, and stays as open as it was made: it
        // was closed to its other readers at every line.
        let elsewhere = dir.path().join("notes.txt");
        std::fs::write(&elsewhere, b"somebody's notes\n").unwrap();
        std::fs::set_permissions(&elsewhere, std::fs::Permissions::from_mode(0o664)).unwrap();
        let linked = Logger::new(dir.path().join("linked.log"));
        std::os::unix::fs::symlink(&elsewhere, linked.path()).unwrap();
        linked.log("through the link");
        assert_eq!(mode(&elsewhere), 0o664);
        let text = std::fs::read_to_string(&elsewhere).unwrap();
        assert!(text.starts_with("somebody's notes\n"), "{text:?}");
        assert!(text.ends_with(" through the link\n"), "{text:?}");
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
