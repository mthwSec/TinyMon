//! The settings window: native Win32 controls grouped into **Metrics**,
//! **Appearance**, and **Hotkeys** sections plus OK / Apply / Cancel.
//!
//! It has its own window class + wndproc and never aliases the main `AppState`.
//! It seeds its controls from `current_settings(main)` and, on Apply/OK, builds
//! a fresh `Settings` (clamped via `Settings::from_config`) and hands it to the
//! main window with `queue_apply`, which keeps it in a UI-thread slot and posts
//! a bare `WM_APP_APPLY`; one combo for both hotkeys is refused, and OK then
//! stays open. After an Apply the main window posts `WM_APP_HOTKEY_STATUS`, and
//! the notes beside the hotkey fields show which combos failed to register
//! (`hotkey_status`). Closing posts `WM_APP_SETTINGS_CLOSED` (it never quits
//! the app).
//!
//! Keyboard navigation (Tab between fields, Esc = Cancel, Space toggles a
//! checkbox or presses a button, Enter presses the focused push button and
//! means OK anywhere else) is provided by `IsDialogMessageW` in the main
//! message loop. OK and Cancel use the standard `IDOK` / `IDCANCEL` ids for it:
//! Esc sends IDCANCEL, and Enter sends the id this window gives for
//! `DM_GETDEFID` (`enter_target`).

use core::ffi::c_void;
use core::mem::size_of;

use core::cell::{Cell, RefCell};

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AlphaBlend, BeginPaint, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontIndirectW,
    CreatePen, CreateSolidBrush, DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect,
    GetStockObject, InvalidateRect, Polyline, RoundRect, SelectObject, SetBkColor, SetBkMode,
    SetBrushOrgEx, SetDCBrushColor, SetStretchBltMode, SetTextColor, StretchBlt, BLENDFUNCTION,
    CLEARTYPE_QUALITY, DC_BRUSH, DRAW_TEXT_FORMAT, DT_CENTER, DT_LEFT, DT_SINGLELINE, DT_VCENTER,
    HALFTONE, HBRUSH, HDC, HFONT, HGDIOBJ, LOGFONTW, NULL_BRUSH, PAINTSTRUCT, PS_SOLID, SRCCOPY,
    TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    InitCommonControlsEx, CDDS_ITEMPREPAINT, CDDS_PREPAINT, DRAWITEMSTRUCT, EM_LIMITTEXT,
    HKM_GETHOTKEY, HKM_SETHOTKEY, HOTKEYF_ALT, HOTKEYF_CONTROL, HOTKEYF_EXT, HOTKEYF_SHIFT,
    ICC_BAR_CLASSES, ICC_HOTKEY_CLASS, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX, NMCUSTOMDRAW,
    NMHDR, ODS_DEFAULT, ODS_FOCUS, ODS_SELECTED, TBM_SETPOS, TBM_SETRANGE,
};
use windows::Win32::UI::HiDpi::AdjustWindowRectExForDpi;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, MapVirtualKeyW, SetFocus, MAPVK_VK_TO_VSC_EX, MOD_ALT, MOD_CONTROL, MOD_SHIFT,
    MOD_WIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetDlgCtrlID, GetDlgItem,
    GetParent, GetWindowLongPtrW, GetWindowRect, GetWindowTextW, IsChild, LoadCursorW, LoadIconW,
    PostMessageW, RegisterClassW, SendMessageW, SetWindowLongPtrW, SetWindowPos, ShowWindow,
    BN_DOUBLECLICKED, BS_OWNERDRAW, CREATESTRUCTW, CW_USEDEFAULT, DC_HASDEFID, DM_GETDEFID,
    ES_NUMBER, GWLP_USERDATA, HMENU, IDC_ARROW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SW_SHOW,
    WA_INACTIVE, WINDOW_EX_STYLE, WINDOW_STYLE, WM_ACTIVATE, WM_COMMAND, WM_CREATE, WM_CTLCOLORBTN,
    WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DPICHANGED, WM_DRAWITEM, WM_ERASEBKGND,
    WM_GETFONT, WM_HSCROLL, WM_KEYDOWN, WM_KEYUP, WM_NOTIFY, WM_PAINT, WM_SETFOCUS, WM_SETFONT,
    WM_SETTEXT, WM_SYSKEYDOWN, WM_SYSKEYUP, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD,
    WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};

use crate::config::Config;
use crate::dark;
use crate::dpi::{dpi_for_window, Scale};
use crate::hotkeys::{blocks_typing, format_hotkey, HotkeySpec};
use crate::placement::{fit_into, work_area_near};
use crate::registry::{sample_rows, MetricKey};
use crate::render::{render_rows, win_w, window_height, LIGHT, THEME};
use crate::settings::{Settings, MAX_FONT_PX, MAX_INTERVAL_MS, MIN_FONT_PX, MIN_INTERVAL_MS};
use crate::{
    create_mono_font, current_settings, hotkey_status, queue_apply, WM_APP_HOTKEY_STATUS,
    WM_APP_SETTINGS_CLOSED,
};

const CLASS: PCWSTR = w!("TinyMonSettings");
const TBM_GETPOS: u32 = 1024; // WM_USER+0 (not exposed as a named const in 0.62)
const TBM_GETRANGEMIN: u32 = 1025; // WM_USER+1
const TBM_GETRANGEMAX: u32 = 1026; // WM_USER+2

// OK / Cancel use the standard dialog ids, which `IsDialogMessageW` sends for
// Esc (IDCANCEL) and for Enter while no push button has the focus (IDOK,
// through DM_GETDEFID and `enter_target`); everything else gets its own id.
const IDOK: i32 = 1;
const IDCANCEL: i32 = 2;
const IDC_APPLY: i32 = 3;
const IDC_OPACITY_VAL: i32 = 1009;
const IDC_OPACITY: i32 = 1010;
const IDC_INTERVAL: i32 = 1011;
const IDC_FONT: i32 = 1012;
const IDC_HK_TOGGLE: i32 = 1013;
const IDC_HK_SETTINGS: i32 = 1014;
const IDC_HK_TOGGLE_CLR: i32 = 1015;
const IDC_HK_SETTINGS_CLR: i32 = 1016;
const IDC_METRIC_BASE: i32 = 1100; // per-row metric checkbox (base + position)
const IDC_MOVEUP_BASE: i32 = 1200; // per-row "move up" button
const IDC_MOVEDN_BASE: i32 = 1300; // per-row "move down" button
const IDC_PREVIEW: i32 = 1020;
const IDC_RESET: i32 = 1021;
// The static labels, each naming the control beside or below it.
const IDC_LBL_INTERVAL: i32 = 1022;
const IDC_LBL_FONT: i32 = 1023;
const IDC_LBL_HK_TOGGLE: i32 = 1024;
const IDC_LBL_HK_SETTINGS: i32 = 1025;
// The notes on the hotkey labels' rows (hotkey_notes).
const IDC_HK_TOGGLE_NOTE: i32 = 1026;
const IDC_HK_SETTINGS_NOTE: i32 = 1027;
const EN_CHANGE: u32 = 0x0300;
/// SS_ENDELLIPSIS: a static's text that does not fit ends in an ellipsis on
/// its one line, instead of wrapping onto a second line that its height clips
/// (the windows crate has it only under a feature TinyMon does not enable).
const SS_ENDELLIPSIS: u32 = 0x4000;
/// SS_RIGHT: a static's text aligns right (a local constant for the same
/// reason).
const SS_RIGHT: u32 = 0x0002;

/// The interval and font labels. They state the ranges apply() clamps to, as
/// literals (range_labels_match_the_clamp_constants keeps them in step).
const INTERVAL_LABEL: &str = "Update interval (250\u{2013}10000 ms)";
const FONT_LABEL: &str = "Font size (9\u{2013}16 px)";
/// The most characters the interval and font edits take (EM_LIMITTEXT): the
/// digits of their largest values.
const INTERVAL_CHARS: usize = 5;
const FONT_CHARS: usize = 2;

thread_local! {
    /// The settings window's DPI (96 = 100%), set in `build_controls` and again
    /// on WM_DPICHANGED. Every coordinate/size/font goes through `dscale()` so
    /// the window scales on high-DPI displays (the layout is authored in
    /// logical 96-dpi units).
    static SCALE: Cell<u32> = const { Cell::new(96) };
    /// The settings window's Segoe UI fonts — regular for controls, semibold for
    /// section titles. Built per-open at the window's DPI and again on a DPI
    /// change; freed on close.
    static UI_FONT: Cell<isize> = const { Cell::new(0) };
    static HDR_FONT: Cell<isize> = const { Cell::new(0) };
    /// The working metric list: every metric in display order, each with its
    /// enabled flag. The row checkboxes mirror this; ▲/▼ reorder it; Apply reads it.
    static METRIC_STATE: RefCell<Vec<(MetricKey, bool)>> = const { RefCell::new(Vec::new()) };
    /// The id of the control that had the focus when the window was last
    /// deactivated (0 until then); WM_SETFOCUS gives the focus back to it.
    static LAST_FOCUS: Cell<i32> = const { Cell::new(0) };
}

/// The settings window's current DPI scale (SCALE).
fn dscale() -> Scale {
    Scale(SCALE.with(|c| c.get()))
}

/// Seed the metric list from settings: enabled metrics first (in their saved
/// order), then the remaining (disabled) ones in the canonical order.
fn init_metric_state(s: &Settings) {
    let mut state: Vec<(MetricKey, bool)> = s.order.iter().map(|k| (*k, true)).collect();
    for k in MetricKey::ALL {
        if !s.order.contains(&k) {
            state.push((k, false));
        }
    }
    METRIC_STATE.with(|st| *st.borrow_mut() = state);
}

/// Re-label + re-check every metric row from `METRIC_STATE` (after a toggle/move).
unsafe fn render_metric_rows(hwnd: HWND) {
    METRIC_STATE.with(|st| {
        for (p, (key, checked)) in st.borrow().iter().enumerate() {
            if let Ok(cb) = GetDlgItem(Some(hwnd), IDC_METRIC_BASE + p as i32) {
                set_text(cb, key.label());
                set_cb_checked(cb, *checked);
                let _ = InvalidateRect(Some(cb), None, true);
            }
        }
    });
}

/// The enabled metrics in their current display order (read by Apply + preview).
fn enabled_order() -> Vec<MetricKey> {
    METRIC_STATE.with(|st| st.borrow().iter().filter(|(_, en)| *en).map(|(k, _)| *k).collect())
}

/// Build a Segoe UI font `px` logical pixels tall with `weight` (400 = regular,
/// 600 = semibold), scaled to the window's DPI. The dated `DEFAULT_GUI_FONT`
/// (Tahoma/MS Sans Serif) is the biggest "un-modern" tell, so every control uses
/// Segoe UI.
unsafe fn create_ui_font(px: i32, weight: i32) -> HFONT {
    let mut lf = LOGFONTW {
        lfHeight: -dscale().px(px),
        lfWeight: weight,
        lfQuality: CLEARTYPE_QUALITY,
        ..Default::default()
    };
    for (i, c) in "Segoe UI".encode_utf16().enumerate() {
        lf.lfFaceName[i] = c;
    }
    CreateFontIndirectW(&lf)
}

/// Create the cached UI fonts at the current DPI scale. Returns the fonts they
/// replace (0 where there was none), for the caller to free with
/// `delete_fonts` once no control uses them any more.
unsafe fn make_fonts() -> [isize; 2] {
    [
        UI_FONT.with(|c| c.replace(create_ui_font(12, 400).0 as isize)),
        HDR_FONT.with(|c| c.replace(create_ui_font(12, 600).0 as isize)),
    ]
}

/// The regular control font (null until `make_fonts`).
unsafe fn ui_font() -> HFONT {
    HFONT(UI_FONT.with(|c| c.get()) as *mut c_void)
}
/// The semibold section-title font.
unsafe fn hdr_font() -> HFONT {
    HFONT(HDR_FONT.with(|c| c.get()) as *mut c_void)
}
/// Delete fonts given by their raw handles (0 = none).
unsafe fn delete_fonts(fonts: [isize; 2]) {
    for h in fonts {
        if h != 0 {
            let _ = DeleteObject(HGDIOBJ(h as *mut c_void));
        }
    }
}
/// Delete the cached fonts (on settings-window close).
unsafe fn free_fonts() {
    delete_fonts([UI_FONT.with(|c| c.replace(0)), HDR_FONT.with(|c| c.replace(0))]);
}

