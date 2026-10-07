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
    up_bps: f64,
    down_bps: f64,
}

impl NetSampler {
    pub fn new() -> Self {
        NetSampler {
            sent: PdhCounter::new(w!("\\Network Interface(*)\\Bytes Sent/sec")),
            recv: PdhCounter::new(w!("\\Network Interface(*)\\Bytes Received/sec")),
            up_bps: 0.0,
            down_bps: 0.0,
        }
    }

    pub fn sample(&mut self) {
        self.up_bps = sum_real(&mut self.sent);
        self.down_bps = sum_real(&mut self.recv);
    }

    pub fn rows(&self, out: &mut Vec<Row>) {
        out.push(net_row("Net\u{2191}", self.up_bps)); // up
        out.push(net_row("Net\u{2193}", self.down_bps)); // down
    }
}

/// Sum a `Network Interface(*)` counter over real adapters only.
fn sum_real(counter: &mut PdhCounter) -> f64 {
    counter.sample().iter().filter(|(name, _)| !is_pseudo(name)).map(|(_, v)| *v).sum()
}

/// Skip software/virtual interfaces so the total reflects real traffic.
fn is_pseudo(name: &str) -> bool {
    name.contains("loopback") || name.contains("isatap") || name.contains("teredo")
}

pub fn net_row(label: &str, bytes_per_sec: f64) -> Row {
    let frac = (bytes_per_sec / NET_CEIL).clamp(0.0, 1.0) as f32;
    Row::bar(label, fmt_rate(bytes_per_sec), frac)
}

#[cfg(test)]
mod tests {
    use super::{is_pseudo, net_row};

    #[test]
    fn skips_pseudo_interfaces() {
        assert!(is_pseudo("loopback pseudo-interface 1"));
        assert!(is_pseudo("teredo tunneling pseudo-interface"));
        assert!(!is_pseudo("intel(r) ethernet connection"));
    }

    #[test]
    fn formats() {
        let r = net_row("Net\u{2191}", 2.0 * 1024.0 * 1024.0);
        assert_eq!(r.label, "Net\u{2191}");
        assert_eq!(r.value, "2.0M/s");
    }
}
