//! Per-core CPU usage via the vendor-neutral PDH `\Processor(*)\% Processor
//! Time` counter — one bar per logical core (the `_Total` instance is skipped).

use windows::core::w;

use crate::pdh::PdhCounter;
use crate::registry::Row;

pub struct PerCpuSampler {
    counter: PdhCounter,
    cores: Vec<(usize, f64)>,
}

impl PerCpuSampler {
    pub fn new() -> Self {
        PerCpuSampler {
            counter: PdhCounter::new(w!("\\Processor(*)\\% Processor Time")),
            cores: Vec::new(),
        }
    }

    pub fn sample(&mut self) {
        // Instance names are "0".."N-1" plus "_total"; keep only the numbered ones.
        let mut cores: Vec<(usize, f64)> = self
            .counter
            .sample()
            .into_iter()
            .filter_map(|(name, v)| name.parse::<usize>().ok().map(|i| (i, v)))
            .collect();
        cores.sort_by_key(|(i, _)| *i);
        self.cores = cores;
    }

    pub fn rows(&self, out: &mut Vec<Row>) {
        for (i, pct) in &self.cores {
            out.push(percpu_row(*i, *pct));
        }
    }
}

pub fn percpu_row(core: usize, pct: f64) -> Row {
    Row::bar(format!("C{}", core), format!("{:.0}%", pct), (pct / 100.0) as f32)
}

#[cfg(test)]
mod tests {
    use super::percpu_row;

    #[test]
    fn formats() {
        let r = percpu_row(3, 42.0);
        assert_eq!(r.label, "C3");
        assert_eq!(r.value, "42%");
        assert!((r.bar.unwrap() - 0.42).abs() < 1e-6);
    }
}
