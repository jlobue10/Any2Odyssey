//! Layout regressions, run without a window: the real `App::update` is
//! driven through `egui::Context::run` with a fixed screen size, and the
//! text the frame painted is read back with its position, together with the
//! accessibility tree: every control with its role, name, value and place.

use std::sync::{Arc, Mutex};

use any2odyssey_core::log::Logger;
use any2odyssey_core::prefs::CaptureMode;
use any2odyssey_core::supervisor::Supervisor;
use any2odyssey_core::theme::Palette;
use egui::{pos2, vec2, Rect};

use super::*;
use crate::core::WindowChoice;

/// The default window and a small one: as narrow as the viewport allows,
/// and as high as its smallest was up to 0.9.6.
const SIZES: [(f32, f32); 2] = [(980.0, 780.0), (700.0, 580.0)];
/// The smallest window the viewport allows.
const SMALLEST: (f32, f32) = (SMALLEST_WINDOW[0], SMALLEST_WINDOW[1]);
/// The window frame is drawn over the outer 12 points.
const FRAME: f32 = 12.0;

/// One painted run of text: what it says and where.
struct Painted {
    text: String,
    rect: Rect,
    /// What the frame lets through of it: a scroll area, a truncating box.
    clip: Rect,
}

/// One entry of the accessibility tree, as assistive software is given it.
#[derive(Clone, Debug)]
struct Widget {
    id: egui::accesskit::NodeId,
    role: egui::accesskit::Role,
    /// Its own label, or the text of what it is labelled by.
    name: String,
    value: Option<String>,
    number: Option<f64>,
    rect: Rect,
    disabled: bool,
    focusable: bool,
}

