//! The eframe application: custom chrome, pages, tray/hotkey wiring.

mod about;
mod controls;
mod help;
mod lens;
mod options_file;
mod system_fonts;
mod theme;
mod viewer;
mod widgets;

pub(crate) use viewer::resolve_export;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use any2odyssey_core::theme::Palette;
use any2odyssey_core::APP_NAME;
use egui::viewport::ResizeDirection;
use egui::{
    pos2, vec2, Align2, Color32, CornerRadius, CursorIcon, FontId, Rect, Sense, ViewportCommand,
};

use crate::core::{self, Core, MenuCommand, UiRequest};
use crate::platform;
use about::AboutState;
use controls::ControlsState;
use help::HelpState;
use theme::color;
use viewer::ViewerState;

const TITLEBAR_HEIGHT: f32 = 54.0;
const EDGE: f32 = 9.0;
/// The side of the square title bar buttons.
const CHROME_BUTTON: f32 = 34.0;
/// The smallest window, in points. Every page scrolls, so the height is
/// what leaves the title bar, the way back and a few lines of a page; the
/// width is what the title bar needs. Up to 0.9.6 the height was 580, more
/// than the whole of a 1920x1080 screen at 200 % (540 points) or of a
/// 1366x768 one at 150 % (512): the window could not be made to fit, and
/// its lower edge, the resize handles with it, lay outside the screen.
const SMALLEST_WINDOW: [f32; 2] = [700.0, 360.0];
/// The face of the Maximize button: a square the built-in typefaces have
/// (U+25FB). It was U+25A1, which none of them has, and what was drawn was
/// egui's stand-in for a missing letter, which is this square. With a
/// typeface of the system added for other text (`system_fonts`), U+25A1
/// would be drawn with that typeface's own, larger square from then on.
const MAXIMIZE: &str = "\u{25FB}";
/// The narrowest page, in points, whose title bar holds its seven buttons
/// beside the title. Below it About runs over the title, and from about 400
/// points About and Help leave the window on the left.
const NARROWEST_PAGE: f32 = 560.0;

/// The largest zoom a window `width` points wide at zoom 1 can take.
///
/// Ctrl+Plus, Ctrl+Minus and Ctrl+0 zoom every eframe window (0.2 to 5.0),
/// and the window keeps its pixels: at 2.0 the default window is a page of
/// 490 points, at 3.0 one of 327 with two title bar buttons outside it. The
/// zoom stops where the page is still whole; a wider window zooms further.
fn largest_zoom(width: f32) -> f32 {
    ((width / NARROWEST_PAGE * 10.0).floor() / 10.0).max(1.0)
}

/// Bring a zoom the window is too narrow for back to [`largest_zoom`].
fn limit_zoom(ctx: &egui::Context) {
    let zoom = ctx.zoom_factor();
    let most = largest_zoom(ctx.viewport_rect().width() * zoom);
    if zoom > most + 0.001 {
        ctx.set_zoom_factor(most);
    }
}

