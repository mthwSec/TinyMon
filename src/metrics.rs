//! Pure metric math plus thin Win32 samplers.
//!
//! The arithmetic (CPU% from time deltas, bytes -> GiB, load -> color) is pure
//! and unit-tested below; the samplers only wrap an OS counter read around it.

use core::mem::size_of;

use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;

/// Color stops for the load bars, as (R, G, B).
pub const GREEN: (u8, u8, u8) = (0x33, 0xCC, 0x44);
pub const YELLOW: (u8, u8, u8) = (0xE0, 0xC0, 0x30);
pub const RED: (u8, u8, u8) = (0xE0, 0x40, 0x40);

/// Combine a FILETIME's two 32-bit halves into a 64-bit 100ns tick count.
fn filetime_ticks(ft: FILETIME) -> u64 {
    ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64)
}

/// CPU busy percentage (0..=100) from the time deltas between two
/// `GetSystemTimes` polls. On Windows the kernel time already includes idle
/// time, so total = kernel + user and the busy portion is total - idle.
pub fn cpu_percent(idle_delta: u64, kernel_delta: u64, user_delta: u64) -> f32 {
    let total = kernel_delta + user_delta;
    if total == 0 {
        return 0.0;
    }
    let busy = total.saturating_sub(idle_delta);
    (busy as f32 / total as f32 * 100.0).clamp(0.0, 100.0)
}

/// Bytes -> gibibytes.
pub fn bytes_to_gib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}

/// Linear-interpolate a single color channel.
fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round() as u8
}

/// Map a 0..=1 load fraction to an (R, G, B) color: green -> yellow -> red.
pub fn load_color(load: f32) -> (u8, u8, u8) {
    let l = load.clamp(0.0, 1.0);
    let (from, to, t) = if l < 0.5 {
        (GREEN, YELLOW, l / 0.5)
    } else {
        (YELLOW, RED, (l - 0.5) / 0.5)
    };
    (
        lerp(from.0, to.0, t),
        lerp(from.1, to.1, t),
        lerp(from.2, to.2, t),
    )
}

/// Rolling CPU sampler: remembers the previous poll so each new poll yields the
/// usage percentage over the interval between them.
pub struct CpuSampler {
    prev: Option<(u64, u64, u64)>,
}

impl CpuSampler {
    pub fn new() -> Self {
        Self { prev: None }
    }

    /// Poll the system times and return CPU usage since the previous poll. The
    /// first call only establishes a baseline and returns 0.
    pub fn sample(&mut self) -> f32 {
        let mut idle = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: GetSystemTimes writes three FILETIME out-params we own and
        // have initialized; the pointers are non-null and non-aliasing.
        let ok = unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) };
        if ok.is_err() {
            return 0.0;
        }
        let cur = (
            filetime_ticks(idle),
            filetime_ticks(kernel),
            filetime_ticks(user),
        );
        let pct = match self.prev {
            Some(p) => cpu_percent(
                cur.0.wrapping_sub(p.0),
                cur.1.wrapping_sub(p.1),
                cur.2.wrapping_sub(p.2),
            ),
            None => 0.0,
        };
        self.prev = Some(cur);
        pct
    }
}

/// Snapshot of physical memory usage.
#[derive(Clone, Copy, Default)]
pub struct MemInfo {
    pub load_pct: f32,
    pub used_gib: f64,
    pub total_gib: f64,
}

/// Read current physical memory usage via `GlobalMemoryStatusEx`.
pub fn read_memory() -> MemInfo {
    let mut ms = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    // SAFETY: GlobalMemoryStatusEx fills the buffer we own; dwLength tells it
    // the struct size so it knows the layout/version.
    if unsafe { GlobalMemoryStatusEx(&mut ms) }.is_err() {
        return MemInfo::default();
    }
    let used = ms.ullTotalPhys.saturating_sub(ms.ullAvailPhys);
    MemInfo {
        load_pct: ms.dwMemoryLoad as f32,
        used_gib: bytes_to_gib(used),
        total_gib: bytes_to_gib(ms.ullTotalPhys),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_all_idle_is_zero() {
        // total = 100 kernel + 0 user; idle = 100 -> 0% busy
        assert_eq!(cpu_percent(100, 100, 0), 0.0);
    }

    #[test]
    fn cpu_no_idle_is_full() {
        // total = 100; idle = 0 -> 100% busy
        assert_eq!(cpu_percent(0, 80, 20), 100.0);
    }

    #[test]
    fn cpu_half_load() {
        // total = 100; idle = 50 -> 50% busy
        assert!((cpu_percent(50, 80, 20) - 50.0).abs() < 1e-3);
    }

    #[test]
    fn cpu_zero_interval_is_safe() {
        assert_eq!(cpu_percent(0, 0, 0), 0.0);
    }

    #[test]
    fn gib_conversion() {
        assert!((bytes_to_gib(1024 * 1024 * 1024) - 1.0).abs() < 1e-9);
        assert_eq!(bytes_to_gib(0), 0.0);
    }

    #[test]
    fn color_hits_endpoints() {
        assert_eq!(load_color(0.0), GREEN);
        assert_eq!(load_color(1.0), RED);
    }

    #[test]
    fn color_midpoint_is_yellow() {
        assert_eq!(load_color(0.5), YELLOW);
    }

    #[test]
    fn color_red_channel_is_monotonic() {
        let r_lo = load_color(0.0).0;
        let r_mid = load_color(0.5).0;
        let r_hi = load_color(1.0).0;
        assert!(r_lo <= r_mid && r_mid <= r_hi);
    }

    #[test]
    fn color_clamps_out_of_range() {
        assert_eq!(load_color(-1.0), GREEN);
        assert_eq!(load_color(2.0), RED);
    }
}
