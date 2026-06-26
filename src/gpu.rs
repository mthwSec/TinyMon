//! GPU detection — vendor-neutral.
//!
//! Enumerate every non-software adapter via DXGI (discrete or integrated, any
//! vendor) and report VRAM used/total. GPU load comes from the vendor-neutral
//! PDH "GPU Engine" performance counters (the same source Task Manager uses),
//! so it works on Intel/AMD/NVIDIA alike — no vendor SDK.

use crate::registry::Row;

use crate::pdh::PdhCounter;

use windows::core::{w, Interface};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter3, IDXGIFactory4, DXGI_ADAPTER_FLAG_SOFTWARE,
    DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO,
};

/// One detected display adapter.
pub struct GpuAdapter {
    pub name: String,
    pub vram_total: u64,
    pub vram_used: u64,
    /// GPU utilization 0..=100, or `None` if unavailable.
    pub load: Option<f32>,
    /// Lowercased "luid_0x..._0x..." token, used to match this adapter to PDH
    /// "GPU Engine" counter instances on multi-GPU systems.
    luid_key: String,
    /// Held for live VRAM polling each tick.
    adapter: Option<IDXGIAdapter3>,
}

impl GpuAdapter {
    /// Re-query live VRAM usage (cheap; called once per timer tick).
    pub fn refresh(&mut self) {
        let Some(a) = &self.adapter else { return };
        let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        // SAFETY: COM call on a live interface; `info` is a stack out-param.
        if unsafe { a.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info) }.is_ok() {
            self.vram_used = info.CurrentUsage;
        }
    }
}

/// Enumerate all non-software GPUs with their VRAM usage/total.
pub fn detect_gpus() -> Vec<GpuAdapter> {
    let mut gpus = Vec::new();
    // SAFETY: DXGI COM calls. Every interface comes from a checked Result before
    // use, and enumeration stops at the first error (DXGI_ERROR_NOT_FOUND).
    unsafe {
        let factory: IDXGIFactory4 = match CreateDXGIFactory1() {
            Ok(f) => f,
            Err(_) => return gpus,
        };
        let mut index = 0u32;
        while let Ok(adapter) = factory.EnumAdapters1(index) {
            index += 1;
            let Ok(desc) = adapter.GetDesc1() else { continue };
            // Skip software adapters (e.g. "Microsoft Basic Render Driver").
            if desc.Flags & DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32 != 0 {
                continue;
            }

            let adapter3 = adapter.cast::<IDXGIAdapter3>().ok();
            let (mut used, mut budget) = (0u64, 0u64);
            if let Some(a) = &adapter3 {
                let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
                if a.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info)
                    .is_ok()
                {
                    used = info.CurrentUsage;
                    budget = info.Budget;
                }
            }
            // Physical dedicated VRAM is the natural "total"; integrated GPUs
            // often report 0 there, so fall back to the DXGI budget.
            let total = if desc.DedicatedVideoMemory > 0 {
                desc.DedicatedVideoMemory as u64
            } else {
                budget
            };
            let luid = desc.AdapterLuid;
            let luid_key = format!("luid_0x{:08x}_0x{:08x}", luid.HighPart as u32, luid.LowPart);

            gpus.push(GpuAdapter {
                name: short_name(&utf16_to_string(&desc.Description)),
                vram_total: total,
                vram_used: used,
                load: None,
                luid_key,
                adapter: adapter3,
            });
        }
    }
    gpus
}

/// Vendor-neutral GPU utilization sampler backed by PDH "GPU Engine" counters.
pub struct GpuLoadSampler {
    counter: PdhCounter,
}

impl GpuLoadSampler {
    pub fn new() -> Self {
        GpuLoadSampler { counter: PdhCounter::new(w!("\\GPU Engine(*)\\Utilization Percentage")) }
    }

    /// Assign each adapter a load %: single-GPU systems take the max across all
    /// engine instances; multi-GPU systems match instances to adapters by LUID.
    pub fn update(&self, gpus: &mut [GpuAdapter]) {
        if !self.counter.ok() {
            return;
        }
        let samples = self.counter.sample();
        let single = gpus.len() == 1;
        for g in gpus.iter_mut() {
            let max = samples
                .iter()
                .filter(|(name, v)| (single || name.contains(&g.luid_key)) && (0.0..=100.5).contains(v))
                .map(|(_, v)| *v)
                .fold(0.0f64, f64::max);
            g.load = Some(max.clamp(0.0, 100.0) as f32);
        }
    }

    /// Diagnostic accessor (used by the dump test only).
    #[cfg(test)]
    pub fn sample_instances(&self) -> Vec<(String, f64)> {
        self.counter.sample()
    }
}

/// Convert a fixed Win32 UTF-16 buffer (NUL-terminated) to a `String`.
fn utf16_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// Shorten a verbose adapter description toward a compact label, e.g.
/// "AMD Radeon RX 9070 XT" -> "RX 9070 XT", "Intel(R) UHD Graphics 770" ->
/// "Intel UHD 770". Falls back to the trimmed original if cleanup empties it.
pub fn short_name(desc: &str) -> String {
    let mut s = desc.to_string();
    for junk in ["(R)", "(TM)", "(C)", "®", "™"] {
        s = s.replace(junk, "");
    }
    // Drop leading vendor/brand words (the spec keeps "Intel", drops the rest).
    for prefix in ["AMD Radeon ", "AMD ", "Radeon ", "NVIDIA ", "GeForce "] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.to_string();
        }
    }
    // "Graphics" is noise in names like "Intel UHD Graphics 770".
    s = s.replace(" Graphics ", " ").replace(" Graphics", "");
    let cleaned = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        desc.trim().to_string()
    } else {
        cleaned
    }
}

