//! The interactive objects during a slide show: their live state, how they are drawn over the
//! slide on the audience screen and in the presenter view, the pointer and keys they take, the
//! presenter's control rows, and which display the show goes to. The phones are in
//! [`super::phones`].
//!
//! Upstream's `show.rs` calls in at a few marked points: when the show starts (displays), before
//! the pointer is read (`pointer`), per key (`shortcut`), after the slide picture is painted
//! (`draw_audience`), in the presenter window, and when the show ends.

use std::collections::BTreeMap;
use std::sync::Arc;

use deckcraft_interactive::injection::{self, Point};
use deckcraft_interactive::measure;
use deckcraft_interactive::object::{self as obj, Config, InteractiveObject, Kind, PollKind, PopupContent, Trigger};
use deckcraft_interactive::state::{ObjectState, TypeState, zoom_view};
use deckcraft_interactive::{poll, state};
use deckcraft_model::{MediaId, Presentation, Shape, ShapeId, ShapeKind};
use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Stroke, StrokeKind, Ui, pos2, vec2};

use super::model_view::Turn;
use super::phones::Phones;
use crate::show::Show;

/// Who is looking: the audience screen takes input only where the object allows it; the
/// presenter's own view takes everything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Audience,
    Presenter,
}

pub use deckcraft_show_ext::DisplayInfo;

/// A drag in progress on an object.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drag {
    /// Turning a model: the pointer position at the last step.
    Model {
        id: u32,
        last: Pos2,
    },
    Wipe {
        id: u32,
    },
    Scrub {
        id: u32,
    },
    Measure {
        id: u32,
        handle: usize,
    },
}

/// A stroke a phone drew on the slide, in slide units (0 to 1).
#[derive(Clone, Debug, PartialEq)]
pub struct InkStroke {
    /// `<phone id>:<stroke number>`.
    pub key: String,
    pub color: Color32,
    pub points: Vec<[f32; 2]>,
}

/// Remove every stroke with a point within `radius` (slide units) of `(x, y)`. Returns whether
/// any went.
pub fn erase_ink(ink: &mut Vec<InkStroke>, x: f32, y: f32, radius: f32) -> bool {
    let n = ink.len();
    ink.retain(|s| !s.points.iter().any(|p| ((p[0] - x).powi(2) + (p[1] - y).powi(2)).sqrt() <= radius));
    n != ink.len()
}

/// The editor window as it was before the show moved to another display.
#[derive(Clone, Copy, Debug)]
pub struct SavedWindow {
    pub pos: Pos2,
    pub inner: egui::Vec2,
    pub maximized: bool,
}

/// deckrt's state for one show. Lives on [`crate::show::Show`].
pub struct Ext {
    /// Object states by slide and shape id. They persist across slide changes (a poll keeps its votes, a
    /// model its turn); an unpinned injection map drops its points when its slide is left.
    pub states: BTreeMap<StateKey, ObjectState>,
    /// The slide the states were last brought up to date for.
    slide: Option<usize>,
    pub drag: Option<Drag>,
    /// The window the drag is happening in. Both windows share this state and each reads its
    /// own pointer, so the other window must leave the drag alone.
    drag_role: Role,
    /// The object the primary button went down on, and in which window, so the click fires on
    /// its release there.
    press_on: Option<(u32, Role)>,
    /// The wheel was taken by a model this frame (so the show does not turn the page).
    pub took_scroll: bool,
    /// The zoom region object that set the show's zoom, if any.
    zoom_owner: Option<u32>,
    /// A web popup opened in the browser already, so it is not opened twice.
    opened_web: Vec<StateKey>,
    pub phones: Option<Phones>,
    /// Live captions for this show.
    pub captions: Option<super::captions::Live>,
    /// Captions were started (or failed) for this show.
    pub captions_tried: bool,
    /// Record: asked for when the show starts, then recording.
    pub record_wanted: bool,
    /// Present in Presenter View: presenter view on one screen too, to see it without a projector.
    pub practice: bool,
    pub record: Option<super::record::Live>,
    /// Presses of Next each object has taken on this slide.
    pub next_used: BTreeMap<u32, u32>,
    /// The slide the objects that run by themselves are timed on, when it opened, when its
    /// animations finished, and which have fired this visit.
    pub opened: Option<usize>,
    pub opened_at: f64,
    pub anims_done_at: Option<f64>,
    pub auto_fired: std::collections::BTreeSet<u32>,
    /// Rehearse with Coach: asked for when the show starts, then listening.
    pub coach_wanted: bool,
    /// Presenter view's columns and next slide (see `notes`), read from the settings once.
    pub layout: super::notes::Layout,
    pub layout_loaded: bool,
    pub coach: Option<super::coach::Live>,
    /// Presenter view's line on the pace kept from a rehearsal, and whether the talk is behind.
    pub pace: Option<(String, bool)>,
    /// The QR column beside the slide on the audience screen.
    pub qr_shown: bool,
    /// A phone's pointer, in slide units.
    pub pointer: Option<[f32; 2]>,
    /// What the phones drew on this slide.
    pub ink: Vec<InkStroke>,
    /// Displays as listed when the show started.
    pub displays: Vec<DisplayInfo>,
    /// The display the audience screen is on, when there is more than one.
    pub audience: Option<usize>,
    /// The display the presenter window is on.
    pub presenter_display: Option<usize>,
    pub restore: Option<SavedWindow>,
    /// The presenter window asked for a different display; applied once it has a position.
    pub move_presenter: bool,
    /// The last thing that went wrong, for the presenter window.
    pub error: Option<String>,
    /// The result of the last Test, for the Phones section.
    pub last_test: Option<Result<String, String>>,
    pub last_hotspot: Option<String>,
    /// Models seen turning this frame (the presenter's drag or a phone's), to keep the picture
    /// drawn at a lower cost while it moves.
    turning: Vec<u32>,
    /// The clock when something a phone can see last changed.
    pub changed_at: f64,
    /// The phone server was started (or failed) for this show.
    pub phones_tried: bool,
}

impl Default for Ext {
    fn default() -> Self {
        Ext {
            states: BTreeMap::new(),
            slide: None,
            drag: None,
            drag_role: Role::Audience,
            press_on: None,
            took_scroll: false,
            zoom_owner: None,
            opened_web: vec![],
            phones: None,
            captions: None,
            captions_tried: false,
            record_wanted: false,
            practice: false,
            record: None,
            next_used: BTreeMap::new(),
            opened: None,
            opened_at: 0.0,
            anims_done_at: None,
            auto_fired: Default::default(),
            coach_wanted: false,
            layout: Default::default(),
            layout_loaded: false,
            coach: None,
            pace: None,
            qr_shown: false,
            pointer: None,
            ink: vec![],
            displays: vec![],
            audience: None,
            presenter_display: None,
            restore: None,
            move_presenter: false,
            error: None,
            last_test: None,
            last_hotspot: None,
            turning: vec![],
            changed_at: 0.0,
            phones_tried: false,
        }
    }
}

/// Slide points to screen points through the slide's rectangle `srect` and the zoom window `uv`.
#[derive(Clone, Copy, Debug)]
pub struct Map {
    pub srect: Rect,
    pub uv: Rect,
    pub sw: f32,
    pub sh: f32,
}

impl Map {
    pub fn new(srect: Rect, uv: Rect, doc: &Presentation) -> Map {
        Map { srect, uv, sw: doc.slide_size.width.max(1.0) as f32, sh: doc.slide_size.height.max(1.0) as f32 }
    }

    pub fn to_screen(&self, x: f32, y: f32) -> Pos2 {
        let nx = (x / self.sw - self.uv.min.x) / self.uv.width().max(1e-6);
        let ny = (y / self.sh - self.uv.min.y) / self.uv.height().max(1e-6);
        pos2(self.srect.min.x + nx * self.srect.width(), self.srect.min.y + ny * self.srect.height())
    }

    /// Screen point to slide units (0 to 1 across the slide).
    pub fn to_unit(&self, p: Pos2) -> (f32, f32) {
        let nx = (p.x - self.srect.min.x) / self.srect.width().max(1e-6);
        let ny = (p.y - self.srect.min.y) / self.srect.height().max(1e-6);
        (self.uv.min.x + nx * self.uv.width(), self.uv.min.y + ny * self.uv.height())
    }

    pub fn rect(&self, x: &deckcraft_geom::Xfrm) -> Rect {
        Rect::from_min_max(self.to_screen(x.x as f32, x.y as f32), self.to_screen((x.x + x.w) as f32, (x.y + x.h) as f32))
    }

    /// A rectangle in slide units (0 to 1).
    pub fn unit_rect(&self, x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect::from_min_max(self.to_screen(x as f32 * self.sw, y as f32 * self.sh), self.to_screen((x + w) as f32 * self.sw, (y + h) as f32 * self.sh))
    }
}

/// One object on the slide, with its box in slide points.
pub struct Item {
    pub id: u32,
    /// The slide the object is on: shape ids are only unique within a slide.
    pub slide: usize,
    pub shape: Shape,
    pub obj: InteractiveObject,
    pub xfrm: deckcraft_geom::Xfrm,
}

/// An object's state is kept by its slide and shape id, so objects with the same id on two slides
/// (each slide numbers its shapes) don't share a state.
pub type StateKey = (usize, u32);

impl Item {
    pub fn key(&self) -> StateKey {
        (self.slide, self.id)
    }
}

/// The objects on slide `idx`, in drawing order. Objects inside groups are left out.
pub fn objects_on(doc: &Presentation, idx: usize) -> Vec<Item> {
    let Some(slide) = doc.slides.get(idx) else { return vec![] };
    slide
        .shapes
        .iter()
        .filter(|s| !s.hidden)
        .filter_map(|s| {
            let o = obj::read_any(s)?;
            let xfrm = s.xfrm?;
            Some(Item { id: s.id.0, slide: idx, shape: s.clone(), obj: o, xfrm })
        })
        .collect()
}

/// The shapes the show draws itself, left out of the slide picture: a live model, and the
/// pictures a swap, a scrub and a wipe show (drawn fitted rather than stretched), and a poll's
/// box (drawn with its bars).
pub fn hidden_in_show(doc: &Presentation, idx: usize) -> Vec<ShapeId> {
    objects_on(doc, idx)
        .into_iter()
        .filter(|it| matches!(it.obj.kind(), Kind::Model | Kind::ImageSwap | Kind::Scrub | Kind::Wipe | Kind::Poll))
        .map(|it| ShapeId(it.id))
        .collect()
}

impl Ext {
    /// Bring the states up to date for the slide the show is on.
    pub fn sync_slide(&mut self, doc: &Presentation, idx: usize) {
        if self.slide != Some(idx) {
            if self.slide.is_some() {
                state::clear_unpinned(&mut self.states);
            }
            self.slide = Some(idx);
            self.next_used.clear();
            self.drag = None;
            self.pointer = None;
            self.ink.clear();
            self.changed_at = f64::MAX;
        }
        for it in objects_on(doc, idx) {
            self.states.entry(it.key()).or_insert_with(|| ObjectState::initial(&it.obj));
        }
    }

    pub fn state_of(&self, id: u32) -> Option<&ObjectState> {
        self.states.get(&(self.slide.unwrap_or(0), id))
    }

    /// Fire an object (its shortcut, a click, the presenter's button). Returns whether anything
    /// changed.
    pub(crate) fn fire(&mut self, show_zoom: &mut Option<crate::show::Zoom>, it: &Item, now: f64, ctx: &egui::Context) -> bool {
        let Some(st) = self.states.get_mut(&it.key()) else { return false };
        let changed = st.fire(&it.obj, now);
        match (&st.st, &it.obj.config) {
            (TypeState::Zoom { zoomed, .. }, Config::Zoom(c)) => {
                if *zoomed {
                    let v = zoom_view(c.source_rect, c.max_scale, [0.0, 0.0]);
                    *show_zoom = Some(crate::show::Zoom::region(v.cx as f32, v.cy as f32, v.scale as f32));
                    self.zoom_owner = Some(it.id);
                } else if self.zoom_owner == Some(it.id) {
                    *show_zoom = None;
                    self.zoom_owner = None;
                }
            }
            (TypeState::Popup { open: true }, Config::Popup(c)) => {
                if let PopupContent::Web { url } = &c.content
                    && !self.opened_web.contains(&it.key())
                    && (url.starts_with("http://") || url.starts_with("https://"))
                {
                    self.opened_web.push(it.key());
                    ctx.open_url(egui::OpenUrl::new_tab(url.clone()));
                }
            }
            // Closed again: the next open opens the page again.
            (TypeState::Popup { open: false }, Config::Popup(_)) => self.opened_web.retain(|k| *k != it.key()),
            _ => {}
        }
        if changed {
            self.changed_at = now;
        }
        changed
    }

