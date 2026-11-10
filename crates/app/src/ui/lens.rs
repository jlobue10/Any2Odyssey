//! Lens-interleaved output for the stereo viewer: the three numbers that
//! describe the Odyssey's lens sheet, the controls and keys that tune them
//! against a calibration pattern, and the interleaved picture for the output
//! window.
//!
//! The numbers are the user's own, kept in `lens.json`. Nothing here reads
//! Samsung's files or switches the panel's lens on.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use any2odyssey_core::fsutil;
use any2odyssey_core::theme::Palette;
use any2odyssey_core::weave::{self, Lens, PITCH_RANGE, SLANT_RANGE};
use egui::{Key, RichText, Ui};
use image::RgbaImage;

use super::theme::color;
use super::widgets::{gold_button, row};

pub const LENS_NOTICE: &str = "Experimental. The picture only separates when the Odyssey's lens is switched on, which this build cannot do yet, and only full screen on the Odyssey at its native resolution without display scaling. There is no eye tracking: it is right for one head position.";
pub const PATTERN_HELP: &str = "Pattern: white for the left eye, black for the right. Right eye closed should look evenly bright, left eye closed evenly dark. Bands: change pitch, then slant. Wrong eye bright: move the offset by 0.5.";
pub const KEYS_HELP: &str =
    "←/→ offset   ↑/↓ pitch   PgUp/PgDn slant   Shift = fine   C pattern   F11 full screen   Esc leave";
/// How long the numbers must have been different from the file before they
/// are written, so a drag or a held key is one write, not hundreds.
const SAVE_DELAY: Duration = Duration::from_secs(1);

/// One of the numbers the keys in the output window change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Knob {
    Pitch,
    Slant,
    Offset,
}

impl Knob {
    /// The size of one key press; Shift takes a tenth of it.
    fn step(self) -> f64 {
        match self {
            Knob::Pitch | Knob::Slant => 0.001,
            Knob::Offset => 0.01,
        }
    }
}

/// What the interleaved texture was made from; any difference remakes it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Woven {
    generation: u64,
    lens: Lens,
    pattern: bool,
    size: [u32; 2],
    origin: (i64, i64),
    stretch: f64,
}

#[derive(Default)]
pub struct LensState {
    pub lens: Lens,
    /// Show the calibration pattern instead of the image.
    pub pattern: bool,
    /// What `file` holds, as far as this process knows.
    saved: Lens,
    dirty_since: Option<Instant>,
    /// Where the numbers are kept. None keeps them in memory only (tests).
    file: Option<PathBuf>,
    /// Why the file could not be read or written, for the page to show.
    pub problem: Option<String>,
    texture: Option<egui::TextureHandle>,
    woven: Option<Woven>,
    /// The size asked for on the previous frame: a window being dragged to a
    /// new size is not interleaved again until it has stopped changing.
    last_size: [u32; 2],
}

/// Read the lens numbers from `path`. A missing file is the default lens.
pub fn read_lens(path: &Path) -> Result<Lens, String> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Lens::default()),
        Err(e) => return Err(format!("Could not read {}: {e}", path.display())),
    };
    serde_json::from_slice::<Lens>(fsutil::strip_utf8_bom(&bytes))
        .map(Lens::sanitized)
        .map_err(|e| format!("{} is not a lens file: {e}", path.display()))
}

impl LensState {
    /// The state backed by `path`: its numbers now, and later changes saved
    /// back to it. A file that cannot be used is left alone until the user
    /// changes a number.
    pub fn load(path: PathBuf) -> LensState {
        let (lens, problem) = match read_lens(&path) {
            Ok(lens) => (lens, None),
            Err(e) => (Lens::default(), Some(e)),
        };
        LensState {
            lens,
            saved: lens,
            file: Some(path),
            problem,
            ..LensState::default()
        }
    }

