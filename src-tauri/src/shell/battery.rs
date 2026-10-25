//! Battery state via Win32 `GetSystemPowerStatus`: the charge level, the
//! charging/AC state and the estimated remaining runtime — the data behind
//! the control center's battery widget. Works on laptops; desktops report
//! "no battery".

use serde::Serialize;
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

/// The machine's battery state. `percent` and `life_seconds` are `None`
/// when Windows reports them as unknown (or when no battery exists).
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct BatterySnapshot {
    pub has_battery: bool,
    pub percent: Option<u8>,
    pub charging: bool,
    pub ac_connected: bool,
    pub life_seconds: Option<u32>,
}

/// Reads the battery state; an API failure reads as "no battery" so the
/// widget degrades to a desktop-friendly "Not available".
pub fn battery_snapshot() -> BatterySnapshot {
    let mut status = SYSTEM_POWER_STATUS::default();
    let ok = unsafe { GetSystemPowerStatus(&mut status) }.is_ok();
    if !ok {
        return BatterySnapshot {
            has_battery: false,
            percent: None,
            charging: false,
            ac_connected: false,
            life_seconds: None,
        };
    }
    // 128 = no system battery; 255 = unknown state.
    let has_battery = status.BatteryFlag != 128 && status.BatteryFlag != 255;
    let percent = (status.BatteryLifePercent != 255 && has_battery)
        .then_some(status.BatteryLifePercent);
    BatterySnapshot {
        has_battery,
        percent,
        charging: has_battery && (status.BatteryFlag & 8 != 0),
        ac_connected: status.ACLineStatus == 1,
        // 0xFFFFFFFF = the estimated runtime is unknown (common while
        // charging or right after boot).
        life_seconds: (status.BatteryLifeTime != u32::MAX)
            .then_some(status.BatteryLifeTime),
    }
}
