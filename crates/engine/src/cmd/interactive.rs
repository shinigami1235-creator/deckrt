//! deckrt's interactive objects: a 3D model, an image swap, a wipe, a zoom region, a popup, a
//! poll, a scrub slider, a measurement overlay and an injection map.
//!
//! Each object is an ordinary shape on the slide that carries its settings as JSON in the
//! `DECKRT.OBJECT` tag ([`deckcraft_interactive::object`]). The shape itself is what PowerPoint
//! shows: a native 3D model with a poster picture, the first picture of a swap, scrub or wipe, a
//! popup's label, a poll's question and answers, or an invisible rectangle for a zoom region, a
//! measurement or an injection map. Pictures and models live in the deck's media and are named
//! from the settings as `"media:<id>"`, so the `.pptx` is the only file.

use deckcraft_geom::Xfrm;
use deckcraft_interactive::object::{self as obj, Anchor, Config, InteractiveObject, Kind, MediaRef, PopupContent, SLIDE_TAG};
use deckcraft_model::style::PictureFill;
use deckcraft_model::text::TextBody;
use deckcraft_model::{Geom, MediaId, Shape, ShapeId, ShapeKind, ShapeStyle};
use serde_json::{Value, json};

use super::*;
use crate::{EngineError, Result, Session};

/// The presentation tag that records which sidecar was imported, so it is imported once.
pub const IMPORTED_TAG: &str = "DECKRT.SIDECAR";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "interactive.insert",
            "Interactive Object",
            ["Insert", "Interactive"],
            None,
            "{kind: model|image_swap|wipe|zoom|popup|poll|scrub|measure|injection_map, paths?: [file], path?, data?: base64, name?, files?: [{data, name}], rect?: [x,y,w,h] pt, at?: [x,y] pt (where the user clicked), object?: settings} → {id, object}",
            has_slide,
            insert
        ),
        cmd!(
            "interactive.update",
            "Change Interactive Object",
            [],
            None,
            "{id, object: settings, targetSlides?: [slide index or null per poll answer]} → {id}",
            has_doc,
            update
        ),
        cmd!(
            "interactive.addImage",
            "Add Picture to Interactive Object",
            [],
            None,
            "{id, path? | data: base64, name?, slot?: before|after|popup (default: appended to the list)} → {id, media}",
            has_doc,
            add_image
        ),
        cmd!("interactive.removeImage", "Remove Picture from Interactive Object", [], None, "{id, index}", has_doc, remove_image),
        cmd!(
            "interactive.replaceModel",
            "Replace 3D Model File",
            [],
            None,
            "{id, path? | data: base64, name?} → {id, layers}",
            has_doc,
            replace_model
        ),
        cmd!("interactive.delete", "Delete Interactive Object", [], None, "{id}", has_doc, delete),
        cmd!(query "interactive.list", "Interactive Objects", [], None, "{slide?: index} → [{id, kind, label, rect, object}]", has_doc, list),
        cmd!(
            "interactive.importSidecar",
            "Import deckrt Sidecar",
            [],
            None,
            "{path?: the .deckrt file (default: next to the presentation), force?: bool} → {imported, skipped, text}",
            has_doc,
            import_sidecar
        ),
        cmd!("interactive.poster", "Refresh 3D Model Picture", [], None, "{id}", has_doc, refresh_poster),
    ]
}

/// The object on shape `id` of the current slide.
pub fn object_of(s: &Session, id: ShapeId) -> Result<(Shape, InteractiveObject)> {
    let sh = shape_of(s, id)?;
    let mut o = obj::read_any(&sh).ok_or_else(|| EngineError::Other(format!("shape {id} is not an interactive object")))?;
    // A native PowerPoint 3D model: its layers are the file's node names.
    if obj::is_native_model(&sh)
        && let ShapeKind::Model3d { media, .. } = sh.kind
        && let Config::Model(c) = &mut o.config
        && let Some(m) = s.doc()?.doc.media(media)
    {
        c.layers = deckcraft_interactive::glb::layers(&m.data).unwrap_or_default();
    }
    Ok((sh, o))
}

fn id_of(p: &Value, cmd: &str) -> Result<ShapeId> {
    id_param(p, "id", cmd)?.ok_or_else(|| bad(cmd, "missing `id`"))
}

/// Media bytes for one or several files: `paths`, `files: [{data, name}]`, or `path` / `data`.
fn files_of(p: &Value, cmd: &str) -> Result<Vec<(String, Vec<u8>)>> {
    let mut out = vec![];
    if let Some(paths) = p.get("paths").and_then(Value::as_array) {
        for path in paths.iter().filter_map(Value::as_str).take(64) {
            out.push(insert::media_bytes(&json!({"path": path}), cmd)?);
        }
    }
    if let Some(files) = p.get("files").and_then(Value::as_array) {
        for f in files.iter().take(64) {
            out.push(insert::media_bytes(f, cmd)?);
        }
    }
    if out.is_empty() && (p.get("path").is_some() || p.get("data").is_some()) {
        out.push(insert::media_bytes(p, cmd)?);
    }
    Ok(out)
}

fn is_glb(bytes: &[u8]) -> bool {
    bytes.starts_with(b"glTF")
}

/// Add a picture file to the deck's media; refuses anything that is not a readable picture.
fn add_picture(s: &mut Session, cmd: &str, name: &str, bytes: Vec<u8>) -> Result<MediaId> {
    let ct = insert::content_type(name, &bytes);
    if !ct.starts_with("image/") {
        return Err(bad(cmd, format!("{name} is not a picture")));
    }
    if ct != "image/svg+xml" && deckcraft_render::decode_image(&bytes).is_none() {
        return Err(EngineError::Other(format!("{name}: the picture can't be read")));
    }
    s.edit(|doc, _| doc.add_media(name, ct, bytes).ok_or_else(|| bad(cmd, NO_ID_SPACE)))
}

