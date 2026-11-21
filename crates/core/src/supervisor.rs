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
    /// When that repeat is due, for as long as it is still to come. Kept
    /// apart from `retry_after`, which a recovery attempt sets as well, and
    /// from `recovering`: the wait is made with recovery off too, and the
    /// recovery checkbox has nothing of it to cancel.
    repeat_after: Option<Instant>,
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
            repeat_after: None,
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
    /// The one wait for another renderer is not one.
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
        self.repeat_after = None;
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
        self.repeat_after = None;
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
        //
        // Whether the repeat is still to come was read off `retry_after`,
        // which a queued recovery attempt sets too. With the repeat made
        // and its worker ready, a later failure queued a recovery, and a
        // relaunch during that countdown which met another renderer was
        // given a second wait. And the wait set `recovering`, so that
        // unticking the recovery checkbox during it turned 3D off with
        // "Automatic recovery cancelled", with recovery off all along.
        if exit_code == exit_code::ALREADY_RUNNING
            && self.wanted
            && (!self.waited_for_other || self.repeat_after.is_some())
        {
            let at = self.repeat_after.unwrap_or(now + RECOVERY_DELAY);
            self.waited_for_other = true;
            self.repeat_after = Some(at);
            self.retry_after = Some(at);
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
                // Whichever start was queued, it is made now: the repeat
                // for another renderer is no longer to come.
                self.retry_after = None;
                self.repeat_after = None;
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
    /// tray's mode commands) is not a recovery and is left alone. Nor is the
    /// one wait for another renderer, which goes on. Returns a status line
    /// when something user-visible changed.
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
        assert!(s.wanted && s.retry_pending() && !s.recovering());
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
    fn the_recovery_checkbox_leaves_the_wait_for_another_renderer_alone() {
        // The wait is made with recovery off too, so it is no recovery to
        // cancel. Unticking the checkbox during it used to turn 3D off with
        // "Automatic recovery cancelled", and so did ticking and unticking
        // it where recovery had been off from the start.
        for recover in [true, false] {
            let mut s = Supervisor::new(recover);
            s.start_requested();
            let now = t0();
            s.worker_exited(exit_code::ALREADY_RUNNING, None, now);
            assert!(!s.recovering(), "waiting is not recovering");
            assert_eq!(s.recover_changed(true, false), None);
            assert_eq!(s.recover_changed(false, false), None);
            assert!(s.wanted && s.retry_pending() && !s.recover);
            assert_eq!(
                s.tick_without_worker(now + RECOVERY_DELAY),
                Action::StartWorker,
                "the repeat is made when it was due"
            );
            // While the repeat has not reported in, there is as little to
            // cancel.
            assert_eq!(s.recover_changed(false, false), None);
            assert!(s.wanted);
        }
        // A recovery that meets another renderer is still a recovery, and
        // the checkbox still cancels it.
        let mut s = Supervisor::new(true);
        s.start_requested();
        let now = t0();
        s.worker_exited(1, None, now);
        s.worker_exited(exit_code::ALREADY_RUNNING, None, now);
        assert!(s.recovering() && s.retry_pending());
        assert!(s.recover_changed(false, false).is_some());
        assert!(!s.wanted && !s.retry_pending());
        assert_eq!(s.tick_without_worker(now + RECOVERY_DELAY), Action::None);
    }

    #[test]
    fn a_queued_recovery_is_not_taken_for_the_repeat_still_to_come() {
        // One wait for another renderer per start the user asked for. This
        // start has had its wait, and the repeat worked. A later failure
        // queues a recovery; Apply during that countdown relaunches at
        // once, and that worker meets a renderer. It used to be told
        // "Trying once more", the queued recovery passing for the repeat.
        let mut s = Supervisor::new(true);
        s.start_requested();
        let mut now = t0();
        s.worker_exited(exit_code::ALREADY_RUNNING, None, now);
        now += RECOVERY_DELAY;
        assert_eq!(s.tick_without_worker(now), Action::StartWorker);
        s.worker_ready(now);
        now += Duration::from_secs(20);
        let msg = s.worker_exited(1, None, now);
        assert!(msg.contains("Recovery attempt 1 in 8 seconds"), "{msg}");
        now += Duration::from_secs(1);
        let msg = s.worker_exited(exit_code::ALREADY_RUNNING, None, now);
        assert_eq!(
            msg,
            "Another renderer is already running. Close it, or Samsung's 3D player, and turn 3D on again."
        );
        assert!(!s.wanted && !s.retry_pending() && !s.recovering());
        assert_eq!(s.tick_without_worker(now + RECOVERY_DELAY), Action::None);
        // The same where the repeat itself failed for another reason and
        // its recovery is what is queued.
        let mut s = Supervisor::new(true);
        s.start_requested();
        let mut now = t0();
        s.worker_exited(exit_code::ALREADY_RUNNING, None, now);
        now += RECOVERY_DELAY;
        assert_eq!(s.tick_without_worker(now), Action::StartWorker);
        s.worker_exited(1, None, now);
        let msg = s.worker_exited(exit_code::ALREADY_RUNNING, None, now);
        assert!(msg.ends_with("and turn 3D on again."), "{msg}");
        assert!(!s.wanted);
        // A relaunch that becomes ready during the wait ends the wait: the
        // other renderer is gone, and one met later is not waited for.
        let mut s = Supervisor::new(true);
        s.start_requested();
        let now = t0();
        s.worker_exited(exit_code::ALREADY_RUNNING, None, now);
        s.worker_ready(now + Duration::from_secs(2));
        assert!(!s.retry_pending());
        let msg = s.worker_exited(exit_code::ALREADY_RUNNING, None, now + RECOVERY_DELAY);
        assert!(msg.ends_with("and turn 3D on again."), "{msg}");
    }

    /// Every order of events the controls can bring about, against the rule
    /// as the guide and the release notes state it. The tests above each
    /// walk one order that once went wrong; the two corrections of 1.0.1
    /// were made in one evening by reading, and this walks all the others.
    ///
    /// `World` keeps its own account of the rule from what the supervisor
    /// is told and what it says, never from its fields:
    ///
    /// - a start the user asked for that meets another renderer waits
    ///   eight seconds and is made once more, with recovery on or off, and
    ///   only once: a renderer met after that turns 3D off with the advice;
    /// - a start made while that wait is still to end (Apply) and meeting
    ///   the renderer again goes on waiting, to the same moment;
    /// - any other unexpected exit is recovered after eight seconds, three
    ///   times in a row at most, and only with recovery on;
    /// - unticking recovery turns 3D off only while a recovery is under
    ///   way (queued, or relaunched and not ready yet) and no worker runs;
    /// - whatever happens, a wanted conversion without a worker gets one
    ///   within eight seconds, and one that keeps failing ends.
    ///
    /// Time is counted in whole seconds; two states that differ only in
    /// how long ago something was are one, so the walk comes to an end.
    #[test]
    fn every_order_of_events_keeps_the_rule() {
        use std::collections::{HashSet, VecDeque};

        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
        enum Worker {
            None,
            /// Being validated and spawned: not running yet.
            Launching,
            Running,
        }

        #[derive(Clone, Copy, Debug)]
        enum Event {
            Start,
            Restart,
            Stop,
            Apply,
            Launched,
            LaunchFailed,
            Ready,
            Exit(i32),
            Recover(bool),
            Pass(u64),
        }

        #[derive(Clone, Debug)]
        struct World {
            base: Instant,
            s: Supervisor,
            now: u64,
            worker: Worker,
            wanted: bool,
            recover: bool,
            failures: u32,
            recovering: bool,
            recovery_due: Option<u64>,
            wait_used: bool,
            wait_due: Option<u64>,
            ready_since: Option<u64>,
            waits: u32,
        }

        type Key = (
            (
                Worker,
                bool,
                bool,
                u32,
                bool,
                Option<i64>,
                bool,
                Option<i64>,
            ),
            (Option<u64>, u32),
            (
                bool,
                bool,
                u32,
                Option<i64>,
                bool,
                Option<u64>,
                bool,
                Option<i64>,
            ),
        );

        impl World {
            fn at(&self, seconds: u64) -> Instant {
                self.base + Duration::from_secs(seconds)
            }
            fn off(&mut self) {
                self.wanted = false;
                self.recovering = false;
                self.recovery_due = None;
                self.wait_due = None;
                self.ready_since = None;
            }
            fn dues(&self) -> Vec<u64> {
                [self.wait_due, self.recovery_due]
                    .into_iter()
                    .flatten()
                    .collect()
            }
            fn allowed(&self, event: Event) -> bool {
                match event {
                    Event::Start => !self.wanted,
                    Event::Restart | Event::Stop | Event::Apply => self.wanted,
                    Event::Launched | Event::LaunchFailed => self.worker == Worker::Launching,
                    Event::Ready | Event::Exit(_) => self.worker == Worker::Running,
                    Event::Recover(enabled) => enabled != self.recover,
                    Event::Pass(_) => true,
                }
            }
            /// One event, and the poll that follows it.
            fn step(&mut self, event: Event) -> Result<(), String> {
                let now = self.at(self.now);
                match event {
                    Event::Start | Event::Restart => {
                        if self.s.start_requested() != Action::StartWorker {
                            return Err("a start the user asked for started nothing".into());
                        }
                        self.off();
                        self.wanted = true;
                        self.failures = 0;
                        self.wait_used = false;
                        self.waits = 0;
                        self.worker = Worker::Launching;
                    }
                    Event::Stop => {
                        self.s.stop_requested();
                        self.off();
                        self.worker = Worker::None;
                    }
                    Event::Apply => {
                        // With no worker one is launched at once, without
                        // asking the supervisor. A worker there is stopped,
                        // and the poll below starts the next.
                        self.worker = match self.worker {
                            Worker::None => Worker::Launching,
                            _ => Worker::None,
                        };
                        self.ready_since = None;
                    }
                    Event::Launched => self.worker = Worker::Running,
                    Event::LaunchFailed => {
                        self.s.start_failed();
                        self.off();
                        self.worker = Worker::None;
                    }
                    Event::Ready => {
                        self.s.worker_ready(now);
                        self.recovering = false;
                        self.recovery_due = None;
                        self.wait_due = None;
                        let since = *self.ready_since.get_or_insert(self.now);
                        if self.now - since >= RECOVERY_RESET_AFTER.as_secs() {
                            self.failures = 0;
                        }
                    }
                    Event::Exit(code) => {
                        let said = self.s.worker_exited(code, None, now);
                        self.worker = Worker::None;
                        self.ready_since = None;
                        let expected = if code == exit_code::ALREADY_RUNNING {
                            if let Some(due) = self.wait_due {
                                // The wait goes on, to the moment it had.
                                format!(
                                    "Another renderer is already running. Trying once more in {} seconds.",
                                    due.saturating_sub(self.now).max(1)
                                )
                            } else if !self.wait_used {
                                self.wait_used = true;
                                self.wait_due = Some(self.now + RECOVERY_DELAY.as_secs());
                                self.waits += 1;
                                "Another renderer is already running. Trying once more in 8 seconds."
                                    .to_owned()
                            } else {
                                self.off();
                                "Another renderer is already running. Close it, or Samsung's 3D player, and turn 3D on again.".to_owned()
                            }
                        } else if code == exit_code::PREREQUISITES_FAILED {
                            self.off();
                            "A prerequisite check failed. Fix the cause and turn 3D on again."
                                .to_owned()
                        } else {
                            let what = match code {
                                exit_code::READY_AND_STOPPED_CLEANLY => "The 3D engine closed",
                                exit_code::NEVER_BECAME_READY => "Conversion did not become ready",
                                _ => "Conversion stopped",
                            };
                            if self.recover && self.failures < MAX_RECOVERY_ATTEMPTS {
                                self.failures += 1;
                                self.recovering = true;
                                self.recovery_due = Some(self.now + RECOVERY_DELAY.as_secs());
                                format!("{what}. Recovery attempt {} in 8 seconds.", self.failures)
                            } else {
                                self.off();
                                format!("{what}. Check the USB connection and logs, then turn 3D on again.")
                            }
                        };
                        if said != expected {
                            return Err(format!("said {said:?} where the rule says {expected:?}"));
                        }
                    }
                    Event::Recover(enabled) => {
                        let running = self.worker == Worker::Running;
                        let said = self.s.recover_changed(enabled, running);
                        self.recover = enabled;
                        let cancels = !enabled && self.wanted && !running && self.recovering;
                        if said.is_some() != cancels {
                            return Err(format!(
                                "unticking said {said:?}; a recovery to cancel: {cancels}"
                            ));
                        }
                        if cancels {
                            // A relaunch in flight is stopped by the caller
                            // when it reports in.
                            self.off();
                            self.worker = Worker::None;
                        }
                    }
                    Event::Pass(seconds) => self.now += seconds,
                }
                // The poll: with no worker, the supervisor is asked.
                if self.worker == Worker::None {
                    let action = self.s.tick_without_worker(self.at(self.now));
                    self.ready_since = None;
                    let dues = self.dues();
                    let earliest = dues.iter().min().copied();
                    let latest = dues.iter().max().copied();
                    let (may, must) = match (self.wanted, earliest, latest) {
                        (false, ..) => (false, false),
                        // Apply stopped the worker: the next comes at once.
                        (true, None, _) | (true, _, None) => (true, true),
                        (true, Some(earliest), Some(latest)) => {
                            (self.now >= earliest, self.now >= latest)
                        }
                    };
                    let started = action == Action::StartWorker;
                    if (started && !may) || (!started && must) {
                        return Err(format!(
                            "the poll at {} answered {action:?}; due: {dues:?}, wanted: {}",
                            self.now, self.wanted
                        ));
                    }
                    if started {
                        self.worker = Worker::Launching;
                        self.wait_due = None;
                        self.recovery_due = None;
                    }
                }
                let pending = self.wanted && !self.dues().is_empty();
                let same = self.s.wanted == self.wanted
                    && self.s.recover == self.recover
                    && self.s.failures() == self.failures
                    && self.s.recovering() == self.recovering
                    && self.s.retry_pending() == pending;
                if !same {
                    return Err(format!("the supervisor and the rule are apart: {self:?}"));
                }
                if self.waits > 1 {
                    return Err("a second wait for another renderer in one start".into());
                }
                if self.failures > MAX_RECOVERY_ATTEMPTS {
                    return Err("more recovery attempts than three in a row".into());
                }
                if self.wanted && self.worker == Worker::None {
                    let latest = self.dues().into_iter().max();
                    if !latest.is_some_and(|due| due <= self.now + RECOVERY_DELAY.as_secs()) {
                        return Err(format!("wanted, no worker, and none to come: {self:?}"));
                    }
                }
                Ok(())
            }
            fn key(&self) -> Key {
                let ahead =
                    |due: Option<u64>| due.map(|due| (due as i64 - self.now as i64).max(-1));
                let since = |then: Option<u64>| {
                    then.map(|then| (self.now - then).min(RECOVERY_RESET_AFTER.as_secs()))
                };
                let instant_ahead = |due: Option<Instant>| {
                    let now = self.at(self.now);
                    due.map(|due| {
                        if due >= now {
                            due.duration_since(now).as_secs() as i64
                        } else {
                            -1
                        }
                    })
                };
                let instant_since = |then: Option<Instant>| {
                    then.map(|then| {
                        self.at(self.now)
                            .saturating_duration_since(then)
                            .as_secs()
                            .min(RECOVERY_RESET_AFTER.as_secs())
                    })
                };
                (
                    (
                        self.worker,
                        self.wanted,
                        self.recover,
                        self.failures,
                        self.recovering,
                        ahead(self.recovery_due),
                        self.wait_used,
                        ahead(self.wait_due),
                    ),
                    (since(self.ready_since), self.waits),
                    (
                        self.s.wanted,
                        self.s.recover,
                        self.s.failures,
                        instant_ahead(self.s.retry_after),
                        self.s.recovering,
                        instant_since(self.s.ready_since),
                        self.s.waited_for_other,
                        instant_ahead(self.s.repeat_after),
                    ),
                )
            }
            /// Left to itself with every start failing as `exits` say, in
            /// turn: 3D is off after a wait and three recoveries at most.
            fn ends(&self, exits: &[i32]) -> Result<(), String> {
                let mut world = self.clone();
                let mut exits = exits.iter().cycle();
                for _ in 0..40 {
                    if !world.wanted {
                        return Ok(());
                    }
                    let event = match world.worker {
                        Worker::None => Event::Pass(RECOVERY_DELAY.as_secs()),
                        Worker::Launching => Event::Launched,
                        Worker::Running => Event::Exit(*exits.next().unwrap()),
                    };
                    world.step(event)?;
                }
                Err(format!("still wanted after forty steps of exits {exits:?}"))
            }
        }

        let events = [
            Event::Start,
            Event::Restart,
            Event::Stop,
            Event::Apply,
            Event::Launched,
            Event::LaunchFailed,
            Event::Ready,
            Event::Exit(exit_code::READY_AND_STOPPED_CLEANLY),
            Event::Exit(exit_code::RENDERER_ERROR),
            Event::Exit(exit_code::ALREADY_RUNNING),
            Event::Exit(exit_code::PREREQUISITES_FAILED),
            Event::Exit(exit_code::NEVER_BECAME_READY),
            Event::Recover(true),
            Event::Recover(false),
            Event::Pass(1),
            Event::Pass(3),
            Event::Pass(8),
            Event::Pass(60),
        ];
        let endings: [&[i32]; 4] = [
            &[exit_code::ALREADY_RUNNING],
            &[exit_code::RENDERER_ERROR],
            &[exit_code::ALREADY_RUNNING, exit_code::RENDERER_ERROR],
            &[exit_code::RENDERER_ERROR, exit_code::ALREADY_RUNNING],
        ];
        let base = Instant::now();
        let mut seen = HashSet::new();
        let mut queue = VecDeque::new();
        for recover in [true, false] {
            let world = World {
                base,
                s: Supervisor::new(recover),
                now: 0,
                worker: Worker::None,
                wanted: false,
                recover,
                failures: 0,
                recovering: false,
                recovery_due: None,
                wait_used: false,
                wait_due: None,
                ready_since: None,
                waits: 0,
            };
            seen.insert(world.key());
            queue.push_back((world, Vec::new()));
        }
        while let Some((world, trail)) = queue.pop_front() {
            for exits in endings {
                if let Err(what) = world.ends(exits) {
                    panic!("after {trail:?}, then exits {exits:?}: {what}");
                }
            }
            for event in events {
                if !world.allowed(event) {
                    continue;
                }
                let mut next = world.clone();
                let mut trail = trail.clone();
                trail.push(event);
                if let Err(what) = next.step(event) {
                    panic!("after {trail:?}: {what}");
                }
                if seen.insert(next.key()) {
                    queue.push_back((next, trail));
                }
            }
        }
        // The walk has been somewhere: every kind of state was reached.
        assert!(seen.len() > 1000, "{} states", seen.len());
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