struct Harness {
    /// What the last frame reported to assistive output.
    events: Vec<egui::output::OutputEvent>,
    /// Outlines the last frame stroked, with their colour.
    outlines: Vec<(Rect, egui::Color32)>,
    /// The accessibility tree of the last frame, and what has the focus.
    widgets: Vec<Widget>,
    focus: Option<egui::accesskit::NodeId>,
    /// The name of the tree's root: the window, as a screen reader says it.
    window_name: Option<String>,
    /// Display scale: physical pixels per point.
    scale: f32,
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
        ctx.enable_accesskit();
        theme::install_fonts(&ctx);
        let palette = Palette::for_dark(false);
        theme::apply(&ctx, &palette);
        let controls = ControlsState::from_core(&core.lock().unwrap());
        let app = App {
            controls,
            core: core.clone(),
            page,
            viewer: super::viewer::ViewerState::default(),
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
            widgets: Vec::new(),
            focus: None,
            window_name: None,
            scale: 1.0,
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
        let mut input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                pos2(0.0, 0.0),
                vec2(self.size.0, self.size.1),
            )),
            events,
            focused: true,
            ..Default::default()
        };
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(self.scale);
        input
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
            collect(
                &clipped.shape,
                clipped.clip_rect,
                &mut painted,
                &mut self.outlines,
            );
        }
        if let Some(update) = &output.platform_output.accesskit_update {
            self.focus = Some(update.focus);
            self.widgets = widgets_of(update);
            if let Some(tree) = &update.tree {
                self.window_name = update
                    .nodes
                    .iter()
                    .find(|(id, _)| *id == tree.root)
                    .and_then(|(_, node)| node.label().map(str::to_owned));
            }
        }
        self.events = output.platform_output.events;
        painted
    }

    /// The widget that has the keyboard focus, if one has.
    fn focused(&self) -> Option<&Widget> {
        let focus = self.focus?;
        self.widgets.iter().find(|w| w.id == focus && w.focusable)
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

/// The tree's entries with their names resolved.
fn widgets_of(update: &egui::accesskit::TreeUpdate) -> Vec<Widget> {
    use egui::accesskit::Action;
    let label_of = |id: &egui::accesskit::NodeId| {
        update
            .nodes
            .iter()
            .find(|(other, _)| other == id)
            .and_then(|(_, node)| node.label().or(node.value()))
            .unwrap_or_default()
            .to_owned()
    };
    update
        .nodes
        .iter()
        .filter_map(|(id, node)| {
            let bounds = node.bounds()?;
            let name = match node.label() {
                Some(label) if !label.is_empty() => label.to_owned(),
                _ => node
                    .labelled_by()
                    .iter()
                    .map(label_of)
                    .collect::<Vec<_>>()
                    .join(" "),
            };
            Some(Widget {
                id: *id,
                role: node.role(),
                name,
                value: node.value().map(str::to_owned),
                number: node.numeric_value(),
                rect: Rect::from_min_max(
                    pos2(bounds.x0 as f32, bounds.y0 as f32),
                    pos2(bounds.x1 as f32, bounds.y1 as f32),
                ),
                disabled: node.is_disabled(),
                focusable: node.supports_action(Action::Focus),
            })
        })
        .collect()
}

fn collect(
    shape: &egui::Shape,
    clip: Rect,
    out: &mut Vec<Painted>,
    outlines: &mut Vec<(Rect, egui::Color32)>,
) {
    match shape {
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, clip, out, outlines)),
        egui::Shape::Text(text) => out.push(Painted {
            text: text.galley.text().to_owned(),
            rect: Rect::from_min_size(text.pos, text.galley.size()),
            clip,
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
            (Page::Viewer, "viewer"),
            (Page::Help, "help"),
            (Page::About, "about"),
        ] {
            let mut h = Harness::new(size, page);
            let painted = h.settle();
            assert!(!painted.is_empty());
            assert_inside_window(&painted, size.0, &format!("{name} {}x{}", size.0, size.1));
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

/// Window titles in letters the built-in typefaces do not have.
const FOREIGN_TITLES: [&str; 5] = [
    "モンスターハンターワイルズ (Monster Hunter Wilds)",
    "照片查看器 - 写真.jpg",
    "한국어 메모장",
    "שלום עולם - פנקס רשימות",
    "مرحبا بالعالم - المفكرة",
];

/// Up to 0.9.6 each of these was a row of boxes, in the box and in the open
/// list, so that such windows could not be told apart. They are drawn with
/// the typefaces this system has for them; where it has none (a container),
/// the test asks for nothing.
#[test]
fn source_titles_in_other_scripts_are_drawn_with_typefaces_of_the_system() {
    for size in SIZES {
        let mut h = with_windows(size, &FOREIGN_TITLES);
        let fonts =
            system_fonts::install_with(&h.ctx, system_fonts::os::System::new(), |_: &str| {});
        let start = |title: &str| title.chars().take(4).collect::<String>();
        let painted = h.settle();
        let shown = find(&painted, &start(FOREIGN_TITLES[0])).rect;
        // The other titles are painted only in the open list, which is an
        // area over the page.
        h.click(shown.center());
        system_fonts::tests::draw_until_settled(&fonts, || {
            h.settle();
        });
        let painted = h.settle();
        let context = format!("{}x{}", size.0, size.1);
        assert_inside_window(&painted, size.0, &context);
        for title in FOREIGN_TITLES {
            find(&painted, &start(title));
            let boxes = system_fonts::tests::undrawn(&h.ctx, title);
            assert!(
                boxes.is_empty(),
                "{context}: {title:?} is drawn with boxes for {boxes:?}; added {:?}",
                fonts.added()
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
    let maximize = find(&painted, MAXIMIZE).rect.center();
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

#[test]
fn long_about_values_wrap_inside_the_window() {
    use any2odyssey_core::about::{AboutInfo, InstallRecord};
    // No spaces to break at, and several times the width of the window.
    let folder: String = std::iter::once("C:".to_owned())
        .chain((0..24).map(|i| format!("\\DeeplyNestedFolder{i:02}")))
        .collect();
    let record = InstallRecord {
        installed: Some("2026-11-08T14:10:33.1234567-05:00".into()),
        version: Some("0.8.6".into()),
        action: Some("Upgrade".into()),
        worker: Some(format!("{folder}\\Any2OdysseyWorker.exe")),
        ..Default::default()
    };
    for size in SIZES {
        let mut h = Harness::new(size, Page::About);
        h.app.about = AboutState::with_info(AboutInfo::assemble(
            "0.8.6",
            Some("0123456789ab"),
            Some(std::path::PathBuf::from(&folder)),
            Ok(Some(record.clone())),
        ));
        let painted = h.settle();
        let context = format!("about {}x{}", size.0, size.1);
        assert_inside_window(&painted, size.0, &context);
        let location = find(&painted, "C:\\DeeplyNestedFolder00");
        assert!(
            location.rect.height() > 30.0,
            "{context}: the path is on one line"
        );
    }
}

/// The viewport asks for at least 700x360, but a tiling window manager or a
/// frame during a restore can hand over anything. Sizes derived by
/// subtracting margins went negative there, which egui rejects with a debug
/// assertion: a panic in debug builds.
#[test]
fn no_page_panics_in_a_window_smaller_than_its_margins() {
    for page in [Page::Controls, Page::Viewer, Page::Help, Page::About] {
        for size in [
            (300.0, 200.0),
            (120.0, 90.0),
            (40.0, 40.0),
            (20.0, 20.0),
            (1.0, 1.0),
            (0.0, 0.0),
        ] {
            Harness::new(size, page).settle();
        }
    }
}

// ---------------------------------------------------------------------------
// The sweep: every page in every state at every size, display scale and zoom
// a user can reach, checked through what the frame painted and through the
// accessibility tree (the same one a screen reader is given).
// ---------------------------------------------------------------------------

mod sweep {
    use std::collections::HashSet;

    use any2odyssey_core::about::{AboutInfo, InstallRecord};
    use any2odyssey_core::update::{evaluate_latest_release, UpdateState};
    use egui::accesskit::Role;

    use super::*;
    use crate::ui::viewer::{Input, Options, Output, ViewerState};

    type Setup = Box<dyn Fn(&mut Harness)>;

    /// A deep folder and a file name in four scripts.
    fn long_path() -> String {
        let mut path = String::from("/home/someone/Pictures");
        for i in 0..14 {
            path.push_str(&format!("/a rather long folder name {i:02}"));
        }
        path.push_str("/モンスターハンター שלום مرحبا 한국어 side-by-side.png");
        path
    }

    /// A newer release with notes as long as they are kept.
    fn update_available() -> UpdateState {
        let notes: String = (1..60)
            .map(|i| {
                format!("- Item {i}: a release note line that says what changed and wraps.\\n")
            })
            .collect();
        let body = format!(
            r#"{{"tag_name":"v99.9.9","published_at":"2027-01-02T03:04:05Z","body":"{notes}","assets":[
            {{"name":"Any2Odyssey-99.9.9-windows-x64.zip","browser_download_url":"https://github.com/jlobue10/Any2Odyssey/releases/download/v99.9.9/Any2Odyssey-99.9.9-windows-x64.zip"}},
            {{"name":"Any2Odyssey-99.9.9-windows-x64-setup.exe","browser_download_url":"https://github.com/jlobue10/Any2Odyssey/releases/download/v99.9.9/Any2Odyssey-99.9.9-windows-x64-setup.exe"}}]}}"#
        );
        let state =
            UpdateState::from_result(evaluate_latest_release(&body).map_err(|e| format!("{e:?}")));
        assert!(state.available().is_some());
        state
    }

    fn viewer(input: Input, output: Output) -> ViewerState {
        ViewerState::for_layout(
            Options {
                input,
                output,
                ..Options::default()
            },
            crate::ui::viewer::READY,
            "",
            None,
        )
    }

    /// The controls with everything long that can be long.
    fn crowded_controls(h: &mut Harness) {
        h.app.tray_failed = true;
        {
            let mut core = h.core();
            core.hotkeys_ok = false;
            core.status = long_statuses()[0].clone();
            core.prefs.mode = CaptureMode::Window;
            core.windows = LONG_TITLES
                .iter()
                .enumerate()
                .map(|(i, title)| WindowChoice {
                    handle: i as isize + 1,
                    title: (*title).to_owned(),
                })
                .collect();
            core.selected_window = Some(1);
        }
        // The widest readouts, and with them the "Unapplied changes" hint.
        h.app.controls.depth_pos = 200;
        h.app.controls.pop_pos = -100;
    }

    /// The viewer with every row it has, the widest numbers and a path, a
    /// status and a complaint that are each several windows wide.
    fn crowded_viewer(h: &mut Harness) {
        let path = long_path();
        h.app.viewer = ViewerState::for_layout(
            Options {
                input: Input::Desktop,
                output: Output::Lens,
                depth_pos: 200,
                pop_pos: -100,
                sharp_pos: 100,
                steady_pos: 95,
                ..Options::default()
            },
            &format!("Preview saved to {path}."),
            &path,
            Some(&format!(
                "Could not save {path}: Permission denied (os error 13)"
            )),
        )
        // The longest the line under Depth detail gets: a GPU that could
        // not be used, with the end of what the runtime said.
        .with_depth_line("Depth is computed on the processor, which is several times slower: the NVIDIA GPU could not be used (…Failed to load library libonnxruntime_providers_cuda.so with error: libcublasLt.so.13: cannot open shared object file: No such file or directory). View logs has the whole message; the GPU is tried again when the program is started again.");
    }

    fn installed_somewhere_deep(h: &mut Harness) {
        h.core().update = UpdateState::Failed("error sending request for url (https://api.github.com/repos/jlobue10/Any2Odyssey/releases/latest): client error (Connect): dns error: No such host is known. (os error 11001)".into());
        let folder: String = std::iter::once("C:".to_owned())
            .chain((0..24).map(|i| format!("\\DeeplyNestedFolder{i:02}")))
            .collect();
        let record = InstallRecord {
            installed: Some("2026-11-08T14:10:33.1234567-05:00".into()),
            version: Some("0.8.6".into()),
            previous_version: Some("0.8.5".into()),
            action: Some("Upgrade".into()),
            app: Some(format!("{folder}\\Any2Odyssey.exe")),
            controls_only: true,
            worker: Some(format!("{folder}\\Any2OdysseyWorker.exe")),
            hash: Some("0123456789abcdef".repeat(4)),
        };
        h.app.about = AboutState::with_info(AboutInfo::assemble(
            "0.8.6",
            Some("0123456789ab"),
            Some(std::path::PathBuf::from(&folder)),
            Ok(Some(record)),
        ));
    }

    /// Every page in every state that changes what is on it.
    fn states() -> Vec<(String, Page, Setup)> {
        let mut out: Vec<(String, Page, Setup)> = vec![
            ("controls".into(), Page::Controls, Box::new(|_| {})),
            (
                "controls, crowded".into(),
                Page::Controls,
                Box::new(crowded_controls),
            ),
        ];
        for (i, status) in long_statuses().into_iter().enumerate() {
            out.push((
                format!("controls, long status {i}"),
                Page::Controls,
                Box::new(move |h| h.core().status = status.clone()),
            ));
        }
        for output in [Output::Anaglyph, Output::Sbs, Output::Odyssey, Output::Lens] {
            for input in [Input::Pair, Input::Ai, Input::Flat, Input::Desktop] {
                out.push((
                    format!("viewer, {output:?} from {input:?}"),
                    Page::Viewer,
                    Box::new(move |h| h.app.viewer = viewer(input, output)),
                ));
            }
        }
        out.push((
            "viewer, crowded".into(),
            Page::Viewer,
            Box::new(crowded_viewer),
        ));
        for (i, topic) in any2odyssey_core::help::topics().iter().enumerate() {
            out.push((
                format!("help, {}", topic.id),
                Page::Help,
                Box::new(move |h| h.app.help.selected = i),
            ));
        }
        out.push(("about".into(), Page::About, Box::new(|_| {})));
        out.push((
            "about, update available".into(),
            Page::About,
            Box::new(|h| h.core().update = update_available()),
        ));
        out.push((
            "about, checking".into(),
            Page::About,
            Box::new(|h| h.core().update = UpdateState::Checking),
        ));
        out.push((
            "about, check failed, installed deep".into(),
            Page::About,
            Box::new(installed_somewhere_deep),
        ));
        out
    }

    /// Window sizes in points with the display scale they come with: the
    /// default and the smallest window at every scale Windows and the Linux
    /// desktops offer, the window maximized on the common screens, the odd
    /// shapes a tiling window manager makes, and the default window with the
    /// zoom at both ends of what [`largest_zoom`] leaves (0.2 to 1.7).
    fn sizes() -> Vec<(f32, f32, f32)> {
        let mut out = Vec::new();
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0] {
            out.push((980.0, 780.0, scale));
            out.push((700.0, 580.0, scale));
            out.push((SMALLEST.0, SMALLEST.1, scale));
        }
        out.extend([
            (1280.0, 720.0, 1.0),  // 1280x720
            (1920.0, 1080.0, 1.0), // 1920x1080
            (3840.0, 2160.0, 1.0), // 4K at 100 %
            (2560.0, 1440.0, 1.5), // 4K at 150 %
            (1920.0, 1080.0, 2.0), // 4K at 200 %
            (1536.0, 864.0, 1.25), // 1920x1080 at 125 %
            (1280.0, 720.0, 1.5),  // 1920x1080 at 150 %
            (700.0, 2000.0, 1.0),
            (3000.0, 580.0, 1.0),
            (888.0, 580.0, 1.0), // the narrowest window with two columns
            (887.0, 580.0, 1.0), // the widest with one
            (3000.0, SMALLEST.1, 1.0),
            (888.0, SMALLEST.1, 1.0),
            (887.0, SMALLEST.1, 1.0),
            // The smallest window at the largest zoom it takes.
            (SMALLEST.0 / 1.2, SMALLEST.1 / 1.2, 1.2),
            // Lower than the viewport allows: a program that sizes windows.
            (700.0, 300.0, 1.0),
            (700.0, 240.0, 1.0),
            (980.0 / 1.7, 780.0 / 1.7, 1.7),
            (980.0 / 1.5, 780.0 / 1.5, 1.5),
            (980.0 / 1.1, 780.0 / 1.1, 1.1),
            (980.0 / 0.5, 780.0 / 0.5, 0.5),
        ]);
        out
    }

    /// What a user operates: it needs a name, room, and a place of its own.
    fn operable(w: &Widget) -> bool {
        matches!(
            w.role,
            Role::Button
                | Role::CheckBox
                | Role::Slider
                | Role::ComboBox
                | Role::TextInput
                | Role::SpinButton
                | Role::Link
        )
    }

    fn short(text: &str) -> String {
        text.chars().take(40).collect()
    }

    /// The checks of one frame. Returns what is wrong, in words.
    fn faults(h: &Harness, painted: &[Painted]) -> Vec<String> {
        let (width, height) = h.size;
        let mut faults = Vec::new();
        for p in painted {
            // What can be seen of it. A page that is longer than the window
            // scrolls, and a line at the edge of what scrolls is cut by that
            // edge: nothing may be seen beyond the window's own frame, where
            // no scrolling brings it back. (What scrolls is seen 3 points
            // beyond its own edge, which is egui's margin for it; the
            // frame's innermost line is drawn there.)
            let seen = p.rect.intersect(p.clip);
            if seen.is_positive()
                && (seen.top() < FRAME - 0.5 || seen.bottom() > height - FRAME + 3.5)
            {
                faults.push(format!(
                    "text over the window's upper or lower edge: {:?} is seen at y {:.0}..{:.0}",
                    short(&p.text),
                    seen.top(),
                    seen.bottom()
                ));
            }
            let clipped =
                p.rect.left() < p.clip.left() - 0.5 || p.rect.right() > p.clip.right() + 0.5;
            // A text field scrolls what it holds; nothing else may be cut.
            if clipped
                && !h
                    .widgets
                    .iter()
                    .any(|w| w.role == Role::TextInput && w.rect.expand(4.0).contains_rect(p.clip))
            {
                faults.push(format!("text cut off sideways: {:?}", short(&p.text)));
            }
            if !clipped && (p.rect.left() < FRAME - 2.5 || p.rect.right() > width - FRAME + 0.5) {
                faults.push(format!(
                    "text outside the window frame: {:?} spans x {:.0}..{:.0}",
                    short(&p.text),
                    p.rect.left(),
                    p.rect.right()
                ));
            }
        }
        // The interface's own signs are ones the built-in typefaces have.
        // One they lack is drawn with egui's stand-in, a square, and changes
        // its shape when a typeface of the system is added for a window
        // title or a file name (letters are what those are added for, and
        // the stand-in itself always reads as lacking).
        let font = egui::FontId::proportional(14.0);
        for p in painted {
            for sign in p.text.chars().filter(|c| {
                *c as u32 >= 0x100 && !c.is_alphanumeric() && !c.is_whitespace() && *c != '\u{25FB}'
            }) {
                if !h.ctx.fonts_mut(|fonts| fonts.has_glyph(&font, sign)) {
                    faults.push(format!(
                        "a sign the built-in typefaces lack: U+{:04X} in {:?}",
                        sign as u32,
                        short(&p.text)
                    ));
                }
            }
        }
        let widgets: Vec<&Widget> = h.widgets.iter().filter(|w| operable(w)).collect();
        for w in &widgets {
            let what = format!("{:?} {:?}", w.role, short(&w.name));
            if w.name.trim().is_empty() {
                faults.push(format!(
                    "no name for assistive software: {:?} with value {:?} / {:?} at {:.0},{:.0}",
                    w.role,
                    w.value,
                    w.number,
                    w.rect.left(),
                    w.rect.top()
                ));
            }
            if w.rect.left() < FRAME - 0.5 || w.rect.right() > width - FRAME + 0.5 {
                faults.push(format!(
                    "cut by the window's edge: {what} spans x {:.0}..{:.0}",
                    w.rect.left(),
                    w.rect.right()
                ));
            }
            // Links are lines of text, as high as their text, and the path
            // box is a line of text with a frame: 20 points.
            if w.role != Role::Link && (w.rect.width() < 24.0 || w.rect.height() < 19.5) {
                faults.push(format!(
                    "too small to hit: {what} is {:.0}x{:.0}",
                    w.rect.width(),
                    w.rect.height()
                ));
            }
        }
        for (i, a) in widgets.iter().enumerate() {
            for b in &widgets[i + 1..] {
                let both = a.rect.intersect(b.rect);
                if both.width() > 1.0 && both.height() > 1.0 {
                    faults.push(format!(
                        "two controls in one place: {:?} {:?} and {:?} {:?}",
                        a.role,
                        short(&a.name),
                        b.role,
                        short(&b.name)
                    ));
                }
            }
        }
        // A button or a checkbox reacts where it is drawn: no wider than
        // its text and what is drawn around it.
        for w in widgets
            .iter()
            .filter(|w| matches!(w.role, Role::Button | Role::CheckBox))
        {
            let text = painted
                .iter()
                .filter(|p| p.text == w.name && w.rect.expand(2.0).contains_rect(p.rect))
                .map(|p| p.rect.width())
                .fold(0.0, f32::max);
            // The fixed-size buttons: the two primary ones and the title
            // bar's squares. A help topic is a row of the topic list.
            let fixed = w.rect.width() <= 190.5 && w.rect.height() == 34.0;
            let topic = h.app.page == Page::Help && w.rect.left() < 0.3 * width;
            if text > 0.0 && !fixed && !topic && w.rect.width() > text + 44.0 {
                faults.push(format!(
                    "reacts beyond what is drawn: {:?} {:?} is {:.0} wide around {:.0} of text",
                    w.role,
                    short(&w.name),
                    w.rect.width(),
                    text
                ));
            }
        }
        faults
    }

    #[test]
    fn every_page_holds_at_every_size_scale_and_zoom() {
        let mut wrong = Vec::new();
        for (name, page, setup) in states() {
            for (width, height, scale) in sizes() {
                let mut h = Harness::new((width, height), page);
                h.scale = scale;
                setup(&mut h);
                let painted = h.settle();
                assert!(!h.widgets.is_empty(), "{name}: no accessibility tree");
                for fault in faults(&h, &painted) {
                    wrong.push(format!(
                        "{name} at {width:.0}x{height:.0} scale {scale}: {fault}"
                    ));
                }
            }
        }
        wrong.dedup();
        assert!(
            wrong.is_empty(),
            "{} fault(s):\n{}",
            wrong.len(),
            wrong.join("\n")
        );
    }

    /// The names and values a screen reader is given for the controls that
    /// have no text of their own.
    #[test]
    fn sliders_boxes_and_fields_are_named_after_their_rows() {
        let named = |h: &Harness, role: Role| -> Vec<String> {
            let mut names: Vec<String> = h
                .widgets
                .iter()
                .filter(|w| w.role == role)
                .map(|w| w.name.clone())
                .collect();
            names.sort();
            names
        };
        let mut h = Harness::new(SIZES[0], Page::Controls);
        h.settle();
        assert_eq!(named(&h, Role::Slider), ["Depth strength", "Pop-out"]);
        assert_eq!(
            named(&h, Role::ComboBox),
            ["Capture", "Input format", "Source window"]
        );
        let depth = h
            .widgets
            .iter()
            .find(|w| w.name == "Depth strength")
            .unwrap();
        assert_eq!(depth.number, Some(90.0), "the slider's value is exposed");
        let capture = h.widgets.iter().find(|w| w.name == "Capture").unwrap();
        assert_eq!(capture.value.as_deref(), Some(CaptureMode::Desktop.label()));

        let mut h = Harness::new(SIZES[0], Page::Viewer);
        crowded_viewer(&mut h);
        h.settle();
        assert_eq!(
            named(&h, Role::Slider),
            ["Depth steadiness", "Depth strength", "Pop-out", "Sharpness"]
        );
        assert_eq!(
            named(&h, Role::ComboBox),
            ["Depth detail", "Detail", "Input format", "Output format"]
        );
        assert_eq!(
            named(&h, Role::SpinButton),
            ["Lens pitch", "Offset", "Slant"]
        );
        assert_eq!(named(&h, Role::TextInput), ["Image"]);
    }

    /// Press Tab until the focus is back where it started; every stop with
    /// its name, after the page has had time to scroll to it.
    fn tab_stops(h: &mut Harness) -> Vec<Widget> {
        let mut stops: Vec<Widget> = Vec::new();
        let mut seen = HashSet::new();
        for _ in 0..120 {
            h.press(egui::Key::Tab);
            // A scroll to the focused control takes up to 0.3 seconds, and
            // a frame without a clock counts as 1/60.
            for _ in 0..24 {
                h.run(Vec::new());
            }
            let Some(focused) = h.focused().cloned() else {
                continue;
            };
            if !seen.insert(focused.id) {
                break;
            }
            stops.push(focused);
        }
        stops
    }

    #[test]
    fn tab_goes_left_to_right_through_the_title_bar_and_the_page_follows_the_focus() {
        // The smallest window, where every page is longer than the window,
        // and the default one, where the Linux viewer is.
        for (size, page, setup) in [
            (
                SIZES[1],
                Page::Controls,
                Box::new(crowded_controls) as Setup,
            ),
            (SIZES[1], Page::Viewer, Box::new(crowded_viewer)),
            (SIZES[0], Page::Viewer, Box::new(crowded_viewer)),
            (
                SIZES[1],
                Page::About,
                Box::new(|h| h.core().update = update_available()),
            ),
            (SIZES[1], Page::Help, Box::new(|h| h.app.help.selected = 6)),
            // The smallest window: 360 points high, of which a page has 250.
            (SMALLEST, Page::Controls, Box::new(crowded_controls)),
            (SMALLEST, Page::Viewer, Box::new(crowded_viewer)),
            (
                SMALLEST,
                Page::About,
                Box::new(|h| h.core().update = update_available()),
            ),
            (SMALLEST, Page::Help, Box::new(|h| h.app.help.selected = 6)),
        ] {
            let mut h = Harness::new(size, page);
            setup(&mut h);
            h.settle();
            let window = Rect::from_min_size(pos2(0.0, 0.0), vec2(size.0, size.1));
            let content = Rect::from_min_max(
                pos2(FRAME, FRAME + TITLEBAR_HEIGHT),
                pos2(size.0 - FRAME, size.1 - FRAME),
            );
            // What can be operated before a key is pressed. (A link that
            // is scrolled out of view has no role or name yet: egui gives
            // text those when it is laid out in view.)
            let operable_at_first: Vec<Widget> = h
                .widgets
                .iter()
                .filter(|w| operable(w) && w.focusable && !w.disabled)
                .cloned()
                .collect();
            let stops = tab_stops(&mut h);
            let names: Vec<&str> = stops.iter().map(|w| w.name.as_str()).collect();
            // Reading order, and nothing before About: the title bar's drag
            // area and the page's background used to be stops of their own.
            assert_eq!(
                names[..7],
                [
                    "About",
                    "Help",
                    "Dark",
                    "Minimize",
                    "Maximize / restore",
                    "Minimize",
                    "Close"
                ],
                "{names:?}"
            );
            // In that order on the screen too, each to the right of the one
            // before it.
            for pair in stops[..7].windows(2) {
                assert!(
                    pair[0].rect.right() <= pair[1].rect.left(),
                    "{:?} is not left of {:?}",
                    pair[0].name,
                    pair[1].name
                );
            }
            // The page's background is not a stop, and neither is the
            // scrolling area of the controls and of the viewer (it was
            // egui's own, for dragging the page: `drag_with_a_finger`).
            // Both had the whole page for their place, without a name.
            let whole_page = Rect::from_min_max(
                pos2(FRAME, FRAME + TITLEBAR_HEIGHT + 8.0),
                pos2(size.0 - FRAME, size.1 - FRAME),
            );
            let near = |a: egui::Pos2, b: egui::Pos2| (a - b).length() < 1.0;
            let over_the_page = stops
                .iter()
                .filter(|s| near(s.rect.min, whole_page.min) && near(s.rect.max, whole_page.max))
                .count();
            assert_eq!(over_the_page, 0, "{names:?}");
            for (i, stop) in stops.iter().enumerate() {
                if stop.role == Role::Unknown {
                    // egui's own: a scrolling area and its bar. Nothing of
                    // this program's is a stop without a name.
                    assert!(i >= 7, "an unnamed stop in the title bar: {names:?}");
                    continue;
                }
                assert!(!stop.name.is_empty(), "stop {i} has no name: {stop:?}");
                assert!(
                    window.contains_rect(stop.rect),
                    "{:?} has the focus at {:?}, outside the {size:?} window",
                    stop.name,
                    stop.rect
                );
                if i >= 7 {
                    assert!(
                        content.expand(1.0).contains_rect(stop.rect),
                        "{:?} has the focus at {:?}, under the title bar or the frame",
                        stop.name,
                        stop.rect
                    );
                }
            }
            // Tab reached everything that can be operated.
            for widget in &operable_at_first {
                assert!(
                    stops.iter().any(|stop| stop.id == widget.id),
                    "Tab never reached {:?} {:?}: {names:?}",
                    widget.role,
                    widget.name
                );
            }
            assert!(operable_at_first.len() >= 8, "{names:?}");
        }
    }

    /// The wheel over the middle of the page, far enough for any page.
    fn wheel_to_the_end(h: &mut Harness) -> Vec<Painted> {
        let middle = pos2(h.size.0 * 0.6, h.size.1 * 0.7);
        h.run(vec![egui::Event::PointerMoved(middle)]);
        for _ in 0..40 {
            h.run(vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -400.0),
                modifiers: Default::default(),
            }]);
        }
        h.settle()
    }

    /// In the smallest window no page is shown whole, and every page is
    /// brought to its last line by the wheel: nothing is below the window
    /// for good. (Up to 0.9.6 the smallest window was 580 points high.)
    #[test]
    fn every_page_scrolls_to_its_end_in_the_smallest_window() {
        let last_topic = any2odyssey_core::help::topics().len() - 1;
        let cases: [(Page, Setup, &str); 4] = [
            (
                Page::Controls,
                Box::new(crowded_controls),
                "Samsung Odyssey G90XF",
            ),
            (Page::Viewer, Box::new(crowded_viewer), "stereo viewer"),
            (
                Page::About,
                Box::new(|h| h.core().update = update_available()),
                "Report a problem",
            ),
            (
                Page::Help,
                Box::new(move |h| h.app.help.selected = last_topic),
                "",
            ),
        ];
        for (page, setup, last) in cases {
            for scale in [1.0, 2.0] {
                let mut h = Harness::new(SMALLEST, page);
                h.scale = scale;
                setup(&mut h);
                let first = h.settle();
                let context = format!("{} at scale {scale}", last);
                // The last line of the Help article is its last paragraph.
                let last = if page == Page::Help {
                    let topic = &any2odyssey_core::help::topics()[last_topic];
                    let body = any2odyssey_core::help::paragraphs(&topic.body);
                    let line = body.last().unwrap().lines().last().unwrap();
                    line.chars().take(30).collect::<String>()
                } else {
                    last.to_owned()
                };
                let inside = |painted: &[Painted]| {
                    painted.iter().any(|p| {
                        let seen = p.rect.intersect(p.clip);
                        p.text.contains(&last)
                            && seen.height() >= p.rect.height().min(14.0) - 0.5
                            && seen.bottom() <= SMALLEST.1 - FRAME + 3.5
                    })
                };
                assert!(
                    !inside(&first),
                    "{context}: the page fits, nothing to scroll"
                );
                let scrolled = wheel_to_the_end(&mut h);
                assert!(
                    inside(&scrolled),
                    "{context}: {last:?} was not brought into the window; what is seen there: {:?}",
                    scrolled
                        .iter()
                        .filter(|p| p.rect.intersect(p.clip).is_positive())
                        .map(|p| (short(&p.text), p.rect.top().round()))
                        .collect::<Vec<_>>()
                );
                assert!(
                    faults(&h, &scrolled).is_empty(),
                    "{context}: {:?}",
                    faults(&h, &scrolled)
                );
            }
        }
    }

    /// Tab until the control called `name` has the focus.
    fn tab_to(h: &mut Harness, name: &str) {
        for _ in 0..60 {
            h.press(egui::Key::Tab);
            h.settle();
            if h.focused().is_some_and(|w| w.name == name) {
                return;
            }
        }
        panic!("Tab never reached {name:?}");
    }

    #[test]
    fn the_keyboard_alone_operates_sliders_checkboxes_lists_and_buttons() {
        let mut h = Harness::new(SIZES[0], Page::Controls);
        h.settle();
        // A slider moves with the arrow keys, one step a press.
        tab_to(&mut h, "Depth strength");
        h.press(egui::Key::ArrowRight);
        h.press(egui::Key::ArrowRight);
        h.press(egui::Key::ArrowLeft);
        assert_eq!(h.app.controls.depth_pos, 91);
        let painted = h.settle();
        find(&painted, "0.91");
        find(&painted, "Unapplied changes");
        // Space ticks a checkbox; this one saves at once.
        assert!(h.core().prefs.auto_convert);
        tab_to(&mut h, "Convert automatically when this app opens");
        h.press(egui::Key::Space);
        h.settle();
        assert!(!h.core().prefs.auto_convert);
        // Space opens a list, Escape closes it without a choice.
        tab_to(&mut h, "Capture");
        h.press(egui::Key::Space);
        let painted = h.settle();
        find(&painted, CaptureMode::Window.label());
        h.press(egui::Key::Escape);
        let painted = h.settle();
        assert!(try_find(&painted, CaptureMode::Window.label()).is_none());
        assert_eq!(h.app.controls.mode, CaptureMode::Desktop);
        // Enter presses a button: Apply takes the slider's new value.
        tab_to(&mut h, "Apply settings / restart conversion");
        h.press(egui::Key::Enter);
        h.settle();
        assert_eq!(h.core().prefs.depth, 0.91);
        // F1 opens Help from anywhere, a text field included.
        let mut h = Harness::new(SIZES[0], Page::Viewer);
        h.settle();
        tab_to(&mut h, "Image");
        h.press(egui::Key::F1);
        h.settle();
        assert!(h.app.page == Page::Help);
    }

    /// Click at `at`; what the click reported as clicked or focused.
    fn clicked(h: &mut Harness, at: egui::Pos2) -> Vec<(bool, String, Option<bool>)> {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        };
        h.run(vec![egui::Event::PointerMoved(at)]);
        h.run(vec![button(true)]);
        let mut events = reported(&h.events);
        h.run(vec![button(false)]);
        events.extend(reported(&h.events));
        h.settle();
        events
    }

    #[test]
    fn a_click_beside_a_button_or_a_checkbox_is_not_a_click_on_it() {
        // Two columns, as in the default window, and high enough to show
        // all of the page: only what is in view is painted and found.
        let mut h = Harness::new((SIZES[0].0, 2000.0), Page::Viewer);
        crowded_viewer(&mut h);
        let painted = h.settle();
        for name in [
            "Reset lens numbers",
            "Save preview PNG…",
            "Choose another screen",
            "Show the calibration pattern",
            "Swap left and right eyes",
        ] {
            let text = find(&painted, name).rect;
            // The same row, 150 points to the right of the text: the card.
            let beside = pos2(text.right() + 150.0, text.center().y);
            assert_eq!(clicked(&mut h, beside), vec![], "a click beside {name:?}");
        }
        // On it, it is one.
        let text = find(&painted, "Show the calibration pattern").rect;
        assert_eq!(
            clicked(&mut h, text.center()),
            vec![(true, "Show the calibration pattern".to_owned(), Some(true))]
        );

        let mut h = Harness::new(SIZES[0], Page::Controls);
        let painted = h.settle();
        let before = h.core().prefs.clone();
        for name in [
            "Convert automatically when this app opens",
            "Recover if the conversion process stops",
            "Keep 3D above apps without taking focus",
        ] {
            let text = find(&painted, name).rect;
            let column_end = if text.left() < 400.0 { 480.0 } else { 944.0 };
            assert!(text.right() + 30.0 < column_end, "{name}");
            let beside = pos2(column_end - 6.0, text.center().y);
            assert_eq!(clicked(&mut h, beside), vec![], "a click beside {name:?}");
        }
        assert_eq!(h.core().prefs, before);
        assert!(!h.app.controls.dirty(&h.core()));
    }

    #[test]
    fn zoom_stops_where_the_title_bar_is_still_whole() {
        assert_eq!(largest_zoom(980.0), 1.7);
        assert_eq!(largest_zoom(700.0), 1.2);
        assert_eq!(largest_zoom(2560.0), 4.5);
        // A window forced narrower than the page still shows at zoom 1.
        assert_eq!(largest_zoom(300.0), 1.0);
        assert_eq!(largest_zoom(0.0), 1.0);
        // The default window, 980 pixels wide, zoomed as far as the keys go:
        // every frame gets the points the zoom of the frame before left.
        let mut h = Harness::new(SIZES[0], Page::Controls);
        h.ctx.set_zoom_factor(5.0);
        let mut painted = Vec::new();
        for _ in 0..6 {
            let zoom = h.ctx.zoom_factor();
            h.size = (980.0 / zoom, 780.0 / zoom);
            h.scale = 1.0;
            painted = h.run(Vec::new());
        }
        assert_eq!(h.ctx.zoom_factor(), 1.7);
        assert_eq!(faults(&h, &painted), Vec::<String>::new());
        // The title and the leftmost button side by side.
        let title = find(&painted, "ANY2ODYSSEY").rect;
        let about = find(&painted, "About").rect;
        assert!(title.right() < about.left() - 13.0, "{title:?} {about:?}");
        // Zooming out is not held back, and neither is 1.0 anywhere.
        h.ctx.set_zoom_factor(0.2);
        for _ in 0..3 {
            let zoom = h.ctx.zoom_factor();
            h.size = (980.0 / zoom, 780.0 / zoom);
            h.run(Vec::new());
        }
        assert_eq!(h.ctx.zoom_factor(), 0.2);
    }

    /// A control that ends in the three points a scroll area may paint over
    /// below its own edge is not in view: those points lie under the window's
    /// frame. Whether a control ends there is a matter of the window's
    /// height, so every height around the default is tried; at 780 to 782
    /// "Reset lens numbers" did, and Tab left it there.
    #[test]
    fn the_page_follows_the_focus_to_a_control_that_ends_under_the_frame() {
        for height in 765..=795 {
            let size = (SIZES[0].0, height as f32);
            let mut h = Harness::new(size, Page::Viewer);
            h.app.viewer = viewer(Input::Pair, Output::Lens);
            h.settle();
            tab_to(&mut h, "Reset lens numbers");
            // A scroll to the focused control takes up to 0.3 seconds.
            for _ in 0..24 {
                h.run(Vec::new());
            }
            let focused = h.focused().expect("the focus").rect;
            assert!(
                focused.bottom() <= size.1 - FRAME + 1.0,
                "in a window {height} points high the focused button ends at {}, under the frame",
                focused.bottom()
            );
            assert!(focused.top() >= FRAME + TITLEBAR_HEIGHT, "{focused:?}");
        }
    }

    /// A gold button in a column of `Ui::columns`, where the layout hands
    /// every widget the column's whole width: it reacts where it is drawn.
    /// (The cards of the pages no longer lay their contents out like that,
    /// so the pages alone do not show it.)
    #[test]
    fn a_gold_button_in_a_justified_column_reacts_where_it_is_drawn() {
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let palette = Palette::for_dark(false);
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(600.0, 200.0))),
            ..Default::default()
        };
        let mut seen = None;
        let _ = ctx.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.columns(1, |columns| {
                    let ui = &mut columns[0];
                    let column = ui.available_width();
                    let drawn = widgets::gold_button_width(ui, "Reset lens numbers");
                    let response = widgets::gold_button(ui, "Reset lens numbers", false, &palette);
                    seen = Some((response.rect.width(), drawn, column));
                });
            });
        });
        let (reacts, drawn, column) = seen.expect("the column was laid out");
        assert!(column > drawn + 200.0, "{column} {drawn}");
        assert!(
            (reacts - drawn).abs() < 0.5,
            "reacts over {reacts} of {drawn}"
        );
    }

    /// The window has a name for assistive software: the tree's root has
    /// none of its own, and on Linux the title is not taken for it.
    #[test]
    fn the_window_is_named_in_the_accessibility_tree() {
        for page in [Page::Controls, Page::Viewer, Page::Help, Page::About] {
            let mut h = Harness::new(SIZES[0], page);
            h.settle();
            assert_eq!(h.window_name.as_deref(), Some(APP_NAME));
        }
    }

    /// Where the page is: the top of the banner's line, which no click on
    /// the page moves. `None` once the page is scrolled so far that the
    /// line is out of view.
    fn banner_at(painted: &[Painted]) -> Option<f32> {
        try_find(painted, "CONTINUOUS DESKTOP 3D").map(|p| p.rect.top())
    }

    /// A page in a window it does not fit: what it is called, the window's
    /// size, and a checkbox and the primary button that are in view.
    type ScrollingPage = (&'static str, Page, (f32, f32), [&'static str; 2]);

    /// The two pages in windows they do not fit: the viewer in its default
    /// window, and the controls in one 640 points high (they fit the
    /// default one).
    fn pages_that_scroll() -> [ScrollingPage; 2] {
        [
            (
                "the viewer",
                Page::Viewer,
                SIZES[0],
                ["Swap left and right eyes", "Start output"],
            ),
            (
                "the controls",
                Page::Controls,
                (980.0, 640.0),
                ["Convert automatically when this app opens", "Turn on 3D"],
            ),
        ]
    }

    fn button_at(at: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Default::default(),
        }
    }

    /// A press on a checkbox or a button was the start of a drag of the
    /// page (`widgets::dragged_by_touch`): the page moved as far as the
    /// pointer had moved in the frame of the press. With a pointer that is
    /// put on a control and pressed in one go that was all the way from
    /// where it had been, and the page went to its end; with a hand that is
    /// still slowing down, a few points at every click.
    #[test]
    fn a_click_leaves_the_page_where_it_is_however_the_pointer_comes_to_it() {
        for (what, page, size, names) in pages_that_scroll() {
            for name in names {
                for arrival in ["at rest", "put there and pressed at once", "slowing down"] {
                    let mut h = Harness::new(size, page);
                    let painted = h.settle();
                    let before = banner_at(&painted);
                    assert!(before.is_some(), "{what}: no banner");
                    let target = h
                        .widgets
                        .iter()
                        .find(|w| w.name == name)
                        .unwrap_or_else(|| panic!("{name} is not on the page"))
                        .rect
                        .center();
                    // The pointer has been lower on the page, and at rest.
                    h.run(vec![egui::Event::PointerMoved(target + vec2(40.0, 250.0))]);
                    h.settle();
                    let frames: Vec<Vec<egui::Event>> = match arrival {
                        "at rest" => vec![
                            vec![egui::Event::PointerMoved(target)],
                            vec![button_at(target, true)],
                            vec![button_at(target, false)],
                        ],
                        "put there and pressed at once" => vec![
                            vec![egui::Event::PointerMoved(target), button_at(target, true)],
                            vec![button_at(target, false)],
                        ],
                        // Up to the control at 8, 8 and 6 points a frame;
                        // the button goes down in a frame in which the
                        // pointer moved 4 more, and it moves 2 while held.
                        _ => {
                            let at = |below: f32| target + vec2(0.0, below);
                            vec![
                                vec![egui::Event::PointerMoved(at(20.0))],
                                vec![egui::Event::PointerMoved(at(12.0))],
                                vec![egui::Event::PointerMoved(at(6.0))],
                                vec![egui::Event::PointerMoved(at(2.0)), button_at(at(2.0), true)],
                                vec![egui::Event::PointerMoved(at(0.0))],
                                vec![button_at(at(0.0), false)],
                            ]
                        }
                    };
                    let mut said = Vec::new();
                    for events in frames {
                        h.run(events);
                        said.extend(reported(&h.events));
                    }
                    assert!(
                        said.iter()
                            .any(|(clicked, label, _)| *clicked && label == name),
                        "{name}, {arrival}: the click said {said:?}"
                    );
                    // A page that was thrown goes on moving for a second.
                    for frame in 0..60 {
                        let painted = h.run(Vec::new());
                        assert_eq!(
                            banner_at(&painted),
                            before,
                            "{what}: a click on {name:?} with the pointer {arrival} moved the page (frame {frame})"
                        );
                    }
                }
            }
        }
    }

    /// A finger still drags the page, from anywhere on it, a checkbox
    /// included, and a drag is not a click.
    #[test]
    fn a_finger_drags_the_page_and_does_not_click() {
        for (what, page, size, names) in pages_that_scroll() {
            let mut h = Harness::new(size, page);
            let painted = h.settle();
            let before = banner_at(&painted).expect("the banner");
            let start = h
                .widgets
                .iter()
                .find(|w| w.name == names[0])
                .expect("the checkbox")
                .rect
                .center();
            let touch = |phase, pos| egui::Event::Touch {
                device_id: egui::TouchDeviceId(1),
                id: egui::TouchId(7),
                phase,
                pos,
                force: None,
            };
            // What a touch screen sends: the touch, and a pointer with it.
            let mut said = Vec::new();
            let mut frames = vec![vec![
                touch(egui::TouchPhase::Start, start),
                egui::Event::PointerMoved(start),
                button_at(start, true),
            ]];
            let mut at = start;
            for _ in 0..5 {
                at.y -= 10.0;
                frames.push(vec![
                    touch(egui::TouchPhase::Move, at),
                    egui::Event::PointerMoved(at),
                ]);
            }
            frames.push(vec![
                touch(egui::TouchPhase::End, at),
                button_at(at, false),
                egui::Event::PointerGone,
            ]);
            for events in frames {
                h.run(events);
                said.extend(reported(&h.events));
            }
            let painted = h.settle();
            assert!(
                banner_at(&painted).is_none_or(|now| now <= before - 40.0),
                "{what}: the banner is at {:?}, it was at {before}",
                banner_at(&painted)
            );
            assert_eq!(said, vec![], "{what}: the drag was taken for a click");
        }
    }

    /// The tree above comes from egui; whether a screen reader is given it
    /// is decided by a feature of eframe, which the tests cannot see. With
    /// `default-features = false` and no "accesskit" the program had none,
    /// on Windows (UI Automation showed the window and nothing in it) and on
    /// Linux (not on the accessibility bus), for as long as it named its
    /// buttons "for assistive software".
    #[test]
    fn the_program_is_built_with_the_accessibility_tree() {
        let manifest = include_str!("../../Cargo.toml");
        let eframe = manifest
            .lines()
            .find(|line| line.starts_with("eframe = "))
            .expect("the eframe dependency");
        assert!(eframe.contains("default-features = false"), "{eframe}");
        assert!(eframe.contains("\"accesskit\""), "{eframe}");
    }
}
