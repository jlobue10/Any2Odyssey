//! Named events and mutexes shared between the controls and the worker.

use any2odyssey_core::ipc;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, OpenEventW, ResetEvent, SetEvent, WaitForSingleObject,
    EVENT_MODIFY_STATE, SYNCHRONIZATION_SYNCHRONIZE,
};

use crate::win::wide;

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
        let h = unsafe { CreateMutexW(None, true, PCWSTR(wide(name).as_ptr()))? };
        if unsafe { windows::Win32::Foundation::GetLastError() } == ERROR_ALREADY_EXISTS {
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

/// Random 32-hex-digit session id for per-worker event names.
pub fn new_session_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = crate::processes::current_pid() as u128;
    let addr = &nanos as *const u128 as usize as u128;
    format!(
        "{:016x}{:016x}",
        (nanos ^ (pid << 64)) as u64,
        (nanos.rotate_left(64) ^ addr) as u64
    )
}
