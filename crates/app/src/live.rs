//! The live desktop in 3D (Linux): a captured screen, given depth by the
//! model, rendered as two views and handed to the weaver output, frame
//! after frame.
//!
//! Three threads share the work so that none waits for another:
//!
//! - the capture thread (`capture`) keeps the newest frame of the screen;
//! - the depth thread runs the model on the newest frame it is given, as
//!   often as the model allows, and keeps a steadied depth map;
//! - the render thread takes every new frame, shrinks it to the size of one
//!   view, shifts its pixels by the latest depth map into a left and a right
//!   view, sharpens them if asked and writes the pair to the weaver.
//!
//! The picture therefore moves at the screen's own rate even when the model
//! is slower; the depth of something that has just moved follows a moment
//! later. While the screen does not change, the model is not asked again
//! and no picture is made, once the last frame's depth has been eased in.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use any2odyssey_core::dibr::Params;
use any2odyssey_core::live::{self, Depth, DepthSmoother, Placement};
use any2odyssey_core::slider::{DEPTH, POPOUT};
use image::RgbaImage;

use crate::capture::{fitted, Capture, CaptureState, Frames};
use crate::depth::RawDepth;
use crate::weaver::{self, FrameSink};

/// The time from one picture to the next that is kept to over any stretch
/// of pictures ([`next_turn`]): 66 a second at the most.
///
/// It has to be shorter than the time a desktop leaves between the frames
/// it sends, or the frames come faster than they are taken: each then waits
/// longer than the one before, until one is left out and the wait begins
/// again from nothing. A desktop is asked for `capture::MOST_A_SECOND` (62)
/// frames at the most, and KDE Plasma spaces its frames by whole
/// milliseconds, 16 for 62 a second, which is 62.5; 62 to 64 a second were
/// counted arriving there. With 16 ms here the frames of a screen that
/// sent 62.5 or 64 a second were 13 ms old in the middle when their
/// pictures reached the output, where those of a screen of 60 were 5, and
/// 24 to 69 of them were left out in 40 seconds.
const FRAME_TIME: Duration = Duration::from_micros(15_000);

/// When the turn of the render loop may begin that follows one which was
/// due at `due` and began at `began`: [`FRAME_TIME`] after that one was
/// due, so that no more pictures are made than that allows, however fast
/// the frames come. A turn that began late is forgiven up to one
/// `FRAME_TIME`: the turn after it may begin that much sooner after it.
///
/// The time used to be counted from each turn's own beginning. A turn that
/// began late (the machine was busy for a moment, and the frame was there
/// before the loop was) then kept the next one 16 ms away from itself, and
/// with it from the frame that came 16.7 ms after the late one: that frame
/// waited almost as long, and the lateness of one turn was worked off at
/// 0.7 ms a frame. Measured with a screen of 60 frames a second and the
/// program held for 60 ms once a second: the frames were back to their
/// usual age from the eleventh and the seventeenth picture after a hold in
/// two runs, and from the third and the fourth with this.
fn next_turn(due: Instant, began: Instant) -> Instant {
    let forgiven = began.checked_sub(FRAME_TIME).unwrap_or(began);
    due.max(forgiven) + FRAME_TIME
}

/// What the user can change while the picture runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Controls {
    /// Depth strength and pop-out, as slider positions (`core::slider`).
    pub depth_pos: i32,
    pub pop_pos: i32,
    /// Sharpening, 0 (off) to 100.
    pub sharpness: i32,
    /// How steady depth is kept from frame to frame, 0 (follows the model
    /// at once) to 95 (slow to change).
    pub steadiness: i32,
    /// Lines per view: 720, 1080, 1440 or 2160.
    pub lines: i32,
    /// The long side of the picture the model sees (`depth::DETAIL`).
    pub detail: i32,
    pub swap: bool,
}

impl Default for Controls {
    fn default() -> Controls {
        let params = Params::default();
        Controls {
            depth_pos: DEPTH.position(params.strength),
            pop_pos: POPOUT.position(params.popout),
            sharpness: 0,
            steadiness: 60,
            lines: 1080,
            detail: crate::depth::LONG_SIDE as i32,
            swap: false,
        }
    }
}

/// How it is going, for the page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    /// Pictures written to the weaver per second, and depth maps per second,
    /// over the last second in which there were any.
    pub picture_rate: f32,
    pub depth_rate: f32,
    /// The screen has not changed for a while and its picture is up to
    /// date, so none is being made: the rates are those of earlier work.
    pub still: bool,
    /// Why depth is missing, when the model cannot run.
    pub depth_error: Option<String>,
    /// Why no picture can be written, when that fails.
    pub output_error: Option<String>,
}

impl Stats {
    /// How the pictures are coming, for the status line (it goes after
    /// "a ... screen, "): the rates while there is work, and that there is
    /// none while the screen is still. The rates are not told then, being
    /// those of the last work, however long ago and however short; only a
    /// model that has not answered yet, or cannot, is still mentioned.
    pub fn describe(&self) -> String {
        let depth = match &self.depth_error {
            Some(e) => Some(format!("no depth: {e}")),
            None if self.still && self.depth_rate > 0.0 => None,
            None if self.depth_rate >= 0.95 => {
                Some(format!("depth {:.0} times a second", self.depth_rate))
            }
            // A slow model (on the processor, at a fine detail) would read
            // as "0 times a second".
            None if self.depth_rate > 0.0 => Some(format!(
                "depth once every {:.1} seconds",
                1.0 / self.depth_rate
            )),
            None => Some("depth starting".into()),
        };
        let pictures = if self.still {
            "not changing, so nothing is redrawn".to_owned()
        } else {
            format!("{:.0} pictures a second", self.picture_rate)
        };
        match depth {
            Some(depth) => format!("{pictures}, {depth}"),
            None => pictures,
        }
    }
}

/// The controls that change the picture by themselves (the others work
/// through the depth map).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Look {
    depth_pos: i32,
    pop_pos: i32,
    sharpness: i32,
    lines: i32,
    swap: bool,
}

/// What a turn of the render loop finds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Found {
    /// The newest frame of the screen.
    serial: u64,
    look: Look,
    detail: u32,
    /// Which depth map there is (`DepthState::version`), the frame and the
    /// detail the model's newest answer was made from, and whether that
    /// answer has been eased in all the way.
    depth: u64,
    mapped: Option<(u64, u32)>,
    settled: bool,
    /// The model has no frame waiting or in work.
    model_free: bool,
    /// Which output the pictures go to; 0 while there is none.
    output: u64,
}

/// What the picture written last was made from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Shown {
    serial: u64,
    look: Look,
    depth: u64,
    output: u64,
}

/// What a turn of the render loop has to do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Turn {
    /// Shrink the screen's frame to a view: it is new, or the view's size is.
    scale: bool,
    /// Give the frame to the model.
    submit: bool,
    /// Ease the frame's own depth map in by one more step.
    settle: bool,
    /// Make a picture and write it.
    render: bool,
}

/// Decide a turn. A screen that does not change sends no frames, and then
/// only what would change the picture is done: the model is asked about a
/// frame once (and again only if that failed, or for another detail), its
/// answer is eased in step by step as it was while the model was asked over
/// and over, and a picture is made when the frame, the depth map, a control
/// or the output is another than for the picture before. With none of
/// that, the turn is idle.
fn plan(shown: Option<&Shown>, found: &Found) -> Turn {
    let new_frame = shown.is_none_or(|shown| shown.serial != found.serial);
    let same = |part: fn(&Shown, &Found) -> bool| shown.is_some_and(|shown| part(shown, found));
    let mapped = found.mapped == Some((found.serial, found.detail));
    // One step a turn, and none before the step before has been shown.
    let settle = !new_frame && mapped && !found.settled && same(|s, f| s.depth == f.depth);
    Turn {
        scale: new_frame || !same(|s, f| s.look.lines == f.look.lines),
        submit: found.model_free && !mapped,
        settle,
        render: new_frame
            || settle
            || !same(|s, f| s.look == f.look && s.depth == f.depth && s.output == f.output),
    }
}

/// A frame on its way to the model, already at the model's size. The three
/// buffers (the render thread's, this one and the depth thread's) change
/// places, so none is made anew for a frame.
#[derive(Default)]
struct ForModel {
    /// A frame is waiting here or the model is at work on one: the render
    /// thread makes no other meanwhile.
    busy: bool,
    /// The frame has not been taken yet.
    waiting: bool,
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    /// Which frame of the screen it is, and the detail it was made for.
    serial: u64,
    detail: u32,
}

impl ForModel {
    /// Make `frame` a buffer of `bytes` bytes for the next frame to be
    /// shrunk into, with no more room than that.
    ///
    /// The three buffers kept the room of the finest depth detail chosen
    /// since the output was started, as the pictures' buffers once did
    /// (`capture::fitted`): 14.6 MB each after *Finest* on a 3840 x 2160
    /// screen, where *Standard* needs 2.4 MB, so 36 MB too many. Here and
    /// where the buffers change places each is let go of when it is larger
    /// than the frame on its way: all three after the first frame of a
    /// coarser detail.
    fn room(frame: &mut Vec<u8>, bytes: usize) {
        fitted(frame, bytes);
        frame.resize(bytes, 0);
    }

    /// The render thread puts `frame` here for the depth thread and gets
    /// the buffer that was here for its next one.
    fn give(&mut self, frame: &mut Vec<u8>, size: (u32, u32), serial: u64, detail: u32) {
        std::mem::swap(&mut self.pixels, frame);
        fitted(frame, self.pixels.len());
        (self.width, self.height) = size;
        (self.serial, self.detail) = (serial, detail);
        (self.busy, self.waiting) = (true, true);
    }

    /// The depth thread takes the frame that is waiting into `pixels` and
    /// leaves the buffer of its last one: the frame's size, which frame it
    /// is and the detail it was made for. The frame's place stays taken
    /// (`busy`) until the thread says it is done.
    fn take(&mut self, pixels: &mut Vec<u8>) -> (u32, u32, u64, u32) {
        self.waiting = false;
        std::mem::swap(&mut self.pixels, pixels);
        fitted(&mut self.pixels, pixels.len());
        (self.width, self.height, self.serial, self.detail)
    }
}

/// The depth map the pictures are rendered with.
#[derive(Default)]
struct DepthState {
    smoother: DepthSmoother,
    /// Raised whenever the map changes, so that a picture made with an
    /// earlier one is known to be out of date.
    version: u64,
    /// The model's newest answer, and the frame and detail it was for. It
    /// is kept as the model gave it, as the smoother is to have it: which
    /// range a map is brought to 0..=1 with is the smoother's to say
    /// (`depth::RawDepth`).
    latest: Option<(u64, u32, RawDepth)>,
    /// That answer has been eased in all the way: the map will not change
    /// until the model answers again.
    settled: bool,
}

impl DepthState {
    /// Throw the map away (the model has failed): pictures are flat until
    /// it answers again. Without a map there is nothing to throw away and
    /// nothing changes, so a model that goes on failing does not have the
    /// same flat picture made again at every attempt.
    fn forget(&mut self) {
        if self.latest.is_some() || !self.smoother.values().is_empty() {
            *self = DepthState {
                version: self.version + 1,
                ..DepthState::default()
            };
        }
    }
}

struct Shared {
    stop: AtomicBool,
    depth_pos: AtomicI32,
    pop_pos: AtomicI32,
    sharpness: AtomicI32,
    steadiness: AtomicI32,
    lines: AtomicI32,
    detail: AtomicI32,
    swap: AtomicBool,
    /// Where the pictures go, once the output is up, and how many outputs
    /// there have been. The capture comes first: the desktop may have to
    /// ask which screen, and its dialog must not end up underneath the
    /// output's window.
    sink: Mutex<(u64, Option<FrameSink>)>,
    /// The frame the depth thread should look at next.
    for_model: Mutex<ForModel>,
    model_wanted: Condvar,
    depth: Mutex<DepthState>,
    stats: Mutex<Stats>,
}

impl Shared {
    fn new() -> Shared {
        Shared {
            stop: AtomicBool::new(false),
            depth_pos: AtomicI32::new(0),
            pop_pos: AtomicI32::new(0),
            sharpness: AtomicI32::new(0),
            steadiness: AtomicI32::new(0),
            lines: AtomicI32::new(1080),
            detail: AtomicI32::new(crate::depth::LONG_SIDE as i32),
            swap: AtomicBool::new(false),
            sink: Mutex::new((0, None)),
            for_model: Mutex::new(ForModel::default()),
            model_wanted: Condvar::new(),
            depth: Mutex::new(DepthState::default()),
            stats: Mutex::new(Stats::default()),
        }
    }

