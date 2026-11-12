//! The stereo viewer page, the home page where Samsung's engine does not
//! exist (Linux): open a side-by-side image, turn it into red/cyan anaglyph
//! or a (swapped) side-by-side pair, preview it, show it in a separate
//! output window and save it as PNG.
//!
//! Besides layout conversion (`core::stereo`) and AI depth (`core::dibr`)
//! there is an experimental lens-interleaved output (`core::weave`, `lens`)
//! tuned by the user's own calibration. Nothing here uses Samsung's engine
//! or its calibration files.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::Arc;

use any2odyssey_core::dibr::{self, DepthMap};
use any2odyssey_core::slider::{DEPTH, POPOUT};
use any2odyssey_core::stereo::{self, Layout};
use any2odyssey_core::theme::Palette;
use egui::{pos2, Color32, Key, Rect, RichText, Sense, Ui, ViewportCommand};
use image::{ImageDecoder, RgbaImage};

use super::lens::{self, LensState};
use super::theme::color;
use super::widgets::{banner, gold_button, row, section};
use crate::core::{Core, MenuCommand, UiRequest};
use crate::lens_switch::{LensStatus, LensSwitch};
use crate::platform;
use crate::weaver::{self, Weaver, WeaverStatus};

pub const READY: &str =
    "Ready. Open a side-by-side stereo image, or drop one on this window. Output starts only when requested.";
pub const NOTICE: &str = "Shows images as anaglyph, side by side or on the Odyssey 3D, and (experimental) a live screen in 3D on the Odyssey. The Odyssey 3D output uses Leia's weaver and eye tracker from your own copy of the SR runtime under Wine; Samsung's conversion engine exists only on Windows, so depth comes from an open model.";
const OUTPUT_HINT: &str =
    "Move this window to the display you want, then double-click or press F11 for full screen. Esc leaves.";
const LOADED: &str = "Image loaded. Check the input format before starting output.";
const NO_IMAGE: &str = "Open an image first.";
const DEPTH_NOT_READY: &str =
    "The image's depth is still being estimated. Start output when the picture is in the preview.";
const OUTPUT_ON: &str =
    "Output window open. Stop output, Esc in that window, or closing it ends it.";
const OUTPUT_OFF: &str = "Output stopped.";
const ODYSSEY_ON: &str =
    "Output on the Odyssey 3D. Stop output, or Esc with the pointer on the Odyssey, ends it.";
const LENS_WINDOWED: &str =
    "Lens output is only right full screen on the Odyssey: double-click or press F11. Esc leaves.";
const ESTIMATING: &str =
    "Estimating depth… The first image also loads the model, which takes a few seconds.";
const CONVERTED: &str = "Converted to 3D. Adjust depth strength and pop-out to taste.";
const DEFAULT_EXPORT_NAME: &str = "Any2Odyssey-preview.png";
const EXPORT_OUTDATED: &str =
    "Nothing was saved: the picture changed while the question was open. Save again for the picture that is shown now.";
/// The largest image the viewer opens: no side longer than this, and no
/// more pixels than [`MAX_IMAGE_PIXELS`]. A file says in a few bytes how
/// large its picture is, and the picture is held in memory several times
/// over (decoded, as a stereo pair, as a texture): a small crafted file
/// must not be able to ask for gigabytes. The side is also the most Leia's
/// weaver and a GPU texture take; the pixel count still lets a full
/// side-by-side pair of 8K views in (15360 × 4320).
const MAX_IMAGE_SIDE: u32 = 16384;
const MAX_IMAGE_PIXELS: u64 = 1 << 26;
/// What the decoder may allocate while reading a file: the picture as the
/// file holds it (at most eight bytes a pixel) and the decoder's own needs.
const MAX_DECODE_BYTES: u64 = 512 * 1024 * 1024;
/// The largest file looked at as an image: twice what the largest picture
/// takes without any compression. One decoder (JPEG) reads the whole file
/// into memory before it looks at anything.
const MAX_FILE_BYTES: u64 = 2 * MAX_DECODE_BYTES;
/// How large the depth model sees the picture (`depth::DETAIL`): the long
/// side in pixels, and its name.
const DEPTH_DETAIL: [(u32, &str); 4] = [
    (518, "Standard"),
    (770, "Fine"),
    (1022, "Finer"),
    (1274, "Finest (slow without a GPU)"),
];
/// The lines-per-view choices of the live screen (`live::LINES`).
const LIVE_LINES: [(i32, &str); 4] = [
    (720, "720 lines (fastest)"),
    (1080, "1080 lines"),
    (1440, "1440 lines"),
    (2160, "2160 lines (sharpest, slowest)"),
];
const LIVE_NEEDS_ODYSSEY: &str = "A live screen is shown through the Odyssey 3D output only.";
#[cfg(not(target_os = "linux"))]
const LIVE_NOT_HERE: &str = "Live capture is not available in this build.";

/// What the source image contains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    /// Left and right eye side by side.
    Pair,
    /// An ordinary 2D image, shown unchanged.
    Flat,
    /// An ordinary 2D image given depth by the model and rendered as a pair.
    Ai,
    /// A screen, captured live and given depth frame by frame (`live`).
    /// There is no image in the document: the output is fed directly.
    Desktop,
}

impl Input {
    #[cfg(target_os = "linux")]
    const ALL: [Input; 4] = [Input::Pair, Input::Ai, Input::Flat, Input::Desktop];
    #[cfg(not(target_os = "linux"))]
    const ALL: [Input; 3] = [Input::Pair, Input::Ai, Input::Flat];
    fn label(self) -> &'static str {
        match self {
            Input::Pair => "Side-by-side stereo pair",
            Input::Ai => "2D image → 3D (AI depth)",
            Input::Flat => "2D image (flat, no depth)",
            Input::Desktop => "Live screen → 3D (AI depth, experimental)",
        }
    }
}

/// How a stereo pair is presented.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    Anaglyph,
    Sbs,
    /// The pair interleaved subpixel by subpixel for the Odyssey's lens.
    /// The document keeps the side-by-side pair; the output window
    /// interleaves it for the pixels it covers.
    Lens,
    /// The pair handed to Leia's own weaver (`weaver`): the panel's factory
    /// calibration and eye tracking, in a full-screen window of the weaver's
    /// helper on the Odyssey instead of this program's output window.
    Odyssey,
}

impl Output {
    const ALL: [Output; 4] = [Output::Anaglyph, Output::Sbs, Output::Odyssey, Output::Lens];
    fn label(self) -> &'static str {
        match self {
            Output::Anaglyph => "Red/cyan anaglyph (needs red/cyan glasses)",
            Output::Sbs => "Side-by-side stereo (needs a matching viewer)",
            Output::Odyssey => "Odyssey 3D: Leia's weaver, eye-tracked (experimental)",
            Output::Lens => "Odyssey lens interleave, own calibration (experimental)",
        }
    }
}

/// The choices that decide the converted image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub input: Input,
    pub output: Output,
    /// Anaglyph only: each eye of a half-width pair is stretched back to the
    /// full width.
    pub half_width: bool,
    pub swap: bool,
    /// AI depth only: slider positions of depth strength and pop-out, on the
    /// same scales as the Windows controls.
    pub depth_pos: i32,
    pub pop_pos: i32,
    /// Live screen only: sharpening (0..=100), how steady depth is kept
    /// between frames (0..=95) and the lines per view.
    pub sharp_pos: i32,
    pub steady_pos: i32,
    pub lines: i32,
    /// AI depth: the long side of the picture the model sees
    /// (`depth::DETAIL`), for images and for a live screen.
    pub detail: u32,
    /// Live screen only: draw the pointer into the picture. Wanted for a
    /// screen shown elsewhere; on the Odyssey's own screen the real pointer
    /// is there already.
    pub pointer: bool,
}

impl Default for Options {
    fn default() -> Options {
        let defaults = dibr::Params::default();
        Options {
            input: Input::Pair,
            output: Output::Anaglyph,
            half_width: true,
            swap: false,
            depth_pos: DEPTH.position(defaults.strength),
            pop_pos: POPOUT.position(defaults.popout),
            sharp_pos: 0,
            steady_pos: 60,
            lines: 1080,
            detail: DEPTH_DETAIL[0].0,
            pointer: true,
        }
    }
}

impl Options {
    pub fn layout(&self) -> Layout {
        match (self.input, self.output) {
            (Input::Flat | Input::Desktop, _) => Layout::Flat,
            (Input::Pair | Input::Ai, Output::Anaglyph) => Layout::Anaglyph,
            (Input::Pair | Input::Ai, Output::Sbs | Output::Lens | Output::Odyssey) => Layout::Sbs,
        }
    }

    /// Whether the output window interleaves a pair for the lens.
    pub fn weaves(&self) -> bool {
        self.output == Output::Lens && !matches!(self.input, Input::Flat | Input::Desktop)
    }

    /// Whether the picture is a live screen rather than the document's.
    pub fn live(&self) -> bool {
        self.input == Input::Desktop
    }

    /// How much each eye of the pair is widened before it is shown: a
    /// half-width pair holds each eye squeezed to half its width.
    fn eye_stretch(&self) -> f64 {
        if self.input == Input::Pair && self.half_width {
            2.0
        } else {
            1.0
        }
    }

    fn params(&self) -> dibr::Params {
        dibr::Params {
            strength: DEPTH.value(self.depth_pos),
            popout: POPOUT.value(self.pop_pos),
        }
    }

    /// Everything of the options that decides the converted image, and
    /// nothing else: options with the same conversion make the same result
    /// of an image, so it is not converted again for a change among the
    /// rest (the lines of a live screen, an output format with the same
    /// layout, a setting the current layout does not use).
    fn conversion(&self) -> Conversion {
        let layout = self.layout();
        let ai = self.input == Input::Ai;
        let params = self.params();
        Conversion {
            ai,
            layout,
            // A flat picture has no eyes to exchange; only a pair squeezed
            // into an anaglyph is stretched.
            swap: self.swap && layout != Layout::Flat,
            half_width: self.half_width && !ai && layout == Layout::Anaglyph,
            strength: if ai { params.strength.to_bits() } else { 0 },
            // Without depth strength there is nothing for pop-out to move.
            popout: if ai && params.strength != 0.0 {
                params.popout.to_bits()
            } else {
                0
            },
        }
    }
}

/// See [`Options::conversion`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Conversion {
    ai: bool,
    layout: Layout,
    swap: bool,
    half_width: bool,
    strength: u64,
    popout: u64,
}

/// The source image and what the current options make of it. No window and
/// no GPU: this is the part the tests drive.
///
/// The images are shared, not copied: where the result is the source
/// unchanged (a flat picture, a side-by-side pair shown side by side) the
/// two are one image, and a thread that reads the source holds it too.
#[derive(Default)]
pub struct Document {
    source: Option<Arc<RgbaImage>>,
    /// The model's depth map of `source`, once it has been estimated.
    depth: Option<DepthMap>,
    result: Option<Arc<RgbaImage>>,
    /// Bumped whenever `result` changes, so the page knows to upload it.
    generation: u64,
    /// Bumped whenever `source` changes, so a depth map that arrives for an
    /// image that has since been replaced is recognized and dropped.
    source_id: u64,
}

/// An image file read and converted, ready to become the document's. Made
/// off the UI thread ([`load`]): reading a large file takes a while.
pub struct Loaded {
    image: Arc<RgbaImage>,
    /// The options the result was made for.
    options: Options,
    /// None for AI depth, whose result follows the model.
    result: Option<Arc<RgbaImage>>,
}

/// Read the image in `path`, refusing one larger than the viewer opens
/// before any of its pixels are decoded.
fn decode(path: &Path) -> Result<RgbaImage, String> {
    let failed = |e: &dyn std::fmt::Display| format!("Could not open {}: {e}", path.display());
    let too_large = |size: &dyn std::fmt::Display| {
        format!(
            "Could not open {}: the image is too large{size}. The viewer opens images of up to {MAX_IMAGE_SIDE} pixels a side and {} million pixels.",
            path.display(),
            MAX_IMAGE_PIXELS / 1_000_000
        )
    };
    let limited = |e: image::ImageError| match e {
        image::ImageError::Limits(_) => too_large(&""),
        e => failed(&e),
    };
    // Only a file is read: a pipe would never end, a device might.
    let file = std::fs::metadata(path).map_err(|e| failed(&e))?;
    if !file.is_file() {
        return Err(failed(&"it is not a file."));
    }
    if file.len() > MAX_FILE_BYTES {
        return Err(too_large(&format_args!(
            " ({} MB on disk)",
            file.len() / 1_000_000
        )));
    }
    let mut reader = image::ImageReader::open(path).map_err(|e| failed(&e))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    // The decoder has read the file's header and nothing else yet.
    let decoder = reader.into_decoder().map_err(limited)?;
    let (width, height) = decoder.dimensions();
    if width > MAX_IMAGE_SIDE
        || height > MAX_IMAGE_SIDE
        || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS
        || decoder.total_bytes() > MAX_DECODE_BYTES
    {
        return Err(too_large(&format_args!(" ({width} × {height} pixels)")));
    }
    let image = image::DynamicImage::from_decoder(decoder).map_err(limited)?;
    Ok(image.into_rgba8())
}

/// Read `path` and convert it for `options`. Blocks for as long as the file
/// takes to decode: the page calls it on a thread of its own.
pub fn load(path: &Path, options: Options) -> Result<Loaded, String> {
    let image = Arc::new(decode(path)?);
    // AI depth has nothing to show until the model has run: the image is
    // taken as it is and the result follows with `set_depth`.
    let result = if options.input == Input::Ai {
        None
    } else {
        let converted = convert(&image, None, options)
            .map_err(|e| format!("Could not use {}: {e}", path.display()))?;
        Some(converted)
    };
    Ok(Loaded {
        image,
        options,
        result,
    })
}

impl Document {
    pub fn result(&self) -> Option<&RgbaImage> {
        self.result.as_deref()
    }