    /// The show's own zoom was dismissed (a click, Esc): the zoom region follows.
    pub(crate) fn follow_zoom(&mut self, show_zoom: &Option<crate::show::Zoom>) {
        if show_zoom.is_none()
            && let Some(id) = self.zoom_owner.take()
            && let Some(ObjectState { st: TypeState::Zoom { zoomed, .. }, .. }) = self.states.get_mut(&(self.slide.unwrap_or(0), id))
        {
            *zoomed = false;
        }
    }

    pub(crate) fn reset(&mut self, it: &Item, show_zoom: &mut Option<crate::show::Zoom>) {
        if let Some(st) = self.states.get_mut(&it.key()) {
            st.reset(&it.obj);
        }
        self.opened_web.retain(|k| *k != it.key());
        if self.zoom_owner == Some(it.id) {
            *show_zoom = None;
            self.zoom_owner = None;
        }
        self.changed_at = f64::MAX;
    }

    pub fn set_visible(&mut self, id: u32, visible: bool) {
        if let Some(st) = self.states.get_mut(&(self.slide.unwrap_or(0), id)) {
            st.visible = visible;
            self.changed_at = f64::MAX;
        }
    }

    /// The models on the slide that a phone may turn, in slide order, visible ones only.
    pub fn models_on(&self, doc: &Presentation, idx: usize) -> Vec<Item> {
        objects_on(doc, idx).into_iter().filter(|it| it.obj.kind() == Kind::Model && self.states.get(&it.key()).is_none_or(|s| s.visible)).collect()
    }

    /// The first poll on the slide.
    pub fn poll_on(&self, doc: &Presentation, idx: usize) -> Option<Item> {
        objects_on(doc, idx).into_iter().find(|it| it.obj.kind() == Kind::Poll)
    }

    /// Turn `id` by a drag of `dx`, `dy` fractions of its surface (deckrt's rule: 240 degrees per
    /// width, 150 per height).
    pub fn turn_model(&mut self, id: u32, dx: f64, dy: f64, now: f64) {
        if let Some(ObjectState { st: TypeState::Model { turn, .. }, .. }) = self.states.get_mut(&(self.slide.unwrap_or(0), id)) {
            turn[0] += dx * 240.0;
            turn[1] = (turn[1] + dy * 150.0).clamp(-180.0, 180.0);
            if !self.turning.contains(&id) {
                self.turning.push(id);
            }
            self.changed_at = now;
        }
    }

    pub fn zoom_model(&mut self, id: u32, factor: f64, now: f64) {
        if let Some(ObjectState { st: TypeState::Model { zoom, .. }, .. }) = self.states.get_mut(&(self.slide.unwrap_or(0), id)) {
            *zoom = (*zoom * factor).clamp(0.2, 8.0);
            self.changed_at = now;
        }
    }

    /// Every model on the slide sharing the dragged one's link group turns with it.
    pub fn turn_linked(&mut self, doc: &Presentation, idx: usize, id: u32, dx: f64, dy: f64, now: f64) {
        let items = objects_on(doc, idx);
        let group = items.iter().find(|it| it.id == id).and_then(|it| match &it.obj.config {
            Config::Model(c) if !c.link_group.trim().is_empty() => Some(c.link_group.clone()),
            _ => None,
        });
        self.turn_model(id, dx, dy, now);
        if let Some(g) = group {
            for it in items {
                if it.id != id && matches!(&it.obj.config, Config::Model(c) if c.link_group == g) {
                    self.turn_model(it.id, dx, dy, now);
                }
            }
        }
    }

    /// End of a frame.
    pub fn end_frame(&mut self) {
        self.turning.clear();
        self.took_scroll = false;
    }
}

// ---------------------------------------------------------------------------------------------
// Drawing

fn pill(painter: &egui::Painter, at: Pos2, align: Align2, text: &str, size: f32) -> Rect {
    let font = FontId::proportional(size);
    let galley = painter.layout_no_wrap(text.to_string(), font, Color32::WHITE);
    let pad = vec2(size * 0.6, size * 0.3);
    let box_size = galley.size() + pad * 2.0;
    let rect = align.anchor_size(at, box_size);
    painter.rect_filled(rect, CornerRadius::same((size * 0.4) as u8), Color32::from_rgba_unmultiplied(20, 20, 24, 215));
    painter.galley(rect.min + pad, galley, Color32::WHITE);
    rect
}

fn color32(hex: &str) -> Color32 {
    let [r, g, b] = obj::parse_color(hex);
    Color32::from_rgb(r, g, b)
}

/// Draw every object of slide `idx` over the slide picture. `role` decides what is shown:
/// the audience never sees presenter-only chrome (nothing here is presenter-only yet; the
/// rows live in the presenter window).
#[allow(clippy::too_many_arguments)]
pub fn draw(
    ctx: &egui::Context,
    painter: &egui::Painter,
    map: Map,
    doc: &Arc<Presentation>,
    idx: usize,
    ext: &mut Ext,
    st: &mut super::State,
    role: Role,
    now: f64,
) {
    ext.sync_slide(doc, idx);
    let ppp = ctx.pixels_per_point();
    let items = objects_on(doc, idx);
    for it in &items {
        let Some(os) = ext.states.get(&it.key()).cloned() else { continue };
        if !os.visible {
            continue;
        }
        let rect = map.rect(&it.xfrm);
        match (&os.st, &it.obj.config) {
            (TypeState::Model { layers, turn, zoom, .. }, Config::Model(c)) => {
                let ShapeKind::Model3d { media, poster } = &it.shape.kind else { continue };
                let hidden: Vec<String> = layers.iter().filter(|(_, on)| !**on).map(|(n, _)| n.clone()).collect();
                let turning = ext.turning.contains(&it.id) || ext.drag.is_some_and(|d| matches!(d, Drag::Model { id, .. } if id == it.id));
                let drawn = st.models.model(doc, *media).and_then(|model| {
                    let t = Turn { turn: *turn, zoom: *zoom };
                    let view = super::model_view::view_of(&model, c, &t);
                    let w = (rect.width() * ppp).round().clamp(1.0, 4096.0) as u32;
                    let h = (rect.height() * ppp).round().clamp(1.0, 4096.0) as u32;
                    let slot = it.id as u64 | ((role == Role::Presenter) as u64) << 40;
                    st.models.draw(ctx, &mut st.gpu, slot, *media, &model, &view, w, h, &hidden, turning)
                });
                match drawn {
                    Some(d) => super::model_view::Models::paint(painter, &d, rect),
                    None => {
                        if let Some((tex, size)) = poster.and_then(|p| st.pictures.get(ctx, doc, p)) {
                            let r = super::pictures::contain(rect, size[0] as f32 / size[1].max(1) as f32);
                            painter.image(tex, r, unit(), Color32::WHITE);
                        }
                    }
                }
                if turning {
                    ctx.request_repaint();
                }
            }
            (TypeState::Swap { index, picker }, Config::ImageSwap(c)) => {
                if let Some(m) = c.images.get(*index).or(c.images.first())
                    && let Some((tex, size)) = st.pictures.get(ctx, doc, m.0)
                {
                    let r = super::pictures::contain(rect, size[0] as f32 / size[1].max(1) as f32);
                    painter.image(tex, r, unit(), Color32::WHITE);
                }
                if c.images.len() > 1 {
                    pill(painter, rect.right_bottom() - vec2(6.0, 6.0), Align2::RIGHT_BOTTOM, &format!("{} / {}", index + 1, c.images.len()), 12.0);
                }
                if *picker {
                    // A strip of every picture along the bottom edge.
                    let n = c.images.len().max(1) as f32;
                    let th = (rect.height() * 0.18).clamp(24.0, 90.0);
                    let strip = Rect::from_min_max(pos2(rect.min.x, rect.max.y - th), rect.max);
                    painter.rect_filled(strip, CornerRadius::ZERO, Color32::from_black_alpha(170));
                    let cw = strip.width() / n;
                    for (i, m) in c.images.iter().enumerate() {
                        let cell = Rect::from_min_size(pos2(strip.min.x + cw * i as f32, strip.min.y), vec2(cw, th)).shrink(3.0);
                        if let Some((tex, size)) = st.pictures.get(ctx, doc, m.0) {
                            let r = super::pictures::contain(cell, size[0] as f32 / size[1].max(1) as f32);
                            painter.image(tex, r, unit(), Color32::WHITE);
                        }
                        if i == *index {
                            painter.rect_stroke(cell, CornerRadius::ZERO, Stroke::new(2.0, Color32::WHITE), StrokeKind::Inside);
                        }
                    }
                }
            }
            (TypeState::Wipe { position }, Config::Wipe(c)) => {
                let p = *position as f32;
                if let Some((tex, size)) = st.pictures.get(ctx, doc, c.after.0) {
                    painter.image(tex, rect, super::pictures::cover_uv(rect, size[0] as f32 / size[1].max(1) as f32), Color32::WHITE);
                }
                let split_x = rect.min.x + rect.width() * p;
                if let Some((tex, size)) = st.pictures.get(ctx, doc, c.before.0) {
                    let clip = Rect::from_min_max(rect.min, pos2(split_x, rect.max.y));
                    let sub = painter.with_clip_rect(clip);
                    sub.image(tex, rect, super::pictures::cover_uv(rect, size[0] as f32 / size[1].max(1) as f32), Color32::WHITE);
                }
                painter.line_segment([pos2(split_x, rect.min.y), pos2(split_x, rect.max.y)], Stroke::new(3.0, Color32::from_black_alpha(120)));
                painter.line_segment([pos2(split_x, rect.min.y), pos2(split_x, rect.max.y)], Stroke::new(1.5, Color32::WHITE));
                let knob = pos2(split_x, rect.center().y);
                painter.circle_filled(knob, 11.0, Color32::from_rgba_unmultiplied(20, 20, 24, 215));
                painter.circle_stroke(knob, 11.0, Stroke::new(1.5, Color32::WHITE));
                painter.text(knob, Align2::CENTER_CENTER, "⇔", FontId::proportional(13.0), Color32::WHITE);
                painter.text(
                    rect.left_top() + vec2(8.0, 6.0),
                    Align2::LEFT_TOP,
                    "Before",
                    FontId::proportional(12.0),
                    Color32::from_rgba_unmultiplied(255, 255, 255, 200),
                );
                painter.text(
                    rect.right_top() + vec2(-8.0, 6.0),
                    Align2::RIGHT_TOP,
                    "After",
                    FontId::proportional(12.0),
                    Color32::from_rgba_unmultiplied(255, 255, 255, 200),
                );
            }
            (TypeState::Scrub { index }, Config::Scrub(c)) => {
                if let Some(m) = c.images.get(*index).or(c.images.first())
                    && let Some((tex, size)) = st.pictures.get(ctx, doc, m.0)
                {
                    let r = super::pictures::contain(rect, size[0] as f32 / size[1].max(1) as f32);
                    painter.image(tex, r, unit(), Color32::WHITE);
                }
                if let Some(label) = c.labels.get(*index) {
                    pill(painter, rect.left_top() + vec2(6.0, 6.0), Align2::LEFT_TOP, label, (rect.height() * 0.06).clamp(11.0, 22.0));
                }
                // The tick strip along the bottom edge.
                let n = c.images.len();
                if n > 1 {
                    let y = rect.max.y - 10.0;
                    painter.line_segment([pos2(rect.min.x + 12.0, y), pos2(rect.max.x - 12.0, y)], Stroke::new(2.0, Color32::from_black_alpha(140)));
                    for i in 0..n {
                        let x = rect.min.x + 12.0 + (rect.width() - 24.0) * i as f32 / (n - 1) as f32;
                        let (r, col) = if i == *index { (6.0, Color32::WHITE) } else { (4.0, Color32::from_gray(190)) };
                        painter.circle_filled(pos2(x, y), r + 1.0, Color32::from_black_alpha(140));
                        painter.circle_filled(pos2(x, y), r, col);
                    }
                }
            }
            (TypeState::Popup { open }, Config::Popup(c)) => {
                if *open {
                    let panel = map.unit_rect(c.panel_position[0], c.panel_position[1], c.panel_size[0], c.panel_size[1]);
                    painter.rect_filled(panel.translate(vec2(4.0, 6.0)), CornerRadius::same(6), Color32::from_black_alpha(90));
                    painter.rect_filled(panel, CornerRadius::same(6), Color32::WHITE);
                    painter.rect_stroke(panel, CornerRadius::same(6), Stroke::new(1.0, Color32::from_gray(160)), StrokeKind::Inside);
                    let inner = panel.shrink(panel.width().min(panel.height()) * 0.06 + 6.0);
                    match &c.content {
                        PopupContent::Text { text } => {
                            let size = (map.srect.height() * 0.03).clamp(12.0, 40.0);
                            let galley = painter.layout(text.clone(), FontId::proportional(size), Color32::from_gray(25), inner.width().max(10.0));
                            let sub = painter.with_clip_rect(inner);
                            sub.galley(inner.min, galley, Color32::from_gray(25));
                        }
                        PopupContent::Image { media } => {
                            if let Some((tex, size)) = st.pictures.get(ctx, doc, media.0) {
                                let r = super::pictures::contain(inner, size[0] as f32 / size[1].max(1) as f32);
                                painter.image(tex, r, unit(), Color32::WHITE);
                            }
                        }
                        PopupContent::Web { url } => {
                            let size = (map.srect.height() * 0.025).clamp(12.0, 28.0);
                            painter.text(
                                inner.center(),
                                Align2::CENTER_CENTER,
                                format!("{url}\nopened in the browser"),
                                FontId::proportional(size),
                                Color32::from_gray(60),
                            );
                        }
                    }
                }
            }
            (TypeState::Poll { open, revealed, votes, texts, ranks, hidden }, Config::Poll(c)) => {
                let answers = PollAnswers { votes, texts, ranks, hidden };
                draw_poll(painter, rect, c, *open, *revealed, &answers, role, poll_bg(doc, idx));
            }
            (TypeState::Measure { preset, handles }, Config::Measure(c)) => {
                let col = color32(&c.color);
                let aspect = (rect.width() / rect.height().max(1.0)) as f64;
                let w = (c.line_width as f32 * map.srect.height() / 1080.0).max(1.0);
                let at = |h: &measure::Handle| pos2(rect.min.x + h[0] as f32 * rect.width(), rect.min.y + h[1] as f32 * rect.height());
                for (a, b) in measure::lines(*preset, handles) {
                    painter.line_segment([at(&a), at(&b)], Stroke::new(w + 2.0, Color32::from_black_alpha(110)));
                    painter.line_segment([at(&a), at(&b)], Stroke::new(w, col));
                }
                for h in handles {
                    let p = at(h);
                    painter.circle_filled(p, 7.0, Color32::from_black_alpha(120));
                    painter.circle_filled(p, 5.5, col);
                    painter.circle_stroke(p, 5.5, Stroke::new(1.0, Color32::WHITE));
                }
                let r = measure::reading(*preset, handles, aspect);
                let text = match &r.detail {
                    Some(d) => format!("{}  ({d})", r.text),
                    None => r.text,
                };
                let size = (map.srect.height() * 0.028).clamp(12.0, 30.0);
                let below = rect.max.y + size * 2.0 < map.srect.max.y;
                if below {
                    pill(painter, pos2(rect.center().x, rect.max.y + 6.0), Align2::CENTER_TOP, &text, size);
                } else {
                    pill(painter, pos2(rect.center().x, rect.min.y - 6.0), Align2::CENTER_BOTTOM, &text, size);
                }
            }
            (TypeState::InjectionMap { points, .. }, Config::InjectionMap(c)) => {
                let size = (map.srect.height() * 0.022).clamp(10.0, 24.0);
                for (i, p) in points.iter().enumerate() {
                    let at = pos2(rect.min.x + p.x as f32 * rect.width(), rect.min.y + p.y as f32 * rect.height());
                    let prod = c.products.get(p.product);
                    let col = prod.map(|p| color32(&p.color)).unwrap_or(Color32::LIGHT_BLUE);
                    let r = size * 0.7;
                    painter.circle_filled(at, r + 1.5, Color32::from_black_alpha(140));
                    painter.circle_filled(at, r, col);
                    painter.text(at, Align2::CENTER_CENTER, format!("{}", i + 1), FontId::proportional(size * 0.8), Color32::WHITE);
                    let dose = injection::dose_text(p.dose, prod.map(|p| p.unit.as_str()).unwrap_or(""));
                    pill(painter, at + vec2(r + 4.0, 0.0), Align2::LEFT_CENTER, &dose, size * 0.8);
                }
                if c.show_total && !points.is_empty() {
                    pill(painter, rect.right_bottom() - vec2(6.0, 6.0), Align2::RIGHT_BOTTOM, &injection::totals_line(c, points), size);
                }
                if role == Role::Presenter {
                    painter.rect_stroke(
                        rect,
                        CornerRadius::ZERO,
                        Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 90)),
                        StrokeKind::Inside,
                    );
                }
            }
            (TypeState::Zoom { .. }, _) if role == Role::Presenter => {
                painter.rect_stroke(
                    rect,
                    CornerRadius::ZERO,
                    Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 90)),
                    StrokeKind::Inside,
                );
            }
            _ => {}
        }
    }
    // What the phones drew.
    let ink_w = (map.srect.width() * 0.006).max(2.0);
    for s in &ext.ink {
        if s.points.len() < 2 {
            if let Some(p) = s.points.first() {
                painter.circle_filled(map.to_screen(p[0] * map.sw, p[1] * map.sh), ink_w / 2.0, s.color);
            }
            continue;
        }
        let pts: Vec<Pos2> = s.points.iter().map(|p| map.to_screen(p[0] * map.sw, p[1] * map.sh)).collect();
        painter.add(egui::Shape::line(pts, Stroke::new(ink_w, s.color)));
    }
    // Live captions along the bottom of the audience screen, and the question the presenter put
    // on the screen above them.
    let below = if role == Role::Audience { super::captions::draw(painter, map.srect, ext.captions.as_ref(), now) } else { 0.0 };
    if role == Role::Audience {
        super::coach::draw(painter, map.srect, ext.coach.as_ref());
        if let Some(ph) = ext.phones.as_ref() {
            super::crowd::draw(painter, map.srect, &ph.crowd, now);
        }
    }
    if let Some(q) = ext.phones.as_ref().and_then(|p| p.questions.shown_question()) {
        super::questions::draw_on_screen(painter, map.srect, q, below);
    }
    // A phone's pointer.
    if let Some([x, y]) = ext.pointer {
        let p = map.to_screen(x * map.sw, y * map.sh);
        painter.circle_filled(p, 14.0, Color32::from_rgba_unmultiplied(255, 60, 60, 80));
        painter.circle_filled(p, 6.0, Color32::from_rgb(255, 60, 60));
    }
    let _ = now;
}

