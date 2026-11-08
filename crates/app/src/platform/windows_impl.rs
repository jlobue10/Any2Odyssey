use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use any2odyssey_core::ipc;
use any2odyssey_core::paths;
use any2odyssey_core::prefs::Preferences;
use any2odyssey_windows::ipc::{Event, InstanceGuard};
use any2odyssey_windows::{diagnostics, engine, processes, win, worker};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    SetForegroundWindow, ShowWindow, SW_HIDE, SW_RESTORE, SW_SHOW,
};

pub use any2odyssey_windows::ipc::ControlEvents;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn run_worker(event: &str, parent: u32, source: isize, preferences: Preferences) -> i32 {
    worker::run(event, parent, source, preferences)
}
pub fn inspect(pid: u32) -> i32 {
    match diagnostics::write_inspect(pid) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}
pub fn layout_probe(parent: u32, worker: u32) -> i32 {
    match diagnostics::write_layout_probe(parent, worker) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}
pub fn test_capture_exclusion() -> i32 {
    diagnostics::test_capture_exclusion()
}
pub fn test_offscreen() -> i32 {
    diagnostics::test_offscreen()
}

pub fn signal(name: &str) -> bool {
    any2odyssey_windows::ipc::signal(name)
}

pub struct SingleInstance(#[allow(dead_code)] InstanceGuard);

pub fn single_instance() -> Option<SingleInstance> {
    InstanceGuard::acquire(ipc::UI_MUTEX)
        .ok()
        .flatten()
        .map(SingleInstance)
}

pub fn validate() -> Result<(), String> {
    engine::validate().map_err(|e| e.to_string())
}

pub fn list_windows(worker_pid: Option<u32>) -> Vec<(isize, String)> {
    let mut exclude = vec![processes::current_pid()];
    exclude.extend(worker_pid);
    win::candidate_windows(&exclude)
        .into_iter()
        .filter_map(|h| {
            let title = win::title(h);
            (!title.is_empty() && title != "Program Manager").then_some((h.0 as isize, title))
        })
        .collect()
}

pub struct Worker {
    child: Child,
    stop: Event,
    restore: Event,
    exit_code: Option<i32>,
}

impl Worker {
    pub fn spawn(source_hwnd: isize, preferences: Preferences) -> Result<Worker, String> {
        // This immutable launch snapshot is independent of later file writes,
        // read failures, or settings changed while validation is in flight.
        let snapshot = serde_json::to_string(&preferences).map_err(|e| e.to_string())?;
        let session = any2odyssey_windows::ipc::new_session_id();
        let (stop_name, restore_name) = ipc::worker_event_names(&session);
        let stop = Event::create(&stop_name, true).map_err(|e| e.to_string())?;
        let restore = Event::create(&restore_name, false).map_err(|e| e.to_string())?;
        let child = Command::new(paths::worker_exe())
            .args([
                "--worker",
                &stop_name,
                &processes::current_pid().to_string(),
                &source_hwnd.to_string(),
                &snapshot,
            ])
            .current_dir(paths::engine_dir())
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(Worker {
            child,
            stop,
            restore,
            exit_code: None,
        })
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// `Some(code)` once the process has exited.
    pub fn has_exited(&mut self) -> Option<i32> {
        if let Some(code) = self.exit_code {
            return Some(code);
        }
        match self.child.try_wait() {
            Ok(Some(status)) => {
                let code = status.code().unwrap_or(-1);
                self.exit_code = Some(code);
                Some(code)
            }
            Ok(None) => None,
            Err(_) => {
                self.exit_code = Some(-1);
                Some(-1)
            }
        }
    }

    pub fn restore(&self) {
        let _ = self.restore.set();
    }

    /// Signal the stop event and wait up to 7 s; kill only our own worker.
    /// Returns a log line when the worker had to be killed.
    pub fn stop(mut self) -> Option<String> {
        let _ = self.stop.set();
        let deadline = Instant::now() + Duration::from_secs(7);
        while Instant::now() < deadline {
            if self.has_exited().is_some() {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        Some("Renderer did not stop in 7 seconds; ending only the owned renderer.".into())
    }
}

pub fn show_window(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    let h = HWND(hwnd as *mut _);
    unsafe {
        let _ = ShowWindow(h, SW_SHOW);
        let _ = ShowWindow(h, SW_RESTORE);
        let _ = SetForegroundWindow(h);
    }
}

pub fn hide_window(hwnd: isize) {
    if hwnd != 0 {
        let _ = unsafe { ShowWindow(HWND(hwnd as *mut _), SW_HIDE) };
    }
}

pub fn open_path(path: &Path) {
    let _ = open::that_detached(path);
}

pub fn install_jump_list() {
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if let Ok(exe) = std::env::current_exe() {
        let _ = any2odyssey_windows::taskbar::install_jump_list(
            &exe.to_string_lossy(),
            &any2odyssey_windows::taskbar::default_tasks(),
        );
    }
}
