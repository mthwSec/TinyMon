//! Per-core CPU usage via the vendor-neutral PDH `\Processor(*)\% Processor
//! Time` counter — one bar per logical core (the `_Total` instance is skipped).

use windows::core::w;

use crate::pdh::PdhCounter;
use crate::registry::Row;

pub struct PerCpuSampler {
    counter: PdhCounter,
    /// (core index, load %), sorted by index; `None` = this tick's reading failed.
    cores: Vec<(usize, Option<f64>)>,
}

impl PerCpuSampler {
    pub fn new() -> Self {
        PerCpuSampler {
            counter: PdhCounter::new(w!("\\Processor(*)\\% Processor Time")),
            cores: Vec::new(),
        }
    }

    pub fn sample(&mut self) {
        self.cores = next_cores(&self.cores, self.counter.try_sample());
    }

    pub fn rows(&self, out: &mut Vec<Row>) {
        core_rows(&self.cores, out);
    }
}

/// The cores of a fresh sample, sorted by index. Instance names are "0".."N-1"
/// plus "_total"; only the numbered ones count. When no numbered core comes
/// back (a failed or empty sample, or `_total` only), the known cores are kept
/// without a value, so the row count stays stable.
fn next_cores(
    prev: &[(usize, Option<f64>)],
    sample: Option<Vec<(String, f64)>>,
) -> Vec<(usize, Option<f64>)> {
    let mut cores: Vec<(usize, Option<f64>)> = sample
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(name, v)| name.parse::<usize>().ok().map(|i| (i, Some(v))))
        .collect();
    if cores.is_empty() {
        return prev.iter().map(|&(i, _)| (i, None)).collect();
    }
    cores.sort_by_key(|(i, _)| *i);
    cores
}

/// One row per core ("C3 n/a" for a core without a value), or a single
/// "Cores n/a" row when no core has been seen yet.
fn core_rows(cores: &[(usize, Option<f64>)], out: &mut Vec<Row>) {
    if cores.is_empty() {
        out.push(Row::na("Cores"));
    }
    for &(i, pct) in cores {
        out.push(match pct {
            Some(p) => percpu_row(i, p),
            None => Row::na(format!("C{}", i)),
        });
    }
}

pub fn percpu_row(core: usize, pct: f64) -> Row {
    Row::bar(format!("C{}", core), format!("{:.0}%", pct), (pct / 100.0) as f32)
}

#[cfg(test)]
mod tests {
    use super::{core_rows, next_cores, percpu_row};

    #[test]
    fn formats() {
        let r = percpu_row(3, 42.0);
        assert_eq!(r.label, "C3");
        assert_eq!(r.value, "42%");
        assert!((r.bar.unwrap() - 0.42).abs() < 1e-6);
    }

    #[test]
    fn parses_sorted_cores_without_total() {
        let sample =
            vec![("10".to_string(), 5.0), ("_total".to_string(), 7.0), ("2".to_string(), 9.0)];
        assert_eq!(next_cores(&[], Some(sample)), [(2, Some(9.0)), (10, Some(5.0))]);
    }

    #[test]
    fn failed_sample_keeps_cores_as_na() {
        let prev = [(0, Some(12.0)), (1, Some(34.0))];
        let cores = next_cores(&prev, None);
        assert_eq!(cores, [(0, None), (1, None)]);
        let mut out = Vec::new();
        core_rows(&cores, &mut out);
        let got: Vec<_> = out.iter().map(|r| (r.label.as_str(), r.value.as_str(), r.bar)).collect();
        assert_eq!(got, [("C0", "n/a", None), ("C1", "n/a", None)]);
        // A sample with only the _total instance reads no core either.
        let total_only = vec![("_total".to_string(), 5.0)];
        assert_eq!(next_cores(&prev, Some(total_only)), [(0, None), (1, None)]);
    }

    #[test]
    fn empty_sample_keeps_cores_as_na() {
        // PDH_NO_DATA (Some(empty)) reads no core, like a failed sample.
        let prev = [(0, Some(12.0)), (1, Some(34.0))];
        assert_eq!(next_cores(&prev, Some(Vec::new())), [(0, None), (1, None)]);
    }

    #[test]
    fn cores_recover_after_an_na_tick() {
        let na = [(0, None), (1, None)];
        let sample = vec![("1".to_string(), 7.0), ("0".to_string(), 5.0)];
        assert_eq!(next_cores(&na, Some(sample)), [(0, Some(5.0)), (1, Some(7.0))]);
    }

    #[test]
    fn partial_sample_replaces_the_cores() {
        // A sample that lacks some cores (try_sample drops an instance with an
        // invalid status) replaces the list instead of being merged into it, so
        // the row count follows the sample; only a sample with no numbered core
        // keeps the known cores.
        let prev = [(0, Some(12.0)), (1, Some(34.0))];
        let sample = vec![("1".to_string(), 3.0)];
        assert_eq!(next_cores(&prev, Some(sample)), [(1, Some(3.0))]);
    }

    #[test]
    fn no_cores_shows_one_na_row() {
        let mut out = Vec::new();
        core_rows(&next_cores(&[], None), &mut out);
        let got: Vec<_> = out.iter().map(|r| (r.label.as_str(), r.value.as_str(), r.bar)).collect();
        assert_eq!(got, [("Cores", "n/a", None)]);
    }
}
