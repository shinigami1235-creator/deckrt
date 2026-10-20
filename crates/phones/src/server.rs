//! The server: one page, one WebSocket endpoint, nothing else.
//!
//! `GET /` serves the phone page. `GET /ws?role=phone&t=<token>&id=<id>`
//! is a phone; `GET /ws?role=presenter&t=<presenter token>` is the presenter
//! window, accepted from the loopback address only. `GET /slide/<n>?t=<token>`
//! is the phone-sized image of slide `n`, from the directory the presenter
//! named in its `slides` message, by number alone. `GET /live/<v>?t=<token>`
//! is the presenter's latest composite of the slide it is showing, when its
//! version is `v`; the presenter sends it as a binary frame and the server
//! keeps only the newest. Every other path is a 404. No path from a request
//! is ever used to open a file.
//!
//! The hub holds every connection. A phone's frames are parsed, checked
//! against its rate limits and relayed to the presenter as `from` messages;
//! the presenter's `broadcast` and `send` are relayed to phones. Stream
//! messages (`drag`, `touch`) are dropped rather than queued when the
//! presenter is behind; everything else waits its turn.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Path as RoutePath, Query, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::PHONE_PAGE;
use crate::protocol::{self, Class, Limits, LiveFrame, PhoneMessage, PresenterMessage, RosterEntry};

/// How many successive ports are tried from the preferred one.
pub const PORT_ATTEMPTS: u16 = 20;
pub const DEFAULT_PORT: u16 = 8720;

#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// LAN addresses to listen on. The loopback address is always added for the presenter window.
    pub addresses: Vec<IpAddr>,
    /// First port to try; `0` takes any free port.
    pub port: u16,
    pub max_phones: usize,
    /// Reuse a token across a restart on a new address, so phones already on the page keep working.
    pub token: Option<String>,
    pub presenter_token: Option<String>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig { addresses: Vec::new(), port: DEFAULT_PORT, max_phones: protocol::DEFAULT_MAX_PHONES, token: None, presenter_token: None }
    }
}

/// Counters for the report.
#[derive(Debug, Default)]
pub struct Stats {
    /// Stream messages dropped because the presenter was behind.
    pub dropped: AtomicU64,
    /// Frames that did not parse or failed a range check.
    pub malformed: AtomicU64,
    /// Messages refused by a rate limit.
    pub rate_limited: AtomicU64,
    pub joins: AtomicU64,
}

enum Outbound {
    Text(String),
    Close(u16, &'static str),
}

struct PhoneEntry {
    name: Option<String>,
    serial: u64,
    tx: Option<mpsc::Sender<Outbound>>,
}

#[derive(Default)]
struct Inner {
    phones: HashMap<String, PhoneEntry>,
    banned: HashSet<String>,
    presenter: Option<mpsc::Sender<Outbound>>,
    presenter_serial: u64,
    serial: u64,
    /// Where the phone-sized slide images are, and how many: set by the presenter.
    slides: Option<(std::path::PathBuf, u32)>,
    /// The presenter's newest composite. One is enough: a phone only ever wants the current picture.
    live: Option<Arc<LiveFrame>>,
    /// deckrt: the slides shown so far (1-based). Only these are served, so a phone can't fetch the
    /// slides still to come.
    shown: HashSet<u32>,
}

pub struct Hub {
    token: String,
    presenter_token: String,
    max_phones: usize,
    inner: Mutex<Inner>,
    pub stats: Stats,
}

impl Hub {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn live_count(inner: &Inner) -> usize {
        inner.phones.values().filter(|p| p.tx.is_some()).count()
    }

    /// Phones with a live connection.
    pub fn connected(&self) -> usize {
        Self::live_count(&self.lock())
    }

    fn roster(inner: &Inner) -> Vec<RosterEntry> {
        let mut list: Vec<RosterEntry> = inner
            .phones
            .iter()
            .filter_map(|(id, p)| p.name.as_ref().map(|n| RosterEntry { id: id.clone(), name: n.clone(), connected: p.tx.is_some() }))
            .collect();
        list.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        list
    }

