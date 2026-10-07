//! Portable config: save/restore window position + settings.
//!
//! Location strategy: prefer `tinymon.cfg` next to the .exe (USB-portable); if
//! that folder isn't writable, fall back to `%APPDATA%\TinyMon\tinymon.cfg`.
//! Writability is decided by an actual test write, never guessed — so the same
//! .exe runs from a read-only USB stick, Program Files, or a normal folder.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Persisted settings (the on-disk DTO). Unknown keys are ignored on read, so
/// configs stay forward/backward compatible across versions.
#[derive(Debug, Default, PartialEq)]
pub struct Config {
    pub x: Option<i32>,
    pub y: Option<i32>,
    /// Ordered enabled metric keys, e.g. ["cpu","ram","gpu"]. `Some(empty)`
    /// means "all hidden"; `None` means "unset -> use defaults".
    pub metrics: Option<Vec<String>>,
    pub opacity: Option<u8>,
    pub interval_ms: Option<u32>,
    pub font_px: Option<i32>,
    /// Hotkeys as text, e.g. "Ctrl+Alt+M".
    pub hotkey_toggle: Option<String>,
    pub hotkey_settings: Option<String>,
    /// Use the light widget theme (`true`) instead of dark.
    pub light: Option<bool>,
}

/// Parse the tiny `key=value` config text. Blank lines, whole-line `#` comments
/// and trailing `# ...` comments are ignored, as is whitespace around keys and
/// values. No value TinyMon reads can contain `#` (numbers, booleans, metric
/// tokens, and hotkeys as `format_hotkey` writes them).
pub fn parse(text: &str) -> Config {
    let mut cfg = Config::default();
    // Tolerate a UTF-8 BOM: editors (e.g. Notepad saving "UTF-8 with BOM")
    // prepend U+FEFF, which would otherwise corrupt the first key.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            let val = strip_comment(val);
            match key.trim() {
                "x" => cfg.x = val.parse().ok(),
                "y" => cfg.y = val.parse().ok(),
                "metrics" => {
                    cfg.metrics = Some(
                        val.split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect(),
                    )
                }
                "opacity" => cfg.opacity = val.parse().ok(),
                "interval_ms" => cfg.interval_ms = val.parse().ok(),
                "font_px" => cfg.font_px = val.parse().ok(),
                "hotkey_toggle" => cfg.hotkey_toggle = Some(val.to_string()),
                "hotkey_settings" => cfg.hotkey_settings = Some(val.to_string()),
                "light" => cfg.light = val.parse().ok(),
                _ => {}
            }
        }
    }
    cfg
}

/// A value without its trailing comment: the text before the first `#`, trimmed.
fn strip_comment(val: &str) -> &str {
    val.split_once('#').map_or(val, |(v, _)| v).trim()
}

/// Serialize to `key=value` text (omits unset fields).
pub fn serialize(cfg: &Config) -> String {
    let mut s = String::new();
    if let Some(x) = cfg.x {
        s.push_str(&format!("x={}\n", x));
    }
    if let Some(y) = cfg.y {
        s.push_str(&format!("y={}\n", y));
    }
    if let Some(m) = &cfg.metrics {
        s.push_str(&format!("metrics={}\n", m.join(",")));
    }
    if let Some(v) = cfg.opacity {
        s.push_str(&format!("opacity={}\n", v));
    }
    if let Some(v) = cfg.interval_ms {
        s.push_str(&format!("interval_ms={}\n", v));
    }
    if let Some(v) = cfg.font_px {
        s.push_str(&format!("font_px={}\n", v));
    }
    if let Some(v) = &cfg.hotkey_toggle {
        s.push_str(&format!("hotkey_toggle={}\n", v));
    }
    if let Some(v) = &cfg.hotkey_settings {
        s.push_str(&format!("hotkey_settings={}\n", v));
    }
    if let Some(v) = cfg.light {
        s.push_str(&format!("light={}\n", v));
    }
    s
}

/// Load config, reading whichever existing file was written **most recently**.
/// This guarantees read-after-write across restarts no matter which location the
/// last save landed in (the .exe folder when writable, else `%APPDATA%`) — a
/// stale `tinymon.cfg` next to the .exe can never shadow a newer `%APPDATA%` one,
/// and the choice doesn't depend on a flaky writability probe at read time.
pub fn load() -> Config {
    let newest = read_candidates()
        .into_iter()
        .filter_map(|p| Some((fs::metadata(&p).ok()?.modified().ok()?, p)))
        .max_by_key(|(mtime, _)| *mtime)
        .map(|(_, p)| p);
    if let Some(path) = newest {
        if let Ok(text) = fs::read_to_string(&path) {
            return parse(&text);
        }
    }
    Config::default()
}

/// Resolve where to *write* the config: the .exe folder if writable (decided by
/// a real test write), else `%APPDATA%\TinyMon` (created if needed). `None` if
/// no writable location exists.
pub fn resolve_write_path() -> Option<PathBuf> {
    if let Some(dir) = exe_dir() {
        if dir_is_writable(&dir) {
            return Some(dir.join("tinymon.cfg"));
        }
    }
    if let Some(dir) = appdata_dir() {
        let _ = fs::create_dir_all(&dir);
        if dir_is_writable(&dir) {
            return Some(dir.join("tinymon.cfg"));
        }
    }
    None
}

/// Write the config to `path` (best-effort; errors are ignored).
pub fn write(path: &Path, cfg: &Config) {
    let _ = fs::write(path, serialize(cfg));
}

// ---- internals -------------------------------------------------------------

fn read_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(dir) = exe_dir() {
        v.push(dir.join("tinymon.cfg"));
    }
    if let Some(dir) = appdata_dir() {
        v.push(dir.join("tinymon.cfg"));
    }
    v
}

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(Path::to_path_buf)
}

