//! Sparkline history, kept per row identity instead of per row position.
//!
//! A row is known by its metric, its label and the number of earlier rows of
//! the same tick that share both (`RowId`), so its series follows it when
//! another metric adds or drops rows, when the order changes and across Apply.
//! Only WM_TIMER records (`History::record`); painting reads one slice per row
//! (`History::get`).

use crate::registry::{MetricKey, Row};

/// Samples kept per row for the sparklines (about a minute at a 1 s interval).
pub const HIST_LEN: usize = 64;

/// A row's identity from tick to tick: its metric, its label, and `nth`, the
/// number of earlier rows of the same tick with the same metric and label
/// (two sensors with one name keep two series).
#[derive(Clone, PartialEq, Debug)]
pub struct RowId {
    pub metric: MetricKey,
    pub label: String,
    pub nth: u16,
}

/// The identity of every row of one tick, where `keys[i]` is the metric of
/// `rows[i]` (as `registry::collect_rows` returns them).
pub fn row_ids(keys: &[MetricKey], rows: &[Row]) -> Vec<RowId> {
    let mut ids: Vec<RowId> = Vec::with_capacity(rows.len());
    for (&metric, row) in keys.iter().zip(rows) {
        let nth = ids.iter().filter(|id| id.metric == metric && id.label == row.label).count();
        ids.push(RowId { metric, label: row.label.clone(), nth: nth as u16 });
    }
    ids
}

/// The recent bar values of each row, oldest first, keyed by `RowId`.
#[derive(Default)]
pub struct History {
    series: Vec<(RowId, Vec<f32>)>,
}

impl History {
    /// Record one tick. A row with a bar appends its value (keeping the last
    /// HIST_LEN); a row without one (n/a) leaves its series as it is, so the
    /// sparkline pauses instead of dipping to zero; rows with a fixed bar color
    /// (battery) never draw a sparkline and are not recorded. Series of rows
    /// that are not in this tick are dropped.
    pub fn record(&mut self, ids: &[RowId], rows: &[Row]) {
        self.series.retain(|(id, _)| ids.contains(id));
        for (id, row) in ids.iter().zip(rows) {
            let Some(v) = row.bar else { continue };
            if row.bar_color.is_some() {
                continue;
            }
            match self.series.iter_mut().find(|(s, _)| s == id) {
                Some((_, s)) => {
                    if s.len() == HIST_LEN {
                        s.remove(0);
                    }
                    s.push(v);
                }
                None => self.series.push((id.clone(), vec![v])),
            }
        }
    }

