//! The Temperature metric (Cargo feature `temp`, on by default): the ACPI
//! thermal zones Windows publishes through the PDH counter
//! `\Thermal Zone Information(*)\Temperature`, read without admin rights or
//! extra software. The counter reports whole kelvin; readings outside 1-150 C
//! are ignored. A zone is shown only once its reading has moved: many boards
//! publish a firmware stub that reads one constant at any load, and a reading
//! that never moved is no evidence of a sensor. A zone that has moved stays
//! shown for the rest of the process, also after a metric Apply rebuilds the
//! source (`PROVEN_ZONES`). The labels name what is read, an ACPI zone:
//! "ACPI Temp" when the board has one zone, otherwise "ACPI" and the zone's
//! ACPI name ("ACPI THRM"), never "CPU Temp". With no zone shown, the metric
//! keeps one "Temp n/a" row, so an enabled metric stays visible.

use core::cell::RefCell;

use windows::core::w;
use windows::Win32::Foundation::LUID;

use crate::gpu;
use crate::gputemp;
use crate::pdh::PdhCounter;
use crate::registry::Row;

thread_local! {
    /// The zones whose reading has moved in this process (UI thread only). A
    /// metric Apply rebuilds the source with an empty zone list; without this
    /// list a real sensor would be hidden again until it moved once more.
    static PROVEN_ZONES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

pub struct TempSource {
    /// The ACPI thermal zones Windows exposes through PDH (no admin).
    zone: PdhCounter,
    /// Every zone with a plausible reading since the source was built, in the
    /// order first seen.
    zones: Vec<Zone>,
    /// One sensor per hardware GPU, read through D3DKMT (`gputemp`).
    gpus: Vec<GpuTemp>,
}

impl TempSource {
    pub fn new() -> Self {
        TempSource {
            zone: PdhCounter::new(w!("\\Thermal Zone Information(*)\\Temperature")),
            zones: Vec::new(),
            gpus: gpu::detect_gpus().iter().map(|g| GpuTemp::new(&g.name, g.luid())).collect(),
        }
    }

    pub fn sample(&mut self) {
        let sample = self.zone.sample();
        PROVEN_ZONES.with(|p| observe_zones(&mut self.zones, &sample, &mut p.borrow_mut()));
        for g in self.gpus.iter_mut() {
            let reading = gputemp::read(g.luid);
            note_gpu_reading(g, reading);
        }
    }

    pub fn rows(&self, out: &mut Vec<Row>) {
        build_rows(&self.gpus, &self.zones, out);
    }
}

/// One GPU's temperature sensor.
struct GpuTemp {
    /// The adapter's short name ("RTX 4080").
    name: String,
    luid: LUID,
    /// Shown: the driver has reported a plausible temperature since the source
    /// was built. Never resets.
    live: bool,
    /// The last sample's reading in degrees Celsius; None without one.
    now: Option<f64>,
}

impl GpuTemp {
    fn new(name: &str, luid: LUID) -> Self {
        GpuTemp { name: name.to_string(), luid, live: false, now: None }
    }
}

/// Record one sample's reading of a GPU sensor: a plausible one makes it
/// live, for good; a missing one leaves it without a current reading.
fn note_gpu_reading(g: &mut GpuTemp, reading: Option<f64>) {
    g.now = reading;
    g.live |= reading.is_some();
}

/// The shown GPU sensors as (label, degrees Celsius or None): "GPU" when one
/// GPU has reported, else "GPU" and the adapter's name.
fn gpu_rows(gpus: &[GpuTemp]) -> Vec<(String, Option<f64>)> {
    let several = gpus.iter().filter(|g| g.live).count() > 1;
    gpus.iter()
        .filter(|g| g.live)
        .map(|g| (if several { format!("GPU {}", g.name) } else { "GPU".into() }, g.now))
        .collect()
}

/// One thermal zone, followed across samples by its PDH instance name.
struct Zone {
    /// The PDH instance name, lowercase (`\_tz.tz00`).
    name: String,
    /// The first plausible reading, in kelvin.
    first: f64,
    /// The last sample's plausible reading in kelvin; None without one.
    now: Option<f64>,
    /// Shown: the reading has moved from `first`, or the zone moved earlier
    /// in the process (`PROVEN_ZONES`). Never resets.
    live: bool,
}

/// Whether a reading in degrees Celsius can be real: 1 C up to 150 C.
fn plausible(celsius: f64) -> bool {
    (1.0..150.0).contains(&celsius)
}

/// Record one sample of (PDH instance, kelvin) pairs. A zone is added at its
/// first plausible reading, live at once if `proven` names it; otherwise it
/// becomes live, for good, at the first reading 0.5 K or more away from that
/// first one (the counter reports whole kelvin), and joins `proven`. A zone
/// without a plausible reading in this sample keeps its state but has no
/// current reading.
fn observe_zones(zones: &mut Vec<Zone>, sample: &[(String, f64)], proven: &mut Vec<String>) {
    for z in zones.iter_mut() {
        z.now = None;
    }
    for (name, k) in sample {
        if !plausible(k - 273.15) {
            continue;
        }
        let i = zones.iter().position(|z| z.name == *name).unwrap_or_else(|| {
            let live = proven.contains(name);
            zones.push(Zone { name: name.clone(), first: *k, now: None, live });
            zones.len() - 1
        });
        let z = &mut zones[i];
        // A second reading under the same name in one sample would look like
        // movement: only the first one counts.
        if z.now.is_some() {
            continue;
        }
        z.now = Some(*k);
        if !z.live && (k - z.first).abs() >= 0.5 {
            z.live = true;
            proven.push(name.clone());
        }
    }
}

/// A zone's label: "ACPI Temp" for a board's only zone; with several, "ACPI"
/// and the zone's ACPI name, the part of its instance name after the last '.'
/// or '\' (`\_tz.thrm` reads "ACPI THRM", `\_tz.tz0` "ACPI TZ0"; Windows trims
/// the names' padding). Nothing says which part of the machine a zone
/// measures, so it is never "CPU Temp".
fn zone_label(instance: &str, several: bool) -> String {
    if !several {
        return "ACPI Temp".into();
    }
    let name = instance.rsplit(['.', '\\']).next().unwrap_or(instance);
    format!("ACPI {name}").trim_end().to_ascii_uppercase()
}

/// The shown zones as (label, degrees Celsius or None), in the order first
/// seen: every live zone, with None when it had no reading in the last sample.
/// Every zone seen counts towards "several", so a real zone next to a hidden
/// stub keeps its name.
fn live_zone_rows(zones: &[Zone]) -> Vec<(String, Option<f64>)> {
    let several = zones.len() > 1;
    zones
        .iter()
        .filter(|z| z.live)
        .map(|z| (zone_label(&z.name, several), z.now.map(|k| k - 273.15)))
        .collect()
}

/// The metric's rows, all put together here: the GPU sensors' rows, then the
/// shown zones' (a value-only n/a row for one without a current reading, so
/// the row count stays stable), else one value-only "Temp n/a" row, so an
/// enabled metric stays visible.
fn build_rows(gpus: &[GpuTemp], zones: &[Zone], out: &mut Vec<Row>) {
    let rows: Vec<_> = gpu_rows(gpus).into_iter().chain(live_zone_rows(zones)).collect();
    if rows.is_empty() {
        out.push(Row::na("Temp"));
    }
    for (label, celsius) in rows {
        out.push(match celsius {
            Some(c) => temp_row(&label, c),
            None => Row::na(label),
        });
    }
}

pub fn temp_row(name: &str, celsius: f64) -> Row {
    Row::bar(name, format!("{:.0}\u{00B0}C", celsius), (celsius / 100.0).clamp(0.0, 1.0) as f32)
}

#[cfg(test)]
mod tests {
    use super::{
        build_rows, live_zone_rows, note_gpu_reading, observe_zones, temp_row, zone_label, GpuTemp,
        Zone,
    };

    #[test]
    fn formats() {
        let r = temp_row("CPU", 55.0);
        assert_eq!(r.label, "CPU");
        assert_eq!(r.value, "55\u{00B0}C");
        assert!((r.bar.unwrap() - 0.55).abs() < 1e-6);
    }

    /// Feed `ticks` to a fresh zone list, one sample of (PDH instance, kelvin)
    /// pairs per tick; `proven` holds the zones proven earlier in the process.
    fn observe(ticks: &[&[(&str, f64)]], proven: &mut Vec<String>) -> Vec<Zone> {
        let mut zones = Vec::new();
        for tick in ticks {
            let sample: Vec<_> = tick.iter().map(|&(n, k)| (n.to_string(), k)).collect();
            observe_zones(&mut zones, &sample, proven);
        }
        zones
    }

    /// The (label, value, has a bar) of each row build_rows puts together.
    fn shown(zones: &[Zone]) -> Vec<(String, String, bool)> {
        shown_with(&[], zones)
    }

    /// The same, with GPU readings.
    fn shown_with(gpus: &[GpuTemp], zones: &[Zone]) -> Vec<(String, String, bool)> {
        let mut out = Vec::new();
        build_rows(gpus, zones, &mut out);
        out.into_iter().map(|r| (r.label, r.value, r.bar.is_some())).collect()
    }

    /// A GPU sensor after `readings`, one per tick.
    fn gpu(name: &str, readings: &[Option<f64>]) -> GpuTemp {
        let mut g = GpuTemp::new(name, Default::default());
        for r in readings {
            note_gpu_reading(&mut g, *r);
        }
        g
    }

    fn row(label: &str, value: &str, bar: bool) -> (String, String, bool) {
        (label.to_string(), value.to_string(), bar)
    }

    #[test]
    fn constant_stub_zone_is_never_shown() {
        // The development machine's only zone: a firmware stub that reads
        // 301 K (27.85 C) at any load.
        let stub: &[(&str, f64)] = &[("\\_tz.tz00", 301.0)];
        let mut proven = Vec::new();
        let zones = observe(&[stub; 600], &mut proven);
        assert_eq!(shown(&zones), [row("Temp", "n/a", false)]);
        assert!(live_zone_rows(&zones).is_empty());
        assert!(proven.is_empty());
    }

    #[test]
    fn zone_is_shown_once_it_moves_and_stays_shown() {
        let tz00 = |k: f64| [("\\_tz.tz00".to_string(), k)];
        let (mut zones, mut proven) = (Vec::new(), Vec::new());
        for k in [318.0, 318.0] {
            observe_zones(&mut zones, &tz00(k), &mut proven);
        }
        assert_eq!(shown(&zones), [row("Temp", "n/a", false)]);
        // One kelvin of movement proves the zone; it stays shown when the
        // reading returns to where it started, with the latest reading.
        for k in [319.0, 318.0] {
            observe_zones(&mut zones, &tz00(k), &mut proven);
        }
        let live = live_zone_rows(&zones);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].0, "ACPI Temp");
        assert!((live[0].1.unwrap() - 44.85).abs() < 1e-9);
        assert_eq!(shown(&zones), [row("ACPI Temp", "45\u{00B0}C", true)]);
        assert_eq!(proven, ["\\_tz.tz00"]);
    }

    #[test]
    fn several_zones_get_stable_firmware_names() {
        // A stub next to a real sensor: only the sensor shows, named after
        // its ACPI zone.
        let mut proven = Vec::new();
        let zones = observe(
            &[
                &[("\\_tz.tz00", 301.0), ("\\_tz.thrm", 320.0)],
                &[("\\_tz.tz00", 301.0), ("\\_tz.thrm", 321.0)],
                &[("\\_tz.tz00", 301.0), ("\\_tz.thrm", 322.0)],
            ],
            &mut proven,
        );
        assert_eq!(shown(&zones), [row("ACPI THRM", "49\u{00B0}C", true)]);
        assert_eq!(proven, ["\\_tz.thrm"]);
    }

    #[test]
    fn implausible_zone_readings_are_ignored() {
        // 0 K and 1000 K create no zone: thrm stays the board's only zone ...
        let zones = observe(
            &[
                &[("\\_tz.thrm", 320.0), ("\\_tz.tz00", 0.0)],
                &[("\\_tz.thrm", 322.0), ("\\_tz.tz00", 1000.0)],
            ],
            &mut Vec::new(),
        );
        assert_eq!(shown(&zones), [row("ACPI Temp", "49\u{00B0}C", true)]);
        // ... and they are no movement: a steady zone stays hidden.
        let zones = observe(
            &[
                &[("\\_tz.tz00", 301.0)],
                &[("\\_tz.tz00", 0.0)],
                &[("\\_tz.tz00", 1000.0)],
                &[("\\_tz.tz00", 301.0)],
            ],
            &mut Vec::new(),
        );
        assert_eq!(shown(&zones), [row("Temp", "n/a", false)]);
    }

    #[test]
    fn proven_zone_is_shown_at_once_after_a_rebuild() {
        // A metric Apply rebuilds the source with a fresh zone list; a zone
        // that moved earlier in the process shows on its first reading.
        let mut proven = vec!["\\_tz.thrm".to_string()];
        let zones = observe(&[&[("\\_tz.thrm", 330.0)]], &mut proven);
        assert_eq!(shown(&zones), [row("ACPI Temp", "57\u{00B0}C", true)]);
        assert_eq!(proven, ["\\_tz.thrm"]);
    }

    #[test]
    fn zone_labels_name_the_acpi_zone() {
        assert_eq!(zone_label("\\_tz.thrm", false), "ACPI Temp");
        assert_eq!(zone_label("\\_tz.thrm", true), "ACPI THRM");
        // Windows trims ACPI names to their real length: the name after the
        // last '.' or '\', not a fixed four characters.
        assert_eq!(zone_label("\\_tz.tz0", true), "ACPI TZ0");
        // Odd names cannot panic: a short one, a non-ASCII one, an empty one.
        assert_eq!(zone_label("tz", true), "ACPI TZ");
        assert_eq!(zone_label("\u{00E9}abc", true), "ACPI \u{00E9}ABC");
        assert_eq!(zone_label("", true), "ACPI");
    }

    #[test]
    fn live_zone_without_a_reading_shows_na() {
        // A shown zone that misses a sample keeps its row as n/a, so the
        // widget keeps its size; it shows its reading again on the next one.
        let tz = |k: f64| [("\\_tz.thrm".to_string(), k)];
        let (mut zones, mut proven) = (Vec::new(), Vec::new());
        for k in [320.0, 322.0] {
            observe_zones(&mut zones, &tz(k), &mut proven);
        }
        observe_zones(&mut zones, &[], &mut proven);
        assert_eq!(shown(&zones), [row("ACPI Temp", "n/a", false)]);
        observe_zones(&mut zones, &tz(323.0), &mut proven);
        assert_eq!(shown(&zones), [row("ACPI Temp", "50\u{00B0}C", true)]);
    }

    #[test]
    fn duplicate_instance_names_in_one_sample_count_once() {
        // Two readings under one name in a sample would look like movement;
        // only the first counts, so a stub stays a stub.
        let dup: &[(&str, f64)] = &[("\\_tz.tz00", 301.0), ("\\_tz.tz00", 303.0)];
        let mut proven = Vec::new();
        let zones = observe(&[dup; 10], &mut proven);
        assert_eq!(shown(&zones), [row("Temp", "n/a", false)]);
        assert!(proven.is_empty());
    }

    #[test]
    fn gpu_rows_come_first_and_name_the_gpu_when_several() {
        let stub: &[(&str, f64)] = &[("\\_tz.tz00", 301.0)];
        let zones = observe(&[stub; 3], &mut Vec::new());
        // One reporting GPU is "GPU"; a GPU that never reported gets no row.
        let gpus = [gpu("RTX 4080", &[Some(52.0)]), gpu("Intel UHD 730", &[None, None])];
        assert_eq!(shown_with(&gpus, &zones), [row("GPU", "52\u{00B0}C", true)]);
        // Several reporting GPUs are named.
        let gpus = [gpu("RTX 4080", &[Some(52.0)]), gpu("Arc 140V", &[Some(48.4)])];
        assert_eq!(
            shown_with(&gpus, &zones),
            [row("GPU RTX 4080", "52\u{00B0}C", true), row("GPU Arc 140V", "48\u{00B0}C", true)]
        );
        // GPU rows come before the live zones.
        let tz = observe(&[&[("\\_tz.thrm", 320.0)], &[("\\_tz.thrm", 322.0)]], &mut Vec::new());
        let gpus = [gpu("RTX 4080", &[Some(61.0)])];
        assert_eq!(
            shown_with(&gpus, &tz),
            [row("GPU", "61\u{00B0}C", true), row("ACPI Temp", "49\u{00B0}C", true)]
        );
    }

    #[test]
    fn gpu_that_reported_once_shows_na_without_a_reading() {
        let gpus = [gpu("RTX 4080", &[Some(55.0), None])];
        assert_eq!(shown_with(&gpus, &[]), [row("GPU", "n/a", false)]);
        // A GPU that never reported adds nothing: the metric's own n/a row.
        let gpus = [gpu("Intel UHD 730", &[None, None, None])];
        assert_eq!(shown_with(&gpus, &[]), [row("Temp", "n/a", false)]);
    }
}
