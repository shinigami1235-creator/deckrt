//! deckrt: the check before a talk. It finds what goes wrong in front of a room: videos and
//! sounds that won't play, fonts that change on another computer, text that runs out of its box
//! or off the slide, 4:3 slides on a 16:9 screen, interactive objects whose files are missing,
//! and a deck too big to copy around. Each finding names its slide and, where there is one, a fix
//! the app runs as a list of ordinary commands.

use deckcraft_interactive::object::{self as obj, Config, PopupContent};
use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::text::AutoFit;
use deckcraft_model::{Presentation, Shape, ShapeKind};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        query "deckrt.check",
        "Check Before Showing",
        ["Slide Show", "Check"],
        None,
        "{} → {problems, notes, items: [{severity: problem|note, slide?, shape?, text, fix?: {label, cmds: [[id, params]]}}]}",
        has_doc,
        check
    )]
}

/// Above this the deck is slow to copy and to open on a venue computer.
const BIG_DECK_BYTES: usize = 200 * 1024 * 1024;

struct Out {
    items: Vec<Value>,
}

impl Out {
    fn push(&mut self, severity: &str, slide: Option<usize>, shape: Option<u32>, text: String, fix: Option<Value>) {
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
        self.items.push(v);
    }
}

fn fix(label: &str, cmds: Vec<(&str, Value)>) -> Value {
    json!({"label": label, "cmds": cmds.into_iter().map(|(c, p)| json!([c, p])).collect::<Vec<_>>()})
}

fn shape_name(sh: &Shape) -> String {
    if sh.name.trim().is_empty() { format!("shape {}", sh.id) } else { sh.name.clone() }
}

fn check(s: &mut Session, _p: &Value) -> Result<Value> {
    let doc = s.doc()?.doc.clone();
    let dir = s.doc()?.path.as_deref().and_then(|p| std::path::Path::new(p).parent().map(|d| d.to_path_buf()));
    let mut out = Out { items: vec![] };
    media(&doc, dir.as_deref(), &mut out);
    fonts(&doc, &mut out);
    text_and_bounds(&doc, &mut out);
    interactive(&doc, &mut out);
    size(&doc, &mut out);
    let problems = out.items.iter().filter(|i| i["severity"] == "problem").count();
    let notes = out.items.len() - problems;
    Ok(json!({"problems": problems, "notes": notes, "items": out.items}))
}

// ---------------------------------------------------------------------------------------------

/// Where a linked clip's file is: a `file:` address made a path (PowerPoint writes
/// `file:///C:\…`), and a relative path taken from the deck's folder. `None` for a web address.
pub fn linked_path(link: &str, dir: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    let l = link.trim();
    let lower = l.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return None;
    }
    let raw = if lower.starts_with("file:") {
        let rest = l.get(5..).unwrap_or("").trim_start_matches('/');
        // A drive letter keeps no leading slash (C:/…); elsewhere the path starts at /.
        let path = if rest.get(1..2) == Some(":") { rest.to_string() } else { format!("/{rest}") };
        path.replace("%20", " ")
    } else {
        l.to_string()
    };
    let p = std::path::PathBuf::from(&raw);
    if p.is_relative()
        && let Some(d) = dir
    {
        return Some(d.join(p));
    }
    Some(p)
}

