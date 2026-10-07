//! Shared PDH (Performance Data Helper) plumbing — open an English counter
//! query and read its per-instance values. Backs the vendor-neutral GPU-load,
//! per-core CPU, disk, and network samplers (no vendor SDKs, no admin).

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PDH_FMT_COUNTERVALUE_ITEM_W, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY,
};

/// One PDH counter query. Opens an English counter `path` (which may contain a
/// `(*)` wildcard), takes a baseline sample, and yields per-instance values on
/// each `sample()`. Inert (no rows) if PDH/the counter is unavailable.
pub struct PdhCounter {
    query: PDH_HQUERY,
    counter: PDH_HCOUNTER,
    ok: bool,
}

impl PdhCounter {
    pub fn new(path: PCWSTR) -> Self {
        let mut query = PDH_HQUERY(core::ptr::null_mut());
        let mut counter = PDH_HCOUNTER(core::ptr::null_mut());
        // SAFETY: standard PDH open + add-counter sequence; out-params are owned.
        let ok = unsafe {
            PdhOpenQueryW(PCWSTR::null(), 0, &mut query) == 0
                && PdhAddEnglishCounterW(query, path, 0, &mut counter) == 0
        };
        if ok {
            // SAFETY: first collection establishes the rate baseline.
            unsafe { PdhCollectQueryData(query) };
        }
        Self { query, counter, ok }
    }

    pub fn ok(&self) -> bool {
        self.ok
    }

    /// Collect a fresh sample; return (lowercased instance name, value) for each
    /// valid instance.
    pub fn sample(&self) -> Vec<(String, f64)> {
        let mut out = Vec::new();
        if !self.ok {
            return out;
        }
        // SAFETY: PDH calls on our own handles. The first call sizes the buffer;
        // the second fills it. The buffer is `Vec<u64>` for 8-byte alignment
        // (PDH_FMT_COUNTERVALUE_ITEM_W holds a pointer + f64).
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return out;
            }
            let mut size = 0u32;
            let mut count = 0u32;
            PdhGetFormattedCounterArrayW(self.counter, PDH_FMT_DOUBLE, &mut size, &mut count, None);
            if size == 0 {
                return out;
            }
            let mut buf = vec![0u64; (size as usize).div_ceil(8)];
            let mut bufsize = (buf.len() * 8) as u32;
            let rc = PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut bufsize,
                &mut count,
                Some(buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W),
            );
            if rc != 0 {
                return out;
            }
            let items = core::slice::from_raw_parts(
                buf.as_ptr() as *const PDH_FMT_COUNTERVALUE_ITEM_W,
                count as usize,
            );
            for it in items {
                // CStatus 0 = PDH_CSTATUS_VALID_DATA, 1 = PDH_CSTATUS_NEW_DATA.
                let v = it.FmtValue.Anonymous.doubleValue;
                if it.FmtValue.CStatus <= 1 && v.is_finite() {
                    out.push((pwstr_to_string(it.szName), v));
                }
            }
        }
        out
    }
}

impl Drop for PdhCounter {
    fn drop(&mut self) {
        if !self.query.0.is_null() {
            // SAFETY: closes the query we opened in `new`.
            unsafe { PdhCloseQuery(self.query) };
        }
    }
}

/// Read a NUL-terminated wide Win32 string into a lowercased `String`.
pub(crate) unsafe fn pwstr_to_string(p: PWSTR) -> String {
    if p.0.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *p.0.add(len) != 0 {
        len += 1;
    }
    String::from_utf16_lossy(core::slice::from_raw_parts(p.0, len)).to_lowercase()
}
