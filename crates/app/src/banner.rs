//! A few words drawn into the picture of the Odyssey 3D output: what a
//! global shortcut did.
//!
//! The desktop's own display for such words (`shortcuts::say`) is on the
//! screen the desktop picks, which on the monitor's own PC was another one
//! than the Odyssey; and on the Odyssey the 3D picture covers the desktop.
//! So while that picture shows, the words are part of it: a dark plate with
//! light letters, the same in both views, which puts it at the depth of the
//! screen itself, where both eyes read it without effort.

use std::sync::OnceLock;

use ab_glyph::{Font, FontVec, PxScale, ScaleFont};

/// The plate behind the letters: black, this much of 255 opaque.
const PLATE_ALPHA: u32 = 190;

/// The typeface of the page (egui's own), if the program has one built in.
fn typeface() -> Option<&'static FontVec> {
    static FACE: OnceLock<Option<FontVec>> = OnceLock::new();
    FACE.get_or_init(|| {
        let fonts = egui::FontDefinitions::default();
        let name = fonts
            .families
            .get(&egui::FontFamily::Proportional)?
            .first()?;
        let data = fonts.font_data.get(name)?;
        FontVec::try_from_vec_and_index(data.font.to_vec(), data.index).ok()
    })
    .as_ref()
}

/// Words on their plate, ready to be put into pictures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plate {
    width: usize,
    height: usize,
    /// RGBA, the top row first; the alpha is how much of the picture
    /// underneath a pixel replaces.
    rgba: Vec<u8>,
}

impl Plate {
    /// `words` in letters fit for a picture of `lines` lines. Nothing
    /// where there is no typeface or nothing to write.
    pub fn of(words: &str, lines: u32) -> Option<Plate> {
        let face = typeface()?;
        let size = (lines as f32 / 22.0).max(16.0);
        let font = face.as_scaled(PxScale::from(size));
        let pad = (size / 2.0).ceil();
        // Where each letter goes, and how far the line runs.
        let mut pen = pad;
        let mut before = None;
        let mut glyphs = Vec::new();
        for letter in words.chars().filter(|c| !c.is_control()) {
            let id = font.glyph_id(letter);
            if let Some(before) = before {
                pen += font.kern(before, id);
            }
            glyphs
                .push(id.with_scale_and_position(size, ab_glyph::point(pen, pad + font.ascent())));
            pen += font.h_advance(id);
            before = Some(id);
        }
        if glyphs.is_empty() {
            return None;
        }
        let width = (pen + pad).ceil() as usize;
        let height = (font.ascent() - font.descent() + 2.0 * pad).ceil() as usize;
        let mut cover = vec![0f32; width * height];
        for glyph in glyphs {
            let Some(outline) = face.outline_glyph(glyph) else {
                continue;
            };
            let bounds = outline.px_bounds();
            outline.draw(|x, y, c| {
                let (x, y) = (
                    bounds.min.x as i64 + x as i64,
                    bounds.min.y as i64 + y as i64,
                );
                if x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height {
                    let at = &mut cover[y as usize * width + x as usize];
                    *at = (*at + c).min(1.0);
                }
            });
        }
        let mut rgba = Vec::with_capacity(cover.len() * 4);
        for c in cover {
            let light = (c * 255.0).round() as u8;
            let alpha = PLATE_ALPHA + ((255 - PLATE_ALPHA) as f32 * c).round() as u32;
            rgba.extend_from_slice(&[light, light, light, alpha.min(255) as u8]);
        }
        Some(Plate {
            width,
            height,
            rgba,
        })
    }

