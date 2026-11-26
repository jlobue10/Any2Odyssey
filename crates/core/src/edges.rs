//! Depth edges put where the picture has them.
//!
//! A depth model sees the picture at a few hundred points a side, and its
//! map is stretched over the picture between those points. Where a near
//! object ends, the stretched map does not step from the object to the
//! background: it slopes, over two or three of the map's points, and the
//! slope is centred only roughly on the object's edge. Rendered as it is,
//! the slope carries a strip of background along with the object in one
//! eye and eats into the object in the other, and the two eyes disagree
//! about where the object ends: a ghost beside it, plainest when the head
//! moves.
//!
//! [`refine_row`] replaces each such slope in a row of the stretched map
//! by a step, and puts the step where the picture's colour changes from
//! the one side's to the other's. Flat areas and gentle slopes (a floor, a
//! wall seen at an angle) stay as they are. It works along rows because
//! that is the direction the views are shifted in.
//!
//! The method follows the "cantos finos" pass of odysseyvk-linux
//! (the-cave-dweller, MIT), which does the same in its weaving shader from
//! the map's sixteen nearest points; here it is done in one dimension on
//! the processor, with the cut chosen for a whole slope at once.

/// How far two neighbouring points of the depth map have to be apart for
/// the slope between them to count as an edge, in pixels of one eye's
/// shift. Below this a step would move the picture by less than the slope
/// smears it, and the slope may as well be the scene's own.
const MIN_SHIFT: f32 = 2.0;
/// The longest slope taken for an edge, in points of the depth map. The
/// model's edges are two or three points wide; anything longer is a real
/// slope of the scene.
const MAX_POINTS: f32 = 3.5;
/// How much shallower than its steepest part the ends of an edge's slope
/// may be and still belong to it: a model's edge tails off at both ends.
const TAIL: f32 = 0.25;
/// The least difference between the colours of the two sides (summed over
/// red, green and blue, each 0..=255) for the colour to say where the edge
/// is. Below it the step goes to the middle of the slope.
const MIN_CONTRAST: u32 = 24;

/// One slope of a row: the pixels `start..=end` rise or fall from the one
/// flat to the other.
#[derive(Clone, Copy, Debug)]
struct Slope {
    start: usize,
    end: usize,
}

/// Working room for [`refine_row`], kept between rows.
#[derive(Debug, Default)]
pub(crate) struct Work {
    slopes: Vec<Slope>,
}

/// The mean colour of `pixels`, each channel rounded down.
fn mean(pixels: &[[u8; 4]]) -> Option<[u32; 3]> {
    if pixels.is_empty() {
        return None;
    }
    let mut sum = [0u32; 3];
    for p in pixels {
        for (s, &c) in sum.iter_mut().zip(&p[..3]) {
            *s += u32::from(c);
        }
    }
    let n = pixels.len() as u32;
    Some(sum.map(|s| s / n))
}

fn distance(pixel: &[u8; 4], colour: &[u32; 3]) -> i64 {
    pixel[..3]
        .iter()
        .zip(colour)
        .map(|(&p, &c)| i64::from(u32::from(p).abs_diff(c)))
        .sum()
}

