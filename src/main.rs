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
mod history;
mod hotkeys;
mod icon;
mod metrics;
mod net;
mod pdh;
mod percpu;
mod placement;
mod registry;
mod render;
mod settings;
mod settings_win;
mod startup;
#[cfg(feature = "temp")]
mod temp;

use core::cell::RefCell;
use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, Ordering};
use std::path::PathBuf;

use windows::core::{w, Error, Result, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, COLORREF, ERROR_ALREADY_EXISTS, HINSTANCE, HWND, LPARAM, LRESULT,
    POINT, RECT, WIN32_ERROR, WPARAM,
};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateFontIndirectW,
    CreateSolidBrush, DeleteDC, DeleteObject, EndPaint, InvalidateRect, SelectObject, UpdateWindow,
    CLEARTYPE_QUALITY, HBITMAP, HDC, HFONT, HGDIOBJ, LOGFONTW, PAINTSTRUCT, SRCCOPY,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, ChangeWindowMessageFilterEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW, FindWindowW, GetClassNameW,
    GetClientRect, GetCursorPos, GetMessageW, GetWindowLongPtrW, GetWindowRect, IsDialogMessageW,
    IsWindow, KillTimer, LoadCursorW, MessageBoxW, PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, SetForegroundWindow, SetLayeredWindowAttributes, SetTimer,
    SetWindowLongPtrW, SetWindowPos, ShowWindow, TrackPopupMenu, TranslateMessage, CREATESTRUCTW,
    CS_HREDRAW, CS_VREDRAW, GWLP_USERDATA, GWL_EXSTYLE, HICON, HTCAPTION, HWND_TOPMOST, IDC_ARROW,
    LWA_ALPHA, MB_ICONWARNING, MB_OK, MF_CHECKED, MF_POPUP, MF_SEPARATOR, MF_STRING, MSG,
    MSGFLT_ALLOW, SM_CXSMICON, SPI_SETWORKAREA, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SWP_SHOWWINDOW, SW_HIDE, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP, WM_CLOSE,
    WM_COMMAND, WM_CREATE, WM_DESTROY, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_ENTERSIZEMOVE,
    WM_ERASEBKGND, WM_EXITSIZEMOVE, WM_HOTKEY, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_NCHITTEST, WM_NCRBUTTONUP, WM_NULL, WM_PAINT, WM_RBUTTONUP, WM_SETTINGCHANGE,
    WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT,
    WS_POPUP,
};

use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;

use dpi::Scale;
use history::{row_ids, History};
use registry::{build_sources, collect_rows, MetricSource, Row};
use render::{render_rows, widget_size, win_w, window_height, BG_COLOR, LIGHT, THEME};
use settings::Settings;
use settings_win::KeyRoute;

// ---- Layout ----------------------------------------------------------------
const MARGIN: i32 = 24; // gap from the work area's edges for the first-run spot and snaps

// ---- Misc ------------------------------------------------------------------
const TIMER_ID: usize = 1;
const CLASS_NAME: PCWSTR = w!("TinyMonWindow");

// ---- Private messages (WM_APP) ---------------------------------------------
// Every WM_APP id lives here, so none is used twice (wm_app_ids_are_distinct).
/// The tray icon's callback (add_tray_icon); lParam holds the mouse message.
const WM_TRAY: u32 = WM_APP + 1;
/// Posted by `queue_apply` when the settings window applies. The message
/// carries nothing (wParam is ignored): the settings wait in `PENDING_APPLY`.
const WM_APP_APPLY: u32 = WM_APP + 2;
/// Posted by the settings window when it closes; clears the single-instance
/// slot once that window is gone.
pub(crate) const WM_APP_SETTINGS_CLOSED: u32 = WM_APP + 3;
/// Posted by a second launch of TinyMon to the running widget, which comes
/// back (claim_single_instance, surface). It goes from one process to
/// another, which can be another build, so its value never changes.
const WM_APP_SURFACE: u32 = WM_APP + 4;
/// Posted by `apply_settings` to the open settings window after an Apply,
/// which may have registered the hotkeys again: the notes beside its hotkey
/// fields show which failed (`hotkey_status`).
pub(crate) const WM_APP_HOTKEY_STATUS: u32 = WM_APP + 5;

// ---- Tray + menu -----------------------------------------------------------
const TRAY_UID: u32 = 1;
/// The timer that retries a failed tray add (add_tray_icon) every
/// TRAY_RETRY_MS, at most TRAY_MAX_TRIES times; TIMER_ID is the sampling one.
const TRAY_TIMER_ID: usize = 2;
const TRAY_RETRY_MS: u32 = 2000;
const TRAY_MAX_TRIES: u8 = 10;
/// The shell's "TaskbarCreated" message, registered in main() (0 until then,
/// or if registering failed). Explorer broadcasts it when it creates the
/// taskbar, after a restart for example, and on Windows 10 also when the
/// primary display's DPI changes; every tray icon is added again then.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
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

