//! Network throughput, shown as separate up and down rows, from the octet
//! counters of GetIfTable2 (the documented source the PDH network counters are
//! filled from). Only hardware NICs count, so a VPN adapter or virtual switch
//! that carries the same bytes again is not added twice. Rates are octet
//! deltas per interface over QueryPerformanceCounter time.

use core::ptr::addr_of;

use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetIfTable2, MIB_IF_ROW2, MIB_IF_TABLE2,
};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

use crate::registry::{fmt_rate, Row};

/// Bar soft-ceiling: ~100 Mbit/s (12.5 MB/s).
const NET_CEIL: f64 = 12_500_000.0;

// Bits of MIB_IF_ROW2.InterfaceAndOperStatusFlags.
const IF_HARDWARE: u8 = 1;
const IF_FILTER: u8 = 2;
const IF_CONNECTOR: u8 = 4;

/// (interface LUID, InOctets, OutOctets) of one interface.
type IfOctets = (u64, u64, u64);

pub struct NetSampler {
    /// The last read: its time in seconds and the counted interfaces' octets;
    /// `None` when it failed or found nothing to count.
    prev: Option<(f64, Vec<IfOctets>)>,
    /// Bytes per second; `None` when the rate is unknown.
    up_bps: Option<f64>,
    down_bps: Option<f64>,
}

impl NetSampler {
    pub fn new() -> Self {
        NetSampler { prev: snapshot(), up_bps: None, down_bps: None }
    }

    pub fn sample(&mut self) {
        let cur = snapshot();
        let (down, up) = match (&self.prev, &cur) {
            (Some((t0, prev)), Some((t1, now))) => {
                let (rx, tx) = octet_delta(prev, now);
                (rate(rx, t1 - t0), rate(tx, t1 - t0))
            }
            _ => (None, None),
        };
        self.down_bps = down;
        self.up_bps = up;
        self.prev = cur;
    }

    pub fn rows(&self, out: &mut Vec<Row>) {
        out.push(net_row("Net\u{2191}", self.up_bps)); // up
        out.push(net_row("Net\u{2193}", self.down_bps)); // down
    }
}

/// Read the counted interfaces with the time of the read. The clock is read
/// just before the table, the same way for `new` and every `sample`.
fn snapshot() -> Option<(f64, Vec<IfOctets>)> {
    let t = now_secs();
    Some((t, pick_nics(&read_if_rows()?)?))
}

/// Every interface's status flags and octet counters, from GetIfTable2.
/// `None` when the call fails.
fn read_if_rows() -> Option<Vec<(u8, IfOctets)>> {
    let mut table: *mut MIB_IF_TABLE2 = core::ptr::null_mut();
    // SAFETY: GetIfTable2 allocates the table and hands it back in `table`;
    // NumEntries rows follow the header. The rows are read through a raw
    // pointer to the first one (never through the declared one-element array),
    // and FreeMibTable releases the table exactly once.
    unsafe {
        if GetIfTable2(&mut table).is_err() || table.is_null() {
            return None;
        }
        let first = addr_of!((*table).Table).cast::<MIB_IF_ROW2>();
        let rows = (0..(*table).NumEntries as usize)
            .map(|i| {
                let r = &*first.add(i);
                let flags = r.InterfaceAndOperStatusFlags._bitfield;
                (flags, (r.InterfaceLuid.Value, r.InOctets, r.OutOctets))
            })
            .collect();
        FreeMibTable(table as *const _);
        Some(rows)
    }
}

/// The interfaces to count: hardware interfaces that are not filter modules;
/// if there are none (a VM guest's synthetic NIC may not report itself as
/// hardware), the non-filter interfaces with a connector present; otherwise
/// `None`, which shows n/a instead of a silent 0. VPN adapters (TAP, Wintun)
/// report neither flag, so they never count.
fn pick_nics(rows: &[(u8, IfOctets)]) -> Option<Vec<IfOctets>> {
    for want in [IF_HARDWARE, IF_CONNECTOR] {
        let nics: Vec<IfOctets> = rows
            .iter()
            .filter(|(flags, _)| flags & want != 0 && flags & IF_FILTER == 0)
            .map(|(_, octets)| *octets)
            .collect();
        if !nics.is_empty() {
            return Some(nics);
        }
    }
    None
}

/// Octets moved between two reads, (in, out), summed over the interfaces
/// present in both. An interface that appeared, or whose counter went back
/// (a reset), adds 0 instead of a spike.
fn octet_delta(prev: &[IfOctets], cur: &[IfOctets]) -> (u64, u64) {
    let mut sum = (0u64, 0u64);
    for &(luid, rx, tx) in cur {
        if let Some(&(_, prx, ptx)) = prev.iter().find(|p| p.0 == luid) {
            sum.0 = sum.0.saturating_add(rx.saturating_sub(prx));
            sum.1 = sum.1.saturating_add(tx.saturating_sub(ptx));
        }
    }
    sum
}

/// Bytes per second over `secs`; `None` without elapsed time.
fn rate(bytes: u64, secs: f64) -> Option<f64> {
    (secs > 0.0).then(|| bytes as f64 / secs)
}

/// Seconds on the QueryPerformanceCounter clock (std's Instant would add
/// about 8 KB to the exe).
fn now_secs() -> f64 {
    let (mut count, mut freq) = (0i64, 0i64);
    // SAFETY: both calls only write the i64 they are given.
    unsafe {
        let _ = QueryPerformanceCounter(&mut count);
        let _ = QueryPerformanceFrequency(&mut freq);
    }
    count as f64 / freq.max(1) as f64
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
    use super::{net_row, octet_delta, pick_nics, rate};

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
    }

    #[test]
    fn falls_back_to_connector_nics_only_when_no_hardware_nic() {
        // Flags: bit 0 hardware interface, bit 1 filter module, bit 2 connector present.
        let row = |flags: u8, luid: u64| (flags, (luid, 0, 0));
        let all = [row(0b101, 1), row(0b010, 2), row(0b100, 3), row(0b000, 4)];
        assert_eq!(pick_nics(&all), Some(vec![(1, 0, 0)]));
        assert_eq!(pick_nics(&all[1..]), Some(vec![(3, 0, 0)]));
        // Filter modules never count, and nothing to count is n/a, not 0.
        assert_eq!(pick_nics(&[row(0b110, 5), row(0b000, 6)]), None);
        assert_eq!(pick_nics(&[row(0b010, 7), row(0b000, 8)]), None);
    }

    #[test]
    fn delta_sums_interfaces_seen_twice() {
        let prev = [(1, 100, 50), (2, 1000, 10)];
        let cur = [(1, 150, 80), (2, 1500, 10)];
        assert_eq!(octet_delta(&prev, &cur), (550, 30));
    }

    #[test]
    fn delta_ignores_new_and_reset_interfaces() {
        // Interface 1's in-counter went back (a reset); interface 3 is new.
        let prev = [(1, 100, 50)];
        let cur = [(1, 90, 60), (3, 5000, 5000)];
        assert_eq!(octet_delta(&prev, &cur), (0, 10));
    }

    #[test]
    fn rate_needs_elapsed_time() {
        assert_eq!(rate(1000, 0.5), Some(2000.0));
        assert_eq!(rate(5, 0.0), None);
    }
}
