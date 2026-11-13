//! The conversion-worker supervisor: the pure decision logic behind
//! "Turn on 3D", automatic recovery and the recovery checkbox. The GUI owns the
//! real process; this type only decides what to do and what to say.
//!
//! Behaviour ported from Any2Odd's `MainForm` (`Poll`, `StartConversion`,
//! `StopConversion`, the recovery checkbox handler) and its `RecoveryValidation`.

use std::time::{Duration, Instant};

use crate::status::exit_code;

/// Recovery attempts in a row before the supervisor gives up. The count
/// starts again once a relaunched worker has stayed ready for
/// [`RECOVERY_RESET_AFTER`], or when the user starts conversion.
pub const MAX_RECOVERY_ATTEMPTS: u32 = 3;
pub const RECOVERY_DELAY: Duration = Duration::from_secs(8);
/// How long a worker must stay ready before earlier failures stop counting.
/// Not zero: a renderer that becomes ready and dies seconds later, every
/// time, would otherwise be relaunched forever instead of three times.
pub const RECOVERY_RESET_AFTER: Duration = Duration::from_secs(60);

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
    /// Recovery attempts since the last manual start or the last worker that
    /// stayed ready for [`RECOVERY_RESET_AFTER`].
    failures: u32,
    retry_after: Option<Instant>,
    /// A recovery is under way: its retry is queued, or its relaunch has not
    /// reported ready yet. This, not `failures`, is what the recovery
    /// checkbox cancels; `failures` outlives a recovery that succeeded.
    recovering: bool,
    /// When the running worker was first seen ready.
    ready_since: Option<Instant>,
    /// A start has already been repeated because another renderer held the
    /// engine. Only one such wait is made per start the user asked for.
    waited_for_other: bool,
}