    /// Decode `path` and convert it. On failure the previous image stays.
    /// The page does the two halves apart, the first on a thread.
    #[cfg(test)]
    pub fn open(&mut self, path: &Path, options: Options) -> Result<(), String> {
        let loaded = load(path, options)?;
        self.install(loaded, options)
            .map_err(|e| format!("Could not use {}: {e}", path.display()))
    }

    /// Take an image that was read for `loaded.options` while the options
    /// are now `options`: converted again only if the two differ in what
    /// decides the result. On failure the previous image stays.
    pub fn install(&mut self, loaded: Loaded, options: Options) -> Result<(), String> {
        let result = if loaded.options.conversion() == options.conversion() {
            loaded.result
        } else if options.input == Input::Ai {
            None
        } else {
            Some(convert(&loaded.image, None, options)?)
        };
        self.replace(loaded.image, result);
        Ok(())
    }

    #[cfg(test)]
    pub fn set_source(&mut self, image: RgbaImage, options: Options) -> Result<(), String> {
        let image = Arc::new(image);
        let converted = if options.input == Input::Ai {
            None
        } else {
            Some(convert(&image, None, options)?)
        };
        self.replace(image, converted);
        Ok(())
    }

    fn replace(&mut self, image: Arc<RgbaImage>, result: Option<Arc<RgbaImage>>) {
        self.source = Some(image);
        self.depth = None;
        self.result = result;
        self.generation += 1;
        self.source_id += 1;
    }

    /// The image the model should look at, with the id to hand back to
    /// [`Document::set_depth`], when the options need a depth map that has
    /// not been estimated yet.
    pub fn depth_request(&self, options: Options) -> Option<(u64, &Arc<RgbaImage>)> {
        if options.input != Input::Ai || self.depth.is_some() {
            return None;
        }
        self.source.as_ref().map(|image| (self.source_id, image))
    }

    /// Drop the depth map, so that one is estimated again (the model is to
    /// see the image at another size). A map still on its way for the old
    /// request is recognized by its id and dropped too.
    pub fn forget_depth(&mut self) {
        self.depth = None;
        self.source_id += 1;
    }

    /// Take the depth map estimated for source `id`. False when the source
    /// has changed since and the map was dropped.
    pub fn set_depth(
        &mut self,
        id: u64,
        depth: DepthMap,
        options: Options,
    ) -> Result<bool, String> {
        if id != self.source_id || self.source.is_none() {
            return Ok(false);
        }
        self.depth = Some(depth);
        self.reconvert(options).map(|()| true)
    }

    /// Convert the loaded image again after an option changed. A source the
    /// new options cannot take (an odd width as a stereo pair) leaves no
    /// result, so nothing stale stays on the output.
    pub fn reconvert(&mut self, options: Options) -> Result<(), String> {
        let Some(source) = &self.source else {
            return Ok(());
        };
        self.generation += 1;
        if options.input == Input::Ai && self.depth.is_none() {
            // Waiting for the model; nothing of another mode stays up.
            self.result = None;
            return Ok(());
        }
        let converted = convert(source, self.depth.as_ref(), options);
        match converted {
            Ok(image) => {
                self.result = Some(image);
                Ok(())
            }
            Err(e) => {
                self.result = None;
                Err(e)
            }
        }
    }

    /// Write the converted image as PNG, whatever extension `path` has.
    pub fn save_png(&self, path: &Path) -> Result<(), String> {
        let Some(result) = &self.result else {
            return Err(NO_IMAGE.into());
        };
        let target = export_target(path)?;
        any2odyssey_core::fsutil::write_atomic_document_with(&target, |writer| {
            result
                .write_to(writer, image::ImageFormat::Png)
                .map_err(std::io::Error::other)
        })
        .map_err(|e| format!("Could not save {}: {e}", path.display()))
    }
}

/// The file an export to `path` is written as. The export is published by
/// renaming a finished file into place, and a rename replaces the name it
/// is given, whatever that is: a link would be replaced instead of what it
/// leads to, and a file the user may not write would be replaced all the
/// same (the folder decides a rename, not the file). So links are followed
/// here, and only a file that could have been written to is replaced.
fn export_target(path: &Path) -> Result<PathBuf, String> {
    let failed = |e: &dyn std::fmt::Display| format!("Could not save {}: {e}", path.display());
    let mut target = path.to_owned();
    // As many links as the system itself follows before it gives up.
    for _ in 0..40 {
        let metadata = match std::fs::symlink_metadata(&target) {
            Ok(metadata) => metadata,
            // A new file (also where a link leads to nothing yet).
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(target),
            Err(e) => return Err(failed(&e)),
        };
        if metadata.file_type().is_symlink() {
            let link = std::fs::read_link(&target).map_err(|e| failed(&e))?;
            // A relative link starts from the folder the link is in.
            target = match target.parent() {
                Some(folder) => folder.join(link),
                None => link,
            };
        } else if metadata.is_dir() {
            return Err(failed(&"it is a folder. Choose a file name."));
        } else if !metadata.is_file() {
            return Err(failed(&"it is not an ordinary file."));
        } else {
            // Opened as a writer would open it, and closed again unchanged.
            return match std::fs::OpenOptions::new().write(true).open(&target) {
                Ok(_) => Ok(target),
                Err(e) => Err(failed(&format_args!(
                    "the existing file cannot be written to ({e}). It was left as it is."
                ))),
            };
        }
    }
    Err(failed(&"too many links lead from it."))
}

fn convert(
    image: &Arc<RgbaImage>,
    depth: Option<&DepthMap>,
    options: Options,
) -> Result<Arc<RgbaImage>, String> {
    if options.input == Input::Ai {
        let depth = depth.ok_or("The depth map is not ready.")?;
        // The rendered pair is full width per eye: no half-width stretch.
        let pair = dibr::synthesize(image, depth, options.params()).map_err(|e| e.to_string())?;
        let converted = stereo::convert_shared(&pair, options.layout(), false, options.swap)
            .map_err(|e| e.to_string())?;
        let changed = match converted {
            Cow::Owned(converted) => Some(converted),
            Cow::Borrowed(_) => None,
        };
        // Side by side and not swapped: the pair as it was rendered.
        return Ok(Arc::new(changed.unwrap_or(pair)));
    }
    let converted =
        stereo::convert_shared(image, options.layout(), options.half_width, options.swap)
            .map_err(|e| e.to_string())?;
    Ok(match converted {
        Cow::Owned(converted) => Arc::new(converted),
        // The source unchanged: the same image, not a copy of it.
        Cow::Borrowed(_) => Arc::clone(image),
    })
}

/// `name` with a `.png` extension unless it already has one (any case).
pub fn with_png_extension(path: PathBuf) -> PathBuf {
    let is_png = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("png"));
    if is_png {
        path
    } else {
        let mut name = path.into_os_string();
        name.push(".png");
        PathBuf::from(name)
    }
}

/// The panel pixel under the top left corner of the window `ctx` draws,
/// where the window system tells (X11; Wayland keeps it to itself). It is
/// right when the Odyssey is the display at the desktop's origin.
fn window_origin(ctx: &egui::Context, scale: f32) -> (i64, i64) {
    ctx.input(|i| i.viewport().inner_rect)
        .map(|rect| {
            (
                (rect.min.x * scale).round() as i64,
                (rect.min.y * scale).round() as i64,
            )
        })
        .unwrap_or((0, 0))
}

/// The largest rectangle with `image`'s aspect ratio centred in `area`.
fn fit_centered(area: Rect, image: egui::Vec2) -> Rect {
    if image.x <= 0.0 || image.y <= 0.0 || area.width() <= 0.0 || area.height() <= 0.0 {
        return Rect::from_center_size(area.center(), egui::Vec2::ZERO);
    }
    let scale = (area.width() / image.x).min(area.height() / image.y);
    Rect::from_center_size(area.center(), image * scale)
}

enum Picked {
    Open(Result<Option<PathBuf>, String>),
    Save(Result<Option<PathBuf>, String>),
}

/// An image file being read on a thread of its own.
struct Loading {
    path: PathBuf,
    loaded: Receiver<Result<Loaded, String>>,
}

/// An export that waits for the answer to "Replace existing PNG?".
#[derive(Debug, PartialEq, Eq)]
struct PendingSave {
    path: PathBuf,
    /// The picture the question is about (`Document::generation`). The page
    /// goes on taking dropped files and images from other launches while
    /// the question is up, and an answer must not save another picture.
    generation: u64,
}

/// What Leia's weaver was last given: a picture of the document (its
/// generation) as it is composed for the two views, or black while that
/// generation has no picture yet.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Woven {
    generation: u64,
    flat: bool,
    stretch: f64,
}

pub struct ViewerState {
    document: Document,
    options: Options,
    /// The path box: what Open reads, and where a loaded file's path shows.
    path_text: String,
    /// The file whose path the box was given to show. The text is not the
    /// path where the name is not Unicode (`typed_path`).
    shown_path: Option<PathBuf>,
    status: String,
    texture: Option<egui::TextureHandle>,
    uploaded: u64,
    output_open: bool,
    output_fullscreen: bool,
    /// An image being read, and the one asked for since: it is read next,
    /// and the one before it is then not shown at all.
    loading: Option<Loading>,
    queued_open: Option<PathBuf>,
    /// A file dialog running on its own thread, with the image that was
    /// loaded when it was opened (`Document::source_id`).
    picking: Option<(Receiver<Picked>, u64)>,
    /// Adding .png selected an existing path the file dialog did not confirm.
    pending_save: Option<PendingSave>,
    /// The model running on its own thread, with the source id it was given.
    estimating: Option<(u64, Receiver<Result<DepthMap, String>>)>,
    /// The source the model last failed on, so the same failure is not
    /// retried on every frame. Cleared by a new image or a new choice of
    /// input format.
    depth_failed: Option<u64>,
    /// The lens numbers and the interleaved picture of the output window.
    lens: LensState,
    /// The Odyssey's lens itself: asked for while lens output is full screen.
    lens_switch: LensSwitch,
    /// Output through Leia's weaver, and the picture it was last given.
    weaver: Weaver,
    woven: Option<Woven>,
    /// The live screen while it is being shown.
    #[cfg(target_os = "linux")]
    live: Option<crate::live::Live>,
    /// "Choose another screen" was pressed: the running capture's token is
    /// not kept.
    #[cfg(target_os = "linux")]
    forget_screen: bool,
    /// The capture's token as last written to its file.
    #[cfg(target_os = "linux")]
    saved_token: Option<String>,
}

impl Default for ViewerState {
    fn default() -> ViewerState {
        ViewerState {
            document: Document::default(),
            options: Options::default(),
            path_text: String::new(),
            shown_path: None,
            status: READY.into(),
            texture: None,
            uploaded: 0,
            output_open: false,
            output_fullscreen: false,
            loading: None,
            queued_open: None,
            picking: None,
            pending_save: None,
            estimating: None,
            depth_failed: None,
            lens: LensState::default(),
            // No helper is looked for: tests and previews start no program.
            lens_switch: LensSwitch::with_helper(None),
            weaver: Weaver::with_helper(None),
            woven: None,
            #[cfg(target_os = "linux")]
            live: None,
            #[cfg(target_os = "linux")]
            forget_screen: false,
            #[cfg(target_os = "linux")]
            saved_token: None,
        }
    }
}

impl ViewerState {
    /// The viewer with the user's saved lens numbers; changes are saved
    /// back. `default()` keeps them in memory only.
    pub fn with_saved_lens() -> ViewerState {
        ViewerState {
            lens: LensState::load(any2odyssey_core::paths::lens_file()),
            lens_switch: LensSwitch::new(),
            weaver: Weaver::new(),
            ..ViewerState::default()
        }
    }

    /// Flush calibration changes when the application exits, even if their
    /// settling delay has not elapsed or the viewer page is no longer shown.
    pub fn save_lens(&mut self) {
        self.lens.store();
    }

    /// The calibration pattern is a picture of its own: it needs no image.
    fn shows_pattern(&self) -> bool {
        self.options.output == Output::Lens && self.lens.pattern
    }

    /// Read the image in `path` on a thread of its own and show it when it
    /// is there (`poll_open`): a large file takes seconds to decode, and
    /// the page and the output must not stand still for them. One file at a
    /// time. Of those asked for while one is read, the last one is next.
    fn open(&mut self, ctx: &egui::Context, path: &Path) {
        if self.loading.is_some() {
            self.queued_open = Some(path.to_owned());
            return;
        }
        let (tx, rx) = mpsc::channel();
        let (ctx, file, options) = (ctx.clone(), path.to_owned(), self.options);
        let spawned = std::thread::Builder::new()
            .name("image-open".into())
            .spawn(move || {
                let _ = tx.send(load(&file, options));
                ctx.request_repaint();
            });
        match spawned {
            Ok(_) => {
                self.status = format!("Opening {}…", path.display());
                self.loading = Some(Loading {
                    path: path.to_owned(),
                    loaded: rx,
                });
            }
            Err(e) => self.status = format!("Could not open {}: {e}", path.display()),
        }
    }

    /// Take the image a thread has read. On failure the previous image
    /// stays, as it does for an image that was asked for again meanwhile.
    fn poll_open(&mut self, ctx: &egui::Context) {
        let Some(loading) = &self.loading else {
            return;
        };
        let loaded = match loading.loaded.try_recv() {
            Ok(loaded) => loaded,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(format!(
                "Could not open {}: reading it stopped unexpectedly.",
                loading.path.display()
            )),
        };
        let Some(Loading { path, .. }) = self.loading.take() else {
            return;
        };
        if let Some(next) = self.queued_open.take() {
            // Another image has been asked for since: that one is shown.
            self.open(ctx, &next);
            return;
        }
        let installed = loaded.and_then(|loaded| {
            self.document
                .install(loaded, self.options)
                .map_err(|e| format!("Could not use {}: {e}", path.display()))
        });
        match installed {
            Ok(()) => {
                self.path_text = path.display().to_string();
                self.shown_path = Some(path);
                self.depth_failed = None;
                // With AI depth the status follows from the estimate.
                if self.options.input != Input::Ai {
                    self.status = LOADED.into();
                }
            }
            Err(e) => self.status = e,
        }
    }

