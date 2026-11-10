//! Layout regressions, run without a window: the real `App::update` is
//! driven through `egui::Context::run` with a fixed screen size, and the
//! text the frame painted is read back with its position.

use std::sync::{Arc, Mutex};

use any2odyssey_core::log::Logger;
use any2odyssey_core::prefs::CaptureMode;
use any2odyssey_core::supervisor::Supervisor;
use any2odyssey_core::theme::Palette;
use egui::{pos2, vec2, Rect};

use super::*;
use crate::core::WindowChoice;

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
    /// What the last frame reported to assistive output.
    events: Vec<egui::output::OutputEvent>,
    /// Outlines the last frame stroked, with their colour.
    outlines: Vec<(Rect, egui::Color32)>,
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
            tray_failed: false,
            _hotkeys: None,
            hidden: false,
            preview: true,
            frames: 0,
            screenshot_taken: false,
        };
        Harness {
            events: Vec::new(),
            outlines: Vec::new(),
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
        self.outlines.clear();
        for clipped in &output.shapes {
            collect(&clipped.shape, &mut painted, &mut self.outlines);
        }
        self.events = output.platform_output.events;
        painted
    }

    fn press(&mut self, key: egui::Key) -> Vec<Painted> {
        let event = |pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Default::default(),
        };
        self.run(vec![event(true)]);
        let mut events = std::mem::take(&mut self.events);
        let painted = self.run(vec![event(false)]);
        events.append(&mut self.events);
        self.events = events;
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

    fn click(&mut self, at: egui::Pos2) -> Vec<Painted> {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        self.run(vec![egui::Event::PointerMoved(at)]);
        self.run(vec![button(true)]);
        self.run(vec![button(false)]);
        self.settle()
    }
}

fn collect(shape: &egui::Shape, out: &mut Vec<Painted>, outlines: &mut Vec<(Rect, egui::Color32)>) {
    match shape {
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out, outlines)),
        egui::Shape::Text(text) => out.push(Painted {
            text: text.galley.text().to_owned(),
            rect: Rect::from_min_size(text.pos, text.galley.size()),
        }),
        egui::Shape::Rect(rect) if rect.stroke.width > 0.0 => {
            outlines.push((rect.rect, rect.stroke.color));
        }
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

const LONG_TITLES: [&str; 3] = [
    "crates/app/src/ui/controls.rs - any2odyssey - Visual Studio Code",
    "How to use 3D on the Odyssey 3D gaming monitor | Samsung UK - Support - Displays and more words in this tab title - Mozilla Firefox",
    "A third window whose title is also much longer than the box that has to show it - Some Application",
];

/// Selected-window mode with `titles` listed and the first one chosen.
fn with_windows(size: (f32, f32), titles: &[&str]) -> Harness {
    let h = Harness::new(size, Page::Controls);
    {
        let mut core = h.core();
        core.prefs.mode = CaptureMode::Window;
        core.windows = titles
            .iter()
            .enumerate()
            .map(|(i, title)| WindowChoice {
                handle: i as isize + 1,
                title: (*title).to_owned(),
            })
            .collect();
        core.selected_window = Some(1);
    }
    h
}

#[test]
fn a_long_source_title_is_elided_and_refresh_keeps_its_place() {
    for size in SIZES {
        let reference = with_windows(size, &["Notepad"]).settle();
        for title in LONG_TITLES {
            let mut h = with_windows(size, &[title]);
            let painted = h.settle();
            let context = format!("{}x{} with {} characters", size.0, size.1, title.len());
            assert_inside_window(&painted, size.0, &context);
            let shown = find(&painted, &title[..20]);
            let refresh = find(&painted, "Refresh");
            assert!(
                shown.rect.right() < refresh.rect.left(),
                "{context}: the title runs under Refresh"
            );
            assert_eq!(
                refresh.rect.left(),
                find(&reference, "Refresh").rect.left(),
                "{context}: the title moved Refresh"
            );
            // 0.8.6: with a 64-character title Refresh sat under "Apply
            // settings / restart conversion", and a click on it applied.
            if let Some(apply) = try_find(&painted, "Apply settings") {
                assert!(
                    !refresh.rect.expand(8.0).intersects(apply.rect.expand(8.0)),
                    "{context}: Refresh overlaps Apply"
                );
            }
        }
    }
}

