//! Shared widget renderer + layout math.
//!
//! Both the always-on-top widget and (later) the settings live-preview draw
//! through [`render_rows`], so the preview can never visually drift from the
//! real widget. All sizes are authored in logical (96-dpi) units and converted
//! to device pixels via [`Scale`].

use windows::Win32::Foundation::{COLORREF, POINT, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreatePen, DeleteObject, DrawTextW, FillRect, GetStockObject, GetTextExtentPoint32W, Polygon,
    Polyline, SelectObject, SetBkMode, SetDCBrushColor, SetTextColor, DC_BRUSH, DT_END_ELLIPSIS,
    DT_LEFT, DT_RIGHT, DT_SINGLELINE, DT_VCENTER, HBRUSH, HDC, HFONT, HGDIOBJ, NULL_PEN, PS_SOLID,
    TRANSPARENT,
};

use crate::dpi::Scale;
use crate::metrics::{load_color, RED};
use crate::registry::Row;

// ---- Layout (logical pixels; scaled by `Scale` at render/size time) --------
pub const PAD: i32 = 8;
pub const COL_GAP: i32 = 8; // min gap between the label and value columns
pub const BAR_GAP: i32 = 3;
pub const BAR_H: i32 = 8;
pub const ROW_GAP: i32 = 8;
/// Widget width and the font size it's tuned for; the widget scales with the
/// chosen font (bigger font -> taller rows AND a proportionally wider window).
pub const WIN_W_BASE: i32 = 200;
pub const FONT_PX_BASE: i32 = 13;

// ---- Colors (0x00BBGGRR) ---------------------------------------------------
pub const BG_COLOR: u32 = 0x0020_2020;
pub const FG_COLOR: u32 = 0x00E0_E0E0;
pub const TRACK_COLOR: u32 = 0x0040_4040;
/// Dim gray for the header title.
pub const HEADER_FG: u32 = 0x0080_8080;
/// 1px top edge on the dark bar track (a subtle inset bevel), a step lighter
/// than the track; `LIGHT` uses a darker edge instead, as `emphasize` does.
const TRACK_EDGE: u32 = 0x0050_5050;

/// Emphasize a fill's highlight / top-line: on the dark theme move the channel
/// ~30% toward white (a lit top edge); on the light theme move it ~30% toward
/// black instead, so the accent stays crisp on the pale background rather than
/// washing out into it.
fn emphasize(c: u8, light_bg: bool) -> u8 {
    if light_bg {
        (c as u32 * 7 / 10) as u8
    } else {
        (c as u32 + (255 - c as u32) * 3 / 10) as u8
    }
}

/// Color scheme handed to [`render_rows`]. Fixed today, but threading it keeps
/// the renderer a pure function of its inputs so the preview reuses it as-is.
pub struct RenderTheme {
    pub bg: u32,
    pub fg: u32,
    pub track: u32,
    pub header_fg: u32,
    pub track_edge: u32,
}

/// Built-in dark theme (the default).
pub const THEME: RenderTheme = RenderTheme {
    bg: BG_COLOR,
    fg: FG_COLOR,
    track: TRACK_COLOR,
    header_fg: HEADER_FG,
    track_edge: TRACK_EDGE,
};

/// Built-in light theme (opt-in). Load-colored bars/sparklines read fine on it.
pub const LIGHT: RenderTheme = RenderTheme {
    bg: 0x00F3_F3F3,
    fg: 0x0020_2020,
    track: 0x00C8_C8C8,
    header_fg: 0x0070_7070,
    track_edge: 0x00B4_B4B4,
};

/// Height of one label line at `scale`/`font_px` (glyph box + a little leading).
pub fn label_h(scale: Scale, font_px: i32) -> i32 {
    scale.px(font_px + 4)
}
/// Full vertical block for one metric row (label line + gap + bar).
pub fn row_block(scale: Scale, font_px: i32) -> i32 {
    label_h(scale, font_px) + scale.px(BAR_GAP) + scale.px(BAR_H)
}
/// Widget width at `scale`/`font_px` (scales from the base so larger fonts fit).
pub fn win_w(scale: Scale, font_px: i32) -> i32 {
    scale.px(WIN_W_BASE * font_px / FONT_PX_BASE)
}
/// Height of the header band (title line + hairline separator + gap).
pub fn header_h(scale: Scale, font_px: i32) -> i32 {
    label_h(scale, font_px) + scale.px(2) + 1 + scale.px(ROW_GAP)
}
/// Window height for `n_rows` metric rows plus the header at `scale`/`font_px`.
pub fn window_height(scale: Scale, n_rows: i32, font_px: i32) -> i32 {
    2 * scale.px(PAD)
        + header_h(scale, font_px)
        + n_rows * row_block(scale, font_px)
        + (n_rows - 1).max(0) * scale.px(ROW_GAP)
}

