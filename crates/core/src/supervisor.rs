//! The conversion-worker supervisor: the pure decision logic behind
//! "Turn on 3D", automatic recovery and the recovery checkbox. The GUI owns the
//! real process; this type only decides what to do and what to say.
//!
//! Behaviour ported from Any2Odd's `MainForm` (`Poll`, `StartConversion`,
//! `StopConversion`, the recovery checkbox handler) and its `RecoveryValidation`.

use std::time::{Duration, Instant};

use crate::status::exit_code;

pub const MAX_RECOVERY_ATTEMPTS: u32 = 3;
pub const RECOVERY_DELAY: Duration = Duration::from_secs(8);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Nothing to do this tick.
    None,
    /// Launch a worker now.
    StartWorker,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Supervisor {
    /// The user wants conversion running.
    pub wanted: bool,
    /// Automatic recovery preference.
    pub recover: bool,
    failures: u32,
    retry_after: Option<Instant>,
}

impl Default for Supervisor {
    fn default() -> Self {
        Supervisor {
            wanted: false,
            recover: true,
            failures: 0,
            retry_after: None,
        }
    }
}

impl Supervisor {
    pub fn new(recover: bool) -> Supervisor {
        Supervisor {
            recover,
            ..Default::default()
        }
    }
    pub fn failures(&self) -> u32 {
        self.failures
    }
    pub fn retry_pending(&self) -> bool {
        self.wanted && self.retry_after.is_some()
    }

    /// User pressed "Turn on 3D" (or auto-convert at launch). Resets retries.
    pub fn start_requested(&mut self) -> Action {
        self.wanted = true;
        self.failures = 0;
        self.retry_after = None;
        Action::StartWorker
    }

    /// User pressed "Turn off 3D" or quit. The caller stops the process.
    pub fn stop_requested(&mut self) {
        self.wanted = false;
        self.retry_after = None;
    }

    /// A start attempt could not even launch (prerequisite failure, bad source).
    pub fn start_failed(&mut self) {
        self.wanted = false;
        self.retry_after = None;
    }

    /// The worker process exited on its own. Returns the status line to show.
    ///
    /// `reason` is the last state the worker wrote to `worker-status.json`
    /// (its error paths report there before exiting), so the line can say
    /// more than a number. What happens next depends on the exit code, see
    /// [`classify`]: a failure is retried like before, an exit that a retry
    /// cannot cure turns conversion off with a message naming the cause.
    pub fn worker_exited(&mut self, exit_code: i32, reason: Option<&str>, now: Instant) -> String {
        let mut what = describe(exit_code);
        if let Some(reason) = reason.map(str::trim).filter(|r| !r.is_empty()) {
            what = format!(
                "{what}. Last renderer status: {}",
                reason.trim_end_matches('.')
            );
        }
        if classify(exit_code) == ExitKind::NotRetryable {
            self.wanted = false;
            self.retry_after = None;
            return format!("{what}. {}", advice(exit_code));
        }
        if self.wanted && self.recover && self.failures < MAX_RECOVERY_ATTEMPTS {
            self.failures += 1;
            self.retry_after = Some(now + RECOVERY_DELAY);
            format!(
                "{what}. Recovery attempt {} in {} seconds.",
                self.failures,
                RECOVERY_DELAY.as_secs()
            )
        } else {
            self.wanted = false;
            self.retry_after = None;
            format!("{what}. Check the USB connection and logs, then turn 3D on again.")
        }
    }

    /// Periodic poll while no worker is running.
    pub fn tick_without_worker(&mut self, now: Instant) -> Action {
        match self.retry_after {
            Some(at) if self.wanted && now >= at => {
                self.retry_after = None;
                Action::StartWorker
            }
            None if self.wanted => Action::StartWorker,
            _ => Action::None,
        }
    }

    /// The recovery checkbox changed. Never stops a running worker and never
    /// starts one. Disabling recovery while no worker is running after a
    /// failure cancels the recovery attempt, whether its retry is still queued
    /// or its relaunch is already in flight (the caller then stops that worker
    /// when it reports in). Returns a status line when something user-visible
    /// changed.
    pub fn recover_changed(&mut self, enabled: bool, worker_running: bool) -> Option<String> {
        self.recover = enabled;
        if !enabled && self.wanted && !worker_running && self.failures > 0 {
            self.wanted = false;
            self.retry_after = None;
            return Some(
                "Automatic recovery cancelled. Turn on 3D to restart manually.".to_owned(),
            );
        }
        None
    }
}

