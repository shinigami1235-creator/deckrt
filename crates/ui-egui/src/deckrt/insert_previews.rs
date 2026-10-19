//! deckrt: pictures in the Insert menus that were lists of names: Chart, WordArt, Action and the
//! SmartArt dialog.
//!
//! Chart, WordArt and Action pictures are the real thing: each is inserted into a scratch deck
//! with the open deck's theme and drawn by the renderer, a few a frame, and kept until the theme
//! changes. SmartArt uses upstream's layout pictures.

use std::collections::HashMap;
use std::sync::Arc;

use deckcraft_engine::Session;
use deckcraft_render::RenderOpts;
use egui::{Rect, RichText, TextureHandle, Ui, pos2, vec2};
use serde_json::{Value, json};

use crate::SlideApp;

/// Pictures drawn so far, for one theme.
#[derive(Default)]
pub struct Thumbs {
    theme: usize,
    map: HashMap<String, Option<TextureHandle>>,
}

/// New pictures drawn per frame, so a menu opens at once and fills in.
const PER_FRAME: usize = 3;

/// The picture of `key`: a scratch slide with the open deck's theme, the command run on it, and
/// the new shape drawn on its own. `None` while waiting for its turn (or when it can't be drawn).
fn thumb(app: &mut SlideApp, ui: &Ui, key: &str, made: &mut usize, cmd: &str, params: Value, width_px: u32) -> Option<TextureHandle> {
    let doc = app.session.active()?.doc.clone();
    let theme = doc.masters.first().map(|m| Arc::as_ptr(m) as usize).unwrap_or(0);
    let th = &mut app.deckrt.insert_thumbs;
    if th.theme != theme {
        *th = Thumbs { theme, map: HashMap::new() };
    }
    if let Some(t) = th.map.get(key) {
        return t.clone();
    }
    if *made >= PER_FRAME {
        ui.ctx().request_repaint();
        return None;
    }
    *made += 1;
    let tex = draw(ui.ctx(), &doc, key, cmd, &params, width_px);
    app.deckrt.insert_thumbs.map.insert(key.to_string(), tex.clone());
    tex
}

fn draw(ctx: &egui::Context, doc: &deckcraft_model::Presentation, key: &str, cmd: &str, params: &Value, width_px: u32) -> Option<TextureHandle> {
    let mut s = Session::with_new();
    let (masters, size) = (doc.masters.clone(), doc.slide_size);
    s.edit(|d, _| {
        d.masters = masters;
        d.slide_size = size;
        d.slides.clear();
        Ok(())
    })
    .ok()?;
    s.execute("slide.new", &json!({"layout": "blank"})).ok()?;
    let r = s.execute(cmd, params).ok()?;
    let id = r.get("id").and_then(Value::as_u64)?;
    if cmd == "insert.wordArt" {
        // WordArt's box spans most of the slide; the picture is of the word.
        let w = if params.get("text").and_then(Value::as_str) == Some("A") { 72.0 } else { 260.0 };
        let _ = s.execute("shape.resize", &json!({"ids": [id], "w": w}));
    }
    let st = s.active()?;
    let sh = st.shapes().iter().find(|sh| sh.id.0 as u64 == id)?.clone();
    let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, &sh);
    let pad = (x.w.max(x.h) * 0.06).max(2.0);
    let (rx, ry, rw, rh) = (x.x - pad, x.y - pad, x.w + 2.0 * pad, x.h + 2.0 * pad);
    if rw <= 1.0 || rh <= 1.0 {
        return None;
    }
    let scale = width_px as f64 / rw;
    let out = (width_px, ((rh * scale).round() as u32).clamp(1, 2000));
    let opts = RenderOpts { scale, size: Some(out), offset: (-rx * scale, -ry * scale), ..Default::default() };
    let img = deckcraft_render::render_slide(&st.doc, 0, &opts);
    Some(crate::textures::upload(ctx, format!("deckrt-insert-{key}"), crate::textures::to_color_image(&img, false)))
}

