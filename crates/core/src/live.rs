//! The per-frame work of live 2D → 3D: shrink a captured frame, keep its
//! depth steady from one frame to the next, and render the two views fast
//! enough for video.
//!
//! [`crate::dibr`] does the same rendering for one still image, a pixel at a
//! time through the image types. Here everything works on plain byte rows,
//! in bands spread over the processor's cores, because it runs for every
//! frame. No capture and no model here: frames and depth come from whoever
//! has them.

use crate::dibr::Params;

/// The order of the colour bytes in a captured pixel (the fourth byte is
/// alpha or padding, and is ignored).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelOrder {
    Rgb,
    Bgr,
}

/// A captured frame as the capture source delivers it.
#[derive(Clone, Copy, Debug)]
pub struct Source<'a> {
    pub bytes: &'a [u8],
    pub width: u32,
    pub height: u32,
    /// Bytes from one row to the next.
    pub stride: usize,
    pub order: PixelOrder,
}

impl Source<'_> {
    /// Whether the rows the header describes are all there.
    pub fn is_complete(&self) -> bool {
        let (w, h) = (self.width as usize, self.height as usize);
        w > 0 && h > 0 && self.stride >= w * 4 && self.bytes.len() >= self.stride * (h - 1) + w * 4
    }
}

/// The threads a frame's rows are spread over: a quarter of the cores, at
/// most six, and two wherever there are two cores at all (a quarter of four
/// or six cores would be one thread, and the picture late for it). The work
/// is light and the cores have other things to do while a picture is live
/// (the depth model, the eye tracker, the desktop).
fn threads_for(rows: usize) -> usize {
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    threads_on(cores).min(rows.max(1))
}

fn threads_on(cores: usize) -> usize {
    (cores / 4).clamp(cores.clamp(1, 2), 6)
}

/// Run `work(first_row, rows)` over `out` cut into bands of whole rows of
/// `row_bytes` each, one band per thread.
fn in_bands(out: &mut [u8], row_bytes: usize, work: impl Fn(usize, &mut [u8]) + Sync) {
    if row_bytes == 0 || out.is_empty() {
        return;
    }
    in_bands_on(threads_for(out.len() / row_bytes), out, row_bytes, work);
}

/// [`in_bands`] over a stated number of threads. The calling thread works
/// the first band itself: one thread fewer to start for every stage of
/// every frame, and none at all for a single band.
fn in_bands_on(
    threads: usize,
    out: &mut [u8],
    row_bytes: usize,
    work: impl Fn(usize, &mut [u8]) + Sync,
) {
    let rows = out.len() / row_bytes;
    if threads <= 1 || rows <= 1 {
        work(0, out);
        return;
    }
    let band = rows.div_ceil(threads);
    let (first, rest) = out.split_at_mut((band * row_bytes).min(out.len()));
    std::thread::scope(|scope| {
        for (index, part) in rest.chunks_mut(band * row_bytes).enumerate() {
            let work = &work;
            scope.spawn(move || work((index + 1) * band, part));
        }
        work(0, first);
    });
}