    pub fn roster_now(&self) -> Vec<RosterEntry> {
        Self::roster(&self.lock())
    }

    fn presenter_text(inner: &Inner, text: String) {
        if let Some(tx) = &inner.presenter {
            let _ = tx.try_send(Outbound::Text(text));
        }
    }

    fn send_roster(inner: &Inner) {
        let r = Self::roster(inner);
        Self::presenter_text(inner, json!({ "t": "roster", "phones": r }).to_string());
    }

    fn attach_phone(&self, id: &str, tx: mpsc::Sender<Outbound>) -> Result<u64, u16> {
        let mut inner = self.lock();
        if inner.banned.contains(id) {
            return Err(protocol::CLOSE_EJECTED);
        }
        let exists = inner.phones.contains_key(id);
        if !exists && Self::live_count(&inner) >= self.max_phones {
            return Err(protocol::CLOSE_FULL);
        }
        // deckrt: phones that left keep their place (and name) for when they come back, but no more
        // than a few rooms' worth: past that the ones gone longest are forgotten, so a stream of
        // new ids can't grow the list without end.
        if !exists {
            let keep = self.max_phones.saturating_mul(4).max(16);
            while inner.phones.len() >= keep {
                let Some(oldest) = inner.phones.iter().filter(|(_, p)| p.tx.is_none()).min_by_key(|(_, p)| p.serial).map(|(k, _)| k.clone()) else {
                    break;
                };
                inner.phones.remove(&oldest);
            }
        }
        inner.serial += 1;
        let serial = inner.serial;
        let entry = inner.phones.entry(id.to_string()).or_insert(PhoneEntry { name: None, serial, tx: None });
        if let Some(old) = entry.tx.take() {
            let _ = old.try_send(Outbound::Close(protocol::CLOSE_REPLACED, "This page was opened somewhere else. Use that one."));
        }
        entry.serial = serial;
        entry.tx = Some(tx);
        if entry.name.is_some() {
            Self::send_roster(&inner);
        }
        Ok(serial)
    }

    fn detach_phone(&self, id: &str, serial: u64) {
        let mut inner = self.lock();
        let Some(entry) = inner.phones.get_mut(id) else { return };
        if entry.serial != serial {
            return;
        }
        entry.tx = None;
        if entry.name.is_none() {
            inner.phones.remove(id);
            return;
        }
        log::info!("phones: {id} disconnected");
        Self::send_roster(&inner);
    }

    /// Record the name; tell the presenter. Returns the name kept and whether the phone was known before.
    fn join(&self, id: &str, name: String) -> (String, bool) {
        let mut inner = self.lock();
        let rejoined = inner.phones.get(id).and_then(|p| p.name.as_ref()).is_some();
        if let Some(entry) = inner.phones.get_mut(id) {
            entry.name = Some(name.clone());
        }
        self.stats.joins.fetch_add(1, Ordering::Relaxed);
        log::info!("phones: {id} joined as {name:?}{}", if rejoined { " (again)" } else { "" });
        Self::presenter_text(&inner, json!({ "t": "joined", "id": id, "name": name, "rejoined": rejoined }).to_string());
        Self::send_roster(&inner);
        (name, rejoined)
    }

    fn name_of(&self, id: &str) -> Option<String> {
        self.lock().phones.get(id).and_then(|p| p.name.clone())
    }

