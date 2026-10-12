//! The Temperature metric (Cargo feature `temp`, on by default): the ACPI
//! thermal zones Windows publishes through the PDH counter
//! `\Thermal Zone Information(*)\Temperature`, read without admin rights or
//! extra software. The counter reports kelvin; readings outside 1-150 C are
//! dropped, and with no reading left the metric shows one "Temp n/a" row, so
//! an enabled metric stays visible.

use windows::core::w;

use crate::pdh::PdhCounter;
use crate::registry::Row;

pub struct TempSource {
    /// The ACPI thermal zones Windows exposes through PDH (no admin).
    zone: PdhCounter,
    /// The last sample's plausible readings, in degrees Celsius.
    native: Vec<f64>,
}

impl TempSource {
    pub fn new() -> Self {
        TempSource {
            zone: PdhCounter::new(w!("\\Thermal Zone Information(*)\\Temperature")),
            native: Vec::new(),
        }
    }

    pub fn sample(&mut self) {
        // Native ACPI thermal-zone temps (the counter reports Kelvin); keep only
        // plausible readings so a bogus/zero zone never shows a wild value.
        self.native = self
            .zone
            .sample()
            .iter()
            .map(|(_, k)| k - 273.15)
            .filter(|c| (1.0..150.0).contains(c))
            .collect();
    }

    pub fn rows(&self, out: &mut Vec<Row>) {
        if !self.native.is_empty() {
            // Windows' ACPI thermal zone(s), one row each.
            let one = self.native.len() == 1;
            for (i, c) in self.native.iter().enumerate() {
                let label = if one { "CPU Temp".to_string() } else { format!("Temp {}", i + 1) };
                out.push(temp_row(&label, *c));
            }
        } else {
            // No plausible thermal zone. Keep the metric visible.
            out.push(Row {
                label: "Temp".into(),
                value: "n/a".into(),
                bar: None,
                color: None,
                bar_color: None,
                alert: false,
            });
        }
    }
}

pub fn temp_row(name: &str, celsius: f64) -> Row {
    Row::bar(name, format!("{:.0}\u{00B0}C", celsius), (celsius / 100.0).clamp(0.0, 1.0) as f32)
}

#[cfg(test)]
mod tests {
    use super::{temp_row, TempSource};

    #[test]
    fn formats() {
        let r = temp_row("CPU", 55.0);
        assert_eq!(r.label, "CPU");
        assert_eq!(r.value, "55\u{00B0}C");
        assert!((r.bar.unwrap() - 0.55).abs() < 1e-6);
    }

    #[test]
    fn zone_rows_keep_their_labels() {
        // The thermal zones are the metric's only source: one zone reads
        // "CPU Temp", several read "Temp 1", "Temp 2", ..., and with none the
        // metric keeps one value-only "Temp n/a" row.
        fn shown(src: &TempSource) -> Vec<(String, String, bool)> {
            let mut out = Vec::new();
            src.rows(&mut out);
            out.into_iter().map(|r| (r.label, r.value, r.bar.is_some())).collect()
        }
        let row = |label: &str, value: &str, bar| (label.to_string(), value.to_string(), bar);
        let mut src = TempSource::new();
        src.native = Vec::new();
        assert_eq!(shown(&src), [row("Temp", "n/a", false)]);
        src.native = vec![45.0];
        assert_eq!(shown(&src), [row("CPU Temp", "45\u{00B0}C", true)]);
        src.native = vec![30.0, 40.0];
        assert_eq!(
            shown(&src),
            [row("Temp 1", "30\u{00B0}C", true), row("Temp 2", "40\u{00B0}C", true)]
        );
    }
}
