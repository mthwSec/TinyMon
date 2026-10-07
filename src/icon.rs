//! A small TinyMon icon drawn at runtime with GDI — no `.ico` asset, no build
//! dependency, so the single self-contained `.exe` stays self-contained. The
//! glyph is a tiny bar-graph (echoing the widget) on a dark rounded square.

use core::ffi::c_void;
use core::mem::size_of;

use windows::core::BOOL;
use windows::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS,
    HGDIOBJ,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO};

/// Pack A,R,G,B into the `0xAARRGGBB` u32 a 32-bit top-down DIB expects (the
/// bytes land as B,G,R,A in memory).
fn argb(a: u8, r: u8, g: u8, b: u8) -> u32 {
    ((a as u32) << 24) | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

/// True if `(x, y)` falls outside the rounded corner of radius `r` (so it should
/// be transparent).
fn corner_outside(x: i32, y: i32, px: i32, r: i32) -> bool {
    let cx = if x < r {
        r
    } else if x >= px - r {
        px - 1 - r
    } else {
        return false;
    };
    let cy = if y < r {
        r
    } else if y >= px - r {
        px - 1 - r
    } else {
        return false;
    };
    let (dx, dy) = (x - cx, y - cy);
    dx * dx + dy * dy > r * r
}

/// Paint the glyph into a top-down BGRA buffer of `px`*`px` pixels. Pure (no
/// Win32) so it's unit-testable.
fn paint_icon(px: i32, buf: &mut [u32]) {
    let bg = argb(0xFF, 0x24, 0x24, 0x28);
    let radius = (px / 6).max(1);
    for y in 0..px {
        for x in 0..px {
            let p = if corner_outside(x, y, px, radius) { 0 } else { bg };
            buf[(y * px + x) as usize] = p;
        }
    }

    // Three ascending bars in the widget's accent hues (cyan / amber / green).
    let m = (px / 6).max(1);
    let usable = px - 2 * m;
    let bw = (usable / 5).max(1); // 3 bars + 2 equal gaps
    let baseline = px - m;
    let bars = [
        (m, usable * 35 / 100, argb(0xFF, 70, 200, 224)),
        (m + 2 * bw, usable * 62 / 100, argb(0xFF, 224, 168, 74)),
        (m + 4 * bw, usable * 90 / 100, argb(0xFF, 92, 200, 92)),
    ];
    for (bx, bh, color) in bars {
        let top = (baseline - bh).max(0);
        for y in top..baseline {
            for x in bx..(bx + bw).min(px) {
                buf[(y * px + x) as usize] = color;
            }
        }
    }
}

/// Build an `HICON` of `px`*`px`. Best-effort: returns a null icon on failure
/// (the window/tray then falls back to the system default).
pub unsafe fn create_app_icon(px: i32) -> HICON {
    let null = HICON(core::ptr::null_mut());
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: px,
            biHeight: -px, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0, // BI_RGB
            ..Default::default()
        },
        ..Default::default()
    };
    // SAFETY: CreateDIBSection gives us a `px*px` 32bpp pixel buffer to fill.
    let mut bits: *mut c_void = core::ptr::null_mut();
    let dib = match CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
        Ok(b) if !bits.is_null() => b,
        _ => return null,
    };
    let buf = core::slice::from_raw_parts_mut(bits as *mut u32, (px * px) as usize);
    paint_icon(px, buf);

    // All-zero AND mask: the 32bpp color bitmap's alpha decides transparency.
    let stride = (((px + 15) / 16) * 2) as usize; // mono scanlines are WORD-aligned
    let mask_bits = vec![0u8; stride * px as usize];
    let mask = CreateBitmap(px, px, 1, 1, Some(mask_bits.as_ptr() as *const c_void));

    let ii = ICONINFO { fIcon: BOOL(1), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: dib };
    let icon = CreateIconIndirect(&ii).unwrap_or(null);

    // The icon owns copies; release our bitmaps.
    let _ = DeleteObject(HGDIOBJ(dib.0));
    let _ = DeleteObject(HGDIOBJ(mask.0));
    icon
}

#[cfg(test)]
mod tests {
    use super::{argb, paint_icon};

    #[test]
    fn glyph_has_corners_bg_and_bars() {
        let px = 32;
        let mut buf = vec![0u32; (px * px) as usize];
        paint_icon(px, &mut buf);
        // top-left corner is transparent (rounded away)
        assert_eq!(buf[0] >> 24, 0);
        // a near-top center pixel is opaque background
        let top_center = ((px / 8) * px + px / 2) as usize;
        assert_eq!(buf[top_center] >> 24, 0xFF);
        // the tall green bar is present somewhere
        let green = argb(0xFF, 92, 200, 92);
        assert!(buf.contains(&green));
    }
}
