//! deckrt: every interactive object type survives a save as `.pptx` and as `.deckcraft`, and
//! the earlier sidecar format migrates into the deck.

use std::io::Cursor;

use deckcraft_engine::Session;
use deckcraft_interactive::object::{self as obj, Config, InteractiveObject, Kind};
use deckcraft_model::{Presentation, ShapeKind};
use serde_json::{Value, json};

fn b64(bytes: &[u8]) -> String {
    deckcraft_engine::cmd::base64_encode(bytes)
}

fn png(rgb: [u8; 3]) -> String {
    let mut out = Cursor::new(vec![]);
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(8, 6, image::Rgb(rgb))).write_to(&mut out, image::ImageFormat::Png).expect("png");
    b64(&out.into_inner())
}

fn files(n: usize) -> Vec<Value> {
    (0..n).map(|i| json!({"data": png([i as u8 * 40, 80, 200]), "name": format!("day-{i:02}.png")})).collect()
}

/// A deck with one object of every kind, each with settings that are not the defaults.
fn deck_with_every_kind() -> Session {
    let mut s = Session::with_new();
    s.execute("slide.new", &json!({"layout": "blank"})).expect("slide");
    s.execute("slide.new", &json!({"layout": "blank"})).expect("slide");
    s.execute("slide.go", &json!({"index": 0})).expect("go");
    let glb = deckcraft_model3d::sample::cube_glb();
    let inserts = [
        json!({"kind": "model", "data": b64(&glb), "name": "head.glb", "at": [200.0, 200.0],
               "object": {"config": {"hidden_layers": ["cap"], "initial_rotation": [0.1, 0.2, 0.3], "link_group": "scans"}, "shortcut": "m", "label": "Head"}}),
        json!({"kind": "image_swap", "files": files(3), "object": {"config": {"index": 1}, "shortcut": "s"}}),
        json!({"kind": "wipe", "files": files(2), "object": {"config": {"initial_position": 0.3}}}),
        json!({"kind": "zoom", "rect": [100.0, 100.0, 200.0, 100.0], "object": {"config": {"max_scale": 3.0}, "visible": false}}),
        json!({"kind": "popup", "object": {"config": {"content": {"kind": "text", "text": "Dermis, 1 to 4 mm thick."}, "panel_size": [0.4, 0.3]}, "shortcut": "p"}}),
        json!({"kind": "poll", "object": {"config": {"question": "Which cell makes collagen?", "options": ["Fibroblast", "Keratinocyte", "Melanocyte"], "correct": 0, "targets": [null, null, null]}, "shortcut": "v"}}),
        json!({"kind": "scrub", "files": files(4), "object": {"config": {"initial_index": 2}}}),
        json!({"kind": "measure", "object": {"config": {"preset": "angle", "color": "#ff0000", "line_width": 4.0}}}),
        json!({"kind": "injection_map", "object": {"config": {"products": [{"name": "Toxin", "unit": "U", "color": "#4c8dff", "default_dose": 2.5}], "show_total": false}, "shortcut": "i"}}),
    ];
    for p in inserts {
        s.execute("interactive.insert", &p).expect("insert");
    }
    s
}

