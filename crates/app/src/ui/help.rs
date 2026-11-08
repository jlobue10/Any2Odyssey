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
        let area = ui.available_rect_before_wrap();
        let list_width = (area.width() * 0.29).max(160.0);
        let left = egui::Rect::from_min_size(area.min, egui::vec2(list_width, area.height()));
        let right = egui::Rect::from_min_max(
            egui::pos2(area.min.x + list_width + 14.0, area.min.y),
            area.max,
        );
        let mut list_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(left)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        egui::Frame::NONE
            .fill(color(p.panel))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(6)
            .show(&mut list_ui, |ui| {
                ui.set_min_size(egui::vec2(left.width() - 12.0, left.height() - 12.0));
                ScrollArea::vertical()
                    .id_salt("help-topics")
                    .auto_shrink([false, false])
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
        let topic = &all[self.selected.min(all.len() - 1)];
        let mut article_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(right)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        egui::Frame::NONE
            .fill(color(p.panel))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(egui::Margin::symmetric(14, 10))
            .show(&mut article_ui, |ui| {
                ui.set_min_size(egui::vec2(right.width() - 28.0, right.height() - 20.0));
                ScrollArea::vertical()
                    .id_salt("help-article")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
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
        ui.advance_cursor_after_rect(area);
        back
    }
}