    /// Relay a phone's message. Stream messages are dropped when the
    /// presenter's queue is full; the rest wait outside the lock.
    async fn forward(&self, id: &str, msg: &PhoneMessage) {
        let (tx, name) = {
            let inner = self.lock();
            (inner.presenter.clone(), inner.phones.get(id).and_then(|p| p.name.clone()))
        };
        let Some(tx) = tx else { return };
        let text = json!({ "t": "from", "id": id, "name": name, "msg": msg }).to_string();
        if msg.class() == Class::Stream {
            if tx.try_send(Outbound::Text(text)).is_err() {
                self.stats.dropped.fetch_add(1, Ordering::Relaxed);
            }
        } else if tokio::time::timeout(Duration::from_secs(5), tx.send(Outbound::Text(text))).await.is_err() {
            self.stats.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn to_phone(&self, id: &str, text: String) -> bool {
        let inner = self.lock();
        match inner.phones.get(id).and_then(|p| p.tx.as_ref()) {
            Some(tx) => tx.try_send(Outbound::Text(text)).is_ok(),
            None => false,
        }
    }

    fn broadcast(&self, text: &str) -> usize {
        let inner = self.lock();
        let mut n = 0;
        for p in inner.phones.values() {
            if let Some(tx) = &p.tx
                && tx.try_send(Outbound::Text(text.to_string())).is_ok()
            {
                n += 1;
            }
        }
        n
    }

    fn eject(&self, id: &str) {
        let mut inner = self.lock();
        inner.banned.insert(id.to_string());
        if let Some(p) = inner.phones.remove(id) {
            log::info!("phones: {id} ({}) ejected", p.name.as_deref().unwrap_or("unnamed"));
            if let Some(tx) = p.tx {
                let _ = tx.try_send(Outbound::Text(json!({ "t": "ejected" }).to_string()));
                let _ = tx.try_send(Outbound::Close(protocol::CLOSE_EJECTED, "You were removed from this show."));
            }
        }
        Self::send_roster(&inner);
    }

    fn attach_presenter(&self, tx: mpsc::Sender<Outbound>) -> u64 {
        let mut inner = self.lock();
        if let Some(old) = inner.presenter.take() {
            let _ = old.try_send(Outbound::Close(protocol::CLOSE_REPLACED, "replaced by a newer presenter connection"));
        }
        inner.presenter_serial += 1;
        inner.presenter = Some(tx);
        let serial = inner.presenter_serial;
        Self::presenter_text(&inner, json!({ "t": "hello", "max_phones": self.max_phones, "connected": Self::live_count(&inner) }).to_string());
        Self::send_roster(&inner);
        serial
    }

    /// The app itself as the presenter, without a WebSocket: what the server would send a
    /// presenter connection arrives on the returned receiver as JSON text, and the app drives
    /// the hub through [`Hub::presenter_broadcast`] and the methods beside it.
    pub fn attach_local_presenter(&self) -> (u64, mpsc::Receiver<String>) {
        let (tx, rx) = mpsc::channel::<Outbound>(1024);
        let serial = self.attach_presenter(tx);
        let (out_tx, out_rx) = mpsc::channel::<String>(1024);
        tokio::spawn(async move {
            let mut rx = rx;
            while let Some(m) = rx.recv().await {
                match m {
                    Outbound::Text(t) => {
                        if out_tx.send(t).await.is_err() {
                            break;
                        }
                    }
                    Outbound::Close(..) => break,
                }
            }
        });
        (serial, out_rx)
    }

    pub fn presenter_broadcast(&self, msg: &serde_json::Value) -> usize {
        self.broadcast(&msg.to_string())
    }
    pub fn presenter_send(&self, to: &str, msg: &serde_json::Value) -> bool {
        self.to_phone(to, msg.to_string())
    }
    pub fn presenter_eject(&self, id: &str) {
        self.eject(id)
    }
    pub fn presenter_roster(&self) {
        let inner = self.lock();
        Self::send_roster(&inner);
    }
    pub fn presenter_slides(&self, dir: std::path::PathBuf, count: u32) {
        self.set_slides(dir, count)
    }
    /// deckrt: slide `n` (1-based) has been on screen, so phones may have its picture.
    pub fn presenter_shown(&self, n: u32) {
        self.lock().shown.insert(n);
    }
    pub fn presenter_live(&self, frame: LiveFrame) {
        self.set_live(frame)
    }
    pub fn presenter_detach(&self, serial: u64) {
        self.detach_presenter(serial)
    }

    fn detach_presenter(&self, serial: u64) {
        let mut inner = self.lock();
        if inner.presenter_serial == serial {
            inner.presenter = None;
        }
    }

    fn set_slides(&self, dir: std::path::PathBuf, count: u32) {
        log::info!("phones: {count} slide images served from {}", dir.display());
        self.lock().slides = Some((dir, count));
    }

    /// The file for slide `n` (1-based), when the presenter has named the
    /// directory and `n` is in range. The name is built here from the
    /// number; nothing from the request reaches the file system.
    fn slide_file(&self, n: u32) -> Option<std::path::PathBuf> {
        let inner = self.lock();
        let (dir, count) = inner.slides.as_ref()?;
        (n >= 1 && n <= *count && inner.shown.contains(&n)).then(|| dir.join(format!("slide-{n:03}.jpg")))
    }

    fn set_live(&self, frame: LiveFrame) {
        self.lock().live = Some(Arc::new(frame));
    }

    /// The composite at `version`, when it is the one the presenter sent last.
    fn live_frame(&self, version: u32) -> Option<Arc<LiveFrame>> {
        self.lock().live.clone().filter(|f| f.version == version)
    }

    /// The newest composite's slide and version, for the report and the tests.
    pub fn live_now(&self) -> Option<(u32, u32)> {
        self.lock().live.as_ref().map(|f| (f.slide, f.version))
    }
}

/// A started server. Dropping it does not stop it; call `stop`.
pub struct Running {
    pub port: u16,
    pub bound: Vec<SocketAddr>,
    pub token: String,
    pub presenter_token: String,
    hub: Arc<Hub>,
    shutdown: watch::Sender<bool>,
    tasks: Vec<JoinHandle<()>>,
}

impl Running {
    /// The URL the QR carries for `address`.
    pub fn url_for(&self, address: &str) -> String {
        format!("http://{address}:{}/?t={}", self.port, self.token)
    }

    /// The presenter window's WebSocket URL, over loopback.
    pub fn presenter_ws(&self) -> String {
        format!("ws://127.0.0.1:{}/ws?role=presenter&t={}", self.port, self.presenter_token)
    }

    pub fn hub(&self) -> &Hub {
        &self.hub
    }

    pub fn hub_arc(&self) -> Arc<Hub> {
        self.hub.clone()
    }

    pub fn connected(&self) -> usize {
        self.hub.connected()
    }

    pub async fn stop(self) {
        let _ = self.shutdown.send(true);
        for t in self.tasks {
            let _ = tokio::time::timeout(Duration::from_secs(3), t).await;
        }
        log::info!("phones: server on port {} stopped", self.port);
    }
}

#[derive(Clone)]
struct AppState {
    hub: Arc<Hub>,
}

#[derive(Debug, Deserialize)]
struct WsQuery {
    #[serde(default)]
    role: Option<String>,
    #[serde(default)]
    t: Option<String>,
    #[serde(default)]
    id: Option<String>,
}

fn router(hub: Arc<Hub>) -> Router {
    Router::new()
        .route("/", get(page))
        .route("/ws", get(ws_route))
        .route("/slide/{n}", get(slide_route))
        .route("/live/{v}", get(live_route))
        .fallback(|| async { (StatusCode::NOT_FOUND, "Not found") })
        .with_state(AppState { hub })
}

async fn page() -> Response {
    let mut r = (StatusCode::OK, PHONE_PAGE).into_response();
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'; img-src 'self' data:; base-uri 'none'; form-action 'none'"));
    r
}

#[derive(Debug, Deserialize)]
struct SlideQuery {
    #[serde(default)]
    t: Option<String>,
}

/// The phone-sized image of one slide, to a phone that has the token.
/// Cached by the phone for the show: the number is the identity and the
/// image for a number never changes while the server runs.
async fn slide_route(RoutePath(n): RoutePath<String>, Query(q): Query<SlideQuery>, State(state): State<AppState>) -> Response {
    let hub = state.hub;
    if q.t.as_deref() != Some(hub.token.as_str()) {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    }
    let Some(file) = n.parse::<u32>().ok().and_then(|n| hub.slide_file(n)) else {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    };
    match tokio::fs::read(&file).await {
        Ok(bytes) => {
            let mut r = (StatusCode::OK, bytes).into_response();
            let h = r.headers_mut();
            h.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"));
            h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=86400"));
            h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
            r
        }
        Err(_) => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

/// The presenter's latest composite, to a phone that has the token. The
/// version is in the path, so the URL of one frame never changes meaning
/// and a phone can keep it for good; a version the server no longer holds
/// is a 404, and the phone falls back to the bare slide.
async fn live_route(RoutePath(v): RoutePath<String>, Query(q): Query<SlideQuery>, State(state): State<AppState>) -> Response {
    let hub = state.hub;
    if q.t.as_deref() != Some(hub.token.as_str()) {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    }
    let Some(frame) = v.parse::<u32>().ok().and_then(|v| hub.live_frame(v)) else {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    };
    let mut r = (StatusCode::OK, frame.jpeg.clone()).into_response();
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=31536000, immutable"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    r
}

async fn ws_route(
    ws: WebSocketUpgrade,
    Query(q): Query<WsQuery>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    State(state): State<AppState>,
) -> Response {
    let hub = state.hub;
    let token = q.t.unwrap_or_default();
    if q.role.as_deref() == Some("presenter") {
        let ok = token == hub.presenter_token;
        let loopback = peer.ip().is_loopback();
        // The socket must admit a live frame; text over `PRESENTER_MAX_MESSAGE` is still refused by the parser.
        let max = protocol::PRESENTER_MAX_MESSAGE.max(protocol::LIVE_FRAME_HEADER + protocol::LIVE_FRAME_MAX_BYTES);
        return ws.max_message_size(max).max_frame_size(max).on_upgrade(move |socket| async move {
            if !ok {
                close_now(socket, protocol::CLOSE_BAD_TOKEN, "bad presenter token").await;
            } else if !loopback {
                close_now(socket, protocol::CLOSE_NOT_LOOPBACK, "the presenter connects from this machine only").await;
            } else {
                presenter_socket(socket, hub).await;
            }
        });
    }
    let token_ok = token == hub.token;
    let id = q.id.unwrap_or_default();
    ws.max_message_size(protocol::PHONE_MAX_MESSAGE).max_frame_size(protocol::PHONE_MAX_MESSAGE).on_upgrade(move |socket| async move {
        if !token_ok {
            log::info!("phones: connection from {peer} refused: bad token");
            close_now(socket, protocol::CLOSE_BAD_TOKEN, "This link is old. Scan the code on the screen again.").await;
        } else if !protocol::valid_id(&id) {
            close_now(socket, protocol::CLOSE_BAD_ID, "bad id").await;
        } else {
            phone_socket(socket, hub, id).await;
        }
    })
}

async fn close_now(mut socket: WebSocket, code: u16, reason: &'static str) {
    let _ = socket.send(Message::Close(Some(CloseFrame { code, reason: reason.into() }))).await;
}

/// The writer half: drains a phone's or the presenter's outbound queue into its socket.
fn spawn_writer(mut sink: futures_util::stream::SplitSink<WebSocket, Message>, mut rx: mpsc::Receiver<Outbound>) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            match m {
                Outbound::Text(t) => {
                    if sink.send(Message::Text(t.into())).await.is_err() {
                        break;
                    }
                }
                Outbound::Close(code, reason) => {
                    let _ = sink.send(Message::Close(Some(CloseFrame { code, reason: reason.into() }))).await;
                    break;
                }
            }
        }
        let _ = sink.close().await;
    })
}

async fn phone_socket(socket: WebSocket, hub: Arc<Hub>, id: String) {
    let (sink, mut stream) = socket.split();
    let (tx, rx) = mpsc::channel::<Outbound>(64);
    let serial = match hub.attach_phone(&id, tx) {
        Ok(s) => s,
        Err(code) => {
            let mut sink = sink;
            let reason = match code {
                protocol::CLOSE_EJECTED => "You were removed from this show.",
                _ => "The show is full.",
            };
            let _ = sink.send(Message::Close(Some(CloseFrame { code, reason: reason.into() }))).await;
            return;
        }
    };
    let writer = spawn_writer(sink, rx);
    let mut limits = Limits::default();
    let mut strikes = 0u32;
    let mut joined = hub.name_of(&id).is_some();
    while let Some(Ok(msg)) = stream.next().await {
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            Message::Binary(_) => {
                strikes += 1;
                if strikes >= protocol::MAX_STRIKES {
                    break;
                }
                continue;
            }
            _ => continue,
        };
        let Some(parsed) = protocol::parse_phone(text.as_str()) else {
            hub.stats.malformed.fetch_add(1, Ordering::Relaxed);
            strikes += 1;
            if strikes >= protocol::MAX_STRIKES {
                let _ = hub.to_phone(&id, json!({ "t": "error", "code": "frames", "text": "Too many bad messages." }).to_string());
                break;
            }
            continue;
        };
        if !limits.allow(parsed.class()) {
            hub.stats.rate_limited.fetch_add(1, Ordering::Relaxed);
            if parsed.class() != Class::Stream {
                hub.to_phone(&id, json!({ "t": "error", "code": "rate", "text": "Too fast. Wait a moment." }).to_string());
            }
            continue;
        }
        match &parsed {
            PhoneMessage::Ping { at } => {
                hub.to_phone(&id, json!({ "t": "pong", "at": at }).to_string());
            }
            PhoneMessage::Join { name } => match protocol::clean_name(name) {
                Some(clean) => {
                    let (kept, rejoined) = hub.join(&id, clean);
                    joined = true;
                    hub.to_phone(&id, json!({ "t": "welcome", "id": id, "name": kept, "rejoined": rejoined }).to_string());
                }
                None => {
                    hub.to_phone(&id, json!({ "t": "error", "code": "name", "text": "Type a name to join." }).to_string());
                }
            },
            _ => {
                if joined {
                    hub.forward(&id, &parsed).await;
                }
            }
        }
    }
    hub.detach_phone(&id, serial);
    let _ = tokio::time::timeout(Duration::from_secs(2), writer).await;
}