    /// Put the plate into both halves of a side-by-side picture: `canvas`
    /// is RGBA, two views of `view` pixels beside each other, with its
    /// bottom row first if `bottom_up`. In each view it goes to the middle
    /// of the `area` (x, y, width, height from the view's top-left corner)
    /// the picture itself has, a tenth of it above its lower edge, and no
    /// further than the area reaches. A canvas of another size is left as
    /// it is.
    pub fn draw(
        &self,
        canvas: &mut [u8],
        view: (u32, u32),
        area: (u32, u32, u32, u32),
        bottom_up: bool,
    ) {
        let (vw, vh) = (view.0 as usize, view.1 as usize);
        let (ax, ay, aw, ah) = (
            area.0 as usize,
            area.1 as usize,
            area.2 as usize,
            area.3 as usize,
        );
        if canvas.len() != vw * vh * 8 || ax + aw > vw || ay + ah > vh {
            return;
        }
        // What of the plate there is room for, about its middle.
        let (columns, rows) = (self.width.min(aw), self.height.min(ah));
        let (skip_x, skip_y) = ((self.width - columns) / 2, (self.height - rows) / 2);
        let left = ax + (aw - columns) / 2;
        let top = ay + (ah - ah / 10).saturating_sub(rows).min(ah - rows);
        for half in 0..2 {
            for row in 0..rows {
                let y = top + row;
                let line = if bottom_up { vh - 1 - y } else { y };
                let to = (line * vw * 2 + half * vw + left) * 4;
                let from = ((skip_y + row) * self.width + skip_x) * 4;
                let target = &mut canvas[to..to + columns * 4];
                let source = &self.rgba[from..from + columns * 4];
                let (target, source) = (target.as_chunks_mut::<4>().0, source.as_chunks::<4>().0);
                for (under, over) in target.iter_mut().zip(source) {
                    let alpha = over[3] as u32;
                    for channel in 0..3 {
                        let mixed =
                            under[channel] as u32 * (255 - alpha) + over[channel] as u32 * alpha;
                        under[channel] = ((mixed + 127) / 255) as u8;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A colour no pixel of a plate leaves as it is: the plate darkens its
    /// red, and a letter lightens its green.
    const UNDER: [u8; 4] = [255, 0, 255, 255];

    fn white(view: (u32, u32)) -> Vec<u8> {
        UNDER.repeat(view.0 as usize * view.1 as usize * 2)
    }

    /// The pixel at (`x`, `y`) of a half, counted from the top.
    fn pixel(
        canvas: &[u8],
        view: (u32, u32),
        half: u32,
        x: u32,
        y: u32,
        bottom_up: bool,
    ) -> [u8; 4] {
        let line = if bottom_up { view.1 - 1 - y } else { y };
        let at = ((line * view.0 * 2 + half * view.0 + x) * 4) as usize;
        canvas[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn the_letters_grow_with_the_picture_and_nothing_is_no_plate() {
        let small = Plate::of("Depth 55", 720).unwrap();
        let large = Plate::of("Depth 55", 2160).unwrap();
        assert!(large.height > small.height * 2 && large.width > small.width * 2);
        assert!(Plate::of("Depth 100", 720).unwrap().width > small.width);
        assert_eq!(Plate::of("", 720), None);
        assert_eq!(Plate::of("\n", 720), None);
        // Letters are there: some pixels are light, most are the plate.
        let light = small
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0] > 200)
            .count();
        assert!(light > 50 && light < small.rgba.len() / 8, "{light}");
    }

    #[test]
    fn both_views_get_the_same_plate_low_in_the_middle_of_the_picture() {
        let view = (640, 360);
        let plate = Plate::of("Pop-out 10", view.1).unwrap();
        for bottom_up in [false, true] {
            let mut canvas = white(view);
            plate.draw(&mut canvas, view, (0, 0, view.0, view.1), bottom_up);
            let changed: Vec<(u32, u32)> = (0..view.1)
                .flat_map(|y| (0..view.0).map(move |x| (x, y)))
                .filter(|&(x, y)| pixel(&canvas, view, 0, x, y, bottom_up) != UNDER)
                .collect();
            assert_eq!(changed.len(), plate.width * plate.height);
            let (left, right) = (changed[0].0, changed.last().unwrap().0);
            let (top, bottom) = (changed[0].1, changed.last().unwrap().1);
            assert!((left + right).abs_diff(view.0) <= 2, "{left} {right}");
            assert_eq!(bottom + 1, view.1 - view.1 / 10);
            assert!(top > view.1 / 2);
            for &(x, y) in &changed {
                assert_eq!(
                    pixel(&canvas, view, 0, x, y, bottom_up),
                    pixel(&canvas, view, 1, x, y, bottom_up)
                );
                // The picture's own alpha is not the plate's business.
                assert_eq!(pixel(&canvas, view, 0, x, y, bottom_up)[3], 255);
            }
        }
    }

    #[test]
    fn the_plate_stays_inside_the_picture_and_a_wrong_canvas_is_left_alone() {
        let view = (400, 300);
        // A picture narrower than the words, with black beside it.
        let area = (150, 100, 100, 150);
        let plate = Plate::of("3D output could not be started at all", 2160).unwrap();
        assert!(plate.width > 100 && plate.height > 150);
        let mut canvas = white(view);
        plate.draw(&mut canvas, view, area, true);
        for half in 0..2 {
            for y in 0..view.1 {
                for x in 0..view.0 {
                    let inside = (150..250).contains(&x) && (100..250).contains(&y);
                    let changed = pixel(&canvas, view, half, x, y, true) != UNDER;
                    assert_eq!(changed, inside, "{half} {x} {y}");
                }
            }
        }
        let mut short = vec![255u8; 100];
        plate.draw(&mut short, view, area, true);
        assert!(short.iter().all(|&b| b == 255));
        let mut canvas = white(view);
        plate.draw(&mut canvas, view, (350, 0, 100, 100), true);
        assert_eq!(canvas, white(view));
    }
}
