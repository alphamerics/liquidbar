//! Bluetooth device enumeration, pairing and unpairing via the WinRT
//! device-enumeration API — the same interface Windows' own Bluetooth
//! settings page uses. Every Bluetooth AEP (classic + LE) that Windows has
//! seen is listed with real paired/connected flags; unpaired devices can be
//! paired and paired devices unpaired on click.

use serde::Serialize;
use windows::core::HSTRING;
use windows::Devices::Bluetooth::{BluetoothConnectionStatus, BluetoothLEDevice};
use windows::Devices::Enumeration::{
    DeviceInformation, DeviceInformationKind, DevicePairingResultStatus,
    DeviceUnpairingResultStatus,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

/// Every Bluetooth AEP: classic (E0CBF06C…) and LE (BB7BB05E…) devices,
/// paired or not. Windows caches devices it has seen, so neighbors show up
/// without a blocking discovery inquiry.
const SELECTOR: &str = "(System.Devices.Aep.ProtocolId:=\"{E0CBF06C-CD8B-4647-BB8A-263B43F0F974}\" \
                         OR System.Devices.Aep.ProtocolId:=\"{BB7BB05E-5972-42B5-94FC-76EAA7084D49}\")";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BluetoothDevice {
    /// The WinRT AEP id — the stable handle for pairing/unpairing.
    pub id: String,
    pub name: String,
    pub connected: bool,
    pub paired: bool,
}

fn winrt() -> Result<(), String> {
    // WinRT needs COM initialized on this thread; S_FALSE (already
    // initialized) is fine too.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    Ok(())
}

/// The remote device's Bluetooth address parsed out of the AEP id, which
/// ends with `{local adapter MAC}-{device MAC}` (colons included).
fn address_from_id(id: &str) -> Option<u64> {
    let mac = id.rsplit('-').next()?;
    let digits: String = mac.chars().filter(|character| *character != ':').collect();
    u64::from_str_radix(&digits, 16).ok()
}

/// Whether the device behind an AEP id is connected right now. Resolves
/// through the classic Bluetooth API first, then the LE one (a device only
/// lives in one of them). Unpaired or unknown devices read as disconnected.
fn is_connected(id: &str) -> bool {
    let Some(address) = address_from_id(id) else {
        return false;
    };
    let connection =
        windows::Devices::Bluetooth::BluetoothDevice::FromBluetoothAddressAsync(address)
            .ok()
            .and_then(|operation| operation.get().ok())
            .and_then(|device| device.ConnectionStatus().ok());
    if let Some(status) = connection {
        return status == BluetoothConnectionStatus::Connected;
    }
    let le_connection = BluetoothLEDevice::FromBluetoothAddressAsync(address)
        .ok()
        .and_then(|operation| operation.get().ok())
        .and_then(|device| device.ConnectionStatus().ok());
    matches!(le_connection, Some(BluetoothConnectionStatus::Connected))
}

/// Every Bluetooth device Windows knows about (paired, remembered, or seen
/// nearby), connected first then alphabetical. Duplicate names (the same
/// physical device often exposes a classic and an LE AEP) collapse into the
/// most informative entry.
pub fn devices() -> Result<Vec<BluetoothDevice>, String> {
    winrt()?;
    // AEP properties are only honored with the AssociationEndpoint kind —
    // the default Device kind ignores the selector and returns nothing.
    let additional: Option<&windows::Foundation::Collections::IIterable<HSTRING>> = None;
    let collection = DeviceInformation::FindAllAsyncWithKindAqsFilterAndAdditionalProperties(
        &HSTRING::from(SELECTOR),
        additional,
        DeviceInformationKind::AssociationEndpoint,
    )
    .map_err(|error| format!("Bluetooth enumeration failed: {error}"))?
    .get()
    .map_err(|error| format!("Bluetooth enumeration failed: {error}"))?;
    let mut devices: Vec<BluetoothDevice> = Vec::new();
    for index in 0..collection
        .Size()
        .map_err(|error| format!("Bluetooth enumeration failed: {error}"))?
    {
        let Ok(device) = collection.GetAt(index) else {
            continue;
        };
        let Ok(name) = device.Name() else {
            continue;
        };
        let name = name.to_string();
        // Anonymous AEPs (raw MACs with no advertising name) are noise.
        if name.is_empty() {
            continue;
        }
        let Ok(id) = device.Id() else {
            continue;
        };
        let id = id.to_string();
        let paired = device
            .Pairing()
            .and_then(|pairing| pairing.IsPaired())
            .unwrap_or(false);
        let connected = if paired { is_connected(&id) } else { false };
        let entry = BluetoothDevice {
            id,
            name: name.clone(),
            connected,
            paired,
        };
        // Prefer an entry that is connected or paired when the same device
        // appears under several protocols.
        match devices.iter_mut().find(|existing| existing.name == name) {
            Some(existing) => {
                if (entry.connected && !existing.connected)
                    || (entry.connected == existing.connected && entry.paired && !existing.paired)
                {
                    *existing = entry;
                }
            }
            None => devices.push(entry),
        }
    }
    devices.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then(b.paired.cmp(&a.paired))
            .then(a.name.cmp(&b.name))
    });
    Ok(devices)
}

/// The DeviceInformation behind an AEP id.
fn device_information(id: &str) -> Result<DeviceInformation, String> {
    DeviceInformation::CreateFromIdAsync(&HSTRING::from(id))
        .map_err(|error| format!("Bluetooth device {id} not found: {error}"))?
        .get()
        .map_err(|error| format!("Bluetooth device {id} not found: {error}"))
}

/// Pairs an unpaired device. Blocks until the pairing settles — for
/// passkey/confirmation devices Windows shows its own pairing panel, which
/// the user completes there. Runs on a blocking worker.
pub fn pair(id: &str) -> Result<(), String> {
    winrt()?;
    let device = device_information(id)?;
    let pairing = device
        .Pairing()
        .map_err(|error| format!("Bluetooth pairing unavailable: {error}"))?;
    if pairing.IsPaired().unwrap_or(false) {
        return Ok(());
    }
    let result = pairing
        .PairAsync()
        .map_err(|error| format!("Could not start pairing: {error}"))?
        .get()
        .map_err(|error| format!("Could not start pairing: {error}"))?;
    let status = result
        .Status()
        .map_err(|error| format!("Could not read the pairing result: {error}"))?;
    match status {
        DevicePairingResultStatus::Paired | DevicePairingResultStatus::AlreadyPaired => Ok(()),
        _ => Err(format!("Pairing failed (status {status:?})")),
    }
}

/// Unpairs a paired device. Runs on a blocking worker.
pub fn unpair(id: &str) -> Result<(), String> {
    winrt()?;
    let device = device_information(id)?;
    let pairing = device
        .Pairing()
        .map_err(|error| format!("Bluetooth pairing unavailable: {error}"))?;
    let result = pairing
        .UnpairAsync()
        .map_err(|error| format!("Could not start unpairing: {error}"))?
        .get()
        .map_err(|error| format!("Could not start unpairing: {error}"))?;
    let status = result
        .Status()
        .map_err(|error| format!("Could not read the unpairing result: {error}"))?;
    match status {
        DeviceUnpairingResultStatus::Unpaired => Ok(()),
        _ => Err(format!("Unpairing failed (status {status:?})")),
    }
}
