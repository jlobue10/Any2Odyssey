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
//! later.

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use any2odyssey_core::dibr::Params;
use any2odyssey_core::live::{self, Depth, DepthSmoother};
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

struct Shared {
    stop: AtomicBool,
    depth_pos: AtomicI32,
    pop_pos: AtomicI32,
    sharpness: AtomicI32,
    steadiness: AtomicI32,
    lines: AtomicI32,
    detail: AtomicI32,
    swap: AtomicBool,
    /// The frame the depth thread should look at next, already small.
    /// Where the pictures go, once the output is up. The capture comes
    /// first: the desktop may have to ask which screen, and its dialog must
    /// not end up underneath the output's window.
    sink: Mutex<Option<FrameSink>>,
    for_model: Mutex<Option<RgbaImage>>,
    model_wanted: Condvar,
    depth: Mutex<DepthSmoother>,
    stats: Mutex<Stats>,
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
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            depth_pos: AtomicI32::new(0),
            pop_pos: AtomicI32::new(0),
            sharpness: AtomicI32::new(0),
            steadiness: AtomicI32::new(0),
            lines: AtomicI32::new(1080),
            detail: AtomicI32::new(crate::depth::LONG_SIDE as i32),
            swap: AtomicBool::new(false),
            sink: Mutex::new(None),
            for_model: Mutex::new(None),
            model_wanted: Condvar::new(),
            depth: Mutex::new(DepthSmoother::default()),
            stats: Mutex::new(Stats::default()),
        });
        let mut live = Live {
            capture,
            shared,
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
        *lock(&self.shared.sink) = Some(sink);
    }

    pub fn has_output(&self) -> bool {
        lock(&self.shared.sink).is_some()
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

fn render_loop(frames: &Frames, shared: &Shared) {
    let mut seen = 0u64;
    let (mut picture, mut pair, mut canvas, mut scratch) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut model_frame: Option<RgbaImage> = None;
    let mut placement = None;
    let (mut counted, mut since) = (0u32, Instant::now());
    let mut started = Instant::now();
    while !shared.stop.load(Ordering::Relaxed) && !frames.ended() {
        // Sixty pictures a second at most: a faster screen (the Odyssey
        // runs at 120) would double the work for depth the eye cannot tell
        // apart, on the processor and the GPU.
        if let Some(early) = FRAME_TIME.checked_sub(started.elapsed()) {
            std::thread::sleep(early);
        }
        started = Instant::now();
        let lines = shared.lines.load(Ordering::Relaxed);
        // The depth thread takes its frame from `for_model` and leaves the
        // place empty; a frame is made for it only then.
        let model_free = lock(&shared.for_model).is_none();
        let detail = shared.detail.load(Ordering::Relaxed).max(0) as u32;
        // A still screen sends no frames. Its last one is worked on again a
        // few times a second all the same: the depth map of a picture that
        // has just appeared arrives after it, and the controls must take
        // effect without waiting for the screen to change.
        let taken = frames.with_newest(seen, Duration::from_millis(150), |slot| {
            let (view, place) = layout((slot.width, slot.height), lines);
            let (_, _, w, h) = place;
            picture.resize(w as usize * h as usize * 4, 0);
            if !live::scale_to_rgba(slot.source(), &mut picture, w, h) {
                return None;
            }
            if model_free {
                let (mw, mh) = crate::depth::model_size(slot.width, slot.height, detail);
                let mut small = RgbaImage::new(mw, mh);
                if live::scale_to_rgba(slot.source(), &mut small, mw, mh) {
                    model_frame = Some(small);
                }
            }
            Some((slot.serial, view, place))
        });
        let Some(Some((serial, view, (x, y, w, h)))) = taken else {
            continue;
        };
        seen = serial;
        if model_free {
            if let Some(small) = model_frame.take() {
                *lock(&shared.for_model) = Some(small);
                shared.model_wanted.notify_one();
            }
        }

        let params = Params {
            strength: DEPTH.value(shared.depth_pos.load(Ordering::Relaxed)),
            popout: POPOUT.value(shared.pop_pos.load(Ordering::Relaxed)),
        };
        pair.resize(picture.len() * 2, 0);
        {
            let depth = lock(&shared.depth);
            let (dw, dh) = depth.size();
            let map = (!depth.values().is_empty()).then(|| Depth {
                values: depth.values(),
                width: dw,
                height: dh,
            });
            live::render_pair(&picture, w, h, map, params, &mut pair);
        }
        let sharpness = shared.sharpness.load(Ordering::Relaxed).clamp(0, 100);
        if sharpness > 0 {
            live::sharpen(&mut pair, w * 2, sharpness as f32 / 100.0, &mut scratch);
            // The pair was sharpened as one picture; where the two views
            // meet, each one's edge column took in the other's. Those two
            // columns go back to what they were (`scratch` has it).
            let (row, seam) = (w as usize * 8, w as usize * 4);
            if seam >= 4 && scratch.len() == pair.len() {
                for (after, before) in pair.chunks_exact_mut(row).zip(scratch.chunks_exact(row)) {
                    after[seam - 4..seam + 4].copy_from_slice(&before[seam - 4..seam + 4]);
                }
            }
        }

        // Each view goes into its half of the picture, on black where the
        // screen's shape leaves room.
        let swap = shared.swap.load(Ordering::Relaxed);
        let (vw, vh) = (view.0 as usize, view.1 as usize);
        let (x, y, w, h) = (x as usize, y as usize, w as usize, h as usize);
        // Black wherever the screen's shape leaves room: made afresh when
        // the picture's size or place changes, or old pixels would stay.
        let placed = (vw, vh, x, y, w, h);
        if canvas.len() != vw * vh * 8 || placement != Some(placed) {
            placement = Some(placed);
            canvas.clear();
            canvas.resize(vw * vh * 8, 0);
            for alpha in canvas.as_chunks_mut::<4>().0 {
                alpha[3] = 255;
            }
        }
        for row in 0..h {
            let from = &pair[row * w * 8..(row + 1) * w * 8];
            let (left, right) = from.split_at(w * 4);
            let (left, right) = if swap { (right, left) } else { (left, right) };
            // The weaver's file holds the bottom row first.
            let line = (vh - 1 - (y + row)) * vw * 8;
            canvas[line + x * 4..line + (x + w) * 4].copy_from_slice(left);
            canvas[line + (vw + x) * 4..line + (vw + x + w) * 4].copy_from_slice(right);
        }
        let sink = lock(&shared.sink).clone();
        let written = match &sink {
            Some(sink) => sink.show_bottom_up(view.0 * 2, view.1, &canvas),
            None => Ok(()),
        };

        counted += 1;
        let elapsed = since.elapsed();
        if elapsed >= Duration::from_secs(1) || written.is_err() {
            let mut stats = lock(&shared.stats);
            stats.picture_rate = counted as f32 / elapsed.as_secs_f32().max(0.001);
            stats.output_error = written.err().map(|e| e.to_string());
            (counted, since) = (0, Instant::now());
        }
    }
}

fn depth_loop(shared: &Shared) {
    let (mut counted, mut since) = (0u32, Instant::now());
    loop {
        let frame = {
            let mut waiting = lock(&shared.for_model);
            loop {
                if shared.stop.load(Ordering::Relaxed) {
                    return;
                }
                // The frame stays in its place while the model runs, so the
                // render thread does not make another one meanwhile.
                if let Some(frame) = waiting.as_ref() {
                    break frame.clone();
                }
                waiting = shared
                    .model_wanted
                    .wait_timeout(waiting, Duration::from_millis(250))
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
        };
        // The frame was made at the size the model is to see it at.
        let estimated = crate::depth::estimate_unless_cancelled(
            &frame,
            frame.width().max(frame.height()),
            || shared.stop.load(Ordering::Relaxed),
        );
        *lock(&shared.for_model) = None;
        if shared.stop.load(Ordering::Relaxed) {
            return;
        }
        match estimated {
            Ok(None) => return,
            Ok(Some(map)) => {
                let steadiness = shared.steadiness.load(Ordering::Relaxed).clamp(0, 95);
                lock(&shared.depth).update(
                    map.width(),
                    map.height(),
                    map.values(),
                    steadiness as f32 / 100.0,
                );
                counted += 1;
                let elapsed = since.elapsed();
                if elapsed >= Duration::from_secs(1) {
                    let mut stats = lock(&shared.stats);
                    stats.depth_rate = counted as f32 / elapsed.as_secs_f32();
                    stats.depth_error = None;
                    (counted, since) = (0, Instant::now());
                }
            }
            Err(e) => {
                {
                    let mut stats = lock(&shared.stats);
                    stats.depth_rate = 0.0;
                    stats.depth_error = Some(e);
                }
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
                let mut scratch = Vec::new();
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
                    live::sharpen(&mut pair, w * 2, 0.5, &mut scratch);
                }
                let sharp = t.elapsed() / n;
                println!("  {lines} lines (view {}x{}, picture {w}x{h}): scale {scale:?}, render {render:?}, sharpen {sharp:?}", view.0, view.1);
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
