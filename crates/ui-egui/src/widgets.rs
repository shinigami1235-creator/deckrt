//! Ribbon widgets: large and small buttons, split buttons, groups, dropdowns, spinners, the colour
//! picker and galleries.

use deckcraft_color::{ColorScheme, Rgba, SchemeSlot};
use deckcraft_model::ColorRef;
use egui::{Align2, Color32, CornerRadius, Rect, Response, Sense, Stroke, Ui, Vec2, pos2, vec2};

use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};

/// Height of the ribbon's content area.
pub const RIBBON_H: f32 = 66.0;

fn bg(ui: &Ui, r: &Response, selected: bool, t: &Tokens) -> Option<Color32> {
    if !r.sense.senses_click() && !r.sense.senses_drag() {
        return None;
    }
    if selected {
        Some(t.selected)
    } else if r.is_pointer_button_down_on() {
        Some(t.pressed)
    } else if r.hovered() && ui.is_enabled() {
        Some(t.hover)
    } else {
        None
    }
}

/// A large ribbon button: 32 px icon over a one- or two-line label.
pub fn big_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool) -> Response {
    big_button_ex(ui, icon, label, enabled, false, false).0
}

/// Large button; with `split`, the label row is a separate dropdown target (returns its response).
pub fn big_button_ex(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, split: bool, selected: bool) -> (Response, Option<Response>) {
    let t = Tokens::get(ui.ctx());
    let font = theme::font(11.0);
    let lines: Vec<&str> = wrap_label(label);
    let text_w = lines.iter().map(|l| ui.fonts_mut(|f| f.layout_no_wrap(l.to_string(), font.clone(), t.text).size().x)).fold(0.0f32, f32::max);
    let w = (text_w + 10.0 + if split && lines.len() == 1 { 10.0 } else { 0.0 }).max(40.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, RIBBON_H - 2.0), if enabled { Sense::click() } else { Sense::hover() });
    // Named for screen readers and UI tests ("Ink to\nShape" reads "Ink to Shape").
    let spoken = label.replace('\n', " ");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &spoken));
    let painter = ui.painter_at(rect.expand(1.0));
    let (main_rect, arrow_rect) = if split {
        (Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + 38.0)), Some(Rect::from_min_max(pos2(rect.min.x, rect.min.y + 38.0), rect.max)))
    } else {
        (rect, None)
    };
    let main_resp =
        if split { ui.interact(main_rect, resp.id.with("main"), if enabled { Sense::click() } else { Sense::hover() }) } else { resp.clone() };
    let arrow_resp = arrow_rect.map(|r| ui.interact(r, resp.id.with("arrow"), if enabled { Sense::click() } else { Sense::hover() }));
    if let Some(c) = bg(ui, &main_resp, selected, &t) {
        painter.rect_filled(if split { main_rect } else { rect }, CornerRadius::same(4), c);
    }
    if let (Some(ar), Some(a)) = (arrow_rect, &arrow_resp)
        && let Some(c) = bg(ui, a, false, &t)
    {
        painter.rect_filled(ar, CornerRadius::same(4), c);
    }
    let icon_rect = Rect::from_center_size(pos2(rect.center().x, rect.min.y + 19.0), vec2(30.0, 30.0));
    icons::paint(&painter, icon_rect, icon, t.text, !enabled);
    let col = if enabled { t.text } else { t.text_faint };
    let mut y = rect.min.y + 39.0;
    for (i, l) in lines.iter().enumerate() {
        let last = i + 1 == lines.len();
        let label_w = ui.fonts_mut(|f| f.layout_no_wrap(l.to_string(), font.clone(), col).size().x);
        let extra = if split && last { 9.0 } else { 0.0 };
        let x = rect.center().x - (label_w + extra) / 2.0;
        painter.text(pos2(x, y), Align2::LEFT_TOP, l, font.clone(), col);
        if split && last {
            chevron(&painter, pos2(x + label_w + 5.0, y + 7.0), 3.0, col);
        }
        y += 12.5;
    }
    (if split { main_resp } else { resp }, arrow_resp)
}

fn wrap_label(label: &str) -> Vec<&str> {
    if let Some((a, b)) = label.split_once('\n') {
        return vec![a, b];
    }
    let words: Vec<&str> = label.split(' ').collect();
    if words.len() >= 2 && label.chars().count() > 8 {
        // Break near the middle.
        let mut best = 1;
        let mut best_diff = usize::MAX;
        for k in 1..words.len() {
            let a: usize = words.iter().take(k).map(|w| w.chars().count() + 1).sum();
            let b: usize = words.iter().skip(k).map(|w| w.chars().count() + 1).sum();
            let d = a.abs_diff(b);
            if d < best_diff {
                best_diff = d;
                best = k;
            }
        }
        let split_at: usize = words.iter().take(best).map(|w| w.len() + 1).sum::<usize>().saturating_sub(1);
        if let (Some(a), Some(b)) = (label.get(..split_at), label.get(split_at + 1..)) {
            return vec![a, b];
        }
    }
    vec![label]
}

