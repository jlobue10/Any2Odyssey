//! Named events and mutexes shared between the controls and the worker.

use std::time::{Duration, Instant};

use any2odyssey_core::ipc;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{
    CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS, ERROR_ALREADY_EXISTS, HANDLE,
    WAIT_OBJECT_0,
};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, GetCurrentProcess, OpenEventW, ResetEvent, SetEvent,
    WaitForSingleObject, EVENT_MODIFY_STATE, SYNCHRONIZATION_SYNCHRONIZE,
};

use crate::win::wide;

/// A second, independently owned handle to the same kernel object, so another
/// thread can wait on it without sharing the original's lifetime.
pub fn duplicate_handle(handle: HANDLE) -> windows::core::Result<HANDLE> {
    let mut copy = HANDLE::default();
    // SAFETY: both process handles are the pseudo-handle for this process and
    // `copy` is a valid out-pointer for the call's duration.
    unsafe {
        let me = GetCurrentProcess();
        DuplicateHandle(me, handle, me, &mut copy, 0, false, DUPLICATE_SAME_ACCESS)?;
    }
    Ok(copy)
}

/// Owned handle to a named kernel event.
pub struct Event(HANDLE);

// Kernel handles may be used from any thread.
unsafe impl Send for Event {}
unsafe impl Sync for Event {}

