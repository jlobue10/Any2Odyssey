//! The eframe application: custom chrome, pages, tray/hotkey wiring.

mod controls;
mod help;
mod theme;
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
use controls::ControlsState;
use help::HelpState;
use theme::color;

const TITLEBAR_HEIGHT: f32 = 54.0;
const EDGE: f32 = 9.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Controls,
    Help,
}

pub struct App {
    core: Arc<Mutex<Core>>,
    page: Page,
    controls: ControlsState,
    help: HelpState,
    palette: Palette,
    wordmark: Option<egui::TextureHandle>,
    #[cfg(windows)]
    tray: Option<crate::tray::Tray>,
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
        .with_resizable(true)
        .with_always_on_top();
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
        #[cfg(windows)]
        let tray = match load_png(include_bytes!("../../../../assets/Any2Odyssey.png"))
            .ok_or_else(|| "icon".to_owned())
            .and_then(|(rgba, w, h)| crate::tray::Tray::new(rgba, w, h))
        {
            Ok(t) => Some(t),
            Err(e) => {
                core.lock()
                    .expect("core")
                    .log(&format!("Tray icon unavailable: {e}"));
                None
            }
        };
        let preview = core.lock().expect("core").preview;
        if !preview {
            platform::install_jump_list();
        }
        let hotkeys = if preview {
            None
        } else {
            crate::hotkeys::register()
        };
        {
            let mut c = core.lock().expect("core");
            c.hotkeys_ok = hotkeys.is_some() || preview;
            if preview {
                c.status = "Ready".into();
            } else if !c.hotkeys_ok {
                c.status =
                    "One of Ctrl+Alt+F8/F9/F10 is in use. Free the shortcuts before starting 3D."
                        .into();
                c.log("Hotkey conflict: not all of Ctrl+Alt+F8/F9/F10 could be registered.");
            } else if c.prefs.auto_convert || c.start_requested {
                c.start_conversion();
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
            Page::Controls
        };
        App {
            core,
            page,
            controls,
            help: HelpState::default(),
            palette,
            wordmark,
            #[cfg(windows)]
            tray,
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

    fn hide_to_tray(&mut self, ctx: &egui::Context) {
        self.core.lock().expect("core").save_preferences();
        self.hidden = true;
        let hwnd = self.core.lock().expect("core").controls_hwnd;
        if hwnd != 0 {
            platform::hide_window(hwnd);
        } else {
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        }
    }

    fn show_controls(&mut self, ctx: &egui::Context) {
        self.hidden = false;
        self.page = Page::Controls;
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
            widgets::gold_button_sized(
                ui,
                text,
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
        if chrome(&mut actions, "▾", "Minimize to tray").clicked() {
            self.hide_to_tray(ctx);
        }
        if chrome(&mut actions, "□", "Maximize / restore").clicked() {
            let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        }
        if chrome(&mut actions, "—", "Minimize").clicked() {
            let to_tray = self.core.lock().expect("core").prefs.minimize_to_tray;
            if to_tray {
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
        ui.add_space(TITLEBAR_HEIGHT);
    }

    fn close_requested(&mut self, ctx: &egui::Context) {
        let (exiting, to_tray) = {
            let c = self.core.lock().expect("core");
            (c.exiting, c.prefs.minimize_to_tray)
        };
        if !exiting && to_tray {
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
        item(ui, "Toggle light / dark mode", MenuCommand::ToggleTheme);
        item(ui, "View logs", MenuCommand::ViewLogs);
        item(ui, "Minimize to tray", MenuCommand::MinimizeToTray);
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
                Ok(()) => println!("{}", request.path.display()),
                Err(e) => eprintln!("Could not save screenshot: {e}"),
            }
            self.screenshot_taken = true;
            self.core.lock().expect("core").exiting = true;
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return true;
        }
        if self.frames > 120 {
            eprintln!("Screenshot was not delivered.");
            self.core.lock().expect("core").exiting = true;
            ctx.send_viewport_cmd(ViewportCommand::Close);
            return true;
        }
        false
    }
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
            let (exiting, to_tray) = {
                let c = self.core.lock().expect("core");
                (c.exiting, c.prefs.minimize_to_tray)
            };
            if !exiting && to_tray {
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
        if ctx.input(|i| i.viewport().minimized.unwrap_or(false)) && !self.hidden {
            let to_tray = self.core.lock().expect("core").prefs.minimize_to_tray;
            if to_tray {
                self.hide_to_tray(ctx);
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
            self.help.select("start");
            self.page = Page::Help;
        }
        #[cfg(windows)]
        if let Some(tray) = &self.tray {
            let c = self.core.lock().expect("core");
            tray.sync(c.wanted(), c.prefs.keep_overlay_on_top);
        }
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
                                ui.set_width(ui.available_width() - 4.0);
                                let mut c = self.core.lock().expect("core");
                                self.controls.show(ui, &mut c, &p, self.wordmark.as_ref());
                            });
                    }
                    Page::Help => {
                        if self.help.show(&mut body, &p) {
                            self.page = Page::Controls;
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