/// Name the smallest window again when the display's scale has changed.
///
/// The window system is told the smallest size in pixels, worked out once
/// for the display the window was made on, and a window that comes onto a
/// display of another scale keeps those pixels (dragged to a second
/// monitor, or the scale changed in Settings while the program is in the
/// tray). Seen in the window on Windows, which was sent what Windows sends
/// at such a change: made at 100 %, it still named 700 x 360 pixels as its
/// smallest size at 150 % and at 200 %, where that is 350 x 180 points and
/// the title bar's buttons lie outside the window. The other way round the
/// same pixels are too many: a window made at 200 % keeps 1400 x 720 of
/// them, which at 100 % is as many points. The size in the command is in
/// points of the page as it is zoomed, and the limit is in the display's
/// own.
fn keep_smallest_window(ctx: &egui::Context) {
    let Some(scale) = ctx.native_pixels_per_point() else {
        return;
    };
    let id = egui::Id::new("smallest-window-scale");
    let before = ctx.data_mut(|data| {
        let before = data.get_temp::<f32>(id);
        data.insert_temp(id, scale);
        before
    });
    // The window was made with the size that fits the display it was made
    // on: only a change is told.
    if before.is_some_and(|before| before != scale) {
        let smallest = egui::Vec2::from(SMALLEST_WINDOW) / ctx.zoom_factor();
        ctx.send_viewport_cmd(ViewportCommand::MinInnerSize(smallest));
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Controls,
    /// The stereo viewer: the home page where conversion does not exist.
    Viewer,
    Help,
    About,
}

/// The page the app opens on and returns to. Conversion needs Samsung's
/// Windows engine, so everywhere else the controls for it would only ever
/// report that; the stereo viewer is what those builds can do.
fn home() -> Page {
    if cfg!(windows) {
        Page::Controls
    } else {
        Page::Viewer
    }
}

pub struct App {
    core: Arc<Mutex<Core>>,
    page: Page,
    controls: ControlsState,
    viewer: ViewerState,
    help: HelpState,
    about: AboutState,
    palette: Palette,
    wordmark: Option<egui::TextureHandle>,
    #[cfg(windows)]
    tray: Option<crate::tray::Tray>,
    /// The notification icon should exist on this build and could not be
    /// created; see [`startup_notices`]. Never set where there is no tray.
    tray_failed: bool,
    _hotkeys: Option<global_hotkey::GlobalHotKeyManager>,
    hidden: bool,
    preview: bool,
    frames: u32,
    screenshot_taken: bool,
}

/// Whether the windows are Wayland surfaces. Asked once: the answer is the
/// environment this program was started in.
fn on_wayland() -> bool {
    #[cfg(not(windows))]
    {
        static ON_WAYLAND: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ON_WAYLAND.get_or_init(platform::uses_wayland)
    }
    #[cfg(windows)]
    {
        false
    }
}

/// The shortest time from one frame's start to the next where the display
/// does not space them (120 a second).
const PACED_FRAME: Duration = Duration::from_micros(8_333);

/// How long to wait before a frame that would start at `now`, when the one
/// before it started at `last`.
fn pacing_wait(last: Option<Instant>, now: Instant) -> Duration {
    last.map_or(Duration::ZERO, |last| {
        PACED_FRAME.saturating_sub(now.saturating_duration_since(last))
    })
}

/// Space this frame from the one before ([`pacing_wait`]).
fn pace(ctx: &egui::Context) {
    let id = egui::Id::new("frame-pacing");
    let last = ctx.data(|data| data.get_temp::<Instant>(id));
    let wait = pacing_wait(last, Instant::now());
    if !wait.is_zero() {
        std::thread::sleep(wait);
    }
    ctx.data_mut(|data| data.insert_temp(id, Instant::now()));
}

fn load_png(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    Some((img.into_raw(), w, h))
}

pub fn run(core: Arc<Mutex<Core>>) -> anyhow::Result<()> {
    let icon =
        load_png(include_bytes!("../../../../assets/Any2Odyssey.png")).map(|(rgba, w, h)| {
            egui::IconData {
                rgba,
                width: w,
                height: h,
            }
        });
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(APP_NAME)
        .with_app_id("any2odyssey")
        .with_inner_size([980.0, 780.0])
        .with_min_inner_size(SMALLEST_WINDOW)
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(true);
    // Above the 3D overlay, which is itself topmost. Without an overlay the
    // controls are an ordinary window and must not cover the output window.
    if cfg!(windows) {
        viewport = viewport.with_always_on_top();
    }
    if let Some(icon) = icon {
        viewport = viewport.with_icon(Arc::new(icon));
    }
    let options = eframe::NativeOptions {
        viewport,
        centered: true,
        // On Wayland a buffer swap that waits for the display waits for the
        // compositor to ask for a frame, and it asks a window nobody can
        // see for none: minimized, on another workspace, or under the
        // full-screen output window. The controls and the output window
        // are drawn by one thread, so the output stopped answering (F11,
        // Esc, a new picture) for as long as the controls were out of
        // sight, and so did `--show`. There the frames are spaced by this
        // program instead (`pace`).
        vsync: !on_wayland(),
        ..Default::default()
    };
    eframe::run_native(
        APP_NAME,
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, core)))),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, core: Arc<Mutex<Core>>) -> App {
        theme::install_fonts(&cc.egui_ctx);
        // A window title or a file name in letters those typefaces lack is
        // drawn with one of the system's, looked for when it is first shown.
        system_fonts::install(&cc.egui_ctx, {
            let core = core.clone();
            move |line: &str| {
                if let Ok(core) = core.lock() {
                    core.log(line);
                }
            }
        });
        let hwnd = window_handle(cc);
        let wordmark = load_png(include_bytes!(
            "../../../../assets/Any2Odyssey-wordmark.png"
        ))
        .map(|(rgba, w, h)| {
            let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
            cc.egui_ctx
                .load_texture("wordmark", image, egui::TextureOptions::LINEAR)
        });
        let (palette, controls) = {
            let mut c = core.lock().expect("core");
            c.controls_hwnd = hwnd;
            (
                Palette::for_dark(c.ui.dark_mode),
                ControlsState::from_core(&c),
            )
        };
        theme::apply(&cc.egui_ctx, &palette);
        let preview = core.lock().expect("core").preview;
        // An isolated preview renders and exits; it must not put a live icon
        // in the notification area or register with the shell.
        #[cfg(windows)]
        let (tray, tray_failed) = if preview {
            (None, false)
        } else {
            match load_png(include_bytes!("../../../../assets/Any2Odyssey.png"))
                .ok_or_else(|| "icon".to_owned())
                .and_then(|(rgba, w, h)| crate::tray::Tray::new(rgba, w, h))
            {
                Ok(t) => (Some(t), false),
                Err(e) => {
                    core.lock()
                        .expect("core")
                        .log(&format!("Tray icon unavailable: {e}"));
                    (None, true)
                }
            }
        };
        #[cfg(not(windows))]
        let tray_failed = false;
        if !preview {
            platform::install_jump_list();
        }
        // The global shortcuts drive conversion; the viewer has a local one.
        let converts = cfg!(windows);
        let hotkeys = if preview || !converts {
            None
        } else {
            crate::hotkeys::register()
        };
        {
            let mut c = core.lock().expect("core");
            c.hotkeys_ok = hotkeys.is_some() || preview || !converts;
            if preview {
                c.status = "Ready".into();
            } else {
                // Shortcuts are a convenience, not a prerequisite: the button,
                // tray and --toggle start conversion without them, so a
                // conflict (or a desktop without global shortcuts) is reported
                // but does not block the automatic start. The report is a
                // notice of its own (`startup_notices`), not the status line:
                // the automatic start below writes "Starting 3D" there before
                // the first frame, which is how the message was never seen.
                if !c.hotkeys_ok {
                    c.log("Global shortcuts unavailable: not all of Ctrl+Alt+F8/F9/F10 could be registered.");
                }
                if converts && (c.prefs.auto_convert || c.start_requested) {
                    c.start_conversion();
                }
            }
            if !preview
                && converts
                && c.prefs.check_updates
                && any2odyssey_core::update::check_due(
                    c.ui.last_update_check,
                    any2odyssey_core::update::now_unix(),
                )
            {
                c.check_updates();
            }
        }
        if !preview {
            core::spawn_poller(core.clone(), cc.egui_ctx.clone());
        }
        let page = if core
            .lock()
            .expect("core")
            .screenshot
            .as_ref()
            .is_some_and(|s| s.help_page)
        {
            Page::Help
        } else {
            home()
        };
        App {
            core,
            page,
            controls,
            viewer: if preview {
                // A preview's picture is of the page as it comes, whatever
                // this user last chose, and it leaves their file alone.
                ViewerState::with_saved_lens().without_saved_options()
            } else {
                ViewerState::with_saved_lens()
            },
            help: HelpState::default(),
            about: AboutState::new(),
            palette,
            wordmark,
            #[cfg(windows)]
            tray,
            tray_failed,
            _hotkeys: hotkeys,
            hidden: false,
            preview,
            frames: 0,
            screenshot_taken: false,
        }
    }

    fn set_dark(&mut self, ctx: &egui::Context, dark: bool) {
        self.palette = Palette::for_dark(dark);
        theme::apply(ctx, &self.palette);
        let mut c = self.core.lock().expect("core");
        c.ui.dark_mode = dark;
        c.save_ui();
    }

    fn tray_available(&self) -> bool {
        #[cfg(windows)]
        {
            self.tray.is_some()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    /// Whether closing or minimizing may hide the window into the tray: the
    /// preference must be on and this desktop must have a notification area
    /// to bring the window back from. Without one, a hidden window could only
    /// be recovered by a global shortcut, which not every desktop offers.
    fn hides_to_tray(&self) -> bool {
        self.tray_available() && self.core.lock().expect("core").prefs.minimize_to_tray
    }

    fn hide_to_tray(&mut self, ctx: &egui::Context) {
        // Without a native handle the only hide available is winit's, which
        // is SW_HIDE: a window that gets no paint message and therefore no
        // frame, the state 0.8.5 removed, and one show_window(0) cannot undo.
        // Treat it like a missing tray and minimize instead.
        let hwnd = self.core.lock().expect("core").controls_hwnd;
        if !self.tray_available() || hwnd == 0 {
            ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
            return;
        }
        self.core.lock().expect("core").save_preferences();
        self.hidden = true;
        platform::hide_window(hwnd);
    }

    /// Whether the window is minimized at this moment.
    ///
    /// The frame's own record (`viewport().minimized`) is taken when the
    /// frame starts, and a show request handled in the same frame restores
    /// the window after that: a frame that started while the controls were
    /// still in the tray then read "minimized" for a window it had just put
    /// back, and hid it again. The window itself is asked where it can be.
    fn minimized_now(&self, ctx: &egui::Context) -> bool {
        let hwnd = self.core.lock().expect("core").controls_hwnd;
        platform::is_minimized(hwnd)
            .unwrap_or_else(|| ctx.input(|i| i.viewport().minimized.unwrap_or(false)))
    }

    fn show_controls(&mut self, ctx: &egui::Context) {
        self.hidden = false;
        self.page = home();
        // `hide_to_tray` hides with a raw ShowWindow(SW_HIDE) that winit does
        // not observe, so its own visibility flag still says "visible" and
        // the Visible(true) command below is a no-op on that path. Un-hide at
        // the same level; the commands stay for a viewport without a handle.
        let hwnd = self.core.lock().expect("core").controls_hwnd;
        if hwnd != 0 {
            platform::show_window(hwnd);
        }
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(ViewportCommand::Focus);
        // A Wayland window can neither restore nor focus itself (the three
        // commands above do nothing there), so `--show` changed nothing on
        // the screen. Asking for attention is what it may do: the desktop
        // marks the window in its task bar.
        if on_wayland() {
            ctx.send_viewport_cmd(ViewportCommand::RequestUserAttention(
                egui::UserAttentionType::Informational,
            ));
        }
    }

    fn handle_requests(&mut self, ctx: &egui::Context) {
        let requests: Vec<UiRequest> = {
            let mut c = self.core.lock().expect("core");
            std::mem::take(&mut c.requests)
        };
        for r in requests {
            match r {
                UiRequest::ShowControls => self.show_controls(ctx),
                UiRequest::ShowHelp => {
                    self.show_controls(ctx);
                    self.help.select("start");
                    self.page = Page::Help;
                }
                UiRequest::ShowAbout => {
                    self.show_controls(ctx);
                    self.page = Page::About;
                }
                UiRequest::ToggleTheme => self.set_dark(ctx, !self.palette.dark),
                UiRequest::MinimizeToTray => self.hide_to_tray(ctx),
                UiRequest::Quit => ctx.send_viewport_cmd(ViewportCommand::Close),
            }
        }
    }

    fn resize_edges(&self, ctx: &egui::Context, screen: Rect) {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        if maximized {
            return;
        }
        let Some(pos) = ctx.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        let l = pos.x < screen.left() + EDGE;
        let r = pos.x > screen.right() - EDGE;
        let t = pos.y < screen.top() + EDGE;
        let b = pos.y > screen.bottom() - EDGE;
        let dir = match (l, r, t, b) {
            (true, _, true, _) => Some(ResizeDirection::NorthWest),
            (_, true, true, _) => Some(ResizeDirection::NorthEast),
            (true, _, _, true) => Some(ResizeDirection::SouthWest),
            (_, true, _, true) => Some(ResizeDirection::SouthEast),
            (true, _, _, _) => Some(ResizeDirection::West),
            (_, true, _, _) => Some(ResizeDirection::East),
            (_, _, true, _) => Some(ResizeDirection::North),
            (_, _, _, true) => Some(ResizeDirection::South),
            _ => None,
        };
        let Some(dir) = dir else { return };
        let cursor = match dir {
            ResizeDirection::North | ResizeDirection::South => CursorIcon::ResizeVertical,
            ResizeDirection::East | ResizeDirection::West => CursorIcon::ResizeHorizontal,
            ResizeDirection::NorthEast | ResizeDirection::SouthWest => CursorIcon::ResizeNeSw,
            ResizeDirection::NorthWest | ResizeDirection::SouthEast => CursorIcon::ResizeNwSe,
        };
        ctx.set_cursor_icon(cursor);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
        }
    }

    fn titlebar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = self.palette;
        let rect = ui.available_rect_before_wrap();
        let bar = Rect::from_min_size(rect.min, vec2(rect.width(), TITLEBAR_HEIGHT));
        let painter = ui.painter();
        painter.rect_filled(
            bar,
            CornerRadius {
                nw: 16,
                ne: 16,
                sw: 0,
                se: 0,
            },
            color(p.header),
        );
        widgets::border_lines(painter, bar, false, &p);
        painter.line_segment(
            [
                pos2(bar.left() + 12.0, bar.bottom() - 5.0),
                pos2(bar.right() - 13.0, bar.bottom() - 5.0),
            ],
            egui::Stroke::new(2.5_f32, color(p.border)),
        );
        painter.text(
            pos2(bar.left() + 22.0, bar.center().y),
            Align2::LEFT_CENTER,
            APP_NAME.to_uppercase(),
            widgets::display_font(14.0),
            color(p.text),
        );
        // Dragged and double-clicked, never focused: as a Tab stop it was the
        // first one of the window, with nothing to show for it and no name.
        let response = ui.interact(bar, ui.id().with("titlebar"), Sense::CLICK | Sense::DRAG);
        if response.drag_started() {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if response.double_clicked() {
            let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        }
        // The buttons end 14 points from the right edge and are made from
        // left to right, which is the order Tab visits them in. Laid out
        // from the right they were made, and visited, Close first.
        let theme_label = if p.dark { "Light" } else { "Dark" };
        let gap = ui.spacing().item_spacing.x;
        let width = ["About", "Help", theme_label]
            .iter()
            .map(|text| widgets::gold_button_width(ui, text))
            .sum::<f32>()
            + 4.0 * CHROME_BUTTON
            + 6.0 * gap;
        let right = bar.right() - 14.0;
        let mut actions = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    pos2(right - width, bar.top() + 6.0),
                    pos2(right, bar.bottom() - 8.0),
                ))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        let chrome = |ui: &mut egui::Ui, text: &str, tip: &str| {
            widgets::gold_button_named(
                ui,
                text,
                tip,
                false,
                &p,
                Some(vec2(CHROME_BUTTON, CHROME_BUTTON)),
                FontId::proportional(15.0),
            )
            .on_hover_text(tip)
        };
        if widgets::gold_button(&mut actions, "About", false, &p)
            .on_hover_text("Version, installation and credits")
            .clicked()
        {
            self.page = Page::About;
        }
        if widgets::gold_button(&mut actions, "Help", false, &p).clicked() {
            self.help.select("start");
            self.page = Page::Help;
        }
        if widgets::gold_button(&mut actions, theme_label, false, &p)
            .on_hover_text("Toggle light / dark mode")
            .clicked()
        {
            self.set_dark(ctx, !p.dark);
        }
        if chrome(&mut actions, "—", "Minimize").clicked() {
            if self.hides_to_tray() {
                self.hide_to_tray(ctx);
            } else {
                ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
            }
        }
        if chrome(&mut actions, MAXIMIZE, "Maximize / restore").clicked() {
            let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        }
        if chrome(&mut actions, "▾", minimize_label(self.tray_available())).clicked() {
            self.hide_to_tray(ctx);
        }
        if chrome(&mut actions, "×", "Close").clicked() {
            self.close_requested(ctx);
        }
        ui.add_space(TITLEBAR_HEIGHT);
    }

    fn close_requested(&mut self, ctx: &egui::Context) {
        let exiting = self.core.lock().expect("core").exiting;
        if !exiting && self.hides_to_tray() {
            self.hide_to_tray(ctx);
        } else {
            // Deferred quit: the window closes once the worker shutdown has finished.
            self.core.lock().expect("core").quit();
        }
    }

    fn context_menu(&mut self, ui: &mut egui::Ui) {
        let (wanted, keep) = {
            let c = self.core.lock().expect("core");
            (c.wanted(), c.prefs.keep_overlay_on_top)
        };
        let command = std::cell::Cell::new(None);
        let item = |ui: &mut egui::Ui, text: &str, cmd: MenuCommand| {
            if ui.button(text).clicked() {
                command.set(Some(cmd));
                ui.close();
            }
        };
        if !cfg!(windows) {
            // No conversion to steer: the entries that still mean something.
            item(ui, "Help", MenuCommand::Help);
            item(ui, "About", MenuCommand::About);
            item(ui, "Toggle light / dark mode", MenuCommand::ToggleTheme);
            item(ui, "View logs", MenuCommand::ViewLogs);
            item(ui, "Minimize", MenuCommand::MinimizeToTray);
            ui.separator();
            item(ui, "Quit completely", MenuCommand::Quit);
            if let Some(cmd) = command.get() {
                self.core.lock().expect("core").menu(cmd);
            }
            return;
        }
        item(ui, "Show controls", MenuCommand::ShowControls);
        item(
            ui,
            if wanted { "Turn 3D off" } else { "Turn 3D on" },
            MenuCommand::Toggle,
        );
        item(
            ui,
            "Restore overlay (Ctrl+Alt+F8)",
            MenuCommand::RestoreOverlay,
        );
        let mut keep_value = keep;
        if ui
            .checkbox(&mut keep_value, "Keep overlay above apps")
            .changed()
        {
            command.set(Some(MenuCommand::KeepOverlayOnTop(keep_value)));
            ui.close();
        }
        ui.separator();
        item(
            ui,
            "Always convert desktop (AI)",
            MenuCommand::AlwaysConvertDesktop,
        );
        item(ui, "Side-by-side mode", MenuCommand::SideBySide);
        item(ui, "AI conversion mode", MenuCommand::AiMode);
        item(ui, "Restart conversion", MenuCommand::Restart);
        ui.separator();
        item(ui, "Help", MenuCommand::Help);
        item(ui, "About", MenuCommand::About);
        item(ui, "Check for updates", MenuCommand::CheckUpdates);
        item(ui, "Toggle light / dark mode", MenuCommand::ToggleTheme);
        item(ui, "View logs", MenuCommand::ViewLogs);
        item(
            ui,
            minimize_label(self.tray_available()),
            MenuCommand::MinimizeToTray,
        );
        ui.separator();
        item(ui, "Quit completely", MenuCommand::Quit);
        if let Some(cmd) = command.get() {
            self.core.lock().expect("core").menu(cmd);
        }
    }
}

