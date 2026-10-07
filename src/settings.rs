//! Resolved runtime settings (defaults applied + clamped), derived from the
//! on-disk `Config`. `Config` stays the pure serialization DTO; `Settings` is
//! what the running app holds and what the settings UI edits.

use crate::config::Config;
use crate::hotkeys::{format_hotkey, parse_hotkey, HotkeySpec};
use crate::registry::MetricKey;

pub const DEFAULT_OPACITY: u8 = 220;
pub const DEFAULT_INTERVAL_MS: u32 = 1000;
pub const DEFAULT_FONT_PX: i32 = 13;

// Clamp ranges (keep the widget usable: not invisible, not a CPU hog, text fits).
pub const MIN_OPACITY: u8 = 40;
pub const MIN_INTERVAL_MS: u32 = 250;
pub const MAX_INTERVAL_MS: u32 = 10_000;
pub const MIN_FONT_PX: i32 = 9;
pub const MAX_FONT_PX: i32 = 16;

#[derive(Clone, PartialEq, Debug)]
pub struct Settings {
    pub pos: Option<(i32, i32)>,
    pub opacity: u8,
    pub interval_ms: u32,
    pub font_px: i32,
    /// Enabled metrics, in display order.
    pub order: Vec<MetricKey>,
    pub hotkey_toggle: Option<HotkeySpec>,
    pub hotkey_settings: Option<HotkeySpec>,
    /// Use the light widget theme instead of the default dark one.
    pub light: bool,
}

impl Settings {
    /// The factory-default settings (what a fresh/empty config resolves to).
    pub fn defaults() -> Settings {
        Settings::from_config(&Config::default())
    }

    /// Resolve runtime settings from a parsed config, applying defaults + clamps.
    pub fn from_config(c: &Config) -> Settings {
        let order = match &c.metrics {
            Some(list) => list.iter().filter_map(|s| MetricKey::from_str(s)).collect(),
            None => MetricKey::DEFAULT.to_vec(),
        };
        Settings {
            pos: match (c.x, c.y) {
                (Some(x), Some(y)) => Some((x, y)),
                _ => None,
            },
            opacity: c.opacity.unwrap_or(DEFAULT_OPACITY).max(MIN_OPACITY),
            interval_ms: c
                .interval_ms
                .unwrap_or(DEFAULT_INTERVAL_MS)
                .clamp(MIN_INTERVAL_MS, MAX_INTERVAL_MS),
            font_px: c.font_px.unwrap_or(DEFAULT_FONT_PX).clamp(MIN_FONT_PX, MAX_FONT_PX),
            order,
            hotkey_toggle: c.hotkey_toggle.as_deref().and_then(parse_hotkey),
            hotkey_settings: c.hotkey_settings.as_deref().and_then(parse_hotkey),
            light: c.light.unwrap_or(false),
        }
    }

    /// Project back to the on-disk DTO.
    pub fn to_config(&self) -> Config {
        Config {
            x: self.pos.map(|p| p.0),
            y: self.pos.map(|p| p.1),
            metrics: Some(self.order.iter().map(|k| k.as_str().to_string()).collect()),
            opacity: Some(self.opacity),
            interval_ms: Some(self.interval_ms),
            font_px: Some(self.font_px),
            hotkey_toggle: self.hotkey_toggle.map(format_hotkey),
            hotkey_settings: self.hotkey_settings.map(format_hotkey),
            light: Some(self.light),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_reproduce_cpu_ram_gpu() {
        let s = Settings::from_config(&Config::default());
        #[cfg(feature = "temp")]
        assert_eq!(s.order, vec![MetricKey::Cpu, MetricKey::Ram, MetricKey::Gpu, MetricKey::Temp]);
        #[cfg(not(feature = "temp"))]
        assert_eq!(s.order, vec![MetricKey::Cpu, MetricKey::Ram, MetricKey::Gpu]);
        assert_eq!(s.opacity, DEFAULT_OPACITY);
        assert_eq!(s.interval_ms, DEFAULT_INTERVAL_MS);
        assert_eq!(s.font_px, DEFAULT_FONT_PX);
        assert_eq!(s.hotkey_toggle, None);
    }

    #[test]
    fn clamps_out_of_range_prefs() {
        let c = Config {
            opacity: Some(0),
            interval_ms: Some(50),
            font_px: Some(99),
            ..Default::default()
        };
        let s = Settings::from_config(&c);
        assert_eq!(s.opacity, MIN_OPACITY);
        assert_eq!(s.interval_ms, MIN_INTERVAL_MS);
        assert_eq!(s.font_px, MAX_FONT_PX);
    }

    #[test]
    fn metrics_map_and_drop_unknown() {
        let c = Config {
            metrics: Some(vec!["gpu".into(), "bogus".into(), "cpu".into()]),
            ..Default::default()
        };
        assert_eq!(Settings::from_config(&c).order, vec![MetricKey::Gpu, MetricKey::Cpu]);
    }

    #[test]
    fn full_round_trip_through_config() {
        let s = Settings::from_config(&Config {
            x: Some(10),
            y: Some(20),
            metrics: Some(vec!["ram".into(), "gpu".into()]),
            opacity: Some(200),
            interval_ms: Some(1500),
            font_px: Some(12),
            hotkey_toggle: Some("Ctrl+Alt+M".into()),
            hotkey_settings: None,
            light: Some(true),
        });
        assert_eq!(Settings::from_config(&s.to_config()), s);
    }
}
