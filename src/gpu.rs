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
        if unsafe { a.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info) }.is_ok()
        {
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
                if a.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info).is_ok() {
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

            gpus.push(GpuAdapter {
                name: short_name(&utf16_to_string(&desc.Description)),
                vram_total: total,
                vram_used: used,
                load: None,
                luid_key: luid_key(luid.HighPart, luid.LowPart),
                adapter: adapter3,
            });
        }
    }
    gpus
}

/// The "luid_0x<high>_0x<low>" token (lowercase hex, HighPart printed as
/// unsigned) that PDH puts in the instance names of an adapter's counters,
/// e.g. "pid_10680_luid_0x00000000_0x000109f8_phys_0_eng_0_engtype_3d".
fn luid_key(high: i32, low: u32) -> String {
    format!("luid_0x{:08x}_0x{:08x}", high as u32, low)
}

/// Vendor-neutral GPU utilization sampler backed by PDH "GPU Engine" counters.
pub struct GpuLoadSampler {
    counter: PdhCounter,
}

impl GpuLoadSampler {
    pub fn new() -> Self {
        GpuLoadSampler { counter: PdhCounter::new(w!("\\GPU Engine(*)\\Utilization Percentage")) }
    }

    /// Assign each adapter its load % (`adapter_load`): Task Manager's busiest
    /// engine, summed across processes. A counter that cannot be read, or an
    /// empty sample, leaves every load unknown (n/a), not 0%: a live desktop
    /// always has engine instances, so empty means PDH_NO_DATA (for example a
    /// restricted token), not an idle GPU.
    pub fn update(&self, gpus: &mut [GpuAdapter]) {
        let Some(samples) = self.counter.try_sample().filter(|s| !s.is_empty()) else {
            for g in gpus.iter_mut() {
                g.load = None;
            }
            return;
        };
        let single = gpus.len() == 1;
        for g in gpus.iter_mut() {
            g.load = Some(adapter_load(&samples, &g.luid_key, single) as f32);
        }
    }

    /// Diagnostic accessor (used by the dump test only).
    #[cfg(test)]
    pub fn sample_instances(&self) -> Vec<(String, f64)> {
        self.counter.sample()
    }
}

/// The engine a "GPU Engine" instance belongs to: its name without the leading
/// "pid_<n>_" and the trailing "_engtype_<type>", e.g.
/// "pid_10680_luid_0x00000000_0x000109f8_phys_0_eng_0_engtype_3d" ->
/// "luid_0x00000000_0x000109f8_phys_0_eng_0". Each process using an engine has
/// its own instance, so instances with the same key are one engine.
fn engine_key(name: &str) -> &str {
    let start = name.find("luid_").unwrap_or(0);
    let end = name.find("_engtype_").unwrap_or(name.len());
    name.get(start..end).unwrap_or(name)
}

/// Task Manager's GPU %: the busiest engine, where an engine's load is the sum
/// over the processes using it. With `luid_key`, only that adapter's instances
/// count. Values outside 0..=100.5 (counter glitches) are skipped and each
/// engine is capped at 100. `None` when no instance matched. About 30 engines,
/// so a small Vec does the grouping (no HashMap).
fn busiest_engine(samples: &[(String, f64)], luid_key: Option<&str>) -> Option<f64> {
    let mut engines: Vec<(&str, f64)> = Vec::new();
    for (name, v) in samples {
        if !(0.0..=100.5).contains(v) || luid_key.is_some_and(|k| !name.contains(k)) {
            continue;
        }
        let key = engine_key(name);
        match engines.iter_mut().find(|(k, _)| *k == key) {
            Some((_, sum)) => *sum += v,
            None => engines.push((key, *v)),
        }
    }
    engines.iter().map(|(_, sum)| sum.min(100.0)).reduce(f64::max)
}

/// One adapter's load %: its own engines, matched strictly by LUID, so the Basic
/// Render Driver's load stays out of a hardware adapter's row. Only a lone
/// adapter that matched nothing (its LUID changed after a driver update) falls
/// back to every engine. No match is 0%: an adapter nobody uses has no instances.
fn adapter_load(samples: &[(String, f64)], luid_key: &str, single: bool) -> f64 {
    let mine = busiest_engine(samples, Some(luid_key));
    let load = if single { mine.or_else(|| busiest_engine(samples, None)) } else { mine };
    load.unwrap_or(0.0)
}

