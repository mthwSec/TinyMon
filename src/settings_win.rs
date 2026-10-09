//! The settings window: native Win32 controls grouped into **Metrics**,
//! **Appearance**, and **Hotkeys** sections plus OK / Apply / Cancel.
//!
//! It has its own window class + wndproc and never aliases the main `AppState`.
//! It seeds its controls from `current_settings(main)` and, on Apply/OK, builds
//! a fresh `Settings` (clamped via `Settings::from_config`) and hands it to the
//! main window with `queue_apply`, which keeps it in a UI-thread slot and posts
//! a bare `WM_APP_APPLY`. Closing posts `WM_APP_SETTINGS_CLOSED` (it never quits
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
    SetDCBrushColor, SetTextColor, BLENDFUNCTION, CLEARTYPE_QUALITY, DC_BRUSH, DRAW_TEXT_FORMAT,
    DT_CENTER, DT_LEFT, DT_SINGLELINE, DT_VCENTER, HBRUSH, HDC, HFONT, HGDIOBJ, LOGFONTW,
    NULL_BRUSH, PAINTSTRUCT, PS_SOLID, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    InitCommonControlsEx, CDDS_ITEMPREPAINT, CDDS_PREPAINT, DRAWITEMSTRUCT, HKM_GETHOTKEY,
    HKM_SETHOTKEY, HOTKEYF_ALT, HOTKEYF_CONTROL, HOTKEYF_EXT, HOTKEYF_SHIFT, ICC_BAR_CLASSES,
    ICC_HOTKEY_CLASS, ICC_STANDARD_CLASSES, INITCOMMONCONTROLSEX, NMCUSTOMDRAW, NMHDR, ODS_DEFAULT,
    ODS_FOCUS, ODS_SELECTED, TBM_SETPOS, TBM_SETRANGE,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, MapVirtualKeyW, SetFocus, MAPVK_VK_TO_VSC_EX, MOD_ALT, MOD_CONTROL, MOD_SHIFT,
    MOD_WIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetDlgCtrlID, GetDlgItem,
    GetParent, GetWindowLongPtrW, GetWindowRect, GetWindowTextW, IsChild, PostMessageW,
    RegisterClassW, SendMessageW, SetWindowLongPtrW, SetWindowPos, BS_OWNERDRAW, CREATESTRUCTW,
    CW_USEDEFAULT, DC_HASDEFID, DM_GETDEFID, ES_NUMBER, GWLP_USERDATA, HMENU, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOZORDER, WINDOW_EX_STYLE, WINDOW_STYLE, WM_COMMAND, WM_CREATE, WM_CTLCOLORBTN,
    WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DRAWITEM, WM_ERASEBKGND, WM_GETFONT,
    WM_HSCROLL, WM_KEYDOWN, WM_KEYUP, WM_NOTIFY, WM_PAINT, WM_SETFOCUS, WM_SETFONT, WM_SETTEXT,
    WM_SYSKEYDOWN, WM_SYSKEYUP, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_OVERLAPPED,
    WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
};

use crate::config::Config;
use crate::dark;
use crate::dpi::{dpi_for_window, Scale};
use crate::hotkeys::{format_hotkey, HotkeySpec};
use crate::registry::{sample_rows, MetricKey};
use crate::render::{render_rows, window_height, LIGHT, THEME};
use crate::settings::{Settings, DEFAULT_FONT_PX, MAX_FONT_PX, MIN_FONT_PX};
use crate::{create_mono_font, current_settings, queue_apply, WM_APP_SETTINGS_CLOSED};

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
const EN_CHANGE: u32 = 0x0300;

thread_local! {
    /// The settings window's DPI (96 = 100%), set in `build_controls`. Every
    /// coordinate/size/font goes through `dscale()` so the window scales on
    /// high-DPI displays (the layout is authored in logical 96-dpi units).
    static SCALE: Cell<u32> = const { Cell::new(96) };
    /// The settings window's Segoe UI fonts — regular for controls, semibold for
    /// section titles. Built per-open at the window's DPI; freed on close.
    static UI_FONT: Cell<isize> = const { Cell::new(0) };
    static HDR_FONT: Cell<isize> = const { Cell::new(0) };
    /// The working metric list: every metric in display order, each with its
    /// enabled flag. The row checkboxes mirror this; ▲/▼ reorder it; Apply reads it.
    static METRIC_STATE: RefCell<Vec<(MetricKey, bool)>> = const { RefCell::new(Vec::new()) };
}

