//! The Windows mobile hotspot, through the WinRT
//! `Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager`.
//! `netsh wlan set hostednetwork` is gone from modern Windows and is not
//! used.
//!
//! Written on Linux without a Windows machine to run it on. The calls
//! follow the `windows` crate's bindings for version 0.61 and the WinRT
//! documentation; `PHONES.md` and `ACCEPTANCE.md` mark this as unverified.
//! Whatever happens here, the presenter always has the manual path in
//! `MANUAL_STEPS`.

use serde::{Deserialize, Serialize};

/// What to do when the automatic start is unavailable or fails. Two taps on Windows 11.
pub const MANUAL_STEPS: &str = "Turn on Mobile hotspot in Windows Settings (Network and internet, Mobile hotspot), then press Refresh here.";

/// How many phones a Windows mobile hotspot takes.
pub const HOTSPOT_CLIENT_LIMIT: u32 = 8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotspotStatus {
    /// `on`, `off`, `in_transition`, `unknown`, or `unsupported` off Windows.
    pub state: String,
    pub ssid: Option<String>,
    pub passphrase: Option<String>,
    pub clients: Option<u32>,
    pub max_clients: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotspotResult {
    pub started: bool,
    pub status: HotspotStatus,
    /// What the presenter should read: what happened, and what to do if it failed.
    pub message: String,
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::Networking::Connectivity::NetworkInformation;
    use windows::Networking::NetworkOperators::{NetworkOperatorTetheringManager, TetheringOperationStatus, TetheringOperationalState};

    fn state_name(s: TetheringOperationalState) -> &'static str {
        match s {
            TetheringOperationalState::On => "on",
            TetheringOperationalState::Off => "off",
            TetheringOperationalState::InTransition => "in_transition",
            _ => "unknown",
        }
    }

    fn manager() -> Result<NetworkOperatorTetheringManager, String> {
        let profile = NetworkInformation::GetInternetConnectionProfile()
            .map_err(|e| format!("No internet connection profile: {e}. A hotspot needs a network to share."))?;
        NetworkOperatorTetheringManager::CreateFromConnectionProfile(&profile).map_err(|e| format!("Cannot create the tethering manager: {e}"))
    }

    fn status_of(m: &NetworkOperatorTetheringManager) -> HotspotStatus {
        let state = m.TetheringOperationalState().map(state_name).unwrap_or("unknown").to_string();
        let (ssid, passphrase) = match m.GetCurrentAccessPointConfiguration() {
            Ok(cfg) => (cfg.Ssid().ok().map(|s| s.to_string()), cfg.Passphrase().ok().map(|s| s.to_string())),
            Err(_) => (None, None),
        };
        HotspotStatus { state, ssid, passphrase, clients: m.ClientCount().ok(), max_clients: m.MaxClientCount().ok() }
    }

    pub fn status() -> Result<HotspotStatus, String> {
        Ok(status_of(&manager()?))
    }

    fn status_text(s: TetheringOperationStatus) -> String {
        match s {
            TetheringOperationStatus::Success => "started".into(),
            TetheringOperationStatus::AlreadyOn => "already on".into(),
            TetheringOperationStatus::WiFiDeviceOff => "the wifi adapter is off; turn wifi on first".into(),
            TetheringOperationStatus::MobileBroadbandDeviceOff => "the mobile broadband device is off".into(),
            TetheringOperationStatus::OperationInProgress => "another hotspot operation is in progress; wait a moment and try again".into(),
            TetheringOperationStatus::NetworkLimitedConnectivity => "the network to share has limited connectivity".into(),
            TetheringOperationStatus::RadioRestriction => "a radio restriction blocks the hotspot".into(),
            TetheringOperationStatus::BandInterference => "band interference blocks the hotspot".into(),
            TetheringOperationStatus::EntitlementCheckFailure | TetheringOperationStatus::EntitlementCheckTimeout => {
                "the entitlement check failed".into()
            }
            TetheringOperationStatus::BluetoothDeviceOff => "the Bluetooth device is off".into(),
            other => format!("status code {}", other.0),
        }
    }

    /// Blocks for as long as Windows takes to bring the hotspot up, so call it off the main thread.
    pub fn start() -> Result<HotspotResult, String> {
        log::info!("hotspot: creating the tethering manager");
        let m = manager()?;
        let before = status_of(&m);
        log::info!("hotspot: state before start: {}", before.state);
        if before.state == "on" {
            return Ok(HotspotResult { started: true, status: before, message: "The hotspot is already on.".into() });
        }
        log::info!("hotspot: StartTetheringAsync");
        let op = m.StartTetheringAsync().map_err(|e| format!("StartTetheringAsync failed: {e}"))?;
        let result = op.get().map_err(|e| format!("Starting the hotspot failed: {e}"))?;
        let status = result.Status().map_err(|e| e.to_string())?;
        let extra = result.AdditionalErrorMessage().map(|s| s.to_string()).unwrap_or_default();
        log::info!("hotspot: result {} {}", status_text(status), extra);
        let after = status_of(&m);
        let started = matches!(status, TetheringOperationStatus::Success | TetheringOperationStatus::AlreadyOn) || after.state == "on";
        let message = if started {
            match (&after.ssid, &after.passphrase) {
                (Some(ssid), Some(pass)) => format!("Hotspot on. Network {ssid}, password {pass}. It takes about {HOTSPOT_CLIENT_LIMIT} phones."),
                _ => format!("Hotspot on. It takes about {HOTSPOT_CLIENT_LIMIT} phones."),
            }
        } else {
            let why = if extra.is_empty() { status_text(status) } else { format!("{} ({extra})", status_text(status)) };
            format!("Windows did not start the hotspot: {why}. {MANUAL_STEPS}")
        };
        Ok(HotspotResult { started, status: after, message })
    }

    pub fn stop() -> Result<(), String> {
        let m = manager()?;
        let op = m.StopTetheringAsync().map_err(|e| e.to_string())?;
        op.get().map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    fn unsupported() -> HotspotStatus {
        HotspotStatus { state: "unsupported".into(), ssid: None, passphrase: None, clients: None, max_clients: None }
    }

    pub fn status() -> Result<HotspotStatus, String> {
        Ok(unsupported())
    }

    pub fn start() -> Result<HotspotResult, String> {
        Ok(HotspotResult {
            started: false,
            status: unsupported(),
            message: format!("Starting a hotspot from here works on Windows only. {MANUAL_STEPS}"),
        })
    }

    pub fn stop() -> Result<(), String> {
        Err("Starting a hotspot from here works on Windows only.".into())
    }
}

pub use imp::{start, status, stop};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fallback_always_names_the_manual_path() {
        assert!(MANUAL_STEPS.contains("Mobile hotspot"));
        assert!(MANUAL_STEPS.contains("Refresh"));
        #[cfg(not(windows))]
        {
            let r = start().unwrap();
            assert!(!r.started);
            assert!(r.message.contains(MANUAL_STEPS));
            assert_eq!(status().unwrap().state, "unsupported");
        }
    }
}
