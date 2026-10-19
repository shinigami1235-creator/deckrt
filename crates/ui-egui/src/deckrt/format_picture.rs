//! deckrt: finer picture and transparency controls. Every value is a slider with a number box;
//! dragging shows the change on the slide as it moves and the drag is one undo step.
//!
//! Used by Format Shape › Picture, the Picture Format tab's Corrections, Color and Transparency
//! menus, and the Shape Fill menus.

use deckcraft_engine::cmd::deckrt_live;
use deckcraft_model::ShapeKind;
use deckcraft_model::style::Fill;
use egui::{RichText, Ui, vec2};
use serde_json::{Value, json};

use crate::SlideApp;
use crate::theme;

/// A slider and a number box for one value. `value` is in the units shown (per cent, say);
/// `param` turns it into the command's parameters. Returns true when the value changed.
#[allow(clippy::too_many_arguments)]
pub fn live_row(
    app: &mut SlideApp,
    ui: &mut Ui,
    label: &str,
    value: f64,
    range: std::ops::RangeInclusive<f64>,
    suffix: &str,
    cmd: &str,
    param: impl Fn(f64) -> Value,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(vec2(96.0, 20.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_width(96.0);
            ui.label(label);
        });
        let mut v = value;
        let slider_w = (ui.available_width() - 74.0).clamp(80.0, 200.0);
        ui.spacing_mut().slider_width = slider_w;
        // The rail in the theme's border grey, so it shows on a white menu.
        let rail = crate::theme::Tokens::get(ui.ctx()).border;
        ui.visuals_mut().widgets.inactive.bg_fill = rail;
        ui.visuals_mut().widgets.hovered.bg_fill = rail;
        let r = ui.add(egui::Slider::new(&mut v, range.clone()).show_value(false).trailing_fill(true));
        changed |= live(app, &r, label, v, value, cmd, &param);
        let mut n = v;
        let d = ui.add_sized(vec2(62.0, 20.0), egui::DragValue::new(&mut n).update_while_editing(false).range(range).suffix(suffix).speed(0.5).max_decimals(0));
        changed |= live(app, &d, label, n, value, cmd, &param);
    });
    changed
}

/// One widget's part of a live row: a drag runs the command at every step inside one live
/// undo step; a click or a typed number runs it once.
fn live(app: &mut SlideApp, r: &egui::Response, label: &str, v: f64, before: f64, cmd: &str, param: &impl Fn(f64) -> Value) -> bool {
    if r.drag_started() {
        deckrt_live::begin(&mut app.session, label);
    }
    if r.dragged() || r.drag_started() {
        app.deckrt.live_touched = true;
    }
    let moved = r.changed() && (v - before).abs() > 1e-9;
    if moved {
        let _ = app.run(cmd, param(v));
    }
    if r.drag_stopped() {
        deckrt_live::end(&mut app.session);
    }
    moved
}

/// After the UI: a live drag whose slider went away (a menu closed mid-drag) is ended, so the
/// next edit gets an undo step of its own.
pub fn end_frame(app: &mut SlideApp) {
    if !app.deckrt.live_touched && deckrt_live::open_label(&app.session).is_some() {
        deckrt_live::end(&mut app.session);
    }
    app.deckrt.live_touched = false;
}

/// The selected picture's adjustments, transparency (0 to 1) and crop.
fn picture(app: &SlideApp) -> Option<(deckcraft_model::style::PictureAdjust, f64, [f64; 4])> {
    let st = app.session.active()?;
    st.selected_shapes().iter().find_map(|sh| match &sh.kind {
        ShapeKind::Picture { fill } => Some((fill.adjust.clone(), 1.0 - fill.alpha.unwrap_or(1.0), fill.crop)),
        _ => None,
    })
}

fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(RichText::new(text).font(theme::bold(13.0)));
}

fn brightness_contrast(app: &mut SlideApp, ui: &mut Ui, adj: &deckcraft_model::style::PictureAdjust) {
    live_row(app, ui, "Brightness", (adj.brightness * 100.0).round(), -100.0..=100.0, "%", "picture.adjust", |v| json!({"brightness": v / 100.0}));
    live_row(app, ui, "Contrast", (adj.contrast * 100.0).round(), -100.0..=100.0, "%", "picture.adjust", |v| json!({"contrast": v / 100.0}));
}

fn saturation(app: &mut SlideApp, ui: &mut Ui, adj: &deckcraft_model::style::PictureAdjust) {
    let s = (adj.saturation.unwrap_or(1.0) * 100.0).round();
    live_row(app, ui, "Saturation", s, 0.0..=400.0, "%", "picture.adjust", |v| json!({"saturation": v / 100.0}));
}

fn transparency(app: &mut SlideApp, ui: &mut Ui, tr: f64) {
    live_row(app, ui, "Transparency", (tr * 100.0).round(), 0.0..=100.0, "%", "picture.adjust", |v| json!({"transparency": v / 100.0}));
}

