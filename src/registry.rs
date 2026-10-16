//! Metric registry — a uniform, toggleable list of metric sources.
//!
//! Each source is sampled once per tick and emits 0..N display rows. Only
//! ENABLED sources are constructed by `build_sources`, so a disabled metric
//! uses no resources: its sampler (and any PDH query) is never created, and
//! toggling it off later drops the source (closing the query via `Drop`).

use windows::Win32::System::SystemInformation::GetTickCount64;

use crate::battery::{battery_row, read_battery, BatteryInfo};
use crate::disk::DiskSampler;
use crate::gpu::{GpuGroup, VramGroup};
use crate::metrics::{read_memory, CpuSampler, MemInfo};
use crate::net::NetSampler;
use crate::percpu::PerCpuSampler;
use crate::settings::Settings;

/// One displayable line. `bar = None` is a value-only row (no bar drawn).
/// `color` is the label's accent color (`None` -> default foreground).
/// `bar_color` overrides the bar fill color (`None` -> the default load color,
/// green→yellow→red by load); used e.g. by battery to color by remaining charge.
/// `alert` lets the label turn red at high load (`render::is_critical`): true
/// for load rows (`Row::bar`), false for throughput rows (`Row::rate`), battery
/// and value-only rows.
pub struct Row {
    pub label: String,
    pub value: String,
    pub bar: Option<f32>,
    pub color: Option<(u8, u8, u8)>,
    pub bar_color: Option<(u8, u8, u8)>,
    pub alert: bool,
}

impl Row {
    /// A load row (CPU, RAM, GPU, VRAM, per-core, temperature) with a
    /// load-colored bar; its label turns red at high load.
    pub fn bar(label: impl Into<String>, value: impl Into<String>, frac: f32) -> Row {
        Row {
            label: label.into(),
            value: value.into(),
            bar: Some(frac),
            color: None,
            bar_color: None,
            alert: true,
        }
    }

    /// A throughput row (disk, net): the same load-colored bar, but `frac` is a
    /// rate against a soft ceiling, so the label never raises the red alert.
    pub fn rate(label: impl Into<String>, value: impl Into<String>, frac: f32) -> Row {
        Row { alert: false, ..Row::bar(label, value, frac) }
    }

    /// A reading that cannot be taken: a value-only "n/a" row (no bar, no
    /// alert), used instead of a misleading 0 or a missing row.
    pub fn na(label: impl Into<String>) -> Row {
        Row {
            label: label.into(),
            value: "n/a".into(),
            bar: None,
            color: None,
            bar_color: None,
            alert: false,
        }
    }
}

/// Stable identity for a metric — used for enable/disable and ordering.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetricKey {
    Cpu,
    Ram,
    Gpu,
    Vram,
    PerCpu,
    Disk,
    Net,
    Uptime,
    Battery,
    #[cfg(feature = "temp")]
    Temp,
    // Fps — deferred by design; not built. The registry is shaped so it would
    // slot in here (feature-gated, like Temp). See src/fps.rs for the rationale
    // and integration sketch.
}

