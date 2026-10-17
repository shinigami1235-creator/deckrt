//! deckrt: recorded narration. Each slide's recording is an audio clip on the slide, the way
//! PowerPoint keeps narration: small, in the bottom-right corner, hidden in the show, and played
//! as the slide comes in. The shape carries the tag `DECKRT.NARRATION` so a new recording
//! replaces it and Clear finds it. PowerPoint plays it through a Play effect at the start of the
//! slide's animations; deckrt plays it by its Start Automatically setting.

use std::sync::Arc;

use deckcraft_model::anim::{AnimClass, AnimStart, Animation};
use deckcraft_model::{MediaClip, Shape, ShapeId, ShapeKind, Slide, Xfrm};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

pub const NARRATION_TAG: &str = "DECKRT.NARRATION";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "deckrt.setNarration",
            "Recorded Narration",
            [],
            None,
            "{slides: [{index, path? | data: base64, ms?}], timings?: bool (default true: each listed slide advances after its ms)} → {slides}; a slide listed without audio loses its narration",
            has_doc,
            set_narration
        ),
        cmd!(
            "deckrt.clearNarration",
            "Clear Narration",
            ["Record", "Clear"],
            None,
            "{scope?: slide|all (default slide), timings?: bool (also clear the slides' advance-after times)} → {slides}",
            has_doc,
            clear_narration
        ),
    ]
}

/// The shape is a recorded narration.
pub fn is_narration(sh: &Shape) -> bool {
    sh.tags.iter().any(|(n, _)| n.eq_ignore_ascii_case(NARRATION_TAG))
}

/// The slide's narration clip.
pub fn narration_of(sl: &Slide) -> Option<(ShapeId, &MediaClip)> {
    sl.shapes.iter().find(|sh| is_narration(sh)).and_then(|sh| match &sh.kind {
        ShapeKind::Media(m) => Some((sh.id, m)),
        _ => None,
    })
}

/// Take the narration off a slide, with the Play effect that started it.
fn remove_from(sl: &mut Slide) -> bool {
    let ids: Vec<ShapeId> = sl.shapes.iter().filter(|sh| is_narration(sh)).map(|sh| sh.id).collect();
    if ids.is_empty() {
        return false;
    }
    sl.shapes.retain(|sh| !ids.contains(&sh.id));
    sl.animations.retain(|a| !ids.contains(&a.shape));
    true
}

fn set_narration(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "deckrt.setNarration";
    let list = p.get("slides").and_then(Value::as_array).ok_or_else(|| bad(cmd, "missing `slides`"))?;
    let timings = p.get("timings").and_then(Value::as_bool).unwrap_or(true);
    let n_slides = s.doc()?.doc.slides.len();
    // Read the audio before the edit, so a missing file changes nothing.
    let mut items: Vec<(usize, Option<(String, Vec<u8>, u32)>, Option<u32>)> = vec![];
    for item in list {
        let i = item.get("index").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "each slide needs an `index`"))? as usize;
        if i >= n_slides {
            return Err(bad(cmd, "no such slide"));
        }
        let ms = item.get("ms").and_then(Value::as_u64).map(|v| v.min(86_400_000) as u32);
        let audio = if item.get("path").is_some() || item.get("data").is_some() {
            let (name, bytes) = super::insert::media_bytes(item, cmd)?;
            let bytes = Arc::new(bytes);
            let probed = deckcraft_media::probe(&bytes).ok().filter(|i| i.audio.is_some()).ok_or_else(|| bad(cmd, format!("{name} isn't audio deckrt can play")))?;
            let bytes = Arc::try_unwrap(bytes).unwrap_or_else(|a| (*a).clone());
            Some((name, bytes, probed.duration_ms.min(u32::MAX as u64) as u32))
        } else {
            None
        };
        items.push((i, audio, ms));
    }
    let size = s.doc()?.doc.slide_size;
    let n = s.edit(|doc, _| {
        let mut n = 0;
        for (i, audio, ms) in items {
            let mut new_shape = None;
            if let Some((name, bytes, dur)) = audio {
                let ct = super::insert::content_type(&name, &bytes);
                let media = doc.add_media(&name, ct, bytes).ok_or_else(|| bad(cmd, NO_ID_SPACE))?;
                let id = ShapeId(doc.alloc_id().ok_or_else(|| bad(cmd, NO_ID_SPACE))?);
                let clip = MediaClip { media, volume: 1.0, autoplay: true, hide_while_not_playing: true, duration_ms: dur, ..Default::default() };
                new_shape = Some(Shape {
                    id,
                    name: "Narration".into(),
                    descr: "Recorded narration".into(),
                    xfrm: Some(Xfrm::new(size.width - 60.0, size.height - 60.0, 48.0, 48.0)),
                    kind: ShapeKind::Media(clip),
                    tags: vec![(NARRATION_TAG.to_string(), "1".to_string())],
                    ..Default::default()
                });
            }
            let Some(sl) = doc.slides.get_mut(i) else { continue };
            let sl = Arc::make_mut(sl);
            remove_from(sl);
            if let Some(shape) = new_shape {
                let id = shape.id;
                sl.shapes.push(shape);
                sl.animations.insert(
                    0,
                    Animation { shape: id, class: AnimClass::Media, effect: "play".into(), start: AnimStart::WithPrevious, duration_ms: 1, ..Default::default() },
                );
                n += 1;
            }
            if timings && let Some(ms) = ms {
                let t = sl.transition.get_or_insert_with(Default::default);
                t.advance_after_ms = Some(ms);
            }
        }
        Ok(n)
    })?;
    Ok(json!({"slides": n}))
}

