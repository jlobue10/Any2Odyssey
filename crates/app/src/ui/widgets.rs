//! Ornamental widgets: gold buttons, section cards with filigree borders,
//! flourishes and the window frame. Drawing follows Any2Odd's DesktopTheme.

use any2odyssey_core::theme::{Palette, PRIMARY_BUTTON, PRIMARY_BUTTON_HOVER, PRIMARY_BUTTON_TEXT};
use egui::epaint::CubicBezierShape;
use egui::{
    pos2, vec2, Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Response, Sense, Shape,
    Stroke, StrokeKind, Ui, Vec2,
};

use super::theme::{color, display_family};

pub fn display_font(size: f32) -> FontId {
    FontId::new(size, display_family())
}

fn bezier(painter: &Painter, points: [Pos2; 4], stroke: Stroke) {
    painter.add(Shape::CubicBezier(CubicBezierShape::from_points_stroke(
        points,
        false,
        Color32::TRANSPARENT,
        stroke,
    )));
}

/// Arc of an ellipse inscribed in `rect`, from `start` sweeping `sweep` degrees (GDI+ convention).
fn arc(painter: &Painter, rect: Rect, start_deg: f32, sweep_deg: f32, stroke: Stroke) {
    let steps = ((sweep_deg.abs() / 12.0).ceil() as usize).max(4);
    let c = rect.center();
    let (rx, ry) = (rect.width() / 2.0, rect.height() / 2.0);
    let points: Vec<Pos2> = (0..=steps)
        .map(|i| {
            let a = (start_deg + sweep_deg * i as f32 / steps as f32).to_radians();
            pos2(c.x + rx * a.cos(), c.y + ry * a.sin())
        })
        .collect();
    painter.add(Shape::line(points, stroke));
}

/// Twin scrolls with a turquoise gem at the centre.
pub fn flourish(painter: &Painter, cx: f32, y: f32, span: f32, p: &Palette) {
    let gold = Stroke::new(1.0_f32, color(p.gold));
    let shine = Stroke::new(1.0_f32, color(p.shine));
    for side in [-1.0f32, 1.0] {
        bezier(
            painter,
            [
                pos2(cx + side * 7.0, y),
                pos2(cx + side * 18.0, y + 9.0),
                pos2(cx + side * 32.0, y - 9.0),
                pos2(cx + side * span, y),
            ],
            gold,
        );
        bezier(
            painter,
            [
                pos2(cx + side * 14.0, y + 2.0),
                pos2(cx + side * 26.0, y + 12.0),
                pos2(cx + side * 39.0, y - 4.0),
                pos2(cx + side * (span + 10.0), y + 2.0),
            ],
            shine,
        );
        arc(
            painter,
            Rect::from_min_size(pos2(cx + side * 20.0 - 5.0, y - 5.0), vec2(10.0, 10.0)),
            if side < 0.0 { 20.0 } else { 160.0 },
            270.0,
            gold,
        );
    }
    painter.circle_stroke(pos2(cx, y), 4.0, gold);
    painter.circle_filled(pos2(cx, y), 2.0, color(p.accent));
}