/// deckrt: an upward chevron (gallery scroll arrows).
pub fn chevron_up(p: &egui::Painter, c: egui::Pos2, s: f32, col: Color32) {
    p.line_segment([pos2(c.x - s, c.y + s * 0.5), pos2(c.x, c.y - s * 0.5)], Stroke::new(1.2, col));
    p.line_segment([pos2(c.x, c.y - s * 0.5), pos2(c.x + s, c.y + s * 0.5)], Stroke::new(1.2, col));
}

pub fn chevron(p: &egui::Painter, c: egui::Pos2, s: f32, col: Color32) {
    p.line_segment([pos2(c.x - s, c.y - s * 0.5), pos2(c.x, c.y + s * 0.5)], Stroke::new(1.2, col));
    p.line_segment([pos2(c.x, c.y + s * 0.5), pos2(c.x + s, c.y - s * 0.5)], Stroke::new(1.2, col));
}

/// A small ribbon button: 16 px icon + label in one row (label may be empty).
pub fn small_button(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, selected: bool) -> Response {
    small_button_ex(ui, icon, label, enabled, selected, false).0
}

pub fn small_button_ex(ui: &mut Ui, icon: Icon, label: &str, enabled: bool, selected: bool, split: bool) -> (Response, Option<Response>) {
    let t = Tokens::get(ui.ctx());
    let font = theme::font(12.0);
    let tw = if label.is_empty() { 0.0 } else { ui.fonts_mut(|f| f.layout_no_wrap(label.to_string(), font.clone(), t.text).size().x) + 5.0 };
    let arrow_w = if split { 12.0 } else { 0.0 };
    let w = 24.0 + tw + arrow_w;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 22.0), if enabled { Sense::click() } else { Sense::hover() });
    if !label.is_empty() {
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    }
    let painter = ui.painter_at(rect);
    let main_rect = Rect::from_min_max(rect.min, pos2(rect.max.x - arrow_w, rect.max.y));
    let main_resp =
        if split { ui.interact(main_rect, resp.id.with("m"), if enabled { Sense::click() } else { Sense::hover() }) } else { resp.clone() };
    let arrow_resp = split.then(|| {
        ui.interact(
            Rect::from_min_max(pos2(rect.max.x - arrow_w, rect.min.y), rect.max),
            resp.id.with("a"),
            if enabled { Sense::click() } else { Sense::hover() },
        )
    });
    if let Some(c) = bg(ui, &main_resp, selected, &t) {
        painter.rect_filled(main_rect, CornerRadius::same(4), c);
    }
    if let Some(a) = &arrow_resp
        && let Some(c) = bg(ui, a, false, &t)
    {
        painter.rect_filled(Rect::from_min_max(pos2(rect.max.x - arrow_w, rect.min.y), rect.max), CornerRadius::same(4), c);
    }
    icons::paint(&painter, Rect::from_center_size(pos2(rect.min.x + 12.0, rect.center().y), vec2(17.0, 17.0)), icon, t.text, !enabled);
    if !label.is_empty() {
        painter.text(pos2(rect.min.x + 24.0, rect.center().y), Align2::LEFT_CENTER, label, font, if enabled { t.text } else { t.text_faint });
    }
    if split {
        chevron(&painter, pos2(rect.max.x - 6.0, rect.center().y), 2.8, t.text_dim);
    }
    (if split { main_resp } else { resp }, arrow_resp)
}

/// A square icon toggle (B, I, U, alignment…).
pub fn icon_toggle(ui: &mut Ui, icon: Icon, tip: &str, enabled: bool, on: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(24.0, 22.0), if enabled { Sense::click() } else { Sense::hover() });
    let painter = ui.painter_at(rect);
    if let Some(c) = bg(ui, &resp, on, &t) {
        painter.rect_filled(rect, CornerRadius::same(4), c);
    }
    icons::paint(&painter, rect.shrink(3.0), icon, t.text, !enabled);
    // Named for screen readers (and tests) by its tooltip.
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, tip));
    resp.on_hover_text(tip)
}

/// Icon with a colour bar under it and a dropdown arrow (Font Color, Highlight, Shape Fill…).
pub fn color_split(ui: &mut Ui, icon: Icon, tip: &str, color: Color32, enabled: bool) -> (Response, Response) {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(34.0, 22.0), Sense::hover());
    let main = ui.interact(Rect::from_min_size(rect.min, vec2(23.0, 22.0)), resp.id.with("m"), if enabled { Sense::click() } else { Sense::hover() });
    let arrow = ui.interact(
        Rect::from_min_max(pos2(rect.min.x + 23.0, rect.min.y), rect.max),
        resp.id.with("a"),
        if enabled { Sense::click() } else { Sense::hover() },
    );
    let painter = ui.painter_at(rect);
    if let Some(c) = bg(ui, &main, false, &t) {
        painter.rect_filled(main.rect, CornerRadius::same(4), c);
    }
    if let Some(c) = bg(ui, &arrow, false, &t) {
        painter.rect_filled(arrow.rect, CornerRadius::same(4), c);
    }
    icons::paint(&painter, Rect::from_min_size(rect.min + vec2(3.0, 1.0), vec2(17.0, 15.0)), icon, t.text, !enabled);
    painter.rect_filled(
        Rect::from_min_size(rect.min + vec2(4.0, 17.0), vec2(15.0, 3.5)),
        CornerRadius::ZERO,
        if enabled { color } else { t.text_faint },
    );
    chevron(&painter, pos2(rect.max.x - 5.5, rect.center().y), 2.6, t.text_dim);
    (main.on_hover_text(tip), arrow)
}

