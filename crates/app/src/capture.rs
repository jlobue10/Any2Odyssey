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
use std::time::Duration;

use any2odyssey_core::live::{PixelOrder, Source};
use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};
use ashpd::desktop::PersistMode;
use pipewire as pw;
use pw::spa;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureState {
    /// The desktop's dialog is open, or the stream is being set up.
    Asking,
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
        let shared = Arc::new(Shared {
            slot: Mutex::new(Slot::default()),
            arrived: Condvar::new(),
            state: Mutex::new(CaptureState::Asking),
            token: Mutex::new(None),
            stop: AtomicBool::new(false),
        });
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
    // Whatever comes of the stream, the session is closed after it.
    let streamed = match node {
        None => Err("the portal returned no stream".to_owned()),
        Some(_) if shared.stop.load(Ordering::Relaxed) => Ok(()),
        Some(node) => stream(shared, node, fd, wake),
    };
    let _ = runtime.block_on(session.close());
    streamed
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
    let (state_error, state_loop, state_shared) = (error.clone(), mainloop.clone(), shared.clone());
    let data = StreamData {
        format: Default::default(),
        shared: shared.clone(),
        spare: Vec::new(),
    };
    let _listener = stream
        .add_local_listener_with_user_data(data)
        .state_changed(move |_, _, old, new| {
            let over = match &new {
                pw::stream::StreamState::Error(reason) => Some(reason.clone()),
                // The source went away (the screen was unplugged, or the
                // desktop ended the cast) after it had been running.
                pw::stream::StreamState::Unconnected
                    if matches!(
                        old,
                        pw::stream::StreamState::Streaming | pw::stream::StreamState::Paused
                    ) =>
                {
                    Some("the screen cast was ended by the desktop".to_owned())
                }
                _ => None,
            };
            if let Some(reason) = over {
                if !state_shared.stop.load(Ordering::Relaxed) {
                    *lock(&state_error) = Some(reason);
                }
                state_loop.quit();
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
            let bgr = match data.format.format() {
                spa::param::video::VideoFormat::BGRx | spa::param::video::VideoFormat::BGRA => true,
                spa::param::video::VideoFormat::RGBx | spa::param::video::VideoFormat::RGBA => {
                    false
                }
                _ => return,
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
            data.spare.extend_from_slice(&frame[..needed]);
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
        Fed(Arc::new(Shared {
            slot: Mutex::new(Slot::default()),
            arrived: Condvar::new(),
            state: Mutex::new(CaptureState::Asking),
            token: Mutex::new(None),
            stop: AtomicBool::new(false),
        }))
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
