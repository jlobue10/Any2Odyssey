//! The worker's status line while a window mode has nothing to show.
//!
//! Selected-window and active-window capture can lose their source at any
//! time (closed, minimized, refused by DWM). Conversion keeps running and
//! picks the source up again, so this is a notice, not a stop reason: it
//! replaces the state in `worker-status.json` while there is no source and
//! puts the previous status back when one returns.
//!
//! The previous status is taken from the file rather than rebuilt, so every
//! field of it comes back as it was written, and it is put back only while
//! the notice is still what the file says: a status written in between
//! (conversion became ready, an error) is newer and stays.

use any2odyssey_core::fsutil::write_json_atomic;
use any2odyssey_core::paths;
use any2odyssey_core::status::WorkerStatus;

use crate::processes::current_pid;

pub const NO_SOURCE_STATUS: &str =
    "No source window to show (closed, minimized or not capturable); waiting for it.";

#[derive(Default)]
pub struct SourceNotice {
    /// The status the notice replaced, while the notice is up.
    replaced: Option<WorkerStatus>,
}

impl SourceNotice {
    /// The surface started (`shown`) or stopped showing a source.
    pub fn source_changed(&mut self, shown: bool) {
        let current = read_own_status();
        let next = if shown {
            self.restore(current.as_ref())
        } else {
            self.raise(current)
        };
        if let Some(mut status) = next {
            status.time = any2odyssey_core::log::now_iso();
            let _ = write_json_atomic(&paths::worker_status_file(), &status);
        }
    }

    /// The worker wrote a status of its own (conversion became ready). While
    /// there is still no source, that status would hide the notice for good:
    /// no further change is raised until a source appears and goes again.
    /// Put the notice back over it; the new status is what returns later.
    pub fn status_written(&mut self) {
        if let Some(mut status) = self.reassert(read_own_status()) {
            status.time = any2odyssey_core::log::now_iso();
            let _ = write_json_atomic(&paths::worker_status_file(), &status);
        }
    }

    /// The notice to write over a newer `current` while the notice is up.
    fn reassert(&mut self, current: Option<WorkerStatus>) -> Option<WorkerStatus> {
        self.replaced.as_ref()?;
        self.raise(current)
    }

    /// The notice to write over `current`, which is remembered. Nothing when
    /// this worker has not written a status yet.
    fn raise(&mut self, current: Option<WorkerStatus>) -> Option<WorkerStatus> {
        let current = current.filter(|status| status.state != NO_SOURCE_STATUS)?;
        let notice = WorkerStatus {
            state: NO_SOURCE_STATUS.to_owned(),
            ..current.clone()
        };
        self.replaced = Some(current);
        Some(notice)
    }

    /// The remembered status, if the notice is still the file's state.
    fn restore(&mut self, current: Option<&WorkerStatus>) -> Option<WorkerStatus> {
        let replaced = self.replaced.take()?;
        (current?.state == NO_SOURCE_STATUS).then_some(replaced)
    }
}

/// The status file's content when this process wrote it.
fn read_own_status() -> Option<WorkerStatus> {
    let text = std::fs::read_to_string(paths::worker_status_file()).ok()?;
    let status: WorkerStatus = serde_json::from_str(&text).ok()?;
    (status.pid == current_pid()).then_some(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Through the constructor, not a struct literal, so a field added to the
    // status later does not have to be repeated here.
    fn status(state: &str) -> WorkerStatus {
        WorkerStatus::now(state, 7)
    }

    #[test]
    fn notice_replaces_the_state_and_the_previous_status_comes_back() {
        let mut notice = SourceNotice::default();
        let active = status("3D active");
        let raised = notice.raise(Some(active.clone())).unwrap();
        assert_eq!(raised.state, NO_SOURCE_STATUS);
        assert_eq!(raised.pid, 7);
        assert_eq!(notice.restore(Some(&raised)), Some(active));
        // Handed back once; a second "source returned" writes nothing.
        assert_eq!(notice.restore(Some(&raised)), None);
    }

    #[test]
    fn a_status_written_while_the_notice_was_up_is_not_overwritten() {
        let mut notice = SourceNotice::default();
        notice.raise(Some(status("Starting conversion"))).unwrap();
        assert_eq!(notice.restore(Some(&status("3D active"))), None);
        // The stale "Starting conversion" is forgotten, not kept for later.
        assert_eq!(notice.restore(Some(&status(NO_SOURCE_STATUS))), None);
    }

    #[test]
    fn a_status_written_while_there_is_no_source_gets_the_notice_back() {
        let mut notice = SourceNotice::default();
        notice.raise(Some(status("Starting conversion"))).unwrap();
        // Conversion became ready with still nothing to show.
        let ready = WorkerStatus::ready_now("3D active", 7);
        let again = notice.reassert(Some(ready.clone())).unwrap();
        assert_eq!(again.state, NO_SOURCE_STATUS);
        // The controls read readiness from the field, not the wording.
        assert!(again.ready);
        // When a source arrives, the ready status comes back, not the stale one.
        assert_eq!(notice.restore(Some(&again)), Some(ready));
    }

    #[test]
    fn a_new_status_is_left_alone_when_no_notice_is_up() {
        let mut notice = SourceNotice::default();
        assert_eq!(notice.reassert(Some(status("3D active"))), None);
        // A notice that was restored is no longer up either.
        let raised = notice.raise(Some(status("Starting conversion"))).unwrap();
        notice.restore(Some(&raised)).unwrap();
        assert_eq!(notice.reassert(Some(status("3D active"))), None);
    }

    #[test]
    fn nothing_is_written_without_a_status_of_this_worker() {
        let mut notice = SourceNotice::default();
        assert_eq!(notice.raise(None), None);
        assert_eq!(notice.restore(None), None);
        // A notice that is already up is never remembered as "previous".
        assert_eq!(notice.raise(Some(status(NO_SOURCE_STATUS))), None);
        assert_eq!(notice.restore(Some(&status(NO_SOURCE_STATUS))), None);
    }
}