/// A ribbon group: contents, then a thin vertical separator. deckrt hook: while the ribbon is
/// being drawn (`fit_begin` to `fit_end`), a group that would run past the window's edge moves
/// into the More menu at the right end, rightmost groups first.
pub fn group<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    group_ex(ui, "", false, add).0
}

/// deckrt: a group with its name under it, as PowerPoint's ribbon has. A name starting with `+`
/// carries on the group before it (one name over both, no line between them).
pub fn group_named<R>(ui: &mut Ui, name: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    group_ex(ui, name, false, add).0
}

/// deckrt: a named group with the small arrow in its corner that opens the group's dialog or pane.
/// Returns whether the arrow was clicked.
pub fn group_launcher<R>(ui: &mut Ui, name: &str, add: impl FnOnce(&mut Ui) -> R) -> (R, bool) {
    group_ex(ui, name, true, add)
}

/// Height of the row of group names under the ribbon's buttons.
pub const GROUP_LABEL_H: f32 = 19.0;

/// One group's place in the ribbon, for drawing the names and lines once every group is placed.
#[derive(Clone)]
struct Span {
    x0: f32,
    x1: f32,
    sep: f32,
    name: String,
    join: bool,
}

fn group_ex<R>(ui: &mut Ui, name: &str, launcher: bool, add: impl FnOnce(&mut Ui) -> R) -> (R, bool) {
    let ctx = ui.ctx().clone();
    let join = name.starts_with('+');
    let shown_name = name.trim_start_matches('+').to_string();
    let Some(mut fit) = ctx.data(|d| d.get_temp::<Fit>(egui::Id::new(FIT_ID))) else {
        let r = ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
            add(ui)
        });
        separator(ui);
        return (r.inner, false);
    };
    let n = fit.next;
    fit.next += 1;
    let wkey = egui::Id::new(("ribbon-group-width", fit.tab, n));
    let hidden = fit.hide_from.is_some_and(|k| n >= k);
    let label_font = theme::font(11.0);
    let label_w = if shown_name.is_empty() || join {
        0.0
    } else {
        ui.fonts_mut(|f| f.layout_no_wrap(shown_name.clone(), label_font.clone(), Color32::WHITE).size().x) + if launcher { 30.0 } else { 12.0 }
    };
    let mut launched = false;
    let r = if !hidden {
        let x0 = ui.cursor().min.x;
        let top = ui.max_rect().min.y;
        let r = ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
            add(ui)
        });
        let content = ui.cursor().min.x - x0;
        if content < label_w {
            ui.add_space(label_w - content);
        }
        let x1 = ui.cursor().min.x;
        if launcher {
            launched = launcher_button(ui, pos2(x1 - 9.0, top + RIBBON_H + GROUP_LABEL_H / 2.0 + 1.0), &shown_name);
        }
        let (gap, _) = ui.allocate_exact_size(vec2(13.0, RIBBON_H - 6.0), Sense::hover());
        fit.spans.push(Span { x0, x1, sep: gap.center().x, name: shown_name.clone(), join });
        fit.label_y = top + RIBBON_H;
        let w = ui.cursor().min.x - x0;
        ctx.data_mut(|d| d.insert_temp(wkey, w));
        r.inner
    } else if fit.open {
        // In the More menu: each hidden group in a card of its own, stacked under the button.
        let w = ctx.data(|d| d.get_temp::<f32>(wkey)).unwrap_or(200.0);
        let pos = pos2((fit.right - w).max(8.0), fit.y);
        let area = egui::Area::new(egui::Id::new(("ribbon-more-group", fit.tab, n))).order(egui::Order::Foreground).fixed_pos(pos).show(&ctx, |ui| {
            let t = Tokens::get(ui.ctx());
            egui::Frame::popup(ui.style()).fill(t.card).show(ui, |ui| {
                ui.set_min_height(RIBBON_H);
                let r = ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing = vec2(1.0, 1.0);
                    add(ui)
                });
                if !shown_name.is_empty() {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&shown_name).font(label_font.clone()).color(t.text_dim));
                        if launcher && launcher_button(ui, ui.cursor().min + vec2(10.0, 8.0), &shown_name) {
                            launched = true;
                        }
                    });
                }
                r.inner
            })
        });
        let rect = area.response.rect;
        fit.y = rect.max.y + 4.0;
        fit.menu_rects.push(rect);
        area.inner.inner
    } else {
        // Out of sight: drawn where nothing shows and nothing can be clicked, so the group's
        // result still comes back to its caller.
        let w = ctx.data(|d| d.get_temp::<f32>(wkey)).unwrap_or(1.0).max(1.0);
        let mut ghost = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(pos2(-20_000.0, -20_000.0), vec2(w, RIBBON_H))).invisible());
        ghost.horizontal_top(add).inner
    };
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(FIT_ID), fit));
    (r, launched)
}

