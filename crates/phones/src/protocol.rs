//! What crosses the WebSocket, and the limits on it.
//!
//! Phones send small JSON objects tagged by `t`. The server parses them into
//! `PhoneMessage` and drops anything that does not parse, so the presenter
//! window only ever sees well-formed values. The presenter sends
//! `PresenterMessage`s, which carry an opaque `msg` the server forwards to
//! phones without reading it.

use std::time::Instant;

use serde::{Deserialize, Serialize};

/// Longest text frame a phone may send, in bytes.
pub const PHONE_MAX_MESSAGE: usize = 4096;
/// Longest text frame the presenter may send. A `show` broadcast carries a
/// poll's question and options and the control queue.
pub const PRESENTER_MAX_MESSAGE: usize = 256 * 1024;
/// Longest live frame (the JPEG after its header) the presenter may send.
/// A 960 px composite at quality 0.72 is tens of kilobytes; the cap leaves
/// room for a busy slide and refuses anything that is not a small picture.
pub const LIVE_FRAME_MAX_BYTES: usize = 512 * 1024;
/// The live frame's header: little-endian `u32` slide number, then `u32` version.
pub const LIVE_FRAME_HEADER: usize = 8;
/// Longest name a phone may take, in characters after trimming.
pub const NAME_MAX_CHARS: usize = 24;
/// Phones connected at once. Beyond this a new connection is closed with
/// `CLOSE_FULL`.
pub const DEFAULT_MAX_PHONES: usize = 100;
/// Malformed frames tolerated from one connection before it is closed.
pub const MAX_STRIKES: u32 = 20;
/// Length of the token in the QR URL. Ten characters from a 57-letter
/// alphabet is beyond guessing from inside a room and short enough to type
/// from the screen when a camera will not scan.
pub const TOKEN_CHARS: usize = 10;
/// Phone id shape: what `localStorage` on the phone holds.
pub const ID_MIN: usize = 8;
pub const ID_MAX: usize = 40;

/// WebSocket close codes the server uses, in the application range.
pub const CLOSE_BAD_TOKEN: u16 = 4001;
pub const CLOSE_BAD_ID: u16 = 4002;
pub const CLOSE_EJECTED: u16 = 4003;
pub const CLOSE_FULL: u16 = 4004;
pub const CLOSE_TOO_MANY_BAD_FRAMES: u16 = 4005;
pub const CLOSE_REPLACED: u16 = 4006;
pub const CLOSE_NOT_LOOPBACK: u16 = 4007;

/// A phone's messages. Unknown tags and mistyped fields fail to parse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum PhoneMessage {
    /// Take part under `name`. Sent once per connection, again after a reconnect.
    Join { name: String },
    /// One vote on the poll object `poll`, replacing any earlier vote.
    Vote { poll: String, option: u32 },
    /// Ask for control (`want: true`) or give it up or leave the queue.
    Control { want: bool },
    /// A coalesced rotation step: `dx`, `dy` as fractions of the touch surface since the last step. `at` is the phone's clock, for the latency measurement.
    Drag { dx: f64, dy: f64, at: f64 },
    /// The pointer on the slide in normalised slide units, or off when `x` is absent.
    Touch {
        #[serde(default)]
        x: Option<f64>,
        #[serde(default)]
        y: Option<f64>,
        at: f64,
    },
    /// Keep-alive; answered with `pong` at once.
    Ping { at: f64 },
    /// Points of a stroke on the current slide, in normalised slide
    /// units, coalesced since the last frame. `stroke` is the phone's own
    /// number for the stroke; `done` closes it. Only a phone holding
    /// control is listened to.
    Draw {
        stroke: u32,
        points: Vec<[f64; 2]>,
        #[serde(default)]
        done: bool,
        at: f64,
    },
    /// Remove every stroke passing near this point, in normalised slide units.
    Erase { x: f64, y: f64, at: f64 },
    /// Which of the slide's models this phone's drags turn: an object id, or
    /// `all` for every model at once. Only a phone holding control is
    /// listened to; the presenter checks the id against the slide.
    Target { id: String },
}

