//! Stereo-layout conversion only: side-by-side pairs to anaglyph or SBS, flat
//! pass-through. No depth estimation and no panel calibration happen here.
//! This is the portable-edition conversion, shared by every OS.

use std::borrow::Cow;

use image::{imageops, RgbaImage};
use thiserror::Error;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// Ordinary 2D stays flat; the image is copied unchanged.
    Flat,
    /// Keep the left/right pair side by side (optionally swapped).
    Sbs,
    /// Red/cyan anaglyph: left eye's red, right eye's green and blue.
    Anaglyph,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StereoError {
    #[error("The source contains no image.")]
    Empty,
    #[error("Side-by-side input must have an even width of at least two pixels.")]
    OddWidth,
}

/// Convert `image` according to `layout`.
///
/// * `half_width`: for anaglyph, stretch each eye to the full source width
///   (half-width SBS sources). Ignored for other layouts.
/// * `swap`: exchange the left and right eyes before composing.
pub fn convert(
    image: &RgbaImage,
    layout: Layout,
    half_width: bool,
    swap: bool,
) -> Result<RgbaImage, StereoError> {
    convert_shared(image, layout, half_width, swap).map(Cow::into_owned)
}

/// [`convert`] that hands `image` itself back where the result is the
/// image unchanged (flat, and a side-by-side pair that is not swapped), so a
/// caller that keeps the source does not hold a second copy of it.
pub fn convert_shared(
    image: &RgbaImage,
    layout: Layout,
    half_width: bool,
    swap: bool,
) -> Result<Cow<'_, RgbaImage>, StereoError> {
    if image.width() == 0 || image.height() == 0 {
        return Err(StereoError::Empty);
    }
    if layout == Layout::Flat {
        return Ok(Cow::Borrowed(image));
    }
    if image.width() < 2 || !image.width().is_multiple_of(2) {
        return Err(StereoError::OddWidth);
    }
    let row_bytes = image.width() as usize * 4;
    let eye = image.width() / 2;
    match layout {
        Layout::Flat => unreachable!(),
        Layout::Sbs if !swap => Ok(Cow::Borrowed(image)),
        Layout::Sbs => {
            let mut out = RgbaImage::new(image.width(), image.height());
            // Copy each eye directly into its destination, preserving every
            // channel without allocating two intermediate eye images.
            for (src, dst) in image
                .as_raw()
                .chunks_exact(row_bytes)
                .zip(out.as_mut().chunks_exact_mut(row_bytes))
            {
                let (left, right) = src.split_at(row_bytes / 2);
                let (dst_left, dst_right) = dst.split_at_mut(row_bytes / 2);
                dst_left.copy_from_slice(right);
                dst_right.copy_from_slice(left);
            }
            Ok(Cow::Owned(out))
        }
        Layout::Anaglyph if half_width => {
            let threads = crate::dibr::threads_for(image.width() as usize, image.height() as usize);
            Ok(Cow::Owned(stretched_anaglyph(image, swap, threads)))
        }
        Layout::Anaglyph => {
            // Red from the left eye, green and blue from the right, written
            // straight from the pair's rows.
            let mut out = RgbaImage::new(eye, image.height());
            for (src, dst) in image
                .as_raw()
                .chunks_exact(row_bytes)
                .zip(out.as_mut().chunks_exact_mut(row_bytes / 2))
            {
                let (first, second) = src.split_at(row_bytes / 2);
                let (left, right) = if swap {
                    (second, first)
                } else {
                    (first, second)
                };
                let pixels = dst
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(left.as_chunks::<4>().0)
                    .zip(right.as_chunks::<4>().0);
                for ((dst, left), right) in pixels {
                    *dst = [left[0], right[1], right[2], 255];
                }
            }
            Ok(Cow::Owned(out))
        }
    }
}

