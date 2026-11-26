//! Live capture of one screen (Linux).
//!
//! The desktop's screen-cast portal does the asking: the desktop shows its
//! own dialog, the user picks a screen, and the portal hands out a PipeWire
//! stream of it. This works the same on Wayland and X11 desktops and needs
//! no privilege of ours. The choice is remembered with the portal's restore
//! token, so the dialog appears once and not at every start.
//!
//! One thread owns the portal session and the PipeWire loop. Each frame is
//! copied as it arrives and put into a slot that holds only the newest one;
//! the reader takes it from there at its own pace, so a slow reader costs
//! frames, never memory or delay.

use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use any2odyssey_core::live::{self, PixelOrder, Source, Turned};
use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};
use ashpd::desktop::PersistMode;
use pipewire as pw;
use pw::spa;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureState {
    /// The desktop has been asked for a screen and has not answered: its
    /// dialog is open.
    Asking,
    /// A screen was chosen and its stream is being set up: no frame has
    /// come yet. A desktop that sends frames only when the screen changes
    /// (xdg-desktop-portal-wlr before 0.9) leaves it at that for as long as
    /// nothing does.
    Starting,
    /// Frames are arriving, this many pixels each.
    Running { width: u32, height: u32 },
    /// Over: the reason, or `None` after an ordinary stop.
    Ended(Option<String>),
}

/// The newest frame, as the stream delivered it.
#[derive(Default)]
pub struct Slot {
    /// Raised for every frame; 0 before the first.
    pub serial: u64,
    pub width: u32,
    pub height: u32,
    stride: usize,
    bgr: bool,
    bytes: Vec<u8>,
    /// When the frame took the slot, and how long after the one before it
    /// (`None` for the first, and for a frame nothing is known about).
    arrived: Option<Instant>,
    spacing: Option<Duration>,
}

/// For a reader that takes a frame every `pace` at the most, of a stream
/// whose newest frame came `spacing` after the one before it: how long
/// after the arrival of the frame taken last the next one to take arrives
/// at the earliest, and how long after it the wait for such a frame is
/// given up. `None` where the frames come no faster than they are taken,
/// and each is taken as it arrives.
///
/// A desktop is asked for up to [`MOST_A_SECOND`] frames a second, more
/// than are shown. Of a screen that changes faster than pictures are made,
/// a frame is always waiting when a turn begins; taken as it is, it has
/// waited for a time that drifts from turn to turn, until one frame more
/// or one fewer lies between two pictures than between the others: of 120
/// frames a second and a turn every 15 ms, every ninth picture came one
/// frame after the one before it where the others came two after. So the
/// frames that come too soon after the one taken last are let pass, and
/// the first that does not is taken as it arrives: the pictures are the
/// same number of frames apart, every second frame of 120 a second and
/// every third of 165. Where no such frame comes (the screen stood still)
/// the newest there is is taken, late by `spacing` once.
pub(crate) fn fresh_from(pace: Duration, spacing: Duration) -> Option<(Duration, Duration)> {
    /// What a frame may be early or late by.
    const LOOSE: Duration = Duration::from_millis(2);
    (spacing < pace).then(|| (pace.saturating_sub(LOOSE), pace + spacing))
}

impl Slot {
    /// When this frame arrived, where that is known.
    pub fn arrived(&self) -> Option<Instant> {
        self.arrived
    }

    pub fn source(&self) -> Source<'_> {
        Source {
            bytes: &self.bytes,
            width: self.width,
            height: self.height,
            stride: self.stride,
            order: if self.bgr {
                PixelOrder::Bgr
            } else {
                PixelOrder::Rgb
            },
        }
    }
}

struct Shared {
    slot: Mutex<Slot>,
    arrived: Condvar,
    state: Mutex<CaptureState>,
    token: Mutex<Option<String>>,
    stop: AtomicBool,
    /// Since when the stream of a cast that was running has been paused.
    paused: Mutex<Option<Instant>>,
    /// How many buffers have come since the last one that held a picture,
    /// and why the newest of them held none.
    unread: Mutex<(u32, &'static str)>,
    /// The reader has been asked to look at the newest frame again
    /// without waiting for another ([`Frames::look_again`]).
    look_again: AtomicBool,
}

impl Shared {
    fn new() -> Shared {
        Shared {
            slot: Mutex::new(Slot::default()),
            arrived: Condvar::new(),
            state: Mutex::new(CaptureState::Asking),
            token: Mutex::new(None),
            stop: AtomicBool::new(false),
            paused: Mutex::new(None),
            unread: Mutex::new((0, "")),
            look_again: AtomicBool::new(false),
        }
    }

    /// A frame has come: `frame` holds its `width` x `height` pixels, rows
    /// `stride` bytes apart. It changes places with the slot's, and the
    /// reader is told; unless it is the picture the slot holds already.
    ///
    /// A desktop sends a frame for every time it has drawn the screen,
    /// whether or not anything on it changed: GNOME while only the pointer
    /// moves and the pointer is not drawn into the picture (it is drawn
    /// onto the screen there, so the screen is drawn again), and any
    /// desktop whose windows redraw themselves with what they showed
    /// before. Each of those was a new picture here: shrunk, given to the
    /// depth model, woven and handed over again, and the page read "5
    /// pictures a second" over a screen on which nothing changed. The
    /// comparison ends soon after the first pixel that differs, and a frame
    /// that is the same to its last pixel costs less than the work it
    /// saves.
    ///
    /// Only the colours are compared ([`same_colours`]): the fourth byte
    /// of a pixel is nobody's to keep the same in the formats that have
    /// nothing in it (BGRx, RGBx), and is not looked at here in those that
    /// call it alpha, so a frame that differs in nothing else makes the
    /// same pictures.
    fn frame_arrived(
        &self,
        frame: &mut Vec<u8>,
        width: u32,
        height: u32,
        stride: usize,
        bgr: bool,
    ) {
        *lock(&self.unread) = (0, "");
        let first = {
            let mut slot = lock(&self.slot);
            if slot.serial != 0
                && (slot.width, slot.height, slot.stride, slot.bgr) == (width, height, stride, bgr)
                && if stride == width as usize * 4 {
                    same_colours(&slot.bytes, frame)
                } else {
                    // Rows with something between them (no frame of the
                    // stream comes so: `Capture::start` keeps the rows
                    // only): every fourth byte is not a pixel's fourth.
                    slot.bytes == *frame
                }
            {
                return;
            }
            std::mem::swap(&mut slot.bytes, frame);
            // What was the slot's has held the frame before this one.
            fitted(frame, slot.bytes.len());
            slot.width = width;
            slot.height = height;
            slot.stride = stride;
            slot.bgr = bgr;
            slot.serial += 1;
            let now = Instant::now();
            slot.spacing = slot
                .arrived
                .map(|before| now.saturating_duration_since(before));
            slot.arrived = Some(now);
            slot.serial == 1
        };
        self.arrived.notify_all();
        let mut state = lock(&self.state);
        let running = CaptureState::Running { width, height };
        if first || *state != running {
            *state = running;
        }
    }

    /// Whether `frame`, still in the stream's buffer, is the picture the
    /// slot holds already; it then counts as arrived and nothing more is
    /// to do with it.
    ///
    /// Every frame used to be copied out of its buffer first and compared
    /// with the slot's afterwards. A desktop that draws the screen again
    /// sends the same picture again, and KDE Plasma draws the Odyssey's
    /// own screen at every refresh while the 3D picture lies on it: under
    /// a film of 24 pictures a second, 62 frames a second came, each
    /// 3840 x 2160, and every one was copied (3.3 ms) for 38 to be thrown
    /// away. Only a frame that lies as the slot keeps them is looked at
    /// here: upright, a byte a colour. The others are compared when they
    /// have been written out ([`Shared::frame_arrived`]).
    fn holds(&self, frame: &Lying<'_>) -> bool {
        let Packing::Bytes { bgr } = frame.packing else {
            return false;
        };
        let (row, rows) = (frame.width as usize * 4, frame.height as usize);
        if frame.turned != Turned::Upright || row == 0 || frame.stride < row {
            return false;
        }
        let same = {
            let slot = lock(&self.slot);
            slot.serial != 0
                && (slot.width, slot.height, slot.stride, slot.bgr)
                    == (frame.width, frame.height, row, bgr)
                && slot.bytes.len() == row * rows
                && if frame.stride == row {
                    same_colours(&slot.bytes, frame.rows)
                } else {
                    let mut lying = frame.rows.chunks(frame.stride);
                    slot.bytes.chunks_exact(row).all(|kept| {
                        lying
                            .next()
                            .and_then(|lying| lying.get(..row))
                            .is_some_and(|lying| same_colours(kept, lying))
                    })
                }
        };
        if same {
            *lock(&self.unread) = (0, "");
        }
        same
    }

    /// A buffer has come that holds no picture, because of `why`.
    fn no_picture(&self, why: &'static str) {
        let mut unread = lock(&self.unread);
        *unread = (unread.0.saturating_add(1), why);
    }