/// Points one `draw` frame may carry. A frame arrives up to 60 times a
/// second, so this is far more than a finger produces.
pub const DRAW_MAX_POINTS: usize = 64;

/// Which bucket a message draws from for rate limiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Join,
    Vote,
    Control,
    /// `drag` and `touch`: many per second by design, dropped rather than queued when too many.
    Stream,
    Ping,
}

impl PhoneMessage {
    pub fn class(&self) -> Class {
        match self {
            PhoneMessage::Join { .. } => Class::Join,
            PhoneMessage::Vote { .. } => Class::Vote,
            PhoneMessage::Control { .. } | PhoneMessage::Target { .. } => Class::Control,
            PhoneMessage::Drag { .. } | PhoneMessage::Touch { .. } | PhoneMessage::Draw { .. } | PhoneMessage::Erase { .. } => Class::Stream,
            PhoneMessage::Ping { .. } => Class::Ping,
        }
    }

    /// Values within range. A parse that succeeds can still carry a nonsense number.
    pub fn is_sane(&self) -> bool {
        let finite = |v: f64| v.is_finite();
        match self {
            PhoneMessage::Join { .. } => true,
            PhoneMessage::Vote { poll, option } => !poll.is_empty() && poll.len() <= 64 && *option < 64,
            PhoneMessage::Control { .. } => true,
            PhoneMessage::Target { id } => !id.is_empty() && id.len() <= 64,
            PhoneMessage::Drag { dx, dy, at } => finite(*dx) && finite(*dy) && finite(*at) && dx.abs() <= 4.0 && dy.abs() <= 4.0,
            PhoneMessage::Touch { x, y, at } => {
                finite(*at)
                    && match (x, y) {
                        (None, None) => true,
                        (Some(x), Some(y)) => finite(*x) && finite(*y) && (-0.5..=1.5).contains(x) && (-0.5..=1.5).contains(y),
                        _ => false,
                    }
            }
            PhoneMessage::Ping { at } => finite(*at),
            PhoneMessage::Draw { points, at, .. } => {
                finite(*at)
                    && !points.is_empty()
                    && points.len() <= DRAW_MAX_POINTS
                    && points.iter().all(|p| finite(p[0]) && finite(p[1]) && (-0.5..=1.5).contains(&p[0]) && (-0.5..=1.5).contains(&p[1]))
            }
            PhoneMessage::Erase { x, y, at } => finite(*at) && finite(*x) && finite(*y) && (-0.5..=1.5).contains(x) && (-0.5..=1.5).contains(y),
        }
    }
}

/// Parse one text frame from a phone. `None` for anything the presenter should never see.
pub fn parse_phone(text: &str) -> Option<PhoneMessage> {
    if text.len() > PHONE_MAX_MESSAGE {
        return None;
    }
    let msg: PhoneMessage = serde_json::from_str(text).ok()?;
    msg.is_sane().then_some(msg)
}

/// The presenter window's messages to the server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum PresenterMessage {
    /// Forward `msg` to every connected phone.
    Broadcast { msg: serde_json::Value },
    /// Forward `msg` to one phone.
    Send { to: String, msg: serde_json::Value },
    /// Close a phone's connection and refuse its id for the rest of the show.
    Eject { id: String },
    /// Ask for the roster again.
    Roster,
    /// Where the phone-sized slide images are: a directory on this machine
    /// holding `slide-NNN.jpg`, and how many there are. The server hands
    /// them out at `GET /slide/<n>?t=<token>` by number alone; no request
    /// ever names a file.
    Slides { dir: String, count: u32 },
}

pub fn parse_presenter(text: &str) -> Option<PresenterMessage> {
    if text.len() > PRESENTER_MAX_MESSAGE {
        return None;
    }
    serde_json::from_str(text).ok()
}

