//! Named events and mutexes shared between the controls and the worker.

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
    /// Create (or open, if it already exists) a named event.
    pub fn create(name: &str, manual_reset: bool) -> windows::core::Result<Event> {
        let h = unsafe { CreateEventW(None, manual_reset, false, PCWSTR(wide(name).as_ptr()))? };
        Ok(Event(h))
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

impl ControlEvents {
    pub fn create() -> windows::core::Result<ControlEvents> {
        Ok(ControlEvents {
            quit: Event::create(ipc::QUIT_EVENT, false)?,
            toggle: Event::create(ipc::TOGGLE_EVENT, false)?,
            show: Event::create(ipc::SHOW_EVENT, false)?,
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
