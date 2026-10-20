//! deckrt: the pace kept from a rehearsal with the coach. Each slide carries the seconds it took
//! in the slide tag `DECKRT.PACE`, so the pace follows the slide when slides move, and presenter
//! view can say how far ahead or behind the talk is.

use std::sync::Arc;

use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

pub const PACE_TAG: &str = "DECKRT.PACE";

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "deckrt.keepPace",
            "Keep Rehearsal Pace",
            [],
            None,
            "{slides: [{index, secs}]} → {slides}; slides not listed lose any pace they had",
            has_doc,
            keep_pace
        ),
        cmd!(
            "deckrt.addNotes",
            "Add to Speaker Notes",
            [],
            None,
            "{slides: [{index, text}]} → {slides}; each text goes after the slide's notes, a blank line between",
            has_doc,
            add_notes
        ),
    ]
}

fn add_notes(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "deckrt.addNotes";
    let list = p.get("slides").and_then(Value::as_array).ok_or_else(|| bad(cmd, "missing `slides`"))?;
    let n_slides = s.doc()?.doc.slides.len();
    let mut items = vec![];
    for item in list {
        let i = item.get("index").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "each slide needs an `index`"))? as usize;
        if i >= n_slides {
            return Err(bad(cmd, "no such slide"));
        }
        let text = item.get("text").and_then(Value::as_str).unwrap_or("").trim().to_string();
        if !text.is_empty() {
            items.push((i, text));
        }
    }
    let n = items.len();
    s.edit(|doc, _| {
        for (i, text) in items {
            let Some(sl) = doc.slides.get_mut(i) else { continue };
            let notes = &mut Arc::make_mut(sl).notes;
            let empty = notes.text().trim().is_empty();
            if empty {
                notes.paragraphs.clear();
            } else {
                notes.paragraphs.push(deckcraft_model::text::Paragraph::new(""));
            }
            for line in text.split('\n') {
                notes.paragraphs.push(deckcraft_model::text::Paragraph::new(line));
            }
        }
        Ok(())
    })?;
    Ok(json!({"slides": n}))
}

/// The seconds a slide took in the kept rehearsal.
pub fn pace_of(slide: &deckcraft_model::Slide) -> Option<f64> {
    slide
        .tags
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(PACE_TAG))
        .and_then(|(_, v)| v.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v >= 0.0)
}

fn keep_pace(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "deckrt.keepPace";
    let list = p.get("slides").and_then(Value::as_array).ok_or_else(|| bad(cmd, "missing `slides`"))?;
    let mut secs: Vec<Option<f64>> = vec![None; s.doc()?.doc.slides.len()];
    for item in list {
        let i = item.get("index").and_then(Value::as_u64).ok_or_else(|| bad(cmd, "each slide needs an `index`"))? as usize;
        let t = item.get("secs").and_then(Value::as_f64).filter(|t| t.is_finite() && *t >= 0.0).ok_or_else(|| bad(cmd, "each slide needs `secs`"))?;
        let slot = secs.get_mut(i).ok_or_else(|| bad(cmd, "no such slide"))?;
        *slot = Some(t);
    }
    let n = secs.iter().flatten().count();
    s.edit(|doc, _| {
        for (sl, t) in doc.slides.iter_mut().zip(&secs) {
            let had = pace_of(sl);
            let want = t.map(|t| (t * 10.0).round() / 10.0);
            if had == want {
                continue;
            }
            let sl = Arc::make_mut(sl);
            sl.tags.retain(|(n, _)| !n.eq_ignore_ascii_case(PACE_TAG));
            if let Some(t) = want {
                sl.tags.push((PACE_TAG.to_string(), format!("{t:.1}")));
            }
        }
        Ok(())
    })?;
    Ok(json!({"slides": n}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pace_is_kept_per_slide_and_replaced() {
        let mut s = Session::with_new();
        for _ in 0..2 {
            s.execute("slide.new", &json!({"layout": "blank"})).expect("slide");
        }
        s.execute("deckrt.keepPace", &json!({"slides": [{"index": 0, "secs": 61.24}, {"index": 2, "secs": 30}]})).expect("keep");
        let paces = |s: &Session| s.doc().expect("doc").doc.slides.iter().map(|s| pace_of(s)).collect::<Vec<_>>();
        assert_eq!(paces(&s), vec![Some(61.2), None, Some(30.0)]);
        s.execute("deckrt.keepPace", &json!({"slides": [{"index": 1, "secs": 12}]})).expect("keep");
        assert_eq!(paces(&s), vec![None, Some(12.0), None]);
        s.execute("edit.undo", &json!({})).expect("undo");
        assert_eq!(paces(&s), vec![Some(61.2), None, Some(30.0)]);
        assert!(s.execute("deckrt.keepPace", &json!({"slides": [{"index": 9, "secs": 1}]})).is_err());
    }

    #[test]
    fn notes_are_added_after_what_was_there() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "blank"})).expect("slide");
        s.execute("deckrt.addNotes", &json!({"slides": [{"index": 0, "text": "Fibroblasts make collagen."}]})).expect("add");
        s.execute("deckrt.addNotes", &json!({"slides": [{"index": 0, "text": "Type I and III."}, {"index": 1, "text": "  "}]})).expect("add");
        let doc = &s.doc().expect("doc").doc;
        assert_eq!(doc.slides[0].notes.text(), "Fibroblasts make collagen.\n\nType I and III.");
        assert_eq!(doc.slides[1].notes.text().trim(), "");
    }
}
