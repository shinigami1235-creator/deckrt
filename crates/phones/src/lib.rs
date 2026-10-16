//! Audience phones. A show starts one small HTTP and WebSocket server on the laptop's LAN
//! address; a phone scans the QR code, opens the one page this server serves, and joins over the
//! WebSocket. The app is the one privileged client, attached in-process, and everything a phone
//! sends is relayed to it.
//!
//! The server owns nothing about the show. It validates, rate-limits and relays; the app applies
//! each phone's intent to the show state and draws the result. `PHONES.md` in the earlier
//! deckrt repository lists every message; the protocol is unchanged.
//!
//! - `protocol`: message shapes, name cleaning, rate limits, size caps.
//! - `addresses`: the machine's addresses and which one the QR should carry.
//! - `server`: the axum server, the hub of connections, the relay.
//! - `hotspot`: the Windows mobile hotspot through WinRT.
//! - `results`: `<deckname>.results.json` written while the show runs.
//! - [`PhoneServer`]: the server driven from ordinary (non-async) code, as the app does.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod addresses;
pub mod hotspot;
pub mod protocol;
pub mod results;
pub mod server;

use std::net::IpAddr;
use std::sync::Arc;

pub use addresses::{AddressInfo, list_addresses, pick_default};
pub use protocol::{PhoneMessage, RosterEntry};
pub use server::{Running, ServerConfig, start};

/// The phone page, served at `/`. One file, no build step, nothing loaded from anywhere else.
pub const PHONE_PAGE: &str = include_str!("../phone/index.html");

/// What the server tells the app, parsed from the presenter messages.
#[derive(Debug, Clone, PartialEq)]
pub enum Inbound {
    Hello { max_phones: usize, connected: usize },
    Roster(Vec<RosterEntry>),
    Joined { id: String, name: String, rejoined: bool },
    From { id: String, name: Option<String>, msg: PhoneMessage },
}

fn parse_inbound(text: &str) -> Option<Inbound> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(String::from);
    match v.get("t")?.as_str()? {
        "hello" => Some(Inbound::Hello {
            max_phones: v.get("max_phones").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
            connected: v.get("connected").and_then(|x| x.as_u64()).unwrap_or(0) as usize,
        }),
        "roster" => Some(Inbound::Roster(serde_json::from_value(v.get("phones")?.clone()).ok()?)),
        "joined" => Some(Inbound::Joined {
            id: s("id")?,
            name: s("name").unwrap_or_default(),
            rejoined: v.get("rejoined").and_then(|x| x.as_bool()).unwrap_or(false),
        }),
        "from" => {
            let msg: PhoneMessage = serde_json::from_value(v.get("msg")?.clone()).ok()?;
            Some(Inbound::From { id: s("id")?, name: s("name"), msg })
        }
        _ => None,
    }
}

/// The server running on its own threads, driven from the app's frame loop.
pub struct PhoneServer {
    rt: tokio::runtime::Runtime,
    running: Option<Running>,
    hub: Arc<server::Hub>,
    serial: u64,
    rx: tokio::sync::mpsc::Receiver<String>,
}

impl PhoneServer {
    /// Start on `addresses` (every private address of the machine when empty) at `port` or the
    /// next free one.
    pub fn start(addresses: Vec<IpAddr>, port: u16) -> Result<PhoneServer, String> {
        Self::start_with_token(addresses, port, None)
    }

    /// [`PhoneServer::start`] keeping `token` from an earlier server, so phones already on the
    /// page keep working after a restart on another address.
    pub fn start_with_token(addresses: Vec<IpAddr>, port: u16, token: Option<String>) -> Result<PhoneServer, String> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|e| format!("the phone server's runtime did not start: {e}"))?;
        let addresses =
            if addresses.is_empty() { list_addresses().into_iter().filter_map(|a| a.address.parse::<IpAddr>().ok()).collect() } else { addresses };
        let running = rt.block_on(start(ServerConfig { addresses, port, token, ..Default::default() }))?;
        let hub = running.hub_arc();
        let (serial, rx) = {
            let hub = hub.clone();
            rt.block_on(async move { hub.attach_local_presenter() })
        };
        Ok(PhoneServer { rt, running: Some(running), hub, serial, rx })
    }

    pub fn port(&self) -> u16 {
        self.running.as_ref().map(|r| r.port).unwrap_or(0)
    }

    /// The token in the QR URL.
    pub fn token(&self) -> String {
        self.running.as_ref().map(|r| r.token.clone()).unwrap_or_default()
    }

    /// The URL the QR carries for `address`.
    pub fn url_for(&self, address: &str) -> String {
        self.running.as_ref().map(|r| r.url_for(address)).unwrap_or_default()
    }

    pub fn connected(&self) -> usize {
        self.hub.connected()
    }

    pub fn roster(&self) -> Vec<RosterEntry> {
        self.hub.roster_now()
    }

    /// Everything the phones sent since the last call.
    pub fn poll(&mut self) -> Vec<Inbound> {
        let mut out = vec![];
        while let Ok(t) = self.rx.try_recv() {
            if let Some(m) = parse_inbound(&t) {
                out.push(m);
            }
        }
        out
    }

    pub fn broadcast(&self, msg: &serde_json::Value) {
        self.hub.presenter_broadcast(msg);
    }
    pub fn send(&self, to: &str, msg: &serde_json::Value) {
        self.hub.presenter_send(to, msg);
    }
    pub fn eject(&self, id: &str) {
        self.hub.presenter_eject(id);
    }
    pub fn set_slides(&self, dir: std::path::PathBuf, count: u32) {
        self.hub.presenter_slides(dir, count);
    }
    pub fn set_live(&self, slide: u32, version: u32, jpeg: Vec<u8>) {
        self.hub.presenter_live(protocol::LiveFrame { slide, version, jpeg });
    }

    /// Fetch the phone page from `url` the way a phone would, with a short timeout.
    pub fn self_test(&self, url: &str) -> Result<String, String> {
        self.rt.block_on(server::self_test(url, std::time::Duration::from_secs(3)))
    }

    pub fn stop(mut self) {
        self.hub.presenter_detach(self.serial);
        if let Some(r) = self.running.take() {
            self.rt.block_on(r.stop());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbound_messages_parse() {
        assert_eq!(parse_inbound(r#"{"t":"hello","max_phones":100,"connected":2}"#), Some(Inbound::Hello { max_phones: 100, connected: 2 }));
        assert_eq!(
            parse_inbound(r#"{"t":"from","id":"p1","name":"Sam","msg":{"t":"vote","poll":"obj","option":1}}"#),
            Some(Inbound::From { id: "p1".into(), name: Some("Sam".into()), msg: PhoneMessage::Vote { poll: "obj".into(), option: 1 } })
        );
        assert_eq!(parse_inbound(r#"{"t":"nope"}"#), None);
    }

    #[test]
    fn the_server_starts_on_loopback_and_serves_the_page() {
        let s = PhoneServer::start(vec![IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)], 0).expect("starts");
        assert!(s.port() > 0);
        let url = s.url_for("127.0.0.1");
        assert!(url.starts_with("http://127.0.0.1:"));
        let answer = s.self_test(&url).expect("page");
        assert!(answer.contains("answers at 127.0.0.1"), "{answer}");
        s.stop();
    }
}
