//! egui visuals derived from the core palette, plus the bundled display font.

use any2odyssey_core::theme::{Palette, Rgb};
use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke, Visuals};

pub const DISPLAY_FONT: &str = "display";
pub const UI_FONT: &str = "ui";

pub fn color(c: Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}

pub fn display_family() -> FontFamily {
    FontFamily::Name(DISPLAY_FONT.into())
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        DISPLAY_FONT.to_owned(),
        std::sync::Arc::new(FontData::from_static(include_bytes!(
            "../../../../tools/iconmaker/fonts/LibreBaskerville-Variable.ttf"
        ))),
    );
    fonts.families.insert(
        display_family(),
        vec![DISPLAY_FONT.to_owned(), "Ubuntu-Light".to_owned()],
    );
    // Fira Sans (OFL) is the UI text face; it also supplies the arrows and
    // triangles that egui's default font lacks.
    fonts.font_data.insert(
        UI_FONT.to_owned(),
        std::sync::Arc::new(FontData::from_static(include_bytes!(
            "../../../../assets/fonts/FiraSans-Regular.ttf"
        ))),
    );
    if let Some(list) = fonts.families.get_mut(&FontFamily::Proportional) {
        list.insert(0, UI_FONT.to_owned());
    }
    if let Some(list) = fonts.families.get_mut(&FontFamily::Monospace) {
        list.push(UI_FONT.to_owned());
    }
    ctx.set_fonts(fonts);
}

pub fn apply(ctx: &egui::Context, p: &Palette) {
    let mut v = if p.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.override_text_color = Some(color(p.text));
    v.panel_fill = Color32::TRANSPARENT;
    v.window_fill = color(p.panel);
    v.extreme_bg_color = color(p.panel);
    v.faint_bg_color = color(p.header);
    v.window_stroke = Stroke::new(1.0_f32, color(p.gold));
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(12);
    v.selection.bg_fill = color(p.accent);
    v.selection.stroke = Stroke::new(1.0_f32, color(p.text));
    v.hyperlink_color = color(p.accent);
    v.widgets.noninteractive.bg_fill = color(p.panel);
    v.widgets.noninteractive.weak_bg_fill = color(p.panel);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, color(p.border));
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, color(p.text));
    v.widgets.inactive.bg_fill = color(p.button);
    v.widgets.inactive.weak_bg_fill = color(p.button);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, color(p.gold));
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, color(p.text));
    v.widgets.hovered.bg_fill = color(p.hover);
    v.widgets.hovered.weak_bg_fill = color(p.hover);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, color(p.accent));
    v.widgets.hovered.fg_stroke = Stroke::new(1.5_f32, color(p.text));
    v.widgets.active.bg_fill = color(p.header);
    v.widgets.active.weak_bg_fill = color(p.header);
    v.widgets.active.bg_stroke = Stroke::new(1.0_f32, color(p.accent));
    v.widgets.active.fg_stroke = Stroke::new(1.5_f32, color(p.text));
    v.widgets.open.bg_fill = color(p.panel);
    v.widgets.open.weak_bg_fill = color(p.panel);
    v.widgets.open.bg_stroke = Stroke::new(1.0_f32, color(p.gold));
    for w in [
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
        &mut v.widgets.noninteractive,
    ] {
        w.corner_radius = CornerRadius::same(8);
    }
    ctx.set_visuals(v);
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(13.0, 7.0);
    style.spacing.slider_width = 180.0;
    style.spacing.combo_width = 220.0;
    style.spacing.interact_size.y = 26.0;
    ctx.set_style(style);
}
