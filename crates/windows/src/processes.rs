//! Process queries: is a named process running, is a PID still alive.

use windows::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
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

/// True while the process with `pid` has not exited. Unknown PIDs count as dead.
pub fn is_pid_alive(pid: u32) -> bool {
    let Ok(handle) = (unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }) else {
        return false;
    };
    let alive = unsafe { WaitForSingleObject(handle, 0) } == WAIT_TIMEOUT;
    let _ = unsafe { CloseHandle(handle) };
    alive
}
