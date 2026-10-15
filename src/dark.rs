//! Dark theming for the settings window, so its native Win32 controls share the
//! widget's dark identity instead of clashing as a light system dialog.
//!
//! Two layers, both best-effort (a failure just leaves a control light, it never
//! breaks the window):
//!   * **Documented** — a dark title bar via `DwmSetWindowAttribute`, a dark
//!     window background brush, and `WM_CTLCOLOR*` replies that paint label /
//!     edit text light on dark. This alone covers backgrounds + text.
//!   * **uxtheme dark mode** — the (undocumented, but stable since Win10 1903,
//!     and called only from 1809 on: `app_mode_arg`)
//!     `SetPreferredAppMode` / `AllowDarkModeForWindow` entry points plus
//!     `SetWindowTheme(.., "DarkMode_Explorer")` ask the common controls to
//!     render their *chrome* (button faces, check glyphs, the trackbar, scroll
//!     bars) dark — things `WM_CTLCOLOR*` can't reach.
//!
//! The palette is a cohesive cool-toned **"slate"** dark theme (window →
//! elevated card → recessed input), tuned to read as an intentional, modern app
//! rather than a flat gray box. The settings window draws its own card panels
//! (see `settings_win`) using these tokens, so the colors all live here.

use core::cell::Cell;
use core::ffi::c_void;
use core::mem::{size_of, transmute};
use std::sync::OnceLock;

use windows::core::{s, w, PCSTR, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HMODULE, HWND};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, HBRUSH};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::UI::Controls::SetWindowTheme;

/// Pack an (R, G, B) triple into a Win32 `COLORREF` (`0x00BBGGRR`). `const` so the
/// palette below can be authored in readable hex-RGB instead of byte-swapped.
const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

// ---- Palette (slate dark, three elevation layers) --------------------------
/// Window background — the lowest layer; cards float above it.
pub const WINDOW_BG: u32 = rgb(0x1E, 0x20, 0x24);
/// Card / panel surface — one step lighter than the window (subtle elevation).
pub const PANEL_BG: u32 = rgb(0x26, 0x28, 0x2E);
/// Input-field background (edit + hotkey controls) — recessed, darker than cards.
pub const EDIT_BG: u32 = rgb(0x16, 0x18, 0x1C);
/// Primary foreground text on the dark surfaces.
pub const TEXT: u32 = rgb(0xE8, 0xEA, 0xED);
/// Secondary / muted text (section titles, a hotkey field's Win note).
pub const TEXT_MUTED: u32 = rgb(0x9A, 0xA0, 0xA6);
/// Amber warning text on a card (the hotkey notes that warn): 7.5:1 on
/// PANEL_BG.
pub const WARN: u32 = rgb(0xE8, 0xB0, 0x4A);

// Owner-draw palette for the controls Win10 refuses to theme dark itself
// (push buttons, the trackbar, check glyphs). All COLORREF = 0x00BBGGRR.
/// Subtle 1px outline for cards, inputs, and secondary buttons.
pub const BORDER: u32 = rgb(0x3A, 0x3D, 0x42);
/// The widget's CPU cyan — accent for the primary button fill, focus rings, the
/// check mark, and the slider thumb/fill.
pub const ACCENT: u32 = rgb(0x46, 0xC8, 0xE0);
/// Accent, pressed (a darker cyan for the primary button's down state).
pub const ACCENT_DOWN: u32 = rgb(0x36, 0xA0, 0xB4);
/// Near-black text/glyph drawn ON the cyan accent (the primary button caption,
/// the checkbox tick) — cyan is light, so its foreground must be dark.
pub const ON_ACCENT: u32 = rgb(0x0E, 0x16, 0x1A);
/// Secondary button face (normal / pressed).
pub const BTN_FACE: u32 = rgb(0x2E, 0x31, 0x36);
pub const BTN_FACE_DOWN: u32 = rgb(0x22, 0x25, 0x29);
/// Slider channel (unfilled track).
pub const TRACK: u32 = rgb(0x34, 0x37, 0x3C);

