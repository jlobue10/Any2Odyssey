//! Layout regressions, run without a window: the real `App::update` is
//! driven through `egui::Context::run` with a fixed screen size, and the
//! text the frame painted is read back with its position.

use std::sync::{Arc, Mutex};

use any2odyssey_core::log::Logger;
use any2odyssey_core::supervisor::Supervisor;
use any2odyssey_core::theme::Palette;
use egui::{pos2, vec2, Rect};

use super::*;

/// The default window and the smallest one the viewport allows.
const SIZES: [(f32, f32); 2] = [(980.0, 780.0), (700.0, 580.0)];
/// The window frame is drawn over the outer 12 points.
const FRAME: f32 = 12.0;

/// One painted run of text: what it says and where.
struct Painted {
    text: String,
    rect: Rect,
}

struct Harness {
    app: App,
    core: Arc<Mutex<Core>>,
    ctx: egui::Context,
    size: (f32, f32),
    _dir: tempfile::TempDir,
}

impl Harness {
    fn new(size: (f32, f32), page: Page) -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let core = Arc::new(Mutex::new(Core::new_preview(Logger::new(
            dir.path().join("test.log"),
        ))));
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let palette = Palette::for_dark(false);
        theme::apply(&ctx, &palette);
        let controls = ControlsState::from_core(&core.lock().unwrap());
        let app = App {
            controls,
            core: core.clone(),
            page,
            help: HelpState::default(),
            about: AboutState::new(),
            palette,
            wordmark: None,
            #[cfg(windows)]
            tray: None,
            _hotkeys: None,
            hidden: false,
            preview: true,
            frames: 0,
            screenshot_taken: false,
        };
        Harness {
            app,
            core,
            ctx,
            size,
            _dir: dir,
        }
    }

    fn core(&self) -> std::sync::MutexGuard<'_, Core> {
        self.core.lock().unwrap()
    }

    fn input(&self, events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                pos2(0.0, 0.0),
                vec2(self.size.0, self.size.1),
            )),
            events,
            ..Default::default()
        }
    }

    fn run(&mut self, events: Vec<egui::Event>) -> Vec<Painted> {
        let mut frame = eframe::Frame::_new_kittest();
        let input = self.input(events);
        let app = &mut self.app;
        let output = self
            .ctx
            .run(input, |ctx| eframe::App::update(app, ctx, &mut frame));
        let mut painted = Vec::new();
        for clipped in &output.shapes {
            collect(&clipped.shape, &mut painted);
        }
        painted
    }

    /// A few frames, so sizes measured in one frame are used by the next.
    fn settle(&mut self) -> Vec<Painted> {
        let mut painted = Vec::new();
        for _ in 0..4 {
            painted = self.run(Vec::new());
        }
        painted
    }
}

fn collect(shape: &egui::Shape, out: &mut Vec<Painted>) {
    match shape {
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
        egui::Shape::Text(text) => out.push(Painted {
            text: text.galley.text().to_owned(),
            rect: Rect::from_min_size(text.pos, text.galley.size()),
        }),
        _ => {}
    }
}

/// `None` when the text is scrolled out of view: only what is visible is painted.
fn try_find<'a>(painted: &'a [Painted], start: &str) -> Option<&'a Painted> {
    painted.iter().find(|p| p.text.starts_with(start))
}

fn find<'a>(painted: &'a [Painted], start: &str) -> &'a Painted {
    try_find(painted, start).unwrap_or_else(|| panic!("{start:?} was not painted"))
}

/// Everything painted stays inside the window frame horizontally.
fn assert_inside_window(painted: &[Painted], width: f32, context: &str) {
    for p in painted {
        assert!(
            p.rect.left() >= FRAME - 0.5 && p.rect.right() <= width - FRAME + 0.5,
            "{context}: {:?} spans x {:.0}..{:.0} in a {width} wide window",
            p.text.chars().take(50).collect::<String>(),
            p.rect.left(),
            p.rect.right()
        );
    }
}

/// The status lines that are long in practice: the damaged-settings recovery
/// text and what the supervisor says when a worker exits.
fn long_statuses() -> Vec<String> {
    let now = std::time::Instant::now();
    let engine_closed = "Conversion stopped: the engine closed itself. See conversion log.";
    let mut recovering = Supervisor::new(true);
    recovering.start_requested();
    let mut giving_up = Supervisor::new(false);
    giving_up.start_requested();
    let mut prerequisite = Supervisor::new(true);
    prerequisite.start_requested();
    vec![
        "Could not load saved settings: expected value at line 1 column 1. Automatic conversion is disabled, and so is the daily update check until you turn it back on from the About page. Fix the settings file and reopen, or choose settings and click Apply to save them with a backup of the original.".to_owned(),
        recovering.worker_exited(1, Some(engine_closed), now),
        giving_up.worker_exited(1, Some(engine_closed), now),
        prerequisite.worker_exited(
            3,
            Some("Reconnect the Odyssey USB cable; its 3D camera is unavailable."),
            now,
        ),
        "Starting 3D — checking prerequisites and loading the AI model.".to_owned(),
    ]
}

#[test]
fn a_long_status_wraps_and_leaves_the_buttons_inside_the_window() {
    for size in SIZES {
        for status in long_statuses() {
            let mut h = Harness::new(size, Page::Controls);
            h.core().status = status.clone();
            let painted = h.settle();
            let context = format!("{}x{} with {} characters", size.0, size.1, status.len());
            assert_inside_window(&painted, size.0, &context);
            let line = find(&painted, &status[..24]);
            let button = find(&painted, "Turn on 3D");
            assert!(
                line.rect.right() <= button.rect.left(),
                "{context}: the status runs under the button"
            );
            // The cards below are where they are with a short status: the
            // row no longer widens the page (which also switched the narrow
            // window to two columns and moved Apply out of it).
            let mut short = Harness::new(size, Page::Controls);
            let reference = short.settle();
            assert_eq!(
                find(&painted, "Refresh").rect.left(),
                find(&reference, "Refresh").rect.left(),
                "{context}: the status moved the settings cards"
            );
            for anchor in ["DEPTH & POP-OUT", "Apply settings"] {
                if let (Some(now), Some(before)) =
                    (try_find(&painted, anchor), try_find(&reference, anchor))
                {
                    assert_eq!(now.rect.left(), before.rect.left(), "{context}: {anchor}");
                }
            }
        }
    }
}

#[test]
fn every_page_stays_inside_the_window_at_both_sizes() {
    for size in SIZES {
        for (page, name) in [
            (Page::Controls, "controls"),
            (Page::Help, "help"),
            (Page::About, "about"),
        ] {
            let mut h = Harness::new(size, page);
            let painted = h.settle();
            assert!(!painted.is_empty());
            // About is covered by its own test: its rows carry this test
            // binary's own, very long, path.
            if page != Page::About {
                assert_inside_window(&painted, size.0, &format!("{name} {}x{}", size.0, size.1));
            }
        }
    }
}