impl MetricKey {
    pub fn as_str(self) -> &'static str {
        match self {
            MetricKey::Cpu => "cpu",
            MetricKey::Ram => "ram",
            MetricKey::Gpu => "gpu",
            MetricKey::Vram => "vram",
            MetricKey::PerCpu => "percpu",
            MetricKey::Disk => "disk",
            MetricKey::Net => "net",
            MetricKey::Uptime => "uptime",
            MetricKey::Battery => "battery",
            #[cfg(feature = "temp")]
            MetricKey::Temp => "temp",
        }
    }
    pub fn from_str(s: &str) -> Option<MetricKey> {
        match s {
            "cpu" => Some(MetricKey::Cpu),
            "ram" => Some(MetricKey::Ram),
            "gpu" => Some(MetricKey::Gpu),
            "vram" => Some(MetricKey::Vram),
            "percpu" => Some(MetricKey::PerCpu),
            "disk" => Some(MetricKey::Disk),
            "net" => Some(MetricKey::Net),
            "uptime" => Some(MetricKey::Uptime),
            "battery" => Some(MetricKey::Battery),
            #[cfg(feature = "temp")]
            "temp" => Some(MetricKey::Temp),
            _ => None,
        }
    }
    /// Human label for the settings UI checkboxes.
    pub fn label(self) -> &'static str {
        match self {
            MetricKey::Cpu => "CPU",
            MetricKey::Ram => "RAM",
            MetricKey::Gpu => "GPU",
            MetricKey::Vram => "VRAM",
            MetricKey::PerCpu => "Per-core CPU",
            MetricKey::Disk => "Disk",
            MetricKey::Net => "Network",
            MetricKey::Uptime => "Uptime",
            MetricKey::Battery => "Battery",
            #[cfg(feature = "temp")]
            MetricKey::Temp => "Temperature",
        }
    }
    /// Per-metric accent color (RGB) for the label text — distinct hues tuned
    /// for the dark background so each metric is easy to pick out.
    pub fn accent(self) -> (u8, u8, u8) {
        match self {
            MetricKey::Cpu => (70, 200, 224),     // cyan
            MetricKey::Ram => (180, 140, 240),    // violet
            MetricKey::Gpu => (92, 200, 92),      // green
            MetricKey::Vram => (130, 200, 150),   // mint (GPU memory)
            MetricKey::PerCpu => (72, 192, 168),  // teal
            MetricKey::Disk => (224, 168, 74),    // amber
            MetricKey::Net => (92, 156, 240),     // blue
            MetricKey::Uptime => (144, 160, 176), // gray-blue
            MetricKey::Battery => (160, 210, 90), // lime
            #[cfg(feature = "temp")]
            MetricKey::Temp => (224, 128, 80), // orange
        }
    }
    /// Whether the metric's value is a delta between two samples (CPU and GPU
    /// load, per-core CPU, disk and network throughput): a reading taken right
    /// after the sampler's baseline is not real, so its rows show `PENDING`
    /// until the first tick. Exhaustive on purpose: a new metric must be
    /// classified. Unrelated to `Row::rate`, which marks the throughput rows
    /// that never raise the red alert.
    pub fn is_delta(self) -> bool {
        match self {
            MetricKey::Cpu
            | MetricKey::PerCpu
            | MetricKey::Gpu
            | MetricKey::Disk
            | MetricKey::Net => true,
            MetricKey::Ram | MetricKey::Vram | MetricKey::Uptime | MetricKey::Battery => false,
            #[cfg(feature = "temp")]
            MetricKey::Temp => false,
        }
    }
    /// Every metric, in the order they appear in the settings list.
    #[cfg(not(feature = "temp"))]
    pub const ALL: [MetricKey; 9] = [
        MetricKey::Cpu,
        MetricKey::Ram,
        MetricKey::Gpu,
        MetricKey::Vram,
        MetricKey::PerCpu,
        MetricKey::Disk,
        MetricKey::Net,
        MetricKey::Uptime,
        MetricKey::Battery,
    ];
    #[cfg(feature = "temp")]
    pub const ALL: [MetricKey; 10] = [
        MetricKey::Cpu,
        MetricKey::Ram,
        MetricKey::Gpu,
        MetricKey::Vram,
        MetricKey::PerCpu,
        MetricKey::Disk,
        MetricKey::Net,
        MetricKey::Uptime,
        MetricKey::Battery,
        MetricKey::Temp,
    ];
    /// The default enabled set, in both builds: what a config without a
    /// `metrics=` key and "Reset to defaults" enable; the rest are opt-in.
    /// Temperature is opt-in too: most desktops have no live thermal zone, so
    /// it would show "Temp n/a" for good.
    pub const DEFAULT: [MetricKey; 3] = [MetricKey::Cpu, MetricKey::Ram, MetricKey::Gpu];
}

/// A live, stateful metric source. Some variants emit multiple rows.
pub enum MetricSource {
    Cpu {
        sampler: CpuSampler,
        pct: Option<f32>,
    },
    Ram {
        info: MemInfo,
    },
    Gpu(GpuGroup),
    Vram(VramGroup),
    PerCpu(PerCpuSampler),
    Disk(DiskSampler),
    Net(NetSampler),
    Uptime {
        ms: u64,
    },
    Battery {
        info: BatteryInfo,
    },
    #[cfg(feature = "temp")]
    Temp(crate::temp::TempSource),
}

