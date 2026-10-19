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
        let _ = s.execute("shape.resize", &json!({"ids": [id], "w": 260.0}));
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

/// Insert › Chart and Change Chart Type: each chart type drawn with its sample data, four to a
/// row. `pick` gets the type clicked.
pub fn chart_menu(app: &mut SlideApp, ui: &mut Ui, types: &[Option<deckcraft_model::chart::ChartType>], mut pick: impl FnMut(&mut SlideApp, String)) {
    ui.set_min_width(4.0 * 112.0);
    let mut made = 0;
    let groups: Vec<Vec<deckcraft_model::chart::ChartType>> = types.split(Option::is_none).map(|g| g.iter().flatten().copied().collect()).collect();
    egui::ScrollArea::vertical().max_height(520.0).show(ui, |ui| {
        for (gi, group) in groups.iter().enumerate() {
            if gi > 0 {
                ui.separator();
            }
            egui::Grid::new(("deckrt-charts", gi)).spacing(vec2(4.0, 4.0)).show(ui, |ui| {
                for (i, k) in group.iter().enumerate() {
                    let ty = format!("{k:?}");
                    let tex = thumb(app, ui, &format!("chart-{ty}"), &mut made, "insert.chart", json!({"type": ty}), 240);
                    if tile(ui, tex.as_ref(), vec2(96.0, 64.0), k.label()).clicked() {
                        pick(app, ty.clone());
                        ui.close();
                    }
                    if i % 4 == 3 {
                        ui.end_row();
                    }
                }
            });
        }
    });
}

/// Insert › WordArt: the six styles drawn in the deck's colours.
pub fn wordart_menu(app: &mut SlideApp, ui: &mut Ui) {
    let mut made = 0;
    egui::Grid::new("deckrt-wordart").spacing(vec2(4.0, 4.0)).show(ui, |ui| {
        for i in 0..6usize {
            let tex = thumb(app, ui, &format!("wordart-{i}"), &mut made, "insert.wordArt", json!({"style": i, "text": "WordArt"}), 320);
            if tile(ui, tex.as_ref(), vec2(132.0, 40.0), &format!("Style {}", i + 1)).clicked() {
                let _ = app.run("insert.wordArt", json!({"style": i}));
                ui.close();
            }
            if i % 3 == 2 {
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