const CLIENT_W: i32 = 280; // left controls column
const GUTTER: i32 = 12;
/// The preview column. Its pane (PREVIEW_W - 20 wide, less a 1 px border on
/// each side) holds the widest widget, at MAX_FONT_PX, 1:1 at every scale.
const PREVIEW_W: i32 = 270;
const CLIENT_W_TOTAL: i32 = CLIENT_W + GUTTER + PREVIEW_W;
const PREVIEW_CLASS: PCWSTR = w!("TinyMonPreview");

/// The settings window's style: fixed size, a caption and a close box.
const STYLE: WINDOW_STYLE = WINDOW_STYLE(WS_OVERLAPPED.0 | WS_CAPTION.0 | WS_SYSMENU.0);

/// Vertical layout of the three sections for `n` metric rows, in logical
/// units, shared by `open` (to size the window), `control_places` (to place
/// the controls) and `paint_chrome` (the cards), so they can't drift apart.
struct Layout {
    metrics_top: i32,
    metrics_h: i32,
    app_top: i32,
    app_h: i32,
    hk_top: i32,
    hk_h: i32,
    btn_top: i32,
    client_h: i32,
}

fn layout(n: i32) -> Layout {
    let metrics_top = 8;
    let metrics_h = 18 + n * 22 + 6; // one row per metric (single ordered list)
    let app_top = metrics_top + metrics_h + 8;
    let app_h = 140;
    let hk_top = app_top + app_h + 8;
    let hk_h = 116;
    let btn_top = hk_top + hk_h + 12;
    let client_h = btn_top + 26 + 12;
    Layout { metrics_top, metrics_h, app_top, app_h, hk_top, hk_h, btn_top, client_h }
}

/// One child control's rectangle, in logical (96-dpi) client units.
#[derive(Clone, Copy, Default, Debug)]
struct Place {
    id: i32,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

impl Place {
    /// This Place in device pixels at scale `s`: (x, y, w, h).
    fn scaled(self, s: Scale) -> (i32, i32, i32, i32) {
        (s.px(self.x), s.px(self.y), s.px(self.w), s.px(self.h))
    }
}

/// Where each child control goes with `n` metric rows (callers pass
/// MetricKey::ALL.len()): the single layout source for every child. Static
/// labels are 18 px tall; at 16 px, 150% and 175% clip their descenders.
fn control_places(n: i32) -> Vec<Place> {
    let lay = layout(n);
    let at = |id, x, y, w, h| Place { id, x, y, w, h };
    let mut v = Vec::with_capacity(3 * n as usize + CREATION_ORDER.len());
    // Metrics: one row per metric, a checkbox and its up and down arrows.
    for p in 0..n {
        let y = lay.metrics_top + 18 + p * 22;
        v.push(at(IDC_METRIC_BASE + p, 22, y, 190, 18));
        v.push(at(IDC_MOVEUP_BASE + p, 216, y, 22, 18));
        v.push(at(IDC_MOVEDN_BASE + p, 240, y, 22, 18));
    }
    let (a, k, b) = (lay.app_top, lay.hk_top, lay.btn_top);
    v.extend([
        // Appearance.
        at(IDC_OPACITY_VAL, 22, a + 22, 220, 18),
        at(IDC_OPACITY, 20, a + 40, 238, 28),
        at(IDC_LBL_INTERVAL, 22, a + 80, 172, 18),
        at(IDC_INTERVAL, 196, a + 78, 62, 22),
        at(IDC_LBL_FONT, 22, a + 108, 172, 18),
        at(IDC_FONT, 196, a + 106, 62, 22),
        // Hotkeys: each capture field has a Clear button, and a note at the
        // right end of its label's row.
        at(IDC_LBL_HK_TOGGLE, 22, k + 20, 78, 18),
        at(IDC_HK_TOGGLE_NOTE, 102, k + 20, 154, 18),
        at(IDC_HK_TOGGLE, 22, k + 38, 170, 22),
        at(IDC_HK_TOGGLE_CLR, 200, k + 38, 56, 22),
        at(IDC_LBL_HK_SETTINGS, 22, k + 66, 78, 18),
        at(IDC_HK_SETTINGS_NOTE, 102, k + 66, 154, 18),
        at(IDC_HK_SETTINGS, 22, k + 84, 170, 22),
        at(IDC_HK_SETTINGS_CLR, 200, k + 84, 56, 22),
        // The buttons, below the cards.
        at(IDC_RESET, 12, b, 64, 26),
        at(IDOK, 104, b, 52, 26),
        at(IDC_APPLY, 160, b, 52, 26),
        at(IDCANCEL, 216, b, 52, 26),
        // The live preview fills its card in the right column.
        at(IDC_PREVIEW, CLIENT_W + GUTTER + 10, 26, PREVIEW_W - 20, lay.client_h - 44),
    ]);
    v
}

/// The controls build_controls creates after the metric rows, in creation
/// order, which is also the Tab order and the z-order. A screen reader names
/// an input after the closest static before it in z-order, so each input
/// comes right after its label, and each hotkey note before its label.
const CREATION_ORDER: [i32; 19] = [
    IDC_OPACITY_VAL,
    IDC_OPACITY,
    IDC_LBL_INTERVAL,
    IDC_INTERVAL,
    IDC_LBL_FONT,
    IDC_FONT,
    IDC_HK_TOGGLE_NOTE,
    IDC_LBL_HK_TOGGLE,
    IDC_HK_TOGGLE,
    IDC_HK_TOGGLE_CLR,
    IDC_HK_SETTINGS_NOTE,
    IDC_LBL_HK_SETTINGS,
    IDC_HK_SETTINGS,
    IDC_HK_SETTINGS_CLR,
    IDC_RESET,
    IDOK,
    IDC_APPLY,
    IDCANCEL,
    IDC_PREVIEW,
];

/// Every control build_controls creates for `n` metric rows, in creation
/// (Tab) order: each row's checkbox, up and down arrow, then CREATION_ORDER.
fn created_ids(n: i32) -> impl Iterator<Item = i32> {
    (0..n)
        .flat_map(|p| [IDC_METRIC_BASE + p, IDC_MOVEUP_BASE + p, IDC_MOVEDN_BASE + p])
        .chain(CREATION_ORDER)
}

/// How much wider and taller than its client area the settings window is at
/// `dpi`: its caption and borders.
fn frame_size(dpi: u32) -> (i32, i32) {
    let mut r = RECT::default();
    // SAFETY: AdjustWindowRectExForDpi only writes the local `r`; if it fails,
    // `r` stays empty and the frame counts as nothing.
    let _ = unsafe { AdjustWindowRectExForDpi(&mut r, STYLE, false, WINDOW_EX_STYLE(0), dpi) };
    (r.right - r.left, r.bottom - r.top)
}

/// The settings window's size at scale `s`: its client area (with every
/// metric row) plus its frame.
fn window_size(s: Scale) -> (i32, i32) {
    let (fw, fh) = frame_size(s.0);
    let client_h = layout(MetricKey::ALL.len() as i32).client_h;
    (s.px(CLIENT_W_TOTAL) + fw, s.px(client_h) + fh)
}

/// The DPI a WM_DPICHANGED reports in `wparam`: the low word (the high word
/// repeats it), with 0 read as 96.
fn dpi_from_wparam(wparam: WPARAM) -> u32 {
    match (wparam.0 & 0xFFFF) as u32 {
        0 => 96,
        dpi => dpi,
    }
}

/// Register the settings window class + init common controls (once at startup).
pub unsafe fn register_class(hinstance: HINSTANCE) {
    let icc = INITCOMMONCONTROLSEX {
        dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_BAR_CLASSES | ICC_HOTKEY_CLASS | ICC_STANDARD_CLASSES,
    };
    let _ = InitCommonControlsEx(&icc);
    // Both classes get the arrow cursor: without a class cursor the pointer
    // keeps whatever shape it last had (an edit's I-beam, say) over the cards
    // and the preview. The caption shows icon resource 1, the embedded
    // icon.ico.
    let arrow = LoadCursorW(None, IDC_ARROW).unwrap_or_default();
    let wc = WNDCLASSW {
        lpfnWndProc: Some(settings_proc),
        hInstance: hinstance,
        hIcon: LoadIconW(Some(hinstance), PCWSTR(1 as _)).unwrap_or_default(),
        hCursor: arrow,
        lpszClassName: CLASS,
        hbrBackground: dark::bg_brush(),
        ..Default::default()
    };
    RegisterClassW(&wc);
    // The live-preview child class.
    let pwc = WNDCLASSW {
        lpfnWndProc: Some(preview_proc),
        hInstance: hinstance,
        hCursor: arrow,
        lpszClassName: PREVIEW_CLASS,
        ..Default::default()
    };
    RegisterClassW(&pwc);
}

/// Open the settings window; returns its handle (HWND(null) on failure).
pub unsafe fn open(main: HWND) -> HWND {
    let hinstance = HINSTANCE(GetModuleHandleW(None).map(|h| h.0).unwrap_or(core::ptr::null_mut()));
    let lay = layout(MetricKey::ALL.len() as i32);
    // Created hidden, then sized and given its dark frame, and only then
    // shown: Windows 10 does not redraw a shown caption when the dark
    // attribute arrives, so a window shown first would keep a light caption.
    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        CLASS,
        w!("TinyMon Settings"),
        STYLE,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        CLIENT_W_TOTAL,
        lay.client_h,
        None,
        None,
        Some(hinstance),
        Some(main.0 as *const c_void),
    )
    .unwrap_or_default();

    if !hwnd.0.is_null() {
        // The client area at the window's DPI (build_controls set it during
        // WM_CREATE) plus the frame, moved from the default spot only as far
        // as it takes to fit the work area of the nearest monitor (a window
        // taller than the work area keeps its top edge inside it).
        let (w, h) = window_size(dscale());
        let mut r = RECT::default();
        let _ = GetWindowRect(hwnd, &mut r);
        r.right = r.left + w;
        r.bottom = r.top + h;
        let (x, y) = fit_into(r.left, r.top, w, h, work_area_near(&r));
        let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
        // Dark title bar + dark mode for the frame (controls are themed in
        // `control`), then the first show.
        dark::apply_window(hwnd);
        let _ = ShowWindow(hwnd, SW_SHOW);
    }
    hwnd
}

/// True for the push buttons: OK, Cancel, Apply, Reset, both Clear buttons and
/// each metric row's up and down arrows.
fn is_push_button(id: i32) -> bool {
    let n = MetricKey::ALL.len() as i32;
    matches!(id, IDOK | IDCANCEL | IDC_APPLY | IDC_RESET | IDC_HK_TOGGLE_CLR | IDC_HK_SETTINGS_CLR)
        || (IDC_MOVEUP_BASE..IDC_MOVEUP_BASE + n).contains(&id)
        || (IDC_MOVEDN_BASE..IDC_MOVEDN_BASE + n).contains(&id)
}

/// The id Enter presses (the DM_GETDEFID answer) while control `focused` has
/// the focus: a push button presses itself; anything else (a checkbox, an edit,
/// a hotkey field, the trackbar, or no control at all) presses OK.
fn enter_target(focused: i32) -> i32 {
    if is_push_button(focused) {
        focused
    } else {
        IDOK
    }
}

/// The control WM_SETFOCUS passes the focus on to: `saved`, the one that had
/// it at the last deactivation, or the first metric checkbox while none was
/// saved (0).
fn focus_target(saved: i32) -> i32 {
    if saved == 0 {
        IDC_METRIC_BASE
    } else {
        saved
    }
}

/// The move an up (`up`) or down arrow in metric row `row` of `len` makes:
/// Some((from, to)), the rows the item leaves and lands in, or None at either
/// end. The focus then goes to the same arrow in row `to`, so the next press
/// moves the same item on. `double` is a double-click's BN_DOUBLECLICKED,
/// which the clicked arrow sends after its first click has already moved the
/// item one row: the item moves on from there, two rows in all.
fn reorder_step(len: usize, row: usize, up: bool, double: bool) -> Option<(usize, usize)> {
    let from = match (double, up) {
        (false, _) => row,
        (true, true) => row.checked_sub(1)?,
        (true, false) => row + 1,
    };
    let to = if up { from.checked_sub(1)? } else { from + 1 };
    (to < len).then_some((from, to))
}