impl MetricSource {
    pub fn key(&self) -> MetricKey {
        match self {
            MetricSource::Cpu { .. } => MetricKey::Cpu,
            MetricSource::Ram { .. } => MetricKey::Ram,
            MetricSource::Gpu(_) => MetricKey::Gpu,
            MetricSource::Vram(_) => MetricKey::Vram,
            MetricSource::PerCpu(_) => MetricKey::PerCpu,
            MetricSource::Disk(_) => MetricKey::Disk,
            MetricSource::Net(_) => MetricKey::Net,
            MetricSource::Uptime { .. } => MetricKey::Uptime,
            MetricSource::Battery { .. } => MetricKey::Battery,
            #[cfg(feature = "temp")]
            MetricSource::Temp(_) => MetricKey::Temp,
        }
    }

    /// Pull fresh data from the OS (called once per timer tick).
    pub fn sample(&mut self) {
        match self {
            MetricSource::Cpu { sampler, pct } => *pct = sampler.sample(),
            MetricSource::Ram { info } => *info = read_memory(),
            MetricSource::Gpu(group) => group.refresh(),
            MetricSource::Vram(g) => g.refresh(),
            MetricSource::PerCpu(s) => s.sample(),
            MetricSource::Disk(s) => s.sample(),
            MetricSource::Net(s) => s.sample(),
            // SAFETY: GetTickCount64 reads a counter; no args or pointers.
            MetricSource::Uptime { ms } => *ms = unsafe { GetTickCount64() },
            MetricSource::Battery { info } => *info = read_battery(),
            #[cfg(feature = "temp")]
            MetricSource::Temp(s) => s.sample(),
        }
    }

    /// Append this source's current rows, each stamped with the metric's accent
    /// color (the renderer uses it for the label text).
    pub fn rows(&self, out: &mut Vec<Row>) {
        let start = out.len();
        match self {
            MetricSource::Cpu { pct, .. } => out.push(pct.map_or_else(|| Row::na("CPU"), cpu_row)),
            MetricSource::Ram { info } => out.push(ram_row(info)),
            MetricSource::Gpu(group) => group.rows(out),
            MetricSource::Vram(g) => g.rows(out),
            MetricSource::PerCpu(s) => s.rows(out),
            MetricSource::Disk(s) => s.rows(out),
            MetricSource::Net(s) => s.rows(out),
            MetricSource::Uptime { ms } => out.push(uptime_row(*ms)),
            MetricSource::Battery { info } => out.push(battery_row(info)),
            #[cfg(feature = "temp")]
            MetricSource::Temp(s) => s.rows(out),
        }
        let accent = self.key().accent();
        for row in &mut out[start..] {
            if row.color.is_none() {
                row.color = Some(accent);
            }
        }
    }
}

/// Construct the ordered list of ENABLED sources from settings. Disabled
/// metrics are simply never built, so they consume nothing.
pub fn build_sources(settings: &Settings) -> Vec<MetricSource> {
    settings
        .order
        .iter()
        .map(|key| match key {
            MetricKey::Cpu => {
                let mut sampler = CpuSampler::new();
                sampler.sample(); // prime the baseline for the first real delta
                MetricSource::Cpu { sampler, pct: Some(0.0) }
            }
            MetricKey::Ram => MetricSource::Ram { info: read_memory() },
            MetricKey::Gpu => MetricSource::Gpu(GpuGroup::new()),
            MetricKey::Vram => MetricSource::Vram(VramGroup::new()),
            MetricKey::PerCpu => MetricSource::PerCpu(PerCpuSampler::new()),
            MetricKey::Disk => MetricSource::Disk(DiskSampler::new()),
            MetricKey::Net => MetricSource::Net(NetSampler::new()),
            // SAFETY: GetTickCount64 reads a counter; no args or pointers.
            MetricKey::Uptime => MetricSource::Uptime { ms: unsafe { GetTickCount64() } },
            MetricKey::Battery => MetricSource::Battery { info: read_battery() },
            #[cfg(feature = "temp")]
            MetricKey::Temp => MetricSource::Temp(crate::temp::TempSource::new()),
        })
        .collect()
}

