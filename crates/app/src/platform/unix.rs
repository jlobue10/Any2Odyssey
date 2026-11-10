//! Linux (and other non-Windows) build: the stereo viewer runs here, but 3D
//! conversion needs Samsung's Windows engine. This module provides what the
//! controls need from the OS: one instance per user, the `--show` / `--quit`
//! / `--toggle` / `--open` hand-over to that instance, and file dialogs.
//!
//! The instance guard and the hand-over are one Unix socket: binding it is
//! the guard, and a later launch that finds it answered writes its request
//! there instead of opening a second window.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use any2odyssey_core::prefs::Preferences;
use any2odyssey_core::update::Comparison;
use any2odyssey_core::{ipc, paths};

const SOCKET_NAME: &str = "any2odyssey.sock";
/// A request and its acknowledgement are one short line each; a peer that
/// stalls must not hold the listener or a launching process for long.
const IO_TIMEOUT: Duration = Duration::from_secs(2);

/// Where the instance socket lives: the data folder when one was set
/// explicitly (an isolated run must not find or disturb the real instance),
/// otherwise the per-user runtime folder, otherwise the data folder.
fn socket_path() -> PathBuf {
    socket_path_from(
        std::env::var_os(paths::DATA_DIR_ENV).is_some(),
        std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from),
        paths::data_dir(),
    )
}

fn socket_path_from(data_overridden: bool, runtime: Option<PathBuf>, data: PathBuf) -> PathBuf {
    let dir = match runtime {
        Some(dir) if !data_overridden && dir.is_absolute() => dir,
        _ => data,
    };
    dir.join(SOCKET_NAME)
}

/// A request another launch hands to the running instance.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Request {
    Quit,
    Show,
    Toggle,
    Open(PathBuf),
}

impl Request {
    fn parse(line: &str) -> Option<Request> {
        let line = line.trim_end_matches(['\r', '\n']);
        match line {
            "quit" => Some(Request::Quit),
            "show" => Some(Request::Show),
            "toggle" => Some(Request::Toggle),
            _ => line
                .strip_prefix("open ")
                .filter(|path| !path.is_empty())
                .map(|path| Request::Open(PathBuf::from(path))),
        }
    }

    fn line(&self) -> String {
        match self {
            Request::Quit => "quit\n".into(),
            Request::Show => "show\n".into(),
            Request::Toggle => "toggle\n".into(),
            Request::Open(path) => format!("open {}\n", path.display()),
        }
    }
}

/// The bound socket between [`single_instance`] and [`ControlEvents::create`].
static LISTENER: Mutex<Option<UnixListener>> = Mutex::new(None);
/// Images handed over with `--open`, or named on this launch's command line.
static OPEN_REQUESTS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Queue an image for the viewer to load.
pub fn queue_open(path: PathBuf) {
    OPEN_REQUESTS.lock().expect("open queue").push(path);
}

/// The images queued since the last call, oldest first.
pub fn take_open_requests() -> Vec<PathBuf> {
    std::mem::take(&mut *OPEN_REQUESTS.lock().expect("open queue"))
}

#[derive(Clone, Default)]
pub struct Flag(Arc<AtomicBool>);
impl Flag {
    /// True once per request, like an auto-reset event.
    pub fn is_signaled(&self) -> bool {
        self.0.swap(false, Ordering::AcqRel)
    }
    fn set(&self) {
        self.0.store(true, Ordering::Release);
    }
}

pub struct ControlEvents {
    pub quit: Flag,
    pub show: Flag,
    pub toggle: Flag,
}

impl ControlEvents {
    /// Start answering the instance socket. Without the socket (the guard
    /// could not bind) the controls still run; other launches cannot reach
    /// them.
    pub fn create() -> Result<ControlEvents, String> {
        let events = ControlEvents {
            quit: Flag::default(),
            show: Flag::default(),
            toggle: Flag::default(),
        };
        let Some(listener) = LISTENER.lock().expect("listener").take() else {
            return Err("the instance socket is not available".into());
        };
        let (quit, show, toggle) = (
            events.quit.clone(),
            events.show.clone(),
            events.toggle.clone(),
        );
        std::thread::Builder::new()
            .name("instance-socket".into())
            .spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { continue };
                    match serve(stream) {
                        Some(Request::Quit) => quit.set(),
                        Some(Request::Show) => show.set(),
                        Some(Request::Toggle) => toggle.set(),
                        Some(Request::Open(path)) => {
                            queue_open(path);
                            show.set();
                        }
                        None => {}
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(events)
    }
}

/// Read one request and acknowledge it. A connection that sends nothing (the
/// liveness probe of another launch) is not a request.
fn serve(stream: UnixStream) -> Option<Request> {
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).ok()?;
    let request = Request::parse(&line)?;
    let _ = (&stream).write_all(b"ok\n");
    Some(request)
}

