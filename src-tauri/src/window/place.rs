//! Where a saved window goes on the screens that exist now.
//!
//! Plain arithmetic on rectangles, with no window system in sight. All positions and sizes are physical
//! pixels of the whole desktop; sizes are converted through the scale factors, so a window keeps the
//! size it had in logical pixels when the display it lands on has another scale than the display it
//! was saved on.

use crate::session::WindowState;

/// A window that is at least this much on screen (its area, and its title bar) stays where it was.
/// A window that is less visible is moved to the primary screen.
const MOSTLY_VISIBLE: (i64, i64) = (1, 2);
/// The part of the top of a window that has to be reachable, so that it can be grabbed and moved.
const TITLE_BAR: f64 = 40.0;

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// A display: the part of it that windows can use (without the task bar) and its scale factor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub work: Rect,
    pub scale: f64,
    pub primary: bool,
}

/// What the window system demands of a window, in logical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// The smallest content area.
    pub min: (f64, f64),
    /// What the title bar and borders add to the content area.
    pub frame: (f64, f64),
}

/// Where to put the window: the outer top-left corner and the size of the content area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

/// A rectangle as edges, in a type that cannot overflow.
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

    /// The area shared with `other`.
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

/// Rounds to whole pixels. Saturates for numbers too big for a pixel count.
#[allow(clippy::cast_possible_truncation)] // a rounded f64 to i64 saturates, which is what we want
fn px(value: f64) -> i64 {
    value.round() as i64
}