/// Render `rows` into a `w`×`h` device-pixel surface using `font`/`scale`/`theme`.
/// Pure view: no `AppState`, no sampling — the widget and the settings preview
/// both call this, so they stay pixel-for-pixel consistent.
#[allow(clippy::too_many_arguments)]
pub unsafe fn render_rows(
    hdc: HDC,
    w: i32,
    h: i32,
    rows: &[Row],
    font: HFONT,
    scale: Scale,
    font_px: i32,
    theme: &RenderTheme,
    history: &[Vec<f32>],
) {
    // Clear the whole frame to the background using the stock DC brush.
    let full = RECT { left: 0, top: 0, right: w, bottom: h };
    SetDCBrushColor(hdc, COLORREF(theme.bg));
    FillRect(hdc, &full, HBRUSH(GetStockObject(DC_BRUSH).0));

    let old_font = SelectObject(hdc, HGDIOBJ(font.0));
    SetBkMode(hdc, TRANSPARENT);

    let pad = scale.px(PAD);
    let content_right = w - pad;
    let lh = label_h(scale, font_px);

    // Header: a dim title + a hairline separator. Pure hierarchy / drag affordance
    // (the whole window already drags via HTCAPTION).
    SetTextColor(hdc, COLORREF(theme.header_fg));
    let mut hdr = RECT { left: pad, top: pad, right: content_right, bottom: pad + lh };
    DrawTextW(hdc, &mut wide("TinyMon"), &mut hdr, DT_LEFT | DT_SINGLELINE | DT_VCENTER);
    let sep_y = pad + lh + scale.px(2);
    paint_rect(
        hdc,
        &RECT { left: pad, top: sep_y, right: content_right, bottom: sep_y + 1 },
        COLORREF(theme.track),
    );

    // Uniform value column = the widest value, so numbers don't jump row-to-row.
    let col_gap = scale.px(COL_GAP);
    let value_col_w = rows
        .iter()
        .map(|r| text_width(hdc, &wide(&r.value)))
        .max()
        .unwrap_or(0)
        .min(content_right - pad - col_gap);

    let rows_top = sep_y + 1 + scale.px(ROW_GAP);
    let rb = row_block(scale, font_px);
    for (i, row) in rows.iter().enumerate() {
        let y = rows_top + i as i32 * (rb + scale.px(ROW_GAP));
        let hist: &[f32] = history.get(i).map(|v| v.as_slice()).unwrap_or(&[]);
        draw_row(hdc, w, y, scale, lh, row, theme, value_col_w, hist);
    }

    SelectObject(hdc, old_font); // restore so our font isn't selected at teardown
}

/// Load fraction at which a load row's label turns red.
const ALERT_AT: f32 = 0.9;

/// Whether `row`'s label is painted red: only rows that opt into the alert
/// (load rows, built with `Row::bar`) at or above `ALERT_AT`. Throughput rows
/// (`Row::rate`), battery and value-only rows never alert.
pub fn is_critical(row: &Row) -> bool {
    row.alert && row.bar.is_some_and(|f| f >= ALERT_AT)
}

