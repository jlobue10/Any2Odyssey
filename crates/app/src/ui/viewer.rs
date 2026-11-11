//! The stereo viewer page, the home page where Samsung's engine does not
//! exist (Linux): open a side-by-side image, turn it into red/cyan anaglyph
//! or a (swapped) side-by-side pair, preview it, show it in a separate
//! output window and save it as PNG.
//!
//! This is layout conversion only (`core::stereo`): no depth is estimated
//! and nothing here drives the Odyssey's calibrated glasses-free mode.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use any2odyssey_core::stereo::{self, Layout};
use any2odyssey_core::theme::Palette;
use egui::{pos2, Color32, Key, Rect, RichText, Sense, Ui, ViewportCommand};
use image::RgbaImage;

use super::theme::color;
use super::widgets::{banner, gold_button, row, section};
use crate::core::{Core, MenuCommand, UiRequest};
use crate::platform;

pub const READY: &str =
    "Ready. Open a side-by-side stereo image, or drop one on this window. Output starts only when requested.";
pub const NOTICE: &str = "This build is the stereo viewer. Samsung's AI depth, eye tracking and the Odyssey's calibrated glasses-free output exist only on Windows.";
const OUTPUT_HINT: &str =
    "Move this window to the display you want, then double-click or press F11 for full screen. Esc leaves.";
const LOADED: &str = "Image loaded. Check the input format before starting output.";
const NO_IMAGE: &str = "Open an image first.";
const OUTPUT_ON: &str =
    "Output window open. Stop output, Esc in that window, or closing it ends it.";
const OUTPUT_OFF: &str = "Output stopped.";
const DEFAULT_EXPORT_NAME: &str = "Any2Odyssey-preview.png";

/// What the source image contains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    /// Left and right eye side by side.
    Pair,
    /// An ordinary 2D image, shown unchanged.
    Flat,
}

impl Input {
    const ALL: [Input; 2] = [Input::Pair, Input::Flat];
    fn label(self) -> &'static str {
        match self {
            Input::Pair => "Side-by-side stereo pair",
            Input::Flat => "2D image (flat, no depth)",
        }
    }
}

/// How a stereo pair is presented.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Output {
    Anaglyph,
    Sbs,
}

impl Output {
    const ALL: [Output; 2] = [Output::Anaglyph, Output::Sbs];
    fn label(self) -> &'static str {
        match self {
            Output::Anaglyph => "Red/cyan anaglyph (needs red/cyan glasses)",
            Output::Sbs => "Side-by-side stereo (needs a matching viewer)",
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
}

impl Default for Options {
    fn default() -> Options {
        Options {
            input: Input::Pair,
            output: Output::Anaglyph,
            half_width: true,
            swap: false,
        }
    }
}

impl Options {
    pub fn layout(&self) -> Layout {
        match (self.input, self.output) {
            (Input::Flat, _) => Layout::Flat,
            (Input::Pair, Output::Anaglyph) => Layout::Anaglyph,
            (Input::Pair, Output::Sbs) => Layout::Sbs,
        }
    }
}

/// The source image and what the current options make of it. No window and
/// no GPU: this is the part the tests drive.
#[derive(Default)]
pub struct Document {
    source: Option<RgbaImage>,
    result: Option<RgbaImage>,
    /// Bumped whenever `result` changes, so the page knows to upload it.
    generation: u64,
}

impl Document {
    pub fn result(&self) -> Option<&RgbaImage> {
        self.result.as_ref()
    }

    /// Decode `path` and convert it. On failure the previous image stays.
    pub fn open(&mut self, path: &Path, options: Options) -> Result<(), String> {
        let image = image::open(path)
            .map_err(|e| format!("Could not open {}: {e}", path.display()))?
            .to_rgba8();
        self.set_source(image, options)
            .map_err(|e| format!("Could not use {}: {e}", path.display()))
    }

    pub fn set_source(&mut self, image: RgbaImage, options: Options) -> Result<(), String> {
        let converted = convert(&image, options)?;
        self.source = Some(image);
        self.result = Some(converted);
        self.generation += 1;
        Ok(())
    }

