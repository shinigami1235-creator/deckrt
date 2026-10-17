//! The interactive objects on the editor canvas: the live 3D model over its poster, a badge on
//! every other object, the turn handle of a selected model, and the last click (where a new
//! object lands).

use deckcraft_interactive::object::{self as obj, Config, Kind};
use deckcraft_model::ShapeKind;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Stroke, Ui, pos2, vec2};

use crate::SlideApp;
use crate::canvas::Xf;
use crate::theme::{self, Tokens};

/// Radius of the turn handle in the middle of a selected model, in screen points.
const HANDLE_R: f32 = 16.0;

fn screen_rect(xf: Xf, x: &deckcraft_geom::Xfrm) -> Rect {
    Rect::from_min_max(xf.to_screen(deckcraft_geom::Point::new(x.x, x.y)), xf.to_screen(deckcraft_geom::Point::new(x.x + x.w, x.y + x.h)))
}

/// The shapes the canvas draws itself: the live 3D models (the slide texture leaves them out),
/// and while the slide's objects play, the pictures and boxes the show draws itself.
pub fn live_shapes(app: &SlideApp) -> Vec<deckcraft_model::ShapeId> {
    let Some(st) = app.session.active() else { return vec![] };
    if st.selection.target != deckcraft_engine::Target::Slides {
        return vec![];
    }
    let mut v: Vec<deckcraft_model::ShapeId> = st.shapes().iter().filter(|s| matches!(s.kind, ShapeKind::Model3d { .. })).map(|s| s.id).collect();
    if let Some(p) = &app.deckrt.playing
        && p.uid == st.uid
        && p.slide == st.selection.slide
    {
        v.extend(super::show::hidden_in_show(&st.doc, st.selection.slide));
    }
    v
}

/// The slide's objects running in the editor, as they do in the show, after a double-click on
/// one. Esc, a click off the slide or another slide stops them.
pub struct Playing {
    uid: u64,
    slide: usize,
    ext: super::show::Ext,
}

impl Playing {
    pub fn new(uid: u64, slide: usize) -> Playing {
        Playing { uid, slide, ext: Default::default() }
    }
}

/// The slide's box on screen and the show's map for it.
fn slide_map(xf: Xf, doc: &deckcraft_model::Presentation) -> super::show::Map {
    let a = xf.to_screen(deckcraft_geom::Point::new(0.0, 0.0));
    let b = xf.to_screen(deckcraft_geom::Point::new(doc.slide_size.width, doc.slide_size.height));
    super::show::Map::new(Rect::from_min_max(a, b), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), doc)
}

/// Pointer input while the objects play: they take every event on the slide.
fn playing_pointer(app: &mut SlideApp, ui: &Ui, resp: &egui::Response, xf: Xf) -> bool {
    let Some(st) = app.session.active() else { return false };
    let (uid, slide, doc) = (st.uid, st.selection.slide, st.doc.clone());
    let Some(mut p) = app.deckrt.playing.take() else { return false };
    if p.uid != uid || p.slide != slide || st.selection.target != deckcraft_engine::Target::Slides {
        return false;
    }
    let map = slide_map(xf, &doc);
    let pos = resp.interact_pointer_pos().or(resp.hover_pos());
    let stop = ui.input(|i| i.key_pressed(egui::Key::Escape))
        || (ui.input(|i| i.pointer.primary_pressed()) && pos.is_some_and(|q| !map.srect.contains(q)));
    if stop {
        app.set_status("");
        ui.ctx().request_repaint();
        return true;
    }
    let now = ui.input(|i| i.time);
    let mut zoom = None;
    super::show::pointer(ui.ctx(), map, &doc, slide, &mut p.ext, &mut zoom, super::show::Role::Audience, now, resp.hovered());
    app.deckrt.playing = Some(p);
    ui.ctx().request_repaint();
    true
}

/// A double-click on an object other than a model starts the slide's objects playing.
fn start_playing(app: &mut SlideApp, ui: &Ui, resp: &egui::Response, xf: Xf) -> bool {
    if !resp.hovered() || !ui.input(|i| i.pointer.button_double_clicked(egui::PointerButton::Primary)) {
        return false;
    }
    let Some(p) = resp.interact_pointer_pos().or(resp.hover_pos()) else { return false };
    let Some(st) = app.session.active() else { return false };
    if st.selection.target != deckcraft_engine::Target::Slides {
        return false;
    }
    let hit = st.shapes().iter().rev().any(|s| {
        !s.hidden
            && obj::read_any(s).is_some_and(|o| !matches!(o.kind(), Kind::Model))
            && screen_rect(xf, &deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, s)).contains(p)
    });
    if !hit {
        return false;
    }
    app.deckrt.playing = Some(Playing { uid: st.uid, slide: st.selection.slide, ext: Default::default() });
    app.set_status("Playing the slide's objects. Esc stops.");
    ui.ctx().request_repaint();
    true
}

