//! A picture resized into its place in another, row by row.
//!
//! The still viewer fits each eye's view into the picture it hands to the
//! Odyssey 3D output with the image crate's `resize` (the triangle filter,
//! bilinear) followed by `replace`. That pair makes the whole resized
//! picture twice over before a pixel reaches its place: once as 16 bytes a
//! pixel, column by column, and once as bytes, copied a pixel at a time. It
//! was most of what a step of a slider cost with that output on.
//!
//! [`resize_into`] makes the same bytes. Every number is worked out by the
//! same steps in the same order as the image crate (0.25) does: the weights
//! of a row or a column, the sum over the rows that feed a row, then the sum
//! over the columns that feed a pixel, clamped and rounded. But a row is
//! made from the few source rows it needs and written where it goes, so
//! nothing is held but a line of numbers per thread, and the rows are
//! shared out over the cores. A test compares the two on pictures of many
//! sizes, so a later version of the image crate that resizes differently
//! is noticed.

use image::RgbaImage;

/// Some columns of an RGBA picture, at full height: one eye of a
/// side-by-side pair, or the whole picture.
#[derive(Clone, Copy, Debug)]
pub struct Part<'a> {
    bytes: &'a [u8],
    /// Bytes from one row of the picture to the next.
    stride: usize,
    /// The first column, and how many there are.
    x: usize,
    width: usize,
    height: usize,
}

impl<'a> Part<'a> {
    /// `width` columns of `image` from column `x` on, cut to what the
    /// picture has (as `image::imageops::crop_imm` cuts).
    pub fn of(image: &'a RgbaImage, x: u32, width: u32) -> Part<'a> {
        let x = x.min(image.width());
        let width = width.min(image.width() - x);
        Part {
            bytes: image.as_raw(),
            stride: image.width() as usize * 4,
            x: x as usize,
            width: width as usize,
            height: image.height() as usize,
        }
    }

    /// How many columns and rows the part has.
    pub fn size(&self) -> (u32, u32) {
        (self.width as u32, self.height as u32)
    }

    /// The bytes of row `y`.
    fn row(&self, y: usize) -> &'a [u8] {
        let start = y * self.stride + self.x * 4;
        &self.bytes[start..start + self.width * 4]
    }
}

/// Which source rows (or columns) feed each row (or column) of the result,
/// and how much of each: the image crate's sampling with its triangle
/// filter, whose support is one pixel of the smaller of the two sizes.
struct Taps {
    /// For each output: the first source index and where its weights start.
    first: Vec<u32>,
    start: Vec<u32>,
    weights: Vec<f32>,
}

impl Taps {
    fn new(size: usize, new_size: usize) -> Taps {
        let mut taps = Taps {
            first: Vec::with_capacity(new_size),
            start: Vec::with_capacity(new_size + 1),
            weights: Vec::new(),
        };
        let ratio = size as f32 / new_size as f32;
        let sratio = if ratio < 1.0 { 1.0 } else { ratio };
        let src_support = sratio;
        for out in 0..new_size {
            // The point in the source that the centre of this output
            // pixel falls on, and the source pixels within reach of it.
            let input = (out as f32 + 0.5) * ratio;
            let left = ((input - src_support).floor() as i64).clamp(0, size as i64 - 1);
            let right = ((input + src_support).ceil() as i64).clamp(left + 1, size as i64);
            let input = input - 0.5;
            taps.first.push(left as u32);
            taps.start.push(taps.weights.len() as u32);
            let from = taps.weights.len();
            let mut sum = 0.0f32;
            for i in left..right {
                let x = (i as f32 - input) / sratio;
                let weight = if x.abs() < 1.0 { 1.0 - x.abs() } else { 0.0 };
                taps.weights.push(weight);
                sum += weight;
            }
            for weight in &mut taps.weights[from..] {
                *weight /= sum;
            }
        }
        taps.start.push(taps.weights.len() as u32);
        taps
    }

    /// The first source index and the weights of output `index`.
    fn of(&self, index: usize) -> (usize, &[f32]) {
        let weights = &self.weights[self.start[index] as usize..self.start[index + 1] as usize];
        (self.first[index] as usize, weights)
    }
}