fn objects(doc: &Presentation) -> Vec<(String, InteractiveObject, ShapeKind)> {
    let mut out: Vec<_> =
        doc.slides.iter().flat_map(|sl| sl.shapes.iter()).filter_map(|sh| obj::read(sh).map(|o| (sh.name.clone(), o, sh.kind.clone()))).collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The objects of the deck as saved in `format` and opened again, with the media they name.
fn after_round_trip(s: &Session, format: &str) -> Presentation {
    let doc = &s.doc().expect("doc").doc;
    let bytes = deckcraft_engine::cmd::file::save_bytes(doc, format).expect("save");
    match format {
        "pptx" => deckcraft_pptx::import(&bytes).expect("import"),
        _ => deckcraft_format::load(&bytes).expect("load"),
    }
}

fn check_round_trip(format: &str) {
    let s = deck_with_every_kind();
    let before = s.doc().expect("doc").doc.clone();
    let orig = objects(&before);
    assert_eq!(orig.len(), 9);
    let back = after_round_trip(&s, format);
    let got = objects(&back);
    assert_eq!(got.len(), 9, "{format}: every object is still an object");
    for ((name, o, kind), (name2, o2, kind2)) in orig.iter().zip(got.iter()) {
        assert_eq!(name, name2, "{format}");
        // Media ids may be renumbered on the way through a .pptx; compare the bytes they name.
        let media_of = |doc: &Presentation, o: &InteractiveObject| -> Vec<Vec<u8>> {
            o.config.media().iter().map(|m| doc.media(*m).map(|m| m.data.to_vec()).unwrap_or_default()).collect()
        };
        assert_eq!(media_of(&before, o), media_of(&back, o2), "{format} {name}: the pictures came back");
        assert!(media_of(&back, o2).iter().all(|b| !b.is_empty()), "{format} {name}: no picture is empty");
        let strip = |o: &InteractiveObject| {
            let mut v: Value = serde_json::from_str(&o.to_json()).expect("json");
            // Media tokens are compared above.
            if let Some(c) = v.get_mut("config").and_then(Value::as_object_mut) {
                for k in ["images", "before", "after"] {
                    c.remove(k);
                }
                if let Some(ct) = c.get_mut("content").and_then(Value::as_object_mut) {
                    ct.remove("media");
                }
            }
            v
        };
        assert_eq!(strip(o), strip(o2), "{format} {name}: the settings are the same");
        match (kind, kind2) {
            (ShapeKind::Model3d { media, poster }, ShapeKind::Model3d { media: m2, poster: p2 }) => {
                assert_eq!(before.media(*media).map(|m| m.data.to_vec()), back.media(*m2).map(|m| m.data.to_vec()), "{format}: the .glb");
                assert!(poster.is_some() && p2.is_some(), "{format}: the model keeps its poster picture");
            }
            (ShapeKind::Picture { fill }, ShapeKind::Picture { fill: f2 }) => {
                assert_eq!(before.media(fill.media).map(|m| m.data.to_vec()), back.media(f2.media).map(|m| m.data.to_vec()), "{format} {name}");
            }
            (a, b) => assert_eq!(std::mem::discriminant(a), std::mem::discriminant(b), "{format} {name}: {a:?} vs {b:?}"),
        }
    }
}

#[test]
fn every_kind_survives_a_pptx_round_trip() {
    check_round_trip("pptx");
}

#[test]
fn every_kind_survives_a_deckcraft_round_trip() {
    check_round_trip("deckcraft");
}

#[test]
fn the_pptx_fallbacks_are_what_another_app_would_show() {
    let s = deck_with_every_kind();
    let back = after_round_trip(&s, "pptx");
    for (name, o, kind) in objects(&back) {
        match (&o.config, &kind) {
            (Config::ImageSwap(c), ShapeKind::Picture { fill }) => assert_eq!(fill.media, c.images[0].0, "{name}: the first picture"),
            (Config::Scrub(c), ShapeKind::Picture { fill }) => assert_eq!(fill.media, c.images[c.initial_index].0, "{name}: the starting picture"),
            (Config::Wipe(c), ShapeKind::Picture { fill }) => assert_eq!(fill.media, c.before.0, "{name}: the before picture"),
            (Config::Poll(c), ShapeKind::Shape) => {
                let text = back.slides.iter().flat_map(|sl| sl.shapes.iter()).find(|sh| sh.name == name).and_then(|sh| sh.text.as_ref()).map(|t| t.text());
                assert!(text.as_deref().is_some_and(|t| t.contains(&c.question) && t.contains("Keratinocyte")), "{name}: {text:?}");
            }
            (Config::Model(_), ShapeKind::Model3d { .. }) => {}
            (Config::Popup(_), ShapeKind::Shape) => {}
            (Config::Zoom(_) | Config::Measure(_) | Config::InjectionMap(_), ShapeKind::Shape) => {}
            (c, k) => panic!("{name}: {} on a {k:?}", c.kind().id()),
        }
    }
}

/// The lecture deck next to its old sidecar: `DECKRT_TEST_DECK` names it, else the copy under
/// `testdecks/` beside the repository. The test returns when neither exists.
fn ecm_deck() -> Option<std::path::PathBuf> {
    let p = std::env::var_os("DECKRT_TEST_DECK")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../testdecks/ecm and fibroblasts.pptx"));
    if p.is_file() && p.with_extension("deckrt").is_file() { Some(p) } else { None }
}

#[test]
fn the_ecm_sidecar_migrates_its_model_into_the_deck() {
    let Some(path) = ecm_deck() else {
        eprintln!("no lecture deck with a sidecar: skipped");
        return;
    };
    let sidecar = std::fs::read_to_string(path.with_extension("deckrt")).expect("sidecar");
    let bytes_before = std::fs::read(path.with_extension("deckrt")).expect("sidecar");
    let mut s = Session::new();
    s.execute("file.open", &json!({"path": path.to_string_lossy()})).expect("open");
    let doc = s.doc().expect("doc").doc.clone();
    assert_eq!(doc.slides.len(), 70);
    let models: Vec<_> = objects(&doc).into_iter().filter(|(_, o, _)| o.kind() == Kind::Model).collect();
    assert_eq!(models.len(), 1, "the one model came over");
    let (_, o, kind) = &models[0];
    let ShapeKind::Model3d { media, poster } = kind else { panic!("a native 3-D model: {kind:?}") };
    let glb = doc.media(*media).expect("the .glb is in the deck");
    assert_eq!(glb.content_type, "model/gltf-binary");
    assert!(glb.data.len() > 1_000_000, "frans.glb is a real model: {} bytes", glb.data.len());
    assert!(poster.is_some_and(|p| doc.media(p).is_some_and(|m| m.data.starts_with(b"\x89PNG"))), "a poster picture was drawn");
    let Config::Model(c) = &o.config else { panic!("model") };
    assert!(!c.layers.is_empty(), "the layer names were read from the file");
    // Anchored to shape 3 of slide 268 in the sidecar: the object sits on that slide.
    let slide = doc.slides.iter().position(|sl| sl.shapes.iter().any(|sh| obj::read(sh).is_some())).expect("slide");
    assert!(sidecar.contains("\"slide_id\": \"268\"") || sidecar.contains("\"slide_id\":\"268\""), "the sidecar names slide 268");
    let ids = deckcraft_pptx::file_slide_ids(&std::fs::read(&path).expect("deck")).expect("ids");
    assert_eq!(Some(slide), ids.iter().position(|id| *id == 268), "the object is on the slide whose file id is 268");
    assert!(o.anchor.is_some(), "anchored to its shape");
    // The deck remembers the import; a second open does not import again.
    assert!(doc.tags.iter().any(|(n, _)| n == deckcraft_engine::cmd::interactive::IMPORTED_TAG));
    assert_eq!(std::fs::read(path.with_extension("deckrt")).expect("sidecar"), bytes_before, "the sidecar on disk is untouched");
    // And it survives a save as .pptx.
    let back = after_round_trip(&s, "pptx");
    let models: Vec<_> = objects(&back).into_iter().filter(|(_, o, _)| o.kind() == Kind::Model).collect();
    assert_eq!(models.len(), 1);
}
