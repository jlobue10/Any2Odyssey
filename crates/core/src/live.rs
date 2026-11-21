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
        let Some(row_bytes) = w.checked_mul(4) else {
            return false;
        };
        w > 0
            && h > 0
            && self.stride >= row_bytes
            && self
                .stride
                .checked_mul(h - 1)
                .and_then(|offset| offset.checked_add(row_bytes))
                .is_some_and(|required| required <= self.bytes.len())
    }
}

/// How the picture lies in a captured frame. A screen cast may hand out the
/// panel's own pixels for a screen that is rotated or mirrored, with a note
/// saying what was done to the upright picture to get them (PipeWire's
/// `SPA_META_VideoTransform`); the wlroots desktops do. The names say it as
/// the note's numbers do: mirrored left to right first, then turned
/// counter-clockwise.
///
/// Compared with sway's own screenshots, pixel for pixel, for all eight.
/// The numbers are not the ones sway's configuration uses: for its
/// `transform 90` the note says three quarters (and the picture's right
/// side is at the frame's bottom), for `transform 270` one quarter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Turned {
    #[default]
    Upright,
    Quarter,
    Half,
    ThreeQuarters,
    Mirrored,
    MirroredQuarter,
    MirroredHalf,
    MirroredThreeQuarters,
}

impl Turned {
    /// The turn a frame's note names; `None` for a number that names none.
    pub fn from_note(note: u32) -> Option<Turned> {
        use Turned::*;
        const ALL: [Turned; 8] = [
            Upright,
            Quarter,
            Half,
            ThreeQuarters,
            Mirrored,
            MirroredQuarter,
            MirroredHalf,
            MirroredThreeQuarters,
        ];
        ALL.get(usize::try_from(note).ok()?).copied()
    }

    /// Whether the picture lies on its side in the frame: its width is the
    /// frame's height.
    pub fn on_its_side(self) -> bool {
        use Turned::*;
        matches!(
            self,
            Quarter | ThreeQuarters | MirroredQuarter | MirroredThreeQuarters
        )
    }

    /// Whether the frame's columns, and its rows, are met from their far
    /// end when the upright picture is read from its top left.
    fn backwards(self) -> (bool, bool) {
        use Turned::*;
        match self {
            Upright | MirroredQuarter => (false, false),
            Mirrored | ThreeQuarters => (true, false),
            MirroredHalf | Quarter => (false, true),
            Half | MirroredThreeQuarters => (true, true),
        }
    }
}

/// The upright rows that are filled side by side from one stretch of a
/// frame's row when the picture lies on its side: sixteen pixels are one
/// line of the processor's cache. Read a column at a time instead, every
/// pixel would be fetched from a row, and a page of memory, of its own.
const SIDE_ROWS: usize = 16;

