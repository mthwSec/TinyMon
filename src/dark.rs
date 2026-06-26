//! Dark theming for the settings window, so its native Win32 controls share the
//! widget's dark identity instead of clashing as a light system dialog.
//!
//! Two layers, both best-effort (a failure just leaves a control light, it never
//! breaks the window):
//!   * **Documented** — a dark title bar via `DwmSetWindowAttribute`, a dark
//!     window background brush, and `WM_CTLCOLOR*` replies that paint label /
//!     edit text light on dark. This alone covers backgrounds + text.
//!   * **uxtheme dark mode** — the (undocumented, but stable since Win10 1903)
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

use windows::core::{w, PCSTR, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HMODULE, HWND};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, HBRUSH};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
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
/// Secondary / muted text (section titles, the opacity readout).
pub const TEXT_MUTED: u32 = rgb(0x9A, 0xA0, 0xA6);

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

type FnSetPreferredAppMode = unsafe extern "system" fn(i32) -> i32;
type FnAllowDarkModeForWindow = unsafe extern "system" fn(HWND, i32) -> i32;
type FnRefresh = unsafe extern "system" fn();

unsafe fn uxtheme() -> Option<HMODULE> {
    // SAFETY: uxtheme.dll ships with every supported Windows; null on failure.
    let h = LoadLibraryW(w!("uxtheme.dll")).ok()?;
    (!h.is_invalid()).then_some(h)
}

/// Turn on process-wide dark mode so the common controls paint their chrome
/// dark. Call once at startup, before any settings window is created.
pub unsafe fn init() {
    let Some(h) = uxtheme() else { return };
    // Ordinal 135 = `SetPreferredAppMode(PreferredAppMode)`; 2 == ForceDark.
    if let Some(p) = GetProcAddress(h, PCSTR(135usize as *const u8)) {
        // SAFETY: the ordinal resolves to SetPreferredAppMode (uxtheme 1903+).
        let f: FnSetPreferredAppMode = transmute(p);
        f(2);
    }
    // Ordinal 104 = `RefreshImmersiveColorPolicyState()`.
    if let Some(p) = GetProcAddress(h, PCSTR(104usize as *const u8)) {
        // SAFETY: the ordinal resolves to a no-argument refresh routine.
        let f: FnRefresh = transmute(p);
        f();
    }
}

/// Opt a top-level window into dark mode and give it a dark title bar.
pub unsafe fn apply_window(hwnd: HWND) {
    if let Some(h) = uxtheme() {
        // Ordinal 133 = `AllowDarkModeForWindow(HWND, BOOL)`.
        if let Some(p) = GetProcAddress(h, PCSTR(133usize as *const u8)) {
            // SAFETY: the ordinal resolves to AllowDarkModeForWindow.
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