/// One tile: the picture fitted into `size` with the name under it. Returns the click.
fn tile(ui: &mut Ui, tex: Option<&TextureHandle>, size: egui::Vec2, label: &str) -> egui::Response {
    let t = crate::theme::Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(size.x + 8.0, size.y + 22.0), egui::Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, egui::CornerRadius::same(4), t.hover);
    }
    let pic = Rect::from_min_size(r.min + vec2(4.0, 4.0), size);
    match tex {
        Some(tex) => {
            let [w, h] = tex.size();
            let k = (pic.width() / w.max(1) as f32).min(pic.height() / h.max(1) as f32);
            let fit = Rect::from_center_size(pic.center(), vec2(w as f32 * k, h as f32 * k));
            ui.painter().image(tex.id(), fit, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), egui::Color32::WHITE);
        }
        None => {
            ui.painter().rect_filled(pic, egui::CornerRadius::same(3), t.hover);
        }
    }
    let g = ui.painter().layout(label.to_string(), crate::theme::font(11.0), t.text, r.width() - 4.0);
    ui.painter().galley(pos2(r.center().x - g.size().x / 2.0, pic.max.y + 3.0), g, t.text);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    resp.on_hover_text(label)
}

/// Insert › WordArt: PowerPoint's twenty styles as a big A each, five to a row, drawn in the
/// deck's colours. The style's name shows on hover.
pub fn wordart_menu(app: &mut SlideApp, ui: &mut Ui) {
    let mut made = 0;
    let t = crate::theme::Tokens::get(ui.ctx());
    egui::Grid::new("deckrt-wordart").spacing(vec2(6.0, 6.0)).show(ui, |ui| {
        for (i, name) in deckcraft_engine::cmd::deckrt_wordart::NAMES.iter().enumerate() {
            let tex = thumb(app, ui, &format!("wordart-a-{i}"), &mut made, "insert.wordArt", json!({"style": i, "text": "A"}), 128);
            let (r, resp) = ui.allocate_exact_size(vec2(58.0, 58.0), egui::Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, egui::CornerRadius::same(3), t.hover);
                ui.painter().rect_stroke(r, egui::CornerRadius::same(3), egui::Stroke::new(1.0, t.accent.gamma_multiply(0.6)), egui::StrokeKind::Inside);
            }
            if let Some(tex) = &tex {
                let [w, h] = tex.size();
                let pic = r.shrink(5.0);
                let k = (pic.width() / w.max(1) as f32).min(pic.height() / h.max(1) as f32);
                let fit = Rect::from_center_size(pic.center(), vec2(w as f32 * k, h as f32 * k));
                ui.painter().image(tex.id(), fit, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), egui::Color32::WHITE);
            }
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, *name));
            if resp.on_hover_text(*name).clicked() {
                let _ = app.run("insert.wordArt", json!({"style": i}));
                ui.close();
            }
            if i % 5 == 4 {
                ui.end_row();
            }
        }
    });
}

/// The action buttons, in PowerPoint's order, with their names.
pub const ACTIONS: [(&str, &str); 12] = [
    ("back", "Back or Previous"),
    ("forward", "Forward or Next"),
    ("beginning", "Beginning"),
    ("end", "End"),
    ("home", "Home"),
    ("information", "Information"),
    ("return", "Return"),
    ("movie", "Video"),
    ("document", "Document"),
    ("sound", "Sound"),
    ("help", "Help"),
    ("custom", "Blank"),
];

/// Insert › Action: the action buttons as pictures. Returns true when one was inserted.
pub fn action_menu(app: &mut SlideApp, ui: &mut Ui) -> bool {
    let mut made = 0;
    let mut done = false;
    ui.label(RichText::new("Action Buttons").strong());
    egui::Grid::new("deckrt-actions").spacing(vec2(2.0, 2.0)).show(ui, |ui| {
        for (i, (k, label)) in ACTIONS.iter().enumerate() {
            let tex = thumb(app, ui, &format!("action-{k}"), &mut made, "insert.actionButton", json!({"kind": k, "rect": [100, 100, 60, 60]}), 96);
            if tile(ui, tex.as_ref(), vec2(70.0, 44.0), label).clicked() {
                let _ = app.run("insert.actionButton", json!({"kind": k}));
                done = true;
            }
            if i % 4 == 3 {
                ui.end_row();
            }
        }
    });
    done
}