/// Per-window state, owned via GWLP_USERDATA for the window's lifetime.
struct AppState {
    sources: Vec<MetricSource>,
    /// Recent bar values for the sparklines, kept per row identity (metric,
    /// label, nth), so they survive row-count changes, reorders and Apply.
    history: History,
    /// True from a build of the sources until the next tick: delta metrics
    /// (`MetricKey::is_delta`) show `registry::PENDING` meanwhile, since their
    /// primed values cover only the moment since their baseline.
    first_tick_pending: bool,
    /// `settings.pos` is the widget's home, the spot the user chose: a drag or
    /// Move to corner sets it, and an unclamped anchored resize for a new row
    /// count or font moves it along while the widget is at home (sync_layout,
    /// placement::after_relayout); a move made only to fit a work area or for
    /// a DPI change does not.
    settings: Settings,
    /// Which of the settings' hotkeys failed to register, as of the last
    /// registration (WM_CREATE, or an Apply that changed a combo or retried
    /// a failed one: hotkeys::should_register). The settings window's notes
    /// read it through hotkey_status.
    hk_status: hotkeys::RegisterResult,
    font: HFONT,
    /// Current monitor DPI (96 = 100%); drives all layout scaling.
    dpi: u32,
    /// The tray icon, drawn at the taskbar's small-icon size (tray_icon); drawn
    /// again when the taskbar is recreated, freed on destroy.
    icon_sm: HICON,
    /// Set by a tray double-click, so that the button-up after it does not
    /// toggle the widget a second time (tray_left_click).
    tray_dblclk: bool,
    /// Tray-add retries since the retry timer (TRAY_TIMER_ID) last started.
    tray_tries: u8,
    visible: bool,
    /// Click-through "lock": when on, the widget ignores the mouse (WS_EX_TRANSPARENT)
    /// so it never blocks clicks; toggled from the tray (runtime-only).
    locked: bool,
    /// True while the widget sits away from its home because home does not fit
    /// the work area there (the widget was fitted inside instead). A drag or
    /// Move to corner, which sets home, clears it; while it is set, a resize
    /// never moves home (relayout).
    displaced: bool,
    /// True from WM_ENTERSIZEMOVE to WM_EXITSIZEMOVE: the user is dragging the
    /// widget (or clicking it, which runs the same move loop).
    in_move: bool,
    /// The widget's top-left when that move loop started.
    move_start: (i32, i32),
    /// The row count the window was last sized for, rows the work-area cap
    /// leaves out included (place_widget, relayout).
    sized_rows: usize,
    /// True until the first sampling tick after startup, which puts the
    /// widget at home for its row count instead of resizing it from a corner
    /// (tick_layout), so home cannot move from one launch to the next.
    startup_layout_pending: bool,
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

/// The widget's size for its current rows at the current DPI + font, at most
/// `max_h` tall (whole rows; render_rows counts the rest as "+N"), and the row
/// count it is sized for, rows left out included.
fn fitted_size(st: &AppState, max_h: i32) -> (i32, i32, usize) {
    let mut tmp: Vec<Row> = Vec::new();
    for s in &st.sources {
        s.rows(&mut tmp);
    }
    let (w, h, _) = widget_size(Scale(st.dpi), st.settings.font_px, tmp.len(), max_h);
    (w, h, tmp.len())
}

/// Put the widget, sized for its current rows, at its home, or as close to it
/// as the work area nearest to home allows (home is kept, so the widget goes
/// back once home fits again). With no home yet (the first run), the top-right
/// corner of the primary work area becomes home and is saved. The widget is at
/// most as tall as that work area. SetWindowPos sends messages to the widget,
/// so it runs after the borrow ends.
unsafe fn place_widget(hwnd: HWND) {
    let p = state_ptr(hwnd);
    if p.is_null() {
        return;
    }
    let (x, y, w, h) = {
        let st = &mut *p;
        // The work area nearest to home at the widget's full height, or the
        // primary one on the first run; its height caps the widget's.
        let wa = match st.settings.pos {
            Some((hx, hy)) => {
                let (w, h, _) = fitted_size(st, i32::MAX);
                let (right, bottom) = (hx.saturating_add(w), hy.saturating_add(h));
                placement::work_area_near(&RECT { left: hx, top: hy, right, bottom })
            }
            None => placement::primary_work_area(),
        };
        let (w, h, n) = fitted_size(st, wa.bottom - wa.top);
        let ((x, y), displaced) = match st.settings.pos {
            Some(home) => placement::place(home, w, h, wa),
            None => {
                // The first run: the top-right corner of the primary work area.
                let home = placement::corner_xy(1, w, h, wa, Scale(st.dpi).px(MARGIN));
                st.settings.pos = Some(home);
                persist(st);
                (home, false)
            }
        };
        st.displaced = displaced;
        st.sized_rows = n;
        (x, y, w, h)
    };
    let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
}

/// Resize the widget for its current rows at the current DPI and font, from
/// `from` (WM_DPICHANGED's suggested rect) or, with None, from the window's own
/// rect, at most as tall as the work area nearest to that rect. With `anchor`,
/// the edges nearest the work area's edges stay put (a widget in a corner grows
/// away from it) and the result is fitted into the work area; without it
/// (during a drag) the top-left stays. Home follows the widget only as
/// placement::after_relayout allows: never to a clamped spot, never while
/// displaced or dragged, and never without `track_home`. Returns whether home
/// changed. SetWindowPos sends messages to the widget, so it runs after the
/// borrow ends.
unsafe fn relayout(hwnd: HWND, from: Option<RECT>, anchor: bool, track_home: bool) -> bool {
    let old = match from {
        Some(r) => r,
        None => {
            let mut r = RECT::default();
            if GetWindowRect(hwnd, &mut r).is_err() {
                return false;
            }
            r
        }
    };
    let p = state_ptr(hwnd);
    if p.is_null() {
        return false;
    }
    let (x, y, w, h, moved) = {
        let st = &mut *p;
        let wa = placement::work_area_near(&old);
        let (w, h, n) = fitted_size(st, wa.bottom - wa.top);
        let (spot, clamped) =
            if anchor { placement::anchored(old, w, h, wa) } else { ((old.left, old.top), false) };
        let (home, displaced) = placement::after_relayout(
            st.settings.pos,
            spot,
            clamped,
            st.displaced,
            st.in_move,
            track_home,
        );
        let moved = home != st.settings.pos;
        st.settings.pos = home;
        st.displaced = displaced;
        st.sized_rows = n;
        (spot.0, spot.1, w, h, moved)
    };
    let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
    moved
}

/// Whether the widget needs a resize: its row count `n` differs from the count
/// it was last sized for (`sized`), or the caller forces one (a new font or
/// metric order). Never during a drag: the first tick after the drag resizes
/// for a row count that changed meanwhile.
fn layout_dirty(sized: usize, n: usize, in_move: bool, force: bool) -> bool {
    (force || n != sized) && !in_move
}

/// Resize the widget for its `n` rows when layout_dirty says so: relayout,
/// anchored, with home following. Returns whether home changed, for the caller
/// to save. relayout sends messages to the widget, so no AppState reference
/// may be live across this call.
unsafe fn sync_layout(hwnd: HWND, n: usize, force: bool) -> bool {
    let p = state_ptr(hwnd);
    if p.is_null() || !layout_dirty((*p).sized_rows, n, (*p).in_move, force) {
        return false;
    }
    relayout(hwnd, None, true, true)
}

/// What a sampling tick does about the widget's size (tick_layout).
#[derive(Clone, Copy, PartialEq, Debug)]
enum TickLayout {
    /// Nothing: the size fits the row count, or a drag is in progress.
    Keep,
    /// Put the widget at home at its new size (place_widget); home stays.
    Place,
    /// Resize it from its nearest corner, home following (sync_layout).
    Sync,
}

/// What a tick that counted `n` rows does to a widget sized for `sized` rows.
/// At startup the widget was sized for the priming sample, which can count
/// other rows than the ticks (per-core CPU often reads few or no cores, and
/// temperatures can too). Resizing it from its nearest corner would keep a
/// lower-half widget's bottom edge and make its new top-left home, so home
/// would move at every launch: the first tick after startup (`startup`)
/// puts it at home instead. Later ticks resize as layout_dirty says.
fn tick_layout(startup: bool, sized: usize, n: usize, in_move: bool) -> TickLayout {
    if !layout_dirty(sized, n, in_move, false) {
        TickLayout::Keep
    } else if startup {
        TickLayout::Place
    } else {
        TickLayout::Sync
    }
}

/// The period of the first tick after the sources are built (at startup and on
/// a metric Apply): at most a second, so the pending marker shows only briefly.
/// That tick re-arms the timer at the configured interval.
fn first_tick_ms(interval_ms: u32) -> u32 {
    interval_ms.min(1000)
}

/// The period TIMER_ID is armed with when an Apply changes the interval.
/// While the first tick after a build of the sources is still due, it is
/// that tick's capped period (first_tick_ms), so the pending marker still
/// ends within a second; the tick then re-arms at the interval. Otherwise
/// it is the interval.
fn timer_period(interval_ms: u32, first_tick_pending: bool) -> u32 {
    if first_tick_pending {
        first_tick_ms(interval_ms)
    } else {
        interval_ms
    }
}

fn main() -> Result<()> {
    // Per-Monitor-V2 DPI awareness must precede any window/GDI sizing.
    dpi::set_process_dpi_aware();
    // SAFETY: the whole window lifecycle is Win32 FFI. Every call is annotated
    // inline; all handles are produced by the OS and used only while valid.
    unsafe {
        // One TinyMon per session: a second launch brings the running widget
        // back and ends here, before it reads or writes any file.
        if !claim_single_instance() {
            return Ok(());
        }
        // Before the window exists: WM_CREATE lets this message through UIPI,
        // and wnd_proc adds the tray icon again when it arrives.
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Relaxed);
        let hinstance = HINSTANCE(GetModuleHandleW(None)?.0);

        // The tray icon, drawn at runtime at the taskbar's small-icon size.
        // The class has no icon: the widget, a captionless tool window,
        // never shows one.
        let icon_sm = tray_icon();

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance,
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
        // The write path is resolved first: loading starts from that file.
        let cfg_path = config::resolve_write_path();
        let cfg = config::load(cfg_path.as_deref());
        let settings = Settings::from_config(&cfg);
        let dpi = dpi::system_dpi();
        let scale = Scale(dpi);
        let font = create_mono_font(scale, settings.font_px);
        let mut sources = build_sources(&settings);
        // Prime each source: instantaneous metrics (RAM, VRAM, battery) get their
        // first value and variable-row metrics (per-core CPU, GPUs) their row
        // count before the window is sized. A delta metric's primed value covers
        // only the moment since its baseline, so it shows as pending until the
        // first tick (first_tick_pending, first_tick_ms).
        for s in sources.iter_mut() {
            s.sample();
        }

        // A provisional rect: home (or (0, 0) on the first run), sized for the
        // current rows at the system DPI. WM_CREATE's place_widget moves it
        // home, or on-screen, before the window is first shown.
        let mut tmp: Vec<Row> = Vec::new();
        for s in &sources {
            s.rows(&mut tmp);
        }
        let win_h = window_height(scale, tmp.len() as i32, settings.font_px);
        let win_width = win_w(scale, settings.font_px);
        let (x, y) = settings.pos.unwrap_or((0, 0));

        let state = Box::new(AppState {
            sources,
            history: History::default(),
            first_tick_pending: true,
            settings,
            hk_status: hotkeys::RegisterResult::default(),
            font,
            dpi,
            icon_sm,
            tray_dblclk: false,
            tray_tries: 0,
            visible: true,
            locked: false,
            displaced: false,
            in_move: false,
            move_start: (0, 0),
            sized_rows: tmp.len(),
            startup_layout_pending: true,
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
        // Shown without taking the focus from the app the user is in.
        show_no_activate(hwnd);
        let _ = UpdateWindow(hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            // When the settings window is open, give it first crack at the
            // message so dialog keyboard navigation works (Tab between fields,
            // Enter presses the focused button or OK, Esc = Cancel, Space
            // toggles a checkbox).
            let sh = {
                let p = state_ptr(hwnd);
                if p.is_null() {
                    HWND(core::ptr::null_mut())
                } else {
                    (*p).settings_hwnd
                }
            };
            if !sh.0.is_null() && IsWindow(Some(sh)).as_bool() {
                // A focused hotkey field gets its keys past IsDialogMessageW,
                // which would take them as navigation, all but the Tab, Enter
                // and Esc key-downs; lone modifiers and Alt+Enter, which would
                // wipe its capture, go nowhere (field_key_route).
                let vk = msg.wParam.0 as u32;
                match settings_win::field_key_route(focused_is_hotkey(), msg.message, vk) {
                    KeyRoute::Drop => continue,
                    KeyRoute::Field => {}
                    KeyRoute::Dialog => {
                        // Dialog keyboard nav (Tab/Enter/Esc/Space) for settings;
                        // Esc sends IDCANCEL, which closes the window.
                        if IsDialogMessageW(sh, &msg).as_bool() {
                            continue;
                        }
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
/// control. While one is focused the message loop routes its keys with
/// `settings_win::field_key_route`, so that `IsDialogMessageW` does not consume
/// the keys the user is trying to bind.
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
        // Stash the Box<AppState> pointer, start the timer (its first tick
        // within a second, see first_tick_ms), place the window, let
        // TaskbarCreated through UIPI, add the tray icon (retried on failure).
        WM_CREATE => {
            let cs = lparam.0 as *const CREATESTRUCTW;
            if !cs.is_null() {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, (*cs).lpCreateParams as isize);
            }
            let p = state_ptr(hwnd);
            let interval = if p.is_null() { 1000 } else { (*p).settings.interval_ms };
            SetTimer(Some(hwnd), TIMER_ID, first_tick_ms(interval), None);
            if !p.is_null() {
                // At startup, register silently (no popup if a combo is taken),
                // keeping the result (hk_status).
                (*p).hk_status = hotkeys::register(
                    hwnd,
                    (*p).settings.hotkey_toggle,
                    (*p).settings.hotkey_settings,
                );
                // Reconcile DPI if the window opened on a non-system-DPI monitor
                // (place_widget below sizes the window for it).
                let st = &mut *p;
                let real = dpi::dpi_for_window(hwnd);
                if real != st.dpi {
                    st.dpi = real;
                    let nf = create_mono_font(Scale(real), st.settings.font_px);
                    let _ = DeleteObject(HGDIOBJ(st.font.0));
                    st.font = nf;
                }
            }
            // Before the first show: from the provisional rect to home, or as
            // close to it as the screen allows.
            place_widget(hwnd);
            // UIPI drops registered messages from senders of lower integrity,
            // so an elevated instance would never hear Explorer's
            // TaskbarCreated; allowing it does nothing at medium integrity.
            let allowed = ChangeWindowMessageFilterEx(
                hwnd,
                TASKBAR_CREATED.load(Ordering::Relaxed),
                MSGFLT_ALLOW,
                None,
            );
            debug_assert!(allowed.is_ok(), "ChangeWindowMessageFilterEx: {allowed:?}");
            if !add_tray_icon(hwnd) {
                start_tray_retry(hwnd);
            }
            LRESULT(0)
        }

        // We repaint the whole client from the back buffer — nothing to erase.
        WM_ERASEBKGND => LRESULT(1),

        // WM_TIMER dispatches on its id. TRAY_TIMER_ID runs only after a
        // failed tray add: try again (retry_tray_icon).
        WM_TIMER if wparam.0 == TRAY_TIMER_ID => {
            retry_tray_icon(hwnd);
            LRESULT(0)
        }

        // TIMER_ID, every interval_ms: sample every enabled source, record the
        // sparklines, resize if the row count changed, then repaint.
        WM_TIMER if wparam.0 == TIMER_ID => {
            let p = state_ptr(hwnd);
            if !p.is_null() {
                let (n, layout) = {
                    let st = &mut *p;
                    for s in st.sources.iter_mut() {
                        s.sample();
                    }
                    // The first tick after a build of the sources may come
                    // early (first_tick_ms): it ends the pending marker and
                    // re-arms the timer at the configured interval.
                    let interval = st.settings.interval_ms;
                    let first = core::mem::take(&mut st.first_tick_pending);
                    if first && first_tick_ms(interval) < interval {
                        SetTimer(Some(hwnd), TIMER_ID, interval, None);
                    }
                    let (rows, keys) = collect_rows(&st.sources, false);
                    st.history.record(&row_ids(&keys, &rows), &rows);
                    let n = rows.len();
                    let startup = core::mem::take(&mut st.startup_layout_pending);
                    (n, tick_layout(startup, st.sized_rows, n, st.in_move))
                };
                // A source's row count can change between ticks (temperature
                // sensors come and go): resize after the borrow, and save home
                // only when the resize moved it. The first tick after startup
                // puts the widget at home instead and never moves home.
                match layout {
                    TickLayout::Keep => {}
                    TickLayout::Place => place_widget(hwnd),
                    TickLayout::Sync => {
                        if sync_layout(hwnd, n, false) {
                            let p = state_ptr(hwnd);
                            if !p.is_null() {
                                persist(&*p);
                            }
                        }
                    }
                }
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }

        // Tray icon callback: a left click toggles show/hide (a double-click
        // too, once: tray_left_click), a right click opens the menu.
        WM_TRAY => {
            match (lparam.0 as u32) & 0xFFFF {
                WM_RBUTTONUP => show_context_menu(hwnd),
                mouse => {
                    // The flag changes under a borrow that ends before the
                    // toggle, which sends messages to the widget.
                    let p = state_ptr(hwnd);
                    let toggle = !p.is_null() && tray_left_click(&mut (*p).tray_dblclk, mouse);
                    if toggle {
                        toggle_visibility(hwnd);
                    }
                }
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
            handle_command(hwnd, wparam.0 & 0xFFFF);
            LRESULT(0)
        }

        // Settings applied (queue_apply). They wait in PENDING_APPLY and wParam
        // is ignored, so a foreign or repeated post finds nothing to apply. The
        // warning box runs a nested message loop, so it is shown only after
        // apply_settings has ended every AppState borrow.
        WM_APP_APPLY => {
            let Some(new) = take_pending_apply() else { return LRESULT(0) };
            if let Some(text) = apply_settings(hwnd, new) {
                show_warning(hwnd, &text);
            }
            LRESULT(0)
        }

        // Settings window closed: clear the single-instance slot, but only once
        // that window is really gone. A stray post while it is open would let a
        // second settings window open next to it, sharing its thread-local state.
        WM_APP_SETTINGS_CLOSED => {
            let p = state_ptr(hwnd);
            if !p.is_null() && !IsWindow(Some((*p).settings_hwnd)).as_bool() {
                (*p).settings_hwnd = HWND(core::ptr::null_mut());
            }
            LRESULT(0)
        }

        // TinyMon was started again (claim_single_instance in the new
        // process): come back, shown and at home (surface). The message
        // carries nothing, so a foreign post can only show the widget.
        WM_APP_SURFACE => {
            surface(hwnd);
            LRESULT(0)
        }

        // Global hotkeys: dispatch to the same actions as the menu. A combo
        // TinyMon owns never reaches a hotkey field of the settings window
        // (RegisterHotKey takes it first), so while such a field has the focus
        // the combo goes into it instead and does nothing else. The settings
        // window is copied out first: capture_fired_hotkey sends messages, and
        // no AppState reference may be live across it.
        WM_HOTKEY => {
            let p = state_ptr(hwnd);
            let sh = if p.is_null() { HWND(core::ptr::null_mut()) } else { (*p).settings_hwnd };
            if settings_win::capture_fired_hotkey(sh, lparam) {
                return LRESULT(0);
            }
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

        // A drag starts (a click runs the same move loop): note where from.
        WM_ENTERSIZEMOVE => {
            let p = state_ptr(hwnd);
            let mut r = RECT::default();
            if !p.is_null() && GetWindowRect(hwnd, &mut r).is_ok() {
                (*p).in_move = true;
                (*p).move_start = (r.left, r.top);
            }
            LRESULT(0)
        }

        // The drag ended: only a real move becomes home (and is saved), so a
        // click on a widget that sits away from home (to focus it for Esc)
        // keeps home. With no drag in progress (a stray post) nothing changes.
        WM_EXITSIZEMOVE => {
            let p = state_ptr(hwnd);
            let mut r = RECT::default();
            if !p.is_null()
                && core::mem::take(&mut (*p).in_move)
                && GetWindowRect(hwnd, &mut r).is_ok()
            {
                let st = &mut *p;
                if let Some(home) = placement::drag_end(st.move_start, (r.left, r.top)) {
                    st.settings.pos = Some(home);
                    st.displaced = false;
                    persist(st);
                }
            }
            LRESULT(0)
        }

        // The monitors or the resolution changed, or the work area did (the
        // taskbar moved, resized, auto-hid or re-docked): back home, or as
        // close to it as now fits.
        WM_DISPLAYCHANGE => {
            place_widget(hwnd);
            LRESULT(0)
        }
        WM_SETTINGCHANGE if wparam.0 == SPI_SETWORKAREA.0 as usize => {
            place_widget(hwnd);
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
                    let (rows, keys) = collect_rows(&st.sources, st.first_tick_pending);
                    let ids = row_ids(&keys, &rows);
                    let hist: Vec<&[f32]> = ids.iter().map(|id| st.history.get(id)).collect();
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
                        &hist,
                    );
                    let _ = BitBlt(hdc, 0, 0, w, h, Some(st.back_dc), 0, 0, SRCCOPY);
                }
            }
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }

        // The widget moved to a monitor with another scale, or its monitor's
        // scale changed: a new font, then a resize from the suggested rect (the
        // window's own when lParam has none), anchored unless a drag is in
        // progress. A system relocation never becomes home (track_home false).
        WM_DPICHANGED => {
            let p = state_ptr(hwnd);
            if !p.is_null() {
                let in_move = {
                    let st = &mut *p;
                    st.dpi = dpi::dpi_from_wparam(wparam);
                    let nf = create_mono_font(Scale(st.dpi), st.settings.font_px);
                    let _ = DeleteObject(HGDIOBJ(st.font.0));
                    st.font = nf;
                    st.in_move
                };
                let r = lparam.0 as *const RECT;
                let from = if r.is_null() { None } else { Some(*r) };
                relayout(hwnd, from, !in_move, false);
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            LRESULT(0)
        }

        // Remove the tray icon, save position, free everything, end the loop.
        WM_DESTROY => {
            remove_tray_icon(hwnd);
            hotkeys::unregister(hwnd);
            let _ = KillTimer(Some(hwnd), TIMER_ID);
            let _ = KillTimer(Some(hwnd), TRAY_TIMER_ID);
            let p = state_ptr(hwnd);
            if !p.is_null() {
                // Close the settings window first if it's still open.
                let sh = (*p).settings_hwnd;
                if !sh.0.is_null() && IsWindow(Some(sh)).as_bool() {
                    let _ = DestroyWindow(sh);
                }
                persist(&*p);
                // Detach the state before reclaiming it, so that no AppState
                // handle is live across SetWindowLongPtrW (the borrow rule)
                // and wnd_proc finds no state from here on.
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                let mut st = Box::from_raw(p); // reclaim; sources drop (PDH queries close)
                destroy_back_buffer(&mut st);
                let _ = DeleteObject(HGDIOBJ(st.font.0));
                let _ = DestroyIcon(st.icon_sm);
            }
            PostQuitMessage(0);
            LRESULT(0)
        }

        // The taskbar was created again: Explorer restarted, or (Windows 10)
        // the primary display's DPI changed. Draw the icon at the tray's
        // current size and add it again (NIM_MODIFY if it is still there),
        // restarting the retry if that fails; then free the old icon, of
        // which the shell keeps its own copy.
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            let p = state_ptr(hwnd);
            if !p.is_null() {
                let new = tray_icon();
                let old = core::mem::replace(&mut (*p).icon_sm, new);
                let _ = KillTimer(Some(hwnd), TRAY_TIMER_ID);
                if !add_tray_icon(hwnd) {
                    start_tray_retry(hwnd);
                }
                let _ = DestroyIcon(old);
            }
            LRESULT(0)
        }

        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Fetch the per-window `AppState` pointer (null before WM_CREATE stored it).
unsafe fn state_ptr(hwnd: HWND) -> *mut AppState {
    GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState
}

// ---- Single instance -------------------------------------------------------

/// Whether this launch keeps running, given its CreateMutexW call for the
/// instance mutex: `created_ok` says whether the call returned a handle, and
/// `last_error` is GetLastError right after it. A handle that comes with
/// ERROR_ALREADY_EXISTS means another TinyMon of this session holds the
/// mutex: this launch exits. Without a handle (ERROR_ACCESS_DENIED next to an
/// elevated instance, say) it runs, and a debug build always runs, so that
/// `cargo run` and debug test copies work next to an installed release.
fn instance_action(created_ok: bool, last_error: WIN32_ERROR, debug: bool) -> bool {
    let other_instance = created_ok && last_error == ERROR_ALREADY_EXISTS;
    debug || !other_instance
}

/// One TinyMon per session: create or open the named mutex
/// `Local\TinyMon.Instance`, whose handle stays open until the process exits,
/// and return whether this launch keeps running (instance_action). When
/// another instance holds the mutex, its widget, hidden or not (FindWindowW
/// finds hidden windows too), is asked to come back (WM_APP_SURFACE), and
/// this launch closes its handle and exits. main() calls it first, before any
/// file is read or written.
unsafe fn claim_single_instance() -> bool {
    let mutex = CreateMutexW(None, false, w!("Local\\TinyMon.Instance"));
    // On success the wrapper leaves the last error alone, so a mutex that was
    // there already still reads ERROR_ALREADY_EXISTS.
    let last_error = GetLastError();
    if instance_action(mutex.is_ok(), last_error, cfg!(debug_assertions)) {
        return true;
    }
    if let Ok(running) = FindWindowW(CLASS_NAME, PCWSTR::null()) {
        let _ = PostMessageW(Some(running), WM_APP_SURFACE, WPARAM(0), LPARAM(0));
    }
    if let Ok(handle) = mutex {
        let _ = CloseHandle(handle);
    }
    false
}

/// Bring the widget back for a second launch (WM_APP_SURFACE): shown without
/// taking the focus (show_no_activate), at home or as close to it as the
/// screen allows (place_widget), with its tray icon, added again if it is
/// gone (add_tray_icon; a failed add starts the retry). Those calls send
/// messages to the widget, so the flag is set before them, with no AppState
/// reference live across them.
unsafe fn surface(hwnd: HWND) {
    let p = state_ptr(hwnd);
    if p.is_null() {
        return;
    }
    (*p).visible = true;
    place_widget(hwnd);
    show_no_activate(hwnd);
    if !add_tray_icon(hwnd) {
        start_tray_retry(hwnd);
    }
}

// ---- Tray icon -------------------------------------------------------------

/// Add the notification-area (tray) icon for this window. If it is there
/// already (an add that timed out may still have landed), NIM_MODIFY updates
/// it instead, so calling this again is harmless. Returns whether the icon is
/// in the tray.
unsafe fn add_tray_icon(hwnd: HWND) -> bool {
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
    Shell_NotifyIconW(NIM_ADD, &nid).as_bool() || Shell_NotifyIconW(NIM_MODIFY, &nid).as_bool()
}

/// Retry a failed tray add every TRAY_RETRY_MS, counting the tries from zero.
unsafe fn start_tray_retry(hwnd: HWND) {
    let p = state_ptr(hwnd);
    if !p.is_null() {
        (*p).tray_tries = 0;
    }
    SetTimer(Some(hwnd), TRAY_TIMER_ID, TRAY_RETRY_MS, None);
}

/// One tick of the tray retry: add the icon again, and stop retrying once it
/// is in the tray or after TRAY_MAX_TRIES tries. A shell that hangs can block
/// each try for Shell_NotifyIconW's timeout, which the cap bounds.
unsafe fn retry_tray_icon(hwnd: HWND) {
    let p = state_ptr(hwnd);
    if p.is_null() {
        return;
    }
    let tries = {
        let st = &mut *p;
        st.tray_tries = st.tray_tries.saturating_add(1);
        st.tray_tries
    };
    if add_tray_icon(hwnd) || tries >= TRAY_MAX_TRIES {
        let _ = KillTimer(Some(hwnd), TRAY_TIMER_ID);
    }
}

/// The DPI the taskbar draws its icons at: that of Explorer's Shell_TrayWnd,
/// or the system DPI without a taskbar. GetSystemMetrics is not suitable for
/// a per-monitor-aware thread, so the tray size is asked for at this DPI.
unsafe fn tray_dpi() -> u32 {
    let dpi = match FindWindowW(w!("Shell_TrayWnd"), PCWSTR::null()) {
        Ok(tray) => GetDpiForWindow(tray),
        Err(_) => 0,
    };
    if dpi == 0 {
        dpi::system_dpi()
    } else {
        dpi
    }
}

/// The tray icon at the taskbar's small-icon size (16 px at 100%; 20, 24 and
/// 32 px at 125, 150 and 200%), so that it stays sharp at any scaling.
unsafe fn tray_icon() -> HICON {
    icon::create_app_icon(GetSystemMetricsForDpi(SM_CXSMICON, tray_dpi()).max(16))
}

/// Whether a left-button tray callback toggles the widget. A version-0 tray
/// icon reports a double-click as DOWN, UP, DBLCLK, UP: the DBLCLK sets
/// `swallow`, so the UP after it does not toggle again and a double-click
/// toggles once, like a click. A new press clears the flag, so an UP lost
/// after a double-click (released off the icon) cannot eat the next click.
fn tray_left_click(swallow: &mut bool, mouse_msg: u32) -> bool {
    match mouse_msg {
        WM_LBUTTONDOWN => *swallow = false,
        WM_LBUTTONDBLCLK => *swallow = true,
        WM_LBUTTONUP => return !core::mem::take(swallow),
        _ => {}
    }
    false
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

/// Flip the widget between shown and hidden (the tray icon, the toggle hotkey,
/// the menu's Show/Hide). Showing never activates it (show_no_activate).
unsafe fn toggle_visibility(hwnd: HWND) {
    let p = state_ptr(hwnd);
    if p.is_null() {
        return;
    }
    let visible = {
        let st = &mut *p;
        st.visible = !st.visible;
        st.visible
    };
    if visible {
        show_no_activate(hwnd);
    } else {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

/// Show the widget without activating it, at the top of the topmost band.
/// ShowWindow(SW_SHOW) would activate it, so the toggle hotkey would take the
/// keyboard focus from the app the user is in, and SW_SHOWNA alone could
/// leave it behind another always-on-top window. SetWindowPos sends messages
/// to the widget, so no AppState reference may be live across this call.
unsafe fn show_no_activate(hwnd: HWND) {
    let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW;
    let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, flags);
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
    persist(st);
}

/// Toggle click-through "lock": when on, `WS_EX_TRANSPARENT` makes the widget
/// ignore the mouse so it never blocks clicks underneath. Unlock from the tray.
/// SetWindowLongPtrW sends WM_STYLECHANGING and WM_STYLECHANGED to the widget,
/// so it runs after the borrow ends.
unsafe fn toggle_lock(hwnd: HWND) {
    let p = state_ptr(hwnd);
    if p.is_null() {
        return;
    }
    let locked = {
        let st = &mut *p;
        st.locked = !st.locked;
        st.locked
    };
    let mut ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    let bit = WS_EX_TRANSPARENT.0 as isize;
    if locked {
        ex |= bit;
    } else {
        ex &= !bit;
    }
    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex);
}

/// Move the widget to a corner (0=TL, 1=TR, 2=BL, 3=BR) of its current monitor's
/// work area with the standard margin; that spot becomes home and is saved.
unsafe fn snap_to_corner(hwnd: HWND, corner: usize) {
    let p = state_ptr(hwnd);
    let mut wr = RECT::default();
    if p.is_null() || GetWindowRect(hwnd, &mut wr).is_err() {
        return;
    }
    let (w, h) = (wr.right - wr.left, wr.bottom - wr.top);
    let (x, y) = {
        let st = &mut *p;
        let m = Scale(st.dpi).px(MARGIN);
        let home = placement::corner_xy(corner, w, h, placement::work_area_near(&wr), m);
        st.settings.pos = Some(home);
        st.displaced = false;
        persist(st);
        home
    };
    let _ = SetWindowPos(hwnd, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
}

// ---- Config ----------------------------------------------------------------

/// Save the settings as they are, home included: the single writer, so saving
/// home never clobbers metric settings and vice versa. It never reads the
/// window's rect, so a spot the widget was moved to only to fit a work area
/// cannot replace home.
fn persist(st: &AppState) {
    let Some(path) = st.cfg_path.as_deref() else { return };
    // Best-effort: the result is ignored, so a failed save is not reported.
    let _ = config::write(path, &st.settings.to_config());
}

/// Open the settings window (single-instance: focus it if already open).
/// settings_win::open sends messages to the widget, so no AppState reference
/// is live across it; the state is fetched again to store the new window.
unsafe fn open_settings(hwnd: HWND) {
    let p = state_ptr(hwnd);
    if p.is_null() {
        return;
    }
    let existing = (*p).settings_hwnd;
    if !existing.0.is_null() && IsWindow(Some(existing)).as_bool() {
        let _ = SetForegroundWindow(existing);
    } else {
        let sh = settings_win::open(hwnd);
        let p = state_ptr(hwnd);
        if !p.is_null() {
            (*p).settings_hwnd = sh;
        }
    }
}

// ---- Settings window hand-off ----------------------------------------------

thread_local! {
    /// Settings on their way from the settings window to WM_APP_APPLY. Both
    /// windows live on the UI thread, so the message itself carries nothing,
    /// and a foreign or repeated WM_APP_APPLY finds the slot empty.
    static PENDING_APPLY: RefCell<Option<Settings>> = const { RefCell::new(None) };
}

/// Put `new` in the hand-off slot, replacing settings not yet applied.
fn stash_apply(new: Settings) {
    PENDING_APPLY.with(|p| *p.borrow_mut() = Some(new));
}

/// Take the settings waiting for WM_APP_APPLY; each stash is taken only once.
fn take_pending_apply() -> Option<Settings> {
    PENDING_APPLY.with(|p| p.borrow_mut().take())
}

/// Hand `new` to the widget `main`: stash it, then post a bare WM_APP_APPLY.
/// Returns whether the post went out; if it failed, the slot is emptied again,
/// so a later stray WM_APP_APPLY cannot apply settings whose hand-off failed.
pub(crate) unsafe fn queue_apply(main: HWND, new: Settings) -> bool {
    stash_apply(new);
    let posted = PostMessageW(Some(main), WM_APP_APPLY, WPARAM(0), LPARAM(0)).is_ok();
    if !posted {
        take_pending_apply();
    }
    posted
}

/// Apply new settings in a fixed order: every AppState change under one short
/// borrow, then the resize after it (SetWindowPos sends messages to the widget,
/// and no AppState reference may be live across such a call), then persist,
/// after the resize, so a home the resize moved is saved, then a post to an
/// open settings window, whose hotkey notes follow the new registration.
/// Returns the hotkey warning, which the caller shows with no AppState
/// reference live.
unsafe fn apply_settings(hwnd: HWND, new: Settings) -> Option<String> {
    let p = state_ptr(hwnd);
    if p.is_null() {
        return None;
    }
    let (n, force, toggle, settings, warn, sh) = {
        let st = &mut *p;
        // The settings window never moves the widget: keep its home.
        let pos = st.settings.pos;
        let old = core::mem::replace(&mut st.settings, Settings { pos, ..new });
        if st.settings.opacity != old.opacity {
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), st.settings.opacity, LWA_ALPHA);
        }
        if st.settings.interval_ms != old.interval_ms {
            // A first tick still due stays capped (timer_period).
            let period = timer_period(st.settings.interval_ms, st.first_tick_pending);
            let _ = KillTimer(Some(hwnd), TIMER_ID);
            SetTimer(Some(hwnd), TIMER_ID, period, None);
        }
        let font_changed = st.settings.font_px != old.font_px;
        if font_changed {
            let nf = create_mono_font(Scale(st.dpi), st.settings.font_px);
            let _ = DeleteObject(HGDIOBJ(st.font.0));
            st.font = nf;
        }
        let order_changed = st.settings.order != old.order;
        if order_changed {
            // The sparkline history is kept: series follow their rows by identity.
            st.sources = build_sources(&st.settings);
            for s in st.sources.iter_mut() {
                s.sample();
            }
            // The new sources' delta values are not real yet: show them as
            // pending and tick early (this re-arm supersedes the interval one).
            st.first_tick_pending = true;
            SetTimer(Some(hwnd), TIMER_ID, first_tick_ms(st.settings.interval_ms), None);
        }
        // The widget's size depends on both the row count (metrics) and the
        // font (rows + width scale with it): a new font or order always
        // resizes, below; otherwise only a changed row count does.
        let force = order_changed || font_changed;
        // The hotkeys register again when a combo changed, or to retry one
        // that failed before (should_register). The warning names only the
        // combos this Apply changed (to_warn): an unchanged one that still
        // fails shows only as its note in the settings window.
        let changed = [
            st.settings.hotkey_toggle != old.hotkey_toggle,
            st.settings.hotkey_settings != old.hotkey_settings,
        ];
        if hotkeys::should_register(changed, st.hk_status) {
            hotkeys::unregister(hwnd);
            st.hk_status =
                hotkeys::register(hwnd, st.settings.hotkey_toggle, st.settings.hotkey_settings);
        }
        let warn = hotkeys::to_warn(st.hk_status, changed);
        let n = collect_rows(&st.sources, false).0.len();
        let (toggle, settings) = (st.settings.hotkey_toggle, st.settings.hotkey_settings);
        (n, force, toggle, settings, warn, st.settings_hwnd)
    };
    // Anchored to the widget's nearest corner; home follows unless the work
    // area clamps the widget or it is displaced (saved below either way).
    sync_layout(hwnd, n, force);
    let p = state_ptr(hwnd);
    if !p.is_null() {
        persist(&*p);
    }
    // An open settings window refreshes its hotkey notes (hotkey_status).
    if IsWindow(Some(sh)).as_bool() {
        let _ = PostMessageW(Some(sh), WM_APP_HOTKEY_STATUS, WPARAM(0), LPARAM(0));
    }
    let _ = InvalidateRect(Some(hwnd), None, false);
    hotkeys::conflict_message(toggle, settings, &warn)
}

/// Show an Apply warning (a hotkey that could not be registered) in a modal
/// box. Its nested message loop runs wnd_proc (timer ticks, even a tray Exit
/// that frees AppState), so it is called with no AppState reference live.
unsafe fn show_warning(hwnd: HWND, text: &str) {
    let text: Vec<u16> = text.encode_utf16().chain(core::iter::once(0)).collect();
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

/// Which hotkeys failed to register, as of the last registration: a copy, read
/// by the settings window for its notes (no failures while there is no state).
pub(crate) unsafe fn hotkey_status(hwnd: HWND) -> hotkeys::RegisterResult {
    let p = state_ptr(hwnd);
    if p.is_null() {
        hotkeys::RegisterResult::default()
    } else {
        (*p).hk_status
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

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SUCCESS};
    use windows::Win32::UI::WindowsAndMessaging::{
        PeekMessageW, HWND_MESSAGE, PM_REMOVE, WINDOW_EX_STYLE, WINDOW_STYLE,
    };

    #[test]
    fn pending_apply_is_handed_over_once() {
        stash_apply(Settings::defaults());
        assert_eq!(take_pending_apply(), Some(Settings::defaults()));
        // A second WM_APP_APPLY (a duplicate or a foreign post) finds nothing.
        assert_eq!(take_pending_apply(), None);
    }

    /// What the hand-off test above cannot see: the message queue_apply posts
    /// carries no pointer, and a post that fails leaves nothing in the slot for
    /// a later stray WM_APP_APPLY to apply.
    #[test]
    fn queue_apply_posts_a_bare_message_or_takes_the_slot_back() {
        // SAFETY: a message-only window of this test's thread stands in for the
        // widget; it is destroyed before the post that must fail.
        unsafe {
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!(""),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
            .expect("message-only window");
            assert!(queue_apply(hwnd, Settings::defaults()));
            let mut msg = MSG::default();
            let got = PeekMessageW(&mut msg, Some(hwnd), WM_APP_APPLY, WM_APP_APPLY, PM_REMOVE);
            assert!(got.as_bool());
            assert_eq!((msg.wParam.0, msg.lParam.0), (0, 0));
            assert_eq!(take_pending_apply(), Some(Settings::defaults()));
            DestroyWindow(hwnd).expect("destroy the stand-in");
            assert!(!queue_apply(hwnd, Settings::defaults()));
            assert_eq!(take_pending_apply(), None);
        }
    }

    #[test]
    fn first_tick_is_capped_at_one_second() {
        assert_eq!(first_tick_ms(250), 250);
        assert_eq!(first_tick_ms(1000), 1000);
        assert_eq!(first_tick_ms(10000), 1000);
    }

    #[test]
    fn interval_apply_keeps_a_pending_first_tick_short() {
        // An Apply that changes only the interval while the first tick after
        // a build of the sources is still due: that tick still comes within a
        // second (it re-arms at the interval), so the pending marker ends.
        assert_eq!(timer_period(10000, true), 1000);
        assert_eq!(timer_period(3000, true), 1000);
        assert_eq!(timer_period(250, true), 250);
        // Otherwise the timer runs at the new interval.
        assert_eq!(timer_period(10000, false), 10000);
        assert_eq!(timer_period(250, false), 250);
    }

    #[test]
    fn layout_dirty_on_count_change_but_not_mid_drag() {
        assert!(layout_dirty(5, 6, false, false));
        assert!(!layout_dirty(5, 5, false, false));
        // During a drag nothing resizes; the first tick after it catches up.
        assert!(!layout_dirty(5, 6, true, false));
        // A new font or metric order resizes even with the same row count.
        assert!(layout_dirty(5, 5, false, true));
    }

    #[test]
    fn first_tick_after_startup_places_the_widget_at_home() {
        // The priming sample counted 5 rows and the first tick counts 16 (or
        // the other way round): back to home at the new size.
        assert_eq!(tick_layout(true, 5, 16, false), TickLayout::Place);
        assert_eq!(tick_layout(true, 16, 5, false), TickLayout::Place);
        // Nothing to catch up, or a drag in progress: the size stays.
        assert_eq!(tick_layout(true, 5, 5, false), TickLayout::Keep);
        assert_eq!(tick_layout(true, 5, 16, true), TickLayout::Keep);
    }

    #[test]
    fn later_ticks_resize_as_layout_dirty_says() {
        assert_eq!(tick_layout(false, 5, 16, false), TickLayout::Sync);
        assert_eq!(tick_layout(false, 16, 5, false), TickLayout::Sync);
        assert_eq!(tick_layout(false, 5, 5, false), TickLayout::Keep);
        assert_eq!(tick_layout(false, 5, 16, true), TickLayout::Keep);
    }

    /// One launch of a widget whose home is `home`, in the work area `wa` at
    /// 100% and font 13, whose priming sample counts `primed` rows and whose
    /// ticks count `steady`: WM_CREATE places it for the primed rows
    /// (place_widget), then the first tick catches up as `first_tick` says,
    /// with the rules place_widget and relayout apply. Returns home after
    /// that tick, which Exit saves for the next launch.
    fn launch(
        home: (i32, i32),
        wa: RECT,
        primed: usize,
        steady: usize,
        first_tick: TickLayout,
    ) -> (i32, i32) {
        let size = |n| widget_size(Scale(96), 13, n, wa.bottom - wa.top);
        let (w, h, _) = size(primed);
        let ((x, y), displaced) = placement::place(home, w, h, wa);
        match first_tick {
            // place_widget (Place) moves the widget, never home; Keep moves
            // neither.
            TickLayout::Keep | TickLayout::Place => home,
            TickLayout::Sync => {
                let old = RECT { left: x, top: y, right: x + w, bottom: y + h };
                let (w, h, _) = size(steady);
                let (spot, clamped) = placement::anchored(old, w, h, wa);
                let (new_home, _) =
                    placement::after_relayout(Some(home), spot, clamped, displaced, false, true);
                new_home.unwrap_or(home)
            }
        }
    }

    #[test]
    fn rows_found_after_priming_do_not_move_home_across_launches() {
        // Home in the lower half of the primary work area, 70% down; the
        // priming sample counts two rows fewer than the ticks.
        let wa = RECT { left: 0, top: 0, right: 1920, bottom: 1080 };
        let home = (1696, 756);
        // Resized from its nearest corner, the widget keeps its bottom edge
        // and its new top-left becomes home: two rows (72 px) up per launch.
        let once = launch(home, wa, 4, 6, TickLayout::Sync);
        assert_eq!(once, (1696, 684));
        assert_eq!(launch(once, wa, 4, 6, TickLayout::Sync), (1696, 612));
        // The first tick after startup puts the widget at home instead.
        let first = tick_layout(true, 4, 6, false);
        let once = launch(home, wa, 4, 6, first);
        assert_eq!(once, home);
        assert_eq!(launch(once, wa, 4, 6, first), home);
    }

    /// How many times a run of left-button tray callbacks toggles the widget,
    /// starting with no double-click pending.
    fn tray_toggles(msgs: &[u32]) -> usize {
        let mut swallow = false;
        msgs.iter().filter(|&&m| tray_left_click(&mut swallow, m)).count()
    }

    #[test]
    fn tray_double_click_toggles_once() {
        // A version-0 tray icon reports a double-click as DOWN, UP, DBLCLK, UP.
        let msgs = [WM_LBUTTONDOWN, WM_LBUTTONUP, WM_LBUTTONDBLCLK, WM_LBUTTONUP];
        assert_eq!(tray_toggles(&msgs), 1);
    }

    #[test]
    fn tray_separate_clicks_each_toggle() {
        let msgs = [WM_LBUTTONDOWN, WM_LBUTTONUP, WM_LBUTTONDOWN, WM_LBUTTONUP];
        assert_eq!(tray_toggles(&msgs), 2);
    }

    #[test]
    fn tray_lost_up_after_double_click_does_not_eat_next_click() {
        // The UP after the DBLCLK never came (the button was released off the
        // icon): the next click still toggles.
        let msgs = [WM_LBUTTONDOWN, WM_LBUTTONUP, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP];
        assert_eq!(tray_toggles(&msgs), 2);
    }

    #[test]
    fn wm_app_ids_are_distinct() {
        let ids =
            [WM_TRAY, WM_APP_APPLY, WM_APP_SETTINGS_CLOSED, WM_APP_SURFACE, WM_APP_HOTKEY_STATUS];
        for (i, id) in ids.iter().enumerate() {
            // In the WM_APP range, below the registered messages (such as
            // TaskbarCreated), and each used once.
            assert!((WM_APP..0xC000).contains(id), "{id:#06X}");
            assert!(!ids[i + 1..].contains(id), "{id:#06X} twice");
        }
        // A second launch posts WM_APP_SURFACE to the running TinyMon, which
        // can be another build: its value never changes.
        assert_eq!(WM_APP_SURFACE, WM_APP + 4);
    }

    #[test]
    fn instance_action_exits_only_for_an_existing_instance() {
        // The mutex was there already: another TinyMon runs, so this one exits.
        assert!(!instance_action(true, ERROR_ALREADY_EXISTS, false));
        // This launch created the mutex: it is the only TinyMon, and runs.
        assert!(instance_action(true, ERROR_SUCCESS, false));
        // A debug build runs next to a running TinyMon too.
        assert!(instance_action(true, ERROR_ALREADY_EXISTS, true));
        // No handle (the mutex of an elevated instance, say): it runs.
        assert!(instance_action(false, ERROR_ACCESS_DENIED, false));
    }
}