/// A number between 0 and 255 to the nearest byte, a half going up: what
/// `f32::round` gives, without a call into the mathematics library for each
/// of the four bytes of each pixel. Cutting the fraction off and taking it
/// from the number are both exact for numbers this small.
fn nearest_byte(value: f32) -> u8 {
    let value = value.clamp(0.0, 255.0);
    let whole = value as u8;
    whole + u8::from(value - f32::from(whole) >= 0.5)
}

/// The resize of one [`Part`] to one size, ready to make any of its rows.
struct Resize<'a> {
    part: Part<'a>,
    width: usize,
    rows: Taps,
    columns: Taps,
    /// The same size as the part: the image crate copies then.
    copy: bool,
}

impl<'a> Resize<'a> {
    /// `None` for a part without pixels (the image crate gives a picture of
    /// zeros then).
    fn new(part: Part<'a>, width: usize, height: usize) -> Option<Resize<'a>> {
        if part.width == 0 || part.height == 0 {
            return None;
        }
        let copy = (width, height) == (part.width, part.height);
        Some(Resize {
            part,
            width,
            rows: Taps::new(part.height, if copy { 0 } else { height }),
            columns: Taps::new(part.width, if copy { 0 } else { width }),
            copy,
        })
    }

    /// Row `y` of the resized picture into `out` (four bytes a pixel).
    /// `line` is working room, kept between rows.
    fn row(&self, y: usize, line: &mut Vec<f32>, out: &mut [u8]) {
        if self.copy {
            out.copy_from_slice(self.part.row(y));
            return;
        }
        // Down the columns first: the source rows that feed this row,
        // weighted and summed, as numbers that are not rounded yet.
        let (first, weights) = self.rows.of(y);
        line.clear();
        line.resize(self.part.width * 4, 0.0);
        for (index, &weight) in weights.iter().enumerate() {
            let source = self.part.row(first + index);
            for (sum, &byte) in line.iter_mut().zip(source) {
                *sum += f32::from(byte) * weight;
            }
        }
        // Then along the row.
        let line = line.as_chunks::<4>().0;
        for (x, pixel) in out.as_chunks_mut::<4>().0[..self.width]
            .iter_mut()
            .enumerate()
        {
            let (first, weights) = self.columns.of(x);
            let mut sum = [0.0f32; 4];
            for (source, &weight) in line[first..first + weights.len()].iter().zip(weights) {
                for (sum, &value) in sum.iter_mut().zip(source) {
                    *sum += value * weight;
                }
            }
            *pixel = sum.map(nearest_byte);
        }
    }
}

/// Resize `part` to `width` x `height` and put it into `canvas` with its
/// top row at row `y`, once for each column in `at` (its left edge): the
/// bytes `image::imageops::resize` with the triangle filter followed by
/// `image::imageops::replace` put there, for each of them. More than one
/// place is for a flat picture, which is the same in both eyes and is
/// resized once. False, and `canvas` untouched, when a place does not lie
/// inside `canvas`.
pub fn resize_into(
    canvas: &mut RgbaImage,
    part: Part<'_>,
    at: &[u32],
    y: u32,
    width: u32,
    height: u32,
) -> bool {
    let (canvas_w, canvas_h) = (canvas.width() as usize, canvas.height() as usize);
    let (w, h, y) = (width as usize, height as usize, y as usize);
    if at.iter().any(|&x| x as usize + w > canvas_w) || y + h > canvas_h {
        return false;
    }
    if w == 0 || h == 0 || at.is_empty() {
        return true;
    }
    let row_bytes = canvas_w * 4;
    let rows = &mut canvas.as_mut()[y * row_bytes..(y + h) * row_bytes];
    let Some(resize) = Resize::new(part, w, h) else {
        for row in rows.chunks_exact_mut(row_bytes) {
            for &x in at {
                row[x as usize * 4..(x as usize + w) * 4].fill(0);
            }
        }
        return true;
    };
    let threads = crate::dibr::threads_for(w, h);
    crate::dibr::in_bands(rows, row_bytes, threads, |first_row, band| {
        let mut line = Vec::new();
        for (index, row) in band.chunks_exact_mut(row_bytes).enumerate() {
            let first = at[0] as usize * 4;
            resize.row(first_row + index, &mut line, &mut row[first..first + w * 4]);
            for &x in &at[1..] {
                row.copy_within(first..first + w * 4, x as usize * 4);
            }
        }
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::imageops::{self, FilterType};
    use image::Rgba;

    /// Every pixel different from its neighbours, alpha included.
    fn noisy(w: u32, h: u32, seed: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            let v = (x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503))
                .wrapping_add(seed.wrapping_mul(977))
                .to_le_bytes();
            Rgba([v[0], v[1] ^ v[3], v[2], 255 - v[1] / 3])
        })
    }

