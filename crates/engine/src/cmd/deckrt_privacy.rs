//! deckrt: the check before sharing a deck that may hold patient material. PowerPoint keeps the
//! cropped-away part of a photo, the camera's metadata, text hidden in the background colour,
//! speaker notes and shapes parked off the slide, all inside the file. This finds them and
//! removes what the presenter asks, and blurs part of a photo for good.

use std::sync::Arc;

use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::{MediaId, Shape, ShapeId, ShapeKind};
use serde_json::{Value, json};

use super::*;
use crate::{EngineError, Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            query "deckrt.privacy",
            "Check Before Sharing",
            ["Review", "Privacy"],
            None,
            "{} → {problems, notes, items: [{severity, slide?, shape?, text, fix?: {label, cmds}}]}",
            has_doc,
            privacy
        ),
        cmd!(
            "deckrt.applyCrops",
            "Delete Cropped Areas",
            [],
            None,
            "{ids?: [shape] (default: every cropped picture)} → {pictures}",
            has_doc,
            apply_crops
        ),
        cmd!("deckrt.stripMetadata", "Remove Photo Details", [], None, "{} → {pictures}", has_doc, strip_metadata),
        cmd!("deckrt.removeNotes", "Remove All Speaker Notes", [], None, "{} → {slides}", has_doc, remove_notes),
        cmd!(
            "deckrt.blurArea",
            "Blur Part of Picture",
            ["Picture Format", "Privacy"],
            None,
            "{id, rect: [x, y, w, h] as fractions of the picture's box, strength?: 1..3} → {id}",
            has_slide,
            blur_area
        ),
    ]
}

fn fix(label: &str, cmds: Vec<(&str, Value)>) -> Value {
    json!({"label": label, "cmds": cmds.into_iter().map(|(c, p)| json!([c, p])).collect::<Vec<_>>()})
}

fn item(severity: &str, slide: Option<usize>, shape: Option<u32>, text: String, fix: Option<Value>) -> Value {
    let mut v = json!({"severity": severity, "text": text});
    if let Some(s) = slide {
        v["slide"] = json!(s);
    }
    if let Some(id) = shape {
        v["shape"] = json!(id);
    }
    if let Some(f) = fix {
        v["fix"] = f;
    }
    v
}

fn name_of(sh: &Shape) -> String {
    if sh.name.trim().is_empty() { format!("shape {}", sh.id) } else { sh.name.clone() }
}

// ---------------------------------------------------------------------------------------------
// Finding

/// Words that suggest a patient's identity: hospital or record numbers, a date of birth, a
/// 13-digit national ID.
pub fn patient_marks(text: &str) -> Vec<String> {
    let mut out = vec![];
    // Lowercased one character at a time, so its byte offsets are the text's own (lowercasing
    // can change a character's length).
    let lower: String = text.chars().map(|c| c.to_lowercase().next().filter(|l| l.len_utf8() == c.len_utf8()).unwrap_or(c)).collect();
    for key in ["hn", "mrn", "patient id", "hospital number", "dob", "date of birth", "เลขที่ผู้ป่วย", "เลขประจำตัว"]
    {
        let mut from = 0;
        while let Some(i) = lower[from..].find(key) {
            let at = from + i;
            from = at + key.len();
            let before_ok = at == 0 || !lower[..at].chars().next_back().is_some_and(|c| c.is_alphanumeric());
            let after: String = lower[from..].chars().take(20).collect();
            let digits = after.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit() || *c == '/' || *c == '-').count();
            let gap = after.chars().take_while(|c| !c.is_ascii_digit()).count();
            if before_ok && gap <= 4 && digits >= 4 {
                let shown: String = text[at..].chars().take(key.chars().count() + gap + digits).collect();
                out.push(shown.trim().to_string());
            }
        }
    }
    // A 13-digit run, with or without dashes: a Thai ID card number.
    let mut run = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_digit() || (c == '-' && !run.is_empty()) {
            run.push(c);
        } else {
            if run.chars().filter(char::is_ascii_digit).count() == 13 {
                out.push(run.clone());
            }
            run.clear();
        }
    }
    out.dedup();
    out
}

