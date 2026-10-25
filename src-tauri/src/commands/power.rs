//! Power/battery commands: the battery snapshot behind the control center's
//! battery widget.

use crate::shell::BatterySnapshot;

/// The battery state (percent, charging/AC, estimated runtime). Desktops
/// without a battery report `hasBattery = false`.
#[tauri::command]
pub fn battery_snapshot() -> BatterySnapshot {
    crate::shell::battery_snapshot()
}
