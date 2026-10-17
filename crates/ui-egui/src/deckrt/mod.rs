//! deckrt: the interactive objects in the editor and the show. Everything deckrt adds to the
//! egui app lives under this module; the upstream files only call into it (see
//! `UPSTREAM_HOOKS.md`).

pub mod canvas;
pub mod captions;
pub mod check;
#[cfg(feature = "gpu3d")]
pub mod gpu;
#[cfg(not(feature = "gpu3d"))]
pub mod gpu {
    //! Without the `gpu3d` feature there is no wgpu path; models draw through the software
    //! rasteriser.
    use std::sync::Arc;

    pub struct Gpu;
    impl Gpu {
        pub fn end_frame(&mut self) {}
        pub fn has_model(&self, _key: u64) -> bool {
            false
        }
        pub fn load(&mut self, _key: u64, _model: Arc<deckcraft_model3d::Model>) {}
        pub fn draw(
            &mut self,
            _slot: u64,
            _key: u64,
            _view: &deckcraft_model3d::View,
            _w: u32,
            _h: u32,
            _hidden: &[String],
        ) -> Option<egui::TextureId> {
            None
        }
    }
}
pub mod model_view;
pub mod pane;
pub mod phones;
pub mod pictures;
pub mod privacy;
pub mod questions;
pub mod show;

use deckcraft_interactive::object::{self as obj, Kind};
use egui::Ui;
use serde_json::{Value, json};

use crate::SlideApp;
use crate::icons::Icon;
use crate::widgets::{group, rows};

/// The pane id of the object settings.
pub const PANE: &str = "interactive";

/// deckrt's own state on the app.
#[derive(Default)]
pub struct State {
    /// Where the user last clicked on the current slide, in slide points: where a new object lands.
    pub last_click: Option<(f64, f64)>,
    /// The slide `last_click` belongs to.
    pub click_slide: Option<(u64, usize)>,
    /// The Advanced section of the pane is open (kept for the session).
    pub advanced: bool,
    pub models: model_view::Models,
    pub gpu: Option<gpu::Gpu>,
    /// The name of the graphics adapter, for the status line.
    pub gpu_name: Option<String>,
    /// The shape whose turn handle is being dragged, with the pointer position at the last step.
    pub turning: Option<(u32, egui::Pos2)>,
    /// A picker is open (a second click on a ribbon button does nothing).
    pub picking: bool,
    /// Pictures the show draws, as textures.
    pub pictures: pictures::Pictures,
    /// Blur Area waiting for a drag over a picture.
    pub blur: Option<privacy::BlurTool>,
    /// Live captions: settings, the model download.
    pub captions: captions::Prefs,
    /// The per-user settings folder (the desktop app fills this in): the speech model lives here.
    pub settings_dir: Option<std::path::PathBuf>,
    /// The deck revision the check before showing last passed (or was waved through) at.
    pub check_passed: Option<u64>,
    /// The connected displays, asked for when a show starts (the desktop app fills this in).
    pub list_displays: Option<Box<dyn FnMut() -> Vec<show::DisplayInfo>>>,
}

impl State {
    /// The displays connected now, or none when the host cannot say.
    pub fn displays_now(&mut self) -> Vec<show::DisplayInfo> {
        self.list_displays.as_mut().map(|f| f()).unwrap_or_default()
    }
}

impl State {
    /// The live turn of a model in the editor.
    pub fn turn(&self, shape: u32) -> model_view::Turn {
        self.models.turns.get(&shape).copied().unwrap_or_default()
    }
}

/// The icon of each object type in the ribbon and the pane.
pub fn icon(kind: Kind) -> Icon {
    match kind {
        Kind::Model => Icon::Models3d,
        Kind::ImageSwap => Icon::ChangePicture,
        Kind::Wipe => Icon::Compare,
        Kind::Zoom => Icon::ZoomGlass,
        Kind::Popup => Icon::CommentsBubble,
        Kind::Poll => Icon::Chart,
        Kind::Scrub => Icon::Duration,
        Kind::Measure => Icon::Ruler,
        Kind::InjectionMap => Icon::Trigger,
    }
}