/// A JPEG's EXIF, XMP or IPTC block, or a PNG's text and EXIF chunks: what a camera or phone
/// writes about where and when.
pub fn has_metadata(b: &[u8]) -> bool {
    if b.starts_with(&[0xFF, 0xD8]) {
        let mut i = 2;
        while i + 4 <= b.len() && b[i] == 0xFF {
            let marker = b[i + 1];
            if marker == 0xDA {
                break;
            }
            let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
            if marker == 0xE1 || marker == 0xED || marker == 0xFE {
                return true;
            }
            i += 2 + len;
        }
        false
    } else if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        png_chunks(b).iter().any(|(t, _, _)| matches!(t.as_slice(), b"tEXt" | b"iTXt" | b"zTXt" | b"eXIf"))
    } else {
        false
    }
}

fn png_chunks(b: &[u8]) -> Vec<(Vec<u8>, usize, usize)> {
    let mut out = vec![];
    let mut i = 8;
    while i + 12 <= b.len() {
        let len = u32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as usize;
        let end = i + 12 + len;
        if end > b.len() {
            break;
        }
        out.push((b[i + 4..i + 8].to_vec(), i, end));
        i = end;
    }
    out
}

/// The same picture without its metadata, byte for byte otherwise (no recompression).
pub fn without_metadata(b: &[u8]) -> Option<Vec<u8>> {
    if b.starts_with(&[0xFF, 0xD8]) {
        let mut out = vec![0xFF, 0xD8];
        let mut i = 2;
        while i + 4 <= b.len() && b[i] == 0xFF {
            let marker = b[i + 1];
            if marker == 0xDA {
                out.extend_from_slice(&b[i..]);
                return Some(out);
            }
            let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
            let end = (i + 2 + len).min(b.len());
            // APP1 (EXIF, XMP), APP13 (IPTC) and comments go; everything else stays.
            if !(marker == 0xE1 || marker == 0xED || marker == 0xFE) {
                out.extend_from_slice(&b[i..end]);
            }
            i = end;
        }
        None
    } else if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        let mut out = b[..8].to_vec();
        for (t, s, e) in png_chunks(b) {
            if !matches!(t.as_slice(), b"tEXt" | b"iTXt" | b"zTXt" | b"eXIf") {
                out.extend_from_slice(&b[s..e]);
            }
        }
        Some(out)
    } else {
        None
    }
}

