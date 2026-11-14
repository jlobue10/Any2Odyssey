//! Linux (and other non-Windows) build: the stereo viewer runs here, but 3D
//! conversion needs Samsung's Windows engine. This module provides what the
//! controls need from the OS: one instance per user, the `--show` / `--quit`
//! / `--toggle` / `--open` hand-over to that instance, and file dialogs.
//!
//! The instance guard and the hand-over are one Unix socket: binding it is
//! the guard, and a later launch that finds it answered writes its request
//! there instead of opening a second window.

use std::ffi::OsString;
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
/// How long a launch waits for another launch to finish looking at the
/// socket and binding its own, which takes that one a few milliseconds.
const LOCK_PATIENCE: Duration = Duration::from_secs(2);
/// How long an instance that is leaving waits for the same before it leaves
/// its socket's name behind for the next launch to clear away.
const EXIT_PATIENCE: Duration = Duration::from_millis(500);

/// The five calls into the C library this module makes. The standard
/// library has no stable way to ask who is at the other end of a socket or
/// to tie a child's life to this process, and its file locks are newer than
/// the oldest compiler this crate builds with.
mod sys {
    use std::ffi::c_int;
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;

    extern "C" {
        fn geteuid() -> u32;
        fn flock(fd: c_int, operation: c_int) -> c_int;
        fn kill(pid: i32, signal: c_int) -> c_int;
    }

    /// End (SIGKILL) every process of the group that `leader` started as
    /// its first member. Only for a child of this process that has not been
    /// waited for: until then the number cannot have gone to another group.
    pub fn end_group(leader: u32) {
        const SIGKILL: c_int = 9;
        // Zero and one are not groups of ours, and their negatives mean
        // "every process" to this call.
        if let Some(group) = i32::try_from(leader).ok().filter(|&group| group > 1) {
            // SAFETY: two numbers; touches no memory of this process.
            let _ = unsafe { kill(-group, SIGKILL) };
        }
    }

    /// The same numbers on Linux, the BSDs and macOS.
    const LOCK_EX: c_int = 2;
    const LOCK_NB: c_int = 4;

    /// Ask the kernel to end this process (SIGTERM) when the thread that
    /// started it ends, which is at the latest when its program does. For
    /// the child side of a fork, before it becomes another program; the
    /// request stays across that. Without it (not Linux, or refused) the
    /// child is simply left to itself, as before.
    pub fn end_with_parent() {
        #[cfg(target_os = "linux")]
        {
            extern "C" {
                fn prctl(option: c_int, ...) -> c_int;
            }
            const PR_SET_PDEATHSIG: c_int = 1;
            const SIGTERM: std::ffi::c_ulong = 15;
            // SAFETY: a system call on two numbers that touches no memory
            // of this process; safe between fork and exec.
            let _ = unsafe { prctl(PR_SET_PDEATHSIG, SIGTERM) };
        }
    }

    /// The user this process acts as.
    pub fn own_uid() -> u32 {
        // SAFETY: takes nothing, cannot fail and touches no memory.
        unsafe { geteuid() }
    }