/// Shown instead of a delta metric's value until a full period has passed
/// since its sampler's baseline: an ellipsis.
pub const PENDING: &str = "\u{2026}";

/// Mask rows whose values are not real yet: each value becomes `PENDING` and a
/// bar drops to an empty track (no bogus level, no red alert); value-only rows
/// stay value-only.
pub fn mask_pending(rows: &mut [Row]) {
    for row in rows {
        row.value = PENDING.into();
        if row.bar.is_some() {
            row.bar = Some(0.0);
        }
    }
}

/// Every source's rows in display order, each tagged with its source's metric
/// (`keys[i]` is the metric of `rows[i]`): the one flatten that painting and
/// the sparkline history (`history::row_ids`) read rows through. While
/// `pending` (from a build of the sources until the first tick), the rows of
/// delta metrics are masked (`mask_pending`); WM_TIMER passes false, so a
/// masked value never reaches the history.
pub fn collect_rows(sources: &[MetricSource], pending: bool) -> (Vec<Row>, Vec<MetricKey>) {
    let mut rows = Vec::new();
    let mut keys = Vec::new();
    for s in sources {
        let start = rows.len();
        s.rows(&mut rows);
        if pending && s.key().is_delta() {
            mask_pending(&mut rows[start..]);
        }
        keys.resize(rows.len(), s.key());
    }
    (rows, keys)
}

/// Representative placeholder rows for the settings live-preview — built through
/// the real formatters (so the preview can't drift) with canned values, so it
/// opens no PDH queries and touches no hardware.
pub fn sample_rows(order: &[MetricKey]) -> Vec<Row> {
    const GIB: u64 = 1024 * 1024 * 1024;
    let mut out = Vec::new();
    for key in order {
        let start = out.len();
        match key {
            MetricKey::Cpu => out.push(cpu_row(45.0)),
            MetricKey::Ram => {
                out.push(ram_row(&MemInfo { load_pct: 62.0, used_gib: 9.9, total_gib: 16.0 }))
            }
            MetricKey::Gpu => out.push(crate::gpu::gpu_row("GPU", Some(30.0))),
            MetricKey::Vram => out.push(crate::gpu::vram_row("VRAM", Some(4 * GIB), 12 * GIB)),
            MetricKey::PerCpu => {
                for (c, &pct) in [22.0_f64, 8.0, 51.0, 16.0].iter().enumerate() {
                    out.push(crate::percpu::percpu_row(c, pct));
                }
            }
            MetricKey::Disk => {
                out.push(crate::disk::disk_row("Disk R", Some(8.0 * 1024.0 * 1024.0)));
                out.push(crate::disk::disk_row("Disk W", Some(2.0 * 1024.0 * 1024.0)));
            }
            MetricKey::Net => {
                out.push(crate::net::net_row("Net\u{2191}", Some(1.5 * 1024.0 * 1024.0)));
                out.push(crate::net::net_row("Net\u{2193}", Some(6.0 * 1024.0 * 1024.0)));
            }
            MetricKey::Uptime => out.push(uptime_row(13_329_000)),
            MetricKey::Battery => out.push(battery_row(&BatteryInfo {
                present: true,
                ac_online: false,
                charging: false,
                percent: 72,
                secs_left: Some(7800),
            })),
            #[cfg(feature = "temp")]
            MetricKey::Temp => out.push(crate::temp::temp_row("Temp", 55.0)),
        }
        let accent = key.accent();
        for row in &mut out[start..] {
            if row.color.is_none() {
                row.color = Some(accent);
            }
        }
    }
    out
}

// ---- Pure row formatters (unit-tested) -------------------------------------

pub fn cpu_row(pct: f32) -> Row {
    Row::bar("CPU", format!("{:.0}%", pct), pct / 100.0)
}

pub fn ram_row(m: &MemInfo) -> Row {
    Row::bar(
        "RAM",
        format!("{:.0}% {:.1}/{:.0}G", m.load_pct, m.used_gib, m.total_gib),
        m.load_pct / 100.0,
    )
}