/// A model file as `.glb`: an `.obj` or `.stl` is converted (an `.obj`'s `.mtl` and textures are
/// read from beside `path`), a `.glb` passes through, anything else is refused with a reason.
fn as_glb(path: Option<&str>, name: String, bytes: Vec<u8>) -> Result<(String, Vec<u8>)> {
    if is_glb(&bytes) {
        return Ok((name, bytes));
    }
    let dir = path.and_then(|p| std::path::Path::new(p).parent().map(|d| d.to_path_buf()));
    let sibling = |rel: &str| -> Option<Vec<u8>> {
        let d = dir.as_ref()?;
        let rel = rel.replace('\\', "/");
        crate::io::read_file(&d.join(rel).to_string_lossy()).ok()
    };
    let glb = deckcraft_model3d::convert::to_glb(&name, &bytes, &sibling)
        .map_err(|e| EngineError::Other(if e.contains(&name) { e } else { format!("{name}: {e}") }))?;
    Ok((deckcraft_model3d::convert::glb_name(&name), glb))
}

fn add_glb(s: &mut Session, cmd: &str, name: &str, bytes: Vec<u8>) -> Result<MediaId> {
    if !is_glb(&bytes) {
        return Err(bad(cmd, format!("{name} is not a .glb model")));
    }
    s.edit(|doc, _| doc.add_media(name, "model/gltf-binary", bytes).ok_or_else(|| bad(cmd, NO_ID_SPACE)))
}

fn image_size(s: &Session, media: MediaId) -> Option<(f64, f64)> {
    let m = s.doc().ok()?.doc.media(media)?;
    image::ImageReader::new(std::io::Cursor::new(m.data.as_ref()))
        .with_guessed_format()
        .ok()
        .and_then(|r| r.into_dimensions().ok())
        .map(|(w, h)| (w as f64, h as f64))
}

/// Where a new object lands (deckrt's rule): over the shape under `at` it takes that shape's
/// box and moves with it; over empty space it is centred on `at` (or the slide centre), a third
/// of the slide wide, kept inside the slide. A second object on exactly the same box steps
/// down and right so both stay visible.
pub fn place(s: &Session, kind: Kind, at: Option<(f64, f64)>, aspect: Option<f64>) -> (Xfrm, Option<Anchor>) {
    let Ok(st) = s.doc() else { return (Xfrm::new(0.0, 0.0, 100.0, 100.0), None) };
    let size = crate::page_size(st);
    let (sw, sh) = (size.width.max(1.0), size.height.max(1.0));
    let (px, py) = at.unwrap_or((sw / 2.0, sh / 2.0));
    let shapes = st.shapes();
    // Topmost shape under the point, groups and lines left out.
    let under = at.and_then(|_| {
        shapes.iter().rev().find(|x| {
            if obj::is_live(x) || x.is_group() || x.is_line() || x.hidden {
                return false;
            }
            let b = xfrm_of(&st.doc, &st.selection, x);
            px >= b.x && px <= b.x + b.w && py >= b.y && py <= b.y + b.h && b.w > 1.0 && b.h > 1.0
        })
    });
    if let Some(u) = under {
        let b = xfrm_of(&st.doc, &st.selection, u);
        return (Xfrm::new(b.x, b.y, b.w, b.h), Some(Anchor { shape: u.id.0, dx: 0.0, dy: 0.0 }));
    }
    let (fw, fh) = match kind {
        Kind::Popup => (0.2, 0.12),
        Kind::Model => (0.3, 0.4),
        _ => (0.34, 0.34),
    };
    let (mut w, mut h) = (sw * fw, sh * fh);
    if let Some(a) = aspect.filter(|a| a.is_finite() && *a > 0.0) {
        h = (w / a).min(sh * 0.6);
        w = h * a;
    }
    let mut x = (px - w / 2.0).clamp(0.0, (sw - w).max(0.0));
    let mut y = (py - h / 2.0).clamp(0.0, (sh - h).max(0.0));
    let taken = |x: f64, y: f64| {
        shapes.iter().any(|o| {
            obj::is_interactive(o)
                && o.xfrm.is_some_and(|b| (b.x - x).abs() < 0.5 && (b.y - y).abs() < 0.5 && (b.w - w).abs() < 0.5 && (b.h - h).abs() < 0.5)
        })
    };
    let mut steps = 0;
    while taken(x, y) && steps < 20 {
        x += sw * 0.03;
        y += sh * 0.03;
        steps += 1;
    }
    (Xfrm::new(x, y, w, h), None)
}

/// The poster picture of a model, drawn with the software rasteriser at the box's aspect.
fn render_poster(s: &mut Session, cmd: &str, glb: MediaId, rect: &Xfrm, c: &obj::ModelConfig) -> Result<Option<MediaId>> {
    let bytes = s.doc()?.doc.media(glb).map(|m| m.data.clone()).ok_or_else(|| bad(cmd, "the model file is missing"))?;
    let aspect = if rect.h > 0.0 { rect.w / rect.h } else { 1.0 };
    let (w, h) = if aspect >= 1.0 { (1024.0, 1024.0 / aspect.max(0.1)) } else { (1024.0 * aspect.max(0.1), 1024.0) };
    let pose = c.camera.map(|p| (p.orbit, p.target, p.fov_deg));
    let png = match deckcraft_model3d::poster_png_with(&bytes, w as u32, h as u32, c.initial_rotation, &c.hidden_layers, pose) {
        Ok(png) => png,
        Err(e) => {
            log::warn!("deckrt: no poster for the model: {e}");
            return Ok(None);
        }
    };
    let id = s.edit(|doc, _| doc.add_media("model-poster.png", "image/png", png).ok_or_else(|| bad(cmd, NO_ID_SPACE)))?;
    Ok(Some(id))
}

