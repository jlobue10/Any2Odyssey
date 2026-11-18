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

use any2odyssey_core::live::{PixelOrder, Source};
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
            let cursor = match proxy.available_cursor_modes().await {
                Ok(modes) if pointer && modes.contains(CursorMode::Embedded) => {
                    CursorMode::Embedded
                }
                _ => CursorMode::Hidden,
            };
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

struct StreamData {
    format: spa::param::video::VideoInfoRaw,
    shared: Arc<Shared>,
    /// Where a frame is copied as it arrives; it then changes places with
    /// the slot's, so the two buffers serve every frame and the reader is
    /// kept waiting for the exchange only, not for the copy.
    spare: Vec<u8>,
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
    };
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
        .param_changed(|_, data, id, param| {
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
            let datas = buffer.datas_mut();
            let Some(plane) = datas.first_mut() else {
                return;
            };
            let chunk = plane.chunk();
            let (offset, size) = (chunk.offset() as usize, chunk.size() as usize);
            let row = width as usize * 4;
            let stride = usize::try_from(chunk.stride())
                .ok()
                .filter(|s| *s >= row)
                .unwrap_or(row);
            let Some(bytes) = plane.data() else { return };
            let needed = match (height as usize).checked_sub(1) {
                Some(rows) if row > 0 => stride * rows + row,
                _ => return,
            };
            let end = offset.saturating_add(if size == 0 {
                needed
            } else {
                size.min(bytes.len())
            });
            let Some(frame) = bytes.get(offset..end.min(bytes.len())) else {
                return;
            };
            if frame.len() < needed {
                return;
            }
            data.spare.clear();
            let (stride, bgr) = match packing {
                Packing::Bytes { bgr } => {
                    data.spare.extend_from_slice(&frame[..needed]);
                    (stride, bgr)
                }
                Packing::Tens { red_high } => {
                    let (width, height) = (width as usize, height as usize);
                    narrow_tens(
                        &frame[..needed],
                        width,
                        height,
                        stride,
                        red_high,
                        &mut data.spare,
                    );
                    (row, true)
                }
            };
            let first = {
                let mut slot = lock(&data.shared.slot);
                std::mem::swap(&mut slot.bytes, &mut data.spare);
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

    let format = spa::pod::object!(
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
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle {
                width: 1920,
                height: 1080
            },
            spa::utils::Rectangle {
                width: 1,
                height: 1
            },
            spa::utils::Rectangle {
                width: 16384,
                height: 16384
            }
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction { num: 60, denom: 1 },
            spa::utils::Fraction { num: 0, denom: 1 },
            // Sixty a second is as many as are rendered; a faster screen
            // would only have its extra frames copied and dropped.
            spa::utils::Fraction { num: 60, denom: 1 }
        ),
    );
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