fn clear_narration(s: &mut Session, p: &Value) -> Result<Value> {
    let all = str_param(p, "scope") == Some("all");
    let timings = p.get("timings").and_then(Value::as_bool).unwrap_or(false);
    let current = s.doc()?.selection.slide;
    let n = s.edit(|doc, _| {
        let mut n = 0;
        for (i, sl) in doc.slides.iter_mut().enumerate() {
            if !all && i != current {
                continue;
            }
            let has = sl.shapes.iter().any(is_narration);
            let timed = timings && sl.transition.as_ref().is_some_and(|t| t.advance_after_ms.is_some());
            if !has && !timed {
                continue;
            }
            let sl = Arc::make_mut(sl);
            if remove_from(sl) {
                n += 1;
            }
            if timings && let Some(t) = sl.transition.as_mut() {
                t.advance_after_ms = None;
            }
        }
        Ok(n)
    })?;
    Ok(json!({"slides": n}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(ms: u32) -> String {
        let rate = 8000u32;
        let n = (rate * ms / 1000) as usize;
        let data_len = (n * 2) as u32;
        let mut out = vec![];
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for k in 0..n {
            out.extend_from_slice(&(((k as f32 * 0.1).sin() * 8000.0) as i16).to_le_bytes());
        }
        base64_encode(&out)
    }

    #[test]
    fn narration_replaces_clears_and_survives_pptx() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "blank"})).expect("slide");
        s.execute("deckrt.setNarration", &json!({"slides": [{"index": 0, "data": wav(1500), "name": "n0.wav", "ms": 1500}, {"index": 1, "ms": 4000}]}))
            .expect("narrate");
        let doc = &s.doc().expect("doc").doc;
        let (_, clip) = narration_of(&doc.slides[0]).expect("narration on slide 1");
        assert!(clip.autoplay && clip.hide_while_not_playing);
        assert!((1400..=1600).contains(&clip.duration_ms), "{}", clip.duration_ms);
        assert_eq!(doc.slides[0].animations.len(), 1);
        assert_eq!(doc.slides[1].transition.as_ref().and_then(|t| t.advance_after_ms), Some(4000));
        assert!(narration_of(&doc.slides[1]).is_none());

        // Recording slide 1 again replaces it.
        s.execute("deckrt.setNarration", &json!({"slides": [{"index": 0, "data": wav(800), "name": "n0.wav", "ms": 800}]})).expect("again");
        let doc = &s.doc().expect("doc").doc;
        assert_eq!(doc.slides[0].shapes.iter().filter(|sh| is_narration(sh)).count(), 1);
        assert_eq!(doc.slides[0].animations.len(), 1);

        // Through PPTX and back.
        let bytes = super::super::file::save_bytes(doc, "pptx").expect("save");
        let back = deckcraft_pptx::import(&bytes).expect("import");
        assert!(narration_of(&back.slides[0]).is_some(), "the tag comes back");
        assert_eq!(back.slides[0].animations.first().map(|a| a.class), Some(AnimClass::Media));

        s.execute("slide.go", &json!({"index": 0})).expect("go");
        let r = s.execute("deckrt.clearNarration", &json!({"scope": "all", "timings": true})).expect("clear");
        assert_eq!(r["slides"], 1);
        let doc = &s.doc().expect("doc").doc;
        assert!(doc.slides.iter().all(|sl| narration_of(sl).is_none() && sl.animations.is_empty()));
        assert!(doc.slides.iter().all(|sl| sl.transition.as_ref().is_none_or(|t| t.advance_after_ms.is_none())));
    }
}