    /// The desktop has answered with a screen.
    fn chosen(&self) {
        let mut state = lock(&self.state);
        if *state == CaptureState::Asking {
            *state = CaptureState::Starting;
        }
    }
}

/// Whether two frames of four bytes a pixel, their rows without gaps, are
/// the same picture: the three colours of every pixel are the same, the
/// fourth byte may be anything.
///
/// The frames were compared byte for byte. The fourth byte of a pixel
/// holds nothing in BGRx and RGBx, and what a desktop leaves there is no
/// more its to keep the same than what lies between the rows, which is why
/// that is no longer kept: a frame that differed there alone was a new
/// picture, shrunk, given to the depth model and woven again over a screen
/// on which nothing had changed.
///
/// A stretch of pixels is looked through whole before it is asked whether
/// any differed, which lets the processor take several pixels at once. A
/// 3840 x 2160 frame that is the same took 0.70 ms byte for byte and takes
/// 0.85 ms this way; one that differs is told within a stretch of where.
fn same_colours(a: &[u8], b: &[u8]) -> bool {
    /// The bytes looked through between two questions: 1024 pixels.
    const STRETCH: usize = 4096;
    if a.len() != b.len() {
        return false;
    }
    let colours = u32::from_ne_bytes([255, 255, 255, 0]);
    let differing = |a: &[[u8; 4]], b: &[[u8; 4]]| {
        a.iter().zip(b).fold(0, |differing, (a, b)| {
            differing | ((u32::from_ne_bytes(*a) ^ u32::from_ne_bytes(*b)) & colours)
        })
    };
    let ((a, a_last), (b, b_last)) = (a.as_chunks::<STRETCH>(), b.as_chunks::<STRETCH>());
    if a.iter()
        .zip(b)
        .any(|(a, b)| differing(a.as_chunks().0, b.as_chunks().0) != 0)
    {
        return false;
    }
    // What is left over: pixels, and bytes that are not a whole pixel
    // (not in a frame; they count as they are).
    let ((a, a_odd), (b, b_odd)) = (a_last.as_chunks(), b_last.as_chunks());
    differing(a, b) == 0 && a_odd == b_odd
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic elsewhere must not take the capture down with it: the data
    // behind these locks is always whole.
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct Capture {
    shared: Arc<Shared>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Capture {
    /// Ask the desktop for a screen and start receiving it. `restore_token`
    /// is what [`Capture::restore_token`] gave last time, to skip the
    /// dialog. `pointer` asks for the pointer to be drawn into the frames.
    /// `wake` is called from the capture thread when the state changes.
    pub fn start(
        restore_token: Option<String>,
        pointer: bool,
        wake: impl Fn() + Send + 'static,
    ) -> Capture {
        let shared = Arc::new(Shared::new());
        let for_thread = shared.clone();
        let run = move || {
            let ended = run(&for_thread, restore_token, pointer, &wake).err();
            let stopped = for_thread.stop.load(Ordering::Relaxed);
            *lock(&for_thread.state) = CaptureState::Ended(if stopped {
                None
            } else {
                ended.or(Some("the screen cast ended".into()))
            });
            for_thread.arrived.notify_all();
            wake();
        };
        let thread = std::thread::Builder::new()
            .name("screen-capture".into())
            .spawn(run);
        let thread = match thread {
            Ok(thread) => Some(thread),
            Err(e) => {
                *lock(&shared.state) =
                    CaptureState::Ended(Some(format!("could not start the capture thread: {e}")));
                None
            }
        };
        Capture { shared, thread }
    }

    pub fn state(&self) -> CaptureState {
        lock(&self.shared.state).clone()
    }

    /// For how long the desktop has had the running cast paused, in which
    /// time no frame can have come; `None` while it runs.
    pub fn paused_for(&self) -> Option<Duration> {
        lock(&self.shared.paused).map(|since| since.elapsed())
    }

    /// How many buffers the desktop has sent since the last one that held
    /// a picture, and what was wrong with the newest of them; `None` when
    /// the last buffer was a picture, or none has come.
    ///
    /// A stream of nothing but such buffers (marked as no picture, in
    /// memory that is not mapped, smaller than the stream's size needs)
    /// left the page at "Waiting for its first picture" for as long as the
    /// output was on, with nothing to say why none came.
    pub fn unread(&self) -> Option<(u32, &'static str)> {
        Some(*lock(&self.shared.unread)).filter(|(count, _)| *count > 0)
    }

    /// The portal's token for the screen that was picked, once there is one.
    pub fn restore_token(&self) -> Option<String> {
        lock(&self.shared.token).clone()
    }

    /// A handle for the thread that reads frames.
    pub fn frames(&self) -> Frames {
        Frames(self.shared.clone())
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.arrived.notify_all();
        if let Some(thread) = self.thread.take() {
            // The loop looks at the flag ten times a second. A thread still
            // inside the desktop's dialog is left to finish on its own.
            if !matches!(self.state(), CaptureState::Asking) {
                let _ = thread.join();
            }
        }
    }
}

/// The reading side of a capture.
#[derive(Clone)]
pub struct Frames(Arc<Shared>);

impl Frames {
    /// Wait until a frame newer than `seen` is there (or `timeout` passes,
    /// or the capture ends) and run `read` on the newest frame, new or not:
    /// a desktop sends frames only when the screen changes, and the reader
    /// may have its own reasons to look at a still screen again. `None`
    /// before the first frame. A frame that arrives while `read` runs is
    /// copied meanwhile and takes the slot when `read` has finished.
    #[cfg(test)]
    pub fn with_newest<T>(
        &self,
        seen: u64,
        timeout: Duration,
        read: impl FnOnce(&Slot) -> T,
    ) -> Option<T> {
        self.with_fresh(seen, None, Duration::ZERO, timeout, read)
    }

    /// [`Frames::with_newest`] for a reader that takes a frame every `pace`
    /// at the most and took its last one, which arrived at `taken`, from a
    /// stream that may send them faster: the frames that come too soon
    /// after that one are let pass first ([`fresh_from`]).
    pub fn with_fresh<T>(
        &self,
        seen: u64,
        taken: Option<Instant>,
        pace: Duration,
        timeout: Duration,
        read: impl FnOnce(&Slot) -> T,
    ) -> Option<T> {
        let stopped = || self.0.stop.load(Ordering::Relaxed);
        let mut slot = lock(&self.0.slot);
        let fresh = slot.spacing.and_then(|spacing| fresh_from(pace, spacing));
        if let (Some(taken), Some((from, until))) = (taken, fresh) {
            let left = (taken + until).saturating_duration_since(Instant::now());
            slot = self
                .0
                .arrived
                .wait_timeout_while(slot, left.min(timeout), |slot| {
                    slot.arrived.is_none_or(|arrived| arrived < taken + from) && !stopped()
                })
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        let (slot, _) = self
            .0
            .arrived
            .wait_timeout_while(slot, timeout, |slot| {
                slot.serial == seen
                    && !stopped()
                    && !self.0.look_again.swap(false, Ordering::Relaxed)
            })
            .unwrap_or_else(|e| e.into_inner());
        (slot.serial != 0).then(|| read(&slot))
    }

    /// End the wait of a reader that is waiting for a frame newer than
    /// the one it has ([`Frames::with_fresh`]), or its next such wait if
    /// it is not waiting now: something other than the screen has changed
    /// what its picture should be.
    pub fn look_again(&self) {
        // Under the slot's lock: a reader that has just found nothing new
        // is then either not yet at its question or already waiting.
        let _slot = lock(&self.0.slot);
        self.0.look_again.store(true, Ordering::Relaxed);
        self.0.arrived.notify_all();
    }

    pub fn ended(&self) -> bool {
        self.0.stop.load(Ordering::Relaxed)
            || matches!(*lock(&self.0.state), CaptureState::Ended(_))
    }
}

/// The runtime the portal client lives on, one for the life of the process.
/// The client keeps its connection to the desktop between uses, and the
/// connection belongs to the runtime it was made on: a runtime per capture
/// leaves the second capture waiting on a connection nobody serves.
pub(crate) fn portal_runtime() -> Result<&'static tokio::runtime::Runtime, String> {
    static RUNTIME: std::sync::OnceLock<Result<tokio::runtime::Runtime, String>> =
        std::sync::OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .thread_name("portal")
                .enable_all()
                .build()
                .map_err(|e| format!("could not start the portal client: {e}"))
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn run(
    shared: &Arc<Shared>,
    restore_token: Option<String>,
    pointer: bool,
    wake: &(impl Fn() + Send + 'static),
) -> Result<(), String> {
    let runtime = portal_runtime()?;
    // The desktop is asked, with the token of the screen chosen last time
    // or without one.
    let ask = |restore_token: Option<String>| async move {
        let proxy = Screencast::new().await?;
        let session = proxy.create_session(Default::default()).await?;
        // Listened for from before the desktop is asked to start (see
        // `unless_ended`). A desktop that cannot be listened to is asked
        // all the same.
        let closed = session.receive_closed().await.ok();
        let started = async {
            // A screen shown somewhere else needs its pointer in the
            // picture; a screen shown on itself has the real one on top
            // already. Where the desktop cannot draw it into the stream,
            // the stream comes without.
            let cursor = wished_cursor(pointer, proxy.available_cursor_modes().await.ok());
            proxy
                .select_sources(
                    &session,
                    SelectSourcesOptions::default()
                        .set_cursor_mode(cursor)
                        .set_sources(ashpd::enumflags2::BitFlags::from(SourceType::Monitor))
                        .set_multiple(false)
                        .set_restore_token(restore_token.as_deref())
                        .set_persist_mode(PersistMode::ExplicitlyRevoked),
                )
                .await?
                // What the desktop answers to this is looked at: where it
                // is "no", the portal has closed the session by now, and a
                // start asked for all the same is refused with "Invalid
                // session", which says nothing (Hyprland's portal backend
                // on a desktop that is not Hyprland answers so).
                .response()?;
            let start = async {
                proxy
                    .start(&session, None, Default::default())
                    .await?
                    .response()
            };
            let response = match closed {
                Some(closed) => unless_ended(start, closed).await.ok_or(Refused::Closed)??,
                None => start.await?,
            };
            let node = response.streams().first().map(|s| s.pipe_wire_node_id());
            let token = response.restore_token().map(str::to_owned);
            let fd = proxy
                .open_pipe_wire_remote(&session, Default::default())
                .await?;
            Ok::<_, Refused>((node, token, fd))
        }
        .await;
        match started {
            Ok((node, token, fd)) => Ok((session, node, token, fd)),
            Err(e) => {
                // The session exists by now, and once `start` has answered
                // the desktop is sharing the screen in it: it is closed, or
                // it (and the desktop's sign that a screen is being shared)
                // would stay for as long as this program runs.
                let _ = session.close().await;
                Err(e)
            }
        }
    };
    let portal = asked_again_without(restore_token, ask);
    let (session, node, token, fd) = runtime.block_on(portal).map_err(|refused| refused.said())?;
    *lock(&shared.token) = token;
    // The dialog is over: the page stops asking for a screen to be chosen.
    shared.chosen();
    wake();
    // Whatever comes of the stream, the session is closed after it.
    let streamed = match node {
        None => Err("the portal returned no stream".to_owned()),
        Some(_) if shared.stop.load(Ordering::Relaxed) => Ok(()),
        Some(node) => stream(shared, node, fd, wake),
    };
    finished_within(runtime, CLOSE_PATIENCE, async move {
        let _ = session.close().await;
    });
    streamed
}

/// The cursor mode to ask the portal for: the pointer drawn into the frames
/// where that is wanted and the desktop can do it, and otherwise nothing,
/// which the portal takes for "hidden".
///
/// "Hidden" used to be asked for by name. The portal checks a named mode
/// against the modes its backend has told it of, and for a moment after the
/// backend has been started again (it had ended, or was updated) it knows of
/// none: the next Start then ended with "Unavailable cursor mode 1", on GNOME
/// with xdg-desktop-portal 1.22. What is not named is not checked.
fn wished_cursor(
    pointer: bool,
    available: Option<ashpd::enumflags2::BitFlags<CursorMode>>,
) -> Option<CursorMode> {
    match available {
        Some(modes) if pointer && modes.contains(CursorMode::Embedded) => {
            Some(CursorMode::Embedded)
        }
        _ => None,
    }
}

/// The remembered token as it may be handed to the portal: `None` for one
/// that holds what no token holds (a zero byte, a line end or another
/// control character), and for none at all.
///
/// The token comes from a file, and after a power cut a file can hold zero
/// bytes where its text was. A string with a zero byte in it is not allowed
/// on the session bus, and the bus ends the connection of whoever sends
/// one. The portal client keeps one connection for the life of the program:
/// with 22 zero bytes in the file (KDE Plasma, xdg-desktop-portal 1.22) the
/// first Start read "Portal request didn't receive a response" and every
/// Start after it "the session's D-Bus cannot be reached", and a program
/// started anew did the same, the file being as it was. Asked without a
/// token the desktop shows its dialog, and the token it then gives takes
/// the file's place.
fn sendable(token: Option<String>) -> Option<String> {
    token.filter(|token| !token.is_empty() && !token.chars().any(char::is_control))
}

/// What `ask` comes to with the remembered `token`, where that is one that
/// can be sent ([`sendable`]); and where the desktop answers that what it
/// was asked with is not valid, what it comes to when asked once more
/// without the token.
///
/// The portal looks at the form of a restore token before anything else and
/// refuses the request for one it does not take for a token:
/// "org.freedesktop.portal.Error.InvalidArgument: Restore token is not
/// valid". xdg-desktop-portal 1.22 says so of two words, of a word with a
/// letter that is not ASCII, and of 22 letters with a hyphen among them;
/// which form it wants is the portal's own matter, and the file may hold
/// what another version wrote, or anything else. The page then read "Live
/// capture ended: the desktop's screen-cast portal refused: ..." at every
/// Start, the file stayed as it was, and nothing said that "Choose another
/// screen" was the way out. Asked without a token the desktop shows its
/// dialog, as it does for a token of the right form that it does not know.
async fn asked_again_without<T, Asked>(
    token: Option<String>,
    ask: impl Fn(Option<String>) -> Asked,
) -> Result<T, Refused>
where
    Asked: std::future::Future<Output = Result<T, Refused>>,
{
    let token = sendable(token);
    let remembered = token.is_some();
    match ask(token).await {
        Err(refused) if remembered && refused.of_what_was_asked() => ask(None).await,
        answered => answered,
    }
}

/// Why the desktop gave no screen cast.
#[derive(Debug)]
enum Refused {
    /// The portal said so, or could not be asked.
    Portal(ashpd::Error),
    /// The desktop closed the session while it was asked to start the cast.
    Closed,
}

impl From<ashpd::Error> for Refused {
    fn from(error: ashpd::Error) -> Refused {
        Refused::Portal(error)
    }
}

/// The portal runs and has no screen cast: none of its backends offers one
/// for this desktop. So it is in an X11 session of Xfce or i3 with the GTK
/// backend, which has file dialogs and no screen cast.
const NO_SCREEN_CAST: &str = "this desktop's portal cannot share a screen. A live screen needs \
    xdg-desktop-portal with the backend of the desktop in use: GNOME, KDE Plasma, COSMIC, \
    Hyprland, niri and the wlroots desktops (sway, labwc, river, Wayfire) each have one; a plain \
    X11 session has none";

/// Nothing owns the portal's name on the bus and nothing can be started
/// for it.
const NO_PORTAL: &str = "no desktop portal answers. A live screen is asked for through \
    xdg-desktop-portal, which is not installed or could not be started";

const NO_BUS: &str =
    "the session's D-Bus cannot be reached, and the desktop is asked for a screen through it";

/// The portal answered "other" and nothing more. So do GNOME's and KDE's
/// backends in an X11 session, and GNOME's on niri when niri was started
/// without its session services.
const NO_REASON: &str = "the desktop's portal could not share a screen and gave no reason. If \
    it never works here: a portal backend shares screens only on the desktop it was made for \
    (GNOME's on GNOME and niri, KDE's on Plasma with Wayland)";

const CLOSED_UNSTARTED: &str = "the desktop gave up the screen cast before it had started, \
    without an answer; its own log may say why";

impl Refused {
    /// The portal turned the request down for what it was asked with (it
    /// calls one of the request's values invalid), not for anything the
    /// desktop cannot do or the user did not want.
    fn of_what_was_asked(&self) -> bool {
        matches!(
            self,
            Refused::Portal(ashpd::Error::Portal(ashpd::PortalError::InvalidArgument(_)))
        )
    }

    /// The reason as the page says it, after "Live capture ended: ".
    ///
    /// All but a cancelled dialog used to be the portal client's own words
    /// after "the desktop's screen-cast portal refused: ": "A portal frontend
    /// implementing `org.freedesktop.portal.ScreenCast` was not found" (an
    /// X11 session with the GTK backend alone), "Portal request didn't
    /// succeed with no information" (GNOME's or KDE's backend in an X11
    /// session), "Portal request failed: org.freedesktop.zbus.Error: The
    /// name org.freedesktop.portal.Desktop was not provided by any .service
    /// files" (no portal installed). None of them says what is missing.
    fn said(&self) -> String {
        use ashpd::desktop::ResponseError::{Cancelled, Other};
        use ashpd::zbus::{fdo, Error as Bus};
        let error = match self {
            Refused::Closed => return CLOSED_UNSTARTED.to_owned(),
            Refused::Portal(error) => error,
        };
        let bus = match error {
            ashpd::Error::Zbus(bus) | ashpd::Error::Portal(ashpd::PortalError::ZBus(bus)) => {
                Some(bus)
            }
            _ => None,
        };
        const UNOWNED: [&str; 2] = [
            "org.freedesktop.DBus.Error.ServiceUnknown",
            "org.freedesktop.DBus.Error.NameHasNoOwner",
        ];
        match (error, bus) {
            (ashpd::Error::Response(Cancelled), _) => "no screen was chosen".to_owned(),
            (ashpd::Error::Response(Other), _) => NO_REASON.to_owned(),
            (ashpd::Error::PortalNotFound(_), _) => NO_SCREEN_CAST.to_owned(),
            (_, Some(Bus::FDO(unowned)))
                if matches!(
                    **unowned,
                    fdo::Error::ServiceUnknown(_) | fdo::Error::NameHasNoOwner(_)
                ) =>
            {
                NO_PORTAL.to_owned()
            }
            (_, Some(Bus::MethodError(name, _, _))) if UNOWNED.contains(&name.as_str()) => {
                NO_PORTAL.to_owned()
            }
            (
                _,
                Some(
                    lost @ (Bus::Connection(..)
                    | Bus::Address(_)
                    | Bus::InputOutput(_)
                    | Bus::Handshake(_)),
                ),
            ) => format!("{NO_BUS} ({lost})"),
            (other, _) => format!("the desktop's screen-cast portal refused: {other}"),
        }
    }
}

/// What `work` comes to, or `None` when `ended` brings something before it
/// is through.
///
/// The desktop answers the request to start a cast, and it may instead close
/// the session and never answer: niri 26.04 does when it cannot start the
/// cast (its screen cast needs a GPU device and PipeWire), with GNOME's
/// portal backend passing the closing on. Only the answer was waited for, so
/// the page went on asking for a screen to be chosen in a dialog that had
/// long gone, until the output was stopped.
///
/// A dialog that is cancelled is answered first and its session closed
/// after that: an answer that is there when the closing is seen counts.
async fn unless_ended<T>(
    work: impl std::future::Future<Output = T>,
    ended: impl ashpd::zbus::export::futures_core::Stream,
) -> Option<T> {
    use std::task::Poll;
    let mut work = std::pin::pin!(work);
    let mut ended = std::pin::pin!(ended);
    // A source of endings that is itself over is not asked again.
    let mut listening = true;
    std::future::poll_fn(|context| {
        if let Poll::Ready(done) = work.as_mut().poll(context) {
            return Poll::Ready(Some(done));
        }
        if !listening {
            return Poll::Pending;
        }
        match ended.as_mut().poll_next(context) {
            Poll::Ready(Some(_)) => Poll::Ready(match work.as_mut().poll(context) {
                Poll::Ready(done) => Some(done),
                Poll::Pending => None,
            }),
            Poll::Ready(None) => {
                listening = false;
                Poll::Pending
            }
            Poll::Pending => Poll::Pending,
        }
    })
    .await
}

/// How long the portal's answer to the closing of a session is waited for.
/// It comes within milliseconds from a portal that is well.
const CLOSE_PATIENCE: Duration = Duration::from_millis(1500);

/// Run `work` on `runtime` and wait until it is through, but for `patience`
/// at the longest: whether it was through by then. It is left to finish by
/// itself otherwise.
///
/// The window's thread waits for this thread when the output is stopped
/// (`Capture::drop`). A portal that does not answer (its program, or the
/// program behind it, stopped or hung) kept this thread in the closing of
/// the session and the window with it: the page stood still for as long as
/// the portal did, and `--quit` was accepted and not acted on.
fn finished_within(
    runtime: &tokio::runtime::Runtime,
    patience: Duration,
    work: impl std::future::Future<Output = ()> + Send + 'static,
) -> bool {
    let (done, finished) = std::sync::mpsc::channel();
    runtime.spawn(async move {
        work.await;
        let _ = done.send(());
    });
    finished.recv_timeout(patience).is_ok()
}

/// What PipeWire tells of a running cast.
enum Seen {
    /// The stream went from one state to another.
    State(pw::stream::StreamState, pw::stream::StreamState),
    /// An object of the graph is gone: its number.
    Removed(u32),
}

/// Why the cast of the screen that is node `node` is over, if `seen` means
/// that it is.
///
/// A stream that pauses is not over: it pauses for a moment whenever the
/// screen's mode changes, and for as long as the session manager is being
/// restarted, and runs again by itself. It also pauses, and that is all it
/// is told, when the screen's node is taken away: the output was unplugged
/// or switched off, the desktop ended the cast, or the portal's own program
/// went away (xdg-desktop-portal-wlr 0.8 does at a change of the screen's
/// mode). No frame comes after that, which looks like a screen on which
/// nothing changes, so the page went on saying that it showed one. The
/// node going is what tells the two apart.
fn ending(node: u32, seen: &Seen) -> Option<String> {
    use pw::stream::StreamState::{Error, Paused, Streaming, Unconnected};
    match seen {
        Seen::State(_, Error(reason)) => Some(reason.clone()),
        // The connection to PipeWire itself was lost after the stream had
        // been running.
        Seen::State(Streaming | Paused, Unconnected) => {
            Some("the screen cast was ended by the desktop".to_owned())
        }
        Seen::Removed(id) if *id == node => {
            Some("the screen cast was ended by the desktop".to_owned())
        }
        _ => None,
    }
}

/// Since when a cast that was running is paused, once its stream has gone
/// from `old` to `new`; `since` is what it was before that.
///
/// A paused stream gets no frames, and the page must not call that a screen
/// on which nothing changes (`Capture::paused_for`). The pause before the
/// first frame is not one of a running cast.
fn pause_after(
    since: Option<Instant>,
    old: &pw::stream::StreamState,
    new: &pw::stream::StreamState,
    now: Instant,
) -> Option<Instant> {
    use pw::stream::StreamState::{Paused, Streaming};
    match (old, new) {
        (Streaming, Paused) => Some(now),
        (_, Paused) => since,
        _ => None,
    }
}

/// How a pixel lies in its four bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Packing {
    /// A byte a colour: blue first (`bgr`) or red first, then one to spare.
    Bytes { bgr: bool },
    /// Ten bits a colour in one 32-bit number, the two bits on top to
    /// spare: red in the highest ten of the thirty (`red_high`) or blue.
    /// wlroots desktops send this, and nothing else, for an output that is
    /// rendered with ten bits (sway's `render_bit_depth 10`); the cast then
    /// ended at once with "no more input formats".
    Tens { red_high: bool },
}

/// The packing of a stream's pixels, for the formats that are asked for.
fn packing(format: spa::param::video::VideoFormat) -> Option<Packing> {
    use spa::param::video::VideoFormat as Format;
    match format {
        Format::BGRx | Format::BGRA => Some(Packing::Bytes { bgr: true }),
        Format::RGBx | Format::RGBA => Some(Packing::Bytes { bgr: false }),
        Format::xRGB_210LE | Format::ARGB_210LE => Some(Packing::Tens { red_high: true }),
        Format::xBGR_210LE | Format::ABGR_210LE => Some(Packing::Tens { red_high: false }),
        _ => None,
    }
}

/// Write a frame of ten bits a colour into `out` as one of a byte a colour
/// (blue, green, red, one to spare), its rows without gaps: the eight
/// highest bits of each colour are kept. `frame` holds `height` rows of
/// `width` pixels, each row `stride` bytes after the one before.
fn narrow_tens(
    frame: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    red_high: bool,
    out: &mut Vec<u8>,
) {
    out.clear();
    out.reserve(width * height * 4);
    for row in frame.chunks(stride.max(1)).take(height) {
        let Some(row) = row.get(..width * 4) else {
            return;
        };
        for pixel in row.as_chunks::<4>().0 {
            let all = u32::from_le_bytes(*pixel);
            let (high, middle, low) = ((all >> 22) as u8, (all >> 12) as u8, (all >> 2) as u8);
            let (red, blue) = if red_high { (high, low) } else { (low, high) };
            out.extend_from_slice(&[blue, middle, red, 255]);
        }
    }
}

/// A frame as a buffer holds it: `height` rows of `width` pixels in `rows`,
/// each row `stride` bytes after the one before, its pixels packed and its
/// picture turned as the stream says.
#[derive(Clone, Copy)]
struct Lying<'a> {
    rows: &'a [u8],
    width: u32,
    height: u32,
    stride: usize,
    packing: Packing,
    turned: Turned,
}

impl Lying<'_> {
    /// Write the frame into `spare` as it is handed on: upright, a byte a
    /// colour, its rows with nothing between them. The upright picture's
    /// width and height, and whether blue comes first in a pixel; `None`
    /// for a frame that is not whole. `narrowed` is room for a frame of ten
    /// bits a colour on its way to being turned; a frame of any other kind
    /// lets go of it.
    ///
    /// This was done where the stream's buffers are read, and nothing
    /// tried it: with a frame's rows taken as they came again, gaps and
    /// all, every test passed.
    fn taken(self, spare: &mut Vec<u8>, narrowed: &mut Vec<u8>) -> Option<(u32, u32, bool)> {
        let Lying {
            rows: frame,
            width,
            height,
            stride,
            packing,
            turned,
        } = self;
        let row = width as usize * 4;
        let lying = |bytes, stride| Source {
            bytes,
            width,
            height,
            stride,
            // Not looked at: a pixel's bytes are moved as they are.
            order: PixelOrder::Bgr,
        };
        let (tens_wide, tens_high) = (width as usize, height as usize);
        // Keep only visible rows. Padding can change on an otherwise
        // identical frame, which would wake the renderer and depth model.
        let wanted = row * tens_high;
        fitted(spare, wanted);
        fitted(narrowed, wanted);
        let taken = match (packing, turned) {
            (Packing::Bytes { bgr }, Turned::Upright) if stride == row => {
                spare.clear();
                spare.extend_from_slice(frame);
                (width, height, bgr)
            }
            (Packing::Tens { red_high }, Turned::Upright) => {
                narrow_tens(frame, tens_wide, tens_high, stride, red_high, spare);
                (width, height, true)
            }
            (Packing::Bytes { bgr }, _) => {
                let (width, height) = live::turn_upright(lying(frame, stride), turned, spare)?;
                (width, height, bgr)
            }
            (Packing::Tens { red_high }, _) => {
                narrow_tens(frame, tens_wide, tens_high, stride, red_high, narrowed);
                let (width, height) = live::turn_upright(lying(narrowed, row), turned, spare)?;
                (width, height, true)
            }
        };
        if !matches!(packing, Packing::Tens { .. }) || turned == Turned::Upright {
            *narrowed = Vec::new();
        }
        Some(taken)
    }
}

/// The note a frame carries about how its picture lies in the buffer
/// (`SPA_META_VideoTransform`): one number, read by
/// [`Turned::from_note`]. The PipeWire library's own type for it is there
/// only when the program is built for PipeWire 0.3.62 or newer; the note's
/// number and its one field are the same for every version.
#[repr(transparent)]
struct TurnNote(u32);

impl spa::buffer::meta::Metadata for TurnNote {
    const META_TYPE: u32 = 8;
}

/// What is asked to come with every frame, each as PipeWire wants it
/// described: the frame's header, which can say that its bytes are no
/// picture, and the note about how the picture lies. A desktop attaches a
/// note only for a receiver that has asked for it, so a rotated screen of
/// a desktop that hands out the panel's own pixels (sway and the other
/// wlroots desktops) was shown lying on its side.
fn wanted_notes() -> Result<Vec<Vec<u8>>, String> {
    let note = |kind: u32, size: usize| {
        let wanted = spa::pod::Object {
            type_: spa::utils::SpaTypes::ObjectParamMeta.as_raw(),
            id: spa::param::ParamType::Meta.as_raw(),
            properties: vec![
                spa::pod::Property::new(
                    spa::sys::SPA_PARAM_META_type,
                    spa::pod::Value::Id(spa::utils::Id(kind)),
                ),
                spa::pod::Property::new(
                    spa::sys::SPA_PARAM_META_size,
                    spa::pod::Value::Int(size as i32),
                ),
            ],
        };
        spa::pod::serialize::PodSerializer::serialize(
            std::io::Cursor::new(Vec::new()),
            &spa::pod::Value::Object(wanted),
        )
        .map(|done| done.0.into_inner())
        .map_err(|e| format!("PipeWire: could not describe the wanted notes: {e:?}"))
    };
    use spa::buffer::meta::{MetaHeader, Metadata};
    Ok(vec![
        note(MetaHeader::META_TYPE, std::mem::size_of::<MetaHeader>())?,
        note(TurnNote::META_TYPE, std::mem::size_of::<TurnNote>())?,
    ])
}

/// What a buffer says about the picture in it.
#[derive(Clone, Copy, Debug)]
struct Told {
    /// Where the picture begins in the buffer's memory, how many bytes of
    /// it are valid, and how far its rows are apart.
    offset: u32,
    size: u32,
    stride: i32,
    /// The buffer or the frame's header is marked as corrupted.
    spoiled: bool,
}

/// The rows of a `width` x `height` picture of four bytes a pixel in the
/// memory `bytes` of a buffer that says `told` about it, and how far apart
/// they are. `None` for a buffer that holds no picture or not all of one.
///
/// A desktop also sends buffers that are no picture, and says so: marked
/// as corrupted (xdg-desktop-portal-wlr for a frame it could not copy, in
/// the buffer and in the frame's header; KDE for an update of the pointer
/// alone), or with no valid bytes at all (GNOME for a frame it could not
/// record, from 43 on with the mark as well). The memory of such a buffer
/// holds an older picture or none; it was read all the same, and shown.
fn picture_in(bytes: &[u8], told: Told, width: u32, height: u32) -> Option<(&[u8], usize)> {
    if told.spoiled || told.size == 0 {
        return None;
    }
    let (offset, size) = (told.offset as usize, told.size as usize);
    let row = width as usize * 4;
    // A distance that cannot be one (none, backwards, shorter than a row)
    // is taken for rows without gaps.
    let stride = usize::try_from(told.stride)
        .ok()
        .filter(|s| *s >= row)
        .unwrap_or(row);
    let needed = match (height as usize).checked_sub(1) {
        Some(rows) if row > 0 => stride.checked_mul(rows)?.checked_add(row)?,
        _ => return None,
    };
    let end = offset.saturating_add(size.min(bytes.len()));
    let valid = bytes.get(offset..end.min(bytes.len()))?;
    Some((valid.get(..needed)?, stride))
}

/// Why [`picture_in`] found no picture in a buffer that says `told` about
/// itself, for the page.
fn no_picture_in(told: Told) -> &'static str {
    if told.spoiled {
        NOT_A_PICTURE
    } else if told.size == 0 {
        "empty"
    } else {
        TOO_SMALL
    }
}

/// What can be wrong with a buffer, said after "the last one was".
const NOT_A_PICTURE: &str = "marked by the desktop as holding no picture";
const TOO_SMALL: &str = "smaller than a picture of the size the desktop named";
const NOT_READABLE: &str =
    "in memory this program cannot read, as a buffer of the graphics card is";
const NOT_ASKED_FOR: &str = "in a pixel format that was not asked for";

/// The longest side of a screen that is shown, in pixels: the same as for
/// a picture that is opened.
const LONGEST_SIDE: u32 = 16384;

/// Why a stream of `width` x `height` pixels is not shown, if it is not:
/// one of its sides is longer than [`LONGEST_SIDE`].
///
/// While a size was wished for, the wish's range ended at 16384 x 16384 and
/// PipeWire refused a stream that was larger than that on both sides ("no
/// more input formats"); one that was larger on one side only it let
/// through, by its way of comparing sizes. Since no size is wished for,
/// nothing was refused: a producer with 17000 x 17000 was shown, with two
/// frames of 1.16 GB held here. The limit is the program's own now, on
/// each side, and has a line of its own on the page.
fn too_large(width: u32, height: u32) -> Option<String> {
    (width > LONGEST_SIDE || height > LONGEST_SIDE).then(|| {
        format!(
            "the screen cast is {width} × {height} pixels, and no more than \
             {LONGEST_SIDE} a side can be shown"
        )
    })
}

/// Room a buffer may keep beyond what a frame needs: not worth giving back.
const SMALL_ROOM: usize = 4096;

/// Let go of `buffer`'s room when it is more than a frame of `wanted` bytes
/// needs: the next frame then gets room of its own size.
///
/// The buffers that take the frames keep the room of the largest frame
/// they have held. After a larger screen (another mode of the shown one, or
/// one picture of it while the desktop changed it) the program stayed that
/// much larger until the output was stopped: 265 MB after 7680 x 4320. A
/// buffer that fits is left alone, so this costs a comparison a frame.
pub(crate) fn fitted(buffer: &mut Vec<u8>, wanted: usize) {
    if buffer.capacity() > wanted.max(SMALL_ROOM) {
        *buffer = Vec::new();
    }
}

/// The most pictures a second a desktop is asked to send: a little more
/// than a screen of 120 Hz draws, because a desktop may leave out a
/// picture that comes a moment sooner after the last one than the limit
/// allows.
///
/// It was 62, a little more than the sixty-odd that are rendered. Measured
/// on a screen of 120 Hz that changed at every refresh, GNOME (mutter 51.1)
/// sent 48 a second when asked for 60, and 60 when asked for 61, 65, 72 or
/// 90; KDE Plasma (KWin 6.8.2) sent what it was asked for, 60, 61, 65, 72
/// or 90.
///
/// But a limit is also a grid. KDE Plasma draws the Odyssey's own screen
/// at every refresh while the 3D picture lies on it, so its cast runs at
/// the limit all the time, a frame every 1/62 s, and what the screen shows
/// is seen at those moments only. Measured with 1.0.3 on the Odyssey at
/// 120 Hz, a film of 24 pictures a second playing on it: the pictures
/// reached the output 32.1 and 48.4 ms apart (two and three of those
/// frames, 597 and 841 times in a minute) where the film's are 41.7 ms
/// apart, each of them on the screen for five refreshes. A video of 60
/// a second lost or doubled a picture twice a second the same way. With
/// the limit above the screen's rate a frame comes for every refresh, and
/// a picture is seen when it is drawn. The frames that are the same as the
/// one before are told while they are still in the stream's buffer
/// ([`Shared::holds`]), and of a screen that changes faster than pictures
/// are made every second or third frame is taken ([`fresh_from`]).
/// A screen of more than 120 Hz is still seen on a grid of 8 ms.
pub(crate) const MOST_A_SECOND: u32 = 125;

/// The stream that is asked for: raw video in one of the pixel formats that
/// are read here, at whatever rate it states for itself and with
/// [`MOST_A_SECOND`] frames a second as the most a desktop is to send. No
/// size is wished for.
///
/// The size is the desktop's to say. KWin (Plasma 6.8) offers every size from
/// 200 x 200 to 10000 x 10000 with the screen's own as its wish, and resizes
/// the picture to whatever the reader wishes for instead; the wish here used
/// to be 1920 x 1080, so every screen came at that size: a 2560 x 1440 one
/// scaled down, a screen turned on its side (1080 x 1920) pressed into the
/// wide shape. Desktops that offer one size (GNOME, wlroots) took no notice.
fn wanted_format() -> spa::pod::Object {
    spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::RGBx,
            spa::param::video::VideoFormat::RGBA,
            // What a desktop that renders the screen with ten bits a colour
            // has to offer, and all it has (`Packing::Tens`).
            spa::param::video::VideoFormat::xRGB_210LE,
            spa::param::video::VideoFormat::xBGR_210LE,
            spa::param::video::VideoFormat::ARGB_210LE,
            spa::param::video::VideoFormat::ABGR_210LE,
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction { num: 60, denom: 1 },
            spa::utils::Fraction { num: 0, denom: 1 },
            // Whatever rate a stream states for itself is taken. With
            // sixty as the most here, a stream of a fixed faster rate was
            // refused ("no more input formats"), and one that comes as the
            // screen changes, which is what the desktops send, was not
            // slowed by it.
            spa::utils::Fraction {
                num: 1000,
                denom: 1
            }
        ),
        // A desktop that sends a picture when the screen has changed says
        // how many a second that may be at the most, and is asked to keep
        // to `MOST_A_SECOND`.
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoMaxFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction {
                num: MOST_A_SECOND,
                denom: 1
            },
            spa::utils::Fraction { num: 1, denom: 1 },
            spa::utils::Fraction {
                num: MOST_A_SECOND,
                denom: 1
            }
        ),
    )
}