impl App {
    /// `--screenshot`: after a few settled frames request a capture, save it
    /// on arrival and close. Returns true once the window is closing.
    fn screenshot_step(&mut self, ctx: &egui::Context) -> bool {
        let request = self.core.lock().expect("core").screenshot.clone();
        let Some(request) = request else { return false };
        if self.frames == 4 {
            ctx.send_viewport_cmd(ViewportCommand::Screenshot(egui::UserData::default()));
        }
        if self.screenshot_taken {
            return true;
        }
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| {
                if let egui::Event::Screenshot { image, .. } = e {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if let Some(image) = image {
            let [w, h] = image.size;
            let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
            match image::RgbaImage::from_raw(w as u32, h as u32, rgba)
                .ok_or("Could not save screenshot: bad image".to_owned())
                .and_then(|img| save_screenshot(&img, &request.path))
            {
                Ok(()) => {
                    // Not `println!`, which panics when the line cannot be
                    // written: with a reader that had gone the picture was
                    // saved and the program ended with exit code 101.
                    crate::say(&request.path.display().to_string());
                    self.core.lock().expect("core").screenshot_outcome = Some(Ok(()));
                }
                Err(e) => {
                    self.core.lock().expect("core").screenshot_outcome = Some(Err(e));
                }
            }
            self.screenshot_taken = true;
            self.core.lock().expect("core").exiting = true;
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return true;
        }
        if self.frames > 120 {
            let mut c = self.core.lock().expect("core");
            c.screenshot_outcome = Some(Err("Screenshot was not delivered.".into()));
            c.exiting = true;
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return true;
        }
        false
    }
}

/// Write the `--screenshot` picture to `path`, in the format its ending
/// names.
///
/// Like the viewer's export it goes to a temporary file beside the
/// destination, which is moved into place once it is whole. Written in
/// place, a picture that was there before was cut to nothing first and
/// stayed empty or half written when the writing failed: on a full disk,
/// and for an ending whose format cannot hold the picture (`.jpg` has no
/// transparency) even on a healthy one.
fn save_screenshot(image: &image::RgbaImage, path: &std::path::Path) -> Result<(), String> {
    let failed = |e: &dyn std::fmt::Display| format!("Could not save {}: {e}", path.display());
    let format = image::ImageFormat::from_path(path).map_err(|e| failed(&e))?;
    let target = viewer::export_target(path)?;
    any2odyssey_core::fsutil::write_atomic_document_with(&target, |writer| {
        image
            .write_to(writer, format)
            .map_err(std::io::Error::other)
    })
    .map_err(|e| failed(&e))
}

/// What the explicit minimize button and menu item do on this desktop: hide
/// into the tray when there is one, otherwise an ordinary minimize.
fn minimize_label(tray_available: bool) -> &'static str {
    if tray_available {
        "Minimize to tray"
    } else {
        "Minimize"
    }
}

/// What a frame does about the tray.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrayStep {
    Stay,
    /// The window was minimized and belongs in the tray: hide it there.
    Hide,
    /// The controls are taken to be in the tray and the window is on the
    /// screen: show them, as a request to show them would.
    Show,
}