/// A scale factor that can be multiplied with: damaged values count as 1.
fn sane(scale: f64) -> f64 {
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

/// How much of `area` lies on a screen.
fn visible(area: Area, screens: &[Screen]) -> i64 {
    screens
        .iter()
        .map(|screen| area.overlap(screen.work.into()))
        .sum()
}

fn mostly_visible(area: Area, screens: &[Screen]) -> bool {
    let (num, den) = MOSTLY_VISIBLE;
    visible(area, screens) * den >= area.size() * num
}

/// The screen the rectangle lies on most (the first of equals), if it lies on any.
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

/// The size of the window on one screen: the saved logical size at that screen's scale, but not
/// smaller than the minimum and not bigger than the screen.
struct Fit {
    scale: f64,
    inner: (i64, i64),
    frame: (i64, i64),
}

impl Fit {
    fn new(logical: (f64, f64), screen: &Screen, limits: Limits) -> Self {
        let scale = sane(screen.scale);
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

    /// The window with its title bar and borders, with its corner at `x`, `y`.
    fn outer(&self, x: i64, y: i64) -> Area {
        Area::new(
            x,
            y,
            self.inner.0 + self.frame.0,
            self.inner.1 + self.frame.1,
        )
    }

    /// The strip at the top of the window that has to be on a screen.
    fn title_bar(&self, x: i64, y: i64) -> Area {
        let outer = self.outer(x, y);
        let height = px(TITLE_BAR * self.scale).min(outer.height());
        Area::new(x, y, outer.width(), height)
    }

    fn placement(&self, x: i64, y: i64, maximized: bool) -> Placement {
        Placement {
            x: to_i32(x),
            y: to_i32(y),
            width: to_u32(self.inner.0),
            height: to_u32(self.inner.1),
            maximized,
        }
    }
}

/// Where to restore `saved` on `screens` (`None` when there are no screens to put it on).
///
/// A window that is mostly on a screen, with its title bar, stays where it was, moved just enough to be
/// whole on that screen. Any other (the screen it was on is gone, or the resolution shrank around it) goes
/// to the middle of the primary screen. Either way the size is the saved size in logical pixels at the
/// scale of the screen it lands on, and fits the screen.
pub fn place(saved: &WindowState, screens: &[Screen], limits: Limits) -> Option<Placement> {
    let primary = screens
        .iter()
        .find(|screen| screen.primary)
        .or_else(|| screens.first())?;
    let was = sane(saved.scale);
    let logical = (f64::from(saved.width) / was, f64::from(saved.height) / was);
    let (x, y) = (i64::from(saved.x), i64::from(saved.y));

    let was_on = Area::new(x, y, i64::from(saved.width), i64::from(saved.height));
    if let Some(screen) = screen_under(was_on, screens) {
        let fit = Fit::new(logical, screen, limits);
        let outer = fit.outer(x, y);
        if mostly_visible(outer, screens) && mostly_visible(fit.title_bar(x, y), screens) {
            let work = Area::from(screen.work);
            let whole = visible(outer, screens) == outer.size();
            let (x, y) = if whole {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Roughly what Windows adds around the content area.
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

    /// A 1080p display with a 40 px task bar.
    fn hd(x: i32, y: i32, primary: bool) -> Screen {
        screen(x, y, 1920, 1040, 1.0, primary)
    }

    fn saved(x: i32, y: i32, width: u32, height: u32, scale: f64) -> WindowState {
        WindowState {
            x,
            y,
            width,
            height,
            scale,
            maximized: false,
        }
    }

    fn put(state: &WindowState, screens: &[Screen]) -> Placement {
        place(state, screens, LIMITS).expect("a placement")
    }

    fn at(x: i32, y: i32, width: u32, height: u32) -> Placement {
        Placement {
            x,
            y,
            width,
            height,
            maximized: false,
        }
    }

    #[test]
    fn a_window_that_is_on_screen_stays_exactly_where_it_was() {
        let screens = [hd(0, 0, true)];
        assert_eq!(
            put(&saved(100, 100, 1280, 800, 1.0), &screens),
            at(100, 100, 1280, 800)
        );
        // at the very corner, and right against the edges
        assert_eq!(
            put(&saved(0, 0, 1280, 800, 1.0), &screens),
            at(0, 0, 1280, 800)
        );
        assert_eq!(
            put(&saved(624, 201, 1280, 800, 1.0), &screens),
            at(624, 201, 1280, 800)
        );
    }

    #[test]
    fn a_window_on_the_second_screen_stays_there_while_it_exists() {
        let screens = [hd(0, 0, true), hd(1920, 0, false)];
        assert_eq!(
            put(&saved(2000, 100, 1280, 800, 1.0), &screens),
            at(2000, 100, 1280, 800)
        );
    }

    #[test]
    fn a_window_on_an_unplugged_screen_goes_to_the_middle_of_the_primary_one() {
        let saved_on_second = saved(2000, 100, 1280, 800, 1.0);
        // outer 1296 x 839 in 1920 x 1040: (624 / 2, 201 / 2)
        assert_eq!(
            put(&saved_on_second, &[hd(0, 0, true)]),
            at(312, 100, 1280, 800)
        );
        // the primary screen need not be first in the list or at the origin
        let shifted = [hd(1920, 0, false), hd(5000, 300, true)];
        assert_eq!(
            put(&saved(-500, 10, 1280, 800, 1.0), &shifted),
            at(5312, 400, 1280, 800)
        );
        // with no screen flagged as primary, the first one is
        assert_eq!(
            put(&saved(-500, 10, 1280, 800, 1.0), &[hd(1920, 0, false)]),
            at(2232, 100, 1280, 800)
        );
    }

    #[test]
    fn screens_to_the_left_and_above_have_negative_positions_and_still_work() {
        let screens = [hd(0, 0, true), screen(-1920, -200, 1920, 1080, 1.0, false)];
        assert_eq!(
            put(&saved(-1800, 20, 1280, 800, 1.0), &screens),
            at(-1800, 20, 1280, 800)
        );
        assert_eq!(
            put(&saved(-1800, -150, 1280, 800, 1.0), &screens),
            at(-1800, -150, 1280, 800),
            "above the primary screen's top is fine on the screen that is there"
        );
        // the left screen is unplugged
        assert_eq!(
            put(&saved(-1800, 20, 1280, 800, 1.0), &screens[..1]),
            at(312, 100, 1280, 800)
        );
    }

    #[test]
    fn a_window_across_two_screens_is_left_alone() {
        let screens = [hd(0, 0, true), hd(1920, 0, false)];
        assert_eq!(
            put(&saved(1500, 100, 1280, 800, 1.0), &screens),
            at(1500, 100, 1280, 800)
        );
        // with the second screen gone, less than half of it is left
        assert_eq!(
            put(&saved(1500, 100, 1280, 800, 1.0), &screens[..1]),
            at(312, 100, 1280, 800)
        );
    }

    #[test]
    fn a_window_mostly_on_screen_is_moved_just_enough_to_be_whole() {
        let screens = [hd(0, 0, true)];
        // sticking out to the right: 920 of 1296 px are visible
        assert_eq!(
            put(&saved(1000, 100, 1280, 800, 1.0), &screens),
            at(624, 100, 1280, 800)
        );
        // and to the left, and below
        assert_eq!(
            put(&saved(-300, 100, 1280, 800, 1.0), &screens),
            at(0, 100, 1280, 800)
        );
        assert_eq!(
            put(&saved(100, 600, 1280, 800, 1.0), &screens),
            at(100, 201, 1280, 800)
        );
    }

    #[test]
    fn a_window_that_is_hardly_on_screen_goes_to_the_primary_screen() {
        let screens = [hd(0, 0, true)];
        for (x, y) in [(1850, 100), (100, 1000), (-1250, 100), (5000, 5000)] {
            assert_eq!(
                put(&saved(x, y, 1280, 800, 1.0), &screens),
                at(312, 100, 1280, 800),
                "{x},{y}"
            );
        }
    }

    #[test]
    fn a_window_whose_title_bar_is_off_the_top_cannot_be_grabbed_and_is_moved() {
        let screens = [hd(0, 0, true)];
        // 52% of the area is visible, but none of the title bar
        assert_eq!(
            put(&saved(100, -400, 1280, 800, 1.0), &screens),
            at(312, 100, 1280, 800)
        );
        // the same window with a little of the title bar showing
        assert_eq!(
            put(&saved(100, -10, 1280, 800, 1.0), &screens),
            at(100, 0, 1280, 800)
        );
    }

    #[test]
    fn a_smaller_resolution_shrinks_the_window_to_fit() {
        let screens = [screen(0, 0, 1366, 728, 1.0, true)];
        // content area: 1366 - 16, 728 - 39
        assert_eq!(
            put(&saved(0, 0, 1920, 1080, 1.0), &screens),
            at(0, 0, 1350, 689)
        );
        // a window that no longer fits and was off to the side is also centred
        assert_eq!(
            put(&saved(2000, 0, 1920, 1080, 1.0), &screens),
            at(0, 0, 1350, 689)
        );
    }

    #[test]
    fn a_bigger_display_scale_keeps_the_logical_size() {
        // the same 1280 x 800 window, saved at 100%, on a display that is now 200%
        let screens = [screen(0, 0, 3840, 2080, 2.0, true)];
        // frame 32 x 78 px: outer 2592 x 1678
        assert_eq!(
            put(&saved(100, 100, 1280, 800, 1.0), &screens),
            at(100, 100, 2560, 1600)
        );
    }

    #[test]
    fn a_bigger_display_scale_on_a_small_screen_shrinks_the_window_to_fit() {
        // 1280 x 800 logical is 1920 x 1200 px at 150%, more than the screen has
        let screens = [screen(0, 0, 1920, 1040, 1.5, true)];
        // frame 24 x 59 px; the window is pulled back onto the screen
        assert_eq!(
            put(&saved(100, 100, 1280, 800, 1.0), &screens),
            at(0, 0, 1896, 981)
        );
    }

    #[test]
    fn a_smaller_display_scale_keeps_the_logical_size() {
        // saved at 200% (2560 x 1600 px), the display is 100% now
        let screens = [hd(0, 0, true)];
        assert_eq!(
            put(&saved(200, 100, 2560, 1600, 2.0), &screens),
            at(200, 100, 1280, 800)
        );
        // at 125%: 1600 x 1000 px is 1280 x 800 logical
        assert_eq!(
            put(&saved(200, 100, 1600, 1000, 1.25), &screens),
            at(200, 100, 1280, 800)
        );
    }

    #[test]
    fn each_screen_has_its_own_scale() {
        let primary = hd(0, 0, true);
        let second = screen(1920, 0, 3840, 2080, 2.0, false);
        // saved on the 200% screen, and it is still there
        assert_eq!(
            put(&saved(2000, 100, 2560, 1600, 2.0), &[primary, second]),
            at(2000, 100, 2560, 1600)
        );
        // the 200% screen is replaced by a 100% one in the same place
        assert_eq!(
            put(
                &saved(2000, 100, 2560, 1600, 2.0),
                &[primary, screen(1920, 0, 1920, 1040, 1.0, false)]
            ),
            at(2000, 100, 1280, 800)
        );
        // it is gone: the size is the logical one at the primary screen's scale
        assert_eq!(
            put(&saved(2000, 100, 2560, 1600, 2.0), &[primary]),
            at(312, 100, 1280, 800)
        );
        // saved on the 100% screen and the primary is the 200% one now
        assert_eq!(
            put(
                &saved(100, 100, 1280, 800, 1.0),
                &[screen(0, 0, 3840, 2080, 2.0, true), hd(3840, 0, false)]
            ),
            at(100, 100, 2560, 1600)
        );
    }

    #[test]
    fn the_window_is_never_smaller_than_the_minimum_or_bigger_than_the_screen() {
        let screens = [hd(0, 0, true)];
        assert_eq!(
            put(&saved(100, 100, 300, 200, 1.0), &screens),
            at(100, 100, 900, 600)
        );
        // a screen smaller than the minimum wins over the minimum
        let tiny = [screen(0, 0, 800, 500, 1.0, true)];
        assert_eq!(put(&saved(0, 0, 1280, 800, 1.0), &tiny), at(0, 0, 784, 461));
    }

    #[test]
    fn maximized_travels_with_the_placement() {
        let state = WindowState {
            maximized: true,
            ..saved(100, 100, 1280, 800, 1.0)
        };
        assert!(put(&state, &[hd(0, 0, true)]).maximized);
        assert!(
            put(&state, &[hd(5000, 0, true)]).maximized,
            "also when moved"
        );
    }

    #[test]
    fn no_screens_means_no_placement() {
        assert_eq!(place(&saved(0, 0, 1280, 800, 1.0), &[], LIMITS), None);
    }

    #[test]
    fn damaged_numbers_do_not_break_the_arithmetic() {
        let screens = [hd(0, 0, true)];
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                put(&saved(100, 100, 1280, 800, scale), &screens),
                at(100, 100, 1280, 800),
                "{scale}"
            );
        }
        let broken = [screen(0, 0, 1920, 1040, f64::NAN, true)];
        assert_eq!(
            put(&saved(100, 100, 1280, 800, 1.0), &broken),
            at(100, 100, 1280, 800)
        );
        // extreme coordinates and sizes stay inside the screen and do not overflow
        for (x, y) in [(i32::MIN, i32::MIN), (i32::MAX, i32::MAX), (i32::MAX, 0)] {
            let p = put(&saved(x, y, 100_000, 100_000, 1.0), &screens);
            assert_eq!(p, at(0, 0, 1904, 1001), "{x},{y}");
        }
    }

    #[test]
    fn a_window_equally_on_two_screens_takes_the_first_ones_scale() {
        let first = screen(0, 0, 1920, 1040, 1.0, true);
        let second = screen(1920, 0, 3840, 2080, 2.0, false);
        // half of the 1296 px wide window on each side
        let placed = put(&saved(1280, 100, 1280, 800, 1.0), &[first, second]);
        assert_eq!((placed.width, placed.height), (1280, 800));
    }
}