struct StreamData {
    format: spa::param::video::VideoInfoRaw,
    shared: Arc<Shared>,
    /// Where a frame is copied as it arrives; it then changes places with
    /// the slot's, so the two buffers serve every frame and the reader is
    /// kept waiting for the exchange only, not for the copy.
    spare: Vec<u8>,
    /// Where a frame of ten bits a colour that does not lie upright is
    /// written as bytes before it is turned; empty for every other frame.
    narrowed: Vec<u8>,
}

fn stream(
    shared: &Arc<Shared>,
    node: u32,
    fd: OwnedFd,
    wake: &(impl Fn() + Send + 'static),
) -> Result<(), String> {
    let failed = |what: &str| {
        let what = what.to_owned();
        move |e: pw::Error| format!("PipeWire: {what}: {e}")
    };
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(failed("no main loop"))?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(failed("no context"))?;
    let core = context
        .connect_fd_rc(fd, None)
        .map_err(failed("could not connect to the screen cast"))?;
    let stream = pw::stream::StreamBox::new(
        &core,
        "any2odyssey-capture",
        pw::properties::properties! {
            *pw::keys::MEDIA_TYPE => "Video",
            *pw::keys::MEDIA_CATEGORY => "Capture",
            *pw::keys::MEDIA_ROLE => "Screen",
        },
    )
    .map_err(failed("no stream"))?;

    let error: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    // Whatever ends the cast says why (unless it was stopped from here)
    // and has the loop left.
    let ended = {
        let (error, mainloop, shared) = (error.clone(), mainloop.clone(), shared.clone());
        move |reason: String| {
            if !shared.stop.load(Ordering::Relaxed) {
                lock(&error).get_or_insert(reason);
            }
            mainloop.quit();
        }
    };
    // The screen's node is watched as well as the stream: when the node
    // goes, the stream is told nothing but to pause (see `ending`). A cast
    // that cannot be watched like this is shown all the same.
    let registry = core.get_registry_rc().ok();
    let node_ended = ended.clone();
    let size_ended = ended.clone();
    let _registry_listener = registry.as_ref().map(|registry| {
        registry
            .add_listener_local()
            .global_remove(move |id| {
                if let Some(reason) = ending(node, &Seen::Removed(id)) {
                    node_ended(reason);
                }
            })
            .register()
    });
    let data = StreamData {
        format: Default::default(),
        shared: shared.clone(),
        spare: Vec::new(),
        narrowed: Vec::new(),
    };
    let notes = wanted_notes()?;
    let _listener = stream
        .add_local_listener_with_user_data(data)
        .state_changed(move |_, data, old, new| {
            {
                let mut paused = lock(&data.shared.paused);
                *paused = pause_after(*paused, &old, &new, Instant::now());
            }
            if let Some(reason) = ending(node, &Seen::State(old, new)) {
                ended(reason);
            }
        })
        .param_changed(move |stream, data, id, param| {
            let Some(param) = param else { return };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Ok((media_type, media_subtype)) = spa::param::format_utils::parse_format(param)
            else {
                return;
            };
            if media_type == spa::param::format::MediaType::Video
                && media_subtype == spa::param::format::MediaSubtype::Raw
            {
                let _ = data.format.parse(param);
                // A size that is not shown ends the cast here, at the start
                // and whenever the size changes: before buffers for it are
                // agreed on, and so before one is mapped or copied.
                let size = data.format.size();
                if let Some(reason) = too_large(size.width, size.height) {
                    size_ended(reason);
                    return;
                }
                // The notes are asked for with every format that is agreed
                // on: the buffers are made anew for it.
                let mut notes: Vec<_> = notes
                    .iter()
                    .filter_map(|note| spa::pod::Pod::from_bytes(note))
                    .collect();
                let _ = stream.update_params(&mut notes);
            }
        })
        .process(|stream, data| {
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(packing) = packing(data.format.format()) else {
                data.shared.no_picture(NOT_ASKED_FOR);
                return;
            };
            let (width, height) = (data.format.size().width, data.format.size().height);
            // The cast is being ended (see `param_changed`).
            if too_large(width, height).is_some() {
                return;
            }
            // A note that names no turn leaves the frame as it comes.
            let turned = buffer
                .find_meta::<TurnNote>()
                .and_then(|note| Turned::from_note(note.0))
                .unwrap_or_default();
            let header_spoiled = buffer
                .find_meta::<spa::buffer::meta::MetaHeader>()
                .is_some_and(|header| {
                    let corrupted = spa::buffer::meta::MetaHeaderFlags::CORRUPTED;
                    header.flags().contains(corrupted)
                });
            let datas = buffer.datas_mut();
            let Some(plane) = datas.first_mut() else {
                data.shared.no_picture(NOT_READABLE);
                return;
            };
            let chunk = plane.chunk();
            let told = Told {
                offset: chunk.offset(),
                size: chunk.size(),
                stride: chunk.stride(),
                spoiled: header_spoiled
                    || chunk.flags().contains(spa::buffer::ChunkFlags::CORRUPTED),
            };
            let Some(bytes) = plane.data() else {
                data.shared.no_picture(NOT_READABLE);
                return;
            };
            let Some((frame, stride)) = picture_in(bytes, told, width, height) else {
                data.shared.no_picture(no_picture_in(told));
                return;
            };
            let lying = Lying {
                rows: frame,
                width,
                height,
                stride,
                packing,
                turned,
            };
            if data.shared.holds(&lying) {
                return;
            }
            let taken = lying.taken(&mut data.spare, &mut data.narrowed);
            let Some((width, height, bgr)) = taken else {
                data.shared.no_picture(TOO_SMALL);
                return;
            };
            let stride = width as usize * 4;
            data.shared
                .frame_arrived(&mut data.spare, width, height, stride, bgr);
        })
        .register()
        .map_err(failed("no stream listener"))?;

    let format = wanted_format();
    let values = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(format),
    )
    .map_err(|e| format!("PipeWire: could not describe the wanted format: {e:?}"))?
    .0
    .into_inner();
    let mut params = [spa::pod::Pod::from_bytes(&values)
        .ok_or("PipeWire: could not describe the wanted format")?];
    // Mapped buffers only: the frames are read on the processor, so the
    // desktop is asked for ordinary memory, not for a GPU buffer.
    stream
        .connect(
            spa::utils::Direction::Input,
            Some(node),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(failed("could not connect the stream"))?;

    // The loop runs until the capture is stopped; it looks ten times a
    // second, and tells the page when the first frame has come, or the
    // first buffer that is none.
    let (timer_loop, timer_shared) = (mainloop.clone(), shared.clone());
    let announced = AtomicBool::new(false);
    let no_picture_told = AtomicBool::new(false);
    let wake_once = {
        let shared = shared.clone();
        move || {
            if announced.load(Ordering::Relaxed) {
                return false;
            }
            if matches!(*lock(&shared.state), CaptureState::Running { .. }) {
                announced.store(true, Ordering::Relaxed);
                return true;
            }
            // Buffers come and none of them holds a picture: the page is
            // told that as well, once; it then looks again by itself.
            if lock(&shared.unread).0 > 0 && !no_picture_told.swap(true, Ordering::Relaxed) {
                return true;
            }
            false
        }
    };
    // `wake` is not 'static inside the loop's closure; a channel-free way to
    // call it is from this thread after each timer tick.
    let ticked = Arc::new(AtomicBool::new(false));
    let tick_flag = ticked.clone();
    let timer = mainloop.loop_().add_timer(move |_| {
        if timer_shared.stop.load(Ordering::Relaxed) {
            timer_loop.quit();
        }
        if wake_once() {
            tick_flag.store(true, Ordering::Relaxed);
            timer_loop.quit();
        }
    });
    let tick = Duration::from_millis(100);
    timer
        .update_timer(Some(tick), Some(tick))
        .into_result()
        .map_err(|e| format!("PipeWire: no timer: {e}"))?;
    loop {
        mainloop.run();
        // The first frame has arrived: tell the page, then carry on, unless
        // the stream ended in the same turn of the loop as that tick. Its
        // request to leave was this same return, and going back in would
        // wait for a stream that is over until the capture is stopped.
        if ticked.swap(false, Ordering::Relaxed) && lock(&error).is_none() {
            wake();
            continue;
        }
        break;
    }
    let _ = stream.disconnect();
    let reason = lock(&error).take();
    match reason {
        Some(reason) => Err(reason),
        None => Ok(()),
    }
}

/// A capture fed by hand, for tests of what reads the frames.
#[cfg(test)]
pub struct Fed(Arc<Shared>);

#[cfg(test)]
impl Fed {
    pub fn start() -> Fed {
        Fed(Arc::new(Shared::new()))
    }

    /// The desktop's dialog has been answered with a screen.
    pub fn chosen(&self) {
        self.0.chosen();
    }

    /// The desktop has paused the cast (since `since`) or lets it run
    /// again (`None`).
    pub fn pause(&self, since: Option<Instant>) {
        *lock(&self.0.paused) = since;
    }

    /// A buffer that the desktop marks as holding no picture.
    pub fn feed_no_picture(&self) {
        self.0.no_picture(NOT_A_PICTURE);
    }

    pub fn frames(&self) -> Frames {
        Frames(self.0.clone())
    }

    /// The capture itself, for what owns one: no desktop is asked and no
    /// thread runs, its frames are the ones fed here.
    pub fn capture(&self) -> Capture {
        Capture {
            shared: self.0.clone(),
            thread: None,
        }
    }

    /// A frame of `width` x `height` BGRx pixels, as the stream delivers one.
    pub fn feed(&self, width: u32, height: u32, bytes: &[u8]) {
        let mut slot = lock(&self.0.slot);
        slot.bytes.clear();
        slot.bytes.extend_from_slice(bytes);
        (slot.width, slot.height, slot.stride) = (width, height, width as usize * 4);
        slot.bgr = true;
        slot.serial += 1;
        // As the stream's thread notes it for a frame of the desktop's.
        slot.arrived = Some(Instant::now());
        drop(slot);
        *lock(&self.0.unread) = (0, "");
        *lock(&self.0.state) = CaptureState::Running { width, height };
        self.0.arrived.notify_all();
    }
}

#[cfg(test)]
impl Drop for Fed {
    fn drop(&mut self) {
        self.0.stop.store(true, Ordering::Relaxed);
        self.0.arrived.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pw::stream::StreamState::{Connecting, Error, Paused, Streaming, Unconnected};

    const ENDED: &str = "the screen cast was ended by the desktop";

    #[test]
    fn no_size_is_wished_for_and_every_format_wished_for_can_be_read() {
        use spa::param::format::FormatProperties as Key;
        let format = wanted_format();
        let has = |key: Key| format.properties.iter().any(|p| p.key == key.as_raw());
        // KWin resizes the picture to the size a reader wishes for: with
        // 1920 x 1080 wished for, a 2560 x 1440 screen came at 1920 x 1080.
        assert!(!has(Key::VideoSize));
        assert!(has(Key::MediaType) && has(Key::MediaSubtype) && has(Key::VideoFramerate));
        assert!(has(Key::VideoMaxFramerate));
        let formats = format
            .properties
            .iter()
            .find(|p| p.key == Key::VideoFormat.as_raw())
            .expect("the pixel formats are named");
        let spa::pod::Value::Choice(spa::pod::ChoiceValue::Id(spa::utils::Choice(
            _,
            spa::utils::ChoiceEnum::Enum {
                default,
                alternatives,
            },
        ))) = &formats.value
        else {
            panic!("the pixel formats are a choice: {:?}", formats.value);
        };
        assert!(!alternatives.is_empty());
        for id in alternatives.iter().chain([default]) {
            let format = spa::param::video::VideoFormat::from_raw(id.0);
            assert!(packing(format).is_some(), "{format:?} is wished for");
        }
        // And it can be written as PipeWire wants it.
        let written = spa::pod::serialize::PodSerializer::serialize(
            std::io::Cursor::new(Vec::new()),
            &spa::pod::Value::Object(format),
        );
        assert!(written.is_ok());
    }

    #[test]
    fn a_little_over_sixty_a_second_is_the_most_asked_for_and_a_streams_own_rate_is_taken() {
        use spa::param::format::FormatProperties as Key;
        use spa::pod::{ChoiceValue, Value};
        use spa::utils::{Choice, ChoiceEnum};
        let format = wanted_format();
        // A range of whole numbers a second: the wish, the least, the most.
        let range = |key: Key| {
            let named = format.properties.iter().find(|p| p.key == key.as_raw());
            let Some(Value::Choice(ChoiceValue::Fraction(Choice(
                _,
                ChoiceEnum::Range { default, min, max },
            )))) = named.map(|p| &p.value)
            else {
                panic!("{key:?} is not a range: {named:?}");
            };
            assert_eq!((default.denom, min.denom, max.denom), (1, 1, 1));
            (default.num, min.num, max.num)
        };
        // What a desktop is to send at the most. It names its own limit as
        // a range from 1 to the screen's rate, and the two are cut to one.
        // Not 60: asked for exactly that, GNOME sent 48 a second.
        // Not 62 either: a film of 24 pictures a second was then seen on a
        // grid of 1/62 s (`MOST_A_SECOND`).
        assert_eq!(range(Key::VideoMaxFramerate), (125, 1, 125));
        assert_eq!(MOST_A_SECOND, 125);
        // The stream's own rate: 0 is "as the screen changes", and a fixed
        // rate of 144 or 240 was refused while 60 was the most here.
        let (_, least, most) = range(Key::VideoFramerate);
        assert_eq!(least, 0);
        assert!(most >= 240);
    }

    #[test]
    fn a_frame_that_differs_only_in_the_byte_that_holds_no_colour_is_the_same_picture() {
        // A screen wider than one stretch of the comparison and not a
        // whole number of them, as BGRx: blue, green, red and a byte that
        // holds nothing.
        let (width, height) = (1500u32, 3u32);
        let bytes = width as usize * height as usize * 4;
        let picture: Vec<u8> = (0..bytes).map(|i| (i * 31 % 253) as u8).collect();
        let shared = Shared::new();
        let serial = || lock(&shared.slot).serial;
        let arrive = |bytes: &[u8], stride, bgr| {
            let mut frame = bytes.to_vec();
            shared.frame_arrived(&mut frame, width, height, stride, bgr);
        };
        let row = width as usize * 4;
        arrive(&picture, row, true);
        assert_eq!(serial(), 1);
        // The desktop sends the screen again with other leftovers in
        // every pixel's fourth byte. It was a new picture each time.
        for leftover in [0u8, 255, 77] {
            let mut again = picture.clone();
            for pixel in again.as_chunks_mut::<4>().0 {
                pixel[3] = pixel[3].wrapping_mul(7) ^ leftover;
            }
            assert_ne!(again, picture);
            arrive(&again, row, true);
            assert_eq!(serial(), 1, "fourth bytes {leftover}");
        }
        // What the reader has is the frame it was told of.
        assert_eq!(lock(&shared.slot).source().bytes, &picture[..]);
        // Any colour of any pixel that differs by anything is a new
        // picture: the first pixel's, the last one's, one in the middle of
        // a stretch and those on either side of where two stretches meet.
        let mut expected = 1;
        let mut shown = picture.clone();
        for pixel in [0, 1, 500, 1023, 1024, 1025, 2047, 2048, 4095, 4096, 4499] {
            for colour in 0..3 {
                for bit in [1u8, 128] {
                    shown[pixel * 4 + colour] ^= bit;
                    arrive(&shown, row, true);
                    expected += 1;
                    assert_eq!(serial(), expected, "pixel {pixel}, colour {colour}");
                    assert_eq!(lock(&shared.slot).source().bytes, &shown[..]);
                    // (and its fourth byte with it, or alone after it)
                    shown[pixel * 4 + 3] ^= bit;
                    arrive(&shown, row, true);
                    assert_eq!(serial(), expected);
                }
            }
        }
        // The same for the formats with red first.
        arrive(&shown, row, false);
        expected += 1;
        assert_eq!(serial(), expected);
        let mut again = shown.clone();
        again[7] ^= 0xff;
        arrive(&again, row, false);
        assert_eq!(serial(), expected);

        // By itself: frames of a few lengths around a stretch and two of
        // them, one byte of each changed in turn.
        for pixels in [0usize, 1, 2, 1023, 1024, 1025, 2048, 2051] {
            let a: Vec<u8> = (0..pixels * 4).map(|i| (i * 13 % 251) as u8).collect();
            assert!(same_colours(&a, &a));
            // (a pixel more is another picture, whatever is in it)
            let mut longer = a.clone();
            longer.extend_from_slice(&[0; 4]);
            assert!(!same_colours(&a, &longer) && !same_colours(&longer, &a));
            for at in (0..pixels * 4).step_by(if pixels > 100 { 97 } else { 1 }) {
                let mut b = a.clone();
                b[at] = b[at].wrapping_add(1 + (at % 200) as u8);
                let fourth = at % 4 == 3;
                assert_eq!(same_colours(&a, &b), fourth, "{pixels} pixels, {at}");
                assert_eq!(same_colours(&b, &a), fourth, "{pixels} pixels, {at}");
            }
        }
        // Bytes that are not a whole pixel count as they are.
        assert!(same_colours(&[1, 2, 3, 4, 5, 6], &[1, 2, 3, 9, 5, 6]));
        assert!(!same_colours(&[1, 2, 3, 4, 5, 6], &[1, 2, 3, 4, 5, 7]));
        // Rows with something between them are compared as they are.
        let padded = Shared::new();
        let mut frame = picture.clone();
        padded.frame_arrived(&mut frame, width - 1, height, row, true);
        let mut frame = picture.clone();
        frame[3] ^= 1;
        padded.frame_arrived(&mut frame, width - 1, height, row, true);
        assert_eq!(lock(&padded.slot).serial, 2);
    }

    #[test]
    fn a_frame_the_slot_holds_already_is_told_in_the_streams_buffer() {
        let (width, height) = (5u32, 3u32);
        let row = width as usize * 4;
        let picture: Vec<u8> = (0..row * height as usize).map(|i| (i * 7) as u8).collect();
        fn lying(rows: &[u8], stride: usize, packing: Packing, turned: Turned) -> Lying<'_> {
            Lying {
                rows,
                width: 5,
                height: 3,
                stride,
                packing,
                turned,
            }
        }
        let bgr = Packing::Bytes { bgr: true };
        let shared = Shared::new();
        // Nothing is held before the first frame.
        assert!(!shared.holds(&lying(&picture, row, bgr, Turned::Upright)));
        shared.frame_arrived(&mut picture.clone(), width, height, row, true);
        shared.no_picture(NOT_A_PICTURE);
        // The same picture again, and it counts as a frame that arrived.
        assert!(shared.holds(&lying(&picture, row, bgr, Turned::Upright)));
        assert_eq!(*lock(&shared.unread), (0, ""));
        // With other leftovers in the bytes that hold no colour, and with
        // something between its rows.
        let mut leftovers = picture.clone();
        let mut padded = Vec::new();
        for (index, line) in picture.chunks(row).enumerate() {
            padded.extend_from_slice(line);
            if index + 1 < height as usize {
                padded.extend_from_slice(&[index as u8 + 9; 8]);
            }
        }
        for pixel in leftovers.as_chunks_mut::<4>().0 {
            pixel[3] ^= 255;
        }
        assert!(shared.holds(&lying(&leftovers, row, bgr, Turned::Upright)));
        assert!(shared.holds(&lying(&padded, row + 8, bgr, Turned::Upright)));
        // One colour of the last pixel is another: a new picture, in
        // either lie.
        for (rows, stride) in [(&picture, row), (&padded, row + 8)] {
            let mut other = rows.clone();
            let last = other.len() - 2;
            other[last] ^= 1;
            assert!(!shared.holds(&lying(&other, stride, bgr, Turned::Upright)));
        }
        // Not the same: the colours the other way round, a picture that
        // is turned, one of another shape, one of ten bits a colour.
        let rgb = Packing::Bytes { bgr: false };
        assert!(!shared.holds(&lying(&picture, row, rgb, Turned::Upright)));
        let tens = Packing::Tens { red_high: true };
        assert!(!shared.holds(&lying(&picture, row, tens, Turned::Upright)));
        assert!(!shared.holds(&lying(&picture, row, bgr, Turned::Half)));
        let wider = Lying {
            width: width + 1,
            ..lying(&picture, row, bgr, Turned::Upright)
        };
        assert!(!shared.holds(&wider));
        // Nothing of all this made a new frame.
        assert_eq!(lock(&shared.slot).serial, 1);
    }

    #[test]
    fn frames_that_come_too_soon_after_the_one_taken_last_are_let_pass() {
        let shared = Arc::new(Shared::new());
        let frames = Frames(shared.clone());
        let arrive = |shade: u8| shared.frame_arrived(&mut vec![shade; 16], 2, 2, 8, true);
        let pace = Duration::from_millis(400);
        let patience = Duration::from_secs(5);
        arrive(1);
        // The first frame: nothing is known of how fast they come.
        let taken = frames.with_fresh(0, None, pace, patience, |slot| {
            (slot.serial, slot.arrived())
        });
        let (serial, first) = taken.unwrap();
        assert_eq!(serial, 1);
        std::thread::sleep(Duration::from_millis(200));
        arrive(2);
        // A second one 200 ms later lies waiting, and the reader takes one
        // every 400 ms: it is let pass, and the one that comes 500 ms after
        // the first is taken as it arrives.
        let sender = {
            let shared = shared.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                shared.frame_arrived(&mut vec![3; 16], 2, 2, 8, true);
            })
        };
        let began = Instant::now();
        let taken = frames.with_fresh(1, first, pace, patience, |slot| slot.serial);
        assert_eq!(taken, Some(3));
        assert!(
            began.elapsed() >= Duration::from_millis(200),
            "{:?}",
            began.elapsed()
        );
        sender.join().unwrap();
        // None comes after that one: the wait is given up, the newest
        // there is is taken, and it is not waited for a second time.
        let third = lock(&shared.slot).arrived;
        let began = Instant::now();
        assert_eq!(
            frames.with_fresh(2, third, pace, patience, |slot| slot.serial),
            Some(3)
        );
        assert!(
            began.elapsed() < Duration::from_secs(3),
            "{:?}",
            began.elapsed()
        );
        // A reader that says no pace takes what is there at once.
        let began = Instant::now();
        assert_eq!(frames.with_newest(2, patience, |slot| slot.serial), Some(3));
        assert!(began.elapsed() < Duration::from_millis(200));
    }

    #[test]
    fn a_reader_asked_to_look_again_does_not_wait_for_another_frame() {
        let shared = Arc::new(Shared::new());
        let frames = Frames(shared.clone());
        shared.frame_arrived(&mut vec![1; 16], 2, 2, 8, true);
        let patience = Duration::from_secs(20);
        // Asked while it waits: the wait ends, with the frame it has.
        let asker = {
            let frames = frames.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                frames.look_again();
            })
        };
        let began = Instant::now();
        assert_eq!(frames.with_newest(1, patience, |slot| slot.serial), Some(1));
        assert!(
            began.elapsed() < Duration::from_secs(10),
            "{:?}",
            began.elapsed()
        );
        asker.join().unwrap();
        // Asked before it waits: its next wait is over at once, and only
        // that one.
        frames.look_again();
        let began = Instant::now();
        assert_eq!(frames.with_newest(1, patience, |slot| slot.serial), Some(1));
        assert!(
            began.elapsed() < Duration::from_secs(10),
            "{:?}",
            began.elapsed()
        );
        let began = Instant::now();
        let short = Duration::from_millis(150);
        assert_eq!(frames.with_newest(1, short, |slot| slot.serial), Some(1));
        assert!(
            began.elapsed() >= Duration::from_millis(100),
            "{:?}",
            began.elapsed()
        );
    }

    #[test]
    fn a_frame_that_is_the_picture_already_there_is_not_a_new_one() {
        let shared = Shared::new();
        let serial = || lock(&shared.slot).serial;
        let picture: Vec<u8> = (0..2 * 3 * 4).collect();
        let arrive = |bytes: &[u8], width, height, stride, bgr| {
            let mut frame = bytes.to_vec();
            shared.frame_arrived(&mut frame, width, height, stride, bgr);
        };
        arrive(&picture, 2, 3, 8, true);
        assert_eq!(serial(), 1);
        assert_eq!(
            *lock(&shared.state),
            CaptureState::Running {
                width: 2,
                height: 3
            }
        );
        // The same picture again, as often as the desktop likes: the reader
        // is not told, and what it reads is what it had.
        for _ in 0..3 {
            arrive(&picture, 2, 3, 8, true);
        }
        assert_eq!(serial(), 1);
        assert_eq!(lock(&shared.slot).source().bytes, &picture[..]);
        // One colour of it differs by one, the last pixel's: a new picture.
        let mut changed = picture.clone();
        changed[2 * 3 * 4 - 2] ^= 1;
        arrive(&changed, 2, 3, 8, true);
        assert_eq!(serial(), 2);
        assert_eq!(lock(&shared.slot).source().bytes, &changed[..]);
        // And back: new again.
        arrive(&picture, 2, 3, 8, true);
        assert_eq!(serial(), 3);
        // The same bytes as another picture: other sides, the colours the
        // other way round.
        arrive(&picture, 3, 2, 12, true);
        assert_eq!(serial(), 4);
        assert_eq!(
            *lock(&shared.state),
            CaptureState::Running {
                width: 3,
                height: 2
            }
        );
        arrive(&picture, 3, 2, 12, false);
        assert_eq!(serial(), 5);
        // A buffer without a picture between two frames that are the same
        // does not make the second one new, and is forgotten with it.
        shared.no_picture(NOT_A_PICTURE);
        arrive(&picture, 3, 2, 12, false);
        assert_eq!(serial(), 5);
        assert_eq!(*lock(&shared.unread), (0, ""));
    }

    #[test]
    fn buffers_that_hold_no_picture_are_counted_with_the_reason_until_one_does() {
        let fed = Fed::start();
        let capture = fed.capture();
        assert_eq!(capture.unread(), None);
        fed.feed_no_picture();
        fed.feed_no_picture();
        assert_eq!(capture.unread(), Some((2, NOT_A_PICTURE)));
        fed.0.no_picture(NOT_READABLE);
        assert_eq!(capture.unread(), Some((3, NOT_READABLE)));
        fed.feed(1, 1, &[0; 4]);
        assert_eq!(capture.unread(), None);
        // What is said of each buffer that `picture_in` finds no picture in.
        let whole = Told {
            offset: 0,
            size: 2 * 2 * 4,
            stride: 8,
            spoiled: false,
        };
        let memory = [0u8; 2 * 2 * 4];
        assert!(picture_in(&memory, whole, 2, 2).is_some());
        for (told, why) in [
            (
                Told {
                    spoiled: true,
                    ..whole
                },
                NOT_A_PICTURE,
            ),
            (Told { size: 0, ..whole }, "empty"),
            (Told { size: 12, ..whole }, TOO_SMALL),
        ] {
            assert!(picture_in(&memory, told, 2, 2).is_none(), "{told:?}");
            assert_eq!(no_picture_in(told), why, "{told:?}");
        }
    }

    #[test]
    fn the_pointer_is_asked_for_by_name_only_where_it_is_drawn() {
        use ashpd::enumflags2::BitFlags;
        let all = CursorMode::Hidden | CursorMode::Embedded | CursorMode::Metadata;
        assert_eq!(wished_cursor(true, Some(all)), Some(CursorMode::Embedded));
        // No pointer wanted: nothing is named, which the portal takes for
        // hidden. Named, it was refused for a moment after the portal's
        // backend had been started again ("Unavailable cursor mode 1").
        assert_eq!(wished_cursor(false, Some(all)), None);
        assert_eq!(wished_cursor(false, None), None);
        // A desktop that cannot draw it, or does not say what it can.
        let cannot = CursorMode::Hidden | CursorMode::Metadata;
        assert_eq!(wished_cursor(true, Some(cannot)), None);
        assert_eq!(wished_cursor(true, Some(BitFlags::empty())), None);
        assert_eq!(wished_cursor(true, None), None);
    }

    #[test]
    fn a_token_that_cannot_be_one_is_not_handed_to_the_portal() {
        let handed = |token: &str| sendable(Some(token.to_owned()));
        // What portals give: a word (xdg-desktop-portal 1.22), a UUID.
        for token in [
            "OQktWWv1O2aZUaxNzjVHdg",
            "TU4MM6_f32JA__rkXhqrgw",
            "f81d4fae-7dec-11d0-a765-00a0c91e6bf6",
        ] {
            assert_eq!(handed(token).as_deref(), Some(token));
        }
        // A file of zero bytes, as a power cut can leave one, and text with
        // a zero byte in it: sent to the portal, each cost the program its
        // connection to the session bus, for good.
        let zeros = "\0".repeat(22);
        for token in [zeros.as_str(), "\0", "abc\0def", "OQktWWv1O2aZUaxNzjVHd\0"] {
            assert_eq!(handed(token), None, "{token:?}");
        }
        // Nothing else that no token holds either, and no empty one.
        for token in ["abc\ndef", "abc\u{1}def", "abc\u{7f}", "a\tb", "\u{85}", ""] {
            assert_eq!(handed(token), None, "{token:?}");
        }
        assert_eq!(sendable(None), None);
        // What is text is the portal's to judge (and is asked for again
        // without it where the portal calls it invalid).
        for token in ["not-a-token-at-all", "two words", "grün-日本"] {
            assert_eq!(handed(token).as_deref(), Some(token));
        }
    }

    #[test]
    fn a_token_the_portal_calls_invalid_is_asked_for_again_without_it() {
        type Answer = Result<&'static str, Refused>;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        // What xdg-desktop-portal 1.22 answers to a restore token that is
        // not one by its form.
        let invalid = |what: &str| {
            Refused::Portal(ashpd::Error::Portal(ashpd::PortalError::InvalidArgument(
                what.to_owned(),
            )))
        };
        let not_valid = || invalid("Restore token is not valid");
        // The desktop's answers in turn, and the tokens it was asked with.
        let outcome = |token: Option<&str>, answers: Vec<Answer>| {
            let answers = std::cell::RefCell::new(std::collections::VecDeque::from(answers));
            let asked = std::cell::RefCell::new(Vec::new());
            let came = runtime.block_on(asked_again_without(token.map(str::to_owned), |token| {
                asked.borrow_mut().push(token);
                let answer = answers.borrow_mut().pop_front().expect("asked once more");
                async move { answer }
            }));
            assert!(answers.borrow().is_empty(), "an answer was not asked for");
            (came.map_err(|refused| refused.said()), asked.into_inner())
        };
        let token = || Some("not-a-token".to_owned());
        // The token in the file is none to the portal: the page read "Live
        // capture ended: the desktop's screen-cast portal refused: ..." at
        // every Start. The desktop is asked again and shows its dialog.
        assert_eq!(
            outcome(Some("not-a-token"), vec![Err(not_valid()), Ok("a screen")]),
            (Ok("a screen"), vec![token(), None])
        );
        // Once: what the second asking comes to is what the page says.
        let (came, asked) = outcome(
            Some("not-a-token"),
            vec![
                Err(not_valid()),
                Err(Refused::Portal(ashpd::Error::Response(
                    ashpd::desktop::ResponseError::Cancelled,
                ))),
            ],
        );
        assert_eq!(came, Err("no screen was chosen".to_owned()));
        assert_eq!(asked, vec![token(), None]);
        let (came, asked) = outcome(
            Some("not-a-token"),
            vec![Err(not_valid()), Err(invalid("Unavailable cursor mode 2"))],
        );
        assert!(came.is_err_and(|said| said.ends_with("Unavailable cursor mode 2")));
        assert_eq!(asked.len(), 2);
        // A token that is taken, and every other refusal: asked once.
        assert_eq!(
            outcome(Some("a-token"), vec![Ok("a screen")]),
            (Ok("a screen"), vec![Some("a-token".to_owned())])
        );
        for refused in [
            Refused::Closed,
            Refused::Portal(ashpd::Error::Response(
                ashpd::desktop::ResponseError::Cancelled,
            )),
            Refused::Portal(ashpd::Error::Response(ashpd::desktop::ResponseError::Other)),
            Refused::Portal(ashpd::Error::Portal(ashpd::PortalError::NotAllowed(
                "no".to_owned(),
            ))),
            Refused::Portal(ashpd::Error::NoResponse),
        ] {
            assert!(!refused.of_what_was_asked(), "{refused:?}");
            let (came, asked) = outcome(Some("a-token"), vec![Err(refused)]);
            assert!(came.is_err());
            assert_eq!(asked.len(), 1);
        }
        // A token that cannot be sent is not: the desktop is asked without
        // one, and once.
        for unsendable in ["abc\0def", "\0\0\0\0", ""] {
            assert_eq!(
                outcome(Some(unsendable), vec![Ok("a screen")]),
                (Ok("a screen"), vec![None])
            );
        }
        // Without a token there is nothing to leave out.
        let (came, asked) = outcome(None, vec![Err(not_valid())]);
        assert!(came.is_err());
        assert_eq!(asked, vec![None]);
    }

    /// What a session's "closed" signals are in a test: each poll takes the
    /// next of these, and after the last nothing more comes.
    struct Signals(std::collections::VecDeque<std::task::Poll<Option<()>>>);

    impl ashpd::zbus::export::futures_core::Stream for Signals {
        type Item = ();

        fn poll_next(
            mut self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Option<()>> {
            self.0.pop_front().unwrap_or(std::task::Poll::Pending)
        }
    }

    /// An answer that is there at the `after`th time it is looked for.
    struct Answer {
        after: u32,
        polled: std::rc::Rc<std::cell::Cell<u32>>,
    }

    impl std::future::Future for Answer {
        type Output = &'static str;

        fn poll(
            self: std::pin::Pin<&mut Self>,
            context: &mut std::task::Context<'_>,
        ) -> std::task::Poll<&'static str> {
            self.polled.set(self.polled.get() + 1);
            if self.polled.get() >= self.after {
                return std::task::Poll::Ready("answered");
            }
            context.waker().wake_by_ref();
            std::task::Poll::Pending
        }
    }

    #[test]
    fn a_session_closed_while_its_start_is_asked_for_ends_the_wait() {
        use std::task::Poll::{Pending, Ready};
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let outcome = |after: u32, signals: &[std::task::Poll<Option<()>>]| {
            let polled = std::rc::Rc::new(std::cell::Cell::new(0));
            let answer = Answer {
                after,
                polled: polled.clone(),
            };
            let signals = Signals(signals.iter().copied().collect());
            (
                runtime.block_on(unless_ended(answer, signals)),
                polled.get(),
            )
        };
        // niri 26.04 where it cannot start the cast: the session is closed
        // and the request to start is never answered. The wait used to go
        // on until the output was stopped.
        assert_eq!(outcome(u32::MAX, &[Ready(Some(()))]).0, None);
        assert_eq!(
            outcome(u32::MAX, &[Pending, Pending, Ready(Some(()))]).0,
            None
        );
        // The ordinary case: an answer, and no closing.
        assert_eq!(outcome(1, &[]), (Some("answered"), 1));
        assert_eq!(outcome(4, &[]), (Some("answered"), 4));
        // A cancelled dialog: the answer is sent first and the session
        // closed after it. An answer that is there when the closing is seen
        // counts, so the page says that no screen was chosen.
        assert_eq!(outcome(2, &[Ready(Some(()))]), (Some("answered"), 2));
        // The closings are over (the bus went away): the answer is still
        // waited for, and they are not asked for again.
        assert_eq!(outcome(5, &[Pending, Ready(None)]), (Some("answered"), 5));
    }

    #[test]
    fn the_page_says_what_is_missing_where_no_screen_cast_can_be_had() {
        use ashpd::desktop::ResponseError;
        use ashpd::zbus;
        let said = |error: ashpd::Error| Refused::Portal(error).said();
        // An X11 session of Xfce or i3 with the GTK portal backend alone.
        let interface = "org.freedesktop.portal.ScreenCast";
        let no_cast = said(ashpd::Error::PortalNotFound(
            zbus::names::OwnedInterfaceName::try_from(interface).unwrap(),
        ));
        assert_eq!(no_cast, NO_SCREEN_CAST);
        assert!(no_cast.contains("xdg-desktop-portal") && no_cast.contains("X11"));
        // No portal installed: the bus knows nobody of that name, said in
        // either of the two ways the bus library has for it.
        let unknown = || zbus::fdo::Error::ServiceUnknown("no .service file".to_owned());
        let wrapped = zbus::Error::FDO(Box::new(unknown()));
        assert_eq!(said(ashpd::Error::Zbus(wrapped)), NO_PORTAL);
        let wrapped = zbus::Error::FDO(Box::new(unknown()));
        let through_portal = ashpd::PortalError::ZBus(wrapped);
        assert_eq!(said(ashpd::Error::Portal(through_portal)), NO_PORTAL);
        // No session bus: the address is named, so that it can be looked at.
        let no_bus = said(ashpd::Error::Zbus(zbus::Error::Address(
            "unix:path=/nowhere".to_owned(),
        )));
        assert!(
            no_bus.starts_with(NO_BUS) && no_bus.contains("/nowhere"),
            "{no_bus}"
        );
        // GNOME's or KDE's backend in an X11 session answers "other".
        assert_eq!(
            said(ashpd::Error::Response(ResponseError::Other)),
            NO_REASON
        );
        // As before: the dialog was cancelled.
        assert_eq!(
            said(ashpd::Error::Response(ResponseError::Cancelled)),
            "no screen was chosen"
        );
        assert_eq!(Refused::Closed.said(), CLOSED_UNSTARTED);
        // Anything else is passed on in the portal client's words.
        assert_eq!(
            said(ashpd::Error::NoResponse),
            "the desktop's screen-cast portal refused: Portal request didn't receive a response"
        );
        // Each is the end of a sentence that begins "Live capture ended: "
        // and is closed with a full stop by the page.
        for text in [
            NO_SCREEN_CAST,
            NO_PORTAL,
            NO_BUS,
            NO_REASON,
            CLOSED_UNSTARTED,
        ] {
            assert!(!text.ends_with('.') && !text.contains("  "), "{text}");
            assert!(text.starts_with(char::is_lowercase), "{text}");
        }
    }

    #[test]
    fn a_portal_that_does_not_answer_is_not_waited_for_longer_than_said() {
        let runtime = portal_runtime().unwrap();
        // An answer that comes is waited for, however short the patience
        // would have been for one that does not.
        let asked = std::time::Instant::now();
        assert!(finished_within(runtime, Duration::from_secs(30), async {}));
        assert!(asked.elapsed() < Duration::from_secs(10));
        // One that never comes: the wait ends when the patience does.
        let patience = Duration::from_millis(200);
        let asked = std::time::Instant::now();
        assert!(!finished_within(runtime, patience, std::future::pending()));
        assert!(asked.elapsed() >= patience);
        assert!(asked.elapsed() < Duration::from_secs(10));
        // The runtime serves the next request all the same.
        assert!(finished_within(runtime, Duration::from_secs(30), async {}));
    }

    #[test]
    fn a_frame_of_ten_bits_a_colour_is_read_as_the_picture_it_is() {
        use spa::param::video::VideoFormat as Format;
        assert_eq!(packing(Format::BGRx), Some(Packing::Bytes { bgr: true }));
        assert_eq!(packing(Format::RGBA), Some(Packing::Bytes { bgr: false }));
        let red_high = Some(Packing::Tens { red_high: true });
        assert_eq!(packing(Format::xRGB_210LE), red_high);
        assert_eq!(packing(Format::ARGB_210LE), red_high);
        let blue_high = Some(Packing::Tens { red_high: false });
        assert_eq!(packing(Format::xBGR_210LE), blue_high);
        assert_eq!(packing(Format::ABGR_210LE), blue_high);
        // Not asked for, and not read as something it is not.
        assert_eq!(packing(Format::RGBx_102LE), None);
        assert_eq!(packing(Format::NV12), None);

        // Two rows of two pixels, each row followed by four bytes that
        // belong to no pixel. Red, green and blue as ten-bit numbers.
        let pixel = |high: u32, middle: u32, low: u32| {
            ((0b11 << 30) | (high << 20) | (middle << 10) | low).to_le_bytes()
        };
        let gap = [0xEE; 4];
        let frame = [
            pixel(1023, 0, 0),
            pixel(0, 1023, 0),
            gap,
            pixel(0, 0, 1023),
            pixel(512, 256, 4),
        ]
        .concat();
        let mut out = vec![7; 3];
        narrow_tens(&frame, 2, 2, 12, true, &mut out);
        // Blue, green, red and a full fourth byte, as `PixelOrder::Bgr`
        // reads them; the gaps are gone.
        assert_eq!(
            out,
            [
                [0, 0, 255, 255],
                [0, 255, 0, 255],
                [255, 0, 0, 255],
                [1, 64, 128, 255]
            ]
            .concat()
        );
        // The same numbers with blue in the high bits.
        narrow_tens(&frame, 2, 2, 12, false, &mut out);
        assert_eq!(
            out,
            [
                [255, 0, 0, 255],
                [0, 255, 0, 255],
                [0, 0, 255, 255],
                [128, 64, 1, 255]
            ]
            .concat()
        );
        // A frame shorter than it says gives the rows it has, and no more.
        narrow_tens(&frame[..14], 2, 2, 12, true, &mut out);
        assert_eq!(out.len(), 8);
    }

    #[test]
    fn a_frame_is_taken_without_what_lies_after_its_rows() {
        // Two rows of three pixels, and the same with `gap` bytes after
        // each row but the last, which no pixel owns: `left` is in them.
        let pixels: Vec<u8> = (1..=24).collect();
        let gapped = |gap: usize, left: u8| {
            let mut rows = pixels[..12].to_vec();
            rows.extend(std::iter::repeat_n(left, gap));
            rows.extend_from_slice(&pixels[12..]);
            rows
        };
        fn lying(rows: &[u8], stride: usize, packing: Packing, turned: Turned) -> Lying<'_> {
            Lying {
                rows,
                width: 3,
                height: 2,
                stride,
                packing,
                turned,
            }
        }
        let bgrx = Packing::Bytes { bgr: true };
        let (mut spare, mut narrowed) = (vec![9; 100], Vec::new());
        // Rows with nothing between them are taken as they are.
        let tight = lying(&pixels, 12, bgrx, Turned::Upright);
        assert_eq!(tight.taken(&mut spare, &mut narrowed), Some((3, 2, true)));
        assert_eq!(spare, pixels);
        // Rows with something between them are taken without it, whatever
        // their distance (one that is no whole number of pixels too) and
        // whatever lies there. They were copied as they came, and what
        // lay there made a frame another picture than the one before.
        let shared = Shared::new();
        for gap in [1, 3, 4, 5, 64] {
            for left in [0xEE, 0x11] {
                let rows = gapped(gap, left);
                let frame = lying(&rows, 12 + gap, bgrx, Turned::Upright);
                assert_eq!(frame.taken(&mut spare, &mut narrowed), Some((3, 2, true)));
                assert_eq!(spare, pixels, "{gap} bytes of {left} after a row");
                shared.frame_arrived(&mut spare, 3, 2, 12, true);
                assert_eq!(lock(&shared.slot).serial, 1);
            }
        }
        // (a colour that differs is a new picture as ever)
        let mut rows = gapped(4, 0xEE);
        rows[16 + 1] ^= 1;
        let frame = lying(&rows, 16, bgrx, Turned::Upright);
        assert!(frame.taken(&mut spare, &mut narrowed).is_some());
        shared.frame_arrived(&mut spare, 3, 2, 12, true);
        assert_eq!(lock(&shared.slot).serial, 2);
        // Red first: said so, the bytes as they are.
        let rows = gapped(4, 0xEE);
        let rgbx = lying(&rows, 16, Packing::Bytes { bgr: false }, Turned::Upright);
        assert_eq!(rgbx.taken(&mut spare, &mut narrowed), Some((3, 2, false)));
        assert_eq!(spare, pixels);
        // Ten bits a colour: as `narrow_tens` writes them, blue first.
        let tens = Packing::Tens { red_high: true };
        let mut narrow = Vec::new();
        narrow_tens(&rows, 3, 2, 16, true, &mut narrow);
        let frame = lying(&rows, 16, tens, Turned::Upright);
        assert_eq!(frame.taken(&mut spare, &mut narrowed), Some((3, 2, true)));
        assert_eq!(spare, narrow);
        assert!(narrowed.is_empty());
        // A frame that lies on its side is taken upright, three rows of
        // two pixels, with or without gaps in the buffer.
        let quarter = Turned::from_note(1).unwrap();
        let source = Source {
            bytes: &pixels,
            width: 3,
            height: 2,
            stride: 12,
            order: PixelOrder::Bgr,
        };
        let mut upright = Vec::new();
        assert_eq!(
            live::turn_upright(source, quarter, &mut upright),
            Some((2, 3))
        );
        for (rows, stride) in [(&pixels, 12), (&rows, 16)] {
            let frame = lying(rows, stride, bgrx, quarter);
            assert_eq!(frame.taken(&mut spare, &mut narrowed), Some((2, 3, true)));
            assert_eq!(spare, upright);
        }
        // Ten bits on their side go through `narrowed`, which the next
        // frame of another kind lets go of.
        let frame = lying(&rows, 16, tens, quarter);
        assert_eq!(frame.taken(&mut spare, &mut narrowed), Some((2, 3, true)));
        assert_eq!((spare.len(), narrowed.len()), (24, 24));
        assert!(tight.taken(&mut spare, &mut narrowed).is_some());
        assert_eq!(narrowed.capacity(), 0);
        // A turned frame that is not all there is none, and nothing is
        // handed on in its place.
        let short = lying(&rows[..20], 16, bgrx, quarter);
        assert_eq!(short.taken(&mut spare, &mut narrowed), None);
    }

    #[test]
    fn the_header_and_the_note_about_the_turn_are_asked_for_as_pipewire_lays_them_out() {
        use spa::pod::{deserialize::PodDeserializer, Value};
        // PipeWire's numbers for the two, and the bytes each of them is: a
        // desktop does not attach a note that is asked for at another size.
        let expected = [(1, 32), (8, 4)];
        let notes = wanted_notes().unwrap();
        assert_eq!(notes.len(), expected.len());
        for (note, (kind, size)) in notes.iter().zip(expected) {
            assert!(spa::pod::Pod::from_bytes(note).is_some());
            let Ok((_, Value::Object(wanted))) = PodDeserializer::deserialize_any_from(note) else {
                panic!("the note {kind} is not an object");
            };
            assert_eq!(wanted.type_, spa::sys::SPA_TYPE_OBJECT_ParamMeta);
            assert_eq!(wanted.id, spa::sys::SPA_PARAM_Meta);
            let values: Vec<_> = wanted.properties.iter().map(|p| &p.value).collect();
            assert_eq!(
                values,
                [&Value::Id(spa::utils::Id(kind)), &Value::Int(size)],
                "note {kind}"
            );
        }
        assert_eq!(std::mem::size_of::<TurnNote>(), 4);
    }

    #[test]
    fn a_buffer_gives_its_picture_only_when_it_says_it_holds_all_of_one() {
        // A picture of 3 x 2 pixels, its rows 16 bytes apart, 8 bytes into
        // a buffer that has 4 more at its end.
        let bytes: Vec<u8> = (0..40).collect();
        let told = Told {
            offset: 8,
            size: 32,
            stride: 16,
            spoiled: false,
        };
        let picture = |told| picture_in(&bytes, told, 3, 2);
        assert_eq!(picture(told), Some((&bytes[8..36], 16)));
        // The last row need not have the gap after it; one byte less than
        // the picture is not a picture.
        assert_eq!(
            picture(Told { size: 28, ..told }),
            Some((&bytes[8..36], 16))
        );
        assert_eq!(picture(Told { size: 27, ..told }), None);
        // More bytes than the buffer has are as many as it has.
        let all = Some((&bytes[8..36], 16));
        assert_eq!(picture(Told { size: 4096, ..told }), all);
        assert_eq!(
            picture(Told {
                size: u32::MAX,
                ..told
            }),
            all
        );
        // Marked as no picture, or without a valid byte: what the memory
        // holds is not looked at.
        assert_eq!(
            picture(Told {
                spoiled: true,
                ..told
            }),
            None
        );
        assert_eq!(picture(Told { size: 0, ..told }), None);
        // A picture that begins where it cannot.
        for offset in [13, 40, 41, u32::MAX] {
            assert_eq!(picture(Told { offset, ..told }), None, "offset {offset}");
        }
        assert_eq!(
            picture(Told { offset: 12, ..told }),
            Some((&bytes[12..40], 16))
        );
        // A distance between rows that cannot be one is taken for none.
        for stride in [0, -16, 11, i32::MIN] {
            assert_eq!(
                picture(Told { stride, ..told }),
                Some((&bytes[8..32], 12)),
                "stride {stride}"
            );
        }
        // Rows further apart than the buffer is long.
        assert_eq!(picture(Told { stride: 24, ..told }), None);
        assert_eq!(
            picture(Told {
                stride: i32::MAX,
                ..told
            }),
            None
        );
        // A size the buffer was not made for, and no size at all.
        assert_eq!(picture_in(&bytes, told, 3, 3), None);
        assert_eq!(picture_in(&bytes, told, 9, 1), None);
        assert_eq!(picture_in(&bytes, told, 0, 2), None);
        assert_eq!(picture_in(&bytes, told, 3, 0), None);
        assert_eq!(picture_in(&bytes, told, u32::MAX, u32::MAX), None);
        assert_eq!(picture_in(&[], told, 1, 1), None);
        assert_eq!(
            picture_in(&bytes, Told { offset: 0, ..told }, 1, 1),
            Some((&bytes[..4], 16))
        );
    }

    #[test]
    fn a_screen_with_a_side_longer_than_16384_is_refused_and_says_so() {
        // Shown: the largest there is, the largest KWin offers, and shapes
        // of one pixel a side.
        for (width, height) in [
            (16384, 16384),
            (16384, 8192),
            (10000, 10000),
            (7680, 4320),
            (16384, 1),
            (1, 16384),
            (1, 1),
        ] {
            assert_eq!(too_large(width, height), None, "{width} x {height}");
        }
        // Refused: one pixel more on either side, however short the other.
        for (width, height) in [
            (16385, 16),
            (16, 16385),
            (16385, 16384),
            (16400, 16400),
            (17000, 17000),
            (20000, 64),
            (u32::MAX, 1),
            (u32::MAX, u32::MAX),
        ] {
            assert!(too_large(width, height).is_some(), "{width} x {height}");
        }
        // What the page says after "Live capture ended: ".
        assert_eq!(
            too_large(17000, 17000).as_deref(),
            Some("the screen cast is 17000 × 17000 pixels, and no more than 16384 a side can be shown")
        );
    }

    #[test]
    fn a_buffer_larger_than_a_frame_needs_is_let_go_of_and_one_that_fits_is_kept() {
        // The room of a frame of 7680 x 4320 before one of 1920 x 1080.
        let mut buffer = Vec::with_capacity(7680 * 4320 * 4);
        buffer.extend_from_slice(&[1, 2, 3]);
        fitted(&mut buffer, 1920 * 1080 * 4);
        assert_eq!((buffer.len(), buffer.capacity()), (0, 0));
        // The next frame takes what it needs, and that is kept from then
        // on, with what is in it.
        buffer.extend_from_slice(&[7; 10_000]);
        let (room, place) = (buffer.capacity(), buffer.as_ptr());
        assert!(room < 20_000);
        fitted(&mut buffer, 10_000);
        fitted(&mut buffer, 16_384 * 16_384 * 4);
        assert_eq!((buffer.capacity(), buffer.as_ptr()), (room, place));
        assert_eq!(buffer, [7; 10_000]);
        // A little too much room is not worth a new buffer for every frame
        // of a screen of a few pixels.
        let mut small = Vec::with_capacity(64);
        fitted(&mut small, 4);
        assert_eq!(small.capacity(), 64);
        let mut empty = Vec::new();
        fitted(&mut empty, 0);
        assert_eq!(empty.capacity(), 0);
    }

    #[test]
    fn a_running_cast_that_is_paused_is_known_to_be_and_since_when() {
        let (then, now) = (Instant::now(), Instant::now() + Duration::from_secs(5));
        // Starting up is not a pause of a running cast.
        assert_eq!(pause_after(None, &Unconnected, &Connecting, now), None);
        assert_eq!(pause_after(None, &Connecting, &Paused, now), None);
        assert_eq!(pause_after(None, &Paused, &Streaming, now), None);
        // Running, then paused: from that moment.
        assert_eq!(pause_after(None, &Streaming, &Paused, now), Some(now));
        // Running again, or over.
        assert_eq!(pause_after(Some(then), &Paused, &Streaming, now), None);
        assert_eq!(pause_after(Some(then), &Paused, &Unconnected, now), None);
        // The page reads it from the capture.
        let fed = Fed::start();
        let capture = fed.capture();
        assert_eq!(capture.paused_for(), None);
        fed.pause(Some(then));
        assert!(capture.paused_for().is_some());
    }

    #[test]
    fn the_dialog_is_known_to_be_over_before_the_first_frame() {
        let fed = Fed::start();
        let capture = fed.capture();
        assert_eq!(capture.state(), CaptureState::Asking);
        fed.chosen();
        assert_eq!(capture.state(), CaptureState::Starting);
        fed.feed(2, 1, &[0; 8]);
        let running = CaptureState::Running {
            width: 2,
            height: 1,
        };
        assert_eq!(capture.state(), running);
        // An answer that comes late changes nothing.
        fed.chosen();
        assert_eq!(capture.state(), running);
    }

    #[test]
    fn a_cast_whose_screen_is_taken_away_is_over_and_one_that_pauses_is_not() {
        let state = |old, new| ending(44, &Seen::State(old, new));
        // What a sway session with xdg-desktop-portal-wlr sends when the
        // shown output is unplugged or switched off, or when the portal's
        // program goes away: the stream pauses, and the link, the port and
        // the node of the screen are removed. Only the node says that it
        // is over.
        assert_eq!(state(Streaming, Paused), None);
        assert_eq!(ending(44, &Seen::Removed(49)), None);
        assert_eq!(ending(44, &Seen::Removed(45)), None);
        assert_eq!(ending(44, &Seen::Removed(44)).as_deref(), Some(ENDED));
        // A change of the screen's mode, and a session manager that is
        // restarted (its link goes and comes back): the cast runs on.
        assert_eq!(state(Paused, Streaming), None);
        // Starting up.
        assert_eq!(state(Unconnected, Connecting), None);
        assert_eq!(state(Connecting, Paused), None);
        // A stream that was never connected has not been ended.
        assert_eq!(state(Connecting, Unconnected), None);
        // PipeWire itself is gone, or says what went wrong.
        assert_eq!(state(Streaming, Unconnected).as_deref(), Some(ENDED));
        assert_eq!(state(Paused, Unconnected).as_deref(), Some(ENDED));
        assert_eq!(
            state(Paused, Error("no more input formats".into())).as_deref(),
            Some("no more input formats")
        );
    }
}

