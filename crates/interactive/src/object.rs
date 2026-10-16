//! An interactive object's settings and their JSON form in the shape tag.
//!
//! The tag value is `{"v": 1, "type": "<kind>", "config": {...}, "label": …, "trigger": …,
//! "shortcut": …, "visible": …, "anchor": …}`. Unknown keys are ignored on read so a newer file
//! still opens; missing keys take their defaults.

use deckcraft_model::{MediaId, Shape, ShapeId, ShapeKind};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// The shape tag that holds an object's settings.
pub const TAG: &str = "DECKRT.OBJECT";
/// The slide tag that gives a slide a durable key, so a poll's targets survive reordering.
pub const SLIDE_TAG: &str = "DECKRT.SLIDE";
/// The settings format written today.
pub const VERSION: u32 = 1;

/// The nine object types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Model,
    ImageSwap,
    Wipe,
    Zoom,
    Popup,
    Poll,
    Scrub,
    Measure,
    InjectionMap,
}

impl Kind {
    pub const ALL: [Kind; 9] =
        [Kind::Model, Kind::ImageSwap, Kind::Wipe, Kind::Zoom, Kind::Popup, Kind::Poll, Kind::Scrub, Kind::Measure, Kind::InjectionMap];

    /// The name shown in the ribbon and the pane.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Model => "3D model",
            Kind::ImageSwap => "Image swap",
            Kind::Wipe => "Wipe",
            Kind::Zoom => "Zoom region",
            Kind::Popup => "Popup",
            Kind::Poll => "Poll",
            Kind::Scrub => "Scrub",
            Kind::Measure => "Measure",
            Kind::InjectionMap => "Injection map",
        }
    }

    /// The `type` value in the tag and in the earlier sidecar files.
    pub fn id(self) -> &'static str {
        match self {
            Kind::Model => "model",
            Kind::ImageSwap => "image_swap",
            Kind::Wipe => "wipe",
            Kind::Zoom => "zoom",
            Kind::Popup => "popup",
            Kind::Poll => "poll",
            Kind::Scrub => "scrub",
            Kind::Measure => "measure",
            Kind::InjectionMap => "injection_map",
        }
    }

    pub fn from_id(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.id() == s)
    }
}

/// Who fires the object: the presenter from the presenter view, or anyone who clicks it on the
/// audience screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    #[default]
    Presenter,
    AudienceClick,
}

/// A picture or model among the presentation's media, written as `"media:<id>"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MediaRef(pub MediaId);

impl MediaRef {
    pub fn id(self) -> MediaId {
        self.0
    }
}

impl Serialize for MediaRef {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&format!("media:{}", self.0.0))
    }
}

impl<'de> Deserialize<'de> for MediaRef {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        // A reference the reader could not resolve stays as a missing item (id 0): the object
        // still opens and the pane says the picture is missing.
        let id = s.strip_prefix("media:").and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
        Ok(MediaRef(MediaId(id)))
    }
}

/// The shape the object moves with, and where it sits relative to that shape's top-left corner,
/// in slide points.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub shape: u32,
    pub dx: f64,
    pub dy: f64,
}

impl Anchor {
    pub fn shape_id(&self) -> ShapeId {
        ShapeId(self.shape)
    }
}