/// The small arrow in a group's corner (Font, Paragraph, Drawing…), centred on `c`.
fn launcher_button(ui: &mut Ui, c: egui::Pos2, name: &str) -> bool {
    let t = Tokens::get(ui.ctx());
    let rect = Rect::from_center_size(c, vec2(15.0, 15.0));
    let resp = ui.interact(rect, ui.id().with(("group-launcher", name)), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("{name} options")));
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, CornerRadius::same(2), t.hover);
    }
    let r = rect.shrink(4.0);
    let s = Stroke::new(1.0, t.text_dim);
    // A corner bracket with an arrow pointing into it, PowerPoint's dialog launcher.
    p.line_segment([r.left_top(), pos2(r.left(), r.bottom() - 1.0)], s);
    p.line_segment([r.left_top(), pos2(r.right() - 1.0, r.top())], s);
    p.line_segment([pos2(r.left() + 2.0, r.top() + 2.0), r.right_bottom()], s);
    p.line_segment([r.right_bottom(), pos2(r.right() - 3.5, r.bottom())], s);
    p.line_segment([r.right_bottom(), pos2(r.right(), r.bottom() - 3.5)], s);
    resp.on_hover_text(format!("{name} options")).clicked()
}

/// Draw the group names and the lines between groups, once all of them are placed.
fn paint_spans(ui: &Ui, spans: &[Span], label_y: f32, top: f32) {
    let t = Tokens::get(ui.ctx());
    let p = ui.painter();
    let mut i = 0;
    while i < spans.len() {
        let mut j = i;
        while j + 1 < spans.len() && spans[j + 1].join {
            j += 1;
        }
        let (x0, x1) = (spans[i].x0, spans[j].x1);
        if !spans[i].name.is_empty() {
            p.text(pos2((x0 + x1) / 2.0, label_y + GROUP_LABEL_H / 2.0 + 1.0), Align2::CENTER_CENTER, &spans[i].name, theme::font(11.0), t.text_dim);
        }
        let sx = spans[j].sep;
        p.line_segment([pos2(sx, top + 4.0), pos2(sx, label_y + GROUP_LABEL_H - 3.0)], Stroke::new(1.0, t.separator));
        i = j + 1;
    }
}

const FIT_ID: &str = "deckrt-ribbon-fit";
const MORE_W: f32 = 52.0;

/// The ribbon's fitting for one frame: which groups go into the More menu.
#[derive(Clone, Default)]
struct Fit {
    tab: u64,
    next: usize,
    hide_from: Option<usize>,
    open: bool,
    right: f32,
    y: f32,
    menu_rects: Vec<Rect>,
    spans: Vec<Span>,
    label_y: f32,
}

fn tab_key(tab: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    tab.hash(&mut h);
    h.finish()
}

/// Start fitting the groups of `tab` into `inner`, from the widths they took when last drawn.
pub fn fit_begin(ui: &Ui, tab: &str, inner: Rect) {
    let ctx = ui.ctx();
    let key = tab_key(tab);
    let count = ctx.data(|d| d.get_temp::<usize>(egui::Id::new(("ribbon-group-count", key)))).unwrap_or(0);
    let widths: Vec<Option<f32>> = (0..count).map(|n| ctx.data(|d| d.get_temp::<f32>(egui::Id::new(("ribbon-group-width", key, n))))).collect();
    let total: f32 = widths.iter().map(|w| w.unwrap_or(0.0)).sum();
    let hide_from = if widths.iter().all(Option::is_some) && total > inner.width() {
        let mut x = 0.0;
        let mut k = 0;
        for (n, w) in widths.iter().enumerate() {
            x += w.unwrap_or(0.0);
            if x + MORE_W > inner.width() {
                k = n;
                break;
            }
        }
        Some(k)
    } else {
        None
    };
    let open = hide_from.is_some() && ctx.data(|d| d.get_temp::<bool>(egui::Id::new(("ribbon-more-open", key)))).unwrap_or(false);
    let fit = Fit {
        tab: key,
        next: 0,
        hide_from,
        open,
        right: inner.max.x,
        y: inner.max.y + GROUP_LABEL_H + 12.0,
        menu_rects: vec![],
        spans: vec![],
        label_y: inner.max.y,
    };
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(FIT_ID), fit));
}

/// End the fitting: the More button, and the menu closing on a click elsewhere.
pub fn fit_end(ui: &mut Ui, tab: &str, inner: Rect) {
    let ctx = ui.ctx().clone();
    let key = tab_key(tab);
    let Some(fit) = ctx.data_mut(|d| d.remove_temp::<Fit>(egui::Id::new(FIT_ID))) else { return };
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(("ribbon-group-count", key)), fit.next));
    paint_spans(ui, &fit.spans, fit.label_y, inner.min.y);
    let open_key = egui::Id::new(("ribbon-more-open", key));
    if fit.hide_from.is_none() {
        ctx.data_mut(|d| d.remove_temp::<bool>(open_key));
        return;
    }
    let rect = Rect::from_min_max(pos2(inner.max.x - MORE_W, inner.min.y), pos2(inner.max.x, inner.min.y + RIBBON_H - 2.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    let resp = big_button_ex(&mut child, Icon::More, "More", true, false, fit.open).0.on_hover_text("The groups that don't fit in the window");
    let mut open = fit.open;
    if resp.clicked() {
        open = !open;
    } else if open
        && ctx.input(|i| i.pointer.any_pressed())
        && let Some(p) = ctx.input(|i| i.pointer.interact_pos())
        && !fit.menu_rects.iter().any(|r| r.contains(p))
        && !rect.contains(p)
        // A drop-down opened from a group in the menu is a popup of its own.
        && !ctx.layer_id_at(p).is_some_and(|l| matches!(l.order, egui::Order::Foreground | egui::Order::Tooltip | egui::Order::Debug))
    {
        open = false;
    }
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        open = false;
    }
    ctx.data_mut(|d| d.insert_temp(open_key, open));
}

