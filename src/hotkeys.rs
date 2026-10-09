//! Global hotkeys: pure text<->spec parsing (unit-tested) plus thin
//! register/unregister wrappers. The settings UI captures combos with the
//! native hotkey control; this module owns the config text form and the
//! `RegisterHotKey` plumbing.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
    MOD_SHIFT, MOD_WIN,
};

/// RegisterHotKey ids (distinct namespace from menu WM_COMMAND ids).
pub const HK_TOGGLE: i32 = 1;
pub const HK_SETTINGS: i32 = 2;

/// A hotkey: `mods` is an OR of `MOD_*` values (without MOD_NOREPEAT); `vk` is
/// a Win32 virtual-key code.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HotkeySpec {
    pub mods: u32,
    pub vk: u32,
}

/// Parse a textual hotkey like "ctrl+alt+m", or a bare key like "p" / "f8"
/// (case-insensitive). Requires exactly one key (letter, digit, F1..F24, or the
/// `0xNN` virtual-key fallback): an unknown token or a second key ("ctrl+a+b")
/// gives `None`. Modifiers are optional, so a single key is a valid global
/// hotkey.
pub fn parse_hotkey(s: &str) -> Option<HotkeySpec> {
    let mut mods = 0u32;
    let mut vk: Option<u32> = None;
    for tok in s.split('+').map(|t| t.trim()).filter(|t| !t.is_empty()) {
        match tok.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods |= MOD_CONTROL.0,
            "alt" => mods |= MOD_ALT.0,
            "shift" => mods |= MOD_SHIFT.0,
            "win" | "super" => mods |= MOD_WIN.0,
            _ => match key_to_vk(tok) {
                Some(k) if vk.is_none() => vk = Some(k),
                _ => return None,
            },
        }
    }
    Some(HotkeySpec { mods, vk: vk? })
}

/// Format a spec back to text, e.g. "Ctrl+Alt+M".
pub fn format_hotkey(h: HotkeySpec) -> String {
    let mut parts: Vec<String> = Vec::new();
    if h.mods & MOD_CONTROL.0 != 0 {
        parts.push("Ctrl".into());
    }
    if h.mods & MOD_ALT.0 != 0 {
        parts.push("Alt".into());
    }
    if h.mods & MOD_SHIFT.0 != 0 {
        parts.push("Shift".into());
    }
    if h.mods & MOD_WIN.0 != 0 {
        parts.push("Win".into());
    }
    parts.push(vk_to_name(h.vk));
    parts.join("+")
}

/// Letter/digit/F-key name -> virtual-key code. (VK of 'A'..'Z' / '0'..'9' is
/// the ASCII value; VK_F1 = 0x70.)
fn key_to_vk(tok: &str) -> Option<u32> {
    let t = tok.to_ascii_uppercase();
    let b = t.as_bytes();
    if b.len() == 1 && (b[0].is_ascii_uppercase() || b[0].is_ascii_digit()) {
        return Some(b[0] as u32);
    }
    if let Some(num) = t.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&num) {
            return Some(0x70 + num - 1);
        }
    }
    // Fallback form emitted by `vk_to_name` for keys outside A-Z/0-9/F1-F24
    // (arrows, Home/End, OEM punctuation, numpad, ...): "0xNN". Parsing it back
    // keeps parse/format inverse, so those combos survive apply + save/reload —
    // otherwise such a captured hotkey is silently dropped on OK.
    if let Some(h) = t.strip_prefix("0X").and_then(|n| u32::from_str_radix(n, 16).ok()) {
        if (1..=0xFF).contains(&h) {
            return Some(h);
        }
    }
    None
}

fn vk_to_name(vk: u32) -> String {
    if (0x41..=0x5A).contains(&vk) || (0x30..=0x39).contains(&vk) {
        return (vk as u8 as char).to_string();
    }
    if (0x70..=0x87).contains(&vk) {
        return format!("F{}", vk - 0x70 + 1);
    }
    format!("0x{:02X}", vk)
}

