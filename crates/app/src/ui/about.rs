//! About page: the running build, the installation record, credits and links.

use any2odyssey_core::about::{
    AboutInfo, Installation, CHANGELOG_URL, CREDITS, ISSUES_URL, RELEASES_URL, REPOSITORY_URL,
};
use any2odyssey_core::theme::Palette;
use any2odyssey_core::update::UpdateState;
use any2odyssey_core::APP_NAME;
use egui::{CornerRadius, RichText, ScrollArea, Ui};

use super::theme::color;
use super::widgets::{display_font, gold_button};
use crate::core::Core;

pub struct AboutState {
    info: AboutInfo,
}

impl Default for AboutState {
    fn default() -> Self {
        Self::new()
    }
}

impl AboutState {
    pub fn new() -> AboutState {
        AboutState {
            // Recorded by build.rs when building from a git checkout.
            info: AboutInfo::gather(option_env!("ANY2ODYSSEY_GIT_SHA")),
        }
    }

    /// Returns true when the user pressed "Controls".
    pub fn show(&mut self, ui: &mut Ui, p: &Palette, core: &mut Core) -> bool {
        let mut back = false;
        ui.horizontal(|ui| {
            if gold_button(ui, "‹  Controls", false, p).clicked() {
                back = true;
            }
        });
        ui.add_space(6.0);
        let area = ui.available_rect_before_wrap();
        let mut panel = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(area)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        let info = &self.info;
        egui::Frame::NONE
            .fill(color(p.panel))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(&mut panel, |ui| {
                ui.set_min_size(egui::vec2(area.width() - 28.0, area.height() - 20.0));
                ScrollArea::vertical()
                    .id_salt("about-article")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new(format!("About {APP_NAME}"))
                                .font(display_font(22.0))
                                .color(color(p.accent)),
                        );
                        ui.add_space(14.0);
                        heading(ui, p, "THIS BUILD");
                        rows(ui, p, "about-build", &info.build_rows());
                        ui.add_space(12.0);
                        heading(ui, p, "UPDATES");
                        body(ui, &core.update.summary());
                        if let UpdateState::Available(release) = &core.update {
                            if let Some(published) = &release.published_at {
                                body(ui, &format!("Published {published}."));
                            }
                            if !release.notes.is_empty() {
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(&release.notes)
                                        .size(13.0)
                                        .color(color(p.muted)),
                                );
                            }
                        }
                        ui.add_space(6.0);
                        let available = core.update.available().cloned();
                        let checking = core.update == UpdateState::Checking;
                        ui.horizontal_wrapped(|ui| {
                            ui.add_enabled_ui(!checking, |ui| {
                                if gold_button(ui, "Check now", false, p).clicked() {
                                    core.check_updates();
                                }
                            });
                            if let Some(release) = &available {
                                if gold_button(ui, "Open release page", true, p).clicked() {
                                    crate::core::open_release_url(&release.page_url);
                                }
                                if let Some(zip) = &release.zip_url {
                                    if gold_button(ui, "Download package", false, p).clicked() {
                                        crate::core::open_release_url(zip);
                                    }
                                }
                            }
                        });
                        let mut automatic = core.prefs.check_updates;
                        if ui
                            .checkbox(&mut automatic, "Check for updates automatically once a day")
                            .changed()
                        {
                            core.prefs.check_updates = automatic;
                            core.save_preferences();
                        }
                        ui.add_space(12.0);
                        heading(ui, p, "INSTALLATION");
                        match info.installation() {
                            Installation::Installed(install) => {
                                rows(ui, p, "about-install", &install)
                            }
                            Installation::NotInstalled => {
                                body(ui, "Not installed by the Any2Odyssey installer; running from the folder shown above. Install.cmd records an installation here.");
                            }
                            Installation::Unreadable(error) => {
                                body(ui, &format!("The installation record could not be read: {error}"));
                            }
                        }
                        ui.add_space(12.0);
                        heading(ui, p, "CREDITS AND LICENSE");
                        body(ui, CREDITS);
                        ui.add_space(12.0);
                        heading(ui, p, "LINKS");
                        for (label, url) in [
                            ("Project page", REPOSITORY_URL),
                            ("Releases", RELEASES_URL),
                            ("Changelog", CHANGELOG_URL),
                            ("Report a problem", ISSUES_URL),
                        ] {
                            ui.hyperlink_to(label, url);
                        }
                    });
            });
        ui.advance_cursor_after_rect(area);
        back
    }
}

fn heading(ui: &mut Ui, p: &Palette, text: &str) {
    ui.label(RichText::new(text).strong().color(color(p.border)));
    ui.add_space(4.0);
}

fn body(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(14.0));
}

fn rows(ui: &mut Ui, p: &Palette, id: &str, rows: &[(String, String)]) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([18.0, 6.0])
        .show(ui, |ui| {
            for (label, value) in rows {
                ui.label(RichText::new(label).color(color(p.muted)).size(14.0));
                ui.label(RichText::new(value).size(14.0));
                ui.end_row();
            }
        });
}
