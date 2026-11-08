//! Vector drawing of the emblem and wordmark with tiny-skia.

use ab_glyph::{Font, FontRef, OutlineCurve, ScaleFont};
use tiny_skia::{
    Color, FillRule, GradientStop, LineJoin, LinearGradient, Mask, Paint, Path, PathBuilder,
    Pixmap, Point, Rect, Shader, SpreadMode, Stroke, Transform,
};

const FONT: &[u8] = include_bytes!("../fonts/LibreBaskerville-Variable.ttf");

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba8(r, g, b, 255)
}
fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::from_rgba8(r, g, b, a)
}

fn gold() -> Color {
    rgb(168, 126, 51)
}
fn light() -> Color {
    rgb(247, 223, 159)
}
fn shade() -> Color {
    rgb(86, 58, 22)
}
fn teal() -> Color {
    rgb(51, 166, 176)
}
fn ink() -> Color {
    rgb(20, 32, 36)
}

/// Drawing surface with a current transform and an optional clip mask.
struct Canvas {
    pixmap: Pixmap,
    transform: Transform,
    clip: Option<Mask>,
}

impl Canvas {
    fn new(width: u32, height: u32) -> Canvas {
        Canvas {
            pixmap: Pixmap::new(width, height).expect("pixmap"),
            transform: Transform::identity(),
            clip: None,
        }
    }
    fn solid(color: Color) -> Paint<'static> {
        let mut p = Paint::default();
        p.set_color(color);
        p.anti_alias = true;
        p
    }
    fn fill(&mut self, path: &Path, paint: &Paint) {
        self.pixmap.fill_path(
            path,
            paint,
            FillRule::Winding,
            self.transform,
            self.clip.as_ref(),
        );
    }
    fn stroke(&mut self, path: &Path, paint: &Paint, width: f32) {
        self.stroke_join(path, paint, width, LineJoin::Miter);
    }
    fn stroke_join(&mut self, path: &Path, paint: &Paint, width: f32, join: LineJoin) {
        let stroke = Stroke {
            width,
            line_join: join,
            ..Stroke::default()
        };
        self.pixmap
            .stroke_path(path, paint, &stroke, self.transform, self.clip.as_ref());
    }
    fn line(&mut self, color: Color, width: f32, x1: f32, y1: f32, x2: f32, y2: f32) {
        let mut pb = PathBuilder::new();
        pb.move_to(x1, y1);
        pb.line_to(x2, y2);
        if let Some(p) = pb.finish() {
            self.stroke(&p, &Self::solid(color), width);
        }
    }
    fn bezier(&mut self, color: Color, width: f32, pts: [f32; 8]) {
        let mut pb = PathBuilder::new();
        pb.move_to(pts[0], pts[1]);
        pb.cubic_to(pts[2], pts[3], pts[4], pts[5], pts[6], pts[7]);
        if let Some(p) = pb.finish() {
            self.stroke(&p, &Self::solid(color), width);
        }
    }
    /// GDI+-style arc: bounding box (x, y, w, h), start angle and sweep in degrees, y-down.
    #[allow(clippy::too_many_arguments)]
    fn arc(
        &mut self,
        color: Color,
        width: f32,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        start: f32,
        sweep: f32,
    ) {
        if let Some(p) = arc_path(x, y, w, h, start, sweep) {
            self.stroke(&p, &Self::solid(color), width);
        }
    }
    fn ellipse_path(x: f32, y: f32, w: f32, h: f32) -> Path {
        PathBuilder::from_oval(Rect::from_xywh(x, y, w, h).expect("rect")).expect("oval")
    }
    fn draw_ellipse(&mut self, color: Color, width: f32, x: f32, y: f32, w: f32, h: f32) {
        let p = Self::ellipse_path(x, y, w, h);
        self.stroke(&p, &Self::solid(color), width);
    }
    fn fill_ellipse(&mut self, paint: &Paint, x: f32, y: f32, w: f32, h: f32) {
        let p = Self::ellipse_path(x, y, w, h);
        self.fill(&p, paint);
    }
    /// Mask for `path` under the current transform, intersected with the current clip.
    fn mask_for(&self, path: &Path) -> Mask {
        let mut mask = Mask::new(self.pixmap.width(), self.pixmap.height()).expect("mask");
        mask.fill_path(path, FillRule::Winding, true, self.transform);
        if let Some(existing) = &self.clip {
            // Intersect: keep only pixels covered by both.
            let data = mask.data_mut();
            for (d, e) in data.iter_mut().zip(existing.data()) {
                *d = ((*d as u32 * *e as u32) / 255) as u8;
            }
        }
        mask
    }
}

