//! Night light via the display gamma ramp: a warm hardware gamma ramp is
//! applied to every monitor's DC (blue reduced, red/green kept), and the
//! original ramp is restored when disabled. This works on any Windows
//! machine — unlike the registry-based OS night light, which has no public
//! API — and is instantly reversible.
//!
//! Two failure modes are handled explicitly instead of silently doing
//! nothing: HDR (advanced color) makes the OS ignore gamma ramps entirely,
//! so it is detected up front and reported; and each monitor gets its own DC
//! built from its device name, because the default display DC only covers
//! the primary monitor.

use std::sync::Mutex;
use windows::core::{w, PCWSTR};
use windows::Win32::Devices::Display::{
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig,
    DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO, DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, QDC_ONLY_ACTIVE_PATHS,
};
use windows::Win32::Foundation::{BOOL, ERROR_SUCCESS, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    CreateDCW, DeleteDC, EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    MONITORINFOEXW,
};
use windows::Win32::UI::ColorSystem::{GetDeviceGammaRamp, SetDeviceGammaRamp};

/// The ramps captured before night light was enabled, keyed by monitor
/// device name — restored exactly on disable.
static ORIGINAL_RAMPS: Mutex<Vec<(String, [u16; 768])>> = Mutex::new(Vec::new());

unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let dcs = &mut *(lparam.0 as *mut Vec<(String, HDC)>);
    // Ask the monitor for its own device name (\\.\DISPLAY1, ...): a DC for
    // the default display only covers the primary monitor, so multi-monitor
    // machines would leave every other display untouched.
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    let name = if GetMonitorInfoW(monitor, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO)
        .as_bool()
    {
        let len = info
            .szDevice
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(info.szDevice.len());
        String::from_utf16_lossy(&info.szDevice[..len])
    } else {
        String::new()
    };
    // A private DC per monitor lets the ramp apply to every display.
    let dc = if name.is_empty() {
        CreateDCW(w!("DISPLAY"), None, None, None)
    } else {
        CreateDCW(w!("DISPLAY"), PCWSTR::from_raw(info.szDevice.as_ptr()), None, None)
    };
    if !dc.is_invalid() {
        dcs.push((name, dc));
    }
    BOOL(1)
}

fn warm_ramp(warm: bool) -> [u16; 768] {
    let mut ramp = [0u16; 768];
    for i in 0..256usize {
        let linear = ((i << 8) | i | (i >> 7)) as u16; // 0..65535 linearization
        let (r, g, b) = if warm {
            (
                linear,
                (f64::from(linear) * 0.88) as u16,
                (f64::from(linear) * 0.68) as u16,
            )
        } else {
            (linear, linear, linear)
        };
        ramp[i] = r;
        ramp[256 + i] = g;
        ramp[512 + i] = b;
    }
    ramp
}

/// True when any active display has advanced color (HDR) enabled: the OS
/// ignores gamma ramps while HDR is active, so night light would silently
/// do nothing. The v1 query reports the user's advanced-color setting
/// (`advancedColorEnabled`, bit 1 of the value union).
fn hdr_active() -> bool {
    unsafe {
        let mut path_count = 0u32;
        let mut mode_count = 0u32;
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
            != ERROR_SUCCESS
        {
            return false;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
        if QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            None,
        )
        .is_err()
        {
            return false;
        }
        for path in &paths[..path_count as usize] {
            let mut color = DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO::default();
            color.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO;
            color.header.size = std::mem::size_of::<DISPLAYCONFIG_GET_ADVANCED_COLOR_INFO>() as u32;
            color.header.adapterId = path.targetInfo.adapterId;
            color.header.id = path.targetInfo.id;
            if DisplayConfigGetDeviceInfo(&mut color.header) != 0 {
                continue;
            }
            if color.Anonymous.value & 0b10 != 0 {
                return true;
            }
        }
        false
    }
}

/// Enables (or disables) night light on every monitor.
pub fn set_night_light(enabled: bool) -> Result<(), String> {
    unsafe {
        if enabled && hdr_active() {
            return Err(
                "Night light has no effect while HDR is on — turn HDR off first".to_string(),
            );
        }
        let mut dcs: Vec<(String, HDC)> = Vec::new();
        let dcs_ptr = &mut dcs as *mut Vec<(String, HDC)> as isize;
        let _ = EnumDisplayMonitors(HDC::default(), None, Some(collect_monitor), LPARAM(dcs_ptr));
        if dcs.is_empty() {
            return Err("No display was found to apply the night light to".to_string());
        }
        let mut errors = 0;
        for (name, dc) in &dcs {
            if enabled {
                // Capture the original ramp once per monitor, so disabling
                // restores exactly what the driver had (even if custom).
                let mut original = [0u16; 768];
                if GetDeviceGammaRamp(*dc, original.as_mut_ptr().cast()).as_bool() {
                    let mut ramps = ORIGINAL_RAMPS
                        .lock()
                        .expect("night light ramp lock poisoned");
                    ramps.retain(|(device, _)| device != name);
                    ramps.push((name.clone(), original));
                }
                let ramp = warm_ramp(true);
                if !SetDeviceGammaRamp(*dc, ramp.as_ptr().cast()).as_bool() {
                    errors += 1;
                }
            } else {
                // Fall back to the identity ramp when no original was
                // captured (e.g. the app restarted while night light was on).
                let ramp = {
                    let mut ramps = ORIGINAL_RAMPS
                        .lock()
                        .expect("night light ramp lock poisoned");
                    ramps
                        .iter()
                        .position(|(device, _)| device == name)
                        .map(|index| ramps.remove(index).1)
                        .unwrap_or_else(|| warm_ramp(false))
                };
                if !SetDeviceGammaRamp(*dc, ramp.as_ptr().cast()).as_bool() {
                    errors += 1;
                }
            }
            let _ = DeleteDC(*dc);
        }
        if !enabled {
            ORIGINAL_RAMPS
                .lock()
                .expect("night light ramp lock poisoned")
                .clear();
        }
        if errors > 0 {
            return Err(format!(
                "{errors} monitor(s) rejected the gamma ramp — a remote session or the display driver may not support it"
            ));
        }
        Ok(())
    }
}

/// Restores the original ramps so the display is never left warm.
pub fn reset_night_light() {
    let _ = set_night_light(false);
}
