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
/// It has to be shorter than the time between the frames of what is
/// commonly shown, 60 a second, or the frames come faster than they are
/// taken: each then waits longer than the one before, until one is left
/// out and the wait begins again from nothing. A desktop was asked for 62
/// frames at the most for a long time, and KDE Plasma spaced them by 16
/// ms; with 16 ms here the frames of such a screen were 13 ms old in the
/// middle when their pictures reached the output, where those of a screen
/// of 60 were 5, and 24 to 69 of them were left out in 40 seconds. A
/// desktop is asked for more now (`capture::MOST_A_SECOND`), and of a
/// screen that does change faster than this every second or third frame
/// is taken, as it arrives (`capture::fresh_from`).
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

/// A diagnostic: with `A2O_LIVE_TIMES=<file>` in the environment, a line
/// is written there for every picture and every depth map of a live
/// screen, their times in microseconds since the first of them:
///
/// - `p <frame> <arrived> <taken> <written> <depth of>`: the picture of
///   the screen's frame `frame`, which arrived then, was taken by the
///   render loop then and was in the output's hands then, drawn with a
///   depth map made from the frame `depth of` (0: none yet).
/// - `d <frame> <begun> <done>`: the depth map of frame `frame`, from when
///   the depth thread took the frame until the map was there to draw with.
/// - `w <frame> <from> <until> <how>`: what the picture of a new frame
///   did about the frame's own map (`Answered`): 1 waited and got it, 2
///   waited and it did not come in time, 3 did not wait because the maps
///   have been slow (or none has come), 4 because the screen shows no
///   film, 5 because the model was not given the frame (it was at work,
///   or the frame has its map).
/// - `s <frame> <at> <why>`: a picture of a frame that has had one is
///   about to be made, 1 to ease its map in a step, 2 for anything else
///   (a newer map, a control, another output).
///
/// - `x <lines>`: so many lines were left out here, because the file was
///   not written as fast as they came.
///
/// It answers two questions that cannot be answered from outside: how
/// evenly a film's pictures arrive as against how evenly they leave, and
/// how many pictures a depth map is behind. Nothing is opened, timed or
/// written without the variable.
mod times {
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;

    /// The lines on their way to a file, which a thread of its own opens
    /// and writes.
    ///
    /// The render loop and the depth thread opened and wrote the file
    /// themselves, a line at a time. With the variable naming a pipe that
    /// nobody reads, the first of them to ask stood still in the opening,
    /// holding the screen's newest frame: no picture was ever made, and
    /// stopping the output waited for the render loop for good. With a
    /// pipe that is read slowly, the pictures stood still for seconds at
    /// a time once the pipe was full (audit round 25: up to 7.8 s between
    /// two pictures of a screen of 60 frames a second, the reader taking
    /// 20 lines a second). Whoever has a line now only leaves it here,
    /// and where there is no room for it (the file takes them slower
    /// than they come) it is counted and left out.
    pub(super) struct Lines {
        waiting: SyncSender<String>,
        /// How many lines have found no room since the last that did.
        left_out: Mutex<u64>,
    }

    impl Lines {
        /// How many lines can wait: a few seconds of them.
        pub(super) const ROOM: usize = 4096;

        /// The lines of `path`, which is made anew. `None` where the
        /// thread that writes it cannot be started.
        pub(super) fn to(path: PathBuf) -> Option<Lines> {
            let (waiting, lines) = sync_channel::<String>(Lines::ROOM);
            std::thread::Builder::new()
                .name("live-times".into())
                .spawn(move || {
                    // A file that cannot be made is no file: the lines
                    // find nobody to take them.
                    let Ok(file) = std::fs::File::create(path) else {
                        return;
                    };
                    let mut file = std::io::BufWriter::new(file);
                    while let Ok(line) = lines.recv() {
                        // Everything that waits, then out with it: a line
                        // is in the file as soon as it can be.
                        let mut next = Some(line);
                        while let Some(line) = next {
                            let _ = writeln!(file, "{line}");
                            next = lines.try_recv().ok();
                        }
                        let _ = file.flush();
                    }
                })
                .ok()?;
            Some(Lines {
                waiting,
                left_out: Mutex::new(0),
            })
        }

        /// Leave `line` to be written. Never waits: a line that finds no
        /// room is counted, and the count is left before the next line
        /// that does.
        pub(super) fn leave(&self, line: String) {
            let mut left_out = self.left_out.lock().unwrap_or_else(|e| e.into_inner());
            if *left_out > 0 && self.waiting.try_send(format!("x {left_out}")).is_ok() {
                *left_out = 0;
            }
            if *left_out > 0 || matches!(self.waiting.try_send(line), Err(TrySendError::Full(_))) {
                *left_out += 1;
            }
        }
    }

    fn file() -> Option<&'static (Instant, Lines)> {
        static FILE: OnceLock<Option<(Instant, Lines)>> = OnceLock::new();
        FILE.get_or_init(|| {
            let path = std::env::var_os("A2O_LIVE_TIMES")?;
            Some((Instant::now(), Lines::to(path.into())?))
        })
        .as_ref()
    }

    /// Whether the times are asked for.
    pub fn wanted() -> bool {
        file().is_some()
    }

    /// Write a line that begins with `kind` and the frame's number,
    /// followed by `times` as microseconds (0 for a time that is none).
    pub fn note(kind: char, frame: u64, times: &[Option<Instant>], last: Option<u64>) {
        let Some((start, file)) = file() else {
            return;
        };
        let mut line = format!("{kind} {frame}");
        for time in times {
            let micros = time.map_or(0, |time| time.saturating_duration_since(*start).as_micros());
            line.push_str(&format!(" {micros}"));
        }
        if let Some(last) = last {
            line.push_str(&format!(" {last}"));
        }
        file.leave(line);
    }
}

/// Evens out the times at which the pictures of a film are shown.
///
/// A film's frames do not reach a screen cast as evenly as the film has
/// them. Measured on the Odyssey's own screen at 120 Hz with a film of 24
/// pictures a second in a browser (`A2O_LIVE_TIMES`, 2700 frames): 56 %
/// of the frames came 38 to 46 ms after the one before, and the others in
/// stretches of 31 to 33 and 50 ms by turns, the browser being a refresh
/// early and a refresh late. They were shown as they came. Here a frame
/// that comes sooner after the last than the frames have been coming is
/// held until its time, 9 ms at the most: over the same 2700 arrivals 91 %
/// are then 38 to 46 ms apart, each held for 3 ms in the mean.
///
/// Only a steady run of frames is evened out: the last eight all within
/// three tenths of their mean, and that mean between 20 and 70 ms (a film
/// or a video of 15 to 50 pictures a second). A desktop, a game and a
/// video of 60 a second are shown as they come.
#[derive(Default)]
struct Pace {
    /// When the last frames arrived, the newest last.
    arrived: std::collections::VecDeque<Instant>,
    /// When the last frame was to be shown.
    shown: Option<Instant>,
    /// When the next frame of a steady run is to arrive.
    next: Option<Instant>,
}