/// The settings window's current DPI scale (set in `build_controls`).
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

/// (Re)create the cached UI fonts (at the current DPI scale).
unsafe fn make_fonts() {
    free_fonts();
    UI_FONT.with(|c| c.set(create_ui_font(12, 400).0 as isize));
    HDR_FONT.with(|c| c.set(create_ui_font(12, 600).0 as isize));
}

/// The regular control font (null until `make_fonts`).
unsafe fn ui_font() -> HFONT {
    HFONT(UI_FONT.with(|c| c.get()) as *mut c_void)
}
/// The semibold section-title font.
unsafe fn hdr_font() -> HFONT {
    HFONT(HDR_FONT.with(|c| c.get()) as *mut c_void)
}
/// Delete the cached fonts (on settings-window close).
unsafe fn free_fonts() {
    for cell in [&UI_FONT, &HDR_FONT] {
        let h = cell.with(|c| c.get());
        if h != 0 {
            let _ = DeleteObject(HGDIOBJ(h as *mut c_void));
            cell.with(|c| c.set(0));
        }
    }
}

const CLIENT_W: i32 = 280; // left controls column
const GUTTER: i32 = 12;
const PREVIEW_W: i32 = 210;
const CLIENT_W_TOTAL: i32 = CLIENT_W + GUTTER + PREVIEW_W;
const PREVIEW_CLASS: PCWSTR = w!("TinyMonPreview");

/// Vertical layout of the three sections — shared by `open` (to size the window)
/// and `build_controls` (to place controls), so they can't drift apart.
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

