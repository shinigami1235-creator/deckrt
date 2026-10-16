//! The phones during a show, on the app's side: the server, the roster, the control queue, the
//! votes, what each phone is told (`show` and `you`), the phone-sized slide pictures, the live
//! frames, the results file, and the Phones section of the presenter window.
//!
//! The server (`deckcraft-phones`) validates and relays; everything a phone wants is applied
//! here to the show's object states, and the audience screen draws those.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use deckcraft_interactive::object::Config;
use deckcraft_interactive::state::{ObjectState, TypeState};
use deckcraft_interactive::poll;
use deckcraft_model::Presentation;
use deckcraft_phones::{AddressInfo, Inbound, PhoneMessage, RosterEntry, hotspot, results};
use egui::{Color32, Ui, vec2};
use serde_json::{Value, json};

use super::show::{Ext, Item, objects_on};

/// The port the server tries first (the old deckrt's), then the ones after it.
pub const PORT: u16 = 8720;
/// Width of the slide pictures phones get.
const PHONE_WIDTH: u32 = 960;
/// A live frame goes out at most this often while something moves.
const LIVE_INTERVAL: f64 = 0.4;

/// Who holds control and who waits. The presenter holds it until something is granted.
#[derive(Default, Debug, Clone)]
pub struct ControlQueue {
    pub queue: Vec<String>,
    pub holders: Vec<String>,
    pub max: usize,
}

impl ControlQueue {
    pub fn new() -> Self {
        ControlQueue { queue: vec![], holders: vec![], max: 1 }
    }
    pub fn request(&mut self, id: &str) -> bool {
        if self.holders.iter().any(|h| h == id) || self.queue.iter().any(|q| q == id) {
            return false;
        }
        self.queue.push(id.to_string());
        true
    }
    pub fn withdraw(&mut self, id: &str) -> bool {
        let before = self.holders.len() + self.queue.len();
        self.holders.retain(|h| h != id);
        self.queue.retain(|q| q != id);
        before != self.holders.len() + self.queue.len()
    }
    /// Grant control, whether or not it was asked for. Returns the holders displaced to make room.
    pub fn grant(&mut self, id: &str) -> Vec<String> {
        if self.holders.iter().any(|h| h == id) {
            return vec![];
        }
        self.queue.retain(|q| q != id);
        self.holders.push(id.to_string());
        let mut displaced = vec![];
        while self.holders.len() > self.max.max(1) {
            displaced.push(self.holders.remove(0));
        }
        displaced
    }
    pub fn revoke(&mut self, id: &str) -> bool {
        let n = self.holders.len();
        self.holders.retain(|h| h != id);
        n != self.holders.len()
    }
    pub fn set_max(&mut self, n: usize) -> Vec<String> {
        self.max = n.clamp(1, 20);
        let mut displaced = vec![];
        while self.holders.len() > self.max {
            displaced.push(self.holders.remove(0));
        }
        displaced
    }
    pub fn standing(&self, id: &str) -> (&'static str, Option<usize>) {
        if self.holders.iter().any(|h| h == id) {
            return ("holder", None);
        }
        match self.queue.iter().position(|q| q == id) {
            Some(i) => ("queued", Some(i + 1)),
            None => ("none", None),
        }
    }
}

/// Names as the presenter's list shows them: a second Sam is `Sam (2)`.
pub fn display_names(roster: &[RosterEntry]) -> HashMap<String, String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut out = HashMap::new();
    for p in roster {
        let key = p.name.to_lowercase();
        let n = seen.entry(key).or_insert(0);
        *n += 1;
        out.insert(p.id.clone(), if *n == 1 { p.name.clone() } else { format!("{} ({})", p.name, n) });
    }
    out
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Off,
    Running,
    Failed(String),
}

/// A phone-sized picture of every slide, made on another thread when the show starts.
struct SlidesJob {
    dir: PathBuf,
    rx: Receiver<Result<u32, String>>,
}

pub struct Phones {
    server: Option<deckcraft_phones::PhoneServer>,
    pub status: Status,
    pub addresses: Vec<AddressInfo>,
    /// The address in the QR.
    pub address: String,
    pub url: String,
    pub roster: Vec<RosterEntry>,
    pub names: HashMap<String, String>,
    pub queue: ControlQueue,
    pub ever_joined: BTreeSet<String>,
    last_show: String,
    deck_path: Option<PathBuf>,
    results_due: Option<f64>,
    pub results_path: Option<PathBuf>,
    slides: Option<SlidesJob>,
    pub slides_ready: Option<u32>,
    pub slides_error: Option<String>,
    /// Which model a phone turns: object id, or `all`.
    targets: HashMap<String, String>,
    live_version: u32,
    live_key: String,
    live_sent_at: f64,
    live_rx: Option<Receiver<Option<Vec<u8>>>>,
    live_current: Option<(usize, u32)>,
    /// Latency samples: phone's `at` to applied, milliseconds.
    pub inputs: Vec<f64>,
}

impl Phones {
    /// Start the server on every private address of the machine. `deck_path` is where the
    /// results file goes.
    pub fn start(deck_path: Option<PathBuf>, address: Option<String>) -> Phones {
        Self::start_with(deck_path, address, None)
    }

