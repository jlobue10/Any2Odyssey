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
    // Asked once: on Linux the answer is put together from the scheduler
    // and several files of the control groups at every call, and this is
    // called for each stage of each frame.
    static CORES: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let cores = *CORES.get_or_init(|| std::thread::available_parallelism().map_or(1, |n| n.get()));
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

/// How near to its end a depth map eased towards a still picture's own map
/// counts as there. At the widest view and the strongest depth this much
/// nearness moves a pixel by about a tenth of its width.
pub const SETTLED: f32 = 1.0 / 2048.0;

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

    /// Take the map of a picture that has not changed once more, in place
    /// of asking the model about the same pixels again: the same step
    /// [`DepthSmoother::update`] makes, so depth eases towards the still
    /// picture's own map as it would have. True once it has arrived and
    /// further steps would change nothing; the last of the way, too small
    /// to see ([`SETTLED`]), is taken at once.
    pub fn settle(&mut self, width: u32, height: u32, raw: &[f32], steadiness: f32) -> bool {
        if !self.update(width, height, raw, steadiness) {
            // A map that cannot be used never will be.
            return true;
        }
        let Some((low, high)) = self.range else {
            return true;
        };
        let scale = |low: f32, high: f32| {
            let span = high - low;
            if span > f32::EPSILON {
                1.0 / span
            } else {
                0.0
            }
        };
        // Where the steps lead: the map normalized with its own range.
        let (mut end_low, mut end_high) = (f32::INFINITY, f32::NEG_INFINITY);
        for &v in raw {
            end_low = end_low.min(v);
            end_high = end_high.max(v);
        }
        let (now, end) = (scale(low, high), scale(end_low, end_high));
        let at_end = |v: f32| ((v - end_low) * end).clamp(0.0, 1.0);
        // A flat map has no range to ease to (a range that only narrows
        // never counts as flat), so its end is taken at once. For any
        // other, both what is shown and what the next step would blend in
        // have to be there: the range moves more slowly than the values.
        if end > 0.0 {
            let mut off = 0.0f32;
            for (&value, &v) in self.values.iter().zip(raw) {
                let target = at_end(v);
                let next = ((v - low) * now).clamp(0.0, 1.0);
                off = off.max((value - target).abs()).max((next - target).abs());
            }
            if off > SETTLED {
                return false;
            }
        }
        self.range = Some((end_low, end_high));
        for (value, &v) in self.values.iter_mut().zip(raw) {
            *value = at_end(v);
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

/// Where the depth map is read for each pixel of a picture: the map
/// stretched over the picture, blended between its four nearest values.
struct DepthAt<'a> {
    depth: Depth<'a>,
    /// Where each column falls in the map, as index and blend weight.
    columns: Vec<(usize, usize, f32)>,
    height: usize,
}

impl<'a> DepthAt<'a> {
    /// `None` for a map that does not match its stated size.
    fn new(depth: Option<Depth<'a>>, width: usize, height: usize) -> Option<DepthAt<'a>> {
        let depth = depth.filter(|d| {
            d.width > 0 && d.height > 0 && d.values.len() == d.width as usize * d.height as usize
        })?;
        let dw = depth.width as usize;
        let columns = (0..width)
            .map(|x| {
                let at =
                    ((x as f32 + 0.5) * dw as f32 / width as f32 - 0.5).clamp(0.0, (dw - 1) as f32);
                let left = at.floor() as usize;
                (left, (left + 1).min(dw - 1), at - left as f32)
            })
            .collect();
        Some(DepthAt {
            depth,
            columns,
            height,
        })
    }

    /// The nearness of every pixel of row `y`.
    fn row(&self, y: usize, nearness: &mut [f32]) {
        let (dw, dh) = (self.depth.width as usize, self.depth.height as usize);
        let at =
            ((y as f32 + 0.5) * dh as f32 / self.height as f32 - 0.5).clamp(0.0, (dh - 1) as f32);
        let top = at.floor() as usize;
        let bottom = (top + 1).min(dh - 1);
        let blend = at - top as f32;
        let (upper, lower) = (
            &self.depth.values[top * dw..],
            &self.depth.values[bottom * dw..],
        );
        for (near, &(l, r, t)) in nearness.iter_mut().zip(&self.columns) {
            let a = upper[l] + (upper[r] - upper[l]) * t;
            let b = lower[l] + (lower[r] - lower[l]) * t;
            *near = a + (b - a) * blend;
        }
    }
}

/// One row of both views: the row's `pixels` moved sideways by their
/// `nearness`. `placed` is working room, as long as the row.
fn shift_row(
    pixels: &[u8],
    nearness: &[f32],
    placed: &mut [f32],
    params: Params,
    width: u32,
    left: &mut [u8],
    right: &mut [u8],
) {
    let w = nearness.len();
    // The same for every pixel of the row: `Params::disparity` in its parts.
    let (scale, screen) = params.disparity_terms(width);
    // Whole pixels, and rows cut to one length: nothing in the loop then
    // has to be checked against the end of a row.
    let pixels = &pixels.as_chunks::<4>().0[..w];
    let placed = &mut placed[..w];
    // Left eye: near points move right. Right eye: near points move left.
    for (view, direction) in [(left, 0.5f32), (right, -0.5f32)] {
        placed.fill(f32::NEG_INFINITY);
        let colours = &mut view.as_chunks_mut::<4>().0[..w];
        for (x, (&near, pixel)) in nearness.iter().zip(pixels).enumerate() {
            let moved = x as f32 + direction * (scale * (near - screen));
            let Some(t) = crate::dibr::column(moved, w) else {
                continue;
            };
            if near > placed[t] {
                placed[t] = near;
                colours[t] = *pixel;
            }
        }
        fill_gaps(view, placed);
    }
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
    let Some(depth) = DepthAt::new(depth, w, h) else {
        for (row, line) in source.chunks_exact(w * 4).zip(out.chunks_exact_mut(w * 8)) {
            line[..w * 4].copy_from_slice(row);
            line[w * 4..].copy_from_slice(row);
        }
        return true;
    };
    in_bands(out, w * 8, |first_row, band| {
        let mut nearness = vec![0.0f32; w];
        let mut placed = vec![f32::NEG_INFINITY; w];
        for (row, line) in band.chunks_exact_mut(w * 8).enumerate() {
            let y = first_row + row;
            depth.row(y, &mut nearness);
            let pixels = &source[y * w * 4..(y + 1) * w * 4];
            let (left, right) = line.split_at_mut(w * 4);
            shift_row(pixels, &nearness, &mut placed, params, width, left, right);
        }
    });
    true
}

/// Where a pair of views goes in the picture an output is handed: two
/// halves of `view` pixels side by side, RGBA, the bottom row first (the
/// weaver's order), each view with its top-left corner at (`x`, `y`) of its
/// half.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    pub view: (u32, u32),
    pub x: u32,
    pub y: u32,
    /// The right view goes into the left half and the left into the right.
    pub swap: bool,
}

impl Placement {
    /// Bytes of the whole picture.
    pub fn bytes(&self) -> usize {
        self.view.0 as usize * self.view.1 as usize * 8
    }

    /// The bytes of `canvas` the rows of a `width` x `height` view go to
    /// (they are next to one another, the view's last row first), or
    /// `None` when the view or the canvas does not fit.
    fn rows(&self, width: usize, height: usize, canvas: usize) -> Option<std::ops::Range<usize>> {
        let (vw, vh) = (self.view.0 as usize, self.view.1 as usize);
        let (x, y) = (self.x as usize, self.y as usize);
        if width == 0 || height == 0 || x + width > vw || y + height > vh || canvas != self.bytes()
        {
            return None;
        }
        let row = vw * 8;
        Some((vh - y - height) * row..(vh - y) * row)
    }

    /// Where the left and the right view's pixels go in one row of the
    /// picture.
    fn halves<'a>(&self, line: &'a mut [u8], width: usize) -> (&'a mut [u8], &'a mut [u8]) {
        let (vw, x) = (self.view.0 as usize, self.x as usize);
        let (first, second) = line.split_at_mut(vw * 4);
        let first = &mut first[x * 4..(x + width) * 4];
        let second = &mut second[x * 4..(x + width) * 4];
        if self.swap {
            (second, first)
        } else {
            (first, second)
        }
    }
}