thread_local! {
    static BG_BRUSH: Cell<isize> = const { Cell::new(0) };
    static PANEL_BRUSH: Cell<isize> = const { Cell::new(0) };
    static EDIT_BRUSH: Cell<isize> = const { Cell::new(0) };
}

/// Cache a process-lifetime solid brush in `cell` (created once, never freed —
/// `WM_CTLCOLOR*` must return a brush that outlives the call).
unsafe fn cached_brush(cell: &'static std::thread::LocalKey<Cell<isize>>, color: u32) -> HBRUSH {
    cell.with(|c| {
        if c.get() == 0 {
            c.set(CreateSolidBrush(COLORREF(color)).0 as isize);
        }
        HBRUSH(c.get() as *mut c_void)
    })
}

/// The window-background brush (also used as the class brush).
pub unsafe fn bg_brush() -> HBRUSH {
    cached_brush(&BG_BRUSH, WINDOW_BG)
}

/// The card-surface brush (label backgrounds, so text blends onto its panel).
pub unsafe fn panel_brush() -> HBRUSH {
    cached_brush(&PANEL_BRUSH, PANEL_BG)
}

/// The input-field background brush (edit + hotkey controls).
pub unsafe fn edit_brush() -> HBRUSH {
    cached_brush(&EDIT_BRUSH, EDIT_BG)
}

/// Ordinal 135: SetPreferredAppMode(PreferredAppMode) from 1903 on,
/// AllowDarkModeForApp(BOOL) on 1809; both take one 32-bit argument.
type FnSetPreferredAppMode = unsafe extern "system" fn(i32) -> i32;
type FnAllowDarkModeForWindow = unsafe extern "system" fn(HWND, i32) -> i32;
type FnRefresh = unsafe extern "system" fn();
type FnRtlGetVersion = unsafe extern "system" fn(*mut OSVERSIONINFOW) -> i32;

/// The argument for uxtheme ordinal 135 on Windows build `build`, or None
/// where the dark-mode ordinals (104, 133 and 135) do not exist: they arrived
/// in 1809 (17763), and older uxtheme builds hold other exports at those
/// ordinals. On 1809 ordinal 135 is AllowDarkModeForApp(BOOL), so 1 allows
/// dark mode; from 1903 (18362) on it is SetPreferredAppMode, and 2 is
/// ForceDark.
fn app_mode_arg(build: u32) -> Option<i32> {
    match build {
        18362.. => Some(2),
        17763.. => Some(1),
        _ => None,
    }
}

/// The real Windows build number, read once with ntdll's RtlGetVersion,
/// which reports the true version whatever the manifest declares; 0 if it
/// cannot be read.
fn os_build() -> u32 {
    static BUILD: OnceLock<u32> = OnceLock::new();
    *BUILD.get_or_init(|| {
        // SAFETY: ntdll.dll is loaded in every process; RtlGetVersion fills the
        // OSVERSIONINFOW whose size field is set and returns an NTSTATUS.
        unsafe {
            let Ok(ntdll) = GetModuleHandleW(w!("ntdll.dll")) else { return 0 };
            let Some(p) = GetProcAddress(ntdll, s!("RtlGetVersion")) else { return 0 };
            let f: FnRtlGetVersion = transmute(p);
            let mut info = OSVERSIONINFOW {
                dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
                ..Default::default()
            };
            if f(&mut info) == 0 {
                info.dwBuildNumber
            } else {
                0
            }
        }
    })
}

/// uxtheme.dll, which holds the dark-mode entry points; None if it cannot be
/// loaded.
unsafe fn uxtheme() -> Option<HMODULE> {
    // SAFETY: uxtheme.dll ships with every supported Windows; null on failure.
    let h = LoadLibraryW(w!("uxtheme.dll")).ok()?;
    (!h.is_invalid()).then_some(h)
}