/// Approximate an elliptical arc with cubic Béziers (<= 90° per segment).
fn arc_path(x: f32, y: f32, w: f32, h: f32, start_deg: f32, sweep_deg: f32) -> Option<Path> {
    let (cx, cy, rx, ry) = (x + w / 2.0, y + h / 2.0, w / 2.0, h / 2.0);
    let segments = ((sweep_deg.abs() / 90.0).ceil() as usize).max(1);
    let step = sweep_deg.to_radians() / segments as f32;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let mut pb = PathBuilder::new();
    let mut a = start_deg.to_radians();
    pb.move_to(cx + rx * a.cos(), cy + ry * a.sin());
    for _ in 0..segments {
        let b = a + step;
        let (ca, sa, cb, sb) = (a.cos(), a.sin(), b.cos(), b.sin());
        pb.cubic_to(
            cx + rx * (ca - k * sa),
            cy + ry * (sa + k * ca),
            cx + rx * (cb + k * sb),
            cy + ry * (sb - k * cb),
            cx + rx * cb,
            cy + ry * sb,
        );
        a = b;
    }
    pb.finish()
}

fn linear(x0: f32, y0: f32, x1: f32, y1: f32, stops: Vec<GradientStop>) -> Shader<'static> {
    LinearGradient::new(
        Point::from_xy(x0, y0),
        Point::from_xy(x1, y1),
        stops,
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap_or(Shader::SolidColor(gold()))
}

/// The five-stop "metal" gradient across `bounds`, running at roughly 72°.
fn metal(bounds: Rect) -> Paint<'static> {
    let stops = vec![
        GradientStop::new(0.0, light()),
        GradientStop::new(0.28, gold()),
        GradientStop::new(0.49, shade()),
        GradientStop::new(0.68, light()),
        GradientStop::new(1.0, gold()),
    ];
    // Direction vector at 72° from the x axis (y down), spanning the bounds.
    let (cx, cy) = (
        bounds.x() + bounds.width() / 2.0,
        bounds.y() + bounds.height() / 2.0,
    );
    let len = (bounds.width().powi(2) + bounds.height().powi(2)).sqrt() / 2.0;
    let (dx, dy) = (
        72f32.to_radians().cos() * len,
        72f32.to_radians().sin() * len,
    );
    Paint {
        shader: linear(cx - dx, cy - dy, cx + dx, cy + dy, stops),
        anti_alias: true,
        ..Paint::default()
    }
}

fn vertical(bounds: Rect, top: Color, bottom: Color) -> Paint<'static> {
    Paint {
        shader: linear(
            bounds.x(),
            bounds.y(),
            bounds.x(),
            bounds.bottom(),
            vec![GradientStop::new(0.0, top), GradientStop::new(1.0, bottom)],
        ),
        anti_alias: true,
        ..Paint::default()
    }
}

fn path_bounds(path: &Path) -> Rect {
    path.bounds()
}

fn translate_path(path: &Path, dx: f32, dy: f32) -> Path {
    path.clone()
        .transform(Transform::from_translate(dx, dy))
        .expect("translate")
}