pub fn separator(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(13.0, RIBBON_H - 6.0), Sense::hover());
    ui.painter().line_segment([pos2(rect.center().x, rect.min.y + 6.0), pos2(rect.center().x, rect.max.y - 4.0)], Stroke::new(1.0, t.separator));
}

/// Two or three rows of small controls stacked in one ribbon column.
pub fn rows<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(1.0, 0.0);
        ui.add_space(0.0);
        add(ui)
    })
    .inner
}

/// A text dropdown field (font name, size): shows `current`, opens a popup with `add_items`.
pub fn dropdown(ui: &mut Ui, id: &str, current: &str, width: f32, enabled: bool, add_items: impl FnOnce(&mut Ui)) -> Response {
    dropdown_wide(ui, id, current, width, width.max(140.0), enabled, add_items)
}

/// deckrt: a dropdown whose list is wider than its field (the font list, so names fit on a line).
pub fn dropdown_wide(ui: &mut Ui, id: &str, current: &str, width: f32, list_width: f32, enabled: bool, add_items: impl FnOnce(&mut Ui)) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 22.0), if enabled { Sense::click() } else { Sense::hover() });
    let painter = ui.painter_at(rect);
    painter.rect(
        rect,
        CornerRadius::same(4),
        t.field_bg,
        Stroke::new(1.0, if resp.hovered() { t.text_faint } else { t.border }),
        egui::StrokeKind::Inside,
    );
    let clip = rect.shrink2(vec2(6.0, 0.0));
    painter.with_clip_rect(Rect::from_min_max(clip.min, pos2(clip.max.x - 12.0, clip.max.y))).text(
        pos2(rect.min.x + 6.0, rect.center().y),
        Align2::LEFT_CENTER,
        current,
        theme::font(12.0),
        if enabled { t.text } else { t.text_faint },
    );
    chevron(&painter, pos2(rect.max.x - 9.0, rect.center().y), 3.0, t.text_dim);
    let _ = id;
    egui::Popup::menu(&resp).width(list_width).show(|ui| {
        egui::ScrollArea::vertical().max_height(360.0).show(ui, add_items);
    });
    resp
}

/// The drop-down for a `color_grid`: a menu that stays open when its hex field is clicked
/// (egui menus close on any click inside). Picking a colour closes it (`color_grid` calls
/// `ui.close()`); other buttons in it must close it themselves.
pub fn color_popup(button: &Response) -> egui::Popup<'_> {
    let mut popup = egui::Popup::menu(button).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    // A colour the Eyedropper took for this menu: open it so `color_grid` hands the colour back.
    if crate::eyedropper::result_waits_for(&button.ctx, button.id) {
        popup = popup.open_memory(egui::SetOpenCommand::Bool(true));
    }
    if popup.is_open() {
        crate::eyedropper::note_open_menu(&button.ctx, button.id);
    }
    popup
}

