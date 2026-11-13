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
use windows::Win32::Foundation::{HANDLE, HWND, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::System::Threading::WaitForSingleObject;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, IsIconic, SetForegroundWindow, ShowWindow, ASFW_ANY, SW_HIDE,
    SW_RESTORE, SW_SHOW, SW_SHOWMINNOACTIVE,
};

pub use any2odyssey_windows::ipc::ControlEvents;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// How long a signalled worker gets to stop before it is killed. The
/// worker's own watchdog uses the same grace (`WATCHDOG_GRACE`).
const STOP_GRACE: Duration = Duration::from_secs(7);

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
        if self.wait_for_exit(STOP_GRACE) {
            return None;
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        Some("Renderer did not stop in 7 seconds; ending only the owned renderer.".into())
    }

    /// Block until the process has exited or `timeout` has passed; true
    /// when it exited. Waits on the process handle, so the caller returns
    /// the moment the worker is gone instead of at the next 50 ms look.
    fn wait_for_exit(&mut self, timeout: Duration) -> bool {
        use std::os::windows::io::AsRawHandle;
        if self.has_exited().is_some() {
            return true;
        }
        let deadline = Instant::now() + timeout;
        let handle = HANDLE(self.child.as_raw_handle());
        // SAFETY: the handle belongs to `self.child`, which outlives the
        // call, and a process handle from `Command::spawn` can be waited on.
        let waited = unsafe { WaitForSingleObject(handle, timeout.as_millis() as u32) };
        if waited != WAIT_OBJECT_0 && waited != WAIT_TIMEOUT {
            // The wait itself failed. Fall back to looking at intervals, so
            // a worker that is stopping still gets its full grace.
            while self.has_exited().is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        self.has_exited().is_some()
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
    let path = path.to_owned();
    // Whether it is a folder is asked on the launch thread too: the caller
    // holds the shared state inside a GUI frame.
    std::thread::spawn(move || shell_open_or_fall_back(&shell_target(&path, path.is_dir())));
}

/// What the shell is handed to open `path`. A folder is named with a
/// trailing separator. Without one the shell takes the name for a program
/// first and tries the executable extensions on it: asked to open the folder
/// `...\Any2Odyssey`, it runs `...\Any2Odyssey.cmd` (or `.exe`, `.bat`,
/// `.com`) when such a file lies beside the folder, and the folder stays
/// shut. With the separator the name can only be the folder.
fn shell_target(path: &Path, is_folder: bool) -> std::ffi::OsString {
    use std::os::windows::ffi::OsStrExt;
    let mut target = path.as_os_str().to_owned();
    let separated = target
        .encode_wide()
        .last()
        .is_some_and(|last| last == u16::from(b'\\') || last == u16::from(b'/'));
    if is_folder && !separated {
        target.push("\\");
    }
    target
}

/// Open a web page in the default browser. The caller has checked the URL
/// against the allow-list (`core::open_release_url`).
pub fn open_url(url: &str) {
    shell_open_detached(url.into());
}

/// Hand `target` to the shell on a thread of its own: callers hold the
/// shared state or the GUI frame, and a launch can take a moment.
///
/// Through `ShellExecuteEx`, not the `open` crate: without a feature that
/// needs another dependency, `open` 5.4 starts `powershell.exe -Command` to
/// do the same, which takes a second or so and is refused outright where
/// PowerShell is restricted by policy. It stays as the fallback, so a
/// failed shell call still opens what it opened before.
fn shell_open_detached(target: std::ffi::OsString) {
    std::thread::spawn(move || shell_open_or_fall_back(&target));
}

fn shell_open_or_fall_back(target: &std::ffi::OsStr) {
    if win::shell_open(target).is_err() {
        let _ = open::that_detached(target);
    }
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
pub fn estimate_depth(
    _: &image::RgbaImage,
    _: u32,
) -> Result<any2odyssey_core::dibr::DepthMap, String> {
    Err(NO_VIEWER.into())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_running(command: &str) -> Worker {
        let mut worker = Worker::fake();
        let _ = worker.child.wait();
        worker.child = Command::new("cmd.exe")
            .args(["/C", command])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("cmd.exe");
        worker
    }

    #[test]
    fn a_folder_is_handed_to_the_shell_with_a_trailing_separator() {
        // Seen on Windows 11: "open" on `...\probe` ran `...\probe.cmd`
        // lying beside the folder `probe`; on `...\probe\` the folder opened.
        let folder = Path::new(r"C:\Users\someone\AppData\Local\Any2Odyssey");
        assert_eq!(
            shell_target(folder, true),
            r"C:\Users\someone\AppData\Local\Any2Odyssey\"
        );
        // Already unambiguous: nothing is added.
        for separated in [r"C:\data\", "C:/data/", r"C:\"] {
            assert_eq!(shell_target(Path::new(separated), true), separated);
        }
        // A file keeps its name: with a separator it would not be found.
        let file = Path::new(r"C:\data\companion.log");
        assert_eq!(shell_target(file, false), file.as_os_str());
        // The real data folder of a test run is a folder.
        let dir = std::env::temp_dir();
        assert!(shell_target(&dir, dir.is_dir())
            .to_string_lossy()
            .ends_with(['\\', '/']));
    }

    #[test]
    fn waiting_for_a_worker_returns_when_it_exits_and_times_out_when_it_does_not() {
        // Exits by itself after about a second: the wait must end with it,
        // long before the timeout.
        let mut quick = fake_running("ping -n 2 127.0.0.1 >NUL");
        let started = Instant::now();
        assert!(quick.wait_for_exit(Duration::from_secs(60)));
        assert!(started.elapsed() < Duration::from_secs(30));
        assert!(quick.has_exited().is_some());
        assert!(quick.wait_for_exit(Duration::ZERO), "already exited");

        // Still running when the timeout ends.
        let mut slow = fake_running("ping -n 30 127.0.0.1 >NUL");
        let started = Instant::now();
        assert!(!slow.wait_for_exit(Duration::from_millis(200)));
        // A timed wait may end up to one tick of the system clock (15.6 ms)
        // early; it must not return at once.
        assert!(started.elapsed() >= Duration::from_millis(150));
        assert!(slow.has_exited().is_none());
        let _ = slow.child.kill();
        let _ = slow.child.wait();
    }
}