    fn start_with(deck_path: Option<PathBuf>, address: Option<String>, token: Option<String>) -> Phones {
        let mut addresses = deckcraft_phones::list_addresses();
        deckcraft_phones::addresses::sort(&mut addresses);
        let chosen = address
            .filter(|a| addresses.iter().any(|x| &x.address == a))
            .or_else(|| deckcraft_phones::pick_default(&addresses).map(|a| a.address.clone()))
            .or_else(|| addresses.first().map(|a| a.address.clone()))
            .unwrap_or_else(|| "127.0.0.1".to_string());
        let ips: Vec<std::net::IpAddr> = addresses.iter().filter_map(|a| a.address.parse().ok()).collect();
        let (server, status, url) = match deckcraft_phones::PhoneServer::start_with_token(ips, PORT, token) {
            Ok(s) => {
                let url = s.url_for(&chosen);
                log::info!("deckrt: phone server at {url}");
                (Some(s), Status::Running, url)
            }
            Err(e) => {
                log::warn!("deckrt: the phone server did not start: {e}");
                (None, Status::Failed(e), String::new())
            }
        };
        Phones {
            server,
            status,
            addresses,
            address: chosen,
            url,
            roster: vec![],
            names: HashMap::new(),
            queue: ControlQueue::new(),
            ever_joined: BTreeSet::new(),
            last_show: String::new(),
            deck_path,
            results_due: None,
            results_path: None,
            slides: None,
            slides_ready: None,
            slides_error: None,
            targets: HashMap::new(),
            live_version: (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) & 0x3fff_ffff) as u32,
            live_key: String::new(),
            live_sent_at: 0.0,
            live_rx: None,
            live_current: None,
            inputs: vec![],
        }
    }

    pub fn running(&self) -> bool {
        self.server.is_some()
    }

    pub fn connected(&self) -> usize {
        self.roster.iter().filter(|r| r.connected).count()
    }

    /// Point the QR at another address (the picker, or after the hotspot came up): the server is
    /// restarted on the current address list with the same token.
    pub fn refresh(&mut self, address: Option<String>, ext_changed: &mut bool) {
        let deck = self.deck_path.clone();
        let old = self.server.take();
        let token = old.as_ref().map(|s| s.token()).filter(|t| !t.is_empty());
        if let Some(s) = old {
            s.stop();
        }
        let fresh = Phones::start_with(deck, address.or_else(|| Some(self.address.clone())), token);
        self.server = fresh.server;
        self.status = fresh.status;
        self.addresses = fresh.addresses;
        self.address = fresh.address;
        self.url = fresh.url;
        self.last_show.clear();
        self.live_current = None;
        self.live_key.clear();
        if let Some(n) = self.slides_ready
            && let Some(dir) = self.slides.as_ref().map(|s| s.dir.clone())
            && let Some(s) = &self.server
        {
            s.set_slides(dir, n);
        }
        *ext_changed = true;
    }

    /// Make the phone-sized pictures of every slide on another thread.
    pub fn prepare_slides(&mut self, doc: &Arc<Presentation>) {
        if self.slides.is_some() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("deckrt-phones-{}", std::process::id()));
        let (tx, rx) = channel();
        let doc = doc.clone();
        let out = dir.clone();
        let spawned = std::thread::Builder::new().name("deckrt-phone-slides".into()).spawn(move || {
            let r = (|| -> Result<u32, String> {
                std::fs::create_dir_all(&out).map_err(|e| format!("cannot make {}: {e}", out.display()))?;
                let h = (PHONE_WIDTH as f64 * doc.slide_size.height / doc.slide_size.width.max(1.0)).round().clamp(1.0, 4096.0) as u32;
                for i in 0..doc.slides.len() {
                    let skip = super::show::hidden_in_show(&doc, i);
                    let img = deckcraft_render::render_slide(
                        &doc,
                        i,
                        &deckcraft_render::RenderOpts {
                            scale: PHONE_WIDTH as f64 / doc.slide_size.width.max(1.0),
                            size: Some((PHONE_WIDTH, h)),
                            skip: &skip,
                            ..Default::default()
                        },
                    );
                    let jpeg = jpeg_of(img.width, img.height, &img.pixels)?;
                    std::fs::write(out.join(format!("slide-{:03}.jpg", i + 1)), jpeg).map_err(|e| e.to_string())?;
                }
                Ok(doc.slides.len() as u32)
            })();
            let _ = tx.send(r);
        });
        if spawned.is_ok() {
            self.slides = Some(SlidesJob { dir, rx });
        }
    }

    /// Everything the phones sent since the last frame, applied to the show. Returns whether the
    /// presenter view should redraw.
    #[allow(clippy::too_many_arguments)]
    pub fn logic(
        &mut self,
        ext_states: &mut BTreeMap<u32, ObjectState>,
        doc: &Arc<Presentation>,
        idx: usize,
        pointer: &mut Option<[f32; 2]>,
        changed_at: &mut f64,
        now: f64,
        blackout: bool,
    ) -> bool {
        let mut redraw = false;
        // The slide pictures.
        if let Some(job) = &self.slides
            && let Ok(r) = job.rx.try_recv()
        {
            match r {
                Ok(n) => {
                    self.slides_ready = Some(n);
                    if let Some(s) = &self.server {
                        s.set_slides(job.dir.clone(), n);
                    }
                    self.last_show.clear();
                }
                Err(e) => self.slides_error = Some(format!("Slide pictures for phones not made: {e}")),
            }
            redraw = true;
        }
        // A live frame that finished encoding.
        if let Some(rx) = &self.live_rx
            && let Ok(frame) = rx.try_recv()
        {
            self.live_rx = None;
            if let Some(jpeg) = frame
                && let Some(s) = &self.server
            {
                self.live_version = self.live_version.wrapping_add(1);
                s.set_live(idx as u32 + 1, self.live_version, jpeg);
                self.live_current = Some((idx, self.live_version));
                self.last_show.clear();
            }
        }
        let Some(server) = self.server.as_mut() else { return redraw };
        let inbound = server.poll();
        let models: Vec<Item> = objects_on(doc, idx).into_iter().filter(|it| it.obj.kind() == deckcraft_interactive::Kind::Model && ext_states.get(&it.id).is_none_or(|s| s.visible)).collect();
        let poll_item = objects_on(doc, idx).into_iter().find(|it| it.obj.kind() == deckcraft_interactive::Kind::Poll);
        let mut tell_you: Vec<String> = vec![];
        for m in inbound {
            redraw = true;
            match m {
                Inbound::Hello { .. } => self.last_show.clear(),
                Inbound::Roster(list) => {
                    // Keep the order phones joined in.
                    let mut ordered: Vec<RosterEntry> = vec![];
                    for r in &self.roster {
                        if let Some(n) = list.iter().find(|x| x.id == r.id) {
                            ordered.push(n.clone());
                        }
                    }
                    for n in list {
                        if !ordered.iter().any(|x| x.id == n.id) {
                            ordered.push(n);
                        }
                    }
                    self.roster = ordered;
                    for r in &self.roster {
                        self.ever_joined.insert(r.id.clone());
                    }
                    self.names = display_names(&self.roster);
                    self.last_show.clear();
                }
                Inbound::Joined { id, .. } => {
                    self.ever_joined.insert(id.clone());
                    tell_you.push(id);
                    self.last_show.clear();
                }
                Inbound::From { id, msg, .. } => match msg {
                    PhoneMessage::Vote { poll: poll_id, option } => {
                        let Some(p) = &poll_item else { continue };
                        if p.id.to_string() != poll_id {
                            continue;
                        }
                        let Config::Poll(c) = &p.obj.config else { continue };
                        if (option as usize) >= c.options.len() {
                            continue;
                        }
                        if let Some(ObjectState { st: TypeState::Poll { open: true, revealed: false, votes }, .. }) = ext_states.get_mut(&p.id) {
                            votes.insert(id.clone(), option as usize);
                            *changed_at = now;
                            self.results_due = Some(now + 1.5);
                            tell_you.push(id);
                        }
                    }
                    PhoneMessage::Control { want } => {
                        if want {
                            self.queue.request(&id);
                        } else {
                            self.queue.withdraw(&id);
                            if self.queue.holders.is_empty() {
                                *pointer = None;
                            }
                        }
                        tell_you.push(id);
                        tell_you.extend(self.queue.queue.iter().cloned());
                        self.last_show.clear();
                    }
                    PhoneMessage::Drag { dx, dy, at } => {
                        if !self.queue.holders.iter().any(|h| h == &id) || !dx.is_finite() || !dy.is_finite() {
                            continue;
                        }
                        let want = self.targets.get(&id).cloned().unwrap_or_default();
                        let turn: Vec<u32> = if want == "all" || (models.len() > 1 && want.is_empty() && self.together(&models)) {
                            models.iter().map(|m| m.id).collect()
                        } else {
                            models.iter().find(|m| m.id.to_string() == want).or(models.first()).map(|m| vec![m.id]).unwrap_or_default()
                        };
                        for id in turn {
                            if let Some(ObjectState { st: TypeState::Model { turn, .. }, .. }) = ext_states.get_mut(&id) {
                                turn[0] += dx * 240.0;
                                turn[1] = (turn[1] + dy * 150.0).clamp(-180.0, 180.0);
                            }
                        }
                        *changed_at = now;
                        self.note_input(at);
                    }
                    PhoneMessage::Target { id: target } => {
                        if !self.queue.holders.iter().any(|h| h == &id) {
                            continue;
                        }
                        if target == "all" || models.iter().any(|m| m.id.to_string() == target) {
                            self.targets.insert(id.clone(), target);
                            tell_you.push(id);
                        }
                    }
                    PhoneMessage::Touch { x, y, at } => {
                        if !self.queue.holders.iter().any(|h| h == &id) {
                            continue;
                        }
                        *pointer = match (x, y) {
                            (Some(x), Some(y)) if x.is_finite() && y.is_finite() => Some([x.clamp(0.0, 1.0) as f32, y.clamp(0.0, 1.0) as f32]),
                            _ => None,
                        };
                        self.note_input(at);
                    }
                    PhoneMessage::Join { .. } | PhoneMessage::Ping { .. } | PhoneMessage::Draw { .. } | PhoneMessage::Erase { .. } => {}
                },
            }
        }
        // What every phone is told, when it changed.
        let msg = self.show_message(ext_states, doc, idx, &models, poll_item.as_ref(), blackout);
        let text = msg.to_string();
        if text != self.last_show {
            if let Some(s) = &self.server {
                s.broadcast(&msg);
            }
            self.last_show = text;
            // A different slide may carry a different poll: every phone hears its own standing.
            tell_you.extend(self.roster.iter().map(|r| r.id.clone()));
        }
        tell_you.sort();
        tell_you.dedup();
        for id in tell_you {
            let m = self.you_message(ext_states, &id, poll_item.as_ref(), &models);
            if let Some(s) = &self.server {
                s.send(&id, &m);
            }
        }
        if self.results_due.is_some_and(|t| now >= t) {
            self.results_due = None;
            self.write_results(ext_states, doc);
        }
        redraw
    }

    fn note_input(&mut self, phone_at: f64) {
        let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0);
        if phone_at.is_finite() && phone_at > 0.0 {
            self.inputs.push(now_ms - phone_at);
            if self.inputs.len() > 500 {
                self.inputs.remove(0);
            }
        }
    }

    /// Models on the slide sharing one link group move together.
    fn together(&self, models: &[Item]) -> bool {
        let groups: Vec<String> = models
            .iter()
            .filter_map(|m| match &m.obj.config {
                Config::Model(c) if !c.link_group.trim().is_empty() => Some(c.link_group.clone()),
                _ => None,
            })
            .collect();
        groups.len() == models.len() && groups.windows(2).all(|w| w[0] == w[1])
    }

    fn show_message(&self, states: &BTreeMap<u32, ObjectState>, doc: &Presentation, idx: usize, models: &[Item], poll_item: Option<&Item>, blackout: bool) -> Value {
        let poll_msg = poll_item.and_then(|p| {
            let Config::Poll(c) = &p.obj.config else { return None };
            let (open, revealed) = match states.get(&p.id) {
                Some(ObjectState { visible, st: TypeState::Poll { open, revealed, .. } }) => (*open && *visible, *revealed && c.reveal_correct),
                _ => (false, false),
            };
            Some(json!({"id": p.id.to_string(), "question": c.question, "options": c.options, "open": open, "revealed": revealed, "correct": if revealed { c.correct } else { None }}))
        });
        let has_model = !models.is_empty();
        let slide = idx + 1;
        let image = match self.slides_ready {
            Some(n) if slide as u32 <= n && !blackout => Some(format!("/slide/{slide}")),
            _ => None,
        };
        let live = self.live_current.filter(|(s, _)| *s == idx).map(|(_, v)| format!("/live/{v}"));
        let mut why = vec![];
        if poll_msg.is_some() {
            why.push("poll");
        }
        if has_model {
            why.push("model");
        }
        json!({
            "t": "show",
            "slide": slide,
            "slides": doc.slides.len(),
            "image": image,
            "blackout": blackout,
            "live": live,
            "poll": poll_msg,
            "control": {
                "holders": self.queue.holders.iter().map(|id| json!({"id": id, "name": self.names.get(id).cloned().unwrap_or_default()})).collect::<Vec<_>>(),
                "waiting": self.queue.queue.len(),
                "max": self.queue.max,
            },
            "has_model": has_model,
            "models": models.iter().enumerate().map(|(i, m)| json!({"id": m.id.to_string(), "name": if m.obj.label.trim().is_empty() { format!("Model {}", i + 1) } else { m.obj.label.clone() }})).collect::<Vec<_>>(),
            "linked": models.len() > 1 && self.together(models),
            "interactive": !why.is_empty(),
            "why": why,
            "aspect": doc.slide_size.width / doc.slide_size.height.max(1.0),
        })
    }

    fn you_message(&self, states: &BTreeMap<u32, ObjectState>, id: &str, poll_item: Option<&Item>, models: &[Item]) -> Value {
        let (poll_id, vote) = match poll_item {
            Some(p) => {
                let v = match states.get(&p.id) {
                    Some(ObjectState { st: TypeState::Poll { votes, .. }, .. }) => votes.get(id).copied(),
                    _ => None,
                };
                (Some(p.id.to_string()), v)
            }
            None => (None, None),
        };
        let (control, position) = self.queue.standing(id);
        let target = if models.is_empty() {
            String::new()
        } else if models.len() > 1 && (self.targets.get(id).is_some_and(|t| t == "all") || (self.targets.get(id).is_none() && self.together(models))) {
            "all".to_string()
        } else {
            self.targets.get(id).filter(|t| models.iter().any(|m| &m.id.to_string() == *t)).cloned().unwrap_or_else(|| models[0].id.to_string())
        };
        let n = self.roster.iter().position(|r| r.id == id).unwrap_or(0);
        let colors = ["#ff5c5c", "#4c8dff", "#2ea05c", "#ffb049", "#b56cff", "#00b5c8"];
        json!({"t": "you", "poll": poll_id, "vote": vote, "control": control, "position": position, "color": colors[n % colors.len()], "target": target})
    }

    pub fn grant(&mut self, id: &str) {
        self.queue.grant(id);
        self.last_show.clear();
        self.tell_all();
    }

    pub fn revoke(&mut self, id: &str, pointer: &mut Option<[f32; 2]>) {
        self.queue.revoke(id);
        if self.queue.holders.is_empty() {
            *pointer = None;
        }
        self.last_show.clear();
        self.tell_all();
    }

    pub fn set_max_holders(&mut self, n: usize) {
        self.queue.set_max(n);
        self.last_show.clear();
        self.tell_all();
    }

    /// Close the phone, refuse its id for the rest of the show, drop its votes.
    pub fn eject(&mut self, id: &str, states: &mut BTreeMap<u32, ObjectState>, pointer: &mut Option<[f32; 2]>) {
        self.queue.withdraw(id);
        for s in states.values_mut() {
            if let TypeState::Poll { votes, .. } = &mut s.st {
                votes.remove(id);
            }
        }
        if let Some(s) = &self.server {
            s.eject(id);
        }
        self.roster.retain(|r| r.id != id);
        self.names = display_names(&self.roster);
        if self.queue.holders.is_empty() {
            *pointer = None;
        }
        self.last_show.clear();
        self.results_due = Some(0.0);
    }

    /// Everyone hears their standing again after the queue moved; `logic` sends it.
    fn tell_all(&mut self) {
        // Forcing the show message out again makes `logic` send every `you`.
        self.last_show.clear();
    }

    /// Fetch the phone page from the laptop itself.
    pub fn self_test(&self) -> Result<String, String> {
        match &self.server {
            Some(s) => s.self_test(&self.url),
            None => Err("The server is not running.".into()),
        }
    }

    pub fn start_hotspot(&mut self) -> String {
        match hotspot::start() {
            Ok(r) => r.message,
            Err(e) => e,
        }
    }

    fn results(&self, states: &BTreeMap<u32, ObjectState>, doc: &Presentation) -> results::ShowResults {
        let mut polls = vec![];
        for (i, _) in doc.slides.iter().enumerate() {
            for it in objects_on(doc, i) {
                let Config::Poll(c) = &it.obj.config else { continue };
                let (votes, revealed) = match states.get(&it.id) {
                    Some(ObjectState { st: TypeState::Poll { votes, revealed, .. }, .. }) => (votes.clone(), *revealed),
                    _ => (BTreeMap::new(), false),
                };
                polls.push(results::PollResult {
                    id: it.id.to_string(),
                    question: c.question.clone(),
                    options: c.options.clone(),
                    counts: poll::tally(c.options.len(), &votes),
                    correct: c.correct,
                    voters: votes.len() as u32,
                    revealed,
                });
            }
        }
        results::ShowResults { deck: String::new(), written_at: String::new(), phones: self.ever_joined.len() as u32, polls }
    }

    /// `<deck>.results.json` next to the deck, when the deck has a poll and a file.
    pub fn write_results(&mut self, states: &BTreeMap<u32, ObjectState>, doc: &Presentation) {
        let Some(deck) = &self.deck_path else { return };
        let r = self.results(states, doc);
        if r.polls.is_empty() {
            return;
        }
        match results::write_results(deck, r) {
            Ok(p) => self.results_path = Some(p),
            Err(e) => log::warn!("deckrt: results not written: {e}"),
        }
    }

    /// Compose the picture a phone shows for this slide (the slide with its objects as they are
    /// now) on another thread, when phones are connected and something changed since the last.
    #[allow(clippy::too_many_arguments)]
    pub fn live_frame(&mut self, ext: &Ext, doc: &Arc<Presentation>, idx: usize, models: &mut super::model_view::Models, now: f64) {
        if self.server.is_none() || self.connected() == 0 || self.live_rx.is_some() || now - self.live_sent_at < LIVE_INTERVAL {
            return;
        }
        let items = objects_on(doc, idx);
        let drawn: Vec<&Item> = items.iter().filter(|it| matches!(it.obj.kind(), deckcraft_interactive::Kind::Model | deckcraft_interactive::Kind::ImageSwap | deckcraft_interactive::Kind::Scrub | deckcraft_interactive::Kind::Wipe) && ext.states.get(&it.id).is_none_or(|s| s.visible)).collect();
        if drawn.is_empty() {
            if self.live_current.is_some() {
                self.live_current = None;
                self.last_show.clear();
            }
            return;
        }
        let key = format!("{idx}|{:?}", drawn.iter().map(|it| ext.states.get(&it.id).map(|s| format!("{:?}", s.st))).collect::<Vec<_>>());
        if key == self.live_key && self.live_current.is_some_and(|(s, _)| s == idx) {
            return;
        }
        // Everything the thread needs, gathered now.
        let mut layers: Vec<Layer> = vec![];
        for it in drawn {
            let Some(os) = ext.states.get(&it.id) else { continue };
            let r = [it.xfrm.x / doc.slide_size.width.max(1.0), it.xfrm.y / doc.slide_size.height.max(1.0), it.xfrm.w / doc.slide_size.width.max(1.0), it.xfrm.h / doc.slide_size.height.max(1.0)];
            match (&os.st, &it.obj.config, &it.shape.kind) {
                (TypeState::Model { layers: vis, turn, zoom, .. }, Config::Model(c), deckcraft_model::ShapeKind::Model3d { media, .. }) => {
                    if let Some(model) = models.model(doc, *media) {
                        let t = super::model_view::Turn { turn: *turn, zoom: *zoom };
                        let view = super::model_view::view_of(&model, c, &t);
                        let hidden: Vec<String> = vis.iter().filter(|(_, on)| !**on).map(|(n, _)| n.clone()).collect();
                        layers.push(Layer::Model { rect: r, model, view, hidden });
                    }
                }
                (TypeState::Swap { index, .. }, Config::ImageSwap(c), _) => {
                    if let Some(m) = c.images.get(*index).or(c.images.first())
                        && let Some(img) = super::pictures::Pictures::decode(doc, m.0)
                    {
                        layers.push(Layer::Picture { rect: r, img, cover: false });
                    }
                }
                (TypeState::Scrub { index }, Config::Scrub(c), _) => {
                    if let Some(m) = c.images.get(*index).or(c.images.first())
                        && let Some(img) = super::pictures::Pictures::decode(doc, m.0)
                    {
                        layers.push(Layer::Picture { rect: r, img, cover: false });
                    }
                }
                (TypeState::Wipe { position }, Config::Wipe(c), _) => {
                    if let (Some(before), Some(after)) = (super::pictures::Pictures::decode(doc, c.before.0), super::pictures::Pictures::decode(doc, c.after.0)) {
                        layers.push(Layer::Picture { rect: r, img: after, cover: true });
                        let cut = [r[0], r[1], r[2] * *position, r[3]];
                        layers.push(Layer::Clipped { rect: r, clip: cut, img: before });
                    }
                }
                _ => {}
            }
        }
        let base = self.slides.as_ref().map(|s| s.dir.join(format!("slide-{:03}.jpg", idx + 1)));
        let (tx, rx) = channel();
        let aspect = doc.slide_size.width / doc.slide_size.height.max(1.0);
        let spawned = std::thread::Builder::new().name("deckrt-live-frame".into()).spawn(move || {
            let _ = tx.send(compose(base, aspect, layers));
        });
        if spawned.is_ok() {
            self.live_rx = Some(rx);
            self.live_key = key;
            self.live_sent_at = now;
        }
    }

    /// Stop the server: the phones are told, the results are written.
    pub fn stop(mut self, states: &BTreeMap<u32, ObjectState>, doc: &Presentation) {
        if let Some(s) = &self.server {
            s.broadcast(&json!({"t": "ended"}));
        }
        self.write_results(states, doc);
        if let Some(s) = self.server.take() {
            s.stop();
        }
        if let Some(j) = self.slides.take() {
            let _ = std::fs::remove_dir_all(&j.dir);
        }
    }
}

