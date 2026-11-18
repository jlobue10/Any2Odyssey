//! Output through Leia's own weaver: the calibrated, eye-tracked picture
//! of the Odyssey 3D.
//!
//! Leia's weaver interleaves two views for the lens with the panel's factory
//! calibration and the viewer's eye positions. It is part of the SR runtime,
//! which this program neither contains nor links. As with the lens switch
//! (`lens_switch`), a separate helper does the talking: `any2odyssey-weaver`
//! (`contrib/wine-lens`) starts Leia's service and eye tracker, opens its own
//! full-screen window on the Odyssey, asks for the lens and weaves whatever
//! picture this module writes into a file the two share.
//!
//! The helper's contract, through its standard streams: it prints `on` once
//! a picture is showing, `eyes 0` / `eyes 1` as eye tracking is lost or
//! found, `tracking: <reason>` if the tracker could not be started, or
//! `error: <reason>` and exits; it leaves when its input closes. Esc in its
//! window makes it leave by itself.
//!
//! The shared file: a 4096-byte header, then three places for a picture,
//! each `capacity` bytes. The header is `A2OWEAV2`, then little-endian
//! 32-bit numbers: capacity, a sequence number (odd while the header is
//! being changed, raised by two per picture), width, height and the place
//! the newest picture is in. A picture is RGBA rows from the bottom row up
//! (the order OpenGL wants), the left view in the left half and the right
//! view in the right half; each half is shown stretched over the whole
//! panel. Pictures go round the three places, so the one being shown is
//! never the one being written, however long a large picture takes.

use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::TryRecvError;
use std::sync::{Arc, Mutex};

use any2odyssey_core::resample::{resize_into, Part};
use image::RgbaImage;

use crate::lens_switch::{self, Running};

pub const HELPER_NAME: &str = "any2odyssey-weaver";
/// A full path to a helper, for trying one without installing it.
pub const HELPER_ENV: &str = "ANY2ODYSSEY_WEAVER";
/// The Odyssey 3D's panel, in pixels.
pub const PANEL: (u32, u32) = (3840, 2160);
/// One view of a still picture. The lens gives each eye about half of the
/// panel's columns, so half the panel's width holds what can be seen.
pub const STILL_VIEW: (u32, u32) = (PANEL.0 / 2, PANEL.1);

const MAGIC: &[u8; 8] = b"A2OWEAV2";
/// Places a picture can be in; the newest is never written over.
const PLACES: u32 = 3;
const HEADER_BYTES: u64 = 4096;
/// Room for a pair of views at the panel's full size.
const CAPACITY: u32 = PANEL.0 * 2 * PANEL.1 * 4;
/// The frame file's whole length. It is made at that length without the
/// room being taken, so a folder must be known to have the room.
const FILE_BYTES: u64 = HEADER_BYTES + CAPACITY as u64 * PLACES as u64;

/// The bytes this user may still write in the file system `folder` is on;
/// `None` where that cannot be asked.
#[cfg(all(target_os = "linux", target_pointer_width = "64"))]
fn room_in(folder: &Path) -> Option<u64> {
    use std::ffi::{c_char, c_int, CString};
    use std::os::unix::ffi::OsStrExt;
    extern "C" {
        fn statvfs(path: *const c_char, answer: *mut [u64; 32]) -> c_int;
    }
    let path = CString::new(folder.as_os_str().as_bytes()).ok()?;
    let mut answer = [0u64; 32];
    // SAFETY: `path` ends in a zero byte and lives across the call. The
    // answer is a `struct statvfs`, which on 64-bit Linux (glibc and musl)
    // is eleven 8-byte numbers and 24 spare bytes, 112 in all: it fits the
    // 256 bytes given with room to spare.
    if unsafe { statvfs(path.as_ptr(), &mut answer) } != 0 {
        return None;
    }
    // The second number is the size of a block (f_frsize), the fifth the
    // blocks free to this user (f_bavail).
    answer[1].checked_mul(answer[4])
}

#[cfg(not(all(target_os = "linux", target_pointer_width = "64")))]
fn room_in(_: &Path) -> Option<u64> {
    None
}

/// Whether a folder with `room` bytes left can take the frame file. A
/// folder that does not say is tried.
fn has_room(room: Option<u64>) -> bool {
    room.is_none_or(|room| room >= FILE_BYTES)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WeaverStatus {
    Idle,
    /// No helper is installed: this output is not available.
    NoHelper,
    /// The helper is running and no picture is showing yet.
    Starting,
    /// Showing. `eyes` is whether the tracker sees the viewer, once known;
    /// `note` is why eye tracking is not running, if it is not.
    On {
        eyes: Option<bool>,
        note: Option<String>,
    },
    Failed(String),
}

impl WeaverStatus {
    /// One line for the page.
    pub fn describe(&self) -> String {
        match self {
            WeaverStatus::Idle => {
                "Odyssey 3D output: off. Start output to show the picture on the Odyssey.".into()
            }
            WeaverStatus::NoHelper => {
                "Odyssey 3D output: not set up (see docs/LENS-SWITCH.md, contrib/wine-lens).".into()
            }
            WeaverStatus::Starting => {
                "Odyssey 3D output: starting Leia's service and eye tracker (up to half a minute)…"
                    .into()
            }
            WeaverStatus::On {
                note: Some(note), ..
            } => format!("Odyssey 3D output: on, without eye tracking ({note})."),
            WeaverStatus::On {
                eyes: Some(true), ..
            } => "Odyssey 3D output: on, following your eyes.".into(),
            WeaverStatus::On {
                eyes: Some(false), ..
            } => "Odyssey 3D output: on, but the tracker does not see you (the picture is 2D until it does).".into(),
            WeaverStatus::On { eyes: None, .. } => "Odyssey 3D output: on.".into(),
            WeaverStatus::Failed(reason) => format!("Odyssey 3D output failed: {reason}"),
        }
    }
}

/// Where a helper is looked for; the same places as the lens helper.
pub fn locate() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(HELPER_ENV).filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(path));
    }
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(HELPER_NAME)));
    let own = any2odyssey_core::paths::data_dir().join(HELPER_NAME);
    let installed = ["/usr/lib/any2odyssey", "/usr/local/lib/any2odyssey"]
        .into_iter()
        .map(|dir| Path::new(dir).join(HELPER_NAME));
    beside
        .into_iter()
        .chain(std::iter::once(own))
        .chain(installed)
        .find(|p| p.is_file())
}

/// A frame file's name: this, the number of the process that made it, a
/// hyphen, a part that cannot be guessed (a running number in earlier
/// versions) and the ending.
const FRAME_PREFIX: &str = "any2odyssey-weaver-";
const FRAME_ENDING: &str = ".frames";

