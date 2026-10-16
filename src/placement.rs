//! Where the widget goes on screen.
//!
//! `settings.pos` is the widget's home: the top-left the user chose, by
//! dragging the widget or with Move to corner. The widget sits at home when
//! it fits the work area of the monitor nearest to home; otherwise it is
//! fitted inside that work area and marked displaced, while home is kept, so
//! the widget returns home when its monitor does. A resize keeps the edges
//! nearest the work area's edges in place and moves home along only when the
//! widget was at home and the resize tracks home (an Apply does; a tick's
//! resize and a system relocation do not, and mark the widget displaced when
//! they leave it away from home). Everything here is pure except the two
//! work-area shims at the end.

use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromPoint, MonitorFromRect, HMONITOR, MONITORINFO,
    MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY,
};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

/// The least move that puts the `w` x `h` rect at (`x`, `y`) inside `wa`; when
/// the rect is larger than `wa`, its top or left edge wins.
pub fn fit_into(x: i32, y: i32, w: i32, h: i32, wa: RECT) -> (i32, i32) {
    // min, then max (not clamp, which panics when the rect is the larger one).
    (x.min(wa.right - w).max(wa.left), y.min(wa.bottom - h).max(wa.top))
}

/// Where a widget that was at `old` goes at its new size `w` x `h` in `wa`: it
/// keeps its right edge when its centre is in the right half of `wa` and its
/// bottom edge when it is in the lower half (a widget in a corner grows away
/// from that corner), and is then fitted into `wa`. The flag tells whether
/// fitting moved it (clamped).
pub fn anchored(old: RECT, w: i32, h: i32, wa: RECT) -> ((i32, i32), bool) {
    let right_half = past_centre(old.left, old.right, wa.left, wa.right);
    let lower_half = past_centre(old.top, old.bottom, wa.top, wa.bottom);
    // Saturating: `old` can be a rect from outside (WM_DPICHANGED's).
    let x = if right_half { old.right.saturating_sub(w) } else { old.left };
    let y = if lower_half { old.bottom.saturating_sub(h) } else { old.top };
    let spot = fit_into(x, y, w, h, wa);
    (spot, spot != (x, y))
}

/// Whether the middle of `lo..hi` lies past the middle of `wa_lo..wa_hi`
/// (summed in i64, so far-off coordinates cannot overflow).
fn past_centre(lo: i32, hi: i32, wa_lo: i32, wa_hi: i32) -> bool {
    lo as i64 + hi as i64 > wa_lo as i64 + wa_hi as i64
}

/// The top-left of a `w` x `h` widget in a corner of `wa`, `margin` from its
/// edges: 0 top-left, 1 top-right, 2 bottom-left, 3 bottom-right (the
/// ID_SNAP_* order). Fitted, so a widget too large for the margins still lands
/// inside.
pub fn corner_xy(corner: usize, w: i32, h: i32, wa: RECT, margin: i32) -> (i32, i32) {
    let (left, right) = (wa.left + margin, wa.right - w - margin);
    let (top, bottom) = (wa.top + margin, wa.bottom - h - margin);
    let (x, y) = match corner {
        0 => (left, top),
        1 => (right, top),
        2 => (left, bottom),
        _ => (right, bottom),
    };
    fit_into(x, y, w, h, wa)
}

/// Where a `w` x `h` widget whose home is `home` goes in `wa` (the work area
/// nearest to home), and whether that is away from home (displaced).
pub fn place(home: (i32, i32), w: i32, h: i32, wa: RECT) -> ((i32, i32), bool) {
    let spot = fit_into(home.0, home.1, w, h, wa);
    (spot, spot != home)
}

/// Home and the displaced flag after a resize put the widget at `spot`. A
/// clamped spot (fitting moved it) is never adopted: home is kept and the
/// widget is displaced. An unclamped spot becomes home only when the resize
/// tracks home (`track_home`; a tick's resize or a system relocation does not)
/// and the widget is neither displaced nor being dragged. Otherwise home is
/// kept and the widget is displaced while it sits away from home, either
/// because it was displaced already or because the resize does not track
/// home, so a later resize that tracks home (an Apply) does not adopt that
/// spot either; a resize that lands at home ends the displacement.
pub fn after_relayout(
    home: Option<(i32, i32)>,
    spot: (i32, i32),
    clamped: bool,
    displaced: bool,
    in_move: bool,
    track_home: bool,
) -> (Option<(i32, i32)>, bool) {
    if clamped {
        (home, true)
    } else if track_home && !displaced && !in_move {
        (Some(spot), false)
    } else {
        (home, home != Some(spot) && (displaced || !track_home))
    }
}

