//! GPU detection — vendor-neutral.
//!
//! Enumerate every non-software adapter via DXGI (discrete or integrated, any
//! vendor), tell integrated (UMA) from discrete adapters and take their memory
//! totals. Load and memory usage come from the vendor-neutral PDH "GPU Engine"
//! and "GPU Adapter Memory" counters (the sources Task Manager uses), so it
//! works on Intel/AMD/NVIDIA alike, with no vendor SDK.

use crate::registry::Row;

use crate::pdh::PdhCounter;

use windows::core::{w, Interface};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIAdapter3, IDXGIFactory4, DXGI_ADAPTER_FLAG_SOFTWARE,
    DXGI_MEMORY_SEGMENT_GROUP, DXGI_MEMORY_SEGMENT_GROUP_LOCAL,
    DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO,
};

/// One detected display adapter.
pub struct GpuAdapter {
    pub name: String,
    /// Integrated (UMA): its GPU memory is mostly shared system memory.
    pub uma: bool,
    /// Bytes: dedicated memory for a discrete adapter, dedicated + shared for UMA.
    pub vram_total: u64,
    /// Adapter-wide bytes in use, by the same rule; `None` while unknown.
    pub vram_used: Option<u64>,
    /// GPU utilization 0..=100, or `None` if unavailable.
    pub load: Option<f32>,
    /// Lowercased "luid_0x..._0x..." token, used to match this adapter to PDH
    /// "GPU Engine" and "GPU Adapter Memory" counter instances.
    luid_key: String,
}

/// Enumerate all non-software GPUs: name, integrated or discrete, and memory
/// total. Usage is not read here: DXGI's QueryVideoMemoryInfo reports only this
/// process's own usage (about 0, TinyMon creates no device); `VramGroup` reads
/// the adapter-wide figures from PDH instead.
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

            // The two budgets tell UMA from discrete; DXGI computes them without
            // a D3D device, so this works from a plain process.
            let adapter3 = adapter.cast::<IDXGIAdapter3>().ok();
            let local = adapter3.as_ref().and_then(|a| budget(a, DXGI_MEMORY_SEGMENT_GROUP_LOCAL));
            let non_local =
                adapter3.as_ref().and_then(|a| budget(a, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL));
            let dedicated = desc.DedicatedVideoMemory as u64;
            let uma = is_uma(local, non_local, dedicated);
            let luid = desc.AdapterLuid;

            gpus.push(GpuAdapter {
                name: short_name(&utf16_to_string(&desc.Description)),
                uma,
                vram_total: vram_total(uma, dedicated, desc.SharedSystemMemory as u64),
                vram_used: None,
                load: None,
                luid_key: luid_key(luid.HighPart, luid.LowPart),
            });
        }
    }
    gpus
}

/// One DXGI memory budget of `adapter`: the LOCAL or NON_LOCAL segment group.
/// `None` when the query fails.
fn budget(adapter: &IDXGIAdapter3, group: DXGI_MEMORY_SEGMENT_GROUP) -> Option<u64> {
    let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
    // SAFETY: COM call on a live interface; `info` is a stack out-param.
    unsafe { adapter.QueryVideoMemoryInfo(0, group, &mut info) }.ok()?;
    Some(info.Budget)
}

/// Integrated (UMA) or discrete, from the two DXGI budgets. "For UMA adapters
/// non-local is always zero" (Microsoft's memory-management guidance), and a
/// UMA adapter's local budget (shared system memory) exceeds its small
/// dedicated carve-out. Any other pair of budgets means discrete; without both
/// budgets only an adapter with no dedicated memory is UMA. A zeroed query
/// therefore reads as discrete, the safe reading.
fn is_uma(local: Option<u64>, non_local: Option<u64>, dedicated: u64) -> bool {
    match (local, non_local) {
        (Some(l), Some(0)) => l > dedicated || dedicated == 0,
        (Some(_), Some(_)) => false,
        _ => dedicated == 0,
    }
}

/// Memory total: Task Manager's "GPU memory" (dedicated + shared system memory)
/// for a UMA adapter, its "Dedicated GPU memory" for a discrete one.
fn vram_total(uma: bool, dedicated: u64, shared_system: u64) -> u64 {
    if uma {
        dedicated.saturating_add(shared_system)
    } else {
        dedicated
    }
}