/// The folders a frame file may go to, best first: the session's runtime
/// folder (memory, and this user's alone), the system's shared memory
/// (memory, but everybody's: what keeps another user from taking the name
/// first is that it cannot be guessed), and the program's own data folder
/// (the disk, where every picture is so much writing: the last resort). A
/// runtime folder that is not a whole path could be anywhere and is not
/// believed.
fn frame_folders(runtime: Option<PathBuf>, shared: &Path, data: PathBuf) -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = runtime.into_iter().filter(|d| d.is_absolute()).collect();
    if shared.is_dir() {
        folders.push(shared.to_owned());
    }
    folders.push(data);
    folders
}

/// The frame file's picture places as memory of this process, where the
/// file itself lives in memory (a `tmpfs`: the session's runtime folder,
/// `/dev/shm`).
///
/// Written with `write`, a picture goes to the system a page at a time, and
/// for a file in memory that is all there is to do: the 66 MB of a pair of
/// 2160-line views took longer to hand over than to render, the largest
/// single part of a live picture's turn. A shared mapping is those same
/// pages, and copying into it puts the same bytes at the same places of the
/// same file, where the helper reads them as before. Only the picture's
/// bytes go this way; the header is written as it always was.
///
/// Two things a `write` cannot do and a store into a mapping can are kept
/// from happening. A page that has no memory yet is given one when it is
/// first touched, and where there is none left the process is killed
/// (SIGBUS) instead of told: so a place is used only once the system has
/// set its memory aside (`fallocate`), and where it will not, the picture
/// is written as before, with the error that gives. And a file on a disk is
/// never mapped (there, setting aside is not always a promise).
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod mapped {
    use std::ffi::{c_int, c_void};
    use std::fs::File;
    use std::os::fd::AsRawFd;

    extern "C" {
        fn fstatfs(fd: c_int, answer: *mut [u64; 32]) -> c_int;
        fn mmap(
            address: *mut c_void,
            length: usize,
            protection: c_int,
            flags: c_int,
            fd: c_int,
            offset: i64,
        ) -> *mut c_void;
        fn munmap(address: *mut c_void, length: usize) -> c_int;
        fn fallocate(fd: c_int, mode: c_int, offset: i64, length: i64) -> c_int;
    }

    /// What `fstatfs` calls a file system that keeps its files in memory.
    const TMPFS_MAGIC: u64 = 0x0102_1994;
    const PROT_READ_WRITE: c_int = 1 | 2;
    const MAP_SHARED: c_int = 1;

    pub struct Places {
        start: *mut u8,
        length: usize,
    }

    // SAFETY: the mapping belongs to this value alone and is reached only
    // through it; nothing about it is tied to the thread that made it.
    unsafe impl Send for Places {}

    impl Places {
        /// Map the `length` bytes of `file`; `None` when the file is not
        /// kept in memory or cannot be mapped.
        pub fn of(file: &File, length: u64) -> Option<Places> {
            let length = usize::try_from(length).ok().filter(|&l| l > 0)?;
            let mut answer = [0u64; 32];
            // SAFETY: the descriptor is open for the call. The answer is a
            // `struct statfs`, which on x86_64 and aarch64 Linux (glibc and
            // musl) is 120 bytes and begins with the 8-byte type of the
            // file system: it fits the 256 bytes given.
            if unsafe { fstatfs(file.as_raw_fd(), &mut answer) } != 0 || answer[0] != TMPFS_MAGIC {
                return None;
            }
            // SAFETY: a new shared mapping of an open file at an address
            // the system chooses; nothing of this process is replaced.
            let start = unsafe {
                mmap(
                    std::ptr::null_mut(),
                    length,
                    PROT_READ_WRITE,
                    MAP_SHARED,
                    file.as_raw_fd(),
                    0,
                )
            };
            // The failure value is the address with every bit set.
            if start.is_null() || start as usize == usize::MAX {
                return None;
            }
            Some(Places {
                start: start.cast(),
                length,
            })
        }

        /// Have the system set memory aside for `bytes` bytes of the file
        /// from `at` on. False when it does not (there is none left, or
        /// this file system cannot): those bytes must then not be touched
        /// through the mapping.
        pub fn reserve(&self, file: &File, at: u64, bytes: usize) -> bool {
            let (Ok(at), Ok(bytes)) = (i64::try_from(at), i64::try_from(bytes)) else {
                return false;
            };
            // SAFETY: two numbers and an open descriptor.
            bytes > 0 && unsafe { fallocate(file.as_raw_fd(), 0, at, bytes) } == 0
        }

        /// The `bytes` bytes of the file from `at` on, to write to. `None`
        /// outside the mapping.
        pub fn slice(&mut self, at: u64, bytes: usize) -> Option<&mut [u8]> {
            let at = usize::try_from(at).ok()?;
            if at.checked_add(bytes)? > self.length {
                return None;
            }
            // SAFETY: the range lies inside the mapping, which stays until
            // this value is dropped, and `&mut self` keeps every other use
            // of it in this process away for as long as the slice lives.
            // The helper maps the file for reading only; the bytes are
            // only ever stored here, never read.
            Some(unsafe { std::slice::from_raw_parts_mut(self.start.add(at), bytes) })
        }
    }

    impl Drop for Places {
        fn drop(&mut self) {
            // SAFETY: exactly the mapping made in `of`, and no slice of it
            // outlives this value.
            unsafe { munmap(self.start.cast(), self.length) };
        }
    }
}

/// Elsewhere there are no picture places as memory: a picture is written.
#[cfg(not(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
)))]
mod mapped {
    use std::convert::Infallible;
    use std::fs::File;

    pub struct Places(Infallible);

    impl Places {
        pub fn of(_: &File, _: u64) -> Option<Places> {
            None
        }

        pub fn reserve(&self, _: &File, _: u64, _: usize) -> bool {
            match self.0 {}
        }

        pub fn slice(&mut self, _: u64, _: usize) -> Option<&mut [u8]> {
            match self.0 {}
        }
    }
}

/// The file the pictures go through. It lives in memory where it can (the
/// session's runtime folder) and only this user can read it.
struct FrameFile {
    file: File,
    path: PathBuf,
    sequence: u32,
    /// The place the newest picture is in.
    place: u32,
    /// Working room for turning a picture upside down, where it is written
    /// and not copied into its place.
    flipped: Vec<u8>,
    /// The places as memory, where the file lives in memory (`mapped`),
    /// and how many bytes of each have had memory set aside.
    mapped: Option<mapped::Places>,
    reserved: [usize; PLACES as usize],
}