    /// Take the whole-file lock on `file` if no other open file holds it.
    /// It goes when `file` is closed, however this process ends.
    pub fn try_lock(file: &std::fs::File) -> std::io::Result<bool> {
        // SAFETY: the descriptor is open for as long as `file` is borrowed,
        // and the call touches no memory of ours.
        if unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) } == 0 {
            return Ok(true);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::WouldBlock {
            Ok(false)
        } else {
            Err(error)
        }
    }

    /// The user at the other end of `stream`, as the kernel recorded it when
    /// the connection was made. `Err` when the kernel does not say.
    ///
    /// Linux only, and only where the two option numbers are the common
    /// ones (they differ on MIPS, PowerPC, SPARC and others).
    #[cfg(all(
        target_os = "linux",
        any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "x86",
            target_arch = "arm",
            target_arch = "riscv64"
        )
    ))]
    pub fn peer_uid(stream: &UnixStream) -> Result<Option<u32>, std::io::Error> {
        use std::ffi::c_void;
        extern "C" {
            fn getsockopt(
                fd: c_int,
                level: c_int,
                name: c_int,
                value: *mut c_void,
                length: *mut u32,
            ) -> c_int;
        }
        const SOL_SOCKET: c_int = 1;
        const SO_PEERCRED: c_int = 17;
        /// `struct ucred`: process, user and group of the peer.
        #[repr(C)]
        struct Credentials {
            pid: i32,
            uid: u32,
            gid: u32,
        }
        let mut credentials = Credentials {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut length = std::mem::size_of::<Credentials>() as u32;
        // SAFETY: `value` points at a `struct ucred` of `length` bytes that
        // lives across the call, and the descriptor is open while `stream`
        // is borrowed.
        let status = unsafe {
            getsockopt(
                stream.as_raw_fd(),
                SOL_SOCKET,
                SO_PEERCRED,
                (&raw mut credentials).cast(),
                &mut length,
            )
        };
        if status != 0 {
            return Err(std::io::Error::last_os_error());
        }
        if length as usize != std::mem::size_of::<Credentials>() {
            return Err(std::io::Error::other("unexpected peer credentials"));
        }
        let _ = (credentials.pid, credentials.gid);
        Ok(Some(credentials.uid))
    }

    /// Elsewhere the question is not asked: who may reach the socket is
    /// decided by its permissions and its folder's alone.
    #[cfg(not(all(
        target_os = "linux",
        any(
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "x86",
            target_arch = "arm",
            target_arch = "riscv64"
        )
    )))]
    pub fn peer_uid(_: &UnixStream) -> Result<Option<u32>, std::io::Error> {
        Ok(None)
    }
}

/// Whether a connection whose other end is `peer` (see [`sys::peer_uid`]) is
/// one of this user's own, `own`. A peer the kernel would not name is not.
fn same_user(peer: Result<Option<u32>, std::io::Error>, own: u32) -> bool {
    match peer {
        Ok(Some(uid)) => uid == own,
        Ok(None) => true,
        Err(_) => false,
    }
}

fn peer_is_this_user(stream: &UnixStream) -> bool {
    same_user(sys::peer_uid(stream), sys::own_uid())
}

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
                    let Ok(stream) = stream else {
                        // A failure that lasts (no descriptors left) must
                        // not turn this loop into a busy one.
                        std::thread::sleep(Duration::from_millis(50));
                        continue;
                    };
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
/// liveness probe of another launch) is not a request, and neither is
/// anything from another user: the socket's permissions keep others out,
/// and this does where a folder or a umask was left open. Connections are
/// served one after another, each with [`IO_TIMEOUT`] for its line, which
/// is enough when only this user's own launches are heard.
fn serve(stream: UnixStream) -> Option<Request> {
    // Before anything is read from it.
    if !peer_is_this_user(&stream) {
        static NOTED: AtomicBool = AtomicBool::new(false);
        if !NOTED.swap(true, Ordering::Relaxed) {
            any2odyssey_core::log::Logger::default_location().log(
                "A request on the instance socket from another user was ignored. Further ones are ignored without a line here.",
            );
        }
        return None;
    }
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
    // A request can name a file: it is told to this user's instance only.
    if !peer_is_this_user(&stream) {
        return false;
    }
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

    /// Remove the entry if it is still the one that was seen. Looking and
    /// removing are two steps by name, so the caller holds the
    /// [`InstanceLock`]: otherwise another launch could clear this entry
    /// away and bind its own between them, and lose it here.
    fn remove_if_unchanged(&self) -> bool {
        Self::at(&self.path).as_ref() == Some(self) && std::fs::remove_file(&self.path).is_ok()
    }
}

/// The lock that makes looking at the socket's name, clearing an abandoned
/// one away and binding a new one a single step among this user's launches,
/// and keeps an instance that is leaving from removing a newer one's name.
/// It is a lock on a file beside the socket, held while this value lives
/// and let go by the kernel if the process dies. The file itself stays.
struct InstanceLock {
    _file: std::fs::File,
}

impl InstanceLock {
    /// Wait up to `patience` for the lock that belongs to `socket`. `None`
    /// when the folder takes no lock file or another launch keeps the lock.
    fn acquire(socket: &Path, patience: Duration) -> Option<InstanceLock> {
        use std::os::unix::fs::OpenOptionsExt;
        let mut name = socket.as_os_str().to_owned();
        name.push(".lock");
        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(name)
            .ok()?;
        let deadline = Instant::now() + patience;
        loop {
            match sys::try_lock(&file) {
                Ok(true) => return Some(InstanceLock { _file: file }),
                Ok(false) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                _ => return None,
            }
        }
    }
}