/// [`render_pair`] straight into the output's picture: the two views of
/// `source` are rendered where `place` puts them in `canvas`, with no pair
/// in between to copy from. What `canvas` holds around the views stays.
/// False (and `canvas` untouched) when the sizes do not fit.
pub fn render_placed(
    source: &[u8],
    width: u32,
    height: u32,
    depth: Option<Depth<'_>>,
    params: Params,
    place: Placement,
    canvas: &mut [u8],
) -> bool {
    let (w, h) = (width as usize, height as usize);
    let Some(rows) = place.rows(w, h, canvas.len()) else {
        return false;
    };
    if source.len() != w * h * 4 {
        return false;
    }
    let depth = DepthAt::new(depth, w, h);
    let row_bytes = place.view.0 as usize * 8;
    in_bands(&mut canvas[rows], row_bytes, |first_row, band| {
        let mut nearness = vec![0.0f32; if depth.is_some() { w } else { 0 }];
        let mut placed = vec![f32::NEG_INFINITY; nearness.len()];
        for (row, line) in band.chunks_exact_mut(row_bytes).enumerate() {
            // The picture's rows run upwards, the view's downwards.
            let y = h - 1 - (first_row + row);
            let pixels = &source[y * w * 4..(y + 1) * w * 4];
            let (left, right) = place.halves(line, w);
            match &depth {
                Some(depth) => {
                    depth.row(y, &mut nearness);
                    shift_row(pixels, &nearness, &mut placed, params, width, left, right);
                }
                None => {
                    left.copy_from_slice(pixels);
                    right.copy_from_slice(pixels);
                }
            }
        }
    });
    true
}

