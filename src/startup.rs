//! "Start with Windows" toggle via the per-user Run key (no admin required):
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value "TinyMon",
//! which holds the exe's path in double quotes. Task Manager's Startup tab can
//! disable the entry without deleting it: it then marks the value of the same
//! name under `...\CurrentVersion\Explorer\StartupApproved\Run`.

use core::ffi::c_void;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_ROUTINE_FLAGS,
    REG_SZ, RRF_RT_REG_BINARY, RRF_RT_REG_SZ,
};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
/// Where Task Manager records whether a Run entry is enabled (approved_disabled).
const STARTUP_APPROVED: PCWSTR =
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run");
const VALUE_NAME: PCWSTR = w!("TinyMon");

/// Does Windows start this exe at logon? True when the Run value launches the
/// running exe and Task Manager has not disabled the entry. A value that names
/// another exe, left by a moved exe or written by another copy, reads as off,
/// so the menu's click (set_enabled(!is_enabled())) points the entry at this
/// exe instead of deleting it.
pub fn is_enabled() -> bool {
    match (run_value(), exe_path()) {
        (Some(cmd), Some(exe)) => launches(&cmd, &exe) && !disabled_in_task_manager(),
        _ => false,
    }
}

/// Turn start-with-Windows on or off for the running exe; true on success.
/// Turning it on writes run_command (the quoted path) as the Run value, over
/// whatever it held, then deletes the StartupApproved value, so that an entry
/// that Task Manager disabled starts again (no value means enabled). Turning
/// it off deletes the Run value.
pub fn set_enabled(enable: bool) -> bool {
    // SAFETY: registry FFI on HKEY_CURRENT_USER with constant names; `data`
    // outlives the call that reads it.
    unsafe {
        if !enable {
            return deleted(RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, VALUE_NAME));
        }
        let Some(data) = exe_path_bytes() else {
            return false;
        };
        let size = data.len() as u32;
        let set = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            VALUE_NAME,
            REG_SZ.0,
            Some(data.as_ptr().cast()),
            size,
        );
        set == ERROR_SUCCESS
            && deleted(RegDeleteKeyValueW(HKEY_CURRENT_USER, STARTUP_APPROVED, VALUE_NAME))
    }
}

/// The Run value's command line for `exe`: the path in double quotes, so that
/// a path with spaces is one program and not a program with arguments
/// (unquoted, `C:\Program Files\...` can start `C:\Program.exe`).
fn run_command(exe: &str) -> String {
    format!("\"{exe}\"")
}

/// Whether the Run value `value` starts `exe`. The value is trimmed; the
/// program is the text inside a leading pair of double quotes, as run_command
/// writes it (anything after the closing quote is an argument), or else the
/// whole value, the unquoted path that earlier versions wrote. An unquoted
/// value with a space does not count: Windows could start another program
/// from it (`C:\Program.exe` for `C:\Program Files\...`), so the menu shows the
/// item off and one click writes the value quoted. Windows file names ignore
/// case, and the comparison ignores the case of ASCII letters (such as a drive
/// letter typed in lower case); a non-ASCII letter must match exactly, a rare
/// miss that a click on the menu item repairs.
fn launches(value: &str, exe: &str) -> bool {
    let value = value.trim();
    let program = match value.strip_prefix('"') {
        Some(quoted) => quoted.split_once('"').map_or(quoted, |(path, _)| path),
        None if value.contains(' ') => return false,
        None => value,
    };
    // Byte by byte: str::eq_ignore_ascii_case adds about 0.4 KiB of code.
    program.len() == exe.len()
        && program.bytes().zip(exe.bytes()).all(|(a, b)| a.eq_ignore_ascii_case(&b))
}

/// Whether the data of the StartupApproved value says that Task Manager has
/// disabled the entry. Task Manager writes 02 00 00 00 ... for an enabled
/// entry and 03 00 00 00 ... for a disabled one. The format is undocumented,
/// so this is a heuristic pinned to those observed forms: the low bit of the
/// first byte means disabled, and no data means enabled.
fn approved_disabled(data: &[u8]) -> bool {
    data.first().is_some_and(|b| b & 1 != 0)
}

/// The Run value's command line, cut at its first zero, when the value is a
/// string of at most 1023 UTF-16 units: REG_SZ, or REG_EXPAND_SZ, which
/// RegGetValueW expands (the limit then applies to the expanded text).
fn run_value() -> Option<String> {
    let mut buf = [0u16; 1024];
    // SAFETY: `buf` is as large as the size passed with it.
    unsafe { get_value(RUN_KEY, RRF_RT_REG_SZ, buf.as_mut_ptr().cast(), size_of_val(&buf)) }?;
    Some(String::from_utf16_lossy(buf.split(|&c| c == 0).next().unwrap_or_default()))
}