/// Outline `text` with the bundled font and fit it (uniform scale, centred)
/// into `bounds`, like Any2Odd's `Letters`.
fn letters(text: &str, bounds: Rect) -> Option<Path> {
    let font = FontRef::try_from_slice(FONT).expect("bundled font");
    let scaled = font.as_scaled(1000.0);
    let mut pb = PathBuilder::new();
    let mut pen = 0.0f32;
    let mut previous: Option<ab_glyph::GlyphId> = None;
    for ch in text.chars() {
        let id = font.glyph_id(ch);
        if let Some(prev) = previous {
            pen += scaled.kern(prev, id);
        }
        if let Some(outline) = font.outline(id) {
            let scale = 1000.0 / font.units_per_em().unwrap_or(1000.0);
            for curve in outline.curves {
                match curve {
                    OutlineCurve::Line(a, b) => {
                        pb.move_to(pen + a.x * scale, -a.y * scale);
                        pb.line_to(pen + b.x * scale, -b.y * scale);
                    }
                    OutlineCurve::Quad(a, c, b) => {
                        pb.move_to(pen + a.x * scale, -a.y * scale);
                        pb.quad_to(
                            pen + c.x * scale,
                            -c.y * scale,
                            pen + b.x * scale,
                            -b.y * scale,
                        );
                    }
                    OutlineCurve::Cubic(a, c1, c2, b) => {
                        pb.move_to(pen + a.x * scale, -a.y * scale);
                        pb.cubic_to(
                            pen + c1.x * scale,
                            -c1.y * scale,
                            pen + c2.x * scale,
                            -c2.y * scale,
                            pen + b.x * scale,
                            -b.y * scale,
                        );
                    }
                }
            }
        }
        pen += scaled.h_advance(id);
        previous = Some(id);
    }
    let raw = pb.finish()?;
    let raw = glyph_outline_to_contours(&raw)?;
    let b = raw.bounds();
    let scale = (bounds.width() / b.width()).min(bounds.height() / b.height());
    let fit = Transform::from_translate(-b.x(), -b.y())
        .post_scale(scale, scale)
        .post_translate(
            bounds.x() + (bounds.width() - b.width() * scale) / 2.0,
            bounds.y() + (bounds.height() - b.height() * scale) / 2.0,
        );
    raw.transform(fit)
}

/// ab_glyph emits disconnected curve segments; rebuild closed contours by
/// chaining segments whose end meets the next start, so fills and strokes work.
fn glyph_outline_to_contours(path: &Path) -> Option<Path> {
    use tiny_skia::PathSegment;
    let mut pb = PathBuilder::new();
    let mut last: Option<Point> = None;
    let mut start: Option<Point> = None;
    let close_to = |a: Point, b: Point| (a.x - b.x).abs() < 0.01 && (a.y - b.y).abs() < 0.01;
    for seg in path.segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                match last {
                    Some(l) if close_to(l, p) => {}
                    _ => {
                        if start.is_some() {
                            pb.close();
                        }
                        pb.move_to(p.x, p.y);
                        start = Some(p);
                    }
                }
                last = Some(p);
            }
            PathSegment::LineTo(p) => {
                pb.line_to(p.x, p.y);
                last = Some(p);
            }
            PathSegment::QuadTo(c, p) => {
                pb.quad_to(c.x, c.y, p.x, p.y);
                last = Some(p);
            }
            PathSegment::CubicTo(c1, c2, p) => {
                pb.cubic_to(c1.x, c1.y, c2.x, c2.y, p.x, p.y);
                last = Some(p);
            }
            PathSegment::Close => {
                pb.close();
                start = None;
            }
        }
    }
    if start.is_some() {
        pb.close();
    }
    pb.finish()
}

/// Engraved lettering: dark halo, metal edge, graphite face, light inner line
/// and an offset echo clipped inside the glyphs.
fn engrave(c: &mut Canvas, path: &Path, edge: f32, turquoise: bool) {
    let mut b = path_bounds(path);
    b = Rect::from_xywh(
        b.x() - edge * 2.0,
        b.y() - edge * 2.0,
        b.width() + edge * 4.0,
        b.height() + edge * 4.0,
    )
    .unwrap_or(b);
    c.stroke_join(path, &Canvas::solid(ink()), edge * 2.8, LineJoin::Round);
    c.stroke_join(path, &metal(b), edge * 2.0, LineJoin::Round);
    let face_top = if turquoise {
        rgb(39, 119, 129)
    } else {
        rgb(53, 69, 69)
    };
    c.fill(path, &vertical(b, face_top, ink()));
    c.stroke_join(path, &Canvas::solid(light()), edge * 0.45, LineJoin::Round);
    let saved = c.clip.take();
    c.clip = saved.clone();
    let mask = c.mask_for(path);
    c.clip = Some(mask);
    let echo = translate_path(path, edge * 1.1, edge * 1.1);
    c.stroke_join(
        &echo,
        &Canvas::solid(rgba(247, 223, 159, 150)),
        edge * 0.55,
        LineJoin::Round,
    );
    c.clip = saved;
}