/// Draw one row: accent-colored label (left, ellipsized) + value (right) sharing
/// a uniform value column, then an optional load-colored bar.
#[allow(clippy::too_many_arguments)]
unsafe fn draw_row(
    hdc: HDC,
    width: i32,
    y: i32,
    scale: Scale,
    label_h: i32,
    row: &Row,
    theme: &RenderTheme,
    value_col_w: i32,
    hist: &[f32],
) {
    let pad = scale.px(PAD);
    let content_right = width - pad;
    let value_left = (content_right - value_col_w).max(pad);
    let label_right = (value_left - scale.px(COL_GAP)).max(pad);
    let mut label_rc = RECT { left: pad, top: y, right: label_right, bottom: y + label_h };
    let mut value_rc = RECT { left: value_left, top: y, right: content_right, bottom: y + label_h };

    // Threshold alert: a load row at or above ALERT_AT turns its label red (the
    // bar / sparkline already reddens via load color); see `is_critical`.
    let critical = is_critical(row);
    // The accent palette is tuned for the dark bg; on a light theme darken the
    // label accents so they stay readable (the value text already uses fg).
    let light_bg = (theme.bg >> 8) & 0xFF > 0x80;
    let dim = |c: u8| (c as u32 * 3 / 5) as u8;
    let label_color = if critical {
        colorref(RED.0, RED.1, RED.2)
    } else {
        match row.color {
            Some((r, g, b)) if light_bg => colorref(dim(r), dim(g), dim(b)),
            Some((r, g, b)) => colorref(r, g, b),
            None => COLORREF(theme.fg),
        }
    };
    SetTextColor(hdc, label_color);
    DrawTextW(
        hdc,
        &mut wide(&row.label),
        &mut label_rc,
        DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
    );
    SetTextColor(hdc, COLORREF(theme.fg));
    DrawTextW(hdc, &mut wide(&row.value), &mut value_rc, DT_RIGHT | DT_SINGLELINE | DT_VCENTER);

    if let Some(frac) = row.bar {
        let bar_h = scale.px(BAR_H);
        let bar_top = y + label_h + scale.px(BAR_GAP);
        let track = RECT { left: pad, top: bar_top, right: content_right, bottom: bar_top + bar_h };
        paint_rect(hdc, &track, COLORREF(theme.track));
        // 1px top edge, a subtle inset bevel: lighter than the track on the dark
        // theme, darker on the light one (the same direction as `emphasize`).
        paint_rect(hdc, &RECT { bottom: track.top + 1, ..track }, COLORREF(theme.track_edge));

        let f = frac.clamp(0.0, 1.0);
        // Load metrics with accumulated history render as a per-sample sparkline;
        // rows with a fixed bar color (e.g. battery) keep a solid level bar.
        if !hist.is_empty() && row.bar_color.is_none() {
            draw_sparkline(hdc, &track, hist, light_bg);
        } else {
            let fill_w = ((track.right - track.left) as f32 * f).round() as i32;
            if fill_w > 0 {
                let filled = RECT { right: track.left + fill_w, ..track };
                let (r, g, b) = row.bar_color.unwrap_or_else(|| load_color(f));
                paint_rect(hdc, &filled, colorref(r, g, b));
                // Soft top-band gradient: lit on dark, shaded on light (see `emphasize`).
                let hi_h = (bar_h * 45 / 100).max(1);
                let hi = RECT { bottom: filled.top + hi_h, ..filled };
                paint_rect(
                    hdc,
                    &hi,
                    colorref(
                        emphasize(r, light_bg),
                        emphasize(g, light_bg),
                        emphasize(b, light_bg),
                    ),
                );
            }
        }
    }
}

/// Draw a smooth area sparkline across `track`: a filled trend area under a
/// crisp top line (lighter on the dark theme, darker on the light one; see
/// `emphasize`), colored by the current (latest) value's load color
/// (green→red). Newest sample on the right — an at-a-glance trend in the bar's
/// space. The interpolated top reads cleanly even with few samples.
unsafe fn draw_sparkline(hdc: HDC, track: &RECT, hist: &[f32], light_bg: bool) {
    let n = hist.len();
    if n == 0 {
        return;
    }
    let tw = track.right - track.left;
    let th = track.bottom - track.top;
    let (r, g, b) = load_color(hist[n - 1].clamp(0.0, 1.0));
    let x_at = |i: usize| -> i32 {
        if n == 1 {
            track.right
        } else {
            track.left + tw * i as i32 / (n as i32 - 1)
        }
    };
    let y_at = |v: f32| -> i32 { track.bottom - (th as f32 * v.clamp(0.0, 1.0)).round() as i32 };

    // Filled area: the sample points, closed down to the track baseline.
    let mut area: Vec<POINT> = (0..n).map(|i| POINT { x: x_at(i), y: y_at(hist[i]) }).collect();
    area.push(POINT { x: x_at(n - 1), y: track.bottom });
    area.push(POINT { x: x_at(0), y: track.bottom });
    SetDCBrushColor(hdc, colorref(r, g, b));
    let prev_brush = SelectObject(hdc, GetStockObject(DC_BRUSH));
    let prev_pen = SelectObject(hdc, GetStockObject(NULL_PEN));
    let _ = Polygon(hdc, &area);
    SelectObject(hdc, prev_brush);
    SelectObject(hdc, prev_pen);

    // Crisp top line, emphasized for the theme (skipped for a single sample).
    if n >= 2 {
        let line: Vec<POINT> = (0..n).map(|i| POINT { x: x_at(i), y: y_at(hist[i]) }).collect();
        let pen = CreatePen(
            PS_SOLID,
            1,
            colorref(emphasize(r, light_bg), emphasize(g, light_bg), emphasize(b, light_bg)),
        );
        let prev = SelectObject(hdc, HGDIOBJ(pen.0));
        let _ = Polyline(hdc, &line);
        SelectObject(hdc, prev);
        let _ = DeleteObject(HGDIOBJ(pen.0));
    }
}