impl Default for Supervisor {
    fn default() -> Self {
        Supervisor {
            wanted: false,
            recover: true,
            failures: 0,
            retry_after: None,
            recovering: false,
            ready_since: None,
            waited_for_other: false,
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
    /// A recovery attempt is queued or its relaunch has not become ready yet.
    pub fn recovering(&self) -> bool {
        self.recovering
    }

    /// User pressed "Turn on 3D" (or auto-convert at launch). Resets retries.
    pub fn start_requested(&mut self) -> Action {
        self.wanted = true;
        self.failures = 0;
        self.waited_for_other = false;
        self.end_attempt();
        Action::StartWorker
    }

    /// User pressed "Turn off 3D" or quit. The caller stops the process.
    pub fn stop_requested(&mut self) {
        self.wanted = false;
        self.end_attempt();
    }

    /// A start attempt could not even launch (prerequisite failure, bad source).
    pub fn start_failed(&mut self) {
        self.wanted = false;
        self.end_attempt();
    }

    /// Nothing is queued, relaunching or ready any more.
    fn end_attempt(&mut self) {
        self.retry_after = None;
        self.recovering = false;
        self.ready_since = None;
    }

    /// The running worker reports that its output is live. Call on every poll
    /// that sees it ready, with the poll's time.
    ///
    /// A recovery that got this far is complete, so the recovery checkbox no
    /// longer has anything to cancel. Once the worker has stayed ready for
    /// [`RECOVERY_RESET_AFTER`], the failures before it stop counting: three
    /// attempts are a limit on failures in a row, not on a whole session in
    /// which every relaunch worked (a display colour-mode change deliberately
    /// restarts the renderer through recovery).
    pub fn worker_ready(&mut self, now: Instant) {
        self.recovering = false;
        self.retry_after = None;
        let since = *self.ready_since.get_or_insert(now);
        if now.saturating_duration_since(since) >= RECOVERY_RESET_AFTER {
            self.failures = 0;
        }
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
        self.ready_since = None;
        // The other renderer is most often our own from a moment ago: when
        // the controls die, their worker keeps the engine until its stop
        // completes or its watchdog ends it, a few seconds. Controls started
        // again inside that time would otherwise turn 3D off and tell the
        // user to close a renderer that has no window. So this start is
        // repeated once, after longer than that renderer can last. It is not
        // a recovery attempt: it is made with recovery off too, and charges
        // no failure. A renderer still there the second time is someone
        // else's, and the message below is right.
        //
        // A start made while that repeat is still to come (Apply or a tray
        // mode command relaunches at once, without asking here) meets the
        // same renderer. It is not the repeat: the wait goes on to its end.
        let repeat_to_come = self.waited_for_other && self.retry_after.is_some();
        if exit_code == exit_code::ALREADY_RUNNING
            && self.wanted
            && (!self.waited_for_other || repeat_to_come)
        {
            let at = match self.retry_after {
                Some(at) if repeat_to_come => at,
                _ => now + RECOVERY_DELAY,
            };
            self.waited_for_other = true;
            self.retry_after = Some(at);
            self.recovering = true;
            return format!(
                "{what}. Trying once more in {} seconds.",
                at.saturating_duration_since(now)
                    .as_secs_f32()
                    .ceil()
                    .max(1.0)
            );
        }
        if classify(exit_code) == ExitKind::NotRetryable {
            self.wanted = false;
            self.end_attempt();
            return format!("{what}. {}", advice(exit_code));
        }
        if self.wanted && self.recover && self.failures < MAX_RECOVERY_ATTEMPTS {
            self.failures += 1;
            self.retry_after = Some(now + RECOVERY_DELAY);
            self.recovering = true;
            format!(
                "{what}. Recovery attempt {} in {} seconds.",
                self.failures,
                RECOVERY_DELAY.as_secs()
            )
        } else {
            self.wanted = false;
            self.end_attempt();
            format!("{what}. Check the USB connection and logs, then turn 3D on again.")
        }
    }

    /// Periodic poll while no worker is running.
    pub fn tick_without_worker(&mut self, now: Instant) -> Action {
        // No worker, so none is ready: a worker the caller stopped and is
        // about to relaunch (Apply) must earn its own stable interval.
        self.ready_since = None;
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
    /// starts one. Disabling recovery while a recovery attempt is under way
    /// and no worker is running cancels that attempt, whether its retry is
    /// still queued or its relaunch is already in flight (the caller then
    /// stops that worker when it reports in). A recovery that already reached
    /// ready is over: a restart the user asked for afterwards (Apply, the
    /// tray's mode commands) is not a recovery and is left alone. Returns a
    /// status line when something user-visible changed.
    pub fn recover_changed(&mut self, enabled: bool, worker_running: bool) -> Option<String> {
        self.recover = enabled;
        if !enabled && self.wanted && !worker_running && self.recovering {
            self.wanted = false;
            self.end_attempt();
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
        let now = t0();
        s.worker_exited(1, None, now);
        assert_eq!(
            s.tick_without_worker(now + RECOVERY_DELAY),
            Action::StartWorker,
            "the relaunch is what makes a worker run again"
        );
        assert!(s.recovering(), "relaunched, not ready yet");
        assert_eq!(s.recover_changed(false, true), None);
        assert!(s.wanted);
    }

    #[test]
    fn isolated_exits_that_each_recover_never_exhaust_the_attempts() {
        // A colour-mode change restarts the renderer through recovery; over a
        // long session that must not add up to "three strikes".
        let mut s = Supervisor::new(true);
        s.start_requested();
        let mut now = t0();
        for _ in 0..10 {
            let msg = s.worker_exited(1, None, now);
            assert!(msg.contains("Recovery attempt 1 in 8 seconds"), "{msg}");
            now += RECOVERY_DELAY;
            assert_eq!(s.tick_without_worker(now), Action::StartWorker);
            s.worker_ready(now);
            assert!(!s.recovering());
            assert_eq!(s.failures(), 1, "not forgiven before it proved stable");
            now += RECOVERY_RESET_AFTER;
            s.worker_ready(now);
            assert_eq!(s.failures(), 0);
        }
        assert!(s.wanted);
    }

    #[test]
    fn a_renderer_that_gets_ready_and_dies_again_is_still_limited() {
        let mut s = Supervisor::new(true);
        s.start_requested();
        let mut now = t0();
        for attempt in 1..=MAX_RECOVERY_ATTEMPTS {
            let msg = s.worker_exited(1, None, now);
            assert!(
                msg.contains(&format!("Recovery attempt {attempt} in")),
                "{msg}"
            );
            now += RECOVERY_DELAY;
            assert_eq!(s.tick_without_worker(now), Action::StartWorker);
            // Ready, polled a few times, gone again within the stable interval.
            for _ in 0..4 {
                s.worker_ready(now);
                now += Duration::from_secs(5);
            }
        }
        let msg = s.worker_exited(1, None, now);
        assert!(msg.contains("Check the USB connection"), "{msg}");
        assert!(!s.wanted && !s.recovering());
    }

    #[test]
    fn a_stable_interval_is_per_worker_not_per_session() {
        // Ready for 50 s, stopped and relaunched by the caller (Apply), ready
        // again for 50 s: neither worker proved stable, so nothing is forgiven.
        let mut s = Supervisor::new(true);
        s.start_requested();
        let mut now = t0();
        s.worker_exited(1, None, now);
        now += RECOVERY_DELAY;
        assert_eq!(s.tick_without_worker(now), Action::StartWorker);
        s.worker_ready(now);
        now += Duration::from_secs(50);
        s.worker_ready(now);
        assert_eq!(s.tick_without_worker(now), Action::StartWorker);
        s.worker_ready(now);
        now += Duration::from_secs(50);
        s.worker_ready(now);
        assert_eq!(s.failures(), 1);
    }

    #[test]
    fn unticking_recovery_during_a_user_restart_after_a_finished_recovery_keeps_3d_on() {
        let mut s = Supervisor::new(true);
        s.start_requested();
        let now = t0();
        s.worker_exited(1, None, now);
        assert_eq!(
            s.tick_without_worker(now + RECOVERY_DELAY),
            Action::StartWorker
        );
        // The relaunch became ready: that recovery is over, though its
        // failure still counts until the worker has proved stable.
        s.worker_ready(now + RECOVERY_DELAY);
        assert_eq!(s.failures(), 1);
        // Apply: the caller stops the worker and relaunches it, so for a few
        // seconds none is running. Unticking the checkbox then cancels nothing.
        assert_eq!(s.recover_changed(false, false), None);
        assert!(s.wanted && !s.recover);
        assert_eq!(
            s.tick_without_worker(now + RECOVERY_DELAY),
            Action::StartWorker,
            "the user's restart goes ahead"
        );
    }

    #[test]
    fn a_ready_worker_leaves_no_retry_queued() {
        // Apply during the countdown starts a worker at once; the deadline it
        // overtook must not delay a later restart.
        let mut s = Supervisor::new(true);
        s.start_requested();
        let now = t0();
        s.worker_exited(1, None, now);
        assert!(s.retry_pending());
        s.worker_ready(now + Duration::from_secs(1));
        assert!(!s.retry_pending());
        assert_eq!(
            s.tick_without_worker(now + Duration::from_secs(2)),
            Action::StartWorker
        );
    }

    #[test]
    fn manual_start_stop_and_unretryable_exits_end_a_recovery() {
        let recovering = || {
            let mut s = Supervisor::new(true);
            s.start_requested();
            s.worker_exited(1, None, t0());
            assert!(s.recovering());
            s
        };
        let mut s = recovering();
        s.start_requested();
        assert!(!s.recovering());
        let mut s = recovering();
        s.stop_requested();
        assert!(!s.recovering());
        let mut s = recovering();
        s.start_failed();
        assert!(!s.recovering());
        let mut s = recovering();
        s.worker_exited(exit_code::PREREQUISITES_FAILED, None, t0());
        assert!(!s.recovering());
        // Another renderer in the way is waited for once, then ends it too.
        let mut s = recovering();
        s.worker_exited(exit_code::ALREADY_RUNNING, None, t0());
        assert!(s.recovering());
        assert_eq!(
            s.tick_without_worker(t0() + RECOVERY_DELAY),
            Action::StartWorker
        );
        s.worker_exited(exit_code::ALREADY_RUNNING, None, t0() + RECOVERY_DELAY);
        assert!(!s.recovering());
    }

    #[test]
    fn a_start_that_finds_another_renderer_is_repeated_once() {
        // Controls killed while converting and started again at once: their
        // old worker still holds the engine for a few seconds.
        for recover in [true, false] {
            let mut s = Supervisor::new(recover);
            s.start_requested();
            let now = t0();
            let msg = s.worker_exited(exit_code::ALREADY_RUNNING, None, now);
            assert_eq!(
                msg,
                "Another renderer is already running. Trying once more in 8 seconds."
            );
            assert!(s.wanted && s.retry_pending());
            assert_eq!(s.failures(), 0, "waiting is not a failed attempt");
            assert_eq!(
                s.tick_without_worker(now + RECOVERY_DELAY / 2),
                Action::None
            );
            assert_eq!(
                s.tick_without_worker(now + RECOVERY_DELAY),
                Action::StartWorker
            );
            // Still there: someone else's. Off, with the advice, for good.
            let later = now + RECOVERY_DELAY;
            let msg = s.worker_exited(exit_code::ALREADY_RUNNING, None, later);
            assert!(msg.ends_with("and turn 3D on again."), "{msg}");
            assert!(!s.wanted && !s.retry_pending());
            assert_eq!(s.tick_without_worker(later + RECOVERY_DELAY), Action::None);
            // The user's next start may wait once again.
            s.start_requested();
            s.worker_exited(exit_code::ALREADY_RUNNING, None, later);
            assert!(s.retry_pending());
            // Turning 3D off while waiting cancels the repeat.
            s.stop_requested();
            assert_eq!(s.tick_without_worker(later + RECOVERY_DELAY), Action::None);
        }
    }

    #[test]
    fn a_relaunch_during_the_wait_for_another_renderer_does_not_end_the_wait() {
        // Controls started again while their old worker still holds the
        // engine, and Apply (or a tray mode command) two seconds into the
        // wait: the controls relaunch at once without asking here, and that
        // worker meets the same renderer. It used to count as the repeat,
        // and 3D went off with the advice to close a renderer.
        let mut s = Supervisor::new(true);
        s.start_requested();
        let now = t0();
        s.worker_exited(exit_code::ALREADY_RUNNING, None, now);
        let early = now + Duration::from_secs(2);
        let msg = s.worker_exited(exit_code::ALREADY_RUNNING, None, early);
        assert_eq!(
            msg,
            "Another renderer is already running. Trying once more in 6 seconds."
        );
        assert!(s.wanted && s.retry_pending() && s.recovering());
        assert_eq!(s.failures(), 0);
        // The repeat is made when it was due, not eight seconds after this.
        assert_eq!(
            s.tick_without_worker(now + RECOVERY_DELAY),
            Action::StartWorker
        );
        // And it is still the only one.
        let later = now + RECOVERY_DELAY;
        let msg = s.worker_exited(exit_code::ALREADY_RUNNING, None, later);
        assert!(msg.ends_with("and turn 3D on again."), "{msg}");
        assert!(!s.wanted && !s.retry_pending());
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
            if code == exit_code::ALREADY_RUNNING {
                // The first time is waited out, see the test below.
                s.worker_exited(code, None, now);
                assert_eq!(s.tick_without_worker(now + RECOVERY_DELAY), Action::StartWorker);
            }
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