fn privacy(s: &mut Session, _p: &Value) -> Result<Value> {
    let doc = s.doc()?.doc.clone();
    let mut items = vec![];
    let (sw, shh) = (doc.slide_size.width, doc.slide_size.height);
    let mut cropped = 0usize;
    let mut with_meta: Vec<MediaId> = vec![];
    for (i, sl) in doc.slides.iter().enumerate() {
        let ctx = Ctx::for_slide(&doc, sl);
        deckcraft_model::walk(&sl.shapes, &mut |sh, depth| {
            if let ShapeKind::Picture { fill } = &sh.kind {
                if fill.crop.iter().any(|c| *c > 0.001) {
                    cropped += 1;
                    items.push(item(
                        "problem",
                        Some(i),
                        Some(sh.id.0),
                        format!("\"{}\" is cropped, but the cropped-away part is still in the file and can be brought back.", name_of(sh)),
                        Some(fix("Delete the cropped part", vec![("deckrt.applyCrops", json!({"ids": [sh.id.0]}))])),
                    ));
                }
                if let Some(m) = doc.media(fill.media)
                    && has_metadata(&m.data)
                    && !with_meta.contains(&fill.media)
                {
                    with_meta.push(fill.media);
                }
            }
            // Off the slide: in the file, never on the screen.
            if depth == 0
                && let Some(c) = &ctx
            {
                let x = resolve::xfrm(c, sh);
                if sw.is_finite() && shh.is_finite() && x.w > 0.0 && x.h > 0.0 && (x.x >= sw || x.y >= shh || x.x + x.w <= 0.0 || x.y + x.h <= 0.0) {
                    items.push(item(
                        "problem",
                        Some(i),
                        Some(sh.id.0),
                        format!("\"{}\" sits outside the slide. It never shows, but it is in the file.", name_of(sh)),
                        Some(fix("Delete it", vec![("slide.go", json!({"index": i})), ("edit.delete", json!({"ids": [sh.id.0]}))])),
                    ));
                }
            }
            // Patient numbers in the slide's text.
            if let Some(t) = &sh.text {
                for m in patient_marks(&t.text()) {
                    items.push(item(
                        "problem",
                        Some(i),
                        Some(sh.id.0),
                        format!("\"{}\" holds what looks like a patient identifier: {m}", name_of(sh)),
                        None,
                    ));
                }
            }
        });
        // Text drawn in the colour behind it.
        if let Some(ctx) = &ctx {
            let bg = match resolve::background(ctx, Some(sl)) {
                (deckcraft_model::Fill::Solid { color }, ph) => Some(ctx.color(&color, ph)),
                _ => None,
            };
            if let Some(bg) = bg {
                for sh in &sl.shapes {
                    let Some(t) = &sh.text else { continue };
                    let Some(para) = t.paragraphs.iter().find(|p| !p.is_empty()) else { continue };
                    let (fill, _) = resolve::fill(ctx, sh);
                    if fill.as_ref().is_some_and(|f| !f.is_none()) {
                        continue;
                    }
                    let rp = resolve::run(ctx, sh, para, &para.runs.first().map(|r| r.props.clone()).unwrap_or_default());
                    let fg = resolve::text_color(ctx, &rp);
                    if fg.contrast(bg) < 1.15 {
                        items.push(item(
                            "problem",
                            Some(i),
                            Some(sh.id.0),
                            format!("Text in \"{}\" is the colour of the background. It can't be seen, but it is in the file.", name_of(sh)),
                            Some(fix("Delete it", vec![("slide.go", json!({"index": i})), ("edit.delete", json!({"ids": [sh.id.0]}))])),
                        ));
                    }
                }
            }
        }
        for m in patient_marks(&sl.notes.text()) {
            items.push(item("problem", Some(i), None, format!("The speaker notes hold what looks like a patient identifier: {m}"), None));
        }
        if sl.hidden {
            items.push(item("note", Some(i), None, "This slide is hidden in the show, but it is in the file.".into(), None));
        }
    }
    // Every other picture the deck holds too: an object's own pictures (a swap's later ones, a
    // wipe's after, a scrub's), picture fills and backgrounds.
    for id in crate::media_refs::presentation_media(&doc) {
        if !with_meta.contains(&id)
            && let Some(m) = doc.media(id)
            && has_metadata(&m.data)
        {
            with_meta.push(id);
        }
    }
    if !with_meta.is_empty() {
        let n = with_meta.len();
        items.push(item(
            "problem",
            None,
            None,
            format!(
                "{n} picture{} still carr{} camera details: the date, the device and sometimes where it was taken.",
                if n == 1 { "" } else { "s" },
                if n == 1 { "ies" } else { "y" }
            ),
            Some(fix("Remove photo details", vec![("deckrt.stripMetadata", json!({}))])),
        ));
    }
    if cropped > 1 {
        items.push(item(
            "note",
            None,
            None,
            format!("{cropped} pictures are cropped."),
            Some(fix("Delete every cropped part", vec![("deckrt.applyCrops", json!({}))])),
        ));
    }
    let with_notes: Vec<usize> = doc.slides.iter().enumerate().filter(|(_, s)| !s.notes.text().trim().is_empty()).map(|(i, _)| i + 1).collect();
    if !with_notes.is_empty() {
        let list = with_notes.iter().take(12).map(|n| n.to_string()).collect::<Vec<_>>().join(", ");
        items.push(item(
            "note",
            None,
            None,
            format!(
                "Speaker notes go with the file (slide{} {list}{}).",
                if with_notes.len() == 1 { "" } else { "s" },
                if with_notes.len() > 12 { " and more" } else { "" }
            ),
            Some(fix("Remove all speaker notes", vec![("deckrt.removeNotes", json!({}))])),
        ));
    }
    let comments: usize = doc.slides.iter().map(|s| s.comments.len()).sum();
    if comments > 0 {
        items.push(item(
            "note",
            None,
            None,
            format!("{comments} comment{} go with the file.", if comments == 1 { "" } else { "s" }),
            Some(fix("Delete all comments", vec![("comment.delete", json!({"all": true}))])),
        ));
    }
    let author = doc.props.author.trim().to_string();
    if !author.is_empty() {
        items.push(item(
            "note",
            None,
            None,
            format!("The file names its author: {author}."),
            Some(fix("Remove the author", vec![("file.properties", json!({"author": "", "company": ""}))])),
        ));
    }
    let problems = items.iter().filter(|i| i["severity"] == "problem").count();
    let notes = items.len() - problems;
    Ok(json!({"problems": problems, "notes": notes, "items": items}))
}