/// Turn on process-wide dark mode so the common controls paint their chrome
/// dark. Call once at startup, before any settings window is created.
/// Does nothing before 1809, where the ordinals do not exist (app_mode_arg).
pub unsafe fn init() {
    let Some(mode) = app_mode_arg(os_build()) else { return };
    let Some(h) = uxtheme() else { return };
    // Ordinal 135 = `SetPreferredAppMode(ForceDark)` from 1903 on,
    // `AllowDarkModeForApp(TRUE)` on 1809 (app_mode_arg).
    if let Some(p) = GetProcAddress(h, PCSTR(135usize as *const u8)) {
        // SAFETY: on the builds app_mode_arg allows (1809+), ordinal 135 is one
        // of the two functions above, each taking one 32-bit argument.
        let f: FnSetPreferredAppMode = transmute(p);
        f(mode);
    }
    // Ordinal 104 = `RefreshImmersiveColorPolicyState()`.
    if let Some(p) = GetProcAddress(h, PCSTR(104usize as *const u8)) {
        // SAFETY: on 1809+ the ordinal resolves to a no-argument refresh routine.
        let f: FnRefresh = transmute(p);
        f();
    }
}

/// Opt a top-level window into dark mode and give it a dark title bar.
pub unsafe fn apply_window(hwnd: HWND) {
    let ordinals = app_mode_arg(os_build()).is_some();
    if let Some(h) = ordinals.then(|| uxtheme()).flatten() {
        // Ordinal 133 = `AllowDarkModeForWindow(HWND, BOOL)`.
        if let Some(p) = GetProcAddress(h, PCSTR(133usize as *const u8)) {
            // SAFETY: on 1809+ the ordinal resolves to AllowDarkModeForWindow.
            let f: FnAllowDarkModeForWindow = transmute(p);
            f(hwnd, 1);
        }
    }
    // Dark title bar (documented DWM attribute, Win10 2004+). Best-effort.
    let on: i32 = 1;
    // SAFETY: DWMWA_USE_IMMERSIVE_DARK_MODE expects a 4-byte BOOL by pointer.
    let _ = DwmSetWindowAttribute(
        hwnd,
        DWMWA_USE_IMMERSIVE_DARK_MODE,
        &on as *const i32 as *const c_void,
        size_of::<i32>() as u32,
    );
}

/// Give a child control the dark common-control theme (button faces, the
/// trackbar, check glyphs, scroll bars). Harmless on controls that ignore it.
pub unsafe fn apply_control(hctrl: HWND) {
    // SAFETY: SetWindowTheme just swaps the visual-style class for this control.
    let _ = SetWindowTheme(hctrl, w!("DarkMode_Explorer"), PCWSTR::null());
}

#[cfg(test)]
mod tests {
    use super::{app_mode_arg, os_build};

    /// The uxtheme dark-mode ordinals exist from 1809 (17763) on; ordinal 135
    /// is AllowDarkModeForApp(BOOL) on 1809 and SetPreferredAppMode from 1903
    /// (18362) on. Older builds hold other exports at those ordinals.
    #[test]
    fn dark_mode_ordinals_follow_the_os_build() {
        for (build, want) in [
            (0, None),
            (15063, None),    // 1703
            (17134, None),    // 1803
            (17763, Some(1)), // 1809: AllowDarkModeForApp(TRUE)
            (18361, Some(1)),
            (18362, Some(2)), // 1903: SetPreferredAppMode(ForceDark)
            (19045, Some(2)),
            (22631, Some(2)),
        ] {
            assert_eq!(app_mode_arg(build), want, "build {build}");
        }
    }

    /// Reads the live system: the test machine runs Windows 10 or later.
    #[test]
    fn os_build_reads_the_real_build() {
        assert!(os_build() >= 10240, "{}", os_build());
    }
}