/// Dark shadow, gold line and inner shine around `r`, with optional flourishes.
pub fn border_lines(painter: &Painter, r: Rect, with_flourish: bool, p: &Palette) {
    if r.width() < 20.0 || r.height() < 20.0 {
        return;
    }
    let outer = Rect::from_min_size(r.min + vec2(2.0, 2.0), r.size() - vec2(5.0, 5.0));
    let inner = Rect::from_min_size(r.min + vec2(5.0, 5.0), r.size() - vec2(11.0, 11.0));
    let shadow = p.frame_shadow();
    painter.rect_stroke(
        outer,
        CornerRadius::same(12),
        Stroke::new(3.2_f32, color(shadow)),
        StrokeKind::Middle,
    );
    painter.rect_stroke(
        outer,
        CornerRadius::same(12),
        Stroke::new(1.5_f32, color(p.gold)),
        StrokeKind::Middle,
    );
    painter.rect_stroke(
        inner,
        CornerRadius::same(9),
        Stroke::new(0.8_f32, color(p.shine)),
        StrokeKind::Middle,
    );
    if with_flourish && r.width() > 210.0 {
        flourish(painter, r.center().x, r.top() + 5.0, 48.0, p);
        flourish(painter, r.center().x, r.bottom() - 6.0, 30.0, p);
        let gold = Stroke::new(1.0_f32, color(p.gold));
        for side in [-1.0f32, 1.0] {
            let x = if side < 0.0 {
                r.left() + 12.0
            } else {
                r.right() - 12.0
            };
            let y = r.top() + 15.0;
            bezier(
                painter,
                [
                    pos2(x, y + 22.0),
                    pos2(x + side * 6.0, y + 6.0),
                    pos2(x - side * 22.0, y + 10.0),
                    pos2(x - side * 30.0, y + 1.0),
                ],
                gold,
            );
            arc(
                painter,
                Rect::from_min_size(pos2(x - 6.0, y + 12.0), vec2(12.0, 12.0)),
                if side < 0.0 { 0.0 } else { 180.0 },
                250.0,
                gold,
            );
        }
    }
}

/// The outer window frame: black shadow, gold border, light highlight.
pub fn window_frame(painter: &Painter, r: Rect, p: &Palette, radius: u8) {
    if r.width() < 24.0 || r.height() < 24.0 {
        return;
    }
    let outside = Rect::from_min_size(r.min + vec2(3.0, 3.0), r.size() - vec2(7.0, 7.0));
    let inside = Rect::from_min_size(r.min + vec2(8.0, 8.0), r.size() - vec2(17.0, 17.0));
    let highlight = Rect::from_min_size(r.min + vec2(10.0, 10.0), r.size() - vec2(21.0, 21.0));
    let rad = |d: u8| CornerRadius::same(radius.saturating_sub(d).max(1));
    painter.rect_stroke(
        outside,
        rad(3),
        Stroke::new(7.0_f32, Color32::from_rgb(25, 27, 27)),
        StrokeKind::Middle,
    );
    painter.rect_stroke(
        inside,
        rad(7),
        Stroke::new(2.0_f32, color(p.border)),
        StrokeKind::Middle,
    );
    painter.rect_stroke(
        highlight,
        rad(9),
        Stroke::new(0.8_f32, color(p.shine)),
        StrokeKind::Middle,
    );
}

/// A gold-edged button. `primary` uses the teal "Turn on 3D" fill.
pub fn gold_button(ui: &mut Ui, text: &str, primary: bool, p: &Palette) -> Response {
    gold_button_sized(ui, text, primary, p, None, display_font(13.0))
}