/// One thing drawn into a live frame.
enum Layer {
    Model { rect: [f64; 4], model: Arc<deckcraft_model3d::Model>, view: deckcraft_model3d::View, hidden: Vec<String> },
    Picture { rect: [f64; 4], img: image::RgbaImage, cover: bool },
    Clipped { rect: [f64; 4], clip: [f64; 4], img: image::RgbaImage },
}

fn jpeg_of(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::RgbaImage::from_raw(w, h, rgba.to_vec()).ok_or("bad picture size")?;
    let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
    let mut out = std::io::Cursor::new(Vec::new());
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 72);
    enc.encode_image(&rgb).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// The slide picture with the objects over it, as a JPEG.
fn compose(base: Option<PathBuf>, aspect: f64, layers: Vec<Layer>) -> Option<Vec<u8>> {
    let w = PHONE_WIDTH;
    let h = (w as f64 / aspect.max(0.1)).round().clamp(1.0, 4096.0) as u32;
    let mut canvas = match base.and_then(|p| std::fs::read(p).ok()).and_then(|b| image::load_from_memory(&b).ok()) {
        Some(b) => image::imageops::resize(&b.to_rgba8(), w, h, image::imageops::FilterType::Triangle),
        None => image::RgbaImage::from_pixel(w, h, image::Rgba([255, 255, 255, 255])),
    };
    let px = |r: [f64; 4]| -> (i64, i64, u32, u32) {
        ((r[0] * w as f64).round() as i64, (r[1] * h as f64).round() as i64, (r[2] * w as f64).round().max(1.0) as u32, (r[3] * h as f64).round().max(1.0) as u32)
    };
    for l in layers {
        match l {
            Layer::Model { rect, model, view, hidden } => {
                let (x, y, bw, bh) = px(rect);
                let img = deckcraft_model3d::soft::render(&model, &view, bw, bh, &hidden, None);
                if let Some(over) = image::RgbaImage::from_raw(img.width, img.height, img.rgba) {
                    image::imageops::overlay(&mut canvas, &over, x, y);
                }
            }
            Layer::Picture { rect, img, cover } => {
                let (x, y, bw, bh) = px(rect);
                let fitted = fit(&img, bw, bh, cover);
                let dx = x + (bw as i64 - fitted.width() as i64) / 2;
                let dy = y + (bh as i64 - fitted.height() as i64) / 2;
                image::imageops::overlay(&mut canvas, &fitted, dx, dy);
            }
            Layer::Clipped { rect, clip, img } => {
                let (x, y, bw, bh) = px(rect);
                let fitted = fit(&img, bw, bh, true);
                let (cx, cy, cw, ch) = px(clip);
                let cw = cw.min(fitted.width());
                let ch = ch.min(fitted.height());
                if cw > 0 && ch > 0 {
                    let sub = image::imageops::crop_imm(&fitted, 0, 0, cw, ch).to_image();
                    let _ = (cx, cy, bw, bh);
                    image::imageops::overlay(&mut canvas, &sub, x, y);
                }
            }
        }
    }
    jpeg_of(w, h, canvas.as_raw()).ok()
}

