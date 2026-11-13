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

use image::{imageops, RgbaImage};

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

/// The file the pictures go through. It lives in memory (the session's
/// runtime folder) and only this user can read it.
struct FrameFile {
    file: File,
    path: PathBuf,
    sequence: u32,
    /// The place the newest picture is in.
    place: u32,
    /// Working room for turning a picture upside down.
    flipped: Vec<u8>,
}

impl FrameFile {
    fn create() -> std::io::Result<FrameFile> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        remove_orphans(&dir);
        // One file per session: a new one never takes a file still in use.
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let number = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = dir.join(format!(
            "any2odyssey-weaver-{}-{number}.frames",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path)?;
        let mut header = [0u8; 24];
        header[..8].copy_from_slice(MAGIC);
        header[8..12].copy_from_slice(&CAPACITY.to_le_bytes());
        let written = file
            .set_len(HEADER_BYTES + u64::from(CAPACITY) * u64::from(PLACES))
            .and_then(|()| file.write_all(&header));
        if let Err(e) = written {
            let _ = std::fs::remove_file(&path);
            return Err(e);
        }
        Ok(FrameFile {
            file,
            path,
            sequence: 0,
            place: 0,
            flipped: Vec::new(),
        })
    }

    fn put(&mut self, at: u64, bytes: &[u8]) -> std::io::Result<()> {
        self.file.seek(SeekFrom::Start(at))?;
        self.file.write_all(bytes)
    }

    /// Replace the picture with `rgba`, rows from the top down.
    fn show(&mut self, width: u32, height: u32, rgba: &[u8]) -> std::io::Result<()> {
        let row = width as usize * 4;
        if row == 0 || rgba.len() != row * height as usize {
            return Err(does_not_fit());
        }
        let mut flipped = std::mem::take(&mut self.flipped);
        flipped.clear();
        flipped.extend(rgba.chunks_exact(row).rev().flatten());
        let shown = self.show_bottom_up(width, height, &flipped);
        self.flipped = flipped;
        shown
    }

    /// Replace the picture with `rgba`, rows from the bottom up: written
    /// into the next place, then the header is pointed at it. The sequence
    /// is odd while the header changes, so the helper never takes a
    /// half-written header for a whole one.
    fn show_bottom_up(&mut self, width: u32, height: u32, rgba: &[u8]) -> std::io::Result<()> {
        let bytes = u64::from(width) * u64::from(height) * 4;
        if width == 0 || height == 0 || bytes > u64::from(CAPACITY) || rgba.len() as u64 != bytes {
            return Err(does_not_fit());
        }
        let place = (self.place + 1) % PLACES;
        self.put(HEADER_BYTES + u64::from(place) * u64::from(CAPACITY), rgba)?;
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
            .and_then(|name| name.strip_prefix("any2odyssey-weaver-"))
            .and_then(|rest| rest.strip_suffix(".frames"))
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
    // A flat picture is identical in both eyes: resize it once and copy the
    // fitted pixels to both halves instead of running the same filter twice.
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

#[cfg(test)]
mod tests {
    use super::*;

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
