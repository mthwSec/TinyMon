//! DPI awareness + scaling.
//!
//! TinyMon declares itself **Per-Monitor-V2** aware so Windows never bitmap-
//! stretches the widget (which would blur the text) and instead sends
//! `WM_DPICHANGED` when it moves between monitors with different scaling. All
//! layout sizes are authored in *logical* (96-dpi) units and converted to
//! device pixels through [`Scale`], so a "13px" font is the same physical size
//! on every display.

use windows::Win32::Foundation::{HWND, WPARAM};
use windows::Win32::UI::HiDpi::{
    GetDpiForSystem, GetDpiForWindow, SetProcessDpiAwarenessContext,
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};

/// Make the process Per-Monitor-V2 DPI aware. The embedded manifest
/// (`tinymon.manifest`) already declares PMv2, so the loader applies it before
/// `main` and this call then fails harmlessly; it is the fallback for a build
/// without resources. Must run before any window or GDI sizing. The real minimum
/// is Windows 10 1703: this and the other DPI functions are static imports, so
/// an older Windows refuses to start the exe rather than falling back.
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

/// The DPI a WM_DPICHANGED reports in `wparam`: the low word (the high word
/// repeats it), with 0 read as 96. The widget's and the settings window's
/// WM_DPICHANGED arms both read it here.
pub fn dpi_from_wparam(wparam: WPARAM) -> u32 {
    match (wparam.0 & 0xFFFF) as u32 {
        0 => 96,
        dpi => dpi,
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
    use super::{dpi_from_wparam, Scale};
    use windows::Win32::Foundation::WPARAM;

    #[test]
    fn dpi_change_reads_the_new_dpi() {
        // WM_DPICHANGED's wParam holds the new DPI in both words; the low word
        // counts, and 0, which would shrink the widget and every settings
        // control to nothing, is 96.
        assert_eq!(dpi_from_wparam(WPARAM(0x0078_0078)), 120);
        assert_eq!(dpi_from_wparam(WPARAM(0)), 96);
    }

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

    /// build.rs compiles tinymon.rc, which embeds `tinymon.manifest` as
    /// RT_MANIFEST #1 next to the version info and the icon, and links those
    /// resources into the test binary too. A build without the resource
    /// compiler fails here instead of shipping an exe without them.
    #[test]
    fn exe_embeds_minimal_manifest() {
        use windows::core::PCWSTR;
        use windows::Win32::System::LibraryLoader::{
            FindResourceW, LoadResource, LockResource, SizeofResource,
        };
        use windows::Win32::UI::WindowsAndMessaging::{RT_GROUP_ICON, RT_MANIFEST, RT_VERSION};
        let id1 = PCWSTR(1 as _);
        // SAFETY: looks up resources of this executable (module None); resource
        // data stays mapped for the life of the process and is only read here.
        unsafe {
            assert!(!FindResourceW(None, id1, RT_VERSION).is_invalid(), "no RT_VERSION #1");
            let res = FindResourceW(None, id1, RT_MANIFEST);
            assert!(!res.is_invalid(), "no RT_MANIFEST #1 resource");
            let data = LoadResource(None, res).expect("LoadResource");
            let len = SizeofResource(None, res) as usize;
            let ptr = LockResource(data) as *const u8;
            assert!(!ptr.is_null() && len > 0);
            let text = std::str::from_utf8(std::slice::from_raw_parts(ptr, len)).expect("UTF-8");
            assert!(text.contains("PerMonitorV2"));
            assert!(text.contains("{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"));
            assert!(text.contains("asInvoker"));
            assert!(!text.contains("Microsoft.Windows.Common-Controls"));
            assert!(!FindResourceW(None, id1, RT_GROUP_ICON).is_invalid(), "no RT_GROUP_ICON #1");
        }
    }
}
