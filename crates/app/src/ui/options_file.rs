//! The viewer's choices, kept between runs.
//!
//! What the image is, how it is shown, and the picture controls (depth,
//! pop-out, depth detail, and a live screen's sharpness, depth steadiness
//! and lines) used to start at their defaults at every launch: a live
//! screen was shown at standard depth detail and 1080 lines again however
//! it had been set the evening before. They are written to a small file in
//! the data folder a moment after the last change, and read at the start.
//!
//! The file is read leniently: a value that is missing, unknown or out of
//! range is the default for that one choice, and a file that cannot be read
//! at all is all defaults. Nothing here is worth a message to the user.
use super::viewer::{Input, Options, Output, DEPTH_DETAIL, LIVE_LINES};
use any2odyssey_core::fsutil;
use any2odyssey_core::slider::{DEPTH, POPOUT};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long the choices have to stand still before they are written: a
/// slider that is dragged changes them at every step.
const SAVE_DELAY: Duration = Duration::from_secs(1);

/// The file's contents. Every field may be missing.
#[derive(Serialize, Deserialize, Default)]
#[serde(default, rename_all = "PascalCase")]
struct Saved {
    input: Option<String>,
    output: Option<String>,
    half_width: Option<bool>,
    swap: Option<bool>,
    depth: Option<i32>,
    popout: Option<i32>,
    sharpness: Option<i32>,
    depth_steadiness: Option<i32>,
    lines: Option<i32>,
    depth_detail: Option<u32>,
    pointer: Option<bool>,
}

const INPUTS: [(Input, &str); 4] = [
    (Input::Pair, "Pair"),
    (Input::Ai, "Ai"),
    (Input::Flat, "Flat"),
    (Input::Desktop, "Desktop"),
];
const OUTPUTS: [(Output, &str); 4] = [
    (Output::Anaglyph, "Anaglyph"),
    (Output::Sbs, "Sbs"),
    (Output::Odyssey, "Odyssey"),
    (Output::Lens, "Lens"),
];