/// Make the shape show what PowerPoint will show for the object.
pub fn sync_fallback(sh: &mut Shape, o: &InteractiveObject) {
    match &o.config {
        Config::Model(_) => {}
        Config::ImageSwap(c) => set_picture(sh, c.images.first().map(|m| m.0)),
        Config::Scrub(c) => set_picture(sh, c.images.get(c.initial_index).or(c.images.first()).map(|m| m.0)),
        Config::Wipe(c) => set_picture(sh, Some(c.before.0)),
        Config::Popup(c) => {
            sh.kind = ShapeKind::Shape;
            sh.geom = Geom::preset("roundRect");
            sh.style = Some(ShapeStyle::accent(deckcraft_color::SchemeSlot::Accent1));
            sh.fill = None;
            sh.line = None;
            let label = if o.label.trim().is_empty() {
                match &c.content {
                    PopupContent::Text { .. } => "Popup".to_string(),
                    PopupContent::Image { .. } => "Picture".to_string(),
                    PopupContent::Web { .. } => "Web page".to_string(),
                }
            } else {
                o.label.clone()
            };
            let mut t = TextBody::from_text(&label);
            for p in &mut t.paragraphs {
                p.props.align = Some(deckcraft_model::text::Align::Center);
            }
            sh.text = Some(t);
        }
        Config::Poll(c) => {
            sh.kind = ShapeKind::Shape;
            sh.geom = Geom::preset("rect");
            sh.style = None;
            // The theme's second dark colour, so the box follows the deck's design.
            sh.fill = Some(deckcraft_model::Fill::Solid { color: deckcraft_model::ColorRef::scheme(deckcraft_color::SchemeSlot::Dk2) });
            sh.line = Some(deckcraft_model::Line::default());
            let mut text = c.question.clone();
            match c.kind {
                obj::PollKind::Choice | obj::PollKind::Ranking => {
                    for (i, opt) in c.options.iter().enumerate() {
                        let mark =
                            if c.kind == obj::PollKind::Ranking { format!("{}.", i + 1) } else { format!("{}.", (b'A' + (i % 26) as u8) as char) };
                        text.push('\n');
                        text.push_str(&format!("{mark}  {opt}"));
                    }
                }
                obj::PollKind::Rating => text.push_str(&format!("\nRate from 1 to {}", c.scale.clamp(2, 10))),
                obj::PollKind::WordCloud => text.push_str("\nAnswer with up to three words"),
                obj::PollKind::FreeText => text.push_str("\nAnswer in a few words"),
            }
            let mut t = TextBody::from_text(&text);
            let white = deckcraft_model::Fill::Solid { color: deckcraft_model::ColorRef::scheme(deckcraft_color::SchemeSlot::Lt1) };
            for (i, p) in t.paragraphs.iter_mut().enumerate() {
                for r in &mut p.runs {
                    r.props.fill = Some(white.clone());
                    r.props.bold = Some(i == 0);
                    r.props.size = Some(if i == 0 { 24.0 } else { 18.0 });
                }
            }
            t.body.inset_l = Some(14.0);
            t.body.inset_r = Some(14.0);
            t.body.inset_t = Some(10.0);
            t.body.inset_b = Some(10.0);
            sh.text = Some(t);
        }
        Config::Zoom(_) | Config::Measure(_) | Config::InjectionMap(_) => {
            sh.kind = ShapeKind::Shape;
            sh.geom = Geom::preset("rect");
            sh.style = None;
            sh.fill = Some(deckcraft_model::Fill::None);
            sh.line = Some(deckcraft_model::Line { fill: Some(deckcraft_model::Fill::None), ..Default::default() });
            sh.text = None;
        }
    }
}

fn set_picture(sh: &mut Shape, media: Option<MediaId>) {
    let Some(media) = media else { return };
    match &mut sh.kind {
        ShapeKind::Picture { fill } => fill.media = media,
        _ => sh.kind = ShapeKind::Picture { fill: PictureFill { media, ..Default::default() } },
    }
    sh.geom = Geom::preset("rect");
    sh.style = None;
    sh.fill = None;
    sh.line = None;
    sh.text = None;
}

fn kind_param(p: &Value, cmd: &str) -> Result<Kind> {
    let k = str_param(p, "kind").ok_or_else(|| bad(cmd, "missing `kind`"))?;
    Kind::from_id(k).ok_or_else(|| bad(cmd, format!("unknown kind `{k}`")))
}

fn at_param(p: &Value) -> Option<(f64, f64)> {
    let a = p.get("at")?.as_array()?;
    let f = |i: usize| a.get(i).and_then(Value::as_f64).filter(|v| v.is_finite());
    Some((f(0)?, f(1)?))
}

/// A settings value from the `object` param, with its type fixed to `kind`.
fn object_param(p: &Value, kind: Kind, cmd: &str) -> Result<InteractiveObject> {
    match p.get("object") {
        None | Some(Value::Null) => Ok(InteractiveObject::new(Config::default_for(kind))),
        Some(v) => {
            let mut v = v.clone();
            if let Some(m) = v.as_object_mut() {
                m.entry("type").or_insert_with(|| json!(kind.id()));
                m.entry("config").or_insert_with(|| json!({}));
            }
            let o = InteractiveObject::from_json(&v.to_string()).ok_or_else(|| bad(cmd, "`object` is not a valid settings value"))?;
            if o.kind() != kind {
                return Err(bad(cmd, format!("`object` is a {} but `kind` is {}", o.kind().id(), kind.id())));
            }
            Ok(o)
        }
    }
}