/// Compare the frame's record of the controls being hidden in the tray
/// (`hidden`) with the window as it is now (`minimized`; a window hidden in
/// the tray is a minimized one, see `platform::hide_window`).
///
/// The record alone was trusted, and only this program's own ways of
/// showing the controls cleared it. A window that anything else restored
/// (seen with `ShowWindow(SW_RESTORE)` and with `SC_RESTORE` from another
/// process, which is what a window manager or a task switcher does) stood
/// on the screen above everything else with no taskbar button, and its
/// frames went on returning before anything was drawn or a click was
/// looked at, the Close button's included, until the tray icon,
/// Ctrl+Alt+F9 or `--show` was used. The same happened without any other
/// program when a show request came within a few milliseconds of the hide.
fn tray_step(hidden: bool, minimized: bool, hides_to_tray: bool) -> TrayStep {
    match (hidden, minimized) {
        (true, false) => TrayStep::Show,
        (false, true) if hides_to_tray => TrayStep::Hide,
        _ => TrayStep::Stay,
    }
}

pub const SHORTCUTS_NOTICE: &str = "Global shortcuts Ctrl+Alt+F8/F9/F10 are unavailable; another app may hold them. The buttons, menus and tray still work.";
pub const TRAY_NOTICE: &str = "The notification icon could not be created; Minimize and Close act normally. See companion.log.";

