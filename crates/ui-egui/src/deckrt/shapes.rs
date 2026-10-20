//! deckrt: the shapes gallery as PowerPoint has it. The full menu starts with the shapes used
//! last, and the Home tab's Drawing group shows the gallery inline, three rows at a time, with
//! arrows to scroll it and one to open the whole menu.

use deckcraft_geom::preset::{CATALOG, Category};
use egui::{Color32, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::SlideApp;
use crate::theme::{self, Tokens};

const RECENT_ID: &str = "deckrt-recent-shapes";
const RECENT_MAX: usize = 16;

/// What the Drawing group shows before any shape has been used: PowerPoint's opening row.
const STARTERS: [&str; 18] = [
    "textBox",
    "line",
    "straightConnector1",
    "rect",
    "ellipse",
    "roundRect",
    "triangle",
    "bentConnector3",
    "curvedConnector3",
    "rightArrow",
    "downArrow",
    "leftBrace",
    "rightBrace",
    "star5",
    "arc",
    "rtTriangle",
    "pentagon",
    "hexagon",
];

fn recent(ctx: &egui::Context) -> Vec<String> {
    ctx.data(|d| d.get_temp::<Vec<String>>(egui::Id::new(RECENT_ID))).unwrap_or_default()
}

/// Remember a shape as just used, for the top of the menu and the inline gallery.
pub fn used(ctx: &egui::Context, name: &str) {
    let mut v = recent(ctx);
    v.retain(|n| n != name);
    v.insert(0, name.to_string());
    v.truncate(RECENT_MAX);
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(RECENT_ID), v));
}

fn label_of(name: &str) -> &'static str {
    CATALOG
        .iter()
        .find(|p| p.name == name)
        .map(|p| p.label)
        .or_else(|| deckcraft_engine::tools::FREEFORM_TOOLS.iter().find(|(n, _)| *n == name).map(|(_, l)| *l))
        .unwrap_or("")
}

fn static_name(name: &str) -> Option<&'static str> {
    CATALOG
        .iter()
        .find(|p| p.name == name)
        .map(|p| p.name)
        .or_else(|| deckcraft_engine::tools::FREEFORM_TOOLS.iter().find(|(n, _)| *n == name).map(|(n, _)| *n))
}

/// One clickable shape cell. Returns whether it was clicked.
fn cell(ui: &mut Ui, name: &'static str, size: egui::Vec2) -> bool {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label_of(name)));
    if resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(2), t.hover);
        ui.painter().rect_stroke(r, CornerRadius::same(2), Stroke::new(1.0, t.accent.gamma_multiply(0.6)), egui::StrokeKind::Inside);
    }
    paint(ui.painter(), r.shrink(size.x * 0.17), name, &t);
    resp.on_hover_text(label_of(name)).clicked()
}

fn paint(p: &egui::Painter, r: Rect, name: &str, t: &Tokens) {
    if name == "textBox" {
        // PowerPoint's text box: a box with an A in it, in the accent blue.
        let blue = Color32::from_rgb(0x2B, 0x6C, 0xD4);
        p.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, blue), egui::StrokeKind::Inside);
        p.text(r.center(), egui::Align2::CENTER_CENTER, "A", theme::bold(r.height() * 0.8), blue);
    } else if deckcraft_engine::tools::FREEFORM_TOOLS.iter().any(|(n, _)| *n == name) {
        crate::ribbon::paint_freeform_icon(p, r, name, t.text);
    } else {
        crate::ribbon::paint_preset(p, r, name, Color32::TRANSPARENT, t.text);
    }
}

/// The whole menu: the shapes used last, then every category under its name.
pub fn gallery(ui: &mut Ui) -> Option<&'static str> {
    menu(ui, false)
}

/// Change Shape and Crop to Shape: the closed shapes only, as in PowerPoint (a line, a text box
/// or a freehand tool can't be what a shape turns into).
pub fn change_gallery(ui: &mut Ui) -> Option<&'static str> {
    menu(ui, true)
}

fn menu(ui: &mut Ui, closed_only: bool) -> Option<&'static str> {
    let t = Tokens::get(ui.ctx());
    let mut out = None;
    ui.set_width(330.0);
    let max_h = (ui.ctx().content_rect().height() - ui.cursor().min.y - 16.0).clamp(260.0, 760.0);
    let keep = |n: &str| !closed_only || (n != "textBox" && CATALOG.iter().any(|p| p.name == n && p.category != Category::Lines));
    let recents: Vec<&'static str> = recent(ui.ctx()).iter().filter_map(|n| static_name(n)).filter(|n| keep(n)).collect();
    egui::ScrollArea::vertical().max_height(max_h).min_scrolled_height(max_h).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 2.0;
        let mut section = |ui: &mut Ui, title: &str, first: bool, names: &mut dyn Iterator<Item = &'static str>| {
            if !first {
                ui.add_space(3.0);
                let y = ui.cursor().min.y;
                ui.painter().line_segment([pos2(ui.max_rect().min.x, y), pos2(ui.max_rect().max.x, y)], Stroke::new(1.0, t.separator));
                ui.add_space(4.0);
            }
            ui.label(egui::RichText::new(title).font(theme::bold(12.5)).color(t.text));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                for n in names {
                    if cell(ui, n, vec2(26.0, 26.0)) {
                        out = Some(n);
                    }
                }
            });
        };
        let mut first = true;
        if !recents.is_empty() {
            section(ui, "Recently Used Shapes", true, &mut recents.iter().copied());
            first = false;
        }
        for cat in Category::ALL {
            if closed_only && cat == Category::Lines {
                continue;
            }
            let mut names: Vec<&'static str> = CATALOG.iter().filter(|p| p.category == cat && keep(p.name)).map(|p| p.name).collect();
            if cat == Category::Lines {
                names.extend(deckcraft_engine::tools::FREEFORM_TOOLS.iter().map(|(n, _)| *n));
            }
            section(ui, cat.label(), first, &mut names.into_iter());
            first = false;
        }
    });
    if let Some(n) = out {
        if !closed_only {
            used(ui.ctx(), n);
        }
        ui.close();
    }
    out
}

