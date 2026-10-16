//! GPU temperatures for the Temperature metric (Cargo feature `temp`), read
//! the way Task Manager reads them: the adapter performance data of D3DKMT
//! (`KMTQAITYPE_ADAPTERPERFDATA`, Windows 10 1803 / WDDM 2.4 and later), with
//! no admin rights and no extra software. The driver reports tenths of a
//! degree Celsius, and 0 when it reports no temperature (integrated GPUs
//! usually do not).

use core::ffi::c_void;
use core::mem::size_of;

use windows::Wdk::Graphics::Direct3D::{
    D3DKMTCloseAdapter, D3DKMTOpenAdapterFromLuid, D3DKMTQueryAdapterInfo, D3DKMT_ADAPTER_PERFDATA,
    D3DKMT_CLOSEADAPTER, D3DKMT_OPENADAPTERFROMLUID, D3DKMT_QUERYADAPTERINFO,
    KMTQAITYPE_ADAPTERPERFDATA,
};
use windows::Win32::Foundation::LUID;

/// Degrees Celsius from the driver's tenths of a degree, or None: 0 means the
/// driver reports no temperature, and a reading outside 1-150 C cannot be
/// real (the bound the ACPI zones use).
pub fn celsius(tenths: u32) -> Option<f64> {
    let c = f64::from(tenths) / 10.0;
    (1.0..150.0).contains(&c).then_some(c)
}

/// The temperature of the adapter with this LUID in degrees Celsius, or None:
/// the adapter cannot be opened (gone, or no WDDM driver), the driver does not
/// answer the query (before WDDM 2.4), or it reports no temperature. The
/// adapter is opened for the query only.
pub fn read(luid: LUID) -> Option<f64> {
    let mut open = D3DKMT_OPENADAPTERFROMLUID { AdapterLuid: luid, hAdapter: 0 };
    // SAFETY: D3DKMT calls with structs that live on this stack frame: the
    // open fills hAdapter, the query writes at most PrivateDriverDataSize
    // bytes into `perf`, and the handle is closed before returning.
    unsafe {
        if D3DKMTOpenAdapterFromLuid(&mut open).is_err() {
            return None;
        }
        let mut perf = D3DKMT_ADAPTER_PERFDATA::default();
        let mut query = D3DKMT_QUERYADAPTERINFO {
            hAdapter: open.hAdapter,
            Type: KMTQAITYPE_ADAPTERPERFDATA,
            pPrivateDriverData: &mut perf as *mut D3DKMT_ADAPTER_PERFDATA as *mut c_void,
            PrivateDriverDataSize: size_of::<D3DKMT_ADAPTER_PERFDATA>() as u32,
        };
        let answered = D3DKMTQueryAdapterInfo(&mut query).is_ok();
        let _ = D3DKMTCloseAdapter(&D3DKMT_CLOSEADAPTER { hAdapter: open.hAdapter });
        if answered {
            celsius(perf.Temperature)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{celsius, read};
    use windows::Win32::Foundation::LUID;

    #[test]
    fn celsius_reads_tenths_and_rejects_implausible_ones() {
        assert_eq!(celsius(0), None); // the driver reports none
        assert_eq!(celsius(452), Some(45.2));
        assert_eq!(celsius(10), Some(1.0));
        assert_eq!(celsius(9), None); // 0.9 C
        assert_eq!(celsius(1499), Some(149.9));
        assert_eq!(celsius(1500), None); // 150 C
        assert_eq!(celsius(u32::MAX), None);
    }

    /// Reads the live system: an adapter that does not exist gives None
    /// instead of failing.
    #[test]
    fn read_gives_none_for_an_unknown_adapter() {
        assert_eq!(read(LUID { LowPart: 0xFFFF_FFF0, HighPart: 0x7FFF_FFF0 }), None);
    }

    /// Reads the live system: every real adapter answers with nothing or a
    /// plausible temperature (the development machine's Intel UHD 730
    /// reports none).
    #[test]
    fn real_adapters_read_a_plausible_temperature_or_none() {
        for g in crate::gpu::detect_gpus() {
            if let Some(c) = read(g.luid()) {
                assert!((1.0..150.0).contains(&c), "{}: {c}", g.name);
            }
        }
    }
}