/// A picture scaled into `bw` × `bh`: fitted inside (`contain`) or filling and cropped (`cover`).
fn fit(img: &image::RgbaImage, bw: u32, bh: u32, cover: bool) -> image::RgbaImage {
    let (iw, ih) = (img.width().max(1) as f64, img.height().max(1) as f64);
    let k = if cover { (bw as f64 / iw).max(bh as f64 / ih) } else { (bw as f64 / iw).min(bh as f64 / ih) };
    let (nw, nh) = (((iw * k).round() as u32).max(1), ((ih * k).round() as u32).max(1));
    let scaled = image::imageops::resize(img, nw, nh, image::imageops::FilterType::Triangle);
    if cover {
        let x = (nw.saturating_sub(bw)) / 2;
        let y = (nh.saturating_sub(bh)) / 2;
        image::imageops::crop_imm(&scaled, x, y, bw.min(nw), bh.min(nh)).to_image()
    } else {
        scaled
    }
}

// ---------------------------------------------------------------------------------------------
// The Phones section of the presenter window

fn button(ui: &mut Ui, s: &str) -> egui::Response {
    ui.add(egui::Button::new(egui::RichText::new(s).size(13.0).color(Color32::WHITE)).fill(Color32::from_gray(60)).min_size(vec2(0.0, 26.0)))
}