impl FrameFile {
    fn create() -> std::io::Result<FrameFile> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
        let data = any2odyssey_core::paths::data_dir();
        if !runtime.as_ref().is_some_and(|dir| dir.is_absolute()) {
            // Where earlier versions put the file without a runtime
            // folder; one of them may have left its file there.
            remove_orphans(&std::env::temp_dir());
        }
        let mut failed = None;
        for folder in frame_folders(runtime, Path::new("/dev/shm"), data.clone()) {
            if folder == data {
                let _ = std::fs::create_dir_all(&folder);
            }
            remove_orphans(&folder);
            // The file is made at its whole length without the room being
            // taken. In a folder too small for it (shared memory of 64 MB
            // in a container, a small runtime folder) the making works and
            // the second or third picture cannot be written, which ends
            // the output: such a folder is passed over while there is
            // another to try.
            if !has_room(room_in(&folder)) {
                failed = Some(std::io::Error::other(format!(
                    "{} has less than the {} MB the pictures need",
                    folder.display(),
                    FILE_BYTES.div_ceil(1 << 20)
                )));
                continue;
            }
            match FrameFile::create_in(&folder) {
                Ok(file) => return Ok(file),
                // Full, or not to be written to: the next folder may do.
                Err(e) => failed = Some(e),
            }
        }
        Err(failed.unwrap_or_else(|| std::io::Error::other("no folder for the frame file")))
    }

    fn create_in(folder: &Path) -> std::io::Result<FrameFile> {
        // One file per session, made new, for this user only, under a name
        // of which a part is drawn at random: a file or a link somebody
        // put there first is never taken or followed, only passed over for
        // another name.
        let mut named = tempfile::Builder::new()
            .prefix(&format!("{FRAME_PREFIX}{}-", std::process::id()))
            .suffix(FRAME_ENDING)
            .rand_bytes(16)
            .tempfile_in(folder)?;
        let mut header = [0u8; 24];
        header[..8].copy_from_slice(MAGIC);
        header[8..12].copy_from_slice(&CAPACITY.to_le_bytes());
        let file = named.as_file_mut();
        file.set_len(FILE_BYTES)?;
        file.write_all(&header)?;
        // From here on the file is removed with the `FrameFile`.
        let (file, path) = named.keep().map_err(|e| e.error)?;
        let mapped = mapped::Places::of(&file, FILE_BYTES);
        Ok(FrameFile {
            file,
            path,
            sequence: 0,
            place: 0,
            flipped: Vec::new(),
            mapped,
            reserved: [0; PLACES as usize],
        })
    }

    fn put(&mut self, at: u64, bytes: &[u8]) -> std::io::Result<()> {
        self.file.seek(SeekFrom::Start(at))?;
        self.file.write_all(bytes)
    }

    /// Where `place` begins in the file.
    fn start_of(place: u32) -> u64 {
        HEADER_BYTES + u64::from(place) * u64::from(CAPACITY)
    }

    /// The first `bytes` bytes of `place` as memory to copy a picture
    /// into, where the file is mapped and the system has set that memory
    /// aside. `None` otherwise: the picture is written then. A place whose
    /// memory is refused ends the copying for this file, so the refusal is
    /// not asked for again at every picture and the write that follows
    /// says what is wrong.
    fn memory_of(&mut self, place: u32, bytes: usize) -> Option<&mut [u8]> {
        let at = FrameFile::start_of(place);
        let reserved = &mut self.reserved[place as usize];
        if *reserved < bytes {
            if !self.mapped.as_ref()?.reserve(&self.file, at, bytes) {
                self.mapped = None;
                return None;
            }
            *reserved = bytes;
        }
        self.mapped.as_mut()?.slice(at, bytes)
    }

    /// Whether a picture of this size, given as `given` bytes, can be shown.
    fn fits(width: u32, height: u32, given: usize) -> bool {
        let bytes = u64::from(width) * u64::from(height) * 4;
        width != 0 && height != 0 && bytes <= u64::from(CAPACITY) && given as u64 == bytes
    }

    /// Replace the picture with `rgba`, rows from the top down.
    fn show(&mut self, width: u32, height: u32, rgba: &[u8]) -> std::io::Result<()> {
        let row = width as usize * 4;
        if row == 0 || rgba.len() != row * height as usize {
            return Err(does_not_fit());
        }
        if !FrameFile::fits(width, height, rgba.len()) {
            return Err(does_not_fit());
        }
        let place = (self.place + 1) % PLACES;
        if let Some(memory) = self.memory_of(place, rgba.len()) {
            // Turned upside down on its way into its place.
            for (into, pixels) in memory
                .chunks_exact_mut(row)
                .zip(rgba.chunks_exact(row).rev())
            {
                into.copy_from_slice(pixels);
            }
            return self.point_at(width, height, place);
        }
        let mut flipped = std::mem::take(&mut self.flipped);
        flipped.clear();
        flipped.reserve(rgba.len());
        for pixels in rgba.chunks_exact(row).rev() {
            flipped.extend_from_slice(pixels);
        }
        let shown = self.show_bottom_up(width, height, &flipped);
        self.flipped = flipped;
        shown
    }

    /// Replace the picture with `rgba`, rows from the bottom up: put into
    /// the next place, then the header is pointed at it.
    fn show_bottom_up(&mut self, width: u32, height: u32, rgba: &[u8]) -> std::io::Result<()> {
        if !FrameFile::fits(width, height, rgba.len()) {
            return Err(does_not_fit());
        }
        let place = (self.place + 1) % PLACES;
        match self.memory_of(place, rgba.len()) {
            Some(memory) => memory.copy_from_slice(rgba),
            None => self.put(FrameFile::start_of(place), rgba)?,
        }
        self.point_at(width, height, place)
    }

    /// Point the header at the picture in `place`, which is whole by now.
    /// The sequence is odd while the header changes, so the helper never
    /// takes a half-written header for a whole one.
    fn point_at(&mut self, width: u32, height: u32, place: u32) -> std::io::Result<()> {
        // A picture copied into its place was stored by this processor;
        // the header is written by the system. Whoever sees the header
        // must see the picture: nothing is left to the order in which a
        // processor makes its stores known.
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
        self.sequence = self.sequence.wrapping_add(1) | 1;
        self.put(12, &self.sequence.to_le_bytes())?;
        let mut fields = [0u8; 12];
        fields[..4].copy_from_slice(&width.to_le_bytes());
        fields[4..8].copy_from_slice(&height.to_le_bytes());
        fields[8..].copy_from_slice(&place.to_le_bytes());
        self.put(16, &fields)?;
        self.place = place;
        self.sequence = self.sequence.wrapping_add(1);
        self.put(12, &self.sequence.to_le_bytes())
    }
}