async fn presenter_socket(socket: WebSocket, hub: Arc<Hub>) {
    let (sink, mut stream) = socket.split();
    let (tx, rx) = mpsc::channel::<Outbound>(256);
    let serial = hub.attach_presenter(tx);
    log::info!("phones: presenter window connected");
    let writer = spawn_writer(sink, rx);
    while let Some(Ok(msg)) = stream.next().await {
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            Message::Binary(bytes) => {
                // A live frame. Anything else binary is dropped and counted, as a phone's bad frame is.
                match protocol::parse_live_frame(&bytes) {
                    Some(frame) => hub.set_live(frame),
                    None => {
                        hub.stats.malformed.fetch_add(1, Ordering::Relaxed);
                    }
                }
                continue;
            }
            _ => continue,
        };
        let Some(parsed) = protocol::parse_presenter(text.as_str()) else { continue };
        match parsed {
            PresenterMessage::Broadcast { msg } => {
                hub.broadcast(&msg.to_string());
            }
            PresenterMessage::Send { to, msg } => {
                hub.to_phone(&to, msg.to_string());
            }
            PresenterMessage::Eject { id } => hub.eject(&id),
            PresenterMessage::Roster => {
                let inner = hub.lock();
                Hub::send_roster(&inner);
            }
            PresenterMessage::Slides { dir, count } => hub.set_slides(std::path::PathBuf::from(dir), count),
            PresenterMessage::Shown { n } => hub.presenter_shown(n),
        }
    }
    hub.detach_presenter(serial);
    log::info!("phones: presenter window disconnected");
    let _ = tokio::time::timeout(Duration::from_secs(2), writer).await;
}

