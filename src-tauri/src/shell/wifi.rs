//! Wi-Fi network enumeration, connecting and joining via the Native WiFi
//! API. Lists the networks visible to every WLAN interface (deduplicated by
//! SSID) and marks saved (profiled) and connected networks; joining an open
//! or password-protected network writes a profile and connects.

use serde::Serialize;
use windows::core::{GUID, PCWSTR};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::WiFi::{
    dot11_BSS_type_infrastructure, wlan_connection_mode_profile, wlan_interface_state_connected,
    wlan_interface_state_disconnected, wlan_intf_opcode_interface_state, WlanCloseHandle,
    WlanConnect, WlanDisconnect, WlanEnumInterfaces, WlanFreeMemory, WlanGetAvailableNetworkList,
    WlanGetProfileList, WlanOpenHandle, WlanQueryInterface, WlanScan, WlanSetProfile,
    WLAN_AVAILABLE_NETWORK_CONNECTED, WLAN_CONNECTION_PARAMETERS, WLAN_INTERFACE_INFO_LIST,
    WLAN_INTERFACE_STATE, WLAN_PROFILE_INFO_LIST,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WifiNetwork {
    pub ssid: String,
    /// 0-100 signal quality, as reported by the WLAN driver.
    pub signal: u32,
    pub connected: bool,
    pub secure: bool,
    /// Whether a saved profile exists for this SSID — joining it needs no
    /// password prompt.
    pub saved: bool,
}

fn ssid_string(ssid: &windows::Win32::NetworkManagement::WiFi::DOT11_SSID) -> String {
    let length = ssid.uSSIDLength as usize;
    let bytes = &ssid.ucSSID[..length.min(ssid.ucSSID.len())];
    String::from_utf8_lossy(bytes)
        .trim_end_matches('\0')
        .to_string()
}

fn open_wlan() -> Result<HANDLE, String> {
    unsafe {
        let mut negotiated = 0u32;
        let mut handle = HANDLE::default();
        if WlanOpenHandle(2, None, &mut negotiated, &mut handle) != 0 {
            return Err("No Wi-Fi service on this system".to_owned());
        }
        Ok(handle)
    }
}

/// The first WLAN interface's GUID, or an error when there is no adapter.
fn first_interface(handle: HANDLE) -> Result<GUID, String> {
    unsafe {
        let mut interfaces: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
        if WlanEnumInterfaces(handle, None, &mut interfaces) != 0 || interfaces.is_null() {
            return Err("No Wi-Fi adapter found".to_owned());
        }
        let list = &*interfaces;
        let count = list.dwNumberOfItems as usize;
        if count == 0 {
            WlanFreeMemory(interfaces.cast());
            return Err("No Wi-Fi adapter found".to_owned());
        }
        let infos = std::slice::from_raw_parts(list.InterfaceInfo.as_ptr(), count);
        let guid = infos[0].InterfaceGuid;
        WlanFreeMemory(interfaces.cast());
        Ok(guid)
    }
}

/// Profile names of every saved network on this interface. Windows names
/// profiles after their SSID by default.
fn saved_ssids(handle: HANDLE, interface: &GUID) -> Vec<String> {
    unsafe {
        let mut list: *mut WLAN_PROFILE_INFO_LIST = std::ptr::null_mut();
        if WlanGetProfileList(handle, interface, None, &mut list) != 0 || list.is_null() {
            return Vec::new();
        }
        let list_ref = &*list;
        let items = std::slice::from_raw_parts(
            list_ref.ProfileInfo.as_ptr(),
            list_ref.dwNumberOfItems as usize,
        );
        let names = items
            .iter()
            .map(|profile| {
                let end = profile
                    .strProfileName
                    .iter()
                    .position(|c| *c == 0)
                    .unwrap_or(profile.strProfileName.len());
                String::from_utf16_lossy(&profile.strProfileName[..end])
            })
            .collect();
        WlanFreeMemory(list.cast());
        names
    }
}

/// Reads the available-network list of one interface into `networks`.
fn collect_interface(
    handle: HANDLE,
    interface: &GUID,
    saved: &[String],
    networks: &mut Vec<WifiNetwork>,
) {
    unsafe {
        let mut net_list: *mut windows::Win32::NetworkManagement::WiFi::WLAN_AVAILABLE_NETWORK_LIST =
            std::ptr::null_mut();
        if WlanGetAvailableNetworkList(handle, interface, 0, None, &mut net_list) != 0
            || net_list.is_null()
        {
            return;
        }
        let available = &*net_list;
        let items = std::slice::from_raw_parts(
            available.Network.as_ptr(),
            available.dwNumberOfItems as usize,
        );
        for network in items {
            let ssid = ssid_string(&network.dot11Ssid);
            if ssid.is_empty() {
                // Hidden networks have no usable display name.
                continue;
            }
            let connected = (network.dwFlags & WLAN_AVAILABLE_NETWORK_CONNECTED) != 0;
            let entry = WifiNetwork {
                ssid: ssid.clone(),
                signal: network.wlanSignalQuality.min(100),
                connected,
                secure: network.bSecurityEnabled.as_bool(),
                saved: saved.iter().any(|name| name.eq_ignore_ascii_case(&ssid)),
            };
            // The list can contain the same SSID several times (one entry
            // per BSS/profile); prefer the connected entry, then the
            // strongest signal.
            match networks
                .iter_mut()
                .find(|existing| existing.ssid == entry.ssid)
            {
                Some(existing) => {
                    if (connected && !existing.connected)
                        || (connected == existing.connected && entry.signal > existing.signal)
                    {
                        *existing = entry;
                    }
                }
                None => networks.push(entry),
            }
        }
        WlanFreeMemory(net_list.cast());
    }
}

/// The networks visible right now. An empty result means no Wi-Fi adapter or
/// no networks — the UI treats that as "nothing to show".
///
/// A fresh scan is triggered first: Windows refreshes its scan cache only
/// opportunistically, so without an explicit scan the list can hold just the
/// currently connected network.
pub fn networks() -> Result<Vec<WifiNetwork>, String> {
    unsafe {
        let handle = open_wlan()?;
        let mut interfaces: *mut WLAN_INTERFACE_INFO_LIST = std::ptr::null_mut();
        if WlanEnumInterfaces(handle, None, &mut interfaces) != 0 || interfaces.is_null() {
            let _ = WlanCloseHandle(handle, None);
            return Err("No Wi-Fi adapter found".to_owned());
        }
        let list = &*interfaces;
        let count = list.dwNumberOfItems as usize;
        if count == 0 {
            WlanFreeMemory(interfaces.cast());
            let _ = WlanCloseHandle(handle, None);
            return Err("No Wi-Fi adapter found".to_owned());
        }
        let infos = std::slice::from_raw_parts(list.InterfaceInfo.as_ptr(), count);
        for info in infos {
            // The scan completes asynchronously; every interface gets its
            // own request before any waiting starts.
            let _ = WlanScan(handle, &info.InterfaceGuid, None, None, None);
        }
        // Wait for the scan results to land, polling the cache: one
        // network is a plausible (connected-only) stale result, so keep
        // reading until more than one shows up, the count stabilizes, or
        // the deadline passes.
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2500);
        std::thread::sleep(std::time::Duration::from_millis(350));
        let mut networks: Vec<WifiNetwork> = Vec::new();
        let mut stable_for = 0;
        loop {
            networks.clear();
            for info in infos {
                let saved = saved_ssids(handle, &info.InterfaceGuid);
                collect_interface(handle, &info.InterfaceGuid, &saved, &mut networks);
            }
            stable_for = if networks.len() == stable_for {
                stable_for + 1
            } else {
                0
            };
            let settled = networks.len() > 1 && stable_for >= 1;
            let timed_out = std::time::Instant::now() >= deadline;
            if settled || timed_out {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        WlanFreeMemory(interfaces.cast());
        let _ = WlanCloseHandle(handle, None);
        networks.sort_by(|a, b| {
            b.connected
                .cmp(&a.connected)
                .then(b.signal.cmp(&a.signal))
                .then(a.ssid.cmp(&b.ssid))
        });
        Ok(networks)
    }
}

/// Disconnects the first WLAN interface from its current network.
pub fn disconnect() -> Result<(), String> {
    unsafe {
        let handle = open_wlan()?;
        let interface = match first_interface(handle) {
            Ok(interface) => interface,
            Err(error) => {
                let _ = WlanCloseHandle(handle, None);
                return Err(error);
            }
        };
        let result = WlanDisconnect(handle, &interface, None);
        let _ = WlanCloseHandle(handle, None);
        if result == 0 {
            Ok(())
        } else {
            Err(format!("Windows refused the disconnect ({result})"))
        }
    }
}

/// Escapes text for embedding into the profile XML.
fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Maps a scanned network's default auth algorithm to the profile XML's
/// `<authentication>` / `<encryption>` pair and whether it takes a key.
fn auth_pair(auth: i32, secure: bool) -> (&'static str, &'static str, bool) {
    use windows::Win32::NetworkManagement::WiFi::{
        DOT11_AUTH_ALGO_80211_OPEN, DOT11_AUTH_ALGO_RSNA_PSK, DOT11_AUTH_ALGO_WPA3_SAE,
        DOT11_AUTH_ALGO_WPA_PSK,
    };
    if auth == DOT11_AUTH_ALGO_WPA3_SAE.0 {
        ("WPA3SAE", "AES", true)
    } else if auth == DOT11_AUTH_ALGO_RSNA_PSK.0 {
        ("WPA2PSK", "AES", true)
    } else if auth == DOT11_AUTH_ALGO_WPA_PSK.0 {
        ("WPAPSK", "TKIP", true)
    } else if auth == DOT11_AUTH_ALGO_80211_OPEN.0 {
        ("open", "none", false)
    } else {
        // Unknown algorithms fall back on the scan's security flag.
        if secure {
            ("WPA2PSK", "AES", true)
        } else {
            ("open", "none", false)
        }
    }
}

/// The auth algorithm the scan reported for an SSID, if it is still visible.
fn scanned_auth(handle: HANDLE, interface: &GUID, ssid: &str) -> Option<(i32, bool)> {
    unsafe {
        let mut net_list: *mut windows::Win32::NetworkManagement::WiFi::WLAN_AVAILABLE_NETWORK_LIST =
            std::ptr::null_mut();
        if WlanGetAvailableNetworkList(handle, interface, 0, None, &mut net_list) != 0
            || net_list.is_null()
        {
            return None;
        }
        let available = &*net_list;
        let items = std::slice::from_raw_parts(
            available.Network.as_ptr(),
            available.dwNumberOfItems as usize,
        );
        let mut result = None;
        for network in items {
            if ssid_string(&network.dot11Ssid) == ssid {
                result = Some((
                    network.dot11DefaultAuthAlgorithm.0,
                    network.bSecurityEnabled.as_bool(),
                ));
                break;
            }
        }
        WlanFreeMemory(net_list.cast());
        result
    }
}

fn profile_xml(
    ssid: &str,
    authentication: &str,
    encryption: &str,
    password: Option<&str>,
) -> String {
    let key = password
        .map(|key| {
            format!(
                "<sharedKey><keyType>passPhrase</keyType><protected>false</protected>\
                 <keyMaterial>{}</keyMaterial></sharedKey>",
                xml_escape(key)
            )
        })
        .unwrap_or_default();
    format!(
        "<?xml version=\"1.0\"?><WLANProfile xmlns=\"http://www.microsoft.com/networking/WLAN/profile/v1\">\
         <name>{name}</name>\
         <SSIDConfig><SSID><name>{name}</name></SSID></SSIDConfig>\
         <connectionType>ESS</connectionType><connectionMode>auto</connectionMode>\
         <MSM><security><authEncryption><authentication>{authentication}</authentication>\
         <encryption>{encryption}</encryption><useOneX>false</useOneX></authEncryption>{key}</security></MSM>\
         </WLANProfile>",
        name = xml_escape(ssid),
        authentication = authentication,
        encryption = encryption,
        key = key
    )
}

/// The interface's live state.
fn interface_state(handle: HANDLE, interface: &GUID) -> Option<WLAN_INTERFACE_STATE> {
    unsafe {
        let mut size = 0u32;
        let mut data: *mut core::ffi::c_void = std::ptr::null_mut();
        if WlanQueryInterface(
            handle,
            interface,
            wlan_intf_opcode_interface_state,
            None,
            &mut size,
            &mut data,
            None,
        ) != 0
            || data.is_null()
        {
            return None;
        }
        let state = *(data as *const WLAN_INTERFACE_STATE);
        WlanFreeMemory(data);
        Some(state)
    }
}

/// Joins a network: open networks connect immediately; saved networks reuse
/// their stored profile; anything else writes a profile containing the
/// password first (the profile survives, so reconnects need no prompt).
/// Blocks until the interface reports connected (or fails) — run on a
/// blocking worker.
pub fn connect(ssid: &str, password: &str) -> Result<(), String> {
    unsafe {
        let handle = open_wlan()?;
        let interface = first_interface(handle)?;
        let saved = saved_ssids(handle, &interface)
            .iter()
            .any(|name| name.eq_ignore_ascii_case(ssid));
        if !saved {
            let (auth, secure) =
                scanned_auth(handle, &interface, ssid).unwrap_or((-1, !password.is_empty()));
            let (authentication, encryption, wants_key) = auth_pair(auth, secure);
            // A key typed for an open network is simply ignored.
            let key = if wants_key && !password.is_empty() {
                Some(password)
            } else {
                None
            };
            let xml = profile_xml(ssid, authentication, encryption, key);
            let xml_wide = xml.encode_utf16().chain([0]).collect::<Vec<u16>>();
            let mut reason = 0u32;
            if WlanSetProfile(
                handle,
                &interface,
                0,
                PCWSTR(xml_wide.as_ptr()),
                None,
                true,
                None,
                &mut reason,
            ) != 0
            {
                let _ = WlanCloseHandle(handle, None);
                return Err(format!(
                    "Could not save the network profile (reason {reason}); the password may be wrong"
                ));
            }
        }
        let profile_wide = ssid.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let parameters = WLAN_CONNECTION_PARAMETERS {
            wlanConnectionMode: wlan_connection_mode_profile,
            strProfile: PCWSTR(profile_wide.as_ptr()),
            pDot11Ssid: std::ptr::null_mut(),
            pDesiredBssidList: std::ptr::null_mut(),
            dot11BssType: dot11_BSS_type_infrastructure,
            dwFlags: 0,
        };
        let started = WlanConnect(handle, &interface, &parameters, None) == 0;
        if !started {
            let _ = WlanCloseHandle(handle, None);
            return Err(format!("Windows refused the connection to {ssid}"));
        }
        // The connection proceeds asynchronously; poll the interface state.
        // Seeing "disconnected" only counts as a failure once the attempt
        // actually started (associating/authenticating was observed), so the
        // brief initial disconnected state is not mistaken for an error.
        let mut attempt_started = false;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(250));
            match interface_state(handle, &interface) {
                Some(state) if state == wlan_interface_state_connected => {
                    let _ = WlanCloseHandle(handle, None);
                    return Ok(());
                }
                Some(state) if state == wlan_interface_state_disconnected => {
                    if attempt_started {
                        let _ = WlanCloseHandle(handle, None);
                        return Err(format!(
                            "Could not join {ssid}; the password may be wrong or the network is out of range"
                        ));
                    }
                }
                Some(state) if state != wlan_interface_state_disconnected => {
                    attempt_started = true;
                }
                _ => {}
            }
        }
        let _ = WlanCloseHandle(handle, None);
        Err(format!("Timed out joining {ssid}"))
    }
}