fn gem(c: &mut Canvas, x: f32, y: f32, radius: f32) {
    let mut pb = PathBuilder::new();
    pb.move_to(x, y - radius);
    pb.line_to(x + radius * 0.65, y);
    pb.line_to(x, y + radius);
    pb.line_to(x - radius * 0.65, y);
    pb.close();
    let Some(p) = pb.finish() else { return };
    let bounds =
        Rect::from_xywh(x - radius, y - radius, radius * 2.0, radius * 2.0).expect("gem rect");
    let len = radius * 1.4;
    let (dx, dy) = (
        55f32.to_radians().cos() * len,
        55f32.to_radians().sin() * len,
    );
    let fill = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    let mut fill = fill;
    fill.shader = linear(
        x - dx,
        y - dy,
        x + dx,
        y + dy,
        vec![
            GradientStop::new(0.0, light()),
            GradientStop::new(1.0, teal()),
        ],
    );
    let _ = bounds;
    c.fill(&p, &fill);
    c.stroke(&p, &Canvas::solid(shade()), 3.0);
    c.stroke(&p, &Canvas::solid(light()), 1.0);
}

fn scroll(c: &mut Canvas, x: f32, y: f32, length: f32, direction: f32) {
    let saved = c.transform;
    c.transform = saved.pre_translate(x, y).pre_scale(direction, 1.0);
    let l = length;
    c.bezier(
        gold(),
        1.7,
        [0.0, 0.0, l * 0.25, -20.0, l * 0.62, 21.0, l, 0.0],
    );
    c.bezier(
        light(),
        0.65,
        [0.0, 2.0, l * 0.26, -17.0, l * 0.62, 24.0, l, 2.0],
    );
    c.bezier(
        gold(),
        1.7,
        [
            l * 0.4,
            -2.0,
            l * 0.2,
            -28.0,
            l * 0.18,
            -28.0,
            l * 0.2,
            -8.0,
        ],
    );
    c.arc(gold(), 1.7, l * 0.48, -9.0, 16.0, 16.0, 15.0, 290.0);
    c.bezier(
        gold(),
        1.7,
        [l * 0.75, 2.0, l * 0.96, 26.0, l * 1.08, 22.0, l, 2.0],
    );
    c.transform = saved;
}

