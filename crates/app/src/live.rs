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

use any2odyssey_core::dibr::{DepthMap, Params};
use any2odyssey_core::live::{self, Depth, DepthSmoother, Placement};
use any2odyssey_core::slider::{DEPTH, POPOUT};
use image::RgbaImage;

use crate::capture::{Capture, CaptureState, Frames};
use crate::weaver::{self, FrameSink};

/// The least time from one picture to the next.
const FRAME_TIME: Duration = Duration::from_micros(16_000);

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
    /// Pictures written to the weaver per second, and depth maps per second.
    pub picture_rate: f32,
    pub depth_rate: f32,
    /// Why depth is missing, when the model cannot run.
    pub depth_error: Option<String>,
    /// Why no picture can be written, when that fails.
    pub output_error: Option<String>,
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

/// The depth map the pictures are rendered with.
#[derive(Default)]
struct DepthState {
    smoother: DepthSmoother,
    /// Raised whenever the map changes, so that a picture made with an
    /// earlier one is known to be out of date.
    version: u64,
    /// The model's newest answer, and the frame and detail it was for.
    latest: Option<(u64, u32, DepthMap)>,
    /// That answer has been eased in all the way: the map will not change
    /// until the model answers again.
    settled: bool,
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
            // The depth thread takes its frame from `for_model` and says
            // when it is done with it; a frame is made for it only then.
            model_free: !lock(&self.for_model).busy,
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

fn render_loop(frames: &Frames, shared: &Shared) {
    let mut seen = 0u64;
    let (mut picture, mut pair, mut canvas, mut small) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut shown: Option<Shown> = None;
    let mut blank = None;
    let (mut counted, mut since) = (0u32, Instant::now());
    let mut failing = false;
    let mut started = Instant::now();
    while !shared.stop.load(Ordering::Relaxed) && !frames.ended() {
        // Sixty pictures a second at most: a faster screen (the Odyssey
        // runs at 120) would double the work for depth the eye cannot tell
        // apart, on the processor and the GPU.
        if let Some(early) = FRAME_TIME.checked_sub(started.elapsed()) {
            std::thread::sleep(early);
        }
        started = Instant::now();
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
                picture.resize(w as usize * h as usize * 4, 0);
                if !live::scale_to_rgba(slot.source(), &mut picture, w, h) {
                    return None;
                }
            }
            let for_model = turn.submit.then(|| {
                let (mw, mh) = crate::depth::model_size(slot.width, slot.height, found.detail);
                small.resize(mw as usize * mh as usize * 4, 0);
                live::scale_to_rgba(slot.source(), &mut small, mw, mh).then_some((mw, mh))
            });
            Some((found, sink, turn, view, place, for_model.flatten()))
        });
        let Some(Some((found, sink, turn, view, (x, y, w, h), for_model))) = taken else {
            continue;
        };
        seen = found.serial;
        if let Some((width, height)) = for_model {
            let mut slot = lock(&shared.for_model);
            std::mem::swap(&mut slot.pixels, &mut small);
            (slot.width, slot.height) = (width, height);
            (slot.serial, slot.detail) = (found.serial, found.detail);
            (slot.busy, slot.waiting) = (true, true);
            drop(slot);
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
            continue;
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
                    slot.waiting = false;
                    std::mem::swap(&mut slot.pixels, &mut pixels);
                    break (slot.width, slot.height, slot.serial, slot.detail);
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
        // The frame was made at the size the model is to see it at.
        let estimated = match RgbaImage::from_raw(width, height, std::mem::take(&mut pixels)) {
            Some(frame) => {
                let estimated = crate::depth::estimate_unless_cancelled(
                    &frame,
                    frame.width().max(frame.height()),
                    || shared.stop.load(Ordering::Relaxed),
                );
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
                {
                    let mut depth = lock(&shared.depth);
                    *depth = DepthState {
                        version: depth.version + 1,
                        ..DepthState::default()
                    };
                }
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
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
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
                latest: Some((6, 518, DepthMap::normalized(2, 1, vec![0.0, 1.0]).unwrap())),
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

        // A 4:3 screen, so the view has black at its sides.
        let (sw, sh) = (48u32, 36u32);
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
        assert!(live::scale_to_rgba(source, &mut picture, w, h));
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
        // towards the frame's map from the depth of the scene before.
        let before = [1.0f32, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0];
        let map = DepthMap::normalized(4, 2, vec![0.0, 0.25, 0.5, 1.0, 0.0, 0.5, 0.75, 1.0]);
        let map = map.unwrap();
        let steps = {
            let mut depth = lock(&shared.depth);
            assert!(depth.smoother.update(4, 2, &before, 0.0));
            assert!(depth.smoother.update(4, 2, map.values(), 0.2));
            assert_ne!(depth.smoother.values(), map.values());
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
        assert_eq!(lock(&shared.depth).smoother.values(), map.values());
        // The answer as it came, then every step: a picture each.
        let eased = 2 + 1 + steps;
        eventually("every step is shown", || pictures() == eased);
        let depth = Depth {
            values: map.values(),
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
                let first = Instant::now();
                map = Some(crate::platform::estimate_depth(&small, detail).expect("depth model"));
                let first = first.elapsed();
                let start = Instant::now();
                for _ in 0..20 {
                    crate::platform::estimate_depth(&small, detail).unwrap();
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
                    live::scale_to_rgba(source, &mut picture, w, h);
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
