//! The Controls page: conversion state, source & input, depth & pop-out,
//! startup & window behaviour, shortcuts, links and version.

use any2odyssey_core::prefs::CaptureMode;
use any2odyssey_core::slider::{DEPTH, POPOUT};
use any2odyssey_core::theme::Palette;
use egui::{RichText, Ui};

use super::theme::color;
use super::widgets::{banner, gold_button, row, section};
use crate::core::{Core, MenuCommand, UiRequest};

/// Width kept for the Refresh button beside the source box.
const REFRESH_WIDTH: f32 = 90.0;
/// The source list may be wider than its box, up to this.
const SOURCE_POPUP_WIDTH: f32 = 360.0;

/// Shown beside Apply while an edit is pending.
const UNAPPLIED_LABEL: &str = "Unapplied changes";
const UNAPPLIED_HINT: &str = "Capture, input, source, depth, pop-out and overlay changes take effect when you click Apply. Turn on 3D, the tray menu and automatic recovery use the applied settings.";

/// The settings that only Apply commits, as the shared state holds them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Applied {
    mode: CaptureMode,
    side_by_side: bool,
    keep_on_top: bool,
    depth_pos: i32,
    pop_pos: i32,
    source: Option<isize>,
}

impl Applied {
    fn of(core: &Core) -> Applied {
        Applied {
            mode: core.prefs.mode,
            side_by_side: core.prefs.side_by_side,
            keep_on_top: core.prefs.keep_overlay_on_top,
            depth_pos: DEPTH.position(core.prefs.depth),
            pop_pos: POPOUT.position(core.prefs.popout),
            source: core.selected_window,
        }
    }
}

/// What the Controls page shows for the settings Apply commits: capture
/// mode, input format, source window, the overlay's keep-on-top and the two
/// sliders. They are edited here and reach the shared state only through
/// [`ControlsState::commit`], which Apply calls. Everything else that saves
/// preferences or launches a worker (a checkbox, hiding to the tray, Quit,
/// the tray menu, Turn on 3D, a recovery relaunch) reads the shared state and
/// therefore uses the applied values, never a half-made edit.
pub struct ControlsState {
    pub depth_pos: i32,
    pub pop_pos: i32,
    pub mode: CaptureMode,
    pub side_by_side: bool,
    pub keep_on_top: bool,
    /// The window chosen in the source box. Like the applied choice it is
    /// never restored from a previous launch.
    pub source: Option<isize>,
    /// The applied values when this page last looked, to notice a change
    /// made elsewhere (a tray command, a refreshed window list).
    seen: Applied,
}

impl ControlsState {
    pub fn from_core(core: &Core) -> ControlsState {
        let seen = Applied::of(core);
        ControlsState {
            depth_pos: seen.depth_pos,
            pop_pos: seen.pop_pos,
            mode: seen.mode,
            side_by_side: seen.side_by_side,
            keep_on_top: seen.keep_on_top,
            source: seen.source,
            seen,
        }
    }

    fn edited(&self) -> Applied {
        Applied {
            mode: self.mode,
            side_by_side: self.side_by_side,
            keep_on_top: self.keep_on_top,
            depth_pos: self.depth_pos,
            pop_pos: self.pop_pos,
            source: self.source,
        }
    }

    /// An edit on this page has not been applied yet.
    pub fn dirty(&self, core: &Core) -> bool {
        self.edited() != Applied::of(core)
    }

    /// Follow applied values that changed behind this page. A tray command
    /// ("Side-by-side mode", "Keep overlay above apps", "Always convert
    /// desktop") is the newer decision about its setting and replaces a
    /// pending edit of the same setting; the others are left alone. (A
    /// command that leaves the applied value as it was is not noticed: the
    /// pending edit stays, and so does the hint beside Apply.) The
    /// source is different: the applied choice only changes here when its
    /// window has gone, which says nothing about a different window picked
    /// in the box, so that pick stays for as long as it is still listed.
    pub fn sync(&mut self, core: &Core) {
        let now = Applied::of(core);
        if now.mode != self.seen.mode {
            self.mode = now.mode;
        }
        if now.side_by_side != self.seen.side_by_side {
            self.side_by_side = now.side_by_side;
        }
        if now.keep_on_top != self.seen.keep_on_top {
            self.keep_on_top = now.keep_on_top;
        }
        if now.depth_pos != self.seen.depth_pos {
            self.depth_pos = now.depth_pos;
        }
        if now.pop_pos != self.seen.pop_pos {
            self.pop_pos = now.pop_pos;
        }
        if now.source != self.seen.source && self.source == self.seen.source {
            self.source = now.source;
        }
        if self
            .source
            .is_some_and(|chosen| !core.windows.iter().any(|w| w.handle == chosen))
        {
            self.source = None;
        }
        self.seen = now;
    }