/// The SmartArt dialog's choice of layout: the categories down the left as PowerPoint has them,
/// every layout of the chosen one as a picture on the right. Returns the layout clicked.
pub fn smartart_picker(app: &mut SlideApp, ui: &mut Ui, current: &str) -> Option<&'static str> {
    use deckcraft_engine::cmd::smartart::{LAYOUTS, category};
    const CATS: [(&str, &str); 9] = [
        ("all", "All"),
        ("list", "List"),
        ("process", "Process"),
        ("cycle", "Cycle"),
        ("hierarchy", "Hierarchy"),
        ("relationship", "Relationship"),
        ("matrix", "Matrix"),
        ("pyramid", "Pyramid"),
        ("picture", "Picture"),
    ];
    let mut out = None;
    let slide = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
    let key = egui::Id::new("deckrt-smartart-category");
    let mut cat: String = ui.data(|d| d.get_temp(key)).unwrap_or_else(|| "all".to_string());
    ui.horizontal_top(|ui| {
        // A fixed height: the separator between the columns would otherwise take the screen's.
        ui.set_height(400.0);
        ui.vertical(|ui| {
            ui.set_width(110.0);
            for (k, label) in CATS {
                let n = LAYOUTS.iter().filter(|(l, _)| k == "all" || category(l) == k).count();
                if n > 0 && ui.selectable_label(cat == k, label).clicked() {
                    cat = k.to_string();
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical().id_salt(("deckrt-smartart-scroll", &cat)).max_height(400.0).auto_shrink([false, true]).show(ui, |ui| {
            let kinds: Vec<(&'static str, &'static str)> = LAYOUTS.iter().copied().filter(|(k, _)| cat == "all" || category(k) == cat).collect();
            egui::Grid::new(("deckrt-smartart", &cat)).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
                for (i, (k, label)) in kinds.iter().enumerate() {
                    let ctx = ui.ctx().clone();
                    let tex = app.session.active().and_then(|d| app.textures.smartart(&ctx, &d.doc, slide, k, "", "", None, 192));
                    let resp = tile(ui, tex.as_ref(), vec2(96.0, 60.0), label);
                    if *k == current {
                        let t = crate::theme::Tokens::get(ui.ctx());
                        ui.painter().rect_stroke(resp.rect, egui::CornerRadius::same(4), egui::Stroke::new(2.0, t.accent), egui::StrokeKind::Inside);
                    }
                    if resp.clicked() {
                        out = Some(*k);
                    }
                    if i % 4 == 3 {
                        ui.end_row();
                    }
                }
            });
        });
    });
    ui.data_mut(|d| d.insert_temp(key, cat));
    out
}

// ---------- The SmartArt and Insert Chart dialogs, laid out as PowerPoint's ----------

/// What each SmartArt layout is for, shown under its preview.
fn smartart_about(kind: &str) -> &'static str {
    match kind {
        "list" => "Items in no set order, each with a few points under it.",
        "blockList" => "Separate blocks of information side by side, wrapping to fill the space.",
        "hBulletList" => "Columns of points, each under its own heading.",
        "process" => "Steps from left to right with arrows between them.",
        "chevron" => "Steps or stages in a line of arrow shapes.",
        "verticalProcess" => "Steps from top to bottom.",
        "stepUp" => "Steps that build on each other, rising from left to right.",
        "cycle" => "Stages that repeat in a loop.",
        "blockCycle" => "A loop of stages drawn as blocks with arrows between them.",
        "radial" => "One idea in the middle with what connects to it around it.",
        "hierarchy" => "A tree: who reports to whom, or what splits into what.",
        "pyramid" => "Levels that build up from a wide base.",
        "invertedPyramid" => "Levels that narrow down from a wide top.",
        "matrix" => "Four parts of one whole in a two by two grid.",
        "titledMatrix" => "Four parts around a title in the middle.",
        "pictureList" => "A picture beside each item, one under the other.",
        "pictureCaption" => "Pictures in a row with a caption under each.",
        "pictureProcess" => "Steps with a picture on each.",
        "pictureHList" => "Pictures side by side with text under each.",
        "timeline" => "Events in order along a line.",
        "venn" => "Overlapping circles for what ideas share.",
        "target" => "Rings inside rings, from the outside in.",
        "textCycle" => "A loop of stages written as text with arrows between them.",
        "continuousCycle" => "A loop with one arrow running all the way round.",
        "horizontalHierarchy" => "A tree that grows from left to right.",
        "gridMatrix" => "Items placed on two axes, in four quarters.",
        "gear" => "Ideas that drive each other, drawn as gears.",
        "funnel" => "Items narrowing down to one result.",
        "segmentedCycle" => "Parts of a loop drawn as slices of a ring.",
        "multidirectionalCycle" => "Stages that can go either way.",
        "orgChart" => "An organization, each person under who they report to.",
        "halfCircleOrgChart" => "An organization chart with each name in a half circle.",
        "cycleMatrix" => "Four parts around a loop.",
        "segmentedPyramid" => "A pyramid split into pieces that fit together.",
        "pie" => "Shares of one whole as slices.",
        "nameTitleOrgChart" => "An organization chart with a name and a job title in each box.",
        "verticalBoxList" => "Headings down the page with a box of text under each.",
        "trapezoidList" => "Items side by side in trapezoid shapes.",
        "tableList" => "One heading across the top with items in columns under it.",
        "accentProcess" => "Steps with a heading and points under each.",
        "continuousBlockProcess" => "Steps along one long arrow.",
        "bendingProcess" => "Many steps that wrap from row to row.",
        "linearVenn" => "Circles overlapping in a line.",
        "stackedVenn" => "Circles growing out of each other, from small to large.",
        "nestedTarget" => "Boxes inside boxes, for things that contain each other.",
        _ => "",
    }
}

