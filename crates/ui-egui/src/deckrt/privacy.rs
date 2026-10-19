//! deckrt: the check before sharing (the same list as the check before a talk, with its own
//! findings) and the Blur Area tool, which blurs the part of a photo dragged over, for good.

use egui::{Color32, CornerRadius, Pos2, Rect, Stroke, StrokeKind, Ui};
use serde_json::json;

use crate::SlideApp;
use crate::canvas::Xf;

pub const DIALOG: &str = "deckrt.privacy";

/// Blur Area waiting for a drag over one picture.
pub struct BlurTool {
    pub target: u32,
    start: Option<Pos2>,
    cur: Option<Pos2>,
}

/// The two buttons on the Review tab.
pub fn ribbon_group(app: &mut SlideApp, ui: &mut Ui) {
    crate::widgets::group_named(ui, "Sharing", |ui| {
        let en = app.session.active().is_some_and(|d| !d.doc.slides.is_empty());
        if crate::widgets::big_button(ui, crate::icons::Icon::Lock, "Check Before\nSharing", en)
            .on_hover_text("Find cropped-away parts, photo details, hidden text and notes that go with the file")
            .clicked()
            && let Ok(r) = app.session.execute("deckrt.privacy", &json!({}))
        {
            let mut d = crate::dialogs::Dialog::new(DIALOG);
            d.params = json!({"result": r, "cmd": "deckrt.privacy", "when": "sharing"});
            app.dialog = Some(d);
        }
        let picture = selected_picture(app);
        let on = app.deckrt.blur.is_some();
        if crate::widgets::big_button_ex(ui, crate::icons::Icon::ArtisticEffects, "Blur\nArea", picture.is_some() || on, false, on)
            .0
            .on_hover_text("Select a picture, then drag over the part to blur. It can't be unblurred from the file.")
            .clicked()
        {
            if on {
                app.deckrt.blur = None;
            } else if let Some(id) = picture {
                app.deckrt.blur = Some(BlurTool { target: id, start: None, cur: None });
                app.deckrt.blur_doc = app.session.active().map(|d| d.uid);
                app.set_status("Drag over the part of the picture to blur. Esc stops.");
            }
        }
    });
}

fn selected_picture(app: &SlideApp) -> Option<u32> {
    let st = app.session.active()?;
    let sh = st.selected_shapes().first().copied()?;
    matches!(sh.kind, deckcraft_model::ShapeKind::Picture { .. }).then_some(sh.id.0)
}

fn picture_rect(app: &SlideApp, id: u32, xf: Xf) -> Option<Rect> {
    let st = app.session.active()?;
    let sh = st.shape(deckcraft_model::ShapeId(id))?;
    let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh);
    Some(Rect::from_min_max(xf.to_screen(deckcraft_geom::Point::new(x.x, x.y)), xf.to_screen(deckcraft_geom::Point::new(x.x + x.w, x.y + x.h))))
}

/// The canvas pointer while Blur Area is on. Returns whether it took the event.
pub fn pointer(app: &mut SlideApp, ui: &Ui, resp: &egui::Response, xf: Xf) -> bool {
    let Some(tool) = app.deckrt.blur.as_ref() else { return false };
    // Another deck came forward: the picture picked belongs to the first one.
    if app.session.active().map(|d| d.uid) != app.deckrt.blur_doc {
        app.deckrt.blur = None;
        return false;
    }
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        app.deckrt.blur = None;
        app.set_status("");
        return true;
    }
    let target = tool.target;
    let Some(rect) = picture_rect(app, target, xf) else {
        app.deckrt.blur = None;
        return false;
    };
    let pos = resp.interact_pointer_pos().or(resp.hover_pos());
    let pressed = ui.input(|i| i.pointer.primary_pressed());
    let down = ui.input(|i| i.pointer.primary_down());
    let Some(tool) = app.deckrt.blur.as_mut() else { return false };
    if pressed && let Some(p) = pos {
        if rect.contains(p) {
            tool.start = Some(p);
            tool.cur = Some(p);
        } else {
            // A press off the picture ends the tool.
            app.deckrt.blur = None;
            return false;
        }
    } else if down && tool.start.is_some() {
        if let Some(p) = ui.input(|i| i.pointer.latest_pos()) {
            tool.cur = Some(rect.clamp(p));
        }
        ui.ctx().request_repaint();
    } else if !down && let (Some(a), Some(b)) = (tool.start, tool.cur) {
        tool.start = None;
        tool.cur = None;
        let r = Rect::from_two_pos(a, b);
        if r.width() > 4.0 && r.height() > 4.0 {
            let frac = [
                ((r.min.x - rect.min.x) / rect.width()) as f64,
                ((r.min.y - rect.min.y) / rect.height()) as f64,
                (r.width() / rect.width()) as f64,
                (r.height() / rect.height()) as f64,
            ];
            match app.session.execute("deckrt.blurArea", &json!({"id": target, "rect": frac})) {
                Ok(_) => app.set_status("Blurred. Undo brings it back until the file is saved and closed."),
                Err(e) => app.set_status(e.to_string()),
            }
            app.drain_requests();
        }
    }
    true
}

/// The picture outlined and the area being dragged.
pub fn draw(app: &SlideApp, painter: &egui::Painter, xf: Xf) {
    let Some(tool) = app.deckrt.blur.as_ref() else { return };
    let Some(rect) = picture_rect(app, tool.target, xf) else { return };
    painter.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(2.0, Color32::from_rgb(0xff, 0xb6, 0x4d)), StrokeKind::Outside);
    if let (Some(a), Some(b)) = (tool.start, tool.cur) {
        let r = Rect::from_two_pos(a, b);
        painter.rect_filled(r, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(255, 182, 77, 60));
        painter.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.5, Color32::from_rgb(0xff, 0xb6, 0x4d)), StrokeKind::Inside);
    }
}