fn media(doc: &Presentation, dir: Option<&std::path::Path>, out: &mut Out) {
    for (i, sl) in doc.slides.iter().enumerate() {
        deckcraft_model::walk(&sl.shapes, &mut |sh, _| {
            let ShapeKind::Media(clip) = &sh.kind else { return };
            let what = if clip.video { "video" } else { "sound" };
            let Some(m) = doc.media(clip.media) else {
                out.push("problem", Some(i), Some(sh.id.0), format!("The {what} \"{}\" has no file in the deck.", shape_name(sh)), None);
                return;
            };
            let bytes: deckcraft_media::Bytes = if m.data.is_empty() {
                match m.link.as_deref() {
                    // A clip that plays from the web isn't checked here.
                    Some(link) if linked_path(link, dir).is_none() => return,
                    Some(link) => match crate::io::read_file(&linked_path(link, dir).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()) {
                        Ok(b) => std::sync::Arc::new(b),
                        Err(_) => {
                            out.push(
                                "problem",
                                Some(i),
                                Some(sh.id.0),
                                format!("The {what} \"{}\" is linked to {link}, which isn't on this computer. Insert the file again so it is kept inside the deck.", m.name),
                                None,
                            );
                            return;
                        }
                    },
                    None => {
                        out.push("problem", Some(i), Some(sh.id.0), format!("The {what} \"{}\" is empty.", m.name), None);
                        return;
                    }
                }
            } else {
                m.data.clone()
            };
            if m.link.is_some() && !m.data.is_empty() {
                // Embedded bytes win; nothing to report.
            } else if m.link.is_some() {
                out.push(
                    "note",
                    Some(i),
                    Some(sh.id.0),
                    format!("The {what} \"{}\" is linked from another file and won't play if the deck is copied without that file.", m.name),
                    None,
                );
            }
            let info = match std::panic::catch_unwind(|| deckcraft_media::probe(&bytes)) {
                Ok(Ok(info)) => info,
                _ => {
                    out.push(
                        "problem",
                        Some(i),
                        Some(sh.id.0),
                        format!("The {what} \"{}\" can't be read. Convert it to MP4 (H.264 and AAC) and insert it again.", m.name),
                        None,
                    );
                    return;
                }
            };
            if !info.playable() {
                let codec = info.video.as_ref().map(|v| v.codec.clone()).or_else(|| info.audio.as_ref().map(|a| a.codec.clone())).unwrap_or_default();
                let kind = if codec.is_empty() { info.container.to_uppercase() } else { format!("{} in {}", codec, info.container.to_uppercase()) };
                out.push(
                    "problem",
                    Some(i),
                    Some(sh.id.0),
                    format!("The {what} \"{}\" is {kind}, which can't be played. Convert it to MP4 (H.264 and AAC) and insert it again.", m.name),
                    None,
                );
                return;
            }
            if clip.video && info.video.as_ref().is_some_and(|v| v.decodable) {
                let first = std::panic::catch_unwind(|| deckcraft_media::poster_frame(&bytes, 0.0).is_ok()).unwrap_or(false);
                if !first {
                    out.push(
                        "problem",
                        Some(i),
                        Some(sh.id.0),
                        format!(
                            "The video \"{}\" opens but its first picture can't be decoded. Convert it to MP4 (H.264) and insert it again.",
                            m.name
                        ),
                        None,
                    );
                    return;
                }
            }
            if clip.video && info.audio.as_ref().is_some_and(|a| !a.decodable) {
                out.push(
                    "problem",
                    Some(i),
                    Some(sh.id.0),
                    format!(
                        "The video \"{}\" will play without sound: its audio is {}. Convert it to MP4 with AAC audio.",
                        m.name,
                        info.audio.as_ref().map(|a| a.codec.as_str()).unwrap_or("")
                    ),
                    None,
                );
            }
        });
    }
}

fn fonts(doc: &Presentation, out: &mut Out) {
    let db = deckcraft_fonts::FontDb::global();
    for family in crate::fonts::missing_fonts(doc) {
        let shown = deckcraft_fonts::substitutes(&family).iter().find(|f| db.has_family(f)).map(|f| f.to_string());
        match shown {
            Some(sub) => out.push(
                "note",
                None,
                None,
                format!(
                    "{family} isn't on this computer, so its text shows in {sub}. Lines may break differently from the computer the deck was made on."
                ),
                Some(fix(&format!("Use {sub}"), vec![("font.replace", json!({"from": family, "to": sub}))])),
            ),
            None => out.push("note", None, None, format!("{family} isn't on this computer, so its text shows in a stand-in font."), None),
        }
    }
}