const SMARTART_CATS: [(&str, &str); 9] = [
    ("all", "All"),
    ("list", "List"),
    ("process", "Process"),
    ("cycle", "Cycle"),
    ("hierarchy", "Hierarchy"),
    ("relationship", "Relationship"),
    ("matrix", "Matrix"),
    ("pyramid", "Pyramid"),
    ("picture", "Picture"),
];

/// A small picture for a category in a dialog's left-hand list.
fn category_icon(p: &egui::Painter, r: Rect, which: &str) {
    use egui::{Color32, CornerRadius, Stroke};
    let t = crate::theme::Tokens::get(p.ctx());
    let s = Stroke::new(1.1, t.text_dim);
    let blue = Color32::from_rgb(0x2B, 0x6C, 0xD4);
    let fill = blue.gamma_multiply(0.25);
    let (x0, y0, w, h) = (r.min.x, r.min.y, r.width(), r.height());
    let at = |fx: f32, fy: f32| pos2(x0 + fx * w, y0 + fy * h);
    let rect = |a: (f32, f32), b: (f32, f32)| Rect::from_min_max(at(a.0, a.1), at(b.0, b.1));
    match which {
        "all" => {
            for (i, j) in [(0.0, 0.0), (0.55, 0.0), (0.0, 0.55), (0.55, 0.55)] {
                p.rect(rect((i + 0.05, j + 0.05), (i + 0.42, j + 0.42)), CornerRadius::ZERO, fill, s, egui::StrokeKind::Inside);
            }
        }
        "list" | "Column" | "Histogram" => {
            if which == "list" {
                for k in 0..3 {
                    let y = 0.2 + k as f32 * 0.3;
                    p.circle_filled(at(0.12, y), 1.6, blue);
                    p.line_segment([at(0.3, y), at(0.95, y)], s);
                }
            } else {
                for (k, hgt) in [0.5f32, 0.85, 0.65, 0.35].iter().enumerate() {
                    let x = 0.08 + k as f32 * 0.23;
                    p.rect_filled(rect((x, 1.0 - hgt), (x + 0.17, 0.95)), CornerRadius::ZERO, blue);
                }
            }
        }
        "process" | "Waterfall" => {
            for k in 0..3 {
                let x = 0.02 + k as f32 * 0.34;
                p.rect(rect((x, 0.35), (x + 0.24, 0.65)), CornerRadius::ZERO, fill, s, egui::StrokeKind::Inside);
            }
        }
        "cycle" | "Sunburst" | "Pie" => {
            p.circle_stroke(r.center(), w * 0.38, Stroke::new(1.6, blue));
            if which != "cycle" {
                p.line_segment([r.center(), at(0.5, 0.12)], s);
                p.line_segment([r.center(), at(0.85, 0.65)], s);
            }
        }
        "hierarchy" => {
            p.rect(rect((0.35, 0.05), (0.65, 0.3)), CornerRadius::ZERO, fill, s, egui::StrokeKind::Inside);
            p.rect(rect((0.02, 0.65), (0.32, 0.9)), CornerRadius::ZERO, fill, s, egui::StrokeKind::Inside);
            p.rect(rect((0.68, 0.65), (0.98, 0.9)), CornerRadius::ZERO, fill, s, egui::StrokeKind::Inside);
            p.line_segment([at(0.5, 0.3), at(0.5, 0.5)], s);
            p.line_segment([at(0.17, 0.5), at(0.83, 0.5)], s);
            p.line_segment([at(0.17, 0.5), at(0.17, 0.65)], s);
            p.line_segment([at(0.83, 0.5), at(0.83, 0.65)], s);
        }
        "relationship" => {
            p.circle(at(0.38, 0.5), w * 0.3, fill, s);
            p.circle(at(0.62, 0.5), w * 0.3, fill, s);
        }
        "matrix" | "Treemap" => {
            for (i, j) in [(0.0, 0.0), (0.5, 0.0), (0.0, 0.5), (0.5, 0.5)] {
                p.rect(rect((i + 0.05, j + 0.05), (i + 0.47, j + 0.47)), CornerRadius::ZERO, if which == "Treemap" { blue } else { fill }, s, egui::StrokeKind::Inside);
            }
        }
        "pyramid" | "Funnel" => {
            let pts = if which == "pyramid" { vec![at(0.5, 0.05), at(0.95, 0.92), at(0.05, 0.92)] } else { vec![at(0.05, 0.1), at(0.95, 0.1), at(0.5, 0.92)] };
            p.add(egui::Shape::convex_polygon(pts, fill, s));
        }
        "picture" => {
            p.rect(rect((0.05, 0.12), (0.95, 0.88)), CornerRadius::ZERO, Color32::TRANSPARENT, s, egui::StrokeKind::Inside);
            p.add(egui::Shape::convex_polygon(vec![at(0.12, 0.82), at(0.42, 0.4), at(0.7, 0.82)], fill, Stroke::NONE));
            p.circle_filled(at(0.72, 0.32), 2.0, blue);
        }
        "Line" | "Stock" | "Radar" | "Combo" => {
            let pts: Vec<_> = [(0.05, 0.8), (0.3, 0.45), (0.55, 0.6), (0.95, 0.15)].iter().map(|(x, y)| at(*x, *y)).collect();
            p.add(egui::Shape::line(pts, Stroke::new(1.6, blue)));
            if which == "Combo" {
                p.rect_filled(rect((0.15, 0.6), (0.3, 0.95)), CornerRadius::ZERO, blue.gamma_multiply(0.5));
                p.rect_filled(rect((0.6, 0.45), (0.75, 0.95)), CornerRadius::ZERO, blue.gamma_multiply(0.5));
            }
        }
        "Bar" => {
            for (k, wd) in [0.5f32, 0.85, 0.65].iter().enumerate() {
                let y = 0.1 + k as f32 * 0.3;
                p.rect_filled(rect((0.05, y), (0.05 + wd, y + 0.2)), CornerRadius::ZERO, blue);
            }
        }
        "Area" | "Surface" => {
            let pts = vec![at(0.05, 0.95), at(0.05, 0.6), at(0.35, 0.3), at(0.65, 0.5), at(0.95, 0.2), at(0.95, 0.95)];
            p.add(egui::Shape::Path(egui::epaint::PathShape { points: pts, closed: true, fill: blue.gamma_multiply(0.7), stroke: egui::epaint::PathStroke::NONE }));
        }
        "Scatter" => {
            for (x, y) in [(0.2, 0.7), (0.35, 0.45), (0.55, 0.6), (0.7, 0.3), (0.85, 0.2)] {
                p.circle_filled(at(x, y), 1.8, blue);
            }
        }
        "BoxWhisker" => {
            p.line_segment([at(0.5, 0.05), at(0.5, 0.95)], s);
            p.rect(rect((0.25, 0.3), (0.75, 0.7)), CornerRadius::ZERO, fill, s, egui::StrokeKind::Inside);
        }
        _ => {}
    }
}