/// What went wrong while the controls opened and stays wrong for the session:
/// shortcuts that could not be registered, a notification icon that could not
/// be created. Shown under the status line for as long as it is true. The
/// status line itself belongs to conversion, and with automatic start (the
/// default) it is rewritten before the first frame, so a message placed there
/// was never seen; nor can these now displace the settings-recovery text.
fn startup_notices(hotkeys_ok: bool, tray_failed: bool) -> Vec<&'static str> {
    let mut notices = Vec::new();
    if !hotkeys_ok {
        notices.push(SHORTCUTS_NOTICE);
    }
    if tray_failed {
        notices.push(TRAY_NOTICE);
    }
    notices
}

#[cfg(windows)]
fn window_handle(cc: &eframe::CreationContext<'_>) -> isize {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match cc.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
        _ => 0,
    }
}
#[cfg(not(windows))]
fn window_handle(_cc: &eframe::CreationContext<'_>) -> isize {
    0
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if on_wayland() {
            pace(ctx);
        }
        // Preview capture needs several settled frames and a bounded timeout.
        // Normal controls repaint for input or a change reported by the poller.
        if self.preview {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        self.frames += 1;
        if self.screenshot_step(ctx) {
            return;
        }
        limit_zoom(ctx);
        keep_smallest_window(ctx);
        // The window's name for assistive software. The tree's root has none
        // of its own, and on Linux the title is not taken for it: a screen
        // reader was given a frame without a name.
        ctx.accesskit_node_builder(egui::accesskit_root_id(), |node| node.set_label(APP_NAME));
        self.handle_requests(ctx);
        if ctx.input(|i| i.viewport().close_requested()) {
            let exiting = self.core.lock().expect("core").exiting;
            if !exiting && self.hides_to_tray() {
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                self.hide_to_tray(ctx);
            } else {
                let mut core = self.core.lock().expect("core");
                core.quit();
                if !core.ready_to_exit() {
                    ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                }
            }
        }
        // After the requests: a show request handled above has just restored
        // the window, and what is asked here is its state now.
        let minimized = self.minimized_now(ctx);
        match tray_step(self.hidden, minimized, self.hides_to_tray()) {
            TrayStep::Hide => self.hide_to_tray(ctx),
            TrayStep::Show => self.show_controls(ctx),
            TrayStep::Stay => {}
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
            self.help.select("start");
            self.page = Page::Help;
        }
        #[cfg(windows)]
        if let Some(tray) = &self.tray {
            let c = self.core.lock().expect("core");
            let update = c.update.available().map(|r| r.version.to_string());
            tray.sync(c.wanted(), c.prefs.keep_overlay_on_top, update.as_deref());
        }
        // The output window is a window of its own: it stays up while the
        // controls are minimized.
        self.viewer.show_output(ctx);
        // The viewer's choices are written from here, a moment after the
        // last change: its page is not drawn while the controls are hidden,
        // minimized or on another page.
        self.viewer.keep_choices(ctx, Instant::now());
        // Global shortcuts are taken here for the same reason: they are for
        // when the page is not in front.
        #[cfg(target_os = "linux")]
        self.viewer.poll_shortcuts(ctx);
        // Process show/quit requests and synchronize the tray before skipping
        // hidden content. Supervision continues on its independent poller.
        if self.hidden || minimized {
            if self.core.lock().expect("core").ready_to_exit() {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            return;
        }
        let p = self.palette;
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        let radius: u8 = if maximized { 1 } else { 18 };
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let screen = ui.max_rect();
                ui.painter()
                    .rect_filled(screen, CornerRadius::same(radius), color(p.background));
                self.resize_edges(ctx, screen);
                let inner = screen.shrink(12.0);
                let mut body = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(inner)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                self.titlebar(&mut body, ctx);
                body.add_space(8.0);
                // For the right click only: not a Tab stop. A page that
                // scrolls senses its own (`widgets::drag_with_a_finger`).
                let background = body.interact(
                    body.available_rect_before_wrap(),
                    body.id().with("body-bg"),
                    Sense::CLICK,
                );
                background.context_menu(|ui| self.context_menu(ui));
                match self.page {
                    Page::Controls => {
                        egui::ScrollArea::vertical()
                            .id_salt("controls-scroll")
                            .auto_shrink([false, false])
                            .scroll_source(widgets::PAGE_SCROLL)
                            .show(&mut body, |ui| {
                                // The right click on the page itself (the
                                // one above is for what lies around it).
                                widgets::drag_with_a_finger(ui)
                                    .context_menu(|ui| self.context_menu(ui));
                                ui.set_width((ui.available_width() - 4.0).max(0.0));
                                let mut c = self.core.lock().expect("core");
                                let tray_available = self.tray_available();
                                let notices = startup_notices(c.hotkeys_ok, self.tray_failed);
                                self.controls.show(
                                    ui,
                                    &mut c,
                                    &p,
                                    self.wordmark.as_ref(),
                                    tray_available,
                                    &notices,
                                );
                                widgets::keep_focus_visible(ui);
                            });
                    }
                    Page::Viewer => {
                        egui::ScrollArea::vertical()
                            .id_salt("viewer-scroll")
                            .auto_shrink([false, false])
                            .scroll_source(widgets::PAGE_SCROLL)
                            .show(&mut body, |ui| {
                                widgets::drag_with_a_finger(ui)
                                    .context_menu(|ui| self.context_menu(ui));
                                ui.set_width((ui.available_width() - 4.0).max(0.0));
                                let mut c = self.core.lock().expect("core");
                                self.viewer.show(ui, &mut c, &p, self.wordmark.as_ref());
                                widgets::keep_focus_visible(ui);
                            });
                    }
                    Page::Help => {
                        if self.help.show(&mut body, &p) {
                            self.page = home();
                        }
                    }
                    Page::About => {
                        let mut c = self.core.lock().expect("core");
                        if self.about.show(&mut body, &p, &mut c) {
                            self.page = home();
                        }
                    }
                }
                widgets::window_frame(ui.painter(), screen, &p, radius);
            });
        if self.core.lock().expect("core").ready_to_exit() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // A file dialog still open is ended here with everything it started.
        // The kernel's signal at this program's end reaches the dialog's own
        // process only: where that is a wrapper script, the window it had
        // opened stayed on the desktop.
        #[cfg(not(windows))]
        platform::close_dialog();
        self.viewer.save_lens();
        let cleanup = {
            let mut c = self.core.lock().expect("core");
            c.save_preferences();
            c.take_shutdown()
        };
        cleanup();
    }
}

