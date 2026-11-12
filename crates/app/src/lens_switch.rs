//! Switching the Odyssey's lens on and off.
//!
//! The lens is switched by Leia's SR runtime, which owns the conversation
//! with the monitor's lens board. A program asks the runtime through Leia's
//! SDK, and the runtime keeps the lens on only while a connected client asks
//! for it. That SDK is commercial and is not part of this program, so the
//! asking is done by a separate helper program (`contrib/lens-helper`) that
//! whoever has the SDK builds and installs. This module only starts that
//! helper, holds it while the lens should be on, and lets it go.
//!
//! What the helper is told and tells, all through its standard streams:
//! it switches the lens on when started; it writes `on` on a line of its own
//! once the runtime has accepted that, or `error: <reason>` and exits; it
//! switches the lens off and exits when its standard input closes. Closing
//! the input is also what happens when this program dies, so a crash does
//! not leave the lens on.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

pub const HELPER_NAME: &str = "any2odyssey-lens-helper";
/// A full path to a helper, for trying one without installing it.
pub const HELPER_ENV: &str = "ANY2ODYSSEY_LENS_HELPER";
/// How long a helper gets to switch the lens off and leave by itself.
const LEAVE_GRACE: Duration = Duration::from_secs(3);
const MAX_REASON: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LensStatus {
    /// Nothing has asked for the lens.
    Idle,
    /// No helper is installed: the lens cannot be switched from here.
    NoHelper,
    /// The helper is running and has not answered yet.
    Starting,
    On,
    /// The last attempt failed; the next request tries again.
    Failed(String),
}

impl LensStatus {
    /// One line for the page and the output window.
    pub fn describe(&self) -> String {
        match self {
            LensStatus::Idle => {
                "Lens switch: off. It is asked for while lens output is full screen.".into()
            }
            LensStatus::NoHelper => {
                "Lens switch: no helper installed, so the lens stays off (see docs/LENS-SWITCH.md)."
                    .into()
            }
            LensStatus::Starting => "Lens switch: asking the Leia SR runtime…".into(),
            LensStatus::On => "Lens switch: on.".into(),
            LensStatus::Failed(reason) => format!("Lens switch failed: {reason}"),
        }
    }
}

/// Where a helper is looked for, first match wins: the environment variable,
/// beside this executable, the user's data folder (where
/// `contrib/wine-lens/setup.sh` puts one), then the package's library folders.
pub fn locate() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(HELPER_ENV).filter(|v| !v.is_empty()) {
        // A named helper that is missing is reported when it fails to start,
        // not silently replaced by another one.
        return Some(PathBuf::from(path));
    }
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(HELPER_NAME)));
    let own = any2odyssey_core::paths::data_dir().join(HELPER_NAME);
    let installed = ["/usr/lib/any2odyssey", "/usr/local/lib/any2odyssey"]
        .into_iter()
        .map(|dir| Path::new(dir).join(HELPER_NAME));
    beside
        .into_iter()
        .chain(std::iter::once(own))
        .chain(installed)
        .find(|p| p.is_file())
}

/// A helper's reason, made safe to show: one line, printable, bounded.
pub(crate) fn tidy(reason: &str) -> String {
    let clean: String = reason
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_REASON)
        .collect();
    let clean = clean.trim();
    if clean.is_empty() {
        "the helper gave no reason".into()
    } else {
        clean.to_owned()
    }
}

/// A helper program that is running: held by its standard input, heard
/// through its standard output. The weaver output (`weaver`) holds its
/// helper the same way.
pub(crate) struct Running {
    child: Child,
    /// Held open while the lens should be on; dropping it is the request to
    /// switch off.
    stdin: Option<ChildStdin>,
    pub(crate) lines: Receiver<String>,
    pub(crate) on: bool,
}

impl Running {
    /// Ask the helper to leave and make sure it does, without waiting here.
    pub(crate) fn let_go(mut self) {
        self.stdin = None;
        let mut child = self.child;
        let reap = move || {
            let deadline = Instant::now() + LEAVE_GRACE;
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => return,
                    Ok(None) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    _ => break,
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        };
        // Without a thread to wait on, the helper still sees its input close
        // and leaves by itself; it is only not reaped until this program ends.
        let _ = std::thread::Builder::new()
            .name("lens-helper-reap".into())
            .spawn(reap);
    }
}

