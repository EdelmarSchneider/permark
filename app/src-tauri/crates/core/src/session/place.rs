//! Where a saved window goes on the screens there are now. Plain rectangle arithmetic (physical pixels of the
//! whole desktop); sizes go through scale factors so a window keeps its logical size on a display of another scale.

use serde::{Deserialize, Serialize};

/// A window at least this visible (area, and its title bar) stays where it was; else -> center of primary.
const MOSTLY_VISIBLE: (i64, i64) = (1, 2);
/// Top strip of a window that must be reachable to grab it (logical px).
const TITLE_BAR: f64 = 40.0;
/// No real window is bigger; larger = damaged data.
const MAX_EXTENT: u32 = 100_000;
/// Offset of a new window from the one it cascades from (logical px, right + down).
const CASCADE: f64 = 30.0;

/// Saved window geometry: outer top-left corner, inner size (restored size while maximized), display scale.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub maximized: bool,
}

impl Geometry {
    /// Damaged values -> `None` (size 0 | > 100 000); bad scale -> 1.
    #[must_use]
    pub fn sane(self) -> Option<Self> {
        let extent = |v: u32| (1..=MAX_EXTENT).contains(&v);
        if !extent(self.width) || !extent(self.height) {
            return None;
        }
        let scale = if (0.25..=16.0).contains(&self.scale) {
            self.scale
        } else {
            1.0
        };
        Some(Self { scale, ..self })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// A display: work area (without task bar), scale factor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub work: Rect,
    pub scale: f64,
    pub primary: bool,
}

/// Window system demands (logical px): smallest content area, what frame + title bar add.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    pub min: (f64, f64),
    pub frame: (f64, f64),
}

/// Outer top-left corner + inner size to set, scale of the target screen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
    pub maximized: bool,
}

