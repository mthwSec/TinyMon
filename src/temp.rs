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
}

impl TempSource {
    pub fn new() -> Self {
        TempSource {
            zone: PdhCounter::new(w!("\\Thermal Zone Information(*)\\Temperature")),
            zones: Vec::new(),
        }
    }

    pub fn sample(&mut self) {
        let sample = self.zone.sample();
        PROVEN_ZONES.with(|p| observe_zones(&mut self.zones, &sample, &mut p.borrow_mut()));
    }

    pub fn rows(&self, out: &mut Vec<Row>) {
        build_rows(&self.zones, out);
    }
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
        z.now = Some(*k);
        if !z.live && (k - z.first).abs() >= 0.5 {
            z.live = true;
            proven.push(name.clone());
        }
    }
}

/// A zone's label: "ACPI Temp" for a board's only zone; with several, "ACPI"
/// and the zone's 4-character ACPI name, the end of its instance name
/// (`\_tz.thrm` reads "ACPI THRM"). Nothing says which part of the machine a
/// zone measures, so it is never "CPU Temp".
fn zone_label(instance: &str, several: bool) -> String {
    if !several {
        return "ACPI Temp".into();
    }
    // `get`, not slicing: a name whose last 4 bytes start inside a character
    // is taken whole instead of panicking.
    let name = instance.get(instance.len().saturating_sub(4)..).unwrap_or(instance);
    format!("ACPI {name}").to_ascii_uppercase()
}

/// The shown zones as (label, degrees Celsius), in the order first seen: the
/// live zones with a reading in the last sample. Every zone seen counts
/// towards "several", so a real zone next to a hidden stub keeps its name.
fn live_zone_rows(zones: &[Zone]) -> Vec<(String, f64)> {
    let several = zones.len() > 1;
    zones
        .iter()
        .filter(|z| z.live)
        .filter_map(|z| z.now.map(|k| (zone_label(&z.name, several), k - 273.15)))
        .collect()
}

/// The metric's rows, all put together here: one per shown zone, else one
/// value-only "Temp n/a" row, so an enabled metric stays visible. Another
/// temperature source (a discrete GPU's, say) would add its rows here.
fn build_rows(zones: &[Zone], out: &mut Vec<Row>) {
    let rows = live_zone_rows(zones);
    if rows.is_empty() {
        out.push(Row::na("Temp"));
    }
    for (label, celsius) in rows {
        out.push(temp_row(&label, celsius));
    }
}

pub fn temp_row(name: &str, celsius: f64) -> Row {
    Row::bar(name, format!("{:.0}\u{00B0}C", celsius), (celsius / 100.0).clamp(0.0, 1.0) as f32)
}

#[cfg(test)]
mod tests {
    use super::{build_rows, live_zone_rows, observe_zones, temp_row, zone_label, Zone};

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
        let mut out = Vec::new();
        build_rows(zones, &mut out);
        out.into_iter().map(|r| (r.label, r.value, r.bar.is_some())).collect()
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
        assert!((live[0].1 - 44.85).abs() < 1e-9);
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
        // An odd name cannot panic: a short one, and one whose last four
        // bytes start inside a character.
        assert_eq!(zone_label("tz", true), "ACPI TZ");
        assert_eq!(zone_label("\u{00E9}abc", true), "ACPI \u{00E9}ABC");
    }
}