    /// Convert the loaded image again after an option changed. A source the
    /// new options cannot take (an odd width as a stereo pair) leaves no
    /// result, so nothing stale stays on the output.
    pub fn reconvert(&mut self, options: Options) -> Result<(), String> {
        let Some(source) = &self.source else {
            return Ok(());
        };
        let converted = convert(source, options);
        self.generation += 1;
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
        result
            .save_with_format(path, image::ImageFormat::Png)
            .map_err(|e| format!("Could not save {}: {e}", path.display()))
    }
}

fn convert(image: &RgbaImage, options: Options) -> Result<RgbaImage, String> {
    stereo::convert(image, options.layout(), options.half_width, options.swap)
        .map_err(|e| e.to_string())
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

pub struct ViewerState {
    document: Document,
    options: Options,
    /// The path box: what Open reads, and where a loaded file's path shows.
    path_text: String,
    status: String,
    texture: Option<egui::TextureHandle>,
    uploaded: u64,
    output_open: bool,
    output_fullscreen: bool,
    /// A file dialog running on its own thread.
    picking: Option<Receiver<Picked>>,
}

impl Default for ViewerState {
    fn default() -> ViewerState {
        ViewerState {
            document: Document::default(),
            options: Options::default(),
            path_text: String::new(),
            status: READY.into(),
            texture: None,
            uploaded: 0,
            output_open: false,
            output_fullscreen: false,
            picking: None,
        }
    }
}

impl ViewerState {
    fn open(&mut self, path: &Path) {
        match self.document.open(path, self.options) {
            Ok(()) => {
                self.path_text = path.display().to_string();
                self.status = LOADED.into();
            }
            Err(e) => self.status = e,
        }
    }

    fn options_changed(&mut self) {
        if let Err(e) = self.document.reconvert(self.options) {
            self.output_open = false;
            self.status = e;
        } else if self.document.result().is_some() {
            self.status = if self.output_open { OUTPUT_ON } else { LOADED }.into();
        }
    }

    fn toggle_output(&mut self) {
        if self.output_open {
            self.stop_output();
        } else if self.document.result().is_none() {
            self.status = NO_IMAGE.into();
        } else {
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
    }

    /// Run a blocking file dialog off the UI thread; `poll_dialog` collects
    /// the answer. One dialog at a time.
    fn pick(&mut self, ctx: &egui::Context, save: bool) {
        if self.picking.is_some() {
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
            Ok(_) => self.picking = Some(rx),
            Err(e) => self.status = format!("Could not open a file dialog: {e}"),
        }
    }

    fn poll_dialog(&mut self) {
        let Some(rx) = &self.picking else { return };
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
            Picked::Open(Ok(Some(path))) => self.open(&path),
            Picked::Save(Ok(Some(path))) => self.save(&with_png_extension(path)),
            Picked::Open(Ok(None)) | Picked::Save(Ok(None)) => {}
            Picked::Open(Err(e)) | Picked::Save(Err(e)) => self.status = e,
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
            self.open(&path);
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.iter().find_map(|f| f.path.clone()));
        if let Some(path) = dropped {
            self.open(&path);
        }
        self.poll_dialog();
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
                    if entered && !state.path_text.trim().is_empty() {
                        let path = PathBuf::from(state.path_text.trim());
                        state.open(&path);
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
                ui.checkbox(
                    &mut state.options.half_width,
                    "Half-width pair: stretch each eye for red/cyan output",
                );
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
            self.options_changed();
        }
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
    }

    /// The output window: the converted image on black, fitted, in a window
    /// of its own that the user places on the display they want. Call once
    /// per frame, outside the page's layout.
    pub fn show_output(&mut self, ctx: &egui::Context) {
        if !self.output_open {
            return;
        }
        let Some(texture) = self.texture.clone() else {
            self.output_open = false;
            return;
        };
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
                    ui.painter().image(
                        texture.id(),
                        fit_centered(area, texture.size_vec2()),
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                    if !fullscreen {
                        ui.painter().text(
                            pos2(area.center().x, area.bottom() - 14.0),
                            egui::Align2::CENTER_BOTTOM,
                            OUTPUT_HINT,
                            egui::FontId::proportional(13.0),
                            Color32::from_gray(190),
                        );
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
}
