//! The settings pane of the selected interactive object: its name, the controls its type owns,
//! Delete, and an Advanced section that holds the rest.

use deckcraft_interactive::measure::Preset;
use deckcraft_interactive::object::{self as obj, Anchor, Config, InteractiveObject, Kind, MediaRef, PopupContent, Product, Trigger};
use deckcraft_model::{MediaId, ShapeId};
use egui::{RichText, Ui};
use serde_json::json;

use super::model_view;
use crate::SlideApp;
use crate::theme::{self, Tokens};
use crate::widgets;

/// A text field edited in place: the committed text comes back when editing ends (focus leaves
/// the field, or Enter). While it is not focused it shows `value`.
fn text_field(ui: &mut Ui, id: egui::Id, value: &str, multiline: bool) -> Option<String> {
    let focused = ui.memory(|m| m.has_focus(id));
    let mut buf: String = if focused { ui.data_mut(|d| d.get_temp::<String>(id)).unwrap_or_else(|| value.to_string()) } else { value.to_string() };
    let r = if multiline {
        ui.add(egui::TextEdit::multiline(&mut buf).id(id).desired_width(f32::INFINITY).desired_rows(3))
    } else {
        ui.add(egui::TextEdit::singleline(&mut buf).id(id).desired_width(f32::INFINITY))
    };
    ui.data_mut(|d| d.insert_temp(id, buf.clone()));
    let enter = !multiline && r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    if (r.lost_focus() || enter) && buf != value {
        ui.data_mut(|d| d.remove::<String>(id));
        return Some(buf);
    }
    None
}

fn label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).font(theme::bold(12.0)));
}

fn hint(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(text).color(t.text_dim).font(theme::font(11.0)));
}

fn media_name(app: &SlideApp, m: MediaId) -> String {
    app.session.active().and_then(|d| d.doc.media(m)).map(|m| m.name.clone()).unwrap_or_else(|| "missing picture".to_string())
}

fn pick_pictures(app: &mut SlideApp, many: bool) -> Vec<String> {
    app.deckrt.picking = true;
    let r = if many {
        match app.services.pick_open_many.as_mut() {
            Some(p) => p("pictures"),
            None => app.services.pick_open.as_mut().and_then(|p| p("pictures")).into_iter().collect(),
        }
    } else {
        app.services.pick_open.as_mut().and_then(|p| p("pictures")).into_iter().collect()
    };
    app.deckrt.picking = false;
    r
}

fn run(app: &mut SlideApp, cmd: &str, p: serde_json::Value) {
    if let Err(e) = app.session.execute(cmd, &p) {
        app.set_status(e.to_string());
    }
}