/// The anaglyph of a half-width pair (even width, not empty): each eye
/// widened to the pair's width, red from the left one, green and blue from
/// the right.
///
/// Widening is nearly all of the work, and it leaves the height alone. The
/// filter then makes every row from that row of the pair and no other (its
/// weights for the rows above and below are exactly zero), so the rows are
/// widened in bands on `threads` threads and the picture is the same as
/// when each eye is widened whole, which the tests hold it against.
fn stretched_anaglyph(image: &RgbaImage, swap: bool, threads: usize) -> RgbaImage {
    let (width, height) = image.dimensions();
    let eye = width / 2;
    let row_bytes = width as usize * 4;
    let (left, right) = if swap { (eye, 0) } else { (0, eye) };
    let mut out = RgbaImage::new(width, height);
    crate::dibr::in_bands(&mut out, row_bytes, threads, |first_row, band| {
        let rows = (band.len() / row_bytes) as u32;
        // Each eye is read where it lies in the pair: the filter needs no
        // copy of its half first.
        let stretched = |x: u32| {
            imageops::resize(
                &*imageops::crop_imm(image, x, first_row as u32, eye, rows),
                width,
                rows,
                imageops::FilterType::Triangle,
            )
        };
        let (red, rest) = (stretched(left), stretched(right));
        let pixels = band
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(red.as_raw().as_chunks::<4>().0)
            .zip(rest.as_raw().as_chunks::<4>().0);
        for ((dst, red), rest) in pixels {
            *dst = [red[0], rest[1], rest[2], 255];
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn sample() -> RgbaImage {
        // 4x2: left eye (20,40,60), right eye (100,120,140), as in Any2Odd's tests.
        RgbaImage::from_fn(4, 2, |x, _| {
            if x < 2 {
                Rgba([20, 40, 60, 255])
            } else {
                Rgba([100, 120, 140, 255])
            }
        })
    }

    #[test]
    fn anaglyph_channels_and_geometry() {
        let img = sample();
        let result = convert(&img, Layout::Anaglyph, false, false).unwrap();
        assert_eq!((result.width(), result.height()), (2, 2));
        assert_eq!(result.get_pixel(0, 0).0, [20, 120, 140, 255]);
        assert_eq!(
            convert(&img, Layout::Anaglyph, true, false)
                .unwrap()
                .width(),
            4
        );
        assert_eq!(
            convert(&img, Layout::Anaglyph, false, true)
                .unwrap()
                .get_pixel(0, 0)
                .0,
            [100, 40, 60, 255]
        );
    }

    #[test]
    fn sbs_swapping_and_flat_passthrough() {
        let img = sample();
        assert_eq!(
            convert(&img, Layout::Sbs, true, false)
                .unwrap()
                .get_pixel(0, 0),
            img.get_pixel(0, 0)
        );
        assert_eq!(
            convert(&img, Layout::Sbs, true, true)
                .unwrap()
                .get_pixel(0, 0),
            img.get_pixel(3, 0)
        );
        assert_eq!(convert(&img, Layout::Flat, true, false).unwrap(), img);
        let odd = RgbaImage::new(3, 2);
        assert_eq!(
            convert(&odd, Layout::Sbs, true, false),
            Err(StereoError::OddWidth)
        );
        assert_eq!(
            convert(&RgbaImage::new(0, 0), Layout::Flat, true, false),
            Err(StereoError::Empty)
        );
    }

    /// The conversion as it was when every eye was copied out of the pair
    /// first. Kept as the definition of the result.
    fn reference(image: &RgbaImage, layout: Layout, half_width: bool, swap: bool) -> RgbaImage {
        if layout == Layout::Flat {
            return image.clone();
        }
        let eye = image.width() / 2;
        let mut left = imageops::crop_imm(image, 0, 0, eye, image.height()).to_image();
        let mut right = imageops::crop_imm(image, eye, 0, eye, image.height()).to_image();
        if swap {
            std::mem::swap(&mut left, &mut right);
        }
        if layout == Layout::Sbs {
            let mut out = RgbaImage::new(image.width(), image.height());
            imageops::replace(&mut out, &left, 0, 0);
            imageops::replace(&mut out, &right, i64::from(eye), 0);
            return out;
        }
        if half_width {
            let stretch = |eye: &RgbaImage| {
                imageops::resize(
                    eye,
                    image.width(),
                    image.height(),
                    imageops::FilterType::Triangle,
                )
            };
            (left, right) = (stretch(&left), stretch(&right));
        }
        let mut out = right;
        for (dst, src) in out.pixels_mut().zip(left.pixels()) {
            dst.0[0] = src.0[0];
            dst.0[3] = 255;
        }
        out
    }

    #[test]
    fn every_conversion_matches_the_copying_one_and_only_changes_are_copies() {
        let mut noise = 0x9e37_79b9u32;
        for (width, height) in [(2, 1), (6, 3), (34, 9)] {
            let source = RgbaImage::from_fn(width, height, |_, _| {
                noise ^= noise << 13;
                noise ^= noise >> 17;
                noise ^= noise << 5;
                Rgba(noise.to_le_bytes())
            });
            for layout in [Layout::Flat, Layout::Sbs, Layout::Anaglyph] {
                for (half_width, swap) in
                    [(false, false), (false, true), (true, false), (true, true)]
                {
                    let shared = convert_shared(&source, layout, half_width, swap).unwrap();
                    let expected = reference(&source, layout, half_width, swap);
                    assert_eq!(
                        *shared, expected,
                        "{width}x{height} {layout:?} {half_width} {swap}"
                    );
                    assert_eq!(
                        convert(&source, layout, half_width, swap).unwrap(),
                        expected
                    );
                    // The source itself comes back exactly where nothing
                    // is changed.
                    let unchanged = layout == Layout::Flat || (layout == Layout::Sbs && !swap);
                    assert_eq!(matches!(shared, Cow::Borrowed(_)), unchanged);
                }
            }
        }
    }

    #[test]
    fn a_half_width_pair_widened_in_bands_is_the_pair_widened_whole() {
        let mut noise = 0x51f1_5eedu32;
        // Heights that do not divide into the bands, a single row, a pair
        // large enough for the public entry to start threads of its own.
        for (width, height) in [(2, 1), (6, 2), (34, 9), (130, 67), (640, 120)] {
            let source = RgbaImage::from_fn(width, height, |_, _| {
                noise ^= noise << 13;
                noise ^= noise >> 17;
                noise ^= noise << 5;
                Rgba(noise.to_le_bytes())
            });
            for swap in [false, true] {
                let expected = reference(&source, Layout::Anaglyph, true, swap);
                for threads in [1, 2, 3, 7, 64, 1000] {
                    assert!(
                        stretched_anaglyph(&source, swap, threads) == expected,
                        "{width}x{height}, swapped {swap}, {threads} threads"
                    );
                }
                assert!(convert(&source, Layout::Anaglyph, true, swap).unwrap() == expected);
            }
        }
    }

    #[test]
    fn anaglyph_output_is_opaque() {
        let img = RgbaImage::from_pixel(6, 2, Rgba([1, 2, 3, 0]));
        let out = convert(&img, Layout::Anaglyph, false, false).unwrap();
        assert!(out.pixels().all(|p| p.0[3] == 255));
    }

    #[test]
    fn sbs_preserves_all_pixels_and_alpha_when_swapping_rows() {
        for width in [2, 6, 16] {
            let source = RgbaImage::from_fn(width, 3, |x, y| {
                Rgba([x as u8, y as u8, (x + y) as u8, (x * 13 + y) as u8])
            });
            for half_width in [false, true] {
                assert_eq!(
                    convert(&source, Layout::Sbs, half_width, false).unwrap(),
                    source
                );
                let swapped = convert(&source, Layout::Sbs, half_width, true).unwrap();
                for (x, y, pixel) in swapped.enumerate_pixels() {
                    assert_eq!(pixel, source.get_pixel((x + width / 2) % width, y));
                }
                assert_eq!(
                    convert(&swapped, Layout::Sbs, half_width, true).unwrap(),
                    source
                );
            }
        }
    }
}
