//! deckrt: the notes in presenter view. The notes get a panel of their own with large text,
//! a divider between the columns and one under the next slide to drag, a button that hides the
//! next slide, and Ctrl + mouse wheel to size the text. The layout and size are kept between
//! shows (`prefs`).

use egui::{Align2, Color32, CornerRadius, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, Ui, pos2, vec2};

use crate::SlideApp;
use crate::show::Show;

/// The presenter view's layout while a show runs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub split: f32,
    pub next_share: f32,
    pub hide_next: bool,
    pub fold_phones: bool,
}

impl Default for Layout {
    fn default() -> Self {
        let p = super::prefs::Prefs::default();
        Layout { split: p.notes_split, next_share: p.next_share, hide_next: p.hide_next, fold_phones: p.fold_phones }
    }
}

pub const NOTES_MIN: f32 = 12.0;
pub const NOTES_MAX: f32 = 72.0;
const SPLIT: std::ops::RangeInclusive<f32> = 0.3..=0.8;
const NEXT: std::ops::RangeInclusive<f32> = 0.12..=0.7;

/// When the show starts: the layout and notes size from the settings.
pub fn start(app: &mut SlideApp, show: &mut Show) {
    if show.ext.layout_loaded {
        return;
    }
    show.ext.layout_loaded = true;
    let p = super::prefs::get(app).clone();
    show.ext.layout = Layout { split: p.notes_split, next_share: p.next_share, hide_next: p.hide_next, fold_phones: p.fold_phones };
    show.notes_px = p.notes_px.clamp(NOTES_MIN, NOTES_MAX);
}

/// When the show ends: keep the layout and size for the next one.
pub fn end(app: &mut SlideApp, show: &Show) {
    if !show.ext.layout_loaded {
        return;
    }
    let mut p = super::prefs::get(app).clone();
    let l = show.ext.layout;
    p.notes_split = l.split;
    p.next_share = l.next_share;
    p.hide_next = l.hide_next;
    p.fold_phones = l.fold_phones;
    p.notes_px = show.notes_px;
    super::prefs::set(app, p);
}

fn resize(show: &mut Show, px: f32) {
    if px.is_finite() {
        show.notes_px = px.clamp(NOTES_MIN, NOTES_MAX);
    }
}

/// A divider: a thin line that lights up under the pointer. Returns where it was dragged to.
fn divider(ui: &mut Ui, rect: Rect, id: &str, vertical: bool) -> Option<Pos2> {
    let r = ui.interact(rect, ui.id().with(id), Sense::drag());
    let lit = r.hovered() || r.dragged();
    if lit {
        ui.ctx().set_cursor_icon(if vertical { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical });
    }
    let c = rect.center();
    let line = if vertical { [pos2(c.x, rect.min.y), pos2(c.x, rect.max.y)] } else { [pos2(rect.min.x, c.y), pos2(rect.max.x, c.y)] };
    ui.painter().line_segment(line, Stroke::new(if lit { 3.0 } else { 1.0 }, Color32::from_gray(if lit { 150 } else { 70 })));
    if r.dragged() { r.interact_pointer_pos() } else { None }
}

/// The two columns of the presenter view, with the divider between them.
pub fn columns(ui: &mut Ui, show: &mut Show, avail: Rect) -> (Rect, Rect) {
    let split = show.ext.layout.split.clamp(*SPLIT.start(), *SPLIT.end());
    let x = avail.min.x + avail.width() * split;
    let left = Rect::from_min_max(avail.min, pos2(x, avail.max.y));
    let right = Rect::from_min_max(pos2(x + 16.0, avail.min.y), avail.max);
    let bar = Rect::from_min_max(pos2(x + 2.0, avail.min.y), pos2(x + 14.0, avail.max.y));
    if let Some(p) = divider(ui, bar, "deckrt-split", true)
        && avail.width() > 1.0
    {
        show.ext.layout.split = ((p.x - 8.0 - avail.min.x) / avail.width()).clamp(*SPLIT.start(), *SPLIT.end());
    }
    (left, right)
}

/// A dark button with light text (the presenter window is dark).
fn button(ui: &mut Ui, text: &str, size: f32) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).size(size).color(Color32::WHITE)).fill(Color32::from_gray(60)).min_size(vec2(0.0, 28.0)))
}