pub fn show(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some((sh, o)) = super::selected(app) else {
        ui.label(RichText::new("Select an interactive object to change it.").color(t.text_dim));
        return;
    };
    let id = sh.id.0;
    let kind = o.kind();
    let mut next = o.clone();
    let mut changed = false;
    let fid = |name: &str| egui::Id::new(("deckrt-pane", id, name));

    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
        crate::icons::paint(ui.painter(), r, super::icon(kind), t.text, false);
        ui.label(RichText::new(kind.label()).color(t.text_dim));
    });
    label(ui, "Name");
    if let Some(v) = text_field(ui, fid("name"), &o.label, false) {
        next.label = v;
        changed = true;
    }
    ui.add_space(6.0);

    match &mut next.config {
        Config::Model(c) => {
            label(ui, "Model file");
            let file = match &sh.kind {
                deckcraft_model::ShapeKind::Model3d { media, .. } => media_name(app, *media),
                _ => "missing file".into(),
            };
            ui.horizontal(|ui| {
                ui.label(file);
                if ui.button("Replace file…").clicked() {
                    app.deckrt.picking = true;
                    let path = app.services.pick_open.as_mut().and_then(|p| p("model"));
                    app.deckrt.picking = false;
                    if let Some(path) = path {
                        run(app, "interactive.replaceModel", json!({"id": id, "path": path}));
                        app.deckrt.models.turns.remove(&id);
                    }
                }
            });
            ui.add_space(4.0);
            label(ui, "Layers");
            if c.layers.is_empty() {
                hint(ui, "This model has no named parts.");
            } else {
                hint(ui, "Ticked layers show when the slide opens.");
            }
            for layer in c.layers.clone() {
                let mut on = !c.hidden_layers.contains(&layer);
                if ui.checkbox(&mut on, &layer).changed() {
                    if on {
                        c.hidden_layers.retain(|h| h != &layer);
                    } else {
                        c.hidden_layers.push(layer.clone());
                    }
                    changed = true;
                }
            }
            hint(ui, "Drag the round handle on the slide to turn the model; use the wheel to zoom.");
        }
        Config::ImageSwap(c) => {
            label(ui, "Pictures");
            hint(ui, "Shown in this order; a click or the key shows the next one.");
            let list = c.images.clone();
            let n = list.len();
            for (i, m) in list.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("{}.", i + 1));
                    ui.add(egui::Label::new(media_name(app, m.0)).truncate());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(n > 1, egui::Button::new("Remove")).clicked() {
                            run(app, "interactive.removeImage", json!({"id": id, "index": i}));
                        }
                        if ui.add_enabled(i + 1 < n, egui::Button::new("Down")).clicked() {
                            c.images.swap(i, i + 1);
                            changed = true;
                        }
                        if ui.add_enabled(i > 0, egui::Button::new("Up")).clicked() {
                            c.images.swap(i, i - 1);
                            changed = true;
                        }
                    });
                });
            }
            if ui.button("Add pictures…").clicked() {
                for path in pick_pictures(app, true) {
                    run(app, "interactive.addImage", json!({"id": id, "path": path}));
                }
            }
        }
        Config::Scrub(c) => {
            label(ui, "Photographs");
            hint(ui, "In slider order, each with its label: 0 U, Day 7.");
            let n = c.images.len();
            let imgs = c.images.clone();
            for (i, m) in imgs.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("{}.", i + 1));
                    let cur = c.labels.get(i).cloned().unwrap_or_default();
                    ui.scope(|ui| {
                        ui.set_max_width(110.0);
                        if let Some(v) = text_field(ui, fid(&format!("label{i}")), &cur, false) {
                            while c.labels.len() <= i {
                                c.labels.push(String::new());
                            }
                            c.labels[i] = v;
                            changed = true;
                        }
                    });
                    ui.add(egui::Label::new(media_name(app, m.0)).truncate());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add_enabled(n > 2, egui::Button::new("Remove")).clicked() {
                            run(app, "interactive.removeImage", json!({"id": id, "index": i}));
                        }
                        if ui.add_enabled(i + 1 < n, egui::Button::new("Down")).clicked() {
                            c.images.swap(i, i + 1);
                            if i + 1 < c.labels.len() {
                                c.labels.swap(i, i + 1);
                            }
                            changed = true;
                        }
                        if ui.add_enabled(i > 0, egui::Button::new("Up")).clicked() {
                            c.images.swap(i, i - 1);
                            if i < c.labels.len() {
                                c.labels.swap(i, i - 1);
                            }
                            changed = true;
                        }
                    });
                });
            }
            if ui.add_enabled(n < 12, egui::Button::new("Add photographs…")).clicked() {
                for path in pick_pictures(app, true) {
                    run(app, "interactive.addImage", json!({"id": id, "path": path}));
                }
            }
        }
        Config::Wipe(c) => {
            for (slot, name, m) in [("before", "Before image", c.before), ("after", "After image", c.after)] {
                label(ui, name);
                ui.horizontal(|ui| {
                    ui.add(egui::Label::new(media_name(app, m.0)).truncate());
                    if ui.button("Replace file…").clicked()
                        && let Some(path) = pick_pictures(app, false).into_iter().next()
                    {
                        run(app, "interactive.addImage", json!({"id": id, "path": path, "slot": slot}));
                    }
                });
            }
        }
        Config::Popup(c) => {
            label(ui, "Content");
            let kind_now = match &c.content {
                PopupContent::Text { .. } => 0,
                PopupContent::Image { .. } => 1,
                PopupContent::Web { .. } => 2,
            };
            ui.horizontal(|ui| {
                for (i, name) in ["Text", "Picture", "Web page"].iter().enumerate() {
                    if ui.selectable_label(kind_now == i, *name).clicked() && kind_now != i {
                        c.content = match i {
                            0 => PopupContent::Text { text: "Type the popup text here.".into() },
                            1 => PopupContent::Image { media: MediaRef(MediaId(0)) },
                            _ => PopupContent::Web { url: "https://".into() },
                        };
                        changed = true;
                    }
                }
            });
            match &mut c.content {
                PopupContent::Text { text } => {
                    if let Some(v) = text_field(ui, fid("popup-text"), text, true) {
                        *text = v;
                        changed = true;
                    }
                }
                PopupContent::Image { media } => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Label::new(if media.0 == MediaId(0) { "No picture yet".to_string() } else { media_name(app, media.0) }).truncate());
                        if ui.button("Choose picture…").clicked()
                            && let Some(path) = pick_pictures(app, false).into_iter().next()
                        {
                            run(app, "interactive.addImage", json!({"id": id, "path": path, "slot": "popup"}));
                        }
                    });
                }
                PopupContent::Web { url } => {
                    if let Some(v) = text_field(ui, fid("popup-url"), url, false) {
                        *url = v;
                        changed = true;
                    }
                    hint(ui, "The show opens the address in the browser, beside the slide.");
                }
            }
        }
        Config::Zoom(c) => {
            label(ui, "Region");
            hint(ui, "The part of the slide that fills the screen, in slide units 0 to 1.");
            let mut r = c.source_rect;
            ui.horizontal(|ui| {
                for (i, name) in ["x", "y", "w", "h"].iter().enumerate() {
                    ui.label(*name);
                    if ui.add(egui::DragValue::new(&mut r[i]).speed(0.005).range(0.0..=1.0).fixed_decimals(3)).changed() {
                        changed = true;
                    }
                }
            });
            c.source_rect = r;
            if ui.button("Use the box").clicked()
                && let Some(st) = app.session.active()
            {
                let size = st.doc.slide_size;
                if let Some(x) = sh.xfrm {
                    c.source_rect = [x.x / size.width.max(1.0), x.y / size.height.max(1.0), x.w / size.width.max(1.0), x.h / size.height.max(1.0)];
                    changed = true;
                }
            }
            hint(ui, "Use the box makes the region the object's own box.");
        }
        Config::Poll(c) => {
            label(ui, "Question");
            if let Some(v) = text_field(ui, fid("question"), &c.question, false) {
                c.question = v;
                changed = true;
            }
            label(ui, "Answers");
            let n = c.options.len();
            for i in 0..n {
                ui.horizontal(|ui| {
                    let mut right = c.correct == Some(i);
                    if ui.radio_value(&mut right, true, "").on_hover_text("The right answer").clicked() {
                        c.correct = Some(i);
                        changed = true;
                    }
                    let cur = c.options.get(i).cloned().unwrap_or_default();
                    if let Some(v) = text_field(ui, fid(&format!("opt{i}")), &cur, false) {
                        c.options[i] = v;
                        changed = true;
                    }
                    if ui.add_enabled(n > 2, egui::Button::new("Remove")).clicked() {
                        c.options.remove(i);
                        if i < c.targets.len() {
                            c.targets.remove(i);
                        }
                        c.correct = match c.correct {
                            Some(k) if k == i => None,
                            Some(k) if k > i => Some(k - 1),
                            k => k,
                        };
                        changed = true;
                    }
                });
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(n < 64, egui::Button::new("Add answer")).clicked() {
                    c.options.push(String::new());
                    changed = true;
                }
                if ui.button("True or false").clicked() {
                    c.options = vec!["True".into(), "False".into()];
                    c.correct = None;
                    c.targets.clear();
                    changed = true;
                }
                if ui.add_enabled(c.correct.is_some(), egui::Button::new("No right answer")).clicked() {
                    c.correct = None;
                    changed = true;
                }
            });
            if ui.checkbox(&mut c.reveal_correct, "The right answer can be revealed").changed() {
                changed = true;
            }
        }
        Config::Measure(c) => {
            label(ui, "Measurement");
            ui.horizontal(|ui| {
                for p in Preset::ALL {
                    if ui.selectable_label(c.preset == p, p.label()).clicked() {
                        c.preset = p;
                        changed = true;
                    }
                }
            });
            ui.horizontal(|ui| {
                label(ui, "Colour");
                let mut rgb = obj::parse_color(&c.color);
                if ui.color_edit_button_srgb(&mut rgb).changed() {
                    c.color = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
                    changed = true;
                }
            });
            hint(ui, "Put the box over the photograph. The handles are placed during the talk.");
        }
        Config::InjectionMap(c) => {
            label(ui, "Products");
            let n = c.products.len();
            for i in 0..n {
                ui.horizontal(|ui| {
                    let mut rgb = obj::parse_color(&c.products[i].color);
                    if ui.color_edit_button_srgb(&mut rgb).changed() {
                        c.products[i].color = format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2]);
                        changed = true;
                    }
                    ui.scope(|ui| {
                        ui.set_max_width(90.0);
                        if let Some(v) = text_field(ui, fid(&format!("prod{i}")), &c.products[i].name.clone(), false) {
                            c.products[i].name = v;
                            changed = true;
                        }
                    });
                    if ui.add(egui::DragValue::new(&mut c.products[i].default_dose).speed(0.1).range(0.0..=1000.0).max_decimals(2)).changed() {
                        changed = true;
                    }
                    ui.scope(|ui| {
                        ui.set_max_width(36.0);
                        if let Some(v) = text_field(ui, fid(&format!("unit{i}")), &c.products[i].unit.clone(), false) {
                            c.products[i].unit = v;
                            changed = true;
                        }
                    });
                    if ui.add_enabled(n > 1, egui::Button::new("Remove")).clicked() {
                        c.products.remove(i);
                        changed = true;
                    }
                });
            }
            if ui.add_enabled(n < 20, egui::Button::new("Add a product")).clicked() {
                c.products.push(Product::default());
                changed = true;
            }
            if ui.checkbox(&mut c.show_total, "Show the total").changed() {
                changed = true;
            }
            hint(ui, "Colour, name, dose and unit. The points are placed during the talk and never saved.");
        }
    }

    ui.add_space(8.0);
    if ui.button("Delete").clicked() {
        run(app, "interactive.delete", json!({"id": id}));
        return;
    }
    ui.add_space(8.0);
    app.deckrt.advanced = widgets::section(ui, "Advanced", app.deckrt.advanced);
    if app.deckrt.advanced {
        changed |= advanced(app, ui, &sh, &mut next, fid);
    }
    if changed && next != o {
        super::update(app, id, &next);
    }
}