// ---------------------------------------------------------------------------------------------
// Fixing

fn encode_like(original: &[u8], img: &image::DynamicImage) -> Option<(Vec<u8>, &'static str)> {
    let mut out = std::io::Cursor::new(vec![]);
    if original.starts_with(&[0xFF, 0xD8]) {
        let rgb = img.to_rgb8();
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
        enc.encode_image(&rgb).ok()?;
        Some((out.into_inner(), "image/jpeg"))
    } else {
        img.write_to(&mut out, image::ImageFormat::Png).ok()?;
        Some((out.into_inner(), "image/png"))
    }
}

fn apply_crops(s: &mut Session, p: &Value) -> Result<Value> {
    let only: Option<Vec<u32>> = p.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_u64).map(|v| v as u32).collect());
    let n = s.edit(|doc, _| {
        let mut jobs: Vec<(usize, ShapeId, MediaId, [f64; 4])> = vec![];
        for (i, sl) in doc.slides.iter().enumerate() {
            deckcraft_model::walk(&sl.shapes, &mut |sh, _| {
                if let ShapeKind::Picture { fill } = &sh.kind
                    && fill.crop.iter().any(|c| *c > 0.001)
                    && only.as_ref().is_none_or(|o| o.contains(&sh.id.0))
                {
                    jobs.push((i, sh.id, fill.media, fill.crop));
                }
            });
        }
        let mut done = 0;
        for (i, id, media, crop) in jobs {
            let Some(m) = doc.media(media) else { continue };
            let Ok(img) = image::load_from_memory(&m.data) else { continue };
            let (w, h) = (img.width() as f64, img.height() as f64);
            let l = (crop[0].max(0.0) * w).round() as u32;
            let t = (crop[1].max(0.0) * h).round() as u32;
            let r = (crop[2].max(0.0) * w).round() as u32;
            let b = (crop[3].max(0.0) * h).round() as u32;
            let cw = img.width().saturating_sub(l + r).max(1);
            let ch = img.height().saturating_sub(t + b).max(1);
            let cut = img.crop_imm(l, t, cw, ch);
            let Some((bytes, ct)) = encode_like(&m.data, &cut) else { continue };
            let name = format!("cropped-{}", m.name);
            let Some(new_id) = doc.add_media(&name, ct, bytes) else { continue };
            if let Some(sh) = deckcraft_model::find_shape_mut(&mut Arc::make_mut(&mut doc.slides[i]).shapes, id) {
                replace_picture(sh, media, new_id);
                if let ShapeKind::Picture { fill } = &mut sh.kind {
                    // What was padding (negative crop) stays as it was.
                    fill.crop = [crop[0].min(0.0), crop[1].min(0.0), crop[2].min(0.0), crop[3].min(0.0)];
                    done += 1;
                }
            }
        }
        Ok(done)
    })?;
    Ok(json!({"pictures": n}))
}

fn strip_metadata(s: &mut Session, _p: &Value) -> Result<Value> {
    let n = s.edit(|doc, _| {
        let mut n = 0;
        for m in doc.media.iter_mut() {
            if !m.content_type.starts_with("image/") || !has_metadata(&m.data) {
                continue;
            }
            if let Some(clean) = without_metadata(&m.data) {
                m.data = Arc::new(clean);
                n += 1;
            }
        }
        Ok(n)
    })?;
    Ok(json!({"pictures": n}))
}

fn remove_notes(s: &mut Session, _p: &Value) -> Result<Value> {
    let n = s.edit(|doc, _| {
        let mut n = 0;
        for sl in doc.slides.iter_mut() {
            if !sl.notes.text().trim().is_empty() {
                Arc::make_mut(sl).notes = deckcraft_model::text::TextBody::default();
                n += 1;
            }
        }
        Ok(n)
    })?;
    Ok(json!({"slides": n}))
}