/// Put the depth edges of one row where the picture has them.
///
/// `nearness` is the row of the stretched depth map, `pixels` the same row
/// of the picture. `scale` is the disparity in pixels between nearness 0
/// and 1 (each eye is shifted by half of it) and `cell` the picture's
/// pixels for each point of the depth map along the row. A map that is not
/// stretched (`cell` of 1 or less) has its edges where it has them and is
/// left alone, and so is a row at a strength too small to show an edge.
/// Values that are not numbers are never part of a slope.
pub(crate) fn refine_row(
    pixels: &[[u8; 4]],
    nearness: &mut [f32],
    scale: f32,
    cell: f32,
    work: &mut Work,
) {
    let w = nearness.len().min(pixels.len());
    let scale = scale.abs();
    if w < 4 || cell <= 1.0 || !cell.is_finite() || !(scale > 0.0 && scale.is_finite()) {
        return;
    }
    // Each eye moves by half the disparity. Between two points of the map
    // the stretched map changes by the same amount from each pixel to the
    // next, so the step from pixel to pixel says how far the points are
    // apart.
    let steep = 2.0 * MIN_SHIFT / scale / cell;
    let tail = steep * TAIL;
    let longest = (MAX_POINTS * cell).ceil() as usize;

    work.slopes.clear();
    let mut x = 0;
    while x + 1 < w {
        let step = nearness[x + 1] - nearness[x];
        if step.abs() < tail {
            x += 1;
            continue;
        }
        let start = x;
        let mut steepest = 0.0f32;
        while x + 1 < w {
            let next = nearness[x + 1] - nearness[x];
            if next.abs() < tail || (next > 0.0) != (step > 0.0) {
                break;
            }
            steepest = steepest.max(next.abs());
            x += 1;
        }
        if steepest >= steep && x - start <= longest {
            work.slopes.push(Slope { start, end: x });
        }
    }

    let margin = cell.ceil() as usize;
    let sample = margin.clamp(2, 8);
    for index in 0..work.slopes.len() {
        let Slope { start, end } = work.slopes[index];
        // Nothing of the neighbouring edges is searched or sampled.
        let floor = match index.checked_sub(1) {
            Some(before) => work.slopes[before].end + 1,
            None => 0,
        };
        let ceiling = match work.slopes.get(index + 1) {
            Some(after) => after.start,
            None => w,
        };
        let first = start.saturating_sub(margin).max(floor);
        let last = (end + margin).min(ceiling - 1);
        let (before, after) = (nearness[start], nearness[end]);
        let middle = (start + end).div_ceil(2);
        let sides = (
            mean(&pixels[first.saturating_sub(sample).max(floor)..first]),
            mean(&pixels[last + 1..(last + 1 + sample).min(ceiling)]),
        );
        // The pixels before `cut` belong to the side the slope starts on.
        let cut = match sides {
            (Some(left), Some(right))
                if left
                    .iter()
                    .zip(&right)
                    .map(|(&l, &r)| l.abs_diff(r))
                    .sum::<u32>()
                    >= MIN_CONTRAST =>
            {
                // The cut that leaves every pixel with the side it looks
                // like at the least cost; of equal ones, the one nearest
                // the middle of the slope.
                let (mut best, mut least, mut cost) = (first, 0i64, 0i64);
                for (at, pixel) in pixels[first..=last].iter().enumerate() {
                    cost += distance(pixel, &left) - distance(pixel, &right);
                    let cut = first + at + 1;
                    if cost < least
                        || (cost == least && cut.abs_diff(middle) < best.abs_diff(middle))
                    {
                        (best, least) = (cut, cost);
                    }
                }
                best
            }
            _ => middle,
        };
        for (at, near) in nearness[first..=last].iter_mut().enumerate() {
            let at = first + at;
            if at < cut {
                if at >= start {
                    *near = before;
                }
            } else if at <= end {
                *near = after;
            }
        }
        // A cut outside the slope: what lies between it and the slope
        // changes sides.
        if cut < start {
            nearness[cut..start].fill(after);
        } else if cut > end + 1 {
            nearness[end + 1..cut].fill(before);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACK: [u8; 4] = [20, 40, 60, 255];
    const FRONT: [u8; 4] = [220, 180, 40, 255];

    /// A row `w` wide with a near object from `edge` on, and its depth as a
    /// model gives it: a map of one point every `cell` pixels, stretched,
    /// with the object's edge `off` pixels away from where the picture has
    /// it.
    fn row(w: usize, edge: usize, cell: usize, off: isize) -> (Vec<[u8; 4]>, Vec<f32>) {
        let pixels = (0..w)
            .map(|x| if x >= edge { FRONT } else { BACK })
            .collect();
        let points: Vec<f32> = (0..w / cell + 2)
            .map(|p| {
                let centre = (p * cell + cell / 2) as isize;
                if centre >= edge as isize + off {
                    1.0
                } else {
                    0.0
                }
            })
            .collect();
        let nearness = (0..w)
            .map(|x| {
                let at = ((x as f32 + 0.5) / cell as f32 - 0.5).max(0.0);
                let left = at.floor() as usize;
                let t = at - left as f32;
                points[left] + (points[left + 1] - points[left]) * t
            })
            .collect();
        (pixels, nearness)
    }

    fn first_near(nearness: &[f32]) -> usize {
        nearness.iter().position(|&n| n > 0.5).unwrap()
    }

    #[test]
    fn a_slope_becomes_a_step_at_the_pictures_edge_wherever_the_map_had_it() {
        for cell in [3usize, 4, 7] {
            for off in [-(cell as isize), -2, 0, 3, cell as isize] {
                let (pixels, mut nearness) = row(200, 97, cell, off);
                // Before: a slope several pixels long.
                let sloped = nearness.iter().filter(|&&n| n > 0.01 && n < 0.99).count();
                assert!(sloped >= cell - 1, "cell {cell}, off {off}: {sloped}");
                refine_row(
                    &pixels,
                    &mut nearness,
                    50.0,
                    cell as f32,
                    &mut Work::default(),
                );
                assert!(
                    nearness.iter().all(|&n| n == 0.0 || n == 1.0),
                    "cell {cell}, off {off}: {nearness:?}"
                );
                assert_eq!(first_near(&nearness), 97, "cell {cell}, off {off}");
                assert!(nearness[97..].iter().all(|&n| n == 1.0));
            }
        }
    }

    #[test]
    fn both_edges_of_a_thin_object_are_found_without_reaching_into_each_other() {
        let w = 120;
        let pixels: Vec<[u8; 4]> = (0..w)
            .map(|x| if (50..62).contains(&x) { FRONT } else { BACK })
            .collect();
        // The map has the object two pixels to the right and blurred.
        let mut nearness: Vec<f32> = (0..w)
            .map(|x| {
                let x = x as f32;
                ((x - 48.0) / 6.0).clamp(0.0, 1.0) * ((68.0 - x) / 6.0).clamp(0.0, 1.0)
            })
            .collect();
        refine_row(&pixels, &mut nearness, 60.0, 4.0, &mut Work::default());
        for (x, &near) in nearness.iter().enumerate() {
            let want = if (50..62).contains(&x) { 1.0 } else { 0.0 };
            assert_eq!(near, want, "pixel {x}: {nearness:?}");
        }
    }

    #[test]
    fn flat_areas_gentle_slopes_and_unstretched_maps_stay_as_they_are() {
        let pixels: Vec<[u8; 4]> = (0..300).map(|x| [(x % 256) as u8, 90, 30, 255]).collect();
        // A floor: the whole range over the whole row.
        let floor: Vec<f32> = (0..300).map(|x| x as f32 / 299.0).collect();
        let mut nearness = floor.clone();
        refine_row(&pixels, &mut nearness, 60.0, 4.0, &mut Work::default());
        assert_eq!(nearness, floor);
        // A step too small to see at this strength.
        let (pixels, slope) = row(200, 97, 4, 1);
        let mut nearness: Vec<f32> = slope.iter().map(|n| n * 0.05).collect();
        let small = nearness.clone();
        refine_row(&pixels, &mut nearness, 60.0, 4.0, &mut Work::default());
        assert_eq!(nearness, small);
        // A map of the picture's own size, and no strength at all.
        for (scale, cell) in [(60.0, 1.0), (0.0, 4.0), (f32::NAN, 4.0), (60.0, f32::NAN)] {
            let mut nearness = slope.clone();
            refine_row(&pixels, &mut nearness, scale, cell, &mut Work::default());
            assert_eq!(nearness, slope, "scale {scale}, cell {cell}");
        }
    }

    #[test]
    fn where_the_colour_does_not_tell_the_step_goes_to_the_middle_of_the_slope() {
        let (_, mut nearness) = row(200, 97, 4, 0);
        let slope: Vec<usize> = (0..200)
            .filter(|&x| nearness[x] > 0.01 && nearness[x] < 0.99)
            .collect();
        let middle = (slope[0] + slope[slope.len() - 1]) / 2;
        let pixels = vec![BACK; 200];
        refine_row(&pixels, &mut nearness, 60.0, 4.0, &mut Work::default());
        assert!(nearness.iter().all(|&n| n == 0.0 || n == 1.0));
        assert!(first_near(&nearness).abs_diff(middle) <= 1);
    }

    #[test]
    fn rows_of_every_length_and_content_are_refined_without_a_panic() {
        // A cheap generator: the same rows every run.
        let mut state = 0x2545_f491u32;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state
        };
        let mut work = Work::default();
        for w in 0..70usize {
            for _ in 0..40 {
                let pixels: Vec<[u8; 4]> = (0..w)
                    .map(|_| {
                        let v = next();
                        [v as u8, (v >> 8) as u8, (v >> 16) as u8, 255]
                    })
                    .collect();
                let mut nearness: Vec<f32> = (0..w).map(|_| (next() % 5) as f32 / 4.0).collect();
                let cell = 1.5 + (next() % 12) as f32;
                refine_row(&pixels, &mut nearness, 40.0, cell, &mut work);
                assert!(nearness.iter().all(|n| (0.0..=1.0).contains(n)));
            }
        }
    }
}
