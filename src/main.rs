//! TinyMon — a tiny, always-on-top Windows resource monitor.
//!
//! Metric rows are driven by a toggleable registry (`registry::MetricSource`)
//! resolved from `settings::Settings`, rather than hardcoded. The window, tray,
//! menu, double-buffered rendering, and portable config are unchanged.

// Build as a GUI app (no console window) for normal/release builds, but keep a
// console under `cargo test` so unit-test output is visible.
#![cfg_attr(not(test), windows_subsystem = "windows")]

mod battery;
mod config;
mod dark;
mod disk;
mod dpi;
// FPS is deferred by design (no-admin/tiny/AV-clean constraints); `fps` is a
// documentation-only module — it carries the rationale + integration sketch and
// emits no metric. See src/fps.rs.
mod fps;
mod gpu;
mod hotkeys;
mod icon;
mod metrics;
mod net;
mod pdh;
mod percpu;
mod registry;
mod render;
mod settings;
mod settings_win;
mod startup;
#[cfg(feature = "temp")]
mod temp;

use core::ffi::c_void;
use std::path::PathBuf;

use windows::core::{w, Error, Result, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontIndirectW,
    CreateSolidBrush, DeleteDC, DeleteObject, EndPaint, GetMonitorInfoW, InvalidateRect,
    MonitorFromWindow, SelectObject, UpdateWindow, CLEARTYPE_QUALITY, HBITMAP, HDC, HFONT, HGDIOBJ,
    LOGFONTW, MONITORINFO, MONITOR_DEFAULTTONEAREST, PAINTSTRUCT, SRCCOPY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu,
    DestroyWindow, DispatchMessageW, GetAncestor, GetClassNameW, GetClientRect, GetCursorPos,
    GetMessageW, GetSystemMetrics, GetWindowLongPtrW, GetWindowRect, IsDialogMessageW, IsWindow,
    KillTimer, LoadCursorW, MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassW,
    SetForegroundWindow, SetLayeredWindowAttributes, SetTimer, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, TrackPopupMenu, TranslateMessage, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, GA_ROOT,
    GWLP_USERDATA, GWL_EXSTYLE, HICON, HTCAPTION, IDC_ARROW, LWA_ALPHA, MB_ICONWARNING, MB_OK,
    MF_CHECKED, MF_POPUP, MF_SEPARATOR, MF_STRING, MSG, SM_CXSCREEN, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_SHOW, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_CLOSE,
    WM_COMMAND, WM_CREATE, WM_DESTROY, WM_ERASEBKGND, WM_EXITSIZEMOVE, WM_HOTKEY, WM_KEYDOWN,
    WM_LBUTTONUP, WM_NCHITTEST, WM_NCRBUTTONUP, WM_NULL, WM_PAINT, WM_RBUTTONUP, WM_TIMER,
    WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, VK_ESCAPE, VK_RETURN, VK_TAB};

use dpi::Scale;
use registry::{build_sources, MetricSource, Row};
use render::{render_rows, win_w, window_height, BG_COLOR, LIGHT, THEME};
use settings::Settings;

// ---- Layout ----------------------------------------------------------------
const MARGIN: i32 = 24; // gap from the screen edge for the default position

// ---- Misc ------------------------------------------------------------------
const TIMER_ID: usize = 1;
const CLASS_NAME: PCWSTR = w!("TinyMonWindow");

// ---- Tray + menu -----------------------------------------------------------
const WM_TRAY: u32 = WM_APP + 1;
/// Window moved to a monitor with different DPI scaling.
const WM_DPICHANGED: u32 = 0x02E0;
const TRAY_UID: u32 = 1;
const ID_SHOWHIDE: usize = 1;
const ID_STARTUP: usize = 2;
const ID_EXIT: usize = 3;
const ID_SETTINGS: usize = 4;
const ID_LOCK: usize = 5;
const ID_SNAP_TL: usize = 6;
const ID_SNAP_TR: usize = 7;
const ID_SNAP_BL: usize = 8;
const ID_SNAP_BR: usize = 9;
const ID_THEME: usize = 10;
/// Posted by the settings window: apply a new metric order (heap-handoff).
pub(crate) const WM_APP_APPLY: u32 = WM_APP + 2;
/// Posted by the settings window when it closes (clears the single-instance slot).
pub(crate) const WM_APP_SETTINGS_CLOSED: u32 = WM_APP + 3;