/// What a worker exit code means for recovery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitKind {
    /// The renderer stopped by itself after it was ready: Samsung's engine
    /// closed its output (or was closed). Nothing says the next start would
    /// fail, so recovery relaunches it like any other unexpected stop.
    EngineClosed,
    /// Rendering failed, or the engine never became ready. Recovery applies.
    Failure,
    /// A retry cannot help until the user acts: another renderer already
    /// holds the engine, or a prerequisite check failed.
    NotRetryable,
}

/// Map a worker exit code to what the supervisor does about it.
pub fn classify(code: i32) -> ExitKind {
    match code {
        exit_code::READY_AND_STOPPED_CLEANLY => ExitKind::EngineClosed,
        exit_code::ALREADY_RUNNING | exit_code::PREREQUISITES_FAILED => ExitKind::NotRetryable,
        _ => ExitKind::Failure,
    }
}

/// The first sentence of the status line for an exit code, without a period.
fn describe(code: i32) -> String {
    match code {
        exit_code::READY_AND_STOPPED_CLEANLY => "The 3D engine closed".to_owned(),
        exit_code::RENDERER_ERROR => "Conversion stopped".to_owned(),
        exit_code::ALREADY_RUNNING => "Another renderer is already running".to_owned(),
        exit_code::PREREQUISITES_FAILED => "A prerequisite check failed".to_owned(),
        exit_code::NEVER_BECAME_READY => "Conversion did not become ready".to_owned(),
        other => format!("Conversion stopped ({other})"),
    }
}

