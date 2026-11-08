//! Integer rectangles and the two placement rules the capture pipeline relies
//! on: fitting a source into the output and placing the capture surface
//! outside every monitor.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub const EMPTY: Rect = Rect {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    };

    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }
    pub fn from_ltrb(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect::new(left, top, right - left, bottom - top)
    }
    pub fn left(&self) -> i32 {
        self.x
    }
    pub fn top(&self) -> i32 {
        self.y
    }
    pub fn right(&self) -> i32 {
        self.x + self.width
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.height
    }
    pub fn size(&self) -> Size {
        Size {
            width: self.width,
            height: self.height,
        }
    }
    pub fn is_empty(&self) -> bool {
        self.width <= 0 || self.height <= 0
    }
    /// True when the two rectangles share any area (touching edges do not count).
    pub fn intersects(&self, other: &Rect) -> bool {
        !self.is_empty()
            && !other.is_empty()
            && self.left() < other.right()
            && other.left() < self.right()
            && self.top() < other.bottom()
            && other.top() < self.bottom()
    }
    pub fn intersect(&self, other: &Rect) -> Rect {
        let l = self.left().max(other.left());
        let t = self.top().max(other.top());
        let r = self.right().min(other.right());
        let b = self.bottom().min(other.bottom());
        if r <= l || b <= t {
            Rect::EMPTY
        } else {
            Rect::from_ltrb(l, t, r, b)
        }
    }
    pub fn contains_rect(&self, inner: &Rect) -> bool {
        inner.left() >= self.left()
            && inner.top() >= self.top()
            && inner.right() <= self.right()
            && inner.bottom() <= self.bottom()
    }
}

/// Fit `source` into `target` preserving aspect ratio, centred, in `target`'s
/// own coordinate space (origin 0,0). An empty source yields an empty rect.
pub fn fit(source: Rect, target: Rect) -> Rect {
    if source.width < 1 || source.height < 1 {
        return Rect::EMPTY;
    }
    let scale = (target.width as f64 / source.width as f64)
        .min(target.height as f64 / source.height as f64);
    let w = (source.width as f64 * scale) as i32;
    let h = (source.height as f64 * scale) as i32;
    Rect::new((target.width - w) / 2, (target.height - h) / 2, w, h)
}

/// Place a surface of `size` to the right of every monitor, aligned with the
/// topmost monitor edge, with a 64 px gap. The result never overlaps a monitor
/// and is therefore never visible, yet remains part of the desktop so DWM
/// keeps compositing it.
pub fn offscreen_bounds(size: Size, monitors: &[Rect]) -> Rect {
    let right = monitors.iter().map(Rect::right).max().unwrap_or(0);
    let top = monitors.iter().map(Rect::top).min().unwrap_or(0);
    Rect::new(right + 64, top, size.width, size.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Port of Any2Odd's `--self-test`.
    #[test]
    fn fit_scales_1080p_to_4k_exactly() {
        let fitted = fit(Rect::new(0, 0, 1920, 1080), Rect::new(0, 0, 3840, 2160));
        assert_eq!(fitted, Rect::new(0, 0, 3840, 2160));
        assert_eq!(fit(Rect::EMPTY, Rect::new(0, 0, 3840, 2160)), Rect::EMPTY);
    }

    #[test]
    fn fit_letterboxes_and_centres() {
        let fitted = fit(Rect::new(0, 0, 1000, 1000), Rect::new(0, 0, 3840, 2160));
        assert_eq!(fitted, Rect::new(840, 0, 2160, 2160));
    }

    #[test]
    fn offscreen_surface_never_overlaps_any_monitor_arrangement() {
        let arrangements: [&[Rect]; 2] = [
            &[
                Rect::new(0, 0, 3840, 2160),
                Rect::new(610, -1440, 2560, 1440),
                Rect::new(3840, 473, 1440, 2560),
            ],
            &[
                Rect::new(-2560, -1440, 2560, 1440),
                Rect::new(0, 0, 3840, 2160),
            ],
        ];
        for monitors in arrangements {
            let off = offscreen_bounds(
                Size {
                    width: 3840,
                    height: 2160,
                },
                monitors,
            );
            assert!(
                monitors.iter().all(|m| !m.intersects(&off)),
                "{off:?} overlaps {monitors:?}"
            );
            assert_eq!(
                off.size(),
                Size {
                    width: 3840,
                    height: 2160
                }
            );
        }
    }

    #[test]
    fn intersection_rules() {
        let a = Rect::new(0, 0, 10, 10);
        assert!(a.intersects(&Rect::new(9, 9, 5, 5)));
        assert!(
            !a.intersects(&Rect::new(10, 0, 5, 5)),
            "touching edges do not intersect"
        );
        assert!(!a.intersects(&Rect::EMPTY));
        assert_eq!(a.intersect(&Rect::new(5, 5, 10, 10)), Rect::new(5, 5, 5, 5));
        assert!(a.contains_rect(&Rect::new(2, 2, 8, 8)));
        assert!(!a.contains_rect(&Rect::new(2, 2, 9, 8)));
    }
}