impl Placement {
    /// The normal (not maximized) geometry this placement gives: what a maximized window restores to.
    #[must_use]
    pub fn normal(self) -> Geometry {
        Geometry {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
            scale: self.scale,
            maximized: false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Area {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
}

impl Area {
    fn new(x: i64, y: i64, width: i64, height: i64) -> Self {
        Self {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
        }
    }
    fn width(self) -> i64 {
        self.right - self.left
    }
    fn height(self) -> i64 {
        self.bottom - self.top
    }
    fn size(self) -> i64 {
        self.width() * self.height()
    }
    fn overlap(self, other: Self) -> i64 {
        let width = (self.right.min(other.right) - self.left.max(other.left)).max(0);
        let height = (self.bottom.min(other.bottom) - self.top.max(other.top)).max(0);
        width * height
    }
}

impl From<Rect> for Area {
    fn from(r: Rect) -> Self {
        Self::new(
            i64::from(r.x),
            i64::from(r.y),
            i64::from(r.width),
            i64::from(r.height),
        )
    }
}

#[allow(clippy::cast_possible_truncation)] // rounded f64 -> i64 saturates: wanted
fn px(value: f64) -> i64 {
    value.round() as i64
}

fn sane_scale(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

fn to_i32(value: i64) -> i32 {
    i32::try_from(value.clamp(i64::from(i32::MIN), i64::from(i32::MAX))).unwrap_or_default()
}

fn to_u32(value: i64) -> u32 {
    u32::try_from(value.clamp(1, i64::from(u32::MAX))).unwrap_or(1)
}

fn visible(area: Area, screens: &[Screen]) -> i64 {
    screens.iter().map(|s| area.overlap(s.work.into())).sum()
}

fn mostly_visible(area: Area, screens: &[Screen]) -> bool {
    let (num, den) = MOSTLY_VISIBLE;
    visible(area, screens) * den >= area.size() * num
}

/// Screen the area lies on most (first of equals).
fn screen_under(area: Area, screens: &[Screen]) -> Option<&Screen> {
    let mut best: Option<(&Screen, i64)> = None;
    for screen in screens {
        let shared = area.overlap(screen.work.into());
        if shared > 0 && best.is_none_or(|(_, most)| shared > most) {
            best = Some((screen, shared));
        }
    }
    best.map(|(screen, _)| screen)
}

/// Window size on one screen: saved logical size at that scale, >= min, <= screen.
struct Fit {
    scale: f64,
    inner: (i64, i64),
    frame: (i64, i64),
}

impl Fit {
    fn new(logical: (f64, f64), screen: &Screen, limits: Limits) -> Self {
        let scale = sane_scale(screen.scale);
        let frame = (px(limits.frame.0 * scale), px(limits.frame.1 * scale));
        let fit = |wanted: f64, min: f64, frame: i64, room: u32| {
            let room = (i64::from(room) - frame).max(1);
            px(wanted * scale).clamp(px(min * scale).min(room), room)
        };
        Self {
            scale,
            inner: (
                fit(logical.0, limits.min.0, frame.0, screen.work.width),
                fit(logical.1, limits.min.1, frame.1, screen.work.height),
            ),
            frame,
        }
    }
    fn outer(&self, x: i64, y: i64) -> Area {
        Area::new(
            x,
            y,
            self.inner.0 + self.frame.0,
            self.inner.1 + self.frame.1,
        )
    }
    fn title_bar(&self, x: i64, y: i64) -> Area {
        let outer = self.outer(x, y);
        Area::new(
            x,
            y,
            outer.width(),
            px(TITLE_BAR * self.scale).min(outer.height()),
        )
    }
    fn placement(&self, x: i64, y: i64, maximized: bool) -> Placement {
        Placement {
            x: to_i32(x),
            y: to_i32(y),
            width: to_u32(self.inner.0),
            height: to_u32(self.inner.1),
            scale: self.scale,
            maximized,
        }
    }
}

/// Where to restore `saved` on `screens` (`None`: no screens). Mostly visible with its title bar -> stays, nudged
/// whole onto its screen; else centered on the primary screen. Size = saved logical size at the target scale.
#[must_use]
pub fn place(saved: &Geometry, screens: &[Screen], limits: Limits) -> Option<Placement> {
    let primary = screens
        .iter()
        .find(|s| s.primary)
        .or_else(|| screens.first())?;
    let was = sane_scale(saved.scale);
    let logical = (f64::from(saved.width) / was, f64::from(saved.height) / was);
    let (x, y) = (i64::from(saved.x), i64::from(saved.y));
    let was_on = Area::new(x, y, i64::from(saved.width), i64::from(saved.height));
    if let Some(screen) = screen_under(was_on, screens) {
        let fit = Fit::new(logical, screen, limits);
        let outer = fit.outer(x, y);
        if mostly_visible(outer, screens) && mostly_visible(fit.title_bar(x, y), screens) {
            let work = Area::from(screen.work);
            let (x, y) = if visible(outer, screens) == outer.size() {
                (x, y)
            } else {
                (
                    x.clamp(work.left, (work.right - outer.width()).max(work.left)),
                    y.clamp(work.top, (work.bottom - outer.height()).max(work.top)),
                )
            };
            return Some(fit.placement(x, y, saved.maximized));
        }
    }
    let fit = Fit::new(logical, primary, limits);
    let outer = fit.outer(0, 0);
    let work = Area::from(primary.work);
    let x = work.left + ((work.width() - outer.width()) / 2).max(0);
    let y = work.top + ((work.height() - outer.height()) / 2).max(0);
    Some(fit.placement(x, y, saved.maximized))
}

/// Geometry of a new window cascaded from `source` (its normal rect when maximized): same size, one step right +
/// down; a window that would leave the source's screen starts again at that screen's top-left corner. Never
/// maximized. Final clamping onto today's screens = [`place`].
#[must_use]
pub fn cascade(source: &Geometry, screens: &[Screen], limits: Limits) -> Geometry {
    let (x, y) = (i64::from(source.x), i64::from(source.y));
    let was_on = Area::new(x, y, i64::from(source.width), i64::from(source.height));
    let step = px(CASCADE * sane_scale(source.scale));
    let (mut nx, mut ny) = (x.saturating_add(step), y.saturating_add(step));
    if let Some(screen) = screen_under(was_on, screens) {
        let scale = sane_scale(screen.scale);
        let frame = (px(limits.frame.0 * scale), px(limits.frame.1 * scale));
        let outer = Area::new(
            nx,
            ny,
            i64::from(source.width) + frame.0,
            i64::from(source.height) + frame.1,
        );
        let work = Area::from(screen.work);
        if outer.right > work.right || outer.bottom > work.bottom {
            (nx, ny) = (work.left, work.top);
        }
    }
    Geometry {
        x: to_i32(nx),
        y: to_i32(ny),
        maximized: false,
        ..*source
    }
}

/// Geometry to save for a window: maximized | minimized -> its last normal rect (+ the flag); maximized without a
/// known normal rect -> its current rect (the flag keeps it maximized); minimized without one -> none (its current
/// rect is the off-screen icon place).
#[must_use]
pub fn to_save(
    maximized: bool,
    minimized: bool,
    normal: Option<Geometry>,
    current: Option<Geometry>,
) -> Option<Geometry> {
    let rect = if minimized {
        normal
    } else if maximized {
        normal.or(current)
    } else {
        current
    };
    rect.map(|g| Geometry { maximized, ..g })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: Limits = Limits {
        min: (900.0, 600.0),
        frame: (16.0, 39.0),
    };

    fn screen(x: i32, y: i32, width: u32, height: u32, scale: f64, primary: bool) -> Screen {
        Screen {
            work: Rect {
                x,
                y,
                width,
                height,
            },
            scale,
            primary,
        }
    }

    fn geo(x: i32, y: i32, width: u32, height: u32, scale: f64) -> Geometry {
        Geometry {
            x,
            y,
            width,
            height,
            scale,
            maximized: false,
        }
    }

    const HD: Screen = Screen {
        work: Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        },
        scale: 1.0,
        primary: true,
    };

    #[test]
    fn a_visible_window_stays_where_it_was() {
        let p = place(&geo(100, 80, 1200, 800, 1.0), &[HD], LIMITS).unwrap();
        assert_eq!((p.x, p.y, p.width, p.height), (100, 80, 1200, 800));
    }

    #[test]
    fn a_window_on_a_vanished_screen_goes_to_the_primary_center() {
        let p = place(&geo(3000, 100, 1200, 800, 1.0), &[HD], LIMITS).unwrap();
        assert_eq!((p.width, p.height), (1200, 800));
        assert_eq!(p.x, (1920 - 1216) / 2);
        assert_eq!(p.y, (1040 - 839) / 2);
    }

    #[test]
    fn a_partly_offscreen_window_is_nudged_fully_on() {
        let p = place(&geo(1200, 300, 1000, 700, 1.0), &[HD], LIMITS).unwrap();
        assert_eq!(p.x, 1920 - 1016);
        assert_eq!(p.y, 300);
    }

    #[test]
    fn a_window_with_its_title_bar_above_the_screen_is_recentered() {
        let p = place(&geo(100, -2000, 1000, 2400, 1.0), &[HD], LIMITS).unwrap();
        assert!(p.y >= 0);
    }

    #[test]
    fn logical_size_survives_a_scale_change() {
        let hidpi = screen(0, 0, 3840, 2080, 2.0, true);
        let p = place(&geo(100, 100, 1200, 800, 1.0), &[hidpi], LIMITS).unwrap();
        assert_eq!((p.width, p.height), (2400, 1600));
    }

    #[test]
    fn size_is_clamped_to_min_and_screen() {
        let small = screen(0, 0, 1280, 700, 1.0, true);
        let p = place(&geo(0, 0, 5000, 5000, 1.0), &[small], LIMITS).unwrap();
        assert_eq!((p.width, p.height), (1280 - 16, 700 - 39));
        let p = place(&geo(10, 10, 200, 100, 1.0), &[HD], LIMITS).unwrap();
        assert_eq!((p.width, p.height), (900, 600));
        // screen smaller than the minimum: the screen wins
        let tiny = screen(0, 0, 640, 480, 1.0, true);
        let p = place(&geo(0, 0, 1200, 800, 1.0), &[tiny], LIMITS).unwrap();
        assert!(p.width <= 640 && p.height <= 480);
    }

    #[test]
    fn second_monitor_left_of_primary_and_negative_coordinates() {
        let left = screen(-1920, 0, 1920, 1040, 1.0, false);
        let p = place(&geo(-1800, 100, 1200, 800, 1.0), &[HD, left], LIMITS).unwrap();
        assert_eq!((p.x, p.y), (-1800, 100));
    }

    #[test]
    fn no_screens_no_placement_and_damaged_values_never_panic() {
        assert_eq!(place(&geo(0, 0, 100, 100, 1.0), &[], LIMITS), None);
        for g in [
            geo(i32::MAX, i32::MAX, u32::MAX, u32::MAX, f64::NAN),
            geo(i32::MIN, i32::MIN, 1, 1, -3.0),
            geo(0, 0, 1000, 1000, f64::INFINITY),
            geo(0, 0, 1000, 1000, 0.0),
        ] {
            let p = place(&g, &[HD, screen(0, 0, 0, 0, f64::NAN, false)], LIMITS).unwrap();
            assert!(p.width >= 1 && p.height >= 1);
        }
    }

    #[test]
    fn primary_is_found_or_first_screen_used() {
        let a = screen(0, 0, 1000, 800, 1.0, false);
        let b = screen(1000, 0, 2000, 1200, 1.0, false);
        let p = place(&geo(9000, 9000, 900, 600, 1.0), &[a, b], LIMITS).unwrap();
        assert!(p.x < 1000, "first screen used without a primary");
    }

    #[test]
    fn a_placement_keeps_its_normal_rect_and_target_scale() {
        let hidpi = screen(0, 0, 3840, 2080, 2.0, true);
        let saved = Geometry {
            maximized: true,
            ..geo(100, 100, 1200, 800, 1.0)
        };
        let p = place(&saved, &[hidpi], LIMITS).unwrap();
        assert!(p.maximized);
        assert_eq!(p.normal(), geo(100, 100, 2400, 1600, 2.0));
    }

    #[test]
    fn a_new_window_cascades_from_the_source() {
        let g = cascade(&geo(100, 80, 1200, 800, 1.0), &[HD], LIMITS);
        assert_eq!(g, geo(130, 110, 1200, 800, 1.0));
        // the step is logical: 60 physical px at scale 2
        let hidpi = screen(0, 0, 3840, 2080, 2.0, true);
        let g = cascade(&geo(100, 80, 2400, 1600, 2.0), &[hidpi], LIMITS);
        assert_eq!((g.x, g.y), (160, 140));
    }

    #[test]
    fn a_maximized_source_gives_its_normal_rect_unmaximized() {
        let source = Geometry {
            maximized: true,
            ..geo(100, 80, 1200, 800, 1.0)
        };
        let g = cascade(&source, &[HD], LIMITS);
        assert_eq!(g, geo(130, 110, 1200, 800, 1.0));
    }

    #[test]
    fn a_cascade_that_would_leave_the_screen_starts_at_its_corner() {
        // right edge: x 704 + 1216 = 1920 fits; 705 + 30 does not
        let g = cascade(&geo(674, 10, 1200, 800, 1.0), &[HD], LIMITS);
        assert_eq!((g.x, g.y), (704, 40));
        let g = cascade(&geo(705, 10, 1200, 800, 1.0), &[HD], LIMITS);
        assert_eq!((g.x, g.y), (0, 0));
        // bottom edge
        let g = cascade(&geo(10, 200, 1200, 800, 1.0), &[HD], LIMITS);
        assert_eq!((g.x, g.y), (0, 0));
        // on a second screen left of the primary: its own corner, and `place` keeps it there
        let left = screen(-1920, 0, 1920, 1040, 1.0, false);
        let g = cascade(&geo(-1215, 10, 1200, 800, 1.0), &[HD, left], LIMITS);
        assert_eq!((g.x, g.y), (-1920, 0));
        let p = place(&g, &[HD, left], LIMITS).unwrap();
        assert_eq!((p.x, p.y), (-1920, 0));
    }

    #[test]
    fn a_cascade_from_an_offscreen_or_damaged_source_never_panics_and_place_recenters() {
        let g = cascade(&geo(9000, 9000, 1200, 800, 1.0), &[HD], LIMITS);
        assert_eq!((g.x, g.y), (9030, 9030));
        let p = place(&g, &[HD], LIMITS).unwrap();
        assert_eq!(p.x, (1920 - 1216) / 2, "centered on the primary");
        for source in [
            geo(i32::MAX, i32::MAX, u32::MAX, u32::MAX, f64::NAN),
            geo(i32::MIN, i32::MIN, 1, 1, -3.0),
            geo(0, 0, 1, 1, f64::INFINITY),
        ] {
            let g = cascade(&source, &[HD], LIMITS);
            assert!(!g.maximized);
            if let Some(p) = place(&g, &[HD], LIMITS) {
                assert!(p.width >= 1 && p.height >= 1);
            }
        }
        // no screens: just the step
        assert_eq!(cascade(&geo(0, 0, 900, 600, 1.0), &[], LIMITS).x, 30);
    }

    #[test]
    fn the_saved_geometry_of_a_maximized_or_minimized_window_is_its_normal_rect() {
        let normal = geo(100, 80, 1200, 800, 1.0);
        let full = geo(-8, -8, 1920, 1040, 1.0);
        let icon = geo(-32000, -32000, 0, 0, 1.0);
        let max = |g: Geometry| Geometry {
            maximized: true,
            ..g
        };
        assert_eq!(to_save(false, false, Some(normal), Some(full)), Some(full));
        assert_eq!(
            to_save(true, false, Some(normal), Some(full)),
            Some(max(normal))
        );
        // restored maximized, never moved: no normal rect known -> still maximized next time
        assert_eq!(to_save(true, false, None, Some(full)), Some(max(full)));
        assert_eq!(to_save(false, true, Some(normal), Some(icon)), Some(normal));
        assert_eq!(to_save(false, true, None, Some(icon)), None);
        assert_eq!(
            to_save(true, true, Some(normal), Some(icon)),
            Some(max(normal))
        );
        assert_eq!(to_save(false, false, None, None), None);
    }

    #[test]
    fn sane_rejects_damaged_geometry() {
        assert_eq!(geo(0, 0, 0, 10, 1.0).sane(), None);
        assert_eq!(geo(0, 0, 200_000, 10, 1.0).sane(), None);
        assert_eq!(geo(0, 0, 10, 10, 99.0).sane().unwrap().scale, 1.0);
        assert_eq!(geo(0, 0, 10, 10, 1.5).sane().unwrap().scale, 1.5);
    }
}