    /// Move one number by `steps` key presses.
    pub fn nudge(&mut self, knob: Knob, steps: f64) {
        let delta = knob.step() * steps;
        match knob {
            Knob::Pitch => self.lens.pitch += delta,
            Knob::Slant => self.lens.slant += delta,
            Knob::Offset => self.lens.offset += delta,
        }
        self.lens = self.lens.sanitized();
    }

    /// Write the numbers now if they differ from the file.
    pub fn store(&mut self) {
        self.dirty_since = None;
        if self.lens == self.saved {
            return;
        }
        let Some(file) = &self.file else {
            self.saved = self.lens;
            return;
        };
        let written = file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| fsutil::write_json_atomic(file, &self.lens));
        match written {
            Ok(()) => {
                self.saved = self.lens;
                self.problem = None;
            }
            Err(e) => {
                // Not retried every frame: the next change tries again.
                self.saved = self.lens;
                self.problem = Some(format!("Could not save {}: {e}", file.display()));
            }
        }
    }

    /// Save changed numbers once they have been still for [`SAVE_DELAY`].
    /// Call once per frame.
    pub fn save_when_settled(&mut self, ctx: &egui::Context, now: Instant) {
        if self.lens == self.saved {
            self.dirty_since = None;
            return;
        }
        match self.dirty_since {
            None => {
                self.dirty_since = Some(now);
                ctx.request_repaint_after(SAVE_DELAY);
            }
            Some(since) if now.duration_since(since) >= SAVE_DELAY => self.store(),
            Some(since) => ctx.request_repaint_after(SAVE_DELAY - now.duration_since(since)),
        }
    }

    /// The controls shown in the viewer's output panel.
    pub fn controls(&mut self, ui: &mut Ui, p: &Palette) {
        ui.add(
            egui::Label::new(RichText::new(LENS_NOTICE).color(color(p.muted)).size(12.0)).wrap(),
        );
        let mut lens = self.lens;
        row(ui, "Lens pitch", |ui| {
            ui.add(
                egui::DragValue::new(&mut lens.pitch)
                    .speed(0.0005)
                    .range(PITCH_RANGE.0..=PITCH_RANGE.1)
                    .max_decimals(9)
                    .min_decimals(4),
            );
            ui.label(RichText::new("pixels per lens").color(color(p.muted)));
        })
        .on_hover_text("The lens period along a row of pixels. Wrong by a little: broad bands across the pattern. Wrong by a lot: fine stripes.");
        row(ui, "Slant", |ui| {
            ui.add(
                egui::DragValue::new(&mut lens.slant)
                    .speed(0.0005)
                    .range(SLANT_RANGE.0..=SLANT_RANGE.1)
                    .max_decimals(9)
                    .min_decimals(4),
            );
            ui.label(RichText::new("pixels per row").color(color(p.muted)));
        })
        .on_hover_text("How far the lenses move sideways from one row to the next. Wrong: the bands tilt. Negative when the lenses lean the other way.");
        row(ui, "Offset", |ui| {
            ui.add(
                egui::DragValue::new(&mut lens.offset)
                    .speed(0.002)
                    .max_decimals(4)
                    .min_decimals(3),
            );
            ui.label(RichText::new("of a lens (0–1)").color(color(p.muted)));
        })
        .on_hover_text(
            "Moves the place where the picture is right from side to side. 0.5 exchanges the eyes.",
        );
        ui.checkbox(&mut lens.bgr, "Subpixels run blue-green-red");
        self.lens = lens.sanitized();
        ui.checkbox(&mut self.pattern, "Show the calibration pattern");
        ui.add(
            egui::Label::new(RichText::new(PATTERN_HELP).color(color(p.muted)).size(12.0)).wrap(),
        );
        ui.add(
            egui::Label::new(
                RichText::new(format!("In the output window:  {KEYS_HELP}"))
                    .color(color(p.muted))
                    .size(12.0),
            )
            .wrap(),
        );
        if gold_button(ui, "Reset lens numbers", false, p).clicked() {
            self.lens = Lens::default();
        }
        if let Some(problem) = &self.problem {
            ui.add(egui::Label::new(RichText::new(problem).color(color(p.text))).wrap());
        }
    }

    /// The keys of the output window. Returns nothing: the numbers change.
    pub fn keys(&mut self, ctx: &egui::Context) {
        let (mut moves, mut toggle) = (Vec::new(), false);
        ctx.input(|i| {
            let fine = if i.modifiers.shift { 0.1 } else { 1.0 };
            for (key, knob, direction) in [
                (Key::ArrowRight, Knob::Offset, 1.0),
                (Key::ArrowLeft, Knob::Offset, -1.0),
                (Key::ArrowUp, Knob::Pitch, 1.0),
                (Key::ArrowDown, Knob::Pitch, -1.0),
                (Key::PageUp, Knob::Slant, 1.0),
                (Key::PageDown, Knob::Slant, -1.0),
            ] {
                let presses = i.num_presses(key);
                if presses > 0 {
                    moves.push((knob, direction * fine * presses as f64));
                }
            }
            toggle = i.key_pressed(Key::C);
        });
        for (knob, steps) in moves {
            self.nudge(knob, steps);
        }
        if toggle {
            self.pattern = !self.pattern;
        }
    }

    /// The line drawn over the pattern so the numbers can be read while
    /// tuning with the keys.
    pub fn readout(&self) -> String {
        format!(
            "pitch {:.4}   slant {:.4}   offset {:.3}",
            self.lens.pitch, self.lens.slant, self.lens.offset
        )
    }

    /// The interleaved picture for an area of `size` panel pixels at
    /// `origin`: the pattern, or `pair` (side by side, generation
    /// `generation`). None when there is nothing to show or the area is
    /// larger than the GPU takes.
    pub fn texture(
        &mut self,
        ctx: &egui::Context,
        pair: Option<&RgbaImage>,
        generation: u64,
        stretch: f64,
        size: [u32; 2],
        origin: (i64, i64),
    ) -> Option<egui::TextureHandle> {
        let settled = size == self.last_size;
        self.last_size = size;
        let wanted = Woven {
            generation,
            lens: self.lens,
            pattern: self.pattern,
            size,
            origin,
            stretch,
        };
        if self.woven == Some(wanted) {
            return self.texture.clone();
        }
        if !settled && self.texture.is_some() {
            // Mid-resize: keep the old picture this frame and come back.
            ctx.request_repaint();
            return self.texture.clone();
        }
        let limit = ctx.input(|i| i.max_texture_side).max(1) as u32;
        let image = if size[0] == 0 || size[1] == 0 || size[0] > limit || size[1] > limit {
            None
        } else {
            render(pair, self.pattern, stretch, size, &self.lens, origin)
        };
        self.woven = Some(wanted);
        self.texture = image.map(|image| {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [image.width() as usize, image.height() as usize],
                image.as_raw(),
            );
            // Nearest: every panel subpixel must keep exactly its own value.
            ctx.load_texture("viewer-lens", image, egui::TextureOptions::NEAREST)
        });
        self.texture.clone()
    }

    /// Drop the interleaved picture (the output window has closed).
    pub fn release(&mut self) {
        self.texture = None;
        self.woven = None;
        self.last_size = [0, 0];
    }
}

