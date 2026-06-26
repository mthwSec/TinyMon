//! "Start with Windows" toggle via the per-user Run key (no admin required):
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value "TinyMon".

use std::os::windows::ffi::OsStrExt;

use windows::core::{w, PCWSTR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ,
};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const VALUE_NAME: PCWSTR = w!("TinyMon");

/// Is TinyMon registered to start with Windows (a "TinyMon" value under Run)?
pub fn is_enabled() -> bool {
    // SAFETY: registry FFI; the opened key is always closed before returning.
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_QUERY_VALUE, &mut hkey).0 != 0 {
            return false;
        }
        // Query with no data buffer — ERROR_SUCCESS means the value exists.
        let mut cb = 0u32;
        let present = RegQueryValueExW(hkey, VALUE_NAME, None, None, None, Some(&mut cb)).0 == 0;
        let _ = RegCloseKey(hkey);
        present
    }
}

/// Enable or disable start-with-Windows. Returns true on success.
pub fn set_enabled(enable: bool) -> bool {
    // SAFETY: registry FFI; the opened key is always closed before returning.
    unsafe {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_SET_VALUE, &mut hkey).0 != 0 {
            return false;
        }
        let ok = if enable {
            match exe_path_bytes() {
                Some(bytes) => RegSetValueExW(hkey, VALUE_NAME, None, REG_SZ, Some(&bytes)).0 == 0,
                None => false,
            }
        } else {
            let r = RegDeleteValueW(hkey, VALUE_NAME).0;
            r == 0 || r == 2 // 2 = ERROR_FILE_NOT_FOUND -> already absent, treat as success
        };
        let _ = RegCloseKey(hkey);
        ok
    }
}

/// The running exe's full path as a NUL-terminated UTF-16 byte buffer (REG_SZ).
fn exe_path_bytes() -> Option<Vec<u8>> {
    let path = std::env::current_exe().ok()?;
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    wide.push(0); // REG_SZ data should be NUL-terminated
    let mut bytes = Vec::with_capacity(wide.len() * 2);
    for u in wide {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    Some(bytes)
}