/// Whether key `vk` types text: Backspace, Tab, Enter, Space, the digits and
/// letters, the numpad's keys and the OEM keys (the punctuation, and the extra
/// key of 102-key keyboards).
pub fn vk_types_text(vk: u32) -> bool {
    matches!(
        vk,
        0x08 | 0x09
            | 0x0D
            | 0x20
            | 0x30..=0x39
            | 0x41..=0x5A
            | 0x60..=0x6F
            | 0xBA..=0xC2
            | 0xDB..=0xDF
            | 0xE2
    )
}

/// Whether binding `spec` takes its key away from typing: a key that types
/// text (vk_types_text), bare or with Shift only. RegisterHotKey takes the
/// combo system-wide, so the key then types in no app.
pub fn blocks_typing(spec: HotkeySpec) -> bool {
    spec.mods & (MOD_CONTROL.0 | MOD_ALT.0 | MOD_WIN.0) == 0 && vk_types_text(spec.vk)
}

/// Which configured hotkeys failed to register (already taken by another app).
/// A field is only `true` when that hotkey was requested AND `RegisterHotKey`
/// failed — so the caller can warn the user about a genuine conflict.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RegisterResult {
    pub toggle_failed: bool,
    pub settings_failed: bool,
}

impl RegisterResult {
    /// Whether either hotkey failed to register.
    pub fn any_failed(self) -> bool {
        self.toggle_failed || self.settings_failed
    }
}

/// Whether an Apply registers the hotkeys again: when a combo changed
/// (`changed`: Show / Hide, Open settings), and also when one failed last time
/// (`status`), so that a combo another app held at startup is retried.
pub fn should_register(changed: [bool; 2], status: RegisterResult) -> bool {
    changed[0] || changed[1] || status.any_failed()
}

/// The failures of `res` an Apply warns about: only those of the combos it
/// changed (`changed`). An unchanged combo that still fails is shown only by
/// its note in the settings window.
pub fn to_warn(res: RegisterResult, changed: [bool; 2]) -> RegisterResult {
    RegisterResult {
        toggle_failed: res.toggle_failed && changed[0],
        settings_failed: res.settings_failed && changed[1],
    }
}

/// Register the configured hotkeys on `hwnd` (best-effort), reporting any that a
/// conflict prevented registering.
pub unsafe fn register(
    hwnd: HWND,
    toggle: Option<HotkeySpec>,
    settings: Option<HotkeySpec>,
) -> RegisterResult {
    let mut res = RegisterResult::default();
    if let Some(h) = toggle {
        res.toggle_failed =
            RegisterHotKey(Some(hwnd), HK_TOGGLE, HOT_KEY_MODIFIERS(h.mods | MOD_NOREPEAT.0), h.vk)
                .is_err();
    }
    if let Some(h) = settings {
        res.settings_failed = RegisterHotKey(
            Some(hwnd),
            HK_SETTINGS,
            HOT_KEY_MODIFIERS(h.mods | MOD_NOREPEAT.0),
            h.vk,
        )
        .is_err();
    }
    res
}

/// Unregister both hotkeys (best-effort).
pub unsafe fn unregister(hwnd: HWND) {
    let _ = UnregisterHotKey(Some(hwnd), HK_TOGGLE);
    let _ = UnregisterHotKey(Some(hwnd), HK_SETTINGS);
}

