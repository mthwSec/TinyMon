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
    read_bps: f64,
    write_bps: f64,
}

impl DiskSampler {
    pub fn new() -> Self {
        DiskSampler {
            read: PdhCounter::new(w!("\\PhysicalDisk(_Total)\\Disk Read Bytes/sec")),
            write: PdhCounter::new(w!("\\PhysicalDisk(_Total)\\Disk Write Bytes/sec")),
            read_bps: 0.0,
            write_bps: 0.0,
        }
    }

    pub fn sample(&mut self) {
        // _Total is a single instance.
        self.read_bps = self.read.sample().first().map(|(_, v)| *v).unwrap_or(0.0);
        self.write_bps = self.write.sample().first().map(|(_, v)| *v).unwrap_or(0.0);
    }

    pub fn rows(&self, out: &mut Vec<Row>) {
        out.push(disk_row("Disk R", self.read_bps));
        out.push(disk_row("Disk W", self.write_bps));
    }
}

pub fn disk_row(label: &str, bytes_per_sec: f64) -> Row {
    let frac = (bytes_per_sec / DISK_CEIL).clamp(0.0, 1.0) as f32;
    Row::bar(label, fmt_rate(bytes_per_sec), frac)
}

#[cfg(test)]
mod tests {
    use super::disk_row;

    #[test]
    fn formats() {
        let r = disk_row("Disk R", 5.0 * 1024.0 * 1024.0);
        assert_eq!(r.label, "Disk R");
        assert_eq!(r.value, "5.0M/s");
        assert!((r.bar.unwrap() - 0.05).abs() < 1e-6); // 5MB / 100MB ceil
    }
}