/// The interleaved image itself, without a window: what the tests check.
pub fn render(
    pair: Option<&RgbaImage>,
    pattern: bool,
    stretch: f64,
    size: [u32; 2],
    lens: &Lens,
    origin: (i64, i64),
) -> Option<RgbaImage> {
    if pattern {
        let (left, right) = weave::calibration_pattern(size[0], size[1]);
        weave::weave(&left, &right, lens, origin).ok()
    } else {
        weave::weave_pair(pair?, size[0], size[1], stretch, lens, origin).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_move_one_number_at_a_time_and_stay_in_range() {
        let mut state = LensState::default();
        let start = state.lens;
        state.nudge(Knob::Pitch, 1.0);
        assert!((state.lens.pitch - (start.pitch + 0.001)).abs() < 1e-12);
        assert_eq!(state.lens.slant, start.slant);
        state.nudge(Knob::Slant, -0.1);
        assert!((state.lens.slant - (start.slant - 0.0001)).abs() < 1e-12);
        // The offset wraps round instead of running away.
        state.nudge(Knob::Offset, -1.0);
        assert!((state.lens.offset - 0.99).abs() < 1e-12);
        state.nudge(Knob::Pitch, -1e9);
        assert_eq!(state.lens.pitch, PITCH_RANGE.0);
    }

    #[test]
    fn numbers_are_saved_after_they_settle_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub").join("lens.json");
        let ctx = egui::Context::default();
        let mut state = LensState::load(file.clone());
        assert_eq!(state.lens, Lens::default(), "no file is the default lens");
        assert!(state.problem.is_none());

        state.nudge(Knob::Pitch, 3.0);
        let t0 = Instant::now();
        state.save_when_settled(&ctx, t0);
        state.save_when_settled(&ctx, t0 + Duration::from_millis(500));
        assert!(!file.exists(), "not while the number may still be moving");
        state.save_when_settled(&ctx, t0 + SAVE_DELAY);
        assert!(file.exists());
        assert!(state.problem.is_none());

        let again = LensState::load(file.clone());
        assert_eq!(again.lens, state.lens);

        // Closing the output saves at once.
        state.nudge(Knob::Offset, 5.0);
        state.store();
        assert_eq!(read_lens(&file).unwrap(), state.lens);
    }

    #[test]
    fn a_broken_file_is_reported_and_replaced_only_by_a_change() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("lens.json");
        std::fs::write(&file, b"{ not json").unwrap();
        let mut state = LensState::load(file.clone());
        assert_eq!(state.lens, Lens::default());
        assert!(state
            .problem
            .as_deref()
            .unwrap()
            .contains("not a lens file"));
        state.store();
        assert_eq!(std::fs::read(&file).unwrap(), b"{ not json", "left alone");
        state.nudge(Knob::Offset, 1.0);
        state.store();
        assert_eq!(read_lens(&file).unwrap(), state.lens);
        assert!(state.problem.is_none());
        // Values out of range in a file are brought inside, not trusted.
        std::fs::write(&file, br#"{"pitch": 0, "offset": 7.25}"#).unwrap();
        let lens = read_lens(&file).unwrap();
        assert_eq!((lens.pitch, lens.offset), (PITCH_RANGE.0, 0.25));
    }

    #[test]
    fn the_pattern_needs_no_image_and_an_image_needs_one() {
        let lens = Lens {
            pitch: 2.0,
            slant: 0.0,
            offset: 0.0,
            bgr: false,
        };
        let pattern = render(None, true, 1.0, [4, 2], &lens, (0, 0)).unwrap();
        assert_eq!(pattern.get_pixel(0, 0).0, [255, 255, 255, 255]);
        assert_eq!(pattern.get_pixel(1, 0).0, [0, 0, 0, 255]);
        assert!(render(None, false, 1.0, [4, 2], &lens, (0, 0)).is_none());
        let pair = RgbaImage::from_fn(8, 2, |x, _| {
            image::Rgba(if x < 4 {
                [9, 9, 9, 255]
            } else {
                [200, 200, 200, 255]
            })
        });
        let woven = render(Some(&pair), false, 1.0, [4, 2], &lens, (0, 0)).unwrap();
        assert_eq!(woven.get_pixel(0, 0).0, [9, 9, 9, 255]);
        assert_eq!(woven.get_pixel(1, 0).0, [200, 200, 200, 255]);
    }

    #[test]
    fn the_readout_names_all_three_numbers() {
        let state = LensState::default();
        assert_eq!(
            state.readout(),
            "pitch 5.4000   slant 0.3000   offset 0.000"
        );
    }
}
