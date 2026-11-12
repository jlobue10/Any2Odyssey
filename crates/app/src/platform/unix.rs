//! Linux (and other non-Windows) build: the stereo viewer runs here, but 3D
//! conversion needs Samsung's Windows engine. This module provides what the
//! controls need from the OS: one instance per user, the `--show` / `--quit`
//! / `--toggle` / `--open` hand-over to that instance, and file dialogs.
//!
//! The instance guard and the hand-over are one Unix socket: binding it is
//! the guard, and a later launch that finds it answered writes its request
//! there instead of opening a second window.

use std::io::{Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use any2odyssey_core::prefs::Preferences;
use any2odyssey_core::update::Comparison;
use any2odyssey_core::{ipc, paths};

const SOCKET_NAME: &str = "any2odyssey.sock";
/// A request and its acknowledgement are one short line each; a peer that
/// stalls must not hold the listener or a launching process for long.
const IO_TIMEOUT: Duration = Duration::from_secs(2);
// Include the newline. Leave ample room for paths, but never accumulate an
// unbounded request from a local peer (or an acknowledgement from a server).
const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_REPLY_BYTES: usize = 16;

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
        if let Some(encoded) = line.strip_prefix("open-bytes ") {
            let bytes: Vec<u8> = serde_json::from_str(encoded).ok()?;
            if bytes.is_empty() || bytes.contains(&0) {
                return None;
            }
            return Some(Request::Open(std::ffi::OsString::from_vec(bytes).into()));
        }
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
            Request::Open(path) => match path.to_str() {
                // Preserve compatibility for ordinary paths. Names containing
                // line breaks need framing; non-Unicode names must not go
                // through Display's lossy conversion either.
                Some(text) if !text.contains(['\r', '\n']) => format!("open {text}\n"),
                _ => format!(
                    "open-bytes {}\n",
                    serde_json::to_string(path.as_os_str().as_bytes())
                        .expect("byte slices always serialize")
                ),
            },
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

/// Read one bounded, newline-terminated message. The timeout covers the whole
/// line, including partial reads and interruptions, rather than starting over
/// whenever the peer sends another byte. `read` applies the remaining timeout
/// to the next blocking read; keeping it separate also permits socket-free tests.
fn read_message(
    limit: usize,
    timeout: Duration,
    mut read: impl FnMut(&mut [u8], Duration) -> std::io::Result<usize>,
) -> Option<String> {
    let started = Instant::now();
    let mut line = Vec::new();
    let mut buffer = [0; 256];
    while line.len() < limit {
        let remaining = timeout.checked_sub(started.elapsed())?;
        if remaining.is_zero() {
            return None;
        }
        let available = buffer.len().min(limit - line.len());
        let count = match read(&mut buffer[..available], remaining) {
            Ok(0) => return None,
            Ok(count) => count,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        };
        if started.elapsed() >= timeout {
            return None;
        }
        let bytes = &buffer[..count];
        if let Some(end) = bytes.iter().position(|&byte| byte == b'\n') {
            line.extend_from_slice(&bytes[..=end]);
            return String::from_utf8(line).ok();
        }
        line.extend_from_slice(bytes);
    }
    None
}

fn read_socket_line(mut stream: &UnixStream, limit: usize) -> Option<String> {
    read_message(limit, IO_TIMEOUT, |buffer, remaining| {
        stream.set_read_timeout(Some(remaining))?;
        stream.read(buffer)
    })
}

/// Read one request and acknowledge it. A connection that sends nothing (the
/// liveness probe of another launch) is not a request.
fn serve(stream: UnixStream) -> Option<Request> {
    stream.set_write_timeout(Some(IO_TIMEOUT)).ok()?;
    let line = read_socket_line(&stream, MAX_REQUEST_BYTES)?;
    let request = Request::parse(&line)?;
    let _ = (&stream).write_all(b"ok\n");
    Some(request)
}

/// Send `request` to the running instance; true when it acknowledged.
fn send(path: &Path, request: &Request) -> bool {
    let line = request.line();
    if line.len() > MAX_REQUEST_BYTES {
        return false;
    }
    let Ok(stream) = UnixStream::connect(path) else {
        return false;
    };
    if stream.set_write_timeout(Some(IO_TIMEOUT)).is_err()
        || (&stream).write_all(line.as_bytes()).is_err()
    {
        return false;
    }
    read_socket_line(&stream, MAX_REPLY_BYTES).is_some_and(|reply| reply.trim_end() == "ok")
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
    socket: Option<SocketFile>,
}

/// The directory entry we created or observed, without following symlinks.
#[derive(PartialEq, Eq)]
struct SocketFile {
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl SocketFile {
    fn at(path: &Path) -> Option<Self> {
        let metadata = std::fs::symlink_metadata(path).ok()?;
        metadata.file_type().is_socket().then(|| Self {
            path: path.to_owned(),
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    fn remove_if_unchanged(&self) -> bool {
        Self::at(&self.path).as_ref() == Some(self) && std::fs::remove_file(&self.path).is_ok()
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        if let Some(socket) = &self.socket {
            socket.remove_if_unchanged();
        }
    }
}

/// `None` when another instance of this user already answers the socket.
pub fn single_instance() -> Option<SingleInstance> {
    single_instance_at(socket_path())
}

fn single_instance_at(path: PathBuf) -> Option<SingleInstance> {
    match bind_instance(&path) {
        Bound::Listening(listener) => {
            let socket = SocketFile::at(&path);
            *LISTENER.lock().expect("listener") = Some(listener);
            Some(SingleInstance { socket })
        }
        Bound::Occupied => None,
        // No socket could be created at all (read-only or missing folder):
        // run without a guard rather than refuse to start.
        Bound::Unavailable => Some(SingleInstance { socket: None }),
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
                // An unrelated file or symlink is never a stale socket.
                let Some(socket) = SocketFile::at(path) else {
                    return Bound::Unavailable;
                };
                match UnixStream::connect(path) {
                    Ok(_) => return Bound::Occupied,
                    Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {}
                    // Permission and other failures do not prove the owner
                    // has exited, so leave the entry alone.
                    Err(_) => return Bound::Unavailable,
                }
                // Recheck the entry before reclaiming an abandoned socket.
                if !socket.remove_if_unchanged() {
                    return Bound::Unavailable;
                }
            }
            Err(_) => return Bound::Unavailable,
        }
    }
    Bound::Unavailable
}

/// Run the first of `candidates` that is installed and return its filename from
/// standard output. `None` when the dialog was cancelled or none exists.
fn dialog(candidates: &[(&str, Vec<String>)]) -> Result<Option<PathBuf>, String> {
    for (program, arguments) in candidates {
        match std::process::Command::new(program)
            .args(arguments)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .output()
        {
            Ok(mut output) => {
                // Zenity and kdialog append one LF after the filename. Any
                // other line breaks belong to that name, including a CR
                // immediately before the delimiter. Keep the OS bytes too.
                if output.stdout.last() == Some(&b'\n') {
                    output.stdout.pop();
                }
                return Ok((output.status.success() && !output.stdout.is_empty())
                    .then(|| std::ffi::OsString::from_vec(output.stdout).into()));
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

/// Estimate depth for the viewer's 2D → 3D mode. The model and the runtime
/// are loaded on the first call and kept for the life of the process; a
/// failed load is tried again on the next call, so installing the model
/// while the program runs is enough. Blocks: call it off the UI thread.
pub fn estimate_depth(
    image: &image::RgbaImage,
    long_side: u32,
) -> Result<any2odyssey_core::dibr::DepthMap, String> {
    Ok(
        crate::depth::estimate_unless_cancelled(image, long_side, || false)?
            .expect("this request cannot be cancelled"),
    )
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
    fn message_reads_are_bounded_and_require_a_complete_utf8_line() {
        fn message(bytes: &[u8], limit: usize) -> Option<String> {
            let mut input = std::io::Cursor::new(bytes);
            read_message(limit, IO_TIMEOUT, |buffer, _| input.read(buffer))
        }
        assert_eq!(message(b"ok\n", 3).as_deref(), Some("ok\n"));
        assert_eq!(message(b"ok\nignored", 16).as_deref(), Some("ok\n"));
        for bytes in [b"".as_slice(), b"ok", b"ok\n", b"\xff\n"] {
            assert_eq!(message(bytes, 2), None, "{bytes:?}");
        }
        assert_eq!(message(b"ok\n", 0), None);

        for limit in [MAX_REQUEST_BYTES, MAX_REPLY_BYTES] {
            let mut full = vec![b'x'; limit];
            full[limit - 1] = b'\n';
            assert_eq!(message(&full, limit).unwrap().len(), limit);
            full.insert(0, b'x');
            assert_eq!(message(&full, limit), None);

            // A peer that never sends a newline cannot cause more than the
            // limit to be read or retained, even if data is always ready.
            let mut consumed = 0;
            assert_eq!(
                read_message(limit, IO_TIMEOUT, |buffer, _| {
                    buffer.fill(b'x');
                    consumed += buffer.len();
                    Ok(buffer.len())
                }),
                None
            );
            assert_eq!(consumed, limit);
        }
    }

    #[test]
    fn fragmented_messages_and_interrupted_reads_keep_their_contents() {
        let expected = "open /tmp/caf\u{e9}.png\n";
        let mut bytes = expected.as_bytes().iter();
        let mut interrupted = true;
        let received = read_message(MAX_REQUEST_BYTES, IO_TIMEOUT, |buffer, _| {
            interrupted = !interrupted;
            if interrupted {
                return Err(std::io::ErrorKind::Interrupted.into());
            }
            let Some(&byte) = bytes.next() else {
                return Ok(0);
            };
            buffer[0] = byte;
            Ok(1)
        });
        assert_eq!(received.as_deref(), Some(expected));
    }

    #[test]
    fn a_trickled_line_cannot_renew_the_message_deadline() {
        let budget = Duration::from_millis(40);
        let mut previous = budget;
        let mut reads = 0;
        let line = read_message(MAX_REQUEST_BYTES, budget, |buffer, remaining| {
            assert!(remaining <= previous);
            previous = remaining;
            reads += 1;
            // Each read makes progress before its own timeout, but the
            // complete message would take far longer than the total budget.
            std::thread::sleep(Duration::from_millis(10));
            buffer[0] = if reads == 20 { b'\n' } else { b'x' };
            Ok(1)
        });
        assert_eq!(line, None);
        assert!(reads < 20);

        let mut called = false;
        assert_eq!(
            read_message(MAX_REPLY_BYTES, Duration::ZERO, |_, _| {
                called = true;
                Ok(0)
            }),
            None
        );
        assert!(!called);
    }

    #[test]
    fn image_handoff_preserves_paths_with_line_breaks() {
        let dir = tempfile::tempdir().unwrap();
        let misleading = dir.path().join("pair.png");
        let wanted = dir.path().join("pair.png\nother.png");
        image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]))
            .save(&misleading)
            .unwrap();
        image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 255, 0, 255]))
            .save(&wanted)
            .unwrap();
        for path in [
            wanted,
            dir.path().join("trailing.png\n"),
            dir.path().join("trailing.png\r"),
            dir.path().join("trailing.png\r\n"),
        ] {
            let request = Request::Open(path.clone());
            let wire = request.line();
            let mut input = std::io::Cursor::new(wire.as_bytes());
            let line = read_message(MAX_REQUEST_BYTES, IO_TIMEOUT, |buffer, _| {
                input.read(buffer)
            })
            .expect("complete bounded request");
            let delivered = Request::parse(&line).expect("valid open request");
            assert_eq!(delivered, request);
            assert_eq!(wire.bytes().filter(|&b| b == b'\n').count(), 1);
            if path.exists() {
                let Request::Open(delivered_path) = delivered else {
                    panic!("open request")
                };
                assert_eq!(
                    image::open(delivered_path)
                        .unwrap()
                        .to_rgba8()
                        .get_pixel(0, 0)
                        .0,
                    [0, 255, 0, 255]
                );
            }
        }
    }

    #[test]
    fn encoded_image_paths_preserve_bytes_and_reject_invalid_payloads() {
        let path: PathBuf =
            std::ffi::OsString::from_vec(b"/tmp/non-unicode-\xff.png".to_vec()).into();
        let request = Request::Open(path);
        let wire = request.line();
        assert!(wire.starts_with("open-bytes "));
        assert_eq!(Request::parse(&wire), Some(request));
        for payload in [
            "",
            "[]",
            "[0]",
            "[47,0,97]",
            "[256]",
            "[-1]",
            "null",
            "[47] junk",
        ] {
            assert_eq!(
                Request::parse(&format!("open-bytes {payload}\n")),
                None,
                "{payload}"
            );
        }
        let ordinary = Request::Open(PathBuf::from("/tmp/caf\u{e9} \"pair\".png"));
        assert_eq!(ordinary.line(), "open /tmp/caf\u{e9} \"pair\".png\n");
        assert_eq!(Request::parse(&ordinary.line()), Some(ordinary));
    }

    #[test]
    fn dialog_preserves_filename_bytes_and_only_removes_its_delimiter() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("dialog-output");
        for bytes in [
            b"/tmp/ordinary.png".as_slice(),
            b"/tmp/line\nbreak.png",
            b"/tmp/trailing.png\n",
            b"/tmp/trailing.png\r\n",
            b"/tmp/non-unicode-\xff.png",
            b"/tmp/trailing space ",
            b"\n",
        ] {
            let mut stdout = bytes.to_vec();
            stdout.push(b'\n');
            std::fs::write(&output, stdout).unwrap();
            let candidates = |status: &str| {
                vec![(
                    "/bin/sh",
                    vec![
                        "-c".into(),
                        "cat \"$1\"; exit \"$2\"".into(),
                        "dialog-fixture".into(),
                        output.to_str().unwrap().into(),
                        status.into(),
                    ],
                )]
            };
            let selected = dialog(&candidates("0")).unwrap().unwrap();
            assert_eq!(selected.as_os_str().as_bytes(), bytes);
            assert_eq!(dialog(&candidates("1")).unwrap(), None);
        }
        for empty in [b"".as_slice(), b"\n"] {
            std::fs::write(&output, empty).unwrap();
            assert_eq!(
                dialog(&[(
                    "/bin/sh",
                    vec![
                        "-c".into(),
                        "cat \"$1\"".into(),
                        "dialog-fixture".into(),
                        output.to_str().unwrap().into(),
                    ]
                )])
                .unwrap(),
                None
            );
        }
    }

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
        // Another test's child process, between fork and exec, can still hold
        // a copy of the listener for a moment; that is the test harness, so
        // give it the moment.
        let reclaimed = (0..200).any(|_| {
            matches!(bind_instance(&path), Bound::Listening(_)) || {
                std::thread::sleep(std::time::Duration::from_millis(10));
                false
            }
        });
        assert!(reclaimed, "a stale socket file must be taken over");
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

    #[test]
    fn an_unavailable_instance_does_not_delete_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SOCKET_NAME);
        std::fs::write(&path, b"unrelated data").unwrap();
        drop(single_instance_at(path.clone()));
        assert_eq!(std::fs::read(&path).unwrap(), b"unrelated data");
    }

    #[test]
    fn an_unavailable_instance_does_not_delete_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SOCKET_NAME);
        let target = dir.path().join("missing-target");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        drop(single_instance_at(path.clone()));
        assert_eq!(std::fs::read_link(path).unwrap(), target);
    }

    #[test]
    fn a_guard_removes_its_own_socket_but_preserves_a_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SOCKET_NAME);
        let _first = UnixListener::bind(&path).unwrap();
        let first_guard = SingleInstance {
            socket: Some(SocketFile::at(&path).unwrap()),
        };
        // Keep the old inode alive at a different name while a new instance
        // owns the original path.
        std::fs::rename(&path, dir.path().join("old.sock")).unwrap();
        let _second = UnixListener::bind(&path).unwrap();
        let second_guard = SingleInstance {
            socket: Some(SocketFile::at(&path).unwrap()),
        };
        drop(first_guard);
        assert!(path.exists());
        drop(second_guard);
        assert!(!path.exists());
    }
}