#[test]
fn clicking_refresh_refreshes_and_does_not_apply() {
    for size in SIZES {
        for title in LONG_TITLES {
            let mut h = with_windows(size, &[title]);
            let painted = h.settle();
            let refresh = find(&painted, "Refresh").rect.center();
            h.click(refresh);
            let core = h.core();
            assert_ne!(core.status, "Preferences saved.", "the click reached Apply");
            assert_eq!(core.status, "Ready");
            // Handle 1 is no real window, so a refresh clears the choice.
            assert_eq!(core.selected_window, None, "the list was not refreshed");
        }
    }
}

#[test]
fn the_source_list_is_bounded_and_inside_the_window() {
    for size in SIZES {
        let mut h = with_windows(size, &LONG_TITLES);
        let painted = h.settle();
        let shown = find(&painted, &LONG_TITLES[0][..20]).rect;
        let painted = h.click(shown.center());
        assert_inside_window(&painted, size.0, &format!("list at {}x{}", size.0, size.1));
        // The other two titles appear only in the open list.
        for title in &LONG_TITLES[1..] {
            let item = find(&painted, &title[..20]);
            assert!(item.rect.top() > shown.bottom(), "{title}: not in the list");
            assert!(
                item.rect.width() <= 360.0_f32.max(shown.width() + 60.0),
                "{title}: {} wide",
                item.rect.width()
            );
        }
    }
}

#[test]
fn startup_notices_stay_visible_whatever_the_status_says() {
    for size in SIZES {
        let mut h = Harness::new(size, Page::Controls);
        h.app.tray_failed = true;
        h.core().hotkeys_ok = false;
        let painted = h.settle();
        assert_inside_window(&painted, size.0, "notices");
        let status = find(&painted, "Ready").rect;
        for notice in [SHORTCUTS_NOTICE, TRAY_NOTICE] {
            let line = find(&painted, notice).rect;
            assert!(line.top() >= status.bottom(), "{notice}: above the status");
        }
        // Automatic start, a failure, recovery: the status line moves on,
        // the notices do not.
        for status in long_statuses() {
            h.core().status = status;
            let painted = h.settle();
            assert_inside_window(&painted, size.0, "notices under a long status");
            find(&painted, SHORTCUTS_NOTICE);
            find(&painted, TRAY_NOTICE);
        }
        // Nothing to say, nothing shown.
        h.app.tray_failed = false;
        h.core().hotkeys_ok = true;
        let painted = h.settle();
        assert!(try_find(&painted, SHORTCUTS_NOTICE).is_none());
        assert!(try_find(&painted, TRAY_NOTICE).is_none());
    }
}

#[test]
fn pending_edits_show_a_hint_and_only_apply_commits_them() {
    // The narrow window is taller here than its minimum, so the single
    // column shows Apply without scrolling.
    for size in [SIZES[0], (SIZES[1].0, 900.0)] {
        let mut h = Harness::new(size, Page::Controls);
        let painted = h.settle();
        assert!(try_find(&painted, "Unapplied changes").is_none());

        h.app.controls.mode = CaptureMode::Active;
        h.app.controls.side_by_side = true;
        h.app.controls.keep_on_top = false;
        h.app.controls.depth_pos = 150;
        let painted = h.settle();
        assert_inside_window(&painted, size.0, "hint");
        find(&painted, "Unapplied changes");
        find(&painted, "Follow active window");
        // Frames alone commit nothing, and neither does Turn on 3D: it
        // starts with the applied settings and the hint stays.
        let applied = any2odyssey_core::prefs::Preferences::default();
        assert_eq!(h.core().prefs, applied);
        let turn_on = find(&painted, "Turn on 3D").rect.center();
        let painted = h.click(turn_on);
        assert_eq!(h.core().prefs, applied);
        find(&painted, "Unapplied changes");

        let apply = find(&painted, "Apply settings").rect.center();
        let painted = h.click(apply);
        assert!(try_find(&painted, "Unapplied changes").is_none());
        let core = h.core();
        assert_eq!(core.prefs.mode, CaptureMode::Active);
        assert!(core.prefs.side_by_side);
        assert!(!core.prefs.keep_overlay_on_top);
        assert_eq!(core.prefs.depth, 1.5);
    }
}