/// Remove frame files left by instances of this program that are gone (one
/// that was killed never got to remove its own). They live in memory, up to
/// two hundred megabytes each.
#[cfg(target_os = "linux")]
fn remove_orphans(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name
            .to_str()
            .and_then(|name| name.strip_prefix(FRAME_PREFIX))
            .and_then(|rest| rest.strip_suffix(FRAME_ENDING))
            .and_then(|rest| rest.split('-').next())
            .and_then(|pid| pid.parse::<u32>().ok())
        else {
            continue;
        };
        if pid != std::process::id() && !Path::new("/proc").join(pid.to_string()).exists() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn remove_orphans(_: &Path) {}

fn does_not_fit() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "the picture does not fit the frame file",
    )
}

impl Drop for FrameFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Where another thread puts its pictures while a session lasts (a live
/// source renders off the UI thread). Pictures given after the session has
/// ended go nowhere.
#[derive(Clone)]
pub struct FrameSink(Arc<Mutex<FrameFile>>);

impl FrameSink {
    /// Replace the picture; see [`Weaver::show`].
    pub fn show(&self, width: u32, height: u32, rgba: &[u8]) -> std::io::Result<()> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .show(width, height, rgba)
    }

    /// Replace the picture with one whose rows run from the bottom up,
    /// which is how the file holds it: nothing is copied but the picture.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn show_bottom_up(&self, width: u32, height: u32, rgba: &[u8]) -> std::io::Result<()> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .show_bottom_up(width, height, rgba)
    }
}

#[cfg(all(test, target_os = "linux"))]
impl FrameSink {
    /// A picture file in `folder` that nobody reads.
    pub(crate) fn in_folder(folder: &Path) -> std::io::Result<FrameSink> {
        Ok(FrameSink(Arc::new(Mutex::new(FrameFile::create_in(
            folder,
        )?))))
    }

    /// How many pictures have been written, and the newest as a reader
    /// finds it: width, height and the bytes.
    pub(crate) fn written(&self) -> (u32, u32, u32, Vec<u8>) {
        use std::os::unix::fs::FileExt;
        let frames = self.0.lock().unwrap();
        let mut header = [0u8; 28];
        frames.file.read_exact_at(&mut header, 0).unwrap();
        let field = |at: usize| u32::from_le_bytes(header[at..at + 4].try_into().unwrap());
        let (sequence, width, height, place) = (field(12), field(16), field(20), field(24));
        assert_eq!(sequence % 2, 0, "a finished picture");
        let mut bytes = vec![0u8; width as usize * height as usize * 4];
        let at = HEADER_BYTES + u64::from(place) * u64::from(CAPACITY);
        frames.file.read_exact_at(&mut bytes, at).unwrap();
        (sequence / 2, width, height, bytes)
    }
}

struct Session {
    /// Whether the desktop keeps the picture out of screen captures, once
    /// the helper has said (it says so for a live screen only).
    excluded: Option<bool>,
    running: Running,
    frames: FrameSink,
    #[cfg(all(test, unix))]
    path: PathBuf,
    eyes: Option<bool>,
    note: Option<String>,
}

pub struct Weaver {
    helper: Option<PathBuf>,
    session: Option<Session>,
    failed: Option<String>,
}

impl Weaver {
    /// The output with the installed helper, if there is one.
    pub fn new() -> Weaver {
        Weaver::with_helper(locate())
    }

    pub fn with_helper(helper: Option<PathBuf>) -> Weaver {
        Weaver {
            helper,
            session: None,
            failed: None,
        }
    }

    pub fn available(&self) -> bool {
        self.helper.is_some()
    }

    pub fn is_running(&self) -> bool {
        self.session.is_some()
    }

    /// Forget the last failure: a new request to show something starts
    /// afresh instead of reporting what went wrong last time.
    pub fn forget_failure(&mut self) {
        self.failed = None;
    }

    /// Start the helper. `overlay` asks for a window that lets the pointer
    /// through (a live desktop); `wake` is called from another thread
    /// whenever the helper has said something.
    pub fn start(&mut self, overlay: bool, wake: impl Fn() + Send + 'static) {
        if self.session.is_some() {
            return;
        }
        self.failed = None;
        let Some(helper) = self.helper.clone() else {
            return;
        };
        let frames = match FrameFile::create() {
            Ok(frames) => frames,
            Err(e) => {
                self.failed = Some(lens_switch::tidy(&format!(
                    "could not create the frame file: {e}"
                )));
                return;
            }
        };
        let path = frames.path.clone();
        let mut command = Command::new(&helper);
        command.arg(&path);
        if overlay {
            command.arg("overlay");
        }
        match lens_switch::start(command, wake) {
            Ok(running) => {
                self.session = Some(Session {
                    excluded: None,
                    running,
                    frames: FrameSink(Arc::new(Mutex::new(frames))),
                    #[cfg(all(test, unix))]
                    path,
                    eyes: None,
                    note: None,
                })
            }
            Err(e) => {
                self.failed = Some(lens_switch::tidy(&format!(
                    "could not start {}: {e}",
                    helper.display()
                )))
            }
        }
    }

    /// Put a side-by-side picture on the Odyssey. Nothing happens while no
    /// helper is running.
    pub fn show(&mut self, width: u32, height: u32, rgba: &[u8]) {
        let Some(session) = &mut self.session else {
            return;
        };
        if let Err(e) = session.frames.show(width, height, rgba) {
            self.failed = Some(lens_switch::tidy(&format!(
                "could not write the picture: {e}"
            )));
            self.stop();
        }
    }

    /// Whether the picture is kept out of screen captures, so that the
    /// Odyssey's own screen can be the one shown: `None` until the helper
    /// has said, or when it does not say.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn excluded_from_capture(&self) -> Option<bool> {
        self.session.as_ref().and_then(|session| session.excluded)
    }

    /// The running session's picture, for another thread to write.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub fn sink(&self) -> Option<FrameSink> {
        self.session.as_ref().map(|session| session.frames.clone())
    }

    /// Take in what the helper has said. True when it has left by itself
    /// without reporting an error (Esc in its window): the output is over.
    pub fn poll(&mut self) -> bool {
        let Some(session) = &mut self.session else {
            return false;
        };
        let gone = loop {
            match session.running.lines.try_recv() {
                Ok(line) => {
                    let line = line.trim();
                    if line == "on" {
                        session.running.on = true;
                    } else if line == "eyes 1" {
                        session.eyes = Some(true);
                    } else if line == "eyes 0" {
                        session.eyes = Some(false);
                    } else if line == "capture: excluded" {
                        session.excluded = Some(true);
                    } else if line == "capture: not excluded" {
                        session.excluded = Some(false);
                    } else if let Some(reason) = line.strip_prefix("tracking:") {
                        session.note = Some(lens_switch::tidy(reason));
                    } else if let Some(reason) = line.strip_prefix("error:") {
                        self.failed = Some(lens_switch::tidy(reason));
                    }
                }
                Err(TryRecvError::Empty) => break false,
                Err(TryRecvError::Disconnected) => break true,
            }
        };
        if !gone && self.failed.is_none() {
            return false;
        }
        let was_on = session.running.on;
        if !was_on {
            // It never showed a picture: leaving without a word is a failure.
            self.failed
                .get_or_insert_with(|| "the helper stopped unexpectedly".into());
        }
        let ended_quietly = self.failed.is_none();
        self.stop();
        ended_quietly
    }

    pub fn stop(&mut self) {
        if let Some(session) = self.session.take() {
            session.running.let_go();
        }
    }

    pub fn status(&self) -> WeaverStatus {
        if let Some(reason) = &self.failed {
            return WeaverStatus::Failed(reason.clone());
        }
        match &self.session {
            Some(session) if session.running.on => WeaverStatus::On {
                eyes: session.eyes,
                note: session.note.clone(),
            },
            Some(_) => WeaverStatus::Starting,
            None if self.helper.is_none() => WeaverStatus::NoHelper,
            None => WeaverStatus::Idle,
        }
    }
}