fn insert(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "interactive.insert";
    let kind = kind_param(p, cmd)?;
    let mut o = object_param(p, kind, cmd)?;
    let files = files_of(p, cmd)?;
    let mut glb = None;
    let mut aspect = None;
    match &mut o.config {
        Config::Model(c) => {
            let (name, bytes) = files.into_iter().next().ok_or_else(|| bad(cmd, "a 3D model needs a .glb, .obj or .stl file"))?;
            let path = p.get("paths").and_then(|v| v.get(0)).and_then(Value::as_str).or_else(|| p.get("path").and_then(Value::as_str));
            let (name, bytes) = as_glb(path, name, bytes)?;
            let layers = deckcraft_interactive::glb::layers(&bytes).map_err(|e| EngineError::Other(format!("{name}: {e}")))?;
            let id = add_glb(s, cmd, &name, bytes)?;
            c.layers = layers;
            c.hidden_layers.retain(|h| c.layers.contains(h));
            glb = Some(id);
        }
        Config::ImageSwap(c) => {
            for (name, bytes) in files {
                c.images.push(MediaRef(add_picture(s, cmd, &name, bytes)?));
            }
            if c.images.is_empty() {
                return Err(bad(cmd, "an image swap needs at least one picture"));
            }
            aspect = image_size(s, c.images[0].0).map(|(w, h)| w / h.max(1.0));
        }
        Config::Scrub(c) => {
            for (name, bytes) in files {
                let label = std::path::Path::new(&name).file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                c.images.push(MediaRef(add_picture(s, cmd, &name, bytes)?));
                c.labels.push(label);
            }
            if c.images.len() < 2 || c.images.len() > 12 {
                return Err(bad(cmd, format!("a scrub needs 2 to 12 pictures. You picked {}.", c.images.len())));
            }
            aspect = image_size(s, c.images[0].0).map(|(w, h)| w / h.max(1.0));
        }
        Config::Wipe(c) => {
            if files.len() != 2 {
                return Err(bad(cmd, format!("a wipe needs two pictures, before and after. You picked {}.", files.len())));
            }
            let mut it = files.into_iter();
            let (n1, b1) = it.next().ok_or_else(|| bad(cmd, "no picture"))?;
            let (n2, b2) = it.next().ok_or_else(|| bad(cmd, "no picture"))?;
            c.before = MediaRef(add_picture(s, cmd, &n1, b1)?);
            c.after = MediaRef(add_picture(s, cmd, &n2, b2)?);
            aspect = image_size(s, c.before.0).map(|(w, h)| w / h.max(1.0));
        }
        Config::Popup(c) => {
            if let Some((name, bytes)) = files.into_iter().next() {
                c.content = PopupContent::Image { media: MediaRef(add_picture(s, cmd, &name, bytes)?) };
            }
        }
        Config::Zoom(_) | Config::Poll(_) | Config::Measure(_) | Config::InjectionMap(_) => {}
    }
    let (rect, anchor) = match rect_param(p, "rect") {
        Some(r) => (r, None),
        None => place(s, kind, at_param(p), aspect),
    };
    if o.anchor.is_none() {
        o.anchor = anchor;
    }
    if let Config::Zoom(c) = &mut o.config
        && p.get("object").is_none()
    {
        // A new zoom region magnifies its own box.
        let size = crate::page_size(s.doc()?);
        c.source_rect = [rect.x / size.width.max(1.0), rect.y / size.height.max(1.0), rect.w / size.width.max(1.0), rect.h / size.height.max(1.0)];
    }
    o.sanitize();
    let mut shape = Shape { xfrm: Some(rect), name: kind.label().to_string(), ..Default::default() };
    if let (Some(glb), Config::Model(c)) = (glb, &o.config) {
        let poster = render_poster(s, cmd, glb, &rect, c)?;
        shape.kind = ShapeKind::Model3d { media: glb, poster };
    }
    sync_fallback(&mut shape, &o);
    obj::write(&mut shape, &o);
    let id = insert::add_shape(s, cmd, shape, true)?;
    s.edit(|doc, sel| {
        if let Some(list) = crate::shapes_mut(doc, sel)
            && let Some(sh) = deckcraft_model::find_shape_mut(list, id)
        {
            sh.name = format!("{} {}", kind.label(), id.0);
        }
        Ok(())
    })?;
    Ok(json!({"id": id, "object": serde_json::from_str::<Value>(&o.to_json()).unwrap_or(Value::Null)}))
}

/// Write `o` onto shape `id` and bring the fallback in line (`poster` redraws a model's picture).
fn write_object(s: &mut Session, cmd: &str, id: ShapeId, o: &InteractiveObject, poster: bool) -> Result<()> {
    let sh = shape_of(s, id)?;
    let new_poster = match (&sh.kind, &o.config) {
        (ShapeKind::Model3d { media, .. }, Config::Model(c)) if poster => {
            let rect = sh.xfrm.unwrap_or_else(|| Xfrm::new(0.0, 0.0, 100.0, 100.0));
            render_poster(s, cmd, *media, &rect, c)?
        }
        _ => None,
    };
    let o = o.clone();
    edit_shapes(s, &json!({"id": id}), cmd, move |sh| {
        if let (ShapeKind::Model3d { poster, .. }, Some(np)) = (&mut sh.kind, new_poster) {
            *poster = Some(np);
        }
        sync_fallback(sh, &o);
        obj::write(sh, &o);
        Ok(())
    })?;
    Ok(())
}

fn update(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "interactive.update";
    let id = id_of(p, cmd)?;
    let (sh, old) = object_of(s, id)?;
    let v = p.get("object").ok_or_else(|| bad(cmd, "missing `object`"))?;
    let mut o = InteractiveObject::from_json(&v.to_string()).ok_or_else(|| bad(cmd, "`object` is not a valid settings value"))?;
    if o.kind() != old.kind() {
        return Err(bad(cmd, "an object cannot change type"));
    }
    // Media the settings name must be in the deck (the add commands put them there).
    let known = |m: MediaId| s.doc().is_ok_and(|d| d.doc.media(m).is_some());
    if o.config.media().iter().any(|m| !known(*m)) {
        return Err(bad(cmd, "the settings name a picture the presentation does not have"));
    }
    if let (Config::Model(new), Config::Model(prev)) = (&mut o.config, &old.config)
        && new.layers.is_empty()
    {
        new.layers = prev.layers.clone();
    }
    // A poll's targets by slide index (the pane and the sidecar import write the keys directly).
    if let (Config::Poll(c), Some(targets)) = (&mut o.config, p.get("targetSlides").and_then(Value::as_array)) {
        let mut keys = Vec::with_capacity(targets.len());
        for t in targets {
            keys.push(match t.as_u64() {
                Some(i) => Some(slide_key(s, i as usize)?),
                None => None,
            });
        }
        c.targets = keys;
    }
    o.sanitize();
    let poster = match (&o.config, &old.config) {
        (Config::Model(a), Config::Model(b)) => {
            a.hidden_layers != b.hidden_layers || a.initial_rotation != b.initial_rotation || a.camera != b.camera
        }
        _ => false,
    } && matches!(sh.kind, ShapeKind::Model3d { .. });
    write_object(s, cmd, id, &o, poster)?;
    Ok(json!({"id": id}))
}