/// The colour grid popup body: theme colours with tints/shades, standard colours, extra entries.
/// Returns the picked colour (`Some(None)` = "No Fill"/"Automatic" when `none_label` is given).
pub fn color_grid(ui: &mut Ui, scheme: &ColorScheme, none_label: Option<&str>) -> Option<Option<ColorRef>> {
    // The Eyedropper's colour, handed back as if its swatch had been clicked.
    if let Some(c) = crate::eyedropper::take_result(ui.ctx()).and_then(|hex| Rgba::from_hex(&hex)) {
        ui.close();
        return Some(Some(ColorRef::rgb(c)));
    }
    let t = Tokens::get(ui.ctx());
    let mut out = None;
    ui.set_min_width(220.0);
    ui.label(egui::RichText::new("Theme Colors").font(theme::bold(12.0)));
    let slots = [
        SchemeSlot::Lt1,
        SchemeSlot::Dk1,
        SchemeSlot::Lt2,
        SchemeSlot::Dk2,
        SchemeSlot::Accent1,
        SchemeSlot::Accent2,
        SchemeSlot::Accent3,
        SchemeSlot::Accent4,
        SchemeSlot::Accent5,
        SchemeSlot::Accent6,
    ];
    let cell = 18.0;
    let sw = |ui: &mut Ui, c: Rgba, tip: &str| -> bool {
        let (r, resp) = ui.allocate_exact_size(vec2(cell, cell), Sense::click());
        ui.painter().rect(
            r.shrink(1.0),
            CornerRadius::same(2),
            theme::to_color32(c),
            Stroke::new(if resp.hovered() { 2.0 } else { 1.0 }, if resp.hovered() { t.accent } else { t.border }),
            egui::StrokeKind::Inside,
        );
        resp.on_hover_text(tip).clicked()
    };
    egui::Grid::new("theme_colors").spacing(vec2(3.0, 1.0)).show(ui, |ui| {
        for slot in slots {
            let alias = match slot {
                SchemeSlot::Lt1 => SchemeSlot::Bg1,
                SchemeSlot::Dk1 => SchemeSlot::Tx1,
                SchemeSlot::Lt2 => SchemeSlot::Bg2,
                SchemeSlot::Dk2 => SchemeSlot::Tx2,
                s => s,
            };
            if sw(ui, scheme.get(slot), slot.label()) {
                out = Some(Some(ColorRef::scheme(alias)));
            }
        }
        ui.end_row();
        for row in 0..5 {
            for slot in slots {
                let base = scheme.get(slot);
                let vars = deckcraft_color::theme_variants(base);
                let Some((mods, label)) = vars.get(row) else { continue };
                let c = deckcraft_color::apply(base, mods);
                if sw(ui, c, label) {
                    let alias = match slot {
                        SchemeSlot::Lt1 => SchemeSlot::Bg1,
                        SchemeSlot::Dk1 => SchemeSlot::Tx1,
                        SchemeSlot::Lt2 => SchemeSlot::Bg2,
                        SchemeSlot::Dk2 => SchemeSlot::Tx2,
                        s => s,
                    };
                    let mut r = ColorRef::scheme(alias);
                    r.mods = mods.clone();
                    out = Some(Some(r));
                }
            }
            ui.end_row();
        }
    });
    ui.add_space(4.0);
    ui.label(egui::RichText::new("Standard Colors").font(theme::bold(12.0)));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        for (c, name) in deckcraft_color::STANDARD_COLORS {
            if sw(ui, c, name) {
                out = Some(Some(ColorRef::rgb(c)));
            }
        }
    });
    ui.separator();
    if let Some(n) = none_label
        && ui.button(n).clicked()
    {
        out = Some(None);
    }
    // "More Colors…": a hex field.
    let id = ui.id().with("hex");
    let mut hex: String = ui.data_mut(|d| d.get_temp::<String>(id).unwrap_or_default());
    ui.horizontal(|ui| {
        ui.label("More Colors… #");
        let r = ui.add(egui::TextEdit::singleline(&mut hex).desired_width(70.0).hint_text("RRGGBB"));
        if r.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter))
            && let Some(c) = Rgba::from_hex(&hex)
        {
            out = Some(Some(ColorRef::rgb(c)));
        }
    });
    ui.data_mut(|d| d.insert_temp(id, hex));
    if ui.button("Eyedropper").on_hover_text("Click the slide to take the colour under the pointer").clicked() {
        crate::eyedropper::request(ui.ctx());
        ui.close();
    }
    if out.is_some() {
        ui.close();
    }
    out
}

