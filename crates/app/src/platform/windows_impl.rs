use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use any2odyssey_core::ipc;
use any2odyssey_core::paths;
use any2odyssey_core::prefs::Preferences;
use any2odyssey_core::update::{self, Comparison};
use any2odyssey_windows::ipc::{Event, InstanceGuard};
use any2odyssey_windows::{diagnostics, engine, processes, win, worker};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, IsIconic, SetForegroundWindow, ShowWindow, ASFW_ANY, SW_HIDE,
    SW_RESTORE, SW_SHOW, SW_SHOWMINNOACTIVE,
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
    // The running instance answers a show request with SetForegroundWindow,
    // which Windows honours only for a process that owns the foreground or
    // has been given the right by one that does. This launch (a shortcut, a
    // jump-list task, a terminal) was started by the user and has that
    // right; the instance that has been sitting in the tray does not, and
    // its window would be restored without the keyboard focus, with a
    // flashing taskbar button instead. Pass the right on before signalling.
    // ASFW_ANY because the instance is known here only by its event name;
    // the grant ends with the next user input. Failure changes nothing: the
    // window is still shown, as before.
    if name == ipc::SHOW_EVENT {
        let _ = unsafe { AllowSetForegroundWindow(ASFW_ANY) };
    }
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
    engine::validate().map_err(|e| e.to_string())?;
    worker_matches_controls()
}

/// Refuse to start a worker of another version than these controls; see
/// `worker_version`. A version that cannot be read is logged and allowed.
fn worker_matches_controls() -> Result<(), String> {
    use crate::worker_version::{check, Agreement};
    let installed = any2odyssey_windows::file_version::file_version(&paths::worker_exe());
    match check(installed, update::Version::current()) {
        Agreement::Same => Ok(()),
        Agreement::Unknown => {
            any2odyssey_core::log::Logger::default_location().log(
                "The installed conversion worker's version could not be read; starting without the version check.",
            );
            Ok(())
        }
        Agreement::Mismatch(message) => Err(message),
    }
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
            // The worker resolves a relative ANY2ODYSSEY_DATA_DIR against its
            // own working directory (the engine folder); hand it the folder
            // the controls resolved so both processes share one data dir.
            .env(paths::DATA_DIR_ENV, paths::data_dir())
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

    /// A short-lived system process standing in for the renderer, so unit
    /// tests can drive the result handling without Samsung's engine.
    #[cfg(test)]
    pub fn fake() -> Worker {
        let session = any2odyssey_windows::ipc::new_session_id();
        let (stop_name, restore_name) = ipc::worker_event_names(&session);
        Worker {
            child: Command::new("cmd.exe")
                .args(["/C", "exit 0"])
                .creation_flags(CREATE_NO_WINDOW)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("cmd.exe"),
            stop: Event::create(&stop_name, true).expect("stop event"),
            restore: Event::create(&restore_name, false).expect("restore event"),
            exit_code: None,
        }
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

/// `WS_EX_TOOLWINDOW`: no taskbar button and no Alt+Tab entry.
const WS_EX_TOOLWINDOW_BIT: isize = 0x0000_0080;

pub fn show_window(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    let h = HWND(hwnd as *mut _);
    unsafe {
        // The taskbar re-reads the extended style only across a hide/show,
        // so a window hidden by `hide_window` gets its button back here.
        let style = win::ex_style(h);
        if style & WS_EX_TOOLWINDOW_BIT != 0 {
            let _ = ShowWindow(h, SW_HIDE);
            win::set_ex_style(h, style & !WS_EX_TOOLWINDOW_BIT);
        }
        let _ = ShowWindow(h, SW_SHOW);
        // Restore only a minimized window. `SW_RESTORE` also un-maximizes a
        // maximized one, so "show controls" on a visible maximized window
        // shrank it; and because both the poller and the frame call this
        // for one request, a window hidden while maximized came back
        // maximized on the first call and was un-maximized by the second.
        // A window hidden by `hide_window` is minimized, so it is iconic
        // here and is restored to the state it was hidden in.
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        let _ = SetForegroundWindow(h);
    }
}

/// Take the controls off the screen, the taskbar and Alt+Tab while keeping
/// them a *visible* window for Win32: eframe runs a frame only from
/// `WM_PAINT`, which a window hidden with `SW_HIDE` never receives, so with
/// `SW_HIDE` the tray menu, a hotkey toggle and above all the deferred Quit
/// would wait for a frame that never came (0.8.4 hung on Quit from the tray).
/// A minimized tool window gets its paint messages and shows nothing.
pub fn hide_window(hwnd: isize) {
    if hwnd == 0 {
        return;
    }
    let h = HWND(hwnd as *mut _);
    unsafe {
        let _ = ShowWindow(h, SW_HIDE);
        win::set_ex_style(h, win::ex_style(h) | WS_EX_TOOLWINDOW_BIT);
        let _ = ShowWindow(h, SW_SHOWMINNOACTIVE);
    }
}

pub fn open_path(path: &Path) {
    // `open` waits up to 250 ms for its launcher; callers hold the shared
    // state or the GUI frame, so do that off their thread.
    let path = path.to_path_buf();
    std::thread::spawn(move || {
        let _ = open::that_detached(path);
    });
}

/// The stereo viewer page is the home page only where Samsung's engine does
/// not exist; on Windows it is not reachable, and these keep it compiling.
pub fn pick_image() -> Result<Option<std::path::PathBuf>, String> {
    Err(NO_VIEWER.into())
}
pub fn pick_save_png(_: &str) -> Result<Option<std::path::PathBuf>, String> {
    Err(NO_VIEWER.into())
}
pub fn take_open_requests() -> Vec<std::path::PathBuf> {
    Vec::new()
}
const NO_VIEWER: &str = "The stereo viewer is part of the Linux build.";

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

/// Ask GitHub for the newest published release and relate it to this build.
/// Blocking: callers run it off the GUI thread.
pub fn check_updates() -> Result<Comparison, String> {
    let body = any2odyssey_windows::update::fetch(&update::Request::latest_release())?;
    update::evaluate_latest_release(&body).map_err(|e| e.to_string())
}