fn unit() -> Rect {
    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))
}

/// The poll panel: the question, and the answers. The room sees how many have voted while voting
/// is open and the split only after the reveal; the presenter's own view shows the split all along.
/// What the room has given a poll so far.
pub struct PollAnswers<'a> {
    pub votes: &'a BTreeMap<String, usize>,
    pub texts: &'a BTreeMap<String, Vec<String>>,
    pub ranks: &'a BTreeMap<String, Vec<usize>>,
    pub hidden: &'a std::collections::BTreeSet<String>,
}

/// The poll panel for each kind. The room sees how many have answered while answers are open and
/// the results after the reveal (Show results); the presenter's own view shows them all along.
fn draw_poll(painter: &egui::Painter, rect: Rect, c: &obj::PollConfig, open: bool, revealed: bool, a: &PollAnswers, role: Role, bg: Color32) {
    match c.kind {
        PollKind::Choice => draw_choice(painter, rect, c, open, revealed, a.votes, role, bg),
        kind => draw_typed(painter, rect, c, kind, open, revealed, a, role, bg),
    }
}

/// The panel's frame and question; returns the area left under the question and the text size.
fn poll_frame(painter: &egui::Painter, rect: Rect, question: &str, rows: f32, bg: Color32) -> (Rect, f32) {
    painter.rect_filled(rect, CornerRadius::same(6), bg);
    let pad = (rect.width().min(rect.height()) * 0.06).clamp(6.0, 20.0);
    let inner = rect.shrink(pad);
    let qsize = (inner.height() / (rows + 2.5)).clamp(9.0, 30.0);
    let q = painter.layout(question.to_string(), FontId::proportional(qsize), Color32::WHITE, inner.width());
    let qh = q.size().y;
    painter.galley(inner.min, q, Color32::WHITE);
    (Rect::from_min_max(pos2(inner.min.x, inner.min.y + qh + 8.0), inner.max), qsize)
}

#[allow(clippy::too_many_arguments)]
fn draw_typed(painter: &egui::Painter, rect: Rect, c: &obj::PollConfig, kind: PollKind, open: bool, revealed: bool, a: &PollAnswers, role: Role, bg: Color32) {
    let painter = &painter.with_clip_rect(rect);
    let n = poll::answered(a.votes, a.texts, a.ranks);
    let (area, qsize) = poll_frame(painter, rect, &c.question, 5.0, bg);
    let small = qsize * 0.8;
    let show = revealed || role == Role::Presenter;
    let status = format!("{}{n} answer{}", if open && !revealed { "Answers open · " } else { "" }, if n == 1 { "" } else { "s" });
    let body = Rect::from_min_max(area.min, pos2(area.max.x, area.max.y - small - 6.0));
    painter.text(pos2(area.min.x, area.max.y), Align2::LEFT_BOTTOM, status, FontId::proportional(small * 0.9), Color32::from_gray(200));
    if !show {
        let hint = match kind {
            PollKind::WordCloud => "Words appear here when the presenter shows them.",
            PollKind::FreeText => "Answers appear here when the presenter shows them.",
            PollKind::Rating => "The result appears here when the presenter shows it.",
            _ => "The room's order appears here when the presenter shows it.",
        };
        painter.text(body.center(), Align2::CENTER_CENTER, hint, FontId::proportional(small), Color32::from_gray(170));
        return;
    }
    match kind {
        PollKind::WordCloud => {
            let words = poll::cloud(a.texts);
            draw_cloud(painter, body, &words, qsize);
        }
        PollKind::FreeText => {
            let list = poll::answers(a.texts, a.hidden);
            // Larger when there are few answers; never smaller than the status line.
            let size = (body.height() / (list.len().max(4) as f32 * 1.9)).clamp(small, qsize * 1.3);
            let mut y = body.min.y;
            for (_, text) in list.iter().rev() {
                let g = painter.layout(text.to_string(), FontId::proportional(size), Color32::WHITE, body.width() - 16.0);
                let h = g.size().y + 10.0;
                if y + h > body.max.y {
                    break;
                }
                let card = Rect::from_min_size(pos2(body.min.x, y), vec2(body.width(), h));
                painter.rect_filled(card, CornerRadius::same(5), Color32::from_white_alpha(16));
                painter.galley(card.min + vec2(8.0, 5.0), g, Color32::WHITE);
                y += h + 6.0;
            }
        }
        PollKind::Rating => {
            let (counts, avg) = poll::rating(c.scale, a.votes);
            let big = (body.height() * 0.32).clamp(16.0, 90.0);
            let avg_text = match avg {
                Some(v) => format!("{v:.1} out of {}", c.scale.clamp(2, 10)),
                None => "No ratings yet".to_string(),
            };
            painter.text(pos2(body.min.x, body.min.y), Align2::LEFT_TOP, avg_text, FontId::proportional(big * 0.7), Color32::WHITE);
            let bars = Rect::from_min_max(pos2(body.min.x, body.min.y + big * 0.9), body.max);
            let max = counts.iter().copied().max().unwrap_or(0).max(1) as f32;
            let k = counts.len().max(1) as f32;
            let w = bars.width() / k;
            for (i, cnt) in counts.iter().enumerate() {
                let x = bars.min.x + w * i as f32;
                let label_h = small + 4.0;
                let hmax = (bars.height() - label_h * 2.0).max(4.0);
                let h = hmax * (*cnt as f32 / max);
                let bar = Rect::from_min_max(pos2(x + w * 0.15, bars.max.y - label_h - h), pos2(x + w * 0.85, bars.max.y - label_h));
                painter.rect_filled(bar, CornerRadius::same(3), Color32::from_rgb(0x3b, 0x5b, 0x9a));
                painter.text(
                    pos2(x + w / 2.0, bars.max.y),
                    Align2::CENTER_BOTTOM,
                    format!("{}", i + 1),
                    FontId::proportional(small),
                    Color32::from_gray(210),
                );
                if *cnt > 0 {
                    painter.text(
                        pos2(x + w / 2.0, bar.min.y - 2.0),
                        Align2::CENTER_BOTTOM,
                        format!("{cnt}"),
                        FontId::proportional(small * 0.85),
                        Color32::WHITE,
                    );
                }
            }
        }
        _ => {
            let order = poll::ranking(c.options.len(), a.ranks);
            let rows = order.len().max(1) as f32;
            let row_h = (body.height() / rows).clamp(small + 6.0, small * 4.0);
            let small = (row_h * 0.42).clamp(small, qsize * 1.3);
            let k = c.options.len().max(1) as f64;
            for (place, (o, avg)) in order.iter().enumerate() {
                let y = body.min.y + row_h * place as f32;
                let row = Rect::from_min_max(pos2(body.min.x, y), pos2(body.max.x, y + row_h - 4.0));
                painter.rect_filled(row, CornerRadius::same(4), Color32::from_white_alpha(14));
                if n > 0 {
                    // Longer bar for a better (smaller) average place.
                    let frac = ((k + 1.0 - avg) / k).clamp(0.0, 1.0) as f32;
                    let bar = Rect::from_min_max(row.min, pos2(row.min.x + row.width() * frac, row.max.y));
                    painter.rect_filled(bar, CornerRadius::same(4), Color32::from_rgb(0x3b, 0x5b, 0x9a));
                }
                let label = format!("{}.  {}", place + 1, c.options.get(*o).cloned().unwrap_or_default());
                painter.text(pos2(row.min.x + 8.0, row.center().y), Align2::LEFT_CENTER, label, FontId::proportional(small), Color32::WHITE);
            }
        }
    }
}