/// Send `request` to the running instance; true when it acknowledged.
fn send(path: &Path, request: &Request) -> bool {
    let Ok(stream) = UnixStream::connect(path) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    if (&stream).write_all(request.line().as_bytes()).is_err() {
        return false;
    }
    let mut reply = String::new();
    BufReader::new(&stream).read_line(&mut reply).is_ok() && reply.trim_end() == "ok"
}

/// Deliver one of the control events (named as on Windows) to the running
/// instance. False when none is running or it did not answer.
pub fn signal(event: &str) -> bool {
    let request = match event {
        ipc::QUIT_EVENT => Request::Quit,
        ipc::SHOW_EVENT => Request::Show,
        ipc::TOGGLE_EVENT => Request::Toggle,
        _ => return false,
    };
    send(&socket_path(), &request)
}

/// Hand an image to the running instance. False when none is running.
pub fn signal_open(path: &Path) -> bool {
    send(&socket_path(), &Request::Open(path.to_path_buf()))
}

pub struct SingleInstance {
    path: PathBuf,
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// `None` when another instance of this user already answers the socket.
pub fn single_instance() -> Option<SingleInstance> {
    let path = socket_path();
    match bind_instance(&path) {
        Bound::Listening(listener) => {
            *LISTENER.lock().expect("listener") = Some(listener);
            Some(SingleInstance { path })
        }
        Bound::Occupied => None,
        // No socket could be created at all (read-only or missing folder):
        // run without a guard rather than refuse to start.
        Bound::Unavailable => Some(SingleInstance { path }),
    }
}

enum Bound {
    Listening(UnixListener),
    Occupied,
    Unavailable,
}

fn bind_instance(path: &Path) -> Bound {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    for _ in 0..2 {
        match UnixListener::bind(path) {
            Ok(listener) => return Bound::Listening(listener),
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                if UnixStream::connect(path).is_ok() {
                    return Bound::Occupied;
                }
                // A socket file nobody answers was left by an instance that
                // did not exit cleanly. Remove it and bind once more.
                if std::fs::remove_file(path).is_err() {
                    return Bound::Unavailable;
                }
            }
            Err(_) => return Bound::Unavailable,
        }
    }
    Bound::Unavailable
}