    /// What the viewer did: the image crate's resize of the part, then its
    /// replace onto the canvas at each place.
    fn with_the_image_crate(
        canvas: &mut RgbaImage,
        image: &RgbaImage,
        (x, part_width): (u32, u32),
        at: &[u32],
        y: u32,
        (width, height): (u32, u32),
    ) {
        let part = imageops::crop_imm(image, x, 0, part_width, image.height());
        let resized = imageops::resize(&*part, width, height, FilterType::Triangle);
        for &left in at {
            imageops::replace(canvas, &resized, i64::from(left), i64::from(y));
        }
    }

    fn both_ways(
        image: &RgbaImage,
        part: (u32, u32),
        at: &[u32],
        y: u32,
        size: (u32, u32),
        canvas: (u32, u32),
    ) -> (RgbaImage, RgbaImage) {
        let blank = RgbaImage::from_pixel(canvas.0, canvas.1, Rgba([9, 8, 7, 255]));
        let (mut ours, mut theirs) = (blank.clone(), blank);
        assert!(resize_into(
            &mut ours,
            Part::of(image, part.0, part.1),
            at,
            y,
            size.0,
            size.1
        ));
        with_the_image_crate(&mut theirs, image, part, at, y, size);
        (ours, theirs)
    }

    #[test]
    fn the_bytes_are_those_of_the_image_crates_resize_and_replace() {
        // (source, the part's columns, the new size)
        type Size = (u32, u32);
        let cases: &[(Size, Size, Size)] = &[
            // Larger, smaller, and one of each.
            ((64, 36), (0, 64), (128, 72)),
            ((64, 36), (0, 64), (32, 18)),
            ((64, 36), (0, 64), (200, 11)),
            ((64, 36), (0, 64), (7, 190)),
            // One side kept: the other is still sampled.
            ((64, 36), (0, 64), (64, 72)),
            ((64, 36), (0, 64), (33, 36)),
            // The same size: a copy.
            ((64, 36), (0, 64), (64, 36)),
            // An eye of a pair, left and right, and odd sizes.
            ((130, 37), (0, 65), (97, 53)),
            ((130, 37), (65, 65), (97, 53)),
            ((131, 37), (65, 65), (301, 20)),
            // Sizes that do not divide, and a great change of size.
            ((641, 181), (0, 320), (1920, 1085)),
            ((1000, 3), (0, 1000), (3, 400)),
            ((3, 1000), (0, 3), (400, 3)),
            // One pixel, one row, one column, and to one pixel.
            ((1, 1), (0, 1), (40, 30)),
            ((50, 1), (0, 50), (20, 20)),
            ((1, 50), (0, 1), (20, 20)),
            ((50, 40), (0, 50), (1, 1)),
            ((50, 40), (10, 30), (1, 77)),
            ((50, 40), (10, 30), (77, 1)),
            // A part that reaches past the picture is cut to it.
            ((50, 40), (45, 30), (20, 20)),
        ];
        for (index, &((sw, sh), part, size)) in cases.iter().enumerate() {
            let image = noisy(sw, sh, index as u32);
            let canvas = (size.0 + 9, size.1 + 5);
            let (ours, theirs) = both_ways(&image, part, &[4], 3, size, canvas);
            assert!(
                ours.as_raw() == theirs.as_raw(),
                "{sw}x{sh} part {part:?} to {size:?}"
            );
        }
    }

    #[test]
    fn plain_and_extreme_pictures_come_out_the_same_too() {
        // Bytes at both ends, where the clamp and the rounding work.
        for value in [0u8, 1, 127, 128, 254, 255] {
            let flat = RgbaImage::from_pixel(23, 17, Rgba([value, 255 - value, value, value]));
            let (ours, theirs) = both_ways(&flat, (0, 23), &[0], 0, (51, 40), (51, 40));
            assert!(ours.as_raw() == theirs.as_raw(), "{value}");
        }
        let stripes = RgbaImage::from_fn(40, 30, |x, y| {
            if (x + y) % 2 == 0 {
                Rgba([255, 0, 255, 0])
            } else {
                Rgba([0, 255, 0, 255])
            }
        });
        for size in [(81, 59), (13, 9), (40, 31), (39, 30)] {
            let (ours, theirs) = both_ways(&stripes, (0, 40), &[0], 0, size, size);
            assert!(ours.as_raw() == theirs.as_raw(), "{size:?}");
        }
    }

