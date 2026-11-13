//! Interleaving a left/right pair for a slanted lenticular panel.
//!
//! A lenticular sheet shows each eye a different set of subpixels: which eye
//! a subpixel belongs to depends only on where it sits under its lens. This
//! module assigns every red, green and blue subpixel of the panel to the left
//! or the right image from three numbers that describe the sheet as the
//! viewer sees it: the lens period along a row, how far the lenses move
//! sideways from one row to the next, and a phase.
//!
//! The model is the textbook one for two views; it is not Samsung's weaver.
//! There is no eye tracking: the picture is right for one head position, and
//! the phase moves it. The numbers come from the user's own calibration
//! (the pattern in [`calibration_pattern`] makes errors visible), and nothing
//! here switches the panel's lens on.

use image::{imageops, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The lens sheet as seen from the viewer's seat, in panel pixels.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Lens {
    /// Lens period along a pixel row, in pixels.
    pub pitch: f64,
    /// Pixels the lenses move to the right from one row to the next row
    /// down. Negative for lenses that lean the other way.
    pub slant: f64,
    /// Where the left view starts under a lens, as a fraction of the period.
    /// Half a period exchanges the eyes.
    pub offset: f64,
    /// Subpixels run blue, green, red from left to right instead of red,
    /// green, blue.
    pub bgr: bool,
}

/// Limits of [`Lens::pitch`]: under two subpixels there is nothing to split
/// between two eyes.
pub const PITCH_RANGE: (f64, f64) = (0.7, 64.0);
/// Limits of [`Lens::slant`].
pub const SLANT_RANGE: (f64, f64) = (-2.0, 2.0);

impl Default for Lens {
    /// A coarse starting point for the calibration screen, not a calibration.
    fn default() -> Lens {
        Lens {
            pitch: 5.4,
            slant: 0.3,
            offset: 0.0,
            bgr: false,
        }
    }
}

impl Lens {
    /// The same lens with every number inside its limits, so values typed by
    /// hand or read from a file can never stall or divide by zero.
    pub fn sanitized(self) -> Lens {
        let fallback = Lens::default();
        let within = |value: f64, range: (f64, f64), fallback: f64| {
            if value.is_finite() {
                value.clamp(range.0, range.1)
            } else {
                fallback
            }
        };
        Lens {
            pitch: within(self.pitch, PITCH_RANGE, fallback.pitch),
            slant: within(self.slant, SLANT_RANGE, fallback.slant),
            offset: if self.offset.is_finite() {
                self.offset.rem_euclid(1.0)
            } else {
                fallback.offset
            },
            bgr: self.bgr,
        }
    }

    /// Whether the subpixel of colour `channel` (0 red, 1 green, 2 blue) in
    /// panel pixel (`x`, `y`) is seen by the right eye.
    pub fn is_right(&self, x: i64, y: i64, channel: usize) -> bool {
        Row::new(self, y).is_right(x, channel)
    }
}

/// One pixel row of a lens: everything about the row worked out once.
struct Row {
    base: f64,
    per_subpixel: f64,
    bgr: bool,
}

impl Row {
    fn new(lens: &Lens, y: i64) -> Row {
        Row {
            base: lens.offset - lens.slant * y as f64 / lens.pitch,
            per_subpixel: 1.0 / (3.0 * lens.pitch),
            bgr: lens.bgr,
        }
    }