/// Convert a fixed Win32 UTF-16 buffer (NUL-terminated) to a `String`.
fn utf16_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// Shorten a verbose adapter description toward a compact label, e.g.
/// "AMD Radeon RX 9070 XT" -> "RX 9070 XT", "Intel(R) UHD Graphics 770" ->
/// "Intel UHD 770", "AMD Radeon(TM) Graphics" -> "Radeon". Falls back to the
/// trimmed original if cleanup empties it.
pub fn short_name(desc: &str) -> String {
    let mut s = desc.to_string();
    for junk in ["(R)", "(TM)", "(C)", "®", "™"] {
        s = s.replace(junk, "");
    }
    // Normalise whitespace first: "Radeon (TM) Graphics" strips like "Radeon(TM) Graphics".
    s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    // Drop the leading company word, then the brand word (the spec keeps "Intel").
    for company in ["AMD ", "NVIDIA "] {
        if let Some(rest) = s.strip_prefix(company) {
            s = rest.to_string();
        }
    }
    let branded = s.clone();
    for brand in ["Radeon ", "GeForce "] {
        if let Some(rest) = s.strip_prefix(brand) {
            s = rest.to_string();
        }
    }
    // "Graphics" is noise in names like "Intel UHD Graphics 770".
    s = s.replace(" Graphics ", " ").replace(" Graphics", "");
    // A Ryzen iGPU ("AMD Radeon(TM) Graphics") names no model: keep the brand.
    if s == "Graphics" {
        s = branded.replace(" Graphics", "");
    }
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
        load_rows(&self.gpus, out);
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
        vram_rows(&self.gpus, out);
    }
}

/// One load row per adapter, or a single "GPU n/a" row when DXGI found no
/// hardware adapter (RDP, or a VM with only the Basic Render Driver).
fn load_rows(gpus: &[GpuAdapter], out: &mut Vec<Row>) {
    if gpus.is_empty() {
        out.push(Row::na("GPU"));
    }
    for g in gpus {
        out.push(gpu_row(&g.name, g.load));
    }
}

/// One "VRAM" row per adapter, qualified by name only when there are several,
/// or a single "VRAM n/a" row when DXGI found no hardware adapter.
fn vram_rows(gpus: &[GpuAdapter], out: &mut Vec<Row>) {
    if gpus.is_empty() {
        out.push(Row::na("VRAM"));
    }
    let multi = gpus.len() > 1;
    for g in gpus {
        let label = if multi { format!("{} VRAM", g.name) } else { "VRAM".to_string() };
        out.push(vram_row(&label, g.vram_used, g.vram_total));
    }
}