/// The home a finished drag adopts: its end spot, but only if the widget
/// really moved (a click on the widget runs the same move loop).
pub fn drag_end(start: (i32, i32), end: (i32, i32)) -> Option<(i32, i32)> {
    (end != start).then_some(end)
}

/// The work area (the monitor minus the taskbar and docked bars) of the
/// monitor nearest to `r`: the one it overlaps most, else the closest one.
pub fn work_area_near(r: &RECT) -> RECT {
    // SAFETY: MonitorFromRect only reads `r`; MONITOR_DEFAULTTONEAREST makes
    // it return a monitor even for a rect off every screen.
    unsafe { work_area(MonitorFromRect(r, MONITOR_DEFAULTTONEAREST)) }
}

/// The primary monitor's work area (the primary monitor holds (0, 0)).
pub fn primary_work_area() -> RECT {
    // SAFETY: value arguments only; MONITOR_DEFAULTTOPRIMARY always returns a
    // monitor.
    unsafe { work_area(MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY)) }
}

/// `mon`'s work area, or the primary screen's full rect if it cannot be read.
unsafe fn work_area(mon: HMONITOR) -> RECT {
    let mut mi =
        MONITORINFO { cbSize: core::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    // SAFETY: GetMonitorInfoW checks any HMONITOR (null included) and writes
    // only `mi`, a local whose cbSize gives its size.
    if GetMonitorInfoW(mon, &mut mi).as_bool() {
        mi.rcWork
    } else {
        let (w, h) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        RECT { left: 0, top: 0, right: w, bottom: h }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        after_relayout, anchored, corner_xy, drag_end, fit_into, place, primary_work_area,
        work_area_near,
    };
    use windows::Win32::Foundation::RECT;

    /// The development machine's work areas: the owner's widget lives on the
    /// left monitor, at (-213, -16), 200 x 216 px.
    const LEFT: RECT = RECT { left: -1920, top: -39, right: 0, bottom: 1041 };
    const PRIMARY: RECT = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
    const OWNER: (i32, i32) = (-213, -16);

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT { left, top, right, bottom }
    }

    #[test]
    fn fit_keeps_owner_spot_on_left_monitor() {
        assert_eq!(fit_into(-213, -16, 200, 216, LEFT), (-213, -16));
    }

    #[test]
    fn fit_pulls_orphaned_spot_onto_primary() {
        assert_eq!(fit_into(-213, -16, 200, 216, PRIMARY), (0, 0));
    }

    #[test]
    fn fit_far_off_spot_lands_in_nearest_corner() {
        assert_eq!(fit_into(5000, 5000, 200, 216, PRIMARY), (1720, 864));
    }

    #[test]
    fn fit_oversized_pins_top_left() {
        assert_eq!(fit_into(100, 100, 200, 2000, PRIMARY), (100, 0));
    }

    /// Not a spec test: WM_DPICHANGED's suggested rect comes from outside,
    /// and an odd one (right edge near i32::MIN on a monitor left of the
    /// primary) must still land in the work area instead of overflowing.
    #[test]
    fn anchored_cannot_overflow_on_an_odd_rect() {
        let wa = rect(-1920, -39, 0, 1041);
        let odd = rect(i32::MAX, i32::MAX, i32::MIN + 100, i32::MIN + 100);
        let ((x, y), clamped) = anchored(odd, 200, 216, wa);
        assert!(clamped);
        assert!((wa.left..=wa.right - 200).contains(&x), "{x}");
        assert!((wa.top..=wa.bottom - 216).contains(&y), "{y}");
    }

    #[test]
    fn anchored_bottom_right_widget_grows_up_and_left() {
        // Resizing with SWP_NOMOVE (the old way) would push the bottom to 1092.
        let old = rect(1696, 840, 1896, 1056);
        assert_eq!(anchored(old, 215, 252, PRIMARY), ((1681, 804), false));
    }

    #[test]
    fn anchored_right_half_keeps_right_edge() {
        let (spot, _) = anchored(rect(-213, -16, -13, 200), 215, 216, LEFT);
        assert_eq!(spot, (-228, -16));
        assert_eq!(spot.0 + 215, -13);
    }

    #[test]
    fn corner_xy_matches_snap_math() {
        // Corners as the Move to corner menu numbers them: 1 TR, 3 BR.
        assert_eq!(corner_xy(1, 200, 216, PRIMARY, 24), (1696, 24));
        assert_eq!(corner_xy(3, 200, 216, PRIMARY, 24), (1696, 840));
    }

    #[test]
    fn place_reports_displacement() {
        // Both monitors present: home's nearest work area is the left one.
        assert_eq!(place(OWNER, 200, 216, LEFT), ((-213, -16), false));
        // Only the primary (the left monitor asleep or undetected at logon).
        assert_eq!(place(OWNER, 200, 216, PRIMARY), ((0, 0), true));
    }

    #[test]
    fn clamped_resize_keeps_home() {
        let got = after_relayout(Some(OWNER), (-228, -16), true, false, false, true);
        assert_eq!(got, (Some((-213, -16)), true));
    }

    #[test]
    fn unclamped_resize_moves_home() {
        let spot = (-228, -16);
        assert_eq!(
            after_relayout(Some(OWNER), spot, false, false, false, true),
            (Some(spot), false)
        );
        // Not during a drag, not for a system relocation, not while displaced.
        assert_eq!(after_relayout(Some(OWNER), spot, false, false, true, true).0, Some(OWNER));
        assert_eq!(after_relayout(Some(OWNER), spot, false, false, false, false).0, Some(OWNER));
        assert_eq!(after_relayout(Some(OWNER), spot, false, true, false, true).0, Some(OWNER));
    }

    /// Not a spec test: the spec pins home for a displaced widget's resize,
    /// but not the flag. A later resize that fits without clamping must leave
    /// the widget displaced, or the resize after it would adopt the temporary
    /// spot as home.
    #[test]
    fn displaced_widget_stays_displaced_through_unclamped_resizes() {
        let (home, displaced) = after_relayout(Some(OWNER), (-228, -16), true, false, false, true);
        let got = after_relayout(home, (-240, -16), false, displaced, false, true);
        assert_eq!(got, (Some(OWNER), true));
    }

    /// A tick's resize or a system relocation (track_home false) keeps home.
    /// When it puts the widget away from home, the widget is displaced, so
    /// the next resize that tracks home (an Apply) keeps home as well; a
    /// resize that lands at home ends the displacement.
    #[test]
    fn resize_without_track_home_away_from_home_displaces_the_widget() {
        let spot = (-213, -52);
        let (home, displaced) = after_relayout(Some(OWNER), spot, false, false, false, false);
        assert_eq!((home, displaced), (Some(OWNER), true));
        assert_eq!(after_relayout(home, spot, false, displaced, false, true), (Some(OWNER), true));
        // At home it is not displaced, and a resize that brings a displaced
        // widget back home ends the displacement, whether it tracks home or not.
        let at_home = after_relayout(Some(OWNER), OWNER, false, false, false, false);
        assert_eq!(at_home, (Some(OWNER), false));
        assert_eq!(after_relayout(Some(OWNER), OWNER, false, true, false, false), at_home);
        assert_eq!(after_relayout(Some(OWNER), OWNER, false, true, false, true), at_home);
    }

    #[test]
    fn click_without_move_keeps_home() {
        let p = (-213, -16);
        assert_eq!(drag_end(p, p), None);
        assert_eq!(drag_end(p, (-180, 40)), Some((-180, 40)));
    }

    /// Not a spec test: the two shims place_widget, relayout, snap_to_corner
    /// and the settings window's open rely on. A rect inside the primary work
    /// area resolves to that work area, and a rect far off every monitor still
    /// gets a non-empty one to fit into.
    #[test]
    fn work_area_shims_always_return_a_usable_area() {
        let primary = primary_work_area();
        assert!(primary.right > primary.left && primary.bottom > primary.top);
        let inside = rect(primary.left, primary.top, primary.left + 10, primary.top + 10);
        assert_eq!(work_area_near(&inside), primary);
        let far = work_area_near(&rect(-30000, -30000, -29800, -29784));
        assert!(far.right > far.left && far.bottom > far.top);
    }
}