/// Per-window state, owned via GWLP_USERDATA for the window's lifetime.
struct AppState {
    sources: Vec<MetricSource>,
    /// Per-row recent bar values for the sparklines (index = row position).
    history: Vec<Vec<f32>>,
    settings: Settings,
    font: HFONT,
    /// Current monitor DPI (96 = 100%); drives all layout scaling.
    dpi: u32,
    /// Runtime-drawn app icon (class, 32px) + tray variant (16px); freed on destroy.
    icon: HICON,
    icon_sm: HICON,
    visible: bool,
    /// Click-through "lock": when on, the widget ignores the mouse (WS_EX_TRANSPARENT)
    /// so it never blocks clicks; toggled from the tray (runtime-only).
    locked: bool,
    // Cached off-screen back buffer for double buffering; recreated on resize.
    back_dc: HDC,
    back_bmp: HBITMAP,
    back_prev: HGDIOBJ,
    back_w: i32,
    back_h: i32,
    cfg_path: Option<PathBuf>,
    /// The open settings window (HWND(null) when closed) — single-instance guard.
    settings_hwnd: HWND,
}

/// Resize the widget to fit its current rows at the current DPI + font.
unsafe fn apply_window_size(hwnd: HWND, st: &AppState) {
    let scale = Scale(st.dpi);
    let mut tmp: Vec<Row> = Vec::new();
    for s in &st.sources {
        s.rows(&mut tmp);
    }
    let w = win_w(scale, st.settings.font_px);
    let h = window_height(scale, tmp.len() as i32, st.settings.font_px);
    let _ = SetWindowPos(hwnd, None, 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
}

/// Recent samples kept per row for the sparklines (~last minute at a 1s interval).
const HIST_LEN: usize = 64;

/// Append the latest bar value of each row to its sparkline history, rebuilding
/// the buffer when the row count changes (a metric toggled, or a variable-row
/// metric like per-core CPU / GPUs changed count).
unsafe fn update_history(st: &mut AppState) {
    let mut rows: Vec<Row> = Vec::new();
    for s in &st.sources {
        s.rows(&mut rows);
    }
    if st.history.len() != rows.len() {
        st.history = vec![Vec::new(); rows.len()];
    }
    for (i, row) in rows.iter().enumerate() {
        let h = &mut st.history[i];
        h.push(row.bar.unwrap_or(0.0));
        if h.len() > HIST_LEN {
            h.remove(0);
        }
    }
}

fn main() -> Result<()> {
    // Per-Monitor-V2 DPI awareness must precede any window/GDI sizing.
    dpi::set_process_dpi_aware();
    // SAFETY: the whole window lifecycle is Win32 FFI. Every call is annotated
    // inline; all handles are produced by the OS and used only while valid.
    unsafe {
        let hinstance = HINSTANCE(GetModuleHandleW(None)?.0);

        // TinyMon icon drawn at runtime: 32px for the class, 16px for the tray.
        let icon = icon::create_app_icon(32);
        let icon_sm = icon::create_app_icon(16);

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance,
            hIcon: icon,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: CreateSolidBrush(COLORREF(BG_COLOR)),
            lpszClassName: CLASS_NAME,
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            return Err(Error::from_thread());
        }
        // Opt the process into dark mode before any window/control is created,
        // so the settings window's native controls paint dark.
        dark::init();
        settings_win::register_class(hinstance);

        // Resolve settings from the saved config, then build the enabled metric
        // sources (disabled metrics are never constructed -> use no resources).
        let cfg = config::load();
        let settings = Settings::from_config(&cfg);
        let cfg_path = config::resolve_write_path();
        let dpi = dpi::system_dpi();
        let scale = Scale(dpi);
        let font = create_mono_font(scale, settings.font_px);
        let mut sources = build_sources(&settings);
        // Prime each source so variable-row metrics (per-core CPU, GPUs) report
        // their row count before we size the window.
        for s in sources.iter_mut() {
            s.sample();
        }

        // Size the window from the actual current row count.
        let mut tmp: Vec<Row> = Vec::new();
        for s in &sources {
            s.rows(&mut tmp);
        }
        let win_h = window_height(scale, tmp.len() as i32, settings.font_px);
        let win_width = win_w(scale, settings.font_px);
        let (x, y) = settings
            .pos
            .unwrap_or_else(|| (GetSystemMetrics(SM_CXSCREEN) - win_width - MARGIN, MARGIN));

        let state = Box::new(AppState {
            sources,
            history: Vec::new(),
            settings,
            font,
            dpi,
            icon,
            icon_sm,
            visible: true,
            locked: false,
            back_dc: HDC(core::ptr::null_mut()),
            back_bmp: HBITMAP(core::ptr::null_mut()),
            back_prev: HGDIOBJ(core::ptr::null_mut()),
            back_w: 0,
            back_h: 0,
            cfg_path,
            settings_hwnd: HWND(core::ptr::null_mut()),
        });

        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            CLASS_NAME,
            w!("TinyMon"),
            WS_POPUP,
            x,
            y,
            win_width,
            win_h,
            None,
            None,
            Some(hinstance),
            Some(Box::into_raw(state) as *const c_void),
        )?;

        // Opacity from settings (state is now stored on the window).
        let opacity = (*state_ptr(hwnd)).settings.opacity;
        SetLayeredWindowAttributes(hwnd, COLORREF(0), opacity, LWA_ALPHA)?;
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            // When the settings window is open, give it first crack at the
            // message so dialog keyboard navigation works (Tab between fields,
            // Enter = OK, Esc = Cancel, Space toggles a checkbox, mnemonics).
            let sh = {
                let p = state_ptr(hwnd);
                if p.is_null() {
                    HWND(core::ptr::null_mut())
                } else {
                    (*p).settings_hwnd
                }
            };
            if !sh.0.is_null() && IsWindow(Some(sh)).as_bool() {
                // While a hotkey-capture control is focused, bypass IsDialogMessageW
                // so it receives the combo keys (incl. Alt-combinations) being
                // bound — otherwise IsDialogMessageW eats them as menu mnemonics /
                // nav. Tab/Enter/Esc are exceptions: Tab moves focus out, Enter
                // activates OK, Esc closes — none can be part of a valid combo (a
                // modifier is required), and a combo is captured live from its
                // modifier+key, so these navigation keys never need to reach the field.
                let capturing = focused_is_hotkey()
                    && !(msg.message == WM_KEYDOWN
                        && (msg.wParam.0 as u16 == VK_TAB.0
                            || msg.wParam.0 as u16 == VK_RETURN.0
                            || msg.wParam.0 as u16 == VK_ESCAPE.0));
                if !capturing {
                    // Dialog keyboard nav (Tab/Enter/Space/mnemonics) for settings.
                    if IsDialogMessageW(sh, &msg).as_bool() {
                        continue;
                    }
                    // Esc closes settings. IsDialogMessageW doesn't map Esc for a
                    // modeless window, so handle it here for keys it owns.
                    if msg.message == WM_KEYDOWN
                        && msg.wParam.0 as u16 == VK_ESCAPE.0
                        && GetAncestor(msg.hwnd, GA_ROOT).0 == sh.0
                    {
                        let _ = DestroyWindow(sh);
                        continue;
                    }
                }
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    Ok(())
}

/// True if the control with keyboard focus (this thread) is a hotkey-capture
/// control. While one is focused the message loop must NOT run `IsDialogMessageW`,
/// which would consume the keys the user is trying to bind.
unsafe fn focused_is_hotkey() -> bool {
    let f = GetFocus();
    if f.0.is_null() {
        return false;
    }
    let mut buf = [0u16; 24];
    let n = GetClassNameW(f, &mut buf);
    n > 0 && String::from_utf16_lossy(&buf[..n as usize]) == "msctls_hotkey32"
}

/// Window procedure. The `unsafe extern "system"` signature is mandated by
/// Win32; each branch documents the calls it makes.
unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        // Stash the Box<AppState> pointer, start the timer (interval from
        // settings), add the tray icon.
        WM_CREATE => {
            let cs = lparam.0 as *const CREATESTRUCTW;
            if !cs.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*cs).lpCreateParams as isize);
            }
            let p = state_ptr(hwnd);
            let interval = if p.is_null() { 1000 } else { (*p).settings.interval_ms };
            SetTimer(Some(hwnd), TIMER_ID, interval, None);
            if !p.is_null() {
                // At startup, register silently (no popup if a combo is taken).
                let _ = hotkeys::register(
                    hwnd,
                    (*p).settings.hotkey_toggle,
                    (*p).settings.hotkey_settings,
                );
                // Reconcile DPI if the window opened on a non-system-DPI monitor.
                let st = &mut *p;
                let real = dpi::dpi_for_window(hwnd);
                if real != st.dpi {
                    st.dpi = real;
                    let nf = create_mono_font(Scale(real), st.settings.font_px);
                    let _ = DeleteObject(HGDIOBJ(st.font.0));
                    st.font = nf;
                    apply_window_size(hwnd, st);
                }
            }
            add_tray_icon(hwnd);
            LRESULT(0)
        }

        // We repaint the whole client from the back buffer — nothing to erase.
        WM_ERASEBKGND => LRESULT(1),

        // Sample every enabled source once per second, then repaint.
        WM_TIMER => {
            let p = state_ptr(hwnd);
            if !p.is_null() {
                let st = &mut *p;
                for s in st.sources.iter_mut() {
                    s.sample();
                }
                update_history(st);
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }

        // Tray icon callback: left-click toggles show/hide, right-click menus.
        WM_TRAY => {
            match (lparam.0 as u32) & 0xFFFF {
                WM_LBUTTONUP => toggle_visibility(hwnd),
                WM_RBUTTONUP => show_context_menu(hwnd),
                _ => {}
            }
            LRESULT(0)
        }

        // Right-click on the window itself (HTCAPTION makes it non-client).
        WM_NCRBUTTONUP => {
            show_context_menu(hwnd);
            LRESULT(0)
        }

        // Menu / programmatic commands.
        WM_COMMAND => {
            handle_command(hwnd, (wparam.0 & 0xFFFF) as usize);
            LRESULT(0)
        }

        // Settings applied: reclaim the new Settings and apply each changed
        // aspect (opacity / interval / font / metrics / hotkeys), then persist.
        WM_APP_APPLY => {
            if wparam.0 != 0 {
                let new = *Box::from_raw(wparam.0 as *mut Settings);
                let p = state_ptr(hwnd);
                if !p.is_null() {
                    let st = &mut *p;
                    let pos = st.settings.pos;
                    let old = st.settings.clone();
                    st.settings = Settings { pos, ..new };

                    if st.settings.opacity != old.opacity {
                        let _ = SetLayeredWindowAttributes(
                            hwnd,
                            COLORREF(0),
                            st.settings.opacity,
                            LWA_ALPHA,
                        );
                    }
                    if st.settings.interval_ms != old.interval_ms {
                        let _ = KillTimer(Some(hwnd), TIMER_ID);
                        SetTimer(Some(hwnd), TIMER_ID, st.settings.interval_ms, None);
                    }
                    let font_changed = st.settings.font_px != old.font_px;
                    if font_changed {
                        let nf = create_mono_font(Scale(st.dpi), st.settings.font_px);
                        let _ = DeleteObject(HGDIOBJ(st.font.0));
                        st.font = nf;
                    }
                    let order_changed = st.settings.order != old.order;
                    if order_changed {
                        st.sources = build_sources(&st.settings);
                        for s in st.sources.iter_mut() {
                            s.sample();
                        }
                        st.history.clear(); // rebuilt for the new row set on next tick
                    }
                    // The widget's size depends on both the row count (metrics)
                    // and the font (rows + width scale with it), so resize when
                    // either changed.
                    if order_changed || font_changed {
                        apply_window_size(hwnd, st);
                    }
                    if st.settings.hotkey_toggle != old.hotkey_toggle
                        || st.settings.hotkey_settings != old.hotkey_settings
                    {
                        hotkeys::unregister(hwnd);
                        let res = hotkeys::register(
                            hwnd,
                            st.settings.hotkey_toggle,
                            st.settings.hotkey_settings,
                        );
                        warn_hotkey_conflicts(hwnd, st, &res);
                    }
                    let _ = InvalidateRect(Some(hwnd), None, false);
                    persist(hwnd, st);
                }
            }
            LRESULT(0)
        }

        // Settings window closed: clear the single-instance slot.
        WM_APP_SETTINGS_CLOSED => {
            let p = state_ptr(hwnd);
            if !p.is_null() {
                (*p).settings_hwnd = HWND(core::ptr::null_mut());
            }
            LRESULT(0)
        }

        // Global hotkeys: dispatch to the same actions as the menu.
        WM_HOTKEY => {
            match wparam.0 as i32 {
                hotkeys::HK_TOGGLE => toggle_visibility(hwnd),
                hotkeys::HK_SETTINGS => open_settings(hwnd),
                _ => {}
            }
            LRESULT(0)
        }

        // Drag anywhere to move (spec: WM_NCHITTEST -> HTCAPTION).
        WM_NCHITTEST => LRESULT(HTCAPTION as isize),

        // Escape hides to the tray (only "Exit" quits).
        WM_KEYDOWN => {
            if wparam.0 == 0x1B {
                hide_to_tray(hwnd);
            }
            LRESULT(0)
        }

        // Closing the window minimizes to the tray instead of exiting.
        WM_CLOSE => {
            hide_to_tray(hwnd);
            LRESULT(0)
        }

        // Persist the window position after a drag finishes.
        WM_EXITSIZEMOVE => {
            let p = state_ptr(hwnd);
            if !p.is_null() {
                persist(hwnd, &mut *p);
            }
            LRESULT(0)
        }

        // Paint: render into the cached back buffer, blit in one operation.
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);
            let p = state_ptr(hwnd);
            if !p.is_null() {
                let st = &mut *p;
                let mut rc = RECT::default();
                let _ = GetClientRect(hwnd, &mut rc);
                let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);
                ensure_back_buffer(st, hdc, w, h);
                if !st.back_dc.0.is_null() {
                    let mut rows: Vec<Row> = Vec::new();
                    for s in &st.sources {
                        s.rows(&mut rows);
                    }
                    let theme = if st.settings.light { &LIGHT } else { &THEME };
                    render_rows(
                        st.back_dc,
                        w,
                        h,
                        &rows,
                        st.font,
                        Scale(st.dpi),
                        st.settings.font_px,
                        theme,
                        &st.history,
                    );
                    let _ = BitBlt(hdc, 0, 0, w, h, Some(st.back_dc), 0, 0, SRCCOPY);
                }
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }

        // DPI changed (moved to a differently-scaled monitor): rescale font + size.
        WM_DPICHANGED => {
            let p = state_ptr(hwnd);
            if !p.is_null() {
                let st = &mut *p;
                let new_dpi = (wparam.0 & 0xFFFF) as u32;
                st.dpi = if new_dpi == 0 { 96 } else { new_dpi };
                let nf = create_mono_font(Scale(st.dpi), st.settings.font_px);
                let _ = DeleteObject(HGDIOBJ(st.font.0));
                st.font = nf;
                let scale = Scale(st.dpi);
                let mut tmp: Vec<Row> = Vec::new();
                for s in &st.sources {
                    s.rows(&mut tmp);
                }
                let w = win_w(scale, st.settings.font_px);
                let h = window_height(scale, tmp.len() as i32, st.settings.font_px);
                // Honor the OS-suggested position; keep our own row-driven size.
                let r = lparam.0 as *const RECT;
                if r.is_null() {
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        0,
                        0,
                        w,
                        h,
                        SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                } else {
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        (*r).left,
                        (*r).top,
                        w,
                        h,
                        SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }

        // Remove the tray icon, save position, free everything, end the loop.
        WM_DESTROY => {
            remove_tray_icon(hwnd);
            hotkeys::unregister(hwnd);
            let _ = KillTimer(Some(hwnd), TIMER_ID);
            let p = state_ptr(hwnd);
            if !p.is_null() {
                // Close the settings window first if it's still open.
                let sh = (*p).settings_hwnd;
                if !sh.0.is_null() && IsWindow(Some(sh)).as_bool() {
                    let _ = DestroyWindow(sh);
                }
                persist(hwnd, &mut *p);
                let mut st = Box::from_raw(p); // reclaim; sources drop (PDH queries close)
                destroy_back_buffer(&mut st);
                let _ = DeleteObject(HGDIOBJ(st.font.0));
                let _ = DestroyIcon(st.icon);
                let _ = DestroyIcon(st.icon_sm);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
            }
            PostQuitMessage(0);
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Fetch the per-window `AppState` pointer (null before WM_CREATE stored it).
unsafe fn state_ptr(hwnd: HWND) -> *mut AppState {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState
}

// ---- Tray icon -------------------------------------------------------------

/// Add the notification-area (tray) icon for this window.
unsafe fn add_tray_icon(hwnd: HWND) {
    let p = state_ptr(hwnd);
    let hicon = if p.is_null() { HICON(core::ptr::null_mut()) } else { (*p).icon_sm };
    let mut nid = NOTIFYICONDATAW {
        cbSize: core::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_UID,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: hicon,
        ..Default::default()
    };
    let tip: Vec<u16> = "TinyMon".encode_utf16().collect();
    nid.szTip[..tip.len()].copy_from_slice(&tip);
    // SAFETY: Shell_NotifyIconW reads the descriptor; hWnd/uID identify the icon.
    let _ = Shell_NotifyIconW(NIM_ADD, &nid);
}

/// Remove our tray icon (called on exit so it doesn't leave a ghost).
unsafe fn remove_tray_icon(hwnd: HWND) {
    let nid = NOTIFYICONDATAW {
        cbSize: core::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: TRAY_UID,
        ..Default::default()
    };
    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
}

// ---- Menu + commands -------------------------------------------------------

/// Build and show the context menu (used by both the tray and the window).
unsafe fn show_context_menu(hwnd: HWND) {
    let p = state_ptr(hwnd);
    if p.is_null() {
        return;
    }
    let visible = (*p).visible;
    let locked = (*p).locked;
    let light = (*p).settings.light;

    let menu = match CreatePopupMenu() {
        Ok(m) => m,
        Err(_) => return,
    };
    let show_label = if visible { w!("Hide") } else { w!("Show") };
    let _ = AppendMenuW(menu, MF_STRING, ID_SHOWHIDE, show_label);
    let _ = AppendMenuW(menu, MF_STRING, ID_SETTINGS, w!("Settings..."));
    let lock_flags = if locked { MF_STRING | MF_CHECKED } else { MF_STRING };
    let _ = AppendMenuW(menu, lock_flags, ID_LOCK, w!("Lock (click-through)"));
    if let Ok(sub) = CreatePopupMenu() {
        let _ = AppendMenuW(sub, MF_STRING, ID_SNAP_TL, w!("Top-left"));
        let _ = AppendMenuW(sub, MF_STRING, ID_SNAP_TR, w!("Top-right"));
        let _ = AppendMenuW(sub, MF_STRING, ID_SNAP_BL, w!("Bottom-left"));
        let _ = AppendMenuW(sub, MF_STRING, ID_SNAP_BR, w!("Bottom-right"));
        let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, w!("Move to corner"));
    }
    let theme_flags = if light { MF_STRING | MF_CHECKED } else { MF_STRING };
    let _ = AppendMenuW(menu, theme_flags, ID_THEME, w!("Light theme"));
    let startup_flags = if startup::is_enabled() { MF_STRING | MF_CHECKED } else { MF_STRING };
    let _ = AppendMenuW(menu, startup_flags, ID_STARTUP, w!("Start with Windows"));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(menu, MF_STRING, ID_EXIT, w!("Exit"));

    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let _ = SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, None, hwnd, None);
    let _ = DestroyMenu(menu);
    // Required after a tray-invoked TrackPopupMenu so the popup dismisses on the
    // first click outside it (MSDN KB135788).
    let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
    handle_command(hwnd, cmd.0 as usize);
}

/// Act on a menu / WM_COMMAND id.
unsafe fn handle_command(hwnd: HWND, id: usize) {
    match id {
        ID_SHOWHIDE => toggle_visibility(hwnd),
        ID_SETTINGS => open_settings(hwnd),
        ID_LOCK => toggle_lock(hwnd),
        ID_SNAP_TL => snap_to_corner(hwnd, 0),
        ID_SNAP_TR => snap_to_corner(hwnd, 1),
        ID_SNAP_BL => snap_to_corner(hwnd, 2),
        ID_SNAP_BR => snap_to_corner(hwnd, 3),
        ID_THEME => toggle_theme(hwnd),
        ID_STARTUP => {
            let _ = startup::set_enabled(!startup::is_enabled());
        }
        ID_EXIT => {
            let _ = DestroyWindow(hwnd);
        }
        _ => {}
    }
}

/// Flip the window between shown and hidden (tray-only).
unsafe fn toggle_visibility(hwnd: HWND) {
    let p = state_ptr(hwnd);
    if p.is_null() {
        return;
    }
    let st = &mut *p;
    st.visible = !st.visible;
    let _ = ShowWindow(hwnd, if st.visible { SW_SHOW } else { SW_HIDE });
}

/// Hide the window to the tray (close/Escape behavior).
unsafe fn hide_to_tray(hwnd: HWND) {
    let p = state_ptr(hwnd);
    if !p.is_null() {
        (*p).visible = false;
    }
    let _ = ShowWindow(hwnd, SW_HIDE);
}

/// Toggle the light/dark widget theme, repaint, and persist the choice.
unsafe fn toggle_theme(hwnd: HWND) {
    let p = state_ptr(hwnd);
    if p.is_null() {
        return;
    }
    let st = &mut *p;
    st.settings.light = !st.settings.light;
    let _ = InvalidateRect(Some(hwnd), None, false);
    persist(hwnd, st);
}

/// Toggle click-through "lock": when on, `WS_EX_TRANSPARENT` makes the widget
/// ignore the mouse so it never blocks clicks underneath. Unlock from the tray.
unsafe fn toggle_lock(hwnd: HWND) {
    let p = state_ptr(hwnd);
    if p.is_null() {
        return;
    }
    let st = &mut *p;
    st.locked = !st.locked;
    let mut ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    let bit = WS_EX_TRANSPARENT.0 as isize;
    if st.locked {
        ex |= bit;
    } else {
        ex &= !bit;
    }
    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
}

/// Move the widget to a corner (0=TL, 1=TR, 2=BL, 3=BR) of its current monitor's
/// work area with the standard margin, then persist the new position.
unsafe fn snap_to_corner(hwnd: HWND, corner: usize) {
    let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
    let mut mi =
        MONITORINFO { cbSize: core::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if !GetMonitorInfoW(mon, &mut mi).as_bool() {
        return;
    }
    let wa = mi.rcWork;
    let mut wr = RECT::default();
    if GetWindowRect(hwnd, &mut wr).is_err() {
        return;
    }
    let (w, h) = (wr.right - wr.left, wr.bottom - wr.top);
    let p = state_ptr(hwnd);
    let dpi = if p.is_null() { 96 } else { (*p).dpi };
    let m = Scale(dpi).px(MARGIN);
    let (x, y) = match corner {
        0 => (wa.left + m, wa.top + m),
        1 => (wa.right - w - m, wa.top + m),
        2 => (wa.left + m, wa.bottom - h - m),
        _ => (wa.right - w - m, wa.bottom - h - m),
    };
    let _ = SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    if !p.is_null() {
        persist(hwnd, &mut *p);
    }
}

// ---- Config ----------------------------------------------------------------

/// Persist the full config (position + settings) — the single writer, so saving
/// position never clobbers metric settings and vice-versa.
unsafe fn persist(hwnd: HWND, st: &mut AppState) {
    let Some(path) = st.cfg_path.clone() else { return };
    let mut rect = RECT::default();
    if GetWindowRect(hwnd, &mut rect).is_ok() {
        st.settings.pos = Some((rect.left, rect.top));
    }
    config::write(&path, &st.settings.to_config());
}

/// Open the settings window (single-instance: focus it if already open).
unsafe fn open_settings(hwnd: HWND) {
    let p = state_ptr(hwnd);
    if p.is_null() {
        return;
    }
    let existing = (*p).settings_hwnd;
    if !existing.0.is_null() && IsWindow(Some(existing)).as_bool() {
        let _ = SetForegroundWindow(existing);
    } else {
        (*p).settings_hwnd = settings_win::open(hwnd);
    }
}

/// After a user-initiated apply, warn (modal) if a configured hotkey couldn't be
/// registered — almost always because another app already owns that combo.
unsafe fn warn_hotkey_conflicts(hwnd: HWND, st: &AppState, res: &hotkeys::RegisterResult) {
    let mut lines = String::new();
    if res.toggle_failed {
        if let Some(h) = st.settings.hotkey_toggle {
            lines.push_str(&format!("\u{2022} Show / Hide: {}\n", hotkeys::format_hotkey(h)));
        }
    }
    if res.settings_failed {
        if let Some(h) = st.settings.hotkey_settings {
            lines.push_str(&format!("\u{2022} Open settings: {}\n", hotkeys::format_hotkey(h)));
        }
    }
    if lines.is_empty() {
        return;
    }
    let body = format!(
        "These hotkeys couldn't be registered (already in use by another app):\n\n{lines}\nPick a different combination in Settings."
    );
    let text: Vec<u16> = body.encode_utf16().chain(core::iter::once(0)).collect();
    let title: Vec<u16> =
        "TinyMon \u{2014} hotkey in use".encode_utf16().chain(core::iter::once(0)).collect();
    let _ = MessageBoxW(
        Some(hwnd),
        PCWSTR(text.as_ptr()),
        PCWSTR(title.as_ptr()),
        MB_OK | MB_ICONWARNING,
    );
}

/// Snapshot of the current settings (read by the settings window to seed it).
pub(crate) unsafe fn current_settings(hwnd: HWND) -> Settings {
    let p = state_ptr(hwnd);
    if p.is_null() {
        Settings::from_config(&config::Config::default())
    } else {
        (*p).settings.clone()
    }
}

// ---- Back buffer -----------------------------------------------------------

/// (Re)create the cached back buffer if missing or the client size changed.
unsafe fn ensure_back_buffer(st: &mut AppState, hdc: HDC, w: i32, h: i32) {
    if !st.back_dc.0.is_null() && st.back_w == w && st.back_h == h {
        return; // still valid — reused across frames
    }
    destroy_back_buffer(st);
    if w <= 0 || h <= 0 {
        return;
    }
    let dc = CreateCompatibleDC(Some(hdc));
    let bmp = CreateCompatibleBitmap(hdc, w, h);
    if dc.0.is_null() || bmp.0.is_null() {
        if !dc.0.is_null() {
            let _ = DeleteDC(dc);
        }
        if !bmp.0.is_null() {
            let _ = DeleteObject(HGDIOBJ(bmp.0));
        }
        return;
    }
    st.back_prev = SelectObject(dc, HGDIOBJ(bmp.0));
    st.back_dc = dc;
    st.back_bmp = bmp;
    st.back_w = w;
    st.back_h = h;
}

/// Release the cached back buffer (restore + delete the bitmap, delete the DC).
unsafe fn destroy_back_buffer(st: &mut AppState) {
    if !st.back_dc.0.is_null() {
        SelectObject(st.back_dc, st.back_prev);
        let _ = DeleteObject(HGDIOBJ(st.back_bmp.0));
        let _ = DeleteDC(st.back_dc);
    }
    st.back_dc = HDC(core::ptr::null_mut());
    st.back_bmp = HBITMAP(core::ptr::null_mut());
    st.back_prev = HGDIOBJ(core::ptr::null_mut());
    st.back_w = 0;
    st.back_h = 0;
}

// ---- Rendering -------------------------------------------------------------

/// Create a monospace (Consolas) font of the given pixel height.
pub(crate) unsafe fn create_mono_font(scale: Scale, px: i32) -> HFONT {
    let mut lf = LOGFONTW {
        lfHeight: -scale.px(px),
        lfWeight: 400, // FW_NORMAL
        lfQuality: CLEARTYPE_QUALITY,
        lfPitchAndFamily: 0x31, // FIXED_PITCH | FF_MODERN (monospace fallback)
        ..Default::default()
    };
    for (i, c) in "Consolas".encode_utf16().enumerate() {
        lf.lfFaceName[i] = c;
    }
    // SAFETY: builds a GDI font from a fully-initialized descriptor; the caller
    // owns the returned HFONT and must DeleteObject it (done in WM_DESTROY).
    CreateFontIndirectW(&lf)
}