    fn is_right(&self, x: i64, channel: usize) -> bool {
        let slot = if self.bgr { 2 - channel } else { channel };
        // The centre of the subpixel, counted in subpixels from the left
        // edge of the panel. Multiplied out each time, never accumulated, so
        // the last column is as exact as the first.
        let position = (3 * x + slot as i64) as f64 + 0.5;
        let phase = self.base + position * self.per_subpixel;
        phase - phase.floor() >= 0.5
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WeaveError {
    #[error("There is no picture to interleave.")]
    Empty,
    #[error("The left and right images must have the same size.")]
    SizeMismatch,
}

/// Interleave `left` and `right`, which must both be exactly as large as the
/// area they will cover on the panel. `origin` is the panel pixel under the
/// top left corner of that area, (0, 0) for a full-screen picture.
pub fn weave(
    left: &RgbaImage,
    right: &RgbaImage,
    lens: &Lens,
    origin: (i64, i64),
) -> Result<RgbaImage, WeaveError> {
    if left.width() == 0 || left.height() == 0 {
        return Err(WeaveError::Empty);
    }
    if left.dimensions() != right.dimensions() {
        return Err(WeaveError::SizeMismatch);
    }
    let lens = lens.sanitized();
    let mut out = RgbaImage::new(left.width(), left.height());
    let width = left.width() as usize;
    let rows = out
        .chunks_exact_mut(width * 4)
        .zip(left.chunks_exact(width * 4))
        .zip(right.chunks_exact(width * 4));
    for (y, ((dst, l), r)) in rows.enumerate() {
        let row = Row::new(&lens, origin.1 + y as i64);
        let pixels = dst
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(l.as_chunks::<4>().0)
            .zip(r.as_chunks::<4>().0);
        for (x, ((dst, l), r)) in pixels.enumerate() {
            let x = origin.0 + x as i64;
            for channel in 0..3 {
                dst[channel] = if row.is_right(x, channel) {
                    r[channel]
                } else {
                    l[channel]
                };
            }
            dst[3] = 255;
        }
    }
    Ok(out)
}

/// The calibration pattern: white for the left eye and black for the right.
/// With the lens on and the numbers right, the screen looks evenly bright
/// with the right eye closed and evenly dark with the left eye closed. Bands
/// mean pitch or slant is off; the wrong eye being bright means the offset
/// is half a period out.
pub fn calibration_pattern(width: u32, height: u32) -> (RgbaImage, RgbaImage) {
    (
        RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 255])),
        RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 255])),
    )
}

/// `image` scaled to fit a `width` x `height` area without distortion and
/// centred on black. `stretch_x` widens the image first: 2.0 for one eye of
/// a half-width pair, 1.0 otherwise.
pub fn fit(image: &RgbaImage, width: u32, height: u32, stretch_x: f64) -> RgbaImage {
    let mut canvas = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 255]));
    if image.width() == 0 || image.height() == 0 || width == 0 || height == 0 {
        return canvas;
    }
    let stretch = if stretch_x.is_finite() && stretch_x > 0.0 {
        stretch_x
    } else {
        1.0
    };
    let shown_w = image.width() as f64 * stretch;
    let shown_h = image.height() as f64;
    let scale = (width as f64 / shown_w).min(height as f64 / shown_h);
    let w = ((shown_w * scale).round() as u32).clamp(1, width);
    let h = ((shown_h * scale).round() as u32).clamp(1, height);
    // Copy native-size pixels straight into the canvas; only a resize needs
    // its own temporary image.
    let resized;
    let scaled = if (w, h) == image.dimensions() {
        image
    } else {
        resized = imageops::resize(image, w, h, imageops::FilterType::Triangle);
        &resized
    };
    imageops::replace(
        &mut canvas,
        scaled,
        ((width - w) / 2) as i64,
        ((height - h) / 2) as i64,
    );
    canvas
}

