//! Portable config: save/restore window position + settings.
//!
//! Location strategy: prefer `tinymon.cfg` next to the .exe (USB-portable); if
//! that folder isn't writable, fall back to `%APPDATA%\TinyMon\tinymon.cfg`.
//! Writability is decided by an actual test write, never guessed — so the same
//! .exe runs from a read-only USB stick, Program Files, or a normal folder.
//!
//! Loading starts from the file this install saves to (see `load`), and the
//! file's bytes are decoded leniently (`decode`): a UTF-16 or ANSI file reads
//! as what it says instead of as "no config". Saving (`write`) normally renames
//! a fully written temp file over the old one, so a crash leaves the old file or
//! the new one, never an empty or torn one.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The config file's name, next to the .exe or in `%APPDATA%\TinyMon`.
const CFG_NAME: &str = "tinymon.cfg";

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

/// Load the config, starting from the file this install saves to: `write_path`
/// is what `resolve_write_path` returned. A portable install (one that saves
/// next to its .exe) reads its own file first even when `%APPDATA%` holds a
/// newer one, which then only seeds a first run, because another install may
/// have written it. Otherwise the newer file wins, so a save made while the .exe
/// folder was still writable is not shadowed by an older `%APPDATA%` file. A
/// missing or unreadable file falls through to the next; with none, defaults.
pub fn load(write_path: Option<&Path>) -> Config {
    let found = read_candidates()
        .into_iter()
        .filter_map(|p| {
            let modified = fs::metadata(&p).ok()?.modified().ok()?;
            Some((p, modified))
        })
        .collect();
    load_first(&load_order(is_portable(write_path, exe_dir().as_deref()), found))
}

/// Resolve where to *write* the config: the .exe folder if writable (decided by
/// a real test write), else `%APPDATA%\TinyMon` (created if needed). `None` if
/// no writable location exists.
pub fn resolve_write_path() -> Option<PathBuf> {
    if let Some(dir) = exe_dir() {
        if dir_is_writable(&dir) {
            return Some(dir.join(CFG_NAME));
        }
    }
    if let Some(dir) = appdata_dir() {
        let _ = fs::create_dir_all(&dir);
        if dir_is_writable(&dir) {
            return Some(dir.join(CFG_NAME));
        }
    }
    None
}

/// Write the config to `path`. The text is staged in `<path>.tmp`, flushed to
/// disk and renamed over `path`, so a crash leaves either the old file or the
/// new one, never an empty or half-written one. The temp file is opened with
/// share mode 0, so a second writer cannot interleave its bytes; it gets an
/// error instead. If the rename fails, the temp file is deleted and the text is
/// written in place as before (not atomic), so a reader that did not share
/// delete does not cost the save; if that fails too, the rename's error is
/// returned.
pub fn write(path: &Path, cfg: &Config) -> io::Result<()> {
    let text = serialize(cfg);
    let tmp = tmp_path(path);
    let mut f =
        OpenOptions::new().write(true).create(true).truncate(true).share_mode(0).open(&tmp)?;
    let staged = f.write_all(text.as_bytes()).and_then(|()| f.sync_all());
    drop(f);
    if let Err(e) = staged {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    // std's rename calls MoveFileExW(MOVEFILE_REPLACE_EXISTING) first. If that is
    // denied, it retries as a POSIX-semantics FileRenameInfoEx rename (NTFS,
    // Windows 10 1607+), which can replace a file another process holds open
    // with FILE_SHARE_DELETE. It fails when the holder did not share delete, on
    // FAT/exFAT with any open holder, and on a read-only target.
    fs::rename(&tmp, path).or_else(|e| {
        let _ = fs::remove_file(&tmp);
        fs::write(path, &text).map_err(|_| e)
    })
}

// ---- internals -------------------------------------------------------------

/// The config files to look for, in this order: the .exe folder's, then the
/// `%APPDATA%` one.
fn read_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(dir) = exe_dir() {
        v.push(dir.join(CFG_NAME));
    }
    if let Some(dir) = appdata_dir() {
        v.push(dir.join(CFG_NAME));
    }
    v
}

