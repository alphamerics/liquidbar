//! Wi-Fi and Bluetooth radio state via the WinRT radios API — the same
//! interface Windows' own quick settings uses, so toggling a radio on or off
//! needs no admin rights and no driver-specific hacks.

use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioKind, RadioState};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

/// The radio of the given kind (Wi-Fi or Bluetooth), or `None` when the
/// machine has no such radio (or the API is unavailable).
fn kind_radio(kind: RadioKind) -> Result<Option<Radio>, String> {
    // WinRT needs COM initialized on this thread; S_FALSE (already
    // initialized) is fine too.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let radios = Radio::GetRadiosAsync()
        .map_err(|error| error.to_string())?
        .get()
        .map_err(|error| error.to_string())?;
    for index in 0..radios.Size().map_err(|error| error.to_string())? {
        if let Ok(radio) = radios.GetAt(index) {
            if radio.Kind().map_err(|error| error.to_string())? == kind {
                return Ok(Some(radio));
            }
        }
    }
    Ok(None)
}

fn radio_enabled(kind: RadioKind) -> Result<bool, String> {
    Ok(kind_radio(kind)?
        .map(|radio| {
            radio
                .State()
                .map(|state| state == RadioState::On)
                .unwrap_or(false)
        })
        .unwrap_or(false))
}

fn set_radio_enabled(kind: RadioKind, enabled: bool) -> Result<(), String> {
    let Some(radio) = kind_radio(kind)? else {
        return Err("No such radio on this system".to_owned());
    };
    let target = if enabled {
        RadioState::On
    } else {
        RadioState::Off
    };
    let status = radio
        .SetStateAsync(target)
        .map_err(|error| error.to_string())?
        .get()
        .map_err(|error| error.to_string())?;
    if status != RadioAccessStatus::Allowed {
        return Err(format!("The radio declined the change (status {status:?})"));
    }
    Ok(())
}

/// Whether the Wi-Fi radio is on. No Wi-Fi radio reads as off.
pub fn wifi_enabled() -> Result<bool, String> {
    radio_enabled(RadioKind::WiFi)
}

pub fn set_wifi_enabled(enabled: bool) -> Result<(), String> {
    set_radio_enabled(RadioKind::WiFi, enabled)
}

/// Whether the Bluetooth radio is on. No Bluetooth radio reads as off.
pub fn bluetooth_enabled() -> Result<bool, String> {
    radio_enabled(RadioKind::Bluetooth)
}

pub fn set_bluetooth_enabled(enabled: bool) -> Result<(), String> {
    set_radio_enabled(RadioKind::Bluetooth, enabled)
}