/// The warning shown after an Apply: one line for each configured hotkey that
/// `res` reports as failed (almost always because another app owns the combo;
/// an Apply passes only the combos it changed, to_warn), or `None` when there
/// is nothing to list.
pub fn conflict_message(
    toggle: Option<HotkeySpec>,
    settings: Option<HotkeySpec>,
    res: &RegisterResult,
) -> Option<String> {
    let mut lines = String::new();
    if res.toggle_failed {
        if let Some(h) = toggle {
            lines.push_str(&format!("\u{2022} Show / Hide: {}\n", format_hotkey(h)));
        }
    }
    if res.settings_failed {
        if let Some(h) = settings {
            lines.push_str(&format!("\u{2022} Open settings: {}\n", format_hotkey(h)));
        }
    }
    if lines.is_empty() {
        return None;
    }
    Some(format!(
        "These hotkeys couldn't be registered (already in use by another app):\n\n{lines}\nPick a different combination in Settings."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let h = parse_hotkey("ctrl+alt+m").unwrap();
        assert_eq!(h.mods, MOD_CONTROL.0 | MOD_ALT.0);
        assert_eq!(h.vk, 0x4D); // 'M'
    }

    #[test]
    fn parse_is_case_insensitive_and_spacey() {
        assert_eq!(parse_hotkey("CTRL + Shift + F5"), parse_hotkey("ctrl+shift+f5"));
        let h = parse_hotkey("ctrl+shift+f5").unwrap();
        assert_eq!(h.vk, 0x74); // VK_F5
    }

    #[test]
    fn parse_allows_bare_key_rejects_unknown_or_keyless() {
        // A bare (modifier-less) key is a valid global hotkey now.
        assert_eq!(parse_hotkey("m"), Some(HotkeySpec { mods: 0, vk: 0x4D }));
        assert_eq!(parse_hotkey("f8"), Some(HotkeySpec { mods: 0, vk: 0x77 }));
        // '[' (VK_OEM_4)
        assert_eq!(parse_hotkey("0xDB"), Some(HotkeySpec { mods: 0, vk: 0xDB }));
        // Still rejected: unknown key, nothing at all, or modifiers with no key.
        assert_eq!(parse_hotkey("ctrl+nope"), None);
        assert_eq!(parse_hotkey(""), None);
        assert_eq!(parse_hotkey("ctrl"), None);
    }

    #[test]
    fn parse_rejects_two_keys() {
        // Exactly one key: "Ctrl+A+B" used to bind Ctrl+B without a word.
        assert_eq!(parse_hotkey("ctrl+a+b"), None);
        assert_eq!(parse_hotkey("a+b"), None);
        // A single key, with or without modifiers, parses as before.
        assert_eq!(
            parse_hotkey("ctrl+alt+m"),
            Some(HotkeySpec { mods: MOD_CONTROL.0 | MOD_ALT.0, vk: 0x4D })
        );
        assert_eq!(parse_hotkey("0xDB"), Some(HotkeySpec { mods: 0, vk: 0xDB }));
    }

    #[test]
    fn parse_counts_a_repeated_key_and_allows_a_modifier_after_the_key() {
        // The same key twice is still a second key.
        assert_eq!(parse_hotkey("a+a"), None);
        // Only the number of keys matters, not their position.
        assert_eq!(parse_hotkey("m+ctrl"), Some(HotkeySpec { mods: MOD_CONTROL.0, vk: 0x4D }));
    }

    #[test]
    fn round_trip() {
        for s in ["Ctrl+M", "Ctrl+Alt+F12", "Alt+Shift+7", "P", "F8"] {
            let h = parse_hotkey(s).unwrap();
            assert_eq!(parse_hotkey(&format_hotkey(h)), Some(h));
        }
    }

    #[test]
    fn round_trip_non_alphanumeric_vks() {
        // The hotkey control can capture keys outside A-Z/0-9/F1-F24 (arrows,
        // Home, OEM ';' / '['). These serialize via the "0xNN" fallback and MUST
        // parse back — both bare and with modifiers — or the combo vanishes on save.
        for vk in
            [0x25u32 /*Left*/, 0x24 /*Home*/, 0xBA /*OEM ;*/, 0xDB /*OEM [*/]
        {
            for mods in [0, MOD_CONTROL.0 | MOD_ALT.0] {
                let h = HotkeySpec { mods, vk };
                assert_eq!(
                    parse_hotkey(&format_hotkey(h)),
                    Some(h),
                    "vk {:#04X} mods {:#X}",
                    vk,
                    mods
                );
            }
        }
    }

    #[test]
    fn conflict_message_lists_only_failed_hotkeys() {
        let toggle = parse_hotkey("ctrl+alt+m");
        let settings = parse_hotkey("ctrl+alt+s");
        assert_eq!(conflict_message(toggle, settings, &RegisterResult::default()), None);
        let toggle_failed = RegisterResult { toggle_failed: true, settings_failed: false };
        let text = conflict_message(toggle, settings, &toggle_failed).expect("a warning");
        assert!(text.contains("Show / Hide: Ctrl+Alt+M"), "{text}");
        assert!(!text.contains("Open settings"), "{text}");
        // A failed flag without a combo has nothing to list.
        assert_eq!(conflict_message(None, settings, &toggle_failed), None);
    }

    #[test]
    fn typing_keys() {
        // Backspace, Tab, Enter, A, Z, 0, 9 and Space.
        let plain = [0x08, 0x09, 0x0D, 0x41, 0x5A, 0x30, 0x39, 0x20];
        // The OEM keys [ ] ; and `, the numpad's 0 and /, and the extra key of
        // 102-key keyboards.
        let more = [0xDB, 0xDD, 0xBA, 0xC0, 0x60, 0x6F, 0xE2];
        for vk in plain.into_iter().chain(more) {
            assert!(vk_types_text(vk), "{vk:#04X}");
        }
        // F1, F24, Left, Home, Delete, Insert, Pause, Volume Mute, Play/Pause,
        // Apps and Num Lock type nothing.
        for vk in [0x70, 0x87, 0x25, 0x24, 0x2E, 0x2D, 0x13, 0xAD, 0xB3, 0x5D, 0x90] {
            assert!(!vk_types_text(vk), "{vk:#04X}");
        }
    }

    #[test]
    fn bare_or_shift_typing_keys_block_typing() {
        let key = |mods: u32, vk: u32| HotkeySpec { mods, vk };
        // A bare "]", and Shift with "[" (which types "{").
        assert!(blocks_typing(key(0, 0xDD)));
        assert!(blocks_typing(key(MOD_SHIFT.0, 0xDB)));
        // Ctrl, Alt or Win make a shortcut of the key; F8 and Left type nothing.
        let (ctrl, alt, win) = (MOD_CONTROL.0, MOD_ALT.0, MOD_WIN.0);
        for spec in [key(ctrl, 0xDD), key(alt, 0x54), key(win, 0x54), key(0, 0x77), key(0, 0x25)] {
            assert!(!blocks_typing(spec), "{spec:?}");
        }
    }

    #[test]
    fn failed_startup_hotkey_is_retried_on_apply() {
        let toggle_failed = RegisterResult { toggle_failed: true, settings_failed: false };
        // Neither combo changed, but Show / Hide failed to register (at
        // startup, say): the Apply registers again, in case it is free now.
        assert!(should_register([false, false], toggle_failed));
        // Nothing changed and nothing failed: nothing to do.
        assert!(!should_register([false, false], RegisterResult::default()));
        // A changed combo is always registered.
        assert!(should_register([true, false], RegisterResult::default()));
    }

    #[test]
    fn warn_only_about_changed_combos() {
        let both = RegisterResult { toggle_failed: true, settings_failed: true };
        let toggle = RegisterResult { toggle_failed: true, settings_failed: false };
        let settings = RegisterResult { toggle_failed: false, settings_failed: true };
        // Both failed, but only Open settings changed: Show / Hide failed
        // before as well, and its note already says so.
        assert_eq!(to_warn(both, [false, true]), settings);
        // Both changed, and only Show / Hide failed.
        assert_eq!(to_warn(toggle, [true, true]), toggle);
    }

    #[test]
    fn register_result_any_failed() {
        assert!(!RegisterResult::default().any_failed());
        assert!(RegisterResult { toggle_failed: true, settings_failed: false }.any_failed());
        assert!(RegisterResult { toggle_failed: false, settings_failed: true }.any_failed());
    }
}