/// Format Shape › Picture: corrections, colour, transparency and crop, each side by side.
pub fn pane(app: &mut SlideApp, ui: &mut Ui) {
    let Some((adj, tr, crop)) = picture(app) else { return };
    heading(ui, "Picture Corrections");
    brightness_contrast(app, ui, &adj);
    if ui.small_button("Reset corrections").clicked() {
        let _ = app.run("picture.adjust", json!({"brightness": 0.0, "contrast": 0.0}));
    }
    heading(ui, "Picture Color");
    saturation(app, ui, &adj);
    let mut grey = adj.grayscale;
    if ui.checkbox(&mut grey, "Grayscale").changed() {
        let _ = app.run("picture.adjust", json!({"grayscale": grey}));
    }
    heading(ui, "Picture Transparency");
    transparency(app, ui, tr);
    heading(ui, "Crop");
    for (i, (label, key)) in [("Left", "left"), ("Top", "top"), ("Right", "right"), ("Bottom", "bottom")].into_iter().enumerate() {
        let v = (crop.get(i).copied().unwrap_or(0.0) * 100.0).round();
        live_row(app, ui, label, v.clamp(0.0, 95.0), 0.0..=95.0, "%", "picture.crop", |v| json!({key: v / 100.0}));
    }
    ui.add_space(6.0);
    if ui.button("Reset Picture").on_hover_text("Undo every correction, colour change, crop and style on this picture").clicked() {
        let _ = app.run("picture.reset", json!({}));
    }
}

fn options_link(app: &mut SlideApp, ui: &mut Ui, label: &str) {
    ui.separator();
    if ui.button(label).clicked() {
        app.ui.pane = Some("format".into());
        app.ui.format_tab = "picture".into();
        ui.close();
    }
}

/// Picture Format › Corrections: the sliders, then steps of 20%.
pub fn corrections_menu(app: &mut SlideApp, ui: &mut Ui) {
    ui.set_min_width(300.0);
    let Some((adj, _, _)) = picture(app) else { return };
    brightness_contrast(app, ui, &adj);
    ui.separator();
    ui.label(RichText::new("Brightness and contrast").small());
    egui::Grid::new("corrections-grid").spacing(vec2(3.0, 3.0)).show(ui, |ui| {
        for c in [-0.4, -0.2, 0.0, 0.2, 0.4] {
            for b in [-0.4, -0.2, 0.0, 0.2, 0.4] {
                let on = (adj.brightness - b).abs() < 0.01 && (adj.contrast - c).abs() < 0.01;
                let text = format!("{:+}/{:+}", (b * 100.0) as i32, (c * 100.0) as i32);
                if ui
                    .add(egui::Button::selectable(on, RichText::new(text).size(10.0)).min_size(vec2(50.0, 20.0)))
                    .on_hover_text(format!("Brightness {:+}%, Contrast {:+}%", (b * 100.0) as i32, (c * 100.0) as i32))
                    .clicked()
                {
                    let _ = app.run("picture.adjust", json!({"brightness": b, "contrast": c}));
                }
            }
            ui.end_row();
        }
    });
    options_link(app, ui, "Picture Corrections Options…");
}

/// Picture Format › Color: the saturation slider above upstream's recolor menu.
pub fn color_sliders(app: &mut SlideApp, ui: &mut Ui) {
    ui.set_min_width(300.0);
    let Some((adj, _, _)) = picture(app) else { return };
    saturation(app, ui, &adj);
    ui.horizontal_wrapped(|ui| {
        for s in [0.0, 0.33, 0.66, 1.0, 1.5, 2.0, 3.0, 4.0] {
            let on = (adj.saturation.unwrap_or(1.0) - s).abs() < 0.01;
            if ui.selectable_label(on, format!("{}%", (s * 100.0) as i32)).clicked() {
                let _ = app.run("picture.adjust", json!({"saturation": s}));
            }
        }
    });
}

/// The link at the bottom of the Color menu.
pub fn color_options(app: &mut SlideApp, ui: &mut Ui) {
    options_link(app, ui, "Picture Color Options…");
}

/// Picture Format › Transparency: the slider, then steps.
pub fn transparency_menu(app: &mut SlideApp, ui: &mut Ui) {
    ui.set_min_width(300.0);
    let Some((_, tr, _)) = picture(app) else { return };
    transparency(app, ui, tr);
    ui.horizontal_wrapped(|ui| {
        for t in [0.0, 0.15, 0.3, 0.5, 0.65, 0.8, 0.95] {
            let on = (tr - t).abs() < 0.005;
            if ui.selectable_label(on, format!("{}%", (t * 100.0) as i32)).clicked() {
                let _ = app.run("picture.adjust", json!({"transparency": t}));
            }
        }
    });
    options_link(app, ui, "Picture Transparency Options…");
}

/// Shape Fill menus: the transparency of the selected shape's solid fill.
pub fn fill_transparency(app: &mut SlideApp, ui: &mut Ui) {
    let fill = app.session.active().and_then(|st| st.selected_shapes().first().and_then(|sh| sh.fill.clone()));
    let Some(Fill::Solid { color }) = fill else { return };
    ui.separator();
    let tr = ((1.0 - color.alpha()) * 100.0).round();
    live_row(app, ui, "Transparency", tr, 0.0..=100.0, "%", "shape.fill", |v| json!({"transparency": v / 100.0}));
}