    #[test]
    fn a_picture_put_in_two_places_is_the_same_in_both_and_the_rest_stays() {
        let image = noisy(40, 30, 5);
        let (ours, theirs) = both_ways(&image, (0, 40), &[2, 100], 6, (70, 50), (180, 60));
        assert!(ours.as_raw() == theirs.as_raw());
        for y in 6..56 {
            for x in 0..70 {
                assert_eq!(ours.get_pixel(2 + x, y), ours.get_pixel(100 + x, y));
            }
        }
        // Around the places the canvas is as it was.
        assert_eq!(ours.get_pixel(0, 0).0, [9, 8, 7, 255]);
        assert_eq!(ours.get_pixel(99, 30).0, [9, 8, 7, 255]);
        assert_eq!(ours.get_pixel(175, 59).0, [9, 8, 7, 255]);
    }

    #[test]
    fn a_part_without_pixels_gives_zeros_and_a_place_outside_is_refused() {
        // The image crate resizes nothing to a picture of zeros.
        for (w, h) in [(0, 0), (0, 5), (5, 0)] {
            let empty = RgbaImage::new(w, h);
            let (ours, theirs) = both_ways(&empty, (0, 4), &[1], 1, (6, 4), (9, 7));
            assert!(ours.as_raw() == theirs.as_raw(), "{w}x{h}");
            assert_eq!(ours.get_pixel(1, 1).0, [0, 0, 0, 0]);
            assert_eq!(ours.get_pixel(0, 0).0, [9, 8, 7, 255]);
        }
        // Columns past the picture's edge: no part at all.
        let image = noisy(8, 8, 1);
        let (ours, theirs) = both_ways(&image, (8, 4), &[0], 0, (5, 5), (5, 5));
        assert!(ours.as_raw() == theirs.as_raw());
        // A place that does not fit changes nothing.
        let mut canvas = RgbaImage::from_pixel(10, 10, Rgba([1, 2, 3, 4]));
        let before = canvas.clone();
        let part = Part::of(&image, 0, 8);
        assert!(!resize_into(&mut canvas, part, &[3], 0, 8, 4));
        assert!(!resize_into(&mut canvas, part, &[0], 7, 4, 4));
        assert!(!resize_into(&mut canvas, part, &[0, 7], 0, 4, 4));
        assert_eq!(canvas, before);
        // Nothing to make is not a failure.
        assert!(resize_into(&mut canvas, part, &[0], 0, 0, 4));
        assert!(resize_into(&mut canvas, part, &[], 0, 4, 4));
        assert_eq!(canvas, before);
    }

    #[test]
    fn a_large_picture_is_the_same_on_several_threads() {
        // Large enough for `dibr::threads_for` to share the rows out.
        let image = noisy(320, 180, 3);
        let size = (640, 360);
        assert!(size.0 * size.1 >= 1 << 16);
        let (ours, theirs) = both_ways(&image, (0, 320), &[0], 0, size, size);
        assert!(ours.as_raw() == theirs.as_raw());
    }

    #[test]
    fn a_number_is_rounded_as_the_mathematics_library_rounds_it() {
        let same = |value: f32| {
            let clamped = value.clamp(0.0, 255.0);
            assert_eq!(nearest_byte(value), clamped.round() as u8, "{value:?}");
        };
        // Around every half and every whole number, to the last digit.
        for step in 0..=510u32 {
            let value = step as f32 * 0.5;
            let mut below = value;
            let mut above = value;
            for _ in 0..4 {
                same(below);
                same(above);
                below = f32::from_bits(below.to_bits().saturating_sub(1));
                above = f32::from_bits(above.to_bits() + 1);
            }
        }
        // A walk through all the numbers there are from 0 to 255.
        let mut bits = 0u32;
        while bits <= 255.0f32.to_bits() {
            same(f32::from_bits(bits));
            bits += 1021;
        }
        for value in [-1.0, -0.4, 255.4, 255.5, 256.0, 1e9, -1e9] {
            same(value);
        }
    }
}