/// The presenter window's composite of the slide it is showing, with the
/// objects and the drawing over it, for phones to show in place of the bare
/// raster. Sent as one binary frame: the header, then the JPEG bytes. The
/// version makes each frame's URL immutable, so a phone can cache it for good.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveFrame {
    /// 1-based slide number the frame belongs to.
    pub slide: u32,
    pub version: u32,
    pub jpeg: Vec<u8>,
}

/// Parse one binary frame from the presenter. `None` for anything that is
/// not a header followed by a JPEG within the cap; the bytes are never
/// served unless they parse.
pub fn parse_live_frame(bytes: &[u8]) -> Option<LiveFrame> {
    if bytes.len() < LIVE_FRAME_HEADER + 4 || bytes.len() > LIVE_FRAME_HEADER + LIVE_FRAME_MAX_BYTES {
        return None;
    }
    let slide = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
    let version = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
    let jpeg = &bytes[LIVE_FRAME_HEADER..];
    // The JPEG start-of-image marker: what is served under `image/jpeg` is at least a JPEG.
    if slide == 0 || jpeg[0..3] != [0xff, 0xd8, 0xff] {
        return None;
    }
    Some(LiveFrame { slide, version, jpeg: jpeg.to_vec() })
}

/// One phone in the roster the presenter sees.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RosterEntry {
    pub id: String,
    pub name: String,
    pub connected: bool,
}

/// A name as a stranger typed it, made safe to keep: trimmed, whitespace
/// collapsed, control characters removed, capped at `NAME_MAX_CHARS`.
/// `None` when nothing is left. The result is text; whoever renders it must
/// render it as text.
pub fn clean_name(raw: &str) -> Option<String> {
    let mut out = String::new();
    let mut last_space = true;
    for ch in raw.chars() {
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
            continue;
        }
        if ch.is_control() {
            continue;
        }
        out.push(ch);
        last_space = false;
        if out.chars().count() >= NAME_MAX_CHARS {
            break;
        }
    }
    let trimmed = out.trim().to_string();
    (!trimmed.is_empty()).then_some(trimmed)
}

