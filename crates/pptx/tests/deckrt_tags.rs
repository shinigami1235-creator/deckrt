//! deckrt: media named inside `DECKRT.*` tag values (`"media:<id>"`) become relationships of the
//! tags part on save and media ids again on open, and a 3-D model's tags travel with it.

use std::io::{Cursor, Read};
use std::sync::Arc;

use deckcraft_model::{MediaId, Presentation, Shape, ShapeId, ShapeKind, defaults};

fn png(rgb: [u8; 3]) -> Vec<u8> {
    let mut out = Cursor::new(vec![]);
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(4, 4, image::Rgb(rgb))).write_to(&mut out, image::ImageFormat::Png).expect("png");
    out.into_inner()
}

fn glb(marker: u8) -> Vec<u8> {
    let mut v = b"glTF\x02\x00\x00\x00\x0d\x00\x00\x00".to_vec();
    v.push(marker);
    v
}

fn zip_names(bytes: &[u8]) -> Vec<String> {
    let zip = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).expect("zip");
    (0..zip.len()).filter_map(|i| zip.name_for_index(i).map(String::from)).collect()
}

fn zip_text(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).expect("zip");
    let mut s = String::new();
    zip.by_name(name).expect(name).read_to_string(&mut s).expect("read");
    s
}

fn deck() -> (Presentation, MediaId, MediaId, MediaId) {
    let mut p = defaults::new_presentation(None);
    let red = p.add_media("red.png", "image/png", png([255, 0, 0])).expect("media");
    let blue = p.add_media("blue.png", "image/png", png([0, 0, 255])).expect("media");
    let model = p.add_media("head.glb", "model/gltf-binary", glb(7)).expect("media");
    (p, red, blue, model)
}

#[test]
fn tag_media_survives_a_round_trip_and_the_parts_are_kept() {
    let (mut p, red, blue, model) = deck();
    let slide = Arc::make_mut(&mut p.slides[0]);
    // A picture (the swap's fallback) whose tag names a second picture and the model.
    let mut sh = Shape {
        id: ShapeId(p.next_id + 1),
        name: "Swap".into(),
        xfrm: Some(deckcraft_geom::Xfrm::new(10.0, 10.0, 100.0, 100.0)),
        kind: ShapeKind::Picture { fill: deckcraft_model::style::PictureFill { media: red, ..Default::default() } },
        ..Default::default()
    };
    sh.tags = vec![
        ("DECKRT.OBJECT".into(), format!(r#"{{"images":["media:{}","media:{}"],"glb":"media:{}","note":"media:999"}}"#, red.0, blue.0, model.0)),
        ("OTHER".into(), "media:1".into()),
    ];
    slide.shapes.push(sh);
    p.fix_next_id();

    let bytes = deckcraft_pptx::export(&p).expect("export");
    let names = zip_names(&bytes);
    assert!(names.iter().any(|n| n == "ppt/tags/_rels/tag1.xml.rels"), "{names:?}");
    let rels = zip_text(&bytes, "ppt/tags/_rels/tag1.xml.rels");
    assert_eq!(rels.matches("<Relationship ").count(), 3, "{rels}");
    assert!(rels.contains("relationships/model3d"), "{rels}");
    let tag = zip_text(&bytes, "ppt/tags/tag1.xml");
    assert!(tag.contains("rel:rId"), "{tag}");
    assert!(!tag.contains("media:7") && tag.contains("media:999"), "an unresolvable token stays as written: {tag}");
    assert!(tag.contains(r#"name="OTHER" val="media:1""#), "other tags are not touched: {tag}");
    // Both pictures and the model are package parts.
    assert_eq!(names.iter().filter(|n| n.starts_with("ppt/media/")).count(), 3, "{names:?}");

    let back = deckcraft_pptx::import(&bytes).expect("import");
    let sh = back.slides[0].shapes.iter().find(|s| s.name == "Swap").expect("the swap");
    let tag = &sh.tags.iter().find(|(n, _)| n == "DECKRT.OBJECT").expect("tag").1;
    let ids = deckcraft_model::tag_media::media_ids(tag);
    assert_eq!(ids.len(), 4, "{tag}");
    assert_eq!(ids[3], MediaId(999), "an unresolvable token reads as written: {tag}");
    let ShapeKind::Picture { fill } = &sh.kind else { panic!("picture") };
    assert_eq!(ids[0], fill.media, "the first image is the picture itself: {tag}");
    let bytes_of = |id: MediaId| back.media(id).map(|m| m.data.as_ref().clone()).unwrap_or_default();
    assert_eq!(bytes_of(ids[1]), png([0, 0, 255]));
    assert_eq!(bytes_of(ids[2]), glb(7));
    assert!(tag.contains("media:999"), "{tag}");
    assert_eq!(sh.tags.iter().find(|(n, _)| n == "OTHER").map(|(_, v)| v.as_str()), Some("media:1"));

    // And once more: a second save and open gives the same picture.
    let again = deckcraft_pptx::import(&deckcraft_pptx::export(&back).expect("export")).expect("import");
    let sh2 = again.slides[0].shapes.iter().find(|s| s.name == "Swap").expect("the swap");
    let tag2 = &sh2.tags.iter().find(|(n, _)| n == "DECKRT.OBJECT").expect("tag").1;
    assert_eq!(deckcraft_model::tag_media::media_ids(tag2).len(), 4);
}

#[test]
fn a_model_keeps_its_tags() {
    let (mut p, red, _blue, model) = deck();
    let slide = Arc::make_mut(&mut p.slides[0]);
    let mut sh = Shape {
        id: ShapeId(p.next_id + 1),
        name: "Head".into(),
        xfrm: Some(deckcraft_geom::Xfrm::new(10.0, 10.0, 100.0, 100.0)),
        kind: ShapeKind::Model3d { media: model, poster: Some(red) },
        ..Default::default()
    };
    sh.tags = vec![("DECKRT.OBJECT".into(), r#"{"type":"model"}"#.into())];
    slide.shapes.push(sh);
    p.fix_next_id();
    let bytes = deckcraft_pptx::export(&p).expect("export");
    let back = deckcraft_pptx::import(&bytes).expect("import");
    let sh = back.slides[0].shapes.iter().find(|s| s.name == "Head").expect("the model");
    assert!(matches!(sh.kind, ShapeKind::Model3d { poster: Some(_), .. }), "{:?}", sh.kind);
    assert_eq!(sh.tags, vec![("DECKRT.OBJECT".to_string(), r#"{"type":"model"}"#.to_string())]);
}