/// A row in a dialog's left-hand list. Returns the click.
fn category_row(ui: &mut Ui, selected: bool, icon: &str, label: &str) -> egui::Response {
    let t = crate::theme::Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), egui::Sense::click());
    if selected {
        ui.painter().rect_filled(r, egui::CornerRadius::same(2), t.selected);
    } else if resp.hovered() {
        ui.painter().rect_filled(r, egui::CornerRadius::same(2), t.hover);
    }
    category_icon(ui.painter(), Rect::from_min_size(r.min + vec2(6.0, 4.0), vec2(16.0, 16.0)), icon);
    ui.painter().text(pos2(r.min.x + 30.0, r.center().y), egui::Align2::LEFT_CENTER, label, crate::theme::font(13.0), t.text);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, label));
    resp
}

/// The dialog's OK and Cancel, at its bottom right. Enter is OK unless a text box has the keys.
fn ok_cancel(ui: &mut Ui) -> (bool, bool) {
    let mut ok = false;
    let mut cancel = false;
    ui.add_space(6.0);
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let t = crate::theme::Tokens::get(ui.ctx());
        if ui.add(egui::Button::new("Cancel").min_size(vec2(80.0, 26.0))).clicked() {
            cancel = true;
        }
        let typing = ui.ctx().memory(|m| m.focused()).is_some_and(|id| egui::text_edit::TextEditState::load(ui.ctx(), id).is_some());
        if ui.add(egui::Button::new(RichText::new("OK").color(t.accent_text)).fill(t.accent).min_size(vec2(80.0, 26.0))).clicked()
            || (!typing && ui.input(|i| i.key_pressed(egui::Key::Enter)))
        {
            ok = true;
        }
    });
    (ok, cancel)
}