/// A phone id is what the phone's `localStorage` holds: letters, digits,
/// `-` and `_`, 8 to 40 of them.
pub fn valid_id(id: &str) -> bool {
    (ID_MIN..=ID_MAX).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A token bucket. `capacity` tokens at most, `per_second` added over time.
#[derive(Debug, Clone)]
pub struct Bucket {
    capacity: f64,
    per_second: f64,
    tokens: f64,
    at: Instant,
}

impl Bucket {
    pub fn new(capacity: f64, per_second: f64) -> Self {
        Bucket { capacity, per_second, tokens: capacity, at: Instant::now() }
    }

    pub fn allow(&mut self) -> bool {
        self.allow_at(Instant::now())
    }

    pub fn allow_at(&mut self, now: Instant) -> bool {
        let dt = now.saturating_duration_since(self.at).as_secs_f64();
        self.at = now;
        self.tokens = (self.tokens + dt * self.per_second).min(self.capacity);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// The per-connection limits: how many of each class a phone may send.
#[derive(Debug, Clone)]
pub struct Limits {
    join: Bucket,
    vote: Bucket,
    control: Bucket,
    stream: Bucket,
    ping: Bucket,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            join: Bucket::new(3.0, 0.3),
            vote: Bucket::new(10.0, 2.0),
            control: Bucket::new(5.0, 1.0),
            stream: Bucket::new(240.0, 120.0),
            ping: Bucket::new(10.0, 1.0),
        }
    }
}

impl Limits {
    pub fn allow(&mut self, class: Class) -> bool {
        self.allow_at(class, Instant::now())
    }

    pub fn allow_at(&mut self, class: Class, now: Instant) -> bool {
        match class {
            Class::Join => self.join.allow_at(now),
            Class::Vote => self.vote.allow_at(now),
            Class::Control => self.control.allow_at(now),
            Class::Stream => self.stream.allow_at(now),
            Class::Ping => self.ping.allow_at(now),
        }
    }
}

/// A random token of `len` letters and digits from the OS's random source,
/// with the letters that look alike left out. One is carried in the QR URL
/// and a longer one belongs to the presenter window.
pub fn new_token(len: usize) -> String {
    use rand::TryRngCore;
    const ALPHABET: &[u8] = b"abcdefghijkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut bytes = vec![0u8; len];
    if rand::rngs::OsRng.try_fill_bytes(&mut bytes).is_err() {
        // No OS randomness: fall back to the clock and the address of a local, still unguessable
        // enough for a token that only has to be scanned off a screen.
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0) as u64
            ^ (&bytes as *const _ as u64);
        let mut x = seed | 1;
        for b in bytes.iter_mut() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *b = (x >> 24) as u8;
        }
    }
    bytes.iter().map(|b| ALPHABET[(*b as usize) % ALPHABET.len()] as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn names_are_trimmed_capped_and_never_empty() {
        assert_eq!(clean_name("  Sam  "), Some("Sam".into()));
        assert_eq!(clean_name("Sam\t\nJones"), Some("Sam Jones".into()));
        assert_eq!(clean_name("   "), None);
        assert_eq!(clean_name("\u{0}\u{7}"), None);
        let long = "abcdefghijklmnopqrstuvwxyz0123456789";
        assert_eq!(clean_name(long).unwrap().chars().count(), NAME_MAX_CHARS);
        // Markup stays as text; rendering as text is the page's job.
        assert_eq!(clean_name("<b>Sam</b>"), Some("<b>Sam</b>".into()));
        assert_eq!(clean_name("Zoë 😀"), Some("Zoë 😀".into()));
    }

    #[test]
    fn ids_have_one_shape() {
        assert!(valid_id("p_1234abcd"));
        assert!(valid_id("abcdefgh"));
        assert!(!valid_id("short"));
        assert!(!valid_id("has space here"));
        assert!(!valid_id("<script>alert(1)</script>"));
        assert!(!valid_id(&"x".repeat(41)));
    }

    #[test]
    fn phone_messages_parse_and_reject_nonsense() {
        assert_eq!(parse_phone(r#"{"t":"join","name":"Sam"}"#), Some(PhoneMessage::Join { name: "Sam".into() }));
        assert_eq!(parse_phone(r#"{"t":"vote","poll":"obj_1","option":2}"#), Some(PhoneMessage::Vote { poll: "obj_1".into(), option: 2 }));
        assert_eq!(parse_phone(r#"{"t":"touch","at":1}"#), Some(PhoneMessage::Touch { x: None, y: None, at: 1.0 }));
        assert_eq!(parse_phone(r#"{"t":"vote","poll":"obj_1","option":-1}"#), None);
        assert_eq!(parse_phone(r#"{"t":"vote","poll":"obj_1","option":999}"#), None);
        assert_eq!(parse_phone(r#"{"t":"drag","dx":1e308,"dy":0,"at":0}"#), None);
        assert_eq!(parse_phone(r#"{"t":"touch","x":0.5,"at":0}"#), None, "x without y");
        assert_eq!(
            parse_phone(r#"{"t":"draw","stroke":3,"points":[[0.1,0.2],[0.3,0.4]],"at":5}"#),
            Some(PhoneMessage::Draw { stroke: 3, points: vec![[0.1, 0.2], [0.3, 0.4]], done: false, at: 5.0 })
        );
        assert_eq!(parse_phone(r#"{"t":"draw","stroke":3,"points":[],"at":5}"#), None, "no points");
        assert_eq!(parse_phone(r#"{"t":"draw","stroke":3,"points":[[7,0]],"at":5}"#), None, "off the slide");
        assert_eq!(parse_phone(r#"{"t":"erase","x":0.5,"y":0.5,"at":1}"#), Some(PhoneMessage::Erase { x: 0.5, y: 0.5, at: 1.0 }));
        assert_eq!(parse_phone(r#"{"t":"target","id":"obj_scan"}"#), Some(PhoneMessage::Target { id: "obj_scan".into() }));
        assert_eq!(parse_phone(r#"{"t":"target","id":"all"}"#), Some(PhoneMessage::Target { id: "all".into() }));
        assert_eq!(parse_phone(r#"{"t":"target","id":""}"#), None, "no id");
        assert_eq!(parse_phone(r#"{"t":"explode"}"#), None);
        assert_eq!(parse_phone("not json"), None);
        assert_eq!(parse_phone(""), None);
        assert_eq!(parse_phone(&format!(r#"{{"t":"join","name":"{}"}}"#, "x".repeat(PHONE_MAX_MESSAGE))), None);
    }

    #[test]
    fn presenter_messages_carry_an_opaque_payload() {
        let m = parse_presenter(r#"{"t":"send","to":"p_1","msg":{"t":"you","vote":1}}"#).unwrap();
        assert_eq!(m, PresenterMessage::Send { to: "p_1".into(), msg: serde_json::json!({"t":"you","vote":1}) });
        assert!(matches!(parse_presenter(r#"{"t":"roster"}"#), Some(PresenterMessage::Roster)));
        assert_eq!(
            parse_presenter(r#"{"t":"slides","dir":"/x/phone","count":7}"#),
            Some(PresenterMessage::Slides { dir: "/x/phone".into(), count: 7 })
        );
        assert_eq!(parse_presenter(r#"{"t":"broadcast"}"#), None);
    }

    #[test]
    fn live_frames_need_a_header_a_jpeg_and_a_size_under_the_cap() {
        let mut good = vec![3, 0, 0, 0, 7, 1, 0, 0];
        good.extend_from_slice(&[0xff, 0xd8, 0xff, 0xe0, 1, 2, 3]);
        assert_eq!(parse_live_frame(&good), Some(LiveFrame { slide: 3, version: 263, jpeg: vec![0xff, 0xd8, 0xff, 0xe0, 1, 2, 3] }));
        assert_eq!(parse_live_frame(&good[..8]), None, "header alone");
        assert_eq!(parse_live_frame(&[]), None);
        let mut png = good.clone();
        png[8..12].copy_from_slice(&[0x89, b'P', b'N', b'G']);
        assert_eq!(parse_live_frame(&png), None, "not a JPEG");
        let mut zero = good.clone();
        zero[0..4].copy_from_slice(&[0, 0, 0, 0]);
        assert_eq!(parse_live_frame(&zero), None, "slide 0");
        let mut big = good.clone();
        big.resize(LIVE_FRAME_HEADER + LIVE_FRAME_MAX_BYTES + 1, 0);
        assert_eq!(parse_live_frame(&big), None, "over the cap");
        big.truncate(LIVE_FRAME_HEADER + LIVE_FRAME_MAX_BYTES);
        assert!(parse_live_frame(&big).is_some(), "at the cap");
    }

    #[test]
    fn buckets_refill_over_time() {
        let t0 = Instant::now();
        let mut b = Bucket::new(3.0, 1.0);
        assert!(b.allow_at(t0) && b.allow_at(t0) && b.allow_at(t0));
        assert!(!b.allow_at(t0));
        assert!(!b.allow_at(t0 + Duration::from_millis(500)));
        assert!(b.allow_at(t0 + Duration::from_millis(1500)));
        assert!(!b.allow_at(t0 + Duration::from_millis(1500)));
        // Never more than the capacity, however long the wait.
        let mut b = Bucket::new(2.0, 100.0);
        let later = t0 + Duration::from_secs(60);
        assert!(b.allow_at(later) && b.allow_at(later));
        assert!(!b.allow_at(later));
    }

    #[test]
    fn limits_keep_classes_apart() {
        let t0 = Instant::now();
        let mut l = Limits::default();
        for _ in 0..3 {
            assert!(l.allow_at(Class::Join, t0));
        }
        assert!(!l.allow_at(Class::Join, t0));
        assert!(l.allow_at(Class::Vote, t0), "a join burst does not block a vote");
    }

    #[test]
    fn tokens_are_random_and_url_safe() {
        let a = new_token(10);
        let b = new_token(10);
        assert_eq!(a.len(), 10);
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric()));
    }
}