/// Interleave a side-by-side `pair` (left eye in the left half) for an area
/// of `width` x `height` panel pixels at `origin`. Each eye is fitted to the
/// area first; `stretch_x` is as in [`fit`].
pub fn weave_pair(
    pair: &RgbaImage,
    width: u32,
    height: u32,
    stretch_x: f64,
    lens: &Lens,
    origin: (i64, i64),
) -> Result<RgbaImage, WeaveError> {
    if pair.width() < 2 || pair.height() == 0 || width == 0 || height == 0 {
        return Err(WeaveError::Empty);
    }
    let eye = pair.width() / 2;
    let left = imageops::crop_imm(pair, 0, 0, eye, pair.height()).to_image();
    let right = imageops::crop_imm(pair, pair.width() - eye, 0, eye, pair.height()).to_image();
    weave(
        &fit(&left, width, height, stretch_x),
        &fit(&right, width, height, stretch_x),
        lens,
        origin,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lens(pitch: f64, slant: f64, offset: f64) -> Lens {
        Lens {
            pitch,
            slant,
            offset,
            bgr: false,
        }
    }

    /// The eye of each subpixel of row `y`, left to right, as a string.
    fn row(lens: &Lens, y: i64, pixels: i64) -> String {
        (0..pixels)
            .flat_map(|x| (0..3).map(move |c| (x, c)))
            .map(|(x, c)| if lens.is_right(x, y, c) { 'R' } else { 'L' })
            .collect()
    }

    #[test]
    fn a_two_pixel_lens_gives_each_eye_three_subpixels_in_turn() {
        // Six subpixels per lens: the first half left, the second half right.
        assert_eq!(row(&lens(2.0, 0.0, 0.0), 0, 4), "LLLRRRLLLRRR");
        // Half a period exchanges the eyes.
        assert_eq!(row(&lens(2.0, 0.0, 0.5), 0, 4), "RRRLLLRRRLLL");
        // A whole period changes nothing.
        assert_eq!(
            row(&lens(2.0, 0.0, 1.0).sanitized(), 0, 4),
            row(&lens(2.0, 0.0, 0.0), 0, 4)
        );
    }

    #[test]
    fn the_split_happens_inside_a_pixel_when_the_lens_asks_for_it() {
        // One pixel per lens: red is left, blue is right, green sits on the
        // boundary (its centre is exactly half way) and goes right.
        assert_eq!(row(&lens(1.0, 0.0, 0.0), 0, 2), "LRRLRR");
        let white = RgbaImage::from_pixel(2, 1, Rgba([255, 255, 255, 255]));
        let black = RgbaImage::from_pixel(2, 1, Rgba([0, 0, 0, 255]));
        let woven = weave(&white, &black, &lens(1.0, 0.0, 0.0), (0, 0)).unwrap();
        assert_eq!(woven.get_pixel(0, 0).0, [255, 0, 0, 255]);
        // A blue-green-red panel has the subpixels in the opposite order.
        let bgr = Lens {
            bgr: true,
            ..lens(1.0, 0.0, 0.0)
        };
        let woven = weave(&white, &black, &bgr, (0, 0)).unwrap();
        assert_eq!(woven.get_pixel(0, 0).0, [0, 0, 255, 255]);
    }

    #[test]
    fn slant_moves_the_pattern_sideways_row_by_row() {
        // One pixel of slant per row: row 1 is row 0 moved one pixel right.
        let slanted = lens(2.0, 1.0, 0.0);
        let first = row(&slanted, 0, 6);
        let second = row(&slanted, 1, 6);
        assert_eq!(&second[3..], &first[..first.len() - 3]);
        // A slant the other way moves it left.
        let other = lens(2.0, -1.0, 0.0);
        let second = row(&other, 1, 6);
        assert_eq!(&second[..second.len() - 3], &first[3..]);
        // And a third of a pixel per row moves it one subpixel.
        let fine = lens(2.0, 1.0 / 3.0, 0.0);
        let third = row(&fine, 3, 6);
        assert_eq!(&third[3..], &first[..first.len() - 3]);
    }

    #[test]
    fn the_far_edge_of_a_4k_row_is_as_exact_as_the_near_edge() {
        // 5.4 pixels per lens repeats every 27 pixels (five lenses), so
        // pixel 3834 = 27 * 142 must look exactly like pixel 0.
        let l = lens(5.4, 0.0, 0.0);
        for x in 0..6 {
            for c in 0..3 {
                assert_eq!(l.is_right(x, 0, c), l.is_right(x + 3834, 0, c), "{x} {c}");
            }
        }
        // Both eyes get half of the subpixels over a whole number of lenses.
        let right = row(&l, 0, 27).matches('R').count();
        assert!((39..=42).contains(&right), "{right} of 81");
    }

    #[test]
    fn a_window_away_from_the_corner_continues_the_panel_pattern() {
        let l = lens(5.4, 0.29, 0.13);
        let left = RgbaImage::from_pixel(40, 30, Rgba([255, 255, 255, 255]));
        let right = RgbaImage::from_pixel(40, 30, Rgba([0, 0, 0, 255]));
        let full = weave(&left, &right, &l, (0, 0)).unwrap();
        let part_l = RgbaImage::from_pixel(10, 8, Rgba([255, 255, 255, 255]));
        let part_r = RgbaImage::from_pixel(10, 8, Rgba([0, 0, 0, 255]));
        let part = weave(&part_l, &part_r, &l, (17, 9)).unwrap();
        for (x, y, pixel) in part.enumerate_pixels() {
            assert_eq!(pixel, full.get_pixel(x + 17, y + 9), "{x},{y}");
        }
    }

    #[test]
    fn each_subpixel_comes_from_the_image_of_its_eye() {
        let left = RgbaImage::from_fn(6, 2, |x, y| Rgba([10 + x as u8, 20 + y as u8, 30, 7]));
        let right = RgbaImage::from_fn(6, 2, |x, y| Rgba([110 + x as u8, 120 + y as u8, 130, 9]));
        let l = lens(1.7, 0.4, 0.2);
        let woven = weave(&left, &right, &l, (0, 0)).unwrap();
        for (x, y, pixel) in woven.enumerate_pixels() {
            for c in 0..3 {
                let from = if l.is_right(x as i64, y as i64, c) {
                    right.get_pixel(x, y)
                } else {
                    left.get_pixel(x, y)
                };
                assert_eq!(pixel.0[c], from.0[c]);
            }
            assert_eq!(pixel.0[3], 255, "the output is opaque");
        }
    }

    #[test]
    fn mismatched_and_empty_images_are_refused() {
        let a = RgbaImage::new(4, 4);
        assert_eq!(
            weave(&a, &RgbaImage::new(4, 3), &Lens::default(), (0, 0)),
            Err(WeaveError::SizeMismatch)
        );
        assert_eq!(
            weave(
                &RgbaImage::new(0, 0),
                &RgbaImage::new(0, 0),
                &Lens::default(),
                (0, 0)
            ),
            Err(WeaveError::Empty)
        );
        assert_eq!(
            weave_pair(&RgbaImage::new(1, 4), 8, 8, 1.0, &Lens::default(), (0, 0)),
            Err(WeaveError::Empty)
        );
    }

    #[test]
    fn numbers_out_of_range_are_brought_back_inside() {
        let wild = Lens {
            pitch: 0.0,
            slant: f64::NAN,
            offset: -0.25,
            bgr: true,
        }
        .sanitized();
        assert_eq!(wild.pitch, PITCH_RANGE.0);
        assert_eq!(wild.slant, Lens::default().slant);
        assert_eq!(wild.offset, 0.75);
        assert!(wild.bgr);
        let huge = lens(f64::INFINITY, 9.0, 3.5).sanitized();
        assert_eq!(huge.pitch, Lens::default().pitch);
        assert_eq!(huge.slant, SLANT_RANGE.1);
        assert_eq!(huge.offset, 0.5);
        // Weaving sanitizes for itself: a zero pitch cannot divide by zero.
        let a = RgbaImage::new(3, 3);
        assert!(weave(&a, &a, &lens(0.0, 0.0, 0.0), (0, 0)).is_ok());
    }

    #[test]
    fn a_lens_file_round_trips_and_tolerates_missing_fields() {
        let l = lens(5.416, 0.2903, 0.37);
        let text = serde_json::to_string(&l).unwrap();
        assert_eq!(serde_json::from_str::<Lens>(&text).unwrap(), l);
        let partial: Lens = serde_json::from_str(r#"{"pitch": 6.5}"#).unwrap();
        assert_eq!(
            partial,
            Lens {
                pitch: 6.5,
                ..Lens::default()
            }
        );
    }

    #[test]
    fn an_eye_is_fitted_centred_and_a_half_width_eye_is_widened() {
        let red = RgbaImage::from_pixel(4, 4, Rgba([255, 0, 0, 255]));
        // A square in a 8x4 area: 4x4 in the middle, black at the sides.
        let fitted = fit(&red, 8, 4, 1.0);
        assert_eq!(fitted.get_pixel(0, 0).0, [0, 0, 0, 255]);
        assert_eq!(fitted.get_pixel(2, 0).0, [255, 0, 0, 255]);
        assert_eq!(fitted.get_pixel(5, 3).0, [255, 0, 0, 255]);
        assert_eq!(fitted.get_pixel(6, 3).0, [0, 0, 0, 255]);
        // The same square as one eye of a half-width pair fills the area.
        let widened = fit(&red, 8, 4, 2.0);
        assert!(widened.pixels().all(|p| p.0 == [255, 0, 0, 255]));
        // Nothing to draw is black, not a panic.
        assert!(fit(&RgbaImage::new(0, 0), 2, 2, 1.0)
            .pixels()
            .all(|p| p.0 == [0, 0, 0, 255]));
    }

    #[test]
    fn a_pair_is_split_fitted_and_interleaved() {
        // Left eye white, right eye black: the result is the pattern.
        let pair = RgbaImage::from_fn(8, 2, |x, _| {
            if x < 4 {
                Rgba([255, 255, 255, 255])
            } else {
                Rgba([0, 0, 0, 255])
            }
        });
        let l = lens(2.0, 0.0, 0.0);
        let woven = weave_pair(&pair, 4, 2, 1.0, &l, (0, 0)).unwrap();
        let (white, black) = calibration_pattern(4, 2);
        assert_eq!(woven, weave(&white, &black, &l, (0, 0)).unwrap());
        assert_eq!(woven.get_pixel(0, 0).0, [255, 255, 255, 255]);
        assert_eq!(woven.get_pixel(1, 0).0, [0, 0, 0, 255]);
    }
}