/// The right column: the next slide, the Phones section and the notes.
pub fn right_column(ui: &mut Ui, show: &mut Show, doc: &deckcraft_model::Presentation, right: Rect, next_tex: Option<&TextureHandle>, notes: &str) {
    let mut y = right.min.y;
    // The next slide, with the button that hides it.
    let head = Rect::from_min_size(pos2(right.min.x, y), vec2(right.width(), 28.0));
    let mut hui = ui.new_child(egui::UiBuilder::new().max_rect(head).layout(egui::Layout::left_to_right(egui::Align::Center)));
    if !show.ext.layout.hide_next {
        hui.label(RichText::new("Next").size(13.0).color(Color32::from_gray(170)));
    }
    hui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let hide = show.ext.layout.hide_next;
        if button(ui, if hide { "Show next slide" } else { "Hide next slide" }, 13.0).clicked() {
            show.ext.layout.hide_next = !hide;
        }
    });
    y += 32.0;
    if !show.ext.layout.hide_next {
        let share = show.ext.layout.next_share.clamp(*NEXT.start(), *NEXT.end());
        let area = Rect::from_min_size(pos2(right.min.x, y), vec2(right.width(), ((right.max.y - y) * share).max(40.0)));
        let aspect = (doc.slide_size.width / doc.slide_size.height.max(1.0)) as f32;
        let (w, h) =
            if area.width() / area.height() > aspect { (area.height() * aspect, area.height()) } else { (area.width(), area.width() / aspect) };
        let nr = Rect::from_min_size(area.min, vec2(w, h));
        match next_tex {
            Some(t) => {
                ui.painter().image(t.id(), nr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            None => {
                ui.painter().rect_filled(nr, CornerRadius::ZERO, Color32::from_gray(40));
                ui.painter().text(nr.center(), Align2::CENTER_CENTER, "End of slide show", crate::theme::font(13.0), Color32::from_gray(170));
            }
        }
        y = nr.max.y + 2.0;
        let bar = Rect::from_min_max(pos2(right.min.x, y), pos2(right.max.x, y + 10.0));
        if let Some(p) = divider(ui, bar, "deckrt-next", false) {
            let room = (right.max.y - right.min.y - 32.0).max(1.0);
            // The share is of the width-fitted height, so turn the pointer's height back into it.
            let want_h = (p.y - right.min.y - 32.0).max(1.0);
            let fit_h = right.width() / aspect;
            let h = want_h.min(fit_h);
            show.ext.layout.next_share = (h / room).clamp(*NEXT.start(), *NEXT.end());
        }
        y += 12.0;
    }
    // The Phones section, or one line when folded away.
    if show.presenter && !show.reading {
        let head = Rect::from_min_size(pos2(right.min.x, y), vec2(right.width(), 28.0));
        let mut pui = ui.new_child(egui::UiBuilder::new().max_rect(head).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let n = show.ext.phones.as_ref().map_or(0, |p| p.connected());
        let fold = show.ext.layout.fold_phones;
        if fold {
            pui.label(RichText::new(format!("Phones: {n} connected")).size(13.0).color(Color32::from_gray(170)));
        }
        pui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if button(ui, if fold { "Show phones" } else { "Hide phones" }, 13.0).clicked() {
                show.ext.layout.fold_phones = !fold;
            }
        });
        if fold {
            y += 34.0;
        } else {
            let used = super::show::presenter_phones(ui, show, Rect::from_min_max(pos2(right.min.x, y), pos2(right.max.x - 150.0, right.max.y)));
            y += used.max(34.0);
        }
    }
    // The notes: a heading with the size buttons, then the text in a panel of its own.
    let head = Rect::from_min_size(pos2(right.min.x, y), vec2(right.width(), 30.0));
    let mut hui = ui.new_child(egui::UiBuilder::new().max_rect(head).layout(egui::Layout::left_to_right(egui::Align::Center)));
    hui.label(RichText::new("Notes").size(15.0).strong().color(Color32::from_gray(210)));
    hui.add_space(10.0);
    if button(&mut hui, "A−", 15.0).on_hover_text("Smaller notes ( [ )").clicked() {
        resize(show, show.notes_px - 4.0);
    }
    hui.label(RichText::new(format!("{} pt", show.notes_px.round() as i32)).size(13.0).color(Color32::from_gray(200)));
    if button(&mut hui, "A+", 15.0).on_hover_text("Bigger notes ( ] )").clicked() {
        resize(show, show.notes_px + 4.0);
    }
    y += 36.0;
    let panel = Rect::from_min_max(pos2(right.min.x, y), right.max);
    if panel.height() < 24.0 {
        return;
    }
    ui.painter().rect_filled(panel, CornerRadius::same(6), Color32::from_gray(34));
    // Ctrl + mouse wheel over the notes sizes the text.
    if ui.rect_contains_pointer(panel) {
        let z = ui.input(|i| i.zoom_delta());
        if (z - 1.0).abs() > 1e-3 {
            resize(show, show.notes_px * z);
        }
    }
    let inner = panel.shrink(14.0);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    let px = show.notes_px;
    egui::ScrollArea::vertical().id_salt("deckrt-notes").auto_shrink([false, false]).show(&mut child, |ui| {
        if notes.trim().is_empty() {
            ui.label(RichText::new("No notes on this slide.").size(px * 0.8).color(Color32::from_gray(150)));
            return;
        }
        for (i, para) in notes.split('\n').enumerate() {
            if i > 0 {
                ui.add_space(px * 0.35);
            }
            let text = if para.is_empty() { " " } else { para };
            ui.add(egui::Label::new(RichText::new(text).size(px).line_height(Some(px * 1.35)).color(Color32::from_gray(240))).wrap());
        }
    });
}
