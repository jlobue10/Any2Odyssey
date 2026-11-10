//! Stereo from one image and its depth map (depth-image-based rendering).
//!
//! Each eye's view is the source image with every pixel shifted sideways by
//! half its disparity, nearer pixels winning where two land on the same
//! place, and the gaps a foreground object leaves behind filled from the
//! background beside it. The result is a full-width side-by-side pair that
//! [`crate::stereo`] turns into anaglyph or leaves as it is.
//!
//! No model runs here: the depth map comes from whoever estimated it.

use image::RgbaImage;
use thiserror::Error;

/// Disparity of the nearest or farthest pixel at depth strength 1, as a
/// fraction of the image width. Three percent keeps a desktop-sized image
/// inside comfortable fusion limits at a normal viewing distance.
pub const MAX_DISPARITY_FRACTION: f32 = 0.03;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DibrError {
    #[error("The source contains no image.")]
    Empty,
    #[error("The depth map does not match its stated size.")]
    BadDepthMap,
    #[error("The depth map contains values that are not numbers.")]
    NotFinite,
}

/// Per-pixel nearness: 0 is the farthest point of the scene, 1 the nearest.
#[derive(Clone, Debug, PartialEq)]
pub struct DepthMap {
    width: u32,
    height: u32,
    values: Vec<f32>,
}

impl DepthMap {
    /// Build a map from raw model output in which larger means nearer,
    /// rescaled so the scene spans 0..=1. A flat scene becomes all zeros.
    pub fn normalized(width: u32, height: u32, raw: Vec<f32>) -> Result<DepthMap, DibrError> {
        if width == 0 || height == 0 {
            return Err(DibrError::Empty);
        }
        if raw.len() != width as usize * height as usize {
            return Err(DibrError::BadDepthMap);
        }
        if raw.iter().any(|v| !v.is_finite()) {
            return Err(DibrError::NotFinite);
        }
        let (min, max) = raw
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), &v| {
                (lo.min(v), hi.max(v))
            });
        let span = max - min;
        let values = if span > f32::EPSILON {
            raw.into_iter().map(|v| (v - min) / span).collect()
        } else {
            vec![0.0; raw.len()]
        };
        Ok(DepthMap {
            width,
            height,
            values,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn get(&self, x: u32, y: u32) -> f32 {
        self.values[y as usize * self.width as usize + x as usize]
    }

    /// The map resampled bilinearly to `width` x `height` (pixel centres
    /// aligned), as needed when the model ran at a lower resolution than the
    /// image.
    pub fn resized(&self, width: u32, height: u32) -> Result<DepthMap, DibrError> {
        if width == 0 || height == 0 {
            return Err(DibrError::Empty);
        }
        if (width, height) == (self.width, self.height) {
            return Ok(self.clone());
        }
        let scale_x = self.width as f32 / width as f32;
        let scale_y = self.height as f32 / height as f32;
        let mut values = Vec::with_capacity(width as usize * height as usize);
        for y in 0..height {
            let fy = ((y as f32 + 0.5) * scale_y - 0.5).clamp(0.0, (self.height - 1) as f32);
            let y0 = fy.floor() as u32;
            let y1 = (y0 + 1).min(self.height - 1);
            let ty = fy - y0 as f32;
            for x in 0..width {
                let fx = ((x as f32 + 0.5) * scale_x - 0.5).clamp(0.0, (self.width - 1) as f32);
                let x0 = fx.floor() as u32;
                let x1 = (x0 + 1).min(self.width - 1);
                let tx = fx - x0 as f32;
                let top = self.get(x0, y0) * (1.0 - tx) + self.get(x1, y0) * tx;
                let bottom = self.get(x0, y1) * (1.0 - tx) + self.get(x1, y1) * tx;
                values.push(top * (1.0 - ty) + bottom * ty);
            }
        }
        Ok(DepthMap {
            width,
            height,
            values,
        })
    }
}

/// The two controls of the conversion, with the ranges the Windows edition
/// uses for Samsung's engine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    /// Depth strength, 0.0..=2.0: how far apart near and far are pushed.
    pub strength: f64,
    /// Pop-out, -1.0..=1.0: where the screen plane sits in the scene. At -1
    /// the nearest point is on the screen and everything else behind it; at
    /// 1 the farthest point is on the screen and everything comes forward.
    pub popout: f64,
}