/// Put `new` in place of `old` everywhere the shape names it, so the original picture leaves the
/// file: the picture itself, the original kept under an artistic effect (dropped, with the
/// effect left baked in), and an interactive object's own pictures in its tag (a swap, wipe or
/// scrub draws those in the show).
fn replace_picture(sh: &mut deckcraft_model::Shape, old: MediaId, new: MediaId) {
    if let ShapeKind::Picture { fill } = &mut sh.kind {
        fill.media = new;
        fill.artistic = None;
    }
    crate::media_refs::visit_mut::tags(&mut sh.tags, &mut |m: &mut MediaId| {
        if *m == old {
            *m = new;
        }
    });
}

fn blur_area(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "deckrt.blurArea";
    let id = id_param(p, "id", cmd)?.ok_or_else(|| bad(cmd, "missing `id`"))?;
    let rect: Vec<f64> = p.get("rect").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_f64).collect()).unwrap_or_default();
    if rect.len() != 4 || rect.iter().any(|v| !v.is_finite()) || rect[2] <= 0.0 || rect[3] <= 0.0 {
        return Err(bad(cmd, "`rect` must be [x, y, w, h] as fractions of the picture"));
    }
    let strength = p.get("strength").and_then(Value::as_u64).unwrap_or(2).clamp(1, 3) as u32;
    let slide = s.doc()?.selection.slide;
    s.edit(|doc, _| {
        let sl = doc.slides.get(slide).ok_or_else(|| EngineError::Other("no slide".into()))?;
        let mut found = None;
        deckcraft_model::walk(&sl.shapes, &mut |sh, _| {
            if sh.id == id
                && let ShapeKind::Picture { fill } = &sh.kind
            {
                found = Some((fill.media, fill.crop));
            }
        });
        let (media, crop) = found.ok_or_else(|| bad(cmd, "not a picture"))?;
        let m = doc.media(media).ok_or_else(|| EngineError::Other("the picture's file is missing".into()))?;
        let mut img = image::load_from_memory(&m.data).map_err(|e| EngineError::Other(format!("the picture can't be read: {e}")))?.to_rgba8();
        let (w, h) = (img.width() as f64, img.height() as f64);
        // The box shows the image between its crops; map the box fractions into image pixels.
        let (vl, vt) = (crop[0] * w, crop[1] * h);
        let vw = w * (1.0 - crop[0] - crop[2]);
        let vh = h * (1.0 - crop[1] - crop[3]);
        let x0 = (vl + rect[0].clamp(0.0, 1.0) * vw).clamp(0.0, w) as u32;
        let y0 = (vt + rect[1].clamp(0.0, 1.0) * vh).clamp(0.0, h) as u32;
        let x1 = (vl + (rect[0] + rect[2]).clamp(0.0, 1.0) * vw).clamp(0.0, w) as u32;
        let y1 = (vt + (rect[1] + rect[3]).clamp(0.0, 1.0) * vh).clamp(0.0, h) as u32;
        if x1 <= x0 + 1 || y1 <= y0 + 1 {
            return Err(bad(cmd, "the area is too small"));
        }
        // Coarse blocks first (nothing of the face survives), then a blur so it reads as blurred.
        let block = (((x1 - x0).max(y1 - y0)) / (14 / strength).max(3)).max(4);
        let mut by = y0;
        while by < y1 {
            let mut bx = x0;
            while bx < x1 {
                let (ex, ey) = ((bx + block).min(x1), (by + block).min(y1));
                let mut acc = [0u64; 4];
                let mut n = 0u64;
                for y in by..ey {
                    for x in bx..ex {
                        let px = img.get_pixel(x, y).0;
                        for k in 0..4 {
                            acc[k] += px[k] as u64;
                        }
                        n += 1;
                    }
                }
                let avg = image::Rgba([(acc[0] / n) as u8, (acc[1] / n) as u8, (acc[2] / n) as u8, (acc[3] / n) as u8]);
                for y in by..ey {
                    for x in bx..ex {
                        img.put_pixel(x, y, avg);
                    }
                }
                bx = ex;
            }
            by = (by + block).min(y1);
        }
        let region = image::imageops::crop_imm(&img, x0, y0, x1 - x0, y1 - y0).to_image();
        let soft = image::imageops::blur(&region, block as f32 * 0.6);
        image::imageops::replace(&mut img, &soft, x0 as i64, y0 as i64);
        let original = m.data.clone();
        let name = format!("blurred-{}", m.name);
        let (bytes, ct) =
            encode_like(&original, &image::DynamicImage::ImageRgba8(img)).ok_or_else(|| EngineError::Other("the picture can't be saved".into()))?;
        let new_id = doc.add_media(&name, ct, bytes).ok_or_else(|| EngineError::Other("no room for another picture".into()))?;
        if let Some(sh) = deckcraft_model::find_shape_mut(&mut Arc::make_mut(&mut doc.slides[slide]).shapes, id) {
            replace_picture(sh, media, new_id);
        }
        Ok(())
    })?;
    Ok(json!({"id": id.0}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patient_identifiers_are_found() {
        assert_eq!(patient_marks("Case 3, HN 6541234, 42F"), vec!["HN 6541234".to_string()]);
        assert_eq!(patient_marks("MRN: 00123456"), vec!["MRN: 00123456".to_string()]);
        assert!(patient_marks("John's DOB 12/03/1985").iter().any(|m| m.starts_with("DOB")));
        assert_eq!(patient_marks("ID 1-1037-01234-56-7"), vec!["1-1037-01234-56-7".to_string()]);
        assert!(patient_marks("John Hnatyshyn wrote 1234 lines").is_empty(), "HN inside a word");
        assert!(patient_marks("Type I collagen, 80%").is_empty());
    }

    #[test]
    fn jpeg_metadata_comes_out_without_recompression() {
        let img = image::DynamicImage::new_rgb8(8, 8);
        let mut jpg = std::io::Cursor::new(vec![]);
        img.write_to(&mut jpg, image::ImageFormat::Jpeg).unwrap();
        let plain = jpg.into_inner();
        // Put an APP1 (EXIF) segment after SOI.
        let exif = b"Exif\0\0GPS 13.7563 N 100.5018 E";
        let mut with = vec![0xFF, 0xD8, 0xFF, 0xE1];
        with.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        with.extend_from_slice(exif);
        with.extend_from_slice(&plain[2..]);
        assert!(has_metadata(&with));
        let clean = without_metadata(&with).unwrap();
        assert!(!has_metadata(&clean));
        assert_eq!(clean, plain, "the picture data is untouched");
        assert!(image::load_from_memory(&clean).is_ok());
    }

    #[test]
    fn crops_and_blur_change_the_picture_for_good() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        let mut img = image::RgbImage::new(100, 100);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = image::Rgb([(x * 2) as u8, (y * 2) as u8, if (x / 10 + y / 10) % 2 == 0 { 255 } else { 0 }]);
        }
        let mut png = std::io::Cursor::new(vec![]);
        image::DynamicImage::ImageRgb8(img).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let data = super::base64_encode(&png.into_inner());
        let id =
            s.execute("insert.picture", &json!({"data": data, "name": "face.png", "rect": [100, 100, 200, 200]})).unwrap()["id"].as_u64().unwrap();
        s.execute("picture.crop", &json!({"left": 0.25, "ids": [id]})).unwrap();
        let r = s.execute("deckrt.privacy", &json!({})).unwrap();
        assert!(r["items"].as_array().unwrap().iter().any(|i| i["text"].as_str().unwrap().contains("cropped")), "{r}");
        s.execute("deckrt.applyCrops", &json!({})).unwrap();
        let r = s.execute("deckrt.privacy", &json!({})).unwrap();
        assert!(!r["items"].as_array().unwrap().iter().any(|i| i["text"].as_str().unwrap().contains("cropped")), "{r}");
        let doc = s.doc().unwrap().doc.clone();
        let mut size = None;
        deckcraft_model::walk(&doc.slides[doc.slides.len() - 1].shapes, &mut |sh, _| {
            if let ShapeKind::Picture { fill } = &sh.kind {
                let m = doc.media(fill.media).unwrap();
                size = Some(image::load_from_memory(&m.data).unwrap().width());
            }
        });
        assert_eq!(size, Some(75), "the left quarter is gone");
        s.execute("deckrt.blurArea", &json!({"id": id, "rect": [0.2, 0.2, 0.5, 0.5]})).unwrap();
        assert!(s.execute("deckrt.blurArea", &json!({"id": id, "rect": [0.2, 0.2, 0.0, 0.5]})).is_err());
    }
}