/// Draw `tex` fitted and centred in `r`.
fn fitted(ui: &Ui, tex: &TextureHandle, r: Rect) {
    let [w, h] = tex.size();
    let k = (r.width() / w.max(1) as f32).min(r.height() / h.max(1) as f32);
    let fit = Rect::from_center_size(r.center(), vec2(w as f32 * k, h as f32 * k));
    ui.painter().image(tex.id(), fit, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), egui::Color32::WHITE);
}

/// Insert › SmartArt: "Choose a SmartArt Graphic". Categories on the left, the layouts as
/// pictures in the middle, the chosen one large on the right with what it is for and the text to
/// put in it. Returns true when the dialog is done.
pub fn smartart_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut crate::dialogs::Dialog) -> bool {
    use deckcraft_engine::cmd::smartart::{LAYOUTS, category};
    let t = crate::theme::Tokens::get(ui.ctx());
    let slide = app.session.active().map(|s| s.selection.slide).unwrap_or(0);
    let cat = d.fields.get("cat").cloned().unwrap_or_else(|| "all".into());
    let kind = d.fields.get("kind").cloned().unwrap_or_else(|| "blockList".into());
    let mut text = d.fields.get("text").cloned().unwrap_or_else(|| "Plan\nBuild\nLaunch".into());
    let mut new_cat = None;
    let mut new_kind = None;
    let mut chose = false;
    ui.horizontal_top(|ui| {
        ui.set_height(420.0);
        // Categories.
        egui::Frame::NONE.stroke(egui::Stroke::new(1.0, t.border)).inner_margin(4.0).show(ui, |ui| {
            ui.set_width(140.0);
            ui.set_height(410.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                for (k, label) in SMARTART_CATS {
                    if category_row(ui, cat == k, k, label).clicked() {
                        new_cat = Some(k.to_string());
                    }
                }
            });
        });
        // Layouts.
        egui::Frame::NONE.stroke(egui::Stroke::new(1.0, t.border)).show(ui, |ui| {
            ui.set_width(4.0 * 92.0 + 18.0);
            ui.set_height(418.0);
            ui.vertical(|ui| {
                let title = SMARTART_CATS.iter().find(|(k, _)| *k == cat).map(|(_, l)| *l).unwrap_or("All");
                let (band, _) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), egui::Sense::hover());
                ui.painter().rect_filled(band, egui::CornerRadius::ZERO, t.hover);
                ui.painter().text(pos2(band.min.x + 8.0, band.center().y), egui::Align2::LEFT_CENTER, title, crate::theme::bold(12.5), t.text);
                egui::ScrollArea::vertical().id_salt(("deckrt-smartart-dialog", &cat)).max_height(390.0).auto_shrink([false, false]).show(ui, |ui| {
                    let kinds: Vec<(&'static str, &'static str)> = LAYOUTS.iter().copied().filter(|(k, _)| cat == "all" || category(k) == cat).collect();
                    egui::Grid::new(("deckrt-smartart-grid", &cat)).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
                        for (i, (k, label)) in kinds.iter().enumerate() {
                            let ctx = ui.ctx().clone();
                            let tex = app.session.active().and_then(|s| app.textures.smartart(&ctx, &s.doc, slide, k, "", "", None, 192));
                            let (r, resp) = ui.allocate_exact_size(vec2(88.0, 70.0), egui::Sense::click());
                            if *k == kind {
                                ui.painter().rect_filled(r, egui::CornerRadius::same(2), t.selected);
                                ui.painter().rect_stroke(r, egui::CornerRadius::same(2), egui::Stroke::new(1.5, t.accent), egui::StrokeKind::Inside);
                            } else if resp.hovered() {
                                ui.painter().rect_filled(r, egui::CornerRadius::same(2), t.hover);
                            }
                            if let Some(tex) = &tex {
                                fitted(ui, tex, r.shrink(6.0));
                            }
                            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, *label));
                            let resp = resp.on_hover_text(*label);
                            if resp.clicked() {
                                new_kind = Some(k.to_string());
                            }
                            if resp.double_clicked() {
                                chose = true;
                            }
                            if i % 4 == 3 {
                                ui.end_row();
                            }
                        }
                    });
                });
            });
        });
        // The chosen layout.
        egui::Frame::NONE.stroke(egui::Stroke::new(1.0, t.border)).inner_margin(8.0).show(ui, |ui| {
            ui.set_width(280.0);
            ui.set_height(402.0);
            ui.vertical(|ui| {
                let (pic, _) = ui.allocate_exact_size(vec2(280.0, 170.0), egui::Sense::hover());
                let ctx = ui.ctx().clone();
                if let Some(tex) = app.session.active().and_then(|s| app.textures.smartart(&ctx, &s.doc, slide, &kind, "", "", None, 448)) {
                    fitted(ui, &tex, pic.shrink(8.0));
                }
                let name = LAYOUTS.iter().find(|(k, _)| *k == kind).map(|(_, l)| *l).unwrap_or("");
                ui.add_space(6.0);
                ui.label(RichText::new(name).font(crate::theme::bold(14.0)));
                ui.add_space(2.0);
                ui.label(smartart_about(&kind));
                ui.add_space(10.0);
                ui.label(RichText::new("Text, one item per line").color(t.text_dim));
                ui.add(egui::TextEdit::multiline(&mut text).desired_rows(6).desired_width(276.0));
            });
        });
    });
    let (mut ok, cancel) = ok_cancel(ui);
    ok |= chose;
    if let Some(c) = new_cat {
        d.fields.insert("cat".into(), c);
    }
    let kind = new_kind.unwrap_or(kind);
    d.fields.insert("kind".into(), kind.clone());
    d.fields.insert("text".into(), text.clone());
    if ok {
        let _ = app.run("insert.smartArt", json!({"kind": kind, "items": text.lines().filter(|l| !l.trim().is_empty()).collect::<Vec<_>>()}));
        app.ui.pane = Some("smartart".into());
    }
    ok || cancel
}