/// The emblem in a 256-unit coordinate space, placed at (x, y) with `size`.
fn emblem(c: &mut Canvas, x: f32, y: f32, size: f32, small: bool) {
    let saved_t = c.transform;
    let saved_clip = c.clip.take();
    c.transform = saved_t
        .pre_translate(x, y)
        .pre_scale(size / 256.0, size / 256.0);
    let silhouette = Canvas::ellipse_path(7.0, 7.0, 242.0, 242.0);
    c.clip = saved_clip.clone();
    let mask = c.mask_for(&silhouette);
    c.clip = Some(mask);

    let disc = Rect::from_xywh(7.0, 7.0, 242.0, 242.0).expect("disc");
    let len = 242.0 * 0.7;
    let (dx, dy) = (
        120f32.to_radians().cos() * len,
        120f32.to_radians().sin() * len,
    );
    let disc_paint = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    let mut disc_paint = disc_paint;
    disc_paint.shader = linear(
        128.0 - dx,
        128.0 - dy,
        128.0 + dx,
        128.0 + dy,
        vec![
            GradientStop::new(0.0, rgb(47, 63, 64)),
            GradientStop::new(1.0, ink()),
        ],
    );
    c.fill_ellipse(&disc_paint, disc.x(), disc.y(), disc.width(), disc.height());

    let rim = metal(Rect::from_xywh(8.0, 8.0, 240.0, 240.0).expect("rim"));
    let rim_path = Canvas::ellipse_path(12.0, 12.0, 232.0, 232.0);
    c.stroke(&rim_path, &rim, if small { 9.0 } else { 6.0 });
    c.draw_ellipse(light(), 1.3, 8.0, 8.0, 240.0, 240.0);
    c.draw_ellipse(ink(), 3.0, 19.0, 19.0, 218.0, 218.0);
    c.draw_ellipse(light(), 1.3, 23.0, 23.0, 210.0, 210.0);
    c.draw_ellipse(teal(), 1.4, 31.0, 31.0, 194.0, 194.0);

    if small {
        if let Some(text) = letters(
            "+1",
            Rect::from_xywh(40.0, 67.0, 176.0, 120.0).expect("rect"),
        ) {
            engrave(c, &text, 5.0, true);
        }
    } else {
        for i in 0..64 {
            let angle = i as f32 * std::f32::consts::PI / 32.0;
            let r = if i % 4 == 0 { 101.0 } else { 105.0 };
            c.line(
                gold(),
                1.25,
                128.0 + angle.cos() * r,
                128.0 + angle.sin() * r,
                128.0 + angle.cos() * 109.0,
                128.0 + angle.sin() * 109.0,
            );
        }
        if let Some(text) = letters(
            "2n",
            Rect::from_xywh(59.0, 50.0, 138.0, 85.0).expect("rect"),
        ) {
            engrave(c, &text, 2.8, false);
        }
        if let Some(text) = letters(
            "+1",
            Rect::from_xywh(76.0, 146.0, 104.0, 60.0).expect("rect"),
        ) {
            engrave(c, &text, 2.4, true);
        }
        c.line(gold(), 1.4, 66.0, 140.0, 116.0, 140.0);
        c.line(gold(), 1.4, 140.0, 140.0, 190.0, 140.0);
        gem(c, 128.0, 140.0, 5.0);
        for side in [-1.0f32, 1.0] {
            let t = c.transform;
            c.transform = t.pre_translate(128.0, 128.0).pre_scale(side, 1.0);
            c.bezier(
                gold(),
                1.5,
                [72.0, -55.0, 100.0, -20.0, 85.0, 4.0, 74.0, 12.0],
            );
            c.bezier(
                light(),
                0.65,
                [75.0, -53.0, 104.0, -20.0, 88.0, 4.0, 77.0, 14.0],
            );
            c.arc(gold(), 1.5, 70.0, -9.0, 17.0, 20.0, 15.0, 290.0);
            c.bezier(
                gold(),
                1.5,
                [73.0, 28.0, 72.0, 53.0, 52.0, 70.0, 35.0, 73.0],
            );
            c.bezier(
                light(),
                0.65,
                [77.0, 26.0, 76.0, 56.0, 54.0, 75.0, 35.0, 77.0],
            );
            c.transform = t;
        }
        gem(c, 128.0, 27.0, 8.0);
        gem(c, 128.0, 229.0, 8.0);
        gem(c, 27.0, 128.0, 6.0);
        gem(c, 229.0, 128.0, 6.0);
    }
    c.transform = saved_t;
    c.clip = saved_clip;
}

/// Convert a premultiplied pixmap to straight-alpha RGBA bytes.
fn to_rgba(pixmap: &Pixmap) -> Vec<u8> {
    pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let d = p.demultiply();
            [d.red(), d.green(), d.blue(), d.alpha()]
        })
        .collect()
}

/// Render the icon at `size` px: draw at 4x, box-downsample, then clear any
/// ringing outside the circular silhouette.
pub fn icon_rgba(size: u32) -> Vec<u8> {
    let big = size * 4;
    let mut c = Canvas::new(big, big);
    emblem(&mut c, 0.0, 0.0, big as f32, size <= 24);
    let src = c.pixmap.data();
    let stride = big as usize * 4;
    let mut out = vec![0u8; (size * size * 4) as usize];
    for y in 0..size as usize {
        for x in 0..size as usize {
            let mut acc = [0u32; 4];
            for dy in 0..4 {
                for dx in 0..4 {
                    let i = (y * 4 + dy) * stride + (x * 4 + dx) * 4;
                    for k in 0..4 {
                        acc[k] += src[i + k] as u32;
                    }
                }
            }
            let a = acc[3] / 16;
            let o = (y * size as usize + x) * 4;
            let cx = x as f64 + 0.5 - size as f64 / 2.0;
            let cy = y as f64 + 0.5 - size as f64 / 2.0;
            if a == 0 || cx * cx + cy * cy > (size as f64 * size as f64) / 4.0 {
                continue;
            }
            // Premultiplied average -> straight alpha.
            for k in 0..3 {
                out[o + k] = ((acc[k] / 16) * 255 / a).min(255) as u8;
            }
            out[o + 3] = a as u8;
        }
    }
    out
}