    /// The file the path box names. While the box still shows the loaded
    /// file's path as it was put there, that is the file itself: the text
    /// stands in for a name that is not Unicode with replacement characters,
    /// and read back it would name no file, or another one. Anything typed
    /// is taken as typed, less the blanks around it.
    fn typed_path(&self) -> Option<PathBuf> {
        if let Some(shown) = &self.shown_path {
            if self.path_text == shown.display().to_string() {
                return Some(shown.clone());
            }
        }
        let typed = self.path_text.trim();
        (!typed.is_empty()).then(|| PathBuf::from(typed))
    }

    /// Whether the picture is still on its way: the options ask for AI
    /// depth, and the model has neither given the image's nor failed on it.
    fn depth_pending(&self) -> bool {
        self.document
            .depth_request(self.options)
            .is_some_and(|(id, _)| self.depth_failed != Some(id))
    }

    /// With the output open and no picture: see the estimate through from
    /// here, and tell whether the output should wait for it. The page does
    /// the same when it is drawn, which it is not while the controls are
    /// minimized or show another page, and the output is up all the same.
    fn awaiting_depth(&mut self, ctx: &egui::Context) -> bool {
        if self.options.live() || self.document.result().is_some() {
            return false;
        }
        self.poll_depth();
        self.ensure_depth(ctx);
        self.document.result().is_none() && self.depth_pending()
    }

    /// Start the model when the options need a depth map there is none of
    /// yet. One estimate at a time; the next starts when it has returned.
    fn ensure_depth(&mut self, ctx: &egui::Context) {
        if self.estimating.is_some() {
            return;
        }
        let Some((id, image)) = self.document.depth_request(self.options) else {
            return;
        };
        if self.depth_failed == Some(id) {
            return;
        }
        // The thread reads the document's own image: nothing is copied.
        let image = Arc::clone(image);
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let detail = self.options.detail;
        let spawned = std::thread::Builder::new()
            .name("depth".into())
            .spawn(move || {
                let _ = tx.send(platform::estimate_depth(&image, detail));
                ctx.request_repaint();
            });
        match spawned {
            Ok(_) => {
                self.estimating = Some((id, rx));
                self.status = ESTIMATING.into();
            }
            Err(e) => {
                self.depth_failed = Some(id);
                self.status = format!("Could not start the depth model: {e}");
            }
        }
    }

    fn poll_depth(&mut self) {
        let Some((id, rx)) = &self.estimating else {
            return;
        };
        let id = *id;
        let estimated = match rx.try_recv() {
            Ok(estimated) => estimated,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("The depth model stopped unexpectedly.".into()),
        };
        self.estimating = None;
        // Both errors and successful maps belong to the request that made
        // them. A late error must not close a newer image's output, and an
        // estimate is irrelevant after leaving AI mode.
        if id != self.document.source_id || self.options.input != Input::Ai {
            return;
        }
        match estimated.and_then(|depth| self.document.set_depth(id, depth, self.options)) {
            Ok(true) => {
                self.status = if self.output_open {
                    OUTPUT_ON
                } else {
                    CONVERTED
                }
                .into();
            }
            // For an image that has been replaced; its own estimate follows.
            Ok(false) => {}
            Err(e) => {
                self.depth_failed = Some(id);
                self.output_open = false;
                self.status = e;
            }
        }
    }

    fn options_changed(&mut self) {
        let converted = self.document.reconvert(self.options);
        self.settle(converted);
    }

    /// After a change of options: close the output and say why if the image
    /// cannot be converted with them, or say what there is now.
    fn settle(&mut self, converted: Result<(), String>) {
        if let Err(e) = converted {
            self.output_open = false;
            self.status = e;
        } else if self.document.result().is_some() {
            self.status = if self.output_open { OUTPUT_ON } else { LOADED }.into();
        }
    }

    fn toggle_output(&mut self) {
        if self.output_open {
            self.stop_output();
        } else if self.options.live() {
            self.weaver.forget_failure();
            if self.options.output != Output::Odyssey {
                self.status = LIVE_NEEDS_ODYSSEY.into();
            } else if !self.weaver.available() {
                self.status = WeaverStatus::NoHelper.describe();
            } else {
                self.output_open = true;
                self.output_fullscreen = false;
                self.status = WeaverStatus::Starting.describe();
            }
        } else if self.document.result().is_none() && !self.shows_pattern() {
            if self.document.depth_request(self.options).is_some() {
                // There is an image: what is missing is its depth, which is
                // on its way, or failed and is tried again for this request
                // (the model may have been installed since).
                self.depth_failed = None;
                self.status = DEPTH_NOT_READY.into();
            } else {
                self.status = NO_IMAGE.into();
            }
        } else if self.options.output == Output::Odyssey && !self.weaver.available() {
            self.status = WeaverStatus::NoHelper.describe();
        } else {
            self.weaver.forget_failure();
            self.output_open = true;
            self.output_fullscreen = false;
            self.status = OUTPUT_ON.into();
        }
    }

    fn stop_output(&mut self) {
        if self.output_open {
            self.output_open = false;
            self.output_fullscreen = false;
            self.status = OUTPUT_OFF.into();
        }
        self.lens.store();
        self.lens.release();
        self.lens_switch.set(false, || {});
        self.weaver.stop();
        self.woven = None;
        self.stop_live();
    }

    #[cfg(target_os = "linux")]
    fn stop_live(&mut self) {
        // Dropping it stops the capture and its threads. The screen that
        // was chosen has been noted already (`remember_screen`).
        self.live = None;
        self.saved_token = None;
    }

    /// Keep the portal's token for the screen that was chosen, so that the
    /// desktop does not ask again. A token is good for one use and each
    /// capture hands out the next one, so it is written as soon as there is
    /// one: a program that is quit while the screen shows has it on file.
    #[cfg(target_os = "linux")]
    fn remember_screen(&mut self) {
        let Some(token) = self.live.as_ref().and_then(|live| live.restore_token()) else {
            return;
        };
        self.remember_token(&any2odyssey_core::paths::capture_token_file(), token);
    }

    #[cfg(target_os = "linux")]
    fn remember_token(&mut self, file: &Path, token: String) {
        if self.forget_screen || self.saved_token.as_deref() == Some(token.as_str()) {
            return;
        }
        // Replace through a private sibling file: mode(0600) on open only
        // protects newly created files and would follow an existing symlink.
        if any2odyssey_core::fsutil::write_atomic(file, token.as_bytes()).is_ok() {
            self.saved_token = Some(token);
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn stop_live(&mut self) {}

    #[cfg(target_os = "linux")]
    fn live_controls(&self) -> crate::live::Controls {
        crate::live::Controls {
            depth_pos: self.options.depth_pos,
            pop_pos: self.options.pop_pos,
            sharpness: self.options.sharp_pos,
            steadiness: self.options.steady_pos,
            lines: self.options.lines,
            detail: self.options.detail as i32,
            swap: self.options.swap,
        }
    }

    /// The live screen through the weaver: start the capture once the
    /// weaver has a picture file, keep its controls current, and say how it
    /// is going. Returns false when the output is over.
    #[cfg(target_os = "linux")]
    fn show_live(&mut self, ctx: &egui::Context, weaver: &WeaverStatus) -> bool {
        use crate::capture::CaptureState;
        let controls = self.live_controls();
        if self.live.is_none() {
            let token = std::fs::read_to_string(any2odyssey_core::paths::capture_token_file())
                .ok()
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty());
            let wake = ctx.clone();
            self.forget_screen = false;
            self.live = Some(crate::live::Live::start(
                token,
                self.options.pointer,
                controls,
                move || wake.request_repaint(),
            ));
        }
        let Some(live) = &mut self.live else {
            return true;
        };
        live.set(controls);
        self.remember_screen();
        let Some(live) = &mut self.live else {
            return true;
        };
        // The output comes up once there is a screen to show: before that
        // its window would only cover the desktop's dialog.
        if matches!(live.capture_state(), CaptureState::Running { .. }) {
            if !self.weaver.is_running() {
                let wake = ctx.clone();
                self.weaver.start(true, move || wake.request_repaint());
            }
            if !live.has_output() {
                if let Some(sink) = self.weaver.sink() {
                    live.show_on(sink);
                }
            }
        }
        let stats = live.stats();
        match live.capture_state() {
            CaptureState::Asking => {
                self.status = "Choose the screen to show in 3D in the desktop's dialog (asked once and remembered). On KDE Plasma 6 it can be the Odyssey itself; on other desktops choose another screen.".into();
            }
            CaptureState::Running { width, height } => {
                let depth = match &stats.depth_error {
                    Some(e) => format!("no depth: {e}"),
                    None if stats.depth_rate > 0.0 => {
                        format!("depth {:.0} times a second", stats.depth_rate)
                    }
                    None => "depth starting".into(),
                };
                self.status = match &stats.output_error {
                    Some(e) => format!("The picture cannot be written: {e}"),
                    None => format!(
                        "Live: a {width} × {height} screen, {:.0} pictures a second, {depth}. {}{}",
                        stats.picture_rate,
                        weaver.describe(),
                        if self.weaver.excluded_from_capture() == Some(false) {
                            " This desktop cannot keep the 3D picture out of a capture: do not show the Odyssey's own screen."
                        } else {
                            ""
                        }
                    ),
                };
                // The rates are worth a look now and then while it runs.
                ctx.request_repaint_after(std::time::Duration::from_secs(1));
            }
            CaptureState::Ended(reason) => {
                self.stop_output();
                self.status = match reason {
                    Some(reason) => format!("Live capture ended: {reason}."),
                    None => OUTPUT_OFF.into(),
                };
                return false;
            }
        }
        true
    }

    #[cfg(not(target_os = "linux"))]
    fn show_live(&mut self, _: &egui::Context, _: &WeaverStatus) -> bool {
        self.stop_output();
        self.status = LIVE_NOT_HERE.into();
        false
    }

    /// Output through Leia's weaver: the helper has the window, this side
    /// keeps its picture current. Call once per frame while that output is
    /// selected and open.
    fn show_odyssey(&mut self, ctx: &egui::Context) {
        self.lens.release();
        self.lens_switch.set(false, || {});
        let awaiting = self.awaiting_depth(ctx);
        if !self.options.live() && self.document.result().is_none() && !awaiting {
            // Nothing to show and nothing on its way: there is no image, or
            // its depth could not be estimated (the status says why).
            self.output_open = false;
            self.weaver.stop();
            self.stop_live();
            return;
        }
        if !self.weaver.is_running() {
            if matches!(self.weaver.status(), WeaverStatus::Failed(_)) {
                // Started and failed: say why and leave it at that.
                self.output_open = false;
                self.stop_live();
                self.status = self.weaver.status().describe();
                return;
            }
            // A still picture's output starts at once. A live screen's
            // starts when the capture runs (`show_live`), with a window
            // that lets the pointer through.
            if !self.options.live() {
                let wake = ctx.clone();
                self.weaver.start(false, move || wake.request_repaint());
            }
            self.woven = None;
        }
        if self.weaver.poll() {
            // Esc in the helper's window.
            self.stop_output();
            return;
        }
        let status = self.weaver.status();
        if let WeaverStatus::Failed(_) = status {
            self.output_open = false;
            self.stop_live();
            self.status = status.describe();
            return;
        }
        if self.options.live() {
            self.show_live(ctx, &status);
            return;
        }
        self.stop_live();
        let wanted = Woven {
            generation: self.document.generation,
            flat: self.options.input == Input::Flat,
            stretch: self.options.eye_stretch(),
        };
        if self.woven != Some(wanted) {
            let picture = match self.document.result() {
                Some(pair) => weaver::compose_still(pair, wanted.flat, wanted.stretch),
                // The image's depth is still being estimated (a new image,
                // another depth detail, AI depth just chosen). The output
                // stays as it is, helper, lens and eye tracking included,
                // and shows black instead of the picture before until the
                // new one is there: stopping the helper would take the
                // lens away and half a minute to bring back.
                None => RgbaImage::from_pixel(
                    weaver::STILL_VIEW.0 * 2,
                    weaver::STILL_VIEW.1,
                    image::Rgba([0, 0, 0, 255]),
                ),
            };
            self.weaver
                .show(picture.width(), picture.height(), picture.as_raw());
            self.woven = Some(wanted);
        }
        if self.document.result().is_some() {
            self.status = match status {
                WeaverStatus::On { .. } => format!("{ODYSSEY_ON} {}", status.describe()),
                _ => status.describe(),
            };
        }
    }

    /// Run a blocking file dialog off the UI thread; `poll_dialog` collects
    /// the answer. One dialog at a time.
    fn pick(&mut self, ctx: &egui::Context, save: bool) {
        if self.picking.is_some() || self.pending_save.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("file-dialog".into())
            .spawn(move || {
                let picked = if save {
                    Picked::Save(platform::pick_save_png(DEFAULT_EXPORT_NAME))
                } else {
                    Picked::Open(platform::pick_image())
                };
                let _ = tx.send(picked);
                ctx.request_repaint();
            });
        match spawned {
            Ok(_) => self.picking = Some((rx, self.document.source_id)),
            Err(e) => self.status = format!("Could not open a file dialog: {e}"),
        }
    }

    fn poll_dialog(&mut self, ctx: &egui::Context) {
        let Some((rx, asked_for)) = &self.picking else {
            return;
        };
        let asked_for = *asked_for;
        let picked = match rx.try_recv() {
            Ok(picked) => picked,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.picking = None;
                return;
            }
        };
        self.picking = None;
        match picked {
            Picked::Open(Ok(Some(path))) => self.open(ctx, &path),
            // The dialog was about the image that was loaded when it was
            // opened; another one since (dropped, or handed over by another
            // launch) is not saved under the name chosen for that one.
            Picked::Save(Ok(Some(_))) if asked_for != self.document.source_id => {
                self.status = EXPORT_OUTDATED.into();
            }
            Picked::Save(Ok(Some(path))) => self.save_selected(path),
            Picked::Open(Ok(None)) | Picked::Save(Ok(None)) => {}
            Picked::Open(Err(e)) | Picked::Save(Err(e)) => self.status = e,
        }
    }