#[allow(dead_code)]
fn _unused(_: Color32) {}

#[cfg(test)]
mod layout_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use any2odyssey_core::log::Logger;

    #[test]
    fn a_screenshot_that_cannot_be_written_leaves_the_earlier_picture() {
        let dir = tempfile::tempdir().unwrap();
        let first = image::RgbaImage::from_pixel(4, 3, image::Rgba([10, 20, 30, 255]));
        let second = image::RgbaImage::from_pixel(2, 5, image::Rgba([200, 100, 0, 128]));
        let png = dir.path().join("shot.png");
        save_screenshot(&first, &png).unwrap();
        assert_eq!(image::open(&png).unwrap().to_rgba8(), first);
        save_screenshot(&second, &png).unwrap();
        assert_eq!(image::open(&png).unwrap().to_rgba8(), second);
        // JPEG holds no transparency: the encoder refuses the picture, which
        // it is asked only after the destination has been opened. Saved in
        // place, that had already emptied the file.
        let jpg = dir.path().join("shot.jpg");
        std::fs::write(&jpg, b"the picture that was there").unwrap();
        let error = save_screenshot(&first, &jpg).unwrap_err();
        assert!(error.contains("shot.jpg"), "{error}");
        assert_eq!(std::fs::read(&jpg).unwrap(), b"the picture that was there");
        // An ending that names no format, and a folder: said, nothing made.
        let unknown = dir.path().join("shot.picture");
        let error = save_screenshot(&first, &unknown).unwrap_err();
        assert!(error.contains("shot.picture"), "{error}");
        assert!(!unknown.exists());
        std::fs::create_dir(dir.path().join("folder.png")).unwrap();
        assert!(save_screenshot(&first, &dir.path().join("folder.png")).is_err());
        // No temporary file is left by any of them.
        let mut names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        assert_eq!(names, ["folder.png", "shot.jpg", "shot.png"]);
    }

    #[test]
    fn missing_tray_minimizes_normally_and_close_quits() {
        let dir = tempfile::tempdir().unwrap();
        let core = Arc::new(Mutex::new(Core::new_preview(Logger::new(
            dir.path().join("test.log"),
        ))));
        let mut app = App {
            controls: ControlsState::from_core(&core.lock().unwrap()),
            core: core.clone(),
            page: Page::Controls,
            viewer: ViewerState::default(),
            help: HelpState::default(),
            about: AboutState::new(),
            palette: Palette::for_dark(false),
            wordmark: None,
            #[cfg(windows)]
            tray: None, // Tray creation failed, regardless of platform support.
            tray_failed: true,
            _hotkeys: None, // Global shortcuts may also be unavailable.
            hidden: false,
            preview: false,
            frames: 0,
            screenshot_taken: false,
        };
        assert!(core.lock().unwrap().prefs.minimize_to_tray);
        assert!(!app.hides_to_tray());
        assert_eq!(minimize_label(app.tray_available()), "Minimize");
        assert_eq!(minimize_label(true), "Minimize to tray");
        let ctx = egui::Context::default();
        let output = ctx.run(Default::default(), |ctx| app.hide_to_tray(ctx));
        assert!(!app.hidden, "a missing icon must never hide the controls");
        assert!(output.viewport_output[&egui::ViewportId::ROOT]
            .commands
            .iter()
            .any(|command| matches!(command, ViewportCommand::Minimized(true))));
        app.close_requested(&ctx);
        assert!(
            core.lock().unwrap().exiting,
            "Close must quit without a tray"
        );
    }

    #[test]
    fn the_record_of_being_in_the_tray_follows_the_window() {
        for hides_to_tray in [false, true] {
            // In the tray and minimized, or on the screen and not hidden.
            assert_eq!(tray_step(true, true, hides_to_tray), TrayStep::Stay);
            assert_eq!(tray_step(false, false, hides_to_tray), TrayStep::Stay);
            // Recorded as hidden, yet on the screen: something other than
            // a show request restored it (seen with ShowWindow(SW_RESTORE)
            // from another process), or a show request and the hide before
            // it crossed. Shown, whatever the preference says by now: the
            // frame used to return early for good and the window, still
            // topmost, answered nothing.
            assert_eq!(tray_step(true, false, hides_to_tray), TrayStep::Show);
        }
        // Minimized by the user: into the tray where that is wanted and an
        // icon exists to bring it back, otherwise an ordinary minimize.
        assert_eq!(tray_step(false, true, true), TrayStep::Hide);
        assert_eq!(tray_step(false, true, false), TrayStep::Stay);
    }

    #[test]
    fn without_a_native_handle_the_frames_record_says_whether_it_is_minimized() {
        let dir = tempfile::tempdir().unwrap();
        let core = Arc::new(Mutex::new(Core::new_preview(Logger::new(
            dir.path().join("test.log"),
        ))));
        let app = App {
            controls: ControlsState::from_core(&core.lock().unwrap()),
            core: core.clone(),
            page: Page::Controls,
            viewer: ViewerState::default(),
            help: HelpState::default(),
            about: AboutState::new(),
            palette: Palette::for_dark(false),
            wordmark: None,
            #[cfg(windows)]
            tray: None,
            tray_failed: true,
            _hotkeys: None,
            hidden: false,
            preview: false,
            frames: 0,
            screenshot_taken: false,
        };
        assert_eq!(core.lock().unwrap().controls_hwnd, 0);
        assert_eq!(platform::is_minimized(0), None);
        let ctx = egui::Context::default();
        for minimized in [true, false] {
            let mut input = egui::RawInput::default();
            input
                .viewports
                .entry(egui::ViewportId::ROOT)
                .or_default()
                .minimized = Some(minimized);
            let _ = ctx.run(input, |ctx| assert_eq!(app.minimized_now(ctx), minimized));
        }
    }

    #[test]
    fn frames_the_display_does_not_space_are_spaced_here() {
        let start = Instant::now();
        // The first frame, and one that comes late enough, start at once.
        assert_eq!(pacing_wait(None, start), Duration::ZERO);
        assert_eq!(
            pacing_wait(Some(start), start + PACED_FRAME),
            Duration::ZERO
        );
        assert_eq!(
            pacing_wait(Some(start), start + Duration::from_secs(1)),
            Duration::ZERO
        );
        // One that comes early waits for the rest of the interval.
        assert_eq!(pacing_wait(Some(start), start), PACED_FRAME);
        assert_eq!(
            pacing_wait(Some(start), start + Duration::from_millis(3)),
            PACED_FRAME - Duration::from_millis(3)
        );
        // A clock that went back does not wait longer than one interval.
        assert_eq!(
            pacing_wait(Some(start + Duration::from_secs(5)), start),
            PACED_FRAME
        );
        // Frames asked for back to back come no faster than the interval.
        let ctx = egui::Context::default();
        pace(&ctx);
        let first = Instant::now();
        pace(&ctx);
        pace(&ctx);
        assert!(first.elapsed() >= PACED_FRAME * 2 - Duration::from_millis(1));
    }

    #[test]
    fn startup_notices_follow_state_and_never_touch_the_status_line() {
        assert!(startup_notices(true, false).is_empty());
        assert_eq!(startup_notices(false, false), vec![SHORTCUTS_NOTICE]);
        assert_eq!(startup_notices(true, true), vec![TRAY_NOTICE]);
        assert_eq!(
            startup_notices(false, true),
            vec![SHORTCUTS_NOTICE, TRAY_NOTICE]
        );
    }
}