fn dim(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(12.0).color(Color32::from_gray(170)));
}

/// Paint the join QR code into `rect`, white on dark with a quiet zone. Returns false when the
/// code could not be made.
pub fn paint_qr(painter: &egui::Painter, rect: egui::Rect, url: &str) -> bool {
    let Ok(code) = qrcode::QrCode::with_error_correction_level(url.as_bytes(), qrcode::EcLevel::M) else { return false };
    let n = code.width();
    if n == 0 {
        return false;
    }
    let side = rect.width().min(rect.height());
    let quiet = 4.0;
    let cell = side / (n as f32 + quiet * 2.0);
    let origin = rect.center() - vec2(side / 2.0, side / 2.0);
    painter.rect_filled(egui::Rect::from_min_size(origin, vec2(side, side)), egui::CornerRadius::same(6), Color32::WHITE);
    let colors = code.to_colors();
    for y in 0..n {
        for x in 0..n {
            if colors.get(y * n + x).copied() == Some(qrcode::Color::Dark) {
                let min = origin + vec2((x as f32 + quiet) * cell, (y as f32 + quiet) * cell);
                painter.rect_filled(egui::Rect::from_min_size(min, vec2(cell + 0.5, cell + 0.5)), egui::CornerRadius::ZERO, Color32::BLACK);
            }
        }
    }
    true
}