/// GPU utilization row (name + load %), or a value-only "n/a" row when the load
/// is unknown. Pure; unit-tested.
pub fn gpu_row(name: &str, load: Option<f32>) -> Row {
    match load {
        Some(l) => Row::bar(name, format!("{:.0}%", l), l / 100.0),
        None => Row::na(name),
    }
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

    /// An adapter with the iGPU's LUID (low part 0x109f8) and the given load.
    fn adapter(load: Option<f32>) -> GpuAdapter {
        GpuAdapter {
            name: "GPU".into(),
            vram_total: 0,
            vram_used: 0,
            load,
            luid_key: luid_key(0, 0x109f8),
            adapter: None,
        }
    }

    /// One "GPU Engine" instance as PdhCounter returns it (lowercased): process
    /// `pid` on engine `eng` (type `ty`) of the adapter whose LUID low part is
    /// `luid_low`, reading `v` percent.
    fn eng(pid: u32, luid_low: u32, eng: u32, ty: &str, v: f64) -> (String, f64) {
        (format!("pid_{pid}_{}_phys_0_eng_{eng}_engtype_{ty}", luid_key(0, luid_low)), v)
    }

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
        assert_eq!(r.bar, None);
    }

    #[test]
    fn no_hardware_adapter_shows_na_rows() {
        let mut out = Vec::new();
        load_rows(&[], &mut out);
        vram_rows(&[], &mut out);
        let got: Vec<_> = out.iter().map(|r| (r.label.as_str(), r.value.as_str(), r.bar)).collect();
        assert_eq!(got, [("GPU", "n/a", None), ("VRAM", "n/a", None)]);
    }

    #[test]
    fn unreadable_load_counter_shows_na() {
        let mut gpus = [adapter(Some(40.0)), adapter(Some(60.0))];
        let sampler = GpuLoadSampler { counter: PdhCounter::new(w!("\\No Such Object(*)\\X")) };
        sampler.update(&mut gpus);
        assert!(gpus.iter().all(|g| g.load.is_none()));
    }

    #[test]
    fn empty_load_sample_shows_na() {
        // A single instance that does not exist: the add succeeds and every
        // collect reports PDH_NO_DATA, the same Some(empty) a restricted token gets.
        let counter = PdhCounter::new(w!("\\Processor(999)\\% Processor Time"));
        assert_eq!(counter.try_sample(), Some(Vec::new()));
        let mut gpus = [adapter(Some(40.0))];
        GpuLoadSampler { counter }.update(&mut gpus);
        assert_eq!(gpus[0].load, None);
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

    #[test]
    fn ryzen_igpu_keeps_brand() {
        // Ryzen APUs and Ryzen 7000+ iGPUs name no model after the brand.
        assert_eq!(short_name("AMD Radeon(TM) Graphics"), "Radeon");
        assert_eq!(short_name("AMD Radeon Graphics"), "Radeon");
    }

    #[test]
    fn apu_model_still_shortened() {
        assert_eq!(short_name("AMD Radeon(TM) Vega 8 Graphics"), "Vega 8");
        assert_eq!(short_name("AMD Radeon(TM) 780M Graphics"), "780M");
    }

    #[test]
    fn odd_spacing_is_normalised_before_stripping() {
        assert_eq!(short_name("AMD Radeon (TM) Graphics"), "Radeon");
        assert_eq!(short_name("NVIDIA  GeForce RTX 4080"), "RTX 4080");
    }

    #[test]
    fn luid_key_matches_pdh_instance_names() {
        assert_eq!(luid_key(0, 0x109f8), "luid_0x00000000_0x000109f8");
        // A negative HighPart prints as unsigned hex, the way PDH writes it.
        assert_eq!(luid_key(-1, 0xdeadbeef), "luid_0xffffffff_0xdeadbeef");
        // PdhCounter lowercases instance names, so the key is an exact prefix.
        let instance = "luid_0x00000000_0x000109F8_phys_0".to_lowercase();
        assert!(instance.starts_with(&luid_key(0, 0x109f8)));
    }

    #[test]
    fn engine_key_strips_pid_and_engtype() {
        assert_eq!(
            engine_key("pid_10680_luid_0x00000000_0x000109f8_phys_0_eng_0_engtype_3d"),
            "luid_0x00000000_0x000109f8_phys_0_eng_0"
        );
        // An empty engine type, and one with a space.
        assert_eq!(
            engine_key("pid_4_luid_0x00000000_0x000109f8_phys_0_eng_11_engtype_"),
            "luid_0x00000000_0x000109f8_phys_0_eng_11"
        );
        assert_eq!(
            engine_key("pid_968_luid_0x00000000_0x000109f8_phys_0_eng_5_engtype_gdi render"),
            "luid_0x00000000_0x000109f8_phys_0_eng_5"
        );
    }

    #[test]
    fn load_sums_processes_sharing_an_engine() {
        // Three processes on eng_0 (60 + 12 + 8) and one on eng_1 (30).
        let s = [
            eng(1, 0x109f8, 0, "3d", 60.0),
            eng(2, 0x109f8, 0, "3d", 12.0),
            eng(3, 0x109f8, 0, "3d", 8.0),
            eng(1, 0x109f8, 1, "copy", 30.0),
        ];
        assert_eq!(busiest_engine(&s, None), Some(80.0));
    }

    #[test]
    fn load_is_clamped_and_skips_garbage() {
        let busy = [eng(1, 0x109f8, 0, "3d", 70.0), eng(2, 0x109f8, 0, "3d", 40.0)];
        assert_eq!(busiest_engine(&busy, None), Some(100.0));
        let glitches = [
            eng(1, 0x109f8, 0, "3d", 1e9),
            eng(2, 0x109f8, 0, "3d", -5.0),
            eng(3, 0x109f8, 0, "3d", 20.0),
        ];
        assert_eq!(busiest_engine(&glitches, None), Some(20.0));
    }

    #[test]
    fn load_matches_adapter_by_luid() {
        let s = [eng(1, 0x109f8, 0, "3d", 10.0), eng(2, 0x10d83, 0, "3d", 90.0)];
        assert_eq!(busiest_engine(&s, Some(&luid_key(0, 0x109f8))), Some(10.0));
        assert_eq!(busiest_engine(&s, Some(&luid_key(0, 0x10d83))), Some(90.0));
        assert_eq!(busiest_engine(&s, Some(&luid_key(0, 0x1234))), None);
        assert_eq!(busiest_engine(&[], None), None);
    }

    #[test]
    fn lone_adapter_uses_its_own_engines_then_falls_back() {
        let igpu = luid_key(0, 0x109f8);
        // iGPU 3D at 20% while the Basic Render Driver (another LUID) is at 70%.
        let s = [eng(1, 0x109f8, 0, "3d", 20.0), eng(2, 0x10d83, 0, "3d", 70.0)];
        assert_eq!(adapter_load(&s, &igpu, true), 20.0);
        // Only another LUID reports (ours changed): a lone adapter reads it all.
        let other = [eng(1, 0x10d83, 0, "3d", 35.0)];
        assert_eq!(adapter_load(&other, &igpu, true), 35.0);
        assert_eq!(adapter_load(&other, &igpu, false), 0.0);
    }

    /// Diagnostic (no assertions), ignored by default: print the adapters
    /// detected on this machine with `cargo test -- --ignored --nocapture dump_`.
    #[test]
    #[ignore = "diagnostic: cargo test -- --ignored --nocapture dump_"]
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

    /// Diagnostic (no assertions), ignored by default: print live PDH "GPU Engine"
    /// instances + utilization with `cargo test -- --ignored --nocapture dump_`.
    /// Sleeps ~1s so the rate counter has a delta.
    #[test]
    #[ignore = "diagnostic: cargo test -- --ignored --nocapture dump_"]
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
        println!("largest single instance = {:.1}%", max);
        // The widget's figure (GpuLoadSampler::update), here over every adapter.
        let busiest = busiest_engine(&samples, None).unwrap_or(0.0);
        println!("busiest engine, summed across processes = {:.1}%", busiest);
    }
}