// Fill the pixels nothing was moved onto, each run from whichever
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

/// Put the views of `pair` (the picture [`render_pair`] makes of a `width`
/// x `height` source) where `place` puts them in `canvas`, sharpened by
/// `sharpen` on the way (0.0 leaves them, 1.0 is strong): each pixel is
/// pushed away from the mean of its four neighbours in its own view; the
/// outermost pixels of a view stay as they are. Sharpening reads the pair
/// and writes the picture, so nothing is copied but the picture itself.
/// What `canvas` holds around the views stays. False (and `canvas`
/// untouched) when the sizes do not fit.
pub fn place_pair(
    pair: &[u8],
    width: u32,
    height: u32,
    sharpen: f32,
    place: Placement,
    canvas: &mut [u8],
) -> bool {
    let (w, h) = (width as usize, height as usize);
    let Some(rows) = place.rows(w, h, canvas.len()) else {
        return false;
    };
    if pair.len() != w * h * 8 {
        return false;
    }
    let sharp = sharpen > 0.0 && w >= 3 && h >= 3;
    // Fixed point: 256 is 1.0.
    let k = (sharpen.min(2.0) * 256.0) as i32;
    let row_bytes = place.view.0 as usize * 8;
    in_bands(&mut canvas[rows], row_bytes, |first_row, band| {
        for (row, line) in band.chunks_exact_mut(row_bytes).enumerate() {
            // The picture's rows run upwards, the pair's downwards.
            let y = h - 1 - (first_row + row);
            let (here_left, here_right) = pair[y * w * 8..(y + 1) * w * 8].split_at(w * 4);
            let (left, right) = place.halves(line, w);
            if !sharp || y == 0 || y + 1 == h {
                left.copy_from_slice(here_left);
                right.copy_from_slice(here_right);
                continue;
            }
            let (above_left, above_right) = pair[(y - 1) * w * 8..y * w * 8].split_at(w * 4);
            let (below_left, below_right) = pair[(y + 1) * w * 8..(y + 2) * w * 8].split_at(w * 4);
            sharpen_row(above_left, here_left, below_left, k, left);
            sharpen_row(above_right, here_right, below_right, k, right);
        }
    });
    true
}