/// Bind every address on one port, trying successive ports from the
/// preferred one. All listeners share the port so one QR URL is right for
/// every address.
async fn bind_all(addresses: &[IpAddr], port: u16) -> Result<(u16, Vec<TcpListener>), String> {
    let attempts = if port == 0 { 1 } else { PORT_ATTEMPTS };
    let mut last_err = String::new();
    for i in 0..attempts {
        let p = if port == 0 { 0 } else { port.saturating_add(i) };
        let mut listeners = Vec::new();
        let mut chosen = p;
        let mut failed = false;
        for (n, a) in addresses.iter().enumerate() {
            match TcpListener::bind(SocketAddr::new(*a, chosen)).await {
                Ok(l) => {
                    if n == 0 && chosen == 0 {
                        chosen = l.local_addr().map(|x| x.port()).unwrap_or(0);
                    }
                    listeners.push(l);
                }
                Err(e) => {
                    last_err = format!("{a}:{chosen}: {e}");
                    log::info!("phones: port {chosen} on {a} unavailable ({e}), trying the next");
                    failed = true;
                    break;
                }
            }
        }
        if !failed {
            return Ok((chosen, listeners));
        }
    }
    Err(format!("No free port from {port} to {}: {last_err}", port.saturating_add(attempts.saturating_sub(1))))
}