/// Uptime as a value-only row (no bar): "3d 04:12" or "04:12:33".
pub fn uptime_row(ms: u64) -> Row {
    let secs = ms / 1000;
    let (d, h, m, s) = (secs / 86400, (secs % 86400) / 3600, (secs % 3600) / 60, secs % 60);
    let value = if d > 0 {
        format!("{}d {:02}:{:02}", d, h, m)
    } else {
        format!("{:02}:{:02}:{:02}", h, m, s)
    };
    Row { label: "Up".into(), value, bar: None, color: None, bar_color: None, alert: false }
}

/// Human-readable byte rate, e.g. "5.0M/s", "12K/s", "300B/s".
pub fn fmt_rate(bps: f64) -> String {
    const M: f64 = 1024.0 * 1024.0;
    const K: f64 = 1024.0;
    if bps >= M {
        format!("{:.1}M/s", bps / M)
    } else if bps >= K {
        format!("{:.0}K/s", bps / K)
    } else {
        format!("{:.0}B/s", bps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A failed GetSystemTimes leaves the CPU without a reading: n/a, not 0%.
    #[test]
    fn cpu_without_a_reading_is_na() {
        let mut out = Vec::new();
        MetricSource::Cpu { sampler: CpuSampler::new(), pct: None }.rows(&mut out);
        assert_eq!((out[0].label.as_str(), out[0].value.as_str()), ("CPU", "n/a"));
        assert!(out[0].bar.is_none());
    }

    #[test]
    fn metric_key_roundtrip() {
        for k in MetricKey::ALL {
            assert_eq!(MetricKey::from_str(k.as_str()), Some(k));
        }
        assert_eq!(MetricKey::from_str("bogus"), None);
    }

    #[test]
    fn sample_rows_covers_order_and_stamps_accent() {
        let rows = sample_rows(&[MetricKey::Cpu, MetricKey::Ram, MetricKey::PerCpu]);
        assert_eq!(rows.len(), 6); // cpu(1) + ram(1) + per-core(4)
        assert_eq!(rows[0].label, "CPU");
        assert!(rows.iter().all(|r| r.color.is_some())); // accent stamped on each
    }

    #[cfg(feature = "temp")]
    #[test]
    fn preview_temp_row_claims_no_cpu_sensor() {
        // No source labels a reading "CPU Temp" any more, so neither does
        // the preview.
        let rows = sample_rows(&[MetricKey::Temp]);
        assert_eq!((rows[0].label.as_str(), rows[0].value.as_str()), ("Temp", "55\u{00B0}C"));
    }

    #[test]
    fn every_metric_has_accent() {
        for k in MetricKey::ALL {
            let (r, g, b) = k.accent();
            assert!(r as u16 + g as u16 + b as u16 > 0);
        }
    }

    #[test]
    fn cpu_row_formats() {
        let r = cpu_row(37.4);
        assert_eq!(r.value, "37%");
        assert!((r.bar.unwrap() - 0.374).abs() < 1e-6);
    }

    #[test]
    fn ram_row_formats() {
        let m = MemInfo { load_pct: 58.0, used_gib: 9.3, total_gib: 16.0 };
        assert_eq!(ram_row(&m).value, "58% 9.3/16G");
    }

    #[test]
    fn uptime_formats() {
        // 3d 04:12:05
        let ms = (3 * 86400 + 4 * 3600 + 12 * 60 + 5) * 1000;
        let r = uptime_row(ms);
        assert_eq!(r.label, "Up");
        assert_eq!(r.value, "3d 04:12");
        assert_eq!(r.bar, None);
        // under a day -> HH:MM:SS
        assert_eq!(uptime_row((4 * 3600 + 12 * 60 + 5) * 1000).value, "04:12:05");
    }

    #[test]
    fn rate_formats() {
        assert_eq!(fmt_rate(5.0 * 1024.0 * 1024.0), "5.0M/s");
        assert_eq!(fmt_rate(2048.0), "2K/s");
        assert_eq!(fmt_rate(300.0), "300B/s");
    }

    #[test]
    fn na_row_is_value_only() {
        let r = Row::na("Disk R");
        assert_eq!(r.label, "Disk R");
        assert_eq!(r.value, "n/a");
        assert_eq!(r.bar, None);
        assert!(!r.alert);
    }

    #[test]
    fn collect_rows_tags_each_row_with_its_metric() {
        let sources = [
            MetricSource::Uptime { ms: 3_600_000 },
            MetricSource::Ram { info: MemInfo { load_pct: 58.0, used_gib: 9.3, total_gib: 16.0 } },
        ];
        let (rows, keys) = collect_rows(&sources, false);
        let labels: Vec<&str> = rows.iter().map(|r| r.label.as_str()).collect();
        assert_eq!(labels, ["Up", "RAM"]);
        assert_eq!(keys, [MetricKey::Uptime, MetricKey::Ram]);
    }

    #[test]
    fn delta_metrics_are_flagged() {
        let delta: Vec<MetricKey> = MetricKey::ALL.into_iter().filter(|k| k.is_delta()).collect();
        assert_eq!(
            delta,
            [MetricKey::Cpu, MetricKey::Gpu, MetricKey::PerCpu, MetricKey::Disk, MetricKey::Net]
        );
    }

    #[test]
    fn pending_mask_hides_values_until_first_full_interval() {
        let mut rows = [cpu_row(73.0), Row::na("GPU")];
        mask_pending(&mut rows);
        assert_eq!((rows[0].value.as_str(), rows[0].bar), ("\u{2026}", Some(0.0)));
        assert_eq!((rows[1].value.as_str(), rows[1].bar), ("\u{2026}", None));
    }

    #[test]
    fn pending_mask_clears_a_bogus_red_alert() {
        let mut rows = [crate::percpu::percpu_row(0, 100.0)];
        assert!(crate::render::is_critical(&rows[0])); // the bogus first value alerts
        mask_pending(&mut rows);
        assert!(!crate::render::is_critical(&rows[0]));
    }

    #[test]
    fn collect_rows_masks_only_delta_metrics_while_pending() {
        fn shown(rows: &[Row]) -> Vec<(&str, &str, Option<f32>)> {
            rows.iter().map(|r| (r.label.as_str(), r.value.as_str(), r.bar)).collect()
        }
        // Built by hand, so no PDH reading decides the rows (a priming sample
        // may read every core, some or none): CPU is a delta row with a bar,
        // and a per-core sampler that has not sampled shows the one value-only
        // "Cores n/a" row that a priming sample reading no core leaves. RAM
        // sits between them, so a mask that reaches past a delta metric's own
        // rows shows on it. A disk sampler that has not sampled always shows
        // two value-only n/a rows, so the mask must cover every row of a
        // metric, not only its first, and each row must be tagged Disk.
        let sources = [
            MetricSource::Cpu { sampler: CpuSampler::new(), pct: Some(73.0) },
            MetricSource::Ram { info: MemInfo { load_pct: 58.0, used_gib: 9.3, total_gib: 16.0 } },
            MetricSource::PerCpu(PerCpuSampler::new()),
            MetricSource::Disk(DiskSampler::new()),
        ];
        let (rows, keys) = collect_rows(&sources, true);
        assert_eq!(
            keys,
            [MetricKey::Cpu, MetricKey::Ram, MetricKey::PerCpu, MetricKey::Disk, MetricKey::Disk]
        );
        assert_eq!(
            shown(&rows),
            [
                ("CPU", PENDING, Some(0.0)),
                ("RAM", "58% 9.3/16G", Some(0.58)),
                ("Cores", PENDING, None),
                ("Disk R", PENDING, None),
                ("Disk W", PENDING, None)
            ]
        );
        let (rows, _) = collect_rows(&sources, false);
        assert_eq!(
            shown(&rows),
            [
                ("CPU", "73%", Some(0.73)),
                ("RAM", "58% 9.3/16G", Some(0.58)),
                ("Cores", "n/a", None),
                ("Disk R", "n/a", None),
                ("Disk W", "n/a", None)
            ]
        );
    }
}
