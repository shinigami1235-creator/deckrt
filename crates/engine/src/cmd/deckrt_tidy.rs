//! deckrt: Tidy This Slide. One undoable step that does what a careful person would do by hand:
//!
//! - shapes in a row that are nearly the same size become the same size, their tops line up
//!   and the gaps between them even out (columns the same, with lefts and vertical gaps);
//! - a text box just under a shape becomes its caption: the shape's width, a set gap below it;
//! - body text much smaller than the rest of the slide's comes up to the rest's size;
//! - a shape partly off the slide comes back on, narrower when it is wider than the slide;
//! - a text box whose text runs past its bottom grows to fit, or shrinks its text when there
//!   is no room below.
//!
//! Placeholders keep their layout's position; only shapes placed on the slide move.

use std::sync::Arc;

use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::text::AutoFit;
use deckcraft_model::{Presentation, Shape, ShapeKind, Slide, Xfrm};
use serde_json::{Value, json};

use super::*;
use crate::{Result, Session};

/// Edges this close count as meant to line up, in points.
const NEAR: f64 = 18.0;
/// Sizes within this share of each other count as meant to match.
const SAME_SIZE: f64 = 0.2;
/// The gap between a shape and its caption.
const CAPTION_GAP: f64 = 10.0;
/// Space kept from the slide's edge when a shape comes back on.
const MARGIN: f64 = 24.0;
/// Body text under this is raised when the slide's other text is bigger.
const SMALL_TEXT: f64 = 12.0;

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "deckrt.tidy",
        "Tidy This Slide",
        ["Home", "Drawing"],
        None,
        "{index?} → {changes: [text]}: lines up, sizes and spaces shapes, puts captions under their shapes, raises tiny text, brings shapes back on the slide and fits text that runs over; one undo step",
        has_slide,
        tidy
    )]
}

/// A shape the tidy may move: placed on the slide, upright, not a line.
fn movable(sh: &Shape) -> bool {
    sh.ph.is_none()
        && sh.xfrm.is_some_and(|x| x.rot == 0.0 && x.w > 1.0 && x.h > 1.0)
        && !matches!(sh.kind, ShapeKind::Connector { .. })
        && !sh.hidden
}