/// Write the upright picture of `source`, which lies in its frame as
/// `turned` says, into `out`: its rows without gaps, the four bytes of each
/// pixel as they were. The upright picture's width and height; `None`, and
/// `out` as it was, for a frame that is not whole.
pub fn turn_upright(source: Source<'_>, turned: Turned, out: &mut Vec<u8>) -> Option<(u32, u32)> {
    if !source.is_complete() {
        return None;
    }
    let (w, h) = (source.width as usize, source.height as usize);
    let on_its_side = turned.on_its_side();
    let (uw, uh) = if on_its_side { (h, w) } else { (w, h) };
    // Every byte is written below, so a buffer of the right size is left
    // as it is: no pass over it to clear it.
    if out.len() != uw * uh * 4 {
        out.clear();
        out.resize(uw * uh * 4, 0);
    }
    let (columns_back, rows_back) = turned.backwards();
    let frame_row = |y: usize| &source.bytes[y * source.stride..][..w * 4];
    in_bands(out, uw * 4, |first_row, band| {
        if !on_its_side {
            for (row, line) in band.chunks_exact_mut(uw * 4).enumerate() {
                let y = first_row + row;
                let input = frame_row(if rows_back { h - 1 - y } else { y });
                if columns_back {
                    let pixels = input.as_chunks::<4>().0.iter().rev();
                    for (dst, src) in line.as_chunks_mut::<4>().0.iter_mut().zip(pixels) {
                        *dst = *src;
                    }
                } else {
                    line.copy_from_slice(input);
                }
            }
            return;
        }
        // An upright row is a column of the frame, and an upright column a
        // row of it.
        for (index, block) in band.chunks_mut(uw * 4 * SIDE_ROWS).enumerate() {
            let y = first_row + index * SIDE_ROWS;
            let rows = block.len() / (uw * 4);
            let first_column = if columns_back { w - y - rows } else { y };
            for x in 0..uw {
                let input = frame_row(if rows_back { h - 1 - x } else { x });
                let pixels = input[first_column * 4..][..rows * 4].as_chunks::<4>().0;
                for (row, line) in block.chunks_exact_mut(uw * 4).enumerate() {
                    let pixel = pixels[if columns_back { rows - 1 - row } else { row }];
                    line[x * 4..][..4].copy_from_slice(&pixel);
                }
            }
        }
    });
    Some((uw as u32, uh as u32))
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

/// Shrink `source` to `width` x `height` RGBA (alpha 255) in `out`, each
/// output pixel the mean of every source pixel it covers. This is the
/// model's input: [`scale_to_rgba`] reads four pixels for an output pixel
/// however many it stands for, which is all of them down to half size and
/// one in fourteen where a 3840 x 2160 screen becomes the model's 518 x 294.
/// Text and thin lines then come and go with every pixel the screen moves,
/// and the depth map with them. Where nothing is shrunk by more than half,
/// this is [`scale_to_rgba`]. False (and `out` untouched) when the sizes do
/// not fit.
pub fn shrink_to_rgba(source: Source<'_>, out: &mut [u8], width: u32, height: u32) -> bool {
    let (w, h) = (width as usize, height as usize);
    if !source.is_complete() || w == 0 || h == 0 || out.len() != w * h * 4 {
        return false;
    }
    let (sw, sh) = (source.width as usize, source.height as usize);
    if sw <= w * 2 && sh <= h * 2 {
        return scale_to_rgba(source, out, width, height);
    }
    let (r, b) = match source.order {
        PixelOrder::Rgb => (0, 2),
        PixelOrder::Bgr => (2, 0),
    };
    // The source columns and rows an output one stands for: at least one.
    let span = |at: usize, from: usize, to: usize| {
        let first = (at * from / to).min(from - 1);
        (first, ((at + 1) * from / to).clamp(first + 1, from))
    };
    let columns: Vec<(usize, usize)> = (0..w).map(|x| span(x, sw, w)).collect();
    in_bands(out, w * 4, |first_row, band| {
        // One row of sums at a time: red, green and blue of each column.
        let mut sums = vec![[0u32; 3]; w];
        for (row, line) in band.chunks_exact_mut(w * 4).enumerate() {
            let (top, bottom) = span(first_row + row, sh, h);
            sums.fill([0; 3]);
            for y in top..bottom {
                let input = &source.bytes[y * source.stride..];
                for (sum, &(x0, x1)) in sums.iter_mut().zip(&columns) {
                    for pixel in input[x0 * 4..x1 * 4].as_chunks::<4>().0 {
                        sum[0] += pixel[r] as u32;
                        sum[1] += pixel[1] as u32;
                        sum[2] += pixel[b] as u32;
                    }
                }
            }
            for ((pixel, sum), &(x0, x1)) in line
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(&sums)
                .zip(&columns)
            {
                let count = ((x1 - x0) * (bottom - top)) as u32;
                let mean = |c: usize| ((sum[c] + count / 2) / count) as u8;
                *pixel = [mean(0), mean(1), mean(2), 255];
            }
        }
    });
    true
}

/// The size a `source`-sized screen is handed to the depth model's thread
/// at, where the model is shown it at `model` (both width, height).
///
/// A screen at least twice the model's size each way goes there at exactly
/// twice the model's size, made with [`shrink_to_rgba`], and the estimator
/// halves it with a filter that reaches over neighbouring pixels. Measured
/// on a 5120 x 1440 desktop with text, at the model's 518 x 140: moved by
/// one pixel, the depth map changed by 6.7 % of its range on average with
/// four samples a pixel (0.9.7, which takes a KDE screen at its own size),
/// by 1.5 % as 0.9.1 had it (KWin halved the screen first), and by 0.8 %
/// this way. A smaller screen is scaled to the model's size as before.
pub fn model_input_size(source: (u32, u32), model: (u32, u32)) -> (u32, u32) {
    let twice = (model.0.saturating_mul(2), model.1.saturating_mul(2));
    if source.0 >= twice.0 && source.1 >= twice.1 {
        twice
    } else {
        model
    }
}

/// How near to its end a depth map eased towards a still picture's own map
/// counts as there. At the widest view and the strongest depth this much
/// nearness moves a pixel by about a tenth of its width.
pub const SETTLED: f32 = 1.0 / 2048.0;

// Limit the old silhouette left behind by a moving depth boundary, in
// normalized depth units. Small fluctuations still use the chosen steadiness.
const MAX_DEPTH_LAG: f32 = 1.0 / 16.0;

/// How far either end of the running range may be from the same end of the
/// newest map's own range, as a part of that map's own span.
///
/// The range follows the scene at 3 % a map at a steadiness of 47 or more.
/// Left to that alone, a cut to a scene with a fifth of the range before
/// (0 to 10, then 0 to 2) is shown at a fifth of its depth, and 118 maps
/// later, some 20 seconds on a processor, at nine tenths of it; a scene
/// with ten times the range (1 to 4, then 1 to 40) has everything nearer
/// than a tenth of the way shown as nearest, for about as long. With this
/// the first is shown at four fifths of its depth from its first map on
/// and at nine tenths 27 maps later, and the second loses the nearest
/// quarter of its range and no more. A scene is never shown at less than
/// two thirds of its depth, and never loses more than a quarter of its
/// range at either end.
const MAX_RANGE_LAG: f32 = 1.0 / 4.0;

/// Depth that stays put between frames.
///
/// The model sees every frame afresh: its scale drifts and its surfaces
/// tremble, which the eyes read as the whole scene breathing. This keeps a
/// running range to normalize with and blends each new map into the last.
///
/// The maps have to come as the model made them: the range of a map that
/// was brought to 0..=1 beforehand is 0 to 1 every time, and the running
/// range then has nothing to follow. That is how the live screen's maps
/// came until 1.0.0. When the nearest or the farthest point of the scene
/// changed, everything else changed its depth with it in one map: at the
/// default steadiness, the second of four points went from 0.333 to 0.3
/// when the nearest rose from 4 to 5, where the running range leaves it at
/// 0.332.
///
/// What follows from the two limits: a small change of the scene's range
/// is taken slowly, as the steadiness says. A large one is taken at once,
/// all but a quarter of the new range at each end ([`MAX_RANGE_LAG`]) and a
/// sixteenth of full depth at each pixel, and the rest slowly.
#[derive(Clone, Debug, Default)]
pub struct DepthSmoother {
    width: u32,
    height: u32,
    values: Vec<f32>,
    range: Option<(f32, f32)>,
}

impl DepthSmoother {
    /// Take the model's raw output (larger is nearer, on whatever scale the
    /// model has) for a new frame. `steadiness` is how much of the previous
    /// map is kept, 0.0 (none) to 0.95.
    ///
    /// The running range moves towards the map's own lowest and highest
    /// number first, keeping `steadiness` + 0.5 of itself and at most 0.97
    /// (and nothing of itself at a steadiness of 0, where nothing is kept),
    /// and each of its ends is then brought to within [`MAX_RANGE_LAG`] of
    /// the map's own span from the map's own end. The map is normalized
    /// with that range, what lies outside it becoming 0 or 1, and blended
    /// into the last one pixel by pixel; no pixel is left further than a
    /// sixteenth from its new value. The first map, and the first of
    /// another size, is taken as it is with its own range.
    ///
    /// Returns false for a map that cannot be used: of the wrong size, with
    /// something in it that is not a number, or with numbers further apart
    /// than a number can say. The previous one stays, and so does the range.
    pub fn update(&mut self, width: u32, height: u32, raw: &[f32], steadiness: f32) -> bool {
        self.step(width, height, raw, steadiness, false)
    }

    /// One step of [`DepthSmoother::update`], or of [`DepthSmoother::settle`]
    /// for a picture that is `still`.
    fn step(&mut self, width: u32, height: u32, raw: &[f32], steadiness: f32, still: bool) -> bool {
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
        // Numbers that are each finite can still lie further apart than a
        // number can say (-f32::MAX and f32::MAX). Their span was infinite
        // then and the map made with it was not a number anywhere, which
        // the next map's blend stopped the program over. Such a map is
        // refused like one that holds no numbers.
        if !(high - low).is_finite() {
            return false;
        }
        let keep = steadiness.clamp(0.0, 0.95);
        let fresh = self.width != width || self.height != height || self.values.len() != count;
        // The range follows the scene more slowly than the map does, so a
        // near object entering does not push everything else back at once.
        // Not further behind than `MAX_RANGE_LAG` allows, though: a scene
        // with quite another range is not shown squeezed or cut off while
        // the range comes round to it.
        let (low, high) = match self.range {
            Some((l, h)) if !fresh => {
                // A picture that stands still has nothing entering it: its
                // range goes as its values do, and both are there together.
                // At the pace of a moving picture's range a still screen was
                // redrawn for half a minute before it was at rest.
                //
                // And at a steadiness of nothing, nothing is kept: not of
                // the range either. Half of it was. After a cut between
                // two scenes of a film a sixth to a fifth of the old depth
                // was still there with the new scene's first map, and up
                // to three pixels in eight of it lay outside the range and
                // were shown as nearest or farthest, where the page says
                // of 0 that none of the map before is kept.
                let slow = if still || keep == 0.0 {
                    keep
                } else {
                    (keep + 0.5).min(0.97)
                };
                let lag = (high - low) * MAX_RANGE_LAG;
                (
                    (l * slow + low * (1.0 - slow)).clamp(low - lag, low + lag),
                    (h * slow + high * (1.0 - slow)).clamp(high - lag, high + lag),
                )
            }
            _ => (low, high),
        };
        let span = high - low;
        // (a span that is finite, and a quarter more of it that is not)
        if !span.is_finite() {
            return false;
        }
        self.range = Some((low, high));
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
                let blended = *old * keep + new * (1.0 - keep);
                *old = blended.clamp(new - MAX_DEPTH_LAG, new + MAX_DEPTH_LAG);
            }
        }
        true
    }

    /// Take the map of a picture that has not changed once more, in place
    /// of asking the model about the same pixels again: the same step
    /// [`DepthSmoother::update`] makes, so depth eases towards the still
    /// picture's own map as it would have. `raw` is the model's output as
    /// for `update`, and the end is that map normalized with its own range:
    /// the running range comes to the map's own, and the values to theirs.
    /// True once it has arrived and further steps would change nothing; the
    /// last of the way, too small to see ([`SETTLED`]), is taken at once.
    ///
    /// The one difference to `update`'s step: the range keeps `steadiness`
    /// of itself, as the values do, and not more. Nothing enters a picture
    /// that stands still, and at the pace of a moving picture's range (3 %
    /// of the way with each step) the end was some 200 steps away, half a
    /// minute on a still live screen, which takes a step every 150 ms. This
    /// way it is some 20 steps at a steadiness of 0.6.
    pub fn settle(&mut self, width: u32, height: u32, raw: &[f32], steadiness: f32) -> bool {
        if !self.step(width, height, raw, steadiness, true) {
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
    // Byte by byte over rows cut to one length, alpha like a colour: with
    // nothing to check against the end of a row and no byte treated
    // differently from the next, the compiler does several at a time.
    let bytes = out[4..end]
        .iter_mut()
        .zip(&here[4..end])
        .zip(&above[4..end])
        .zip(&below[4..end])
        .zip(&here[..end - 4])
        .zip(&here[8..]);
    for (((((out, &centre), &above), &below), &before), &after) in bytes {
        let centre = centre as i32;
        let around = above as i32 + below as i32 + before as i32 + after as i32;
        let value = centre + (((centre * 4 - around) * k) >> 10);
        *out = value.clamp(0, 255) as u8;
    }
    // Alpha is not a colour: it is put back as it was.
    let pixels = out.as_chunks_mut::<4>().0.iter_mut();
    for (out, here) in pixels.zip(here.as_chunks::<4>().0) {
        out[3] = here[3];
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
        // The scene flips: history at either edge is bounded.
        assert!(smoother.update(2, 1, &[6.0, 2.0], 0.5));
        let values = smoother.values();
        assert_eq!(values, [1.0 - MAX_DEPTH_LAG, MAX_DEPTH_LAG]);
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
    fn a_map_whose_numbers_lie_too_far_apart_to_measure_is_refused_not_a_panic() {
        // Every number is finite and their span is not. The first such map
        // was taken and became "not a number" throughout; the second one's
        // blend then panicked (`clamp` between two of those).
        let wide = [-f32::MAX, f32::MAX];
        let mut smoother = DepthSmoother::default();
        assert!(!smoother.update(2, 1, &wide, 0.5));
        assert!(!smoother.update(2, 1, &wide, 0.5));
        assert!(smoother.values().is_empty());
        // After a map that can be used, it is refused and that one stays;
        // easing towards it ends at once, as for any map that cannot be
        // used.
        assert!(smoother.update(2, 1, &[2.0, 6.0], 0.5));
        assert!(!smoother.update(2, 1, &wide, 0.5));
        assert!(smoother.settle(2, 1, &wide, 0.5));
        assert_eq!(smoother.values(), [0.0, 1.0]);
        // Maps at the two ends of what a number can say, one after the
        // other: each has a span, and whatever becomes of the second, the
        // map stays made of numbers from 0 to 1.
        for steadiness in [0.0, 0.5, 0.95] {
            let maps = [
                [-f32::MAX, -f32::MAX / 2.0],
                [f32::MAX / 2.0, f32::MAX],
                [-f32::MAX, 0.0],
                [0.0, f32::MAX],
                [f32::MAX / 2.0, f32::MAX],
                [-1.0, 1.0],
                [f32::MIN_POSITIVE, 0.0],
            ];
            let mut smoother = DepthSmoother::default();
            for map in maps.iter().chain(maps.iter().rev()) {
                smoother.update(2, 1, map, steadiness);
                smoother.settle(2, 1, map, steadiness);
                assert!(
                    smoother.values().iter().all(|v| (0.0..=1.0).contains(v)),
                    "{:?} after {map:?} at {steadiness}",
                    smoother.values()
                );
            }
        }
    }

    #[test]
    fn at_a_steadiness_of_nothing_each_map_is_shown_by_itself() {
        // Scenes whose numbers have quite other ranges, one after the
        // other and back again, as a film cuts between them. "How much of
        // the previous depth map is kept for each new one", says the
        // page: at 0, none. Half of the running range was kept all the
        // same, and a scene was shown squeezed or cut off at its near end
        // by the range of the scene before.
        let scenes = [
            [0.0f32, 2.5, 5.0, 10.0],
            [0.0, 0.5, 1.0, 2.0],
            [1.0, 2.0, 3.0, 40.0],
            [3.0, 3.5, 3.9, 4.0],
        ];
        let mut smoother = DepthSmoother::default();
        for scene in scenes.iter().chain(scenes.iter().rev()) {
            assert!(smoother.update(4, 1, scene, 0.0));
            let mut alone = DepthSmoother::default();
            assert!(alone.update(4, 1, scene, 0.0));
            assert_eq!(smoother.values(), alone.values(), "{scene:?}");
            assert_eq!(smoother.range, alone.range, "{scene:?}");
        }
    }

    #[test]
    fn a_moving_foreground_tracks_both_edges_while_small_fluctuations_stay_smooth() {
        for keep in [0.6, 0.95] {
            let mut smoother = DepthSmoother::default();
            assert!(smoother.update(6, 1, &[0.0, 1.0, 1.0, 0.0, 0.4, 0.0], keep));
            assert!(smoother.update(6, 1, &[0.0, 0.0, 1.0, 1.0, 0.42, 0.0], keep));
            let d = smoother.values();
            assert!(d[1] <= MAX_DEPTH_LAG && d[3] >= 1.0 - MAX_DEPTH_LAG);
            assert_eq!((d[0], d[2], d[5]), (0.0, 1.0, 0.0));
            assert!((d[4] - (0.4 * keep + 0.42 * (1.0 - keep))).abs() < 1e-6);
        }
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
            // The model was asked once; `eased` takes the one answer again
            // and again.
            let mut eased = DepthSmoother::default();
            assert!(eased.update(w, h, &before, steadiness));
            assert!(eased.update(w, h, &still, steadiness));
            let mut steps = 0;
            let mut last = eased.values().to_vec();
            loop {
                steps += 1;
                assert!(steps < 2000, "never settled at {steadiness}");
                if eased.settle(w, h, &still, steadiness) {
                    break;
                }
                // Eased, not jumped: no step moves a value far.
                for (a, b) in last.iter().zip(eased.values()) {
                    assert!((a - b).abs() <= 0.5, "{a} {b} at step {steps}");
                }
                last = eased.values().to_vec();
            }
            // It ends on the picture's own map exactly.
            assert_eq!(eased.values(), own.values());
            // And in the time the values take, not in the time a moving
            // picture's range takes (some 200 steps at 0.6: half a minute
            // of a still live screen being redrawn).
            let most = if steadiness == 0.0 {
                4
            } else if steadiness <= 0.6 {
                40
            } else {
                400
            };
            assert!(steps <= most, "{steps} steps at {steadiness}");
            // More steps change nothing.
            assert!(eased.settle(w, h, &still, steadiness));
            assert_eq!(eased.values(), own.values());
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

    /// A row of the views and a sharpened row as they were made before:
    /// a byte at a time, each looked up by its place and checked against
    /// the end of its row, the column through the C library's `round`.
    /// Kept as the definition of both.
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

        pub fn sharpen_row(above: &[u8], here: &[u8], below: &[u8], k: i32, out: &mut [u8]) {
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

    #[test]
    fn a_row_sharpened_several_bytes_at_a_time_is_the_row_sharpened_a_byte_at_a_time() {
        let mut bytes = noise();
        // Every width up to past several of the compiler's steps, and rows
        // as long as a screen's. Alpha is noise too: it must come through.
        for w in (3..=70usize).chain([97, 333, 1021, 3840]) {
            let (above, here, below) = (bytes(w * 4), bytes(w * 4), bytes(w * 4));
            // None, hardly any, the page's range and the most there is.
            for k in [0, 1, 90, 256, 511, 512] {
                // Stale bytes show a byte that was not written.
                let (mut now, mut before) = (vec![0xa5u8; w * 4], vec![0xa5u8; w * 4]);
                sharpen_row(&above, &here, &below, k, &mut now);
                a_byte_at_a_time::sharpen_row(&above, &here, &below, k, &mut before);
                assert!(now == before, "{w} wide, sharpened by {k}");
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
    fn a_shrunk_screen_is_the_mean_of_all_its_pixels_not_of_four() {
        // Columns of black and white one pixel wide, as small text is: what
        // four samples make of them depends on where they happen to fall.
        let (sw, sh, w, h) = (3840usize, 64usize, 518u32, 8u32);
        let stripes = |shift: usize| -> Vec<u8> {
            (0..sw * sh)
                .flat_map(|i| {
                    [if (i % sw + shift).is_multiple_of(2) {
                        255u8
                    } else {
                        0
                    }; 4]
                })
                .collect()
        };
        let shrunk = |bytes: &[u8], by: fn(Source<'_>, &mut [u8], u32, u32) -> bool| {
            let source = Source {
                bytes,
                width: sw as u32,
                height: sh as u32,
                stride: sw * 4,
                order: PixelOrder::Rgb,
            };
            let mut out = vec![0u8; (w * h * 4) as usize];
            assert!(by(source, &mut out, w, h));
            out
        };
        for shift in [0, 1] {
            let out = shrunk(&stripes(shift), shrink_to_rgba);
            for pixel in out.as_chunks::<4>().0 {
                // Seven or eight columns each: 3 of 7 to 4 of 7 are white.
                assert!((109..=146).contains(&pixel[0]), "{pixel:?}");
                assert_eq!(pixel[3], 255);
            }
        }
        // Moved by one pixel, the picture stays what it was.
        let (still, moved) = (
            shrunk(&stripes(0), shrink_to_rgba),
            shrunk(&stripes(1), shrink_to_rgba),
        );
        let change = |a: &[u8], b: &[u8]| {
            a.iter()
                .zip(b)
                .map(|(x, y)| x.abs_diff(*y) as u32)
                .max()
                .unwrap()
        };
        assert!(change(&still, &moved) <= 40, "{}", change(&still, &moved));
        // (what the four samples do with the same two frames)
        assert!(
            change(
                &shrunk(&stripes(0), scale_to_rgba),
                &shrunk(&stripes(1), scale_to_rgba)
            ) >= 127
        );
    }

    #[test]
    fn shrinking_keeps_to_the_rows_and_colours_of_the_frame() {
        // 6 x 4 with padded rows, blue first: thirds of the width, halves of
        // the height... of 9 x 6, so that more than half is taken away.
        let (sw, sh, stride) = (9usize, 6usize, 9 * 4 + 12);
        let mut bytes = vec![0xEEu8; stride * sh];
        for y in 0..sh {
            for x in 0..sw {
                let at = y * stride + x * 4;
                // blue, green, red, padding
                bytes[at..at + 4].copy_from_slice(&[(x * 10) as u8, (y * 20) as u8, 200, 7]);
            }
        }
        let source = Source {
            bytes: &bytes[..stride * (sh - 1) + sw * 4],
            width: sw as u32,
            height: sh as u32,
            stride,
            order: PixelOrder::Bgr,
        };
        let mut out = vec![0u8; 3 * 2 * 4];
        assert!(shrink_to_rgba(source, &mut out, 3, 2));
        // Columns 0-2, 3-5, 6-8 and rows 0-2, 3-5: their means.
        assert_eq!(
            &out[..12],
            &[200, 20, 10, 255, 200, 20, 40, 255, 200, 20, 70, 255]
        );
        assert_eq!(
            &out[12..],
            &[200, 80, 10, 255, 200, 80, 40, 255, 200, 80, 70, 255]
        );
        // Sizes that do not fit leave the picture alone.
        let mut wrong = vec![9u8; 5];
        assert!(!shrink_to_rgba(source, &mut wrong, 3, 2));
        assert_eq!(wrong, [9; 5]);
        // Down to half size it is the scaling there was.
        let (mut ours, mut theirs) = (vec![0u8; 5 * 3 * 4], vec![0u8; 5 * 3 * 4]);
        assert!(
            shrink_to_rgba(source, &mut ours, 5, 3) && scale_to_rgba(source, &mut theirs, 5, 3)
        );
        assert_eq!(ours, theirs);
    }

    #[test]
    fn shrinking_is_the_rounded_mean_whichever_side_is_shrunk_by_more_than_half() {
        // Against the same thing written out plainly, a pixel at a time
        // with wide sums: sizes where only the width, only the height or
        // both go down by more than half, and values whose means are not
        // whole numbers.
        let plain = |source: Source<'_>, w: usize, h: usize| -> Vec<u8> {
            let (sw, sh) = (source.width as usize, source.height as usize);
            let cover = |at: usize, from: usize, to: usize| {
                let first = (at * from / to).min(from - 1);
                (first, ((at + 1) * from / to).max(first + 1).min(from))
            };
            let mut out = Vec::new();
            for y in 0..h {
                let (top, bottom) = cover(y, sh, h);
                for x in 0..w {
                    let (left, right) = cover(x, sw, w);
                    let count = ((right - left) * (bottom - top)) as u64;
                    for channel in [2, 1, 0] {
                        let mut sum = 0u64;
                        for row in top..bottom {
                            for column in left..right {
                                sum +=
                                    source.bytes[row * source.stride + column * 4 + channel] as u64;
                            }
                        }
                        out.push(((sum * 2 + count) / (count * 2)) as u8);
                    }
                    out.push(255);
                }
            }
            out
        };
        for (sw, sh, w, h) in [
            (9usize, 4usize, 2usize, 3usize),
            (4, 9, 3, 2),
            (7, 7, 3, 3),
            (5, 1, 1, 1),
            (1, 5, 1, 1),
            (64, 48, 9, 30),
            (48, 64, 30, 9),
            (100, 3, 7, 5),
        ] {
            let stride = sw * 4 + 8;
            let bytes: Vec<u8> = (0..stride * sh)
                .map(|i| (i as u32).wrapping_mul(2_654_435_761).to_be_bytes()[0])
                .collect();
            let source = Source {
                bytes: &bytes[..stride * (sh - 1) + sw * 4],
                width: sw as u32,
                height: sh as u32,
                stride,
                order: PixelOrder::Bgr,
            };
            let mut out = vec![0u8; w * h * 4];
            assert!(shrink_to_rgba(source, &mut out, w as u32, h as u32));
            assert_eq!(out, plain(source, w, h), "{sw} x {sh} to {w} x {h}");
        }
        // Three pixels of 0, 0 and 2: two thirds, which is 1 and not 0.
        let bytes = [0, 0, 0, 0, 0, 0, 0, 0, 2, 2, 2, 0];
        let source = Source {
            bytes: &bytes,
            width: 3,
            height: 1,
            stride: 12,
            order: PixelOrder::Rgb,
        };
        let mut out = [0u8; 4];
        assert!(shrink_to_rgba(source, &mut out, 1, 1));
        assert_eq!(out, [1, 1, 1, 255]);
    }

    #[test]
    fn a_large_screen_goes_to_the_model_at_twice_its_size_and_a_small_one_at_its_own() {
        assert_eq!(model_input_size((3840, 2160), (518, 294)), (1036, 588));
        assert_eq!(model_input_size((5120, 1440), (518, 140)), (1036, 280));
        assert_eq!(model_input_size((1036, 588), (518, 294)), (1036, 588));
        // Not twice the model's size both ways: scaled to the model's size.
        assert_eq!(model_input_size((1920, 1080), (1274, 714)), (1274, 714));
        assert_eq!(model_input_size((1035, 2000), (518, 294)), (518, 294));
        assert_eq!(model_input_size((0, 0), (518, 294)), (518, 294));
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

    #[test]
    fn a_frame_without_pixels_or_with_rows_closer_than_their_width_is_refused() {
        let bytes = [9u8; 64];
        let frame = |width, height, stride| Source {
            bytes: &bytes,
            width,
            height,
            stride,
            order: PixelOrder::Rgb,
        };
        assert!(frame(4, 4, 16).is_complete());
        let mut out = [7u8; 16];
        assert!(scale_to_rgba(frame(4, 4, 16), &mut out, 2, 2));
        for (width, height, stride) in [
            (0, 4, 16),
            (4, 0, 16),
            (0, 0, 0),
            (4, 4, 15),
            (4, 4, 5),
            (4, 2, 0),
            (4, 2, usize::MAX),
            (4, 3, 1usize << (usize::BITS - 1)),
        ] {
            let source = frame(width, height, stride);
            assert!(!source.is_complete(), "{width}x{height}, stride {stride}");
            let mut out = [7u8; 16];
            assert!(!scale_to_rgba(source, &mut out, 2, 2));
            assert_eq!(out, [7u8; 16]);
        }
        // Nor is a frame scaled to no size at all.
        for (w, h) in [(0, 5), (5, 0)] {
            assert!(!scale_to_rgba(frame(4, 4, 16), &mut [], w, h));
        }
    }

    #[test]
    fn every_pixel_of_a_scaled_frame_is_the_mean_of_its_own_rows_and_columns() {
        // The source columns (or rows) an output column (or row) is the
        // mean of when a length is halved, doubled and quartered.
        type Samples = fn(usize) -> (usize, usize);
        let halved: Samples = |i| (2 * i, 2 * i + 1);
        let doubled: Samples = |i| (i / 2, i / 2);
        let quartered: Samples = |i| (4 * i + 1, 4 * i + 3);
        let (sw, sh) = (16usize, 12usize);
        let stride = sw * 4 + 12;
        let bytes = noise()(stride * sh);
        for order in [PixelOrder::Rgb, PixelOrder::Bgr] {
            let source = Source {
                bytes: &bytes,
                width: sw as u32,
                height: sh as u32,
                stride,
                order,
            };
            let colour = |x: usize, y: usize, c: usize| {
                let c = match (order, c) {
                    (PixelOrder::Bgr, 0) => 2,
                    (PixelOrder::Bgr, 2) => 0,
                    _ => c,
                };
                u32::from(bytes[y * stride + x * 4 + c])
            };
            for ((w, h), across, down) in [
                ((8usize, 6usize), halved, halved),
                ((32, 24), doubled, doubled),
                ((4, 3), quartered, quartered),
                ((8, 24), halved, doubled),
                ((32, 3), doubled, quartered),
                ((4, 6), quartered, halved),
            ] {
                let mut out = vec![0u8; w * h * 4];
                assert!(scale_to_rgba(source, &mut out, w as u32, h as u32));
                for y in 0..h {
                    for x in 0..w {
                        let ((x0, x1), (y0, y1)) = (across(x), down(y));
                        let mean = |c: usize| {
                            ((colour(x0, y0, c)
                                + colour(x1, y0, c)
                                + colour(x0, y1, c)
                                + colour(x1, y1, c)
                                + 2)
                                / 4) as u8
                        };
                        assert_eq!(
                            out[(y * w + x) * 4..][..4],
                            [mean(0), mean(1), mean(2), 255],
                            "{order:?} to {w}x{h} at ({x}, {y})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_range_follows_a_new_scene_slowly_and_settling_ends_on_the_maps_own_range() {
        assert_eq!(SETTLED, 1.0 / 2048.0);
        let close = |a: f32, b: f32| (a - b).abs() < 1e-5;
        let mut smoother = DepthSmoother::default();
        assert!(smoother.update(2, 1, &[2.0, 6.0], 0.2));
        assert_eq!(smoother.range, Some((2.0, 6.0)));
        // The model's own numbers, as the live screen hands them over: the
        // nearest point comes a little nearer and the farthest with it.
        // Keeping 0.2 of the map keeps 0.7 of the range: 2.15 to 6.3.
        assert!(smoother.update(2, 1, &[2.5, 7.0], 0.2));
        let (low, high) = smoother.range.unwrap();
        assert!(close(low, 2.15) && close(high, 6.3), "{low} {high}");
        let values = smoother.values();
        assert!(
            close(values[0], 0.8 * (2.5 - 2.15) / 4.15) && close(values[1], 1.0),
            "{values:?}"
        );
        // The range is never kept whole, however steady the map.
        assert!(smoother.update(2, 1, &[2.5, 7.0], 0.9));
        let (low, high) = smoother.range.unwrap();
        assert!(
            close(low, 2.15 * 0.97 + 2.5 * 0.03) && close(high, 6.3 * 0.97 + 7.0 * 0.03),
            "{low} {high}"
        );
        // The same two maps brought to 0..=1 first, as the live screen's
        // were until 1.0.0: there is nothing for the range to follow, and
        // the far point is where the new map has it at once.
        let mut inert = DepthSmoother::default();
        assert!(inert.update(2, 1, &[0.0, 1.0], 0.2));
        assert!(inert.update(2, 1, &[0.0, 1.0], 0.2));
        assert_eq!(inert.range, Some((0.0, 1.0)));
        assert_eq!(inert.values(), [0.0, 1.0]);

        // A still picture whose farthest point is not at zero.
        let mut eased = DepthSmoother::default();
        assert!(eased.update(2, 2, &[2.0, 6.0, 3.0, 5.0], 0.5));
        let still = [3.0, 5.0, 4.5, 3.5];
        let mut steps = 0;
        while !eased.settle(2, 2, &still, 0.5) {
            steps += 1;
            assert!(steps < 2000, "never settled");
        }
        assert!(steps > 0, "the map was not there at the first step");
        assert_eq!(eased.values(), [0.0, 1.0, 0.75, 0.25]);
        assert_eq!(eased.range, Some((3.0, 5.0)));
    }

    /// How much of full depth the scene of the newest map `raw` is shown
    /// at, and how much of its own range is cut off at the far and at the
    /// near end.
    fn shown(smoother: &DepthSmoother, raw: &[f32]) -> (f32, f32, f32) {
        let (low, high) = smoother.range.unwrap();
        let own = raw
            .iter()
            .fold((f32::MAX, f32::MIN), |(l, h), &v| (l.min(v), h.max(v)));
        let span = own.1 - own.0;
        (
            (own.1.min(high) - own.0.max(low)) / (high - low),
            (low - own.0).max(0.0) / span,
            (own.1 - high).max(0.0) / span,
        )
    }

    #[test]
    fn the_range_is_at_most_a_quarter_of_a_new_scenes_span_behind_it() {
        assert_eq!(MAX_RANGE_LAG, 0.25);
        let close = |a: f32, b: f32| (a - b).abs() < 1e-5;
        // A cut to a scene with a fifth of the range, at the default
        // steadiness. The range went to 0..9.76: the scene was shown at
        // 20 % of its depth and reached 90 % with the 118th map after.
        let (before, after) = ([0.0, 2.5, 5.0, 10.0], [0.0, 0.5, 1.0, 2.0]);
        let mut smoother = DepthSmoother::default();
        assert!(smoother.update(4, 1, &before, 0.6));
        assert!(smoother.update(4, 1, &after, 0.6));
        assert_eq!(smoother.range, Some((0.0, 2.5)));
        assert_eq!(shown(&smoother, &after), (0.8, 0.0, 0.0));
        let values = smoother.values();
        assert!(
            close(values[1], 0.23) && close(values[2], 0.46) && close(values[3], 0.8625),
            "{values:?}"
        );
        let mut maps = 0;
        while shown(&smoother, &after).0 < 0.9 {
            assert!(smoother.update(4, 1, &after, 0.6));
            maps += 1;
        }
        assert_eq!(maps, 27);

        // Something comes ten times nearer than the nearest point before.
        // The range went to 1..5.08, and everything from a tenth of the
        // way was shown as nearest; now the nearest quarter is.
        let (before, after) = ([1.0, 2.0, 3.0, 4.0], [1.0, 2.0, 3.0, 40.0]);
        let mut smoother = DepthSmoother::default();
        assert!(smoother.update(4, 1, &before, 0.6));
        assert!(smoother.update(4, 1, &after, 0.6));
        assert_eq!(smoother.range, Some((1.0, 30.25)));
        assert_eq!(shown(&smoother, &after), (1.0, 0.0, 0.25));
        // What was there before is followed at once, all but the
        // sixteenth each pixel may lag by: so large a change is not one to
        // calm.
        let values = smoother.values();
        assert!(
            close(values[1], 1.0 / 29.25 + MAX_DEPTH_LAG)
                && close(values[2], 2.0 / 29.25 + MAX_DEPTH_LAG)
                && values[3] == 1.0,
            "{values:?}"
        );
        // A small rise of the nearest point is calmed: the range is inside
        // the limit and moves as the steadiness says, and the points
        // behind stay where they were (0.333 and 0.667; brought to 0..=1
        // first, the map had them at 0.3 and 0.5625).
        let mut smoother = DepthSmoother::default();
        assert!(smoother.update(4, 1, &before, 0.6));
        assert!(smoother.update(4, 1, &[1.0, 2.0, 3.0, 5.0], 0.6));
        let (low, high) = smoother.range.unwrap();
        assert!(low == 1.0 && close(high, 4.03), "{low} {high}");
        let values = smoother.values();
        assert!(
            close(values[1], 0.332013) && close(values[2], 0.664026) && values[3] == 1.0,
            "{values:?}"
        );

        // Whatever the two scenes and the steadiness: the new one is shown
        // at two thirds of full depth or more, a quarter of its range at
        // the most is cut off at either end, and it stays so while the
        // range comes round.
        let mut state = 0x2545_f491u32;
        let mut number = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state % 20_001) as f32 / 100.0 - 100.0
        };
        for round in 0..2000 {
            let steadiness = [0.0, 0.2, 0.6, 0.95][round % 4];
            let scale = [0.001, 1.0, 50.0][round % 3];
            let mut smoother = DepthSmoother::default();
            let first: Vec<f32> = (0..6).map(|_| number()).collect();
            assert!(smoother.update(3, 2, &first, steadiness));
            let next: Vec<f32> = (0..6).map(|_| number() * scale).collect();
            for map in 0..3 {
                assert!(smoother.update(3, 2, &next, steadiness));
                let (depth, far, near) = shown(&smoother, &next);
                let what = format!("{first:?} then {next:?} at {steadiness}, map {map}");
                assert!(depth >= 2.0 / 3.0 - 1e-4, "{depth}: {what}");
                assert!(
                    far <= 0.25 + 1e-4 && near <= 0.25 + 1e-4,
                    "{far} {near}: {what}"
                );
            }
        }
        // A scene with no depth at all has no span to be behind by: flat
        // at once, but for what each pixel may lag by.
        let mut smoother = DepthSmoother::default();
        assert!(smoother.update(2, 1, &[2.0, 6.0], 0.9));
        assert!(smoother.update(2, 1, &[5.0, 5.0], 0.9));
        assert_eq!(smoother.range, Some((5.0, 5.0)));
        assert_eq!(smoother.values(), [0.0, MAX_DEPTH_LAG]);
    }

    #[test]
    fn a_stretched_map_of_a_slope_is_that_slope_between_the_outermost_centres() {
        // Blending between four neighbours gives a plane back exactly, so
        // the plane itself says what the nearness of every pixel must be.
        let (dw, dh) = (5u32, 4u32);
        let values: Vec<f32> = (0..dw * dh)
            .map(|i| (3 * (i % dw) + 10 * (i / dw)) as f32 / 42.0)
            .collect();
        let map = Depth {
            values: &values,
            width: dw,
            height: dh,
        };
        let centre = |i: usize, out: usize, source: u32| {
            ((i as f64 + 0.5) * f64::from(source) / out as f64 - 0.5)
                .clamp(0.0, f64::from(source - 1))
        };
        for (w, h) in [
            (9usize, 7usize),
            (10, 8),
            (3, 2),
            (5, 9),
            (11, 4),
            (1, 1),
            (2, 13),
        ] {
            let at = DepthAt::new(Some(map), w, h).unwrap();
            let mut nearness = vec![0.0f32; w];
            for y in 0..h {
                at.row(y, &mut nearness);
                for (x, &near) in nearness.iter().enumerate() {
                    let expected = (3.0 * centre(x, w, dw) + 10.0 * centre(y, h, dh)) / 42.0;
                    assert!(
                        (f64::from(near) - expected).abs() < 1e-5,
                        "{w}x{h} at ({x}, {y}): {near}, not {expected}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_picture_with_no_rows_or_no_columns_is_refused_everywhere() {
        let params = Params::default();
        let place = Placement {
            view: (4, 4),
            x: 0,
            y: 0,
            swap: false,
        };
        let mut canvas = vec![7u8; place.bytes()];
        for (w, h) in [(0u32, 3u32), (3, 0)] {
            assert!(!render_pair(&[], w, h, None, params, &mut []));
            assert!(!render_placed(&[], w, h, None, params, place, &mut canvas));
            assert!(!place_pair(&[], w, h, 0.5, place, &mut canvas));
        }
        assert!(canvas.iter().all(|&byte| byte == 7));
    }

    #[test]
    fn views_too_narrow_to_have_an_inside_are_placed_unsharpened() {
        let mut bytes = noise();
        for (w, h) in [(1u32, 3u32), (2, 3), (1, 5), (2, 4)] {
            let pair = bytes((w * h * 8) as usize);
            let place = Placement {
                view: (w, h),
                x: 0,
                y: 0,
                swap: false,
            };
            let (mut sharp, mut plain) = (vec![0u8; pair.len()], vec![0u8; pair.len()]);
            assert!(place_pair(&pair, w, h, 1.0, place, &mut sharp));
            assert!(place_pair(&pair, w, h, 0.0, place, &mut plain));
            assert!(sharp == plain, "{w}x{h}");
        }
    }

    /// How `picture` (rows of pixels) lies in a frame whose note is `note`:
    /// mirrored left to right for the numbers from four on, then turned
    /// counter-clockwise by as many quarters as the number has beyond that.
    fn turned_as_noted(picture: &[Vec<[u8; 4]>], note: u32) -> Vec<Vec<[u8; 4]>> {
        let mut rows = picture.to_vec();
        if note >= 4 {
            rows.iter_mut().for_each(|row| row.reverse());
        }
        for _ in 0..note % 4 {
            // The right column becomes the top row.
            let (w, h) = (rows[0].len(), rows.len());
            rows = (0..w)
                .map(|y| (0..h).map(|x| rows[x][w - 1 - y]).collect())
                .collect();
        }
        rows
    }

    /// `rows` as a frame whose rows are `gap` bytes apart beyond their width.
    fn framed(rows: &[Vec<[u8; 4]>], gap: usize) -> (Vec<u8>, u32, u32, usize) {
        let (w, h) = (rows[0].len(), rows.len());
        let mut bytes = Vec::new();
        for (y, row) in rows.iter().enumerate() {
            bytes.extend(row.iter().flatten());
            if y + 1 < h {
                bytes.extend(std::iter::repeat_n(0xEE, gap));
            }
        }
        (bytes, w as u32, h as u32, w * 4 + gap)
    }

    #[test]
    fn a_frame_is_put_upright_from_each_of_the_eight_ways_it_can_lie() {
        // Six pixels, by hand: the picture, and the frames that hold it.
        let [a, b, c, d, e, f] = [b"aaaa", b"bbbb", b"cccc", b"dddd", b"eeee", b"ffff"].map(|p| *p);
        let picture = vec![vec![a, b, c], vec![d, e, f]];
        let frames: [&[&[[u8; 4]]]; 8] = [
            &[&[a, b, c], &[d, e, f]],
            // A quarter turn counter-clockwise: the right side is on top.
            // This is what sway sends for an output with `transform 270`.
            &[&[c, f], &[b, e], &[a, d]],
            &[&[f, e, d], &[c, b, a]],
            // And this for `transform 90`.
            &[&[d, a], &[e, b], &[f, c]],
            // Mirrored, and then turned.
            &[&[c, b, a], &[f, e, d]],
            &[&[a, d], &[b, e], &[c, f]],
            &[&[d, e, f], &[a, b, c]],
            &[&[f, c], &[e, b], &[d, a]],
        ];
        for (note, frame) in frames.iter().enumerate() {
            let frame: Vec<Vec<[u8; 4]>> = frame.iter().map(|row| row.to_vec()).collect();
            assert_eq!(turned_as_noted(&picture, note as u32), frame, "note {note}");
            let turned = Turned::from_note(note as u32).unwrap();
            assert_eq!(turned.on_its_side(), frame.len() == 3, "note {note}");
            let (bytes, width, height, stride) = framed(&frame, 5);
            let source = Source {
                bytes: &bytes,
                width,
                height,
                stride,
                order: PixelOrder::Bgr,
            };
            let mut out = vec![7; 3];
            assert_eq!(turn_upright(source, turned, &mut out), Some((3, 2)));
            assert_eq!(out, b"aaaabbbbccccddddeeeeffff", "note {note}");
        }
        assert_eq!(Turned::from_note(8), None);
        assert_eq!(Turned::from_note(u32::MAX), None);
        assert_eq!(Turned::default(), Turned::Upright);
    }

    #[test]
    fn frames_of_every_shape_are_put_upright_whole_and_a_frame_that_is_not_whole_is_refused() {
        // Shapes of one row, one column, fewer rows than are filled side by
        // side, and enough for several bands of several blocks each.
        for (w, h) in [
            (1, 1),
            (1, 9),
            (9, 1),
            (3, 2),
            (17, 16),
            (31, 33),
            (150, 70),
        ] {
            let pixel = |x: usize, y: usize| ((y * w + x + 1) as u32).to_le_bytes();
            let picture: Vec<Vec<[u8; 4]>> = (0..h)
                .map(|y| (0..w).map(|x| pixel(x, y)).collect())
                .collect();
            let upright: Vec<u8> = picture.iter().flatten().flatten().copied().collect();
            // A buffer of the right size keeps nothing of what it held.
            let mut out = vec![0xAB; upright.len()];
            for note in 0..8 {
                for gap in [0, 12] {
                    let (bytes, width, height, stride) =
                        framed(&turned_as_noted(&picture, note), gap);
                    let source = Source {
                        bytes: &bytes,
                        width,
                        height,
                        stride,
                        order: PixelOrder::Rgb,
                    };
                    let turned = Turned::from_note(note).unwrap();
                    let size = turn_upright(source, turned, &mut out);
                    assert_eq!(size, Some((w as u32, h as u32)), "{w}x{h} note {note}");
                    assert!(out == upright, "{w}x{h} note {note} gap {gap}");

                    let short = Source {
                        bytes: &bytes[..bytes.len() - 1],
                        ..source
                    };
                    let mut kept = vec![9; 5];
                    assert_eq!(turn_upright(short, turned, &mut kept), None);
                    assert_eq!(kept, [9; 5]);
                    out.fill(0xAB);
                }
            }
        }
    }
}
