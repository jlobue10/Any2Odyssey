//! The eframe application: custom chrome, pages, tray/hotkey wiring.

mod about;
mod controls;
mod help;
mod lens;
mod theme;
mod viewer;
mod widgets;

use std::sync::{Arc, Mutex};
use std::time::Duration;

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
        .with_min_inner_size([700.0, 580.0])
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
            viewer: ViewerState::with_saved_lens(),
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
        let response = ui.interact(bar, ui.id().with("titlebar"), Sense::click_and_drag());
        if response.drag_started() {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if response.double_clicked() {
            let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        }
        let mut actions = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    pos2(bar.left() + 8.0, bar.top() + 6.0),
                    pos2(bar.right() - 14.0, bar.bottom() - 8.0),
                ))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        let chrome = |ui: &mut egui::Ui, text: &str, tip: &str| {
            widgets::gold_button_named(
                ui,
                text,
                tip,
                false,
                &p,
                Some(vec2(34.0, 34.0)),
                FontId::proportional(15.0),
            )
            .on_hover_text(tip)
        };
        if chrome(&mut actions, "×", "Close").clicked() {
            self.close_requested(ctx);
        }
        if chrome(&mut actions, "▾", minimize_label(self.tray_available())).clicked() {
            self.hide_to_tray(ctx);
        }
        if chrome(&mut actions, "□", "Maximize / restore").clicked() {
            let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        }
        if chrome(&mut actions, "—", "Minimize").clicked() {
            if self.hides_to_tray() {
                self.hide_to_tray(ctx);
            } else {
                ctx.send_viewport_cmd(ViewportCommand::Minimized(true));
            }
        }
        let theme_label = if p.dark { "Light" } else { "Dark" };
        if widgets::gold_button(&mut actions, theme_label, false, &p)
            .on_hover_text("Toggle light / dark mode")
            .clicked()
        {
            self.set_dark(ctx, !p.dark);
        }
        if widgets::gold_button(&mut actions, "Help", false, &p).clicked() {
            self.help.select("start");
            self.page = Page::Help;
        }
        if widgets::gold_button(&mut actions, "About", false, &p)
            .on_hover_text("Version, installation and credits")
            .clicked()
        {
            self.page = Page::About;
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
                .ok_or("bad image".to_owned())
                .and_then(|img| img.save(&request.path).map_err(|e| e.to_string()))
            {
                Ok(()) => {
                    println!("{}", request.path.display());
                    self.core.lock().expect("core").screenshot_outcome = Some(Ok(()));
                }
                Err(e) => {
                    self.core.lock().expect("core").screenshot_outcome =
                        Some(Err(format!("Could not save screenshot: {e}")));
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

/// What the explicit minimize button and menu item do on this desktop: hide
/// into the tray when there is one, otherwise an ordinary minimize.
fn minimize_label(tray_available: bool) -> &'static str {
    if tray_available {
        "Minimize to tray"
    } else {
        "Minimize"
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
        // Preview capture needs several settled frames and a bounded timeout.
        // Normal controls repaint for input or a change reported by the poller.
        if self.preview {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        self.frames += 1;
        if self.screenshot_step(ctx) {
            return;
        }
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
        if ctx.input(|i| i.viewport().minimized.unwrap_or(false))
            && !self.hidden
            && self.hides_to_tray()
        {
            self.hide_to_tray(ctx);
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
        // Process show/quit requests and synchronize the tray before skipping
        // hidden content. Supervision continues on its independent poller.
        if self.hidden || ctx.input(|i| i.viewport().minimized.unwrap_or(false)) {
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
                let background = body.interact(
                    body.available_rect_before_wrap(),
                    body.id().with("body-bg"),
                    Sense::click(),
                );
                background.context_menu(|ui| self.context_menu(ui));
                match self.page {
                    Page::Controls => {
                        egui::ScrollArea::vertical()
                            .id_salt("controls-scroll")
                            .auto_shrink([false, false])
                            .show(&mut body, |ui| {
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
                            });
                    }
                    Page::Viewer => {
                        egui::ScrollArea::vertical()
                            .id_salt("viewer-scroll")
                            .auto_shrink([false, false])
                            .show(&mut body, |ui| {
                                ui.set_width((ui.available_width() - 4.0).max(0.0));
                                let mut c = self.core.lock().expect("core");
                                self.viewer.show(ui, &mut c, &p, self.wordmark.as_ref());
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