/// `[theta_deg, phi_deg, radius]` orbit, the point looked at, and the vertical field of view.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraPose {
    pub orbit: [f64; 3],
    pub target: [f64; 3],
    pub fov_deg: f64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelConfig {
    /// The node names of the `.glb`, in file order.
    pub layers: Vec<String>,
    /// Layers hidden when the slide opens.
    pub hidden_layers: Vec<String>,
    /// Radians about x, y, z applied to the model before the camera looks at it.
    pub initial_rotation: [f64; 3],
    /// The camera when the slide opens; `None` frames the whole model.
    pub camera: Option<CameraPose>,
    /// Models on one slide sharing a non-empty name turn together in the show.
    pub link_group: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SwapConfig {
    pub images: Vec<MediaRef>,
    /// The picture shown when the slide opens.
    pub index: usize,
    /// `advance` shows the next picture on a click; `picker` shows a strip to choose from.
    pub on_click: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WipeConfig {
    pub before: MediaRef,
    pub after: MediaRef,
    /// Divider position when the slide opens, 0 to 1 of the width.
    pub initial_position: f64,
}

impl Default for WipeConfig {
    fn default() -> Self {
        WipeConfig { before: MediaRef(MediaId(0)), after: MediaRef(MediaId(0)), initial_position: 0.5 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ZoomConfig {
    /// The region that fills the slide, `[x, y, w, h]` in slide units (0 to 1).
    pub source_rect: [f64; 4],
    pub max_scale: f64,
    pub duration_ms: u32,
}

impl Default for ZoomConfig {
    fn default() -> Self {
        ZoomConfig { source_rect: [0.25, 0.25, 0.5, 0.5], max_scale: 2.5, duration_ms: 300 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PopupContent {
    Text { text: String },
    Image { media: MediaRef },
    Web { url: String },
}

impl Default for PopupContent {
    fn default() -> Self {
        PopupContent::Text { text: "Type the popup text here.".into() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PopupConfig {
    pub content: PopupContent,
    /// Panel size and position in slide units.
    pub panel_size: [f64; 2],
    pub panel_position: [f64; 2],
}

impl Default for PopupConfig {
    fn default() -> Self {
        PopupConfig { content: PopupContent::default(), panel_size: [0.5, 0.5], panel_position: [0.25, 0.25] }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PollConfig {
    pub question: String,
    /// Two or more.
    pub options: Vec<String>,
    /// Index into `options`; `None` for a poll with no right answer.
    pub correct: Option<usize>,
    pub reveal_correct: bool,
    /// One entry per option: the key ([`SLIDE_TAG`]) of the slide the option leads to, or none.
    pub targets: Vec<Option<String>>,
}

impl Default for PollConfig {
    fn default() -> Self {
        PollConfig {
            question: "Type the question here.".into(),
            options: vec!["Yes".into(), "No".into()],
            correct: None,
            reveal_correct: true,
            targets: vec![],
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScrubConfig {
    /// Two to twelve, in slider order.
    pub images: Vec<MediaRef>,
    /// One per image: `0 U`, `Day 7`.
    pub labels: Vec<String>,
    pub initial_index: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MeasureConfig {
    pub preset: crate::measure::Preset,
    /// `#rrggbb`.
    pub color: String,
    /// Line width in pixels on a 1080-pixel-tall slide.
    pub line_width: f64,
}

impl Default for MeasureConfig {
    fn default() -> Self {
        MeasureConfig { preset: crate::measure::Preset::Thirds, color: "#ffd400".into(), line_width: 3.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Product {
    pub name: String,
    /// `U`, `mL`.
    pub unit: String,
    /// `#rrggbb`.
    pub color: String,
    pub default_dose: f64,
}

impl Default for Product {
    fn default() -> Self {
        Product { name: "Product".into(), unit: "U".into(), color: "#4c8dff".into(), default_dose: 1.0 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InjectionMapConfig {
    pub products: Vec<Product>,
    pub show_total: bool,
}

impl Default for InjectionMapConfig {
    fn default() -> Self {
        InjectionMapConfig {
            products: vec![
                Product { name: "Toxin".into(), unit: "U".into(), color: "#4c8dff".into(), default_dose: 4.0 },
                Product { name: "Filler".into(), unit: "mL".into(), color: "#ff5c8d".into(), default_dose: 0.1 },
            ],
            show_total: true,
        }
    }
}

/// The type-specific settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "config", rename_all = "snake_case")]
pub enum Config {
    Model(ModelConfig),
    ImageSwap(SwapConfig),
    Wipe(WipeConfig),
    Zoom(ZoomConfig),
    Popup(PopupConfig),
    Poll(PollConfig),
    Scrub(ScrubConfig),
    Measure(MeasureConfig),
    InjectionMap(InjectionMapConfig),
}

impl Config {
    pub fn kind(&self) -> Kind {
        match self {
            Config::Model(_) => Kind::Model,
            Config::ImageSwap(_) => Kind::ImageSwap,
            Config::Wipe(_) => Kind::Wipe,
            Config::Zoom(_) => Kind::Zoom,
            Config::Popup(_) => Kind::Popup,
            Config::Poll(_) => Kind::Poll,
            Config::Scrub(_) => Kind::Scrub,
            Config::Measure(_) => Kind::Measure,
            Config::InjectionMap(_) => Kind::InjectionMap,
        }
    }

    pub fn default_for(kind: Kind) -> Config {
        match kind {
            Kind::Model => Config::Model(ModelConfig::default()),
            Kind::ImageSwap => Config::ImageSwap(SwapConfig { on_click: "advance".into(), ..Default::default() }),
            Kind::Wipe => Config::Wipe(WipeConfig::default()),
            Kind::Zoom => Config::Zoom(ZoomConfig::default()),
            Kind::Popup => Config::Popup(PopupConfig::default()),
            Kind::Poll => Config::Poll(PollConfig::default()),
            Kind::Scrub => Config::Scrub(ScrubConfig::default()),
            Kind::Measure => Config::Measure(MeasureConfig::default()),
            Kind::InjectionMap => Config::InjectionMap(InjectionMapConfig::default()),
        }
    }

    /// Every media item the settings name, in order (the model's `.glb` and poster are on the
    /// shape itself, not here).
    pub fn media(&self) -> Vec<MediaId> {
        match self {
            Config::ImageSwap(c) => c.images.iter().map(|m| m.0).collect(),
            Config::Scrub(c) => c.images.iter().map(|m| m.0).collect(),
            Config::Wipe(c) => vec![c.before.0, c.after.0],
            Config::Popup(PopupConfig { content: PopupContent::Image { media }, .. }) => vec![media.0],
            _ => vec![],
        }
    }
}

/// One interactive object: its type-specific settings and what every type shares.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InteractiveObject {
    #[serde(default = "version")]
    pub v: u32,
    #[serde(flatten)]
    pub config: Config,
    /// The name shown in the pane and the presenter view; empty means the type's name.
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub trigger: Trigger,
    /// One key that fires the object in the show.
    #[serde(default)]
    pub shortcut: Option<char>,
    /// Whether the object shows when the slide opens.
    #[serde(default = "yes")]
    pub visible: bool,
    #[serde(default)]
    pub anchor: Option<Anchor>,
}

fn version() -> u32 {
    VERSION
}
fn yes() -> bool {
    true
}

impl InteractiveObject {
    pub fn new(config: Config) -> Self {
        let trigger = if config.kind() == Kind::Measure { Trigger::AudienceClick } else { Trigger::Presenter };
        InteractiveObject { v: VERSION, config, label: String::new(), trigger, shortcut: None, visible: true, anchor: None }
    }

    pub fn kind(&self) -> Kind {
        self.config.kind()
    }

    /// The label, or the type's name when none was given.
    pub fn display_label(&self) -> String {
        if self.label.trim().is_empty() { self.kind().label().to_string() } else { self.label.clone() }
    }

    /// The tag value.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn from_json(s: &str) -> Option<Self> {
        let mut o: InteractiveObject = serde_json::from_str(s).ok()?;
        o.sanitize();
        Some(o)
    }

    /// Values read from a file are hostile: keep every number where the show can use it.
    pub fn sanitize(&mut self) {
        let finite = |v: f64, d: f64| if v.is_finite() { v } else { d };
        match &mut self.config {
            Config::Model(c) => {
                c.layers.truncate(10_000);
                c.hidden_layers.retain(|h| c.layers.contains(h));
                for r in &mut c.initial_rotation {
                    *r = finite(*r, 0.0);
                }
                if let Some(cam) = &mut c.camera {
                    let ok = cam.orbit.iter().chain(cam.target.iter()).all(|v| v.is_finite()) && cam.fov_deg.is_finite() && cam.orbit[2] > 0.0;
                    if !ok {
                        c.camera = None;
                    }
                }
            }
            Config::ImageSwap(c) => {
                c.images.truncate(1000);
                c.index = c.index.min(c.images.len().saturating_sub(1));
                if c.on_click != "picker" {
                    c.on_click = "advance".into();
                }
            }
            Config::Wipe(c) => c.initial_position = finite(c.initial_position, 0.5).clamp(0.0, 1.0),
            Config::Zoom(c) => {
                let r = &mut c.source_rect;
                r[0] = finite(r[0], 0.0).clamp(0.0, 1.0);
                r[1] = finite(r[1], 0.0).clamp(0.0, 1.0);
                r[2] = finite(r[2], 0.5).clamp(0.01, 1.0);
                r[3] = finite(r[3], 0.5).clamp(0.01, 1.0);
                c.max_scale = finite(c.max_scale, 2.5).clamp(1.0, 20.0);
                c.duration_ms = c.duration_ms.min(5000);
            }
            Config::Popup(c) => {
                c.panel_size = [finite(c.panel_size[0], 0.5).clamp(0.05, 1.0), finite(c.panel_size[1], 0.5).clamp(0.05, 1.0)];
                c.panel_position = [finite(c.panel_position[0], 0.25).clamp(0.0, 0.95), finite(c.panel_position[1], 0.25).clamp(0.0, 0.95)];
            }
            Config::Poll(c) => {
                c.options.truncate(64);
                while c.options.len() < 2 {
                    c.options.push(String::new());
                }
                if c.correct.is_some_and(|i| i >= c.options.len()) {
                    c.correct = None;
                }
                c.targets.truncate(c.options.len());
            }
            Config::Scrub(c) => {
                c.images.truncate(12);
                c.labels.truncate(c.images.len());
                while c.labels.len() < c.images.len() {
                    c.labels.push(format!("{}", c.labels.len() + 1));
                }
                c.initial_index = c.initial_index.min(c.images.len().saturating_sub(1));
            }
            Config::Measure(c) => {
                c.line_width = finite(c.line_width, 3.0).clamp(1.0, 20.0);
                if !is_color(&c.color) {
                    c.color = "#ffd400".into();
                }
            }
            Config::InjectionMap(c) => {
                c.products.truncate(20);
                for p in &mut c.products {
                    p.default_dose = finite(p.default_dose, 1.0).max(0.0);
                    if !is_color(&p.color) {
                        p.color = "#4c8dff".into();
                    }
                }
            }
        }
        if let Some(a) = &mut self.anchor {
            a.dx = finite(a.dx, 0.0);
            a.dy = finite(a.dy, 0.0);
        }
        self.label.truncate(200);
    }
}

/// `#rrggbb`, case-insensitive.
pub fn is_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// `#rrggbb` → (r, g, b); grey for anything else.
pub fn parse_color(s: &str) -> [u8; 3] {
    if !is_color(s) {
        return [180, 180, 180];
    }
    let v = |i: usize| u8::from_str_radix(s.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
    [v(1), v(3), v(5)]
}

/// The object on `shape`, if it carries one.
pub fn read(shape: &Shape) -> Option<InteractiveObject> {
    shape.tags.iter().find(|(n, _)| n == TAG).and_then(|(_, v)| InteractiveObject::from_json(v))
}

/// Put `obj` on `shape`, replacing any object tag there.
pub fn write(shape: &mut Shape, obj: &InteractiveObject) {
    let json = obj.to_json();
    if let Some(t) = shape.tags.iter_mut().find(|(n, _)| n == TAG) {
        t.1 = json;
    } else {
        shape.tags.push((TAG.to_string(), json));
    }
}

/// Take the object tag off `shape`.
pub fn clear(shape: &mut Shape) {
    shape.tags.retain(|(n, _)| n != TAG);
}

pub fn is_interactive(shape: &Shape) -> bool {
    shape.tags.iter().any(|(n, _)| n == TAG)
}

/// A PowerPoint 3D model that carries no object tag: upstream's native model, drawn by deckrt
/// too but with no layers or camera of its own.
pub fn is_native_model(shape: &Shape) -> bool {
    matches!(shape.kind, ShapeKind::Model3d { .. }) && !is_interactive(shape)
}

/// The slide key a poll target names: the slide's [`SLIDE_TAG`] value.
pub fn slide_key(slide: &deckcraft_model::Slide) -> Option<&str> {
    slide.tags.iter().find(|(n, _)| n == SLIDE_TAG).map(|(_, v)| v.as_str())
}

/// A fresh slide key: eight hex digits from the slide id and a counter, unique within the deck.
pub fn new_slide_key(doc: &deckcraft_model::Presentation, slide: usize) -> String {
    let taken: Vec<&str> = doc.slides.iter().filter_map(|s| slide_key(s)).collect();
    let mut n: u32 = 0;
    loop {
        let key = format!("s{:08x}", (slide as u32).wrapping_mul(2_654_435_761).wrapping_add(n));
        if !taken.contains(&key.as_str()) {
            return key;
        }
        n = n.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_round_trips_through_json() {
        for k in Kind::ALL {
            let mut o = InteractiveObject::new(Config::default_for(k));
            o.label = "Thing".into();
            o.shortcut = Some('m');
            o.anchor = Some(Anchor { shape: 14, dx: 3.5, dy: -2.0 });
            let json = o.to_json();
            assert!(json.contains(&format!("\"type\":\"{}\"", k.id())), "{json}");
            let back = InteractiveObject::from_json(&json).expect("parses");
            assert_eq!(back, o, "{json}");
        }
    }

    #[test]
    fn media_refs_are_strings_and_a_bad_one_reads_as_missing() {
        let c = Config::ImageSwap(SwapConfig { images: vec![MediaRef(MediaId(7)), MediaRef(MediaId(9))], index: 1, on_click: "advance".into() });
        let json = InteractiveObject::new(c).to_json();
        assert!(json.contains("\"media:7\""));
        let back = InteractiveObject::from_json(&json.replace("media:9", "rel:rId4")).expect("parses");
        assert_eq!(back.config.media(), vec![MediaId(7), MediaId(0)]);
    }

    #[test]
    fn tag_read_write_on_a_shape() {
        let mut s = Shape::default();
        assert!(read(&s).is_none());
        let o = InteractiveObject::new(Config::default_for(Kind::Poll));
        write(&mut s, &o);
        assert_eq!(read(&s), Some(o.clone()));
        write(&mut s, &o);
        assert_eq!(s.tags.len(), 1);
        clear(&mut s);
        assert!(!is_interactive(&s));
    }

    #[test]
    fn hostile_numbers_are_kept_in_range() {
        let json = r#"{"v":1,"type":"zoom","config":{"source_rect":[1e300,-5,0,"x"],"max_scale":1e9,"duration_ms":9999999},"shortcut":null}"#;
        let o = InteractiveObject::from_json(json);
        assert!(o.is_none(), "a non-number fails the parse");
        let json = r#"{"v":1,"type":"zoom","config":{"source_rect":[1e300,-5,0,2],"max_scale":1e9,"duration_ms":9999999}}"#;
        let o = InteractiveObject::from_json(json).expect("parses");
        let Config::Zoom(z) = o.config else { panic!("zoom") };
        assert_eq!(z.source_rect, [1.0, 0.0, 0.01, 1.0]);
        assert_eq!(z.max_scale, 20.0);
        assert_eq!(z.duration_ms, 5000);
    }

    #[test]
    fn unknown_keys_and_missing_keys_are_fine() {
        let json = r#"{"type":"poll","config":{"question":"Q","options":["A","B","C"],"correct":5,"future":1},"later":true}"#;
        let o = InteractiveObject::from_json(json).expect("parses");
        assert_eq!(o.v, VERSION);
        assert!(o.visible);
        let Config::Poll(p) = o.config else { panic!("poll") };
        assert_eq!(p.correct, None, "an out-of-range right answer is dropped");
        assert!(p.reveal_correct);
    }

    #[test]
    fn colors() {
        assert_eq!(parse_color("#4c8dff"), [0x4c, 0x8d, 0xff]);
        assert_eq!(parse_color("red"), [180, 180, 180]);
        assert!(!is_color("#12345"));
    }
}