/// The selected model's box on screen, when one is selected.
fn selected_model(app: &SlideApp, xf: Xf) -> Option<(u32, Rect)> {
    let st = app.session.active()?;
    let sh = st.selected_shapes().first().copied()?;
    if !matches!(sh.kind, ShapeKind::Model3d { .. }) {
        return None;
    }
    let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh);
    Some((sh.id.0, screen_rect(xf, &x)))
}

/// Pointer input the objects take before the slide does: a drag on a selected model's turn
/// handle orbits it, the wheel over a selected model zooms it. Every primary press is
/// remembered as the last click. Returns whether the event was consumed.
pub fn pointer(app: &mut SlideApp, ui: &Ui, resp: &egui::Response, xf: Xf) -> bool {
    if super::privacy::pointer(app, ui, resp, xf) {
        return true;
    }
    if app.deckrt.playing.is_some() {
        return playing_pointer(app, ui, resp, xf);
    }
    if start_playing(app, ui, resp, xf) {
        return true;
    }
    let pos = resp.interact_pointer_pos().or(resp.hover_pos());
    if resp.hovered()
        && ui.input(|i| i.pointer.primary_pressed())
        && let Some(p) = pos
    {
        let sp = xf.to_slide(p);
        app.deckrt.last_click = Some((sp.x, sp.y));
    }
    let Some((id, rect)) = selected_model(app, xf) else {
        app.deckrt.turning = None;
        return false;
    };
    // A turn in progress follows the pointer wherever it goes.
    if let Some((tid, last)) = app.deckrt.turning {
        if tid != id || ui.input(|i| !i.pointer.primary_down()) {
            app.deckrt.turning = None;
            return true;
        }
        if let Some(p) = ui.input(|i| i.pointer.latest_pos()) {
            let d = p - last;
            let t = app.deckrt.models.turns.entry(id).or_default();
            t.turn[0] += (d.x / rect.width().max(1.0)) as f64 * 240.0;
            t.turn[1] += (d.y / rect.height().max(1.0)) as f64 * 150.0;
            app.deckrt.turning = Some((id, p));
            ui.ctx().request_repaint();
        }
        return true;
    }
    let Some(p) = pos else { return false };
    let on_handle = p.distance(rect.center()) <= HANDLE_R + 2.0;
    if on_handle && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if on_handle && resp.hovered() && ui.input(|i| i.pointer.primary_pressed()) {
        app.deckrt.turning = Some((id, p));
        return true;
    }
    if resp.hovered() && rect.contains(p) {
        let wheel = ui.input(|i| i.smooth_scroll_delta.y);
        if wheel.abs() > 0.0 {
            let t = app.deckrt.models.turns.entry(id).or_default();
            let z = if t.zoom > 0.0 { t.zoom } else { 1.0 };
            t.zoom = (z * (1.0 + wheel as f64 / 300.0)).clamp(0.2, 8.0);
            ui.ctx().request_repaint();
            return true;
        }
    }
    false
}

