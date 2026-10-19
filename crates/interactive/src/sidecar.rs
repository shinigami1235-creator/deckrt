//! Import of the earlier deckrt sidecar files (`<deck>.deckrt` next to `<deck>.pptx`, assets in
//! `assets/` beside it). Every object is matched to a slide by the file's slide id, then to its
//! shape by shape id, then by a fingerprint score over the slide's shapes; an object that matches
//! nothing is placed on its slide at the position the sidecar recorded.

use std::path::{Path, PathBuf};

use deckcraft_geom::Xfrm;
use deckcraft_model::{MediaId, Presentation, Shape, ShapeKind};
use serde::Deserialize;
use sha2::Digest;

use crate::measure::Preset;
use crate::object::*;

#[derive(Debug, Clone, Deserialize)]
pub struct Sidecar {
    #[serde(default)]
    pub format_version: u32,
    #[serde(default)]
    pub source: Source,
    #[serde(default)]
    pub objects: Vec<OldObject>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Source {
    pub filename: String,
    pub content_hash: String,
    pub slide_size_emu: [f64; 2],
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct OldFingerprint {
    pub kind: String,
    pub text_hash: Option<String>,
    pub media_hash: Option<String>,
    pub bbox: Option<[f64; 4]>,
    pub z_index: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct OldAnchor {
    pub slide_id: String,
    pub shape_id: Option<String>,
    pub shape_name: Option<String>,
    pub fingerprint: Option<OldFingerprint>,
    pub offset: [f64; 2],
    pub mode: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OldObject {
    #[serde(default)]
    pub id: String,
    pub anchor: OldAnchor,
    #[serde(default = "default_size")]
    pub size: [f64; 2],
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub config: serde_json::Value,
    #[serde(default)]
    pub trigger: String,
    #[serde(default)]
    pub shortcut: Option<String>,
    #[serde(default = "yes")]
    pub visible_by_default: bool,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub unplaced: Option<serde_json::Value>,
}

fn default_size() -> [f64; 2] {
    [0.3, 0.3]
}
fn yes() -> bool {
    true
}

pub fn parse(json: &str) -> Result<Sidecar, String> {
    let s: Sidecar = serde_json::from_str(json).map_err(|e| format!("the sidecar is not readable: {e}"))?;
    if s.format_version != 1 {
        return Err(format!("the sidecar is format version {}; only version 1 is known", s.format_version));
    }
    Ok(s)
}

/// The sidecar path for a deck: `talk.pptx` → `talk.deckrt`.
pub fn sidecar_path(deck: &Path) -> PathBuf {
    deck.with_extension("deckrt")
}

/// How one object found its place.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolution {
    /// Anchored to the slide, or the shape id still exists.
    Direct,
    /// The shape was found by its fingerprint at this score.
    Fingerprint(f64),
    /// No shape matched: placed where the sidecar recorded it, anchored to the slide.
    Placed,
    /// The slide is gone from the deck: skipped.
    SlideRemoved,
    /// The file an object needs is missing: skipped.
    AssetMissing(String),
}

/// One object ready to be created in the deck.
#[derive(Debug, Clone)]
pub struct Imported {
    pub old_id: String,
    pub slide: usize,
    /// Slide points.
    pub rect: Xfrm,
    pub obj: InteractiveObject,
    /// The `.glb` of a model, already added as a media item.
    pub glb: Option<MediaId>,
    pub resolution: Resolution,
}

pub struct Report {
    pub objects: Vec<Imported>,
    pub skipped: Vec<(String, Resolution)>,
}

impl Report {
    /// What the user is told once the deck has opened: `Brought in from talk.deckrt: 3D model on
    /// slide 12, poll on slide 20. Save the presentation to keep them.` Skipped objects get a
    /// second sentence saying why.
    pub fn plain_words(&self, file_name: &str) -> String {
        if self.objects.is_empty() && self.skipped.is_empty() {
            return format!("{file_name} holds no interactive objects.");
        }
        let mut s = String::new();
        if !self.objects.is_empty() {
            let items: Vec<String> = self.objects.iter().map(|o| format!("{} on slide {}", o.obj.kind().words(), o.slide + 1)).collect();
            let (list, more) =
                if items.len() > 8 { (items[..8].join(", "), format!(" and {} more", items.len() - 8)) } else { (items.join(", "), String::new()) };
            s.push_str(&format!(
                "Brought in from {file_name}: {list}{more}. Save the presentation to keep {}.",
                if self.objects.len() == 1 { "it" } else { "them" }
            ));
        }
        if !self.skipped.is_empty() {
            let why: Vec<String> = self
                .skipped
                .iter()
                .map(|(id, r)| match r {
                    Resolution::SlideRemoved => format!("{id}: its slide is not in the deck"),
                    Resolution::AssetMissing(p) => format!("{id}: the file {p} is missing next to the .deckrt"),
                    _ => format!("{id}: its type is unknown"),
                })
                .collect();
            if !s.is_empty() {
                s.push(' ');
            }
            s.push_str(&format!("Not brought in: {}.", why.join("; ")));
        }
        s
    }

    /// `9 interactive objects: 7 reattached, 1 placed from the sidecar's position, 1 skipped.`
    pub fn text(&self) -> String {
        let n = self.objects.len() + self.skipped.len();
        let reattached = self.objects.iter().filter(|o| matches!(o.resolution, Resolution::Direct | Resolution::Fingerprint(_))).count();
        let placed = self.objects.iter().filter(|o| o.resolution == Resolution::Placed).count();
        let mut parts = vec![format!("{reattached} reattached")];
        if placed > 0 {
            parts.push(format!("{placed} placed at the sidecar's position"));
        }
        if !self.skipped.is_empty() {
            let why: Vec<String> = self
                .skipped
                .iter()
                .map(|(id, r)| match r {
                    Resolution::SlideRemoved => format!("{id}: its slide is gone"),
                    Resolution::AssetMissing(p) => format!("{id}: {p} is missing"),
                    _ => id.clone(),
                })
                .collect();
            parts.push(format!("{} skipped ({})", self.skipped.len(), why.join("; ")));
        }
        format!("{n} interactive object{}: {}.", if n == 1 { "" } else { "s" }, parts.join(", "))
    }
}

/// `sha256:<hex>` of text, or none for empty text.
pub fn text_hash(text: &str) -> Option<String> {
    if text.trim().is_empty() {
        return None;
    }
    Some(sha256(text.as_bytes()))
}

pub fn sha256(bytes: &[u8]) -> String {
    let d = sha2::Sha256::digest(bytes);
    format!("sha256:{}", d.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// The kind name the sidecar fingerprints use.
fn kind_name(s: &Shape) -> &'static str {
    match &s.kind {
        ShapeKind::Picture { .. } => "picture",
        ShapeKind::Group { .. } => "group",
        ShapeKind::Table(_) => "table",
        ShapeKind::Chart(_) | ShapeKind::Opaque { .. } | ShapeKind::Model3d { .. } => "graphic_frame",
        ShapeKind::Connector { .. } => "connector",
        _ => "shape",
    }
}

/// The text of a shape as the sidecar hashed it: every run joined, paragraphs separated by a newline.
pub fn shape_text(s: &Shape) -> String {
    let Some(t) = &s.text else { return String::new() };
    t.paragraphs.iter().map(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>()).collect::<Vec<_>>().join("\n")
}

fn media_hash(doc: &Presentation, s: &Shape) -> Option<String> {
    if let ShapeKind::Picture { fill } = &s.kind {
        return doc.media(fill.media).filter(|m| !m.data.is_empty()).map(|m| sha256(&m.data));
    }
    None
}

fn norm_box(doc: &Presentation, x: &Xfrm) -> [f64; 4] {
    let (w, h) = (doc.slide_size.width.max(1e-6), doc.slide_size.height.max(1e-6));
    [x.x / w, x.y / h, x.w / w, x.h / h]
}

fn iou(a: [f64; 4], b: [f64; 4]) -> f64 {
    let iw = ((a[0] + a[2]).min(b[0] + b[2]) - a[0].max(b[0])).max(0.0);
    let ih = ((a[1] + a[3]).min(b[1] + b[3]) - a[1].max(b[1])).max(0.0);
    let inter = iw * ih;
    let union = a[2] * a[3] + b[2] * b[3] - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

const W_KIND: f64 = 0.20;
const W_MEDIA: f64 = 0.30;
const W_TEXT: f64 = 0.20;
const W_BBOX: f64 = 0.20;
const W_NAME: f64 = 0.10;
pub const MATCH_THRESHOLD: f64 = 0.7;

/// How much `candidate` looks like the shape the object was anchored to, 0 to 1.
pub fn score(fp: &OldFingerprint, stored_name: Option<&str>, doc: &Presentation, candidate: &Shape) -> f64 {
    let mut applicable = W_KIND + W_BBOX + W_NAME;
    let mut got = if fp.kind == kind_name(candidate) { W_KIND } else { 0.0 };
    if let Some(h) = &fp.media_hash {
        applicable += W_MEDIA;
        if media_hash(doc, candidate).as_deref() == Some(h.as_str()) {
            got += W_MEDIA;
        }
    }
    if let Some(h) = &fp.text_hash {
        applicable += W_TEXT;
        if text_hash(&shape_text(candidate)).as_deref() == Some(h.as_str()) {
            got += W_TEXT;
        }
    }
    if let (Some(a), Some(x)) = (fp.bbox, candidate.xfrm) {
        got += W_BBOX * iou(a, norm_box(doc, &x));
    }
    if stored_name.is_some_and(|n| n == candidate.name) {
        got += W_NAME;
    }
    got / applicable
}

/// Where an object lands: the slide index for a file slide id, the anchor shape, and the box.
pub struct Place {
    pub slide: usize,
    pub anchor: Option<(deckcraft_model::ShapeId, Xfrm)>,
    pub rect: Xfrm,
    pub resolution: Resolution,
}

pub fn place(o: &OldObject, doc: &Presentation, file_slide_ids: &[u32]) -> Place {
    let (sw, sh) = (doc.slide_size.width, doc.slide_size.height);
    let size = (o.size[0].max(0.02) * sw, o.size[1].max(0.02) * sh);
    let slide = o.anchor.slide_id.parse::<u32>().ok().and_then(|id| file_slide_ids.iter().position(|x| *x == id)).filter(|i| *i < doc.slides.len());
    let Some(slide) = slide else {
        return Place { slide: 0, anchor: None, rect: Xfrm::new(0.0, 0.0, size.0, size.1), resolution: Resolution::SlideRemoved };
    };
    let shapes = &doc.slides[slide].shapes;
    let at_offset = |base: Option<&Xfrm>| -> Xfrm {
        match base {
            Some(b) => Xfrm::new(b.x + o.anchor.offset[0] * sw, b.y + o.anchor.offset[1] * sh, size.0, size.1),
            None => Xfrm::new(o.anchor.offset[0] * sw, o.anchor.offset[1] * sh, size.0, size.1),
        }
    };
    if o.anchor.mode != "shape" {
        return Place { slide, anchor: None, rect: at_offset(None), resolution: Resolution::Direct };
    }
    // Rule 2: shape id.
    let by_id = o.anchor.shape_id.as_deref().and_then(|s| s.parse::<u32>().ok()).and_then(|id| shapes.iter().find(|s| s.id.0 == id));
    if let Some(s) = by_id
        && let Some(x) = s.xfrm
    {
        return Place { slide, anchor: Some((s.id, x)), rect: at_offset(Some(&x)), resolution: Resolution::Direct };
    }
    // Rule 3: fingerprint.
    if let Some(fp) = &o.anchor.fingerprint {
        let mut best: Option<(&Shape, f64)> = None;
        for s in shapes {
            let sc = score(fp, o.anchor.shape_name.as_deref(), doc, s);
            let better = match best {
                None => true,
                Some((bs, b)) => {
                    sc > b + 1e-12
                        || ((sc - b).abs() <= 1e-12
                            && (shapes.iter().position(|x| x.id == s.id).unwrap_or(0) as i64 - fp.z_index as i64).abs()
                                < (shapes.iter().position(|x| x.id == bs.id).unwrap_or(0) as i64 - fp.z_index as i64).abs())
                }
            };
            if better {
                best = Some((s, sc));
            }
        }
        if let Some((s, sc)) = best
            && sc >= MATCH_THRESHOLD
            && let Some(x) = s.xfrm
        {
            return Place { slide, anchor: Some((s.id, x)), rect: at_offset(Some(&x)), resolution: Resolution::Fingerprint(sc) };
        }
        // No match: the recorded box of the old shape, plus the offset, is where it last was.
        if let Some(b) = fp.bbox {
            let old = Xfrm::new(b[0] * sw, b[1] * sh, b[2] * sw, b[3] * sh);
            return Place { slide, anchor: None, rect: at_offset(Some(&old)), resolution: Resolution::Placed };
        }
    }
    Place { slide, anchor: None, rect: at_offset(None), resolution: Resolution::Placed }
}

fn f64s(v: &serde_json::Value, key: &str) -> Vec<f64> {
    v.get(key).and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_f64()).collect()).unwrap_or_default()
}
fn strs(v: &serde_json::Value, key: &str) -> Vec<String> {
    v.get(key).and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default()
}
fn string(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or_default().to_string()
}

/// Turn the old config into the new one. `load` adds a file (relative to the sidecar's folder)
/// to the deck's media and gives its id, or none when the file is missing.
pub fn convert(o: &OldObject, dir: &Path, load: &mut dyn FnMut(&Path) -> Option<MediaId>) -> Result<(Config, Option<MediaId>), Resolution> {
    let c = &o.config;
    let mut load_rel = |rel: &str| -> Result<MediaId, Resolution> {
        // Only files in the sidecar's own folder or below it: no absolute paths, no `..`.
        let inside = std::path::Path::new(rel).components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir));
        if !inside {
            return Err(Resolution::AssetMissing(rel.to_string()));
        }
        let p = dir.join(rel);
        load(&p).ok_or_else(|| Resolution::AssetMissing(rel.to_string()))
    };
    let kind = Kind::from_id(&o.kind).ok_or(Resolution::Placed)?;
    let (config, glb) = match kind {
        Kind::Model => {
            let glb = load_rel(&string(c, "model_path"))?;
            let layers = strs(c, "layers");
            let vis = c.get("layer_visibility").and_then(|v| v.as_object());
            let hidden = layers.iter().filter(|l| vis.and_then(|m| m.get(*l)).and_then(|b| b.as_bool()) == Some(false)).cloned().collect();
            let rot = f64s(c, "initial_rotation");
            let camera = c.get("initial_camera").filter(|v| !v.is_null()).and_then(|v| {
                let orbit = f64s(v, "orbit");
                let target = f64s(v, "target");
                (orbit.len() == 3 && target.len() == 3).then(|| CameraPose {
                    orbit: [orbit[0], orbit[1], orbit[2]],
                    target: [target[0], target[1], target[2]],
                    fov_deg: v.get("fov_deg").and_then(|f| f.as_f64()).unwrap_or(30.0),
                })
            });
            (
                Config::Model(ModelConfig {
                    layers,
                    hidden_layers: hidden,
                    initial_rotation: [rot.first().copied().unwrap_or(0.0), rot.get(1).copied().unwrap_or(0.0), rot.get(2).copied().unwrap_or(0.0)],
                    camera,
                    link_group: string(c, "link_group"),
                }),
                Some(glb),
            )
        }
        Kind::ImageSwap => {
            let mut images = vec![];
            for p in strs(c, "images") {
                images.push(MediaRef(load_rel(&p)?));
            }
            let on_click = if string(c, "on_click") == "picker" { "picker" } else { "advance" };
            (
                Config::ImageSwap(SwapConfig {
                    images,
                    index: c.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize,
                    on_click: on_click.into(),
                }),
                None,
            )
        }
        Kind::Wipe => {
            let before = MediaRef(load_rel(&string(c, "before"))?);
            let after = MediaRef(load_rel(&string(c, "after"))?);
            (Config::Wipe(WipeConfig { before, after, initial_position: c.get("initial_position").and_then(|f| f.as_f64()).unwrap_or(0.5) }), None)
        }
        Kind::Zoom => {
            let r = f64s(c, "source_rect");
            let source_rect = if r.len() == 4 { [r[0], r[1], r[2], r[3]] } else { [o.anchor.offset[0], o.anchor.offset[1], o.size[0], o.size[1]] };
            (
                Config::Zoom(ZoomConfig {
                    source_rect,
                    max_scale: c.get("max_scale").and_then(|f| f.as_f64()).unwrap_or(2.5),
                    duration_ms: c.get("duration_ms").and_then(|f| f.as_u64()).unwrap_or(300) as u32,
                }),
                None,
            )
        }
        Kind::Popup => {
            let content = match c.get("content") {
                Some(v) if string(v, "kind") == "image" => PopupContent::Image { media: MediaRef(load_rel(&string(v, "path"))?) },
                Some(v) if string(v, "kind") == "web" => PopupContent::Web { url: string(v, "url") },
                Some(v) => PopupContent::Text { text: string(v, "text") },
                None => PopupContent::default(),
            };
            let ps = f64s(c, "panel_size");
            let pp = f64s(c, "panel_position");
            (
                Config::Popup(PopupConfig {
                    content,
                    panel_size: if ps.len() == 2 { [ps[0], ps[1]] } else { [0.5, 0.5] },
                    panel_position: if pp.len() == 2 { [pp[0], pp[1]] } else { [0.25, 0.25] },
                }),
                None,
            )
        }
        Kind::Poll => {
            let targets = c
                .get("targets")
                .and_then(|t| t.as_array())
                .map(|a| a.iter().map(|t| t.as_str().map(|s| format!("sldid:{s}"))).collect())
                .unwrap_or_default();
            (
                Config::Poll(PollConfig {
                    question: string(c, "question"),
                    options: strs(c, "options"),
                    correct: c.get("correct").and_then(|v| v.as_u64()).map(|v| v as usize),
                    reveal_correct: c.get("reveal_correct").and_then(|v| v.as_bool()).unwrap_or(true),
                    targets,
                    ..Default::default()
                }),
                None,
            )
        }
        Kind::Scrub => {
            let mut images = vec![];
            for p in strs(c, "images") {
                images.push(MediaRef(load_rel(&p)?));
            }
            (
                Config::Scrub(ScrubConfig {
                    images,
                    labels: strs(c, "labels"),
                    initial_index: c.get("initial_index").and_then(|i| i.as_u64()).unwrap_or(0) as usize,
                }),
                None,
            )
        }
        Kind::Measure => (
            Config::Measure(MeasureConfig {
                preset: Preset::from_id(&string(c, "preset")).unwrap_or(Preset::Thirds),
                color: string(c, "color"),
                line_width: c.get("line_width").and_then(|f| f.as_f64()).unwrap_or(3.0),
            }),
            None,
        ),
        Kind::InjectionMap => {
            let products = c
                .get("products")
                .and_then(|p| p.as_array())
                .map(|a| {
                    a.iter()
                        .map(|p| Product {
                            name: string(p, "name"),
                            unit: string(p, "unit"),
                            color: string(p, "color"),
                            default_dose: p.get("default_dose").and_then(|f| f.as_f64()).unwrap_or(1.0),
                        })
                        .collect()
                })
                .unwrap_or_default();
            (Config::InjectionMap(InjectionMapConfig { products, show_total: c.get("show_total").and_then(|v| v.as_bool()).unwrap_or(true) }), None)
        }
    };
    Ok((config, glb))
}

/// Match and convert every object of `sidecar`. `file_slide_ids` are the `p:sldId` ids of the
/// deck in slide order; `load` adds an asset file to the deck.
pub fn import(sidecar: &Sidecar, dir: &Path, doc: &Presentation, file_slide_ids: &[u32], load: &mut dyn FnMut(&Path) -> Option<MediaId>) -> Report {
    let mut objects = vec![];
    let mut skipped = vec![];
    for o in &sidecar.objects {
        // What a skipped object is called in the report: its label, else its type, else its id.
        let id = match (o.label.as_deref().map(str::trim).filter(|l| !l.is_empty()), crate::object::Kind::from_id(&o.kind)) {
            (Some(l), _) => l.to_string(),
            (None, Some(k)) => k.label().to_string(),
            (None, None) if !o.id.is_empty() => o.id.clone(),
            (None, None) => o.kind.clone(),
        };
        let p = place(o, doc, file_slide_ids);
        if p.resolution == Resolution::SlideRemoved {
            skipped.push((id, p.resolution));
            continue;
        }
        let (config, glb) = match convert(o, dir, load) {
            Ok(x) => x,
            Err(r) => {
                skipped.push((id, r));
                continue;
            }
        };
        let mut obj = InteractiveObject::new(config);
        obj.label = o.label.clone().unwrap_or_default();
        obj.trigger = if o.trigger == "audience_click" { Trigger::AudienceClick } else { Trigger::Presenter };
        obj.shortcut = o.shortcut.as_deref().and_then(|s| s.chars().next());
        obj.visible = o.visible_by_default;
        obj.anchor = p.anchor.map(|(sid, x)| Anchor { shape: sid.0, dx: p.rect.x - x.x, dy: p.rect.y - x.y });
        obj.sanitize();
        objects.push(Imported { old_id: id, slide: p.slide, rect: p.rect, obj, glb, resolution: p.resolution });
    }
    // Poll targets name file slide ids (`sldid:<id>`): the caller turns those into slide keys
    // once it knows which slide each id is.
    Report { objects, skipped }
}

/// The slide index a converted poll target (`sldid:<id>`) points at.
pub fn target_slide_index(target: &str, file_slide_ids: &[u32]) -> Option<usize> {
    let id = target.strip_prefix("sldid:")?.parse::<u32>().ok()?;
    file_slide_ids.iter().position(|x| *x == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_model::text::TextBody;
    use deckcraft_model::{ShapeId, Slide};
    use std::sync::Arc;

    fn deck() -> Presentation {
        let mut d = Presentation::default();
        d.slides.clear();
        d.slide_size = deckcraft_geom::Size::new(960.0, 540.0);
        for i in 0..3 {
            let mut s = Slide::default();
            let mut sh =
                Shape { id: ShapeId(3), name: format!("Picture {i}"), xfrm: Some(Xfrm::new(96.0, 108.0, 288.0, 270.0)), ..Default::default() };
            sh.text = Some(TextBody::from_text("hello"));
            s.shapes.push(sh);
            d.slides.push(Arc::new(s));
        }
        d
    }

    const SIDECAR: &str = r##"{
      "format_version": 1,
      "source": {"filename": "t.pptx", "content_hash": "sha256:00", "slide_size_emu": [12192000, 6858000], "imported_at": "x"},
      "objects": [
        {"id": "a", "type": "zoom", "anchor": {"slide_id": "257", "shape_id": "3", "shape_name": "Picture 1", "fingerprint": null, "offset": [0.01, 0.02], "mode": "shape"}, "size": [0.3, 0.5],
         "config": {"source_rect": [0.1, 0.2, 0.3, 0.5], "max_scale": 2.5, "duration_ms": 300}, "trigger": "presenter", "shortcut": "z", "visible_by_default": true, "label": "Z"},
        {"id": "b", "type": "poll", "anchor": {"slide_id": "258", "shape_id": null, "shape_name": null, "fingerprint": null, "offset": [0.5, 0.2], "mode": "slide"}, "size": [0.4, 0.4],
         "config": {"question": "Q", "options": ["A", "B"], "correct": 1, "targets": ["256", null]}, "trigger": "presenter", "shortcut": null, "visible_by_default": true},
        {"id": "c", "type": "measure", "anchor": {"slide_id": "999", "shape_id": null, "shape_name": null, "fingerprint": null, "offset": [0.5, 0.2], "mode": "slide"}, "size": [0.4, 0.4],
         "config": {"preset": "angle", "color": "#ff0000", "line_width": 2}, "trigger": "audience_click", "shortcut": null, "visible_by_default": true},
        {"id": "d", "type": "popup", "anchor": {"slide_id": "256", "shape_id": "77", "shape_name": "Picture 0", "fingerprint": {"kind": "shape", "text_hash": null, "media_hash": null, "bbox": [0.1, 0.2, 0.3, 0.5], "z_index": 0}, "offset": [0.0, 0.0], "mode": "shape"}, "size": [0.2, 0.1],
         "config": {"content": {"kind": "image", "path": "assets/missing.png"}, "panel_size": [0.5, 0.5], "panel_position": [0.4, 0.3]}, "trigger": "presenter", "shortcut": "p", "visible_by_default": false}
      ]}"##;

    #[test]
    fn objects_find_their_slides_shapes_and_fallbacks() {
        let sc = parse(SIDECAR).expect("parses");
        let doc = deck();
        let mut loads = 0;
        let r = import(&sc, Path::new("/nowhere"), &doc, &[256, 257, 258], &mut |_| {
            loads += 1;
            None
        });
        assert_eq!(r.objects.len(), 2, "{}", r.text());
        let a = &r.objects[0];
        assert_eq!((a.slide, a.resolution.clone()), (1, Resolution::Direct));
        let an = a.obj.anchor.expect("anchored");
        assert!(an.shape == 3 && (an.dx - 9.6).abs() < 1e-9 && (an.dy - 10.8).abs() < 1e-9, "{an:?}");
        assert!((a.rect.x - 105.6).abs() < 1e-9 && (a.rect.w - 288.0).abs() < 1e-9);
        assert_eq!(a.obj.shortcut, Some('z'));
        let b = &r.objects[1];
        assert_eq!((b.slide, b.obj.anchor), (2, None));
        let Config::Poll(p) = &b.obj.config else { panic!("poll") };
        assert_eq!(p.targets, vec![Some("sldid:256".to_string()), None]);
        assert_eq!(target_slide_index("sldid:256", &[256, 257, 258]), Some(0));
        assert_eq!(r.skipped.len(), 2);
        assert_eq!(r.skipped[0].1, Resolution::SlideRemoved);
        assert_eq!(r.skipped[1].1, Resolution::AssetMissing("assets/missing.png".into()));
        assert_eq!(loads, 1);
        assert!(r.text().starts_with("4 interactive objects: 2 reattached, 2 skipped ("), "{}", r.text());
        let words = r.plain_words("t.deckrt");
        assert_eq!(
            words,
            "Brought in from t.deckrt: zoom region on slide 2, poll on slide 3. Save the presentation to keep them. Not brought in: Measure: its slide is not in the deck; Popup: the file assets/missing.png is missing next to the .deckrt."
        );
    }

    #[test]
    fn a_fingerprint_finds_a_renumbered_shape() {
        let doc = deck();
        let fp =
            OldFingerprint { kind: "shape".into(), text_hash: text_hash("hello"), media_hash: None, bbox: Some([0.1, 0.2, 0.3, 0.5]), z_index: 0 };
        let o = OldObject {
            id: "x".into(),
            anchor: OldAnchor {
                slide_id: "256".into(),
                shape_id: Some("77".into()),
                shape_name: Some("Picture 0".into()),
                fingerprint: Some(fp),
                offset: [0.0, 0.0],
                mode: "shape".into(),
            },
            size: [0.3, 0.5],
            kind: "zoom".into(),
            config: serde_json::json!({}),
            trigger: String::new(),
            shortcut: None,
            visible_by_default: true,
            label: None,
            unplaced: None,
        };
        let p = place(&o, &doc, &[256, 257, 258]);
        assert!(matches!(p.resolution, Resolution::Fingerprint(s) if s > 0.99), "{:?}", p.resolution);
        assert_eq!(p.anchor.map(|a| a.0), Some(ShapeId(3)));
    }

    #[test]
    fn the_sidecar_path_sits_next_to_the_deck() {
        assert_eq!(sidecar_path(Path::new("/a/b/talk.pptx")), PathBuf::from("/a/b/talk.deckrt"));
    }
}