impl Pace {
    /// The gaps a run is told by.
    const RUN: usize = 8;
    /// The longest a frame is held.
    const HOLD: Duration = Duration::from_millis(9);
    /// What the time from frame to frame is taken to be shorter than it
    /// was found: the frames are shown a little sooner one after the
    /// other until one is shown as it comes, so nothing builds up.
    const EARLY: Duration = Duration::from_micros(500);

    /// When to show the frame that arrived at `arrived`: then, or later.
    fn show_at(&mut self, arrived: Instant) -> Instant {
        self.arrived.push_back(arrived);
        if self.arrived.len() > Pace::RUN + 1 {
            self.arrived.pop_front();
        }
        let gaps = || {
            let times = self.arrived.iter();
            times
                .clone()
                .zip(times.skip(1))
                .map(|(a, b)| b.saturating_duration_since(*a))
        };
        let mut at = arrived;
        self.next = None;
        if let (Some(shown), true) = (self.shown, self.arrived.len() == Pace::RUN + 1) {
            let mean = gaps().sum::<Duration>() / Pace::RUN as u32;
            let steady = gaps().all(|gap| gap.abs_diff(mean) * 10 <= mean * 3);
            let film = (Duration::from_millis(20)..=Duration::from_millis(70)).contains(&mean);
            if steady && film {
                at = (shown + mean - Pace::EARLY).clamp(arrived, arrived + Pace::HOLD);
                self.next = Some(arrived + mean);
            }
        }
        self.shown = Some(at);
        at
    }

    /// The longest of the last three times from frame to frame, where all
    /// three are a film's or a video's (22 to 70 ms): a screen whose
    /// picture has time to wait for its own depth map (`Answered`).
    ///
    /// The shortest was 25 ms, which is three refreshes of a screen of
    /// 120 Hz to the microsecond: what a video of 30 a second has between
    /// two frames when the second comes a refresh early, as a browser's do
    /// by turns. Such a frame was measured now a hair under and now a hair
    /// over. Fed that way (a 3840 x 2160 screen, 300 frames, TensorRT), 56
    /// to 76 of the frames counted as no video's and were drawn with the
    /// map of the frame before, 14 to 25 of them twice. 22 ms lies between
    /// that and what 48 or 50 a second have (16.7 and 25 ms by turns).
    ///
    /// Not the steady run [`Pace::show_at`] tells: that takes eight frames
    /// to be told again after one frame that came out of turn, and a
    /// browser's film has such a frame every few seconds. Measured at the
    /// output with the run asked for (a film, a minute): the frames that
    /// were shown twice came eight and nine in a second, then none.
    fn calm(&self) -> Option<Duration> {
        let mut newest = self.arrived.iter().rev();
        let mut later = *newest.next()?;
        let mut longest = Duration::ZERO;
        for _ in 0..3 {
            let earlier = *newest.next()?;
            let gap = later.saturating_duration_since(earlier);
            if !(Duration::from_millis(22)..=Duration::from_millis(70)).contains(&gap) {
                return None;
            }
            (longest, later) = (longest.max(gap), earlier);
        }
        Some(longest)
    }

    /// Whether such a screen's next frame is still to come at `now`: not
    /// yet later than its frames are late by.
    fn awaited(&self, now: Instant) -> bool {
        match (self.calm(), self.arrived.back()) {
            (Some(gap), Some(&last)) => now <= last + gap + Pace::HOLD * 2,
            _ => false,
        }
    }

    /// Whether the next frame of a steady run is to arrive within `time`
    /// of `now`, give or take what such frames are early and late by (it
    /// has not arrived: the run is told again with every frame that does).
    ///
    /// A picture made between two frames, because a depth map has come,
    /// takes the render loop as long as any other. With one made whenever
    /// a map came, a film's frame that arrived meanwhile waited for it:
    /// measured at 2160 lines, a frame was in the output's hands 8 to 22
    /// ms after it arrived where it had been 9 to 13, and the pictures
    /// were no more even than the frames. So no such picture is begun
    /// when a frame is about to come; the map is drawn with from that
    /// frame on.
    fn next_within(&self, now: Instant, time: Duration) -> bool {
        self.next.is_some_and(|next| {
            let early = next.checked_sub(Pace::HOLD).unwrap_or(next);
            now + time >= early && now <= next + Pace::HOLD * 2
        })
    }

    /// Whether the next frame of a screen that changes faster than a film
    /// is about to come at `now`: its last three frames came less than 25
    /// ms after the one before in the mean and none of them 50 ms after
    /// (a game, a video of 60 a second, a desktop that is scrolled, a
    /// pointer on the move), and the next is not yet later than such
    /// frames are late by.
    ///
    /// [`Pace::next_within`] knows of a film's frames only, and such a
    /// screen's next frame is nearer still: it always comes within the
    /// time a picture takes. A picture begun between two of its frames,
    /// because a depth map has come, is a second picture of a frame that
    /// has one, at another depth, and the frame that arrives meanwhile
    /// waits for it. Measured without this rule (audit round 25, a screen
    /// of 60 frames a second, a stand-in model of 15 ms a map, 1080
    /// lines): 952 of 1041 frames had two pictures, 115 pictures a second
    /// for 60 frames. On a 3840 x 2160 screen at 2160 lines, a map in 25
    /// ms: 443 of 1035 frames had two, and a frame's picture was in the
    /// frame file 13.5 ms after the frame was sent, the times from
    /// picture to picture spread by 3.8 ms (standard deviation), where
    /// that is 11.7 ms and 1.8 ms with the rule. The map is drawn with
    /// from the next frame on, as it was before a map's arrival woke the
    /// loop; a screen that has stopped changing is no longer told so, and
    /// its last frame gets its map.
    ///
    /// The mean, not each of the three: a video of 60 a second whose
    /// frames come a refresh early and a refresh late by turns on a
    /// screen of 120 Hz has them 8 and 25 ms apart. With each of the
    /// three asked to be under 25 ms that was no such screen, and 697 of
    /// its 1041 frames had two pictures (the same rig, a map in 5 ms).
    fn quick(&self, now: Instant) -> bool {
        let mut newest = self.arrived.iter().rev();
        let Some(&last) = newest.next() else {
            return false;
        };
        let (mut later, mut longest) = (last, Duration::ZERO);
        for _ in 0..3 {
            let Some(&earlier) = newest.next() else {
                return false;
            };
            let gap = later.saturating_duration_since(earlier);
            if gap >= Duration::from_millis(50) {
                return false;
            }
            (longest, later) = (longest.max(gap), earlier);
        }
        last.saturating_duration_since(later) < Duration::from_millis(25) * 3
            && now <= last + longest + Pace::HOLD * 2
    }
}

/// What the model last answered, and how long its answers take: what a
/// frame's picture can wait for ([`Answered::worth_waiting`]).
///
/// A frame's picture was made as soon as the frame was there, with the
/// depth map of the frame before, or of the one before that; when the
/// frame's own map came, a second picture of the same frame was made with
/// it, unless the next frame was about to come. Measured from outside on
/// the Odyssey's own screen with a film of 24 pictures a second (2160
/// lines, *Finest*, TensorRT, 50 seconds): 1199 frames made 1366 pictures,
/// one to nine frames in every second shown twice, 14 ms apart, with two
/// depth maps, and the others once with a map now of their own frame and
/// now of the one before. What moves in the film stands at another depth
/// from picture to picture, and its edges with it. Since the model answers
/// in a few milliseconds (`depth::Fast`), a frame's picture waits for the
/// frame's own map and is made once.
#[derive(Default)]
struct Answered {
    /// The frame the model answered last, with a map or without.
    serial: u64,
    /// How long its answers have been taking, the last ones counting most.
    took: Option<Duration>,
}