fn text_and_bounds(doc: &Presentation, out: &mut Out) {
    let sw = doc.slide_size.width;
    let shh = doc.slide_size.height;
    for (i, sl) in doc.slides.iter().enumerate() {
        let Some(ctx) = Ctx::for_slide(doc, sl) else { continue };
        for sh in &sl.shapes {
            if obj::is_live(sh) {
                continue;
            }
            let x = resolve::xfrm(&ctx, sh);
            let has_text = sh.text.as_ref().is_some_and(|t| t.paragraphs.iter().any(|p| !p.is_empty()));
            // Off the slide: shapes the audience won't see in full.
            if sw.is_finite() && shh.is_finite() && x.w > 0.0 && x.h > 0.0 && x.rot == 0.0 {
                let inside_w = (x.x + x.w).min(sw) - x.x.max(0.0);
                let inside_h = (x.y + x.h).min(shh) - x.y.max(0.0);
                let fully_out = inside_w <= 0.0 || inside_h <= 0.0;
                let part_out = !fully_out && (inside_w < x.w - 2.0 || inside_h < x.h - 2.0);
                if part_out && has_text {
                    out.push(
                        "note",
                        Some(i),
                        Some(sh.id.0),
                        format!("\"{}\" sits partly off the slide, so part of its text won't show.", shape_name(sh)),
                        None,
                    );
                }
            }
            if !has_text {
                continue;
            }
            let Some(body) = &sh.text else { continue };
            let bp = resolve::body(&ctx, sh);
            if matches!(bp.autofit, Some(AutoFit::Shrink { .. }) | Some(AutoFit::Shape)) {
                continue;
            }
            let rect = deckcraft_render::shape_geometry(sh, x.w, x.h).text_rect;
            if rect.height() <= 1.0 {
                continue;
            }
            let need = deckcraft_text::fit_height(&ctx, sh, body, rect, &deckcraft_text::NoFields);
            if need > rect.height() + 4.0 {
                let select = json!({"ids": [sh.id.0]});
                // Past its box but still on the slide is a note; off the bottom of the slide, the
                // audience loses the last lines.
                let bottom = x.y + rect.y0 + need;
                let off_slide = shh.is_finite() && bottom > shh + 2.0;
                let (severity, text) = if off_slide {
                    ("problem", format!("Text in \"{}\" runs off the bottom of the slide.", shape_name(sh)))
                } else {
                    ("note", format!("Text in \"{}\" runs past the bottom of its box and may cover what is under it.", shape_name(sh)))
                };
                out.push(
                    severity,
                    Some(i),
                    Some(sh.id.0),
                    text,
                    Some(fix(
                        "Shrink text to fit",
                        vec![("slide.go", json!({"index": i})), ("edit.select", select), ("format.autofit", json!({"mode": "shrink"}))],
                    )),
                );
            }
        }
    }
}

fn interactive(doc: &Presentation, out: &mut Out) {
    let keys: Vec<String> = doc.slides.iter().filter_map(|s| obj::slide_key(s).map(str::to_string)).collect();
    for (i, sl) in doc.slides.iter().enumerate() {
        deckcraft_model::walk(&sl.shapes, &mut |sh, _| {
            let Some(o) = obj::read_any(sh) else { return };
            let name = if o.label.trim().is_empty() { shape_name(sh) } else { o.label.clone() };
            let missing = |m: deckcraft_model::MediaId| doc.media(m).is_none_or(|x| x.data.is_empty());
            match &o.config {
                Config::Model(_) => {
                    if let ShapeKind::Model3d { media, .. } = sh.kind {
                        match doc.media(media) {
                            Some(m) if deckcraft_model3d::load(&m.data).is_ok() => {}
                            _ => out.push(
                                "problem",
                                Some(i),
                                Some(sh.id.0),
                                format!("The 3D model \"{name}\" has no readable model file. Use Replace file in its settings."),
                                None,
                            ),
                        }
                    }
                }
                Config::ImageSwap(c) => {
                    if c.images.iter().any(|r| missing(r.0)) {
                        out.push("problem", Some(i), Some(sh.id.0), format!("The image swap \"{name}\" is missing a picture."), None);
                    }
                }
                Config::Scrub(c) => {
                    if c.images.iter().any(|r| missing(r.0)) {
                        out.push("problem", Some(i), Some(sh.id.0), format!("The scrub slider \"{name}\" is missing a picture."), None);
                    }
                }
                Config::Wipe(c) => {
                    if missing(c.before.0) || missing(c.after.0) {
                        out.push("problem", Some(i), Some(sh.id.0), format!("The wipe \"{name}\" is missing its before or after picture."), None);
                    }
                }
                Config::Popup(c) => {
                    if let PopupContent::Image { media } = &c.content
                        && missing(media.0)
                    {
                        out.push("problem", Some(i), Some(sh.id.0), format!("The popup \"{name}\" is missing its picture."), None);
                    }
                    if let PopupContent::Web { url } = &c.content {
                        out.push(
                            "note",
                            Some(i),
                            Some(sh.id.0),
                            format!("The popup \"{name}\" opens {url}, which needs an internet connection at the venue."),
                            None,
                        );
                    }
                }
                Config::Poll(c) => {
                    for (k, t) in c.targets.iter().enumerate() {
                        if let Some(t) = t
                            && !keys.iter().any(|x| x == t)
                        {
                            let answer = c.options.get(k).cloned().unwrap_or_default();
                            out.push(
                                "problem",
                                Some(i),
                                Some(sh.id.0),
                                format!("In the poll \"{name}\", the answer \"{answer}\" leads to a slide that was deleted."),
                                None,
                            );
                        }
                    }
                }
                _ => {}
            }
        });
    }
}

