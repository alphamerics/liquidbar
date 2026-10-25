//! Airplane mode via the WinRT radio API: turns every radio (Wi-Fi,
//! Bluetooth, mobile broadband) off — and back on.

use windows::Devices::Radios::{Radio, RadioState};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

/// Toggles every controllable radio. `enabled` (airplane on) switches all
/// radios off; disabling turns them back on.
pub fn set_airplane_mode(enabled: bool) -> Result<(), String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    let radios = Radio::GetRadiosAsync()
        .map_err(|error| error.to_string())?
        .get()
        .map_err(|error| error.to_string())?;
    let target = if enabled {
        RadioState::Off
    } else {
        RadioState::On
    };
    let mut failures = 0;
    for index in 0..radios.Size().map_err(|error| error.to_string())? {
        let radio = match radios.GetAt(index) {
            Ok(radio) => radio,
            Err(_) => continue,
        };
        // Some radio kinds (FM, etc.) decline state changes; skip them.
        if radio
            .SetStateAsync(target)
            .and_then(|operation| operation.get())
            .is_err()
        {
            failures += 1;
        }
    }
    if failures > 0 {
        eprintln!("[airplane] {failures} radio(s) declined the state change");
    }
    Ok(())
}
