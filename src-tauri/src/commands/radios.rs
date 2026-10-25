//! Radio commands: Wi-Fi, Bluetooth, airplane mode and night light — all
//! blocking WinRT calls, so each runs on a blocking worker.

use crate::shell;

#[tauri::command]
pub async fn wifi_networks() -> Result<Vec<shell::WifiNetwork>, String> {
    tauri::async_runtime::spawn_blocking(shell::wifi_networks)
        .await
        .map_err(|error| error.to_string())?
}

/// Joins a Wi-Fi network. Saved/open networks ignore the password; new
/// secure networks save a profile with it first. Blocks until the join
/// settles (or fails) — runs on a blocking worker.
#[tauri::command]
pub async fn wifi_connect(ssid: String, password: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || shell::wifi_connect(&ssid, &password))
        .await
        .map_err(|error| error.to_string())?
}

/// Disconnects the Wi-Fi adapter from its current network. Runs on a
/// blocking worker.
#[tauri::command]
pub async fn wifi_disconnect() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(shell::wifi_disconnect)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn wifi_enabled() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(shell::wifi_enabled)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn set_wifi_enabled(enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || shell::set_wifi_enabled(enabled))
        .await
        .map_err(|error| error.to_string())?
}

/// Paired and connected Bluetooth devices for the control center view.
#[tauri::command]
pub async fn bluetooth_devices() -> Result<Vec<shell::BluetoothDevice>, String> {
    tauri::async_runtime::spawn_blocking(shell::bluetooth_devices)
        .await
        .map_err(|error| error.to_string())?
}

/// Pairs an unpaired Bluetooth device (by WinRT AEP id). Blocks until the
/// pairing settles — confirmation/passkey devices show Windows' own pairing
/// panel. Runs on a blocking worker.
#[tauri::command]
pub async fn bluetooth_pair(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || shell::bluetooth_pair(&id))
        .await
        .map_err(|error| error.to_string())?
}

/// Unpairs a paired Bluetooth device (by WinRT AEP id). Runs on a blocking
/// worker.
#[tauri::command]
pub async fn bluetooth_unpair(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || shell::bluetooth_unpair(&id))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn bluetooth_enabled() -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(shell::bluetooth_enabled)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn set_bluetooth_enabled(enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || shell::set_bluetooth_enabled(enabled))
        .await
        .map_err(|error| error.to_string())?
}

/// Airplane mode: switches every radio (Wi-Fi, Bluetooth, mobile broadband)
/// off — and back on. WinRT needs COM, so this runs on a blocking worker.
#[tauri::command]
pub async fn set_airplane_mode(enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || shell::set_airplane_mode(enabled))
        .await
        .map_err(|error| error.to_string())?
}

/// Night light via the display gamma ramp (see shell::night_light).
#[tauri::command]
pub async fn set_night_light(enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || shell::set_night_light(enabled))
        .await
        .map_err(|error| error.to_string())?
}
