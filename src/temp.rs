//! Optional CPU/GPU temperature via LibreHardwareMonitor's WMI namespace
//! (`ROOT\LibreHardwareMonitor`), reading `Sensor` rows where
//! `SensorType='Temperature'`. Vendor-neutral and admin-free, but it only
//! produces data when LibreHardwareMonitor is running; otherwise it shows
//! "n/a" and never fails. Gated behind the `temp` Cargo feature so the default
//! build pulls in no COM/WMI bindings.

use windows::core::{w, BSTR, PCWSTR};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, CLSCTX_INPROC_SERVER,
    COINIT_MULTITHREADED, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL, RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Variant::{
    VariantChangeType, VariantClear, VARIANT, VAR_CHANGE_FLAGS, VT_BSTR, VT_R8,
};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, IWbemServices, WbemLocator, WBEM_FLAG_FORWARD_ONLY,
    WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_INFINITE,
};

use crate::pdh::PdhCounter;
use crate::registry::Row;

pub struct TempSource {
    services: Option<IWbemServices>,
    sensors: Vec<(String, f64)>,
    /// Native fallback: the ACPI thermal zone(s) Windows exposes through PDH
    /// (no admin, no LHM) — gives a CPU/system temperature when LHM isn't running.
    zone: PdhCounter,
    native: Vec<f64>,
}

impl TempSource {
    pub fn new() -> Self {
        TempSource {
            services: connect(),
            sensors: Vec::new(),
            zone: PdhCounter::new(w!("\\Thermal Zone Information(*)\\Temperature")),
            native: Vec::new(),
        }
    }

    pub fn sample(&mut self) {
        self.sensors.clear();
        if let Some(svc) = &self.services {
            // SAFETY: WMI COM calls on a connected services interface; every
            // result is checked and all VARIANTs are cleared.
            unsafe { query_temps(svc, &mut self.sensors) };
        }
        // Native ACPI thermal-zone temps (the counter reports Kelvin); keep only
        // plausible readings so a bogus/zero zone never shows a wild value.
        self.native = self
            .zone
            .sample()
            .iter()
            .map(|(_, k)| k - 273.15)
            .filter(|c| (1.0..150.0).contains(c))
            .collect();
    }

    pub fn rows(&self, out: &mut Vec<Row>) {
        if !self.sensors.is_empty() {
            // LHM is running: accurate, named sensors (CPU Package, GPU Core, ...).
            for (name, c) in &self.sensors {
                out.push(temp_row(name, *c));
            }
        } else if !self.native.is_empty() {
            // Fallback: native Windows thermal zone(s) — a CPU/system temperature.
            let one = self.native.len() == 1;
            for (i, c) in self.native.iter().enumerate() {
                let label = if one { "CPU Temp".to_string() } else { format!("Temp {}", i + 1) };
                out.push(temp_row(&label, *c));
            }
        } else {
            // Nothing available (no LHM, no thermal zone). Keep the metric visible.
            out.push(Row {
                label: "Temp".into(),
                value: "n/a".into(),
                bar: None,
                color: None,
                bar_color: None,
            });
        }
    }
}

/// Connect to the LHM WMI namespace; `None` if LHM isn't installed/running.
fn connect() -> Option<IWbemServices> {
    // SAFETY: standard WMI COM connect sequence; each step is checked and any
    // failure (e.g. namespace absent) returns None.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let locator: IWbemLocator =
            CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER).ok()?;
        let services = locator
            .ConnectServer(
                &BSTR::from("ROOT\\LibreHardwareMonitor"),
                &BSTR::new(),
                &BSTR::new(),
                &BSTR::new(),
                0,
                &BSTR::new(),
                None,
            )
            .ok()?;
        // Best-effort RPC auth on the proxy (RPC_C_AUTHN_WINNT=10, RPC_C_AUTHZ_NONE=0).
        let _ = CoSetProxyBlanket(
            &services,
            10,
            0,
            PCWSTR::null(),
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        );
        Some(services)
    }
}

unsafe fn query_temps(svc: &IWbemServices, out: &mut Vec<(String, f64)>) {
    let Ok(enumerator) = svc.ExecQuery(
        &BSTR::from("WQL"),
        &BSTR::from("SELECT Name, Value FROM Sensor WHERE SensorType='Temperature'"),
        WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
        None,
    ) else {
        return;
    };
    loop {
        let mut objs: [Option<IWbemClassObject>; 1] = [None];
        let mut returned = 0u32;
        let _ = enumerator.Next(WBEM_INFINITE, &mut objs, &mut returned);
        if returned == 0 {
            break;
        }
        if let Some(obj) = &objs[0] {
            if let (Some(name), Some(c)) = (get_bstr(obj, w!("Name")), get_f64(obj, w!("Value"))) {
                out.push((name, c));
            }
        }
    }
}

unsafe fn get_bstr(obj: &IWbemClassObject, name: PCWSTR) -> Option<String> {
    let mut v = VARIANT::default();
    if obj.Get(name, 0, &mut v, None, None).is_err() {
        return None;
    }
    let result = if v.Anonymous.Anonymous.vt == VT_BSTR {
        Some(v.Anonymous.Anonymous.Anonymous.bstrVal.to_string())
    } else {
        None
    };
    let _ = VariantClear(&mut v);
    result
}

unsafe fn get_f64(obj: &IWbemClassObject, name: PCWSTR) -> Option<f64> {
    let mut v = VARIANT::default();
    if obj.Get(name, 0, &mut v, None, None).is_err() {
        return None;
    }
    let mut coerced = VARIANT::default();
    let value = if VariantChangeType(&mut coerced, &v, VAR_CHANGE_FLAGS(0), VT_R8).is_ok() {
        Some(coerced.Anonymous.Anonymous.Anonymous.dblVal)
    } else {
        None
    };
    let _ = VariantClear(&mut coerced);
    let _ = VariantClear(&mut v);
    value
}

pub fn temp_row(name: &str, celsius: f64) -> Row {
    Row::bar(name, format!("{:.0}\u{00B0}C", celsius), (celsius / 100.0).clamp(0.0, 1.0) as f32)
}

#[cfg(test)]
mod tests {
    use super::temp_row;

    #[test]
    fn formats() {
        let r = temp_row("CPU", 55.0);
        assert_eq!(r.label, "CPU");
        assert_eq!(r.value, "55\u{00B0}C");
        assert!((r.bar.unwrap() - 0.55).abs() < 1e-6);
    }
}