/// Word cloud: words in rows, the most given largest, centred, as many as fit.
fn draw_cloud(painter: &egui::Painter, area: Rect, words: &[(String, u32)], qsize: f32) {
    if words.is_empty() {
        painter.text(area.center(), Align2::CENTER_CENTER, "No words yet", FontId::proportional(qsize * 0.8), Color32::from_gray(170));
        return;
    }
    let palette = [
        Color32::from_rgb(0x8f, 0xc1, 0xff),
        Color32::from_rgb(0xff, 0xd1, 0x66),
        Color32::from_rgb(0x8a, 0xe2, 0xb0),
        Color32::from_rgb(0xff, 0x9d, 0xb5),
        Color32::from_rgb(0xc9, 0xa7, 0xff),
        Color32::WHITE,
    ];
    let top = words[0].1.max(1) as f32;
    // Shrink everything until the rows fit the area.
    let mut scale = 1.0f32;
    for _ in 0..14 {
        let mut rows: Vec<(f32, f32)> = vec![(0.0, 0.0)];
        let mut fits = true;
        for (w, n) in words.iter().take(60) {
            let size = (qsize * 0.7 + qsize * 1.6 * (*n as f32 / top)) * scale;
            let g = painter.layout_no_wrap(w.clone(), FontId::proportional(size), Color32::WHITE);
            let (wid, hgt) = (g.size().x + size * 0.6, g.size().y);
            match rows.last_mut() {
                Some(last) if !(last.0 + wid > area.width() && last.0 > 0.0) => {
                    last.0 += wid;
                    last.1 = last.1.max(hgt);
                }
                _ => rows.push((wid, hgt)),
            }
            if wid > area.width() {
                fits = false;
            }
        }
        let total: f32 = rows.iter().map(|r| r.1).sum();
        if fits && total <= area.height() {
            break;
        }
        scale *= 0.85;
    }
    // Lay out and draw.
    let mut rows: Vec<Vec<(Arc<egui::Galley>, Color32)>> = vec![vec![]];
    let mut row_w = 0.0f32;
    let mut row_ws: Vec<f32> = vec![0.0];
    for (i, (w, n)) in words.iter().take(60).enumerate() {
        let size = (qsize * 0.7 + qsize * 1.6 * (*n as f32 / top)) * scale;
        let col = palette[i % palette.len()];
        let g = painter.layout_no_wrap(w.clone(), FontId::proportional(size), col);
        let wid = g.size().x + size * 0.6;
        if row_w + wid > area.width() && row_w > 0.0 {
            rows.push(vec![]);
            row_ws.push(0.0);
            row_w = 0.0;
        }
        row_w += wid;
        if let Some(rw) = row_ws.last_mut() {
            *rw = row_w;
        }
        if let Some(row) = rows.last_mut() {
            row.push((g, col));
        }
    }
    let heights: Vec<f32> = rows.iter().map(|r| r.iter().map(|(g, _)| g.size().y).fold(0.0, f32::max)).collect();
    let total: f32 = heights.iter().sum();
    let mut y = area.center().y - total / 2.0;
    for (r, row) in rows.into_iter().enumerate() {
        let mut x = area.center().x - row_ws[r] / 2.0;
        for (g, col) in row {
            let gy = y + (heights[r] - g.size().y) / 2.0;
            let size = g.size();
            let gap = size.y * 0.3;
            if gy + size.y <= area.max.y + 1.0 && gy >= area.min.y - 1.0 {
                painter.galley(pos2(x + gap, gy), g, col);
            }
            x += size.x + gap * 2.0;
        }
        y += heights[r];
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_choice(painter: &egui::Painter, rect: Rect, c: &obj::PollConfig, open: bool, revealed: bool, votes: &BTreeMap<String, usize>, role: Role, bg: Color32) {
    let counts = poll::tally(c.options.len(), votes);
    let total: u32 = counts.iter().sum();
    let pct = poll::percentages(&counts);
    painter.rect_filled(rect, CornerRadius::same(6), bg);
    let pad = (rect.width().min(rect.height()) * 0.06).clamp(6.0, 20.0);
    let inner = rect.shrink(pad);
    let painter = &painter.with_clip_rect(rect);
    let n = c.options.len().max(1) as f32;
    let status = if revealed {
        format!("{total} vote{}", if total == 1 { "" } else { "s" })
    } else if open {
        format!("Voting open · {total} vote{}", if total == 1 { "" } else { "s" })
    } else {
        String::new()
    };
    // The text size: the largest at which the question (wrapped), the rows and the status all
    // fit in the box, down to a floor where the box is just too small.
    let mut qsize = (inner.height() / (n + 2.5)).clamp(9.0, 30.0);
    let mut q = painter.layout(c.question.clone(), FontId::proportional(qsize), Color32::WHITE, inner.width());
    for _ in 0..12 {
        let asize = qsize * 0.85;
        let need = q.size().y + 6.0 + n * (asize + 6.0) + if status.is_empty() { 0.0 } else { asize + 4.0 };
        if need <= inner.height() || qsize <= 9.0 {
            break;
        }
        qsize = (qsize * 0.88).max(9.0);
        q = painter.layout(c.question.clone(), FontId::proportional(qsize), Color32::WHITE, inner.width());
    }
    let asize = qsize * 0.85;
    let qh = q.size().y;
    painter.galley(inner.min, q, Color32::WHITE);
    let top = inner.min.y + qh + 6.0;
    let bottom = if status.is_empty() { inner.max.y } else { inner.max.y - asize - 4.0 };
    let row_h = ((bottom - top) / n).clamp(asize + 6.0, asize * 2.4);
    let show_split = revealed || role == Role::Presenter;
    for (i, opt) in c.options.iter().enumerate() {
        let y = top + row_h * i as f32;
        let row = Rect::from_min_max(pos2(inner.min.x, y), pos2(inner.max.x, y + row_h - 4.0));
        let letter = (b'A' + (i % 26) as u8) as char;
        // The right answer is marked only when the poll reveals it: Show the split shows the
        // votes without giving the answer away.
        let right = c.correct == Some(i) && c.reveal_correct;
        if show_split && (open || revealed) {
            let frac = pct.get(i).copied().unwrap_or(0) as f32 / 100.0;
            let bar = Rect::from_min_max(row.min, pos2(row.min.x + row.width() * frac, row.max.y));
            let col = if revealed && right { Color32::from_rgb(0x2e, 0xa0, 0x5c) } else { Color32::from_rgb(0x3b, 0x5b, 0x9a) };
            painter.rect_filled(row, CornerRadius::same(4), Color32::from_white_alpha(14));
            painter.rect_filled(bar, CornerRadius::same(4), col);
            let txt = format!("{}  {}%", counts.get(i).copied().unwrap_or(0), pct.get(i).copied().unwrap_or(0));
            painter.text(pos2(row.max.x - 6.0, row.center().y), Align2::RIGHT_CENTER, txt, FontId::proportional(asize), Color32::WHITE);
        } else {
            painter.rect_filled(row, CornerRadius::same(4), Color32::from_white_alpha(14));
        }
        let label = format!("{letter}.  {opt}");
        let col = if revealed && right {
            Color32::WHITE
        } else if revealed {
            Color32::from_gray(200)
        } else {
            Color32::WHITE
        };
        painter.text(pos2(row.min.x + 8.0, row.center().y), Align2::LEFT_CENTER, label, FontId::proportional(asize), col);
        if revealed && right {
            painter.text(pos2(row.min.x + row.width() * 0.62, row.center().y), Align2::LEFT_CENTER, "✓", FontId::proportional(asize), Color32::WHITE);
        }
    }
    if !status.is_empty() {
        painter.text(pos2(inner.min.x, inner.max.y), Align2::LEFT_BOTTOM, status, FontId::proportional(asize * 0.9), Color32::from_gray(200));
    }
}

// ---------------------------------------------------------------------------------------------
// Pointer input

/// Whether an object takes a click or a drag from `role`.
fn takes_input(it: &Item, role: Role) -> bool {
    match role {
        Role::Presenter => true,
        // The audience screen: objects marked for it, and models, whose drag is the point of them.
        Role::Audience => it.obj.trigger == Trigger::AudienceClick || matches!(it.obj.kind(), Kind::Model | Kind::Wipe | Kind::Scrub),
    }
}

/// Pointer input over the objects. `resp` is a hover/drag interaction covering the slide. Returns
/// whether the primary click (press and release) was taken by an object, so the show does not
/// also act on it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn pointer(
    ctx: &egui::Context,
    map: Map,
    doc: &Arc<Presentation>,
    idx: usize,
    ext: &mut Ext,
    show_zoom: &mut Option<crate::show::Zoom>,
    role: Role,
    now: f64,
    hovered: bool,
) -> bool {
    ext.sync_slide(doc, idx);
    let (pressed, down, released, latest, scroll) = ctx.input(|i| {
        (i.pointer.primary_pressed(), i.pointer.primary_down(), i.pointer.primary_released(), i.pointer.latest_pos(), i.smooth_scroll_delta.y)
    });
    let items = objects_on(doc, idx);
    // A drag in progress follows the pointer wherever it goes, in the window it started in.
    if let Some(d) = ext.drag {
        if ext.drag_role != role {
            return false;
        }
        if !down {
            ext.drag = None;
            ext.press_on = None;
            return released;
        }
        let Some(p) = latest else { return true };
        match d {
            Drag::Model { id, last } => {
                if let Some(it) = items.iter().find(|it| it.id == id) {
                    let rect = map.rect(&it.xfrm);
                    let dx = (p.x - last.x) / rect.width().max(1.0);
                    let dy = (p.y - last.y) / rect.height().max(1.0);
                    if dx != 0.0 || dy != 0.0 {
                        ext.turn_linked(doc, idx, id, dx as f64, dy as f64, now);
                    }
                    ext.drag = Some(Drag::Model { id, last: p });
                }
            }
            Drag::Wipe { id } => {
                if let Some(it) = items.iter().find(|it| it.id == id) {
                    let rect = map.rect(&it.xfrm);
                    let pos = ((p.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 1.0) as f64;
                    if let Some(ObjectState { st: TypeState::Wipe { position }, .. }) = ext.states.get_mut(&(idx, id)) {
                        *position = pos;
                        ext.changed_at = now;
                    }
                }
            }
            Drag::Scrub { id } => {
                if let Some(it) = items.iter().find(|it| it.id == id)
                    && let Config::Scrub(c) = &it.obj.config
                {
                    let rect = map.rect(&it.xfrm);
                    let n = c.images.len();
                    if n > 1 {
                        let f = ((p.x - rect.min.x - 12.0) / (rect.width() - 24.0).max(1.0)).clamp(0.0, 1.0);
                        let i = (f * (n - 1) as f32).round() as usize;
                        if let Some(ObjectState { st: TypeState::Scrub { index }, .. }) = ext.states.get_mut(&(idx, id))
                            && *index != i
                        {
                            *index = i;
                            ext.changed_at = now;
                        }
                    }
                }
            }
            Drag::Measure { id, handle } => {
                if let Some(it) = items.iter().find(|it| it.id == id) {
                    let rect = map.rect(&it.xfrm);
                    let to = [((p.x - rect.min.x) / rect.width().max(1.0)) as f64, ((p.y - rect.min.y) / rect.height().max(1.0)) as f64];
                    if let Some(ObjectState { st: TypeState::Measure { preset, handles }, .. }) = ext.states.get_mut(&(idx, id)) {
                        *handles = measure::move_handle(*preset, handles, handle, to);
                        ext.changed_at = now;
                    }
                }
            }
        }
        ctx.request_repaint();
        return true;
    }
    if !hovered {
        return false;
    }
    let Some(p) = latest else { return false };
    // Topmost object under the pointer that takes input. A measurement's handles sit on its
    // edges, so a press just outside its box still reaches them.
    let under = items.iter().rev().find(|it| {
        let r = map.rect(&it.xfrm);
        let r = if it.obj.kind() == Kind::Measure { r.expand(18.0) } else { r };
        takes_input(it, role) && ext.states.get(&it.key()).is_none_or(|s| s.visible) && r.contains(p)
    });
    let Some(it) = under else { return false };
    let rect = map.rect(&it.xfrm);
    // The wheel and a two-finger pinch on a touch screen zoom a model.
    let pinch = ctx.input(|i| i.zoom_delta());
    if it.obj.kind() == Kind::Model && (scroll.abs() > 0.0 || (pinch - 1.0).abs() > 1e-3) {
        let f = if (pinch - 1.0).abs() > 1e-3 { pinch as f64 } else { 1.0 + scroll as f64 / 300.0 };
        ext.zoom_model(it.id, f, now);
        ext.took_scroll = true;
        ctx.request_repaint();
    }
    match it.obj.kind() {
        Kind::Model | Kind::Wipe => ctx.set_cursor_icon(egui::CursorIcon::Grab),
        Kind::Measure => ctx.set_cursor_icon(egui::CursorIcon::Grab),
        _ => ctx.set_cursor_icon(egui::CursorIcon::PointingHand),
    }
    // A click fires on the release that follows a press on the same object, so the release is
    // taken too and the show does not turn the page on it. Drags start on the press.
    // A press and its release in one frame (a quick click) is a click too.
    let clicked = released && (ext.press_on == Some((it.id, role)) || pressed);
    if pressed {
        ext.press_on = Some((it.id, role));
        ext.drag_role = role;
    }
    if released {
        ext.press_on = None;
    }
    if !pressed && !clicked {
        return false;
    }
    match (&it.obj.config, ext.states.get(&it.key()).map(|s| s.st.clone())) {
        (Config::Model(_), _) => {
            if pressed {
                ext.drag = Some(Drag::Model { id: it.id, last: p });
            }
            true
        }
        (Config::Wipe(_), _) if clicked => true,
        (Config::Wipe(_), _) => {
            ext.drag = Some(Drag::Wipe { id: it.id });
            // The first press already moves the divider.
            let pos = ((p.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 1.0) as f64;
            if let Some(ObjectState { st: TypeState::Wipe { position }, .. }) = ext.states.get_mut(&it.key()) {
                *position = pos;
                ext.changed_at = now;
            }
            true
        }
        (Config::Scrub(c), _) => {
            // On the tick strip: scrub (from the press); elsewhere: the next photograph (on the click).
            if p.y >= rect.max.y - 22.0 && c.images.len() > 1 {
                if clicked {
                    return true;
                }
                ext.drag = Some(Drag::Scrub { id: it.id });
                let f = ((p.x - rect.min.x - 12.0) / (rect.width() - 24.0).max(1.0)).clamp(0.0, 1.0);
                let i = (f * (c.images.len() - 1) as f32).round() as usize;
                if let Some(ObjectState { st: TypeState::Scrub { index }, .. }) = ext.states.get_mut(&it.key()) {
                    *index = i;
                    ext.changed_at = now;
                }
            } else if clicked {
                ext.fire(show_zoom, it, now, ctx);
            }
            true
        }
        (Config::Measure(_), Some(TypeState::Measure { handles, .. })) => {
            if clicked {
                return false;
            }
            let at = |h: &measure::Handle| pos2(rect.min.x + h[0] as f32 * rect.width(), rect.min.y + h[1] as f32 * rect.height());
            let best = handles.iter().enumerate().map(|(i, h)| (i, at(h).distance(p))).min_by(|a, b| a.1.total_cmp(&b.1));
            match best {
                Some((i, d)) if d <= 18.0 => {
                    ext.drag = Some(Drag::Measure { id: it.id, handle: i });
                    true
                }
                _ => false,
            }
        }
        (Config::InjectionMap(_), Some(TypeState::InjectionMap { .. })) => {
            // Only the presenter places points: a point carries a product and a dose chosen in
            // the presenter's row.
            if role != Role::Presenter || !clicked {
                return role == Role::Presenter;
            }
            let x = ((p.x - rect.min.x) / rect.width().max(1.0)).clamp(0.0, 1.0) as f64;
            let y = ((p.y - rect.min.y) / rect.height().max(1.0)).clamp(0.0, 1.0) as f64;
            if let Some(ObjectState { st: TypeState::InjectionMap { points, product, dose, .. }, .. }) = ext.states.get_mut(&it.key()) {
                points.push(Point { x, y, product: *product, dose: *dose });
                ext.changed_at = now;
            }
            true
        }
        (Config::ImageSwap(c), Some(TypeState::Swap { picker: true, .. })) => {
            if !clicked {
                return true;
            }
            // A click on the strip picks that picture; elsewhere closes the strip.
            let th = (rect.height() * 0.18).clamp(24.0, 90.0);
            if p.y >= rect.max.y - th && !c.images.is_empty() {
                let i =
                    (((p.x - rect.min.x) / rect.width().max(1.0)) * c.images.len() as f32).floor().clamp(0.0, c.images.len() as f32 - 1.0) as usize;
                if let Some(ObjectState { st: TypeState::Swap { index, picker }, .. }) = ext.states.get_mut(&it.key()) {
                    *index = i;
                    *picker = false;
                    ext.changed_at = now;
                }
            } else {
                ext.fire(show_zoom, it, now, ctx);
            }
            true
        }
        (Config::ImageSwap(_) | Config::Zoom(_) | Config::Popup(_), _) => {
            if clicked {
                ext.fire(show_zoom, it, now, ctx);
            }
            true
        }
        (Config::Poll(_), _) => {
            // The presenter opens and closes voting from the row; a click on the box does nothing.
            role == Role::Presenter
        }
        _ => false,
    }
}

/// Next was pressed (a key, the clicker, a click on the slide, a Next button). When the slide's
/// animations are done and an object still has presses of Next to take, it fires and the show
/// stays on the slide. Returns whether an object took it.
pub(crate) fn take_next(show: &mut Show, doc: &Presentation, now: f64, ctx: &egui::Context) -> bool {
    if show.ended || show.grid || show.reading {
        return false;
    }
    let idx = show.state.slide;
    if show.state.step < deckcraft_anim::ShowState::steps(doc, idx) {
        return false;
    }
    show.ext.sync_slide(doc, idx);
    for it in objects_on(doc, idx) {
        // An object hidden from the screen doesn't take Next: the click would run it unseen.
        if show.ext.states.get(&it.key()).is_some_and(|s| !s.visible) {
            continue;
        }
        let used = show.ext.next_used.get(&it.id).copied().unwrap_or(0);
        if it.obj.next_clicks > used {
            show.ext.next_used.insert(it.id, used + 1);
            show.ext.fire(&mut show.zoom, &it, now, ctx);
            ctx.request_repaint_of(egui::ViewportId::ROOT);
            return true;
        }
    }
    false
}

/// Objects that run by themselves: when the slide opens or when its animations have played,
/// each after its delay, once per visit.
fn fire_on_open(show: &mut Show, doc: &Presentation, now: f64, ctx: &egui::Context) {
    let idx = show.state.slide;
    if show.ended {
        return;
    }
    if show.ext.opened != Some(idx) {
        show.ext.opened = Some(idx);
        show.ext.opened_at = now;
        show.ext.anims_done_at = None;
        show.ext.auto_fired.clear();
    }
    let done = show.state.step >= deckcraft_anim::ShowState::steps(doc, idx) && !show.state.playing;
    if done && show.ext.anims_done_at.is_none() {
        show.ext.anims_done_at = Some(now);
    }
    show.ext.sync_slide(doc, idx);
    let mut waiting = false;
    for it in objects_on(doc, idx) {
        if show.ext.auto_fired.contains(&it.id) {
            continue;
        }
        let start = if it.obj.after_animations {
            show.ext.anims_done_at
        } else if it.obj.with_slide {
            Some(show.ext.opened_at)
        } else {
            continue;
        };
        let delay = if it.obj.delay.is_finite() { it.obj.delay.clamp(0.0, 3600.0) } else { 0.0 };
        match start {
            Some(t0) if now - t0 >= delay => {
                show.ext.auto_fired.insert(it.id);
                show.ext.fire(&mut show.zoom, &it, now, ctx);
                ctx.request_repaint_of(egui::ViewportId::ROOT);
            }
            _ => waiting = true,
        }
    }
    if waiting {
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

/// A poll's box colour: the theme's second dark colour, as the saved file has it.
fn poll_bg(doc: &Presentation, idx: usize) -> Color32 {
    let Some(sl) = doc.slides.get(idx) else { return Color32::from_rgb(0x1e, 0x29, 0x3b) };
    let Some(ctx) = deckcraft_model::resolve::Ctx::for_slide(doc, sl) else { return Color32::from_rgb(0x1e, 0x29, 0x3b) };
    let c = ctx.color(&deckcraft_model::ColorRef::scheme(deckcraft_color::SchemeSlot::Dk2), None);
    Color32::from_rgb(c.r, c.g, c.b)
}

/// Whether a text box (a number being typed into a box, say) has the keyboard, so the show's keys
/// should leave it alone.
pub(crate) fn typing(ctx: &egui::Context) -> bool {
    ctx.memory(|m| m.focused()).is_some_and(|id| egui::text_edit::TextEditState::load(ctx, id).is_some())
}

/// An object's shortcut key, pressed in either window. Returns whether a key was taken.
pub(crate) fn shortcut(
    ext: &mut Ext,
    show_zoom: &mut Option<crate::show::Zoom>,
    doc: &Presentation,
    idx: usize,
    key: egui::Key,
    mods: egui::Modifiers,
    now: f64,
    ctx: &egui::Context,
) -> bool {
    if mods.command || mods.ctrl || mods.alt {
        return false;
    }
    let name = key.name();
    let mut chars = name.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else { return false };
    let c = c.to_ascii_lowercase();
    ext.sync_slide(doc, idx);
    let mut taken = false;
    for it in objects_on(doc, idx) {
        if it.obj.shortcut.is_some_and(|s| s.to_ascii_lowercase() == c) {
            ext.fire(show_zoom, &it, now, ctx);
            taken = true;
        }
    }
    taken
}

// ---------------------------------------------------------------------------------------------
// The presenter's control rows

/// The presenter window is dark and the app's light theme draws dark text in its own widgets
/// (sliders, number fields, pickers): give them light text and dark fills there.
pub(crate) fn dark_widgets(ui: &mut Ui) {
    let v = ui.visuals_mut();
    v.override_text_color = Some(Color32::from_gray(235));
    v.widgets.inactive.bg_fill = Color32::from_gray(60);
    v.widgets.inactive.weak_bg_fill = Color32::from_gray(60);
    v.widgets.hovered.bg_fill = Color32::from_gray(80);
    v.widgets.hovered.weak_bg_fill = Color32::from_gray(80);
    v.widgets.active.bg_fill = Color32::from_gray(90);
    v.widgets.active.weak_bg_fill = Color32::from_gray(90);
    v.widgets.open.bg_fill = Color32::from_gray(70);
    v.widgets.open.weak_bg_fill = Color32::from_gray(70);
    v.extreme_bg_color = Color32::from_gray(40);
    v.selection.bg_fill = Color32::from_rgb(0x3b, 0x5b, 0x9a);
    v.window_fill = Color32::from_gray(35);
    v.panel_fill = Color32::from_gray(35);
}

fn row_button(ui: &mut Ui, text: &str, on: bool) -> egui::Response {
    let fill = if on { Color32::from_rgb(0x3b, 0x5b, 0x9a) } else { Color32::from_gray(60) };
    ui.add(egui::Button::new(egui::RichText::new(text).size(13.0).color(Color32::WHITE)).fill(fill).min_size(vec2(0.0, 26.0)))
}

fn dim(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(12.0).color(Color32::from_gray(170)));
}

/// One row per object on the current slide, in the presenter window. `go_to` receives a slide
/// index when the presenter follows a poll's winning answer.
pub(crate) fn presenter_rows(
    ui: &mut Ui,
    ext: &mut Ext,
    show_zoom: &mut Option<crate::show::Zoom>,
    doc: &Arc<Presentation>,
    idx: usize,
    now: f64,
    go_to: &mut Option<usize>,
) {
    dark_widgets(ui);
    ext.sync_slide(doc, idx);
    let items = objects_on(doc, idx);
    if items.is_empty() {
        return;
    }
    for it in &items {
        let Some(os) = ext.states.get(&it.key()).cloned() else { continue };
        let label = it.obj.display_label();
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(&label).size(13.0).color(Color32::WHITE).strong());
            if let Some(k) = it.obj.shortcut {
                dim(ui, &format!("key {}", k.to_ascii_uppercase()));
            }
            if row_button(ui, if os.visible { "Hide" } else { "Show" }, false).clicked() {
                ext.set_visible(it.id, !os.visible);
            }
            match (&os.st, &it.obj.config) {
                (TypeState::Model { layers, .. }, Config::Model(_)) => {
                    for (name, on) in layers {
                        if row_button(ui, name, *on).clicked()
                            && let Some(ObjectState { st: TypeState::Model { layers, .. }, .. }) = ext.states.get_mut(&it.key())
                        {
                            layers.insert(name.clone(), !on);
                            ext.changed_at = now;
                        }
                    }
                    if row_button(ui, "Reset view", false).clicked() {
                        ext.reset(it, show_zoom);
                    }
                    dim(ui, "Drag the model on the slide to turn it; the wheel zooms.");
                }
                (TypeState::Swap { index, .. }, Config::ImageSwap(c)) => {
                    if row_button(ui, "Next", false).clicked() {
                        ext.fire(show_zoom, it, now, ui.ctx());
                    }
                    dim(ui, &format!("{} of {}", index + 1, c.images.len()));
                    if row_button(ui, "First", false).clicked() {
                        ext.reset(it, show_zoom);
                    }
                }
                (TypeState::Wipe { position }, Config::Wipe(_)) => {
                    let mut v = *position as f32;
                    if ui.add(egui::Slider::new(&mut v, 0.0..=1.0).show_value(false)).changed()
                        && let Some(ObjectState { st: TypeState::Wipe { position }, .. }) = ext.states.get_mut(&it.key())
                    {
                        *position = v as f64;
                        ext.changed_at = now;
                    }
                    if row_button(ui, "Reset", false).clicked() {
                        ext.reset(it, show_zoom);
                    }
                }
                (TypeState::Zoom { zoomed, .. }, Config::Zoom(_)) => {
                    if row_button(ui, if *zoomed { "Zoom out" } else { "Zoom in" }, *zoomed).clicked() {
                        ext.fire(show_zoom, it, now, ui.ctx());
                    }
                }
                (TypeState::Popup { open }, Config::Popup(_)) => {
                    if row_button(ui, if *open { "Close" } else { "Open" }, *open).clicked() {
                        ext.fire(show_zoom, it, now, ui.ctx());
                    }
                }
                (TypeState::Poll { open, revealed, votes, texts, ranks, hidden }, Config::Poll(c)) if c.kind != PollKind::Choice => {
                    let n = poll::answered(votes, texts, ranks);
                    if !*revealed && row_button(ui, if *open { "Close answers" } else { "Open answers" }, *open).clicked() {
                        ext.fire(show_zoom, it, now, ui.ctx());
                    }
                    if !*revealed
                        && n > 0
                        && row_button(ui, "Show results", false).clicked()
                        && let Some(ObjectState { st: TypeState::Poll { open, revealed, .. }, .. }) = ext.states.get_mut(&it.key())
                    {
                        *open = false;
                        *revealed = true;
                        ext.changed_at = now;
                    }
                    dim(ui, &format!("{} · {n} answer{}", c.kind.label(), if n == 1 { "" } else { "s" }));
                    if c.kind == PollKind::FreeText {
                        let list: Vec<(String, String)> = texts.iter().filter_map(|(id, t)| t.first().map(|a| (id.clone(), a.clone()))).collect();
                        let hidden_now = hidden.clone();
                        // Every answer, in a list that scrolls, so any of them can be hidden.
                        let mut toggle = None;
                        egui::ScrollArea::vertical().id_salt(("deckrt-free-text", it.key())).max_height(180.0).show(ui, |ui| {
                            for (pid, a) in list.iter() {
                                let off = hidden_now.contains(pid);
                                let short: String = a.chars().take(48).collect();
                                if row_button(ui, &format!("{}{short}", if off { "Hidden: " } else { "" }), off)
                                    .on_hover_text("Click to hide it from the screen, or bring it back")
                                    .clicked()
                                {
                                    toggle = Some(pid.clone());
                                }
                            }
                        });
                        if let Some(pid) = toggle
                            && let Some(ObjectState { st: TypeState::Poll { hidden, .. }, .. }) = ext.states.get_mut(&it.key())
                        {
                            if !hidden.remove(&pid) {
                                hidden.insert(pid);
                            }
                            ext.changed_at = now;
                        }
                    }
                    if row_button(ui, "Reset", false).clicked() {
                        ext.reset(it, show_zoom);
                    }
                }
                (TypeState::Poll { open, revealed, votes, .. }, Config::Poll(c)) => {
                    let counts = poll::tally(c.options.len(), votes);
                    if !*revealed && row_button(ui, if *open { "Close voting" } else { "Open voting" }, *open).clicked() {
                        ext.fire(show_zoom, it, now, ui.ctx());
                    }
                    if c.reveal_correct
                        && c.correct.is_some()
                        && !*revealed
                        && row_button(ui, "Reveal answer", false).clicked()
                        && let Some(ObjectState { st: TypeState::Poll { open, revealed, .. }, .. }) = ext.states.get_mut(&it.key())
                    {
                        *open = false;
                        *revealed = true;
                        ext.changed_at = now;
                    }
                    if (c.correct.is_none() || !c.reveal_correct)
                        && !*revealed
                        && !votes.is_empty()
                        && row_button(ui, "Show the split", false).clicked()
                        && let Some(ObjectState { st: TypeState::Poll { open, revealed, .. }, .. }) = ext.states.get_mut(&it.key())
                    {
                        *open = false;
                        *revealed = true;
                        ext.changed_at = now;
                    }
                    let tally: Vec<String> = c.options.iter().zip(counts.iter()).map(|(o, n)| format!("{o} {n}")).collect();
                    dim(ui, &tally.join(" · "));
                    if row_button(ui, "Reset", false).clicked() {
                        ext.reset(it, show_zoom);
                    }
                    // After the reveal: the winner's slide is offered, never taken on its own.
                    match poll::branch(c, &counts, *revealed, doc) {
                        poll::Branch::Winner(o) => {
                            if o.has_target {
                                match o.slide {
                                    Some(s) => {
                                        if row_button(ui, &poll::go_to_label(s), true).clicked() {
                                            *go_to = Some(s);
                                        }
                                    }
                                    None => dim(ui, &format!("{}: its slide is no longer in the deck", o.label)),
                                }
                            }
                        }
                        poll::Branch::Tie(offers) => {
                            dim(ui, &format!("Tie between {}", offers.iter().map(|o| o.label.as_str()).collect::<Vec<_>>().join(" and ")));
                            for o in offers {
                                match (o.has_target, o.slide) {
                                    (true, Some(s)) => {
                                        if row_button(ui, &format!("{}: go to slide {}", o.label, s + 1), true).clicked() {
                                            *go_to = Some(s);
                                        }
                                    }
                                    (true, None) => dim(ui, &format!("{}: its slide is no longer in the deck", o.label)),
                                    _ => {}
                                }
                            }
                        }
                        poll::Branch::None => {}
                    }
                }
                (TypeState::Scrub { index }, Config::Scrub(c)) => {
                    let n = c.images.len();
                    let mut v = *index as f32;
                    if n > 1
                        && ui.add(egui::Slider::new(&mut v, 0.0..=(n - 1) as f32).step_by(1.0).show_value(false)).changed()
                        && let Some(ObjectState { st: TypeState::Scrub { index }, .. }) = ext.states.get_mut(&it.key())
                    {
                        *index = (v.round() as usize).min(n.saturating_sub(1));
                        ext.changed_at = now;
                    }
                    dim(ui, c.labels.get(*index).map(String::as_str).unwrap_or(""));
                    if row_button(ui, "Next", false).clicked() {
                        ext.fire(show_zoom, it, now, ui.ctx());
                    }
                }
                (TypeState::Measure { preset, handles }, Config::Measure(c)) => {
                    for p in measure::Preset::ALL {
                        if row_button(ui, p.label(), *preset == p).clicked()
                            && let Some(ObjectState { st: TypeState::Measure { preset, handles }, .. }) = ext.states.get_mut(&it.key())
                        {
                            // A preset button starts that measurement with fresh handles, the
                            // active one included.
                            *preset = p;
                            *handles = measure::default_handles(p);
                            ext.changed_at = now;
                        }
                    }
                    let aspect = it.xfrm.w / it.xfrm.h.max(1e-6);
                    let r = measure::reading(*preset, handles, aspect);
                    dim(ui, &r.text);
                    if row_button(ui, "Reset", false).clicked() {
                        ext.reset(it, show_zoom);
                    }
                    let _ = c;
                }
                (TypeState::InjectionMap { points, product, dose, pinned }, Config::InjectionMap(c)) => {
                    for (i, p) in c.products.iter().enumerate() {
                        let (r, g, b) = {
                            let v = obj::parse_color(&p.color);
                            (v[0], v[1], v[2])
                        };
                        let fill = if *product == i { Color32::from_rgb(r, g, b) } else { Color32::from_gray(60) };
                        let btn = ui.add(
                            egui::Button::new(egui::RichText::new(&p.name).size(13.0).color(Color32::WHITE)).fill(fill).min_size(vec2(0.0, 26.0)),
                        );
                        if btn.clicked()
                            && let Some(ObjectState { st: TypeState::InjectionMap { product, dose, .. }, .. }) = ext.states.get_mut(&it.key())
                        {
                            *product = i;
                            *dose = p.default_dose;
                        }
                    }
                    let mut d = *dose;
                    let unit = c.products.get(*product).map(|p| p.unit.as_str()).unwrap_or("");
                    if ui.add(egui::DragValue::new(&mut d).update_while_editing(false).speed(0.1).range(0.0..=1000.0).max_decimals(2).suffix(format!(" {unit}"))).changed()
                        && let Some(ObjectState { st: TypeState::InjectionMap { dose, .. }, .. }) = ext.states.get_mut(&it.key())
                    {
                        *dose = d;
                    }
                    if row_button(ui, "Undo last", false).clicked()
                        && let Some(ObjectState { st: TypeState::InjectionMap { points, .. }, .. }) = ext.states.get_mut(&it.key())
                    {
                        points.pop();
                        ext.changed_at = now;
                    }
                    if row_button(ui, "Clear", false).clicked()
                        && let Some(ObjectState { st: TypeState::InjectionMap { points, .. }, .. }) = ext.states.get_mut(&it.key())
                    {
                        points.clear();
                        ext.changed_at = now;
                    }
                    let mut pin = *pinned;
                    if ui
                        .checkbox(&mut pin, egui::RichText::new("Pin").color(Color32::WHITE))
                        .on_hover_text("Keep the points when the slide changes")
                        .changed()
                        && let Some(ObjectState { st: TypeState::InjectionMap { pinned, .. }, .. }) = ext.states.get_mut(&it.key())
                    {
                        *pinned = pin;
                    }
                    let line = injection::totals_line(c, points);
                    dim(ui, if line.is_empty() { "No points yet. Click the slide picture to place one." } else { &line });
                }
                _ => {}
            }
        });
        ui.add_space(2.0);
    }
}

// ---------------------------------------------------------------------------------------------
// Displays

use deckcraft_show_ext::{presenter_frame as frame_of, split};

/// The presenter window frame covering `display`, in egui points.
pub fn presenter_frame(display: &DisplayInfo) -> Option<(Pos2, egui::Vec2)> {
    frame_of(display).map(|f| (pos2(f[0], f[1]), vec2(f[2], f[3])))
}

/// When the show starts: on two displays the audience screen goes borderless-fullscreen on the
/// one that is not the primary and the presenter window to the primary; otherwise full screen
/// where the window is.
pub fn place_show(ctx: &egui::Context, show: &mut Show, displays: Vec<DisplayInfo>) {
    show.ext.displays = displays;
    match split(&show.ext.displays) {
        Some((audience, presenter)) if show.presenter => {
            log::info!("deckrt: audience screen on display {}, presenter view on display {}", audience + 1, presenter + 1);
            show.ext.audience = Some(audience);
            show.ext.presenter_display = Some(presenter);
            show.ext.restore = ctx.input(|i| {
                let v = i.viewport();
                let outer = v.outer_rect?;
                let inner = v.inner_rect.map(|r| r.size()).unwrap_or_else(|| outer.size());
                Some(SavedWindow { pos: outer.min, inner, maximized: v.maximized.unwrap_or(false) })
            });
            // Full screen here first, so a display that went away between the listing and this
            // command still leaves the show full screen somewhere; then the chosen display.
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::SetMonitor(audience));
        }
        _ => {
            // One screen: the show alone, as PowerPoint does, unless recording or the coach
            // needs presenter view's controls, or Present in Presenter View asked for it. Then
            // presenter view covers the screen, over the show.
            if show.ext.displays.len() == 1 && !show.ext.record_wanted && !show.ext.coach_wanted && !show.ext.practice {
                show.presenter = false;
            }
            if show.ext.practice {
                // The show stays in the editor's window, behind presenter view, rather than
                // full screen over it.
                show.presenter = true;
                show.ext.presenter_display = show.ext.displays.first().map(|d| d.index);
                return;
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true))
        }
    }
}