    /// The recorded values of row `id`, oldest first (empty for an unknown row).
    pub fn get(&self, id: &RowId) -> &[f32] {
        self.series.iter().find(|(s, _)| s == id).map(|(_, v)| v.as_slice()).unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::{row_ids, History, RowId, HIST_LEN};
    use crate::battery::{battery_row, BatteryInfo};
    use crate::registry::{MetricKey, Row};

    /// A row labelled `label` with bar `bar` (`None`: an n/a row, no bar).
    fn row(label: &str, bar: Option<f32>) -> Row {
        match bar {
            Some(f) => Row::bar(label, "", f),
            None => Row::na(label),
        }
    }

    /// Record one tick of (metric, label, bar) rows.
    fn tick(h: &mut History, rows: &[(MetricKey, &str, Option<f32>)]) {
        let keys: Vec<MetricKey> = rows.iter().map(|r| r.0).collect();
        let built: Vec<Row> = rows.iter().map(|r| row(r.1, r.2)).collect();
        h.record(&row_ids(&keys, &built), &built);
    }

    fn id(metric: MetricKey, label: &str, nth: u16) -> RowId {
        RowId { metric, label: label.into(), nth }
    }

    #[cfg(feature = "temp")]
    #[test]
    fn row_count_change_in_one_metric_keeps_other_histories() {
        use MetricKey::{Cpu, Temp};
        let mut h = History::default();
        tick(&mut h, &[(Cpu, "CPU", Some(0.1)), (Temp, "A", Some(0.5)), (Temp, "B", Some(0.6))]);
        tick(&mut h, &[(Cpu, "CPU", Some(0.2)), (Temp, "A", Some(0.5))]);
        assert_eq!(h.get(&id(Cpu, "CPU", 0)), [0.1, 0.2]);
        assert_eq!(h.get(&id(Temp, "A", 0)), [0.5, 0.5]);
        assert!(h.get(&id(Temp, "B", 0)).is_empty());
    }

    #[cfg(feature = "temp")]
    #[test]
    fn bar_less_rows_record_nothing() {
        use MetricKey::Temp;
        let mut h = History::default();
        tick(&mut h, &[(Temp, "Temp", None)]);
        assert!(h.get(&id(Temp, "Temp", 0)).is_empty());
        // The first real reading starts the series: no 0.0 dip before it.
        tick(&mut h, &[(Temp, "ACPI Temp", Some(0.48))]);
        assert_eq!(h.get(&id(Temp, "ACPI Temp", 0)), [0.48]);
    }

    #[test]
    fn na_tick_pauses_series_without_dropping_it() {
        use MetricKey::Gpu;
        let mut h = History::default();
        tick(&mut h, &[(Gpu, "GPU", Some(0.2))]);
        tick(&mut h, &[(Gpu, "GPU", Some(0.3))]);
        tick(&mut h, &[(Gpu, "GPU", None)]);
        tick(&mut h, &[(Gpu, "GPU", Some(0.4))]);
        assert_eq!(h.get(&id(Gpu, "GPU", 0)), [0.2, 0.3, 0.4]);
    }

    #[cfg(feature = "temp")]
    #[test]
    fn duplicate_labels_keep_separate_series() {
        use MetricKey::Temp;
        let keys = [Temp, Temp];
        let rows = [row("Temperature", Some(0.3)), row("Temperature", Some(0.7))];
        let ids = row_ids(&keys, &rows);
        assert_eq!((ids[0].nth, ids[1].nth), (0, 1));
        let mut h = History::default();
        h.record(&ids, &rows);
        h.record(&ids, &rows);
        assert_eq!(h.get(&ids[0]), [0.3, 0.3]);
        assert_eq!(h.get(&ids[1]), [0.7, 0.7]);
    }

    #[test]
    fn reorder_keeps_histories() {
        use MetricKey::{Cpu, Ram};
        let mut h = History::default();
        tick(&mut h, &[(Cpu, "CPU", Some(0.1)), (Ram, "RAM", Some(0.5))]);
        tick(&mut h, &[(Ram, "RAM", Some(0.6)), (Cpu, "CPU", Some(0.2))]);
        assert_eq!(h.get(&id(Cpu, "CPU", 0)).len(), 2);
        assert_eq!(h.get(&id(Ram, "RAM", 0)).len(), 2);
    }

    #[test]
    fn history_is_capped_at_hist_len() {
        let mut h = History::default();
        for i in 0..70 {
            tick(&mut h, &[(MetricKey::Cpu, "CPU", Some(i as f32))]);
        }
        let s = h.get(&id(MetricKey::Cpu, "CPU", 0));
        assert_eq!(HIST_LEN, 64);
        assert_eq!(s.len(), HIST_LEN);
        assert_eq!(s[0], 6.0); // the 7th recorded value
    }

    #[test]
    fn fixed_color_rows_are_not_recorded() {
        let info = BatteryInfo {
            present: true,
            ac_online: false,
            charging: false,
            percent: 72,
            secs_left: None,
        };
        let rows = [battery_row(&info)];
        let ids = row_ids(&[MetricKey::Battery], &rows);
        let mut h = History::default();
        for _ in 0..3 {
            h.record(&ids, &rows);
        }
        assert!(h.get(&ids[0]).is_empty());
    }
}