/// A row of gallery tiles drawn by `paint_tile`; returns the clicked index.
pub fn gallery(
    ui: &mut Ui,
    n: usize,
    tile: Vec2,
    selected: Option<usize>,
    mut paint_tile: impl FnMut(&egui::Painter, Rect, usize),
    mut tip: impl FnMut(usize) -> String,
) -> Option<usize> {
    let t = Tokens::get(ui.ctx());
    let mut clicked = None;
    for i in 0..n {
        let (rect, resp) = ui.allocate_exact_size(tile, Sense::click());
        let painter = ui.painter_at(rect.expand(2.0));
        let sel = selected == Some(i);
        if sel || resp.hovered() {
            painter.rect_filled(rect.expand(1.0), CornerRadius::same(4), if sel { t.selected } else { t.hover });
        }
        paint_tile(&painter, rect.shrink(3.0), i);
        if sel {
            painter.rect_stroke(rect.expand(1.0), CornerRadius::same(4), Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
        }
        if resp.on_hover_text(tip(i)).clicked() {
            clicked = Some(i);
        }
    }
    clicked
}

/// deckrt: PowerPoint's in-ribbon gallery. One row of `cols` tiles in a framed box, with the
/// arrows down its right side: up and down page through the rows, the bottom one is `more`
/// (attach the full gallery to it with `egui::Popup::menu`). The row with the selected tile is
/// shown when the selection changes.
#[allow(clippy::too_many_arguments)]
pub fn framed_gallery(
    ui: &mut Ui,
    id_salt: &str,
    n: usize,
    tile: Vec2,
    cols: usize,
    selected: Option<usize>,
    enabled: bool,
    paint_tile: impl FnMut(&egui::Painter, Rect, usize),
    tip: impl FnMut(usize) -> String,
) -> (Option<usize>, Response) {
    let t = Tokens::get(ui.ctx());
    let cols = cols.max(1);
    let rows = n.div_ceil(cols).max(1);
    let key = ui.id().with(("framed-gallery", id_salt));
    let (mut row, last_sel): (usize, Option<usize>) = ui.data(|d| d.get_temp(key)).unwrap_or((0, None));
    if selected != last_sel
        && let Some(s) = selected
    {
        row = s / cols;
    }
    row = row.min(rows - 1);
    let box_size = vec2(cols as f32 * tile.x + 4.0 + 16.0, tile.y + 6.0);
    let (rect, _) = ui.allocate_exact_size(vec2(box_size.x, RIBBON_H - 2.0), Sense::hover());
    let frame = Rect::from_min_size(rect.min + vec2(0.0, 1.0), box_size);
    ui.painter().rect(frame, CornerRadius::same(2), t.card, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    let tiles = Rect::from_min_size(frame.min + vec2(2.0, 3.0), vec2(cols as f32 * tile.x, tile.y));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(tiles).layout(egui::Layout::left_to_right(egui::Align::Min)));
    child.spacing_mut().item_spacing = vec2(0.0, 0.0);
    if !enabled {
        child.disable();
    }
    let first = row * cols;
    let count = n.saturating_sub(first).min(cols);
    let mut paint_tile = paint_tile;
    let mut tip = tip;
    let picked = gallery(
        &mut child,
        count,
        tile,
        selected.and_then(|s| s.checked_sub(first)).filter(|s| *s < count),
        |p, r, i| paint_tile(p, r, first + i),
        |i| tip(first + i),
    )
    .map(|i| first + i);
    // The arrows.
    let col = Rect::from_min_max(pos2(tiles.max.x + 1.0, frame.min.y), frame.max);
    ui.painter().line_segment([col.left_top(), col.left_bottom()], Stroke::new(1.0, t.border));
    let h = col.height() / 3.0;
    let mut more = None;
    for k in 0..3 {
        let r = Rect::from_min_size(pos2(col.min.x, col.min.y + k as f32 * h), vec2(col.width(), h));
        let (label, on) = match k {
            0 => ("Row up", row > 0),
            1 => ("Row down", row + 1 < rows),
            _ => ("More", enabled),
        };
        let resp = ui.interact(r, key.with(k), if on { Sense::click() } else { Sense::hover() });
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, on, label));
        if resp.hovered() && on {
            ui.painter().rect_filled(r.shrink(1.0), CornerRadius::ZERO, t.hover);
        }
        let c = r.center();
        let colr = if on { t.text_dim } else { t.text_faint };
        match k {
            0 => chevron_up(ui.painter(), c, 3.0, colr),
            1 => chevron(ui.painter(), c, 3.0, colr),
            _ => {
                ui.painter().line_segment([pos2(c.x - 3.0, c.y - 3.5), pos2(c.x + 3.0, c.y - 3.5)], Stroke::new(1.2, colr));
                chevron(ui.painter(), c + vec2(0.0, 1.5), 3.0, colr);
            }
        }
        if resp.clicked() {
            match k {
                0 => row = row.saturating_sub(1),
                1 => row += 1,
                _ => {}
            }
        }
        if k == 2 {
            more = Some(resp);
        }
    }
    ui.data_mut(|d| d.insert_temp(key, (row, selected)));
    #[allow(clippy::expect_used)]
    (picked, more.expect("three arrows"))
}

/// A labelled number field with up/down steppers. Returns the new value when committed.
pub fn spinner(ui: &mut Ui, id: &str, value: f64, step: f64, suffix: &str, width: f32) -> Option<f64> {
    let t = Tokens::get(ui.ctx());
    let key = ui.id().with(id);
    let mut text: String = ui.data_mut(|d| d.get_temp::<String>(key)).unwrap_or_else(|| fmt_num(value, suffix));
    let mut out = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let r = ui.add(egui::TextEdit::singleline(&mut text).desired_width(width).font(theme::font(12.0)));
        if r.has_focus() {
            ui.data_mut(|d| d.insert_temp(key, text.clone()));
        } else {
            ui.data_mut(|d| d.remove::<String>(key));
        }
        if r.lost_focus() {
            let num: String = text.chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect();
            if let Ok(v) = num.parse::<f64>() {
                out = Some(v);
            }
        }
        let (rect, _) = ui.allocate_exact_size(vec2(12.0, 22.0), Sense::hover());
        let up = ui.interact(Rect::from_min_max(rect.min, pos2(rect.max.x, rect.center().y)), key.with("up"), Sense::click());
        let down = ui.interact(Rect::from_min_max(pos2(rect.min.x, rect.center().y), rect.max), key.with("dn"), Sense::click());
        let p = ui.painter();
        let c = rect.center();
        p.line_segment([pos2(c.x - 3.0, c.y - 3.0), pos2(c.x, c.y - 6.0)], Stroke::new(1.0, t.text_dim));
        p.line_segment([pos2(c.x, c.y - 6.0), pos2(c.x + 3.0, c.y - 3.0)], Stroke::new(1.0, t.text_dim));
        p.line_segment([pos2(c.x - 3.0, c.y + 3.0), pos2(c.x, c.y + 6.0)], Stroke::new(1.0, t.text_dim));
        p.line_segment([pos2(c.x, c.y + 6.0), pos2(c.x + 3.0, c.y + 3.0)], Stroke::new(1.0, t.text_dim));
        if up.clicked() {
            out = Some(value + step);
        }
        if down.clicked() {
            out = Some(value - step);
        }
    });
    out
}

pub fn fmt_num(v: f64, suffix: &str) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    format!("{s}{suffix}")
}

/// The text a kept-open text box shows: the typed draft while the stored value is still what the
/// draft started from, otherwise the stored value (another document, a reorder or an undo changed it).
pub fn text_draft(stored: Option<(String, String)>, current: &str) -> String {
    match stored {
        Some((base, buf)) if base == current => buf,
        _ => current.to_string(),
    }
}