/// Slide Show › Present in Presenter View (Alt+F5): the show from the current slide with presenter
/// view, on one screen as well as two.
pub fn present_in_presenter_view(app: &mut SlideApp) {
    let from = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
    app.start_show(from, false);
    if let Some(s) = app.show.as_mut() {
        s.ext.practice = true;
        s.presenter = true;
    }
}

/// The Slide Show tab's button for it, after Presenter View.
pub fn practice_button(app: &mut SlideApp, ui: &mut Ui) {
    let en = app.session.active().is_some_and(|d| !d.doc.slides.is_empty());
    if crate::widgets::big_button(ui, crate::icons::Icon::PresenterView, "Present in\nPresenter View", en)
        .on_hover_text("Presenter view on this screen, with no projector needed (Alt+F5)")
        .clicked()
    {
        present_in_presenter_view(app);
    }
}

/// When the show ends: the editor window goes back where it was.
pub fn restore_window(ctx: &egui::Context, show: &mut Show) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
    if let Some(saved) = show.ext.restore.take() {
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(saved.pos));
        if saved.maximized {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
        } else if saved.inner.x > 1.0 && saved.inner.y > 1.0 && saved.inner.is_finite() {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(saved.inner));
        }
    }
}

/// Move the audience screen to display `index` (the presenter picked one, or pressed Swap).
pub fn set_audience_display(ctx: &egui::Context, show: &mut Show, index: usize) {
    if show.ext.audience == Some(index) {
        return;
    }
    let was_presenter = show.ext.presenter_display;
    let left = show.ext.audience;
    show.ext.audience = Some(index);
    if was_presenter == Some(index) {
        // Swap: the presenter window takes the display the audience screen leaves (with three
        // displays, not just any other one).
        show.ext.presenter_display = left.filter(|i| *i != index).or_else(|| show.ext.displays.iter().map(|d| d.index).find(|i| *i != index));
        show.ext.move_presenter = true;
    }
    ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::SetMonitor(index));
}

