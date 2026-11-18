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
}

impl Slot {
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
        }
    }

    /// The desktop has answered with a screen.
    fn chosen(&self) {
        let mut state = lock(&self.state);
        if *state == CaptureState::Asking {
            *state = CaptureState::Starting;
        }
    }
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
    pub fn with_newest<T>(
        &self,
        seen: u64,
        timeout: Duration,
        read: impl FnOnce(&Slot) -> T,
    ) -> Option<T> {
        let slot = lock(&self.0.slot);
        let (slot, _) = self
            .0
            .arrived
            .wait_timeout_while(slot, timeout, |slot| {
                slot.serial == seen && !self.0.stop.load(Ordering::Relaxed)
            })
            .unwrap_or_else(|e| e.into_inner());
        (slot.serial != 0).then(|| read(&slot))
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
fn portal_runtime() -> Result<&'static tokio::runtime::Runtime, String> {
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
    let portal = async {
        let proxy = Screencast::new().await?;
        let session = proxy.create_session(Default::default()).await?;
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
                .await?;
            let response = proxy
                .start(&session, None, Default::default())
                .await?
                .response()?;
            let node = response.streams().first().map(|s| s.pipe_wire_node_id());
            let token = response.restore_token().map(str::to_owned);
            let fd = proxy
                .open_pipe_wire_remote(&session, Default::default())
                .await?;
            ashpd::Result::Ok((node, token, fd))
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
    let (session, node, token, fd) = runtime.block_on(portal).map_err(|e| match e {
        ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled) => {
            "no screen was chosen".to_owned()
        }
        other => format!("the desktop's screen-cast portal refused: {other}"),
    })?;
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

/// The stream that is asked for: raw video in one of the pixel formats that
/// are read here, at whatever rate it states for itself and with sixty
/// frames a second as the most a desktop is to send. No size is wished for.
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
        // Sixty a second is as many as are rendered. A desktop that sends
        // a picture when the screen has changed says how many a second
        // that may be at the most, and is asked to keep to sixty: of a
        // faster screen (the Odyssey runs at 165) every picture was copied
        // and more than half of them never looked at.
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoMaxFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction { num: 60, denom: 1 },
            spa::utils::Fraction { num: 1, denom: 1 },
            spa::utils::Fraction { num: 60, denom: 1 }
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
                return;
            };
            let (width, height) = (data.format.size().width, data.format.size().height);
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
            let Some(bytes) = plane.data() else { return };
            let Some((frame, stride)) = picture_in(bytes, told, width, height) else {
                return;
            };
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
            // What the frame takes in the buffer it is written to: its rows
            // as they come, or without their gaps.
            let wanted = match (packing, turned) {
                (Packing::Bytes { .. }, Turned::Upright) => frame.len(),
                _ => row * tens_high,
            };
            fitted(&mut data.spare, wanted);
            fitted(&mut data.narrowed, wanted);
            let (width, height, stride, bgr) = match (packing, turned) {
                (Packing::Bytes { bgr }, Turned::Upright) => {
                    data.spare.clear();
                    data.spare.extend_from_slice(frame);
                    (width, height, stride, bgr)
                }
                (Packing::Tens { red_high }, Turned::Upright) => {
                    let spare = &mut data.spare;
                    narrow_tens(frame, tens_wide, tens_high, stride, red_high, spare);
                    (width, height, row, true)
                }
                (Packing::Bytes { bgr }, _) => {
                    let upright = live::turn_upright(lying(frame, stride), turned, &mut data.spare);
                    let Some((width, height)) = upright else {
                        return;
                    };
                    (width, height, width as usize * 4, bgr)
                }
                (Packing::Tens { red_high }, _) => {
                    let narrowed = &mut data.narrowed;
                    narrow_tens(frame, tens_wide, tens_high, stride, red_high, narrowed);
                    let upright = live::turn_upright(lying(narrowed, row), turned, &mut data.spare);
                    let Some((width, height)) = upright else {
                        return;
                    };
                    (width, height, width as usize * 4, true)
                }
            };
            if !matches!(packing, Packing::Tens { .. }) || turned == Turned::Upright {
                data.narrowed = Vec::new();
            }
            let first = {
                let mut slot = lock(&data.shared.slot);
                std::mem::swap(&mut slot.bytes, &mut data.spare);
                // What was the slot's has held the frame before this one.
                fitted(&mut data.spare, slot.bytes.len());
                slot.width = width;
                slot.height = height;
                slot.stride = stride;
                slot.bgr = bgr;
                slot.serial += 1;
                slot.serial == 1
            };
            data.shared.arrived.notify_all();
            let mut state = lock(&data.shared.state);
            let running = CaptureState::Running { width, height };
            if first || *state != running {
                *state = running;
            }
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
    // second, and tells the page when the first frame has come.
    let (timer_loop, timer_shared) = (mainloop.clone(), shared.clone());
    let announced = AtomicBool::new(false);
    let wake_once = {
        let shared = shared.clone();
        move || {
            if !announced.load(Ordering::Relaxed)
                && matches!(*lock(&shared.state), CaptureState::Running { .. })
            {
                announced.store(true, Ordering::Relaxed);
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
        drop(slot);
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
    fn sixty_a_second_is_the_most_asked_for_and_a_streams_own_rate_is_taken() {
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
        assert_eq!(range(Key::VideoMaxFramerate), (60, 1, 60));
        // The stream's own rate: 0 is "as the screen changes", and a fixed
        // rate of 144 or 240 was refused while 60 was the most here.
        let (_, least, most) = range(Key::VideoFramerate);
        assert_eq!(least, 0);
        assert!(most >= 240);
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
