//! Windows Core Audio endpoint enumeration with Bluetooth detection.
//!
//! Opening a Bluetooth headset microphone switches the headset from A2DP
//! (stereo, 44.1/48 kHz) to the hands-free profile (mono, 16 kHz), degrading
//! everything the streamer hears and everything captured from that headset.
//! We use this module to never open such a mic implicitly.

use serde::Serialize;
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, EDataFlow, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eConsole, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, STGM_READ};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::core::GUID;

/// DEVPKEY_Device_EnumeratorName — "BTHENUM" / "BTHHFENUM" for Bluetooth audio.
const PKEY_ENUMERATOR_NAME: PROPERTYKEY =
    PROPERTYKEY { fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0), pid: 24 };

#[derive(Serialize, Clone, Debug)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub bluetooth: bool,
    pub default: bool,
}

fn enumerator() -> Option<IMMDeviceEnumerator> {
    unsafe {
        // Already-initialized threads (e.g. the STA UI thread) return an error we can ignore.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()
    }
}

fn describe(dev: &IMMDevice, default_id: &str) -> Option<AudioDevice> {
    unsafe {
        let id = dev.GetId().ok()?.to_string().ok()?;
        let store = dev.OpenPropertyStore(STGM_READ).ok()?;
        let name = store.GetValue(&PKEY_Device_FriendlyName).map(|v| v.to_string()).unwrap_or_default();
        let enumerator = store.GetValue(&PKEY_ENUMERATOR_NAME).map(|v| v.to_string()).unwrap_or_default();
        Some(AudioDevice {
            default: id == default_id,
            // Enumerator is authoritative; the name check covers drivers that hide it.
            bluetooth: enumerator.to_ascii_uppercase().starts_with("BTH") || name.contains("Hands-Free"),
            id,
            name,
        })
    }
}

fn default_id(e: &IMMDeviceEnumerator, flow: EDataFlow) -> String {
    unsafe {
        e.GetDefaultAudioEndpoint(flow, eConsole)
            .ok()
            .and_then(|d| d.GetId().ok())
            .and_then(|p| p.to_string().ok())
            .unwrap_or_default()
    }
}

fn list(flow: EDataFlow) -> Vec<AudioDevice> {
    let Some(e) = enumerator() else { return Vec::new() };
    let def = default_id(&e, flow);
    unsafe {
        let Ok(coll) = e.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE) else { return Vec::new() };
        let n = coll.GetCount().unwrap_or(0);
        (0..n).filter_map(|i| coll.Item(i).ok()).filter_map(|d| describe(&d, &def)).collect()
    }
}

pub fn inputs() -> Vec<AudioDevice> {
    list(eCapture)
}

pub fn outputs() -> Vec<AudioDevice> {
    list(eRender)
}

/// Resolves a WASAPI `device_id` setting ("default" or an endpoint id) to a capture device.
pub fn resolve_input(device_id: &str) -> Option<AudioDevice> {
    let all = inputs();
    if device_id == "default" {
        all.into_iter().find(|d| d.default)
    } else {
        all.into_iter().find(|d| d.id == device_id)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn lists_devices() {
        for d in super::inputs() {
            eprintln!("IN  {d:?}");
        }
        for d in super::outputs() {
            eprintln!("OUT {d:?}");
        }
    }
}