impl Drop for Weaver {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Where a view `eye` pixels large, each pixel `stretch` times as wide as it
/// is tall, goes inside one `view`-sized half of the picture: as large as
/// the panel allows without distortion, centred. Returns (x, y, width,
/// height) in the half's own pixels, which are not square: the half is
/// shown stretched over the whole panel.
pub fn place_view(eye: (u32, u32), stretch: f64, view: (u32, u32)) -> (u32, u32, u32, u32) {
    let (panel_w, panel_h) = (f64::from(PANEL.0), f64::from(PANEL.1));
    let shown_w = f64::from(eye.0.max(1)) * stretch.max(f64::MIN_POSITIVE);
    let shown_h = f64::from(eye.1.max(1));
    let scale = (panel_w / shown_w).min(panel_h / shown_h);
    let w = ((shown_w * scale / panel_w) * f64::from(view.0)).round();
    let h = ((shown_h * scale / panel_h) * f64::from(view.1)).round();
    let w = (w as u32).clamp(1, view.0);
    let h = (h as u32).clamp(1, view.1);
    ((view.0 - w) / 2, (view.1 - h) / 2, w, h)
}

/// The side-by-side picture for the weaver from a still image: `pair` holds
/// the left and right views side by side (`flat`: one view for both eyes),
/// each fitted into its half on black.
pub fn compose_still(pair: &RgbaImage, flat: bool, stretch: f64) -> RgbaImage {
    let view = STILL_VIEW;
    let mut canvas = RgbaImage::from_pixel(view.0 * 2, view.1, image::Rgba([0, 0, 0, 255]));
    let eye_w = if flat { pair.width() } else { pair.width() / 2 };
    let eye = (eye_w.max(1), pair.height().max(1));
    let (x, y, w, h) = place_view(eye, if flat { 1.0 } else { stretch }, view);
    let part = |side: u32| Part::of(pair, side * eye_w, eye.0);
    // Each view is resized straight into its half, its rows shared out over
    // the cores (`core::resample`: the image crate's triangle resize and
    // replace, byte for byte, which made the whole resized view twice over
    // before a pixel reached the picture and were most of what a step of a
    // slider cost). A flat picture is the same in both eyes: it is resized
    // once and each row copied to the other half.
    if flat {
        resize_into(&mut canvas, part(0), &[x, view.0 + x], y, w, h);
    } else {
        resize_into(&mut canvas, part(0), &[x], y, w, h);
        resize_into(&mut canvas, part(1), &[view.0 + x], y, w, h);
    }
    canvas
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::imageops;

    #[test]
    fn a_view_is_fitted_to_the_panel_not_to_its_half() {
        let view = (1920, 2160);
        // A 16:9 view fills its half entirely, whatever its size.
        assert_eq!(place_view((1920, 1080), 1.0, view), (0, 0, 1920, 2160));
        assert_eq!(place_view((960, 1080), 2.0, view), (0, 0, 1920, 2160));
        // A square view is as tall as the panel and 9/16 of its width.
        assert_eq!(place_view((1000, 1000), 1.0, view), (420, 0, 1080, 2160));
        // A very wide view is letterboxed.
        assert_eq!(place_view((3200, 900), 1.0, view), (0, 540, 1920, 1080));
        // Degenerate sizes still give a rectangle inside the half.
        let (x, y, w, h) = place_view((0, 0), 0.0, view);
        assert!(w >= 1 && h >= 1 && x + w <= view.0 && y + h <= view.1);
    }

    #[test]
    fn a_still_pair_keeps_its_views_on_their_own_sides() {
        // Left eye red, right eye blue, 16:9 per eye.
        let pair = RgbaImage::from_fn(64, 18, |x, _| {
            if x < 32 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            }
        });
        let canvas = compose_still(&pair, false, 1.0);
        assert_eq!(canvas.dimensions(), (STILL_VIEW.0 * 2, STILL_VIEW.1));
        assert_eq!(canvas.get_pixel(10, 10).0, [255, 0, 0, 255]);
        assert_eq!(
            canvas.get_pixel(STILL_VIEW.0 - 10, 2000).0,
            [255, 0, 0, 255]
        );
        assert_eq!(canvas.get_pixel(STILL_VIEW.0 + 10, 10).0, [0, 0, 255, 255]);
        // A flat image goes to both eyes.
        let flat = RgbaImage::from_pixel(32, 18, image::Rgba([7, 8, 9, 255]));
        let canvas = compose_still(&flat, true, 1.0);
        assert_eq!(
            canvas.get_pixel(5, 5),
            canvas.get_pixel(STILL_VIEW.0 + 5, 5)
        );
        assert_eq!(canvas.get_pixel(5, 5).0, [7, 8, 9, 255]);
    }

    /// `compose_still` as it was while the image crate resized each view
    /// whole and its pixels were then copied into the picture.
    fn compose_still_in_turn(pair: &RgbaImage, flat: bool, stretch: f64) -> RgbaImage {
        let view = STILL_VIEW;
        let mut canvas = RgbaImage::from_pixel(view.0 * 2, view.1, image::Rgba([0, 0, 0, 255]));
        let eye_w = if flat { pair.width() } else { pair.width() / 2 };
        let eye = (eye_w.max(1), pair.height().max(1));
        let (x, y, w, h) = place_view(eye, if flat { 1.0 } else { stretch }, view);
        for side in 0..if flat { 1 } else { 2u32 } {
            let from_x = if flat { 0 } else { side * eye_w };
            let part = imageops::crop_imm(pair, from_x, 0, eye.0, eye.1);
            let fitted = imageops::resize(&*part, w, h, imageops::FilterType::Triangle);
            imageops::replace(
                &mut canvas,
                &fitted,
                i64::from(side * view.0 + x),
                i64::from(y),
            );
            if flat {
                imageops::replace(&mut canvas, &fitted, i64::from(view.0 + x), i64::from(y));
            }
        }
        canvas
    }