/// Insert Chart's categories and the types under each, in PowerPoint's order (Map is left out:
/// PowerPoint makes one with an online service).
fn chart_categories() -> Vec<(&'static str, &'static str, Vec<deckcraft_model::chart::ChartType>)> {
    use deckcraft_model::chart::ChartType as C;
    vec![
        ("Column", "Column", vec![C::Column, C::StackedColumn, C::PercentColumn]),
        ("Line", "Line", vec![C::Line, C::LineMarkers, C::StackedLine]),
        ("Pie", "Pie", vec![C::Pie, C::Doughnut]),
        ("Bar", "Bar", vec![C::Bar, C::StackedBar, C::PercentBar]),
        ("Area", "Area", vec![C::Area, C::StackedArea]),
        ("Scatter", "X Y (Scatter)", vec![C::Scatter, C::Bubble]),
        ("Stock", "Stock", vec![C::Stock]),
        ("Surface", "Surface", vec![C::Surface]),
        ("Radar", "Radar", vec![C::Radar, C::FilledRadar]),
        ("Treemap", "Treemap", vec![C::Treemap]),
        ("Sunburst", "Sunburst", vec![C::Sunburst]),
        ("Histogram", "Histogram", vec![C::Histogram, C::Pareto]),
        ("BoxWhisker", "Box & Whisker", vec![C::BoxWhisker]),
        ("Waterfall", "Waterfall", vec![C::Waterfall]),
        ("Funnel", "Funnel", vec![C::Funnel]),
        ("Combo", "Combo", vec![C::Combo]),
    ]
}

