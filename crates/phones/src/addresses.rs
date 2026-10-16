//! The laptop's addresses, and which one the QR code should carry.
//!
//! A laptop commonly has several: the wifi adapter, a VPN, a virtual
//! adapter from Docker or Hyper-V, the hotspot when it is on. Only a private
//! LAN address (RFC 1918) can reach a phone in the room, and when there are
//! several of those the presenter picks rather than the program guessing.

use std::net::{IpAddr, Ipv4Addr};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddressInfo {
    pub address: String,
    /// The adapter's name as the OS reports it.
    pub interface: String,
    /// In one of the RFC 1918 ranges.
    pub private: bool,
    /// Looks like a Windows mobile hotspot or Internet Connection Sharing adapter (`192.168.137.x`).
    pub hotspot: bool,
    /// Looks like a virtual adapter or a VPN, from the interface name.
    pub virtual_adapter: bool,
}

pub fn is_private(ip: Ipv4Addr) -> bool {
    let [a, b, _, _] = ip.octets();
    a == 10 || (a == 192 && b == 168) || (a == 172 && (16..=31).contains(&b))
}

/// Windows gives the hotspot and Internet Connection Sharing adapter `192.168.137.1`.
pub fn is_hotspot(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    a == 192 && b == 168 && c == 137
}

/// Names that belong to VPNs and virtual adapters rather than the room.
pub fn looks_virtual(interface: &str) -> bool {
    let n = interface.to_ascii_lowercase();
    [
        "vethernet",
        "hyper-v",
        "vmware",
        "virtualbox",
        "vbox",
        "docker",
        "wsl",
        "tailscale",
        "zerotier",
        "wireguard",
        "openvpn",
        "vpn",
        "tun",
        "tap",
        "veth",
        "br-",
        "utun",
        "npcap",
        "loopback",
    ]
    .iter()
    .any(|k| n.contains(k))
}

pub fn classify(ip: Ipv4Addr, interface: &str) -> AddressInfo {
    AddressInfo {
        address: ip.to_string(),
        interface: interface.to_string(),
        private: is_private(ip),
        hotspot: is_hotspot(ip),
        virtual_adapter: looks_virtual(interface),
    }
}

/// Every IPv4 address on the machine except loopback and link-local,
/// private ones first, then hotspot ahead of the rest, then by interface name.
pub fn list_addresses() -> Vec<AddressInfo> {
    let mut out: Vec<AddressInfo> = Vec::new();
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for i in ifaces {
            if let IpAddr::V4(ip) = i.ip() {
                if ip.is_loopback() || ip.is_link_local() || ip.is_unspecified() {
                    continue;
                }
                out.push(classify(ip, &i.name));
            }
        }
    }
    sort(&mut out);
    out
}

pub fn sort(list: &mut [AddressInfo]) {
    list.sort_by(|a, b| {
        let rank = |x: &AddressInfo| (!x.private, !x.hotspot, x.virtual_adapter);
        rank(a).cmp(&rank(b)).then_with(|| a.interface.cmp(&b.interface)).then_with(|| a.address.cmp(&b.address))
    });
}

/// The address the QR carries when the presenter has not chosen: the
/// hotspot when it is up, else the single private non-virtual address, else
/// `None` so the presenter is asked.
pub fn pick_default(list: &[AddressInfo]) -> Option<&AddressInfo> {
    if let Some(h) = list.iter().find(|a| a.hotspot) {
        return Some(h);
    }
    let plain: Vec<&AddressInfo> = list.iter().filter(|a| a.private && !a.virtual_adapter).collect();
    match plain.len() {
        1 => Some(plain[0]),
        0 => {
            let private: Vec<&AddressInfo> = list.iter().filter(|a| a.private).collect();
            if private.len() == 1 { Some(private[0]) } else { None }
        }
        _ => None,
    }
}

/// Whether the presenter must be asked: several candidates, or none.
pub fn needs_choice(list: &[AddressInfo]) -> bool {
    pick_default(list).is_none() && list.iter().filter(|a| a.private).count() != 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(ip: &str, iface: &str) -> AddressInfo {
        classify(ip.parse().unwrap(), iface)
    }

    #[test]
    fn private_ranges() {
        assert!(is_private("192.168.1.14".parse().unwrap()));
        assert!(is_private("10.0.0.5".parse().unwrap()));
        assert!(is_private("172.16.0.1".parse().unwrap()));
        assert!(is_private("172.31.255.254".parse().unwrap()));
        assert!(!is_private("172.32.0.1".parse().unwrap()));
        assert!(!is_private("172.15.0.1".parse().unwrap()));
        assert!(!is_private("8.8.8.8".parse().unwrap()));
        assert!(!is_private("169.254.1.1".parse().unwrap()));
    }

    #[test]
    fn one_private_address_is_picked_without_asking() {
        let list = vec![a("192.168.1.14", "Wi-Fi")];
        assert_eq!(pick_default(&list).map(|x| x.address.as_str()), Some("192.168.1.14"));
        assert!(!needs_choice(&list));
    }

    #[test]
    fn a_vpn_beside_the_wifi_does_not_force_a_choice() {
        let mut list = vec![a("10.8.0.2", "OpenVPN TAP-Windows6"), a("192.168.1.14", "Wi-Fi")];
        sort(&mut list);
        assert_eq!(list[0].address, "192.168.1.14", "the wifi sorts first: {list:?}");
        assert_eq!(pick_default(&list).map(|x| x.address.as_str()), Some("192.168.1.14"));
    }

    #[test]
    fn two_plain_private_addresses_ask_the_presenter() {
        let list = vec![a("192.168.1.14", "Wi-Fi"), a("192.168.50.3", "Ethernet")];
        assert_eq!(pick_default(&list), None);
        assert!(needs_choice(&list));
    }

    #[test]
    fn the_hotspot_wins_once_it_is_up() {
        let mut list = vec![a("192.168.1.14", "Wi-Fi"), a("192.168.137.1", "Local Area Connection* 12")];
        sort(&mut list);
        assert_eq!(pick_default(&list).map(|x| x.address.as_str()), Some("192.168.137.1"));
        assert!(list[0].hotspot);
    }

    #[test]
    fn a_public_address_alone_is_offered_but_not_picked() {
        let list = vec![a("203.0.113.5", "Ethernet")];
        assert_eq!(pick_default(&list), None);
        assert!(!list[0].private);
    }
}