/// Run the first of `candidates` that is installed and return its trimmed
/// standard output. `None` when the dialog was cancelled or none exists.
fn dialog(candidates: &[(&str, Vec<String>)]) -> Result<Option<PathBuf>, String> {
    for (program, arguments) in candidates {
        match std::process::Command::new(program)
            .args(arguments)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
        {
            Ok(output) => {
                let text = String::from_utf8_lossy(&output.stdout);
                let text = text.trim_end_matches(['\r', '\n']);
                return Ok(
                    (output.status.success() && !text.is_empty()).then(|| PathBuf::from(text))
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(format!("{program}: {e}")),
        }
    }
    Err(NO_DIALOG.into())
}

pub const NO_DIALOG: &str =
    "No file dialog is installed (zenity or kdialog). Type or paste the path, or drop the file on this window.";

/// Ask for an image to open. Blocks until the dialog closes; call it off the
/// UI thread.
pub fn pick_image() -> Result<Option<PathBuf>, String> {
    dialog(&[
        (
            "zenity",
            vec![
                "--file-selection".into(),
                "--title=Open stereo image".into(),
                "--file-filter=Images | *.png *.jpg *.jpeg *.bmp *.PNG *.JPG *.JPEG *.BMP".into(),
                "--file-filter=All files | *".into(),
            ],
        ),
        (
            "kdialog",
            vec![
                "--title".into(),
                "Open stereo image".into(),
                "--getopenfilename".into(),
                ".".into(),
                "Images (*.png *.jpg *.jpeg *.bmp)".into(),
            ],
        ),
    ])
}

/// Ask where to save a PNG, suggesting `name`. Blocks like [`pick_image`].
pub fn pick_save_png(name: &str) -> Result<Option<PathBuf>, String> {
    dialog(&[
        (
            "zenity",
            vec![
                "--file-selection".into(),
                "--save".into(),
                "--confirm-overwrite".into(),
                "--title=Save preview".into(),
                format!("--filename={name}"),
                "--file-filter=PNG | *.png".into(),
            ],
        ),
        (
            "kdialog",
            vec![
                "--title".into(),
                "Save preview".into(),
                "--getsavefilename".into(),
                name.into(),
                "PNG (*.png)".into(),
            ],
        ),
    ])
}

pub fn run_worker(_: &str, _: u32, _: isize, _: Preferences) -> i32 {
    eprintln!("The conversion worker runs only on Windows.");
    1
}
pub fn inspect(_: u32) -> i32 {
    1
}
pub fn layout_probe(_: u32, _: u32) -> i32 {
    1
}
pub fn test_capture_exclusion() -> i32 {
    1
}
pub fn test_offscreen() -> i32 {
    1
}
pub fn validate() -> Result<(), String> {
    Err("3D conversion needs Samsung's Odyssey 3D Hub engine, which exists only on Windows.".into())
}
pub fn list_windows(_: Option<u32>) -> Vec<(isize, String)> {
    Vec::new()
}
pub struct Worker;
impl Worker {
    pub fn spawn(_: isize, _: Preferences) -> Result<Worker, String> {
        Err("3D conversion is only available on Windows.".into())
    }
    #[cfg(test)]
    pub fn fake() -> Worker {
        Worker
    }
    pub fn pid(&self) -> u32 {
        0
    }
    pub fn has_exited(&mut self) -> Option<i32> {
        Some(1)
    }
    pub fn restore(&self) {}
    pub fn stop(self) -> Option<String> {
        None
    }
}
pub fn show_window(_: isize) {}
pub fn hide_window(_: isize) {}
pub fn open_path(path: &Path) {
    let _ = open::that_detached(path);
}
pub fn install_jump_list() {}
pub fn check_updates() -> Result<Comparison, String> {
    Err("Checking for updates is available on Windows only.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_survive_the_wire_and_junk_is_not_a_request() {
        for request in [
            Request::Quit,
            Request::Show,
            Request::Toggle,
            Request::Open(PathBuf::from("/home/user/My Pictures/pair one.png")),
        ] {
            assert_eq!(Request::parse(&request.line()), Some(request));
        }
        for junk in ["", "\n", "open ", "open", "QUIT", "show now", "restart"] {
            assert_eq!(Request::parse(junk), None, "{junk:?}");
        }
    }

    #[test]
    fn an_isolated_run_keeps_its_socket_out_of_the_runtime_folder() {
        let runtime = Some(PathBuf::from("/run/user/1000"));
        let data = PathBuf::from("/home/u/.local/share/any2odyssey");
        assert_eq!(
            socket_path_from(false, runtime.clone(), data.clone()),
            PathBuf::from("/run/user/1000/any2odyssey.sock")
        );
        assert_eq!(
            socket_path_from(true, runtime, data.clone()),
            data.join(SOCKET_NAME)
        );
        assert_eq!(
            socket_path_from(false, None, data.clone()),
            data.join(SOCKET_NAME)
        );
        // A relative runtime folder is a broken environment, not a place.
        assert_eq!(
            socket_path_from(false, Some(PathBuf::from("run")), data.clone()),
            data.join(SOCKET_NAME)
        );
    }

    #[test]
    fn a_second_bind_is_refused_while_the_first_answers_and_a_stale_file_is_reclaimed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SOCKET_NAME);
        let Bound::Listening(first) = bind_instance(&path) else {
            panic!("the first instance must bind");
        };
        assert!(matches!(bind_instance(&path), Bound::Occupied));
        // The instance is gone but its file is still there.
        drop(first);
        assert!(path.exists());
        assert!(matches!(bind_instance(&path), Bound::Listening(_)));
    }

    #[test]
    fn a_request_is_delivered_and_acknowledged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SOCKET_NAME);
        let Bound::Listening(listener) = bind_instance(&path) else {
            panic!("bind");
        };
        let server = std::thread::spawn(move || {
            // The first connection is a silent probe; the second a request.
            let probe = listener.accept().unwrap().0;
            assert_eq!(serve(probe), None);
            serve(listener.accept().unwrap().0)
        });
        drop(UnixStream::connect(&path).unwrap());
        let image = PathBuf::from("/tmp/a b.png");
        assert!(send(&path, &Request::Open(image.clone())));
        assert_eq!(server.join().unwrap(), Some(Request::Open(image)));
        // Nobody listens any more: the request is reported as not delivered.
        assert!(!send(&path, &Request::Show));
    }
}