/// Whether Task Manager has disabled the entry: approved_disabled on the
/// StartupApproved value, when that is binary data of at most 16 bytes.
fn disabled_in_task_manager() -> bool {
    let mut buf = [0u8; 16];
    // SAFETY: `buf` is as large as the size passed with it.
    let size = unsafe {
        get_value(STARTUP_APPROVED, RRF_RT_REG_BINARY, buf.as_mut_ptr().cast(), buf.len())
    };
    size.and_then(|n| buf.get(..n)).is_some_and(approved_disabled)
}

/// RegGetValueW for the value "TinyMon" of HKCU\`key`, of a type in `flags`,
/// into the `size` bytes at `data`, which must be writable (a string comes
/// back with a final zero). Returns the number of bytes read, or None on any
/// error: the value or its key is missing, it has another type, it is larger
/// than `size`, or it cannot be read (access denied).
unsafe fn get_value(
    key: PCWSTR,
    flags: REG_ROUTINE_FLAGS,
    data: *mut c_void,
    size: usize,
) -> Option<usize> {
    let mut size = size as u32;
    let r =
        RegGetValueW(HKEY_CURRENT_USER, key, VALUE_NAME, flags, None, Some(data), Some(&mut size));
    (r == ERROR_SUCCESS).then_some(size as usize)
}

/// A registry delete succeeded, or found nothing to delete.
fn deleted(result: WIN32_ERROR) -> bool {
    result == ERROR_SUCCESS || result == ERROR_FILE_NOT_FOUND
}

/// The running exe's full path, or None when it cannot be read or is not
/// valid Unicode.
fn exe_path() -> Option<String> {
    std::env::current_exe().ok()?.into_os_string().into_string().ok()
}

/// The Run value's data for the running exe: run_command as a NUL-terminated
/// UTF-16 byte buffer (REG_SZ).
fn exe_path_bytes() -> Option<Vec<u8>> {
    let mut wide: Vec<u16> = run_command(&exe_path()?).encode_utf16().collect();
    wide.push(0); // REG_SZ data should be NUL-terminated
    let mut bytes = Vec::with_capacity(wide.len() * 2);
    for u in wide {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_command_quotes_the_exe_path() {
        // Unquoted, the path could start C:\Program.exe instead.
        assert_eq!(
            run_command(r"C:\Program Files\TinyMon\tinymon.exe"),
            r#""C:\Program Files\TinyMon\tinymon.exe""#
        );
    }

    #[test]
    fn launches_accepts_quoted_and_legacy_values_case_insensitively() {
        let exe = r"d:\tinymon\TARGET\release\tinymon.exe";
        // The unquoted path that earlier versions wrote, and run_command's form.
        assert!(launches(r"D:\TinyMon\target\release\tinymon.exe", exe));
        assert!(launches(r#""D:\TinyMon\target\release\tinymon.exe""#, exe));
        // Spaces around the value do not count.
        assert!(launches(r#" "D:\TinyMon\target\release\tinymon.exe" "#, exe));
    }

    #[test]
    fn launches_rejects_stale_or_other_copy() {
        let exe = r"C:\b\tinymon.exe";
        // Left by an exe that has moved since.
        assert!(!launches(r"D:\old\tinymon.exe", exe));
        // Another copy, with an argument after its quoted path.
        assert!(!launches(r#""C:\a\tinymon.exe" --flag"#, exe));
        // This exe with an argument after its quoted path; an unclosed quote
        // takes the rest of the value.
        assert!(launches(r#""C:\b\tinymon.exe" --flag"#, exe));
        assert!(launches(r#""C:\b\tinymon.exe"#, exe));
    }

    #[test]
    fn unquoted_value_with_a_space_needs_repair() {
        // Unquoted, "C:\Program Files\TinyMon\tinymon.exe" can start
        // C:\Program.exe: such a value reads as off, so one click on the menu
        // item writes it quoted.
        let exe = r"C:\Program Files\TinyMon\tinymon.exe";
        assert!(!launches(r"C:\Program Files\TinyMon\tinymon.exe", exe));
        assert!(launches(r#""C:\Program Files\TinyMon\tinymon.exe""#, exe));
    }

    #[test]
    fn approved_disabled_reads_task_manager_flag() {
        // Task Manager's enabled (02) and disabled (03) forms: the flag, three
        // zero bytes, then a timestamp.
        assert!(!approved_disabled(&[0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        let disabled = [0x03, 0, 0, 0, 0x00, 0xA0, 0xC9, 0xB5, 0xE5, 0x57, 0xDD, 0x01];
        assert!(approved_disabled(&disabled));
        // The low bit decides, as for the 06 (enabled) and 01 and 07
        // (disabled) forms seen on other machines.
        assert!(!approved_disabled(&[0x06]));
        assert!(approved_disabled(&[0x01]));
        assert!(approved_disabled(&[0x07]));
        // No data: nothing says disabled.
        assert!(!approved_disabled(&[]));
    }
}