/// One row of one view, sharpened from the rows around it into `out`; its
/// first and last pixel are copied.
fn sharpen_row(above: &[u8], here: &[u8], below: &[u8], k: i32, out: &mut [u8]) {
    let end = here.len() - 4;
    out[..4].copy_from_slice(&here[..4]);
    out[end..].copy_from_slice(&here[end..]);
    for x in (4..end).step_by(4) {
        for i in x..x + 3 {
            let centre = here[i] as i32;
            let around =
                above[i] as i32 + below[i] as i32 + here[i - 4] as i32 + here[i + 4] as i32;
            let value = centre + (((centre * 4 - around) * k) >> 10);
            out[i] = value.clamp(0, 255) as u8;
        }
        out[x + 3] = here[x + 3];
    }
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
    fn a_still_pictures_map_is_eased_in_as_before_and_then_stays_put() {
        let (w, h) = (16u32, 9u32);
        let before: Vec<f32> = (0..w * h).map(|i| (i % 7) as f32).collect();
        let still: Vec<f32> = (0..w * h).map(|i| ((i * 5) % 11) as f32 * 0.25).collect();
        let own = {
            let mut fresh = DepthSmoother::default();
            assert!(fresh.update(w, h, &still, 0.0));
            fresh
        };
        for steadiness in [0.0, 0.6, 0.95] {
            // `again` is asked about the same pixels for ever, as the model
            // was; `eased` takes the one answer again and again.
            let mut again = DepthSmoother::default();
            assert!(again.update(w, h, &before, steadiness));
            assert!(again.update(w, h, &still, steadiness));
            let mut eased = again.clone();
            let mut steps = 0;
            loop {
                steps += 1;
                assert!(steps < 2000, "never settled at {steadiness}");
                assert!(again.update(w, h, &still, steadiness));
                if eased.settle(w, h, &still, steadiness) {
                    break;
                }
                assert_eq!(eased.values(), again.values(), "step {steps}");
            }
            // It ends on the picture's own map exactly, a step the eye
            // cannot see away from where asking again had got to.
            assert_eq!(eased.values(), own.values());
            for (a, b) in again.values().iter().zip(eased.values()) {
                assert!((a - b).abs() <= SETTLED, "{a} {b} at {steadiness}");
            }
            // More steps change nothing.
            assert!(eased.settle(w, h, &still, steadiness));
            assert_eq!(eased.values(), own.values());
            // Depth that follows the model at once is there after the range
            // has caught up; steadier depth takes longer.
            assert!(steadiness > 0.0 || steps < 40, "{steps}");
        }
        // A flat picture is flat at once (its range has nowhere to ease
        // to), and an unusable map ends the easing.
        let mut smoother = DepthSmoother::default();
        assert!(smoother.update(2, 1, &[1.0, 3.0], 0.5));
        assert!(smoother.settle(2, 1, &[4.0, 4.0], 0.5));
        assert_eq!(smoother.values(), [0.0, 0.0]);
        assert!(smoother.settle(2, 1, &[f32::NAN, 1.0], 0.5));
        assert_eq!(smoother.values(), [0.0, 0.0]);
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

    /// A row of the views as it was made before: a byte at a time, each
    /// looked up by its place and checked against the end of its row, the
    /// column through the C library's `round`. Kept as its definition.
    mod a_byte_at_a_time {
        use super::super::*;

        pub fn shift_row(
            pixels: &[u8],
            nearness: &[f32],
            placed: &mut [f32],
            params: Params,
            width: u32,
            left: &mut [u8],
            right: &mut [u8],
        ) {
            let w = nearness.len();
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
                fill_gaps(view, placed);
            }
        }
    }

    fn noise() -> impl FnMut(usize) -> Vec<u8> {
        let mut noise = 0x9e37_79b9u32;
        move |count| {
            let mut next = || {
                noise ^= noise << 13;
                noise ^= noise >> 17;
                noise ^= noise << 5;
                noise as u8
            };
            (0..count).map(|_| next()).collect()
        }
    }

    #[test]
    fn a_row_of_the_views_made_in_whole_pixels_is_the_row_made_a_byte_at_a_time() {
        let mut bytes = noise();
        // Every width up to a good many, and rows as long as a screen's.
        for w in (1..=70usize).chain([97, 333, 1021, 3840]) {
            let pixels = bytes(w * 4);
            // Trembling depth, near blocks on a slope, a flat scene, and
            // values no depth map should hold but a caller could pass.
            let odd = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 1.0e30, -7.5];
            let maps: [Vec<f32>; 4] = [
                bytes(w).iter().map(|&v| v as f32 / 255.0).collect(),
                (0..w)
                    .map(|x| match (x / 5) % 3 {
                        0 => 1.0,
                        _ => x as f32 / w as f32 * 0.5,
                    })
                    .collect(),
                vec![0.5; w],
                bytes(w)
                    .iter()
                    .map(|&v| match v {
                        0..5 => odd[v as usize],
                        _ => v as f32 / 255.0,
                    })
                    .collect(),
            ];
            for nearness in &maps {
                for (strength, popout) in [
                    (0.0, 0.3),
                    (0.9, 0.0),
                    (2.0, -1.0),
                    (2.0, 1.0),
                    (1.37, 0.41),
                ] {
                    let params = Params { strength, popout };
                    // Stale bytes show a pixel that was not written.
                    let mut now = (vec![0xa5u8; w * 4], vec![0x5au8; w * 4], vec![0.0f32; w]);
                    let mut before = now.clone();
                    let width = w as u32;
                    shift_row(
                        &pixels, nearness, &mut now.2, params, width, &mut now.0, &mut now.1,
                    );
                    a_byte_at_a_time::shift_row(
                        &pixels,
                        nearness,
                        &mut before.2,
                        params,
                        width,
                        &mut before.0,
                        &mut before.1,
                    );
                    assert!(
                        now.0 == before.0 && now.1 == before.1,
                        "{w} wide, {params:?}, nearness {nearness:?}"
                    );
                }
            }
        }
    }

    /// The live picture as it was put together before the views were
    /// rendered and sharpened straight into the output's picture: the pair,
    /// a copy of it to sharpen from, the two columns at the seam put back,
    /// then every row copied to its place. Kept to compare with.
    mod as_before {
        use super::super::*;

        pub fn sharpen(image: &mut [u8], width: u32, amount: f32, scratch: &mut Vec<u8>) {
            let w = width as usize;
            if amount <= 0.0
                || w < 3
                || image.len() < w * 4 * 3
                || !image.len().is_multiple_of(w * 4)
            {
                return;
            }
            scratch.clear();
            scratch.extend_from_slice(image);
            let before: &[u8] = scratch;
            let row = w * 4;
            let rows = image.len() / row;
            let k = (amount.min(2.0) * 256.0) as i32;
            for (y, line) in image.chunks_exact_mut(row).enumerate() {
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
                        let around = above[i] as i32
                            + below[i] as i32
                            + here[i - 4] as i32
                            + here[i + 4] as i32;
                        let value = centre + (((centre * 4 - around) * k) >> 10);
                        line[i] = value.clamp(0, 255) as u8;
                    }
                }
            }
        }

        pub fn picture(
            source: &[u8],
            (width, height): (u32, u32),
            depth: Option<Depth<'_>>,
            params: Params,
            sharpness: i32,
            place: Placement,
            canvas: &mut [u8],
        ) {
            let (w, h) = (width as usize, height as usize);
            let mut pair = vec![0u8; source.len() * 2];
            assert!(render_pair(source, width, height, depth, params, &mut pair));
            let mut scratch = Vec::new();
            if sharpness > 0 {
                sharpen(&mut pair, width * 2, sharpness as f32 / 100.0, &mut scratch);
                let (row, seam) = (w * 8, w * 4);
                if seam >= 4 && scratch.len() == pair.len() {
                    for (after, before) in pair.chunks_exact_mut(row).zip(scratch.chunks_exact(row))
                    {
                        after[seam - 4..seam + 4].copy_from_slice(&before[seam - 4..seam + 4]);
                    }
                }
            }
            let (vw, vh) = (place.view.0 as usize, place.view.1 as usize);
            let (x, y) = (place.x as usize, place.y as usize);
            for row in 0..h {
                let from = &pair[row * w * 8..(row + 1) * w * 8];
                let (left, right) = from.split_at(w * 4);
                let (left, right) = if place.swap {
                    (right, left)
                } else {
                    (left, right)
                };
                let line = (vh - 1 - (y + row)) * vw * 8;
                canvas[line + x * 4..line + (x + w) * 4].copy_from_slice(left);
                canvas[line + (vw + x) * 4..line + (vw + x + w) * 4].copy_from_slice(right);
            }
        }
    }

    /// A picture with detail in every direction, and a depth map for it
    /// with a near block (smaller than the picture, as the model's is).
    fn a_scene(w: u32, h: u32) -> (Vec<u8>, Vec<f32>, (u32, u32)) {
        let image = RgbaImage::from_fn(w, h, |x, y| {
            image::Rgba([
                (x * 37 + y * 11) as u8,
                ((y * 53) ^ (x * 7)) as u8,
                ((x + y) * 29 % 251) as u8,
                (200 + (x + y) % 56) as u8,
            ])
        });
        let (dw, dh) = (w.div_ceil(2).max(1), h.div_ceil(2).max(1));
        let depth = (0..dw * dh)
            .map(|i| {
                let (x, y) = (i % dw, i / dw);
                if x * 3 > dw && x * 3 < dw * 2 && y * 3 > dh {
                    1.0
                } else {
                    x as f32 / dw as f32 * 0.6
                }
            })
            .collect();
        (image.into_raw(), depth, (dw, dh))
    }

    #[test]
    fn the_picture_put_together_in_one_pass_is_the_one_made_in_stages_before() {
        let params = Params {
            strength: 1.6,
            popout: 0.4,
        };
        // (picture, view, corner): filling the view, letterboxed,
        // pillarboxed, off centre, and pictures too small to sharpen.
        let cases = [
            ((64u32, 36u32), (64u32, 36u32), (0u32, 0u32)),
            ((64, 18), (64, 36), (0, 9)),
            ((40, 36), (64, 36), (12, 0)),
            ((21, 13), (64, 36), (5, 3)),
            ((2, 5), (8, 6), (3, 0)),
            ((5, 2), (8, 6), (1, 2)),
            ((1, 1), (2, 2), (1, 1)),
        ];
        let mut compared = 0;
        for ((w, h), view, (x, y)) in cases {
            let (source, values, (dw, dh)) = a_scene(w, h);
            let map = Depth {
                values: &values,
                width: dw,
                height: dh,
            };
            for depth in [Some(map), None] {
                for swap in [false, true] {
                    for sharpness in [0, 1, 35, 100] {
                        let place = Placement { view, x, y, swap };
                        // What is around the views must stay: a pattern
                        // there shows any byte written out of place.
                        let around: Vec<u8> = (0..place.bytes()).map(|i| (i % 253) as u8).collect();
                        let mut before = around.clone();
                        as_before::picture(
                            &source,
                            (w, h),
                            depth,
                            params,
                            sharpness,
                            place,
                            &mut before,
                        );
                        let mut now = around.clone();
                        if sharpness > 0 {
                            let mut pair = vec![0u8; source.len() * 2];
                            assert!(render_pair(&source, w, h, depth, params, &mut pair));
                            assert!(place_pair(
                                &pair,
                                w,
                                h,
                                sharpness as f32 / 100.0,
                                place,
                                &mut now
                            ));
                        } else {
                            assert!(render_placed(&source, w, h, depth, params, place, &mut now));
                            // Without sharpening, placing a pair is a copy.
                            let mut pair = vec![0u8; source.len() * 2];
                            assert!(render_pair(&source, w, h, depth, params, &mut pair));
                            let mut copied = around.clone();
                            assert!(place_pair(&pair, w, h, 0.0, place, &mut copied));
                            assert!(copied == before, "copying {w}x{h} into {place:?}");
                        }
                        assert!(
                            now == before,
                            "{w}x{h} into {place:?}, sharpness {sharpness}, depth {}",
                            depth.is_some()
                        );
                        compared += 1;
                    }
                }
            }
        }
        assert_eq!(compared, cases.len() * 16);
    }

    #[test]
    fn a_picture_that_does_not_fit_its_place_is_refused_and_nothing_is_written() {
        let (source, _, _) = a_scene(4, 3);
        let pair = vec![9u8; source.len() * 2];
        let fits = Placement {
            view: (6, 4),
            x: 2,
            y: 1,
            swap: false,
        };
        let mut canvas = vec![7u8; fits.bytes()];
        let untouched = canvas.clone();
        let params = Params::default();
        for place in [
            // Too far right, too far down, and a canvas of another size.
            Placement { x: 3, ..fits },
            Placement { y: 2, ..fits },
            Placement {
                view: (6, 5),
                ..fits
            },
        ] {
            assert!(!render_placed(
                &source,
                4,
                3,
                None,
                params,
                place,
                &mut canvas
            ));
            assert!(!place_pair(&pair, 4, 3, 0.5, place, &mut canvas));
        }
        assert!(!render_placed(
            &source[4..],
            4,
            3,
            None,
            params,
            fits,
            &mut canvas
        ));
        assert!(!place_pair(&pair[8..], 4, 3, 0.5, fits, &mut canvas));
        assert!(!render_placed(&[], 0, 0, None, params, fits, &mut canvas));
        assert_eq!(canvas, untouched);
        assert!(render_placed(
            &source,
            4,
            3,
            None,
            params,
            fits,
            &mut canvas
        ));
        // The top row of the view is the third row of the picture from the
        // bottom, in both halves; the one row of room is above it, which
        // is the picture's last.
        let row = 6 * 8;
        assert_eq!(canvas[2 * row + 8..2 * row + 24], source[..16]);
        assert_eq!(canvas[2 * row + 24 + 8..2 * row + 48], source[..16]);
        assert_eq!(canvas[3 * row..], untouched[3 * row..]);
    }

    #[test]
    fn sharpening_raises_contrast_at_an_edge_and_leaves_flat_areas_and_the_border() {
        // Each view 5x3: a bright column in the middle of the left one, the
        // right one flat.
        let (w, h) = (5usize, 3usize);
        let mut pair = vec![0u8; w * h * 8];
        for y in 0..h {
            for x in 0..w * 2 {
                let v = if x == 2 { 200 } else { 100 };
                pair[(y * w * 2 + x) * 4..(y * w * 2 + x) * 4 + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        let place = Placement {
            view: (w as u32, h as u32),
            x: 0,
            y: 0,
            swap: false,
        };
        // Rows come out bottom first; this picture is the same upside down.
        let mut image = vec![0u8; pair.len()];
        assert!(place_pair(
            &pair, w as u32, h as u32, 0.0, place, &mut image
        ));
        assert_eq!(image, pair, "amount 0 changes nothing");
        assert!(place_pair(
            &pair, w as u32, h as u32, 1.0, place, &mut image
        ));
        let at = |x: usize, y: usize| image[(y * w * 2 + x) * 4];
        assert!(at(2, 1) > 200, "the bright column gets brighter");
        assert!(at(1, 1) < 100, "its neighbour gets darker");
        assert_eq!(at(0, 1), 100, "the border column is left");
        assert_eq!(at(2, 0), 200, "the border row is left");
        assert_eq!(image[(w * 2 + 2) * 4 + 3], 255, "alpha is left");
        // The other view's edge column takes nothing in from this one.
        assert_eq!(at(4, 1), 100);
        assert!((5..10).all(|x| at(x, 1) == 100), "the flat view stays flat");
    }
}
