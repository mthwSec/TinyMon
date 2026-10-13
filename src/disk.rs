//! Disk throughput via PDH `\PhysicalDisk(_Total)\Disk Read Bytes/sec` and
//! `\PhysicalDisk(_Total)\Disk Write Bytes/sec`, shown as separate read (R) and
//! write (W) rows.

use windows::core::w;

use crate::pdh::PdhCounter;
use crate::registry::{fmt_rate, Row};

/// Bar soft-ceiling for disk throughput (~100 MB/s).
const DISK_CEIL: f64 = 100.0 * 1024.0 * 1024.0;

pub struct DiskSampler {
    read: PdhCounter,
    write: PdhCounter,
    /// Read bytes per second; `None` when the counter could not be read or
    /// reported no `_Total` instance (`total`).
    read_bps: Option<f64>,
    /// Written bytes per second, by the same rule.
    write_bps: Option<f64>,
}

impl DiskSampler {
    pub fn new() -> Self {
        DiskSampler {
            read: PdhCounter::new(w!("\\PhysicalDisk(_Total)\\Disk Read Bytes/sec")),
            write: PdhCounter::new(w!("\\PhysicalDisk(_Total)\\Disk Write Bytes/sec")),
            read_bps: None,
            write_bps: None,
        }
    }

    pub fn sample(&mut self) {
        self.read_bps = total(self.read.try_sample());
        self.write_bps = total(self.write.try_sample());
    }

    pub fn rows(&self, out: &mut Vec<Row>) {
        out.push(disk_row("Disk R", self.read_bps));
        out.push(disk_row("Disk W", self.write_bps));
    }
}

/// The value of the single `_Total` instance. `None` when the counter could not
/// be read or reported no instance: `_Total` always exists, so an empty sample
/// is a failure, not an idle disk.
fn total(sample: Option<Vec<(String, f64)>>) -> Option<f64> {
    sample?.first().map(|(_, v)| *v)
}

/// Disk throughput row, or a value-only "n/a" row when the rate is unknown.
pub fn disk_row(label: &str, bytes_per_sec: Option<f64>) -> Row {
    match bytes_per_sec {
        Some(b) => Row::rate(label, fmt_rate(b), (b / DISK_CEIL).clamp(0.0, 1.0) as f32),
        None => Row::na(label),
    }
}

#[cfg(test)]
mod tests {
    use super::{disk_row, total};

    #[test]
    fn formats() {
        let r = disk_row("Disk R", Some(5.0 * 1024.0 * 1024.0));
        assert_eq!(r.label, "Disk R");
        assert_eq!(r.value, "5.0M/s");
        assert!((r.bar.unwrap() - 0.05).abs() < 1e-6); // 5MB / 100MB ceil
    }

    #[test]
    fn unavailable_is_na_not_zero() {
        let r = disk_row("Disk W", None);
        assert_eq!(r.value, "n/a");
        assert_eq!(r.bar, None);
    }

    #[test]
    fn total_reads_the_single_instance() {
        assert_eq!(total(Some(vec![("_total".to_string(), 42.0)])), Some(42.0));
        assert_eq!(total(Some(Vec::new())), None);
        assert_eq!(total(None), None);
    }
}
