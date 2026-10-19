//! deckrt: previews without starting the show.
//!
//! - Transitions › Preview plays the slide's transition on the editor canvas, from the slide
//!   before it, as PowerPoint does. Upstream opened the slide show for it.
//! - AutoPreview (on by default, under the Preview buttons): picking a transition plays it, and
//!   picking an animation plays that shape's effects on the canvas.
//! - Hovering a tile in the Transitions or Animations gallery plays the effect inside the tile on
//!   a small card, so the effect can be seen before it is picked.

use std::sync::Arc;

use deckcraft_anim::{Source, Timeline};
use deckcraft_model::anim::{AnimClass, AnimStart, Animation};
use deckcraft_model::{Presentation, ShapeId};
use deckcraft_render::{RenderOpts, ShapeState};
use egui::{Color32, CornerRadius, Pos2, Rect, TextureHandle, Ui, pos2, vec2};

use crate::SlideApp;
use crate::canvas::Xf;
use crate::show::{part_textures, quad_mesh, state_for};

/// A transition playing on the editor canvas.
pub struct TransPreview {
    uid: u64,
    slide: usize,
    slide_ptr: usize,
    kind: String,
    option: String,
    dur: f64,
    from: Option<usize>,
    started: Option<f64>,
    old: Option<TextureHandle>,
    new: Option<TextureHandle>,
    parts: Option<[Option<TextureHandle>; 4]>,
    morph: Option<TextureHandle>,
}

/// How long the finished slide stays after the transition before the canvas is back to editing.
const HOLD: f64 = 0.4;

pub fn active(app: &SlideApp) -> bool {
    app.deckrt.trans_preview.is_some()
}

pub fn stop(app: &mut SlideApp) {
    app.deckrt.trans_preview = None;
}

/// Transitions › Preview: play the current slide's transition on the canvas.
pub fn start_transition(app: &mut SlideApp) {
    let Some(d) = app.session.active() else { return };
    if d.selection.target != deckcraft_engine::Target::Slides {
        return;
    }
    let slide = d.selection.slide;
    let Some(sl) = d.doc.slides.get(slide) else { return };
    let Some(tr) = sl.transition.as_ref().filter(|t| t.kind != "none") else {
        app.set_status("This slide has no transition.");
        return;
    };
    let dur = if tr.duration_ms > 0 { tr.duration_ms as f64 / 1000.0 } else { 1.0 };
    let p = TransPreview {
        uid: d.uid,
        slide,
        slide_ptr: Arc::as_ptr(sl) as usize,
        kind: tr.kind.clone(),
        option: tr.option.clone(),
        dur,
        from: slide.checked_sub(1),
        started: None,
        old: None,
        new: None,
        parts: None,
        morph: None,
    };
    app.anim_preview = None;
    app.deckrt.trans_preview = Some(p);
}

/// After a transition is picked: preview it when AutoPreview is on.
pub fn auto_transition(app: &mut SlideApp) {
    if super::prefs::get(app).auto_preview {
        start_transition(app);
    }
}

/// After an animation is picked: play the selected shapes' effects on the canvas when
/// AutoPreview is on.
pub fn auto_animation(app: &mut SlideApp) {
    if !super::prefs::get(app).auto_preview {
        return;
    }
    let Some(d) = app.session.active() else { return };
    if d.selection.target != deckcraft_engine::Target::Slides {
        return;
    }
    let slide = d.selection.slide;
    let Some(sl) = d.doc.slides.get(slide) else { return };
    let ids = d.selection.shapes.clone();
    let mut only = (**sl).clone();
    only.animations.retain(|a| ids.contains(&a.shape));
    if only.animations.is_empty() {
        return;
    }
    // One click plays them all.
    for (i, a) in only.animations.iter_mut().enumerate() {
        if i > 0 && a.start == AnimStart::OnClick {
            a.start = AnimStart::AfterPrevious;
        }
    }
    let timeline = Timeline::new(&only, d.doc.slide_size.width, d.doc.slide_size.height);
    let ptr = Arc::as_ptr(sl) as usize;
    let uid = d.uid;
    app.deckrt.trans_preview = None;
    app.anim_preview = Some(crate::AnimPreview { uid, slide, slide_ptr: ptr, started: None, elapsed: 0.0, timeline });
}