/// Section header inside panes.
pub fn section(ui: &mut Ui, title: &str, open: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
    let p = ui.painter();
    let c = pos2(rect.min.x + 8.0, rect.center().y);
    if open {
        chevron(p, c, 3.5, t.text);
    } else {
        p.line_segment([pos2(c.x - 2.0, c.y - 4.0), pos2(c.x + 2.0, c.y)], Stroke::new(1.2, t.text));
        p.line_segment([pos2(c.x + 2.0, c.y), pos2(c.x - 2.0, c.y + 4.0)], Stroke::new(1.2, t.text));
    }
    p.text(pos2(rect.min.x + 18.0, rect.center().y), Align2::LEFT_CENTER, title, theme::bold(13.0), t.text);
    if resp.clicked() { !open } else { open }
}

/// A button with an optional label and a painted dropdown chevron (the UI font has no ▾).
pub fn drop_button(ui: &mut Ui, label: &str, size: egui::Vec2, enabled: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let r = ui.add_enabled(enabled, egui::Button::new(if label.is_empty() { String::new() } else { format!("{label}   ") }).min_size(size));
    let col = if enabled { t.text_dim } else { t.text_faint };
    chevron(ui.painter(), egui::pos2(r.rect.max.x - 8.0, r.rect.center().y), 3.0, col);
    r
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::cell::RefCell;

    use super::*;
    use egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;

    /// #32: clicking the "More Colors" hex field closed the colour menu before anything could be
    /// typed. It stays open now; Enter picks the colour and closes it.
    #[test]
    fn the_hex_field_keeps_the_color_menu_open() {
        let picked = RefCell::new(None);
        let scheme = deckcraft_model::Theme::default().colors;
        let mut harness = egui_kittest::Harness::builder().with_size(vec2(400.0, 500.0)).build_ui(|ui| {
            theme::install_fonts(ui.ctx());
            let r = ui.button("Pick");
            color_popup(&r).show(|ui| {
                if let Some(c) = color_grid(ui, &scheme, None) {
                    *picked.borrow_mut() = Some(c);
                }
            });
        });
        harness.get_by_label("Pick").click();
        harness.run();
        harness.get_by_role(Role::TextInput).click();
        harness.run();
        assert!(harness.query_by_role(Role::TextInput).is_some(), "the menu is still open after clicking the hex field");
        harness.get_by_role(Role::TextInput).type_text("00FF00");
        harness.run();
        harness.key_press(egui::Key::Enter);
        harness.run();
        assert_eq!(*picked.borrow(), Some(Some(ColorRef::rgb(Rgba::from_hex("00FF00").unwrap()))));
        assert!(harness.query_by_role(Role::TextInput).is_none(), "picking a colour closes the menu");
    }

    /// Choosing Eyedropper closes the menu and asks for it; the colour it takes comes back through
    /// the same menu, as if its swatch had been clicked.
    #[test]
    fn the_eyedroppers_colour_comes_back_through_the_menu_that_asked() {
        let picked = RefCell::new(None);
        let scheme = deckcraft_model::Theme::default().colors;
        let mut harness = egui_kittest::Harness::builder().with_size(vec2(400.0, 500.0)).build_ui(|ui| {
            theme::install_fonts(ui.ctx());
            let r = ui.button("Pick");
            color_popup(&r).show(|ui| {
                if let Some(c) = color_grid(ui, &scheme, None) {
                    *picked.borrow_mut() = Some(c);
                }
            });
        });
        harness.get_by_label("Pick").click();
        harness.run();
        harness.get_by_label("Eyedropper").click();
        harness.run();
        assert!(harness.query_by_label("Eyedropper").is_none(), "choosing it closes the menu");
        assert!(picked.borrow().is_none(), "nothing is picked yet");
        // The app starts the Eyedropper from this request; here the colour is taken at once.
        assert!(harness.ctx.data_mut(|d| d.remove_temp::<bool>(egui::Id::new("deckcraft.eyedropper.request"))).unwrap_or(false));
        let menu = crate::eyedropper::open_menu(&harness.ctx).expect("the open menu noted its button");
        crate::eyedropper::store_result(&harness.ctx, menu, "00FF00");
        harness.run();
        assert_eq!(*picked.borrow(), Some(Some(ColorRef::rgb(Rgba::from_hex("00FF00").unwrap()))));
        assert!(harness.query_by_label("Eyedropper").is_none(), "the menu closes again");
        harness.run();
        assert_eq!(picked.borrow().iter().count(), 1, "the colour is handed back once");
    }

    /// #413: a draft is kept while the stored text is unchanged ...
    #[test]
    fn text_draft_keeps_typing_while_the_stored_text_is_unchanged() {
        let stored = Some(("old".to_string(), "old plus typing".to_string()));
        assert_eq!(text_draft(stored, "old"), "old plus typing");
    }

    /// ... and dropped when the stored text changes underneath it.
    #[test]
    fn text_draft_drops_a_stale_draft_when_the_stored_text_changes() {
        let stored = Some(("old".to_string(), "old plus typing".to_string()));
        assert_eq!(text_draft(stored, "undone"), "undone");
        assert_eq!(text_draft(None, "fresh"), "fresh");
    }
}
