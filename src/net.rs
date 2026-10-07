//! Network throughput via PDH `\Network Interface(*)\Bytes Sent/sec` and
//! `\Network Interface(*)\Bytes Received/sec`, summed across real adapters
//! (loopback/tunnel pseudo-interfaces are skipped), shown as separate up (↑) and
//! down (↓) rows.

use windows::core::w;

use crate::pdh::PdhCounter;
use crate::registry::{fmt_rate, Row};

/// Bar soft-ceiling: ~100 Mbit/s (12.5 MB/s).
const NET_CEIL: f64 = 12_500_000.0;

pub struct NetSampler {
    sent: PdhCounter,
    recv: PdhCounter,
    /// Bytes per second; `None` when the counter could not be read.
    up_bps: Option<f64>,
    down_bps: Option<f64>,
}

impl NetSampler {
    pub fn new() -> Self {
        NetSampler {
            sent: PdhCounter::new(w!("\\Network Interface(*)\\Bytes Sent/sec")),
            recv: PdhCounter::new(w!("\\Network Interface(*)\\Bytes Received/sec")),
            up_bps: None,
            down_bps: None,
        }
    }

    pub fn sample(&mut self) {
        self.up_bps = sum_real(self.sent.try_sample());
        self.down_bps = sum_real(self.recv.try_sample());
    }

    pub fn rows(&self, out: &mut Vec<Row>) {
        out.push(net_row("Net\u{2191}", self.up_bps)); // up
        out.push(net_row("Net\u{2193}", self.down_bps)); // down
    }
}

/// Sum a `Network Interface(*)` sample over real adapters only. `None` when the
/// counter could not be read; no instance at all is a real idle `Some(0.0)`.
fn sum_real(sample: Option<Vec<(String, f64)>>) -> Option<f64> {
    Some(sample?.iter().filter(|(name, _)| !is_pseudo(name)).map(|(_, v)| *v).sum())
}

/// Skip software/virtual interfaces so the total reflects real traffic.
fn is_pseudo(name: &str) -> bool {
    name.contains("loopback") || name.contains("isatap") || name.contains("teredo")
}

/// Network throughput row, or a value-only "n/a" row when the rate is unknown.
pub fn net_row(label: &str, bytes_per_sec: Option<f64>) -> Row {
    match bytes_per_sec {
        Some(b) => Row::rate(label, fmt_rate(b), (b / NET_CEIL).clamp(0.0, 1.0) as f32),
        None => Row::na(label),
    }
}

#[cfg(test)]
mod tests {
    use super::{is_pseudo, net_row, sum_real};

    #[test]
    fn skips_pseudo_interfaces() {
        assert!(is_pseudo("loopback pseudo-interface 1"));
        assert!(is_pseudo("teredo tunneling pseudo-interface"));
        assert!(!is_pseudo("intel(r) ethernet connection"));
    }

    #[test]
    fn formats() {
        let r = net_row("Net\u{2191}", Some(2.0 * 1024.0 * 1024.0));
        assert_eq!(r.label, "Net\u{2191}");
        assert_eq!(r.value, "2.0M/s");
    }

    #[test]
    fn unavailable_is_na_not_zero() {
        let r = net_row("Net\u{2193}", None);
        assert_eq!(r.value, "n/a");
        assert_eq!(r.bar, None);
        assert_eq!(sum_real(None), None);
        // No instance at all is a real idle reading, not a failure.
        assert_eq!(sum_real(Some(Vec::new())), Some(0.0));
        // Loopback is still filtered out of a real sample.
        let sample = vec![
            ("loopback pseudo-interface 1".to_string(), 500.0),
            ("intel(r) ethernet connection".to_string(), 7.0),
        ];
        assert_eq!(sum_real(Some(sample)), Some(7.0));
    }
}