unsafe extern "system" fn settings_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_CREATE => {
            let cs = lparam.0 as *const CREATESTRUCTW;
            let main =
                if cs.is_null() { HWND(core::ptr::null_mut()) } else { HWND((*cs).lpCreateParams) };
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, main.0 as isize);
            build_controls(hwnd, main);
            LRESULT(0)
        }

        // The window moved to a monitor with another scale, or its monitor's
        // scale changed. The new fonts come first; place_controls moves every
        // control to its Place at the new scale and hands it the new font, and
        // only then are the old fonts freed, which the controls used until
        // then. The window then takes its size at the new DPI, at the top-left
        // of the rect Windows suggests (lParam), or where it is without one.
        WM_DPICHANGED => {
            SCALE.with(|c| c.set(dpi_from_wparam(wparam)));
            let old = make_fonts();
            place_controls(hwnd);
            delete_fonts(old);
            let (w, h) = window_size(dscale());
            // SAFETY: a non-null lParam of WM_DPICHANGED points to the suggested
            // window RECT, valid while the message is handled; as_ref reads
            // nothing from a null one.
            let (x, y, flags) = match (lparam.0 as *const RECT).as_ref() {
                Some(r) => (r.left, r.top, SWP_NOZORDER | SWP_NOACTIVATE),
                None => (0, 0, SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOMOVE),
            };
            let _ = SetWindowPos(hwnd, None, x, y, w, h, flags);
            let _ = InvalidateRect(Some(hwnd), None, true);
            LRESULT(0)
        }

        // Deactivation: remember the control that has the focus, for
        // WM_SETFOCUS to give it back; then the default handling.
        WM_ACTIVATE => {
            if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE {
                let focus = GetFocus();
                if IsChild(hwnd, focus).as_bool() {
                    LAST_FOCUS.with(|c| c.set(GetDlgCtrlID(focus)));
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }

        // The frame itself took the focus, as DefWindowProcW makes it do on
        // every activation: pass it on to the control that had it at the last
        // deactivation, else to the first metric checkbox (focus_target), so
        // typing goes on where it was and Tab navigation has a starting point.
        WM_SETFOCUS => {
            let id = focus_target(LAST_FOCUS.with(|c| c.get()));
            if let Ok(c) = GetDlgItem(Some(hwnd), id) {
                let _ = SetFocus(Some(c));
            }
            LRESULT(0)
        }

        // Opacity trackbar moved: reflect the live value + update the preview.
        // The trackbar itself redraws only the thumb's old and new spots, so
        // after a jump (a key, a click on the channel) the accent fill between
        // them would be stale: repaint all of it.
        WM_HSCROLL => {
            if let Ok(tb) = GetDlgItem(Some(hwnd), IDC_OPACITY) {
                let pos = SendMessageW(tb, TBM_GETPOS, None, None).0;
                if let Ok(lbl) = GetDlgItem(Some(hwnd), IDC_OPACITY_VAL) {
                    set_text(lbl, &format!("Opacity   {}", pos));
                }
                let _ = InvalidateRect(Some(tb), None, false);
            }
            refresh_preview(hwnd);
            LRESULT(0)
        }

        // Enter: IsDialogMessageW asks which button is the default whenever the
        // focused control is not a default push button (an owner-drawn button
        // never is), and sends IDOK when the window does not answer. The answer
        // is the focused push button, else OK (enter_target).
        DM_GETDEFID => {
            let focus = GetFocus();
            let id = if IsChild(hwnd, focus).as_bool() { GetDlgCtrlID(focus) } else { 0 };
            LRESULT(((DC_HASDEFID as isize) << 16) | enter_target(id) as u16 as isize)
        }

        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as i32;
            let notif = ((wparam.0 >> 16) & 0xFFFF) as u32;
            let n = MetricKey::ALL.len() as i32;
            match id {
                // OK closes the window once the settings are handed over:
                // apply refuses one combo for both hotkeys. Apply leaves the
                // window open either way.
                IDOK => {
                    if apply(hwnd) {
                        let _ = DestroyWindow(hwnd);
                    }
                }
                IDC_APPLY => {
                    apply(hwnd);
                }
                IDCANCEL => {
                    let _ = DestroyWindow(hwnd);
                }
                IDC_RESET => {
                    reset_to_defaults(hwnd);
                    refresh_preview(hwnd);
                }
                IDC_HK_TOGGLE_CLR => {
                    clear_hotkey(hwnd, IDC_HK_TOGGLE);
                    refresh_hotkey_notes(hwnd);
                }
                IDC_HK_SETTINGS_CLR => {
                    clear_hotkey(hwnd, IDC_HK_SETTINGS);
                    refresh_hotkey_notes(hwnd);
                }
                // Live-preview triggers: the font edit changed, or a metric toggled.
                IDC_FONT if notif == EN_CHANGE => refresh_preview(hwnd),
                // A hotkey field changed, typed or filled by
                // capture_fired_hotkey: the notes follow it.
                IDC_HK_TOGGLE | IDC_HK_SETTINGS if notif == EN_CHANGE => refresh_hotkey_notes(hwnd),
                _ if (IDC_METRIC_BASE..IDC_METRIC_BASE + n).contains(&id) => {
                    // Toggle this row's metric on/off in the working list.
                    let p = (id - IDC_METRIC_BASE) as usize;
                    METRIC_STATE.with(|st| {
                        let mut s = st.borrow_mut();
                        s[p].1 = !s[p].1;
                    });
                    render_metric_rows(hwnd);
                    refresh_preview(hwnd);
                }
                // An up or down arrow moves its row's item, and the focus goes
                // with the item, to the same arrow in its new row (reorder_step).
                _ if (IDC_MOVEUP_BASE..IDC_MOVEUP_BASE + n).contains(&id)
                    || (IDC_MOVEDN_BASE..IDC_MOVEDN_BASE + n).contains(&id) =>
                {
                    let up = id < IDC_MOVEDN_BASE;
                    let base = if up { IDC_MOVEUP_BASE } else { IDC_MOVEDN_BASE };
                    let double = notif == BN_DOUBLECLICKED;
                    let step = reorder_step(n as usize, (id - base) as usize, up, double);
                    if let Some((from, to)) = step {
                        METRIC_STATE.with(|st| st.borrow_mut().swap(from, to));
                        render_metric_rows(hwnd);
                        refresh_preview(hwnd);
                        if let Ok(b) = GetDlgItem(Some(hwnd), base + to as i32) {
                            let _ = SetFocus(Some(b));
                        }
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }

        // The widget applied settings and may have registered the hotkeys
        // again: the notes show which of them failed (hotkey_status).
        WM_APP_HOTKEY_STATUS => {
            refresh_hotkey_notes(hwnd);
            LRESULT(0)
        }

        WM_DESTROY => {
            free_fonts();
            METRIC_STATE.with(|s| s.borrow_mut().clear());
            LAST_FOCUS.with(|c| c.set(0));
            let main = HWND(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut c_void);
            if !main.0.is_null() {
                let _ = PostMessageW(Some(main), WM_APP_SETTINGS_CLOSED, WPARAM(0), LPARAM(0));
            }
            LRESULT(0)
        }

        // Paint the window background + the elevated section "cards" behind the
        // child controls (replacing the native dark group boxes, which Win10
        // renders as a clashing light etched frame).
        WM_ERASEBKGND => {
            paint_chrome(hwnd, HDC(wparam.0 as *mut c_void));
            LRESULT(1)
        }

        // Dark theming for control text/background. Static labels (and the
        // trackbar's backfill) sit ON a card, so they take the card surface;
        // edit + hotkey fields get the recessed input fill. A hotkey note's
        // text takes its kind's colour (static_text_color).
        WM_CTLCOLORSTATIC => {
            let hdc = HDC(wparam.0 as *mut c_void);
            SetTextColor(hdc, COLORREF(static_text_color(HWND(lparam.0 as *mut c_void))));
            SetBkColor(hdc, COLORREF(dark::PANEL_BG));
            LRESULT(dark::panel_brush().0 as isize)
        }
        WM_CTLCOLORBTN => {
            let hdc = HDC(wparam.0 as *mut c_void);
            SetTextColor(hdc, COLORREF(dark::TEXT));
            SetBkColor(hdc, COLORREF(dark::WINDOW_BG));
            LRESULT(dark::bg_brush().0 as isize)
        }
        WM_CTLCOLOREDIT => {
            let hdc = HDC(wparam.0 as *mut c_void);
            SetTextColor(hdc, COLORREF(dark::TEXT));
            SetBkColor(hdc, COLORREF(dark::EDIT_BG));
            LRESULT(dark::edit_brush().0 as isize)
        }

        // Owner-draw the dark push buttons + metric checkboxes.
        WM_DRAWITEM => {
            let dis = &*(lparam.0 as *const DRAWITEMSTRUCT);
            let id = dis.CtlID as i32;
            if (IDC_METRIC_BASE..IDC_METRIC_BASE + MetricKey::ALL.len() as i32).contains(&id) {
                draw_checkbox(dis);
            } else {
                draw_button(dis);
            }
            LRESULT(1)
        }
        // Custom-draw the opacity trackbar dark.
        WM_NOTIFY => {
            let hdr = &*(lparam.0 as *const NMHDR);
            // NM_CUSTOMDRAW == NM_FIRST (0) - 12.
            if hdr.code == (-12i32) as u32 {
                trackbar_customdraw(&*(lparam.0 as *const NMCUSTOMDRAW))
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn build_controls(hwnd: HWND, main: HWND) {
    let hinstance = HINSTANCE(GetModuleHandleW(None).map(|h| h.0).unwrap_or(core::ptr::null_mut()));
    SCALE.with(|c| c.set(dpi_for_window(hwnd)));
    delete_fonts(make_fonts());
    let font = HGDIOBJ(ui_font().0);
    let cur = current_settings(main);
    let n = MetricKey::ALL.len() as i32;
    let places = control_places(n);

    // Each control goes to its Place (control_places), authored in logical
    // (96-dpi) units and scaled to the window's DPI.
    let s = dscale();
    let child = |class, text, style: u32, id| unsafe {
        let p = places.iter().find(|p| p.id == id).copied().unwrap_or_default();
        let (x, y, w, h) = p.scaled(s);
        control(hwnd, class, text, WINDOW_STYLE(style), x, y, w, h, id, hinstance, font)
    };
    let base = WS_CHILD.0 | WS_VISIBLE.0;
    let tab = base | WS_TABSTOP.0;
    // Push buttons + metric checkboxes are owner-drawn (Win10 won't theme them
    // dark); see WM_DRAWITEM. The metric checkboxes track their own checked
    // state in GWLP_USERDATA (BS_OWNERDRAW has no built-in check state).
    let btn = tab | BS_OWNERDRAW as u32;
    let field = tab | WS_BORDER.0;
    let number = field | ES_NUMBER as u32;

    init_metric_state(&cur);
    let opacity_text = format!("Opacity   {}", cur.opacity);
    let interval_text = cur.interval_ms.to_string();
    let font_text = cur.font_px.to_string();
    // In creation order, which is the Tab order (created_ids). The cards
    // behind the controls are painted in `paint_chrome`.
    for id in created_ids(n) {
        let (class, text, style) = match id {
            IDC_OPACITY_VAL => (w!("STATIC"), opacity_text.as_str(), base),
            IDC_OPACITY => (w!("msctls_trackbar32"), "", tab),
            IDC_LBL_INTERVAL => (w!("STATIC"), INTERVAL_LABEL, base | SS_ENDELLIPSIS),
            IDC_INTERVAL => (w!("EDIT"), interval_text.as_str(), number),
            IDC_LBL_FONT => (w!("STATIC"), FONT_LABEL, base | SS_ENDELLIPSIS),
            IDC_FONT => (w!("EDIT"), font_text.as_str(), number),
            IDC_LBL_HK_TOGGLE => (w!("STATIC"), "Show / Hide", base),
            IDC_LBL_HK_SETTINGS => (w!("STATIC"), "Open settings", base),
            // The notes beside them, right-aligned (refresh_hotkey_notes).
            IDC_HK_TOGGLE_NOTE | IDC_HK_SETTINGS_NOTE => {
                (w!("STATIC"), "", base | SS_RIGHT | SS_ENDELLIPSIS)
            }
            IDC_HK_TOGGLE | IDC_HK_SETTINGS => (w!("msctls_hotkey32"), "", field),
            IDC_HK_TOGGLE_CLR | IDC_HK_SETTINGS_CLR => (w!("BUTTON"), "Clear", btn),
            // OK is the primary. NB: BS_DEFPUSHBUTTON can't coexist with
            // BS_OWNERDRAW (they share BS_TYPEMASK), so OK is the default only
            // by its id: draw_button fills it cyan, and the DM_GETDEFID answer
            // (enter_target) makes Enter press OK unless another push button
            // has the focus, which Enter then presses instead. Esc => Cancel.
            IDC_RESET => (w!("BUTTON"), "Reset", btn),
            IDOK => (w!("BUTTON"), "OK", btn),
            IDC_APPLY => (w!("BUTTON"), "Apply", btn),
            IDCANCEL => (w!("BUTTON"), "Cancel", btn),
            // The live preview: a real mini-widget rendered with the SAME
            // render_rows as the widget, updated as the controls change.
            IDC_PREVIEW => (PREVIEW_CLASS, "", base | WS_BORDER.0),
            // A metric row: its checkbox (labelled by render_metric_rows), then
            // its up and down arrows, which reorder the list.
            _ if id < IDC_MOVEUP_BASE => (w!("BUTTON"), "", btn),
            _ if id < IDC_MOVEDN_BASE => (w!("BUTTON"), "\u{25B2}", btn),
            _ => (w!("BUTTON"), "\u{25BC}", btn),
        };
        let c = child(class, text, style, id);
        match id {
            IDC_OPACITY => {
                SendMessageW(c, TBM_SETRANGE, Some(WPARAM(1)), Some(range(40, 255)));
                SendMessageW(c, TBM_SETPOS, Some(WPARAM(1)), Some(LPARAM(cur.opacity as isize)));
            }
            IDC_INTERVAL => {
                SendMessageW(c, EM_LIMITTEXT, Some(WPARAM(INTERVAL_CHARS)), None);
            }
            IDC_FONT => {
                SendMessageW(c, EM_LIMITTEXT, Some(WPARAM(FONT_CHARS)), None);
            }
            IDC_HK_TOGGLE => set_hotkey_control(c, cur.hotkey_toggle),
            IDC_HK_SETTINGS => set_hotkey_control(c, cur.hotkey_settings),
            _ => {}
        }
    }
    render_metric_rows(hwnd);
    refresh_hotkey_notes(hwnd);

    // Start focus on the first checkbox so Tab has somewhere to go; seed preview.
    if let Ok(c) = GetDlgItem(Some(hwnd), IDC_METRIC_BASE) {
        let _ = SetFocus(Some(c));
    }
    refresh_preview(hwnd);
}

/// After a DPI change: move every control to its Place at the current scale
/// and hand it the current UI font (build_controls places each control as it
/// creates it).
unsafe fn place_controls(hwnd: HWND) {
    let s = dscale();
    let font = WPARAM(ui_font().0 as usize);
    for p in control_places(MetricKey::ALL.len() as i32) {
        if let Ok(c) = GetDlgItem(Some(hwnd), p.id) {
            let (x, y, w, h) = p.scaled(s);
            let _ = SetWindowPos(c, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
            SendMessageW(c, WM_SETFONT, Some(font), Some(LPARAM(1)));
        }
    }
}

/// Invalidate the live-preview child so it repaints from the current controls.
unsafe fn refresh_preview(settings: HWND) {
    if let Ok(preview) = GetDlgItem(Some(settings), IDC_PREVIEW) {
        let _ = InvalidateRect(Some(preview), None, true);
    }
}

/// Repopulate the controls from the factory defaults. Does NOT apply — the user
/// still clicks Apply / OK.
unsafe fn reset_to_defaults(hwnd: HWND) {
    let d = Settings::defaults();
    init_metric_state(&d);
    render_metric_rows(hwnd);
    if let Ok(tb) = GetDlgItem(Some(hwnd), IDC_OPACITY) {
        SendMessageW(tb, TBM_SETPOS, Some(WPARAM(1)), Some(LPARAM(d.opacity as isize)));
        // TBM_SETPOS redraws only the thumb's old and new spots: repaint the
        // whole trackbar, or the accent fill between them stays stale.
        let _ = InvalidateRect(Some(tb), None, false);
    }
    if let Ok(lbl) = GetDlgItem(Some(hwnd), IDC_OPACITY_VAL) {
        set_text(lbl, &format!("Opacity   {}", d.opacity));
    }
    if let Ok(e) = GetDlgItem(Some(hwnd), IDC_INTERVAL) {
        set_text(e, &d.interval_ms.to_string());
    }
    if let Ok(e) = GetDlgItem(Some(hwnd), IDC_FONT) {
        set_text(e, &d.font_px.to_string());
    }
    clear_hotkey(hwnd, IDC_HK_TOGGLE);
    clear_hotkey(hwnd, IDC_HK_SETTINGS);
    refresh_hotkey_notes(hwnd);
}

/// Window procedure for the live-preview child window.
unsafe extern "system" fn preview_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint_preview(hwnd);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Where the preview puts a widget of `nat_w` x `nat_h` device pixels in a
/// pane of `pane_w` x `pane_h`: (x, y, w, h). 1:1 in the middle when it fits;
/// else scaled down uniformly to the pane's width or height, whichever limits
/// it, and centred the other way. All zero for an empty widget.
fn fit_preview(nat_w: i32, nat_h: i32, pane_w: i32, pane_h: i32) -> (i32, i32, i32, i32) {
    if nat_w <= 0 || nat_h <= 0 {
        return (0, 0, 0, 0);
    }
    // The aspect ratios compare by cross-multiplying, in i64 so no product
    // can overflow.
    let (nw, nh, pw, ph) = (nat_w as i64, nat_h as i64, pane_w as i64, pane_h as i64);
    let (w, h) = if nat_w <= pane_w && nat_h <= pane_h {
        (nat_w, nat_h)
    } else if nw * ph >= nh * pw {
        (pane_w, (nh * pw / nw) as i32)
    } else {
        ((nw * ph / nh) as i32, pane_h)
    };
    ((pane_w - w) / 2, (pane_h - h) / 2, w, h)
}

/// Render a faithful mini-widget from the parent settings' live control values,
/// using the SAME `render_rows` as the real widget, dimmed by the opacity slider.
/// It is the widget at its true size, with every row (so never a "+N"): 1:1
/// in the middle of the pane, or scaled down to fit a pane it outgrows
/// (fit_preview).
unsafe fn paint_preview(hwnd: HWND) {
    let parent = GetParent(hwnd).unwrap_or_default();
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let (pane_w, pane_h) = (rc.right - rc.left, rc.bottom - rc.top);
    if pane_w > 0 && pane_h > 0 && !parent.0.is_null() {
        // Enabled order from the parent's checkboxes.
        let order = enabled_order();
        let rows = sample_rows(&order);
        let opacity = read_trackbar(parent, IDC_OPACITY).clamp(40, 255) as u8;
        // The font field reads as apply() reads it: text that is no number
        // stands for the widget's current font.
        let main = HWND(GetWindowLongPtrW(parent, GWLP_USERDATA) as *mut c_void);
        let cur = current_settings(main);
        let font_px = read_font(parent, cur.font_px);
        let scale = Scale(dpi_for_window(hwnd));
        let font = create_mono_font(scale, font_px);

        // The widget's true size, as tall as its rows need (no cap), and the
        // spot and size it gets in the pane.
        let (w, h) = (win_w(scale, font_px), window_height(scale, rows.len() as i32, font_px));
        let (x, y, fit_w, fit_h) = fit_preview(w, h, pane_w, pane_h);

        // Render the widget into an off-screen buffer, fill the pane with the
        // widget bg, then blend the content at the chosen opacity (simulating
        // the layered widget's translucency over its own bg).
        let mem = CreateCompatibleDC(Some(hdc));
        let bmp = CreateCompatibleBitmap(hdc, w, h);
        let prev = SelectObject(mem, HGDIOBJ(bmp.0));
        // Representative history so the preview shows sparklines like the widget
        // (the live widget accumulates real history per tick).
        let preview_hist: Vec<Vec<f32>> = rows
            .iter()
            .map(|r| match r.bar {
                Some(f) => (0..48)
                    .map(|i| (f + 0.18 * (i as f32 * 0.45).sin()).clamp(0.05, 0.97))
                    .collect(),
                None => Vec::new(),
            })
            .collect();
        // Reflect the widget's light/dark choice in the preview.
        let theme = if cur.light { &LIGHT } else { &THEME };
        let hist: Vec<&[f32]> = preview_hist.iter().map(Vec::as_slice).collect();
        render_rows(mem, w, h, &rows, font, scale, font_px, theme, &hist);
        SetDCBrushColor(hdc, COLORREF(theme.bg));
        FillRect(hdc, &rc, HBRUSH(GetStockObject(DC_BRUSH).0));
        let bf = BLENDFUNCTION {
            BlendOp: 0,
            BlendFlags: 0,
            SourceConstantAlpha: opacity,
            AlphaFormat: 0,
        };
        if (fit_w, fit_h) == (w, h) {
            let _ = AlphaBlend(hdc, x, y, w, h, mem, 0, 0, w, h, bf);
        } else {
            // Scaled down: HALFTONE into a second buffer, which AlphaBlend then
            // blends 1:1 (stretching in AlphaBlend itself would use
            // COLORONCOLOR, which leaves the text jagged).
            let small = CreateCompatibleDC(Some(hdc));
            let sbmp = CreateCompatibleBitmap(hdc, fit_w, fit_h);
            let sprev = SelectObject(small, HGDIOBJ(sbmp.0));
            SetStretchBltMode(small, HALFTONE);
            let _ = SetBrushOrgEx(small, 0, 0, None);
            let _ = StretchBlt(small, 0, 0, fit_w, fit_h, Some(mem), 0, 0, w, h, SRCCOPY);
            let _ = AlphaBlend(hdc, x, y, fit_w, fit_h, small, 0, 0, fit_w, fit_h, bf);
            SelectObject(small, sprev);
            let _ = DeleteObject(HGDIOBJ(sbmp.0));
            let _ = DeleteDC(small);
        }

        SelectObject(mem, prev);
        let _ = DeleteObject(HGDIOBJ(bmp.0));
        let _ = DeleteDC(mem);
        let _ = DeleteObject(HGDIOBJ(font.0));
    }
    let _ = EndPaint(hwnd, &ps);
}

/// Create one child control and give it the standard GUI font.
#[allow(clippy::too_many_arguments)]
unsafe fn control(
    parent: HWND,
    class: PCWSTR,
    text: &str,
    style: WINDOW_STYLE,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    id: i32,
    hinstance: HINSTANCE,
    font: HGDIOBJ,
) -> HWND {
    let mut wtext: Vec<u16> = text.encode_utf16().collect();
    wtext.push(0);
    let ctrl = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        class,
        PCWSTR(wtext.as_ptr()),
        style,
        x,
        y,
        w,
        h,
        Some(parent),
        Some(HMENU(id as usize as *mut c_void)),
        Some(hinstance),
        None,
    )
    .unwrap_or_default();
    SendMessageW(ctrl, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
    dark::apply_control(ctrl);
    ctrl
}

/// Whether an owner-drawn metric checkbox is checked. `BS_OWNERDRAW` keeps no
/// check state of its own, so we store it in the control's `GWLP_USERDATA`.
unsafe fn cb_checked(cb: HWND) -> bool {
    GetWindowLongPtrW(cb, GWLP_USERDATA) != 0
}
unsafe fn set_cb_checked(cb: HWND, on: bool) {
    SetWindowLongPtrW(cb, GWLP_USERDATA, on as isize);
}

/// Fill a rectangle with a solid color.
unsafe fn fill(hdc: HDC, rc: &RECT, color: u32) {
    let b = CreateSolidBrush(COLORREF(color));
    FillRect(hdc, rc, b);
    let _ = DeleteObject(HGDIOBJ(b.0));
}
/// Fill + 1px-outline a rounded rectangle (corner radius `r`). Uses the DC brush
/// for the fill and a temporary pen for the border; pass `border == fill` for a
/// seamless (borderless) fill.
unsafe fn round_panel(hdc: HDC, rc: &RECT, r: i32, fill_col: u32, border_col: u32) {
    let pen = CreatePen(PS_SOLID, 1, COLORREF(border_col));
    let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
    SetDCBrushColor(hdc, COLORREF(fill_col));
    let old_brush = SelectObject(hdc, GetStockObject(DC_BRUSH));
    let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, r, r);
    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);
    let _ = DeleteObject(HGDIOBJ(pen.0));
}
/// Stroke a 1px rounded outline (hollow — no fill). Used for focus rings.
unsafe fn round_outline(hdc: HDC, rc: &RECT, r: i32, color: u32) {
    let pen = CreatePen(PS_SOLID, 1, COLORREF(color));
    let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
    let old_brush = SelectObject(hdc, GetStockObject(NULL_BRUSH));
    let _ = RoundRect(hdc, rc.left, rc.top, rc.right, rc.bottom, r, r);
    SelectObject(hdc, old_brush);
    SelectObject(hdc, old_pen);
    let _ = DeleteObject(HGDIOBJ(pen.0));
}
/// Draw a check mark inside `box_rc`.
unsafe fn draw_check(hdc: HDC, box_rc: &RECT, color: u32) {
    let pen = CreatePen(PS_SOLID, dscale().px(2).max(1), COLORREF(color));
    let old = SelectObject(hdc, HGDIOBJ(pen.0));
    let w = box_rc.right - box_rc.left;
    let pts = [
        POINT { x: box_rc.left + w * 22 / 100, y: box_rc.top + w * 52 / 100 },
        POINT { x: box_rc.left + w * 42 / 100, y: box_rc.top + w * 72 / 100 },
        POINT { x: box_rc.left + w * 78 / 100, y: box_rc.top + w * 28 / 100 },
    ];
    let _ = Polyline(hdc, &pts);
    SelectObject(hdc, old);
    let _ = DeleteObject(HGDIOBJ(pen.0));
}
/// Draw a control's caption in its own font, in `color`, on the dark surface.
unsafe fn draw_label(hdc: HDC, rc: &RECT, ctrl: HWND, fmt: DRAW_TEXT_FORMAT, color: u32) {
    let f = SendMessageW(ctrl, WM_GETFONT, None, None).0;
    if f != 0 {
        SelectObject(hdc, HGDIOBJ(f as *mut c_void));
    }
    let mut buf = [0u16; 48];
    let n = GetWindowTextW(ctrl, &mut buf);
    let mut r = *rc;
    SetTextColor(hdc, COLORREF(color));
    SetBkMode(hdc, TRANSPARENT);
    DrawTextW(hdc, &mut buf[..n.max(0) as usize], &mut r, fmt);
}

/// UTF-16 encode a `&str` for the wide text APIs (no NUL; `DrawTextW` takes a slice).
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Paint the window background + the elevated section "cards" behind the child
/// controls. Each card is a rounded, bordered `PANEL_BG` panel one step lighter
/// than the window, with a semibold title in its top-left.
unsafe fn paint_chrome(hwnd: HWND, hdc: HDC) {
    let mut client = RECT::default();
    let _ = GetClientRect(hwnd, &mut client);
    fill(hdc, &client, dark::WINDOW_BG);

    let lay = layout(MetricKey::ALL.len() as i32);
    let s = dscale();
    // Card rects, authored in logical units then scaled to the window DPI.
    let card = |x0: i32, y0: i32, x1: i32, y1: i32| RECT {
        left: s.px(x0),
        top: s.px(y0),
        right: s.px(x1),
        bottom: s.px(y1),
    };
    let cards = [
        (card(10, lay.metrics_top, 270, lay.metrics_top + lay.metrics_h), "Metrics"),
        (card(10, lay.app_top, 270, lay.app_top + lay.app_h), "Appearance"),
        (card(10, lay.hk_top, 270, lay.hk_top + lay.hk_h), "Hotkeys"),
        (card(CLIENT_W + GUTTER, 8, CLIENT_W_TOTAL - 2, lay.client_h - 8), "Live preview"),
    ];
    for (r, title) in cards {
        round_panel(hdc, &r, s.px(8), dark::PANEL_BG, dark::BORDER);
        let f = hdr_font();
        if !f.0.is_null() {
            SelectObject(hdc, HGDIOBJ(f.0));
        }
        SetTextColor(hdc, COLORREF(dark::TEXT_MUTED));
        SetBkMode(hdc, TRANSPARENT);
        // Keep the title band in the top ~16 logical px so it clears the first
        // control in each card (rows start at +18..+22) and the preview child (+26).
        let mut tr = RECT {
            left: r.left + s.px(12),
            top: r.top + s.px(2),
            right: r.right - s.px(8),
            bottom: r.top + s.px(16),
        };
        DrawTextW(hdc, &mut wide(title), &mut tr, DT_LEFT | DT_SINGLELINE | DT_VCENTER);
    }
}

/// Owner-draw a push button. The default (OK) button is the cyan-filled
/// "primary"; the rest are bordered secondary buttons. Rounded, with an accent
/// focus ring.
unsafe fn draw_button(dis: &DRAWITEMSTRUCT) {
    let rc = dis.rcItem;
    let s = dscale();
    let id = dis.CtlID as i32;
    let pressed = dis.itemState.0 & ODS_SELECTED.0 != 0;
    let focused = dis.itemState.0 & ODS_FOCUS.0 != 0;
    let primary = id == IDOK || dis.itemState.0 & ODS_DEFAULT.0 != 0;
    // The Clear buttons sit on a card; the rest sit on the window background — fill
    // the corners with the right backdrop so the rounded edges don't show black.
    let on_card = id == IDC_HK_TOGGLE_CLR
        || id == IDC_HK_SETTINGS_CLR
        || (IDC_MOVEUP_BASE..IDC_MOVEUP_BASE + MetricKey::ALL.len() as i32).contains(&id)
        || (IDC_MOVEDN_BASE..IDC_MOVEDN_BASE + MetricKey::ALL.len() as i32).contains(&id);
    fill(dis.hDC, &rc, if on_card { dark::PANEL_BG } else { dark::WINDOW_BG });

    let (face, text) = if primary {
        (if pressed { dark::ACCENT_DOWN } else { dark::ACCENT }, dark::ON_ACCENT)
    } else if pressed {
        (dark::BTN_FACE_DOWN, dark::TEXT)
    } else {
        (dark::BTN_FACE, dark::TEXT)
    };
    let border = if primary { face } else { dark::BORDER };
    round_panel(dis.hDC, &rc, s.px(6), face, border);
    if focused {
        let i = s.px(2);
        let r =
            RECT { left: rc.left + i, top: rc.top + i, right: rc.right - i, bottom: rc.bottom - i };
        round_outline(dis.hDC, &r, s.px(5), if primary { dark::ON_ACCENT } else { dark::ACCENT });
    }
    draw_label(dis.hDC, &rc, dis.hwndItem, DT_CENTER | DT_VCENTER | DT_SINGLELINE, text);
}

/// Owner-draw a metric checkbox on the Metrics card: a recessed box that fills
/// with the cyan accent (dark tick) when checked, accent outline when focused.
unsafe fn draw_checkbox(dis: &DRAWITEMSTRUCT) {
    let rc = dis.rcItem;
    let s = dscale();
    fill(dis.hDC, &rc, dark::PANEL_BG);
    let sz = s.px(15);
    let top = rc.top + (rc.bottom - rc.top - sz) / 2;
    let box_rc = RECT { left: rc.left, top, right: rc.left + sz, bottom: top + sz };
    let checked = cb_checked(dis.hwndItem);
    let focused = dis.itemState.0 & ODS_FOCUS.0 != 0;
    if checked {
        round_panel(dis.hDC, &box_rc, s.px(4), dark::ACCENT, dark::ACCENT);
        draw_check(dis.hDC, &box_rc, dark::ON_ACCENT);
    } else {
        let edge = if focused { dark::ACCENT } else { dark::BORDER };
        round_panel(dis.hDC, &box_rc, s.px(4), dark::EDIT_BG, edge);
    }
    if focused && checked {
        let o = s.px(2);
        let ring = RECT {
            left: box_rc.left - o,
            top: box_rc.top - o,
            right: box_rc.right + o,
            bottom: box_rc.bottom + o,
        };
        round_outline(dis.hDC, &ring, s.px(5), dark::ACCENT);
    }
    let label = RECT { left: rc.left + sz + s.px(7), ..rc };
    draw_label(dis.hDC, &label, dis.hwndItem, DT_LEFT | DT_VCENTER | DT_SINGLELINE, dark::TEXT);
}

/// Recolor the opacity trackbar dark — Win10 ignores dark-mode theming for it,
/// so we custom-draw a dark channel + accent thumb via `NM_CUSTOMDRAW`.
unsafe fn trackbar_customdraw(cd: &NMCUSTOMDRAW) -> LRESULT {
    const CDRF_DODEFAULT: isize = 0x0;
    const CDRF_SKIPDEFAULT: isize = 0x4;
    const CDRF_NOTIFYITEMDRAW: isize = 0x20;
    const TBCD_TICS: usize = 1;
    const TBCD_THUMB: usize = 2;
    const TBCD_CHANNEL: usize = 3;
    if cd.dwDrawStage == CDDS_PREPAINT {
        return LRESULT(CDRF_NOTIFYITEMDRAW);
    }
    if cd.dwDrawStage == CDDS_ITEMPREPAINT {
        match cd.dwItemSpec {
            TBCD_CHANNEL => {
                fill(cd.hdc, &cd.rc, dark::TRACK);
                // Accent fill from the left edge up to the current value.
                let tb = cd.hdr.hwndFrom;
                let pos = SendMessageW(tb, TBM_GETPOS, None, None).0 as i32;
                let lo = SendMessageW(tb, TBM_GETRANGEMIN, None, None).0 as i32;
                let hi = SendMessageW(tb, TBM_GETRANGEMAX, None, None).0 as i32;
                let span = (hi - lo).max(1);
                let frac = (pos - lo).clamp(0, span) as f32 / span as f32;
                let w = cd.rc.right - cd.rc.left;
                let filled = RECT { right: cd.rc.left + (w as f32 * frac) as i32, ..cd.rc };
                fill(cd.hdc, &filled, dark::ACCENT);
                return LRESULT(CDRF_SKIPDEFAULT);
            }
            TBCD_THUMB => {
                round_panel(cd.hdc, &cd.rc, dscale().px(3), dark::ACCENT, dark::ACCENT_DOWN);
                return LRESULT(CDRF_SKIPDEFAULT);
            }
            TBCD_TICS => return LRESULT(CDRF_SKIPDEFAULT),
            _ => {}
        }
    }
    LRESULT(CDRF_DODEFAULT)
}

/// Read the controls, build a clamped `Settings`, show its interval and font
/// in their edits, and hand it to the main window (`queue_apply`: nothing is
/// left behind if the post fails). One combo for both hotkeys is refused
/// (same_combo): the notes say so, the focus goes to the Open settings field,
/// and nothing is handed over. Returns whether the settings were handed over.
unsafe fn apply(hwnd: HWND) -> bool {
    let main = HWND(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut c_void);
    if main.0.is_null() {
        return false;
    }
    let cur = current_settings(main);

    // Enabled metrics in the user's arranged order (from the reorderable list).
    let metrics: Vec<String> = enabled_order().iter().map(|k| k.as_str().to_string()).collect();
    let opacity = read_trackbar(hwnd, IDC_OPACITY).clamp(0, 255) as u8;
    // The numbers in the edits, clamped to their ranges; any other text keeps
    // the current value (parse_clamped).
    let text = read_edit_text(hwnd, IDC_INTERVAL);
    let interval = parse_clamped(&text, MIN_INTERVAL_MS, MAX_INTERVAL_MS, cur.interval_ms);
    let font = read_font(hwnd, cur.font_px);

    // The fields cannot show Win: a saved Win combo survives an untouched field.
    let hk_toggle = effective_hotkey(cur.hotkey_toggle, read_hotkey(hwnd, IDC_HK_TOGGLE));
    let hk_settings = effective_hotkey(cur.hotkey_settings, read_hotkey(hwnd, IDC_HK_SETTINGS));

    // Build a Config and resolve it (this applies all the clamping/defaults).
    let cfg = Config {
        x: cur.pos.map(|p| p.0),
        y: cur.pos.map(|p| p.1),
        metrics: Some(metrics),
        opacity: Some(opacity),
        interval_ms: Some(interval),
        font_px: Some(font),
        hotkey_toggle: hk_toggle.map(format_hotkey),
        hotkey_settings: hk_settings.map(format_hotkey),
        light: Some(cur.light), // theme is toggled from the tray, not this dialog
    };
    let new = Settings::from_config(&cfg);
    // The edits show the values that apply: a number out of range reads as
    // the end of the range, any other text as the value kept.
    for (id, value) in [(IDC_INTERVAL, new.interval_ms), (IDC_FONT, new.font_px as u32)] {
        if let Ok(e) = GetDlgItem(Some(hwnd), id) {
            set_text(e, &value.to_string());
        }
    }
    // One combo cannot serve both actions: its second registration would
    // fail. Nothing is applied until one of the two fields changes.
    if same_combo(hk_toggle, hk_settings) {
        refresh_hotkey_notes(hwnd);
        if let Ok(f) = GetDlgItem(Some(hwnd), IDC_HK_SETTINGS) {
            let _ = SetFocus(Some(f));
        }
        return false;
    }
    queue_apply(main, new)
}

/// Clear a hotkey capture field (so the user can remove a binding).
unsafe fn clear_hotkey(dlg: HWND, id: i32) {
    if let Ok(hk) = GetDlgItem(Some(dlg), id) {
        SendMessageW(hk, HKM_SETHOTKEY, Some(WPARAM(0)), None);
    }
}

/// Set a control's text (used to update the live opacity readout).
unsafe fn set_text(ctrl: HWND, text: &str) {
    let mut w: Vec<u16> = text.encode_utf16().collect();
    w.push(0);
    SendMessageW(ctrl, WM_SETTEXT, None, Some(LPARAM(w.as_ptr() as isize)));
}

fn range(lo: u32, hi: u32) -> LPARAM {
    LPARAM((((lo & 0xFFFF) | ((hi & 0xFFFF) << 16)) as i32) as isize)
}

unsafe fn read_trackbar(dlg: HWND, id: i32) -> i32 {
    match GetDlgItem(Some(dlg), id) {
        Ok(tb) => SendMessageW(tb, TBM_GETPOS, None, None).0 as i32,
        Err(_) => 220,
    }
}

/// An edit control's text: its first 15 characters, more than EM_LIMITTEXT
/// lets anyone type ("" without the control).
unsafe fn read_edit_text(dlg: HWND, id: i32) -> String {
    let Ok(ctrl) = GetDlgItem(Some(dlg), id) else { return String::new() };
    let mut buf = [0u16; 16];
    let n = GetWindowTextW(ctrl, &mut buf);
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// The number an edit's `text` sets: its digits (spaces around them do not
/// count), clamped to lo..=hi. Digits beyond what a u32 holds saturate, so
/// they give `hi`; anything that is not digits, nothing included, gives
/// `fallback`, the current value.
fn parse_clamped(text: &str, lo: u32, hi: u32, fallback: u32) -> u32 {
    let t = text.trim();
    if t.is_empty() || !t.bytes().all(|b| b.is_ascii_digit()) {
        return fallback;
    }
    let n = t.bytes().fold(0u32, |n, b| n.saturating_mul(10).saturating_add(u32::from(b - b'0')));
    // max and min rather than clamp, whose check that lo <= hi could panic.
    n.max(lo).min(hi)
}

/// The font size the font edit of `dlg` sets (parse_clamped), with `current`
/// for text that is no number: what apply() saves and the preview shows.
unsafe fn read_font(dlg: HWND, current: i32) -> i32 {
    let text = read_edit_text(dlg, IDC_FONT);
    parse_clamped(&text, MIN_FONT_PX as u32, MAX_FONT_PX as u32, current as u32) as i32
}

/// Read a hotkey control -> spec (`word_to_spec`: None while it holds no key).
/// A bare key with no modifier is allowed: the user may bind a single key as a
/// global hotkey.
unsafe fn read_hotkey(dlg: HWND, id: i32) -> Option<HotkeySpec> {
    let hk = GetDlgItem(Some(dlg), id).ok()?;
    word_to_spec(SendMessageW(hk, HKM_GETHOTKEY, None, None).0 as u32)
}

/// Show `spec` in a hotkey control, flagged HOTKEYF_EXT when the key is an
/// extended one, so the control names Left "Left" and not "Num 4".
unsafe fn set_hotkey_control(hk: HWND, spec: Option<HotkeySpec>) {
    if let Some(s) = spec {
        let ext = needs_ext(s.vk, MapVirtualKeyW(s.vk, MAPVK_VK_TO_VSC_EX));
        SendMessageW(hk, HKM_SETHOTKEY, Some(WPARAM(spec_to_word(s, ext) as usize)), None);
    }
}

/// Called for the widget's WM_HOTKEY (`lparam` is its lParam): RegisterHotKey
/// takes a combo TinyMon owns before any window sees it, so a hotkey field of
/// the settings window `settings` cannot capture it by itself. If one of those
/// fields has the focus, the combo goes into that field, followed by the
/// WM_COMMAND(EN_CHANGE) a typed combo sends (HKM_SETHOTKEY sends none), and
/// the result is true: the hotkey then does nothing else. Otherwise false.
pub unsafe fn capture_fired_hotkey(settings: HWND, lparam: LPARAM) -> bool {
    let focus = GetFocus();
    if !IsChild(settings, focus).as_bool() {
        return false;
    }
    let id = GetDlgCtrlID(focus);
    if id != IDC_HK_TOGGLE && id != IDC_HK_SETTINGS {
        return false;
    }
    set_hotkey_control(focus, Some(hotkey_from_wm_hotkey(lparam)));
    let wp = WPARAM(((EN_CHANGE as usize) << 16) | id as usize);
    SendMessageW(settings, WM_COMMAND, Some(wp), Some(LPARAM(focus.0 as isize)));
    true
}

/// Show each hotkey field's note (hotkey_notes, against the saved combos and
/// which of them failed to register, hotkey_status) in the static on its
/// label's row, and keep the note's kind in that static's GWLP_USERDATA (0 for
/// none), which WM_CTLCOLORSTATIC colours it by.
unsafe fn refresh_hotkey_notes(hwnd: HWND) {
    let main = HWND(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut c_void);
    let cur = current_settings(main);
    let status = hotkey_status(main);
    let fields = [read_hotkey(hwnd, IDC_HK_TOGGLE), read_hotkey(hwnd, IDC_HK_SETTINGS)];
    let saved = [cur.hotkey_toggle, cur.hotkey_settings];
    let notes = hotkey_notes(fields, saved, [status.toggle_failed, status.settings_failed]);
    for (id, note) in [IDC_HK_TOGGLE_NOTE, IDC_HK_SETTINGS_NOTE].into_iter().zip(notes) {
        if let Ok(st) = GetDlgItem(Some(hwnd), id) {
            SetWindowLongPtrW(st, GWLP_USERDATA, note.map_or(0, |n| n as isize));
            set_text(st, note.map_or("", HotkeyNote::text));
        }
    }
}

/// The text colour of static `ctl`: a hotkey note's by the kind it keeps in
/// GWLP_USERDATA (amber for a warning, muted for the Win note); every other
/// static's, and a note's with nothing to say, dark::TEXT.
unsafe fn static_text_color(ctl: HWND) -> u32 {
    let id = GetDlgCtrlID(ctl);
    if id != IDC_HK_TOGGLE_NOTE && id != IDC_HK_SETTINGS_NOTE {
        return dark::TEXT;
    }
    match HotkeyNote::from_code(GetWindowLongPtrW(ctl, GWLP_USERDATA)) {
        Some(note) if note.is_warning() => dark::WARN,
        Some(_) => dark::TEXT_MUTED,
        None => dark::TEXT,
    }
}

/// Map RegisterHotKey MOD_* bits to the hotkey control's HOTKEYF_* bits.
fn mods_to_hotkeyf(mods: u32) -> u32 {
    let mut hf = 0;
    if mods & MOD_SHIFT.0 != 0 {
        hf |= HOTKEYF_SHIFT;
    }
    if mods & MOD_CONTROL.0 != 0 {
        hf |= HOTKEYF_CONTROL;
    }
    if mods & MOD_ALT.0 != 0 {
        hf |= HOTKEYF_ALT;
    }
    hf
}

fn hotkeyf_to_mods(hf: u32) -> u32 {
    let mut m = 0;
    if hf & HOTKEYF_SHIFT != 0 {
        m |= MOD_SHIFT.0;
    }
    if hf & HOTKEYF_CONTROL != 0 {
        m |= MOD_CONTROL.0;
    }
    if hf & HOTKEYF_ALT != 0 {
        m |= MOD_ALT.0;
    }
    m
}

/// A hotkey control's word (vk in the low byte, HOTKEYF_* in the next) as a
/// spec: None while it holds no key (empty, or only modifiers so far).
/// HOTKEYF_EXT only tells the control how to name the key, so it is dropped.
fn word_to_spec(word: u32) -> Option<HotkeySpec> {
    let vk = word & 0xFF;
    if vk == 0 {
        return None;
    }
    Some(HotkeySpec { mods: hotkeyf_to_mods((word >> 8) & 0xFF), vk })
}

/// A spec as a hotkey control's word; `ext` adds HOTKEYF_EXT, without which the
/// control names an extended key after the key that shares its scan code (Left
/// as "Num 4").
fn spec_to_word(spec: HotkeySpec, ext: bool) -> u32 {
    let hf = mods_to_hotkeyf(spec.mods) | if ext { HOTKEYF_EXT } else { 0 };
    (spec.vk & 0xFF) | ((hf & 0xFF) << 8)
}

/// Whether `vk` is an extended key, given `vsc_ex` =
/// `MapVirtualKeyW(vk, MAPVK_VK_TO_VSC_EX)`. Page Up, Page Down, End, Home,
/// the arrows, Insert, Delete and Num Lock are listed by vk, because that
/// mapping returns their numpad scan codes; any other key counts when its scan
/// code has the E0 prefix (Pause's E1 prefix does not).
fn needs_ext(vk: u32, vsc_ex: u32) -> bool {
    matches!(vk, 0x21..=0x28 | 0x2D | 0x2E | 0x90) || (vsc_ex & 0xFF00) == 0xE000
}

/// The hotkey Apply saves for one action, from the `saved` combo and what its
/// `field` reads. The control cannot show Win, so a field the user did not
/// touch reads a saved Win combo without Win; the saved combo then stays.
/// Otherwise (no Win, or a field the user changed or cleared) the field wins.
fn effective_hotkey(saved: Option<HotkeySpec>, field: Option<HotkeySpec>) -> Option<HotkeySpec> {
    if let (Some(s), Some(f)) = (saved, field) {
        let untouched = f.vk == s.vk && f.mods == (s.mods & !MOD_WIN.0);
        if s.mods & MOD_WIN.0 != 0 && untouched {
            return saved;
        }
    }
    field
}

/// Whether the two actions would get one and the same combo, which apply()
/// refuses: a combo registers for one hotkey only. Empty fields bind nothing.
fn same_combo(a: Option<HotkeySpec>, b: Option<HotkeySpec>) -> bool {
    a.is_some() && a == b
}

/// A note at the right end of a hotkey field's label row (hotkey_notes). Its
/// value is the code the note's static keeps in GWLP_USERDATA (0: no note).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum HotkeyNote {
    /// The combo is a key that types text, bare or with Shift only: once
    /// registered, it no longer types in any app (hotkeys::blocks_typing).
    BlocksTyping = 1,
    /// The combo has Win, which the field cannot show (set in the config).
    PlusWin = 2,
    /// The Open settings combo is the Show / Hide one (same_combo), which
    /// Apply and OK refuse.
    SameAsShowHide = 3,
    /// The saved combo failed to register (almost always because another app
    /// holds it), and the field still holds it.
    InUse = 4,
}

impl HotkeyNote {
    /// Every note, for from_code.
    const ALL: [HotkeyNote; 4] = [
        HotkeyNote::BlocksTyping,
        HotkeyNote::PlusWin,
        HotkeyNote::SameAsShowHide,
        HotkeyNote::InUse,
    ];

    fn text(self) -> &'static str {
        match self {
            HotkeyNote::BlocksTyping => "Blocks typing this key",
            HotkeyNote::PlusWin => "+ Win (set in config)",
            HotkeyNote::SameAsShowHide => "Same as Show / Hide",
            HotkeyNote::InUse => "In use by another app",
        }
    }

    /// A warning shows amber (dark::WARN): every note but the Win one, which
    /// shows muted.
    fn is_warning(self) -> bool {
        self != HotkeyNote::PlusWin
    }

    /// The note whose code a note's static keeps (None for 0, or any other).
    fn from_code(code: isize) -> Option<HotkeyNote> {
        HotkeyNote::ALL.into_iter().find(|n| *n as isize == code)
    }
}

/// The notes for the Show / Hide and Open settings fields, from what the
/// `fields` read, the `saved` combos and which of those `failed` to register,
/// judged on the combo Apply would save for each (effective_hotkey). By rank:
/// the Open settings combo is the Show / Hide one (same_combo); the saved
/// combo failed to register and the field still holds it; a key that types
/// text, bare or with Shift only, blocks typing it; a combo with Win gets the
/// Win the field cannot show.
fn hotkey_notes(
    fields: [Option<HotkeySpec>; 2],
    saved: [Option<HotkeySpec>; 2],
    failed: [bool; 2],
) -> [Option<HotkeyNote>; 2] {
    let combos = [effective_hotkey(saved[0], fields[0]), effective_hotkey(saved[1], fields[1])];
    let note = |i: usize| {
        let spec = combos[i]?;
        if i == 1 && same_combo(combos[0], combos[1]) {
            Some(HotkeyNote::SameAsShowHide)
        } else if failed[i] && combos[i] == saved[i] {
            Some(HotkeyNote::InUse)
        } else if blocks_typing(spec) {
            Some(HotkeyNote::BlocksTyping)
        } else if spec.mods & MOD_WIN.0 != 0 {
            Some(HotkeyNote::PlusWin)
        } else {
            None
        }
    };
    [note(0), note(1)]
}

/// Where the message loop sends a message while the settings window is open
/// (field_key_route).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum KeyRoute {
    /// To IsDialogMessageW: keyboard navigation (Tab, Enter, Esc, Space).
    Dialog,
    /// Past IsDialogMessageW, straight to its window: a focused hotkey field
    /// gets its keys.
    Field,
    /// Nowhere.
    Drop,
}

/// Shift, Ctrl and Alt (also as their left and right keys) and both Win keys.
fn is_modifier_vk(vk: u32) -> bool {
    matches!(vk, 0x10..=0x12 | 0xA0..=0xA5 | 0x5B | 0x5C)
}

/// How the message loop routes message `msg` (`vk` is its wParam) while the
/// settings window is open; `in_field` says a hotkey field has the focus.
/// Such a field takes every key as part of a combo, so its keys skip the
/// dialog manager, except the Tab, Enter and Esc key-downs, which navigate.
/// A modifier's key message would restart the field's capture and wipe it
/// (the Shift of Shift+Tab, the Alt of Alt+Tab), and Alt+Enter would clear
/// it, so they are dropped: GetMessageW has updated the key state anyway, and
/// the field reads the modifiers from it when the combo's key arrives.
pub fn field_key_route(in_field: bool, msg: u32, vk: u32) -> KeyRoute {
    if !in_field {
        return KeyRoute::Dialog;
    }
    match msg {
        WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP if is_modifier_vk(vk) => KeyRoute::Drop,
        WM_SYSKEYDOWN if vk == 0x0D => KeyRoute::Drop, // Alt+Enter
        WM_KEYDOWN if matches!(vk, 0x09 | 0x0D | 0x1B) => KeyRoute::Dialog, // Tab, Enter, Esc
        _ => KeyRoute::Field,
    }
}

/// The combo a WM_HOTKEY reports: its lParam is MAKELONG(mods, vk). Only the
/// Alt, Ctrl, Shift and Win bits count as modifiers.
fn hotkey_from_wm_hotkey(lparam: LPARAM) -> HotkeySpec {
    let mods = (lparam.0 & 0xFFFF) as u32 & (MOD_ALT.0 | MOD_CONTROL.0 | MOD_SHIFT.0 | MOD_WIN.0);
    HotkeySpec { mods, vk: ((lparam.0 >> 16) & 0xFFFF) as u32 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mods_hotkeyf_round_trip() {
        // Every combination of the three supported modifiers must survive the
        // RegisterHotKey MOD_* <-> hotkey-control HOTKEYF_* mapping unchanged —
        // a mismatch here would corrupt a captured combo on save/reopen.
        for m in 0..8u32 {
            let mods = (if m & 1 != 0 { MOD_CONTROL.0 } else { 0 })
                | (if m & 2 != 0 { MOD_ALT.0 } else { 0 })
                | (if m & 4 != 0 { MOD_SHIFT.0 } else { 0 });
            assert_eq!(hotkeyf_to_mods(mods_to_hotkeyf(mods)), mods);
        }
    }

    #[test]
    fn hotkey_word_round_trips_through_control_format() {
        // The control word packs vk in the low byte and HOTKEYF_* in the next:
        // what set_hotkey_control writes, read_hotkey reads back, EXT or not.
        let spec = HotkeySpec { mods: MOD_CONTROL.0 | MOD_ALT.0, vk: 0x4D };
        for ext in [false, true] {
            assert_eq!(word_to_spec(spec_to_word(spec, ext)), Some(spec), "ext {ext}");
        }
    }

    #[test]
    fn word_to_spec_drops_ext_and_incomplete_captures() {
        // HOTKEYF_EXT (0x08) only tells the control how to name the key.
        assert_eq!(word_to_spec(0x0825), Some(HotkeySpec { mods: 0, vk: 0x25 }));
        assert_eq!(
            word_to_spec(0x0E24),
            Some(HotkeySpec { mods: MOD_CONTROL.0 | MOD_ALT.0, vk: 0x24 })
        );
        // A lone modifier (Shift held, no key yet) and an empty field: no hotkey.
        assert_eq!(word_to_spec(0x0100), None);
        assert_eq!(word_to_spec(0), None);
    }

    #[test]
    fn spec_to_word_sets_ext_on_request() {
        let ctrl_alt_home = HotkeySpec { mods: MOD_CONTROL.0 | MOD_ALT.0, vk: 0x24 };
        assert_eq!(spec_to_word(ctrl_alt_home, true), 0x0E24);
        assert_eq!(spec_to_word(HotkeySpec { mods: 0, vk: 0xDB }, false), 0x00DB);
    }

    #[test]
    fn navigation_and_e0_keys_need_ext() {
        // (vk, MapVirtualKeyW(vk, MAPVK_VK_TO_VSC_EX)). The navigation keys and
        // Num Lock map to numpad scan codes, so needs_ext lists them by vk; the
        // others count by their E0 prefix, and Pause's E1 prefix does not.
        for (vk, sc) in [
            (0x25, 0x004B), // Left
            (0x24, 0x0047), // Home
            (0x21, 0x0049), // Page Up
            (0x2D, 0x0052), // Insert
            (0x2E, 0x0053), // Delete
            (0x90, 0x0045), // Num Lock
            (0x6F, 0xE035), // numpad /
            (0xAD, 0xE020), // Volume Mute
            (0x5D, 0xE05D), // Apps
        ] {
            assert!(needs_ext(vk, sc), "vk {vk:#04X}");
        }
        for (vk, sc) in [
            (0x41, 0x001E), // A
            (0xDB, 0x001A), // [
            (0x13, 0xE11D), // Pause
            (0x70, 0x003B), // F1
            (0x64, 0x004B), // numpad 4
        ] {
            assert!(!needs_ext(vk, sc), "vk {vk:#04X}");
        }
    }

    #[test]
    fn win_combo_survives_an_untouched_field() {
        let t = |mods: u32| Some(HotkeySpec { mods, vk: 0x54 }); // T
        let (win, shift, ctrl) = (MOD_WIN.0, MOD_SHIFT.0, MOD_CONTROL.0);
        // The field cannot show Win, so an untouched field reads the saved
        // combo minus Win: the saved combo stays.
        assert_eq!(effective_hotkey(t(win), t(0)), t(win));
        assert_eq!(effective_hotkey(t(win | shift), t(shift)), t(win | shift));
        // A field the user changed or cleared wins.
        assert_eq!(effective_hotkey(t(win), t(ctrl)), t(ctrl));
        assert_eq!(effective_hotkey(t(win), None), None);
        // Without Win in the saved combo the field always wins.
        assert_eq!(effective_hotkey(t(ctrl), t(0)), t(0));
    }

    #[test]
    fn modifiers_never_reach_a_focused_hotkey_field() {
        use windows::Win32::UI::WindowsAndMessaging::{WM_CHAR, WM_LBUTTONDOWN, WM_MOUSEMOVE};
        let (tab, enter, esc, m) = (0x09, 0x0D, 0x1B, 0x4D);
        // Shift, Ctrl and Alt (either side) and Win: GetMessageW has already
        // updated the key state, which the field reads when the key comes.
        for vk in (0x10..=0x12).chain(0xA0..=0xA5).chain([0x5B, 0x5C]) {
            for msg in [WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP] {
                assert_eq!(field_key_route(true, msg, vk), KeyRoute::Drop, "{msg:#06X} {vk:#04X}");
            }
        }
        // Alt+Enter would clear the field.
        assert_eq!(field_key_route(true, WM_SYSKEYDOWN, enter), KeyRoute::Drop);
        // Tab, Enter and Esc key-downs navigate: the next control, OK, Cancel.
        for vk in [tab, enter, esc] {
            assert_eq!(field_key_route(true, WM_KEYDOWN, vk), KeyRoute::Dialog, "{vk:#04X}");
        }
        // The rest goes to the field: a key with or without Alt, a key-up,
        // mouse messages and characters (whatever their wParam).
        assert_eq!(field_key_route(true, WM_KEYDOWN, m), KeyRoute::Field);
        assert_eq!(field_key_route(true, WM_SYSKEYDOWN, m), KeyRoute::Field);
        assert_eq!(field_key_route(true, WM_KEYUP, tab), KeyRoute::Field);
        for msg in [WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_CHAR] {
            assert_eq!(field_key_route(true, msg, 0x10), KeyRoute::Field, "{msg:#06X}");
        }
        // Outside a hotkey field the dialog manager sees everything.
        let elsewhere = [(WM_KEYDOWN, 0x10), (WM_SYSKEYDOWN, enter), (WM_KEYDOWN, m), (WM_CHAR, m)];
        for (msg, vk) in elsewhere {
            assert_eq!(field_key_route(false, msg, vk), KeyRoute::Dialog, "{msg:#06X} {vk:#04X}");
        }
    }

    #[test]
    fn hotkey_from_wm_hotkey_unpacks_mods_and_vk() {
        // WM_HOTKEY's lParam is MAKELONG(mods, vk): MOD_ALT | MOD_CONTROL (3)
        // and F24 (0x87).
        let ctrl_alt_f24 = HotkeySpec { mods: MOD_CONTROL.0 | MOD_ALT.0, vk: 0x87 };
        assert_eq!(hotkey_from_wm_hotkey(LPARAM(0x0087_0003)), ctrl_alt_f24);
        // MOD_NOREPEAT (0x4000), which TinyMon registers with, never leaks
        // into the modifiers.
        assert_eq!(hotkey_from_wm_hotkey(LPARAM(0x0087_4003)), ctrl_alt_f24);
    }

    #[test]
    fn enter_presses_the_focused_push_button_else_ok() {
        let n = MetricKey::ALL.len() as i32;
        // A focused push button is what Enter presses.
        for id in [
            IDCANCEL,
            IDC_APPLY,
            IDC_RESET,
            IDC_HK_TOGGLE_CLR,
            IDC_HK_SETTINGS_CLR,
            IDC_MOVEUP_BASE + 3,
            IDC_MOVEDN_BASE + (n - 1),
            IDOK,
        ] {
            assert_eq!(enter_target(id), id, "push button {id}");
        }
        // Anywhere else Enter means OK: a checkbox, an edit, a hotkey field,
        // the trackbar, no focus, and an id past the last arrow row.
        for id in [
            IDC_METRIC_BASE,
            IDC_FONT,
            IDC_INTERVAL,
            IDC_HK_TOGGLE,
            IDC_OPACITY,
            0,
            IDC_MOVEUP_BASE + n,
        ] {
            assert_eq!(enter_target(id), IDOK, "not a push button: {id}");
        }
    }

    #[test]
    fn reorder_step_follows_the_item() {
        // A click on an arrow in row 3 of 10 moves the item there: (from, to).
        assert_eq!(reorder_step(10, 3, true, false), Some((3, 2)));
        assert_eq!(reorder_step(10, 3, false, false), Some((3, 4)));
        // Nothing moves past either end.
        assert_eq!(reorder_step(10, 0, true, false), None);
        assert_eq!(reorder_step(10, 9, false, false), None);
        // A double-click's BN_DOUBLECLICKED comes from the same arrow after its
        // first click has moved the item one row: the item moves on from there.
        assert_eq!(reorder_step(10, 3, true, true), Some((2, 1)));
        assert_eq!(reorder_step(10, 3, false, true), Some((4, 5)));
        // The first click already took it to the end.
        assert_eq!(reorder_step(10, 1, true, true), None);
        assert_eq!(reorder_step(10, 8, false, true), None);
    }

    #[test]
    fn focus_target_falls_back_to_first_metric() {
        // Nothing saved yet: the first metric checkbox.
        assert_eq!(focus_target(0), IDC_METRIC_BASE);
        // Else the control that had the focus at the last deactivation.
        assert_eq!(focus_target(IDC_FONT), IDC_FONT);
    }

    #[test]
    fn layout_ids_unique_nonzero_and_inside_client() {
        // 9 metrics without the temp feature, 10 with it: both in one run.
        for n in [9, 10] {
            let client_h = layout(n).client_h;
            let places = control_places(n);
            for (i, p) in places.iter().enumerate() {
                assert_ne!(p.id, 0, "{p:?}");
                assert!(places[..i].iter().all(|q| q.id != p.id), "duplicate id: {p:?}");
                assert!(p.x >= 0 && p.y >= 0 && p.w > 0 && p.h > 0, "{p:?}");
                assert!(p.x + p.w <= CLIENT_W_TOTAL && p.y + p.h <= client_h, "{p:?}");
            }
        }
    }

    #[test]
    fn layout_controls_do_not_overlap() {
        for n in [9, 10] {
            let places = control_places(n);
            for (i, a) in places.iter().enumerate() {
                for b in &places[i + 1..] {
                    let apart = a.x + a.w <= b.x
                        || b.x + b.w <= a.x
                        || a.y + a.h <= b.y
                        || b.y + b.h <= a.y;
                    assert!(apart, "{a:?} overlaps {b:?}");
                }
            }
        }
    }

    #[test]
    fn card_controls_sit_inside_their_cards() {
        for n in [9, 10] {
            let lay = layout(n);
            let places = control_places(n);
            let rows: Vec<i32> = (0..n)
                .flat_map(|p| [IDC_METRIC_BASE + p, IDC_MOVEUP_BASE + p, IDC_MOVEDN_BASE + p])
                .collect();
            let appearance = [
                IDC_OPACITY_VAL,
                IDC_OPACITY,
                IDC_LBL_INTERVAL,
                IDC_INTERVAL,
                IDC_LBL_FONT,
                IDC_FONT,
            ];
            let hotkeys = [
                IDC_LBL_HK_TOGGLE,
                IDC_HK_TOGGLE_NOTE,
                IDC_HK_TOGGLE,
                IDC_HK_TOGGLE_CLR,
                IDC_LBL_HK_SETTINGS,
                IDC_HK_SETTINGS_NOTE,
                IDC_HK_SETTINGS,
                IDC_HK_SETTINGS_CLR,
            ];
            // Each card's top, height and controls, as paint_chrome paints it.
            let cards = [
                (lay.metrics_top, lay.metrics_h, &rows[..]),
                (lay.app_top, lay.app_h, &appearance[..]),
                (lay.hk_top, lay.hk_h, &hotkeys[..]),
            ];
            for (top, h, ids) in cards {
                for &id in ids {
                    let p = places.iter().find(|p| p.id == id).expect("placed");
                    // Between the card's sides (x 10..270), below its title
                    // band (its top 16 px) and above its bottom edge.
                    assert!(p.x >= 10 && p.x + p.w <= 270, "{p:?}");
                    assert!(p.y >= top + 16 && p.y + p.h <= top + h, "{p:?}");
                }
            }
        }
    }

    #[test]
    fn every_created_control_has_a_place() {
        for n in [9, 10] {
            let places = control_places(n);
            for id in created_ids(n) {
                assert_eq!(places.iter().filter(|p| p.id == id).count(), 1, "id {id}");
            }
            // And nothing else is placed: the table is exactly what is created.
            assert_eq!(created_ids(n).count(), places.len());
        }
    }

    #[test]
    fn each_input_is_created_right_after_its_label() {
        // A screen reader names a control without text of its own (the
        // trackbar, an edit, a hotkey field) after the closest static before
        // it in z-order, which is the creation order. So that static must be
        // the input's label: a hotkey note created between its label and its
        // field would give the field the note's text, or no name at all.
        for pair in [
            [IDC_OPACITY_VAL, IDC_OPACITY],
            [IDC_LBL_INTERVAL, IDC_INTERVAL],
            [IDC_LBL_FONT, IDC_FONT],
            [IDC_LBL_HK_TOGGLE, IDC_HK_TOGGLE],
            [IDC_LBL_HK_SETTINGS, IDC_HK_SETTINGS],
        ] {
            assert!(CREATION_ORDER.windows(2).any(|w| w == pair), "label, input: {pair:?}");
        }
    }

    #[test]
    fn settings_window_fits_the_left_monitor() {
        // The development machine's left monitor (its work area): a window
        // whose default spot runs past its right and bottom edges moves the
        // least to fit, as open() moves it with placement::fit_into.
        let left = RECT { left: -1920, top: -39, right: 0, bottom: 1041 };
        assert_eq!(fit_into(-100, 900, 562, 605, left), (-562, 436));
    }

    #[test]
    fn dpi_change_reads_the_new_dpi_and_scales_the_places() {
        // WM_DPICHANGED's wParam holds the new DPI in both words; the low word
        // counts, and 0, which would shrink every control to nothing, is 96.
        assert_eq!(dpi_from_wparam(WPARAM(0x0078_0078)), 120);
        assert_eq!(dpi_from_wparam(WPARAM(0)), 96);
        // place_controls moves each control to its Place at the new scale: at
        // 96 dpi the Place itself, and at 120 dpi OK (all 10 metrics) lands
        // where spec 8.5's manual check expects it, client (130, 670), 65 x 33.
        for n in [9, 10] {
            for p in control_places(n) {
                assert_eq!(p.scaled(Scale(96)), (p.x, p.y, p.w, p.h), "{p:?}");
            }
        }
        let ok = control_places(10).into_iter().find(|p| p.id == IDOK).expect("OK");
        assert_eq!(ok.scaled(Scale(120)), (130, 670, 65, 33));
    }

    #[test]
    fn preview_pane_fits_the_widest_widget_at_any_dpi() {
        for n in [9, 10] {
            let pane = control_places(n).into_iter().find(|p| p.id == IDC_PREVIEW).expect("pane");
            for dpi in [96, 120, 144, 168, 192] {
                let s = Scale(dpi);
                // The pane's client area: its Place less a 1 px border each side.
                let (inner, widest) = (s.px(pane.w) - 2, win_w(s, MAX_FONT_PX));
                assert!(inner >= widest, "{dpi} dpi: pane {inner} < widget {widest}");
            }
        }
    }

    #[test]
    fn fit_preview_centers_or_scales_uniformly() {
        // A widget that fits the 248 x 528 pane: 1:1, in the middle.
        assert_eq!(fit_preview(200, 221, 248, 528), (24, 153, 200, 221));
        // Too tall: down to the pane's height, the width in proportion.
        assert_eq!(fit_preview(246, 624, 248, 528), (20, 0, 208, 528));
        // Too wide: down to the pane's width, the height in proportion.
        assert_eq!(fit_preview(300, 100, 248, 528), (0, 223, 248, 82));
        // Nothing to show.
        assert_eq!(fit_preview(0, 10, 248, 528), (0, 0, 0, 0));
    }

    #[test]
    fn parse_clamped_saturates_and_clamps() {
        // Below the interval's range: its bottom.
        assert_eq!(parse_clamped("100", 250, 10000, 1000), 250);
        // More than a u32 holds: the top (the old reader kept the old value).
        assert_eq!(parse_clamped("99999999999", 250, 10000, 1000), 10000);
        // A font size the old reader wrapped to a negative i32, so to 9.
        assert_eq!(parse_clamped("3000000000", 9, 16, 13), 16);
        // Spaces around the digits do not count.
        assert_eq!(parse_clamped(" 14 ", 9, 16, 13), 14);
        // Anything but digits keeps the current value, the fallback.
        for text in ["", "12a", "-5"] {
            assert_eq!(parse_clamped(text, 9, 16, 13), 13, "{text:?}");
        }
    }

    #[test]
    fn range_labels_match_the_clamp_constants() {
        // The labels are literals (no format! outside the tests): they must
        // state the ranges apply() clamps to.
        let interval = format!("Update interval ({MIN_INTERVAL_MS}\u{2013}{MAX_INTERVAL_MS} ms)");
        assert_eq!(INTERVAL_LABEL, interval);
        assert_eq!(FONT_LABEL, format!("Font size ({MIN_FONT_PX}\u{2013}{MAX_FONT_PX} px)"));
    }

    #[test]
    fn number_fields_hold_their_largest_value() {
        // EM_LIMITTEXT: each edit takes exactly the digits of its top value.
        assert_eq!(MAX_INTERVAL_MS.to_string().len(), INTERVAL_CHARS);
        assert_eq!(MAX_FONT_PX.to_string().len(), FONT_CHARS);
    }

    #[test]
    fn owner_brackets_and_win_get_notes() {
        use HotkeyNote::{BlocksTyping, PlusWin};
        let key = |mods: u32, vk: u32| Some(HotkeySpec { mods, vk });
        // Neither combo failed to register.
        let ok = [false, false];
        // The owner's bare "]" (Show / Hide) and "[" (Open settings), saved
        // and in the fields: both keys no longer type anywhere.
        let brackets = [key(0, 0xDD), key(0, 0xDB)];
        assert_eq!(hotkey_notes(brackets, brackets, ok), [Some(BlocksTyping), Some(BlocksTyping)]);
        // A saved Win+T, which the field shows as T: the note adds the Win.
        let win_t = [key(MOD_WIN.0, 0x54), None];
        assert_eq!(hotkey_notes([key(0, 0x54), None], win_t, ok), [Some(PlusWin), None]);
        // A shortcut and a key that types nothing: no notes.
        let quiet = [key(MOD_CONTROL.0 | MOD_ALT.0, 0x4D), key(0, 0x77)];
        assert_eq!(hotkey_notes(quiet, quiet, ok), [None, None]);
        // Shift and a typing key still type: a note.
        let shifted = [key(MOD_SHIFT.0, 0xDB), key(0, 0x77)];
        assert_eq!(hotkey_notes(shifted, shifted, ok), [Some(BlocksTyping), None]);
    }

    #[test]
    fn notes_follow_the_combo_apply_would_save() {
        let key = |mods: u32, vk: u32| Some(HotkeySpec { mods, vk });
        // Neither combo failed to register.
        let ok = [false, false];
        // The saved Win+T's field now reads Ctrl+T: Apply saves Ctrl+T, so
        // the Win note goes.
        let win_t = [key(MOD_WIN.0, 0x54), None];
        assert_eq!(hotkey_notes([key(MOD_CONTROL.0, 0x54), None], win_t, ok), [None, None]);
        // A saved "]" whose field was cleared: Apply unbinds it, so no note;
        // a "]" typed into a field that was empty: a note at once.
        assert_eq!(hotkey_notes([None, None], [key(0, 0xDD), None], ok), [None, None]);
        let typed = hotkey_notes([None, key(0, 0xDD)], [None, None], ok);
        assert_eq!(typed, [None, Some(HotkeyNote::BlocksTyping)]);
    }

    #[test]
    fn same_combo_for_both_actions_is_rejected() {
        let ctrl_alt = MOD_CONTROL.0 | MOD_ALT.0;
        let x = Some(HotkeySpec { mods: ctrl_alt, vk: 0x4D });
        let y = Some(HotkeySpec { mods: ctrl_alt, vk: 0x53 });
        assert!(same_combo(x, x));
        // Two empty fields bind nothing, so nothing clashes; nor do a combo
        // and an empty field, or two different combos.
        assert!(!same_combo(None, None));
        assert!(!same_combo(x, None));
        assert!(!same_combo(x, y));
    }

    #[test]
    fn notes_rank_conflict_then_registration_then_typing() {
        use HotkeyNote::{InUse, SameAsShowHide};
        let key = |mods: u32, vk: u32| Some(HotkeySpec { mods, vk });
        let (ctrl_alt_m, bracket) = (key(MOD_CONTROL.0 | MOD_ALT.0, 0x4D), key(0, 0xDD));
        // One combo in both fields: the second field's note says so.
        let both = [ctrl_alt_m, ctrl_alt_m];
        assert_eq!(hotkey_notes(both, [None, None], [false, false]), [None, Some(SameAsShowHide)]);
        // The saved "]" failed to register and its field still holds it: in
        // use, which outranks blocking typing.
        let failed = [true, false];
        assert_eq!(hotkey_notes([bracket, None], [bracket, None], failed), [Some(InUse), None]);
        // The field holds another combo now: the failure was the old combo's.
        assert_eq!(hotkey_notes([ctrl_alt_m, None], [bracket, None], failed), [None, None]);
        // No combos: no notes, whatever the flags say.
        assert_eq!(hotkey_notes([None, None], [None, None], [true, true]), [None, None]);
    }

    #[test]
    fn conflict_notes_follow_the_combos_apply_would_save() {
        use HotkeyNote::{BlocksTyping, InUse, PlusWin, SameAsShowHide};
        let key = |mods: u32, vk: u32| Some(HotkeySpec { mods, vk });
        // A saved Win+E failed to register. Its untouched field reads E, yet
        // Apply would keep Win+E (effective_hotkey): the field still holds it.
        let win_e = [key(MOD_WIN.0, 0x45), None];
        assert_eq!(hotkey_notes([key(0, 0x45), None], win_e, [true, false]), [Some(InUse), None]);
        // E in the other field as well: Apply would save Win+E and E, which
        // differ, so nothing clashes.
        let es = [key(0, 0x45), key(0, 0x45)];
        assert_eq!(hotkey_notes(es, win_e, [false, false]), [Some(PlusWin), Some(BlocksTyping)]);
        // A config with one combo for both: its second registration fails,
        // and the clash outranks the failure.
        let both = [key(MOD_CONTROL.0 | MOD_ALT.0, 0x4D); 2];
        assert_eq!(hotkey_notes(both, both, [false, true]), [None, Some(SameAsShowHide)]);
    }
}