/// Whether `write_path` is the config file next to the .exe (a portable
/// install). Compared as OS strings; both sides come from `exe_dir`.
fn is_portable(write_path: Option<&Path>, exe_dir: Option<&Path>) -> bool {
    match (write_path, exe_dir) {
        (Some(w), Some(d)) => w.as_os_str() == d.join(CFG_NAME).as_os_str(),
        _ => false,
    }
}

/// The order to try the existing files in. `found` holds them with their
/// modification times in `read_candidates` order. A portable install keeps that
/// order (its own file first); otherwise the newer file goes first. There are
/// at most two, so one compare-and-swap does instead of a sort.
fn load_order(portable: bool, mut found: Vec<(PathBuf, SystemTime)>) -> Vec<PathBuf> {
    if !portable && found.len() == 2 && found[1].1 > found[0].1 {
        found.swap(0, 1);
    }
    found.into_iter().map(|(p, _)| p).collect()
}

/// Parse the first of `paths` that can actually be read; defaults if none can.
fn load_first(paths: &[PathBuf]) -> Config {
    for p in paths {
        if let Ok(bytes) = fs::read(p) {
            return parse(&decode(&bytes));
        }
    }
    Config::default()
}

/// The text of a config file; never fails. A file that starts with a UTF-16
/// byte order mark (PowerShell 5.1 `Out-File` or `>`, Notepad "UTF-16") is
/// decoded as UTF-16. Anything else is read as ASCII: a leading UTF-8 BOM is
/// dropped and every other non-ASCII byte becomes a space. Keys and values are
/// ASCII, so UTF-8 and ANSI files read alike, and a value padded with a
/// no-break space still trims to the same token.
fn decode(bytes: &[u8]) -> String {
    fn utf16(b: &[u8], unit: fn([u8; 2]) -> u16) -> String {
        let units: Vec<u16> = b.chunks_exact(2).map(|c| unit([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units)
    }
    match bytes {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, u16::from_le_bytes),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, u16::from_be_bytes),
        _ => {
            let text = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
            text.iter().map(|&c| if c.is_ascii() { c as char } else { ' ' }).collect()
        }
    }
}

