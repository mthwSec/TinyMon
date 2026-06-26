//! DPI awareness + scaling.
//!
//! TinyMon declares itself **Per-Monitor-V2** aware so Windows never bitmap-
//! stretches the widget (which would blur the text) and instead sends
//! `WM_DPICHANGED` when it moves between monitors with different scaling. All
//! layout sizes are authored in *logical* (96-dpi) units and converted to
//! device pixels through [`Scale`], so a "13px" font is the same physical size
//! on every display.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::HiDpi::{
    GetDpiForSystem, GetDpiForWindow, SetProcessDpiAwarenessContext,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

/// Make the process Per-Monitor-V2 DPI aware. Best-effort: on older Windows
/// without this API the call simply fails and the OS keeps system-DPI scaling.
/// Must run before any window or GDI sizing.
pub fn set_process_dpi_aware() {
    // SAFETY: a one-shot, process-global call with a static context handle.
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

/// DPI for a window's current monitor (96 = 100% if unavailable).
pub fn dpi_for_window(hwnd: HWND) -> u32 {
    // SAFETY: reads the window's DPI; returns 0 on failure, which we map to 96.
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 {
        96
    } else {
        dpi
    }
}

/// System (primary-monitor) DPI — used to size the window before it exists.
pub fn system_dpi() -> u32 {
    // SAFETY: no arguments; returns the system DPI.
    let dpi = unsafe { GetDpiForSystem() };
    if dpi == 0 {
        96
    } else {
        dpi
    }
}

/// A DPI scale factor (the raw dots-per-inch, 96 == 100%).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Scale(pub u32);

impl Scale {
    /// Convert a logical (96-dpi) length to physical device pixels, rounded to
    /// nearest. `Scale(96)` is the identity.
    pub fn px(self, design: i32) -> i32 {
        (design * self.0 as i32 + 48) / 96
    }
}

#[cfg(test)]
mod tests {
    use super::Scale;

    #[test]
    fn identity_at_96() {
        let s = Scale(96);
        for v in [0, 8, 13, 200, 437] {
            assert_eq!(s.px(v), v);
        }
    }

    #[test]
    fn scales_125_150_200() {
        assert_eq!(Scale(120).px(13), 16); // 16.25 -> 16
        assert_eq!(Scale(120).px(10), 13); // 12.5 -> 13 (round half up)
        assert_eq!(Scale(144).px(8), 12); // 12.0
        assert_eq!(Scale(144).px(13), 20); // 19.5 -> 20
        assert_eq!(Scale(144).px(200), 300); // 300.0
        assert_eq!(Scale(192).px(8), 16); // 16.0
        assert_eq!(Scale(192).px(200), 400); // 400.0
    }
}
