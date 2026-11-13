//! Stereo-layout conversion only: side-by-side pairs to anaglyph or SBS, flat
//! pass-through. No depth estimation and no panel calibration happen here.
//! This is the portable-edition conversion, shared by every OS.

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
    if image.width() == 0 || image.height() == 0 {
        return Err(StereoError::Empty);
    }
    if layout == Layout::Flat {
        return Ok(image.clone());
    }
    if image.width() < 2 || !image.width().is_multiple_of(2) {
        return Err(StereoError::OddWidth);
    }
    if layout == Layout::Sbs {
        if !swap {
            return Ok(image.clone());
        }
        let mut out = RgbaImage::new(image.width(), image.height());
        // Copy each eye directly into its destination, preserving every
        // channel without allocating two intermediate eye images.
        let row_bytes = image.width() as usize * 4;
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
        return Ok(out);
    }
    let eye = image.width() / 2;
    let mut left = imageops::crop_imm(image, 0, 0, eye, image.height()).to_image();
    let mut right = imageops::crop_imm(image, eye, 0, eye, image.height()).to_image();
    if swap {
        std::mem::swap(&mut left, &mut right);
    }
    match layout {
        Layout::Flat | Layout::Sbs => unreachable!(),
        Layout::Anaglyph => {
            if half_width {
                left = imageops::resize(
                    &left,
                    image.width(),
                    image.height(),
                    imageops::FilterType::Triangle,
                );
                right = imageops::resize(
                    &right,
                    image.width(),
                    image.height(),
                    imageops::FilterType::Triangle,
                );
            }
            let mut out = right;
            for (dst, src) in out.pixels_mut().zip(left.pixels()) {
                dst.0[0] = src.0[0];
                dst.0[3] = 255;
            }
            Ok(out)
        }
    }
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
