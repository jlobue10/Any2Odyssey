//! The conversion-worker supervisor: the pure decision logic behind
//! "Turn on 3D", automatic recovery and the recovery checkbox. The GUI owns the
//! real process; this type only decides what to do and what to say.
//!
//! Behaviour ported from Any2Odd's `MainForm` (`Poll`, `StartConversion`,
//! `StopConversion`, the recovery checkbox handler) and its `RecoveryValidation`.

use std::time::{Duration, Instant};

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
    pub fn worker_exited(&mut self, exit_code: i32, now: Instant) -> String {
        if self.wanted && self.recover && self.failures < MAX_RECOVERY_ATTEMPTS {
            self.failures += 1;
            self.retry_after = Some(now + RECOVERY_DELAY);
            format!(
                "Conversion stopped ({exit_code}). Recovery attempt {} in {} seconds.",
                self.failures,
                RECOVERY_DELAY.as_secs()
            )
        } else {
            self.wanted = false;
            self.retry_after = None;
            "Conversion stopped. Check the USB connection and logs, then turn 3D on again."
                .to_owned()
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
            let msg = s.worker_exited(1, now);
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
        let msg = s.worker_exited(1, now);
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
        let msg = s.worker_exited(4, t0());
        assert!(msg.starts_with("Conversion stopped."));
        assert!(!s.wanted);
    }

    // Port of RecoveryValidation: a queued retry must respect a later opt-out.
    #[test]
    fn disabling_recovery_cancels_a_queued_retry() {
        let mut s = Supervisor::new(true);
        s.start_requested();
        let now = t0();
        s.worker_exited(1, now);
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
        s.worker_exited(1, t0());
        s.tick_without_worker(t0() + RECOVERY_DELAY); // restarted
        assert_eq!(s.recover_changed(false, true), None);
        assert!(s.wanted);
    }

    #[test]
    fn manual_start_resets_failure_count() {
        let mut s = Supervisor::new(true);
        s.start_requested();
        s.worker_exited(1, t0());
        assert_eq!(s.failures(), 1);
        s.start_requested();
        assert_eq!(s.failures(), 0);
        assert!(!s.retry_pending());
    }
}