impl Event {
    /// Create a named event that does not exist yet.
    ///
    /// `CreateEventW` opens an existing object of that name and reports
    /// success. Every event this program creates is its own to listen on,
    /// so an object that is already there was made by someone else, with a
    /// reset mode and an access list of their choosing: a program in this
    /// session that got there first would own Quit and Toggle. That is an
    /// error here (`ERROR_ALREADY_EXISTS`), not an event to trust.
    pub fn create(name: &str, manual_reset: bool) -> windows::core::Result<Event> {
        // Keep the name buffer alive past the call and read the thread's
        // last error immediately, as `InstanceGuard::acquire` does.
        let name = wide(name);
        let (h, last_error) = unsafe {
            let h = CreateEventW(None, manual_reset, false, PCWSTR(name.as_ptr()));
            (h, windows::Win32::Foundation::GetLastError())
        };
        let event = Event(h?);
        if last_error == ERROR_ALREADY_EXISTS {
            // Dropping `event` closes our handle to the foreign object.
            return Err(ERROR_ALREADY_EXISTS.to_hresult().into());
        }
        Ok(event)
    }
    /// Open an existing event; `Err` when nobody created it.
    pub fn open(name: &str) -> windows::core::Result<Event> {
        let h = unsafe {
            OpenEventW(
                EVENT_MODIFY_STATE | SYNCHRONIZATION_SYNCHRONIZE,
                false,
                PCWSTR(wide(name).as_ptr()),
            )?
        };
        Ok(Event(h))
    }
    pub fn set(&self) -> windows::core::Result<()> {
        unsafe { SetEvent(self.0) }
    }
    pub fn reset(&self) -> windows::core::Result<()> {
        unsafe { ResetEvent(self.0) }
    }
    /// Non-blocking check. Auto-reset events are consumed by a positive poll.
    pub fn is_signaled(&self) -> bool {
        unsafe { WaitForSingleObject(self.0, 0) == WAIT_OBJECT_0 }
    }
    /// The raw handle, for multi-object waits. Valid while `self` lives.
    pub fn handle(&self) -> HANDLE {
        self.0
    }
    /// A second owned handle to the same event.
    pub fn try_clone(&self) -> windows::core::Result<Event> {
        duplicate_handle(self.0).map(Event)
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// Signal a named auto-reset event if some process created it.
pub fn signal(name: &str) -> bool {
    Event::open(name).and_then(|e| e.set()).is_ok()
}

/// A named mutex used purely as a single-instance guard.
pub struct InstanceGuard(HANDLE);
unsafe impl Send for InstanceGuard {}

impl InstanceGuard {
    /// `Ok(Some(guard))` when this process is the first owner, `Ok(None)` when
    /// another instance already holds the name.
    pub fn acquire(name: &str) -> windows::core::Result<Option<InstanceGuard>> {
        // Keep the name buffer alive past the call and read the thread's last
        // error immediately: dropping the buffer in between would run the
        // allocator before GetLastError, which must see CreateMutexW's value.
        let name = wide(name);
        let (h, last_error) = unsafe {
            let h = CreateMutexW(None, true, PCWSTR(name.as_ptr()));
            (h, windows::Win32::Foundation::GetLastError())
        };
        let h = h?;
        if last_error == ERROR_ALREADY_EXISTS {
            let _ = unsafe { CloseHandle(h) };
            return Ok(None);
        }
        Ok(Some(InstanceGuard(h)))
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

/// The three application-wide events owned by the controls process.
pub struct ControlEvents {
    pub quit: Event,
    pub toggle: Event,
    pub show: Event,
}

/// How long [`ControlEvents::create`] waits for an event name to become
/// free. The caller has just become the only instance, so a name still in
/// use normally belongs to the previous instance, which released the
/// instance mutex a moment before its process (and with it the events) is
/// gone, or to a `--toggle`/`--show`/`--quit` launch or the uninstaller in
/// the microseconds between opening and closing the event.
const CONTROL_EVENT_PATIENCE: Duration = Duration::from_secs(3);

impl ControlEvents {
    /// Create the three events. Call only after acquiring the UI instance
    /// mutex: with that held, nobody else may legitimately own these names.
    ///
    /// `Err` when a name stays taken: some other program created it and
    /// keeps it. The controls then run without these events (requests from
    /// other launches and the installer's quit do not arrive, and the
    /// caller logs why) rather than take Quit and Toggle orders from an
    /// object they do not own.
    pub fn create() -> windows::core::Result<ControlEvents> {
        Self::create_named(
            [ipc::QUIT_EVENT, ipc::TOGGLE_EVENT, ipc::SHOW_EVENT],
            CONTROL_EVENT_PATIENCE,
        )
    }

    /// `names` are the quit, toggle and show events, in that order.
    fn create_named(names: [&str; 3], patience: Duration) -> windows::core::Result<ControlEvents> {
        let deadline = Instant::now() + patience;
        let own = |name: &str| loop {
            match Event::create(name, false) {
                Err(e)
                    if e.code() == ERROR_ALREADY_EXISTS.to_hresult()
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(e) if e.code() == ERROR_ALREADY_EXISTS.to_hresult() => {
                    return Err(windows::core::Error::new(
                        e.code(),
                        format!("{name} already exists and is held by another program"),
                    ));
                }
                other => return other,
            }
        };
        Ok(ControlEvents {
            quit: own(names[0])?,
            toggle: own(names[1])?,
            show: own(names[2])?,
        })
    }
}

/// 32-hex-digit session id for per-worker event names. Not random: it mixes
/// the clock, the PID and a stack address only so that successive workers
/// of one controls process get distinct `Local\` names. Nothing relies on
/// it being unguessable; the namespace is already per session.
pub fn new_session_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let addr = &nanos as *const u128 as usize;
    session_id(nanos, crate::processes::current_pid(), addr)
}

/// Each input lands in the output: the PID is rotated into the high half of
/// the first word, where the sub-second clock bits never reach it, and the
/// address covers the (mostly constant) high clock word.
fn session_id(nanos: u128, pid: u32, addr: usize) -> String {
    format!(
        "{:016x}{:016x}",
        (nanos as u64) ^ (pid as u64).rotate_left(32),
        ((nanos >> 64) as u64) ^ (addr as u64)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Event names of this test run only: never the installed program's.
    fn test_names(tag: &str) -> [String; 3] {
        let id = new_session_id();
        ["quit", "toggle", "show"].map(|kind| format!("Local\\Any2Odyssey-test-{tag}-{kind}-{id}"))
    }

    fn borrowed(names: &[String; 3]) -> [&str; 3] {
        [&names[0], &names[1], &names[2]]
    }

    #[test]
    fn an_event_that_already_exists_is_an_error_not_a_shared_object() {
        let names = test_names("exists");
        let first = Event::create(&names[0], false).unwrap();
        // The same name again: a manual-reset "squat" must not be adopted.
        let second = Event::create(&names[0], true).err().expect("adopted");
        assert_eq!(second.code(), ERROR_ALREADY_EXISTS.to_hresult());
        // The refusal closed its handle and left the first event working.
        assert!(!first.is_signaled());
        first.set().unwrap();
        assert!(first.is_signaled());
        // Once the owner is gone the name is free again.
        drop(first);
        let again = Event::create(&names[0], false).unwrap();
        assert!(!again.is_signaled(), "a new object, not the old state");
    }

    #[test]
    fn control_events_refuse_a_name_someone_else_keeps() {
        let names = test_names("squat");
        // Another program holds the toggle name and never lets go.
        let squatter = Event::create(&names[1], true).unwrap();
        let started = Instant::now();
        let error = ControlEvents::create_named(borrowed(&names), Duration::from_millis(150))
            .err()
            .expect("a squatted name was accepted");
        assert!(started.elapsed() >= Duration::from_millis(150));
        assert_eq!(error.code(), ERROR_ALREADY_EXISTS.to_hresult());
        assert!(error.message().contains(&names[1]), "{}", error.message());
        // Nothing of ours is left on the names that were free.
        drop(squatter);
        let events =
            ControlEvents::create_named(borrowed(&names), Duration::from_millis(150)).unwrap();
        assert!(!events.quit.is_signaled());
        assert!(signal(&names[0]));
        assert!(events.quit.is_signaled());
        assert!(!events.toggle.is_signaled() && !events.show.is_signaled());
    }

    #[test]
    fn control_events_wait_out_a_previous_instance_that_is_still_exiting() {
        let names = test_names("exiting");
        let leaving = Event::create(&names[0], false).unwrap();
        let leaver = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(80));
            drop(leaving);
        });
        let events =
            ControlEvents::create_named(borrowed(&names), Duration::from_secs(30)).unwrap();
        leaver.join().unwrap();
        // Our own fresh, auto-reset event: one signal is one observation.
        assert!(signal(&names[0]));
        assert!(events.quit.is_signaled());
        assert!(!events.quit.is_signaled());
    }

    #[test]
    fn session_id_is_32_hex_digits_and_depends_on_every_input() {
        let id = session_id(1_762_700_000_123_456_789, 4242, 0x7ffd_1234_5678);
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(
            session_id(1_762_700_000_123_456_789, 4242, 0x7ffd_1234_5678),
            session_id(1_762_700_000_123_456_789, 4243, 0x7ffd_1234_5678)
        );
        assert_ne!(
            session_id(1_762_700_000_123_456_789, 4242, 0x7ffd_1234_5678),
            session_id(1_762_700_000_123_456_790, 4242, 0x7ffd_1234_5678)
        );
        assert_ne!(
            session_id(1_762_700_000_123_456_789, 4242, 0x7ffd_1234_5678),
            session_id(1_762_700_000_123_456_789, 4242, 0x7ffd_1234_5679)
        );
        assert_eq!(new_session_id().len(), 32);
    }
}