/// Pick a shape to draw.
pub fn draw_with(app: &mut SlideApp, name: &str) {
    app.session.set_tool(deckcraft_engine::ToolKind::Shape { preset: name.to_string() });
}

const COLS: usize = 7;
const ROWS: usize = 3;

/// The Drawing group's inline gallery: three rows of shapes, the arrows to scroll them and the
/// one that opens the whole menu.
pub fn inline(app: &mut SlideApp, ui: &mut Ui, enabled: bool) {
    let t = Tokens::get(ui.ctx());
    let mut names: Vec<&'static str> = recent(ui.ctx()).iter().filter_map(|n| static_name(n)).collect();
    for n in STARTERS.iter().copied().chain(CATALOG.iter().map(|p| p.name)) {
        if !names.contains(&n) {
            names.push(n);
        }
    }
    let rows_total = names.len().div_ceil(COLS);
    let off_id = egui::Id::new("deckrt-inline-shapes-row");
    let mut first_row: usize = ui.ctx().data(|d| d.get_temp(off_id)).unwrap_or(0);
    let cell_size = vec2(22.0, 21.0);
    let box_size = vec2(COLS as f32 * cell_size.x + 2.0 + 15.0, ROWS as f32 * cell_size.y + 2.0);
    let (rect, _) = ui.allocate_exact_size(vec2(box_size.x, crate::widgets::RIBBON_H - 2.0), Sense::hover());
    let frame = Rect::from_min_size(pos2(rect.min.x, rect.min.y + 1.0), box_size);
    ui.painter().rect(frame, CornerRadius::same(2), t.card, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    let grid = Rect::from_min_size(frame.min + vec2(1.0, 1.0), vec2(COLS as f32 * cell_size.x, ROWS as f32 * cell_size.y));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(grid).layout(egui::Layout::left_to_right(egui::Align::Min)));
    child.set_clip_rect(grid);
    if !enabled {
        child.disable();
    }
    child.spacing_mut().item_spacing = vec2(0.0, 0.0);
    for row in 0..ROWS {
        for col in 0..COLS {
            let i = (first_row + row) * COLS + col;
            let at = Rect::from_min_size(grid.min + vec2(col as f32 * cell_size.x, row as f32 * cell_size.y), cell_size);
            let Some(&n) = names.get(i) else { continue };
            let mut c = child.new_child(egui::UiBuilder::new().max_rect(at));
            if cell(&mut c, n, cell_size) {
                used(ui.ctx(), n);
                draw_with(app, n);
            }
        }
    }
    // The scroll column: up, down, and the whole menu.
    let col = Rect::from_min_max(pos2(grid.max.x + 1.0, frame.min.y), frame.max);
    ui.painter().line_segment([col.left_top(), col.left_bottom()], Stroke::new(1.0, t.border));
    let h = col.height() / 3.0;
    let parts: Vec<Rect> = (0..3).map(|k| Rect::from_min_size(pos2(col.min.x, col.min.y + k as f32 * h), vec2(col.width(), h))).collect();
    let can_up = first_row > 0;
    let can_down = first_row + ROWS < rows_total;
    let mut more = None;
    for (k, r) in parts.iter().enumerate() {
        let (label, on) = match k {
            0 => ("Row up", can_up && enabled),
            1 => ("Row down", can_down && enabled),
            _ => ("More shapes", enabled),
        };
        let resp = ui.interact(*r, ui.id().with(("inline-shapes-scroll", k)), if on { Sense::click() } else { Sense::hover() });
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, on, label));
        if resp.hovered() && on {
            ui.painter().rect_filled(r.shrink(1.0), CornerRadius::ZERO, t.hover);
        }
        let c = r.center();
        let col = if on { t.text_dim } else { t.text_faint };
        let s = 3.0;
        match k {
            0 => crate::widgets::chevron_up(ui.painter(), c, s, col),
            1 => crate::widgets::chevron(ui.painter(), c, s, col),
            _ => {
                ui.painter().line_segment([pos2(c.x - s, c.y - s - 0.5), pos2(c.x + s, c.y - s - 0.5)], Stroke::new(1.2, col));
                crate::widgets::chevron(ui.painter(), c + vec2(0.0, 1.5), s, col);
            }
        }
        if resp.clicked() {
            match k {
                0 => first_row = first_row.saturating_sub(1),
                1 => first_row += 1,
                _ => more = Some(resp.clone()),
            }
        }
        if k == 2 {
            egui::Popup::menu(&resp).show(|ui| {
                if let Some(p) = gallery(ui) {
                    draw_with(app, p);
                }
            });
        }
    }
    let _ = more;
    ui.ctx().data_mut(|d| d.insert_temp(off_id, first_row.min(rows_total.saturating_sub(ROWS))));
}