    /// What there is to make a picture of frame `serial` from, and where it
    /// would go.
    fn find(&self, serial: u64) -> (Found, Option<FrameSink>) {
        let (output, sink) = lock(&self.sink).clone();
        // The depth thread takes its frame from `for_model` and says when
        // it is done with it; a frame is made for it only then. This is
        // looked at before the depth map: the thread publishes its answer
        // first and frees its place after, so a model seen free here has
        // its answer in what is read next. Read the other way round, the
        // answer could come between the two and its frame be sent again.
        let model_free = !lock(&self.for_model).busy;
        let (depth, mapped, settled) = {
            let depth = lock(&self.depth);
            let mapped = depth.latest.as_ref().map(|(s, d, _)| (*s, *d));
            (depth.version, mapped, depth.settled)
        };
        let found = Found {
            serial,
            look: Look {
                depth_pos: self.depth_pos.load(Ordering::Relaxed),
                pop_pos: self.pop_pos.load(Ordering::Relaxed),
                sharpness: self.sharpness.load(Ordering::Relaxed).clamp(0, 100),
                lines: self.lines.load(Ordering::Relaxed),
                swap: self.swap.load(Ordering::Relaxed),
            },
            detail: self.detail.load(Ordering::Relaxed).max(0) as u32,
            depth,
            mapped,
            settled,
            model_free,
            output,
        };
        (found, sink)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct Live {
    capture: Capture,
    shared: Arc<Shared>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl Live {
    /// Ask for a screen and start working on its pictures, with the pointer
    /// drawn into them if `pointer`; they are shown once [`Live::show_on`]
    /// has said where. `wake` is called when the capture's state changes.
    pub fn start(
        restore_token: Option<String>,
        pointer: bool,
        controls: Controls,
        wake: impl Fn() + Send + 'static,
    ) -> Live {
        let capture = Capture::start(restore_token, pointer, wake);
        let mut live = Live {
            capture,
            shared: Arc::new(Shared::new()),
            threads: Vec::new(),
        };
        live.set(controls);
        let (frames, shared) = (live.capture.frames(), live.shared.clone());
        let render = std::thread::Builder::new()
            .name("live-render".into())
            .spawn(move || render_loop(&frames, &shared));
        let shared = live.shared.clone();
        let depth = std::thread::Builder::new()
            .name("live-depth".into())
            .spawn(move || depth_loop(&shared));
        for thread in [render, depth] {
            match thread {
                Ok(thread) => live.threads.push(thread),
                Err(e) => {
                    lock(&live.shared.stats).output_error =
                        Some(format!("could not start a worker thread: {e}"))
                }
            }
        }
        live
    }

    /// Give the pictures somewhere to go (the weaver's picture file).
    pub fn show_on(&mut self, sink: FrameSink) {
        let mut output = lock(&self.shared.sink);
        *output = (output.0 + 1, Some(sink));
    }

    pub fn has_output(&self) -> bool {
        lock(&self.shared.sink).1.is_some()
    }

    pub fn set(&mut self, controls: Controls) {
        let s = &self.shared;
        s.depth_pos.store(controls.depth_pos, Ordering::Relaxed);
        s.pop_pos.store(controls.pop_pos, Ordering::Relaxed);
        s.sharpness.store(controls.sharpness, Ordering::Relaxed);
        s.steadiness.store(controls.steadiness, Ordering::Relaxed);
        s.lines.store(controls.lines, Ordering::Relaxed);
        s.detail.store(controls.detail, Ordering::Relaxed);
        s.swap.store(controls.swap, Ordering::Relaxed);
    }

    pub fn capture_state(&self) -> CaptureState {
        self.capture.state()
    }

    pub fn restore_token(&self) -> Option<String> {
        self.capture.restore_token()
    }

    /// For how long the desktop has had the cast paused
    /// (`Capture::paused_for`).
    pub fn paused_for(&self) -> Option<Duration> {
        self.capture.paused_for()
    }

    /// Buffers the desktop has sent that held no picture
    /// (`Capture::unread`).
    pub fn unread(&self) -> Option<(u32, &'static str)> {
        self.capture.unread()
    }

    /// A live screen around `capture` with nothing at work on it, for tests
    /// of what the page says.
    #[cfg(test)]
    pub(crate) fn idle(capture: Capture) -> Live {
        Live {
            capture,
            shared: Arc::new(Shared::new()),
            threads: Vec::new(),
        }
    }

    #[cfg(test)]
    fn frames(&self) -> Frames {
        self.capture.frames()
    }

    pub fn stats(&self) -> Stats {
        lock(&self.shared.stats).clone()
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.model_wanted.notify_all();
        for thread in self.threads.drain(..) {
            // The render thread wakes within its frame wait. The depth
            // thread may be inside the model for a while on a slow machine;
            // it is left to finish by itself rather than waited for here.
            if thread.thread().name() == Some("live-render") {
                let _ = thread.join();
            }
        }
        // The depth thread keeps what is shared for as long as it runs, and
        // the output's picture file is held open (and kept from being
        // removed) by whoever has the output: it is let go of here. Left
        // with the thread, the file would stay until the model had finished,
        // and for good when the program ends before that.
        lock(&self.shared.sink).1 = None;
    }
}

/// The size of one view for a screen of `screen` pixels at `lines` lines:
/// 16:9 like the panel, and where the screen's picture goes inside it
/// (x, y, width, height).
fn layout(screen: (u32, u32), lines: i32) -> ((u32, u32), (u32, u32, u32, u32)) {
    let lines = lines.clamp(360, weaver::PANEL.1 as i32) as u32;
    let view = ((lines * 16 / 9) & !1, lines);
    (view, weaver::place_view(screen, 1.0, view))
}

/// How long a rate is measured over before it is told.
const RATE_TIME: Duration = Duration::from_secs(1);

/// How long there has to be nothing to do before the screen is said to be
/// still: longer than a blinking cursor or a clock leaves between frames.
const STILL_TIME: Duration = Duration::from_secs(2);

fn render_loop(frames: &Frames, shared: &Shared) {
    let mut seen = 0u64;
    let (mut picture, mut pair, mut canvas, mut small) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut shown: Option<Shown> = None;
    let mut blank = None;
    let (mut counted, mut since) = (0u32, Instant::now());
    let mut failing = false;
    // Since when there has been no picture to make, and whether the page
    // has been told that the screen is still.
    let (mut resting, mut still) = (None::<Instant>, false);
    // When the next turn may begin.
    let mut due = Instant::now();
    while !shared.stop.load(Ordering::Relaxed) && !frames.ended() {
        // A little more than sixty pictures a second at most: a faster
        // screen (the Odyssey runs at 165) would double the work for depth
        // the eye cannot tell apart, on the processor and the GPU. A
        // desktop is asked to send no more than that, and keeps to it; this
        // is for a stream that does not (`next_turn`).
        if let Some(early) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(early);
        }
        due = next_turn(due, Instant::now());
        // A still screen sends no frames. Its last one is looked at again a
        // few times a second all the same: the depth map of a picture that
        // has just appeared arrives after it, and the controls must take
        // effect without waiting for the screen to change. Most of those
        // turns find nothing to do (`plan`).
        let taken = frames.with_newest(seen, Duration::from_millis(150), |slot| {
            let (found, sink) = shared.find(slot.serial);
            let turn = plan(shown.as_ref(), &found);
            let (view, place) = layout((slot.width, slot.height), found.look.lines);
            let (_, _, w, h) = place;
            if turn.scale {
                // The buffers of the pictures kept the room of the most
                // lines chosen since the output was started: 71 MB after
                // 2160 lines and back to 1080.
                fitted(&mut picture, w as usize * h as usize * 4);
                picture.resize(w as usize * h as usize * 4, 0);
                if !live::shrink_to_rgba(slot.source(), &mut picture, w, h) {
                    return None;
                }
            }
            let for_model = turn.submit.then(|| {
                let model = crate::depth::model_size(slot.width, slot.height, found.detail);
                // A screen much larger than the model's picture goes to the
                // depth thread at twice that size, every pixel counted, and
                // is brought down the rest of the way there
                // (`live::model_input_size`).
                let (mw, mh) = live::model_input_size((slot.width, slot.height), model);
                ForModel::room(&mut small, mw as usize * mh as usize * 4);
                live::shrink_to_rgba(slot.source(), &mut small, mw, mh).then_some((mw, mh))
            });
            Some((found, sink, turn, view, place, for_model.flatten()))
        });
        let Some(Some((found, sink, turn, view, (x, y, w, h), for_model))) = taken else {
            continue;
        };
        seen = found.serial;
        if let Some(size) = for_model {
            lock(&shared.for_model).give(&mut small, size, found.serial, found.detail);
            shared.model_wanted.notify_one();
        }
        // The depth map's version as this turn leaves it.
        let mut depth_now = found.depth;
        if turn.settle {
            let steadiness = shared.steadiness.load(Ordering::Relaxed).clamp(0, 95);
            let mut depth = lock(&shared.depth);
            let depth = &mut *depth;
            // Still the answer the turn was planned with?
            if let Some((_, _, map)) = depth
                .latest
                .as_ref()
                .filter(|(serial, detail, _)| (*serial, *detail) == (found.serial, found.detail))
            {
                depth.settled = depth.smoother.settle(
                    map.width(),
                    map.height(),
                    map.values(),
                    steadiness as f32 / 100.0,
                );
                depth.version += 1;
                depth_now = depth.version;
            }
        }
        if !turn.render {
            // Idle. The pictures written before the wait are told of once;
            // the rate then stays as it was while there was work.
            if counted > 0 && since.elapsed() >= RATE_TIME {
                lock(&shared.stats).picture_rate = counted as f32 / since.elapsed().as_secs_f32();
                (counted, since) = (0, Instant::now());
            }
            if !still && resting.get_or_insert_with(Instant::now).elapsed() >= STILL_TIME {
                lock(&shared.stats).still = true;
                still = true;
            }
            continue;
        }
        resting = None;
        if still {
            lock(&shared.stats).still = false;
            still = false;
        }
        let Some(sink) = sink else {
            // Nowhere to show it yet. It is made when there is: the output
            // is then another than the one noted here.
            shown = Some(Shown {
                serial: found.serial,
                look: found.look,
                depth: depth_now,
                output: found.output,
            });
            continue;
        };

        let look = found.look;
        let params = Params {
            strength: DEPTH.value(look.depth_pos),
            popout: POPOUT.value(look.pop_pos),
        };
        // Each view goes into its half of the picture, the bottom row
        // first as the weaver's file holds it.
        let place = Placement {
            view,
            x,
            y,
            swap: look.swap,
        };
        // Black wherever the screen's shape leaves room: made afresh when
        // the picture's size or place changes, or old pixels would stay.
        let placed = (view, x, y, w, h);
        if canvas.len() != place.bytes() || blank != Some(placed) {
            blank = Some(placed);
            fitted(&mut canvas, place.bytes());
            canvas.clear();
            canvas.resize(place.bytes(), 0);
            for alpha in canvas.as_chunks_mut::<4>().0 {
                alpha[3] = 255;
            }
        }
        // Without sharpening the views are rendered where they go. With
        // it they are rendered side by side first, and sharpened from
        // there into their places.
        if look.sharpness > 0 {
            fitted(&mut pair, picture.len() * 2);
            pair.resize(picture.len() * 2, 0);
        } else if pair.capacity() > 0 {
            pair = Vec::new();
        }
        let depth_shown = {
            let depth = lock(&shared.depth);
            let (dw, dh) = depth.smoother.size();
            let map = (!depth.smoother.values().is_empty()).then(|| Depth {
                values: depth.smoother.values(),
                width: dw,
                height: dh,
            });
            if look.sharpness > 0 {
                live::render_pair(&picture, w, h, map, params, &mut pair);
            } else {
                live::render_placed(&picture, w, h, map, params, place, &mut canvas);
            }
            depth.version
        };
        if look.sharpness > 0 {
            let amount = look.sharpness as f32 / 100.0;
            live::place_pair(&pair, w, h, amount, place, &mut canvas);
        }
        let written = sink.show_bottom_up(view.0 * 2, view.1, &canvas);
        // A picture that could not be written is made again next turn.
        shown = written.is_ok().then_some(Shown {
            serial: found.serial,
            look,
            depth: depth_shown,
            output: found.output,
        });

        // The rate is that of the pictures written, from the first of
        // them on: a still screen's wait before it is not part of it.
        if counted == 0 && since.elapsed() >= RATE_TIME {
            since = Instant::now();
        }
        counted += 1;
        let elapsed = since.elapsed();
        if elapsed >= RATE_TIME || written.is_err() || failing {
            let mut stats = lock(&shared.stats);
            if elapsed >= RATE_TIME {
                stats.picture_rate = counted as f32 / elapsed.as_secs_f32().max(0.001);
                (counted, since) = (0, Instant::now());
            }
            failing = written.is_err();
            stats.output_error = written.err().map(|e| e.to_string());
        }
    }
}

fn depth_loop(shared: &Shared) {
    // How many live screens have a depth thread at this moment: a stopped
    // one's thread may still be inside the model when the next has begun.
    static RUNNING: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    RUNNING.fetch_add(1, Ordering::SeqCst);
    // The model's answer as it gave it, not the map a still image gets
    // (`depth::RawDepth`).
    depth_loop_with(shared, |frame, detail| {
        let stopped = || shared.stop.load(Ordering::Relaxed);
        crate::depth::estimate_raw_unless_cancelled(
            frame,
            detail,
            crate::depth::Purpose::Live,
            stopped,
        )
    });
    // The last live screen has ended: the model goes, and with it what it
    // holds of the GPU's memory or the processor's. The next start loads
    // it again, in about a second, while the output comes up. Whether this
    // was the last is asked when the model is free to be let go of, not
    // here: another live screen may begin before that
    // (`depth::unload_if`).
    last_one_lets_go(&RUNNING, |unused| crate::depth::unload_if(unused));
}

/// A depth thread that ends counts itself out of `running`, and
/// `unload_if` is given the question whether none is left, to ask when the
/// model is free to be let go of. Answered here and now, a live screen that
/// begins while `unload_if` waits for a map being made has its model let go
/// of behind its first map (`depth::unload_if`).
fn last_one_lets_go(
    running: &std::sync::atomic::AtomicUsize,
    unload_if: impl FnOnce(&dyn Fn() -> bool),
) {
    running.fetch_sub(1, Ordering::SeqCst);
    unload_if(&|| running.load(Ordering::SeqCst) == 0);
}

/// How bright a pixel of a black bar may be at the most, in each colour: a
/// video's black is 16, and a player and the shrinking add a little.
const BAR_BLACK: u8 = 24;

/// The black bars a film is shown between on a screen of another shape:
/// how many rows at the top and at the bottom of `frame`, and how many
/// columns at its left and at its right, are black from end to end (top,
/// bottom, left, right).
///
/// The model does not know a bar for nothing. It reads the bars of a film
/// as a frame in front of the picture: over 100 seconds of a film of
/// 1280 x 534 on a screen of 1280 x 720 (audit round 24, Tears of Steel) it
/// had the bars at three quarters of the way from far to near in the mean,
/// and the nearest point of the whole map lay in a bar in 480 of 600 maps.
/// The range from far to near is the map's, so the film itself was left
/// three quarters of it in the middle, two fifths or less in every tenth
/// map and 27 % at the least. With the bars named, the depth thread puts
/// them where the farthest point of the picture is
/// (`depth::RawDepth::far_at_the_edges`), and the range is the picture's.
///
/// A bar has to be a hundredth of the side or more (a border of a pixel or
/// two is none), and together the two of a side at most three quarters of
/// it (an upright video on a wide screen has two thirds): what is dark for
/// more than that is a dark picture, and a picture that is black all over
/// has no bars. Rows are looked at first, columns between them: a frame
/// that is no film between bars is told by its first row and column.
fn black_bars(frame: &RgbaImage) -> (u32, u32, u32, u32) {
    let (w, h) = (frame.width() as usize, frame.height() as usize);
    let pixels = frame.as_raw().as_chunks::<4>().0;
    let black = |p: &[u8; 4]| p[..3].iter().all(|&colour| colour <= BAR_BLACK);
    let bars = |first: usize, last: usize, side: usize| {
        let bar = |bar: usize| if bar * 100 >= side { bar } else { 0 };
        if (first + last) * 4 > side * 3 {
            (0, 0)
        } else {
            (bar(first), bar(last))
        }
    };
    let row = |y: usize| pixels[y * w..(y + 1) * w].iter().all(black);
    let top = (0..h).take_while(|&y| row(y)).count();
    if top == h {
        return (0, 0, 0, 0);
    }
    let bottom = (0..h).rev().take_while(|&y| row(y)).count();
    let (top, bottom) = bars(top, bottom, h);
    let column = |x: usize| (top..h - bottom).all(|y| black(&pixels[y * w + x]));
    let left = (0..w).take_while(|&x| column(x)).count();
    let right = (0..w).rev().take_while(|&x| column(x)).count();
    let (left, right) = bars(left, right, w);
    (top as u32, bottom as u32, left as u32, right as u32)
}

/// The rows (or columns) of a depth map of `map` rows that a bar of `bar`
/// rows of the model's frame of `frame` rows takes: every row the bar
/// reaches into, and one more, where the model's answer is on its way from
/// what it says of the bar to what it says of the picture.
fn bar_in_map(bar: u32, frame: u32, map: u32) -> u32 {
    match bar {
        0 => 0,
        bar => (u64::from(bar) * u64::from(map)).div_ceil(u64::from(frame.max(1))) as u32 + 1,
    }
}

/// [`depth_loop`] with the model as `estimate`, which is given a frame and
/// the long side the model is to see it at.
fn depth_loop_with(
    shared: &Shared,
    estimate: impl Fn(&RgbaImage, u32) -> Result<Option<RawDepth>, String>,
) {
    let (mut counted, mut since) = (0u32, Instant::now());
    let mut failing = false;
    let mut pixels = Vec::new();
    loop {
        let (width, height, serial, detail) = {
            let mut slot = lock(&shared.for_model);
            loop {
                if shared.stop.load(Ordering::Relaxed) {
                    return;
                }
                // Nothing to do (a still screen): the maps made before the
                // wait are told of once; the rate then stays as it was
                // while there was work.
                if !slot.waiting && counted > 0 && since.elapsed() >= RATE_TIME {
                    lock(&shared.stats).depth_rate = counted as f32 / since.elapsed().as_secs_f32();
                    (counted, since) = (0, Instant::now());
                }
                // The frame's place stays taken while the model runs, so
                // the render thread does not make another one meanwhile.
                if slot.waiting {
                    break slot.take(&mut pixels);
                }
                slot = shared
                    .model_wanted
                    .wait_timeout(slot, Duration::from_millis(250))
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
        };
        // The rate is that of the maps made, from when the first of them
        // was begun: a still screen's wait before it is not part of it.
        if counted == 0 && since.elapsed() >= RATE_TIME {
            since = Instant::now();
        }
        // The frame was made for this depth detail: at the size the model
        // is to see it at, or at twice that size for a large screen, which
        // the estimator halves (`live::model_input_size`). So the model is
        // asked for the detail and not, as it was, for the frame's own long
        // side: a frame of twice the size then went through the model as it
        // came. Every screen of 1036 pixels or more across was estimated
        // at twice the chosen detail, in four to five times the time on a
        // processor, and Fine, Finer and Finest were one and the same on a
        // 3840 x 2160 screen.
        let estimated = match RgbaImage::from_raw(width, height, std::mem::take(&mut pixels)) {
            Some(frame) => {
                let mut estimated = estimate(&frame, detail);
                // What the model says of black bars is not the picture's
                // depth (`black_bars`).
                if let Ok(Some(map)) = &mut estimated {
                    let (top, bottom, left, right) = black_bars(&frame);
                    let rows = |bar| bar_in_map(bar, height, map.height());
                    let columns = |bar| bar_in_map(bar, width, map.width());
                    map.far_at_the_edges(rows(top), rows(bottom), columns(left), columns(right));
                }
                pixels = frame.into_raw();
                estimated
            }
            None => Err("the frame for the depth model is incomplete".into()),
        };
        if shared.stop.load(Ordering::Relaxed) {
            return;
        }
        match estimated {
            Ok(None) => return,
            Ok(Some(map)) => {
                let steadiness = shared.steadiness.load(Ordering::Relaxed).clamp(0, 95);
                {
                    let mut depth = lock(&shared.depth);
                    if depth.smoother.update(
                        map.width(),
                        map.height(),
                        map.values(),
                        steadiness as f32 / 100.0,
                    ) {
                        depth.version += 1;
                        depth.settled = false;
                        depth.latest = Some((serial, detail, map));
                    }
                }
                // Free for the next frame only now that the answer is
                // there to be seen: the render thread would otherwise send
                // the same frame once more.
                lock(&shared.for_model).busy = false;
                counted += 1;
                let elapsed = since.elapsed();
                if elapsed >= RATE_TIME || failing {
                    let mut stats = lock(&shared.stats);
                    stats.depth_error = None;
                    failing = false;
                    if elapsed >= RATE_TIME {
                        stats.depth_rate = counted as f32 / elapsed.as_secs_f32();
                        (counted, since) = (0, Instant::now());
                    }
                }
            }
            Err(e) => {
                // New frames must not use the previous scene's depth while
                // inference is failing. Recovery starts a fresh smoother too.
                lock(&shared.depth).forget();
                lock(&shared.for_model).busy = false;
                {
                    let mut stats = lock(&shared.stats);
                    stats.depth_rate = 0.0;
                    stats.depth_error = Some(e);
                }
                (failing, counted) = (true, 0);
                // The picture carries on flat. Try again now and then: the
                // model may be installed while this runs.
                for _ in 0..20 {
                    if shared.stop.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_view_is_sixteen_by_nine_and_the_screen_is_fitted_into_it() {
        // The Odyssey itself: the whole view.
        assert_eq!(
            layout((3840, 2160), 1080),
            ((1920, 1080), (0, 0, 1920, 1080))
        );
        // A 32:9 screen is letterboxed.
        assert_eq!(
            layout((5120, 1440), 1080),
            ((1920, 1080), (0, 270, 1920, 540))
        );
        // A 4:3 screen is pillarboxed.
        assert_eq!(layout((1600, 1200), 720), ((1280, 720), (160, 0, 960, 720)));
        // Odd requests are brought into range.
        assert_eq!(layout((1920, 1080), 99999).0, (3840, 2160));
        assert_eq!(layout((1920, 1080), 0).0, (640, 360));
    }

    #[test]
    fn failed_depth_estimation_returns_new_frames_to_flat_output() {
        const CHILD: &str = "ANY2ODYSSEY_TEST_DEPTH_FAILURE";
        if std::env::var_os(CHILD).is_none() {
            // Isolate the estimator/runtime caches and environment from the
            // other tests. No model, runtime, capture portal or GPU is needed.
            let dir = tempfile::tempdir().unwrap();
            let model = dir.path().join("model.onnx");
            std::fs::write(&model, b"test model placeholder").unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "live::tests::failed_depth_estimation_returns_new_frames_to_flat_output",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env(crate::depth::MODEL_ENV, &model)
                .env("ORT_DYLIB_PATH", dir.path().join("missing-runtime.so"))
                .output()
                .unwrap();
            let said = format!(
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(output.status.success(), "{said}");
            // The child is asked for this test by its name. Under another
            // name (the test's or its module's, after a renaming) it runs
            // no test at all and ends well, and that was all that was
            // looked at: the test would have stayed green without running.
            assert!(
                said.contains("running 1 test\n")
                    && said.contains("test result: ok. 1 passed; 0 failed;"),
                "the test did not run in its own process:\n{said}"
            );
            return;
        }
        let mut previous = DepthSmoother::default();
        assert!(previous.update(2, 1, &[0.0, 1.0], 0.0));
        let shared = Arc::new(Shared {
            steadiness: AtomicI32::new(90),
            for_model: Mutex::new(ForModel {
                busy: true,
                waiting: true,
                pixels: vec![0; 28 * 14 * 4],
                width: 28,
                height: 14,
                serial: 7,
                detail: 518,
            }),
            depth: Mutex::new(DepthState {
                smoother: previous,
                version: 3,
                latest: Some((6, 518, RawDepth::new(2, 1, vec![0.0, 1.0]).unwrap())),
                settled: true,
            }),
            stats: Mutex::new(Stats {
                depth_rate: 30.0,
                ..Stats::default()
            }),
            ..Shared::new()
        });
        let worker_state = shared.clone();
        let worker = std::thread::spawn(move || depth_loop(&worker_state));
        let deadline = Instant::now() + Duration::from_secs(5);
        while lock(&shared.stats).depth_error.is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        shared.stop.store(true, Ordering::Relaxed);
        shared.model_wanted.notify_all();
        worker.join().unwrap();
        let stats = lock(&shared.stats);
        assert!(stats
            .depth_error
            .as_deref()
            .is_some_and(|e| e.contains("ONNX Runtime could not be loaded")));
        assert_eq!(stats.depth_rate, 0.0);
        drop(stats);

        let picture = RgbaImage::from_fn(128, 1, |x, _| {
            image::Rgba([x as u8, 255 - x as u8, 70, 255])
        });
        let mut rendered = vec![0; picture.len() * 2];
        let mut flat = rendered.clone();
        // The frame can be given to the model again, and the picture made
        // with the old depth is known to be out of date.
        assert!(!lock(&shared.for_model).busy);
        let mut state = lock(&shared.depth);
        assert!(state.version > 3 && state.latest.is_none() && !state.settled);
        let depth = &mut state.smoother;
        let (width, height) = depth.size();
        let map = (!depth.values().is_empty()).then(|| Depth {
            values: depth.values(),
            width,
            height,
        });
        live::render_pair(&picture, 128, 1, map, Params::default(), &mut rendered);
        live::render_pair(&picture, 128, 1, None, Params::default(), &mut flat);
        assert!(
            rendered == flat,
            "an estimator failure kept warping new pixels with the old scene's depth"
        );
        assert!(depth.values().is_empty());
        // Recovery must begin with the new map, without blending the old scene.
        assert!(depth.update(2, 1, &[1.0, 0.0], 0.9));
        assert_eq!(depth.values(), &[1.0, 0.0]);
    }

    /// A frame of `w` x `h` pixels, each as `pixel(x, y)` says (red, green
    /// and blue alike).
    fn grey_frame(w: u32, h: u32, pixel: impl Fn(u32, u32) -> u8) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            image::Rgba([pixel(x, y), pixel(x, y), pixel(x, y), 255])
        })
    }

    #[test]
    fn the_black_bars_around_a_picture_are_found_and_a_dark_picture_is_not_bars() {
        // A film of 1280 x 534 on a screen of 1280 x 720, as the model's
        // frame of 1036 x 588: 76 rows above and below, a video's black.
        let film = grey_frame(1036, 588, |x, y| match y {
            76..512 => 40 + ((x + y) % 200) as u8,
            _ => 16,
        });
        assert_eq!(black_bars(&film), (76, 76, 0, 0));
        // An upright video on a wide screen: bars at the sides, of two
        // widths, and a little noise in them.
        let upright = grey_frame(400, 200, |x, y| match x {
            0..150 => ((x * 7 + y * 3) % 20) as u8,
            150..290 => 30 + (y % 100) as u8,
            _ => ((x + y) % 24) as u8,
        });
        assert_eq!(black_bars(&upright), (0, 0, 150, 110));
        // Bars all around.
        let small = grey_frame(200, 100, |x, y| {
            if (20..180).contains(&x) && (10..95).contains(&y) {
                200
            } else {
                0
            }
        });
        assert_eq!(black_bars(&small), (10, 5, 20, 20));
        // One pixel in a bar's row that is not black, and the row is the
        // picture's; so is everything below it.
        let mut marked = film.clone();
        marked.put_pixel(1000, 70, image::Rgba([25, 0, 0, 255]));
        assert_eq!(black_bars(&marked), (70, 76, 0, 0));
        // No bars: a picture to its edges, a border of one row (less than
        // a hundredth of the side), a picture that is black all over, and
        // a dark picture with something in one corner: more than three
        // quarters of it are black, which is a picture and no bars.
        assert_eq!(
            black_bars(&grey_frame(64, 36, |x, y| 30 + (x + y) as u8)),
            (0, 0, 0, 0)
        );
        let bordered = grey_frame(300, 300, |_, y| if y == 0 || y == 299 { 0 } else { 90 });
        assert_eq!(black_bars(&bordered), (0, 0, 0, 0));
        assert_eq!(black_bars(&grey_frame(64, 36, |_, _| 3)), (0, 0, 0, 0));
        let night = grey_frame(100, 100, |x, y| if x > 90 && y > 90 { 255 } else { 0 });
        assert_eq!(black_bars(&night), (0, 0, 0, 0));
        // A bar of three rows of a hundred is one; in a map of half the
        // rows it takes the two rows it reaches into and one more.
        let thin = grey_frame(100, 100, |_, y| if y < 3 { 0 } else { 90 });
        assert_eq!(black_bars(&thin), (3, 0, 0, 0));
        assert_eq!(bar_in_map(3, 100, 50), 3);
        assert_eq!(bar_in_map(76, 588, 294), 39);
        assert_eq!(bar_in_map(0, 588, 294), 0);
    }

    #[test]
    fn what_the_model_says_of_black_bars_does_not_take_the_pictures_depth() {
        // The depth thread as the program runs it. The frame: a picture
        // between bars of two rows above and below. The model, as the
        // real one does, has the bars nearer than anything in the picture
        // (9, where the picture goes from 1 to 4), in a map of half the
        // frame's size.
        let frame = grey_frame(16, 12, |x, y| {
            if (2..10).contains(&y) {
                60 + x as u8
            } else {
                16
            }
        });
        let said: Vec<f32> = (0..6u32)
            .flat_map(|y| {
                (0..8u32).map(move |x| match y {
                    0 | 5 => 9.0,
                    1 | 4 => 5.0 + x as f32 * 0.1,
                    2 => 1.0 + x as f32 * 0.25,
                    _ => 4.0 - x as f32 * 0.25,
                })
            })
            .collect();
        let shared = Arc::new(Shared::new());
        let (state, answer) = (shared.clone(), said.clone());
        let depth = std::thread::spawn(move || {
            depth_loop_with(&state, |_, _| {
                Ok(Some(RawDepth::new(8, 6, answer.clone()).unwrap()))
            })
        });
        {
            let mut slot = lock(&shared.for_model);
            slot.pixels = frame.into_raw();
            (slot.width, slot.height) = (16, 12);
            (slot.serial, slot.detail) = (1, 518);
            (slot.busy, slot.waiting) = (true, true);
        }
        shared.model_wanted.notify_one();
        eventually("the model's answer", || !lock(&shared.for_model).busy);
        {
            let depth = lock(&shared.depth);
            let values = depth.smoother.values();
            // The bars' rows of the map, and the row beside each where the
            // model's answer is between bar and picture, are as far as the
            // farthest point of the picture; the picture has the whole
            // range from far to near. (Given the map as the model said
            // it, the picture was left three eighths of the range.)
            for y in [0, 1, 4, 5] {
                assert!(
                    values[y * 8..(y + 1) * 8].iter().all(|&v| v == 0.0),
                    "row {y}: {values:?}"
                );
            }
            let close = |a: f32, b: f32| (a - b).abs() < 1e-6;
            assert!(
                values[16] == 0.0 && close(values[23], 1.75 / 3.0),
                "{values:?}"
            );
            assert!(
                close(values[24], 1.0) && close(values[31], 1.25 / 3.0),
                "{values:?}"
            );
            // The answer that is kept for a still screen's easing is the
            // same one.
            let (_, _, latest) = depth.latest.as_ref().unwrap();
            assert_eq!(latest.values()[..16], [1.0; 16]);
            assert_eq!(latest.values()[16..32], said[16..32]);
        }
        shared.stop.store(true, Ordering::Relaxed);
        shared.model_wanted.notify_all();
        depth.join().unwrap();
    }

    #[test]
    fn a_live_screen_that_ends_asks_whether_it_was_the_last_once_the_model_is_free() {
        use std::sync::atomic::AtomicUsize;
        // One live screen, and it ends. Its letting-go of the model has to
        // wait for a map that is being made, and another live screen
        // begins meanwhile: asked then, the model is wanted. (The thread
        // counted itself out and said "the last" before it waited, and the
        // new screen's model went behind its first map. `depth::unload_if`
        // has a test of its own; this is the thread's side of it, which
        // could be put back as it was with every test passing.)
        let running = AtomicUsize::new(1);
        let mut unused = None;
        last_one_lets_go(&running, |nobody_left| {
            running.fetch_add(1, Ordering::SeqCst);
            unused = Some(nobody_left());
        });
        assert_eq!(unused, Some(false), "let go of under a new live screen");
        assert_eq!(running.load(Ordering::SeqCst), 1);
        // That one ends with none after it: the model goes.
        let mut unused = None;
        last_one_lets_go(&running, |nobody_left| unused = Some(nobody_left()));
        assert_eq!(unused, Some(true));
        assert_eq!(running.load(Ordering::SeqCst), 0);
        // Of two at once the first to end leaves the model to the other.
        let running = AtomicUsize::new(2);
        let mut unused = None;
        last_one_lets_go(&running, |nobody_left| unused = Some(nobody_left()));
        assert_eq!(unused, Some(false));
    }

    #[test]
    fn the_models_own_numbers_reach_the_smoother_not_a_map_brought_to_0_1_first() {
        // The depth thread as the program runs it, the model played by the
        // test: of four points the nearest comes a little nearer (4, then
        // 5, in the model's numbers), and then much nearer (40).
        let maps = [
            [1.0f32, 2.0, 3.0, 4.0],
            [1.0, 2.0, 3.0, 5.0],
            [1.0, 2.0, 3.0, 40.0],
        ];
        let shared = Arc::new(Shared::new());
        shared.steadiness.store(60, Ordering::Relaxed);
        let state = shared.clone();
        let depth = std::thread::spawn(move || {
            depth_loop_with(&state, |frame, _| {
                // (the frame says which of the maps is its own)
                let raw = maps[frame.as_raw()[0] as usize].to_vec();
                Ok(Some(RawDepth::new(4, 1, raw).unwrap()))
            })
        });
        // What the smoother makes of the same numbers, given them itself.
        let mut given = DepthSmoother::default();
        for (frame, map) in maps.iter().enumerate() {
            let serial = frame as u64 + 1;
            {
                let mut slot = lock(&shared.for_model);
                slot.pixels = vec![frame as u8; 4 * 4];
                (slot.width, slot.height) = (4, 1);
                (slot.serial, slot.detail) = (serial, 518);
                (slot.busy, slot.waiting) = (true, true);
            }
            shared.model_wanted.notify_one();
            eventually("the model's answer", || !lock(&shared.for_model).busy);
            let depth = lock(&shared.depth);
            // The answer is kept as it came, for the still screen's easing.
            let (answered, _, latest) = depth.latest.as_ref().unwrap();
            assert_eq!((*answered, latest.values()), (serial, &map[..]));
            // And what is shown is what the smoother makes of those
            // numbers at the steadiness chosen: which range each map is
            // measured against is the smoother's to say
            // (`live::DepthSmoother`), from the numbers as they came.
            assert!(given.update(4, 1, map, 0.6));
            let values = depth.smoother.values();
            assert_eq!(values, given.values(), "map {frame}");
            if frame == 0 {
                assert_eq!(values, [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0]);
            }
        }
        shared.stop.store(true, Ordering::Relaxed);
        shared.model_wanted.notify_all();
        depth.join().unwrap();
    }

    #[test]
    fn the_frames_for_the_model_keep_no_more_room_than_the_detail_chosen_needs() {
        // A 3840 x 2160 screen: its frames go to the model at twice the
        // model's size (`live::model_input_size`).
        let bytes = |detail: u32| {
            let model = crate::depth::model_size(3840, 2160, detail);
            let (w, h) = live::model_input_size((3840, 2160), model);
            ((w, h), w as usize * h as usize * 4)
        };
        let (finest, standard) = (bytes(1274), bytes(518));
        assert_eq!((finest.1, standard.1), (14_554_176, 2_436_672));
        // The render thread's buffer, the one between the threads and the
        // depth thread's, as the two loops pass a frame along.
        let (mut small, mut slot, mut pixels) = (Vec::new(), ForModel::default(), Vec::new());
        let mut serial = 0;
        let mut frame =
            |detail, (size, bytes), small: &mut Vec<u8>, slot: &mut ForModel, pixels: &mut _| {
                serial += 1;
                ForModel::room(small, bytes);
                assert_eq!(small.len(), bytes);
                slot.give(small, size, serial, detail);
                assert!(slot.busy && slot.waiting);
                assert_eq!(slot.take(pixels), (size.0, size.1, serial, detail));
                assert!(slot.busy && !slot.waiting);
                assert_eq!(pixels.len(), bytes);
            };
        for _ in 0..4 {
            frame(1274, finest, &mut small, &mut slot, &mut pixels);
        }
        let held = |small: &Vec<u8>, slot: &ForModel, pixels: &Vec<u8>| {
            small.capacity() + slot.pixels.capacity() + pixels.capacity()
        };
        // While the detail stays, the three buffers change places and none
        // is made anew.
        assert_eq!(held(&small, &slot, &pixels), finest.1 * 3);
        let places = [small.as_ptr(), slot.pixels.as_ptr(), pixels.as_ptr()];
        for _ in 0..3 {
            frame(1274, finest, &mut small, &mut slot, &mut pixels);
        }
        assert_eq!(
            [small.as_ptr(), slot.pixels.as_ptr(), pixels.as_ptr()],
            places
        );
        // Standard is chosen: with its first frame the room of the finest
        // is gone. (All three kept it: 43.7 MB held where 7.3 MB do.)
        frame(518, standard, &mut small, &mut slot, &mut pixels);
        assert_eq!(held(&small, &slot, &pixels), standard.1);
        for _ in 0..3 {
            frame(518, standard, &mut small, &mut slot, &mut pixels);
        }
        assert_eq!(held(&small, &slot, &pixels), standard.1 * 3);
        // And back to a finer one: each buffer grows as its turn comes.
        for _ in 0..3 {
            frame(1022, bytes(1022), &mut small, &mut slot, &mut pixels);
        }
        assert_eq!(held(&small, &slot, &pixels), bytes(1022).1 * 3);
    }

    #[test]
    fn a_large_screen_is_estimated_at_the_chosen_depth_detail() {
        let capture = crate::capture::Fed::start();
        let shared = Arc::new(Shared::new());
        shared.lines.store(360, Ordering::Relaxed);
        // What the model is asked: the frame's size and the long side it
        // is to see the frame at. It answers as the estimator does, with
        // a map of the size it worked at.
        let asked = Arc::new(Mutex::new(Vec::new()));
        let (frames, render_state) = (capture.frames(), shared.clone());
        let render = std::thread::spawn(move || render_loop(&frames, &render_state));
        let (depth_state, seen) = (shared.clone(), asked.clone());
        let depth = std::thread::spawn(move || {
            depth_loop_with(&depth_state, |frame, long_side| {
                lock(&seen).push((frame.dimensions(), long_side));
                let (w, h) = crate::depth::model_size(frame.width(), frame.height(), long_side);
                let values = (0..w * h).map(|i| i as f32).collect();
                Ok(Some(RawDepth::new(w, h, values).unwrap()))
            })
        });
        for ((sw, sh), detail, frame) in [
            // At least twice the model's picture each way: handed over at
            // twice its size, and the map is of the chosen detail all the
            // same. (It was of twice the detail: 1036 x 588 for Standard.)
            ((2560u32, 1440u32), 518u32, (1036u32, 588u32)),
            ((2560, 1440), 770, (1540, 868)),
            ((2560, 1440), 1274, (2548, 1428)),
            // A 32:9 screen, and one that stands upright.
            ((5120, 1440), 518, (1036, 280)),
            ((1440, 2560), 518, (588, 1036)),
            // Smaller than twice the model's picture: at the model's size.
            ((1920, 1080), 1022, (1022, 574)),
            ((1000, 562), 518, (518, 294)),
        ] {
            shared.detail.store(detail as i32, Ordering::Relaxed);
            let screen = vec![90u8; sw as usize * sh as usize * 4];
            capture.feed(sw, sh, &screen);
            let wanted = crate::depth::model_size(sw, sh, detail);
            eventually("the map of this screen at this detail", || {
                lock(&shared.depth)
                    .latest
                    .as_ref()
                    .is_some_and(|(_, made_for, map)| {
                        *made_for == detail && (map.width(), map.height()) == wanted
                    })
            });
            assert_eq!(
                lock(&asked).last().copied(),
                Some((frame, detail)),
                "a {sw} x {sh} screen at detail {detail}"
            );
            // Not more than twice the model's picture goes through the
            // thread's hands, and the long side of the map is the detail.
            assert_eq!(wanted.0.max(wanted.1), detail);
        }
        shared.stop.store(true, Ordering::Relaxed);
        shared.model_wanted.notify_all();
        drop(capture);
        render.join().unwrap();
        depth.join().unwrap();
    }

    fn a_look() -> Look {
        Look {
            depth_pos: 10,
            pop_pos: 0,
            sharpness: 0,
            lines: 1080,
            swap: false,
        }
    }

    /// A still screen whose picture is up to date in every respect: the
    /// model has answered for the frame and the answer has been eased in.
    fn at_rest() -> (Shown, Found) {
        let shown = Shown {
            serial: 5,
            look: a_look(),
            depth: 9,
            output: 1,
        };
        let found = Found {
            serial: 5,
            look: a_look(),
            detail: 518,
            depth: 9,
            mapped: Some((5, 518)),
            settled: true,
            model_free: true,
            output: 1,
        };
        (shown, found)
    }

    const IDLE: Turn = Turn {
        scale: false,
        submit: false,
        settle: false,
        render: false,
    };

    #[test]
    fn a_still_screen_whose_picture_is_up_to_date_is_left_alone() {
        let (shown, found) = at_rest();
        assert_eq!(plan(Some(&shown), &found), IDLE);
        // Whatever the model is doing.
        let busy = Found {
            model_free: false,
            ..found
        };
        assert_eq!(plan(Some(&shown), &busy), IDLE);
    }

    #[test]
    fn every_new_frame_is_shrunk_shown_and_given_to_a_model_that_is_free() {
        let (shown, found) = at_rest();
        let first = Found {
            serial: 1,
            depth: 0,
            mapped: None,
            settled: false,
            ..found
        };
        let all = Turn {
            scale: true,
            submit: true,
            settle: false,
            render: true,
        };
        assert_eq!(plan(None, &first), all);
        // The next frame, with the map of the one before still in use.
        let next = Found { serial: 6, ..found };
        assert_eq!(plan(Some(&shown), &next), all);
        let busy = Found {
            model_free: false,
            ..next
        };
        assert_eq!(
            plan(Some(&shown), &busy),
            Turn {
                submit: false,
                ..all
            }
        );
    }

    #[test]
    fn a_still_frame_goes_to_the_model_once_and_its_map_is_eased_in_step_by_step() {
        let (shown, found) = at_rest();
        // The frame was shown while the model was at work on the one
        // before. Now the model is free: this frame goes to it, and no
        // picture is made (nothing about it has changed).
        let unmapped = Found {
            mapped: Some((4, 518)),
            settled: false,
            ..found
        };
        let only_submit = Turn {
            submit: true,
            ..IDLE
        };
        assert_eq!(plan(Some(&shown), &unmapped), only_submit);
        // While the model works on it, nothing.
        let in_work = Found {
            model_free: false,
            ..unmapped
        };
        assert_eq!(plan(Some(&shown), &in_work), IDLE);
        // Its answer is there: shown as it is, and not asked for again.
        let answered = Found {
            depth: 10,
            mapped: Some((5, 518)),
            settled: false,
            ..found
        };
        let only_render = Turn {
            render: true,
            ..IDLE
        };
        assert_eq!(plan(Some(&shown), &answered), only_render);
        // That shown, the map is eased in a step a turn, each step shown...
        let step = Turn {
            settle: true,
            render: true,
            ..IDLE
        };
        let shown = Shown { depth: 10, ..shown };
        assert_eq!(plan(Some(&shown), &answered), step);
        let (shown, eased) = (
            Shown { depth: 11, ..shown },
            Found {
                depth: 11,
                ..answered
            },
        );
        assert_eq!(plan(Some(&shown), &eased), step);
        // ...until it is there.
        let settled = Found {
            settled: true,
            ..eased
        };
        assert_eq!(plan(Some(&shown), &settled), IDLE);
        // A map of this frame made for another detail is not this one's.
        let other_detail = Found {
            mapped: Some((5, 770)),
            settled: false,
            ..eased
        };
        assert_eq!(plan(Some(&shown), &other_detail), only_submit);
    }

    #[test]
    fn a_failed_estimate_is_shown_flat_and_asked_for_again_when_the_model_is_free() {
        let (shown, found) = at_rest();
        // The depth thread threw the map away: another version, no answer.
        let failed = Found {
            depth: 10,
            mapped: None,
            settled: false,
            ..found
        };
        assert_eq!(
            plan(Some(&shown), &failed),
            Turn {
                submit: true,
                render: true,
                ..IDLE
            }
        );
        // The flat picture is up; the frame waits for the next attempt.
        let shown = Shown { depth: 10, ..shown };
        let waiting = Found {
            model_free: false,
            ..failed
        };
        assert_eq!(plan(Some(&shown), &waiting), IDLE);
    }

    #[test]
    fn the_controls_take_effect_on_a_still_screen() {
        let (shown, found) = at_rest();
        let only_render = Turn {
            render: true,
            ..IDLE
        };
        for look in [
            Look {
                depth_pos: 11,
                ..a_look()
            },
            Look {
                pop_pos: -3,
                ..a_look()
            },
            Look {
                sharpness: 40,
                ..a_look()
            },
            Look {
                swap: true,
                ..a_look()
            },
        ] {
            let changed = Found { look, ..found };
            assert_eq!(plan(Some(&shown), &changed), only_render, "{look:?}");
        }
        // Other lines are another size of view: the frame is shrunk anew.
        let lines = Found {
            look: Look {
                lines: 1440,
                ..a_look()
            },
            ..found
        };
        assert_eq!(
            plan(Some(&shown), &lines),
            Turn {
                scale: true,
                render: true,
                ..IDLE
            }
        );
        // Another detail is another question to the model; the picture
        // changes when the answer is there.
        let detail = Found {
            detail: 770,
            ..found
        };
        assert_eq!(
            plan(Some(&shown), &detail),
            Turn {
                submit: true,
                ..IDLE
            }
        );
    }

    #[test]
    fn a_new_output_and_a_picture_that_was_not_written_get_the_picture_again() {
        let (shown, found) = at_rest();
        let output = Found { output: 2, ..found };
        assert_eq!(
            plan(Some(&shown), &output),
            Turn {
                render: true,
                ..IDLE
            }
        );
        // After a failed write nothing counts as shown. No easing step is
        // taken for a picture nobody saw the step before of.
        let unsettled = Found {
            settled: false,
            ..found
        };
        for found in [found, unsettled] {
            assert_eq!(
                plan(None, &found),
                Turn {
                    scale: true,
                    render: true,
                    ..IDLE
                }
            );
        }
    }

    /// Every state `plan` tells apart: two frames, two looks (the second
    /// with other lines), two details, two depth maps, two outputs.
    fn every_state() -> Vec<(Option<Shown>, Found)> {
        let looks = [
            a_look(),
            Look {
                lines: 1440,
                ..a_look()
            },
        ];
        let mut shown = vec![None];
        let mut found = Vec::new();
        for serial in [5, 6] {
            for look in looks {
                for depth in [9, 10] {
                    for output in [0, 1] {
                        shown.push(Some(Shown {
                            serial,
                            look,
                            depth,
                            output,
                        }));
                        for detail in [518, 770] {
                            let mapped = [(5, 518), (6, 518), (5, 770), (6, 770)].map(Some);
                            for mapped in mapped.into_iter().chain([None]) {
                                for (settled, model_free) in
                                    [(false, false), (false, true), (true, false), (true, true)]
                                {
                                    found.push(Found {
                                        serial,
                                        look,
                                        detail,
                                        depth,
                                        mapped,
                                        settled,
                                        model_free,
                                        output,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        let mut states = Vec::new();
        for shown in &shown {
            for found in &found {
                states.push((*shown, *found));
            }
        }
        states
    }

    fn up_to_date(shown: Option<&Shown>, found: &Found) -> bool {
        shown
            == Some(&Shown {
                serial: found.serial,
                look: found.look,
                depth: found.depth,
                output: found.output,
            })
    }

    #[test]
    fn a_turn_is_idle_only_when_nothing_is_out_of_date_and_nothing_is_left_to_ask_or_ease() {
        let states = every_state();
        assert_eq!(states.len(), 17 * 640);
        let mut idle = 0;
        for (shown, found) in states {
            let turn = plan(shown.as_ref(), &found);
            let state = format!("{shown:?} {found:?}");
            let current = up_to_date(shown.as_ref(), &found);
            let mapped = found.mapped == Some((found.serial, found.detail));
            // A picture is made exactly when the last one is out of date in
            // any respect, or the depth map takes a step.
            assert_eq!(turn.render, !current || turn.settle, "{state}");
            // A step is taken only from a picture that showed the step
            // before, and never past the end.
            let eased_from = shown
                .is_some_and(|shown| (shown.serial, shown.depth) == (found.serial, found.depth));
            assert_eq!(
                turn.settle,
                eased_from && mapped && !found.settled,
                "{state}"
            );
            // The frame is shrunk whenever it or the view's size is new,
            // and what is shrunk is shown.
            assert!(!turn.scale || turn.render, "{state}");
            let same_view = shown.is_some_and(|shown| {
                (shown.serial, shown.look.lines) == (found.serial, found.look.lines)
            });
            assert_eq!(turn.scale, !same_view, "{state}");
            // The model gets a frame only when it is free and has not
            // answered for this frame at this detail.
            assert_eq!(turn.submit, found.model_free && !mapped, "{state}");
            if turn == IDLE {
                idle += 1;
                // Nothing stale is left alone: the picture is the one that
                // would be made now; the frame's map is there and has
                // arrived, or the model is at work and its answer (or its
                // failure) will be another depth map than the one shown.
                assert!(current, "{state}");
                let nothing_left = if mapped {
                    found.settled
                } else {
                    !found.model_free
                };
                assert!(nothing_left, "{state}");
            }
        }
        assert!(idle > 0);
    }

    /// A screen that does not change, the model and the loop's turns, with
    /// the model and the output played by the test.
    struct Still {
        shown: Option<Shown>,
        found: Found,
        /// The frame and detail the model is at work on, and the turns it
        /// still takes.
        in_work: Option<((u64, u32), u32)>,
        /// The steps until the newest answer has been eased in.
        to_ease: u32,
        submitted: Vec<(u64, u32)>,
    }

    impl Still {
        /// One turn as `render_loop` takes it, then what the depth thread
        /// does meanwhile. `eases` is how many steps an answer takes to
        /// arrive, `written` whether the picture could be written.
        fn turn(&mut self, model_takes: u32, eases: u32, written: bool) -> Turn {
            let turn = plan(self.shown.as_ref(), &self.found);
            if turn.submit {
                assert!(self.in_work.is_none(), "a frame for a model at work");
                let frame = (self.found.serial, self.found.detail);
                self.in_work = Some((frame, model_takes));
                self.found.model_free = false;
                self.submitted.push(frame);
            }
            if turn.settle {
                self.to_ease -= 1;
                self.found.settled = self.to_ease == 0;
                self.found.depth += 1;
            }
            if turn.render {
                self.shown = written.then_some(Shown {
                    serial: self.found.serial,
                    look: self.found.look,
                    depth: self.found.depth,
                    output: self.found.output,
                });
            }
            match self.in_work {
                Some((frame, 0)) => {
                    self.in_work = None;
                    self.found.depth += 1;
                    self.found.mapped = Some(frame);
                    self.found.settled = false;
                    self.to_ease = eases;
                    self.found.model_free = true;
                }
                Some((frame, turns)) => self.in_work = Some((frame, turns - 1)),
                None => {}
            }
            turn
        }
    }

    #[test]
    fn a_still_screen_comes_to_rest_from_every_state_with_its_frame_asked_about_once() {
        let mut rested = 0;
        for (shown, found) in every_state() {
            let frame = (found.serial, found.detail);
            // A model that is not free is at work on this frame or on one
            // the screen has replaced since.
            let at_work = if found.model_free {
                vec![None]
            } else {
                vec![Some(frame), Some((4, found.detail))]
            };
            for at_work in at_work {
                for (model_takes, eases, fails) in
                    [(0, 1, false), (3, 1, true), (0, 4, true), (3, 4, false)]
                {
                    let state = format!("{shown:?} {found:?} {at_work:?} {model_takes} {eases}");
                    let mapped = found.mapped == Some(frame);
                    let mut still = Still {
                        shown,
                        found,
                        in_work: at_work.map(|frame| (frame, model_takes)),
                        to_ease: if mapped && !found.settled { eases } else { 0 },
                        submitted: Vec::new(),
                    };
                    // The first picture cannot be written, if `fails`.
                    let mut written = !fails;
                    let mut turns = 0;
                    loop {
                        turns += 1;
                        assert!(turns < 100, "never at rest: {state}");
                        let on_its_way = still.in_work.is_some();
                        let turn = still.turn(model_takes, eases, written);
                        written |= turn.render;
                        // Nothing to do and nothing on its way: from here
                        // on no turn can find anything else.
                        if turn == IDLE && !on_its_way {
                            break;
                        }
                    }
                    assert!(still.in_work.is_none(), "{state}");
                    // At rest the picture is the frame's, with the frame's
                    // own depth map, eased in all the way.
                    assert!(up_to_date(still.shown.as_ref(), &still.found), "{state}");
                    assert_eq!(still.found.mapped, Some(frame), "{state}");
                    assert!(still.found.settled && still.found.model_free, "{state}");
                    // The model was given the frame once, unless it had it
                    // already or had answered for it.
                    let asked = match at_work {
                        Some(at_work) => at_work != frame,
                        None => !mapped,
                    };
                    let expected = if asked { vec![frame] } else { Vec::new() };
                    assert_eq!(still.submitted, expected, "{state}");
                    rested += 1;
                }
            }
        }
        assert!(rested > 17 * 640 * 4);
    }

    #[test]
    fn a_model_that_fails_again_leaves_the_flat_picture_as_it_is() {
        let mut smoother = DepthSmoother::default();
        assert!(smoother.update(2, 1, &[0.0, 1.0], 0.0));
        let mut depth = DepthState {
            smoother,
            version: 3,
            latest: Some((6, 518, RawDepth::new(2, 1, vec![0.0, 1.0]).unwrap())),
            settled: true,
        };
        // The first failure takes the map away: another picture, flat.
        depth.forget();
        assert_eq!(depth.version, 4);
        assert!(depth.latest.is_none() && !depth.settled);
        assert!(depth.smoother.values().is_empty());
        // The next attempts fail as well, and nothing has changed: the
        // loop only gives the frame to the model again, and rests.
        depth.forget();
        depth.forget();
        assert_eq!(depth.version, 4);
        let (shown, found) = at_rest();
        let (shown, flat) = (
            Shown { depth: 4, ..shown },
            Found {
                depth: depth.version,
                mapped: None,
                settled: false,
                ..found
            },
        );
        let only_submit = Turn {
            submit: true,
            ..IDLE
        };
        assert_eq!(plan(Some(&shown), &flat), only_submit);
    }

    #[test]
    fn a_model_seen_free_has_its_answer_seen_with_it() {
        // The model is at work on frame 5 of a screen that has stopped
        // changing; the picture of that frame is up.
        let shared = Arc::new(Shared::new());
        {
            let mut slot = lock(&shared.for_model);
            (slot.serial, slot.detail, slot.busy) = (5, 518, true);
        }
        let turn_state = shared.clone();
        // A turn looks at what there is while the depth thread is about to
        // answer: the model's place is kept from it here until the answer
        // has been published and the place freed, in that order, as the
        // depth thread does it.
        let mut place = lock(&shared.for_model);
        let turn = std::thread::spawn(move || turn_state.find(5).0);
        std::thread::sleep(Duration::from_millis(100));
        {
            let mut depth = lock(&shared.depth);
            depth.version += 1;
            depth.latest = Some((5, 518, RawDepth::new(1, 1, vec![0.0]).unwrap()));
        }
        place.busy = false;
        drop(place);
        let found = turn.join().unwrap();
        // A model seen free has answered, and that answer is what the turn
        // must go by. With the map looked at first and the place after, the
        // turn had the map from before the answer and a free model: frame 5
        // went to the model a second time.
        assert!(found.model_free);
        assert_eq!(found.mapped, Some((5, 518)));
        let shown = Shown {
            serial: 5,
            look: found.look,
            depth: found.depth - 1,
            output: found.output,
        };
        assert!(!plan(Some(&shown), &found).submit);
    }

    /// The render loop's turns as far as their times go, on a clock of the
    /// test's own: a frame arrives every `every` microseconds, a turn works
    /// for `work`, and the turns named in `held` are kept from finishing
    /// for that much longer. For each turn: when it took its frame, which
    /// frame that was, and how long the frame had been waiting.
    fn paced(
        every: u64,
        work: u64,
        turns: usize,
        held: &[(usize, u64)],
    ) -> Vec<(Duration, u64, Duration)> {
        let micros = Duration::from_micros;
        let start = Instant::now();
        let (mut now, mut due) = (start, start);
        let mut seen = None;
        let mut taken = Vec::new();
        for turn in 0..turns {
            // The loop sleeps until the turn is due...
            now = now.max(due);
            due = next_turn(due, now);
            // ...and waits for a frame it has not seen.
            let newest = (now - start).as_micros() as u64 / every;
            let frame = if seen == Some(newest) {
                newest + 1
            } else {
                newest
            };
            let arrived = start + micros(frame * every);
            now = now.max(arrived);
            seen = Some(frame);
            taken.push((now - start, frame, now - arrived));
            let more = held.iter().find(|(at, _)| *at == turn).map_or(0, |h| h.1);
            now += micros(work + more);
        }
        taken
    }

    #[test]
    fn a_frame_is_taken_as_it_arrives_and_a_late_turn_is_made_up_for_at_once() {
        // A screen of 60 frames a second, a turn of 5 ms, and one turn that
        // takes 20 ms longer (the machine was busy).
        let taken = paced(16_667, 5_000, 40, &[(10, 20_000)]);
        for (turn, (_, frame, waited)) in taken.iter().enumerate() {
            match turn {
                // The frame that came while the long turn was at work has
                // waited (8.3 ms); the one after it is taken as it arrives
                // again. Counted from each turn's own beginning, with
                // 16 ms, the frames after it waited 7.7, 7.0, 6.3 ms and so
                // on, twelve of them.
                11 => assert!(*waited > Duration::from_millis(8), "{waited:?}"),
                _ => assert_eq!(*waited, Duration::ZERO, "turn {turn}"),
            }
            // No frame is left out.
            assert_eq!(*frame, turn as u64);
        }
        // A turn held for longer than a frame's time costs the frames that
        // came meanwhile and nothing after them.
        let taken = paced(16_667, 5_000, 40, &[(10, 50_000)]);
        assert_eq!((taken[11].1, taken[12].1), (13, 14));
        assert!(taken[12..].iter().all(|t| t.2 == Duration::ZERO));
        // The fastest a desktop sends when it is asked for 62 at the most
        // (KDE Plasma: 62 to 64 a second): every frame, none waiting.
        for every in [16_129, 16_000, 15_625] {
            let taken = paced(every, 5_000, 200, &[]);
            for (turn, (_, frame, waited)) in taken.iter().enumerate() {
                assert_eq!((*frame, *waited), (turn as u64, Duration::ZERO), "{every}");
            }
        }
    }

    #[test]
    fn a_stream_that_keeps_to_no_limit_is_taken_at_the_loops_own_pace() {
        // 165 frames a second, as a screen cast of the Odyssey would be
        // from a source that is not a desktop.
        let taken = paced(6_061, 5_000, 400, &[(100, 40_000), (200, 200_000)]);
        let turns_in = |from: Duration, to: Duration| {
            taken.iter().filter(|t| t.0 >= from && t.0 < to).count() as u32
        };
        // Over any second no more pictures than `FRAME_TIME` allows, and
        // one more after each turn that was late.
        let a_second = Duration::from_secs(1);
        let most = (a_second.as_micros() / FRAME_TIME.as_micros()) as u32 + 1;
        let end = taken.last().unwrap().0;
        let mut from = Duration::ZERO;
        while from + a_second <= end {
            let count = turns_in(from, from + a_second);
            assert!(count <= most + 2, "{count} pictures from {from:?}");
            from += Duration::from_millis(100);
        }
        // Never two turns closer than a turn's own work, and after the one
        // that makes up for a late turn the pace is `FRAME_TIME` again.
        let gaps: Vec<Duration> = taken.windows(2).map(|w| w[1].0 - w[0].0).collect();
        let short = gaps.iter().filter(|gap| **gap < FRAME_TIME).count();
        // (The turn that is forgiven comes after 5 ms, and the one after it
        // 10 ms later: back on the pace of the turns before the late one.)
        assert_eq!(short, 4, "{gaps:?}");
        // Frames are left out, never taken twice or out of order.
        assert!(taken.windows(2).all(|w| w[1].1 > w[0].1));
    }

    #[test]
    fn the_loop_is_faster_than_the_frames_a_desktop_is_asked_for() {
        // KDE Plasma spaces the frames it sends by whole milliseconds.
        let desktop = Duration::from_millis(1000 / u64::from(crate::capture::MOST_A_SECOND));
        assert!(
            FRAME_TIME + Duration::from_micros(500) <= desktop,
            "{FRAME_TIME:?} against {desktop:?}"
        );
    }

    #[test]
    fn the_output_is_let_go_of_with_the_live_screen_whatever_the_model_is_doing() {
        let dir = tempfile::tempdir().unwrap();
        let sink = FrameSink::in_folder(dir.path()).unwrap();
        let files = || std::fs::read_dir(dir.path()).unwrap().count();
        let fed = crate::capture::Fed::start();
        let shared = Arc::new(Shared::new());
        shared.lines.store(360, Ordering::Relaxed);
        let (frames, render_state) = (fed.frames(), shared.clone());
        let render = std::thread::Builder::new()
            .name("live-render".into())
            .spawn(move || render_loop(&frames, &render_state))
            .unwrap();
        // The depth thread, inside the model for as long as the test says.
        let (finish, finished) = std::sync::mpsc::channel::<()>();
        let depth_state = shared.clone();
        let depth = std::thread::Builder::new()
            .name("live-depth".into())
            .spawn(move || {
                let _ = finished.recv();
                drop(depth_state);
            })
            .unwrap();
        let mut live = Live {
            capture: fed.capture(),
            shared,
            threads: vec![render],
        };
        live.show_on(sink.clone());
        fed.feed(16, 9, &[90; 16 * 9 * 4]);
        eventually("a picture", || sink.written().0 == 1);
        // The output ends (its session lets go of the file), then the live
        // screen: the file is removed although the model is still at work.
        // The program may end before the model has finished, and nothing
        // would remove the file then.
        drop(sink);
        assert_eq!(files(), 1);
        drop(live);
        assert_eq!(files(), 0, "the picture file outlived the live screen");
        finish.send(()).unwrap();
        depth.join().unwrap();
    }

    #[test]
    fn the_page_is_told_when_the_screen_is_still_and_when_it_changes_again() {
        let dir = tempfile::tempdir().unwrap();
        let sink = FrameSink::in_folder(dir.path()).unwrap();
        let capture = crate::capture::Fed::start();
        let shared = Arc::new(Shared::new());
        shared.lines.store(360, Ordering::Relaxed);
        *lock(&shared.sink) = (1, Some(sink.clone()));
        let (frames, worker_state) = (capture.frames(), shared.clone());
        let worker = std::thread::spawn(move || render_loop(&frames, &worker_state));
        let still = || lock(&shared.stats).still;

        let first = Instant::now();
        capture.feed(16, 9, &[90; 16 * 9 * 4]);
        eventually("the first picture", || sink.written().0 == 1);
        assert!(!still());
        // No model here, so the frame waits for one for ever; the picture
        // is up to date all the same, and nothing is made.
        eventually("the screen is said to be still", still);
        assert!(first.elapsed() >= STILL_TIME, "{:?}", first.elapsed());
        assert_eq!(sink.written().0, 1);
        // A slider: one picture, and with it the screen is no longer said
        // to be still (the page is told before the picture is written).
        shared.depth_pos.store(3, Ordering::Relaxed);
        eventually("the picture with the control", || sink.written().0 == 2);
        assert!(!still());

        shared.stop.store(true, Ordering::Relaxed);
        drop(capture);
        worker.join().unwrap();
    }

    #[test]
    fn the_status_tells_the_rates_of_work_and_says_when_there_is_none() {
        let working = Stats {
            picture_rate: 59.6,
            depth_rate: 29.6,
            ..Stats::default()
        };
        assert_eq!(
            working.describe(),
            "60 pictures a second, depth 30 times a second"
        );
        // Before the model's first answer, and when it cannot answer.
        let starting = Stats {
            depth_rate: 0.0,
            ..working.clone()
        };
        assert_eq!(starting.describe(), "60 pictures a second, depth starting");
        let failing = Stats {
            depth_rate: 0.0,
            depth_error: Some("no model".into()),
            ..working.clone()
        };
        assert_eq!(
            failing.describe(),
            "60 pictures a second, no depth: no model"
        );
        // A model slower than once a second is not "0 times a second".
        let slow = Stats {
            depth_rate: 0.25,
            ..working
        };
        assert_eq!(
            slow.describe(),
            "60 pictures a second, depth once every 4.0 seconds"
        );
        // A still screen: the rates are those of the last work (here one
        // picture and a first depth map that took three seconds), and are
        // not told as if they were going on.
        let still = Stats {
            picture_rate: 1.0,
            depth_rate: 0.33,
            still: true,
            ..Stats::default()
        };
        assert_eq!(still.describe(), "not changing, so nothing is redrawn");
        // What the model has not done yet or cannot do is still said.
        let waiting = Stats {
            depth_rate: 0.0,
            ..still.clone()
        };
        assert_eq!(
            waiting.describe(),
            "not changing, so nothing is redrawn, depth starting"
        );
        let flat = Stats {
            depth_error: Some("no model".into()),
            ..still
        };
        assert_eq!(
            flat.describe(),
            "not changing, so nothing is redrawn, no depth: no model"
        );
    }

    /// Wait until `done` says so, at most ten seconds.
    fn eventually(what: &str, done: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < deadline, "never: {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// The picture the loop should have written for `picture` (one view's
    /// pixels) placed by `layout`.
    fn expected_picture(
        picture: &[u8],
        (view, (x, y, w, h)): ((u32, u32), (u32, u32, u32, u32)),
        depth: Option<Depth<'_>>,
        look: Look,
    ) -> Vec<u8> {
        let place = Placement {
            view,
            x,
            y,
            swap: look.swap,
        };
        let params = Params {
            strength: DEPTH.value(look.depth_pos),
            popout: POPOUT.value(look.pop_pos),
        };
        let mut canvas: Vec<u8> = [0, 0, 0, 255].repeat(place.bytes() / 4);
        if look.sharpness > 0 {
            let mut pair = vec![0u8; picture.len() * 2];
            assert!(live::render_pair(picture, w, h, depth, params, &mut pair));
            let amount = look.sharpness as f32 / 100.0;
            assert!(live::place_pair(&pair, w, h, amount, place, &mut canvas));
        } else {
            assert!(live::render_placed(
                picture,
                w,
                h,
                depth,
                params,
                place,
                &mut canvas
            ));
        }
        canvas
    }

    #[test]
    fn a_still_screen_is_shown_once_and_again_only_for_what_changes() {
        use any2odyssey_core::live::{PixelOrder, Source};
        let dir = tempfile::tempdir().unwrap();
        let sink = FrameSink::in_folder(dir.path()).unwrap();
        let capture = crate::capture::Fed::start();
        let shared = Arc::new(Shared::new());
        shared.lines.store(360, Ordering::Relaxed);
        shared.steadiness.store(20, Ordering::Relaxed);
        // There is no depth thread here: the test plays the model, and the
        // loop must never give it the still frame a second time.
        let (frames, worker_state) = (capture.frames(), shared.clone());
        let worker = std::thread::spawn(move || render_loop(&frames, &worker_state));
        let pictures = || sink.written().0;
        // Several of the loop's waits for a frame (150 ms each).
        let a_while = Duration::from_millis(600);

        // This 4:3 screen needs both side bars and a reduction beyond 2:1.
        let (sw, sh) = (1000u32, 750u32);
        let screen: Vec<u8> = (0..sw * sh * 4).map(|i| (i * 7 % 251) as u8).collect();
        capture.feed(sw, sh, &screen);
        // The frame goes to the model at once, and nowhere else: there is
        // no output yet.
        eventually("the frame is given to the model", || {
            lock(&shared.for_model).waiting
        });
        std::thread::sleep(a_while);
        assert_eq!(pictures(), 0);
        {
            let slot = lock(&shared.for_model);
            assert_eq!((slot.serial, slot.detail), (1, 518));
            assert_eq!(
                (slot.width, slot.height),
                crate::depth::model_size(sw, sh, 518)
            );
            assert_eq!(
                slot.pixels.len(),
                slot.width as usize * slot.height as usize * 4
            );
        }

        // The output comes up: the picture that was waiting is written,
        // flat (the model has not answered), and then nothing more.
        *lock(&shared.sink) = (1, Some(sink.clone()));
        eventually("the first picture", || pictures() == 1);
        let placed = layout((sw, sh), 360);
        let (view, (_, _, w, h)) = placed;
        let mut picture = vec![0u8; w as usize * h as usize * 4];
        let source = Source {
            bytes: &screen,
            width: sw,
            height: sh,
            stride: sw as usize * 4,
            order: PixelOrder::Bgr,
        };
        assert!(live::shrink_to_rgba(source, &mut picture, w, h));
        let mut look = shared.find(1).0.look;
        let (_, width, height, bytes) = sink.written();
        assert_eq!((width, height), (view.0 * 2, view.1));
        assert!(bytes == expected_picture(&picture, placed, None, look));
        std::thread::sleep(a_while);
        assert_eq!(pictures(), 1, "a still screen was shown again");

        // A control: one more picture, with it.
        shared.sharpness.store(50, Ordering::Relaxed);
        eventually("the sharpened picture", || pictures() == 2);
        look.sharpness = 50;
        assert!(sink.written().3 == expected_picture(&picture, placed, None, look));
        std::thread::sleep(a_while);
        assert_eq!(pictures(), 2);

        // The model answers, as the depth thread does: a first step
        // towards the frame's map from the depth of the scene before. The
        // answer is in the model's own numbers, on another scale than the
        // scene before; where it ends is the map of this frame by itself.
        let before = [1.0f32, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0];
        let own = [0.0f32, 0.25, 0.5, 1.0, 0.0, 0.5, 0.75, 1.0];
        let map = RawDepth::new(4, 2, vec![2.0, 3.0, 4.0, 6.0, 2.0, 4.0, 5.0, 6.0]).unwrap();
        assert_eq!(map.clone().normalized().unwrap().values(), own);
        let steps = {
            let mut depth = lock(&shared.depth);
            assert!(depth.smoother.update(4, 2, &before, 0.0));
            assert!(depth.smoother.update(4, 2, map.values(), 0.2));
            assert_ne!(depth.smoother.values(), own);
            depth.version += 1;
            depth.latest = Some((1, 518, map.clone()));
            // How many steps the easing will take from here.
            let mut eased = depth.smoother.clone();
            let mut steps = 1;
            while !eased.settle(4, 2, map.values(), 0.2) {
                steps += 1;
            }
            steps
        };
        assert!(steps >= 3, "{steps}");
        lock(&shared.for_model).busy = false;
        // It is shown and eased in, a picture a step, to the frame's own
        // map exactly; and there it rests.
        eventually("the map is eased in", || lock(&shared.depth).settled);
        assert_eq!(lock(&shared.depth).smoother.values(), own);
        // The answer as it came, then every step: a picture each.
        let eased = 2 + 1 + steps;
        eventually("every step is shown", || pictures() == eased);
        let depth = Depth {
            values: &own,
            width: 4,
            height: 2,
        };
        assert!(sink.written().3 == expected_picture(&picture, placed, Some(depth), look));
        std::thread::sleep(a_while);
        assert_eq!(pictures(), eased, "a settled screen was shown again");
        assert!(
            !lock(&shared.for_model).busy,
            "the model was given the frame it had answered for"
        );

        // The screen changes: its frame is shown and goes to the model.
        let screen: Vec<u8> = screen.iter().map(|b| !b).collect();
        capture.feed(sw, sh, &screen);
        eventually("the new frame", || pictures() == eased + 1);
        eventually("the new frame is given to the model", || {
            let slot = lock(&shared.for_model);
            slot.waiting && slot.serial == 2
        });

        shared.stop.store(true, Ordering::Relaxed);
        drop(capture);
        worker.join().unwrap();
    }

    #[test]
    fn the_model_is_shown_a_large_screen_at_the_detail_chosen_not_at_twice_it() {
        // Both loops as the program runs them, the capture fed by hand and
        // the model played by the test. 0.9.8 handed a large screen over at
        // twice the model's size for the estimator to halve, and told the
        // estimator that frame's long side for the detail: the model was
        // shown 1036 x 588 at "Standard" and 1274 x 714 at "Fine".
        for ((sw, sh), detail, twice) in [
            ((1920u32, 1080u32), 518u32, true),
            ((1920, 1080), 770, true),
            ((1920, 1080), 1022, false),
            ((2560, 1440), 1274, true),
            ((5120, 1440), 518, true),
            ((1000, 600), 518, false),
        ] {
            let what = format!("{sw} x {sh} at {detail}");
            let capture = crate::capture::Fed::start();
            let shared = Arc::new(Shared::new());
            shared.detail.store(detail as i32, Ordering::Relaxed);
            // (the frame's size, the detail, and its darkest and lightest red)
            type Asked = ((u32, u32), u32, (u8, u8));
            let asked: Arc<Mutex<Vec<Asked>>> = Arc::default();
            let (frames, state) = (capture.frames(), shared.clone());
            let render = std::thread::spawn(move || render_loop(&frames, &state));
            let (state, seen) = (shared.clone(), asked.clone());
            let depth = std::thread::spawn(move || {
                depth_loop_with(&state, |frame, detail| {
                    let reds = frame.pixels().map(|pixel| pixel.0[0]);
                    let range = (reds.clone().min().unwrap(), reds.max().unwrap());
                    lock(&seen).push((frame.dimensions(), detail, range));
                    // The map comes back at the size the estimator shows the
                    // model the frame at (`Estimator::estimate_raw`).
                    let (w, h) = crate::depth::model_size(frame.width(), frame.height(), detail);
                    Ok(RawDepth::new(w, h, vec![0.0; (w * h) as usize]).ok())
                })
            });
            // Columns of black and white one pixel wide, as small text is.
            let screen: Vec<u8> = (0..sw * sh)
                .flat_map(|i| [if i % sw % 2 == 0 { 255u8 } else { 0 }; 4])
                .collect();
            capture.feed(sw, sh, &screen);
            eventually(&what, || lock(&shared.depth).latest.is_some());
            shared.stop.store(true, Ordering::Relaxed);
            shared.model_wanted.notify_all();
            render.join().unwrap();
            depth.join().unwrap();

            let model = crate::depth::model_size(sw, sh, detail);
            let (frame, told, (darkest, lightest)) = lock(&asked)[0];
            if sw > frame.0 * 2 {
                // Shrunk by more than half: every pixel is counted, four
                // or five columns to one, so nothing is left black or
                // white. Four samples of it leave both.
                assert!(
                    darkest >= 80 && lightest <= 175,
                    "{what}: {darkest} to {lightest}"
                );
            }
            let expected = if twice {
                (model.0 * 2, model.1 * 2)
            } else {
                model
            };
            assert_eq!(frame, expected, "{what}: the frame handed over");
            assert_eq!(told, detail, "{what}: the detail the estimator is told");
            let answer = lock(&shared.depth)
                .latest
                .as_ref()
                .map(|(_, _, map)| (map.width(), map.height()));
            assert_eq!(answer, Some(model), "{what}: the size the model saw");
        }
    }

    #[test]
    fn the_defaults_match_the_still_viewer() {
        let controls = Controls::default();
        assert_eq!(DEPTH.value(controls.depth_pos), Params::default().strength);
        assert_eq!(POPOUT.value(controls.pop_pos), Params::default().popout);
        assert_eq!(controls.lines, 1080);
    }
}

/// Timings of the stages on this machine. Run by hand:
/// `cargo test --release -p any2odyssey live::bench -- --ignored --nocapture`
#[cfg(test)]
mod bench {
    use super::*;
    use any2odyssey_core::live::{PixelOrder, Source};

    #[test]
    #[ignore = "a measurement, not a check; needs the depth model"]
    fn stage_timings() {
        for (sw, sh) in [(3840u32, 2160u32), (5120, 1440)] {
            let bytes: Vec<u8> = (0..sw as usize * sh as usize * 4)
                .map(|i| ((i / 4 % sw as usize) ^ (i / 4 / sw as usize)) as u8)
                .collect();
            let source = Source {
                bytes: &bytes,
                width: sw,
                height: sh,
                stride: sw as usize * 4,
                order: PixelOrder::Bgr,
            };
            let mut map = None;
            for (detail, name) in crate::depth::DETAIL {
                let (mw, mh) = crate::depth::model_size(sw, sh, detail);
                let mut small = RgbaImage::new(mw, mh);
                live::scale_to_rgba(source, &mut small, mw, mh);
                // As the live screen's thread asks: no room is given back
                // between two frames of one size.
                let estimate = || {
                    crate::depth::estimate_unless_cancelled(
                        &small,
                        detail,
                        crate::depth::Purpose::Live,
                        || false,
                    )
                    .expect("depth model")
                    .expect("not cancelled")
                };
                let first = Instant::now();
                map = Some(estimate());
                let first = first.elapsed();
                let start = Instant::now();
                for _ in 0..20 {
                    estimate();
                }
                println!(
                    "{sw}x{sh} depth {name} ({mw}x{mh}): first {first:?}, then {:?} each",
                    start.elapsed() / 20
                );
            }
            let map = map.unwrap();
            for lines in [720, 1080, 1440, 2160] {
                let (view, (_, _, w, h)) = layout((sw, sh), lines);
                let mut picture = vec![0u8; (w * h * 4) as usize];
                let mut pair = vec![0u8; (w * h * 8) as usize];
                let place = Placement {
                    view,
                    x: 0,
                    y: 0,
                    swap: false,
                };
                let mut canvas = vec![0u8; place.bytes()];
                let depth = Depth {
                    values: map.values(),
                    width: map.width(),
                    height: map.height(),
                };
                let n = 20;
                let t = Instant::now();
                for _ in 0..n {
                    live::shrink_to_rgba(source, &mut picture, w, h);
                }
                let scale = t.elapsed() / n;
                let t = Instant::now();
                for _ in 0..n {
                    live::render_pair(&picture, w, h, Some(depth), Params::default(), &mut pair);
                }
                let render = t.elapsed() / n;
                let t = Instant::now();
                for _ in 0..n {
                    live::render_placed(
                        &picture,
                        w,
                        h,
                        Some(depth),
                        Params::default(),
                        place,
                        &mut canvas,
                    );
                }
                let placed = t.elapsed() / n;
                let t = Instant::now();
                for _ in 0..n {
                    live::place_pair(&pair, w, h, 0.5, place, &mut canvas);
                }
                let sharp = t.elapsed() / n;
                println!("  {lines} lines (view {}x{}, picture {w}x{h}): scale {scale:?}, render side by side {render:?} or in place {placed:?}, sharpen into place {sharp:?}", view.0, view.1);
            }
        }
    }

    /// The middle one of `times`, in milliseconds.
    fn median(times: &mut [Duration]) -> f64 {
        times.sort();
        times[times.len() / 2].as_secs_f64() * 1e3
    }

    /// A turn of the render loop on a 4K screen that changes with every
    /// frame, stage by stage and as a whole, the picture handed over as the
    /// weaver gets it (a frame file in memory where there is `/dev/shm`).
    /// No model is needed: the depth map is made up. Run by hand:
    /// `cargo test --release -p any2odyssey live::bench::turn_timings -- --ignored --nocapture`
    #[test]
    #[ignore = "a measurement, not a check"]
    fn turn_timings() {
        let (sw, sh) = (3840u32, 2160u32);
        let screens: Vec<Vec<u8>> = (0..2u32)
            .map(|seed| {
                (0..sw * sh * 4)
                    .map(|i| {
                        let (x, y) = (i / 4 % sw, i / 4 / sw);
                        ((x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503))
                            >> (3 + i % 4 * 8)) as u8
                            ^ (seed * 91) as u8
                    })
                    .collect()
            })
            .collect();
        let (dw, dh) = (518u32, 294u32);
        let values: Vec<f32> = (0..dw * dh)
            .map(|i| {
                let (x, y) = (i % dw, i / dw);
                let near = x > dw / 3 && x < dw / 2 && y > dh / 4 && y < dh / 2;
                (y as f32 / dh as f32 * 0.6 + if near { 0.3 } else { 0.0 }).min(1.0)
            })
            .collect();
        let depth = Some(Depth {
            values: &values,
            width: dw,
            height: dh,
        });
        let memory = std::path::Path::new("/dev/shm");
        let dir = if memory.is_dir() {
            tempfile::tempdir_in(memory)
        } else {
            tempfile::tempdir()
        }
        .unwrap();
        let sink = FrameSink::in_folder(dir.path()).unwrap();
        for lines in [1080, 1440, 2160] {
            for sharpness in [0, 50] {
                let (view, (x, y, w, h)) = layout((sw, sh), lines);
                let place = Placement {
                    view,
                    x,
                    y,
                    swap: false,
                };
                let mut picture = vec![0u8; (w * h * 4) as usize];
                let mut pair = vec![0u8; if sharpness > 0 { picture.len() * 2 } else { 0 }];
                let mut canvas = vec![0u8; place.bytes()];
                let (mut scale, mut render, mut sharpen, mut write, mut turn) =
                    (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
                for frame in 0..65 {
                    let source = Source {
                        bytes: &screens[frame % 2],
                        width: sw,
                        height: sh,
                        stride: sw as usize * 4,
                        order: PixelOrder::Bgr,
                    };
                    let params = Params::default();
                    let t0 = Instant::now();
                    live::shrink_to_rgba(source, &mut picture, w, h);
                    let t1 = Instant::now();
                    if sharpness > 0 {
                        live::render_pair(&picture, w, h, depth, params, &mut pair);
                    } else {
                        live::render_placed(&picture, w, h, depth, params, place, &mut canvas);
                    }
                    let t2 = Instant::now();
                    if sharpness > 0 {
                        let amount = sharpness as f32 / 100.0;
                        live::place_pair(&pair, w, h, amount, place, &mut canvas);
                    }
                    let t3 = Instant::now();
                    sink.show_bottom_up(view.0 * 2, view.1, &canvas).unwrap();
                    let t4 = Instant::now();
                    // The first turns make the buffers and the file's pages.
                    if frame >= 5 {
                        scale.push(t1 - t0);
                        render.push(t2 - t1);
                        sharpen.push(t3 - t2);
                        write.push(t4 - t3);
                        turn.push(t4 - t0);
                    }
                }
                let turn = median(&mut turn);
                println!(
                    "{lines} lines, sharpness {sharpness}: scale {:.2} ms, render {:.2}, sharpen {:.2}, hand over {:.2}; a turn {turn:.2} ms ({:.0} a second)",
                    median(&mut scale),
                    median(&mut render),
                    median(&mut sharpen),
                    median(&mut write),
                    1000.0 / turn
                );
            }
        }
    }
}

/// The whole live path against the remembered screen, without the page.
/// Run by hand: `cargo test --release -p any2odyssey live::probe -- --ignored --nocapture`
#[cfg(test)]
mod probe {
    use super::*;
    use crate::weaver::{Weaver, WeaverStatus};

    #[test]
    #[ignore = "a measurement; needs the Odyssey, the Wine setup and a remembered screen"]
    fn the_live_path_end_to_end() {
        // More than one round checks that it can be stopped and started
        // again in one process, as the page does.
        let rounds: u32 = std::env::var("PROBE_ROUNDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        for round in 1..=rounds {
            println!("round {round}");
            one_round();
        }
    }

    fn one_round() {
        let file = any2odyssey_core::paths::capture_token_file();
        let token = std::fs::read_to_string(&file)
            .ok()
            .map(|t| t.trim().to_owned());
        let mut weaver = Weaver::new();
        weaver.start(true, || {});
        let start = Instant::now();
        let sink = loop {
            weaver.poll();
            if let Some(sink) = weaver.sink() {
                break sink;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "{:?}",
                weaver.status()
            );
            std::thread::sleep(Duration::from_millis(50));
        };
        let controls = Controls {
            lines: std::env::var("PROBE_LINES")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1080),
            detail: std::env::var("PROBE_DETAIL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(crate::depth::LONG_SIDE as i32),
            ..Controls::default()
        };
        let mut live = Live::start(token, false, controls, || {});
        live.show_on(sink);
        let frames = live.frames();
        let seconds: u64 = std::env::var("PROBE_SECONDS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(15);
        let (mut seen, mut last) = (0u64, 0u64);
        let (mut low, mut high, mut count) = ([255u64; 3], [0u64; 3], 0u32);
        while start.elapsed() < Duration::from_secs(seconds + 40) {
            weaver.poll();
            let on = matches!(weaver.status(), WeaverStatus::On { .. });
            let got = frames.with_newest(seen, Duration::from_millis(200), |slot| {
                let s = slot.source();
                let (w, h) = (s.width as usize, s.height as usize);
                let (mut sum, mut n) = ([0u64; 3], 0u64);
                for y in (h / 10..h * 9 / 10).step_by(16) {
                    for x in (w / 10..w * 9 / 10).step_by(16) {
                        let p = &s.bytes[y * s.stride + x * 4..];
                        for c in 0..3 {
                            sum[c] += p[c] as u64;
                        }
                        n += 1;
                    }
                }
                (slot.serial, sum.map(|v| v / n.max(1)))
            });
            if let Some((serial, mean)) = got.filter(|(serial, _)| *serial != seen) {
                seen = serial;
                count += 1;
                for c in 0..3 {
                    low[c] = low[c].min(mean[c]);
                    high[c] = high[c].max(mean[c]);
                }
            }
            let second = start.elapsed().as_secs();
            if second != last {
                last = second;
                println!(
                    "{second:3}s weaver {:?} excluded {:?} capture {:?} stats {:?} | cast frames {count}, mean low {low:?} high {high:?}",
                    weaver.status(),
                    weaver.excluded_from_capture(),
                    live.capture_state(),
                    live.stats()
                );
                (low, high, count) = ([255; 3], [0; 3], 0);
                if on && second > seconds {
                    break;
                }
            }
        }
        if let Some(token) = live.restore_token() {
            let _ = std::fs::write(&file, token);
        }
        let stopping = Instant::now();
        drop(live);
        weaver.stop();
        println!("stopped in {:?}", stopping.elapsed());
    }
}