fn is_caption(sh: &Shape) -> bool {
    sh.text_box && sh.text.as_ref().is_some_and(|t| !t.text().trim().is_empty())
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    match v.len() {
        0 => 0.0,
        n if n % 2 == 1 => v[n / 2],
        n => (v[n / 2 - 1] + v[n / 2]) / 2.0,
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= SAME_SIZE * a.abs().max(b.abs())
}

/// Groups of indices whose `key` lies within [`NEAR`] of the group's first member, when their
/// `size` is alike too.
fn bands(items: &[(usize, Xfrm)], key: impl Fn(&Xfrm) -> f64, size: impl Fn(&Xfrm) -> f64) -> Vec<Vec<usize>> {
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by(|a, b| key(&items[*a].1).partial_cmp(&key(&items[*b].1)).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<Vec<usize>> = vec![];
    for i in order {
        let x = &items[i].1;
        match out.iter_mut().find(|g| {
            let f = &items[g[0]].1;
            (key(f) - key(x)).abs() <= NEAR && close(size(f), size(x))
        }) {
            Some(g) => g.push(i),
            None => out.push(vec![i]),
        }
    }
    out.retain(|g| g.len() >= 2);
    out
}

/// Line up, size and space a row (`horizontal`) or a column of shapes. Returns what changed.
fn arrange(items: &mut [(usize, Xfrm)], group: &[usize], horizontal: bool) -> Vec<&'static str> {
    let mut changed = vec![];
    let ws: Vec<f64> = group.iter().map(|i| items[*i].1.w).collect();
    let hs: Vec<f64> = group.iter().map(|i| items[*i].1.h).collect();
    let (mw, mh) = (median(ws.clone()), median(hs.clone()));
    let alike = ws.iter().all(|w| close(*w, mw)) && hs.iter().all(|h| close(*h, mh));
    let differ = ws.iter().any(|w| (w - mw).abs() > 0.5) || hs.iter().any(|h| (h - mh).abs() > 0.5);
    if alike && differ {
        for i in group {
            let x = &mut items[*i].1;
            if (x.w - mw).abs() > 0.5 || (x.h - mh).abs() > 0.5 {
                // Resize about the centre so the shape stays where it was.
                x.x += (x.w - mw) / 2.0;
                x.y += (x.h - mh) / 2.0;
                x.w = mw;
                x.h = mh;
            }
        }
        changed.push("same size");
    }
    let edge = median(group.iter().map(|i| if horizontal { items[*i].1.y } else { items[*i].1.x }).collect());
    if group.iter().any(|i| (if horizontal { items[*i].1.y } else { items[*i].1.x } - edge).abs() > 0.5) {
        for i in group {
            if horizontal {
                items[*i].1.y = edge;
            } else {
                items[*i].1.x = edge;
            }
        }
        changed.push("lined up");
    }
    if group.len() >= 3 {
        let mut g = group.to_vec();
        let pos = |x: &Xfrm| if horizontal { x.x } else { x.y };
        let len = |x: &Xfrm| if horizontal { x.w } else { x.h };
        g.sort_by(|a, b| pos(&items[*a].1).partial_cmp(&pos(&items[*b].1)).unwrap_or(std::cmp::Ordering::Equal));
        let (first, last) = (items[g[0]].1, items[g[g.len() - 1]].1);
        let total: f64 = g.iter().map(|i| len(&items[*i].1)).sum();
        let span = pos(&last) + len(&last) - pos(&first);
        let gap = (span - total) / (g.len() - 1) as f64;
        if gap > 0.0 {
            let mut at = pos(&first);
            let mut moved = false;
            for i in &g {
                let x = &mut items[*i].1;
                let p = if horizontal { &mut x.x } else { &mut x.y };
                if (*p - at).abs() > 0.5 {
                    moved = true;
                }
                *p = at;
                at += if horizontal { x.w } else { x.h } + gap;
            }
            if moved {
                changed.push("spaced evenly");
            }
        }
    }
    changed
}

/// The body text sizes set on a text body's runs.
fn sizes(sh: &Shape) -> Vec<f64> {
    sh.text.as_ref().map(|t| t.paragraphs.iter().flat_map(|p| p.runs.iter().filter_map(|r| r.props.size)).collect()).unwrap_or_default()
}

/// The tidied slide and what changed, worked out on a copy.
pub fn tidy_slide(doc: &Presentation, index: usize) -> Option<(Slide, Vec<String>)> {
    let src = doc.slides.get(index)?;
    let mut sl: Slide = (**src).clone();
    let (sw, shh) = (doc.slide_size.width, doc.slide_size.height);
    let mut notes: Vec<String> = vec![];
    let plural = |n: usize, one: &str, many: &str| if n == 1 { one.to_string() } else { many.replace('#', &n.to_string()) };

    // Shapes and captions.
    let shapes: Vec<(usize, Xfrm)> =
        sl.shapes.iter().enumerate().filter(|(_, s)| movable(s) && !is_caption(s)).filter_map(|(i, s)| s.xfrm.map(|x| (i, x))).collect();
    let mut items = shapes;
    let mut did: std::collections::BTreeMap<&'static str, usize> = std::collections::BTreeMap::new();
    for (horizontal, groups) in [(true, bands(&items, |x| x.y + x.h / 2.0, |x| x.h)), (false, bands(&items, |x| x.x + x.w / 2.0, |x| x.w))] {
        for g in groups {
            for what in arrange(&mut items, &g, horizontal) {
                *did.entry(what).or_default() += g.len();
            }
        }
    }
    for (i, x) in &items {
        if let Some(s) = sl.shapes.get_mut(*i) {
            s.xfrm = Some(*x);
        }
    }
    for (what, n) in &did {
        notes.push(match *what {
            "same size" => plural(*n, "Made 1 shape the same size as its neighbours", "Made # shapes the same size"),
            "lined up" => plural(*n, "Lined up 1 shape", "Lined up # shapes"),
            _ => plural(*n, "Evened the gaps around 1 shape", "Evened the gaps between # shapes"),
        });
    }

    // Captions: a text box just under a shape takes its width and a set gap.
    let anchors: Vec<Xfrm> = items.iter().map(|(_, x)| *x).collect();
    let mut taken: Vec<usize> = vec![];
    let mut captions = 0;
    let mut caption_ids: Vec<deckcraft_model::ShapeId> = vec![];
    for s in sl.shapes.iter_mut().filter(|s| movable(s) && is_caption(s)) {
        let Some(t) = s.xfrm else { continue };
        let best = anchors
            .iter()
            .enumerate()
            .filter(|(k, _)| !taken.contains(k))
            .filter(|(_, a)| {
                let gap = t.y - (a.y + a.h);
                let overlap = (t.x + t.w).min(a.x + a.w) - t.x.max(a.x);
                (-NEAR..=60.0).contains(&gap) && overlap >= 0.3 * t.w.min(a.w)
            })
            .min_by(|(_, a), (_, b)| ((t.y - a.y - a.h).abs()).partial_cmp(&(t.y - b.y - b.h).abs()).unwrap_or(std::cmp::Ordering::Equal));
        if let Some((k, a)) = best {
            taken.push(k);
            caption_ids.push(s.id);
            let want = Xfrm { x: a.x, y: a.y + a.h + CAPTION_GAP, w: a.w, ..t };
            if (want.x - t.x).abs() > 0.5 || (want.y - t.y).abs() > 0.5 || (want.w - t.w).abs() > 0.5 {
                s.xfrm = Some(want);
                captions += 1;
            }
        }
    }
    if captions > 0 {
        notes.push(plural(captions, "Put 1 caption under its shape", "Put # captions under their shapes"));
    }

    // Tiny text comes up: a caption to its fellow captions' size, anything else to 12 pt.
    let fellow: Vec<f64> = sl.shapes.iter().filter(|s| caption_ids.contains(&s.id)).flat_map(sizes).filter(|v| *v >= SMALL_TEXT).collect();
    let caption_size = if fellow.is_empty() { SMALL_TEXT } else { median(fellow) };
    {
        let mut raised = 0;
        let mut to = SMALL_TEXT;
        for s in sl.shapes.iter_mut().filter(|s| s.ph.is_none()) {
            let small = sizes(s).iter().any(|v| *v < SMALL_TEXT);
            if !small {
                continue;
            }
            let target = if caption_ids.contains(&s.id) { caption_size } else { SMALL_TEXT };
            to = to.max(target);
            if let Some(t) = s.text.as_mut() {
                for p in &mut t.paragraphs {
                    for r in &mut p.runs {
                        if r.props.size.is_some_and(|v| v < SMALL_TEXT) {
                            r.props.size = Some(target);
                        }
                    }
                    if p.end_props.size.is_some_and(|v| v < SMALL_TEXT) {
                        p.end_props.size = Some(target);
                    }
                }
                raised += 1;
            }
        }
        if raised > 0 {
            notes.push(plural(raised, &format!("Raised small text to {to:.0} pt in 1 text box"), &format!("Raised small text in # text boxes")));
        }
    }

    // Back onto the slide.
    if sw.is_finite() && shh.is_finite() && sw > MARGIN * 3.0 && shh > MARGIN * 3.0 {
        let mut back = 0;
        for s in sl.shapes.iter_mut().filter(|s| movable(s)) {
            let Some(mut x) = s.xfrm else { continue };
            let off = x.x < -0.5 || x.y < -0.5 || x.x + x.w > sw + 0.5 || x.y + x.h > shh + 0.5;
            if !off {
                continue;
            }
            x.w = x.w.min(sw - 2.0 * MARGIN);
            x.h = x.h.min(shh - 2.0 * MARGIN);
            // `w` and `h` leave a margin each side, so the low bound never passes the high one.
            x.x = x.x.max(MARGIN.min(x.x.max(0.0))).min(sw - MARGIN - x.w);
            x.y = x.y.max(MARGIN.min(x.y.max(0.0))).min(shh - MARGIN - x.h);
            s.xfrm = Some(x);
            back += 1;
        }
        if back > 0 {
            notes.push(plural(back, "Brought 1 shape back onto the slide", "Brought # shapes back onto the slide"));
        }
    }

    // Text that runs past its box: a text box grows when there is room, else the text shrinks.
    let measured = {
        let mut d = doc.clone();
        if let Some(slot) = d.slides.get_mut(index) {
            *slot = Arc::new(sl.clone());
        }
        let mut fixes = vec![];
        if let Some(cur) = d.slides.get(index)
            && let Some(ctx) = Ctx::for_slide(&d, cur)
        {
            for (i, s) in cur.shapes.iter().enumerate() {
                let Some(body) = &s.text else { continue };
                if !movable(s) || body.text().trim().is_empty() {
                    continue;
                }
                if matches!(resolve::body(&ctx, s).autofit, Some(AutoFit::Shrink { .. }) | Some(AutoFit::Shape)) {
                    continue;
                }
                let x = resolve::xfrm(&ctx, s);
                let rect = deckcraft_render::shape_geometry(s, x.w, x.h).text_rect;
                if rect.height() <= 1.0 {
                    continue;
                }
                let need = deckcraft_text::fit_height(&ctx, s, body, rect, &deckcraft_text::NoFields);
                if need > rect.height() + 2.0 {
                    let grow = need - rect.height() + 2.0;
                    let scale = (rect.height() / need).clamp(0.3, 1.0);
                    fixes.push((i, s.text_box && x.y + x.h + grow <= shh - MARGIN / 2.0, grow, scale));
                }
            }
        }
        fixes
    };
    let (mut grew, mut shrank) = (0, 0);
    for (i, room, grow, scale) in measured {
        let Some(s) = sl.shapes.get_mut(i) else { continue };
        if room && let Some(x) = s.xfrm.as_mut() {
            x.h += grow;
            grew += 1;
        } else if let Some(t) = s.text.as_mut() {
            t.body.autofit = Some(AutoFit::Shrink { font_scale: Some(scale), line_reduction: 0.0 });
            shrank += 1;
        }
    }
    if grew > 0 {
        notes.push(plural(grew, "Made 1 text box taller to fit its text", "Made # text boxes taller to fit their text"));
    }
    if shrank > 0 {
        notes.push(plural(shrank, "Set 1 shape's text to shrink to fit", "Set # shapes' text to shrink to fit"));
    }
    Some((sl, notes))
}

fn tidy(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "deckrt.tidy";
    let index = match p.get("index").and_then(Value::as_u64) {
        Some(i) => i as usize,
        None => s.doc()?.selection.slide,
    };
    let (slide, changes) = tidy_slide(&s.doc()?.doc, index).ok_or_else(|| bad(cmd, "no such slide"))?;
    if !changes.is_empty() {
        s.edit(|doc, _| {
            let slot = doc.slides.get_mut(index).ok_or_else(|| bad(cmd, "no such slide"))?;
            *slot = Arc::new(slide);
            Ok(())
        })?;
    }
    Ok(json!({"changes": changes}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xf(s: &Session, id: u64) -> Xfrm {
        let doc = &s.doc().expect("doc").doc;
        let sl = &doc.slides[s.doc().expect("doc").selection.slide];
        sl.shapes.iter().find(|sh| sh.id.0 as u64 == id).and_then(|sh| sh.xfrm).expect("shape")
    }

    #[test]
    fn a_messy_row_comes_out_tidy() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "titleOnly", "title": "Layers"})).expect("slide");
        let ids: Vec<u64> = [[52.0, 150.0, 250.0, 150.0], [361.0, 142.0, 232.0, 165.0], [641.0, 157.0, 260.0, 140.0]]
            .iter()
            .map(|r| s.execute("shape.insert", &json!({"preset": "roundRect", "rect": r})).expect("shape")["id"].as_u64().expect("id"))
            .collect();
        let cap = |s: &mut Session, r: [f64; 4], t: &str, size: f64| -> u64 {
            let id = s.execute("insert.textBox", &json!({"rect": r, "text": t})).expect("box")["id"].as_u64().expect("id");
            s.execute("format.size", &json!({"size": size})).expect("size");
            id
        };
        let c0 = cap(&mut s, [58.0, 330.0, 240.0, 80.0], "Epidermis: the outer layer.", 9.0);
        let c1 = cap(&mut s, [370.0, 322.0, 228.0, 80.0], "Dermis: collagen and elastin.", 14.0);
        let c2 = cap(&mut s, [700.0, 338.0, 420.0, 80.0], "Fat pads shrink and slide down with age.", 14.0);
        let r = s.execute("deckrt.tidy", &json!({})).expect("tidy");
        let changes: Vec<String> = r["changes"].as_array().expect("list").iter().filter_map(|v| v.as_str().map(String::from)).collect();
        assert!(!changes.is_empty());
        let b: Vec<Xfrm> = ids.iter().map(|i| xf(&s, *i)).collect();
        assert!(b.iter().all(|x| (x.w - b[0].w).abs() < 0.01 && (x.h - b[0].h).abs() < 0.01), "same size: {b:?}");
        assert!(b.iter().all(|x| (x.y - b[0].y).abs() < 0.01), "tops lined up: {b:?}");
        let g1 = b[1].x - (b[0].x + b[0].w);
        let g2 = b[2].x - (b[1].x + b[1].w);
        assert!((g1 - g2).abs() < 0.01, "even gaps {g1} {g2}");
        for (c, bx) in [(c0, b[0]), (c1, b[1]), (c2, b[2])] {
            let x = xf(&s, c);
            assert!((x.x - bx.x).abs() < 0.01 && (x.w - bx.w).abs() < 0.01, "caption under its box: {x:?} {bx:?}");
            assert!((x.y - (bx.y + bx.h + CAPTION_GAP)).abs() < 0.01);
        }
        let doc = &s.doc().expect("doc").doc;
        let sl = &doc.slides[1];
        let small = sl.shapes.iter().find(|sh| sh.id.0 as u64 == c0).map(sizes).unwrap_or_default();
        assert!(small.iter().all(|v| *v >= 14.0), "{small:?}");
        assert!(sl.shapes.iter().all(|sh| sh.xfrm.is_none_or(|x| x.x + x.w <= 960.0)), "nothing off the right edge");
        // One undo puts it all back.
        s.execute("edit.undo", &json!({})).expect("undo");
        assert_eq!(xf(&s, c2).x, 700.0);
        // A tidy slide stays as it is.
        s.execute("edit.redo", &json!({})).expect("redo");
        let again = s.execute("deckrt.tidy", &json!({})).expect("tidy");
        assert_eq!(again["changes"].as_array().map(|a| a.len()), Some(0), "{again}");
    }

    #[test]
    fn shapes_far_apart_are_left_alone() {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "blank"})).expect("slide");
        let a = s.execute("shape.insert", &json!({"preset": "rect", "rect": [50.0, 50.0, 100.0, 100.0]})).expect("a")["id"].as_u64().expect("id");
        let b = s.execute("shape.insert", &json!({"preset": "ellipse", "rect": [500.0, 300.0, 300.0, 60.0]})).expect("b")["id"].as_u64().expect("id");
        let r = s.execute("deckrt.tidy", &json!({})).expect("tidy");
        assert_eq!(r["changes"].as_array().map(|a| a.len()), Some(0), "{r}");
        assert_eq!(xf(&s, a).x, 50.0);
        assert_eq!(xf(&s, b).y, 300.0);
    }
}