/// The Phones section: the QR, the address, the connected phones with Grant and Eject, the
/// queue, Test, Refresh and Start hotspot. Returns the height used.
pub fn section(ui: &mut Ui, ext: &mut Ext, rect: egui::Rect) -> f32 {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    let ui = &mut child;
    let top = rect.min.y;
    ui.label(egui::RichText::new("Phones").size(13.0).color(Color32::from_gray(170)));
    let Some(ph) = ext.phones.as_mut() else {
        dim(ui, "The phone server is off.");
        return ui.min_rect().max.y - top;
    };
    match &ph.status {
        Status::Failed(e) => {
            dim(ui, &format!("The phone server did not start: {e}"));
        }
        Status::Off => dim(ui, "The phone server is off."),
        Status::Running => {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(96.0, 96.0), egui::Sense::hover());
                paint_qr(ui.painter(), r, &ph.url);
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(&ph.url).size(12.0).color(Color32::WHITE));
                    let connected = ph.connected();
                    dim(ui, &format!("{connected} connected"));
                    if button(ui, if ext.qr_shown { "Hide QR from audience" } else { "Show QR to audience" }).clicked() {
                        ext.qr_shown = !ext.qr_shown;
                    }
                });
            });
            if ph.addresses.len() > 1 {
                ui.horizontal(|ui| {
                    dim(ui, "Address");
                    let mut chosen = None;
                    egui::ComboBox::from_id_salt("deckrt-phone-address").selected_text(ph.address.clone()).show_ui(ui, |ui| {
                        for a in &ph.addresses {
                            let label = format!("{} ({}{})", a.address, a.interface, if a.hotspot { ", hotspot" } else { "" });
                            if ui.selectable_label(a.address == ph.address, label).clicked() {
                                chosen = Some(a.address.clone());
                            }
                        }
                    });
                    if let Some(a) = chosen {
                        let mut changed = false;
                        ph.refresh(Some(a), &mut changed);
                    }
                });
            }
            ui.horizontal_wrapped(|ui| {
                if button(ui, "Test").on_hover_text("Fetch the phone page from this laptop").clicked() {
                    ext.last_test = Some(ph.self_test());
                }
                if button(ui, "Refresh").on_hover_text("Read the addresses again").clicked() {
                    let mut changed = false;
                    ph.refresh(None, &mut changed);
                }
                if button(ui, "Start hotspot").on_hover_text("Turn on the Windows mobile hotspot when the venue wifi keeps phones apart").clicked() {
                    let msg = ph.start_hotspot();
                    let mut changed = false;
                    ph.refresh(None, &mut changed);
                    ext.last_hotspot = Some(msg);
                }
            });
            if let Some(t) = &ext.last_test {
                match t {
                    Ok(_) => dim(ui, "Test passed: the page answers from this laptop. If no phone can open it, the wifi or the firewall is in the way."),
                    Err(e) => dim(ui, &format!("Test failed: {e}")),
                }
            }
            if let Some(h) = &ext.last_hotspot {
                dim(ui, h);
            }
            if let Some(e) = &ph.slides_error {
                dim(ui, e);
            }
            // Who is connected.
            let roster = ph.roster.clone();
            let names = ph.names.clone();
            let mut grant = None;
            let mut revoke = None;
            let mut eject = None;
            for r in &roster {
                ui.horizontal(|ui| {
                    let name = names.get(&r.id).cloned().unwrap_or_else(|| r.name.clone());
                    let (standing, pos) = ph.queue.standing(&r.id);
                    let line = match (standing, pos) {
                        ("holder", _) => format!("{name} (has control)"),
                        ("queued", Some(p)) => format!("{name} (asked, {p} in line)"),
                        _ => name.clone(),
                    };
                    ui.label(egui::RichText::new(line).size(13.0).color(if r.connected { Color32::WHITE } else { Color32::from_gray(140) }));
                    if standing == "holder" {
                        if button(ui, "Take back").clicked() {
                            revoke = Some(r.id.clone());
                        }
                    } else if button(ui, "Grant").clicked() {
                        grant = Some(r.id.clone());
                    }
                    if button(ui, "Eject").clicked() {
                        eject = Some(r.id.clone());
                    }
                });
            }
            if let Some(id) = grant {
                ph.grant(&id);
            }
            if let Some(id) = revoke {
                ph.revoke(&id, &mut ext.pointer);
            }
            if let Some(id) = eject {
                ph.eject(&id, &mut ext.states, &mut ext.pointer);
            }
            ui.horizontal(|ui| {
                dim(ui, "Holders at once");
                let mut n = ph.queue.max as i32;
                if ui.add(egui::DragValue::new(&mut n).range(1..=20)).changed() {
                    ph.set_max_holders(n.max(1) as usize);
                }
                if !ph.queue.holders.is_empty() && button(ui, "Take all back").clicked() {
                    let ids = ph.queue.holders.clone();
                    for id in ids {
                        ph.revoke(&id, &mut ext.pointer);
                    }
                }
            });
            if let Some(p) = &ph.results_path {
                dim(ui, &format!("Votes in {}", p.display()));
            }
        }
    }
    ui.min_rect().max.y - top
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_queue_grants_displaces_and_reports_standing() {
        let mut q = ControlQueue::new();
        assert!(q.request("a"));
        assert!(!q.request("a"));
        assert!(q.request("b"));
        assert_eq!(q.standing("b"), ("queued", Some(2)));
        assert!(q.grant("a").is_empty());
        assert_eq!(q.standing("a"), ("holder", None));
        assert_eq!(q.grant("b"), vec!["a".to_string()], "one holder at a time: the first is displaced");
        assert_eq!(q.standing("a"), ("none", None));
        assert!(q.revoke("b"));
        assert!(q.holders.is_empty());
        q.set_max(2);
        q.grant("a");
        q.grant("b");
        assert_eq!(q.set_max(1), vec!["a".to_string()]);
    }

    #[test]
    fn duplicate_names_are_numbered() {
        let roster = vec![
            RosterEntry { id: "1".into(), name: "Sam".into(), connected: true },
            RosterEntry { id: "2".into(), name: "sam".into(), connected: true },
            RosterEntry { id: "3".into(), name: "Ana".into(), connected: false },
        ];
        let n = display_names(&roster);
        assert_eq!(n["1"], "Sam");
        assert_eq!(n["2"], "sam (2)");
        assert_eq!(n["3"], "Ana");
    }

    #[test]
    fn a_live_frame_composes_the_model_over_the_slide() {
        let model = Arc::new(deckcraft_model3d::load(&deckcraft_model3d::sample::cube_glb()).expect("cube"));
        let view = deckcraft_model3d::View::framing(&model);
        let jpeg = compose(None, 16.0 / 9.0, vec![Layer::Model { rect: [0.25, 0.25, 0.5, 0.5], model, view, hidden: vec![] }]).expect("frame");
        let img = image::load_from_memory(&jpeg).expect("jpeg").to_rgb8();
        assert_eq!(img.width(), PHONE_WIDTH);
        let c = img.get_pixel(480, 270);
        assert!(c[0] > c[1] + 30 && c[0] > c[2] + 30, "the red cube is in the middle: {c:?}");
        let corner = img.get_pixel(5, 5);
        assert!(corner[0] > 240 && corner[1] > 240, "the slide shows through around it: {corner:?}");
    }

    #[test]
    fn the_qr_paints() {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let (r, _) = ui.allocate_exact_size(vec2(200.0, 200.0), egui::Sense::hover());
                assert!(paint_qr(ui.painter(), r, "http://192.168.1.20:8720/?t=abcdefghij"));
            });
        });
    }
}