/// Memory in use by the same rule: dedicated usage for a discrete adapter,
/// dedicated + shared usage for a UMA one. Shared usage is almost all of a UMA
/// adapter's figure, so without it the reading is `None`, not a misleading 0;
/// an unknown dedicated part counts as 0.
fn vram_used(uma: bool, dedicated: Option<u64>, shared: Option<u64>) -> Option<u64> {
    if !uma {
        return dedicated;
    }
    Some(dedicated.unwrap_or(0).saturating_add(shared?))
}

/// Bytes one adapter uses: the sum of its "luid_..._phys_N" instances of a
/// "GPU Adapter Memory" counter. `None` (not 0) when no instance matches.
fn adapter_bytes(samples: &[(String, f64)], luid_key: &str) -> Option<u64> {
    samples
        .iter()
        .filter(|(name, _)| name.starts_with(luid_key))
        .map(|(_, v)| *v as u64)
        .reduce(u64::saturating_add)
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
/// detection; each tick it reads adapter-wide usage from the PDH "GPU Adapter
/// Memory" counters (instantaneous byte counts, valid on the first collect;
/// Windows 10 1709 and later).
pub struct VramGroup {
    gpus: Vec<GpuAdapter>,
    dedicated: PdhCounter,
    shared: PdhCounter,
}

impl VramGroup {
    pub fn new() -> Self {
        VramGroup {
            gpus: detect_gpus(),
            dedicated: PdhCounter::new(w!("\\GPU Adapter Memory(*)\\Dedicated Usage")),
            shared: PdhCounter::new(w!("\\GPU Adapter Memory(*)\\Shared Usage")),
        }
    }
    /// Read both counters once and update every adapter's usage; a missing
    /// counter, a failed collect or no instance of the adapter's LUID is n/a.
    pub fn refresh(&mut self) {
        let dedicated = self.dedicated.try_sample().unwrap_or_default();
        let shared = self.shared.try_sample().unwrap_or_default();
        for g in self.gpus.iter_mut() {
            let (d, s) =
                (adapter_bytes(&dedicated, &g.luid_key), adapter_bytes(&shared, &g.luid_key));
            g.vram_used = vram_used(g.uma, d, s);
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

/// One memory row per adapter, labelled by kind: "GPU Mem" for an integrated
/// (UMA) adapter, "VRAM" for a discrete one. With several adapters the name
/// follows the label ("VRAM RTX 3050", "GPU Mem UHD 620"), so a cut label still
/// says which memory it is; the name part drops a leading "Intel " (the GPU
/// load row keeps it). A single "VRAM n/a" row when DXGI found no hardware
/// adapter.
fn vram_rows(gpus: &[GpuAdapter], out: &mut Vec<Row>) {
    if gpus.is_empty() {
        out.push(Row::na("VRAM"));
    }
    let multi = gpus.len() > 1;
    for g in gpus {
        let kind = if g.uma { "GPU Mem" } else { "VRAM" };
        let label = if multi {
            format!("{} {}", kind, g.name.strip_prefix("Intel ").unwrap_or(&g.name))
        } else {
            kind.to_string()
        };
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

/// GPU memory row (used/total + a memory-pressure bar that reddens as it
/// fills), or a value-only "n/a" row unless the usage is known and the total is
/// non-zero. Pure; unit-tested.
pub fn vram_row(name: &str, used: Option<u64>, total: u64) -> Row {
    match used {
        Some(u) if total > 0 => {
            Row::bar(name, fmt_vram(u, total), (u as f32 / total as f32).clamp(0.0, 1.0))
        }
        _ => Row::na(name),
    }
}

/// Format used/total with a unit that suits the total (GiB or MiB).
fn fmt_vram(used: u64, total: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: f64 = 1024.0 * 1024.0;
    if total >= GIB {
        format!("{:.1}/{:.0}G", used as f64 / GIB as f64, total as f64 / GIB as f64)
    } else {
        format!("{:.0}/{:.0}M", used as f64 / MIB, total as f64 / MIB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIB: u64 = 1024 * 1024;
    const GIB: u64 = 1024 * MIB;

    /// An adapter with the iGPU's LUID (low part 0x109f8) and the given load.
    fn adapter(load: Option<f32>) -> GpuAdapter {
        GpuAdapter {
            name: "GPU".into(),
            uma: true,
            vram_total: 0,
            vram_used: None,
            load,
            luid_key: luid_key(0, 0x109f8),
        }
    }

    /// An adapter named `name` with the given kind and memory figures.
    fn mem(name: &str, uma: bool, used: Option<u64>, total: u64) -> GpuAdapter {
        GpuAdapter { name: name.into(), uma, vram_total: total, vram_used: used, ..adapter(None) }
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
    fn load_rows_gives_one_row_per_adapter() {
        // With adapters there is no extra "GPU n/a" row, even when a load is unknown.
        let gpus = [
            GpuAdapter { name: "Intel UHD 730".into(), ..adapter(Some(40.0)) },
            GpuAdapter { name: "RTX 3050".into(), ..adapter(None) },
        ];
        let mut out = Vec::new();
        load_rows(&gpus, &mut out);
        let got: Vec<_> = out.iter().map(|r| (r.label.as_str(), r.value.as_str(), r.bar)).collect();
        assert_eq!(got, [("Intel UHD 730", "40%", Some(0.4)), ("RTX 3050", "n/a", None)]);
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
        let r = vram_row("VRAM", Some(2 * GIB), 16 * GIB);
        assert_eq!(r.label, "VRAM");
        assert_eq!(r.value, "2.0/16G");
        assert!((r.bar.unwrap() - 0.125).abs() < 1e-6);
        assert_eq!(vram_row("VRAM", Some(0), 512 * MIB).value, "0/512M"); // MiB below 1 GiB
    }

    #[test]
    fn vram_unavailable_is_na() {
        let r = vram_row("VRAM", None, 16 * GIB);
        assert_eq!(r.value, "n/a");
        assert_eq!(r.bar, None);
    }

    #[test]
    fn vram_row_without_a_total_is_na() {
        // A total of 0 (an adapter classed discrete that reports no dedicated
        // memory): n/a, not "0/0M" over a full bar.
        let r = vram_row("VRAM", Some(5), 0);
        assert_eq!(r.value, "n/a");
        assert_eq!(r.bar, None);
    }

    #[test]
    fn uma_needs_both_budget_signals() {
        // The UHD 730 as measured: LOCAL budget 7369 MiB, NON_LOCAL 0, 128 MiB carve-out.
        assert!(is_uma(Some(7369 * MIB), Some(0), 128 * MIB));
        // A zeroed query reads as discrete, the safe reading.
        assert!(!is_uma(Some(0), Some(0), 8 * GIB));
        assert!(!is_uma(Some(7 * GIB), Some(8 * GIB), 8 * GIB));
        // Without both budgets, only an adapter with no dedicated memory is UMA.
        assert!(is_uma(None, None, 0));
        assert!(!is_uma(None, None, 4 * GIB));
    }

    #[test]
    fn adapter_bytes_sums_one_adapters_instances() {
        let s = [
            ("luid_0x00000000_0x000109f8_phys_0".to_string(), 100.0),
            ("luid_0x00000000_0x000109f8_phys_1".to_string(), 50.0),
            ("luid_0x00000000_0x00010d83_phys_0".to_string(), 7.0),
        ];
        assert_eq!(adapter_bytes(&s, &luid_key(0, 0x109f8)), Some(150));
        assert_eq!(adapter_bytes(&s, &luid_key(0, 0x10d83)), Some(7));
        assert_eq!(adapter_bytes(&s, &luid_key(0, 0x1234)), None);
    }

    #[test]
    fn discrete_vram_is_dedicated_memory() {
        assert_eq!(vram_total(false, 16 * GIB, 16 * GIB), 16 * GIB);
        assert_eq!(vram_used(false, Some(10 * GIB), Some(300 * MIB)), Some(10 * GIB));
        assert_eq!(vram_used(false, None, Some(300 * MIB)), None);
    }

    #[test]
    fn integrated_vram_is_all_gpu_memory() {
        let total = vram_total(true, 128 * MIB, 8027 * MIB);
        assert_eq!(total, 8155 * MIB);
        let used = vram_used(true, Some(0), Some(793 * MIB));
        assert_eq!(used, Some(793 * MIB));
        assert_eq!(vram_row("GPU Mem", used, total).value, "0.8/8G");
        assert_eq!(vram_used(true, None, Some(5)), Some(5));
        assert_eq!(vram_used(true, None, None), None);
    }

    #[test]
    fn integrated_vram_without_shared_usage_is_na() {
        // A UMA tick whose Shared Usage read failed: the dedicated part alone (0 on
        // the UHD 730) would show a real-looking "0.0/8G", so the usage is unknown.
        let total = vram_total(true, 128 * MIB, 8027 * MIB);
        assert_eq!(vram_row("GPU Mem", vram_used(true, Some(0), None), total).value, "n/a");
        assert_eq!(vram_used(true, Some(64 * MIB), None), None);
    }

    #[test]
    fn vram_rows_label_by_kind_then_name_when_several() {
        let rows = |gpus: &[GpuAdapter]| {
            let mut out = Vec::new();
            vram_rows(gpus, &mut out);
            out.into_iter().map(|r| (r.label, r.value)).collect::<Vec<_>>()
        };
        let row = |label: &str, value: &str| (label.to_string(), value.to_string());
        let igpu = mem("Intel UHD 730", true, Some(793 * MIB), 8155 * MIB);
        assert_eq!(rows(&[igpu]), [row("GPU Mem", "0.8/8G")]);
        let dgpu = mem("RTX 3050", false, Some(2 * GIB), 16 * GIB);
        assert_eq!(rows(&[dgpu]), [row("VRAM", "2.0/16G")]);
        // Label first, so an ellipsis keeps it; the name loses a leading "Intel ".
        let two =
            [mem("Intel UHD 620", true, Some(GIB), 8 * GIB), mem("RTX 3050", false, None, 8 * GIB)];
        assert_eq!(rows(&two), [row("GPU Mem UHD 620", "1.0/8G"), row("VRAM RTX 3050", "n/a")]);
        // A name that is only "Intel" is kept as it is.
        let bare = [mem("Intel", true, Some(GIB), 8 * GIB), mem("RTX 3050", false, None, 8 * GIB)];
        assert_eq!(rows(&bare)[0].0, "GPU Mem Intel");
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
    fn raw_pdh_names_match_once_lowercased() {
        // Raw PDH instance names spell the LUID in uppercase hex ("0x000109F8",
        // "engtype_3D"); PdhCounter lowercases them, and LUID matching relies on it.
        let read = |raw: &str| {
            let mut buf: Vec<u16> = raw.encode_utf16().chain([0]).collect();
            // SAFETY: a NUL-terminated buffer that outlives the call.
            unsafe { crate::pdh::pwstr_to_string(windows::core::PWSTR(buf.as_mut_ptr())) }
        };
        let key = luid_key(0, 0x109f8);
        let engine = read("pid_968_luid_0x00000000_0x000109F8_phys_0_eng_0_engtype_3D");
        assert_eq!(busiest_engine(&[(engine, 42.0)], Some(&key)), Some(42.0));
        let memory = read("luid_0x00000000_0x000109F8_phys_0");
        assert_eq!(adapter_bytes(&[(memory, 793.0)], &key), Some(793));
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
    fn engine_key_keeps_odd_names_whole() {
        // Without "luid_" the key starts at the beginning of the name.
        assert_eq!(engine_key("pid_1_eng_0_engtype_3d"), "pid_1_eng_0");
        assert_eq!(engine_key("pid_1_eng_0"), "pid_1_eng_0");
        // "_engtype_" before "luid_" gives no valid range: the whole name is the
        // key, and the lookup never panics (release builds abort on a panic).
        assert_eq!(engine_key("x_engtype_3d_luid_y"), "x_engtype_3d_luid_y");
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
        let mut vram = VramGroup::new();
        vram.refresh();
        for g in &vram.gpus {
            println!(
                "GPU: {:<16} uma={}  VRAM {:?} / {} MiB  key={}",
                g.name,
                g.uma,
                g.vram_used.map(|u| u / MIB),
                g.vram_total / MIB,
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