impl Default for Params {
    fn default() -> Params {
        Params {
            strength: 0.9,
            popout: 0.0,
        }
    }
}

impl Params {
    /// Signed disparity in pixels for nearness `depth` in an image `width`
    /// pixels wide. Positive is in front of the screen (the left eye sees the
    /// point further right than the right eye does), negative behind it.
    pub fn disparity(&self, depth: f32, width: u32) -> f32 {
        let strength = self.strength.clamp(0.0, 2.0) as f32;
        let popout = self.popout.clamp(-1.0, 1.0) as f32;
        // Nearness that lies on the screen plane.
        let screen = 0.5 - popout * 0.5;
        strength * MAX_DISPARITY_FRACTION * width as f32 * (depth - screen)
    }
}

/// Render the left and right views of `image` and return them side by side,
/// left eye first, each at the source size (so the result is twice as wide).
pub fn synthesize(
    image: &RgbaImage,
    depth: &DepthMap,
    params: Params,
) -> Result<RgbaImage, DibrError> {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return Err(DibrError::Empty);
    }
    let depth = depth.resized(width, height)?;
    let mut out = RgbaImage::new(width * 2, height);
    let w = width as usize;
    let mut colors = vec![[0u8; 4]; w];
    let mut nearness = vec![f32::NEG_INFINITY; w];
    for y in 0..height {
        // Left eye: near points move right. Right eye: near points move left.
        for (eye, direction) in [(0u32, 0.5f32), (1, -0.5)] {
            nearness.fill(f32::NEG_INFINITY);
            for x in 0..width {
                let d = depth.get(x, y);
                let target = (x as f32 + direction * params.disparity(d, width)).round();
                if target < 0.0 || target >= width as f32 {
                    continue;
                }
                let t = target as usize;
                if d > nearness[t] {
                    nearness[t] = d;
                    colors[t] = image.get_pixel(x, y).0;
                }
            }
            fill_gaps(&mut colors, &mut nearness);
            for (x, color) in colors.iter().enumerate() {
                out.put_pixel(eye * width + x as u32, y, image::Rgba(*color));
            }
        }
    }
    Ok(out)
}