/// Once a frame, before the UI: the object pane follows the selection, and the model turn
/// state is dropped for shapes that are gone.
pub fn logic(app: &mut SlideApp) {
    let Some(st) = app.session.active() else { return };
    let selected = st.selected_shapes().first().map(|s| (s.id.0, obj::is_live(s)));
    match selected {
        Some((_, true)) if app.ui.pane.as_deref() == Some("format") || app.ui.pane.is_none() => app.ui.pane = Some(PANE.into()),
        Some((_, false)) | None if app.ui.pane.as_deref() == Some(PANE) => app.ui.pane = Some("format".into()),
        _ => {}
    }
    // The last click belongs to one slide of one presentation.
    let key = (st.uid, st.selection.slide);
    if app.deckrt.click_slide != Some(key) {
        app.deckrt.last_click = None;
        app.deckrt.click_slide = Some(key);
    }
    let live: Vec<u32> = st.shapes().iter().map(|s| s.id.0).collect();
    app.deckrt.models.turns.retain(|id, _| live.contains(id));
}

/// After the UI: textures of models no longer on screen are released.
pub fn end_frame(app: &mut SlideApp) {
    if let Some(g) = app.deckrt.gpu.as_mut() {
        g.end_frame();
    }
}

/// The Insert tab's Interactive group.
pub fn ribbon_group(app: &mut SlideApp, ui: &mut Ui) {
    let en = app.session.active().is_some_and(|d| !d.doc.slides.is_empty());
    group(ui, |ui| {
        if crate::widgets::big_button(ui, icon(Kind::Model), "3D\nModel", en).on_hover_text("3D model from a .glb, .obj or .stl file").clicked() {
            insert(app, Kind::Model);
        }
        let columns: [[Kind; 3]; 3] = [
            [Kind::ImageSwap, Kind::Wipe, Kind::Scrub],
            [Kind::Zoom, Kind::Popup, Kind::Poll],
            [Kind::Measure, Kind::InjectionMap, Kind::InjectionMap],
        ];
        for (ci, col) in columns.iter().enumerate() {
            rows(ui, |ui| {
                for (ri, k) in col.iter().enumerate() {
                    if ci == 2 && ri == 2 {
                        continue;
                    }
                    if crate::widgets::small_button(ui, icon(*k), k.label(), en, false).on_hover_text(tip(*k)).clicked() {
                        insert(app, *k);
                    }
                }
            });
        }
    });
}

fn tip(k: Kind) -> &'static str {
    match k {
        Kind::Model => "A 3D model from a .glb, .obj or .stl file",
        Kind::ImageSwap => "Pictures shown one at a time: click or press the key for the next",
        Kind::Wipe => "Before and after under one slider",
        Kind::Scrub => "2 to 12 pictures under one slider, each with a label",
        Kind::Zoom => "A region of the slide that fills the screen on a click",
        Kind::Popup => "Text, a picture or a web page that opens over the slide",
        Kind::Poll => "A question the room answers on their phones",
        Kind::Measure => "Facial thirds, fifths, an angle or a ratio over a photograph",
        Kind::InjectionMap => "Dose points placed over a photograph during the talk",
    }
}

/// Picture file extensions the pickers accept.
pub const IMAGE_EXTS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// Pick the files a type needs (a cancelled picker adds nothing) and insert the object where
/// the user last clicked.
pub fn insert(app: &mut SlideApp, kind: Kind) {
    if app.deckrt.picking {
        return;
    }
    let paths = match kind {
        Kind::Model => match pick_one(app, "model") {
            Some(p) => vec![p],
            None => return,
        },
        Kind::ImageSwap => {
            let p = pick_many(app, "pictures");
            if p.is_empty() {
                return;
            }
            p
        }
        Kind::Wipe => loop {
            let p = pick_many(app, "pictures");
            if p.is_empty() {
                return;
            }
            if p.len() == 2 {
                break p;
            }
            app.set_status(format!("Wipe needs two images. You picked {}. Pick the before image and the after image.", p.len()));
        },
        Kind::Scrub => loop {
            let p = pick_many(app, "pictures");
            if p.is_empty() {
                return;
            }
            if (2..=12).contains(&p.len()) {
                break p;
            }
            app.set_status(format!("Scrub needs 2 to 12 images in slider order. You picked {}.", p.len()));
        },
        _ => vec![],
    };
    insert_with_paths(app, kind, &paths);
}