pub const WORDMARK_WIDTH: u32 = 1400;
pub const WORDMARK_HEIGHT: u32 = 250;

/// The 1400 x 250 wordmark: emblem, rule, "ANY2ODYSSEY" and the `2n + 1` seal line.
pub fn wordmark_rgba() -> (u32, u32, Vec<u8>) {
    let mut c = Canvas::new(WORDMARK_WIDTH, WORDMARK_HEIGHT);
    emblem(&mut c, 6.0, 14.0, 220.0, false);
    c.line(gold(), 1.8, 260.0, 118.0, 1362.0, 118.0);
    c.line(light(), 0.7, 260.0, 121.0, 1362.0, 121.0);
    if let Some(text) = letters(
        "ANY2ODYSSEY",
        Rect::from_xywh(269.0, 35.0, 1072.0, 145.0).expect("rect"),
    ) {
        engrave(&mut c, &text, 3.2, false);
    }
    gem(&mut c, 252.0, 111.0, 9.0);
    gem(&mut c, 1367.0, 111.0, 9.0);
    scroll(&mut c, 695.0, 202.0, 135.0, -1.0);
    scroll(&mut c, 970.0, 202.0, 135.0, 1.0);
    if let Some(text) = letters(
        "2n  +  1",
        Rect::from_xywh(736.0, 184.0, 193.0, 40.0).expect("rect"),
    ) {
        engrave(&mut c, &text, 1.1, true);
    }
    c.line(gold(), 1.1, 274.0, 202.0, 526.0, 202.0);
    c.line(gold(), 1.1, 1140.0, 202.0, 1342.0, 202.0);
    c.arc(gold(), 1.1, 267.0, 196.0, 12.0, 12.0, 20.0, 285.0);
    c.arc(gold(), 1.1, 1336.0, 196.0, 12.0, 12.0, 180.0, 285.0);
    (WORDMARK_WIDTH, WORDMARK_HEIGHT, to_rgba(&c.pixmap))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_produce_a_bounded_path() {
        let p = letters("2n", Rect::from_xywh(59.0, 50.0, 138.0, 85.0).unwrap()).unwrap();
        let b = p.bounds();
        assert!(
            b.x() >= 58.0 && b.right() <= 198.0 && b.y() >= 49.0 && b.bottom() <= 136.0,
            "{b:?}"
        );
        assert!(
            b.width() > 100.0 || b.height() > 60.0,
            "fitted to at least one axis: {b:?}"
        );
    }

    #[test]
    fn icon_is_opaque_inside_and_transparent_outside() {
        let rgba = icon_rgba(32);
        let px = |x: usize, y: usize| &rgba[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
        assert_eq!(px(0, 0)[3], 0);
        assert_eq!(px(31, 31)[3], 0);
        assert!(px(16, 16)[3] > 200, "centre alpha {}", px(16, 16)[3]);
        assert!(px(16, 2)[3] > 200, "rim alpha {}", px(16, 2)[3]);
    }

    #[test]
    fn arc_covers_requested_sweep() {
        let p = arc_path(0.0, 0.0, 100.0, 100.0, 0.0, 180.0).unwrap();
        let b = p.bounds();
        assert!(
            (b.x() - 0.0).abs() < 1.5 && (b.right() - 100.0).abs() < 1.5,
            "{b:?}"
        );
        assert!(b.y() >= 49.0 && (b.bottom() - 100.0).abs() < 1.5, "{b:?}");
    }
}