fn name_of<T: PartialEq + Copy>(names: &[(T, &'static str)], value: T) -> Option<String> {
    names
        .iter()
        .find(|(known, _)| *known == value)
        .map(|(_, name)| (*name).to_owned())
}

fn named<T: Copy>(names: &[(T, &'static str)], name: Option<&str>) -> Option<T> {
    names
        .iter()
        .find(|(_, known)| Some(*known) == name)
        .map(|(value, _)| *value)
}

impl Saved {
    fn of(options: Options) -> Saved {
        Saved {
            input: name_of(&INPUTS, options.input),
            output: name_of(&OUTPUTS, options.output),
            half_width: Some(options.half_width),
            swap: Some(options.swap),
            depth: Some(options.depth_pos),
            popout: Some(options.pop_pos),
            sharpness: Some(options.sharp_pos),
            depth_steadiness: Some(options.steady_pos),
            lines: Some(options.lines),
            depth_detail: Some(options.detail),
            pointer: Some(options.pointer),
        }
    }

    /// The choices these values make: each one that is there and can be
    /// chosen on the page, the default for every other.
    fn options(&self) -> Options {
        let default = Options::default();
        let within = |value: Option<i32>, min: i32, max: i32, otherwise: i32| {
            value
                .filter(|v| (min..=max).contains(v))
                .unwrap_or(otherwise)
        };
        Options {
            input: named(&INPUTS, self.input.as_deref())
                // (a live screen is on the page of the Linux edition only)
                .filter(|input| cfg!(target_os = "linux") || *input != Input::Desktop)
                .unwrap_or(default.input),
            output: named(&OUTPUTS, self.output.as_deref()).unwrap_or(default.output),
            half_width: self.half_width.unwrap_or(default.half_width),
            swap: self.swap.unwrap_or(default.swap),
            depth_pos: within(self.depth, DEPTH.min, DEPTH.max, default.depth_pos),
            pop_pos: within(self.popout, POPOUT.min, POPOUT.max, default.pop_pos),
            sharp_pos: within(self.sharpness, 0, 100, default.sharp_pos),
            steady_pos: within(self.depth_steadiness, 0, 95, default.steady_pos),
            lines: self
                .lines
                .filter(|lines| LIVE_LINES.iter().any(|(known, _)| known == lines))
                .unwrap_or(default.lines),
            detail: self
                .depth_detail
                .filter(|detail| DEPTH_DETAIL.iter().any(|(known, _)| known == detail))
                .unwrap_or(default.detail),
            pointer: self.pointer.unwrap_or(default.pointer),
        }
    }
}

/// The choices in `path`, or the defaults where it has none.
pub fn read(path: &Path) -> Options {
    fsutil::read_limited(path, fsutil::SMALL_FILE_LIMIT)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Saved>(fsutil::strip_utf8_bom(&bytes)).ok())
        .unwrap_or_default()
        .options()
}

/// Keeps a file up to date with the viewer's choices.
#[derive(Default)]
pub struct OptionsFile {
    /// Where they are kept. None keeps them in memory only (tests, previews).
    file: Option<PathBuf>,
    /// What the file holds, as far as this process knows.
    saved: Option<Options>,
    /// The choices that wait to be written, and since when they stand.
    pending: Option<(Options, Instant)>,
    /// The choices the file last refused to take: not tried again until
    /// they change, or the program ends.
    failed: Option<Options>,
}

impl OptionsFile {
    /// The file at `path` and the choices it holds now.
    pub fn load(path: PathBuf) -> (OptionsFile, Options) {
        let options = read(&path);
        let file = OptionsFile {
            file: Some(path),
            saved: Some(options),
            ..OptionsFile::default()
        };
        (file, options)
    }

    /// Write `options` now if the file does not hold them.
    pub fn store(&mut self, options: Options) {
        self.pending = None;
        let Some(file) = &self.file else {
            return;
        };
        if self.saved == Some(options) {
            return;
        }
        let written = file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| fsutil::write_json_atomic(file, &Saved::of(options)));
        match written {
            Ok(()) => {
                self.saved = Some(options);
                self.failed = None;
            }
            Err(_) => self.failed = Some(options),
        }
    }

    /// Write `options` once they have been the same for [`SAVE_DELAY`].
    /// Call once per frame; returns how long to wait before the next look,
    /// when there is something waiting.
    pub fn save_when_settled(&mut self, options: Options, now: Instant) -> Option<Duration> {
        if self.file.is_none() || self.saved == Some(options) || self.failed == Some(options) {
            self.pending = None;
            return None;
        }
        match self.pending {
            Some((waiting, since)) if waiting == options => {
                let elapsed = now.duration_since(since);
                if elapsed >= SAVE_DELAY {
                    self.store(options);
                    None
                } else {
                    Some(SAVE_DELAY - elapsed)
                }
            }
            _ => {
                self.pending = Some((options, now));
                Some(SAVE_DELAY)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changed() -> Options {
        Options {
            input: Input::Ai,
            output: Output::Odyssey,
            half_width: false,
            swap: true,
            depth_pos: DEPTH.max,
            pop_pos: POPOUT.min,
            sharp_pos: 40,
            steady_pos: 80,
            lines: 2160,
            detail: 1022,
            pointer: false,
        }
    }

    #[test]
    fn the_choices_are_there_again_at_the_next_start() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deeper").join("viewer-settings.json");
        let (mut file, options) = OptionsFile::load(path.clone());
        assert_eq!(options, Options::default(), "no file: the defaults");
        assert!(!path.exists(), "nothing is written for the defaults");
        file.store(Options::default());
        assert!(!path.exists());
        file.store(changed());
        assert_eq!(read(&path), changed());
        assert_eq!(OptionsFile::load(path.clone()).1, changed());
        // Back at the defaults: the file says so, it is not left as it was.
        file.store(Options::default());
        assert_eq!(read(&path), Options::default());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_live_screen_is_remembered_with_its_controls() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("viewer-settings.json");
        let live = Options {
            input: Input::Desktop,
            ..changed()
        };
        OptionsFile::load(path.clone()).0.store(live);
        assert_eq!(read(&path), live);
    }

    #[test]
    fn a_dragged_slider_is_written_once_it_stands_still() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("viewer-settings.json");
        let (mut file, mut options) = OptionsFile::load(path.clone());
        let start = Instant::now();
        assert_eq!(
            file.save_when_settled(options, start),
            None,
            "nothing changed"
        );
        for step in 0..10u32 {
            options.sharp_pos += 1;
            let wait =
                file.save_when_settled(options, start + Duration::from_millis(100 * step as u64));
            assert_eq!(wait, Some(SAVE_DELAY));
            assert!(!path.exists(), "written while the slider moves");
        }
        let still = start + Duration::from_millis(900);
        assert_eq!(
            file.save_when_settled(options, still + Duration::from_millis(400)),
            Some(Duration::from_millis(600))
        );
        assert!(!path.exists());
        assert_eq!(file.save_when_settled(options, still + SAVE_DELAY), None);
        assert_eq!(read(&path), options);
        // And not again while nothing changes.
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            file.save_when_settled(options, still + SAVE_DELAY * 5),
            None
        );
        assert!(!path.exists());
    }

    #[test]
    fn a_file_that_cannot_be_written_is_not_tried_at_every_frame() {
        let dir = tempfile::tempdir().unwrap();
        // A folder where the file should be.
        let path = dir.path().join("viewer-settings.json");
        std::fs::create_dir(&path).unwrap();
        let (mut file, _) = OptionsFile::load(path.clone());
        let now = Instant::now();
        assert_eq!(file.save_when_settled(changed(), now), Some(SAVE_DELAY));
        assert_eq!(file.save_when_settled(changed(), now + SAVE_DELAY), None);
        assert_eq!(
            file.save_when_settled(changed(), now + SAVE_DELAY * 2),
            None
        );
        assert!(path.is_dir());
        // Without a file nothing is written and nothing waits.
        let mut memory = OptionsFile::default();
        assert_eq!(memory.save_when_settled(changed(), now), None);
        memory.store(changed());
    }

    #[test]
    fn what_the_page_cannot_choose_is_the_default_for_that_choice_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("viewer-settings.json");
        let default = Options::default();
        let read_text = |text: &str| {
            std::fs::write(&path, text).unwrap();
            read(&path)
        };
        for broken in ["", "{", "[]", "null", "\"Ai\"", "{\"Lines\": \"many\"}"] {
            assert_eq!(read_text(broken), default, "{broken:?}");
        }
        let odd = read_text(
            r#"{"Input":"Hologram","Output":"Odyssey","Depth":100000,"Popout":-100000,"Sharpness":101,
                "DepthSteadiness":96,"Lines":1081,"DepthDetail":519,"Pointer":false,"Later":[1,2]}"#,
        );
        assert_eq!(
            odd,
            Options {
                output: Output::Odyssey,
                pointer: false,
                ..default
            }
        );
        // With the mark some editors put at the start of a file.
        assert_eq!(
            read_text("\u{feff}{\"Lines\":1440,\"DepthDetail\":770}"),
            Options {
                lines: 1440,
                detail: 770,
                ..default
            }
        );
        // Every choice of the page comes back as it was.
        for (lines, _) in LIVE_LINES {
            for (detail, _) in DEPTH_DETAIL {
                let options = Options {
                    lines,
                    detail,
                    ..changed()
                };
                OptionsFile::load(path.clone()).0.store(options);
                assert_eq!(read(&path), options);
            }
        }
    }
}