/// Scale `source` to `width` x `height` RGBA (alpha 255) in `out`. Each
/// output pixel is the mean of up to 2 x 2 source pixels spread over the
/// area it covers: cheap, and enough against the shimmer plain point
/// sampling gives fine text when a desktop is shrunk. False (and `out`
/// untouched) when the sizes do not fit.
pub fn scale_to_rgba(source: Source<'_>, out: &mut [u8], width: u32, height: u32) -> bool {
    let (w, h) = (width as usize, height as usize);
    if !source.is_complete() || w == 0 || h == 0 || out.len() != w * h * 4 {
        return false;
    }
    let (sw, sh) = (source.width as usize, source.height as usize);
    let (r, b) = match source.order {
        PixelOrder::Rgb => (0, 2),
        PixelOrder::Bgr => (2, 0),
    };
    // At native size all four samples below would be the same pixel.
    // Copy/reorder once instead, keeping padding out and alpha opaque.
    if (w, h) == (sw, sh) {
        in_bands(out, w * 4, |first_row, band| {
            for (row, line) in band.chunks_exact_mut(w * 4).enumerate() {
                let start = (first_row + row) * source.stride;
                let input = &source.bytes[start..start + w * 4];
                for (dst, src) in line
                    .as_chunks_mut::<4>()
                    .0
                    .iter_mut()
                    .zip(input.as_chunks::<4>().0)
                {
                    *dst = [src[r], src[1], src[b], 255];
                }
            }
        });
        return true;
    }
    // Two sample columns per output column, at 1/4 and 3/4 of its span.
    let columns: Vec<(usize, usize)> = (0..w)
        .map(|x| {
            let at = |quarter: usize| (((x * 4 + quarter) * sw) / (w * 4)).min(sw - 1) * 4;
            (at(1), at(3))
        })
        .collect();
    in_bands(out, w * 4, |first_row, band| {
        for (row, line) in band.chunks_exact_mut(w * 4).enumerate() {
            let y = first_row + row;
            let at =
                |quarter: usize| (((y * 4 + quarter) * sh) / (h * 4)).min(sh - 1) * source.stride;
            let (top, bottom) = (&source.bytes[at(1)..], &source.bytes[at(3)..]);
            for (pixel, &(x0, x1)) in line.as_chunks_mut::<4>().0.iter_mut().zip(&columns) {
                let mean = |c: usize| {
                    ((top[x0 + c] as u32
                        + top[x1 + c] as u32
                        + bottom[x0 + c] as u32
                        + bottom[x1 + c] as u32
                        + 2)
                        / 4) as u8
                };
                pixel[0] = mean(r);
                pixel[1] = mean(1);
                pixel[2] = mean(b);
                pixel[3] = 255;
            }
        }
    });
    true
}

/// Depth that stays put between frames.
///
/// The model sees every frame afresh: its scale drifts and its surfaces
/// tremble, which the eyes read as the whole scene breathing. This keeps a
/// running range to normalize with and blends each new map into the last.
#[derive(Clone, Debug, Default)]
pub struct DepthSmoother {
    width: u32,
    height: u32,
    values: Vec<f32>,
    range: Option<(f32, f32)>,
}

impl DepthSmoother {
    /// Take the model's raw output (larger is nearer) for a new frame.
    /// `steadiness` is how much of the previous map is kept, 0.0 (none)
    /// to 0.95. Returns false for a map that cannot be used; the previous
    /// one stays.
    pub fn update(&mut self, width: u32, height: u32, raw: &[f32], steadiness: f32) -> bool {
        let count = width as usize * height as usize;
        if count == 0 || raw.len() != count {
            return false;
        }
        let (mut low, mut high) = (f32::INFINITY, f32::NEG_INFINITY);
        for &v in raw {
            if !v.is_finite() {
                return false;
            }
            low = low.min(v);
            high = high.max(v);
        }
        let keep = steadiness.clamp(0.0, 0.95);
        let fresh = self.width != width || self.height != height || self.values.len() != count;
        // The range follows the scene more slowly than the map does, so a
        // near object entering does not push everything else back at once.
        let (low, high) = match self.range {
            Some((l, h)) if !fresh => {
                let slow = (keep + 0.5).min(0.97);
                (
                    l * slow + low * (1.0 - slow),
                    h * slow + high * (1.0 - slow),
                )
            }
            _ => (low, high),
        };
        self.range = Some((low, high));
        let span = high - low;
        let scale = if span > f32::EPSILON { 1.0 / span } else { 0.0 };
        if fresh {
            self.width = width;
            self.height = height;
            self.values = raw
                .iter()
                .map(|&v| ((v - low) * scale).clamp(0.0, 1.0))
                .collect();
        } else {
            for (old, &v) in self.values.iter_mut().zip(raw) {
                let new = ((v - low) * scale).clamp(0.0, 1.0);
                *old = *old * keep + new * (1.0 - keep);
            }
        }
        true
    }