/// The presenter window's builder: on its own display when the show is split.
pub fn presenter_builder(show: &Show) -> egui::ViewportBuilder {
    let mut b = egui::ViewportBuilder::default().with_title("Presenter View — deckrt").with_inner_size([1100.0, 700.0]);
    if let Some(d) = show.ext.presenter_display.and_then(|i| show.ext.displays.iter().find(|d| d.index == i))
        && let Some((pos, size)) = presenter_frame(d)
    {
        b = b.with_position(pos).with_inner_size(size).with_maximized(true);
    }
    b
}

/// The display picker and Swap in the presenter window's toolbar.
pub fn presenter_toolbar(ui: &mut Ui, show: &mut Show) {
    dark_widgets(ui);
    super::record::presenter_controls(ui, show);
    super::coach::presenter_status(ui, show);
    if show.ext.captions.is_some() {
        ui.separator();
        super::captions::presenter_controls(ui, show);
    }
    if show.ext.displays.len() < 2 {
        return;
    }
    let ctx = ui.ctx().clone();
    ui.separator();
    dim(ui, "Audience on");
    let current = show.ext.audience.unwrap_or(0);
    let labels: Vec<(usize, String)> = show.ext.displays.iter().map(|d| (d.index, d.label())).collect();
    let cur_label = labels.iter().find(|(i, _)| *i == current).map(|(_, l)| l.clone()).unwrap_or_default();
    let mut chosen = None;
    egui::ComboBox::from_id_salt("deckrt-audience-display").selected_text(cur_label).show_ui(ui, |ui| {
        for (i, l) in &labels {
            if ui.selectable_label(*i == current, l).clicked() {
                chosen = Some(*i);
            }
        }
    });
    if let Some(i) = chosen {
        set_audience_display(&ctx, show, i);
    }
    if row_button(ui, "Swap", false).on_hover_text("Exchange the audience screen and the presenter window").clicked()
        && let Some(other) = show.ext.presenter_display
    {
        set_audience_display(&ctx, show, other);
    }
}

