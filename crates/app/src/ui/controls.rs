//! The Controls page: conversion state, source & input, depth & pop-out,
//! startup & window behaviour, shortcuts, links and version.

use any2odyssey_core::prefs::CaptureMode;
use any2odyssey_core::slider::{DEPTH, POPOUT};
use any2odyssey_core::theme::Palette;
use egui::{RichText, Ui};

use super::theme::color;
use super::widgets::{banner, gold_button, row, section};
use crate::core::{Core, MenuCommand, UiRequest};

pub struct ControlsState {
    pub depth_pos: i32,
    pub pop_pos: i32,
}

impl ControlsState {
    pub fn from_core(core: &Core) -> ControlsState {
        ControlsState {
            depth_pos: DEPTH.position(core.prefs.depth),
            pop_pos: POPOUT.position(core.prefs.popout),
        }
    }

    fn push_sliders(&self, core: &mut Core) {
        core.prefs.depth = DEPTH.value(self.depth_pos);
        core.prefs.popout = POPOUT.value(self.pop_pos);
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        core: &mut Core,
        p: &Palette,
        wordmark: Option<&egui::TextureHandle>,
        tray_available: bool,
    ) {
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
        });
        ui.add_space(5.0);
        let two_columns = ui.available_width() >= 860.0;
        let source_panel = |ui: &mut Ui, core: &mut Core| {
            section(ui, "SOURCE & INPUT", p, |ui| {
                row(ui, "Capture", |ui| {
                    let mut idx = core.prefs.mode.index();
                    egui::ComboBox::from_id_salt("capture-mode")
                        .width(ui.available_width())
                        .selected_text(CaptureMode::from_index(idx).label())
                        .show_ui(ui, |ui| {
                            for m in CaptureMode::ALL {
                                ui.selectable_value(&mut idx, m.index(), m.label());
                            }
                        });
                    core.prefs.mode = CaptureMode::from_index(idx);
                });
                row(ui, "Input format", |ui| {
                    let mut sbs = core.prefs.side_by_side;
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
                    core.prefs.side_by_side = sbs;
                });
                row(ui, "Source window", |ui| {
                    let enabled = core.prefs.mode != CaptureMode::Desktop;
                    ui.add_enabled_ui(enabled, |ui| {
                        let selected_title = core.selected_window.and_then(|h| core.windows.iter().find(|w| w.handle == h)).map(|w| w.title.clone()).unwrap_or_else(|| "Choose a window".into());
                        let refresh_width = 90.0;
                        egui::ComboBox::from_id_salt("source-window").width((ui.available_width() - refresh_width).max(80.0)).selected_text(selected_title).show_ui(ui, |ui| {
                            let windows = core.windows.clone();
                            for w in windows {
                                ui.selectable_value(&mut core.selected_window, Some(w.handle), &w.title);
                            }
                        });
                        if gold_button(ui, "Refresh", false, p).clicked() {
                            core.refresh_windows();
                        }
                    });
                })
                .on_hover_text("Selected window requires a choice each time this app opens; also seeds Follow active window. Refresh updates the list without choosing a replacement.");
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
                if gold_button(ui, "Apply settings / restart conversion", false, p).clicked() {
                    state.push_sliders(core);
                    core.apply();
                }
            });
        };
        if two_columns {
            ui.columns(2, |cols| {
                source_panel(&mut cols[0], core);
                tuning_panel(&mut cols[1], core, self);
            });
        } else {
            source_panel(ui, core);
            ui.add_space(5.0);
            tuning_panel(ui, core, self);
        }
        ui.add_space(5.0);
        section(ui, "STARTUP & WINDOW BEHAVIOR", p, |ui| {
            let mut auto = core.prefs.auto_convert;
            let mut recover = core.supervisor.recover;
            let mut minimize = core.prefs.minimize_to_tray;
            let mut keep = core.prefs.keep_overlay_on_top;
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
            // Only Apply commits the sliders: a depth or pop-out dragged but
            // not applied is not saved by a checkbox change, exactly as the
            // tray hide and Quit do not save it either.
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
            if keep != core.prefs.keep_overlay_on_top {
                core.prefs.keep_overlay_on_top = keep;
            }
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