/// What to do about an exit that recovery must not retry.
fn advice(code: i32) -> &'static str {
    match code {
        exit_code::ALREADY_RUNNING => "Close it, or Samsung's 3D player, and turn 3D on again.",
        _ => "Fix the cause and turn 3D on again.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn exit_schedules_up_to_three_retries_then_gives_up() {
        let mut s = Supervisor::new(true);
        assert_eq!(s.start_requested(), Action::StartWorker);
        let now = t0();
        for attempt in 1..=3 {
            let msg = s.worker_exited(1, None, now);
            assert!(
                msg.contains(&format!("Recovery attempt {attempt} in 8 seconds")),
                "{msg}"
            );
            assert!(s.retry_pending());
            assert_eq!(
                s.tick_without_worker(now),
                Action::None,
                "not before the delay"
            );
            assert_eq!(
                s.tick_without_worker(now + Duration::from_secs(7)),
                Action::None
            );
            assert_eq!(
                s.tick_without_worker(now + RECOVERY_DELAY),
                Action::StartWorker
            );
        }
        let msg = s.worker_exited(1, None, now);
        assert!(msg.starts_with("Conversion stopped. Check the USB connection"));
        assert!(!s.wanted && !s.retry_pending());
        assert_eq!(
            s.tick_without_worker(now + Duration::from_secs(60)),
            Action::None
        );
    }

    #[test]
    fn exit_without_recovery_turns_off() {
        let mut s = Supervisor::new(false);
        s.start_requested();
        let msg = s.worker_exited(4, None, t0());
        assert!(msg.starts_with("Conversion did not become ready."), "{msg}");
        assert!(!s.wanted);
    }

    // Port of RecoveryValidation: a queued retry must respect a later opt-out.
    #[test]
    fn disabling_recovery_cancels_a_queued_retry() {
        let mut s = Supervisor::new(true);
        s.start_requested();
        let now = t0();
        s.worker_exited(1, None, now);
        assert!(s.retry_pending());
        let msg = s.recover_changed(false, false);
        assert_eq!(
            msg.as_deref(),
            Some("Automatic recovery cancelled. Turn on 3D to restart manually.")
        );
        assert!(!s.wanted && !s.retry_pending());
        assert_eq!(s.tick_without_worker(now + RECOVERY_DELAY), Action::None);
    }

    #[test]
    fn disabling_recovery_is_idle_without_a_queued_retry() {
        let mut s = Supervisor::new(true);
        assert_eq!(s.recover_changed(false, false), None);
        assert!(!s.recover);
    }

    #[test]
    fn re_enabling_recovery_does_not_start_conversion() {
        let mut s = Supervisor::new(false);
        assert_eq!(s.recover_changed(true, false), None);
        assert!(!s.wanted);
        assert_eq!(s.tick_without_worker(t0()), Action::None);
    }

    #[test]
    fn disabling_recovery_leaves_a_running_worker_alone() {
        let mut s = Supervisor::new(true);
        s.start_requested();
        // Worker running: no failures, nothing queued.
        assert_eq!(s.recover_changed(false, true), None);
        assert!(s.wanted, "the running worker keeps its wanted state");
        // Even with an earlier failure count, a running worker is not touched.
        s.recover = true;
        s.worker_exited(1, None, t0());
        s.tick_without_worker(t0() + RECOVERY_DELAY); // restarted
        assert_eq!(s.recover_changed(false, true), None);
        assert!(s.wanted);
    }

    #[test]
    fn manual_start_resets_failure_count() {
        let mut s = Supervisor::new(true);
        s.start_requested();
        s.worker_exited(1, None, t0());
        assert_eq!(s.failures(), 1);
        s.start_requested();
        assert_eq!(s.failures(), 0);
        assert!(!s.retry_pending());
    }

    #[test]
    fn unretryable_exits_turn_off_with_a_named_cause_even_with_recovery_on() {
        for (code, expected) in [
            (
                exit_code::ALREADY_RUNNING,
                "Another renderer is already running. Close it, or Samsung's 3D player, and turn 3D on again.",
            ),
            (
                exit_code::PREREQUISITES_FAILED,
                "A prerequisite check failed. Fix the cause and turn 3D on again.",
            ),
        ] {
            let mut s = Supervisor::new(true);
            s.start_requested();
            let now = t0();
            let msg = s.worker_exited(code, None, now);
            assert_eq!(msg, expected);
            assert!(!s.wanted && !s.retry_pending());
            assert_eq!(s.failures(), 0, "no attempt is charged for {code}");
            assert_eq!(
                s.tick_without_worker(now + RECOVERY_DELAY),
                Action::None
            );
            assert_eq!(classify(code), ExitKind::NotRetryable);
        }
    }

    #[test]
    fn exit_codes_are_classified_and_described() {
        assert_eq!(
            classify(exit_code::READY_AND_STOPPED_CLEANLY),
            ExitKind::EngineClosed
        );
        assert_eq!(classify(exit_code::RENDERER_ERROR), ExitKind::Failure);
        assert_eq!(classify(exit_code::NEVER_BECAME_READY), ExitKind::Failure);
        assert_eq!(classify(-1), ExitKind::Failure);
        // The engine closing itself is recovered like any other stop.
        let mut s = Supervisor::new(true);
        s.start_requested();
        let msg = s.worker_exited(exit_code::READY_AND_STOPPED_CLEANLY, None, t0());
        assert_eq!(
            msg,
            "The 3D engine closed. Recovery attempt 1 in 8 seconds."
        );
        assert!(s.retry_pending());
        // Codes the worker does not define still show the number.
        let mut s = Supervisor::new(false);
        s.start_requested();
        let msg = s.worker_exited(-1, None, t0());
        assert!(msg.starts_with("Conversion stopped (-1)."), "{msg}");
    }

    #[test]
    fn the_last_renderer_status_is_quoted_without_a_double_period() {
        let mut s = Supervisor::new(true);
        s.start_requested();
        let msg = s.worker_exited(
            exit_code::RENDERER_ERROR,
            Some("Desktop capture error: device removed."),
            t0(),
        );
        assert_eq!(
            msg,
            "Conversion stopped. Last renderer status: Desktop capture error: device removed. Recovery attempt 1 in 8 seconds."
        );
        let mut s = Supervisor::new(true);
        s.start_requested();
        let msg = s.worker_exited(
            exit_code::PREREQUISITES_FAILED,
            Some("  3D Stereo WebCam not found  "),
            t0(),
        );
        assert_eq!(
            msg,
            "A prerequisite check failed. Last renderer status: 3D Stereo WebCam not found. Fix the cause and turn 3D on again."
        );
        let mut s = Supervisor::new(false);
        s.start_requested();
        let msg = s.worker_exited(exit_code::RENDERER_ERROR, Some("   "), t0());
        assert_eq!(
            msg,
            "Conversion stopped. Check the USB connection and logs, then turn 3D on again."
        );
    }
}