    fn save_selected(&mut self, selected: PathBuf) {
        let destination = with_png_extension(selected.clone());
        if destination.is_dir() {
            // Said at once, not after asking whether to replace it.
            self.status = format!(
                "Could not save {}: it is a folder. Choose a file name.",
                destination.display()
            );
            return;
        }
        if destination != selected {
            // The native dialog confirmed only `selected`. Adding the suffix
            // can select a different existing file, including a dangling
            // symlink whose target the export would create.
            match std::fs::symlink_metadata(&destination) {
                Ok(_) => {
                    self.pending_save = Some(PendingSave {
                        path: destination,
                        generation: self.document.generation,
                    });
                    return;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    self.status = format!("Could not check {}: {e}", destination.display());
                    return;
                }
            }
        }
        self.save(&destination);
    }

    fn finish_save_confirmation(&mut self, replace: bool) {
        let Some(pending) = self.pending_save.take() else {
            return;
        };
        if !replace {
            self.status = "Export cancelled.".into();
        } else if pending.generation != self.document.generation {
            self.status = EXPORT_OUTDATED.into();
        } else {
            self.save(&pending.path);
        }
    }

    fn show_save_confirmation(&mut self, ctx: &egui::Context) {
        let Some(PendingSave { path, generation }) = &self.pending_save else {
            return;
        };
        if *generation != self.document.generation {
            // The question is about a picture that is no longer the one
            // shown: it is withdrawn rather than left to be answered.
            self.pending_save = None;
            self.status = EXPORT_OUTDATED.into();
            return;
        }
        let modal = egui::Modal::new(egui::Id::new("viewer-replace-png")).show(ctx, |ui| {
            ui.set_max_width(460.0);
            ui.heading("Replace existing PNG?");
            ui.label("Adding .png selects an existing file:");
            ui.label(RichText::new(format!("{path:?}")).monospace());
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    Some(false)
                } else if ui.button("Replace").clicked() {
                    Some(true)
                } else {
                    None
                }
            })
            .inner
        });
        if let Some(replace) = modal.inner {
            self.finish_save_confirmation(replace);
        } else if modal.should_close() {
            self.finish_save_confirmation(false);
        }
    }

    fn save(&mut self, path: &Path) {
        self.status = match self.document.save_png(path) {
            Ok(()) => format!("Preview saved to {}.", path.display()),
            Err(e) => e,
        };
    }

    /// Upload the converted image when it changed. Images wider or taller
    /// than the GPU's texture limit are scaled down for display only.
    fn sync_texture(&mut self, ctx: &egui::Context) {
        if self.uploaded == self.document.generation {
            return;
        }
        self.uploaded = self.document.generation;
        let Some(result) = self.document.result() else {
            self.texture = None;
            return;
        };
        let limit = ctx.input(|i| i.max_texture_side).max(1) as u32;
        let (w, h) = result.dimensions();
        let scaled;
        let shown = if w > limit || h > limit {
            let scale = (limit as f32 / w as f32).min(limit as f32 / h as f32);
            scaled = image::imageops::resize(
                result,
                ((w as f32 * scale) as u32).max(1),
                ((h as f32 * scale) as u32).max(1),
                image::imageops::FilterType::Triangle,
            );
            &scaled
        } else {
            result
        };
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [shown.width() as usize, shown.height() as usize],
            shown.as_raw(),
        );
        self.texture = Some(ctx.load_texture("viewer-result", image, egui::TextureOptions::LINEAR));
    }

    /// Requests that reach the page from outside its widgets: images handed
    /// over by another launch, dropped files, a finished dialog and the
    /// local output shortcut.
    fn collect_input(&mut self, ctx: &egui::Context) {
        for path in platform::take_open_requests() {
            self.open(ctx, &path);
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.iter().find_map(|f| f.path.clone()));
        if let Some(path) = dropped {
            self.open(ctx, &path);
        }
        self.poll_dialog(ctx);
        self.poll_open(ctx);
        self.poll_depth();
        if ctx.input(|i| i.modifiers.ctrl && i.modifiers.alt && i.key_pressed(Key::F10)) {
            self.toggle_output();
        }
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        core: &mut Core,
        p: &Palette,
        wordmark: Option<&egui::TextureHandle>,
    ) {
        let ctx = ui.ctx().clone();
        self.collect_input(&ctx);
        banner(ui, wordmark, p);
        ui.add_space(4.0);
        section(ui, "STEREO VIEWER", p, |ui| {
            ui.horizontal(|ui| {
                let button_width = 190.0;
                let status_width = (ui.available_width() - button_width - 12.0).max(100.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(status_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_max_width(status_width);
                        ui.add(
                            egui::Label::new(RichText::new(&self.status).color(color(p.text)))
                                .wrap(),
                        );
                    },
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if self.output_open {
                        "Stop output"
                    } else {
                        "Start output"
                    };
                    if super::widgets::gold_button_sized(
                        ui,
                        label,
                        true,
                        p,
                        Some(egui::vec2(button_width, 34.0)),
                        super::widgets::display_font(13.0),
                    )
                    .clicked()
                    {
                        self.toggle_output();
                    }
                });
            });
            ui.add(egui::Label::new(RichText::new(NOTICE).color(color(p.muted)).size(12.0)).wrap());
        });
        ui.add_space(5.0);
        let before = self.options;
        let two_columns = ui.available_width() >= 860.0;
        let source_panel = |ui: &mut Ui, state: &mut ViewerState| {
            section(ui, "SOURCE & INPUT", p, |ui| {
                row(ui, "Image", |ui| {
                    let open_width = 90.0;
                    let box_width = (ui.available_width() - open_width).max(80.0);
                    let edit = ui.add(
                        egui::TextEdit::singleline(&mut state.path_text)
                            .desired_width(box_width)
                            .hint_text("Path of a PNG, JPEG or BMP"),
                    );
                    let entered = edit.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                    if entered {
                        if let Some(path) = state.typed_path() {
                            state.open(ui.ctx(), &path);
                        }
                    }
                    if gold_button(ui, "Open…", false, p).clicked() {
                        state.pick(ui.ctx(), false);
                    }
                })
                .on_hover_text("Open… uses zenity or kdialog. Without either, type the path and press Enter, or drop the file on this window.");
                row(ui, "Input format", |ui| {
                    egui::ComboBox::from_id_salt("viewer-input")
                        .width(ui.available_width())
                        .selected_text(state.options.input.label())
                        .show_ui(ui, |ui| {
                            for input in Input::ALL {
                                ui.selectable_value(&mut state.options.input, input, input.label());
                            }
                        });
                });
                ui.checkbox(&mut state.options.swap, "Swap left and right eyes");
            });
        };
        let output_panel = |ui: &mut Ui, state: &mut ViewerState| {
            section(ui, "OUTPUT", p, |ui| {
                row(ui, "Output format", |ui| {
                    egui::ComboBox::from_id_salt("viewer-output")
                        .width(ui.available_width())
                        .selected_text(state.options.output.label())
                        .show_ui(ui, |ui| {
                            for output in Output::ALL {
                                ui.selectable_value(
                                    &mut state.options.output,
                                    output,
                                    output.label(),
                                );
                            }
                        });
                });
                if matches!(state.options.input, Input::Ai | Input::Desktop) {
                    row(ui, "Depth strength", |ui| {
                        ui.add(
                            egui::Slider::new(&mut state.options.depth_pos, DEPTH.min..=DEPTH.max)
                                .show_value(false)
                                .step_by(1.0),
                        );
                        ui.label(RichText::new(DEPTH.readout(state.options.depth_pos)).monospace());
                    })
                    .on_hover_text("How far apart near and far are pushed: 0.00–2.00.");
                    row(ui, "Pop-out", |ui| {
                        ui.add(
                            egui::Slider::new(&mut state.options.pop_pos, POPOUT.min..=POPOUT.max)
                                .show_value(false)
                                .step_by(1.0),
                        );
                        ui.label(RichText::new(POPOUT.readout(state.options.pop_pos)).monospace());
                    })
                    .on_hover_text("Where the screen plane sits: −1.00 keeps everything behind the screen, 1.00 brings everything forward.");
                    row(ui, "Depth detail", |ui| {
                        let chosen = DEPTH_DETAIL
                            .iter()
                            .find(|(side, _)| *side == state.options.detail)
                            .map_or("Standard", |(_, name)| name);
                        egui::ComboBox::from_id_salt("viewer-depth-detail")
                            .width(ui.available_width())
                            .selected_text(chosen)
                            .show_ui(ui, |ui| {
                                for (side, name) in DEPTH_DETAIL {
                                    ui.selectable_value(&mut state.options.detail, side, name);
                                }
                            });
                    })
                    .on_hover_text("How large the depth model sees the picture. Finer finds thin things and edges better and takes about twice as long per step; with an NVIDIA GPU all but Finest keep up with a live screen's pictures.");
                } else {
                    ui.checkbox(
                        &mut state.options.half_width,
                        "Half-width pair: stretch each eye back to full width",
                    )
                    .on_hover_text("For red/cyan and lens output. Side-by-side output keeps the pair as it is.");
                }
                if state.options.input == Input::Desktop {
                    row(ui, "Sharpness", |ui| {
                        ui.add(
                            egui::Slider::new(&mut state.options.sharp_pos, 0..=100)
                                .show_value(false)
                                .step_by(1.0),
                        );
                        ui.label(
                            RichText::new(format!("{:3}", state.options.sharp_pos)).monospace(),
                        );
                    })
                    .on_hover_text("Sharpens the two views after they are rendered. 0 is off.");
                    row(ui, "Depth steadiness", |ui| {
                        ui.add(
                            egui::Slider::new(&mut state.options.steady_pos, 0..=95)
                                .show_value(false)
                                .step_by(1.0),
                        );
                        ui.label(RichText::new(format!("{:3}", state.options.steady_pos)).monospace());
                    })
                    .on_hover_text("How much of the previous depth map is kept for each new one. Higher is calmer but slower to follow movement.");
                    row(ui, "Detail", |ui| {
                        let chosen = LIVE_LINES
                            .iter()
                            .find(|(lines, _)| *lines == state.options.lines)
                            .map_or("1080 lines", |(_, name)| name);
                        egui::ComboBox::from_id_salt("viewer-lines")
                            .width(ui.available_width())
                            .selected_text(chosen)
                            .show_ui(ui, |ui| {
                                for (lines, name) in LIVE_LINES {
                                    ui.selectable_value(&mut state.options.lines, lines, name);
                                }
                            });
                    })
                    .on_hover_text("The size of each eye's view. More lines are sharper and cost more processor time per picture.");
                    ui.checkbox(
                        &mut state.options.pointer,
                        "Draw the pointer into the picture",
                    )
                    .on_hover_text("For a screen other than the Odyssey. Switch it off when the Odyssey shows its own screen: the real pointer is on top of the picture there. Takes effect at the next start.");
                    if gold_button(ui, "Choose another screen", false, p)
                        .on_hover_text("Forgets the screen that was chosen, so the desktop asks again at the next start.")
                        .clicked()
                    {
                        let _ = std::fs::remove_file(any2odyssey_core::paths::capture_token_file());
                        #[cfg(target_os = "linux")]
                        {
                            state.forget_screen = true;
                        }
                    }
                }
                if state.options.output == Output::Odyssey {
                    ui.add(
                        egui::Label::new(
                            RichText::new(state.weaver.status().describe())
                                .color(color(p.muted))
                                .size(12.0),
                        )
                        .wrap(),
                    )
                    .on_hover_text("Shown by Leia's own weaver in your copy of the SR runtime under Wine: factory lens calibration and eye tracking. The picture opens full screen on the Odyssey 3D by itself.");
                }
                if state.options.output == Output::Lens {
                    ui.add_space(4.0);
                    state.lens.controls(ui, p);
                    state.lens_switch.poll();
                    ui.add(
                        egui::Label::new(
                            RichText::new(state.lens_switch.status().describe())
                                .color(color(p.muted))
                                .size(12.0),
                        )
                        .wrap(),
                    );
                }
                ui.add_space(4.0);
                if gold_button(ui, "Save preview PNG…", false, p).clicked() {
                    if state.document.result().is_none() {
                        state.status = NO_IMAGE.into();
                    } else {
                        state.pick(ui.ctx(), true);
                    }
                }
            });
        };
        if two_columns {
            ui.columns(2, |cols| {
                source_panel(&mut cols[0], self);
                output_panel(&mut cols[1], self);
            });
        } else {
            source_panel(ui, self);
            ui.add_space(5.0);
            output_panel(ui, self);
        }
        if self.options != before {
            if self.options.live() != before.live() && self.output_open {
                // A live screen and a still picture use the output differently.
                self.stop_output();
            }
            if self.options.detail != before.detail {
                self.document.forget_depth();
                self.depth_failed = None;
            }
            if self.options.input != before.input {
                // A fresh choice of input format is a reason to try the
                // model again after a failure.
                self.depth_failed = None;
            }
            // Converting is the expensive part of a change, and a slider
            // makes one at every step: only what changes the result does it.
            if self.options.conversion() != before.conversion()
                || self.options.detail != before.detail
            {
                self.options_changed();
            } else {
                self.settle(Ok(()));
            }
        }
        self.lens.save_when_settled(&ctx, std::time::Instant::now());
        self.ensure_depth(&ctx);
        self.sync_texture(&ctx);
        ui.add_space(5.0);
        section(ui, "PREVIEW", p, |ui| {
            let height = 260.0;
            let (area, _) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::hover());
            match &self.texture {
                Some(texture) => {
                    ui.painter().image(
                        texture.id(),
                        fit_centered(area, texture.size_vec2()),
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                }
                None => {
                    ui.painter().text(
                        area.center(),
                        egui::Align2::CENTER_CENTER,
                        "No image",
                        egui::FontId::proportional(14.0),
                        color(p.muted),
                    );
                }
            }
        });
        ui.add_space(6.0);
        ui.label(
            RichText::new(
                "Ctrl+Alt+F10  Output on / off (while this window has focus)   ·   F1  Help",
            )
            .color(color(p.muted))
            .size(12.0),
        );
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if gold_button(ui, "View logs", false, p).clicked() {
                core.menu(MenuCommand::ViewLogs);
            }
            if gold_button(ui, "Minimize", false, p).clicked() {
                core.requests.push(UiRequest::MinimizeToTray);
            }
            if gold_button(ui, "Quit completely", false, p).clicked() {
                core.quit();
            }
        });
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!(
                "{} {}  ·  stereo viewer",
                any2odyssey_core::APP_NAME,
                any2odyssey_core::VERSION
            ))
            .color(color(p.muted))
            .size(11.0),
        );
        self.show_save_confirmation(&ctx);
    }

    /// The output window: the converted image on black, fitted, in a window
    /// of its own that the user places on the display they want. Call once
    /// per frame, outside the page's layout.
    pub fn show_output(&mut self, ctx: &egui::Context) {
        if !self.output_open {
            // Every way the output ends comes through here on the next frame.
            self.lens_switch.set(false, || {});
            self.weaver.stop();
            self.stop_live();
            return;
        }
        if self.options.output == Output::Odyssey {
            self.show_odyssey(ctx);
            return;
        }
        // Another output format: nothing of the Odyssey output stays on.
        self.weaver.stop();
        self.stop_live();
        self.woven = None;
        let weaving = self.options.weaves() || self.shows_pattern();
        // While AI depth is being estimated the window stays, black, and
        // the picture comes into it by itself. The page uploads the picture
        // when it is drawn; with the controls minimized this does.
        let awaiting = self.awaiting_depth(ctx);
        self.sync_texture(ctx);
        let plain = self.texture.clone();
        if plain.is_none() && !self.shows_pattern() && !awaiting {
            self.output_open = false;
            self.lens.release();
            self.lens_switch.set(false, || {});
            return;
        }
        // The lens is only right for a picture interleaved for the whole
        // panel, so it is asked for exactly while that is what is shown.
        let wake = ctx.clone();
        self.lens_switch
            .set(weaving && self.output_fullscreen, move || {
                wake.request_repaint()
            });
        self.lens_switch.poll();
        let lens_status = self.lens_switch.status();
        let stretch = self.options.eye_stretch();
        let generation = self.document.generation;
        let pair = self.document.result.as_deref();
        let lens_state = &mut self.lens;
        let id = egui::ViewportId::from_hash_of("any2odyssey-output");
        let builder = egui::ViewportBuilder::default()
            .with_title("Any2Odyssey output")
            .with_app_id("any2odyssey")
            .with_inner_size([960.0, 540.0])
            .with_min_inner_size([320.0, 180.0]);
        let fullscreen = self.output_fullscreen;
        let (mut close, mut toggle, mut escape) = (false, false, false);
        ctx.show_viewport_immediate(id, builder, |ctx, _class| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(Color32::BLACK))
                .show(ctx, |ui| {
                    let area = ui.max_rect();
                    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                    let mut hint = (!fullscreen).then_some(OUTPUT_HINT.to_owned());
                    if weaving {
                        lens_state.keys(ctx);
                        // One texel per panel pixel: the picture covers the
                        // whole window and is made for exactly its pixels.
                        let scale = ctx.pixels_per_point();
                        let size = [
                            (area.width() * scale).round() as u32,
                            (area.height() * scale).round() as u32,
                        ];
                        let origin = if fullscreen {
                            (0, 0)
                        } else {
                            window_origin(ctx, scale)
                        };
                        let woven =
                            lens_state.texture(ctx, pair, generation, stretch, size, origin);
                        if let Some(woven) = woven {
                            ui.painter().image(woven.id(), area, uv, Color32::WHITE);
                        }
                        hint = match (fullscreen, lens_state.pattern) {
                            (false, _) => Some(LENS_WINDOWED.to_owned()),
                            (true, true) => Some(format!(
                                "{}   ·   {}   ·   {}",
                                lens_state.readout(),
                                lens::KEYS_HELP,
                                lens_status.describe()
                            )),
                            // The picture stays clean unless the lens that
                            // was asked for did not come on.
                            (true, false) => match &lens_status {
                                LensStatus::Failed(_) => Some(lens_status.describe()),
                                _ => None,
                            },
                        };
                    } else if let Some(texture) = &plain {
                        ui.painter().image(
                            texture.id(),
                            fit_centered(area, texture.size_vec2()),
                            uv,
                            Color32::WHITE,
                        );
                    }
                    if let Some(hint) = hint {
                        let at = pos2(area.center().x, area.bottom() - 14.0);
                        let font = egui::FontId::proportional(13.0);
                        let text = ui
                            .painter()
                            .layout_no_wrap(hint, font, Color32::from_gray(210));
                        let frame = egui::Align2::CENTER_BOTTOM
                            .anchor_size(at, text.size())
                            .expand(6.0);
                        // A dark plate: the pattern behind it may be white.
                        ui.painter()
                            .rect_filled(frame, 4.0, Color32::from_black_alpha(200));
                        ui.painter()
                            .galley(frame.min + egui::vec2(6.0, 6.0), text, Color32::WHITE);
                    }
                    if ui
                        .interact(area, ui.id().with("output"), Sense::click())
                        .double_clicked()
                    {
                        toggle = true;
                    }
                });
            ctx.input(|i| {
                toggle |= i.key_pressed(Key::F11);
                escape |= i.key_pressed(Key::Escape);
                close |= i.viewport().close_requested();
            });
        });
        if escape && !fullscreen {
            close = true;
        }
        if close {
            self.stop_output();
            return;
        }
        if toggle || (escape && fullscreen) {
            self.output_fullscreen = !fullscreen;
            ctx.send_viewport_cmd_to(id, ViewportCommand::Fullscreen(self.output_fullscreen));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(windows))]
    #[test]
    fn the_depth_detail_choices_are_the_models() {
        assert_eq!(DEPTH_DETAIL, crate::depth::DETAIL);
    }

    /// A 4x2 pair: the left eye is pure red, the right eye pure blue.
    fn pair() -> RgbaImage {
        RgbaImage::from_fn(4, 2, |x, _| {
            if x < 2 {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            }
        })
    }

    #[test]
    fn options_choose_the_stereo_layout() {
        let mut options = Options::default();
        assert_eq!(options.layout(), Layout::Anaglyph);
        options.output = Output::Sbs;
        assert_eq!(options.layout(), Layout::Sbs);
        // A 2D image is shown as it is, whatever output format is selected.
        options.input = Input::Flat;
        assert_eq!(options.layout(), Layout::Flat);
        options.output = Output::Anaglyph;
        assert_eq!(options.layout(), Layout::Flat);
    }

    #[test]
    fn a_pair_becomes_an_anaglyph_and_options_reconvert_it() {
        let mut document = Document::default();
        let mut options = Options::default();
        document.set_source(pair(), options).unwrap();
        // Half-width pair stretched back to full width: left eye's red with
        // the right eye's blue in every pixel.
        let anaglyph = document.result().unwrap().clone();
        assert_eq!(anaglyph.dimensions(), (4, 2));
        assert_eq!(anaglyph.get_pixel(0, 0).0, [255, 0, 255, 255]);
        let generation = document.generation;

        options.half_width = false;
        document.reconvert(options).unwrap();
        assert_eq!(document.result().unwrap().dimensions(), (2, 2));
        assert!(document.generation > generation, "the page must re-upload");

        options.output = Output::Sbs;
        options.swap = true;
        document.reconvert(options).unwrap();
        let swapped = document.result().unwrap();
        assert_eq!(swapped.dimensions(), (4, 2));
        assert_eq!(swapped.get_pixel(0, 0).0, [0, 0, 255, 255]);
        assert_eq!(swapped.get_pixel(3, 0).0, [255, 0, 0, 255]);
    }

    #[test]
    fn an_odd_width_is_refused_as_a_pair_and_accepted_as_flat() {
        let odd = RgbaImage::from_pixel(3, 2, image::Rgba([9, 9, 9, 255]));
        let mut document = Document::default();
        let mut options = Options::default();
        let error = document.set_source(odd.clone(), options).unwrap_err();
        assert!(error.contains("even width"), "{error}");
        assert!(document.result().is_none(), "a refused image is not loaded");

        options.input = Input::Flat;
        document.set_source(odd, options).unwrap();
        assert_eq!(document.result().unwrap().dimensions(), (3, 2));
        // Back to "stereo pair": the flat image must not stay on the output
        // as if it were the converted result.
        options.input = Input::Pair;
        assert!(document.reconvert(options).is_err());
        assert!(document.result().is_none());
        // And the source is still there for the next change of mind.
        options.input = Input::Flat;
        document.reconvert(options).unwrap();
        assert!(document.result().is_some());
    }

    #[test]
    fn appending_png_does_not_overwrite_a_different_existing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let chosen = dir.path().join("out");
        let destination = dir.path().join("out.png");
        std::fs::write(&destination, b"existing export").unwrap();
        let mut state = ViewerState::default();
        state.document.set_source(pair(), state.options).unwrap();
        let (tx, rx) = mpsc::channel();
        state.picking = Some((rx, state.document.source_id));
        tx.send(Picked::Save(Ok(Some(chosen.clone())))).unwrap();
        state.poll_dialog(&egui::Context::default());
        assert_eq!(std::fs::read(&destination).unwrap(), b"existing export");
        assert_eq!(pending_path(&state), Some(&destination));
        state.finish_save_confirmation(false);
        assert!(state.pending_save.is_none());
        assert_eq!(std::fs::read(&destination).unwrap(), b"existing export");
        state.save_selected(chosen);
        state.finish_save_confirmation(true);
        assert!(state.pending_save.is_none());
        assert_eq!(
            image::open(destination).unwrap().to_rgba8(),
            *state.document.result().unwrap()
        );
    }

    #[test]
    fn new_png_destinations_and_unchanged_names_need_no_extra_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = ViewerState::default();
        state.document.set_source(pair(), state.options).unwrap();
        let chosen = dir.path().join("new export");
        let destination = with_png_extension(chosen.clone());
        state.save_selected(chosen);
        assert!(state.pending_save.is_none());
        assert!(destination.exists());
        // An exact PNG path has already had the native dialog's overwrite check.
        std::fs::write(&destination, b"old").unwrap();
        state.save_selected(destination.clone());
        assert!(state.pending_save.is_none());
        assert_eq!(
            image::open(destination).unwrap().to_rgba8(),
            *state.document.result().unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_appended_png_that_is_a_dangling_symlink_requires_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let chosen = dir.path().join("link");
        let destination = with_png_extension(chosen.clone());
        let missing = dir.path().join("missing-target.png");
        std::os::unix::fs::symlink(&missing, &destination).unwrap();
        let mut state = ViewerState::default();
        state.document.set_source(pair(), state.options).unwrap();
        state.save_selected(chosen.clone());
        assert_eq!(pending_path(&state), Some(&destination));
        assert_eq!(std::fs::read_link(&destination).unwrap(), missing);
        state.finish_save_confirmation(false);
        assert_eq!(std::fs::read_link(&destination).unwrap(), missing);
        // Confirmed, the export goes where the link leads, and the link
        // stays a link.
        state.save_selected(chosen);
        state.finish_save_confirmation(true);
        assert_eq!(std::fs::read_link(&destination).unwrap(), missing);
        assert_eq!(
            image::open(&missing).unwrap().to_rgba8(),
            *state.document.result().unwrap()
        );
    }

    fn pending_path(state: &ViewerState) -> Option<&PathBuf> {
        state.pending_save.as_ref().map(|pending| &pending.path)
    }

    #[cfg(unix)]
    #[test]
    fn an_export_follows_links_to_the_file_and_leaves_what_it_may_not_write() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let mut document = Document::default();
        document.set_source(pair(), Options::default()).unwrap();
        let exported = |path: &Path| image::open(path).unwrap().to_rgba8();

        // A link to a link to a file in another folder, one of them relative.
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir(&elsewhere).unwrap();
        let file = elsewhere.join("picture.png");
        std::fs::write(&file, b"old export").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
        let inner = dir.path().join("inner.png");
        std::os::unix::fs::symlink("elsewhere/picture.png", &inner).unwrap();
        let outer = dir.path().join("outer.png");
        std::os::unix::fs::symlink(&inner, &outer).unwrap();
        assert_eq!(
            export_target(&outer).unwrap(),
            dir.path().join("elsewhere/picture.png")
        );
        document.save_png(&outer).unwrap();
        assert_eq!(exported(&file), *document.result().unwrap());
        for link in [&inner, &outer] {
            assert!(link.is_symlink(), "{link:?} is still a link");
        }
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o640,
            "the file that was replaced keeps its permissions"
        );
        assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 1);

        // Links in a circle lead nowhere.
        let (a, b) = (dir.path().join("a.png"), dir.path().join("b.png"));
        std::os::unix::fs::symlink(&a, &b).unwrap();
        std::os::unix::fs::symlink(&b, &a).unwrap();
        let error = document.save_png(&a).unwrap_err();
        assert!(error.contains("too many links"), "{error}");

        // A folder, and something that is neither file nor folder.
        let error = document.save_png(&elsewhere).unwrap_err();
        assert!(error.contains("it is a folder"), "{error}");
        let socket = dir.path().join("socket.png");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let error = document.save_png(&socket).unwrap_err();
        assert!(error.contains("not an ordinary file"), "{error}");

        // A file this user may not write is left as it is, directly and
        // through a link. (Whoever may write everything, root, may.)
        let locked = dir.path().join("locked.png");
        std::fs::write(&locked, b"keep me").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o444)).unwrap();
        let to_locked = dir.path().join("to-locked.png");
        std::os::unix::fs::symlink(&locked, &to_locked).unwrap();
        let may_write_anything = std::fs::OpenOptions::new()
            .write(true)
            .open(&locked)
            .is_ok();
        for path in [&locked, &to_locked] {
            let saved = document.save_png(path);
            if may_write_anything {
                saved.unwrap();
                assert_eq!(exported(&locked), *document.result().unwrap());
            } else {
                let error = saved.unwrap_err();
                assert!(error.contains("cannot be written to"), "{error}");
                assert_eq!(std::fs::read(&locked).unwrap(), b"keep me");
            }
            assert_eq!(
                std::fs::metadata(&locked).unwrap().permissions().mode() & 0o777,
                0o444
            );
        }
    }

    #[test]
    fn a_folder_is_refused_before_any_question_and_a_new_file_needs_none() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = ViewerState::default();
        state.document.set_source(pair(), state.options).unwrap();
        // "out" becomes "out.png", which is a folder.
        let folder = dir.path().join("out.png");
        std::fs::create_dir(&folder).unwrap();
        for chosen in [dir.path().join("out"), folder.clone()] {
            state.status.clear();
            state.save_selected(chosen);
            assert!(state.pending_save.is_none(), "no question about a folder");
            assert!(state.status.contains("it is a folder"), "{}", state.status);
        }
        assert!(folder.is_dir());
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
        assert_eq!(
            export_target(&dir.path().join("new.png")).unwrap(),
            dir.path().join("new.png")
        );
    }

    #[test]
    fn an_answer_about_one_picture_does_not_save_another() {
        let dir = tempfile::tempdir().unwrap();
        let chosen = dir.path().join("out");
        let destination = dir.path().join("out.png");
        let other = RgbaImage::from_pixel(6, 2, image::Rgba([9, 8, 7, 255]));
        let ctx = egui::Context::default();

        // Replace is pressed after another image has arrived.
        let mut state = ViewerState::default();
        state.document.set_source(pair(), state.options).unwrap();
        std::fs::write(&destination, b"existing export").unwrap();
        state.save_selected(chosen.clone());
        assert!(state.pending_save.is_some());
        state
            .document
            .set_source(other.clone(), state.options)
            .unwrap();
        state.finish_save_confirmation(true);
        assert_eq!(std::fs::read(&destination).unwrap(), b"existing export");
        assert_eq!(state.status, EXPORT_OUTDATED);
        assert!(state.pending_save.is_none());

        // The question itself goes away when the picture changes under it,
        // also for a change of the same image (its depth arriving).
        state.save_selected(chosen.clone());
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            state.show_save_confirmation(ctx)
        });
        assert!(
            state.pending_save.is_some(),
            "the same picture: still asked"
        );
        state.options.swap = true;
        state.options_changed();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            state.show_save_confirmation(ctx)
        });
        assert!(state.pending_save.is_none());
        assert_eq!(state.status, EXPORT_OUTDATED);
        assert_eq!(std::fs::read(&destination).unwrap(), b"existing export");

        // A save dialog that was open while another image was loaded.
        let (tx, rx) = mpsc::channel();
        state.picking = Some((rx, state.document.source_id));
        state.document.set_source(pair(), state.options).unwrap();
        state.status.clear();
        tx.send(Picked::Save(Ok(Some(dir.path().join("late.png")))))
            .unwrap();
        state.poll_dialog(&ctx);
        assert_eq!(state.status, EXPORT_OUTDATED);
        assert!(!dir.path().join("late.png").exists());
        // The same image with another setting is what the dialog was about.
        let (tx, rx) = mpsc::channel();
        state.picking = Some((rx, state.document.source_id));
        state.options.swap = false;
        state.options_changed();
        tx.send(Picked::Save(Ok(Some(dir.path().join("late.png")))))
            .unwrap();
        state.poll_dialog(&ctx);
        assert_eq!(
            image::open(dir.path().join("late.png")).unwrap().to_rgba8(),
            *state.document.result().unwrap()
        );
    }

    #[test]
    fn escape_cancels_the_export_confirmation_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("existing.png");
        std::fs::write(&path, b"existing export").unwrap();
        let mut state = ViewerState {
            pending_save: Some(PendingSave {
                path: path.clone(),
                generation: 0,
            }),
            ..ViewerState::default()
        };
        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            state.show_save_confirmation(ctx)
        });
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| state.show_save_confirmation(ctx));
        assert!(state.pending_save.is_none());
        assert_eq!(std::fs::read(path).unwrap(), b"existing export");
    }

    #[test]
    fn shutdown_flushes_calibration_before_the_settling_delay() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("lens.json");
        let mut state = ViewerState {
            lens: LensState::load(file.clone()),
            ..ViewerState::default()
        };
        state.lens.nudge(lens::Knob::Pitch, 2.0);
        state.save_lens();
        assert_eq!(lens::read_lens(&file).unwrap(), state.lens.lens);
    }

    #[test]
    fn open_decodes_a_file_and_save_writes_a_png_whatever_the_name() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("pair.bmp");
        pair().save(&source).unwrap();
        let mut document = Document::default();
        assert_eq!(
            document.save_png(&dir.path().join("x.png")).unwrap_err(),
            NO_IMAGE
        );
        document.open(&source, Options::default()).unwrap();

        let target = with_png_extension(dir.path().join("out.jpg"));
        assert_eq!(target, dir.path().join("out.jpg.png"));
        document.save_png(&target).unwrap();
        let written = image::open(&target).unwrap().to_rgba8();
        assert_eq!(&written, document.result().unwrap());
        assert_eq!(
            image::ImageFormat::from_path(&target).unwrap(),
            image::ImageFormat::Png
        );

        let missing = document
            .open(&dir.path().join("nope.png"), Options::default())
            .unwrap_err();
        assert!(missing.starts_with("Could not open "), "{missing}");
        assert!(document.result().is_some(), "the loaded image stays");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn failed_png_exports_preserve_the_previous_file() {
        const CHILD_PATH: &str = "ANY2ODYSSEY_TEST_PNG_FAILURE_PATH";
        const CHILD_SIZE: &str = "ANY2ODYSSEY_TEST_PNG_FAILURE_SIZE";
        if let Some(path) = std::env::var_os(CHILD_PATH) {
            let size: u32 = std::env::var(CHILD_SIZE).unwrap().parse().unwrap();
            let mut noise = 0x12345678u32;
            let source = RgbaImage::from_fn(size, size, |_, _| {
                noise ^= noise << 13;
                noise ^= noise >> 17;
                noise ^= noise << 5;
                image::Rgba(noise.to_le_bytes())
            });
            let mut document = Document::default();
            document
                .set_source(
                    source,
                    Options {
                        input: Input::Flat,
                        ..Options::default()
                    },
                )
                .unwrap();
            let error = document
                .save_png(Path::new(&path))
                .expect_err("write or final flush failure must be reported");
            assert!(error.starts_with("Could not save "), "{error}");
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let mut failures = Vec::new();
        // A tiny PNG fails only when its buffered bytes are flushed; a
        // noisy large PNG also exercises a failure partway through encoding.
        // Restrict only the child process, never this parallel test runner.
        for (blocks, size) in [("0", "2"), ("1", "128")] {
            let target = dir.path().join(format!("preview-{size}.png"));
            std::fs::write(&target, b"previous export").unwrap();
            let output = std::process::Command::new("sh")
                .args(["-c", "ulimit -f \"$1\"; trap '' XFSZ; exec \"$2\" --exact ui::viewer::tests::failed_png_exports_preserve_the_previous_file --nocapture", "png-save-test", blocks])
                .arg(std::env::current_exe().unwrap())
                .env(CHILD_PATH, &target)
                .env(CHILD_SIZE, size)
                .output().unwrap();
            if !output.status.success() || std::fs::read(&target).unwrap() != b"previous export" {
                failures.push(format!(
                    "{size}x{size}: {}\n{}\n{}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            2,
            "no abandoned staging files"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn saved_capture_tokens_are_private_even_when_replacing_a_public_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("capture-token");
        std::fs::write(&path, b"old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let mut viewer = ViewerState::default();
        viewer.remember_token(&path, "next token".into());
        assert_eq!(std::fs::read(&path).unwrap(), b"next token");
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(viewer.saved_token.as_deref(), Some("next token"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn saving_a_capture_token_does_not_follow_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("unrelated");
        let path = dir.path().join("capture-token");
        std::fs::write(&target, b"preserve me").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        ViewerState::default().remember_token(&path, "next token".into());
        assert_eq!(std::fs::read(&target).unwrap(), b"preserve me");
        assert_eq!(std::fs::read(&path).unwrap(), b"next token");
        assert!(!std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn capture_token_failure_is_retried_and_forgetting_prevents_a_new_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("capture-token");
        std::fs::create_dir(&path).unwrap();
        let mut viewer = ViewerState::default();
        viewer.remember_token(&path, "next token".into());
        assert!(viewer.saved_token.is_none());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        std::fs::remove_dir(&path).unwrap();
        viewer.remember_token(&path, "next token".into());
        assert_eq!(viewer.saved_token.as_deref(), Some("next token"));
        viewer.forget_screen = true;
        viewer.remember_token(&path, "forgotten token".into());
        assert_eq!(std::fs::read(path).unwrap(), b"next token");
    }

    #[test]
    fn lens_output_keeps_the_pair_and_interleaves_only_stereo() {
        let mut options = Options {
            output: Output::Lens,
            ..Options::default()
        };
        // The document holds the side-by-side pair; the output window makes
        // the interleaved picture for the pixels it covers.
        assert_eq!(options.layout(), Layout::Sbs);
        assert!(options.weaves());
        assert_eq!(options.eye_stretch(), 2.0, "half-width pair by default");
        options.half_width = false;
        assert_eq!(options.eye_stretch(), 1.0);
        // The AI pair is rendered at full width per eye whatever the box says.
        options.half_width = true;
        options.input = Input::Ai;
        assert_eq!(options.eye_stretch(), 1.0);
        assert!(options.weaves());
        // A flat image has no second eye to interleave.
        options.input = Input::Flat;
        assert_eq!(options.layout(), Layout::Flat);
        assert!(!options.weaves());

        let mut document = Document::default();
        options.input = Input::Pair;
        document.set_source(pair(), options).unwrap();
        assert_eq!(document.result().unwrap(), &pair());
    }

    #[test]
    fn the_calibration_pattern_opens_the_output_without_an_image() {
        let mut state = ViewerState::default();
        state.lens.pattern = true;
        // The pattern belongs to lens output only.
        state.toggle_output();
        assert!(!state.output_open);
        assert_eq!(state.status, NO_IMAGE);

        state.options.output = Output::Lens;
        state.toggle_output();
        assert!(state.output_open);
        assert_eq!(state.status, OUTPUT_ON);
        state.toggle_output();
        assert!(!state.output_open);

        state.lens.pattern = false;
        state.toggle_output();
        assert!(
            !state.output_open,
            "no pattern and no image: nothing to show"
        );
    }

    fn queued_depth(state: &mut ViewerState) -> mpsc::Sender<Result<DepthMap, String>> {
        let (tx, rx) = mpsc::channel();
        state.estimating = Some((state.document.source_id, rx));
        tx
    }

    #[test]
    fn checking_for_depth_borrows_the_source_until_a_request_is_started() {
        let mut state = ViewerState::default();
        state.options.input = Input::Ai;
        state.document.set_source(pair(), state.options).unwrap();
        let (id, requested) = state.document.depth_request(state.options).unwrap();
        assert!(
            Arc::ptr_eq(requested, state.document.source.as_ref().unwrap()),
            "checking eligibility must not allocate and copy an image"
        );
        assert!(state.depth_pending());
        state.depth_failed = Some(id);
        assert!(!state.depth_pending());
        let ctx = egui::Context::default();
        for _ in 0..10 {
            state.ensure_depth(&ctx);
            assert!(
                state.estimating.is_none(),
                "failed requests stay suppressed"
            );
        }
        assert_eq!(
            Arc::strong_count(state.document.source.as_ref().unwrap()),
            1,
            "nobody else holds the image, and nobody has copied it"
        );
    }

    #[test]
    fn obsolete_depth_errors_cannot_close_a_new_image_or_replace_its_status() {
        let mut state = ViewerState::default();
        state.options.input = Input::Ai;
        state.document.set_source(pair(), state.options).unwrap();
        let tx = queued_depth(&mut state);
        state.document.set_source(pair(), state.options).unwrap();
        state.output_open = true;
        state.status = "new image".into();
        tx.send(Err("old model error".into())).unwrap();
        state.poll_depth();
        assert!(state.estimating.is_none());
        assert!(state.output_open);
        assert_eq!(state.status, "new image");
        assert_eq!(state.depth_failed, None);
        assert!(state.document.depth_request(state.options).is_some());
    }

    #[test]
    fn depth_results_and_disconnects_are_ignored_after_leaving_ai_mode() {
        for outcome in [
            Some(Err("old model error".into())),
            Some(Ok(DepthMap::normalized(1, 1, vec![1.0]).unwrap())),
            None,
        ] {
            let mut state = ViewerState::default();
            state.options.input = Input::Ai;
            state.document.set_source(pair(), state.options).unwrap();
            let tx = queued_depth(&mut state);
            state.options.input = Input::Flat;
            state.options_changed();
            state.toggle_output();
            if let Some(outcome) = outcome {
                tx.send(outcome).unwrap();
            }
            drop(tx);
            let generation = state.document.generation;
            state.poll_depth();
            assert!(state.output_open);
            assert_eq!(state.status, OUTPUT_ON);
            assert_eq!(state.document.generation, generation);
            assert!(state.document.depth.is_none());
            assert_eq!(state.depth_failed, None);
        }
    }

    #[test]
    fn changing_detail_invalidates_old_errors_but_current_errors_are_reported() {
        let mut state = ViewerState::default();
        state.options.input = Input::Ai;
        state.document.set_source(pair(), state.options).unwrap();
        let tx = queued_depth(&mut state);
        state.document.forget_depth();
        state.status = "finer detail".into();
        tx.send(Err("old detail error".into())).unwrap();
        state.poll_depth();
        assert_eq!(state.status, "finer detail");
        assert_eq!(state.depth_failed, None);

        let tx = queued_depth(&mut state);
        state.output_open = true;
        tx.send(Err("current error".into())).unwrap();
        state.poll_depth();
        assert!(!state.output_open);
        assert_eq!(state.status, "current error");
        assert_eq!(state.depth_failed, Some(state.document.source_id));
    }

    #[test]
    fn png_extension_is_added_only_when_missing() {
        for (given, expected) in [
            ("a.png", "a.png"),
            ("a.PNG", "a.PNG"),
            ("a", "a.png"),
            ("a.jpeg", "a.jpeg.png"),
            ("dir.d/a", "dir.d/a.png"),
        ] {
            assert_eq!(
                with_png_extension(PathBuf::from(given)),
                PathBuf::from(expected)
            );
        }
    }

    #[test]
    fn the_image_is_fitted_and_centred_without_distortion() {
        let area = Rect::from_min_size(pos2(10.0, 20.0), egui::vec2(400.0, 100.0));
        let wide = fit_centered(area, egui::vec2(800.0, 100.0));
        assert_eq!(wide.size(), egui::vec2(400.0, 50.0));
        assert_eq!(wide.center(), area.center());
        let tall = fit_centered(area, egui::vec2(50.0, 100.0));
        assert_eq!(tall.size(), egui::vec2(50.0, 100.0));
        assert_eq!(
            fit_centered(area, egui::Vec2::ZERO).size(),
            egui::Vec2::ZERO
        );
    }

    /// A stand-in for the weaver's helper that says where its frame file
    /// is, reports a picture and stays until it is let go.
    #[cfg(unix)]
    fn odyssey_showing(dir: &Path, transition: usize) -> (ViewerState, PathBuf, egui::Context) {
        use std::os::unix::fs::PermissionsExt;
        use std::time::{Duration, Instant};
        let said = dir.join(format!("frames-{transition}"));
        let helper = dir.join(format!("weaver-{transition}.sh"));
        std::fs::write(
            &helper,
            // The name is there whole or not at all, and then the word the
            // real helper prints once a picture is showing.
            format!(
                "#!/bin/sh\nprintf '%s' \"$1\" > '{0}.tmp' && mv '{0}.tmp' '{0}'\necho on\ncat >/dev/null\n",
                said.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut state = ViewerState::default();
        state.options.output = Output::Odyssey;
        if transition != 0 {
            state.options.input = Input::Ai;
        }
        state.document.set_source(pair(), state.options).unwrap();
        if transition != 0 {
            state
                .document
                .set_depth(state.document.source_id, a_depth_map(), state.options)
                .unwrap();
        }
        assert!(state.document.result().is_some());
        state.weaver = Weaver::with_helper(Some(helper));
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            state.weaver.start(false, || {});
            if !matches!(state.weaver.status(), WeaverStatus::Failed(ref e) if e.contains("busy")) {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        while !matches!(state.weaver.status(), WeaverStatus::On { .. }) || !said.exists() {
            assert!(!state.weaver.poll(), "helper stopped");
            assert!(!matches!(state.weaver.status(), WeaverStatus::Failed(_)));
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let frames = PathBuf::from(std::fs::read_to_string(&said).unwrap());
        state.output_open = true;
        let ctx = egui::Context::default();
        // The picture is on the Odyssey before anything changes.
        state.show_output(&ctx);
        assert!(state.output_open && state.weaver.is_running());
        let (size, black) = picture_in(&frames);
        assert_eq!(size, (weaver::STILL_VIEW.0 * 2, weaver::STILL_VIEW.1));
        assert!(!black, "the still picture is showing");
        (state, frames, ctx)
    }

    #[cfg(unix)]
    fn a_depth_map() -> DepthMap {
        DepthMap::normalized(2, 1, vec![0.0, 1.0]).unwrap()
    }

    /// The size of the picture the weaver's frame file holds now, and
    /// whether it is all black (`weaver`'s module text describes the file).
    #[cfg(unix)]
    fn picture_in(frames: &Path) -> ((u32, u32), bool) {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(frames).unwrap();
        let mut header = [0u8; 28];
        file.read_exact(&mut header).unwrap();
        let field = |at: usize| u32::from_le_bytes(header[at..at + 4].try_into().unwrap());
        let (capacity, sequence, width, height, place) =
            (field(8), field(12), field(16), field(20), field(24));
        assert_eq!(sequence % 2, 0, "a finished picture");
        let mut pixels = vec![0u8; width as usize * height as usize * 4];
        file.seek(SeekFrom::Start(
            4096 + u64::from(place) * u64::from(capacity),
        ))
        .unwrap();
        file.read_exact(&mut pixels).unwrap();
        let black = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [0, 0, 0, 255]);
        ((width, height), black)
    }

    /// The three ways a picture that is showing comes to wait for its depth.
    #[cfg(unix)]
    fn make_depth_pending(state: &mut ViewerState, transition: usize) {
        match transition {
            // AI depth is chosen for the image.
            0 => {
                state.options.input = Input::Ai;
                state.options_changed();
            }
            // Another depth detail.
            1 => {
                state.document.forget_depth();
                state.options_changed();
            }
            // Another image.
            _ => state.document.set_source(pair(), state.options).unwrap(),
        }
        assert!(state.document.result().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn odyssey_output_goes_black_while_depth_is_pending_and_resumes_by_itself() {
        let dir = tempfile::tempdir().unwrap();
        for transition in 0..3 {
            let (mut state, frames, ctx) = odyssey_showing(dir.path(), transition);
            make_depth_pending(&mut state, transition);
            // The estimate is running (no model is started by this test).
            let estimate = queued_depth(&mut state);
            state.status = ESTIMATING.into();
            state.show_output(&ctx);
            assert!(
                state.output_open,
                "transition {transition} closed the output"
            );
            assert!(state.weaver.is_running(), "the helper stays, lens and all");
            assert_eq!(state.status, ESTIMATING);
            let (size, black) = picture_in(&frames);
            assert_eq!(size, (weaver::STILL_VIEW.0 * 2, weaver::STILL_VIEW.1));
            assert!(black, "the picture before survives transition {transition}");
            // Waiting writes nothing more.
            let written = state.woven;
            state.show_output(&ctx);
            assert_eq!(state.woven, written);
            assert!(state.output_open && state.estimating.is_some());

            // The estimate arrives while only the output is being drawn (the
            // controls are minimized): the picture comes back by itself.
            estimate.send(Ok(a_depth_map())).unwrap();
            state.show_output(&ctx);
            assert!(state.output_open && state.weaver.is_running());
            assert!(state.document.result().is_some());
            assert!(!picture_in(&frames).1, "the new picture is showing");
            assert!(state.status.starts_with(ODYSSEY_ON), "{}", state.status);
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_estimate_closes_the_waiting_odyssey_output_with_its_message() {
        let dir = tempfile::tempdir().unwrap();
        let (mut state, _frames, ctx) = odyssey_showing(dir.path(), 2);
        make_depth_pending(&mut state, 2);
        let estimate = queued_depth(&mut state);
        state.show_output(&ctx);
        assert!(state.output_open && state.weaver.is_running());
        estimate
            .send(Err("the model is not installed".into()))
            .unwrap();
        state.show_output(&ctx);
        assert!(!state.output_open);
        assert!(!state.weaver.is_running());
        assert_eq!(state.status, "the model is not installed");
        assert_eq!(state.depth_failed, Some(state.document.source_id));
        // And it stays closed: the failed image is not tried again.
        state.show_output(&ctx);
        assert!(!state.output_open && state.estimating.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn stopping_the_output_during_the_wait_is_not_undone_by_the_estimate() {
        let dir = tempfile::tempdir().unwrap();
        let (mut state, _frames, ctx) = odyssey_showing(dir.path(), 1);
        make_depth_pending(&mut state, 1);
        let estimate = queued_depth(&mut state);
        state.show_output(&ctx);
        assert!(state.output_open);
        state.toggle_output();
        assert!(!state.output_open);
        assert_eq!(state.status, OUTPUT_OFF);
        state.show_output(&ctx);
        assert!(!state.weaver.is_running());
        estimate.send(Ok(a_depth_map())).unwrap();
        // The page takes the estimate; the output stays off.
        state.poll_depth();
        state.show_output(&ctx);
        assert!(state.document.result().is_some());
        assert!(!state.output_open && !state.weaver.is_running());
        assert_eq!(state.status, CONVERTED);
        // Started again by the user, it shows the new picture.
        assert!(state.woven.is_none());
    }

    #[test]
    fn the_output_window_waits_for_depth_too_and_closes_when_there_is_none_to_come() {
        let ctx = egui::Context::default();
        let frame = |state: &mut ViewerState| {
            let _ = ctx.run(egui::RawInput::default(), |ctx| state.show_output(ctx));
        };
        let depth = || DepthMap::normalized(2, 1, vec![0.0, 1.0]).unwrap();
        let mut state = ViewerState::default();
        state.options.input = Input::Ai;
        state.options.output = Output::Sbs;
        state.document.set_source(pair(), state.options).unwrap();
        state
            .document
            .set_depth(state.document.source_id, depth(), state.options)
            .unwrap();
        state.toggle_output();
        frame(&mut state);
        assert!(state.output_open && state.texture.is_some());

        // Another image: its depth is on its way.
        state.document.set_source(pair(), state.options).unwrap();
        let estimate = queued_depth(&mut state);
        frame(&mut state);
        assert!(state.output_open, "the window waits");
        assert!(state.texture.is_none(), "black, not the picture before");
        estimate.send(Ok(depth())).unwrap();
        frame(&mut state);
        assert!(state.output_open && state.texture.is_some());
        assert_eq!(state.status, OUTPUT_ON);

        // Depth that fails closes it, with the reason.
        state.document.set_source(pair(), state.options).unwrap();
        let estimate = queued_depth(&mut state);
        frame(&mut state);
        assert!(state.output_open);
        estimate.send(Err("no model".into())).unwrap();
        frame(&mut state);
        assert!(!state.output_open);
        assert_eq!(state.status, "no model");

        // A pair that cannot be shown has nothing coming: closed at once.
        let mut state = ViewerState::default();
        state.options.input = Input::Flat;
        state.document.set_source(pair(), state.options).unwrap();
        state.toggle_output();
        state.document = Document::default();
        frame(&mut state);
        assert!(!state.output_open);
    }

    /// A PNG file that says it holds a `width` x `height` greyscale picture
    /// and holds next to nothing: the header, an empty data chunk, the end.
    fn png_claiming(width: u32, height: u32) -> Vec<u8> {
        fn crc(bytes: &[u8]) -> u32 {
            let mut crc = !0u32;
            for &byte in bytes {
                crc ^= u32::from(byte);
                for _ in 0..8 {
                    crc = (crc >> 1) ^ (0xedb8_8320 & (crc & 1).wrapping_neg());
                }
            }
            !crc
        }
        let mut file = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut chunk = |kind: &[u8; 4], data: &[u8]| {
            file.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let mut body = kind.to_vec();
            body.extend_from_slice(data);
            file.extend_from_slice(&body);
            file.extend_from_slice(&crc(&body).to_be_bytes());
        };
        let mut header = Vec::new();
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        // Eight bits, greyscale, the only compression and filter, no interlace.
        header.extend_from_slice(&[8, 0, 0, 0, 0]);
        chunk(b"IHDR", &header);
        chunk(b"IDAT", &[0x78, 0x9c, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]);
        chunk(b"IEND", &[]);
        file
    }

    #[test]
    fn an_image_too_large_to_hold_is_refused_from_its_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.png");
        let mut document = Document::default();
        document.set_source(pair(), Options::default()).unwrap();
        let generation = document.generation;
        for (width, height, says_size) in [
            // The file of the report: 22 000 pixels a side in a few bytes.
            (22_000, 22_000, false),
            (MAX_IMAGE_SIDE + 1, 1, false),
            (1, MAX_IMAGE_SIDE + 1, false),
            (0x7fff_ffff, 0x7fff_ffff, false),
            // Each side is allowed, the picture is not.
            (MAX_IMAGE_SIDE, MAX_IMAGE_SIDE, true),
            (MAX_IMAGE_SIDE, 4097, true),
        ] {
            let file = png_claiming(width, height);
            assert!(file.len() < 100);
            std::fs::write(&path, file).unwrap();
            for error in [
                decode(&path).unwrap_err(),
                load(&path, Options::default()).err().unwrap(),
                document.open(&path, Options::default()).unwrap_err(),
            ] {
                assert!(error.starts_with("Could not open "), "{error}");
                assert!(error.contains("too large"), "{width}x{height}: {error}");
                assert!(error.contains("16384 pixels a side"), "{error}");
                assert_eq!(
                    error.contains(&format!("({width} × {height} pixels)")),
                    says_size,
                    "{error}"
                );
            }
        }
        assert_eq!(document.generation, generation, "the loaded image stays");
        // The limit is not in the way of the largest picture it allows: the
        // header is accepted, and the (missing) pixels are what fails.
        std::fs::write(&path, png_claiming(MAX_IMAGE_SIDE, 4096)).unwrap();
        let error = decode(&path).unwrap_err();
        assert!(!error.contains("too large"), "{error}");
        // And an ordinary file is an ordinary image.
        pair().save(&path).unwrap();
        assert_eq!(decode(&path).unwrap(), pair());
        // Something that is not a file is not opened at all (a pipe would
        // keep the reader waiting for good).
        let error = decode(dir.path()).unwrap_err();
        assert!(error.ends_with("it is not a file."), "{error}");
        // Nor is a file no image can be as large as; its size is all that
        // is looked at (nothing of it is on disk here).
        let sparse = dir.path().join("sparse.jpg");
        let file = std::fs::File::create(&sparse).unwrap();
        if file.set_len(MAX_FILE_BYTES + 1).is_ok() {
            let error = decode(&sparse).unwrap_err();
            assert!(error.contains("too large (1073 MB on disk)"), "{error}");
        }
    }

    /// Wait for the image being read and take it.
    fn finish_opening(state: &mut ViewerState, ctx: &egui::Context) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while state.loading.is_some() {
            assert!(
                std::time::Instant::now() < deadline,
                "the image never arrived"
            );
            state.poll_open(ctx);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    #[test]
    fn an_image_is_read_off_the_ui_thread_and_the_last_one_asked_for_is_shown() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let file = |name: &str, color: u8| {
            let path = dir.path().join(name);
            RgbaImage::from_pixel(4, 2, image::Rgba([color, 0, 0, 255]))
                .save(&path)
                .unwrap();
            path
        };
        let (first, second, third) = (file("1.png", 10), file("2.png", 20), file("3.png", 30));
        let red =
            |state: &ViewerState| state.document.source.as_ref().unwrap().get_pixel(0, 0).0[0];

        let mut state = ViewerState::default();
        state.open(&ctx, &first);
        assert!(state.loading.is_some());
        assert!(
            state.document.result().is_none(),
            "nothing is read on this thread"
        );
        assert!(state.status.starts_with("Opening "), "{}", state.status);
        finish_opening(&mut state, &ctx);
        assert_eq!(red(&state), 10);
        assert_eq!(state.status, LOADED);
        assert_eq!(state.path_text, first.display().to_string());
        assert!(state.document.result().is_some());

        // Three more in a row: one is being read, the last one is next and
        // the one between is never read. The one being read is not shown.
        let source_id = state.document.source_id;
        state.open(&ctx, &second);
        state.open(&ctx, &first);
        state.open(&ctx, &third);
        assert_eq!(state.loading.as_ref().unwrap().path, second);
        assert_eq!(state.queued_open.as_ref(), Some(&third));
        finish_opening(&mut state, &ctx);
        assert_eq!(red(&state), 30);
        assert_eq!(
            state.document.source_id,
            source_id + 1,
            "one image was shown"
        );
        assert_eq!(state.path_text, third.display().to_string());

        // A file that cannot be read leaves the image and says so.
        state.open(&ctx, &dir.path().join("missing.png"));
        finish_opening(&mut state, &ctx);
        assert!(
            state.status.starts_with("Could not open "),
            "{}",
            state.status
        );
        assert_eq!(red(&state), 30);
        // So does one the options cannot take (an odd width as a pair).
        let odd = dir.path().join("odd.png");
        RgbaImage::new(3, 2).save(&odd).unwrap();
        state.open(&ctx, &odd);
        finish_opening(&mut state, &ctx);
        assert!(
            state.status.starts_with("Could not use "),
            "{}",
            state.status
        );
        assert_eq!(red(&state), 30);

        // Options changed while the file was read: converted for the new
        // ones, not shown as it was converted for the old.
        state.open(&ctx, &second);
        state.options.output = Output::Sbs;
        state.options.swap = true;
        finish_opening(&mut state, &ctx);
        let mut expected = Document::default();
        expected.open(&second, state.options).unwrap();
        assert_eq!(state.document.result(), expected.result());
        // With AI depth the image is taken and the estimate decides.
        state.options.input = Input::Ai;
        state.open(&ctx, &first);
        finish_opening(&mut state, &ctx);
        assert!(state.document.result().is_none());
        assert!(state.depth_pending());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_file_whose_name_is_not_unicode_is_opened_by_its_own_bytes() {
        use std::os::unix::ffi::OsStringExt;
        let dir = tempfile::tempdir().unwrap();
        let name = std::ffi::OsString::from_vec(b"pair-\xff\xfe.png".to_vec());
        let path = dir.path().join(name);
        let mut bytes = std::io::Cursor::new(Vec::new());
        pair()
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        std::fs::write(&path, bytes.into_inner()).unwrap();
        // What a launch hands on (`main::image_argument`, the socket's
        // `open-bytes`) is the path itself; no text stands in for it.
        let ctx = egui::Context::default();
        let mut state = ViewerState::default();
        state.open(&ctx, &path);
        finish_opening(&mut state, &ctx);
        assert_eq!(state.status, LOADED);
        assert_eq!(state.document.source.as_deref(), Some(&pair()));
    }

    #[test]
    fn an_unchanged_picture_is_shared_with_its_source_instead_of_copied() {
        let mut document = Document::default();
        let mut options = Options {
            input: Input::Flat,
            ..Options::default()
        };
        document.set_source(pair(), options).unwrap();
        let shared = |document: &Document| {
            Arc::ptr_eq(
                document.source.as_ref().unwrap(),
                document.result.as_ref().unwrap(),
            )
        };
        assert!(shared(&document), "a flat picture is its source");
        options.input = Input::Pair;
        options.output = Output::Sbs;
        document.reconvert(options).unwrap();
        assert!(shared(&document), "so is a pair shown side by side");
        options.swap = true;
        document.reconvert(options).unwrap();
        assert!(!shared(&document));
        assert_eq!(
            document.source.as_deref(),
            Some(&pair()),
            "the source is as it was"
        );
        options.output = Output::Anaglyph;
        document.reconvert(options).unwrap();
        assert!(!shared(&document));
    }

    #[test]
    fn only_options_that_change_the_result_ask_for_a_conversion() {
        let base = Options::default();
        let same = |a: Options, b: Options| a.conversion() == b.conversion();
        let with = |change: fn(&mut Options)| {
            let mut options = Options::default();
            change(&mut options);
            options
        };
        // A pair as an anaglyph: both boxes count, the AI sliders do not.
        assert!(!same(base, with(|o| o.swap = true)));
        assert!(!same(base, with(|o| o.half_width = false)));
        assert!(same(base, with(|o| o.depth_pos = 150)));
        assert!(same(base, with(|o| o.pop_pos = -40)));
        // Nor does anything of a live screen, or the model's detail (which
        // asks for another depth map instead, see `forget_depth`).
        for change in [
            (|o| o.sharp_pos = 80) as fn(&mut Options),
            |o| o.steady_pos = 5,
            |o| o.lines = 2160,
            |o| o.pointer = false,
            |o| o.detail = 1274,
        ] {
            assert!(same(base, with(change)));
        }
        // The three outputs that keep the pair side by side make one result,
        // and there half-width only changes how it is shown.
        let sbs = with(|o| o.output = Output::Sbs);
        assert!(!same(base, sbs));
        assert!(same(sbs, with(|o| o.output = Output::Lens)));
        assert!(same(sbs, with(|o| o.output = Output::Odyssey)));
        let mut narrow = sbs;
        narrow.half_width = false;
        assert!(same(sbs, narrow));
        // A flat picture is one result whatever else is set.
        let flat = with(|o| o.input = Input::Flat);
        let mut other = flat;
        other.swap = true;
        other.output = Output::Lens;
        other.half_width = false;
        assert!(same(flat, other));
        assert!(same(flat, with(|o| o.input = Input::Desktop)));
        // AI depth: the sliders count, half-width does not, and pop-out has
        // nothing to move without depth strength.
        let ai = with(|o| o.input = Input::Ai);
        let mut changed = ai;
        changed.depth_pos += 1;
        assert!(!same(ai, changed));
        changed = ai;
        changed.pop_pos += 1;
        assert!(!same(ai, changed));
        changed = ai;
        changed.half_width = false;
        assert!(same(ai, changed));
        let mut still = ai;
        still.depth_pos = 0;
        changed = still;
        changed.pop_pos = 77;
        assert!(same(still, changed));

        // What the key says is what the conversion does: equal keys, equal
        // results, for an image that tells every difference apart.
        let image = RgbaImage::from_fn(6, 3, |x, y| {
            image::Rgba([(x * 40) as u8, (y * 80) as u8, (x * y * 20) as u8, 255])
        });
        let depth = DepthMap::normalized(3, 1, vec![0.0, 0.5, 1.0]).unwrap();
        let mut seen: Vec<(Conversion, Option<RgbaImage>)> = Vec::new();
        for input in [Input::Pair, Input::Flat, Input::Ai, Input::Desktop] {
            for output in Output::ALL {
                for (half_width, swap) in
                    [(false, false), (false, true), (true, false), (true, true)]
                {
                    for (depth_pos, pop_pos) in [(0, 0), (0, 60), (200, 0), (200, 60)] {
                        let options = Options {
                            input,
                            output,
                            half_width,
                            swap,
                            depth_pos,
                            pop_pos,
                            ..Options::default()
                        };
                        let mut document = Document::default();
                        document.set_source(image.clone(), options).unwrap();
                        if input == Input::Ai {
                            let id = document.source_id;
                            document.set_depth(id, depth.clone(), options).unwrap();
                        }
                        let result = document.result().cloned();
                        let key = options.conversion();
                        match seen.iter().find(|(known, _)| *known == key) {
                            Some((_, known)) => assert_eq!(*known, result, "{options:?}"),
                            None => seen.push((key, result)),
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn output_starts_only_with_an_image_and_stops_when_the_options_break_it() {
        let mut state = ViewerState::default();
        assert_eq!(state.status, READY);
        state.toggle_output();
        assert!(!state.output_open);
        assert_eq!(state.status, NO_IMAGE);

        state.options.input = Input::Flat;
        state
            .document
            .set_source(
                RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 255])),
                state.options,
            )
            .unwrap();
        state.toggle_output();
        assert!(state.output_open);
        assert_eq!(state.status, OUTPUT_ON);

        state.options.input = Input::Pair;
        state.options_changed();
        assert!(!state.output_open, "nothing valid is left to show");
        assert!(state.status.contains("even width"));

        state.options.input = Input::Flat;
        state.options_changed();
        state.toggle_output();
        state.toggle_output();
        assert!(!state.output_open);
        assert_eq!(state.status, OUTPUT_OFF);
    }

    #[test]
    fn starting_output_before_the_depth_is_there_says_so_and_tries_a_failed_one_again() {
        let mut state = ViewerState::default();
        state.options.input = Input::Ai;
        state.document.set_source(pair(), state.options).unwrap();
        // The estimate is running: there is an image, so "open one" is wrong.
        let estimate = queued_depth(&mut state);
        state.toggle_output();
        assert!(!state.output_open);
        assert_eq!(state.status, DEPTH_NOT_READY);
        assert!(state.estimating.is_some(), "the estimate goes on");

        // It failed. Asking for output is asking for the picture again.
        estimate.send(Err("no model".into())).unwrap();
        state.poll_depth();
        assert_eq!(state.status, "no model");
        assert!(!state.depth_pending());
        state.toggle_output();
        assert!(!state.output_open);
        assert_eq!(state.status, DEPTH_NOT_READY);
        assert!(state.depth_pending(), "the next frame starts the model");

        // With the picture there, output starts.
        let estimate = queued_depth(&mut state);
        estimate
            .send(Ok(DepthMap::normalized(2, 1, vec![0.0, 1.0]).unwrap()))
            .unwrap();
        state.poll_depth();
        state.toggle_output();
        assert!(state.output_open);
        assert_eq!(state.status, OUTPUT_ON);

        // An image the options cannot take is not waiting for anything.
        let mut state = ViewerState::default();
        state.options.input = Input::Flat;
        let odd = RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 255]));
        state.document.set_source(odd, state.options).unwrap();
        state.options.input = Input::Pair;
        state.options_changed();
        state.toggle_output();
        assert!(!state.output_open);
        assert_eq!(state.status, NO_IMAGE);
    }

    #[test]
    fn the_path_box_names_the_file_it_shows_and_typed_text_as_typed() {
        let mut state = ViewerState::default();
        assert_eq!(state.typed_path(), None);
        state.path_text = "   ".into();
        assert_eq!(state.typed_path(), None);
        state.path_text = "  /tmp/typed.png ".into();
        assert_eq!(state.typed_path(), Some(PathBuf::from("/tmp/typed.png")));

        // A loaded file whose name ends in a blank is that file, not the
        // one its trimmed text would name.
        let blank = PathBuf::from("/tmp/pair.png ");
        state.shown_path = Some(blank.clone());
        state.path_text = blank.display().to_string();
        assert_eq!(state.typed_path(), Some(blank));
        // Edited, the box is text again.
        state.path_text.insert(0, ' ');
        assert_eq!(state.typed_path(), Some(PathBuf::from("/tmp/pair.png")));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn enter_in_the_path_box_reopens_a_file_whose_name_is_not_unicode() {
        use std::os::unix::ffi::OsStringExt;
        let dir = tempfile::tempdir().unwrap();
        let name = std::ffi::OsString::from_vec(b"pair-\xff\xfe.png".to_vec());
        let path = dir.path().join(name);
        let mut bytes = std::io::Cursor::new(Vec::new());
        pair()
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        std::fs::write(&path, bytes.into_inner()).unwrap();
        let ctx = egui::Context::default();
        let mut state = ViewerState::default();
        state.open(&ctx, &path);
        finish_opening(&mut state, &ctx);
        assert_eq!(state.status, LOADED);
        // The box can only show a likeness of the name, which names no file.
        assert!(state.path_text.contains('\u{fffd}'), "{}", state.path_text);
        assert!(!Path::new(&state.path_text).exists());
        // What Enter in the box opens is the file all the same.
        assert_eq!(state.typed_path(), Some(path.clone()));
        state.status.clear();
        let again = state.typed_path().unwrap();
        state.open(&ctx, &again);
        finish_opening(&mut state, &ctx);
        assert_eq!(state.status, LOADED);
        assert_eq!(state.shown_path, Some(path));
    }
}