/// `path` with ".tmp" appended (`tinymon.cfg.tmp`), built on the OS string.
fn tmp_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".tmp");
    PathBuf::from(s)
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
    use std::io::Read;
    use std::time::Duration;

    /// A fresh, empty folder under %TEMP% for one test's files.
    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tinymon_test_{name}"));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).expect("create scratch folder");
        d
    }

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
        let p = scratch("roundtrip").join("tinymon.cfg");
        let c = Config { x: Some(7), y: Some(9), ..Default::default() };
        write(&p, &c).expect("write");
        let text = fs::read_to_string(&p).expect("cfg should exist");
        assert_eq!(parse(&text), c);
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
    fn value_ends_at_the_first_hash() {
        // A comment may itself hold a '#': the value still ends at the first one.
        let c = parse("x=5  # see #12\nhotkey_toggle=Ctrl+M # was #3\n");
        assert_eq!(c.x, Some(5));
        assert_eq!(c.hotkey_toggle.as_deref(), Some("Ctrl+M"));
    }

    #[test]
    fn comment_only_value_is_empty() {
        // `key=# note` is an empty value. An empty metrics list means "all hidden"
        // (only a missing key means the defaults), an empty number or boolean is
        // unset, and an empty hotkey stays as text that binds nothing.
        let c = parse("metrics=  # none\nopacity=# x\nlight=# x\nhotkey_toggle=# none\n");
        assert_eq!(c.metrics, Some(Vec::new()));
        assert_eq!((c.opacity, c.light), (None, None));
        assert_eq!(c.hotkey_toggle.as_deref(), Some(""));
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

    #[test]
    fn decode_utf16le_bom() {
        // PowerShell 5.1 `Out-File` and `>` write UTF-16LE with a BOM.
        let mut bytes = vec![0xFF, 0xFE];
        for u in "opacity=200\r\nx=5\r\n".encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        let c = parse(&decode(&bytes));
        assert_eq!((c.opacity, c.x), (Some(200), Some(5)));
    }

    #[test]
    fn decode_utf16be_bom() {
        let mut bytes = vec![0xFE, 0xFF];
        for u in "opacity=201".encode_utf16() {
            bytes.extend_from_slice(&u.to_be_bytes());
        }
        assert_eq!(parse(&decode(&bytes)).opacity, Some(201));
    }

    #[test]
    fn decode_utf8_with_bom_still_parses() {
        assert_eq!(parse(&decode(b"\xEF\xBB\xBFopacity=199")).opacity, Some(199));
    }

    #[test]
    fn decode_drops_a_utf8_bom() {
        // decode's own promise, not only parse's tolerance: without the strip the
        // three bytes would become three spaces, which trim() hides from parse.
        assert_eq!(decode(b"\xEF\xBB\xBFopacity=199"), "opacity=199");
    }

    #[test]
    fn decode_invalid_utf8_is_lossy_not_fatal() {
        // An ANSI comment line: invalid UTF-8, which made the whole file unreadable.
        let c = parse(&decode(b"metrics=cpu\n# caf\xE9 \xE1\xE2\nopacity=150\n"));
        assert_eq!(c.metrics, Some(vec!["cpu".into()]));
        assert_eq!(c.opacity, Some(150));
    }

    #[test]
    fn decode_keeps_unicode_whitespace_tolerance() {
        // UTF-8 no-break spaces (C2 A0) next to values still trim away.
        let c = parse(&decode(b"metrics=cpu,\xC2\xA0ram\nx=1\xC2\xA0\ny=2\n"));
        assert_eq!(c.metrics, Some(vec!["cpu".into(), "ram".into()]));
        assert_eq!((c.x, c.y), (Some(1), Some(2)));
    }

    #[test]
    fn decode_reads_an_ansi_no_break_space_as_a_space() {
        // A Windows-1252 no-break space is the lone byte A0 (invalid UTF-8): it
        // trims away like the UTF-8 form C2 A0 instead of becoming U+FFFD.
        let c = parse(&decode(b"x=1\xA0\nmetrics=cpu,\xA0ram\n"));
        assert_eq!(c.x, Some(1));
        assert_eq!(c.metrics, Some(vec!["cpu".into(), "ram".into()]));
    }

    #[test]
    fn is_portable_matches_only_the_exe_folder_file() {
        let d = Path::new(r"D:\TinyMon");
        let appdata = Path::new(r"C:\Users\me\AppData\Roaming\TinyMon");
        assert!(is_portable(Some(&d.join("tinymon.cfg")), Some(d)));
        assert!(!is_portable(Some(&appdata.join("tinymon.cfg")), Some(d)));
        assert!(!is_portable(None, Some(d)));
    }

    #[test]
    fn load_order_portable_prefers_own_file_even_if_older() {
        let (exe, app) = (PathBuf::from("exe.cfg"), PathBuf::from("appdata.cfg"));
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let day = Duration::from_secs(86_400);
        let found = vec![(exe.clone(), t0), (app.clone(), t0 + day)];
        assert_eq!(load_order(true, found), vec![exe, app]);
    }

    #[test]
    fn load_order_appdata_mode_prefers_newer() {
        let (exe, app) = (PathBuf::from("exe.cfg"), PathBuf::from("appdata.cfg"));
        let t0 = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        let day = Duration::from_secs(86_400);
        let exe_newer = vec![(exe.clone(), t0 + day), (app.clone(), t0)];
        assert_eq!(load_order(false, exe_newer), vec![exe.clone(), app.clone()]);
        let app_newer = vec![(exe.clone(), t0), (app.clone(), t0 + day)];
        assert_eq!(load_order(false, app_newer), vec![app, exe]);
    }

    #[test]
    fn load_first_skips_missing_and_uses_next() {
        let d = scratch("load_first");
        let missing = d.join("missing.cfg");
        let other = d.join("other.cfg");
        fs::write(&other, "opacity=77\n").expect("write other.cfg");
        assert_eq!(load_first(&[missing.clone(), other]).opacity, Some(77));
        assert_eq!(load_first(&[missing]), Config::default());
    }

    #[test]
    fn load_first_skips_an_unreadable_file() {
        // The file exists, so load()'s metadata check keeps it, but it cannot be
        // read: another handle holds it without sharing. The next file is used,
        // not the defaults (which the next save would write over the user's file).
        let d = scratch("load_first_unreadable");
        let locked = d.join("locked.cfg");
        let other = d.join("other.cfg");
        fs::write(&locked, "opacity=150\n").expect("write locked.cfg");
        fs::write(&other, "opacity=77\n").expect("write other.cfg");
        let _held = fs::OpenOptions::new().read(true).share_mode(0).open(&locked).expect("hold");
        assert!(fs::metadata(&locked).and_then(|m| m.modified()).is_ok());
        assert_eq!(load_first(&[locked, other]).opacity, Some(77));
    }

    #[test]
    fn write_replaces_file_instead_of_truncating_in_place() {
        let p = scratch("write_replaces").join("tinymon.cfg");
        fs::write(&p, "x=1\n").expect("seed");
        // A reader holding the file open (std shares read, write and delete).
        let mut reader = fs::File::open(&p).expect("open reader");
        write(&p, &Config { x: Some(2), ..Default::default() }).expect("write");
        let mut old = String::new();
        reader.read_to_string(&mut old).expect("read through the old handle");
        assert_eq!(old, "x=1\n");
        assert_eq!(parse(&fs::read_to_string(&p).expect("read")).x, Some(2));
        assert!(!tmp_path(&p).exists());
    }

    #[test]
    fn write_overwrites_a_stale_temp() {
        // A crash left a closed temp file, longer than the next save: it is
        // truncated, so none of its old tail is renamed into the config.
        let p = scratch("write_stale_temp").join("tinymon.cfg");
        fs::write(&p, "x=1\n").expect("seed");
        fs::write(tmp_path(&p), "x=1\ny=2\nmetrics=cpu,ram,gpu\nlight=false\n").expect("stale");
        let cfg = Config { light: Some(true), ..Default::default() };
        write(&p, &cfg).expect("write");
        assert_eq!(fs::read_to_string(&p).expect("read"), serialize(&cfg));
        assert!(!tmp_path(&p).exists());
    }

    #[test]
    fn write_leaves_read_only_target_untouched() {
        let p = scratch("write_read_only").join("tinymon.cfg");
        fs::write(&p, "x=1\n").expect("seed");
        let writable = fs::metadata(&p).expect("metadata").permissions();
        let mut read_only = writable.clone();
        read_only.set_readonly(true);
        fs::set_permissions(&p, read_only).expect("make read-only");
        let res = write(&p, &Config { x: Some(2), ..Default::default() });
        let content = fs::read_to_string(&p).expect("read");
        fs::set_permissions(&p, writable).expect("make writable again");
        assert!(res.is_err());
        assert_eq!(content, "x=1\n");
        assert!(!tmp_path(&p).exists());
    }

    #[test]
    fn write_refuses_a_temp_another_writer_holds() {
        let p = scratch("write_held_temp").join("tinymon.cfg");
        fs::write(&p, "x=1\n").expect("seed");
        // Another writer is in the middle of its own save.
        let _held = fs::File::create(tmp_path(&p)).expect("hold the temp file");
        assert!(write(&p, &Config { x: Some(2), ..Default::default() }).is_err());
        assert_eq!(fs::read_to_string(&p).expect("read"), "x=1\n");
    }

    #[test]
    fn refused_write_leaves_the_other_writers_temp_alone() {
        let p = scratch("write_held_temp_kept").join("tinymon.cfg");
        fs::write(&p, "x=1\n").expect("seed");
        let mut held = fs::File::create(tmp_path(&p)).expect("hold the temp file");
        held.write_all(b"x=3\n").expect("the other writer's bytes");
        assert!(write(&p, &Config { x: Some(2), ..Default::default() }).is_err());
        drop(held);
        assert_eq!(fs::read_to_string(tmp_path(&p)).expect("temp still there"), "x=3\n");
    }

    #[test]
    fn write_falls_back_in_place_when_rename_is_denied() {
        let p = scratch("write_no_share_delete").join("tinymon.cfg");
        fs::write(&p, "x=1\n").expect("seed");
        // A holder that shares read and write but not delete blocks the rename.
        let _holder = fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1 | 0x2) // FILE_SHARE_READ | FILE_SHARE_WRITE
            .open(&p)
            .expect("open holder");
        write(&p, &Config { x: Some(2), ..Default::default() }).expect("write");
        assert_eq!(parse(&fs::read_to_string(&p).expect("read")).x, Some(2));
        assert!(!tmp_path(&p).exists());
    }
}