/// The presenter window asked to move: once per request, from inside the presenter viewport.
pub fn move_presenter_if_asked(ui: &Ui, show: &mut Show) {
    if !show.ext.move_presenter {
        return;
    }
    show.ext.move_presenter = false;
    if let Some(d) = show.ext.presenter_display.and_then(|i| show.ext.displays.iter().find(|d| d.index == i))
        && let Some((pos, size)) = presenter_frame(d)
    {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(false));
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(true));
    }
}

/// Media a show draws that the slide picture leaves out: for the poster fallback and the phones'
/// pictures.
pub fn poster_of(sh: &Shape) -> Option<MediaId> {
    match &sh.kind {
        ShapeKind::Model3d { poster, .. } => *poster,
        _ => None,
    }
}

// ---------------------------------------------------------------------------------------------
// Glue called from upstream's `show.rs`

use crate::SlideApp;

/// Once a frame, after the keys: the states follow the slide, the show's own zoom is followed,
/// the phones are served and sent a live picture when something they can see changed.
pub fn logic(app: &mut SlideApp, show: &mut Show, ctx: &egui::Context, doc: &Arc<Presentation>, now: f64) {
    let idx = show.state.slide;
    super::notes::start(app, show);
    show.ext.sync_slide(doc, idx);
    if !show.reading && !show.preview_only {
        fire_on_open(show, doc, now, ctx);
    }
    show.ext.follow_zoom(&show.zoom);
    if show.presenter && !show.reading && !show.preview_only && show.ext.phones.is_none() && !show.ext.phones_tried {
        show.ext.phones_tried = true;
        let deck_path = app.session.active().and_then(|d| d.path.clone()).map(std::path::PathBuf::from);
        let mut phones = Phones::start(deck_path, None);
        phones.prepare_slides(doc);
        if let super::phones::Status::Failed(e) = &phones.status {
            show.ext.error = Some(format!("The phone server did not start: {e}"));
        }
        show.ext.phones = Some(phones);
    }
    super::record::logic(show, ctx);
    // The end of the show opens the feedback form on the phones, once.
    if show.ended
        && let Some(ph) = show.ext.phones.as_mut()
        && !ph.crowd.auto_asked
    {
        ph.crowd.auto_asked = true;
        if !ph.crowd.feedback_open {
            ph.crowd.feedback_open = true;
            ph.changed();
        }
    }
    super::coach::logic(app, show, ctx, doc, now);
    super::captions::logic(app, show, ctx, doc, now);
    show.ext.pace = if show.ext.coach.is_none() && !show.reading && !show.ended {
        super::coach::pace_line(doc, idx, now - show.slide_start, now - show.show_start)
    } else {
        None
    };
    let blackout = show.blank.is_some();
    let Ext { phones, states, pointer, ink, changed_at, .. } = &mut show.ext;
    if let Some(ph) = phones.as_mut()
        && ph.logic(states, doc, idx, pointer, ink, changed_at, now, blackout)
    {
        ctx.request_repaint();
    }
    if show.ext.phones.as_ref().is_some_and(|p| p.running()) {
        let mut phones = show.ext.phones.take();
        if let Some(ph) = phones.as_mut() {
            ph.live_frame(&show.ext, doc, idx, &mut app.deckrt.models, now);
        }
        show.ext.phones = phones;
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}

/// The audience screen's pointer, before the show acts on a click. Returns whether an object
/// took the click.
pub fn audience_pointer(show: &mut Show, ctx: &egui::Context, srect: Rect, doc: &Arc<Presentation>, now: f64) -> bool {
    if show.grid || show.ended || show.blank.is_some() || show.pen {
        return false;
    }
    let hovered = ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| srect.contains(p));
    let map = Map::new(srect, show.zoom_uv(), doc);
    let idx = show.state.slide;
    // Two fingers on a touch screen: a pinch zooms the slide in and out and a drag moves the
    // zoomed view. Over a 3D model the pinch zooms the model instead (in `pointer`).
    if let Some(mt) = ctx.multi_touch()
        && mt.num_touches >= 2
        && srect.contains(mt.start_pos)
    {
        let over_model = objects_on(doc, idx).iter().any(|it| it.obj.kind() == Kind::Model && map.rect(&it.xfrm).contains(mt.start_pos));
        if !over_model {
            let (dx, dy) = (mt.translation_delta.x / srect.width().max(1.0), mt.translation_delta.y / srect.height().max(1.0));
            match show.zoom.as_mut() {
                Some(z) => {
                    if !z.touch(dx, dy, mt.zoom_delta) {
                        show.zoom = None;
                    }
                }
                None if mt.zoom_delta > 1.01 => {
                    let (ux, uy) = map.to_unit(mt.start_pos);
                    show.zoom = Some(crate::show::Zoom::region(ux, uy, mt.zoom_delta.max(1.05)));
                }
                None => {}
            }
            show.ext.took_scroll = true;
            ctx.request_repaint();
            return true;
        }
    }
    pointer(ctx, map, doc, idx, &mut show.ext, &mut show.zoom, Role::Audience, now, hovered)
}

/// The objects over the slide picture on the audience screen.
pub fn draw_audience(
    app: &mut SlideApp,
    show: &mut Show,
    ctx: &egui::Context,
    painter: &egui::Painter,
    srect: Rect,
    doc: &Arc<Presentation>,
    now: f64,
) {
    let map = Map::new(srect, show.zoom_uv(), doc);
    let idx = show.state.slide;
    draw(ctx, painter, map, doc, idx, &mut show.ext, &mut app.deckrt, Role::Audience, now);
}

/// Width of the QR column beside the slide, when the QR is shown to the audience.
pub fn audience_avail(show: &Show, avail: Rect) -> Rect {
    if !show.ext.qr_shown || !show.ext.phones.as_ref().is_some_and(|p| p.running()) {
        return avail;
    }
    let w = (avail.width() * 0.26).clamp(180.0, 560.0);
    Rect::from_min_max(avail.min, pos2(avail.max.x - w, avail.max.y))
}

/// The QR column: the code large enough to scan from the back of a room, the address under it.
pub fn draw_qr(show: &Show, painter: &egui::Painter, full: Rect, srect: Rect) {
    if !show.ext.qr_shown {
        return;
    }
    let Some(ph) = show.ext.phones.as_ref().filter(|p| p.running()) else { return };
    let col = Rect::from_min_max(pos2(srect.max.x + 12.0, full.min.y), full.max);
    if col.width() < 60.0 {
        return;
    }
    let side = (col.width() - 24.0).min(col.height() - 80.0).max(40.0);
    let qr = Rect::from_center_size(pos2(col.center().x, col.center().y - 24.0), vec2(side, side));
    super::phones::paint_qr(painter, qr, &ph.url);
    painter.text(pos2(col.center().x, qr.max.y + 12.0), Align2::CENTER_TOP, "Scan to join", FontId::proportional(22.0), Color32::WHITE);
    painter.text(pos2(col.center().x, qr.max.y + 42.0), Align2::CENTER_TOP, ph.url.clone(), FontId::proportional(15.0), Color32::from_gray(200));
}

/// The end of the show: the phones are told and the results written, the pictures dropped.
pub fn end_show(app: &mut SlideApp, show: &mut Show, doc: &Presentation) {
    super::record::end_show(app, show);
    super::coach::end_show(app, show);
    super::notes::end(app, show);
    if let Some(ph) = show.ext.phones.take() {
        ph.stop(&show.ext.states, doc);
    }
    app.deckrt.pictures.clear();
}

/// In the presenter window: the objects over the current slide's picture (taking the presenter's
/// clicks and drags) and one control row per object under it. `go_to` is set when the presenter
/// follows a poll's winning answer.
#[allow(clippy::too_many_arguments)]
pub fn presenter_slide(
    ui: &mut Ui,
    show: &mut Show,
    doc: &Arc<Presentation>,
    deckrt: &mut super::State,
    cr: Rect,
    rows: Rect,
    now: f64,
    go_to: &mut Option<usize>,
) {
    let idx = show.state.slide;
    let map = Map::new(cr, show.zoom_uv(), doc);
    let hovered = ui.rect_contains_pointer(cr);
    if !show.zoom_arm && !show.grid {
        let _ = ui.interact(cr, ui.id().with("deckrt-presenter-slide"), egui::Sense::click_and_drag());
        pointer(ui.ctx(), map, doc, idx, &mut show.ext, &mut show.zoom, Role::Presenter, now, hovered);
    }
    draw(ui.ctx(), ui.painter(), map, doc, idx, &mut show.ext, deckrt, Role::Presenter, now);
    if rows.height() < 20.0 {
        return;
    }
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rows));
    egui::ScrollArea::vertical().id_salt("deckrt-rows").show(&mut child, |ui| {
        if let Some(e) = &show.ext.error {
            dim(ui, e);
        }
        presenter_rows(ui, &mut show.ext, &mut show.zoom, doc, idx, now, go_to);
    });
}