/// All detected GPUs plus their shared load sampler — one toggleable "group".
pub struct GpuGroup {
    gpus: Vec<GpuAdapter>,
    load: GpuLoadSampler,
}

impl GpuGroup {
    pub fn new() -> Self {
        GpuGroup { gpus: detect_gpus(), load: GpuLoadSampler::new() }
    }
    /// Refresh GPU load for every adapter (once per tick).
    pub fn refresh(&mut self) {
        self.load.update(&mut self.gpus);
    }
    /// Append one utilization row per adapter.
    pub fn rows(&self, out: &mut Vec<Row>) {
        for g in &self.gpus {
            out.push(gpu_row(&g.name, g.load));
        }
    }
}

/// VRAM (video memory) source — a separate toggleable metric from GPU load, so
/// it can be enabled and ordered independently. Reuses the DXGI adapter
/// detection and polls live usage each tick.
pub struct VramGroup {
    gpus: Vec<GpuAdapter>,
}

impl VramGroup {
    pub fn new() -> Self {
        VramGroup { gpus: detect_gpus() }
    }
    pub fn refresh(&mut self) {
        for g in self.gpus.iter_mut() {
            g.refresh();
        }
    }
    pub fn rows(&self, out: &mut Vec<Row>) {
        // One "VRAM" row per adapter; qualify by name only when there are several.
        let multi = self.gpus.len() > 1;
        for g in &self.gpus {
            let label = if multi { format!("{} VRAM", g.name) } else { "VRAM".to_string() };
            out.push(vram_row(&label, g.vram_used, g.vram_total));
        }
    }
}

/// GPU utilization row (name + load %). Pure; unit-tested.
pub fn gpu_row(name: &str, load: Option<f32>) -> Row {
    let value = match load {
        Some(l) => format!("{:.0}%", l),
        None => "n/a".to_string(),
    };
    Row::bar(name, value, load.map(|l| l / 100.0).unwrap_or(0.0))
}

/// GPU video-memory row (used/total + a memory-pressure bar that reddens as VRAM
/// fills). Pure; unit-tested.
pub fn vram_row(name: &str, used: u64, total: u64) -> Row {
    let frac = if total > 0 { (used as f32 / total as f32).clamp(0.0, 1.0) } else { 0.0 };
    Row::bar(name, fmt_vram(used, total), frac)
}

/// Format VRAM used/total with a unit that suits the total (GiB or MiB).
fn fmt_vram(used: u64, total: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: f64 = 1024.0 * 1024.0;
    if total >= GIB {
        format!("{:.1}/{:.0}G", used as f64 / GIB as f64, total as f64 / GIB as f64)
    } else if total > 0 {
        format!("{:.0}/{:.0}M", used as f64 / MIB, total as f64 / MIB)
    } else {
        "n/a".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_row_load() {
        let r = gpu_row("RX", Some(55.0));
        assert_eq!(r.label, "RX");
        assert_eq!(r.value, "55%");
        assert!((r.bar.unwrap() - 0.55).abs() < 1e-6);
    }

    #[test]
    fn gpu_row_no_load() {
        let r = gpu_row("Intel", None);
        assert_eq!(r.value, "n/a");
        assert_eq!(r.bar, Some(0.0));
    }

    #[test]
    fn vram_row_formats() {
        let r = vram_row("VRAM", 2 * 1024 * 1024 * 1024, 16 * 1024 * 1024 * 1024);
        assert_eq!(r.label, "VRAM");
        assert_eq!(r.value, "2.0/16G");
        assert!((r.bar.unwrap() - 0.125).abs() < 1e-6);
        assert_eq!(vram_row("VRAM", 0, 128 * 1024 * 1024).value, "0/128M"); // integrated, MiB
    }

    #[test]
    fn amd_discrete() {
        assert_eq!(short_name("AMD Radeon RX 9070 XT"), "RX 9070 XT");
    }

    #[test]
    fn intel_integrated() {
        assert_eq!(short_name("Intel(R) UHD Graphics 770"), "Intel UHD 770");
    }

    #[test]
    fn nvidia_discrete() {
        assert_eq!(short_name("NVIDIA GeForce RTX 4080"), "RTX 4080");
    }

    #[test]
    fn unknown_passthrough() {
        assert_eq!(short_name("Some Custom GPU"), "Some Custom GPU");
    }

    #[test]
    fn bare_vendor_word_kept() {
        assert_eq!(short_name("AMD"), "AMD");
    }

    /// Diagnostic (no assertions): `cargo test -- --nocapture` to print the
    /// adapters detected on this machine.
    #[test]
    fn dump_detected_gpus() {
        for g in detect_gpus() {
            println!(
                "GPU: {:<16} VRAM {} / {} MiB  load={:?}  key={}",
                g.name,
                g.vram_used / (1024 * 1024),
                g.vram_total / (1024 * 1024),
                g.load,
                g.luid_key
            );
        }
    }

    /// Diagnostic: print live PDH "GPU Engine" instances + utilization. Run with
    /// `cargo test -- --nocapture`. Sleeps ~1s so the rate counter has a delta.
    #[test]
    fn dump_gpu_load() {
        let s = GpuLoadSampler::new();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let samples = s.sample_instances();
        println!("--- {} GPU Engine instances (nonzero shown) ---", samples.len());
        let mut max = 0.0f64;
        for (n, v) in &samples {
            if *v > 0.0 {
                println!("  {} = {:.1}%", n, v);
            }
            if *v > max {
                max = *v;
            }
        }
        println!("overall max GPU utilization = {:.1}%", max);
    }
}