/// Fill a rectangle with a solid color via the stock DC brush (no allocation).
unsafe fn paint_rect(hdc: HDC, rc: &RECT, color: COLORREF) {
    SetDCBrushColor(hdc, color);
    FillRect(hdc, rc, HBRUSH(GetStockObject(DC_BRUSH).0));
}

/// Pixel width of `s` in the font currently selected into `hdc`.
unsafe fn text_width(hdc: HDC, s: &[u16]) -> i32 {
    let mut sz = SIZE::default();
    let _ = GetTextExtentPoint32W(hdc, s, &mut sz);
    sz.cx
}

/// Pack (R, G, B) into a COLORREF (0x00BBGGRR).
fn colorref(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}

/// UTF-16 encode a string for the wide Win32 text APIs.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

#[cfg(test)]
mod tests {
    use super::{emphasize, header_h, is_critical, label_h, row_block, win_w, window_height};
    use crate::battery::{battery_row, BatteryInfo};
    use crate::disk::disk_row;
    use crate::dpi::Scale;
    use crate::gpu::{gpu_row, vram_row};
    use crate::metrics::MemInfo;
    use crate::net::net_row;
    use crate::percpu::percpu_row;
    use crate::registry::{cpu_row, ram_row, uptime_row};

    #[test]
    fn geometry_identity_at_96() {
        let s = Scale(96);
        assert_eq!(label_h(s, 14), 18);
        assert_eq!(row_block(s, 14), 29);
        assert_eq!(win_w(s, 13), 200); // base width at the base font
        assert_eq!(win_w(s, 14), 215);
        assert_eq!(header_h(s, 13), 28);
        assert_eq!(header_h(s, 14), 29);
        assert_eq!(window_height(s, 3, 13), 144); // 116 rows + 28 header
        assert_eq!(window_height(s, 4, 14), 185); // 156 rows + 29 header
    }

    #[test]
    fn geometry_scales_with_dpi() {
        // 150%
        let s = Scale(144);
        assert_eq!(label_h(s, 14), 27);
        assert_eq!(row_block(s, 14), 44);
        assert_eq!(win_w(s, 14), 323);
        assert_eq!(header_h(s, 14), 43);
        assert_eq!(window_height(s, 4, 14), 279);
        // 200%
        assert_eq!(win_w(Scale(192), 14), 430);
        assert_eq!(window_height(Scale(192), 4, 14), 369);
    }

    #[test]
    fn emphasize_dark_matches_old_lighten() {
        // The dark theme keeps the old lighten(): 30% of the way toward white.
        for c in 0..=255u8 {
            let old = (c as u32 + (255 - c as u32) * 3 / 10) as u8;
            assert_eq!(emphasize(c, false), old, "c = {c}");
        }
    }

    #[test]
    fn emphasize_light_darkens_by_30_percent() {
        // The light theme moves 30% of the way toward black instead.
        for c in 0..=255u8 {
            assert_eq!(emphasize(c, true), (c as u32 * 7 / 10) as u8, "c = {c}");
        }
        assert_eq!(emphasize(200, true), 140);
        assert_eq!(emphasize(255, true), 178);
        assert_eq!(emphasize(0, true), 0);
    }

    #[test]
    fn throughput_rows_never_alert() {
        // Both bars are full (1.0) with no bar_color: the old predicate said true.
        let disk = disk_row("Disk R", Some(500.0 * 1024.0 * 1024.0));
        let net = net_row("Net\u{2193}", Some(50e6));
        for row in [&disk, &net] {
            // The premise: without a full bar this test would prove nothing.
            assert_eq!((row.bar, row.bar_color), (Some(1.0), None), "{}", row.label);
            assert!(!is_critical(row));
        }
    }

    #[test]
    fn load_rows_alert_from_ninety_percent() {
        const MIB: u64 = 1024 * 1024;
        assert!(is_critical(&cpu_row(90.0)));
        let ram = MemInfo { load_pct: 95.0, used_gib: 15.2, total_gib: 16.0 };
        assert!(is_critical(&ram_row(&ram)));
        assert!(is_critical(&gpu_row("GPU", Some(97.0))));
        assert!(is_critical(&vram_row("VRAM", Some(95 * MIB), 100 * MIB)));
        assert!(is_critical(&percpu_row(0, 99.0)));
        #[cfg(feature = "temp")]
        assert!(is_critical(&crate::temp::temp_row("CPU", 92.0)));
        assert!(!is_critical(&cpu_row(89.0)));
    }

    #[test]
    fn battery_and_value_rows_never_alert() {
        let full = BatteryInfo {
            present: true,
            ac_online: true,
            charging: false,
            percent: 100,
            secs_left: None,
        };
        assert!(!is_critical(&battery_row(&full)));
        assert!(!is_critical(&uptime_row(3_600_000)));
    }
}