fn layout() -> Layout {
    let n = MetricKey::ALL.len() as i32;
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

/// Register the settings window class + init common controls (once at startup).
pub unsafe fn register_class(hinstance: HINSTANCE) {
    let icc = INITCOMMONCONTROLSEX {
        dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_BAR_CLASSES | ICC_HOTKEY_CLASS | ICC_STANDARD_CLASSES,
    };
    let _ = InitCommonControlsEx(&icc);
    let wc = WNDCLASSW {
        lpfnWndProc: Some(settings_proc),
        hInstance: hinstance,
        lpszClassName: CLASS,
        hbrBackground: dark::bg_brush(),
        ..Default::default()
    };
    RegisterClassW(&wc);
    // The live-preview child class.
    let pwc = WNDCLASSW {
        lpfnWndProc: Some(preview_proc),
        hInstance: hinstance,
        lpszClassName: PREVIEW_CLASS,
        ..Default::default()
    };
    RegisterClassW(&pwc);
}

/// Open the settings window; returns its handle (HWND(null) on failure).
pub unsafe fn open(main: HWND) -> HWND {
    let hinstance = HINSTANCE(GetModuleHandleW(None).map(|h| h.0).unwrap_or(core::ptr::null_mut()));
    let lay = layout();
    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        CLASS,
        w!("TinyMon Settings"),
        WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
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

    // Grow the window so the *client* area is exactly (CLIENT_W, client_h),
    // accounting for the caption + borders (DPI/theme-robust, vs. guessing).
    if !hwnd.0.is_null() {
        let (mut cr, mut wr) = (RECT::default(), RECT::default());
        if GetClientRect(hwnd, &mut cr).is_ok() && GetWindowRect(hwnd, &mut wr).is_ok() {
            let nc_w = (wr.right - wr.left) - (cr.right - cr.left);
            let nc_h = (wr.bottom - wr.top) - (cr.bottom - cr.top);
            // Client area scaled to the window's DPI (set by build_controls during
            // WM_CREATE); non-client (caption/border) is already in device px.
            let s = dscale();
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                s.px(CLIENT_W_TOTAL) + nc_w,
                s.px(lay.client_h) + nc_h,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        // Dark title bar + dark mode for the frame (controls are themed in
        // `control`). Done after sizing so the frame repaints once.
        dark::apply_window(hwnd);
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

        // Keep keyboard focus on a real control (so Tab navigation has a
        // starting point) whenever the frame itself would take focus.
        WM_SETFOCUS => {
            if let Ok(c) = GetDlgItem(Some(hwnd), IDC_METRIC_BASE) {
                let _ = SetFocus(Some(c));
            }
            LRESULT(0)
        }

        // Opacity trackbar moved: reflect the live value + update the preview.
        WM_HSCROLL => {
            if let Ok(tb) = GetDlgItem(Some(hwnd), IDC_OPACITY) {
                let pos = SendMessageW(tb, TBM_GETPOS, None, None).0;
                if let Ok(lbl) = GetDlgItem(Some(hwnd), IDC_OPACITY_VAL) {
                    set_text(lbl, &format!("Opacity   {}", pos));
                }
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
            match id {
                IDOK => {
                    apply(hwnd);
                    let _ = DestroyWindow(hwnd);
                }
                IDC_APPLY => apply(hwnd),
                IDCANCEL => {
                    let _ = DestroyWindow(hwnd);
                }
                IDC_RESET => {
                    reset_to_defaults(hwnd);
                    refresh_preview(hwnd);
                }
                IDC_HK_TOGGLE_CLR => clear_hotkey(hwnd, IDC_HK_TOGGLE),
                IDC_HK_SETTINGS_CLR => clear_hotkey(hwnd, IDC_HK_SETTINGS),
                // Live-preview triggers: the font edit changed, or a metric toggled.
                IDC_FONT if notif == EN_CHANGE => refresh_preview(hwnd),
                _ if (IDC_METRIC_BASE..IDC_METRIC_BASE + MetricKey::ALL.len() as i32)
                    .contains(&id) =>
                {
                    // Toggle this row's metric on/off in the working list.
                    let p = (id - IDC_METRIC_BASE) as usize;
                    METRIC_STATE.with(|st| {
                        let mut s = st.borrow_mut();
                        s[p].1 = !s[p].1;
                    });
                    render_metric_rows(hwnd);
                    refresh_preview(hwnd);
                }
                _ if (IDC_MOVEUP_BASE..IDC_MOVEUP_BASE + MetricKey::ALL.len() as i32)
                    .contains(&id) =>
                {
                    let p = (id - IDC_MOVEUP_BASE) as usize;
                    if p > 0 {
                        METRIC_STATE.with(|st| st.borrow_mut().swap(p, p - 1));
                        render_metric_rows(hwnd);
                        refresh_preview(hwnd);
                    }
                }
                _ if (IDC_MOVEDN_BASE..IDC_MOVEDN_BASE + MetricKey::ALL.len() as i32)
                    .contains(&id) =>
                {
                    let p = (id - IDC_MOVEDN_BASE) as usize;
                    if p + 1 < MetricKey::ALL.len() {
                        METRIC_STATE.with(|st| st.borrow_mut().swap(p, p + 1));
                        render_metric_rows(hwnd);
                        refresh_preview(hwnd);
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }

        WM_DESTROY => {
            free_fonts();
            METRIC_STATE.with(|s| s.borrow_mut().clear());
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
        // edit + hotkey fields get the recessed input fill.
        WM_CTLCOLORSTATIC => {
            let hdc = HDC(wparam.0 as *mut c_void);
            SetTextColor(hdc, COLORREF(dark::TEXT));
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
    make_fonts();
    let font = HGDIOBJ(ui_font().0);
    let cur = current_settings(main);
    let lay = layout();

    // The layout is authored in logical (96-dpi) units; the child helper scales
    // every control's position/size to the window's DPI.
    let s = dscale();
    let child = |class, text, style: u32, x, y, w, h, id| unsafe {
        control(
            hwnd,
            class,
            text,
            WINDOW_STYLE(style),
            s.px(x),
            s.px(y),
            s.px(w),
            s.px(h),
            id,
            hinstance,
            font,
        )
    };
    let base = WS_CHILD.0 | WS_VISIBLE.0;
    let tab = base | WS_TABSTOP.0;
    // Push buttons + metric checkboxes are owner-drawn (Win10 won't theme them
    // dark); see WM_DRAWITEM. The metric checkboxes track their own checked
    // state in GWLP_USERDATA (BS_OWNERDRAW has no built-in check state).
    let btn = tab | BS_OWNERDRAW as u32;

    // --- Metrics (single ordered list; ▲/▼ reorder) --- (card painted in `paint_chrome`)
    init_metric_state(&cur);
    for p in 0..MetricKey::ALL.len() as i32 {
        let y = lay.metrics_top + 18 + p * 22;
        child(w!("BUTTON"), "", btn, 22, y, 190, 18, IDC_METRIC_BASE + p); // checkbox (label set on render)
        child(w!("BUTTON"), "\u{25B2}", btn, 216, y, 22, 18, IDC_MOVEUP_BASE + p); // ▲ move up
        child(w!("BUTTON"), "\u{25BC}", btn, 240, y, 22, 18, IDC_MOVEDN_BASE + p);
        // ▼ move down
    }
    render_metric_rows(hwnd);

    // --- Appearance --- (card panel + title painted in `paint_chrome`)
    let a = lay.app_top;
    let opacity_text = format!("Opacity   {}", cur.opacity);
    child(w!("STATIC"), &opacity_text, base, 22, a + 22, 220, 16, IDC_OPACITY_VAL);
    let tb = child(w!("msctls_trackbar32"), "", tab, 20, a + 40, 238, 28, IDC_OPACITY);
    SendMessageW(tb, TBM_SETRANGE, Some(WPARAM(1)), Some(range(40, 255)));
    SendMessageW(tb, TBM_SETPOS, Some(WPARAM(1)), Some(LPARAM(cur.opacity as isize)));

    let interval_text = cur.interval_ms.to_string();
    child(w!("STATIC"), "Update interval (ms)", base, 22, a + 80, 150, 16, 0);
    child(
        w!("EDIT"),
        &interval_text,
        tab | WS_BORDER.0 | ES_NUMBER as u32,
        196,
        a + 78,
        62,
        22,
        IDC_INTERVAL,
    );

    let font_text = cur.font_px.to_string();
    child(w!("STATIC"), "Font size (px)", base, 22, a + 108, 150, 16, 0);
    child(
        w!("EDIT"),
        &font_text,
        tab | WS_BORDER.0 | ES_NUMBER as u32,
        196,
        a + 106,
        62,
        22,
        IDC_FONT,
    );

    // --- Hotkeys (each capture field has a Clear button) --- (card in `paint_chrome`)
    let k = lay.hk_top;
    child(w!("STATIC"), "Show / Hide", base, 22, k + 20, 160, 16, 0);
    let hk1 =
        child(w!("msctls_hotkey32"), "", tab | WS_BORDER.0, 22, k + 38, 170, 22, IDC_HK_TOGGLE);
    set_hotkey_control(hk1, cur.hotkey_toggle);
    child(w!("BUTTON"), "Clear", btn, 200, k + 38, 56, 22, IDC_HK_TOGGLE_CLR);
    child(w!("STATIC"), "Open settings", base, 22, k + 66, 160, 16, 0);
    let hk2 =
        child(w!("msctls_hotkey32"), "", tab | WS_BORDER.0, 22, k + 84, 170, 22, IDC_HK_SETTINGS);
    set_hotkey_control(hk2, cur.hotkey_settings);
    child(w!("BUTTON"), "Clear", btn, 200, k + 84, 56, 22, IDC_HK_SETTINGS_CLR);

    // --- Buttons (OK is the primary; Esc => Cancel) ---
    // NB: BS_DEFPUSHBUTTON can't coexist with BS_OWNERDRAW (they share BS_TYPEMASK),
    // so OK is the default only by its id: draw_button fills it cyan, and the
    // DM_GETDEFID answer (enter_target) makes Enter press OK unless another push
    // button has the focus, which Enter then presses instead.
    let b = lay.btn_top;
    child(w!("BUTTON"), "Reset", btn, 12, b, 64, 26, IDC_RESET);
    child(w!("BUTTON"), "OK", btn, 104, b, 52, 26, IDOK);
    child(w!("BUTTON"), "Apply", btn, 160, b, 52, 26, IDC_APPLY);
    child(w!("BUTTON"), "Cancel", btn, 216, b, 52, 26, IDCANCEL);

    // --- Live preview (right column) — a real mini-widget rendered with the SAME
    // render_rows as the widget, updated as the controls change. (card in `paint_chrome`) ---
    let px = CLIENT_W + GUTTER;
    child(
        PREVIEW_CLASS,
        "",
        base | WS_BORDER.0,
        px + 10,
        26,
        PREVIEW_W - 20,
        lay.client_h - 44,
        IDC_PREVIEW,
    );

    // Start focus on the first checkbox so Tab has somewhere to go; seed preview.
    if let Ok(c) = GetDlgItem(Some(hwnd), IDC_METRIC_BASE) {
        let _ = SetFocus(Some(c));
    }
    refresh_preview(hwnd);
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

/// Render a faithful mini-widget from the parent settings' live control values,
/// using the SAME `render_rows` as the real widget, dimmed by the opacity slider.
unsafe fn paint_preview(hwnd: HWND) {
    let parent = GetParent(hwnd).unwrap_or_default();
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
    if w > 0 && h > 0 && !parent.0.is_null() {
        // Enabled order from the parent's checkboxes.
        let order = enabled_order();
        let rows = sample_rows(&order);
        let opacity = read_trackbar(parent, IDC_OPACITY).clamp(40, 255) as u8;
        let font_px = (read_edit_u32(parent, IDC_FONT, DEFAULT_FONT_PX as u32) as i32)
            .clamp(MIN_FONT_PX, MAX_FONT_PX);
        let scale = Scale(dpi_for_window(hwnd));
        let font = create_mono_font(scale, font_px);

        // The mini-widget's natural height; render it that tall and center it in
        // the (taller) pane so it doesn't sit atop a large empty void.
        let ch = window_height(scale, rows.len() as i32, font_px).min(h);
        let y_off = ((h - ch) / 2).max(0);

        // Render the widget into an off-screen buffer, fill the pane with the
        // widget bg, then blend the content (centered) at the chosen opacity
        // (simulating the layered widget's translucency over its own bg).
        let mem = CreateCompatibleDC(Some(hdc));
        let bmp = CreateCompatibleBitmap(hdc, w, ch);
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
        let main = HWND(GetWindowLongPtrW(parent, GWLP_USERDATA) as *mut c_void);
        let theme = if !main.0.is_null() && current_settings(main).light { &LIGHT } else { &THEME };
        let hist: Vec<&[f32]> = preview_hist.iter().map(Vec::as_slice).collect();
        render_rows(mem, w, ch, &rows, font, scale, font_px, theme, &hist);
        SetDCBrushColor(hdc, COLORREF(theme.bg));
        FillRect(hdc, &rc, HBRUSH(GetStockObject(DC_BRUSH).0));
        let bf = BLENDFUNCTION {
            BlendOp: 0,
            BlendFlags: 0,
            SourceConstantAlpha: opacity,
            AlphaFormat: 0,
        };
        let _ = AlphaBlend(hdc, 0, y_off, w, ch, mem, 0, 0, w, ch, bf);

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

    let lay = layout();
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

/// Read the controls, build a clamped `Settings`, and hand it to the main window
/// (`queue_apply`: nothing is left behind if the post fails).
unsafe fn apply(hwnd: HWND) {
    let main = HWND(GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut c_void);
    if main.0.is_null() {
        return;
    }
    let cur = current_settings(main);

    // Enabled metrics in the user's arranged order (from the reorderable list).
    let metrics: Vec<String> = enabled_order().iter().map(|k| k.as_str().to_string()).collect();
    let opacity = read_trackbar(hwnd, IDC_OPACITY).clamp(0, 255) as u8;
    let interval = read_edit_u32(hwnd, IDC_INTERVAL, cur.interval_ms);
    let font = read_edit_u32(hwnd, IDC_FONT, cur.font_px as u32) as i32;

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
    queue_apply(main, new);
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

/// Read an edit control's text as a u32 (falling back to `default` if empty).
unsafe fn read_edit_u32(dlg: HWND, id: i32, default: u32) -> u32 {
    let Ok(ctrl) = GetDlgItem(Some(dlg), id) else { return default };
    let mut buf = [0u16; 16];
    let n = GetWindowTextW(ctrl, &mut buf);
    if n <= 0 {
        return default;
    }
    String::from_utf16_lossy(&buf[..n as usize]).trim().parse().unwrap_or(default)
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
}
