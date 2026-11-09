//! Process queries: is a named process running, is the parent still alive.

use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_TIMEOUT};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
};

pub fn current_pid() -> u32 {
    unsafe { GetCurrentProcessId() }
}

/// Names (without `.exe`, case-insensitive) of all running processes.
pub fn running_process_names() -> Vec<String> {
    let mut names = Vec::new();
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return names;
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    if unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok() {
        loop {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
            names.push(
                name.trim_end_matches(".exe")
                    .trim_end_matches(".EXE")
                    .to_owned(),
            );
            if unsafe { Process32NextW(snapshot, &mut entry) }.is_err() {
                break;
            }
        }
    }
    let _ = unsafe { CloseHandle(snapshot) };
    names
}

pub fn is_process_running(name: &str) -> bool {
    running_process_names()
        .iter()
        .any(|n| n.eq_ignore_ascii_case(name))
}

/// A `SYNCHRONIZE` handle to another process, opened once and held open.
///
/// Holding the handle keeps the kernel process object alive after the process
/// exits, so its PID cannot be recycled to an unrelated process while this
/// watch exists. Polling `OpenProcess(pid)` instead would report a reused PID
/// as "still running".
pub struct ParentWatch(HANDLE);

// Kernel handles may be used from any thread.
unsafe impl Send for ParentWatch {}
unsafe impl Sync for ParentWatch {}

impl ParentWatch {
    /// Open `pid` for synchronization. `None` when the process cannot be
    /// opened (already gone, PID 0, or access denied): callers treat that as
    /// an absent parent.
    pub fn open(pid: u32) -> Option<ParentWatch> {
        if pid == 0 {
            return None;
        }
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }.ok()?;
        Some(ParentWatch(handle))
    }

    /// True while the watched process has not exited.
    pub fn is_alive(&self) -> bool {
        let state = unsafe { WaitForSingleObject(self.0, 0) };
        state == WAIT_TIMEOUT
    }

    /// The raw handle, for multi-object waits. Valid while `self` lives.
    pub fn handle(&self) -> HANDLE {
        self.0
    }

    /// A second owned handle to the same process object.
    pub fn try_clone(&self) -> windows::core::Result<ParentWatch> {
        crate::ipc::duplicate_handle(self.0).map(ParentWatch)
    }
}

impl Drop for ParentWatch {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_follows_a_real_process_and_rejects_pid_zero() {
        assert!(ParentWatch::open(0).is_none());
        let watch = ParentWatch::open(current_pid()).expect("own process opens");
        assert!(watch.is_alive());
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "exit 0"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn cmd");
        let watch = ParentWatch::open(child.id()).expect("child opens");
        let copy = watch.try_clone().expect("duplicates");
        assert_ne!(copy.handle(), watch.handle());
        child.wait().expect("child exits");
        assert!(!watch.is_alive());
        drop(watch);
        // The duplicate outlives the original and still sees the exit.
        assert!(!copy.is_alive());
    }
}