/// Fill the pixels nothing was warped onto. A gap opens where a near object
/// has moved off the background behind it, so it is filled from whichever
/// neighbour is farther away; a gap at the edge of the row takes the only
/// neighbour it has. A row with nothing in it at all is left as it is.
fn fill_gaps(colors: &mut [[u8; 4]], nearness: &mut [f32]) {
    let len = colors.len();
    let mut x = 0;
    while x < len {
        if nearness[x] != f32::NEG_INFINITY {
            x += 1;
            continue;
        }
        let start = x;
        while x < len && nearness[x] == f32::NEG_INFINITY {
            x += 1;
        }
        let left = start.checked_sub(1);
        let right = (x < len).then_some(x);
        let source = match (left, right) {
            (Some(l), Some(r)) => {
                if nearness[l] <= nearness[r] {
                    l
                } else {
                    r
                }
            }
            (Some(l), None) => l,
            (None, Some(r)) => r,
            (None, None) => return,
        };
        let (color, depth) = (colors[source], nearness[source]);
        for i in start..x {
            colors[i] = color;
            nearness[i] = depth;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    const BACK: Rgba<u8> = Rgba([10, 20, 30, 255]);
    const FRONT: Rgba<u8> = Rgba([200, 100, 50, 255]);

    /// A 100-pixel-wide row image with a near object in columns 40..60.
    fn scene() -> (RgbaImage, DepthMap) {
        let image = RgbaImage::from_fn(
            100,
            2,
            |x, _| {
                if (40..60).contains(&x) {
                    FRONT
                } else {
                    BACK
                }
            },
        );
        let raw = (0..200)
            .map(|i| {
                if (40..60).contains(&(i % 100)) {
                    9.0
                } else {
                    3.0
                }
            })
            .collect();
        (image, DepthMap::normalized(100, 2, raw).unwrap())
    }

    fn front_span(pair: &RgbaImage, eye: u32) -> (u32, u32) {
        let columns: Vec<u32> = (0..100)
            .filter(|&x| *pair.get_pixel(eye * 100 + x, 0) == FRONT)
            .collect();
        (*columns.first().unwrap(), *columns.last().unwrap())
    }

    #[test]
    fn model_output_is_rescaled_and_bad_maps_are_refused() {
        let map = DepthMap::normalized(2, 2, vec![2.0, 4.0, 6.0, 4.0]).unwrap();
        assert_eq!(
            [map.get(0, 0), map.get(1, 0), map.get(0, 1), map.get(1, 1)],
            [0.0, 0.5, 1.0, 0.5]
        );
        // A scene with no depth at all is flat, not a division by zero.
        let flat = DepthMap::normalized(2, 1, vec![7.0, 7.0]).unwrap();
        assert_eq!([flat.get(0, 0), flat.get(1, 0)], [0.0, 0.0]);
        assert_eq!(
            DepthMap::normalized(2, 2, vec![1.0; 3]),
            Err(DibrError::BadDepthMap)
        );
        assert_eq!(
            DepthMap::normalized(1, 2, vec![1.0, f32::NAN]),
            Err(DibrError::NotFinite)
        );
        assert_eq!(DepthMap::normalized(0, 2, vec![]), Err(DibrError::Empty));
    }

    #[test]
    fn resizing_interpolates_between_pixel_centres() {
        let map = DepthMap::normalized(2, 1, vec![0.0, 1.0]).unwrap();
        let wide = map.resized(4, 1).unwrap();
        assert_eq!((wide.width(), wide.height()), (4, 1));
        let row: Vec<f32> = (0..4).map(|x| wide.get(x, 0)).collect();
        assert_eq!(row, vec![0.0, 0.25, 0.75, 1.0]);
        assert_eq!(map.resized(2, 1).unwrap(), map);
        assert_eq!(map.resized(0, 1), Err(DibrError::Empty));
    }

    #[test]
    fn disparity_follows_strength_and_the_screen_plane_follows_popout() {
        let p = |strength, popout| Params { strength, popout };
        // Width 1000, strength 1: 30 px between nearest and farthest.
        assert_eq!(p(1.0, 0.0).disparity(1.0, 1000), 15.0);
        assert_eq!(p(1.0, 0.0).disparity(0.0, 1000), -15.0);
        assert_eq!(p(1.0, 0.0).disparity(0.5, 1000), 0.0);
        assert_eq!(p(2.0, 0.0).disparity(1.0, 1000), 30.0);
        assert_eq!(p(0.0, 0.0).disparity(1.0, 1000), 0.0);
        // Pop-out -1: nothing in front of the screen. Pop-out 1: nothing behind.
        assert_eq!(p(1.0, -1.0).disparity(1.0, 1000), 0.0);
        assert_eq!(p(1.0, -1.0).disparity(0.0, 1000), -30.0);
        assert_eq!(p(1.0, 1.0).disparity(0.0, 1000), 0.0);
        assert_eq!(p(1.0, 1.0).disparity(1.0, 1000), 30.0);
        // Out-of-range settings are clamped, not extrapolated.
        assert_eq!(p(9.0, 5.0).disparity(1.0, 1000), 60.0);
    }

    #[test]
    fn a_near_object_shifts_opposite_ways_in_the_two_eyes() {
        let (image, depth) = scene();
        // Strength 2, screen plane mid-scene: the object is 3 px in front
        // (disparity +3 of a 100 px width... 0.03 * 100 * 2 * 0.5).
        let params = Params {
            strength: 2.0,
            popout: 0.0,
        };
        assert_eq!(params.disparity(1.0, 100), 3.0);
        let pair = synthesize(&image, &depth, params).unwrap();
        assert_eq!(pair.dimensions(), (200, 2));
        // Half the disparity each way, rounded to whole pixels: the object
        // lands further right in the left eye than in the right eye by the
        // full disparity, and keeps its width in both.
        let (l0, l1) = front_span(&pair, 0);
        let (r0, r1) = front_span(&pair, 1);
        assert!(l0 > 40 && r0 < 40, "left {l0}..{l1}, right {r0}..{r1}");
        assert_eq!(l1 - l0, 19);
        assert_eq!(r1 - r0, 19);
        assert_eq!(l0 - r0, 3);
    }

    #[test]
    fn gaps_are_filled_with_background_and_nothing_is_left_transparent() {
        let (image, depth) = scene();
        let pair = synthesize(
            &image,
            &depth,
            Params {
                strength: 2.0,
                popout: 0.0,
            },
        )
        .unwrap();
        for pixel in pair.pixels() {
            assert!(
                *pixel == BACK || *pixel == FRONT,
                "unexpected pixel {pixel:?}"
            );
        }
        // The object did not smear: exactly its 20 columns in each eye.
        for eye in 0..2 {
            let count = (0..100)
                .filter(|&x| *pair.get_pixel(eye * 100 + x, 1) == FRONT)
                .count();
            assert_eq!(count, 20, "eye {eye}");
        }
    }

    #[test]
    fn zero_strength_reproduces_the_image_in_both_eyes() {
        let (image, depth) = scene();
        let pair = synthesize(
            &image,
            &depth,
            Params {
                strength: 0.0,
                popout: 0.7,
            },
        )
        .unwrap();
        for (x, y, pixel) in image.enumerate_pixels() {
            assert_eq!(pair.get_pixel(x, y), pixel);
            assert_eq!(pair.get_pixel(100 + x, y), pixel);
        }
    }

    #[test]
    fn a_low_resolution_depth_map_is_stretched_to_the_image() {
        let image = RgbaImage::from_pixel(8, 4, BACK);
        let depth = DepthMap::normalized(2, 1, vec![0.0, 1.0]).unwrap();
        let pair = synthesize(&image, &depth, Params::default()).unwrap();
        assert_eq!(pair.dimensions(), (16, 4));
        assert_eq!(
            synthesize(&RgbaImage::new(0, 0), &depth, Params::default()),
            Err(DibrError::Empty)
        );
    }

    #[test]
    fn gap_filling_prefers_the_farther_neighbour_and_handles_the_edges() {
        let none = f32::NEG_INFINITY;
        let a = [1, 1, 1, 255];
        let b = [2, 2, 2, 255];
        // Gap between a far pixel (left) and a near pixel (right).
        let mut colors = vec![a, [0; 4], [0; 4], b];
        let mut nearness = vec![0.1, none, none, 0.9];
        fill_gaps(&mut colors, &mut nearness);
        assert_eq!(colors, vec![a, a, a, b]);
        // Gaps at both edges take their only neighbour.
        let mut colors = vec![[0; 4], a, b, [0; 4]];
        let mut nearness = vec![none, 0.5, 0.2, none];
        fill_gaps(&mut colors, &mut nearness);
        assert_eq!(colors, vec![a, a, b, b]);
        // Nothing to copy from: untouched.
        let mut colors = vec![[0; 4]; 3];
        let mut nearness = vec![none; 3];
        fill_gaps(&mut colors, &mut nearness);
        assert_eq!(colors, vec![[0; 4]; 3]);
    }
}