/// Start the server. Loopback is bound as well as every LAN address given,
/// on one port. Needs a tokio runtime.
pub async fn start(config: ServerConfig) -> Result<Running, String> {
    let mut addresses: Vec<IpAddr> = vec![IpAddr::V4(Ipv4Addr::LOCALHOST)];
    for a in &config.addresses {
        if !addresses.contains(a) && !a.is_loopback() {
            addresses.push(*a);
        }
    }
    log::info!("phones: binding {} on port {}", addresses.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", "), config.port);
    let (port, listeners) = bind_all(&addresses, config.port).await?;
    let token = config.token.unwrap_or_else(|| protocol::new_token(protocol::TOKEN_CHARS));
    let presenter_token = config.presenter_token.unwrap_or_else(|| protocol::new_token(32));
    let hub = Arc::new(Hub {
        token: token.clone(),
        presenter_token: presenter_token.clone(),
        max_phones: config.max_phones,
        inner: Mutex::new(Inner::default()),
        stats: Stats::default(),
    });
    let (shutdown, _) = watch::channel(false);
    let mut tasks = Vec::new();
    let mut bound = Vec::new();
    for l in listeners {
        let addr = l.local_addr().map_err(|e| e.to_string())?;
        bound.push(addr);
        let app = router(hub.clone());
        let mut rx = shutdown.subscribe();
        tasks.push(tokio::spawn(async move {
            let serve = axum::serve(l, app.into_make_service_with_connect_info::<SocketAddr>()).with_graceful_shutdown(async move {
                let _ = rx.changed().await;
            });
            if let Err(e) = serve.await {
                log::info!("phones: server on {addr} ended: {e}");
            }
        }));
    }
    log::info!("phones: listening on port {port} ({})", bound.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", "));
    Ok(Running { port, bound, token, presenter_token, hub, shutdown, tasks })
}