fn size(doc: &Presentation, out: &mut Out) {
    let (w, h) = (doc.slide_size.width, doc.slide_size.height);
    if w.is_finite() && h.is_finite() && h > 0.0 && (w / h - 4.0 / 3.0).abs() < 0.02 {
        out.push("note", None, None, "The slides are 4:3. On a 16:9 screen they show with black bars at the sides.".into(), None);
    }
    let total: usize = doc.media.iter().map(|m| m.data.len()).sum();
    if total > BIG_DECK_BYTES {
        out.push(
            "note",
            None,
            None,
            format!("Pictures, videos and models add up to {} MB, which is slow to copy to a venue computer.", total / (1024 * 1024)),
            Some(fix("Compress pictures", vec![("picture.compress", json!({"all": true, "ppi": 220, "deleteCropped": true}))])),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clean_deck_has_no_problems_and_overflowing_text_is_found() {
        let mut s = Session::new();
        s.execute("file.new", &json!({})).unwrap();
        let r = s.execute("deckrt.check", &json!({})).unwrap();
        assert_eq!(r["problems"], 0, "{r}");
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        s.execute("insert.textBox", &json!({"rect": [100, 100, 200, 30], "text": "one"})).unwrap();
        s.execute("format.autofit", &json!({"mode": "none"})).unwrap();
        let long = "A line of text that keeps going. ".repeat(40);
        let id = s.execute("slide.inspect", &json!({})).unwrap()["shapes"].as_array().unwrap().last().unwrap()["id"].as_u64().unwrap();
        s.execute("edit.select", &json!({"ids": [id]})).unwrap();
        s.execute("text.set", &json!({"id": id, "text": long})).unwrap();
        s.execute("format.autofit", &json!({"mode": "none"})).unwrap();
        let r = s.execute("deckrt.check", &json!({})).unwrap();
        let items = r["items"].as_array().unwrap();
        let item = items.iter().find(|i| i["text"].as_str().unwrap_or("").contains("Text in")).unwrap_or_else(|| panic!("no overflow found: {r}"));
        assert_eq!(item["severity"], "problem", "{item}");
        for c in item["fix"]["cmds"].as_array().unwrap() {
            s.execute(c[0].as_str().unwrap(), &c[1]).unwrap();
        }
        let r = s.execute("deckrt.check", &json!({})).unwrap();
        assert!(!r["items"].as_array().unwrap().iter().any(|i| i["text"].as_str().unwrap_or("").contains("Text in")), "{r}");
    }

    #[test]
    fn a_four_by_three_deck_gets_a_note() {
        let mut s = Session::new();
        s.execute("file.new", &json!({"size": [720, 540]})).unwrap();
        let r = s.execute("deckrt.check", &json!({})).unwrap();
        assert!(r["items"].as_array().unwrap().iter().any(|i| i["text"].as_str().unwrap_or("").contains("4:3")), "{r}");
    }
}