impl Drop for SingleInstance {
    fn drop(&mut self) {
        if let Some(socket) = &self.socket {
            // Without the lock the name is left for the next launch to
            // clear away, which it does once nobody answers.
            if let Some(_lock) = InstanceLock::acquire(&socket.path, EXIT_PATIENCE) {
                socket.remove_if_unchanged();
            }
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
    // One launch at a time, so that of two launches that find an abandoned
    // socket the second finds the first one's new socket answering, instead
    // of removing it or giving up and running as a second instance.
    let lock = InstanceLock::acquire(path, LOCK_PATIENCE);
    for _ in 0..2 {
        let taken = match bind_private(path) {
            Ok(listener) => return Bound::Listening(listener),
            Err(e) => matches!(
                e.kind(),
                std::io::ErrorKind::AddrInUse | std::io::ErrorKind::AlreadyExists
            ),
        };
        // An unrelated file or symlink is never a stale socket.
        let Some(socket) = SocketFile::at(path).filter(|_| taken) else {
            return Bound::Unavailable;
        };
        match UnixStream::connect(path) {
            Ok(_) => return Bound::Occupied,
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {}
            // Permission and other failures do not prove the owner
            // has exited, so leave the entry alone.
            Err(_) => return Bound::Unavailable,
        }
        // Nothing is removed without the lock (a folder that takes no lock
        // file, or a launch that has kept the lock for seconds): the entry
        // could be a socket another launch has just bound.
        if lock.is_none() || !socket.remove_if_unchanged() {
            return Bound::Unavailable;
        }
    }
    Bound::Unavailable
}

/// Bind the instance socket at `path`, which must not exist, so that it is
/// at no moment a socket another user could connect to, whatever the umask
/// and the folder: it is bound inside a new folder only this user can enter,
/// given its permissions there, and only then given its name by a hard
/// link, which fails where a rename would replace. Changing the umask
/// instead would change it for every thread.
fn bind_private(path: &Path) -> std::io::Result<UnixListener> {
    use std::os::unix::fs::PermissionsExt;
    let private = std::fs::Permissions::from_mode(0o600);
    let dir = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let staged = tempfile::Builder::new()
        .prefix(".instance-socket-")
        // Asked for: a new folder is otherwise as open as the umask leaves it.
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in(dir)
        .and_then(|staging| {
            let entered_by = std::fs::metadata(staging.path())?.permissions().mode();
            if entered_by & 0o077 != 0 {
                return Err(std::io::Error::other("the staging folder is not private"));
            }
            let name = staging.path().join("s");
            let listener = UnixListener::bind(&name)?;
            std::fs::set_permissions(&name, private.clone())?;
            // The staging folder and the first name go when this returns.
            std::fs::hard_link(&name, path).map(|()| listener)
        });
    match staged {
        Ok(listener) => Ok(listener),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(e),
        // No hard links here, or a path too long for a socket once the
        // staging folder is in it: bind in place and close it at once.
        Err(_) => {
            let listener = UnixListener::bind(path)?;
            std::fs::set_permissions(path, private)?;
            Ok(listener)
        }
    }
}

/// The file dialogs, which are other programs. One is open at a time:
/// [`Dialogs::close`] ends it, and so does starting another. Either way it
/// has no answer (`None`).
struct Dialogs {
    open: Mutex<OpenDialog>,
}

#[derive(Default)]
struct OpenDialog {
    /// Tells the dialogs apart: the thread waiting for one must not take
    /// another's for its own.
    started: u64,
    /// Kept here until its end has been collected, so that whoever ends it
    /// holds the process itself and not a number that may be another's.
    child: Option<(u64, std::process::Child)>,
}

/// The dialogs of [`pick_image`] and [`pick_save_png`].
static DIALOGS: Dialogs = Dialogs::new();

impl Dialogs {
    const fn new() -> Self {
        Self {
            open: Mutex::new(OpenDialog {
                started: 0,
                child: None,
            }),
        }
    }

    fn open(&self) -> std::sync::MutexGuard<'_, OpenDialog> {
        self.open
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// End the dialog that is open, if one is, and collect it. The call
    /// waiting for its answer returns `None`.
    fn close(&self) {
        let ended = self.open().child.take();
        if let Some((_, child)) = ended {
            end(child);
        }
    }

    /// Run the first of `candidates` that is installed and return its filename from
    /// standard output. `None` when the dialog was cancelled or none exists.
    ///
    /// A dialog still open when this program ends is ended with it: nothing
    /// would read its answer, and its window would stay on the desktop.
    fn run(&self, candidates: &[(&str, Vec<String>)]) -> Result<Option<PathBuf>, String> {
        use std::os::unix::process::CommandExt;
        for (program, arguments) in candidates {
            let mut command = std::process::Command::new(program);
            command
                .args(arguments)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                // A group of its own, so that it can be ended together with
                // whatever it started (a dialog that is a wrapper script).
                .process_group(0);
            // SAFETY: runs in the child between fork and exec and makes one
            // system call there: no allocation, no lock, nothing of the parent's.
            unsafe {
                command.pre_exec(|| {
                    sys::end_with_parent();
                    Ok(())
                });
            }
            let mut child = match command.spawn() {
                Ok(child) => child,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(format!("{program}: {e}")),
            };
            let mut stdout = child.stdout.take().expect("asked for above");
            let id = {
                let mut open = self.open();
                open.started += 1;
                let id = open.started;
                let before = open.child.replace((id, child));
                drop(open);
                if let Some((_, before)) = before {
                    end(before);
                }
                id
            };
            let mut answer = Vec::new();
            let read = stdout.read_to_end(&mut answer);
            drop(stdout);
            // Its end is collected here unless it was ended from elsewhere
            // meanwhile; until then it stays where `close` finds it.
            let status = loop {
                let mut open = self.open();
                match &mut open.child {
                    Some((owner, child)) if *owner == id => match child.try_wait() {
                        Ok(None) => {}
                        Ok(Some(status)) => {
                            open.child = None;
                            break status;
                        }
                        Err(e) => {
                            open.child = None;
                            return Err(format!("{program}: {e}"));
                        }
                    },
                    _ => return Ok(None),
                }
                drop(open);
                std::thread::sleep(Duration::from_millis(10));
            };
            if let Err(e) = read {
                return Err(format!("{program}: {e}"));
            }
            // Zenity and kdialog append one LF after the filename. Any
            // other line breaks belong to that name, including a CR
            // immediately before the delimiter. Keep the OS bytes too.
            if answer.last() == Some(&b'\n') {
                answer.pop();
            }
            return Ok((status.success() && !answer.is_empty())
                .then(|| std::ffi::OsString::from_vec(answer).into()));
        }
        Err(NO_DIALOG.into())
    }
}

/// End a dialog nobody waits for any more and collect it, so that it is not
/// left as a process that has ended but is still listed.
fn end(mut child: std::process::Child) {
    // Not collected yet, so its number, which is also its group's, is still
    // its own: nothing else can be hit.
    if matches!(child.try_wait(), Ok(None)) {
        sys::end_group(child.id());
        let _ = child.kill();
    }
    let _ = child.wait();
}

/// Close the file dialog that is open, if one is: the [`pick_image`] or
/// [`pick_save_png`] waiting for it returns `None`. For a dialog that hangs
/// or whose window cannot be found any more.
pub fn close_dialog() {
    DIALOGS.close();
}

pub const NO_DIALOG: &str =
    "No file dialog is installed (zenity or kdialog). Type or paste the path, or drop the file on this window.";

/// Ask for an image to open. Blocks until the dialog closes; call it off the
/// UI thread.
pub fn pick_image() -> Result<Option<PathBuf>, String> {
    DIALOGS.run(&[
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
    DIALOGS.run(&[
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
/// The capture diagnostics look at the Windows worker's windows. Asked for
/// here they failed without a word.
const WINDOWS_ONLY_DIAGNOSTIC: &str = "This diagnostic runs only on Windows.";
pub fn inspect(_: u32) -> i32 {
    eprintln!("{WINDOWS_ONLY_DIAGNOSTIC}");
    1
}
pub fn layout_probe(_: u32, _: u32) -> i32 {
    eprintln!("{WINDOWS_ONLY_DIAGNOSTIC}");
    1
}
pub fn test_capture_exclusion() -> i32 {
    eprintln!("{WINDOWS_ONLY_DIAGNOSTIC}");
    1
}
pub fn test_offscreen() -> i32 {
    eprintln!("{WINDOWS_ONLY_DIAGNOSTIC}");
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
/// Whether winit opens the windows on Wayland. Its rule: Wayland first,
/// wherever `WAYLAND_DISPLAY` or `WAYLAND_SOCKET` names something.
pub fn uses_wayland() -> bool {
    names_wayland(
        std::env::var_os("WAYLAND_DISPLAY"),
        std::env::var_os("WAYLAND_SOCKET"),
    )
}

fn names_wayland(display: Option<OsString>, socket: Option<OsString>) -> bool {
    let named = |value: Option<OsString>| value.is_some_and(|value| !value.is_empty());
    named(display) || named(socket)
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

    /// A dialog of this test's own: the program's one is shared by every
    /// test running at the same time.
    fn dialog(candidates: &[(&str, Vec<String>)]) -> Result<Option<PathBuf>, String> {
        Dialogs::new().run(candidates)
    }

    /// A "dialog" that never answers: a script that waits for a program it
    /// started, which holds the dialog's output open like the real window
    /// of a wrapper script would. Both write their process numbers to `note`.
    #[cfg(target_os = "linux")]
    fn hanging_dialog(note: &Path) -> Vec<(&'static str, Vec<String>)> {
        vec![(
            "/bin/sh",
            vec![
                "-c".into(),
                "sleep 600 & echo $$ $! >\"$1\"; wait; echo /tmp/never.png".into(),
                "dialog-fixture".into(),
                note.to_str().unwrap().into(),
            ],
        )]
    }

    /// The process numbers a [`hanging_dialog`] noted, once it has and once
    /// `dialogs` holds it. The script writes them as soon as it runs, and on
    /// a busy machine that is before the thread that started it has put it
    /// where `close` and the next dialog find it: a `close` in between
    /// closed nothing, and a dialog started in between was the one ended.
    #[cfg(target_os = "linux")]
    fn noted_pids(dialogs: &Dialogs, note: &Path) -> Vec<u32> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let text = std::fs::read_to_string(note).unwrap_or_default();
            if text.ends_with('\n') && dialogs.open().child.is_some() {
                return text
                    .split_whitespace()
                    .map(|p| p.parse().unwrap())
                    .collect();
            }
            assert!(Instant::now() < deadline, "the dialog was not started");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Still running: listed, and not as ended. An ended process is listed
    /// as waiting to be collected (Z) and then, for the moment in which
    /// whoever inherited it collects it, as dead (X): one that was Z at one
    /// look can be X at the next, and that is not a process running again.
    #[cfg(target_os = "linux")]
    fn running(pid: u32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| !ended(&stat))
    }

    /// Whether `stat`, a process's line in `/proc`, is of one that has ended.
    #[cfg(target_os = "linux")]
    fn ended(stat: &str) -> bool {
        // "<pid> (<name>) <state> ...": the name may hold anything.
        let state = stat.rsplit(')').next().unwrap();
        state.starts_with(" Z") || state.starts_with(" X")
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_process_being_collected_is_not_taken_for_one_running() {
        assert!(!ended("71 (sleep) S 1 70 70 0 -1 4194560"));
        assert!(!ended("71 (sh) R 1 70 70 0 -1 4194560"));
        assert!(ended("71 (sleep) Z 1 70 70 0 -1 4228108"));
        assert!(ended("71 (sleep) X 1 70 70 0 -1 4228108"));
        assert!(ended("71 (a) S (b) X 1 70 70 0 -1 4228108"));
        // This test itself is running.
        assert!(running(std::process::id()));
    }

    #[cfg(target_os = "linux")]
    fn gone_within(pid: u32, patience: Duration) -> bool {
        let deadline = Instant::now() + patience;
        while running(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        !running(pid)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_dialog_that_never_answers_can_be_closed_and_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("pids");
        let dialogs = Arc::new(Dialogs::new());
        // Nothing open: nothing happens.
        dialogs.close();
        let waiting = {
            let (dialogs, fixture) = (dialogs.clone(), hanging_dialog(&note));
            std::thread::spawn(move || dialogs.run(&fixture))
        };
        let pids = noted_pids(&dialogs, &note);
        let (script, started_by_it) = (pids[0], pids[1]);
        assert!(running(script) && running(started_by_it));
        dialogs.close();
        // Collected by the time `close` returns: not even listed as ended,
        // which a child that nobody waited for stays until this test ends.
        assert!(
            !Path::new(&format!("/proc/{script}")).exists(),
            "the closed dialog was left uncollected"
        );
        // What it started went with it and no longer holds its output open,
        // so the thread waiting for the answer is released, without one.
        assert!(gone_within(started_by_it, Duration::from_secs(5)));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !waiting.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            waiting.is_finished(),
            "the closed dialog is still waited for"
        );
        assert_eq!(waiting.join().unwrap(), Ok(None));
        assert!(dialogs.open().child.is_none());
        // And the next dialog is an ordinary one.
        let answer = dir.path().join("answer");
        std::fs::write(&answer, "/tmp/chosen.png\n").unwrap();
        let next = vec![("/bin/cat", vec![answer.to_str().unwrap().to_owned()])];
        assert_eq!(
            dialogs.run(&next),
            Ok(Some(PathBuf::from("/tmp/chosen.png")))
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_new_dialog_ends_the_one_still_open() {
        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("pids");
        let dialogs = Arc::new(Dialogs::new());
        let waiting = {
            let (dialogs, fixture) = (dialogs.clone(), hanging_dialog(&note));
            std::thread::spawn(move || dialogs.run(&fixture))
        };
        let pids = noted_pids(&dialogs, &note);
        let answer = dir.path().join("answer");
        std::fs::write(&answer, "/tmp/second.png\n").unwrap();
        let second = vec![("/bin/cat", vec![answer.to_str().unwrap().to_owned()])];
        assert_eq!(
            dialogs.run(&second),
            Ok(Some(PathBuf::from("/tmp/second.png")))
        );
        assert!(!Path::new(&format!("/proc/{}", pids[0])).exists());
        assert!(gone_within(pids[1], Duration::from_secs(5)));
        // The first one's caller gets no answer, and not the second one's.
        assert_eq!(waiting.join().unwrap(), Ok(None));
        assert!(dialogs.open().child.is_none());
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

    /// The viewer runs a dialog on a thread that waits for it. This test
    /// starts itself once more as the program with such a thread, has that
    /// program end while its "dialog" (a sleep) is open, and looks whether
    /// the sleep went with it.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_dialog_left_open_ends_with_the_program() {
        const NOTE: &str = "ANY2ODYSSEY_TEST_DIALOG_PID_FILE";
        let written = |note: &Path| {
            std::fs::read_to_string(note)
                .ok()
                .filter(|text| text.ends_with('\n'))
        };
        if let Some(note) = std::env::var_os(NOTE).map(PathBuf::from) {
            let fixture = vec![(
                "/bin/sh",
                vec![
                    "-c".into(),
                    "echo $$ >\"$1\"; exec sleep 600".into(),
                    "dialog-fixture".into(),
                    note.to_str().unwrap().into(),
                ],
            )];
            std::thread::spawn(move || dialog(&fixture));
            let deadline = Instant::now() + Duration::from_secs(10);
            while written(&note).is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            std::process::exit(0);
        }

        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("dialog-pid");
        let this_test = format!(
            "{}::a_dialog_left_open_ends_with_the_program",
            module_path!().split_once("::").unwrap().1
        );
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", &this_test])
            .env(NOTE, &note)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        let pid: u32 = written(&note)
            .expect("the dialog was started")
            .trim()
            .parse()
            .unwrap();
        // Gone, or ended and not yet collected by whoever inherited it.
        let deadline = Instant::now() + Duration::from_secs(5);
        while running(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let outlived = running(pid);
        if outlived {
            let _ = std::process::Command::new("kill")
                .arg(pid.to_string())
                .status();
        }
        assert!(!outlived, "the dialog outlived the program that opened it");
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

    /// A socket file nobody listens on any more, as a killed instance
    /// leaves it. Waits out a copy of the listener in another test's child
    /// process between its fork and its exec.
    fn abandoned_socket(path: &Path) {
        drop(UnixListener::bind(path).unwrap());
        let deadline = Instant::now() + Duration::from_secs(10);
        while UnixStream::connect(path).is_ok() {
            assert!(Instant::now() < deadline, "the listener never went away");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn launches_racing_for_an_abandoned_socket_leave_one_instance_and_no_unguarded_one() {
        for _ in 0..20 {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join(SOCKET_NAME);
            abandoned_socket(&path);
            let barrier = std::sync::Barrier::new(8);
            let bound: Vec<Bound> = std::thread::scope(|scope| {
                let launches: Vec<_> = (0..8)
                    .map(|_| {
                        scope.spawn(|| {
                            barrier.wait();
                            bind_instance(&path)
                        })
                    })
                    .collect();
                // Every launch's result is kept until all have finished, so
                // the winner's listener is there for the others to find.
                launches.into_iter().map(|l| l.join().unwrap()).collect()
            });
            let count = |wanted: fn(&Bound) -> bool| bound.iter().filter(|b| wanted(b)).count();
            assert_eq!(count(|b| matches!(b, Bound::Listening(_))), 1);
            assert_eq!(count(|b| matches!(b, Bound::Occupied)), 7);
            // The one socket is the winner's, and it answers.
            assert!(UnixStream::connect(&path).is_ok());
        }
    }

    #[test]
    fn the_socket_is_private_from_its_first_moment_and_nothing_is_left_beside_it() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        // A folder anyone may enter, as a data folder can be.
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = dir.path().join(SOCKET_NAME);
        let Bound::Listening(_listener) = bind_instance(&path) else {
            panic!("bind");
        };
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        assert!(metadata.file_type().is_socket());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        let mut names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [SOCKET_NAME.to_owned(), format!("{SOCKET_NAME}.lock")]
        );
        assert!(UnixStream::connect(&path).is_ok());
        // Binding where something already is fails rather than replaces.
        assert_eq!(
            bind_private(&path).unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
    }

    #[test]
    fn only_this_users_own_connections_are_heard() {
        let (near, far) = UnixStream::pair().unwrap();
        assert!(peer_is_this_user(&near) && peer_is_this_user(&far));
        let own = sys::own_uid();
        assert!(same_user(Ok(Some(own)), own));
        assert!(!same_user(Ok(Some(own.wrapping_add(1))), own));
        // Root is another user too, and so is a peer the kernel would not name.
        assert!(!same_user(Ok(Some(0)), 1000));
        assert!(!same_user(Err(std::io::Error::other("no answer")), own));
        // Where the kernel is not asked, the socket's permissions decide.
        assert!(same_user(Ok(None), own));
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        assert_eq!(sys::peer_uid(&near).unwrap(), Some(own));
        // A request from this user still goes through.
        (&far).write_all(b"toggle\n").unwrap();
        assert_eq!(serve(near), Some(Request::Toggle));
    }

    #[test]
    fn a_leaving_instance_does_not_touch_the_socket_while_a_launch_holds_the_lock() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(SOCKET_NAME);
        let Bound::Listening(_listener) = bind_instance(&path) else {
            panic!("bind");
        };
        let guard = SingleInstance {
            socket: SocketFile::at(&path),
        };
        // Not at once: `bind_instance` has only just closed its own lock, and
        // a child that another test's thread forked in that moment keeps a
        // copy of it open until its exec.
        let launch = InstanceLock::acquire(&path, LOCK_PATIENCE).expect("the lock is free");
        assert!(
            InstanceLock::acquire(&path, Duration::ZERO).is_none(),
            "one holder at a time"
        );
        drop(guard);
        assert!(
            path.exists(),
            "left for the launch that is looking at it to clear away"
        );
        drop(launch);
        drop(SingleInstance {
            socket: SocketFile::at(&path),
        });
        assert!(!path.exists());
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

    #[test]
    fn wayland_is_used_wherever_the_window_library_would_use_it() {
        let os = |text: &str| Some(OsString::from(text));
        assert!(names_wayland(os("wayland-0"), None));
        assert!(names_wayland(None, os("5")));
        assert!(names_wayland(os(""), os("5")));
        // Empty counts as unset, as it does for the window library.
        assert!(!names_wayland(None, None));
        assert!(!names_wayland(os(""), os("")));
    }
}