/// Insert › Chart (and Change Chart Type, with `change` set to the chart's id in the dialog's
/// params): the categories on the left, the category's types as small pictures across the top,
/// and the chosen type large with its name. Returns true when the dialog is done.
pub fn chart_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut crate::dialogs::Dialog) -> bool {
    let t = crate::theme::Tokens::get(ui.ctx());
    let cats = chart_categories();
    let change = d.params.get("change").and_then(Value::as_u64);
    let cat: usize = d.fields.get("cat").and_then(|v| v.parse().ok()).unwrap_or(0).min(cats.len() - 1);
    let mut ty = d.fields.get("type").cloned().unwrap_or_else(|| format!("{:?}", cats[cat].2[0]));
    let mut new_cat = None;
    let mut chose = false;
    let mut made = 0;
    ui.horizontal_top(|ui| {
        ui.set_height(440.0);
        egui::Frame::NONE.inner_margin(4.0).show(ui, |ui| {
            ui.set_width(160.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                for (i, (icon, label, _)) in cats.iter().enumerate() {
                    if category_row(ui, i == cat, icon, label).clicked() {
                        new_cat = Some(i);
                    }
                }
            });
        });
        ui.separator();
        ui.vertical(|ui| {
            ui.set_width(560.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for k in &cats[cat].2 {
                    let name = format!("{k:?}");
                    let tex = thumb(app, ui, &format!("chart-{name}"), &mut made, "insert.chart", json!({"type": name}), 240);
                    let (r, resp) = ui.allocate_exact_size(vec2(104.0, 72.0), egui::Sense::click());
                    if name == ty {
                        ui.painter().rect_filled(r, egui::CornerRadius::same(2), t.selected);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(r, egui::CornerRadius::same(2), t.hover);
                    }
                    if let Some(tex) = &tex {
                        fitted(ui, tex, r.shrink(5.0));
                    }
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, k.label()));
                    let resp = resp.on_hover_text(k.label());
                    if resp.clicked() {
                        ty = name.clone();
                    }
                    if resp.double_clicked() {
                        ty = name.clone();
                        chose = true;
                    }
                }
            });
            ui.separator();
            let label = cats.iter().flat_map(|c| c.2.iter()).find(|k| format!("{k:?}") == ty).map(|k| k.label()).unwrap_or("");
            ui.label(RichText::new(label).font(crate::theme::font(16.0)));
            ui.add_space(4.0);
            let (pic, _) = ui.allocate_exact_size(vec2(400.0, 280.0), egui::Sense::hover());
            ui.painter().rect_stroke(pic, egui::CornerRadius::ZERO, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
            if let Some(tex) = thumb(app, ui, &format!("chart-big-{ty}"), &mut made, "insert.chart", json!({"type": ty}), 640) {
                fitted(ui, &tex, pic.shrink(8.0));
            }
        });
    });
    let (mut ok, cancel) = ok_cancel(ui);
    ok |= chose;
    if let Some(c) = new_cat {
        d.fields.insert("cat".into(), c.to_string());
        ty = format!("{:?}", cats[c].2[0]);
    }
    d.fields.insert("type".into(), ty.clone());
    if ok {
        let _ = match change {
            Some(id) => app.run("chart.type", json!({"id": id, "type": ty})),
            None => app.run("insert.chart", json!({"type": ty})),
        };
    }
    ok || cancel
}