/// What the screen cast of the remembered screen contains, for a few
/// seconds. Run by hand while something is on that screen:
/// `cargo test --release -p any2odyssey capture::probe -- --ignored --nocapture`
#[cfg(test)]
mod probe {
    use super::*;

    #[test]
    #[ignore = "a measurement; needs a remembered screen and a desktop session"]
    fn what_the_cast_sees() {
        let file = any2odyssey_core::paths::capture_token_file();
        let token = std::fs::read_to_string(&file)
            .ok()
            .map(|t| t.trim().to_owned());
        let capture = Capture::start(token, false, || {});
        let frames = capture.frames();
        let seconds: u64 = std::env::var("PROBE_SECONDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(6);
        let start = std::time::Instant::now();
        let (mut seen, mut count) = (0u64, 0u32);
        let mut last_print = 0u64;
        let mut kinds = std::collections::BTreeMap::new();
        while start.elapsed() < Duration::from_secs(seconds) {
            let got = frames.with_newest(seen, Duration::from_millis(500), |slot| {
                let s = slot.source();
                let (w, h) = (s.width as usize, s.height as usize);
                let mut sum = [0u64; 3];
                let mut n = 0u64;
                for y in (h / 10..h * 9 / 10).step_by(16) {
                    for x in (w / 10..w * 9 / 10).step_by(16) {
                        let p = &s.bytes[y * s.stride + x * 4..];
                        sum[0] += p[0] as u64;
                        sum[1] += p[1] as u64;
                        sum[2] += p[2] as u64;
                        n += 1;
                    }
                }
                (
                    slot.serial,
                    w,
                    h,
                    [sum[0] / n.max(1), sum[1] / n.max(1), sum[2] / n.max(1)],
                )
            });
            if let Some((serial, w, h, mean)) = got.filter(|(serial, ..)| *serial != seen) {
                seen = serial;
                count += 1;
                *kinds.entry(mean).or_insert(0u32) += 1;
                let tenth = start.elapsed().as_millis() as u64 / 500;
                if tenth != last_print {
                    last_print = tenth;
                    println!(
                        "{:5.1}s frame {serial} {w}x{h} mean bytes {mean:?}",
                        start.elapsed().as_secs_f32()
                    );
                }
            }
            if frames.ended() {
                break;
            }
        }
        println!(
            "state {:?}, {count} frames in {:?}",
            capture.state(),
            start.elapsed()
        );
        let mut kinds: Vec<_> = kinds.into_iter().collect();
        kinds.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        println!(
            "distinct pictures by mean colour (count): {:?}",
            &kinds[..kinds.len().min(8)]
        );
        if let Some(token) = capture.restore_token() {
            let _ = std::fs::write(&file, token);
        }
    }
}