fn add_image(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "interactive.addImage";
    let id = id_of(p, cmd)?;
    let (_, mut o) = object_of(s, id)?;
    let (name, bytes) = insert::media_bytes(p, cmd)?;
    let media = add_picture(s, cmd, &name, bytes)?;
    let slot = str_param(p, "slot").unwrap_or("");
    match (&mut o.config, slot) {
        (Config::ImageSwap(c), _) => c.images.push(MediaRef(media)),
        (Config::Scrub(c), _) => {
            if c.images.len() >= 12 {
                return Err(bad(cmd, "a scrub holds at most 12 pictures"));
            }
            c.images.push(MediaRef(media));
            c.labels.push(std::path::Path::new(&name).file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
        }
        (Config::Wipe(c), "after") => c.after = MediaRef(media),
        (Config::Wipe(c), _) => c.before = MediaRef(media),
        (Config::Popup(c), _) => c.content = PopupContent::Image { media: MediaRef(media) },
        _ => return Err(bad(cmd, format!("a {} takes no pictures", o.kind().label().to_lowercase()))),
    }
    o.sanitize();
    write_object(s, cmd, id, &o, false)?;
    Ok(json!({"id": id, "media": media}))
}

fn remove_image(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "interactive.removeImage";
    let id = id_of(p, cmd)?;
    let (_, mut o) = object_of(s, id)?;
    let i = usize_param(p, "index").ok_or_else(|| bad(cmd, "missing `index`"))?;
    match &mut o.config {
        Config::ImageSwap(c) => {
            if c.images.len() <= 1 {
                return Err(bad(cmd, "an image swap keeps at least one picture"));
            }
            if i >= c.images.len() {
                return Err(bad(cmd, "no such picture"));
            }
            c.images.remove(i);
        }
        Config::Scrub(c) => {
            if c.images.len() <= 2 {
                return Err(bad(cmd, "a scrub keeps at least two pictures"));
            }
            if i >= c.images.len() {
                return Err(bad(cmd, "no such picture"));
            }
            c.images.remove(i);
            if i < c.labels.len() {
                c.labels.remove(i);
            }
        }
        _ => return Err(bad(cmd, "only an image swap or a scrub has a picture list")),
    }
    o.sanitize();
    write_object(s, cmd, id, &o, false)?;
    Ok(json!({"id": id}))
}

fn replace_model(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "interactive.replaceModel";
    let id = id_of(p, cmd)?;
    let (sh, mut o) = object_of(s, id)?;
    let Config::Model(c) = &mut o.config else { return Err(bad(cmd, "not a 3D model")) };
    let (name, bytes) = insert::media_bytes(p, cmd)?;
    let (name, bytes) = as_glb(p.get("path").and_then(Value::as_str), name, bytes)?;
    let layers = deckcraft_interactive::glb::layers(&bytes).map_err(|e| EngineError::Other(format!("{name}: {e}")))?;
    let glb = add_glb(s, cmd, &name, bytes)?;
    c.layers = layers.clone();
    c.hidden_layers.retain(|h| c.layers.contains(h));
    c.camera = None;
    let rect = sh.xfrm.unwrap_or_else(|| Xfrm::new(0.0, 0.0, 100.0, 100.0));
    let poster = render_poster(s, cmd, glb, &rect, c)?;
    let o2 = o.clone();
    edit_shapes(s, &json!({"id": id}), cmd, move |sh| {
        sh.kind = ShapeKind::Model3d { media: glb, poster };
        obj::write(sh, &o2);
        Ok(())
    })?;
    Ok(json!({"id": id, "layers": layers}))
}

fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "interactive.delete";
    let id = id_of(p, cmd)?;
    object_of(s, id)?;
    s.execute("edit.delete", &json!({"ids": [id]}))?;
    Ok(json!({"id": id}))
}

fn rect_json(x: &Xfrm) -> Value {
    json!([x.x, x.y, x.w, x.h])
}

fn list(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let shapes: Vec<Shape> = match usize_param(p, "slide") {
        Some(i) => st.doc.slides.get(i).map(|sl| sl.shapes.clone()).ok_or_else(|| bad("interactive.list", "no such slide"))?,
        None => st.shapes().to_vec(),
    };
    let mut out = vec![];
    for sh in &shapes {
        if let Some(o) = obj::read_any(sh) {
            out.push(json!({
                "id": sh.id,
                "kind": o.kind().id(),
                "label": o.display_label(),
                "rect": rect_json(&xfrm_of(&st.doc, &st.selection, sh)),
                "object": serde_json::from_str::<Value>(&o.to_json()).unwrap_or(Value::Null),
            }));
        }
    }
    Ok(Value::Array(out))
}

fn refresh_poster(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "interactive.poster";
    let id = id_of(p, cmd)?;
    let (_, o) = object_of(s, id)?;
    if !matches!(o.config, Config::Model(_)) {
        return Err(bad(cmd, "not a 3D model"));
    }
    write_object(s, cmd, id, &o, true)?;
    Ok(json!({"id": id}))
}

/// The sidecar next to the presentation, when one exists and was not imported before.
#[cfg(not(target_arch = "wasm32"))]
pub fn pending_sidecar(s: &Session) -> Option<String> {
    let st = s.active()?;
    let path = st.path.as_deref()?;
    let sidecar = deckcraft_interactive::sidecar::sidecar_path(std::path::Path::new(path));
    if !sidecar.is_file() {
        return None;
    }
    if st.doc.tags.iter().any(|(n, _)| n == IMPORTED_TAG) {
        return None;
    }
    Some(sidecar.to_string_lossy().to_string())
}

#[cfg(target_arch = "wasm32")]
pub fn pending_sidecar(_s: &Session) -> Option<String> {
    None
}

/// The dialog the UI shows once after a sidecar came in (`params.text` holds the words).
pub const IMPORTED_DIALOG: &str = "deckrt.imported";

/// `file.open` hook: import the deck's old sidecar once, as the earlier deckrt would have, and
/// tell the user what came over.
/// deckrt: after any edit, objects anchored to a shape that moved move with it (and an object
/// moved by hand learns its new offset), on every slide the edit changed.
pub fn follow_anchors(old: &deckcraft_model::Presentation, doc: &mut deckcraft_model::Presentation) {
    for (i, sl) in doc.slides.iter_mut().enumerate() {
        let Some(before) = old.slides.get(i).filter(|o| o.id == sl.id && !std::sync::Arc::ptr_eq(o, sl)) else { continue };
        let anchored = sl.shapes.iter().any(|sh| obj::read(sh).is_some_and(|o| o.anchor.is_some()));
        if !anchored {
            continue;
        }
        let mut shapes = sl.shapes.clone();
        deckcraft_interactive::anchor::follow(&before.shapes, &mut shapes);
        if shapes != sl.shapes {
            std::sync::Arc::make_mut(sl).shapes = shapes;
        }
    }
}

/// deckrt: copied objects anchored to a shape copied with them follow the copy; an anchor to a
/// shape left behind is dropped.
pub fn remap_anchors(shapes: &mut [Shape], map: &std::collections::HashMap<ShapeId, ShapeId>) {
    for sh in shapes.iter_mut() {
        let Some(mut o) = obj::read(sh) else { continue };
        let Some(a) = o.anchor else { continue };
        o.anchor = map.get(&a.shape_id()).map(|n| Anchor { shape: n.0, ..a });
        obj::write(sh, &o);
    }
}