/// Fetch `/` from `url` the way a phone would, from this machine, within
/// `timeout`. Separates a server that is down from a network that blocks
/// phones. Plain HTTP over a TCP stream; the page is small.
pub async fn self_test(url: &str, timeout: Duration) -> Result<String, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let rest = url.strip_prefix("http://").ok_or_else(|| format!("{url} is not an http:// URL"))?;
    let host_port = rest.split('/').next().unwrap_or(rest);
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse::<u16>().map_err(|_| format!("bad port in {url}"))?),
        None => (host_port.to_string(), 80),
    };
    let connect = tokio::net::TcpStream::connect((host.as_str(), port));
    let mut stream = tokio::time::timeout(timeout, connect)
        .await
        .map_err(|_| format!("No answer from {host}:{port} within {} ms.", timeout.as_millis()))?
        .map_err(|e| format!("Cannot connect to {host}:{port}: {e}"))?;
    stream.write_all(format!("GET / HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n").as_bytes()).await.map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    let _ = tokio::time::timeout(timeout, stream.read_to_end(&mut buf))
        .await
        .map_err(|_| "The server accepted the connection and then did not answer.".to_string())?;
    let text = String::from_utf8_lossy(&buf);
    if text.starts_with("HTTP/1.1 200") && text.contains("deckrt-phone-page") {
        Ok(format!("The phone page answers at {host}:{port} from this laptop."))
    } else {
        Err(format!("{host}:{port} answered with something other than the phone page: {}", text.lines().next().unwrap_or("")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stream of phones that come and go doesn't grow the list without end, and the ones still
    /// connected are never the ones forgotten.
    #[test]
    fn phones_that_left_are_forgotten_past_a_limit() {
        let hub = Hub { token: "t".into(), presenter_token: "p".into(), max_phones: 4, inner: Mutex::new(Inner::default()), stats: Stats::default() };
        let (tx, _rx) = mpsc::channel(4);
        let here = hub.attach_phone("here", tx.clone()).unwrap();
        let _ = here;
        for i in 0..200 {
            let id = format!("p{i}");
            let serial = hub.attach_phone(&id, tx.clone()).unwrap();
            hub.join(&id, format!("Phone {i}"));
            hub.detach_phone(&id, serial);
        }
        let inner = hub.lock();
        assert!(inner.phones.len() <= 16, "{} kept", inner.phones.len());
        assert!(inner.phones.contains_key("here"));
        assert!(inner.phones.contains_key("p199"));
    }

    #[tokio::test]
    async fn the_next_port_is_taken_when_the_first_is_busy() {
        let blocker = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let busy = blocker.local_addr().unwrap().port();
        let (port, listeners) = bind_all(&[IpAddr::V4(Ipv4Addr::LOCALHOST)], busy).await.unwrap();
        // Windows hands out ephemeral ports in sequence, so another test may hold busy + 1 too:
        // the next free one after it is right, not busy + 1 exactly.
        assert!(port > busy && port < busy + PORT_ATTEMPTS, "{port} after {busy}");
        assert_eq!(listeners.len(), 1);
    }

    #[tokio::test]
    async fn unknown_paths_are_404_and_the_page_carries_its_marker() {
        let running = start(ServerConfig { port: 0, ..Default::default() }).await.unwrap();
        let ok = self_test(&format!("http://127.0.0.1:{}/", running.port), Duration::from_secs(2)).await;
        assert!(ok.is_ok(), "{ok:?}");
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for path in [
            "/etc/passwd",
            "/../Cargo.toml",
            "/assets/x.glb",
            "/index.html",
            "/slide/1",
            &format!("/slide/1?t={}", running.token),
            "/slide/../manifest.json",
        ] {
            let mut s = tokio::net::TcpStream::connect(("127.0.0.1", running.port)).await.unwrap();
            s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
            let mut buf = Vec::new();
            s.read_to_end(&mut buf).await.unwrap();
            let text = String::from_utf8_lossy(&buf);
            assert!(text.starts_with("HTTP/1.1 404") || text.starts_with("HTTP/1.1 400"), "{path}: {}", text.lines().next().unwrap_or(""));
        }
        running.stop().await;
    }
}