    #[test]
    fn views_resized_row_by_row_give_the_bytes_the_image_crate_gave() {
        // Every pixel differs from its neighbours and the two views differ.
        let noisy = |w: u32, h: u32| {
            RgbaImage::from_fn(w, h, |x, y| {
                let v = (x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)).to_le_bytes();
                image::Rgba([v[0], v[1], v[2] ^ v[3], 255 - v[1] / 3])
            })
        };
        for (w, h, flat, stretch) in [
            (641, 181, false, 1.0),
            (640, 180, false, 2.0),
            (300, 400, false, 0.5),
            (1, 1, false, 1.0),
            (641, 181, true, 1.0),
            // A view larger than its half in both directions, and one that
            // is exactly its half (a half-width pair of the panel's size:
            // nothing is resized).
            (4100, 1200, false, 1.0),
            (3840, 2160, false, 2.0),
            (1920, 2160, true, 1.0),
            // Pictures that have no view to show.
            (0, 0, false, 1.0),
            (0, 7, true, 1.0),
            (2, 0, false, 1.0),
            (1, 9, false, 1.0),
        ] {
            let pair = noisy(w, h);
            assert!(
                compose_still(&pair, flat, stretch).as_raw()
                    == compose_still_in_turn(&pair, flat, stretch).as_raw(),
                "{w}x{h} flat={flat} stretch={stretch}"
            );
        }
    }

    #[test]
    fn a_flat_still_duplicates_detail_and_alpha_with_centered_black_borders() {
        let flat = RgbaImage::from_fn(3, 4, |x, y| {
            image::Rgba([(x * 70) as u8, (y * 60) as u8, 90, (x * 30 + y * 40) as u8])
        });
        // Flat ignores the SBS eye stretch. A 3:4 image fills the panel
        // height and occupies 810 pixels of each 1920-wide half.
        let canvas = compose_still(&flat, true, 2.0);
        let fitted = imageops::resize(&flat, 810, 2160, imageops::FilterType::Triangle);
        for y in 0..STILL_VIEW.1 {
            for x in 0..STILL_VIEW.0 {
                let expected = if (555..1365).contains(&x) {
                    *fitted.get_pixel(x - 555, y)
                } else {
                    image::Rgba([0, 0, 0, 255])
                };
                assert_eq!(*canvas.get_pixel(x, y), expected);
                assert_eq!(*canvas.get_pixel(x + STILL_VIEW.0, y), expected);
            }
        }
    }

    #[test]
    fn a_folder_too_small_for_the_frame_file_is_passed_over() {
        assert_eq!(
            FILE_BYTES,
            HEADER_BYTES + u64::from(CAPACITY) * u64::from(PLACES)
        );
        // A container's 64 MB of shared memory takes the first picture and
        // not the second; a folder that does not say is tried.
        assert!(!has_room(Some(64 << 20)));
        assert!(!has_room(Some(FILE_BYTES - 1)));
        assert!(has_room(Some(FILE_BYTES)));
        assert!(has_room(None));
        // Where the question can be asked, a real folder answers it.
        #[cfg(all(target_os = "linux", target_pointer_width = "64"))]
        {
            let dir = tempfile::tempdir().unwrap();
            assert!(room_in(dir.path()).is_some_and(|room| room > 0));
            assert_eq!(room_in(&dir.path().join("none")), None);
        }
    }

    #[test]
    fn the_frame_file_goes_to_memory_that_is_the_users_own_before_anywhere_else() {
        let dir = tempfile::tempdir().unwrap();
        let (runtime, shared, data) = (
            dir.path().join("run"),
            dir.path().join("shm"),
            dir.path().join("data"),
        );
        let missing = dir.path().join("none");
        std::fs::create_dir(&shared).unwrap();
        // (The runtime folder is believed without looking: the session made it.)
        assert_eq!(
            frame_folders(Some(runtime.clone()), &shared, data.clone()),
            [runtime.clone(), shared.clone(), data.clone()]
        );
        // No runtime folder, an empty one or one that is not a whole path:
        // shared memory under a name nobody can guess, never a folder that
        // depends on where the program was started.
        for runtime in [None, Some(PathBuf::new()), Some(PathBuf::from("run/user"))] {
            assert_eq!(
                frame_folders(runtime, &shared, data.clone()),
                [shared.clone(), data.clone()]
            );
        }
        // Without shared memory either, the program's own folder.
        assert_eq!(
            frame_folders(None, &missing, data.clone()),
            std::slice::from_ref(&data)
        );
        assert_eq!(
            frame_folders(Some(runtime.clone()), &missing, data.clone()),
            [runtime, data]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_frame_file_is_new_private_and_under_a_name_that_cannot_be_guessed() {
        let dir = tempfile::tempdir().unwrap();
        let first = FrameFile::create_in(dir.path()).unwrap();
        let second = FrameFile::create_in(dir.path()).unwrap();
        assert_ne!(first.path, second.path);
        let own = format!("{FRAME_PREFIX}{}-", std::process::id());
        for file in [&first, &second] {
            assert_eq!(file.path.parent(), Some(dir.path()));
            let name = file.path.file_name().unwrap().to_str().unwrap();
            let drawn = name
                .strip_prefix(&own)
                .and_then(|rest| rest.strip_suffix(FRAME_ENDING))
                .unwrap_or_else(|| panic!("{name}"));
            // The part between is the drawn one: long, and without a hyphen
            // (the number before the first hyphen is the process's).
            assert!(drawn.len() >= 16 && !drawn.contains('-'), "{name}");
            let bytes = std::fs::metadata(&file.path).unwrap().len();
            assert_eq!(
                bytes,
                HEADER_BYTES + u64::from(CAPACITY) * u64::from(PLACES)
            );
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&file.path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "{name}");
        }
        let (kept, gone) = (second.path.clone(), first.path.clone());
        drop(first);
        assert!(!gone.exists() && kept.exists());
        // A folder that is not there is an error, not a file somewhere else.
        assert!(FrameFile::create_in(&dir.path().join("none")).is_err());
    }

    /// The header and the first `bytes` bytes of each place of `frames`,
    /// read from the file as another program reads them.
    #[cfg(unix)]
    fn as_read(frames: &FrameFile, bytes: usize) -> Vec<u8> {
        use std::os::unix::fs::FileExt;
        let mut all = vec![0u8; HEADER_BYTES as usize + bytes * PLACES as usize];
        let (header, places) = all.split_at_mut(HEADER_BYTES as usize);
        frames.file.read_exact_at(header, 0).unwrap();
        for (place, into) in places.chunks_exact_mut(bytes).enumerate() {
            let at = FrameFile::start_of(place as u32);
            frames.file.read_exact_at(into, at).unwrap();
        }
        all
    }

    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    #[test]
    fn a_picture_copied_into_its_place_is_the_picture_written_there() {
        // A frame file in memory is mapped, and one on a disk is not: the
        // test needs the first kind.
        let Ok(dir) = tempfile::tempdir_in("/dev/shm") else {
            eprintln!("skipped: no /dev/shm to make a frame file in");
            return;
        };
        let mut copied = FrameFile::create_in(dir.path()).unwrap();
        let mut written = FrameFile::create_in(dir.path()).unwrap();
        if copied.mapped.is_none() {
            eprintln!("skipped: /dev/shm is not a file system in memory here");
            return;
        }
        written.mapped = None;
        // Pictures that grow and shrink, through every place more than
        // once, given from the top down and from the bottom up in turn.
        let sizes = [
            (2u32, 2u32),
            (64, 36),
            (640, 360),
            (3, 5),
            (1280, 720),
            (64, 36),
            (1, 1),
            (641, 359),
            (1280, 720),
            (2, 1),
        ];
        let largest = 1280 * 720 * 4;
        for (index, (w, h)) in sizes.into_iter().enumerate() {
            let rgba: Vec<u8> = (0..w as usize * h as usize * 4)
                .map(|i| (i.wrapping_mul(31).wrapping_add(index * 97) ^ (i >> 7)) as u8)
                .collect();
            for frames in [&mut copied, &mut written] {
                if index % 2 == 0 {
                    frames.show(w, h, &rgba).unwrap();
                } else {
                    frames.show_bottom_up(w, h, &rgba).unwrap();
                }
            }
            assert!(copied.mapped.is_some(), "picture {index}");
            assert_eq!(
                (copied.sequence, copied.place),
                (written.sequence, written.place)
            );
            assert_eq!(copied.sequence, 2 * (index as u32 + 1));
            // Every byte a reader can find is the same: the header and all
            // three places, the ones not written this time included.
            assert!(
                as_read(&copied, largest) == as_read(&written, largest),
                "picture {index}, {w}x{h}"
            );
        }
        // What does not fit is refused the same way and changes nothing.
        let before = as_read(&copied, largest);
        for frames in [&mut copied, &mut written] {
            assert!(frames.show(2, 2, &[0; 15]).is_err());
            assert!(frames.show_bottom_up(0, 2, &[]).is_err());
            assert!(frames.show(PANEL.0 * 2 + 1, PANEL.1, &[]).is_err());
        }
        assert!(as_read(&copied, largest) == before);
        // The copy needs no second picture to turn the first upside down in.
        assert_eq!(copied.flipped.capacity(), 0);
        assert!(written.flipped.capacity() > 0);
    }

    #[cfg(all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    ))]
    #[test]
    fn only_memory_that_was_set_aside_inside_the_file_is_handed_out() {
        let Ok(dir) = tempfile::tempdir_in("/dev/shm") else {
            eprintln!("skipped: no /dev/shm to make a frame file in");
            return;
        };
        let mut frames = FrameFile::create_in(dir.path()).unwrap();
        let Some(places) = frames.mapped.as_mut() else {
            eprintln!("skipped: /dev/shm is not a file system in memory here");
            return;
        };
        // Inside the file, to its last byte; never past it.
        assert_eq!(places.slice(0, 16).map(|s| s.len()), Some(16));
        assert_eq!(places.slice(FILE_BYTES - 8, 8).map(|s| s.len()), Some(8));
        assert!(places.slice(FILE_BYTES - 8, 9).is_none());
        assert!(places.slice(FILE_BYTES, 1).is_none());
        assert!(places.slice(u64::MAX, 1).is_none());
        assert!(places.slice(1, usize::MAX).is_none());
        assert!(!places.reserve(&frames.file, 0, 0));
        // A place is set aside as far as the largest picture it has held.
        frames.show_bottom_up(4, 2, &[7; 32]).unwrap();
        frames.show_bottom_up(2, 2, &[8; 16]).unwrap();
        assert_eq!(frames.reserved, [0, 32, 16]);
        frames.show_bottom_up(1, 1, &[9; 4]).unwrap();
        frames.show_bottom_up(1, 1, &[9; 4]).unwrap();
        assert_eq!(frames.reserved, [4, 32, 16]);
        // A file that is not in memory is written, as before: nothing is
        // mapped, nothing set aside, and the reader finds the picture.
        let mut written = FrameFile::create_in(dir.path()).unwrap();
        written.mapped = None;
        written.show_bottom_up(1, 1, &[1, 2, 3, 4]).unwrap();
        assert_eq!(written.reserved, [0; 3]);
        assert_eq!(
            as_read(&written, 4)[HEADER_BYTES as usize + 4..][..4],
            [1, 2, 3, 4]
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn frame_files_of_processes_that_are_gone_are_removed_under_old_and_new_names() {
        let dir = tempfile::tempdir().unwrap();
        // No process has this number: it is above the largest there can be.
        let gone = u32::MAX - 1;
        let (me, init) = (std::process::id(), 1);
        let name = |pid: u32, rest: &str| format!("{FRAME_PREFIX}{pid}-{rest}{FRAME_ENDING}");
        let orphans = [
            name(gone, "0"),
            name(gone, "17"),
            name(gone, "aB3dE6gH9jK2mN5p"),
        ];
        let kept = [
            name(me, "0"),
            name(me, "aB3dE6gH9jK2mN5p"),
            name(init, "3"),
            format!("{FRAME_PREFIX}{gone}-0.other"),
            format!("{FRAME_PREFIX}x-0{FRAME_ENDING}"),
            format!("other-{gone}-0{FRAME_ENDING}"),
        ];
        for file in orphans.iter().chain(&kept) {
            std::fs::write(dir.path().join(file), b"").unwrap();
        }
        remove_orphans(dir.path());
        for file in &orphans {
            assert!(!dir.path().join(file).exists(), "{file} stayed");
        }
        for file in &kept {
            assert!(dir.path().join(file).exists(), "{file} was removed");
        }
        // This process's own file from `create_in` stays as well.
        let own = FrameFile::create_in(dir.path()).unwrap();
        remove_orphans(dir.path());
        assert!(own.path.exists());
    }

    #[test]
    fn without_a_helper_nothing_starts() {
        let mut weaver = Weaver::with_helper(None);
        assert!(!weaver.available());
        weaver.start(false, || {});
        assert!(!weaver.is_running());
        assert_eq!(weaver.status(), WeaverStatus::NoHelper);
        weaver.show(2, 1, &[0; 8]);
        assert!(!weaver.poll());
    }

    #[cfg(unix)]
    mod with_a_helper {
        use super::*;
        use std::os::unix::fs::PermissionsExt;
        use std::time::{Duration, Instant};

        fn script(dir: &Path, body: &str) -> PathBuf {
            let path = dir.join("weaver.sh");
            std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        }

        /// Start, trying again while the freshly written script is "busy"
        /// (another test's child between fork and exec holds it open).
        fn start(weaver: &mut Weaver) {
            for _ in 0..200 {
                weaver.start(false, || {});
                match weaver.status() {
                    WeaverStatus::Failed(reason) if reason.contains("busy") => {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    _ => return,
                }
            }
        }

        fn wait_for(
            weaver: &mut Weaver,
            done: impl Fn(&WeaverStatus) -> bool,
        ) -> (WeaverStatus, bool) {
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut ended = false;
            loop {
                ended |= weaver.poll();
                let status = weaver.status();
                if done(&status) || ended || Instant::now() > deadline {
                    return (status, ended);
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }

        #[test]
        fn the_helper_gets_the_frame_file_and_its_words_become_the_status() {
            let dir = tempfile::tempdir().unwrap();
            let copy = dir.path().join("copy");
            // Reports, then copies the picture's header and first pixels when told to (a line on
            // its input would be a protocol of its own, so it waits for the sequence to settle).
            let helper = script(
                dir.path(),
                &format!(
                    "echo on; echo 'tracking: no camera'; echo 'eyes 1'\nwhile [ -e \"$1\" ] && [ \"$(od -An -tu4 -j12 -N4 \"$1\" | tr -d ' ')\" != 2 ]; do sleep 0.02; done\n{{ head -c 28 \"$1\"; tail -c +{} \"$1\" | head -c 16; }} > '{}.tmp' && mv '{}.tmp' '{}'\ncat >/dev/null",
                    HEADER_BYTES + u64::from(CAPACITY) + 1,
                    copy.display(),
                    copy.display(),
                    copy.display()
                ),
            );
            let mut weaver = Weaver::with_helper(Some(helper));
            start(&mut weaver);
            let (status, _) = wait_for(&mut weaver, |s| {
                matches!(
                    s,
                    WeaverStatus::On {
                        eyes: Some(true),
                        note: Some(_)
                    }
                )
            });
            assert_eq!(
                status,
                WeaverStatus::On {
                    eyes: Some(true),
                    note: Some("no camera".into())
                }
            );
            // Two rows of two pixels, top row first.
            weaver.show(
                2,
                2,
                &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
            );
            assert!(weaver.is_running(), "{:?}", weaver.status());
            let deadline = Instant::now() + Duration::from_secs(10);
            while !copy.exists() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            let bytes = std::fs::read(&copy).unwrap();
            assert_eq!(&bytes[..8], MAGIC);
            assert_eq!(bytes[8..12], CAPACITY.to_le_bytes());
            assert_eq!(
                bytes[12..16],
                2u32.to_le_bytes(),
                "even: a finished picture"
            );
            assert_eq!(bytes[16..20], 2u32.to_le_bytes());
            assert_eq!(bytes[20..24], 2u32.to_le_bytes());
            assert_eq!(
                bytes[24..28],
                1u32.to_le_bytes(),
                "the first picture's place"
            );
            // In its place, bottom row first.
            assert_eq!(
                &bytes[28..44],
                &[9, 10, 11, 12, 13, 14, 15, 16, 1, 2, 3, 4, 5, 6, 7, 8]
            );
            let path = weaver.session.as_ref().unwrap().path.clone();
            weaver.stop();
            assert!(!path.exists(), "the frame file goes with the session");
            assert_eq!(weaver.status(), WeaverStatus::Idle);
        }

        #[test]
        fn leaving_after_a_picture_ends_the_output_and_leaving_before_one_is_a_failure() {
            let dir = tempfile::tempdir().unwrap();
            let mut weaver = Weaver::with_helper(Some(script(dir.path(), "echo on")));
            start(&mut weaver);
            let (status, ended) = wait_for(&mut weaver, |_| false);
            assert!(ended, "Esc in the helper's window: over, not failed");
            assert_eq!(status, WeaverStatus::Idle);

            let dir = tempfile::tempdir().unwrap();
            let mut weaver =
                Weaver::with_helper(Some(script(dir.path(), "echo 'error: no Odyssey'")));
            start(&mut weaver);
            let (status, ended) = wait_for(&mut weaver, |s| matches!(s, WeaverStatus::Failed(_)));
            assert!(!ended);
            assert_eq!(status, WeaverStatus::Failed("no Odyssey".into()));

            let dir = tempfile::tempdir().unwrap();
            let mut weaver = Weaver::with_helper(Some(script(dir.path(), "exit 0")));
            start(&mut weaver);
            let (status, _) = wait_for(&mut weaver, |s| matches!(s, WeaverStatus::Failed(_)));
            assert_eq!(
                status,
                WeaverStatus::Failed("the helper stopped unexpectedly".into())
            );
        }
    }
}

/// Timings of a still picture's way to the Odyssey 3D output on this
/// machine. Run by hand:
/// `cargo test --release -p any2odyssey weaver::bench -- --ignored --nocapture`
#[cfg(all(test, target_os = "linux"))]
mod bench {
    use super::*;
    use std::time::{Duration, Instant};

    /// The middle one of `times`, in milliseconds.
    fn median(times: &mut [Duration]) -> f64 {
        times.sort();
        times[times.len() / 2].as_secs_f64() * 1e3
    }

    #[test]
    #[ignore = "a measurement, not a check"]
    fn still_timings() {
        let memory = Path::new("/dev/shm");
        let dir = if memory.is_dir() {
            tempfile::tempdir_in(memory)
        } else {
            tempfile::tempdir()
        }
        .unwrap();
        let sink = FrameSink::in_folder(dir.path()).unwrap();
        // Pairs as wide as two 1080-line, two 2160-line and two 8K-wide
        // views, and one picture for both eyes.
        for (w, h, flat) in [
            (3840u32, 1080u32, false),
            (7680, 2160, false),
            (7680, 4320, false),
            (3840, 2160, true),
        ] {
            let pair = RgbaImage::from_fn(w, h, |x, y| {
                let v = (x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)).to_le_bytes();
                image::Rgba([v[0], v[1], v[2], 255])
            });
            let (mut compose, mut hand_over) = (Vec::new(), Vec::new());
            for round in 0..12 {
                let t0 = Instant::now();
                let picture = compose_still(&pair, flat, 1.0);
                let t1 = Instant::now();
                sink.show(picture.width(), picture.height(), picture.as_raw())
                    .unwrap();
                let t2 = Instant::now();
                // The first rounds make the file's pages.
                if round >= 2 {
                    compose.push(t1 - t0);
                    hand_over.push(t2 - t1);
                }
            }
            println!(
                "{w}x{h}{}: compose {:.1} ms, hand over {:.1} ms",
                if flat { " for both eyes" } else { " pair" },
                median(&mut compose),
                median(&mut hand_over)
            );
        }
    }
}