/// Insert `kind` with files already chosen (the ribbon, drops and tests).
pub fn insert_with_paths(app: &mut SlideApp, kind: Kind, paths: &[String]) {
    let mut params = json!({"kind": kind.id()});
    // Where it lands: over the selected shape (an ordinary one), else where the user last
    // clicked, else the middle of the slide.
    let selected = app.session.active().and_then(|st| {
        let sh = st.selected_shapes().first().copied()?;
        if obj::is_live(sh) || sh.is_group() || sh.is_line() {
            return None;
        }
        let x = deckcraft_engine::cmd::xfrm_of(&st.doc, &st.selection, sh);
        Some((x.x + x.w / 2.0, x.y + x.h / 2.0))
    });
    if let Some((x, y)) = selected.or(app.deckrt.last_click) {
        params["at"] = json!([x, y]);
    }
    if !paths.is_empty() {
        params["paths"] = json!(paths);
    }
    match app.session.execute("interactive.insert", &params) {
        Ok(r) => {
            let anchored = r["object"]["anchor"].is_object();
            let slide = app.session.active().map(|d| d.selection.slide + 1).unwrap_or(1);
            let where_ = if anchored {
                let name = r["object"]["anchor"]["shape"]
                    .as_u64()
                    .and_then(|id| app.session.active().and_then(|d| d.shape(deckcraft_model::ShapeId(id as u32)).map(|s| s.name.clone())));
                match name {
                    Some(n) => format!("over {n}"),
                    None => format!("to slide {slide}"),
                }
            } else {
                format!("to slide {slide}")
            };
            app.set_status(format!("{} added {where_}.", kind.label()));
            app.ui.pane = Some(PANE.into());
            app.deckrt.advanced = false;
        }
        Err(e) => app.set_status(e.to_string()),
    }
    app.drain_requests();
}

fn pick_one(app: &mut SlideApp, purpose: &str) -> Option<String> {
    let pick = app.services.pick_open.as_mut()?;
    app.deckrt.picking = true;
    let r = pick(purpose);
    app.deckrt.picking = false;
    r
}

fn pick_many(app: &mut SlideApp, purpose: &str) -> Vec<String> {
    app.deckrt.picking = true;
    let r = match app.services.pick_open_many.as_mut() {
        Some(pick) => pick(purpose),
        None => app.services.pick_open.as_mut().and_then(|p| p(purpose)).into_iter().collect(),
    };
    app.deckrt.picking = false;
    r
}

/// Titles of the `deckrt.*` dialogs.
pub fn dialog_title(id: &str) -> &'static str {
    match id {
        "deckrt.imported" => "Interactive objects brought in",
        check::DIALOG => check::title(),
        captions::DIALOG => captions::title(),
        privacy::DIALOG => "Check before sharing",
        _ => "deckrt",
    }
}

/// Bodies of the `deckrt.*` dialogs. Returns whether the dialog closes.
pub fn dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut crate::dialogs::Dialog) -> bool {
    match d.id.as_str() {
        check::DIALOG => check::body(app, ui, d),
        captions::DIALOG => captions::body(app, ui),
        privacy::DIALOG => check::body(app, ui, d),
        "deckrt.imported" => {
            let text = d.params.get("text").and_then(Value::as_str).unwrap_or("").to_string();
            ui.set_max_width(460.0);
            ui.label(text);
            ui.add_space(8.0);
            let mut done = false;
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 26.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(egui::Button::new("OK").min_size(egui::vec2(72.0, 24.0))).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = true;
                }
            });
            done
        }
        _ => true,
    }
}

/// The object on the selected shape, if it is one.
pub fn selected(app: &SlideApp) -> Option<(deckcraft_model::Shape, obj::InteractiveObject)> {
    let st = app.session.active()?;
    let sh = st.selected_shapes().first().copied()?.clone();
    if obj::is_native_model(&sh) {
        // A PowerPoint model: the engine fills in its layers from the file.
        return deckcraft_engine::cmd::interactive::object_of(&app.session, sh.id).ok();
    }
    let o = obj::read(&sh)?;
    Some((sh, o))
}

/// Write changed settings back through the engine (one undo step).
pub fn update(app: &mut SlideApp, id: u32, o: &obj::InteractiveObject) {
    let v: Value = serde_json::from_str(&o.to_json()).unwrap_or(Value::Null);
    if let Err(e) = app.session.execute("interactive.update", &json!({"id": id, "object": v})) {
        app.set_status(e.to_string());
    }
}