pub fn after_open(s: &mut Session) {
    let Some(path) = pending_sidecar(s) else { return };
    let file_name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "the .deckrt file".into());
    match import_sidecar(s, &json!({"path": path})) {
        Ok(r) => {
            log::info!("deckrt: {}", r["text"].as_str().unwrap_or(""));
            // The objects are part of the opened deck now, not a string of edits to undo one by
            // one. The deck stays unsaved so a save keeps them.
            if let Some(st) = s.active_mut() {
                st.history.undo.clear();
                st.history.redo.clear();
            }
            let words = r["words"].as_str().unwrap_or("").to_string();
            s.ui_requests.push(crate::UiRequest::Dialog { id: IMPORTED_DIALOG.into(), params: json!({"text": words, "file": file_name}) });
        }
        Err(e) => {
            log::warn!("deckrt: the sidecar could not be imported: {e}");
            s.ui_requests.push(crate::UiRequest::Dialog {
                id: IMPORTED_DIALOG.into(),
                params: json!({"text": format!("{file_name} could not be brought in: {e}"), "file": file_name}),
            });
        }
    }
}

fn import_sidecar(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "interactive.importSidecar";
    let st = s.doc()?;
    let deck_path = st.path.clone();
    let sidecar = match str_param(p, "path") {
        Some(x) => std::path::PathBuf::from(x),
        None => deckcraft_interactive::sidecar::sidecar_path(std::path::Path::new(
            deck_path.as_deref().ok_or_else(|| bad(cmd, "the presentation has no file, so there is no sidecar next to it"))?,
        )),
    };
    if !bool_or(p, "force", false) && st.doc.tags.iter().any(|(n, _)| n == IMPORTED_TAG) {
        return Err(EngineError::Other("the sidecar was already imported into this presentation".into()));
    }
    let json = crate::io::read_file(&sidecar.to_string_lossy()).map_err(EngineError::Other)?;
    let sc = deckcraft_interactive::sidecar::parse(&String::from_utf8_lossy(&json)).map_err(EngineError::Other)?;
    let dir = sidecar.parent().map(|d| d.to_path_buf()).unwrap_or_default();
    // The file's own slide ids, which the sidecar names.
    let file_ids: Vec<u32> = match &deck_path {
        Some(dp) => crate::io::read_file(dp).ok().and_then(|b| deckcraft_pptx::file_slide_ids(&b).ok()).unwrap_or_default(),
        None => vec![],
    };
    let file_ids = if file_ids.len() == st.doc.slides.len() { file_ids } else { (0..st.doc.slides.len() as u32).map(|i| 256 + i).collect() };
    let doc = st.doc.clone();
    let mut loaded: Vec<(std::path::PathBuf, MediaId)> = vec![];
    let report = {
        let mut load = |path: &std::path::Path| -> Option<MediaId> {
            if let Some((_, id)) = loaded.iter().find(|(p, _)| p == path) {
                return Some(*id);
            }
            let bytes = crate::io::read_file(&path.to_string_lossy()).ok()?;
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "asset".into());
            let ct = if is_glb(&bytes) { "model/gltf-binary" } else { insert::content_type(&name, &bytes) };
            if !(ct.starts_with("image/") || ct == "model/gltf-binary") {
                return None;
            }
            let id = s.edit(|doc, _| doc.add_media(&name, ct, bytes).ok_or_else(|| bad(cmd, NO_ID_SPACE))).ok()?;
            loaded.push((path.to_path_buf(), id));
            Some(id)
        };
        deckcraft_interactive::sidecar::import(&sc, &dir, &doc, &file_ids, &mut load)
    };
    let text = report.text();
    let words = report.plain_words(&sidecar.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "the .deckrt file".into()));
    let current = s.doc()?.selection.slide;
    let mut imported = vec![];
    for item in &report.objects {
        if s.execute("slide.go", &json!({"index": item.slide})).is_err() {
            continue;
        }
        let mut o = item.obj.clone();
        // Poll targets name file slide ids; give each target slide a durable key instead.
        if let Config::Poll(c) = &mut o.config {
            for t in c.targets.iter_mut() {
                let Some(target) = t.clone() else { continue };
                let Some(i) = deckcraft_interactive::sidecar::target_slide_index(&target, &file_ids) else {
                    *t = None;
                    continue;
                };
                *t = Some(slide_key(s, i)?);
            }
        }
        let mut shape = Shape { xfrm: Some(item.rect), name: o.kind().label().to_string(), ..Default::default() };
        if let (Some(glb), Config::Model(c)) = (item.glb, &o.config) {
            let poster = render_poster(s, cmd, glb, &item.rect, c)?;
            shape.kind = ShapeKind::Model3d { media: glb, poster };
        }
        sync_fallback(&mut shape, &o);
        obj::write(&mut shape, &o);
        let id = insert::add_shape(s, cmd, shape, false)?;
        s.edit(|doc, sel| {
            if let Some(list) = crate::shapes_mut(doc, sel)
                && let Some(sh) = deckcraft_model::find_shape_mut(list, id)
            {
                sh.name = format!("{} {}", o.kind().label(), id.0);
            }
            Ok(())
        })?;
        imported.push(json!({"id": id, "slide": item.slide, "kind": o.kind().id(), "from": item.old_id}));
    }
    let _ = s.execute("slide.go", &json!({"index": current}));
    let stamp = sidecar.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    s.edit(|doc, _| {
        doc.tags.retain(|(n, _)| n != IMPORTED_TAG);
        doc.tags.push((IMPORTED_TAG.to_string(), stamp));
        Ok(())
    })?;
    Ok(json!({
        "imported": imported,
        "skipped": report.skipped.iter().map(|(id, r)| json!({"id": id, "why": format!("{r:?}")})).collect::<Vec<_>>(),
        "text": text,
        "words": words,
    }))
}

/// The durable key of slide `index` (its `DECKRT.SLIDE` tag), made when missing.
pub fn slide_key(s: &mut Session, index: usize) -> Result<String> {
    if let Some(k) = s.doc()?.doc.slides.get(index).and_then(|sl| obj::slide_key(sl).map(String::from)) {
        return Ok(k);
    }
    s.edit(|doc, _| {
        let key = obj::new_slide_key(doc, index);
        let sl = doc.slides.get_mut(index).ok_or_else(|| bad("interactive", "no such slide"))?;
        std::sync::Arc::make_mut(sl).tags.push((SLIDE_TAG.to_string(), key.clone()));
        Ok(key)
    })
}

