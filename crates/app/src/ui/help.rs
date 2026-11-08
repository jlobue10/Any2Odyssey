//! Written Help page: topic list on the left, article on the right.

use any2odyssey_core::help::{paragraphs, topics};
use any2odyssey_core::theme::Palette;
use egui::{Color32, CornerRadius, RichText, ScrollArea, Ui};

use super::theme::color;
use super::widgets::{display_font, gold_button};

#[derive(Default)]
pub struct HelpState {
    pub selected: usize,
}

impl HelpState {
    pub fn select(&mut self, id: &str) {
        if let Some(i) = topics().iter().position(|t| t.id == id) {
            self.selected = i;
        }
    }

    /// Returns true when the user pressed "Controls".
    pub fn show(&mut self, ui: &mut Ui, p: &Palette) -> bool {
        let mut back = false;
        ui.horizontal(|ui| {
            if gold_button(ui, "‹  Controls", false, p).clicked() {
                back = true;
            }
        });
        ui.add_space(6.0);
        let all = topics();
        let list_width = (ui.available_width() * 0.29).max(160.0);
        ui.horizontal_top(|ui| {
            ui.allocate_ui(egui::vec2(list_width, ui.available_height()), |ui| {
                egui::Frame::NONE
                    .fill(color(p.panel))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(6)
                    .show(ui, |ui| {
                        ui.set_min_height(ui.available_height());
                        ScrollArea::vertical()
                            .id_salt("help-topics")
                            .show(ui, |ui| {
                                for (i, t) in all.iter().enumerate() {
                                    let selected = i == self.selected;
                                    let fill = if selected {
                                        color(p.header)
                                    } else {
                                        Color32::TRANSPARENT
                                    };
                                    let response = egui::Frame::NONE
                                        .fill(fill)
                                        .corner_radius(CornerRadius::same(6))
                                        .inner_margin(egui::Margin::symmetric(8, 8))
                                        .show(ui, |ui| {
                                            ui.set_width(ui.available_width());
                                            ui.label(RichText::new(&t.title).color(color(p.text)));
                                        });
                                    if response.response.interact(egui::Sense::click()).clicked() {
                                        self.selected = i;
                                    }
                                }
                            });
                    });
            });
            ui.add_space(14.0);
            let topic = &all[self.selected.min(all.len() - 1)];
            egui::Frame::NONE
                .fill(color(p.panel))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ScrollArea::vertical()
                        .id_salt("help-article")
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(&topic.title)
                                    .font(display_font(22.0))
                                    .color(color(p.accent)),
                            );
                            ui.add_space(14.0);
                            for paragraph in paragraphs(&topic.body) {
                                let mut lines = paragraph.lines();
                                let first = lines.next().unwrap_or("");
                                if crate::guide::is_caps_heading(first) {
                                    ui.label(RichText::new(first).strong().color(color(p.border)));
                                    let rest: Vec<&str> = lines.collect();
                                    if !rest.is_empty() {
                                        ui.label(RichText::new(rest.join("\n")).size(14.0));
                                    }
                                } else {
                                    ui.label(RichText::new(paragraph).size(14.0));
                                }
                                ui.add_space(10.0);
                            }
                            for link in &topic.links {
                                ui.hyperlink_to(&link.label, &link.url);
                            }
                        });
                });
        });
        back
    }
}