    /// Nearness per pixel, 0 (far) to 1 (near), row by row; empty before
    /// the first update.
    pub fn values(&self) -> &[f32] {
        &self.values
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// A depth map to render with: nearness 0..=1, `width` x `height`.
#[derive(Clone, Copy, Debug)]
pub struct Depth<'a> {
    pub values: &'a [f32],
    pub width: u32,
    pub height: u32,
}

/// Render the left and right views of the RGBA image `source` (`width` x
/// `height`) into `out`, side by side (`2 * width` x `height`, RGBA): the
/// same picture [`crate::dibr::synthesize`] makes, band by band. Without a
/// usable depth map both views are the source. False when the sizes do not
/// fit.
pub fn render_pair(
    source: &[u8],
    width: u32,
    height: u32,
    depth: Option<Depth<'_>>,
    params: Params,
    out: &mut [u8],
) -> bool {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || source.len() != w * h * 4 || out.len() != w * h * 8 {
        return false;
    }
    let depth = depth.filter(|d| {
        d.width > 0 && d.height > 0 && d.values.len() == d.width as usize * d.height as usize
    });
    let Some(depth) = depth else {
        for (row, line) in source.chunks_exact(w * 4).zip(out.chunks_exact_mut(w * 8)) {
            line[..w * 4].copy_from_slice(row);
            line[w * 4..].copy_from_slice(row);
        }
        return true;
    };
    let (dw, dh) = (depth.width as usize, depth.height as usize);
    // Where each column falls in the depth map, as index and blend weight.
    let columns: Vec<(usize, usize, f32)> = (0..w)
        .map(|x| {
            let at = ((x as f32 + 0.5) * dw as f32 / w as f32 - 0.5).clamp(0.0, (dw - 1) as f32);
            let left = at.floor() as usize;
            (left, (left + 1).min(dw - 1), at - left as f32)
        })
        .collect();
    in_bands(out, w * 8, |first_row, band| {
        let mut nearness = vec![0.0f32; w];
        let mut placed = vec![f32::NEG_INFINITY; w];
        for (row, line) in band.chunks_exact_mut(w * 8).enumerate() {
            let y = first_row + row;
            let at = ((y as f32 + 0.5) * dh as f32 / h as f32 - 0.5).clamp(0.0, (dh - 1) as f32);
            let top = at.floor() as usize;
            let bottom = (top + 1).min(dh - 1);
            let blend = at - top as f32;
            let (upper, lower) = (&depth.values[top * dw..], &depth.values[bottom * dw..]);
            for (near, &(l, r, t)) in nearness.iter_mut().zip(&columns) {
                let a = upper[l] + (upper[r] - upper[l]) * t;
                let b = lower[l] + (lower[r] - lower[l]) * t;
                *near = a + (b - a) * blend;
            }
            let pixels = &source[y * w * 4..(y + 1) * w * 4];
            let (left, right) = line.split_at_mut(w * 4);
            // Left eye: near points move right. Right eye: near points move left.
            for (view, direction) in [(left, 0.5f32), (right, -0.5f32)] {
                placed.fill(f32::NEG_INFINITY);
                for x in 0..w {
                    let near = nearness[x];
                    let target = (x as f32 + direction * params.disparity(near, width)).round();
                    if target < 0.0 || target >= w as f32 {
                        continue;
                    }
                    let t = target as usize;
                    if near > placed[t] {
                        placed[t] = near;
                        view[t * 4..t * 4 + 4].copy_from_slice(&pixels[x * 4..x * 4 + 4]);
                    }
                }
                fill_gaps(view, &mut placed);
            }
        }
    });
    true
}

/// Fill the pixels nothing was moved onto, each run from whichever
/// neighbour is farther away (a near object uncovers background).
fn fill_gaps(view: &mut [u8], placed: &mut [f32]) {
    let len = placed.len();
    let mut x = 0;
    while x < len {
        if placed[x] != f32::NEG_INFINITY {
            x += 1;
            continue;
        }
        let start = x;
        while x < len && placed[x] == f32::NEG_INFINITY {
            x += 1;
        }
        let source = match (start.checked_sub(1), (x < len).then_some(x)) {
            (Some(l), Some(r)) => {
                if placed[l] <= placed[r] {
                    l
                } else {
                    r
                }
            }
            (Some(l), None) => l,
            (None, Some(r)) => r,
            (None, None) => {
                // Nothing landed in this row at all: black, not stale bytes.
                view.fill(0);
                return;
            }
        };
        let color = [
            view[source * 4],
            view[source * 4 + 1],
            view[source * 4 + 2],
            view[source * 4 + 3],
        ];
        let near = placed[source];
        for i in start..x {
            view[i * 4..i * 4 + 4].copy_from_slice(&color);
            placed[i] = near;
        }
    }
}

/// Sharpen the RGBA picture `image` (`width` pixels wide) in place by
/// `amount` (0.0 leaves it, 1.0 is strong): each pixel is pushed away from
/// the mean of its four neighbours. `scratch` is working room the caller
/// keeps between frames.
pub fn sharpen(image: &mut [u8], width: u32, amount: f32, scratch: &mut Vec<u8>) {
    let w = width as usize;
    if amount <= 0.0 || w < 3 || image.len() < w * 4 * 3 || !image.len().is_multiple_of(w * 4) {
        return;
    }
    scratch.clear();
    scratch.extend_from_slice(image);
    let before: &[u8] = scratch;
    let row = w * 4;
    let rows = image.len() / row;
    // Fixed point: 256 is 1.0.
    let k = (amount.min(2.0) * 256.0) as i32;
    in_bands(image, row, |first_row, band| {
        for (index, line) in band.chunks_exact_mut(row).enumerate() {
            let y = first_row + index;
            if y == 0 || y + 1 >= rows {
                continue;
            }
            let (above, here, below) = (
                &before[(y - 1) * row..y * row],
                &before[y * row..(y + 1) * row],
                &before[(y + 1) * row..(y + 2) * row],
            );
            for x in 1..w - 1 {
                for c in 0..3 {
                    let i = x * 4 + c;
                    let centre = here[i] as i32;
                    let around =
                        above[i] as i32 + below[i] as i32 + here[i - 4] as i32 + here[i + 4] as i32;
                    let value = centre + (((centre * 4 - around) * k) >> 10);
                    line[i] = value.clamp(0, 255) as u8;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dibr::{self, DepthMap};
    use image::RgbaImage;

    #[test]
    fn native_size_conversion_preserves_pixels_and_ignores_row_padding() {
        for (w, h) in [(1, 1), (3, 7), (64, 40)] {
            let stride = w * 4 + 13;
            // The last row need not have trailing padding.
            let bytes: Vec<u8> = (0..stride * (h - 1) + w * 4)
                .map(|i| (i % 251) as u8)
                .collect();
            for order in [PixelOrder::Rgb, PixelOrder::Bgr] {
                let source = Source {
                    bytes: &bytes,
                    width: w as u32,
                    height: h as u32,
                    stride,
                    order,
                };
                let mut out = vec![0; w * h * 4];
                assert!(scale_to_rgba(source, &mut out, w as u32, h as u32));
                for y in 0..h {
                    for x in 0..w {
                        let p = &bytes[y * stride + x * 4..][..4];
                        let expected = match order {
                            PixelOrder::Rgb => [p[0], p[1], p[2], 255],
                            PixelOrder::Bgr => [p[2], p[1], p[0], 255],
                        };
                        assert_eq!(&out[(y * w + x) * 4..][..4], &expected);
                    }
                }
                // Invalid input is rejected before any output is changed.
                let short = Source {
                    bytes: &bytes[..bytes.len() - 1],
                    ..source
                };
                let previous = out.clone();
                assert!(!scale_to_rgba(short, &mut out, w as u32, h as u32));
                assert_eq!(out, previous);
            }
        }
    }

    #[test]
    fn a_frame_is_shrunk_by_averaging_and_its_colours_put_in_order() {
        // 4x2 BGRx with a stride wider than the row; shrink to 2x1.
        let stride = 20;
        let mut bytes = vec![0u8; stride * 2];
        for y in 0..2 {
            for x in 0..4 {
                let p = y * stride + x * 4;
                bytes[p] = (x * 10) as u8; // blue
                bytes[p + 1] = 100; // green
                bytes[p + 2] = if x < 2 { 200 } else { 0 }; // red
                bytes[p + 3] = 7; // padding
            }
        }
        let source = Source {
            bytes: &bytes,
            width: 4,
            height: 2,
            stride,
            order: PixelOrder::Bgr,
        };
        let mut out = vec![0u8; 8];
        assert!(scale_to_rgba(source, &mut out, 2, 1));
        // Left pixel: columns 0 and 1; right pixel: columns 2 and 3.
        assert_eq!(out, [200, 100, 5, 255, 0, 100, 25, 255]);
        // The same bytes read as RGBx swap red and blue.
        let rgb = Source {
            order: PixelOrder::Rgb,
            ..source
        };
        assert!(scale_to_rgba(rgb, &mut out, 2, 1));
        assert_eq!(out[..4], [5, 100, 200, 255]);
        // Sizes that do not fit are refused.
        assert!(!scale_to_rgba(source, &mut out, 3, 1));
        let short = Source {
            bytes: &bytes[..10],
            ..source
        };
        assert!(!scale_to_rgba(short, &mut out, 2, 1));
    }

    #[test]
    fn enlarging_repeats_pixels_and_stays_inside_the_frame() {
        let bytes = [1, 2, 3, 0, 4, 5, 6, 0];
        let source = Source {
            bytes: &bytes,
            width: 2,
            height: 1,
            stride: 8,
            order: PixelOrder::Rgb,
        };
        let mut out = vec![0u8; 4 * 4 * 2];
        assert!(scale_to_rgba(source, &mut out, 4, 2));
        assert_eq!(out[..4], [1, 2, 3, 255]);
        assert_eq!(out[12..16], [4, 5, 6, 255]);
        assert_eq!(out[16..20], [1, 2, 3, 255]);
    }

    #[test]
    fn the_live_renderer_makes_the_same_pair_as_the_still_renderer() {
        // A textured image with a near block, wide enough for several bands.
        let (w, h) = (96u32, 40u32);
        let image = RgbaImage::from_fn(w, h, |x, y| {
            image::Rgba([(x * 2) as u8, (y * 5) as u8, ((x + y) % 251) as u8, 255])
        });
        let raw: Vec<f32> = (0..w * h)
            .map(|i| {
                let (x, y) = (i % w, i / w);
                if (30..60).contains(&x) && (10..30).contains(&y) {
                    8.0
                } else {
                    1.0 + x as f32 / w as f32
                }
            })
            .collect();
        let map = DepthMap::normalized(w, h, raw).unwrap();
        let values: Vec<f32> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .map(|(x, y)| map.get(x, y))
            .collect();
        for params in [
            Params::default(),
            Params {
                strength: 2.0,
                popout: 1.0,
            },
            Params {
                strength: 1.3,
                popout: -1.0,
            },
        ] {
            let still = dibr::synthesize(&image, &map, params).unwrap();
            let mut live = vec![0u8; (w * h * 8) as usize];
            let depth = Depth {
                values: &values,
                width: w,
                height: h,
            };
            assert!(render_pair(
                image.as_raw(),
                w,
                h,
                Some(depth),
                params,
                &mut live
            ));
            assert_eq!(live, *still.as_raw(), "{params:?}");
        }
    }

    #[test]
    fn without_depth_both_views_are_the_source_and_bad_sizes_are_refused() {
        let source = [1u8, 2, 3, 255, 4, 5, 6, 255];
        let mut out = [0u8; 16];
        assert!(render_pair(
            &source,
            2,
            1,
            None,
            Params::default(),
            &mut out
        ));
        assert_eq!(out[..8], source);
        assert_eq!(out[8..], source);
        // A depth map that does not match its stated size counts as none.
        let bad = Depth {
            values: &[0.5],
            width: 2,
            height: 2,
        };
        out.fill(0);
        assert!(render_pair(
            &source,
            2,
            1,
            Some(bad),
            Params::default(),
            &mut out
        ));
        assert_eq!(out[8..], source);
        assert!(!render_pair(
            &source,
            2,
            1,
            None,
            Params::default(),
            &mut out[..8]
        ));
        assert!(!render_pair(
            &source[..4],
            2,
            1,
            None,
            Params::default(),
            &mut out
        ));
    }

    #[test]
    fn depth_is_normalized_blended_and_keeps_its_range_steady() {
        let mut smoother = DepthSmoother::default();
        assert!(smoother.values().is_empty());
        assert!(smoother.update(2, 1, &[2.0, 6.0], 0.5));
        assert_eq!(smoother.values(), [0.0, 1.0]);
        assert_eq!(smoother.size(), (2, 1));
        // The same scene again changes nothing.
        assert!(smoother.update(2, 1, &[2.0, 6.0], 0.5));
        assert_eq!(smoother.values(), [0.0, 1.0]);
        // The scene flips: the map moves halfway, not all the way.
        assert!(smoother.update(2, 1, &[6.0, 2.0], 0.5));
        let values = smoother.values();
        assert!((values[0] - 0.5).abs() < 1e-6 && (values[1] - 0.5).abs() < 1e-6);
        // Unusable maps are refused and leave the last one in place.
        assert!(!smoother.update(2, 1, &[1.0], 0.5));
        assert!(!smoother.update(2, 1, &[1.0, f32::NAN], 0.5));
        assert!(!smoother.update(0, 0, &[], 0.5));
        assert_eq!(smoother.values().len(), 2);
        // A new size starts afresh; a flat scene is all far.
        assert!(smoother.update(1, 3, &[4.0, 4.0, 4.0], 0.9));
        assert_eq!(smoother.values(), [0.0, 0.0, 0.0]);
    }

    #[test]
    fn two_cores_already_share_the_rows_and_six_threads_are_the_most() {
        let threads: Vec<usize> = [1, 2, 3, 4, 7, 8, 12, 16, 24, 64]
            .into_iter()
            .map(threads_on)
            .collect();
        assert_eq!(threads, [1, 2, 2, 2, 2, 2, 3, 4, 6, 6]);
    }

    #[test]
    fn bands_cover_every_row_once_whatever_the_number_of_threads() {
        let row_bytes = 3;
        for rows in [1usize, 2, 3, 5, 6, 7, 40] {
            let expected: Vec<u8> = (0..rows)
                .flat_map(|row| [row as u8 + 1, row as u8 + 2, row as u8 + 3])
                .collect();
            for threads in 1..=8 {
                // Every byte has its row's number added, so a row worked
                // twice or not at all shows.
                let mut out = vec![0u8; rows * row_bytes];
                in_bands_on(threads, &mut out, row_bytes, |first_row, band| {
                    for (row, line) in band.chunks_exact_mut(row_bytes).enumerate() {
                        for (i, byte) in line.iter_mut().enumerate() {
                            *byte += (first_row + row + i) as u8 + 1;
                        }
                    }
                });
                assert_eq!(out, expected, "{rows} rows on {threads} threads");
            }
        }
    }

    #[test]
    fn sharpening_raises_contrast_at_an_edge_and_leaves_flat_areas_and_the_border() {
        // 5x3: a bright column in the middle.
        let (w, h) = (5usize, 3usize);
        let mut image = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let v = if x == 2 { 200 } else { 100 };
                image[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        let before = image.clone();
        let mut scratch = Vec::new();
        sharpen(&mut image, w as u32, 0.0, &mut scratch);
        assert_eq!(image, before, "amount 0 changes nothing");
        sharpen(&mut image, w as u32, 1.0, &mut scratch);
        let at = |x: usize, y: usize| image[(y * w + x) * 4];
        assert!(at(2, 1) > 200, "the bright column gets brighter");
        assert!(at(1, 1) < 100, "its neighbour gets darker");
        assert_eq!(at(0, 1), 100, "the border column is left");
        assert_eq!(at(2, 0), 200, "the border row is left");
        assert_eq!(image[(w + 2) * 4 + 3], 255, "alpha is left");
    }
}