fn advanced(app: &mut SlideApp, ui: &mut Ui, sh: &deckcraft_model::Shape, next: &mut InteractiveObject, fid: impl Fn(&str) -> egui::Id) -> bool {
    let mut changed = false;
    let id = sh.id.0;
    label(ui, "Behaviour");
    ui.horizontal(|ui| {
        ui.label("Fired by");
        for (tr, name) in [(Trigger::Presenter, "the presenter"), (Trigger::AudienceClick, "a click on the screen")] {
            if ui.selectable_label(next.trigger == tr, name).clicked() {
                next.trigger = tr;
                changed = true;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label("Shortcut key");
        let cur = next.shortcut.map(|c| c.to_string()).unwrap_or_default();
        ui.scope(|ui| {
            ui.set_max_width(40.0);
            if let Some(v) = text_field(ui, fid("shortcut"), &cur, false) {
                next.shortcut = v.trim().chars().next().map(|c| c.to_ascii_lowercase());
                changed = true;
            }
        });
        hint(ui, "One letter or digit pressed during the show.");
    });
    if ui.checkbox(&mut next.visible, "Visible when the slide opens").changed() {
        changed = true;
    }
    ui.add_space(6.0);
    label(ui, "Position and size");
    if let Some(st) = app.session.active() {
        let size = st.doc.slide_size;
        let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh);
        let mut v = [x.x / size.width.max(1.0), x.y / size.height.max(1.0), x.w / size.width.max(1.0), x.h / size.height.max(1.0)];
        let mut moved = false;
        ui.horizontal(|ui| {
            for (i, name) in ["x", "y", "w", "h"].iter().enumerate() {
                ui.label(*name);
                if ui.add(egui::DragValue::new(&mut v[i]).speed(0.005).range(0.0..=1.0).fixed_decimals(3)).changed() {
                    moved = true;
                }
            }
        });
        hint(ui, "In slide units, 0 to 1 across the slide.");
        if moved {
            run(
                app,
                "shape.setBounds",
                json!({"ids": [id], "x": v[0] * size.width, "y": v[1] * size.height, "w": (v[2] * size.width).max(4.0), "h": (v[3] * size.height).max(4.0)}),
            );
        }
    }
    ui.add_space(6.0);
    label(ui, "Moves with");
    let shapes: Vec<(u32, String)> = app
        .session
        .active()
        .map(|st| st.shapes().iter().filter(|s| s.id.0 != id && !obj::is_interactive(s)).map(|s| (s.id.0, s.name.clone())).collect())
        .unwrap_or_default();
    let cur = next.anchor.map(|a| a.shape);
    let cur_name = cur.and_then(|c| shapes.iter().find(|(i, _)| *i == c).map(|(_, n)| n.clone())).unwrap_or_else(|| "The slide".into());
    egui::ComboBox::from_id_salt(fid("anchor")).selected_text(cur_name).show_ui(ui, |ui| {
        if ui.selectable_label(cur.is_none(), "The slide").clicked() && cur.is_some() {
            next.anchor = None;
            changed = true;
        }
        for (sid, name) in &shapes {
            if ui.selectable_label(cur == Some(*sid), name).clicked() && cur != Some(*sid) {
                let (dx, dy) = app
                    .session
                    .active()
                    .and_then(|st| {
                        let a = st.shape(ShapeId(*sid)).map(|s| deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, s))?;
                        let me = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh);
                        Some((me.x - a.x, me.y - a.y))
                    })
                    .unwrap_or((0.0, 0.0));
                next.anchor = Some(Anchor { shape: *sid, dx, dy });
                changed = true;
            }
        }
    });
    hint(ui, "When that shape moves, the object moves with it.");
    ui.add_space(6.0);
    match &mut next.config {
        Config::Model(c) => {
            label(ui, "Initial rotation");
            ui.horizontal(|ui| {
                for (i, name) in ["x", "y", "z"].iter().enumerate() {
                    ui.label(*name);
                    let mut deg = c.initial_rotation[i].to_degrees();
                    if ui.add(egui::DragValue::new(&mut deg).speed(1.0).suffix("°").fixed_decimals(0)).changed() {
                        c.initial_rotation[i] = deg.to_radians();
                        changed = true;
                    }
                }
            });
            label(ui, "Camera when the slide opens");
            ui.horizontal(|ui| {
                let turn = app.deckrt.turn(id);
                if ui.add_enabled(!turn.is_zero(), egui::Button::new("Use the current view")).clicked()
                    && let deckcraft_model::ShapeKind::Model3d { media, .. } = &sh.kind
                    && let Some(doc) = app.session.active().map(|d| d.doc.clone())
                    && let Some(model) = app.deckrt.models.model(&doc, *media)
                {
                    let view = model_view::view_of(&model, c, &turn);
                    c.camera = Some(model_view::pose_of(&view));
                    app.deckrt.models.turns.remove(&id);
                    changed = true;
                }
                if ui.add_enabled(c.camera.is_some(), egui::Button::new("Automatic")).clicked() {
                    c.camera = None;
                    app.deckrt.models.turns.remove(&id);
                    changed = true;
                }
            });
            hint(ui, match c.camera {
                Some(_) => "The saved view. Automatic frames the whole model.",
                None => "Automatic: the whole model framed from the front.",
            });
            label(ui, "Turns together with");
            if let Some(v) = text_field(ui, fid("link"), &c.link_group, false) {
                c.link_group = v.trim().to_string();
                changed = true;
            }
            hint(ui, "Models on this slide with the same name here turn together in the show.");
        }
        Config::ImageSwap(c) => {
            ui.horizontal(|ui| {
                ui.label("Picture shown first");
                let n = c.images.len().max(1);
                let mut first = c.index + 1;
                if ui.add(egui::DragValue::new(&mut first).range(1..=n)).changed() {
                    c.index = first - 1;
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("On click");
                for (v, name) in [("advance", "Next picture"), ("picker", "A strip to choose from")] {
                    if ui.selectable_label(c.on_click == v, name).clicked() {
                        c.on_click = v.into();
                        changed = true;
                    }
                }
            });
        }
        Config::Scrub(c) => {
            ui.horizontal(|ui| {
                ui.label("Photograph shown first");
                let n = c.images.len().max(1);
                let mut first = c.initial_index + 1;
                if ui.add(egui::DragValue::new(&mut first).range(1..=n)).changed() {
                    c.initial_index = first - 1;
                    changed = true;
                }
            });
        }
        Config::Wipe(c) => {
            ui.horizontal(|ui| {
                ui.label("Divider when the slide opens");
                if ui.add(egui::Slider::new(&mut c.initial_position, 0.0..=1.0).show_value(false)).changed() {
                    changed = true;
                }
            });
        }
        Config::Zoom(c) => {
            ui.horizontal(|ui| {
                ui.label("Largest magnification");
                if ui.add(egui::DragValue::new(&mut c.max_scale).speed(0.1).range(1.0..=20.0).suffix("×").fixed_decimals(1)).changed() {
                    changed = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Zoom time");
                if ui.add(egui::DragValue::new(&mut c.duration_ms).speed(10).range(0..=5000).suffix(" ms")).changed() {
                    changed = true;
                }
            });
        }
        Config::Popup(c) => {
            label(ui, "Panel position and size");
            let mut v = [c.panel_position[0], c.panel_position[1], c.panel_size[0], c.panel_size[1]];
            ui.horizontal(|ui| {
                for (i, name) in ["x", "y", "w", "h"].iter().enumerate() {
                    ui.label(*name);
                    if ui.add(egui::DragValue::new(&mut v[i]).speed(0.005).range(0.0..=1.0).fixed_decimals(2)).changed() {
                        changed = true;
                    }
                }
            });
            c.panel_position = [v[0], v[1]];
            c.panel_size = [v[2], v[3]];
            hint(ui, "In slide units. The panel opens over the slide at this place.");
        }
        Config::Poll(c) => {
            label(ui, "After the vote");
            hint(ui, "The winning answer offers its slide to the presenter.");
            let slides: Vec<(usize, String)> = app
                .session
                .active()
                .map(|st| {
                    st.doc.slides.iter().enumerate().map(|(i, s)| (i, format!("Slide {} {}", i + 1, s.title().chars().take(30).collect::<String>()))).collect()
                })
                .unwrap_or_default();
            while c.targets.len() < c.options.len() {
                c.targets.push(None);
            }
            for i in 0..c.options.len() {
                let cur = c.targets.get(i).cloned().flatten();
                let cur_idx = cur.as_deref().and_then(|k| app.session.active().and_then(|st| deckcraft_engine::cmd::interactive::slide_of_key(&st.doc, k)));
                let text = match cur_idx {
                    Some(i) => slides.get(i).map(|(_, n)| n.clone()).unwrap_or_else(|| "No slide".into()),
                    None if cur.is_some() => "A slide that is gone".into(),
                    None => "No slide".into(),
                };
                ui.horizontal(|ui| {
                    ui.label(c.options.get(i).cloned().unwrap_or_default());
                    egui::ComboBox::from_id_salt(fid(&format!("target{i}"))).selected_text(text).show_ui(ui, |ui| {
                        if ui.selectable_label(cur.is_none(), "No slide").clicked() {
                            c.targets[i] = None;
                            changed = true;
                        }
                        for (si, name) in &slides {
                            if ui.selectable_label(cur_idx == Some(*si), name).clicked() {
                                match deckcraft_engine::cmd::interactive::slide_key(&mut app.session, *si) {
                                    Ok(k) => {
                                        c.targets[i] = Some(k);
                                        changed = true;
                                    }
                                    Err(e) => app.set_status(e.to_string()),
                                }
                            }
                        }
                    });
                });
            }
        }
        Config::Measure(c) => {
            ui.horizontal(|ui| {
                ui.label("Line width");
                if ui.add(egui::DragValue::new(&mut c.line_width).speed(0.5).range(1.0..=20.0).suffix(" px")).changed() {
                    changed = true;
                }
            });
        }
        Config::InjectionMap(_) => {}
    }
    let _ = Kind::Model;
    changed
}