/// The Phones section in the presenter window's right column. Returns the height it used.
pub fn presenter_phones(ui: &mut Ui, show: &mut Show, rect: Rect) -> f32 {
    if !show.presenter || show.reading {
        return 0.0;
    }
    let h = super::phones::section(ui, &mut show.ext, rect) + 8.0;
    // deckrt: questions from the phones, under the Phones section.
    let Some(ph) = show.ext.phones.as_mut().filter(|p| p.running()) else { return h };
    let qrect = Rect::from_min_max(pos2(rect.min.x, rect.min.y + h), rect.max);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(qrect));
    dark_widgets(&mut child);
    let slide = show.state.slide;
    if super::crowd::section(&mut child, &mut ph.crowd, slide) {
        ph.changed();
    }
    child.add_space(4.0);
    if super::questions::section(&mut child, &mut ph.questions) {
        ph.changed();
    }
    h + child.min_rect().height() + 8.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_map_goes_both_ways() {
        let doc = Presentation { slide_size: deckcraft_geom::Size::new(960.0, 540.0), ..Default::default() };
        let m = Map::new(Rect::from_min_size(pos2(100.0, 50.0), vec2(960.0, 540.0)), unit(), &doc);
        let p = m.to_screen(480.0, 270.0);
        assert_eq!(p, pos2(580.0, 320.0));
        let (x, y) = m.to_unit(p);
        assert!((x - 0.5).abs() < 1e-6 && (y - 0.5).abs() < 1e-6);
        let z = Map::new(m.srect, Rect::from_min_size(pos2(0.5, 0.5), vec2(0.5, 0.5)), &doc);
        assert_eq!(z.to_screen(480.0, 270.0), pos2(100.0, 50.0));
    }

    #[test]
    fn shortcuts_fire_the_object_with_that_key() {
        let mut s = deckcraft_engine::Session::with_new();
        s.execute("slide.new", &serde_json::json!({"layout": "blank"})).expect("slide");
        s.execute("interactive.insert", &serde_json::json!({"kind": "popup", "object": {"shortcut": "p"}})).expect("insert");
        let doc = s.doc().expect("doc").doc.clone();
        let mut ext = Ext::default();
        let mut zoom = None;
        let ctx = egui::Context::default();
        assert!(shortcut(&mut ext, &mut zoom, &doc, 1, egui::Key::P, egui::Modifiers::NONE, 0.0, &ctx));
        let open = ext.states.values().any(|s| matches!(s.st, TypeState::Popup { open: true }));
        assert!(open);
        assert!(!shortcut(&mut ext, &mut zoom, &doc, 1, egui::Key::Q, egui::Modifiers::NONE, 0.0, &ctx));
        assert!(!shortcut(&mut ext, &mut zoom, &doc, 1, egui::Key::P, egui::Modifiers::COMMAND, 0.0, &ctx));
    }

    #[test]
    fn next_runs_an_object_in_the_click_order_then_moves_on() {
        let mut s = deckcraft_engine::Session::with_new();
        s.execute("slide.new", &serde_json::json!({"layout": "blank"})).expect("slide");
        s.execute("interactive.insert", &serde_json::json!({"kind": "popup", "object": {"next_clicks": 2}})).expect("insert");
        let doc = s.doc().expect("doc").doc.clone();
        let mut show = Show::new(&doc, 1, false, false);
        let ctx = egui::Context::default();
        let open = |show: &Show| show.ext.states.values().any(|s| matches!(s.st, TypeState::Popup { open: true }));
        assert!(take_next(&mut show, &doc, 0.0, &ctx));
        assert!(open(&show));
        assert!(take_next(&mut show, &doc, 0.1, &ctx));
        assert!(!open(&show), "the second Next closes it again");
        assert!(!take_next(&mut show, &doc, 0.2, &ctx), "the third Next is the show's");
    }

    #[test]
    fn a_hidden_object_does_not_take_next() {
        let mut s = deckcraft_engine::Session::with_new();
        s.execute("slide.new", &serde_json::json!({"layout": "blank"})).expect("slide");
        s.execute("interactive.insert", &serde_json::json!({"kind": "popup", "object": {"next_clicks": 1}})).expect("insert");
        let doc = s.doc().expect("doc").doc.clone();
        let mut show = Show::new(&doc, 1, false, false);
        let ctx = egui::Context::default();
        show.ext.sync_slide(&doc, 1);
        let id = objects_on(&doc, 1).pop().expect("popup").id;
        show.ext.set_visible(id, false);
        assert!(!take_next(&mut show, &doc, 0.0, &ctx), "Next goes to the show");
    }

    #[test]
    fn swap_with_three_displays_puts_the_presenter_where_the_audience_was() {
        let doc = deckcraft_model::defaults::new_presentation(None);
        let ctx = egui::Context::default();
        let mut show = Show::new(&doc, 0, false, false);
        let d = |index: usize| deckcraft_show_ext::DisplayInfo { index, origin: [index as i32 * 1920, 0], size_px: [1920, 1080], scale: 1.0, primary: index == 0 };
        show.ext.displays = vec![d(0), d(1), d(2)];
        show.ext.audience = Some(2);
        show.ext.presenter_display = Some(0);
        set_audience_display(&ctx, &mut show, 0);
        assert_eq!((show.ext.audience, show.ext.presenter_display), (Some(0), Some(2)));
    }

    #[test]
    fn present_in_presenter_view_keeps_presenter_view_on_one_screen() {
        let doc = deckcraft_model::defaults::new_presentation(None);
        let ctx = egui::Context::default();
        let one = || vec![DisplayInfo { index: 0, origin: [0, 0], size_px: [1920, 1080], scale: 1.0, primary: true }];
        let mut show = Show::new(&doc, 0, false, true);
        place_show(&ctx, &mut show, one());
        assert!(!show.presenter, "a plain show on one screen is the show alone");
        let mut show = Show::new(&doc, 0, false, false);
        show.ext.practice = true;
        place_show(&ctx, &mut show, one());
        assert!(show.presenter);
        assert_eq!(show.ext.presenter_display, Some(0), "presenter view covers the one screen");
    }

    #[test]
    fn an_object_runs_by_itself_after_its_delay() {
        let mut s = deckcraft_engine::Session::with_new();
        s.execute("slide.new", &serde_json::json!({"layout": "blank"})).expect("slide");
        s.execute("interactive.insert", &serde_json::json!({"kind": "popup", "object": {"with_slide": true, "delay": 2.0}})).expect("insert");
        let doc = s.doc().expect("doc").doc.clone();
        let mut show = Show::new(&doc, 1, false, false);
        let ctx = egui::Context::default();
        let open = |show: &Show| show.ext.states.values().any(|s| matches!(s.st, TypeState::Popup { open: true }));
        fire_on_open(&mut show, &doc, 10.0, &ctx);
        assert!(!open(&show));
        fire_on_open(&mut show, &doc, 11.0, &ctx);
        assert!(!open(&show));
        fire_on_open(&mut show, &doc, 12.1, &ctx);
        assert!(open(&show), "two seconds after the slide opened");
        fire_on_open(&mut show, &doc, 20.0, &ctx);
        assert!(open(&show), "once per visit, so it isn't toggled shut");
    }

    #[test]
    fn a_zoom_region_sets_the_shows_zoom_and_follows_its_dismissal() {
        let mut s = deckcraft_engine::Session::with_new();
        s.execute("slide.new", &serde_json::json!({"layout": "blank"})).expect("slide");
        s.execute("interactive.insert", &serde_json::json!({"kind": "zoom", "rect": [240.0, 135.0, 480.0, 270.0]})).expect("insert");
        let doc = s.doc().expect("doc").doc.clone();
        let mut ext = Ext::default();
        ext.sync_slide(&doc, 1);
        let it = objects_on(&doc, 1).pop().expect("the zoom");
        let mut zoom = None;
        let ctx = egui::Context::default();
        assert!(ext.fire(&mut zoom, &it, 0.0, &ctx));
        assert!(zoom.is_some());
        zoom = None;
        ext.follow_zoom(&zoom);
        assert!(matches!(ext.states[&it.key()].st, TypeState::Zoom { zoomed: false, .. }));
    }

    /// Both windows share the object states and each reads its own pointer: a drag begun on the
    /// audience screen must survive the presenter window's frames, where no button is down.
    #[test]
    fn a_drag_in_one_window_survives_the_other_windows_frames() {
        let mut s = deckcraft_engine::Session::with_new();
        s.execute("slide.new", &serde_json::json!({"layout": "blank"})).expect("slide");
        let png = {
            let mut out = Vec::new();
            let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([10, 20, 30, 255]));
            image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).expect("png");
            out
        };
        let png = deckcraft_engine::cmd::base64_encode(&png);
        let files = serde_json::json!([{"data": png, "name": "a.png"}, {"data": png, "name": "b.png"}]);
        s.execute("interactive.insert", &serde_json::json!({"kind": "wipe", "files": files, "rect": [0.0, 0.0, 960.0, 540.0]})).expect("insert");
        let doc = std::sync::Arc::new(s.doc().expect("doc").doc.clone());
        let mut ext = Ext::default();
        let mut zoom = None;
        let ctx = egui::Context::default();
        let map = Map::new(Rect::from_min_size(pos2(0.0, 0.0), vec2(960.0, 540.0)), unit(), &doc);
        let press = egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(pos2(480.0, 270.0)),
                egui::Event::PointerButton {
                    pos: pos2(480.0, 270.0),
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        };
        ctx.run_ui(press, |ui| {
            pointer(ui.ctx(), map, &doc, 1, &mut ext, &mut zoom, Role::Audience, 0.0, true);
        })
        .drop_without_applying_deltas();
        assert!(matches!(ext.drag, Some(Drag::Wipe { .. })), "the press starts the drag");
        // The presenter window's frame: no button down there.
        let other = egui::Context::default();
        other
            .run_ui(egui::RawInput::default(), |ui| {
                pointer(ui.ctx(), map, &doc, 1, &mut ext, &mut zoom, Role::Presenter, 0.1, false);
            })
            .drop_without_applying_deltas();
        assert!(matches!(ext.drag, Some(Drag::Wipe { .. })), "the other window leaves it alone");
        // The audience pointer moves: the divider follows.
        let moved = egui::RawInput { events: vec![egui::Event::PointerMoved(pos2(240.0, 270.0))], ..Default::default() };
        ctx.run_ui(moved, |ui| {
            pointer(ui.ctx(), map, &doc, 1, &mut ext, &mut zoom, Role::Audience, 0.2, true);
        })
        .drop_without_applying_deltas();
        let pos = ext.states.values().find_map(|s| match s.st {
            TypeState::Wipe { position } => Some(position),
            _ => None,
        });
        assert!(pos.is_some_and(|p| (p - 0.25).abs() < 0.01), "{pos:?}");
    }

    #[test]
    fn an_unpinned_injection_map_drops_its_points_on_the_next_slide() {
        let mut s = deckcraft_engine::Session::with_new();
        s.execute("slide.new", &serde_json::json!({"layout": "blank"})).expect("slide");
        s.execute("interactive.insert", &serde_json::json!({"kind": "injection_map"})).expect("insert");
        let doc = s.doc().expect("doc").doc.clone();
        let mut ext = Ext::default();
        ext.sync_slide(&doc, 1);
        let id = objects_on(&doc, 1).pop().expect("the map").id;
        if let Some(ObjectState { st: TypeState::InjectionMap { points, .. }, .. }) = ext.states.get_mut(&(1, id)) {
            points.push(Point { x: 0.5, y: 0.5, product: 0, dose: 4.0 });
        }
        ext.sync_slide(&doc, 0);
        ext.sync_slide(&doc, 1);
        assert!(matches!(&ext.states[&(1, id)].st, TypeState::InjectionMap { points, .. } if points.is_empty()));
    }

    /// Two slides that each have a poll with the same shape id keep their own votes.
    #[test]
    fn objects_with_the_same_id_on_two_slides_keep_their_own_state() {
        let mut s = deckcraft_engine::Session::with_new();
        for _ in 0..2 {
            s.execute("slide.new", &serde_json::json!({"layout": "blank"})).expect("slide");
            s.execute("interactive.insert", &serde_json::json!({"kind": "poll"})).expect("insert");
        }
        let doc = s.doc().expect("doc").doc.clone();
        let (a, b) = (objects_on(&doc, 1).pop().expect("poll 1"), objects_on(&doc, 2).pop().expect("poll 2"));
        let mut ext = Ext::default();
        ext.sync_slide(&doc, 1);
        if let Some(ObjectState { st: TypeState::Poll { votes, .. }, .. }) = ext.states.get_mut(&a.key()) {
            votes.insert("phone".into(), 0);
        }
        ext.sync_slide(&doc, 2);
        let votes = |ext: &Ext, it: &Item| match &ext.states[&it.key()].st {
            TypeState::Poll { votes, .. } => votes.len(),
            _ => 99,
        };
        assert_eq!((votes(&ext, &a), votes(&ext, &b)), (1, 0), "ids {} and {}", a.id, b.id);
    }
}