/// Draw the objects of the current slide over the slide texture.
pub fn draw(app: &mut SlideApp, ui: &mut Ui, painter: &egui::Painter, xf: Xf) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    if st.selection.target != deckcraft_engine::Target::Slides {
        return;
    }
    let doc = st.doc.clone();
    let selected: Vec<u32> = st.selection.shapes.iter().map(|s| s.0).collect();
    let items: Vec<(deckcraft_model::Shape, obj::InteractiveObject, deckcraft_geom::Xfrm)> = st
        .shapes()
        .iter()
        .filter_map(|s| obj::read_any(s).map(|o| (s.clone(), o, deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, s))))
        .collect();
    let ppp = ui.ctx().pixels_per_point();
    for (sh, o, x) in items {
        let rect = screen_rect(xf, &x);
        if sh.hidden {
            continue;
        }
        if let (ShapeKind::Model3d { media, .. }, Config::Model(c)) = (&sh.kind, &o.config) {
            let turn = app.deckrt.turn(sh.id.0);
            let turning = app.deckrt.turning.is_some_and(|(id, _)| id == sh.id.0);
            if let Some(model) = app.deckrt.models.model(&doc, *media) {
                let view = super::model_view::view_of(&model, c, &turn);
                let w = (rect.width() * ppp).round().max(1.0) as u32;
                let h = (rect.height() * ppp).round().max(1.0) as u32;
                let hidden = c.hidden_layers.clone();
                let drawn = app.deckrt.models.draw(ui.ctx(), &mut app.deckrt.gpu, sh.id.0 as u64, *media, &model, &view, w, h, &hidden, turning);
                if let Some(d) = drawn {
                    // The slide texture leaves the model out (`live_shapes`), so the slide shows
                    // through the model's transparent background.
                    super::model_view::Models::paint(painter, &d, rect);
                }
            }
            if selected.contains(&sh.id.0) {
                turn_handle(painter, rect.center(), &t, !turn.is_zero());
            }
        } else {
            badge(painter, rect, &o, &t);
        }
    }
    // The objects playing, over their pictures.
    if let Some(mut p) = app.deckrt.playing.take() {
        if let Some(st) = app.session.active()
            && p.uid == st.uid
            && p.slide == st.selection.slide
        {
            let doc = st.doc.clone();
            let map = slide_map(xf, &doc);
            let now = ui.input(|i| i.time);
            super::show::draw(ui.ctx(), painter, map, &doc, p.slide, &mut p.ext, &mut app.deckrt, super::show::Role::Audience, now);
            let label = painter.layout_no_wrap("Playing · Esc stops".into(), egui::FontId::proportional(12.0), Color32::WHITE);
            let r = Rect::from_min_size(map.srect.min + vec2(8.0, 8.0), label.size() + vec2(14.0, 8.0));
            painter.rect_filled(r, CornerRadius::same(5), Color32::from_rgba_unmultiplied(232, 106, 28, 230));
            painter.galley(r.min + vec2(7.0, 4.0), label, Color32::WHITE);
            app.deckrt.playing = Some(p);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(30));
        }
    }
    super::privacy::draw(app, painter, xf);
}

/// The round handle a selected model is turned with.
fn turn_handle(p: &egui::Painter, c: Pos2, t: &Tokens, turned: bool) {
    p.circle(c, HANDLE_R, Color32::from_rgba_unmultiplied(255, 255, 255, 210), Stroke::new(1.5, t.accent));
    // Two arcs with arrow heads: "turn".
    let n = 14;
    let arc = |from: f32, to: f32| -> Vec<Pos2> {
        (0..=n)
            .map(|i| {
                let a = from + (to - from) * i as f32 / n as f32;
                pos2(c.x + 9.0 * a.cos(), c.y + 9.0 * a.sin())
            })
            .collect()
    };
    let col = if turned { t.accent } else { t.text };
    for (from, to) in [(0.3_f32, 2.6_f32), (3.44, 5.74)] {
        let pts = arc(from, to);
        p.add(egui::Shape::line(pts.clone(), Stroke::new(1.6, col)));
        if let (Some(a), Some(b)) = (pts.get(n - 1), pts.last()) {
            let d = (*b - *a).normalized();
            let nrm = vec2(-d.y, d.x);
            p.add(egui::Shape::convex_polygon(vec![*b + d * 3.0, *b - d * 2.0 + nrm * 3.0, *b - d * 2.0 - nrm * 3.0], col, Stroke::NONE));
        }
    }
}

/// A small label in the corner of an object that has no live picture of its own in the editor.
fn badge(p: &egui::Painter, rect: Rect, o: &obj::InteractiveObject, t: &Tokens) {
    let text = match &o.config {
        Config::ImageSwap(c) => format!("Image swap · {} pictures", c.images.len()),
        Config::Scrub(c) => format!("Scrub · {} pictures", c.images.len()),
        Config::Wipe(_) => "Wipe".to_string(),
        Config::Zoom(_) => "Zoom region".to_string(),
        Config::Popup(_) => "Popup".to_string(),
        Config::Poll(_) => "Poll".to_string(),
        Config::Measure(c) => format!("Measure · {}", c.preset.label()),
        Config::InjectionMap(_) => "Injection map".to_string(),
        Config::Model(_) => "3D model".to_string(),
    };
    let invisible = matches!(o.kind(), Kind::Zoom | Kind::Measure | Kind::InjectionMap);
    if invisible {
        let dash = egui::Shape::dashed_line(
            &[rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom(), rect.left_top()],
            Stroke::new(1.2, t.accent),
            5.0,
            4.0,
        );
        p.extend(dash);
    }
    let font = theme::font(11.0);
    let galley = p.layout_no_wrap(text, font, Color32::WHITE);
    let size = galley.size() + vec2(10.0, 4.0);
    let at = Rect::from_min_size(rect.left_top() + vec2(2.0, 2.0), size);
    p.rect_filled(at, CornerRadius::same(3), t.accent);
    p.galley(at.min + vec2(5.0, 2.0), galley, Color32::WHITE);
    let _ = Align2::LEFT_TOP;
}
