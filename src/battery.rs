//! Battery metric: charge percent + AC/charging state (+ time remaining), read
//! via `GetSystemPowerStatus`. No admin, no extra runtime deps. On a desktop
//! with no battery it reports "n/a". Stateless — re-read each tick (like RAM).
//!
//! Rendered with a charge-level bar colored full→empty = green→red (the inverse
//! of the normal `load_color`, so a healthy 100% reads green, not the warning red
//! a load bar would paint). Value-only "n/a" when there's no battery.

use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

use crate::registry::Row;

// SYSTEM_POWER_STATUS sentinels.
const FLAG_CHARGING: u8 = 8;
const FLAG_NO_BATTERY: u8 = 128;
const PERCENT_UNKNOWN: u8 = 255;
const AC_ONLINE: u8 = 1;
const TIME_UNKNOWN: u32 = u32::MAX;

/// Parsed battery snapshot (the fields the row formats from).
#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct BatteryInfo {
    pub present: bool,
    pub ac_online: bool,
    pub charging: bool,
    /// 0..=100, meaningful only when `present`.
    pub percent: u8,
    /// Seconds of runtime left on battery, if the OS reports it.
    pub secs_left: Option<u32>,
}

/// Read the current battery status. Best-effort: on failure reports "not present".
pub fn read_battery() -> BatteryInfo {
    let mut s = SYSTEM_POWER_STATUS::default();
    // SAFETY: GetSystemPowerStatus fills the struct we own; no pointers retained.
    if unsafe { GetSystemPowerStatus(&mut s) }.is_err() {
        return BatteryInfo::default();
    }
    let present = s.BatteryFlag & FLAG_NO_BATTERY == 0 && s.BatteryLifePercent != PERCENT_UNKNOWN;
    BatteryInfo {
        present,
        ac_online: s.ACLineStatus == AC_ONLINE,
        charging: s.BatteryFlag & FLAG_CHARGING != 0,
        percent: if present { s.BatteryLifePercent.min(100) } else { 0 },
        secs_left: (s.BatteryLifeTime != TIME_UNKNOWN).then_some(s.BatteryLifeTime),
    }
}

/// Format the battery value string (pure; unit-tested).
pub fn battery_value(info: &BatteryInfo) -> String {
    if !info.present {
        return "n/a".into();
    }
    let p = info.percent;
    if info.charging {
        format!("{p}% chg")
    } else if info.ac_online {
        format!("{p}% AC")
    } else if let Some(secs) = info.secs_left.filter(|&s| s > 0) {
        format!("{p}% {}:{:02}", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{p}%")
    }
}

/// Battery row: a charge-level bar (green when full → red when low), or a
/// value-only "n/a" row when no battery is present.
pub fn battery_row(info: &BatteryInfo) -> Row {
    if !info.present {
        return Row::na("Bat");
    }
    let value = battery_value(info);
    let frac = (info.percent as f32 / 100.0).clamp(0.0, 1.0);
    // Color by charge remaining: 100% -> green, 0% -> red (load_color inverted).
    // A high charge is good news, so the row never raises the red label alert.
    let charge_color = crate::metrics::load_color(1.0 - frac);
    Row {
        label: "Bat".into(),
        value,
        bar: Some(frac),
        color: None,
        bar_color: Some(charge_color),
        alert: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(present: bool, ac: bool, chg: bool, pct: u8, secs: Option<u32>) -> BatteryInfo {
        BatteryInfo { present, ac_online: ac, charging: chg, percent: pct, secs_left: secs }
    }

    #[test]
    fn no_battery_is_na() {
        assert_eq!(battery_value(&info(false, true, false, 0, None)), "n/a");
    }

    #[test]
    fn no_battery_row_is_value_only() {
        let r = battery_row(&info(false, true, false, 0, None));
        assert_eq!((r.label.as_str(), r.value.as_str()), ("Bat", "n/a"));
        assert_eq!((r.bar, r.color, r.bar_color, r.alert), (None, None, None, false));
    }

    #[test]
    fn charging_and_ac_states() {
        assert_eq!(battery_value(&info(true, true, true, 72, None)), "72% chg");
        assert_eq!(battery_value(&info(true, true, false, 100, None)), "100% AC");
    }

    #[test]
    fn on_battery_shows_time_then_falls_back() {
        // 9000s = 2h30m
        assert_eq!(battery_value(&info(true, false, false, 85, Some(9000))), "85% 2:30");
        // unknown / zero time -> just the percent
        assert_eq!(battery_value(&info(true, false, false, 85, None)), "85%");
        assert_eq!(battery_value(&info(true, false, false, 85, Some(0))), "85%");
    }
}