/// The Preview button's arrow: the AutoPreview switch.
pub fn auto_preview_menu(app: &mut SlideApp, ui: &mut Ui) {
    let mut on = super::prefs::get(app).auto_preview;
    if ui.checkbox(&mut on, "AutoPreview").on_hover_text("Play an effect on the slide when you pick it").changed() {
        let mut p = super::prefs::get(app).clone();
        p.auto_preview = on;
        super::prefs::set(app, p);
    }
}

fn slide_rect(doc: &Presentation, xf: Xf) -> Rect {
    Rect::from_min_size(xf.origin, vec2(doc.slide_size.width as f32 * xf.scale, doc.slide_size.height as f32 * xf.scale))
}

/// On the canvas, after the slide is drawn: the transition's frame over it.
pub fn draw(app: &mut SlideApp, ui: &Ui, painter: &egui::Painter, xf: Xf) {
    let Some(mut p) = app.deckrt.trans_preview.take() else { return };
    let Some(d) = app.session.active() else { return };
    let alive = d.uid == p.uid
        && d.selection.target == deckcraft_engine::Target::Slides
        && d.selection.slide == p.slide
        && d.doc.slides.get(p.slide).is_some_and(|s| Arc::as_ptr(s) as usize == p.slide_ptr)
        && app.show.is_none();
    if !alive {
        return;
    }
    let doc = d.doc.clone();
    let ctx = ui.ctx().clone();
    let now = ui.input(|i| i.time);
    let start = *p.started.get_or_insert(now);
    let t = ((now - start) / p.dur.max(0.01)).max(0.0);
    if t > 1.0 + HOLD / p.dur.max(0.01) {
        return;
    }
    let rect = slide_rect(&doc, xf);
    let ppp = ctx.pixels_per_point();
    let w = ((rect.width() * ppp).round() as u32).clamp(16, 1920);
    let size = (w, ((w as f64) * doc.slide_size.height / doc.slide_size.width.max(1.0)).round().max(1.0) as u32);
    let scale = size.0 as f64 / doc.slide_size.width.max(1.0);
    let threads = if cfg!(target_arch = "wasm32") { 0 } else { std::thread::available_parallelism().map(|n| n.get().min(8) as u16).unwrap_or(0) };
    let tl = deckcraft_anim::ShowState::timeline(&doc, p.slide);
    let first = |id: ShapeId| state_for(&tl, id, 0, 0.0, &[], &[]);
    if p.new.is_none() {
        let opts = RenderOpts { scale, size: Some(size), state: Some(&first), threads, ..Default::default() };
        let img = deckcraft_render::render_slide(&doc, p.slide, &opts);
        p.new = Some(crate::textures::upload(&ctx, "deckrt-trans-new", crate::textures::to_color_image(&img, false)));
        if let Some(f) = p.from {
            let opts = RenderOpts { scale, size: Some(size), threads, ..Default::default() };
            let img = deckcraft_render::render_slide(&doc, f, &opts);
            p.old = Some(crate::textures::upload(&ctx, "deckrt-trans-old", crate::textures::to_color_image(&img, false)));
        }
    }
    let progress = t.min(1.0);
    painter.rect_filled(rect, CornerRadius::ZERO, Color32::BLACK);
    if p.kind == "morph"
        && let Some(from) = p.from
        && let (Some(a), Some(b)) = (doc.slides.get(from), doc.slides.get(p.slide))
    {
        let hidden_b = |id: ShapeId| {
            let st = first(id);
            !st.visible || st.opacity <= 0.0
        };
        let m = deckcraft_anim::morph_frame(&doc, a, b, progress, &hidden_b);
        let f = |id: ShapeId| ShapeState { opacity: m.opacity_of(id), ..Default::default() };
        let opts = RenderOpts { scale, size: Some(size), state: Some(&f), threads, ..Default::default() };
        let text: Vec<deckcraft_render::TextMorph> = m
            .text
            .iter()
            .map(|x| deckcraft_render::TextMorph { old: x.old, new: x.new, from: x.from, to: x.to, chars: x.chars, t: m.mix })
            .collect();
        let paths: Vec<deckcraft_render::PathMorph> = m
            .paths
            .iter()
            .map(|x| deckcraft_render::PathMorph { id: x.id, from: x.from.clone(), from_box: x.from_box, to_box: x.to_box, t: m.mix })
            .collect();
        let img = deckcraft_render::render_blend(&doc, from, p.slide, m.mix, &m.shapes, &text, &paths, &opts);
        let ci = crate::textures::to_color_image(&img, false);
        let frame = match p.morph.take() {
            Some(mut h) => {
                crate::textures::replace(&ctx, &mut h, ci);
                h
            }
            None => crate::textures::upload(&ctx, "deckrt-morph", ci),
        };
        painter.image(frame.id(), rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        p.morph = Some(frame);
    } else if let Some(new) = p.new.clone() {
        if p.parts.is_none() && deckcraft_anim::uses_slide_parts(&p.kind, &p.option) {
            p.parts = Some(part_textures(&ctx, &doc, p.from, p.slide, size));
        }
        let aspect = doc.slide_size.width / doc.slide_size.height.max(1.0);
        let part = |k: usize| p.parts.as_ref().and_then(|x| x.get(k).cloned().flatten());
        let parts = [part(0), part(1), part(2), part(3)];
        for l in deckcraft_anim::transition_frame(&p.kind, &p.option, progress, aspect) {
            let old_or_black = |tex: Option<&TextureHandle>| match tex {
                Some(o) => quad_mesh(Some(o.id()), rect, &l, Color32::WHITE),
                None => quad_mesh(None, rect, &l, Color32::BLACK),
            };
            let mesh = match l.source {
                Source::Old => old_or_black(p.old.as_ref()),
                Source::New => quad_mesh(Some(new.id()), rect, &l, Color32::WHITE),
                Source::Black => quad_mesh(None, rect, &l, Color32::BLACK),
                Source::White => quad_mesh(None, rect, &l, Color32::WHITE),
                Source::OldBack => old_or_black(parts[0].as_ref().or(p.old.as_ref())),
                Source::NewBack => quad_mesh(Some(parts[2].as_ref().unwrap_or(&new).id()), rect, &l, Color32::WHITE),
                Source::OldContent => match &parts[1] {
                    Some(t) => quad_mesh(Some(t.id()), rect, &l, Color32::WHITE),
                    None => continue,
                },
                Source::NewContent => match &parts[3] {
                    Some(t) => quad_mesh(Some(t.id()), rect, &l, Color32::WHITE),
                    None => continue,
                },
            };
            painter.add(egui::Shape::mesh(mesh));
        }
    }
    ctx.request_repaint();
    app.deckrt.trans_preview = Some(p);
}

// ---------- Gallery tiles that play on hover ----------

/// The pointer has been over `r` since this time, for one tile at a time.
fn hover_since(p: &egui::Painter, r: Rect) -> Option<f64> {
    let ctx = p.ctx();
    let over = ctx.pointer_hover_pos().is_some_and(|pos| r.contains(pos) && p.clip_rect().contains(pos));
    let key = egui::Id::new("deckrt-tile-hover");
    let tile = (r.min.x.round() as i32, r.min.y.round() as i32);
    let now = ctx.input(|i| i.time);
    let mut cur: Option<((i32, i32), f64)> = ctx.data(|d| d.get_temp(key)).flatten();
    if over {
        if cur.is_none_or(|(t, _)| t != tile) {
            cur = Some((tile, now));
            ctx.data_mut(|d| d.insert_temp(key, cur));
        }
        cur.map(|(_, since)| since)
    } else {
        if cur.is_some_and(|(t, _)| t == tile) {
            ctx.data_mut(|d| d.insert_temp::<Option<((i32, i32), f64)>>(key, None));
        }
        None
    }
}

/// Two little slides for a tile: the old one grey, the new one blue. The slides' contents
/// (used by transitions that move them apart from the background) are left out.
fn card(p: &egui::Painter, area: Rect, l: &deckcraft_anim::Layer) {
    let back = match l.source {
        Source::Old | Source::OldBack => Color32::from_rgb(0xC9, 0xCE, 0xD6),
        Source::New | Source::NewBack => Color32::from_rgb(0x2E, 0x6F, 0xD8),
        Source::Black => Color32::BLACK,
        Source::White => Color32::WHITE,
        Source::OldContent | Source::NewContent => return,
    };
    p.add(egui::Shape::mesh(quad_mesh(None, area, l, back)));
}

/// A Transitions gallery tile while the pointer is over it: the effect plays between two small
/// slides, over and over. False when the tile isn't hovered (upstream draws it as before).
pub fn transition_tile(p: &egui::Painter, r: Rect, id: &str, label: &str) -> bool {
    if id == "none" {
        return false;
    }
    let Some(since) = hover_since(p, r) else { return false };
    let now = p.ctx().input(|i| i.time);
    let cycle = 2.2;
    let t = (((now - since) % cycle) / 1.4).clamp(0.0, 1.0);
    let pic = Rect::from_min_size(r.min + vec2(4.0, 4.0), vec2(r.width() - 8.0, r.height() - 20.0));
    let aspect = 16.0 / 9.0;
    let (w, h) = if pic.width() / pic.height() > aspect { (pic.height() * aspect, pic.height()) } else { (pic.width(), pic.width() / aspect) };
    let area = Rect::from_center_size(pic.center(), vec2(w, h));
    let clipped = p.with_clip_rect(area.intersect(p.clip_rect()));
    clipped.rect_filled(area, CornerRadius::ZERO, Color32::BLACK);
    for l in deckcraft_anim::transition_frame(id, "", t, aspect as f64) {
        card(&clipped, area, &l);
    }
    let tok = crate::theme::Tokens::get(p.ctx());
    p.text(pos2(r.center().x, r.max.y - 7.0), egui::Align2::CENTER_CENTER, label, crate::theme::font(10.0), tok.text);
    p.ctx().request_repaint();
    true
}

/// An Animations gallery tile while the pointer is over it: the effect plays on a small shape.
pub fn animation_tile(p: &egui::Painter, r: Rect, label: &str, class: AnimClass, enabled: bool) -> bool {
    if !enabled {
        return false;
    }
    let Some(info) = deckcraft_model::anim::ANIMATIONS.iter().find(|a| a.1 == label && a.2 == class) else { return false };
    let Some(since) = hover_since(p, r) else { return false };
    let now = p.ctx().input(|i| i.time);
    // A small slide with one shape and the effect on it, built once a frame (cheap).
    let mut slide = deckcraft_model::Slide::default();
    let id = ShapeId(1);
    slide.shapes.push(deckcraft_model::Shape {
        id,
        xfrm: Some(deckcraft_geom::Xfrm { x: 26.0, y: 31.0, w: 48.0, h: 38.0, ..Default::default() }),
        ..Default::default()
    });
    let mut a = Animation {
        shape: id,
        class,
        effect: info.0.to_string(),
        option: info.5.first().map(|o| o.to_string()).unwrap_or_default(),
        start: AnimStart::WithPrevious,
        duration_ms: info.4.clamp(300, 2000),
        preset_id: Some(info.3),
        ..Default::default()
    };
    if class == AnimClass::Path {
        a.path = Some(deckcraft_engine::cmd::animation::default_path(&a.effect, &a.option));
    }
    match a.effect.as_str() {
        "spin" => a.amount = Some(360.0),
        "growShrink" => a.amount = Some(1.5),
        _ => {}
    }
    let dur = a.duration_ms as f64 / 1000.0;
    slide.animations.push(a);
    let tl = Timeline::new(&slide, 100.0, 100.0);
    let cycle = 0.3 + dur + 0.8;
    let t = ((now - since) % cycle - 0.3).max(0.0);
    let st = state_for(&tl, id, 0, t, &[], &[]);
    // The tile's picture area is the 100 × 100 slide.
    let pic = Rect::from_min_size(r.min + vec2(2.0, 2.0), vec2(r.width() - 4.0, r.height() - 18.0));
    let side = pic.width().min(pic.height());
    let area = Rect::from_center_size(pic.center(), vec2(side, side));
    let k = side / 100.0;
    let tok = crate::theme::Tokens::get(p.ctx());
    let clipped = p.with_clip_rect(pic.intersect(p.clip_rect()));
    if st.visible && st.opacity > 0.0 {
        let base = st.tint.map(crate::theme::to_color32).unwrap_or(tok.accent);
        let col = base.gamma_multiply(st.opacity.clamp(0.0, 1.0) as f32);
        let (sw, sh) = (48.0 * st.scale.0, 38.0 * st.scale.1);
        let c = pos2(area.min.x + ((50.0 + st.offset.x) as f32) * k, area.min.y + ((50.0 + st.offset.y) as f32) * k);
        // The reveal clip, in the shape's own box.
        let [cl, ct, cr, cb] = st.clip.unwrap_or([0.0, 0.0, 0.0, 0.0]);
        let (x0, y0, x1, y1) = (-0.5 + cl, -0.5 + ct, 0.5 - cr, 0.5 - cb);
        if x1 > x0 && y1 > y0 {
            let rot = (st.rotate as f32).to_radians();
            let (s, co) = rot.sin_cos();
            let pt = |u: f64, v: f64| -> Pos2 {
                let (dx, dy) = ((u * sw) as f32 * k, (v * sh) as f32 * k);
                pos2(c.x + dx * co - dy * s, c.y + dx * s + dy * co)
            };
            let pts = vec![pt(x0, y0), pt(x1, y0), pt(x1, y1), pt(x0, y1)];
            clipped.add(egui::Shape::convex_polygon(pts, col, egui::Stroke::NONE));
        }
    }
    p.text(pos2(r.center().x, r.max.y - 8.0), egui::Align2::CENTER_CENTER, label, crate::theme::font(10.0), tok.text);
    p.ctx().request_repaint();
    true
}

/// Every animation as a tile, by category, in a list that scrolls to fit the screen. A click runs
/// `cmd` (`animation.set` replaces the shape's effect, `animation.add` adds one) and previews it.
pub fn effect_gallery(app: &mut SlideApp, ui: &mut Ui, cmd: &str) {
    let t = crate::theme::Tokens::get(ui.ctx());
    let room = (ui.ctx().content_rect().height() - 220.0).clamp(240.0, 640.0);
    let mut picked = None;
    egui::ScrollArea::vertical().max_height(room).show(ui, |ui| {
        for (cls, label) in [(AnimClass::Entrance, "Entrance"), (AnimClass::Emphasis, "Emphasis"), (AnimClass::Exit, "Exit"), (AnimClass::Path, "Motion Paths")] {
            ui.label(egui::RichText::new(label).font(crate::theme::bold(12.0)));
            ui.horizontal_wrapped(|ui| {
                for a in deckcraft_model::anim::ANIMATIONS.iter().filter(|a| a.2 == cls) {
                    let (r, resp) = ui.allocate_exact_size(vec2(56.0, 54.0), egui::Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(r, CornerRadius::same(4), t.hover);
                    }
                    crate::ribbon::paint_anim_tile(ui.painter(), r.shrink(2.0), a.1, a.2, true);
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, a.1));
                    if resp.on_hover_text(a.1).clicked() {
                        picked = Some((a.0, a.2));
                    }
                }
            });
        }
    });
    if let Some((effect, class)) = picked {
        let _ = app.run(cmd, serde_json::json!({"effect": effect, "class": class.xml()}));
        auto_animation(app);
        ui.close();
    }
}