fn appdata_dir() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("TinyMon"))
}

fn dir_is_writable(dir: &Path) -> bool {
    let probe = dir.join(".tinymon_write_test");
    let ok = match fs::File::create(&probe) {
        Ok(mut f) => f.write_all(b"ok").is_ok(),
        Err(_) => false,
    };
    let _ = fs::remove_file(&probe);
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let c = parse("x=100\ny=24\n");
        assert_eq!(c.x, Some(100));
        assert_eq!(c.y, Some(24));
    }

    #[test]
    fn parse_tolerates_comments_blanks_ws() {
        let c = parse("# comment\n\n  x = -5 \ny=  17\nbogus line\n");
        assert_eq!(c.x, Some(-5));
        assert_eq!(c.y, Some(17));
    }

    #[test]
    fn empty_is_default() {
        assert_eq!(parse(""), Config::default());
    }

    #[test]
    fn metrics_round_trip() {
        let c = Config { metrics: Some(vec!["cpu".into(), "gpu".into()]), ..Default::default() };
        assert_eq!(serialize(&c), "metrics=cpu,gpu\n");
        assert_eq!(parse(&serialize(&c)), c);
    }

    #[test]
    fn prefs_round_trip() {
        let c = Config {
            opacity: Some(180),
            interval_ms: Some(2000),
            font_px: Some(14),
            hotkey_toggle: Some("Ctrl+Alt+M".into()),
            hotkey_settings: Some("Ctrl+Alt+S".into()),
            light: Some(true),
            ..Default::default()
        };
        assert_eq!(parse(&serialize(&c)), c);
    }

    #[test]
    fn unknown_keys_ignored() {
        let c = parse("future_thing=42\nopacity=200\n");
        assert_eq!(c.opacity, Some(200));
    }

    #[test]
    fn tolerates_utf8_bom() {
        // A leading BOM must not corrupt the first key (Notepad "UTF-8 with BOM").
        let c = parse("\u{feff}metrics=cpu,temp\nopacity=200\n");
        assert_eq!(c.metrics, Some(vec!["cpu".into(), "temp".into()]));
        assert_eq!(c.opacity, Some(200));
    }

    #[test]
    fn temp_dir_is_writable() {
        assert!(dir_is_writable(&std::env::temp_dir()));
    }

    #[test]
    fn file_write_read_roundtrip() {
        let p = std::env::temp_dir().join("tinymon_test_cfg_roundtrip.cfg");
        let c = Config { x: Some(7), y: Some(9), ..Default::default() };
        write(&p, &c);
        let text = fs::read_to_string(&p).expect("cfg should exist");
        assert_eq!(parse(&text), c);
        let _ = fs::remove_file(&p);
    }

    #[test]
    fn parse_strips_inline_comments() {
        let c = parse(
            "x=100   # window position\n\
             metrics=cpu,ram,gpu   # which, in order\n\
             hotkey_toggle=Ctrl+Alt+M  # optional; e.g. Ctrl+Shift+F1\n\
             opacity=200#tight\n",
        );
        assert_eq!(c.x, Some(100));
        assert_eq!(c.metrics, Some(vec!["cpu".into(), "ram".into(), "gpu".into()]));
        assert_eq!(c.hotkey_toggle.as_deref(), Some("Ctrl+Alt+M"));
        assert_eq!(c.opacity, Some(200));
    }

    #[test]
    fn parse_readme_example_block() {
        // The README's "Keys" block, verbatim (its en dashes written as \u{2013}).
        let c = parse(
            "x=100                     # window position (set automatically on move)\n\
             y=24\n\
             metrics=cpu,ram,gpu       # which metrics to show, in order\n\
             opacity=220               # 40\u{2013}255\n\
             interval_ms=1000          # 250\u{2013}10000\n\
             font_px=13                # 9\u{2013}16\n\
             hotkey_toggle=Ctrl+Alt+M  # optional; e.g. Ctrl+Shift+F1\n\
             hotkey_settings=Ctrl+Alt+S\n",
        );
        assert_eq!((c.x, c.y), (Some(100), Some(24)));
        assert_eq!(c.metrics, Some(vec!["cpu".into(), "ram".into(), "gpu".into()]));
        assert_eq!((c.opacity, c.interval_ms, c.font_px), (Some(220), Some(1000), Some(13)));
        assert_eq!(c.hotkey_toggle.as_deref(), Some("Ctrl+Alt+M"));
        assert_eq!(c.hotkey_settings.as_deref(), Some("Ctrl+Alt+S"));
    }

    #[test]
    fn owner_config_still_parses_identically() {
        // The owner's real 137-byte file; its hotkeys are the bare keys "]" and "[".
        let text = "x=-213\ny=-16\nmetrics=cpu,ram,gpu,temp,uptime\nopacity=188\n\
                    interval_ms=1000\nfont_px=13\nhotkey_toggle=0xDD\nhotkey_settings=0xDB\n\
                    light=false\n";
        assert_eq!(text.len(), 137);
        let c = parse(text);
        assert_eq!(
            c,
            Config {
                x: Some(-213),
                y: Some(-16),
                metrics: Some(vec![
                    "cpu".into(),
                    "ram".into(),
                    "gpu".into(),
                    "temp".into(),
                    "uptime".into()
                ]),
                opacity: Some(188),
                interval_ms: Some(1000),
                font_px: Some(13),
                hotkey_toggle: Some("0xDD".into()),
                hotkey_settings: Some("0xDB".into()),
                light: Some(false),
            }
        );
        assert_eq!(serialize(&c), text);
    }
}