    /// Apply: hand every edit to the shared state. The caller saves and
    /// restarts through `Core::apply`.
    pub fn commit(&mut self, core: &mut Core) {
        core.prefs.mode = self.mode;
        core.prefs.side_by_side = self.side_by_side;
        core.prefs.keep_overlay_on_top = self.keep_on_top;
        core.prefs.depth = DEPTH.value(self.depth_pos);
        core.prefs.popout = POPOUT.value(self.pop_pos);
        core.selected_window = self.source;
        self.seen = Applied::of(core);
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        core: &mut Core,
        p: &Palette,
        wordmark: Option<&egui::TextureHandle>,
        tray_available: bool,
        notices: &[&str],
    ) {
        self.sync(core);
        banner(ui, wordmark, p);
        ui.add_space(4.0);
        // CONVERSION
        section(ui, "CONVERSION", p, |ui| {
            ui.horizontal(|ui| {
                let button_width = 190.0;
                let status_width = (ui.available_width() - button_width - 12.0).max(100.0);
                // A vertical child with a width cap: inside the row's own
                // horizontal layout a label never wraps, so a long status
                // (every error is one) ran off the window and pushed the
                // button out with it.
                ui.allocate_ui_with_layout(
                    egui::vec2(status_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_max_width(status_width);
                        ui.add(
                            egui::Label::new(RichText::new(&core.status).color(color(p.text)))
                                .wrap(),
                        );
                    },
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if core.wanted() {
                        "Turn off 3D"
                    } else {
                        "Turn on 3D"
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
                        core.toggle();
                    }
                });
            });
            // Standing conditions from launch (shortcuts or tray icon
            // unavailable) stay here, where conversion status cannot
            // overwrite them.
            for notice in notices {
                ui.add(
                    egui::Label::new(RichText::new(*notice).color(color(p.muted)).size(12.0))
                        .wrap(),
                );
            }
        });
        ui.add_space(5.0);
        let two_columns = ui.available_width() >= 860.0;
        let source_panel = |ui: &mut Ui, core: &mut Core, state: &mut ControlsState| {
            section(ui, "SOURCE & INPUT", p, |ui| {
                row(ui, "Capture", |ui| {
                    let mut idx = state.mode.index();
                    egui::ComboBox::from_id_salt("capture-mode")
                        .width(ui.available_width())
                        .selected_text(CaptureMode::from_index(idx).label())
                        .show_ui(ui, |ui| {
                            for m in CaptureMode::ALL {
                                ui.selectable_value(&mut idx, m.index(), m.label());
                            }
                        });
                    state.mode = CaptureMode::from_index(idx);
                });
                row(ui, "Input format", |ui| {
                    let mut sbs = state.side_by_side;
                    let text = if sbs {
                        "Side-by-side input (manual)"
                    } else {
                        "2D → 3D automatically (AI)"
                    };
                    egui::ComboBox::from_id_salt("input-format")
                        .width(ui.available_width())
                        .selected_text(text)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut sbs, false, "2D → 3D automatically (AI)");
                            ui.selectable_value(&mut sbs, true, "Side-by-side input (manual)");
                        });
                    state.side_by_side = sbs;
                });
                row(ui, "Source window", |ui| {
                    let enabled = state.mode != CaptureMode::Desktop;
                    ui.add_enabled_ui(enabled, |ui| {
                        let selected = state
                            .source
                            .and_then(|h| core.windows.iter().find(|w| w.handle == h))
                            .map(|w| w.title.clone());
                        // The box gets a fixed share of the row and elides a
                        // long title. Left to itself it grows with the text
                        // (a row never wraps), which slid Refresh under the
                        // Apply button of the neighbouring card.
                        let combo_width = (ui.available_width() - REFRESH_WIDTH).max(80.0);
                        let popup_width = combo_width.max(SOURCE_POPUP_WIDTH);
                        let combo = ui
                            .allocate_ui_with_layout(
                                egui::vec2(combo_width, 26.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    ui.set_max_width(combo_width);
                                    egui::ComboBox::from_id_salt("source-window")
                                        .width(combo_width)
                                        .truncate()
                                        .selected_text(
                                            selected.as_deref().unwrap_or("Choose a window"),
                                        )
                                        .show_ui(ui, |ui| {
                                            // The list is bounded too; the
                                            // full title is in the hover text.
                                            ui.set_max_width(popup_width);
                                            ui.style_mut().wrap_mode =
                                                Some(egui::TextWrapMode::Truncate);
                                            let windows = core.windows.clone();
                                            for w in windows {
                                                ui.selectable_value(
                                                    &mut state.source,
                                                    Some(w.handle),
                                                    &w.title,
                                                )
                                                .on_hover_text(&w.title);
                                            }
                                        })
                                        .response
                                },
                            )
                            .inner;
                        if let Some(title) = selected {
                            combo.on_hover_text(title);
                        }
                        if gold_button(ui, "Refresh", false, p).clicked() {
                            core.refresh_windows();
                            state.sync(core);
                        }
                    });
                })
                .on_hover_text("Selected window requires a choice each time this app opens; also seeds Follow active window. Refresh updates the list without choosing a replacement. Apply to use the choice.");
            });
        };
        let tuning_panel = |ui: &mut Ui, core: &mut Core, state: &mut ControlsState| {
            section(ui, "DEPTH & POP-OUT", p, |ui| {
                row(ui, "Depth strength", |ui| {
                    ui.add(egui::Slider::new(&mut state.depth_pos, DEPTH.min..=DEPTH.max).show_value(false).step_by(1.0));
                    ui.label(RichText::new(DEPTH.readout(state.depth_pos)).monospace());
                })
                .on_hover_text("Depth strength: 0.00–2.00. Apply settings to save; active conversion briefly restarts.");
                row(ui, "Pop-out", |ui| {
                    ui.add(egui::Slider::new(&mut state.pop_pos, POPOUT.min..=POPOUT.max).show_value(false).step_by(1.0));
                    ui.label(RichText::new(POPOUT.readout(state.pop_pos)).monospace());
                })
                .on_hover_text("Pop-out: −1.00–1.00. Apply settings to save; active conversion briefly restarts.");
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    if gold_button(ui, "Apply settings / restart conversion", false, p).clicked() {
                        state.commit(core);
                        core.apply();
                    }
                    // Turn on 3D, the tray and recovery start with the
                    // applied values; say so while the page shows others.
                    if state.dirty(core) {
                        ui.label(
                            RichText::new(UNAPPLIED_LABEL)
                                .color(color(p.accent))
                                .size(12.0),
                        )
                        .on_hover_text(UNAPPLIED_HINT);
                    }
                });
            });
        };
        if two_columns {
            ui.columns(2, |cols| {
                source_panel(&mut cols[0], core, self);
                tuning_panel(&mut cols[1], core, self);
            });
        } else {
            source_panel(ui, core, self);
            ui.add_space(5.0);
            tuning_panel(ui, core, self);
        }
        ui.add_space(5.0);
        section(ui, "STARTUP & WINDOW BEHAVIOR", p, |ui| {
            let mut auto = core.prefs.auto_convert;
            let mut recover = core.supervisor.recover;
            let mut minimize = core.prefs.minimize_to_tray;
            let mut keep = self.keep_on_top;
            let draw = |ui: &mut Ui,
                        auto: &mut bool,
                        recover: &mut bool,
                        minimize: &mut bool,
                        keep: &mut bool,
                        column: usize| {
                if column == 0 {
                    ui.checkbox(auto, "Convert automatically when this app opens");
                    ui.checkbox(recover, "Recover if the conversion process stops");
                }
                if column == 1 {
                    // Without a notification area the setting cannot act: a
                    // hidden window would have nothing to bring it back.
                    ui.add_enabled(
                        tray_available,
                        egui::Checkbox::new(minimize, "Minimize and close to the tray"),
                    )
                    .on_disabled_hover_text(
                        "No notification icon is available (see companion.log on Windows); Minimize and Close act normally.",
                    );
                    ui.checkbox(
                        keep,
                        "Keep 3D above apps without taking focus (Apply to change)",
                    );
                }
            };
            if two_columns {
                ui.columns(2, |cols| {
                    draw(
                        &mut cols[0],
                        &mut auto,
                        &mut recover,
                        &mut minimize,
                        &mut keep,
                        0,
                    );
                    draw(
                        &mut cols[1],
                        &mut auto,
                        &mut recover,
                        &mut minimize,
                        &mut keep,
                        1,
                    );
                });
            } else {
                draw(ui, &mut auto, &mut recover, &mut minimize, &mut keep, 0);
                draw(ui, &mut auto, &mut recover, &mut minimize, &mut keep, 1);
            }
            // Only Apply commits the sliders and the capture, input, source
            // and overlay choices: these three checkboxes save at once, but
            // what they save is the applied state, exactly as the tray hide
            // and Quit do. Keep-on-top is one of the Apply settings and only
            // becomes a pending edit here.
            if auto != core.prefs.auto_convert {
                core.prefs.auto_convert = auto;
                core.save_preferences();
            }
            if recover != core.supervisor.recover {
                core.recover_changed(recover);
            }
            if minimize != core.prefs.minimize_to_tray {
                core.prefs.minimize_to_tray = minimize;
                core.save_preferences();
            }
            self.keep_on_top = keep;
        });
        ui.add_space(6.0);
        ui.label(
            RichText::new(crate::core::shortcuts_line())
                .color(color(p.muted))
                .size(12.0),
        );
        ui.label(
            RichText::new(format!(
                "{} {}",
                any2odyssey_core::APP_NAME,
                any2odyssey_core::VERSION
            ))
            .color(color(p.muted))
            .size(12.0),
        );
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            if gold_button(ui, "View logs", false, p).clicked() {
                core.menu(MenuCommand::ViewLogs);
            }
            if gold_button(ui, super::minimize_label(tray_available), false, p).clicked() {
                core.requests.push(UiRequest::MinimizeToTray);
            }
            if gold_button(ui, "Quit completely", false, p).clicked() {
                core.quit();
            }
        });
        ui.add_space(4.0);
        ui.label(
            RichText::new(crate::core::version_line())
                .color(color(p.muted))
                .size(11.0),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::WindowChoice;
    use any2odyssey_core::log::Logger;

    fn core() -> (tempfile::TempDir, Core) {
        let dir = tempfile::tempdir().unwrap();
        let core = Core::new_preview(Logger::new(dir.path().join("test.log")));
        (dir, core)
    }

    fn windows(handles: &[isize]) -> Vec<WindowChoice> {
        handles
            .iter()
            .map(|&handle| WindowChoice {
                handle,
                title: format!("Window {handle}"),
            })
            .collect()
    }

    /// Every Apply setting edited away from its default.
    fn edit_everything(state: &mut ControlsState) {
        state.mode = CaptureMode::Window;
        state.side_by_side = true;
        state.keep_on_top = false;
        state.depth_pos = 150;
        state.pop_pos = -25;
        state.source = Some(20);
    }

    #[test]
    fn edits_reach_the_shared_state_only_through_commit() {
        let (_dir, mut core) = core();
        core.windows = windows(&[10, 20]);
        let mut state = ControlsState::from_core(&core);
        assert!(!state.dirty(&core));
        let applied = core.prefs.clone();
        edit_everything(&mut state);
        assert!(state.dirty(&core));
        // What a checkbox, the tray hide, a tray command, Turn on 3D, Quit
        // and a recovery relaunch read: all of it is still the applied state.
        state.sync(&core);
        core.save_preferences();
        core.recover_changed(false);
        core.menu(MenuCommand::Restart);
        assert_eq!(
            core.prefs,
            any2odyssey_core::prefs::Preferences {
                recover: false,
                ..applied.clone()
            }
        );
        assert_eq!(core.selected_window, None);
        assert!(state.dirty(&core), "the edits are still pending");
        assert_eq!(state.source, Some(20));

        state.commit(&mut core);
        assert!(!state.dirty(&core));
        assert_eq!(core.prefs.mode, CaptureMode::Window);
        assert!(core.prefs.side_by_side);
        assert!(!core.prefs.keep_overlay_on_top);
        assert_eq!(core.prefs.depth, 1.5);
        assert_eq!(core.prefs.popout, -0.25);
        assert_eq!(core.selected_window, Some(20));
        // Committing is not a change "made elsewhere".
        state.sync(&core);
        assert_eq!(state.mode, CaptureMode::Window);
        assert_eq!(state.source, Some(20));
    }

    #[test]
    fn a_saved_value_between_slider_steps_is_not_a_pending_edit() {
        let (_dir, mut core) = core();
        core.prefs.depth = 0.904;
        let state = ControlsState::from_core(&core);
        assert_eq!(state.depth_pos, 90);
        assert!(!state.dirty(&core));
    }

    #[test]
    fn a_tray_command_replaces_only_the_pending_edit_of_its_own_setting() {
        let (_dir, mut core) = core();
        core.windows = windows(&[10, 20]);
        let mut state = ControlsState::from_core(&core);
        edit_everything(&mut state);
        // The tray's Keep item toggles the applied value (on by default),
        // not the unchecked box on the page.
        let applied_keep = core.prefs.keep_overlay_on_top;
        assert!(applied_keep);
        core.menu(MenuCommand::KeepOverlayOnTop(!applied_keep));
        assert!(!core.prefs.keep_overlay_on_top);
        state.sync(&core);
        core.menu(MenuCommand::KeepOverlayOnTop(true));
        state.sync(&core);
        assert!(state.keep_on_top, "the tray's choice is the newer one");
        assert_eq!(state.mode, CaptureMode::Window, "other edits stay pending");
        assert_eq!(state.depth_pos, 150);

        // "AI conversion mode" while AI is already applied changes nothing
        // that could be noticed: the pending edit and its hint stay.
        core.menu(MenuCommand::AiMode);
        state.sync(&core);
        assert!(state.side_by_side);
        assert!(state.dirty(&core));
        core.menu(MenuCommand::SideBySide);
        state.sync(&core);
        core.menu(MenuCommand::AiMode);
        state.sync(&core);
        assert!(!state.side_by_side);
        assert_eq!(state.mode, CaptureMode::Window);

        state.mode = CaptureMode::Active;
        core.prefs.mode = CaptureMode::Window; // as an Apply elsewhere would
        state.sync(&core);
        assert_eq!(state.mode, CaptureMode::Window);
        assert_eq!(core.prefs.depth, 0.9, "nothing committed the sliders");
    }

    #[test]
    fn a_pending_source_lasts_as_long_as_its_window_is_listed() {
        let (_dir, mut core) = core();
        core.windows = windows(&[10, 20, 30]);
        core.selected_window = Some(10);
        let mut state = ControlsState::from_core(&core);
        assert_eq!(state.source, Some(10));
        state.source = Some(20);
        // Refresh: the applied window is gone and is cleared, as before.
        // That is no reason to drop a different window picked in the box.
        core.windows = windows(&[20, 30]);
        core.selected_window = None;
        state.sync(&core);
        assert_eq!(state.source, Some(20));
        assert!(state.dirty(&core));
        // The picked window goes too: no substitute, a choice is required.
        core.windows = windows(&[30]);
        state.sync(&core);
        assert_eq!(state.source, None);
        assert!(!state.dirty(&core));
        // With no pending pick, the box follows the applied choice.
        core.windows = windows(&[30, 40]);
        core.selected_window = Some(40);
        state.sync(&core);
        assert_eq!(state.source, Some(40));
        core.windows = windows(&[30]);
        core.selected_window = None;
        state.sync(&core);
        assert_eq!(state.source, None);
    }

    #[test]
    fn a_new_launch_requires_a_source_choice() {
        let (_dir, mut core) = core();
        core.prefs.mode = CaptureMode::Window;
        core.windows = windows(&[10, 20]);
        let mut state = ControlsState::from_core(&core);
        state.sync(&core);
        assert_eq!(
            state.source, None,
            "the list is not consent to its first entry"
        );
        assert!(!state.dirty(&core));
    }
}