impl Answered {
    /// A screen that is no film waits only for maps as quick as this: a
    /// pointer or a game is not to be later by more.
    const AT_ONCE: Duration = Duration::from_millis(4);
    /// What a picture takes to make after its wait, and a little: that
    /// much of a film's time from frame to frame is never waited away.
    const PICTURE: Duration = Duration::from_millis(14);

    fn note(&mut self, serial: u64, took: Duration) {
        self.serial = serial;
        self.took = Some(self.took.map_or(took, |before| (before * 3 + took) / 4));
    }

    /// How long the picture of a frame the model has just been given is
    /// worth holding back for the frame's own map. `film` is the time
    /// from frame to frame of a screen that shows a film or a video
    /// ([`Pace::calm`]).
    ///
    /// A film's picture waits where the maps have been taking half that
    /// time or less: for one and a half times what they took and 2 ms,
    /// and never so long that the picture would not be made before the
    /// next frame. Measured on the Odyssey's own screen (`A2O_LIVE_TIMES`,
    /// 1726 frames of a film of 24 a second, 2160 lines, *Finest*,
    /// TensorRT): a map took 15.0 ms in the middle, 16.1 ms for nine of
    /// ten and 19.2 ms at the most, two and a half times what the model
    /// takes alone, the GPU also weaving the picture; every frame's
    /// picture was written 12.6 ms after the frame came, with the map of
    /// the frame before. (A first rule waited only for maps of 12 ms and
    /// less, and never waited once.)
    ///
    /// Any other screen waits only for maps that are there at once.
    /// Nothing before the model has answered (its first answer takes
    /// seconds), and nothing where it is slow: a processor's model.
    fn worth_waiting(&self, film: Option<Duration>) -> Option<Duration> {
        let took = self.took?;
        match film {
            Some(gap) => {
                let longest = gap.checked_sub(Answered::PICTURE)?;
                (took * 2 <= gap).then(|| (took * 3 / 2 + Duration::from_millis(2)).min(longest))
            }
            None => (took <= Answered::AT_ONCE).then(|| took * 2 + Duration::from_millis(2)),
        }
    }
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
    /// Which frame the model answered last, for a picture that waits for
    /// its frame's own map.
    answered: Mutex<Answered>,
    map_answered: Condvar,
    stats: Mutex<Stats>,
    /// The screen's frames, for the depth thread to say that a map is
    /// there ([`Frames::look_again`]); none where nothing waits for them.
    frames: Mutex<Option<Frames>>,
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
            answered: Mutex::new(Answered::default()),
            map_answered: Condvar::new(),
            stats: Mutex::new(Stats::default()),
            frames: Mutex::new(None),
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
        *lock(&shared.frames) = Some(frames.clone());
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

/// How long before a film's next frame no picture is begun between two
/// frames: as long as a picture takes at the most lines, and a little.
const BETWEEN_FRAMES: Duration = Duration::from_millis(16);

fn render_loop(frames: &Frames, shared: &Shared) {
    let mut seen = 0u64;
    let (mut picture, mut pair, mut canvas, mut small) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut shown: Option<Shown> = None;
    // The place and size the picture was last made for, the mark of
    // that (0 is nobody's), and the mark the room here was made black
    // under.
    let mut blank = None;
    let (mut mark, mut canvas_mark) = (0u64, 0u64);
    let (mut counted, mut since) = (0u32, Instant::now());
    let mut failing = false;
    // Since when there has been no picture to make, and whether the page
    // has been told that the screen is still.
    let (mut resting, mut still) = (None::<Instant>, false);
    // When the next turn may begin.
    let mut due = Instant::now();
    // When the frame taken last arrived.
    let mut arrived = None;
    let mut pace = Pace::default();
    // The frame whose picture waited for the frame's own map, and got it.
    let mut own_map = None;
    while !shared.stop.load(Ordering::Relaxed) && !frames.ended() {
        // A little more than sixty pictures a second at most: a faster
        // screen (the Odyssey runs at 165) would double the work for depth
        // the eye cannot tell apart, on the processor and the GPU. A
        // desktop is asked to send no more than that, and keeps to it; this
        // is for a stream that does not (`next_turn`).
        if let Some(early) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(early);
        }
        // When the turn after this one may begin, if this one takes a
        // frame (below).
        let paced = next_turn(due, Instant::now());
        // A still screen sends no frames. Its last one is looked at again a
        // few times a second all the same: the depth map of a picture that
        // has just appeared arrives after it, and the controls must take
        // effect without waiting for the screen to change. Most of those
        // turns find nothing to do (`plan`).
        let patience = Duration::from_millis(150);
        let mut taken_at = None;
        let taken = frames.with_fresh(seen, arrived, FRAME_TIME, patience, |slot| {
            arrived = slot.arrived();
            taken_at = times::wanted().then(Instant::now);
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
            due = paced;
            continue;
        };
        let new_frame = found.serial != seen;
        // The pace is that of the frames taken. A turn that finds no new
        // one (nothing came for 150 ms, or a depth map has come:
        // `Frames::look_again`) leaves the next turn due when it was. It
        // was counted like any other, and the frame that came after it
        // waited out the rest of a `FRAME_TIME`: with a screen of 60
        // frames a second and a model that answers in 15 ms (audit round
        // 25), the frame after such a turn was taken 12 ms after it had
        // arrived, and the six after that 10, 8, 7, 5, 4 and 2 ms after,
        // the loop then running to its own 15 ms and not to the frames.
        if new_frame {
            due = paced;
        }
        // A frame of a film that came early is held until its time. The
        // model is given it first: its map is no later for the wait.
        let held = new_frame
            .then_some(arrived)
            .flatten()
            .map(|arrived| pace.show_at(arrived));
        seen = found.serial;
        if new_frame && for_model.is_none() && times::wanted() {
            times::note('w', found.serial, &[Some(Instant::now()), None], Some(5));
        }
        if let Some(size) = for_model {
            // How long this frame's own map is worth waiting for, by the
            // maps before it (`Answered`).
            let calm = pace.calm();
            let wait = new_frame
                .then(|| lock(&shared.answered).worth_waiting(calm))
                .flatten();
            let from = times::wanted().then(Instant::now);
            lock(&shared.for_model).give(&mut small, size, found.serial, found.detail);
            shared.model_wanted.notify_one();
            if let Some(wait) = wait {
                let until = Instant::now() + wait;
                let mut answered = lock(&shared.answered);
                while answered.serial != found.serial && !shared.stop.load(Ordering::Relaxed) {
                    let Some(left) = until.checked_duration_since(Instant::now()) else {
                        break;
                    };
                    answered = shared
                        .map_answered
                        .wait_timeout(answered, left)
                        .unwrap_or_else(|e| e.into_inner())
                        .0;
                }
                own_map = (answered.serial == found.serial).then_some(found.serial);
            }
            if new_frame && from.is_some() {
                let how = match wait {
                    Some(_) if own_map == Some(found.serial) => 1,
                    Some(_) => 2,
                    None if calm.is_some() => 3,
                    None => 4,
                };
                times::note('w', found.serial, &[from, Some(Instant::now())], Some(how));
            }
        }
        if let Some(early) = held.and_then(|at| at.checked_duration_since(Instant::now())) {
            std::thread::sleep(early);
        }
        // No picture between two frames of a film when the next is about
        // to come (`Pace::next_within`): what it would have shown is in
        // that frame's picture.
        if !new_frame && pace.next_within(Instant::now(), BETWEEN_FRAMES) {
            continue;
        }
        // Nor between two frames of a screen that changes faster than a
        // film's (`Pace::quick`).
        if !new_frame && pace.quick(Instant::now()) {
            continue;
        }
        // And none at all between two frames of a film whose picture was
        // made with its own map (`Answered`): there is nothing newer to
        // show it with, and easing that map in further would be a second
        // picture of the frame at another depth. A film that has stopped
        // is no longer awaited, and its last frame is eased in.
        if !new_frame && own_map == Some(found.serial) && pace.awaited(Instant::now()) {
            continue;
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
        if !new_frame && times::wanted() {
            let why = if turn.settle { 1 } else { 2 };
            times::note('s', found.serial, &[Some(Instant::now())], Some(why));
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
        // The mark is another for each such change, and whatever holds a
        // picture made under another mark is made black first.
        let placed = (view, x, y, w, h);
        if blank != Some(placed) {
            blank = Some(placed);
            mark += 1;
        }
        let black = |canvas: &mut [u8]| {
            canvas.fill(0);
            for alpha in canvas.as_chunks_mut::<4>().0 {
                alpha[3] = 255;
            }
        };
        // Without sharpening the views are rendered where they go. With
        // it they are rendered side by side first, and sharpened from
        // there into their places.
        if look.sharpness > 0 {
            fitted(&mut pair, picture.len() * 2);
            pair.resize(picture.len() * 2, 0);
        } else if pair.capacity() > 0 {
            pair = Vec::new();
        }
        let mut depth_shown = 0;
        let mut depth_of = 0;
        let mut render = |canvas: &mut [u8]| {
            depth_shown = {
                let depth = lock(&shared.depth);
                depth_of = depth.latest.as_ref().map_or(0, |latest| latest.0);
                let (dw, dh) = depth.smoother.size();
                let map = (!depth.smoother.values().is_empty()).then(|| Depth {
                    values: depth.smoother.values(),
                    width: dw,
                    height: dh,
                });
                if look.sharpness > 0 {
                    live::render_pair(&picture, w, h, map, params, &mut pair);
                } else {
                    live::render_placed(&picture, w, h, map, params, place, canvas);
                }
                depth.version
            };
            if look.sharpness > 0 {
                let amount = look.sharpness as f32 / 100.0;
                live::place_pair(&pair, w, h, amount, place, canvas);
            }
        };
        // The picture is made where the output reads it. Only where that
        // is no memory to draw in is it made here and handed over.
        let drawn = sink.draw_bottom_up(view.0 * 2, view.1, mark, |canvas, stale| {
            if stale {
                black(canvas);
            }
            render(canvas);
        });
        let written = match drawn {
            Ok(true) => {
                if canvas.capacity() > 0 {
                    (canvas, canvas_mark) = (Vec::new(), 0);
                }
                Ok(())
            }
            Ok(false) => {
                if canvas.len() != place.bytes() || canvas_mark != mark {
                    canvas_mark = mark;
                    fitted(&mut canvas, place.bytes());
                    canvas.clear();
                    canvas.resize(place.bytes(), 0);
                    black(&mut canvas);
                }
                render(&mut canvas);
                sink.show_bottom_up(view.0 * 2, view.1, &canvas)
            }
            Err(e) => Err(e),
        };
        if times::wanted() {
            let written = Some(Instant::now());
            times::note(
                'p',
                found.serial,
                &[arrived, taken_at, written],
                Some(depth_of),
            );
        }
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
/// that is no film between bars is told by its first row and column. The
/// bar above and the bar below are taken to be of one height where one of
/// them has subtitles, a player's controls or the pointer in it.
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
    // A bar with something in it: subtitles, a player's controls, the
    // pointer. Its rows are black from end to end only as far as that
    // reaches, and the rest of the bar would be the picture's: the range
    // would be the bar's again for as long as a subtitle stands, and the
    // film's depth would pump with every line of them. A film's two bars
    // are of one height. Where one reaches further than the other and the
    // same rows at the other edge are black in four pixels of five (two
    // long lines of subtitles leave about that), that one is a bar as
    // high, with what stands in it.
    let mostly_black = |rows: std::ops::Range<usize>| {
        let dark = pixels[rows.start * w..rows.end * w]
            .iter()
            .filter(|pixel| black(pixel))
            .count();
        dark * 5 >= rows.len() * w * 4
    };
    let (top, bottom) = match (top, bottom) {
        (top, bottom) if top > bottom && top <= h / 2 && mostly_black(h - top..h) => (top, top),
        (top, bottom) if bottom > top && bottom <= h / 2 && mostly_black(0..bottom) => {
            (bottom, bottom)
        }
        as_found => as_found,
    };
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
        let begun = times::wanted().then(Instant::now);
        let asked = Instant::now();
        // Whoever holds a picture back for this frame's map is told that
        // the model has answered, with a map or without.
        let answer = |serial: u64| {
            lock(&shared.answered).note(serial, asked.elapsed());
            shared.map_answered.notify_all();
        };
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
                if begun.is_some() {
                    times::note('d', serial, &[begun, Some(Instant::now())], None);
                }
                // Free for the next frame only now that the answer is
                // there to be seen: the render thread would otherwise send
                // the same frame once more.
                lock(&shared.for_model).busy = false;
                answer(serial);
                // The render loop is told, and does not wait for the
                // screen's next frame to draw with this map and to give
                // the model its next frame. It did wait. A film's frame
                // comes every 41.7 ms, and a map that takes longer than
                // that (48 ms at *Finest* on the Odyssey's own screen,
                // with the card also weaving) was there 6 ms after a frame
                // had come and gone unasked: the model then stood idle for
                // 35 ms until the next, and made 14 maps a second where it
                // can make 20. And a map that takes less was drawn with
                // from the next frame on, one frame late, where it is now
                // drawn with while its own frame is still on the screen.
                if let Some(frames) = lock(&shared.frames).as_ref() {
                    frames.look_again();
                }
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
                answer(serial);
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
        // And a picture that is held back for this frame's own map is
        // told that the model has answered, though with none: it is not
        // held for the whole of its wait.
        assert_eq!(lock(&shared.answered).serial, 7);
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
        // One pixel in a row that is not black, and the row is not black
        // from end to end: a bar at one edge alone ends there.
        let one_bar = grey_frame(1036, 588, |x, y| match y {
            0..76 => 16,
            _ => 40 + ((x + y) % 200) as u8,
        });
        assert_eq!(black_bars(&one_bar), (76, 0, 0, 0));
        let mut marked = one_bar.clone();
        marked.put_pixel(1000, 70, image::Rgba([25, 0, 0, 255]));
        assert_eq!(black_bars(&marked), (70, 0, 0, 0));
        // The film again with something in a bar: the pointer in the upper
        // one, two lines of subtitles in the lower one. Each is still a
        // bar as high as the other; a subtitle that comes and goes does
        // not move the picture's edge.
        let mut pointed = film.clone();
        pointed.put_pixel(1000, 70, image::Rgba([255, 255, 255, 255]));
        assert_eq!(black_bars(&pointed), (76, 76, 0, 0));
        let subtitled = grey_frame(1036, 588, |x, y| match y {
            76..512 => 40 + ((x + y) % 200) as u8,
            525..540 | 548..563 if (200..840).contains(&x) && x % 3 == 0 => 235,
            _ => 16,
        });
        assert_eq!(black_bars(&subtitled), (76, 76, 0, 0));
        // With something in both bars at once, both are as high as the
        // one that is black from end to end for longer.
        let mut both = subtitled.clone();
        both.put_pixel(1000, 70, image::Rgba([255, 255, 255, 255]));
        assert_eq!(black_bars(&both), (70, 70, 0, 0));
        // A bar above a picture whose lower rows are merely dark is one
        // bar: three pixels of ten are lit there.
        let dark_below = grey_frame(1036, 588, |x, y| match y {
            0..76 => 16,
            512.. if x % 10 < 7 => 10,
            _ => 40 + ((x + y) % 200) as u8,
        });
        assert_eq!(black_bars(&dark_below), (76, 0, 0, 0));
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
        let fresh = crate::capture::fresh_from(FRAME_TIME, micros(every));
        for turn in 0..turns {
            // The loop sleeps until the turn is due...
            now = now.max(due);
            due = next_turn(due, now);
            // ...lets the frames pass that come too soon after the one it
            // took last, where they come faster than turns
            // (`Frames::with_fresh`; a frame always comes in time here)...
            if let (Some(seen), Some((from, _))) = (seen, fresh) {
                let from = (micros(seen * every) + from).as_micros() as u64;
                now = now.max(start + micros(from.div_ceil(every) * every));
            }
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
        // Every third frame is taken, as it arrives: 55 pictures a second,
        // each as far from the one before as the others. Taken as they
        // lay waiting, they were two and three frames apart in turns.
        for (index, pair) in taken.windows(2).enumerate() {
            let late = [100, 200].contains(&index);
            assert!(late || pair[1].1 - pair[0].1 == 3, "turn {index}: {pair:?}");
            assert!(
                late || pair[1].2 == Duration::ZERO,
                "turn {index}: {pair:?}"
            );
        }
        // Frames are left out, never taken twice or out of order.
        assert!(taken.windows(2).all(|w| w[1].1 > w[0].1));
    }

    #[test]
    fn of_a_screen_that_changes_at_every_refresh_every_second_frame_is_taken() {
        // What a desktop sends of a screen of 120 Hz now that it is asked
        // for more than 62 a second, with the frames a little early and a
        // little late by turns. With a frame taken as it lay waiting when
        // a turn was due, every ninth picture was one frame after the one
        // before it and the others two.
        for every in [8_333, 8_342, 8_000] {
            let taken = paced(every, 5_000, 300, &[(50, 30_000)]);
            for (index, pair) in taken.windows(2).enumerate() {
                let late = index == 50;
                assert!(
                    late || pair[1].1 - pair[0].1 == 2,
                    "{every}, {index}: {pair:?}"
                );
                assert!(
                    late || pair[1].2 == Duration::ZERO,
                    "{every}, {index}: {pair:?}"
                );
            }
        }
    }

    /// The times `Pace` shows frames at that arrive `gaps` microseconds
    /// after one another, as the gaps between them and how long the
    /// longest was held, in microseconds.
    fn paced_gaps(gaps: &[u64]) -> (Vec<u64>, u64) {
        let start = Instant::now();
        let mut pace = Pace::default();
        let (mut arrived, mut shown, mut longest) = (start, Vec::new(), 0);
        for gap in std::iter::once(&0).chain(gaps) {
            arrived += Duration::from_micros(*gap);
            let at = pace.show_at(arrived);
            assert!(at >= arrived, "a frame cannot be shown before it is there");
            longest = longest.max((at - arrived).as_micros() as u64);
            shown.push(at);
        }
        let gaps = shown.windows(2).map(|w| (w[1] - w[0]).as_micros() as u64);
        (gaps.collect(), longest)
    }

    #[test]
    fn a_film_whose_frames_come_early_and_late_by_turns_is_shown_evenly() {
        // 24 pictures a second on a screen of 120 Hz, the browser a
        // refresh early and a refresh late by turns: four refreshes, then
        // six.
        let uneven: Vec<u64> = (0..60)
            .map(|i| if i % 2 == 0 { 33_360 } else { 50_040 })
            .collect();
        let (gaps, longest) = paced_gaps(&uneven);
        // Once a run of them has been seen, each is shown a film's frame
        // after the one before, give or take a millisecond.
        for (index, gap) in gaps.iter().enumerate().skip(Pace::RUN + 2) {
            assert!((40_500..=42_900).contains(gap), "frame {index}: {gap}");
        }
        // The early ones are held for a refresh, no frame for longer.
        assert!((7_000..=9_000).contains(&longest), "{longest}");
        // A film that comes evenly is shown as it comes.
        let (gaps, longest) = paced_gaps(&[41_700; 40]);
        assert!(gaps.iter().all(|gap| *gap == 41_700), "{gaps:?}");
        assert_eq!(longest, 0);
    }

    #[test]
    fn no_picture_is_begun_between_two_frames_when_a_films_next_frame_is_about_to_come() {
        let start = Instant::now();
        let ms = Duration::from_millis;
        let mut pace = Pace::default();
        // Nothing is known of a run yet.
        assert!(!pace.next_within(start, ms(16)));
        let mut arrived = start;
        for _ in 0..Pace::RUN + 2 {
            arrived += ms(42);
            pace.show_at(arrived);
        }
        // The next frame is to come 42 ms after the last, or up to 9 ms
        // sooner. Right after a frame there is time for a picture; from 16
        // ms before the earliest the next may come there is not; and when
        // the frame has stayed away for a while (the film was paused)
        // there is again.
        assert!(!pace.next_within(arrived + ms(2), ms(16)));
        assert!(!pace.next_within(arrived + ms(15), ms(16)));
        assert!(pace.next_within(arrived + ms(18), ms(16)));
        assert!(pace.next_within(arrived + ms(41), ms(16)));
        assert!(pace.next_within(arrived + ms(55), ms(16)));
        assert!(!pace.next_within(arrived + ms(70), ms(16)));
        // A frame that is of no steady run ends it.
        pace.show_at(arrived + ms(400));
        assert!(!pace.next_within(arrived + ms(430), ms(16)));
    }

    #[test]
    fn what_is_no_steady_run_of_a_films_frames_is_shown_as_it_comes() {
        // A video of 60 a second, a screen that changes at every refresh,
        // a desktop that changes now and then, and a film that has just
        // begun or just been paused.
        let desktop = [
            5_000, 300_000, 16_000, 16_000, 900_000, 41_000, 42_000, 8_000, 41_000,
        ];
        let begun = [41_700; 7];
        let paused: Vec<u64> = [41_700; 12]
            .into_iter()
            .chain([2_000_000, 20_000])
            .collect();
        for gaps in [&[16_667; 40][..], &[8_333; 40], &desktop, &begun] {
            let (shown, longest) = paced_gaps(gaps);
            assert_eq!((shown.as_slice(), longest), (gaps, 0));
        }
        let (shown, longest) = paced_gaps(&paused);
        assert_eq!((shown, longest), (paused, 0));
        // An uneven run that does not hold: nothing is held for longer
        // than the limit, and what is held is caught up with.
        let rough: Vec<u64> = (0..80).map(|i| 30_000 + (i * 7919 % 23) * 1_000).collect();
        let (shown, longest) = paced_gaps(&rough);
        assert!(longest <= Pace::HOLD.as_micros() as u64);
        let (came, went): (u64, u64) = (rough.iter().sum(), shown.iter().sum());
        assert!(went >= came && went - came <= Pace::HOLD.as_micros() as u64);
    }

    #[test]
    fn frames_that_come_no_faster_than_pictures_are_made_are_each_taken() {
        // 60 a second and the 62 to 64 of a desktop that keeps to an older
        // limit: nothing is let pass.
        for every in [16_667, 16_129, 15_625] {
            let spacing = Duration::from_micros(every);
            assert_eq!(crate::capture::fresh_from(FRAME_TIME, spacing), None);
        }
        // A screen of 120 Hz: the frame 8.3 ms after the one taken last is
        // let pass, the one 16.7 ms after it is taken, and when nothing
        // has come 23.3 ms after it the wait is over.
        let (from, until) =
            crate::capture::fresh_from(FRAME_TIME, Duration::from_micros(8_333)).unwrap();
        assert!(Duration::from_micros(8_333) < from && from < Duration::from_micros(16_000));
        assert_eq!(until, FRAME_TIME + Duration::from_micros(8_333));
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
    #[cfg(target_os = "linux")]
    fn a_line_of_the_times_is_left_and_never_waited_for() {
        use std::io::BufRead;
        let dir = tempfile::tempdir().unwrap();
        // A file: every line, in the order they were left.
        let path = dir.path().join("times.txt");
        let lines = times::Lines::to(path.clone()).unwrap();
        for line in 0..1000 {
            lines.leave(format!("p {line}"));
        }
        let wanted: Vec<String> = (0..1000).map(|line| format!("p {line}")).collect();
        eventually("the lines are in the file", || {
            std::fs::read_to_string(&path).is_ok_and(|text| text.lines().eq(wanted.iter()))
        });
        // A pipe that nobody reads. The thread that leaves a line there
        // is not kept (it stood still in the opening of the file, and
        // with it the render loop and the screen's newest frame); what
        // finds no room is counted.
        let pipe = dir.path().join("pipe");
        let made = std::process::Command::new("mkfifo").arg(&pipe).status();
        if !made.is_ok_and(|status| status.success()) {
            eprintln!("skipped in part: no mkfifo to make a pipe with");
            return;
        }
        let lines = times::Lines::to(pipe.clone()).unwrap();
        let began = Instant::now();
        for line in 0..times::Lines::ROOM + 500 {
            lines.leave(format!("p {line}"));
        }
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "{:?}",
            began.elapsed()
        );
        // A reader comes: it gets the lines that found room, and before
        // the next line that does, how many did not.
        let (read, first) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            let mut pipe = std::io::BufReader::new(std::fs::File::open(pipe).unwrap()).lines();
            let mut take = |lines: usize| -> Vec<String> {
                pipe.by_ref().take(lines).map(Result::unwrap).collect()
            };
            read.send(take(times::Lines::ROOM)).unwrap();
            take(2)
        });
        let first = first.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(first.iter().eq((0..times::Lines::ROOM)
            .map(|line| format!("p {line}"))
            .collect::<Vec<_>>()
            .iter()));
        lines.leave("p last".into());
        assert_eq!(reader.join().unwrap(), ["x 500", "p last"]);
    }

    #[test]
    fn no_picture_is_begun_between_two_frames_of_a_screen_that_changes_faster_than_a_film() {
        let ms = Duration::from_millis;
        let start = Instant::now();
        let came = |gaps: &[u64]| {
            let mut pace = Pace::default();
            let mut arrived = start;
            pace.show_at(arrived);
            for gap in gaps {
                arrived += Duration::from_micros(*gap);
                pace.show_at(arrived);
            }
            (pace, arrived)
        };
        // Nothing is known before three frames have followed a first.
        assert!(!Pace::default().quick(start));
        assert!(!came(&[16_667, 16_667]).0.quick(start + ms(40)));
        // 60 a second, every second frame of a screen of 120 Hz and of
        // one of 90, a game between 80 and 140: the next frame is about
        // to come from the moment one has, and for as long after its time
        // as such frames are late by.
        // And a video of 60, 50 or 48 a second whose frames come a
        // refresh early and a refresh late by turns on a screen of 120 Hz.
        for gaps in [
            [16_667; 3],
            [16_000, 16_000, 24_000],
            [22_222; 3],
            [7_100, 12_500, 9_000],
            [8_333, 25_000, 8_333],
            [25_000, 8_333, 25_000],
            [16_667, 25_000, 16_667],
        ] {
            let (pace, last) = came(&gaps);
            let longest = Duration::from_micros(*gaps.iter().max().unwrap());
            assert!(pace.quick(last), "{gaps:?}");
            assert!(pace.quick(last + longest + Pace::HOLD), "{gaps:?}");
            // The screen has stopped changing: its last frame gets the
            // map that comes for it.
            assert!(
                !pace.quick(last + longest + Pace::HOLD * 2 + ms(1)),
                "{gaps:?}"
            );
        }
        // A film's frames, a video of 30 a second and a desktop that
        // changes now and then are not such a screen: theirs is the rule
        // of the steady run, or none.
        for gaps in [
            [41_700; 3],
            [33_333; 3],
            [25_000; 3],
            [16_667, 16_667, 300_000],
            [8_333, 8_333, 60_000],
        ] {
            let (pace, last) = came(&gaps);
            assert!(!pace.quick(last + ms(1)), "{gaps:?}");
        }
        // And the first frames after a pause are not yet one.
        let (pace, last) = came(&[16_667, 16_667, 16_667, 900_000, 16_667, 16_667]);
        assert!(!pace.quick(last + ms(1)));
    }

    #[test]
    #[cfg(target_os = "linux")]
    #[ignore = "a matter of milliseconds: run alone"]
    fn a_turn_that_finds_no_new_frame_does_not_keep_the_next_frame_waiting() {
        // Like the film's test below it is a matter of milliseconds, 12
        // of them here: with the other tests running beside it on every
        // core the frames were taken anything from 3 to 22 ms after they
        // came, whatever the loop did. So it is run alone:
        // `cargo test -p any2odyssey a_turn_that_finds -- --ignored`
        // No output: a turn is over as soon as its frame has been given
        // to the model. The test plays the model.
        let capture = crate::capture::Fed::start();
        let shared = Arc::new(Shared::new());
        shared.lines.store(360, Ordering::Relaxed);
        let (frames, state) = (capture.frames(), shared.clone());
        let worker = std::thread::spawn(move || render_loop(&frames, &state));
        let waker = capture.frames();
        // Feed a frame and say how long it took to be taken (the turn
        // that takes a frame gives it to the model, which is free).
        let taken_after = |serial: u64| {
            lock(&shared.for_model).busy = false;
            let fed = Instant::now();
            capture.feed(16, 9, &[serial as u8; 16 * 9 * 4]);
            while lock(&shared.for_model).serial != serial {
                assert!(fed.elapsed() < Duration::from_secs(10), "never taken");
                std::thread::sleep(Duration::from_micros(100));
            }
            fed.elapsed()
        };
        let mut waited = Vec::new();
        for round in 0..15u64 {
            // Two frames come one right after the other and are taken
            // (the second of them is what the next turn is due after);
            // then a depth map comes (the loop is told to look again, and
            // finds nothing to do); and 3 ms later the screen's next
            // frame.
            std::thread::sleep(Duration::from_millis(40));
            taken_after(round * 3 + 1);
            taken_after(round * 3 + 2);
            waker.look_again();
            std::thread::sleep(Duration::from_millis(3));
            waited.push(taken_after(round * 3 + 3));
        }
        shared.stop.store(true, Ordering::Relaxed);
        drop(capture);
        worker.join().unwrap();
        // It is taken as it comes. With the turn in between counted
        // against the pace, it was taken a `FRAME_TIME` after that turn
        // was due: 12 ms after it came.
        waited.sort();
        assert!(
            waited[waited.len() / 2] < Duration::from_millis(6),
            "{waited:?}"
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn the_picture_is_whole_after_every_change_of_lines_screen_and_sharpening() {
        use any2odyssey_core::live::{PixelOrder, Source};
        // In memory, where the picture is drawn in its place in the file
        // and what the place held is three pictures old, and on a disk,
        // where it is made in the loop's own room and written.
        let memory = tempfile::tempdir_in("/dev/shm");
        if memory.is_err() {
            eprintln!("skipped in part: no /dev/shm to make a frame file in");
        }
        for dir in memory.into_iter().chain(tempfile::tempdir()) {
            let sink = FrameSink::in_folder(dir.path()).unwrap();
            let capture = crate::capture::Fed::start();
            let shared = Arc::new(Shared::new());
            *lock(&shared.sink) = (1, Some(sink.clone()));
            let (frames, state) = (capture.frames(), shared.clone());
            let worker = std::thread::spawn(move || render_loop(&frames, &state));
            // Screens of the panel's shape, wider (21:9, 32:9), upright
            // and 4:3; each change is made twice over and then stays for
            // three pictures more, so that every place of the file has
            // held a picture of another size, place or kind before.
            let screens = [(64u32, 36u32), (84, 36), (36, 64), (128, 36), (40, 30)];
            let mut turn = 0u32;
            for step in 0..24u32 {
                let (sw, sh) = screens[(step / 2 % 5) as usize];
                let lines = [360, 480, 540][(step / 3 % 3) as usize];
                let sharpness = [0, 50][(step / 4 % 2) as usize];
                let swap = step / 5 % 2 == 1;
                shared.lines.store(lines, Ordering::Relaxed);
                shared.sharpness.store(sharpness, Ordering::Relaxed);
                shared.swap.store(swap, Ordering::Relaxed);
                for _ in 0..if step % 2 == 0 { 1 } else { 4 } {
                    turn += 1;
                    // Nothing of it is black, and no two frames are alike.
                    let screen: Vec<u8> = (0..sw * sh)
                        .flat_map(|i| [(i % 199) as u8 | 1, (i % 83 + turn) as u8 | 1, 255, 255])
                        .collect();
                    capture.feed(sw, sh, &screen);
                    let placed = layout((sw, sh), lines);
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
                    let look = shared.find(0).0.look;
                    let wanted = expected_picture(&picture, placed, None, look);
                    eventually("the picture of the frame", || {
                        let (_, width, height, bytes) = sink.written();
                        (width, height) == (view.0 * 2, view.1) && bytes == wanted
                    });
                }
            }
            shared.stop.store(true, Ordering::Relaxed);
            drop(capture);
            worker.join().unwrap();
        }
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
    fn a_picture_waits_for_its_frames_own_map_only_where_the_model_is_quick() {
        let ms = Duration::from_millis;
        let us = Duration::from_micros;
        let film = Some(us(41_700));
        let mut answered = Answered::default();
        // Before the model has answered once: its first answer takes
        // seconds, and nothing is known of the next.
        assert_eq!(answered.worth_waiting(film), None);
        // As measured at the finest detail with TensorRT: 15 ms a map, 19
        // at the most. One and a half times that and 2 ms covers it.
        answered.note(1, ms(15));
        assert_eq!(answered.worth_waiting(film), Some(us(24_500)));
        // A video of 30 a second leaves less: its picture is still made
        // before the next frame.
        assert_eq!(answered.worth_waiting(Some(us(33_300))), Some(us(19_300)));
        // 60 a second is no film, and a desktop or a game is not made
        // later by a map that takes 15 ms...
        assert_eq!(answered.worth_waiting(None), None);
        // ...only by one that is there almost at once.
        let mut at_once = Answered::default();
        at_once.note(1, ms(1));
        assert_eq!(at_once.worth_waiting(None), Some(ms(4)));
        // The CUDA provider at the finest detail with the GPU also
        // weaving (40 ms was measured), or a processor: a film's picture
        // is made as before, with the map there is.
        let mut slow = Answered::default();
        slow.note(1, ms(40));
        assert_eq!(slow.worth_waiting(film), None);
        // One slow answer among quick ones (the GPU busy for a moment)
        // does not end the waiting, and several do.
        answered.note(2, ms(35));
        assert_eq!(answered.took, Some(ms(20)));
        assert!(answered.worth_waiting(film).is_some());
        answered.note(3, ms(35));
        assert_eq!(answered.worth_waiting(film), None);
        assert_eq!(answered.serial, 3);
    }

    /// Feed a film of `frames` frames, 40 ms apart, to the render loop and
    /// to a model that takes `model` for a map, and say how many pictures
    /// were written for the last `counted` of them.
    #[cfg(target_os = "linux")]
    fn pictures_of_a_film(frames: u32, counted: u32, model: Duration) -> u32 {
        let dir = tempfile::tempdir().unwrap();
        let sink = FrameSink::in_folder(dir.path()).unwrap();
        let capture = crate::capture::Fed::start();
        let shared = Arc::new(Shared::new());
        shared.lines.store(720, Ordering::Relaxed);
        *lock(&shared.sink) = (1, Some(sink.clone()));
        *lock(&shared.frames) = Some(capture.frames());
        let (source, state) = (capture.frames(), shared.clone());
        let render = std::thread::spawn(move || render_loop(&source, &state));
        let state = shared.clone();
        let depth = std::thread::spawn(move || {
            depth_loop_with(&state, |frame, detail| {
                std::thread::sleep(model);
                let (w, h) = crate::depth::model_size(frame.width(), frame.height(), detail);
                // Another map for every frame, as a film's are.
                let near = f32::from(frame.get_pixel(0, 0).0[0]);
                let values = (0..w * h).map(|i| (i % w) as f32 + near).collect();
                Ok(RawDepth::new(w, h, values).ok())
            })
        });
        let began = Instant::now();
        let mut before = 0;
        for frame in 0..frames {
            if let Some(early) = (Duration::from_millis(40) * frame).checked_sub(began.elapsed()) {
                std::thread::sleep(early);
            }
            if frame == frames - counted {
                before = sink.written().0;
            }
            let screen: Vec<u8> = (0..320 * 180)
                .flat_map(|i| [frame as u8, (i % 251) as u8, (i % 7) as u8, 255])
                .collect();
            capture.feed(320, 180, &screen);
        }
        // What the last frame is still shown with, before a film that has
        // stopped is eased in.
        std::thread::sleep(Duration::from_millis(35));
        let written = sink.written().0 - before;
        shared.stop.store(true, Ordering::Relaxed);
        shared.model_wanted.notify_all();
        render.join().unwrap();
        depth.join().unwrap();
        written
    }

    #[test]
    #[cfg(target_os = "linux")]
    #[ignore = "a matter of milliseconds: run alone"]
    fn a_films_frame_is_shown_once_with_its_own_map_where_the_model_is_quick() {
        // Twenty frames once the run is told for a film and the model for
        // quick. Each made a second picture, with its map eased in a step
        // further or with the map that came after the first, some 14 ms
        // after its first: a frame at two depths, and forty pictures here.
        // It is a matter of milliseconds: with the other tests running
        // beside it on every core, frames are missed and runs end, and
        // anything from 13 to 25 pictures was counted. So it is run alone:
        // `cargo test -p any2odyssey a_films_frame -- --ignored`
        // A map takes 15 ms, as measured on the Odyssey's own screen.
        let written = pictures_of_a_film(36, 20, Duration::from_millis(15));
        assert!(
            (20..=22).contains(&written),
            "{written} pictures for 20 frames"
        );
    }

    #[test]
    fn a_film_is_awaited_until_its_next_frame_is_overdue() {
        let ms = Duration::from_millis;
        let mut pace = Pace::default();
        let start = Instant::now();
        // Nothing has come: nothing is awaited.
        assert!(!pace.awaited(start));
        assert_eq!(pace.calm(), None);
        // A browser's film: now four refreshes, now six, and five.
        let mut arrived = start;
        for gap in [42, 33, 50, 42] {
            assert_eq!(
                pace.calm(),
                None,
                "three times from frame to frame are needed"
            );
            arrived += ms(gap);
            pace.show_at(arrived);
        }
        pace.show_at(arrived + ms(42));
        arrived += ms(42);
        assert_eq!(pace.calm(), Some(ms(50)));
        // Between two frames, and for as long after the next was due as
        // such frames are late by; a film that has stopped is not.
        assert!(pace.awaited(arrived + ms(14)));
        assert!(pace.awaited(arrived + ms(50) + Pace::HOLD));
        assert!(!pace.awaited(arrived + ms(50) + Pace::HOLD * 2 + ms(1)));
        // One frame out of turn ends the steady run for eight frames; the
        // waiting only until three have come in turn again.
        arrived += ms(92);
        pace.show_at(arrived);
        assert_eq!(pace.calm(), None);
        for _ in 0..3 {
            arrived += ms(42);
            pace.show_at(arrived);
        }
        assert_eq!(pace.calm(), Some(ms(42)));
        assert!(pace.next.is_none(), "no steady run yet");
        // A desktop's or a game's 60 a second is never made to wait so.
        let mut quick = Pace::default();
        for frame in 0..12 {
            quick.show_at(start + ms(16) * frame);
        }
        assert_eq!(quick.calm(), None);
        assert!(!quick.awaited(start + ms(16) * 11 + ms(5)));
        // A video of 30 a second on a screen of 120 Hz, a refresh early
        // and late by turns: three refreshes and five, the three a hair
        // short as often as not. It is a video's all the same.
        let us = Duration::from_micros;
        let mut video = Pace::default();
        let mut arrived = start;
        for gap in [33_333, 24_930, 41_740, 24_960, 41_700] {
            arrived += us(gap);
            video.show_at(arrived);
        }
        assert_eq!(video.calm(), Some(us(41_740)));
        // 48 a second there comes two refreshes and three by turns, and
        // is none.
        let mut faster = Pace::default();
        let mut arrived = start;
        for gap in [16_667, 25_000, 16_667, 25_000, 16_667] {
            arrived += us(gap);
            faster.show_at(arrived);
        }
        assert_eq!(faster.calm(), None);
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
                // Where TensorRT makes its form of the model for this
                // size, the maps are timed once it has taken over.
                let making = Instant::now();
                while crate::depth::runs_on() == Some(crate::depth::RunsOn::Preparing)
                    && making.elapsed() < Duration::from_secs(180)
                {
                    estimate();
                }
                let making = making.elapsed();
                let start = Instant::now();
                for _ in 0..20 {
                    estimate();
                }
                println!(
                    "{sw}x{sh} depth {name} ({mw}x{mh}): first {first:?}, then {:?} each ({}; waited {making:.1?} for it)",
                    start.elapsed() / 20,
                    crate::depth::runs_on().map_or_else(String::new, |on| on.for_log()),
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
        // `copied`: as it was done before, the picture made here and
        // copied into the file.
        for (lines, sharpness, copied) in [1080, 1440, 2160]
            .into_iter()
            .flat_map(|lines| [(0, false), (50, false), (0, true)].map(|(s, c)| (lines, s, c)))
        {
            {
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
                    let (mut t2, mut t3) = (t1, t1);
                    // As the render loop does it: the picture is made in
                    // its place in the file, and only where that cannot be
                    // is it made here and handed over.
                    let mut make = |canvas: &mut [u8]| {
                        if sharpness > 0 {
                            live::render_pair(&picture, w, h, depth, params, &mut pair);
                        } else {
                            live::render_placed(&picture, w, h, depth, params, place, canvas);
                        }
                        t2 = Instant::now();
                        if sharpness > 0 {
                            let amount = sharpness as f32 / 100.0;
                            live::place_pair(&pair, w, h, amount, place, canvas);
                        }
                        t3 = Instant::now();
                    };
                    let direct = !copied
                        && sink
                            .draw_bottom_up(view.0 * 2, view.1, 1, |canvas, _| make(canvas))
                            .unwrap();
                    if !direct {
                        make(&mut canvas);
                        sink.show_bottom_up(view.0 * 2, view.1, &canvas).unwrap();
                    }
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
                    "{lines} lines, sharpness {sharpness}{}: scale {:.2} ms, render {:.2}, sharpen {:.2}, hand over {:.2}; a turn {turn:.2} ms ({:.0} a second)",
                    if copied { ", copied" } else { "" },
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