pub fn gold_button_sized(
    ui: &mut Ui,
    text: &str,
    primary: bool,
    p: &Palette,
    fixed: Option<Vec2>,
    font: FontId,
) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER);
    let padding = vec2(13.0, 7.0);
    let size = fixed.unwrap_or(galley.size() + padding * 2.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let pressed = response.is_pointer_button_down_on();
        let mut fill = if primary {
            if hovered {
                PRIMARY_BUTTON_HOVER
            } else {
                PRIMARY_BUTTON
            }
        } else if hovered {
            p.hover
        } else {
            p.button
        };
        if pressed {
            fill = fill.blend(p.border, 0.15);
        }
        let top = fill.blend(
            any2odyssey_core::theme::Rgb(255, 255, 255),
            if p.dark { 0.03 } else { 0.18 },
        );
        let inner = rect.shrink(1.0);
        let painter = ui.painter();
        painter.rect_filled(inner, CornerRadius::same(9), color(fill));
        // Light upper band approximates the vertical gradient.
        let band = Rect::from_min_max(inner.min, pos2(inner.max.x, inner.center().y));
        painter.rect_filled(
            band,
            CornerRadius {
                nw: 9,
                ne: 9,
                sw: 0,
                se: 0,
            },
            color(top).gamma_multiply(0.55),
        );
        painter.rect_stroke(
            inner,
            CornerRadius::same(9),
            Stroke::new(1.0_f32, color(p.gold)),
            StrokeKind::Middle,
        );
        painter.rect_stroke(
            inner.shrink(2.0),
            CornerRadius::same(7),
            Stroke::new(1.0_f32, color(p.shine).gamma_multiply(0.4)),
            StrokeKind::Middle,
        );
        let text_color = if !ui.is_enabled() {
            color(p.muted)
        } else if primary {
            color(PRIMARY_BUTTON_TEXT)
        } else {
            color(p.text)
        };
        let offset = if pressed { vec2(0.0, 1.0) } else { Vec2::ZERO };
        painter.galley(
            rect.center() - galley.size() / 2.0 + offset,
            galley,
            text_color,
        );
        if response.has_focus() {
            painter.rect_stroke(
                rect.shrink(4.0),
                CornerRadius::same(6),
                Stroke::new(1.0_f32, color(p.accent)),
                StrokeKind::Middle,
            );
        }
    }
    response
}

/// A card with the panel fill, filigree border and a display-font title.
pub fn section<R>(
    ui: &mut Ui,
    title: &str,
    p: &Palette,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    let background = ui.painter().add(Shape::Noop);
    let frame = egui::Frame::NONE.inner_margin(egui::Margin::symmetric(16, 16));
    let out = frame.show(ui, |ui| {
        ui.label(
            egui::RichText::new(title)
                .font(display_font(13.0))
                .color(color(p.text))
                .strong(),
        );
        ui.add_space(6.0);
        add_contents(ui)
    });
    let rect = out.response.rect;
    let painter = ui.painter();
    painter.set(
        background,
        Shape::rect_filled(rect.shrink(1.0), CornerRadius::same(16), color(p.panel)),
    );
    border_lines(painter, rect, true, p);
    out.inner
}

/// Heading used above slider rows and combo rows (26 % label column).
pub fn row(ui: &mut Ui, label: &str, add_contents: impl FnOnce(&mut Ui)) -> Response {
    ui.horizontal(|ui| {
        let label_width = (ui.available_width() * 0.26).max(90.0);
        ui.allocate_ui_with_layout(
            vec2(label_width, 26.0),
            egui::Layout::left_to_right(Align2::LEFT_CENTER.y()),
            |ui| {
                ui.label(label);
            },
        );
        ui.allocate_ui_with_layout(
            vec2(ui.available_width(), 26.0),
            egui::Layout::left_to_right(Align2::LEFT_CENTER.y()),
            add_contents,
        );
    })
    .response
}

/// The branded banner: wordmark on the left, tagline on the right.
pub fn banner(ui: &mut Ui, wordmark: Option<&egui::TextureHandle>, p: &Palette) {
    let height = 86.0;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let painter = ui.painter();
    let mut used = 0.0;
    if let Some(tex) = wordmark {
        let size = tex.size_vec2();
        let scale = ((height - 4.0) / size.y).min(rect.width() / size.x);
        let draw = size * scale;
        let image_rect =
            Rect::from_min_size(pos2(rect.left(), rect.center().y - draw.y / 2.0), draw);
        painter.image(
            tex.id(),
            image_rect,
            Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        used = draw.x;
    }
    if rect.width() - used > 235.0 {
        painter.text(
            pos2(rect.right() - 4.0, rect.center().y - 8.0),
            Align2::RIGHT_CENTER,
            "CONTINUOUS DESKTOP 3D",
            FontId::proportional(12.0),
            color(p.muted),
        );
        painter.text(
            pos2(rect.right() - 4.0, rect.center().y + 8.0),
            Align2::RIGHT_CENTER,
            "Escape stays with your game.",
            FontId::proportional(12.0),
            color(p.muted),
        );
    }
}