/// The slide index a key names.
pub fn slide_of_key(doc: &deckcraft_model::Presentation, key: &str) -> Option<usize> {
    doc.slides.iter().position(|sl| obj::slide_key(sl) == Some(key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_interactive::object::Kind;

    fn png(rgb: [u8; 3]) -> String {
        let mut out = std::io::Cursor::new(vec![]);
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(8, 6, image::Rgb(rgb))).write_to(&mut out, image::ImageFormat::Png).expect("png");
        base64_encode(&out.into_inner())
    }

    fn session() -> Session {
        let mut s = Session::with_new();
        let _ = s.execute("slide.new", &json!({"layout": "blank"}));
        s
    }

    fn objects(s: &Session) -> Vec<(ShapeId, InteractiveObject)> {
        s.doc().expect("doc").shapes().iter().filter_map(|sh| obj::read(sh).map(|o| (sh.id, o))).collect()
    }

    #[test]
    fn every_fileless_kind_inserts_undoes_and_redoes() {
        let mut s = session();
        for k in [Kind::Zoom, Kind::Popup, Kind::Poll, Kind::Measure, Kind::InjectionMap] {
            let r = s.execute("interactive.insert", &json!({"kind": k.id(), "at": [100.0, 100.0]})).expect("insert");
            assert_eq!(r["object"]["type"], k.id());
            let id = ShapeId(r["id"].as_u64().expect("id") as u32);
            let sh = s.doc().expect("doc").shape(id).expect("shape").clone();
            assert_eq!(obj::read(&sh).map(|o| o.kind()), Some(k));
            assert!(sh.name.starts_with(k.label()), "{}", sh.name);
            assert_eq!(s.doc().expect("doc").selection.shapes, vec![id]);
        }
        assert_eq!(objects(&s).len(), 5);
        s.execute("edit.undo", &json!({})).expect("undo");
        assert_eq!(objects(&s).len(), 4);
        s.execute("edit.redo", &json!({})).expect("redo");
        assert_eq!(objects(&s).len(), 5);
        // Five objects dropped on the same point step so each stays visible.
        let boxes: Vec<_> = s.doc().expect("doc").shapes().iter().filter_map(|x| x.xfrm).map(|x| (x.x.round(), x.y.round())).collect();
        let mut uniq = boxes.clone();
        uniq.dedup();
        assert_eq!(uniq.len(), boxes.len(), "{boxes:?}");
    }

    #[test]
    fn a_zoom_region_starts_on_its_own_box_and_a_measure_takes_audience_clicks() {
        let mut s = session();
        let r = s.execute("interactive.insert", &json!({"kind": "zoom", "rect": [96.0, 54.0, 192.0, 108.0]})).expect("insert");
        let z = r["object"]["config"]["source_rect"].as_array().expect("rect");
        let v: Vec<f64> = z.iter().filter_map(Value::as_f64).collect();
        assert!((v[0] - 0.1).abs() < 1e-9 && (v[2] - 0.2).abs() < 1e-9, "{v:?}");
        let r = s.execute("interactive.insert", &json!({"kind": "measure"})).expect("insert");
        assert_eq!(r["object"]["trigger"], "audience_click");
    }

    #[test]
    fn an_object_over_a_shape_takes_its_box_and_is_anchored_to_it() {
        let mut s = session();
        let r = s.execute("shape.insert", &json!({"preset": "rect", "rect": [100.0, 100.0, 200.0, 150.0]})).expect("shape");
        let under = r["id"].as_u64().expect("id");
        let r = s.execute("interactive.insert", &json!({"kind": "popup", "at": [150.0, 120.0]})).expect("insert");
        let id = ShapeId(r["id"].as_u64().expect("id") as u32);
        let sh = s.doc().expect("doc").shape(id).expect("shape").clone();
        assert_eq!(sh.xfrm.map(|x| (x.x, x.y, x.w, x.h)), Some((100.0, 100.0, 200.0, 150.0)));
        assert_eq!(obj::read(&sh).and_then(|o| o.anchor).map(|a| a.shape), Some(under as u32));
        // Over empty space: centred on the point, inside the slide.
        let r = s.execute("interactive.insert", &json!({"kind": "popup", "at": [5.0, 5.0]})).expect("insert");
        let id = ShapeId(r["id"].as_u64().expect("id") as u32);
        let x = s.doc().expect("doc").shape(id).expect("shape").xfrm.expect("box");
        assert_eq!((x.x, x.y), (0.0, 0.0));
    }

    #[test]
    fn a_swap_takes_pictures_in_order_and_shows_the_first() {
        let mut s = session();
        assert!(s.execute("interactive.insert", &json!({"kind": "image_swap"})).is_err(), "no pictures");
        let r = s
            .execute(
                "interactive.insert",
                &json!({"kind": "image_swap", "files": [{"data": png([255, 0, 0]), "name": "red.png"}, {"data": png([0, 0, 255]), "name": "blue.png"}]}),
            )
            .expect("insert");
        let id = ShapeId(r["id"].as_u64().expect("id") as u32);
        let sh = s.doc().expect("doc").shape(id).expect("shape").clone();
        let Config::ImageSwap(c) = obj::read(&sh).expect("object").config else { panic!("swap") };
        assert_eq!(c.images.len(), 2);
        let ShapeKind::Picture { fill } = &sh.kind else { panic!("picture fallback") };
        assert_eq!(fill.media, c.images[0].0);
        let x = sh.xfrm.expect("box");
        assert!((x.w / x.h - 8.0 / 6.0).abs() < 1e-6, "the box takes the picture's aspect: {x:?}");
        // Add a third, remove the first: the fallback follows.
        s.execute("interactive.addImage", &json!({"id": id, "data": png([0, 255, 0]), "name": "green.png"})).expect("add");
        s.execute("interactive.removeImage", &json!({"id": id, "index": 0})).expect("remove");
        let sh = s.doc().expect("doc").shape(id).expect("shape").clone();
        let Config::ImageSwap(c) = obj::read(&sh).expect("object").config else { panic!("swap") };
        assert_eq!(c.images.len(), 2);
        let ShapeKind::Picture { fill } = &sh.kind else { panic!("picture fallback") };
        assert_eq!(fill.media, c.images[0].0);
        assert_eq!(s.doc().expect("doc").doc.media(fill.media).map(|m| m.name.as_str()), Some("blue.png"));
        // The media the swap names are counted as referenced.
        let refs = crate::media_refs::slide_media(s.doc().expect("doc").current_slide().expect("slide"));
        assert!(refs.contains(&c.images[1].0));
        // Undo takes the removal back in one step.
        s.execute("edit.undo", &json!({})).expect("undo");
        let sh = s.doc().expect("doc").shape(id).expect("shape").clone();
        let Config::ImageSwap(c) = obj::read(&sh).expect("object").config else { panic!("swap") };
        assert_eq!(c.images.len(), 3);
    }

    #[test]
    fn a_wipe_needs_exactly_two_pictures_and_a_scrub_two_to_twelve() {
        let mut s = session();
        let f = |n: usize| -> Vec<Value> { (0..n).map(|i| json!({"data": png([i as u8, 0, 0]), "name": format!("day-{i:02}.png")})).collect() };
        let e = s.execute("interactive.insert", &json!({"kind": "wipe", "files": f(3)})).expect_err("three is wrong");
        assert!(e.to_string().contains("You picked 3"), "{e}");
        assert_eq!(objects(&s).len(), 0, "nothing was added");
        s.execute("interactive.insert", &json!({"kind": "wipe", "files": f(2)})).expect("two");
        assert!(s.execute("interactive.insert", &json!({"kind": "scrub", "files": f(1)})).is_err());
        assert!(s.execute("interactive.insert", &json!({"kind": "scrub", "files": f(13)})).is_err());
        let r = s.execute("interactive.insert", &json!({"kind": "scrub", "files": f(3)})).expect("three");
        assert_eq!(r["object"]["config"]["labels"], json!(["day-00", "day-01", "day-02"]));
        assert_eq!(objects(&s).len(), 2);
    }

    #[test]
    fn update_keeps_the_type_and_refuses_unknown_media() {
        let mut s = session();
        let r = s.execute("interactive.insert", &json!({"kind": "poll"})).expect("insert");
        let id = r["id"].as_u64().expect("id");
        let mut o = r["object"].clone();
        o["config"]["question"] = json!("Which bone is on the thumb side?");
        o["config"]["options"] = json!(["Ulna", "Radius"]);
        o["config"]["correct"] = json!(1);
        o["shortcut"] = json!("v");
        s.execute("interactive.update", &json!({"id": id, "object": o})).expect("update");
        let sh = s.doc().expect("doc").shape(ShapeId(id as u32)).expect("shape").clone();
        let o = obj::read(&sh).expect("object");
        assert_eq!(o.shortcut, Some('v'));
        let text = sh.text.as_ref().map(|t| t.text()).unwrap_or_default();
        assert!(text.contains("thumb side") && text.contains("B.  Radius"), "{text}");
        let mut wrong = r["object"].clone();
        wrong["type"] = json!("zoom");
        assert!(s.execute("interactive.update", &json!({"id": id, "object": wrong})).is_err());
        let r = s.execute("interactive.insert", &json!({"kind": "image_swap", "files": [{"data": png([1, 2, 3]), "name": "a.png"}]})).expect("swap");
        let mut o = r["object"].clone();
        o["config"]["images"] = json!(["media:9999"]);
        assert!(s.execute("interactive.update", &json!({"id": r["id"], "object": o})).is_err());
        let r = s.execute("interactive.list", &json!({})).expect("list");
        assert_eq!(r.as_array().map(Vec::len), Some(2));
        s.execute("interactive.delete", &json!({"id": id})).expect("delete");
        assert_eq!(objects(&s).len(), 1);
    }

    #[test]
    fn a_polls_targets_can_be_given_by_slide_index() {
        let mut s = session();
        let _ = s.execute("slide.new", &json!({"layout": "blank"}));
        let _ = s.execute("slide.go", &json!({"index": 0}));
        let r = s.execute("interactive.insert", &json!({"kind": "poll"})).expect("insert");
        let id = r["id"].as_u64().expect("id");
        let mut o = r["object"].clone();
        o["config"]["options"] = json!(["Ulna", "Radius"]);
        s.execute("interactive.update", &json!({"id": id, "object": o, "targetSlides": [null, 1]})).expect("update");
        let doc = s.doc().expect("doc");
        let o = obj::read(doc.shape(ShapeId(id as u32)).expect("shape")).expect("object");
        let Config::Poll(c) = &o.config else { panic!("poll") };
        assert_eq!(c.targets.len(), 2);
        assert!(c.targets[0].is_none());
        let key = c.targets[1].clone().expect("a key for slide 2");
        assert_eq!(slide_of_key(&doc.doc, &key), Some(1));
        assert!(s.execute("interactive.update", &json!({"id": id, "object": r["object"], "targetSlides": [7]})).is_err(), "no slide 8");
    }

    #[test]
    fn a_model_inserts_with_its_layers_and_a_poster() {
        let mut s = session();
        let glb = deckcraft_model3d::sample::cube_glb();
        let r = s
            .execute("interactive.insert", &json!({"kind": "model", "data": base64_encode(&glb), "name": "head.glb", "at": [480.0, 270.0]}))
            .expect("insert");
        let id = ShapeId(r["id"].as_u64().expect("id") as u32);
        assert_eq!(r["object"]["config"]["layers"], json!(["body", "cap"]));
        let sh = s.doc().expect("doc").shape(id).expect("shape").clone();
        let ShapeKind::Model3d { media, poster } = sh.kind else { panic!("model") };
        let st = s.doc().expect("doc");
        assert_eq!(st.doc.media(media).map(|m| m.content_type.as_str()), Some("model/gltf-binary"));
        let poster = poster.expect("a poster was drawn");
        assert!(st.doc.media(poster).is_some_and(|m| m.data.starts_with(b"\x89PNG")));
        // Hiding a layer redraws the poster.
        let mut o = r["object"].clone();
        o["config"]["hidden_layers"] = json!(["cap"]);
        s.execute("interactive.update", &json!({"id": id, "object": o})).expect("update");
        let sh = s.doc().expect("doc").shape(id).expect("shape").clone();
        let ShapeKind::Model3d { poster: p2, .. } = sh.kind else { panic!("model") };
        assert_ne!(p2, Some(poster));
        assert!(s.execute("interactive.insert", &json!({"kind": "model", "data": png([0, 0, 0]), "name": "x.glb"})).is_err(), "not a glb");
    }
}