pub struct LensSwitch {
    helper: Option<PathBuf>,
    wanted: bool,
    running: Option<Running>,
    failed: Option<String>,
}

impl LensSwitch {
    /// The switch with the installed helper, if there is one.
    pub fn new() -> LensSwitch {
        LensSwitch::with_helper(locate())
    }

    pub fn with_helper(helper: Option<PathBuf>) -> LensSwitch {
        LensSwitch {
            helper,
            wanted: false,
            running: None,
            failed: None,
        }
    }

    /// Say whether the lens should be on now. Call as often as convenient:
    /// only a change does anything. `wake` is called from another thread
    /// whenever the helper has said something, so the caller can look again.
    pub fn set(&mut self, on: bool, wake: impl Fn() + Send + 'static) {
        if on == self.wanted {
            return;
        }
        self.wanted = on;
        if !on {
            // A failure stays readable until the next request.
            if let Some(running) = self.running.take() {
                running.let_go();
            }
            return;
        }
        self.failed = None;
        let Some(helper) = self.helper.clone() else {
            return;
        };
        match start(Command::new(&helper), wake) {
            Ok(running) => self.running = Some(running),
            Err(e) => {
                self.failed = Some(tidy(&format!("could not start {}: {e}", helper.display())))
            }
        }
    }

    /// Take in what the helper has said since the last call.
    pub fn poll(&mut self) {
        let Some(running) = &mut self.running else {
            return;
        };
        let gone = loop {
            match running.lines.try_recv() {
                Ok(line) => {
                    let line = line.trim();
                    if line == "on" {
                        running.on = true;
                    } else if let Some(reason) = line.strip_prefix("error:") {
                        self.failed = Some(tidy(reason));
                    }
                }
                Err(TryRecvError::Empty) => break false,
                Err(TryRecvError::Disconnected) => break true,
            }
        };
        if gone || self.failed.is_some() {
            // The helper left while the lens was wanted: the lens is off.
            self.failed
                .get_or_insert_with(|| "the helper stopped unexpectedly".into());
            if let Some(running) = self.running.take() {
                running.let_go();
            }
        }
    }

    pub fn status(&self) -> LensStatus {
        if let Some(reason) = &self.failed {
            return LensStatus::Failed(reason.clone());
        }
        match &self.running {
            Some(running) if running.on => LensStatus::On,
            Some(_) => LensStatus::Starting,
            None if self.helper.is_none() => LensStatus::NoHelper,
            None => LensStatus::Idle,
        }
    }
}

impl Drop for LensSwitch {
    fn drop(&mut self) {
        if let Some(running) = self.running.take() {
            running.let_go();
        }
    }
}