/// The accessible name a click or a focus change reported, if any.
fn reported(events: &[egui::output::OutputEvent]) -> Vec<(bool, String, Option<bool>)> {
    use egui::output::OutputEvent;
    events
        .iter()
        .filter_map(|event| match event {
            OutputEvent::Clicked(info) => Some((true, info)),
            OutputEvent::FocusGained(info) => Some((false, info)),
            _ => None,
        })
        .map(|(clicked, info)| {
            (
                clicked,
                info.label.clone().unwrap_or_default(),
                info.selected,
            )
        })
        .collect()
}

#[test]
fn custom_buttons_and_help_topics_have_accessible_names() {
    let mut h = Harness::new(SIZES[0], Page::Controls);
    let painted = h.settle();
    // A glyph-faced chrome button reports what it does, not its glyph.
    let maximize = find(&painted, "□").rect.center();
    h.run(vec![egui::Event::PointerMoved(maximize)]);
    let button = |pressed| egui::Event::PointerButton {
        pos: maximize,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    h.run(vec![button(true)]);
    h.run(vec![button(false)]);
    assert_eq!(
        reported(&h.events),
        vec![(true, "Maximize / restore".to_owned(), None)]
    );
    // A text button reports its text; this one opens Help.
    let help = find(&painted, "Help").rect.center();
    h.run(vec![egui::Event::PointerMoved(help)]);
    let button = |pressed| egui::Event::PointerButton {
        pos: help,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    h.run(vec![button(true)]);
    h.run(vec![button(false)]);
    assert_eq!(reported(&h.events), vec![(true, "Help".to_owned(), None)]);
    assert!(h.app.page == Page::Help);

    // Keyboard: Tab reaches the topic rows, each announces its title and
    // whether it is the selected one, and the focused row is outlined.
    let topics = any2odyssey_core::help::topics();
    let accent = theme::color(h.app.palette.accent);
    let mut announced = Vec::new();
    for _ in 0..40 {
        h.press(egui::Key::Tab);
        let Some((clicked, label, selected)) = reported(&h.events)
            .into_iter()
            .find(|(_, label, _)| topics.iter().any(|t| t.title == *label))
        else {
            continue;
        };
        assert!(!clicked);
        let index = topics.iter().position(|t| t.title == label).unwrap();
        assert_eq!(selected, Some(index == h.app.help.selected), "{label}");
        let painted = h.run(Vec::new());
        let row = painted
            .iter()
            .filter(|p| p.text == label)
            .map(|p| p.rect)
            .find(|r| r.left() < 300.0)
            .unwrap_or_else(|| panic!("{label} is not in the list"));
        assert!(
            h.outlines
                .iter()
                .any(|(rect, colour)| *colour == accent && rect.contains_rect(row)),
            "{label}: the focused topic has no outline"
        );
        announced.push(index);
        if announced.len() >= 2 {
            break;
        }
    }
    assert!(announced.len() >= 2, "Tab never reached the topics");
    // Enter selects the focused topic.
    let focused = *announced.last().unwrap();
    assert_ne!(h.app.help.selected, focused);
    h.press(egui::Key::Enter);
    assert_eq!(h.app.help.selected, focused);
    // The mouse still selects, and the click is reported with the title.
    let painted = h.settle();
    let fourth = &topics[3].title;
    let row = painted
        .iter()
        .find(|p| p.text == *fourth && p.rect.left() < 300.0)
        .expect("fourth topic is listed")
        .rect
        .center();
    h.click(row);
    assert_eq!(h.app.help.selected, 3);
}