/// Start a helper with its input held and its output read line by line on
/// a thread of its own; `wake` runs whenever it has said something.
pub(crate) fn start(
    mut command: Command,
    wake: impl Fn() + Send + 'static,
) -> std::io::Result<Running> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let (tx, rx) = mpsc::channel();
    let read = move || {
        if let Some(stdout) = stdout {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    return;
                }
                wake();
            }
        }
        // Dropping the sender tells `poll` the helper has closed its output.
        drop(tx);
        wake();
    };
    if let Err(e) = std::thread::Builder::new()
        .name("lens-helper-read".into())
        .spawn(read)
    {
        let _ = child.kill();
        let _ = child.wait();
        return Err(e);
    }
    Ok(Running {
        child,
        stdin,
        lines: rx,
        on: false,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("helper.sh");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// Ask for the lens. A script written a moment ago can be "busy" while
    /// another test's child process is between fork and exec with the file
    /// still open for writing; that is the test harness, so try again.
    fn turn_on(switch: &mut LensSwitch, wake: impl Fn() + Send + Clone + 'static) {
        for _ in 0..200 {
            switch.set(true, wake.clone());
            match switch.status() {
                LensStatus::Failed(reason) if reason.contains("busy") => {
                    switch.set(false, || {});
                    std::thread::sleep(Duration::from_millis(10));
                }
                _ => return,
            }
        }
    }

    fn wait_for(switch: &mut LensSwitch, done: impl Fn(&LensStatus) -> bool) -> LensStatus {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            switch.poll();
            let status = switch.status();
            if done(&status) || Instant::now() > deadline {
                return status;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn wait_for_file(path: &Path) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if path.exists() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn without_a_helper_nothing_is_started_and_the_page_says_so() {
        let mut switch = LensSwitch::with_helper(None);
        assert_eq!(switch.status(), LensStatus::NoHelper);
        switch.set(true, || {});
        switch.poll();
        assert_eq!(switch.status(), LensStatus::NoHelper);
        assert!(switch.status().describe().contains("no helper"));
    }

    #[test]
    fn the_helper_is_held_while_wanted_and_told_to_switch_off_by_closing_its_input() {
        let dir = tempfile::tempdir().unwrap();
        let off = dir.path().join("off");
        let helper = script(
            dir.path(),
            &format!("echo on\ncat >/dev/null\n: > '{}'", off.display()),
        );
        let mut switch = LensSwitch::with_helper(Some(helper));
        assert_eq!(switch.status(), LensStatus::Idle);
        turn_on(&mut switch, || {});
        assert_eq!(
            wait_for(&mut switch, |s| *s == LensStatus::On),
            LensStatus::On
        );
        // Asking again changes nothing and the helper stays.
        switch.set(true, || {});
        assert!(!off.exists());
        switch.set(false, || {});
        assert_eq!(switch.status(), LensStatus::Idle);
        assert!(wait_for_file(&off), "the helper never saw its input close");
    }

    #[test]
    fn dropping_the_switch_lets_the_helper_go() {
        let dir = tempfile::tempdir().unwrap();
        let off = dir.path().join("off");
        let helper = script(
            dir.path(),
            &format!("echo on\ncat >/dev/null\n: > '{}'", off.display()),
        );
        let mut switch = LensSwitch::with_helper(Some(helper));
        turn_on(&mut switch, || {});
        wait_for(&mut switch, |s| *s == LensStatus::On);
        drop(switch);
        assert!(wait_for_file(&off));
    }

    #[test]
    fn a_reported_error_is_shown_kept_after_switching_off_and_cleared_by_the_next_request() {
        let dir = tempfile::tempdir().unwrap();
        let helper = script(dir.path(), "echo 'error: SR runtime unavailable'\nexit 1");
        let mut switch = LensSwitch::with_helper(Some(helper));
        turn_on(&mut switch, || {});
        let failed = LensStatus::Failed("SR runtime unavailable".into());
        assert_eq!(
            wait_for(&mut switch, |s| matches!(s, LensStatus::Failed(_))),
            failed
        );
        switch.set(false, || {});
        assert_eq!(switch.status(), failed);
        turn_on(&mut switch, || {});
        assert_ne!(switch.status(), failed);
    }

    #[test]
    fn a_helper_that_leaves_without_a_word_is_a_failure() {
        let dir = tempfile::tempdir().unwrap();
        let helper = script(dir.path(), "exit 0");
        let mut switch = LensSwitch::with_helper(Some(helper));
        turn_on(&mut switch, || {});
        assert_eq!(
            wait_for(&mut switch, |s| matches!(s, LensStatus::Failed(_))),
            LensStatus::Failed("the helper stopped unexpectedly".into())
        );
    }

    #[test]
    fn a_helper_that_cannot_be_started_is_a_failure_with_its_path() {
        let mut switch = LensSwitch::with_helper(Some("/nonexistent/lens-helper".into()));
        switch.set(true, || {});
        match switch.status() {
            LensStatus::Failed(reason) => assert!(reason.contains("/nonexistent/lens-helper")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_wake_callback_runs_when_the_helper_speaks() {
        let dir = tempfile::tempdir().unwrap();
        let helper = script(dir.path(), "echo on\ncat >/dev/null");
        let (tx, rx) = mpsc::channel();
        let mut switch = LensSwitch::with_helper(Some(helper));
        turn_on(&mut switch, move || {
            let _ = tx.send(());
        });
        assert!(rx.recv_timeout(Duration::from_secs(10)).is_ok());
        switch.poll();
        assert_eq!(switch.status(), LensStatus::On);
    }

    #[test]
    fn reasons_are_made_safe_to_show() {
        assert_eq!(tidy("  a\tb\u{1b}[31m  "), "ab[31m");
        assert_eq!(tidy(" \n"), "the helper gave no reason");
        assert_eq!(tidy(&"x".repeat(1000)).len(), MAX_REASON);
    }
}
