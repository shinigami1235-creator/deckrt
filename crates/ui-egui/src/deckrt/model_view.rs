//! Drawing a 3D model object on the editor canvas and the show screen.
//!
//! The model bytes come from the deck's media and are decoded once per media item. A model is
//! drawn through wgpu when the app gave us a device ([`super::gpu`]), else through the software
//! rasteriser into an egui texture, at a lower resolution while it is being turned so the drag
//! stays fluid.

use std::collections::HashMap;
use std::sync::Arc;

use deckcraft_interactive::object::{CameraPose, ModelConfig};
use deckcraft_model::MediaId;
use deckcraft_model3d::{Model, View};
use egui::{Color32, Rect, TextureHandle, TextureId, TextureOptions};

/// How a model is turned in the editor on top of its configured camera (never saved unless the
/// user asks for it in the pane).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Turn {
    /// Yaw and pitch in degrees, added by dragging.
    pub turn: [f64; 2],
    /// Zoom factor from the wheel; 1 is the configured distance.
    pub zoom: f64,
}

impl Turn {
    pub fn is_zero(&self) -> bool {
        self.turn == [0.0, 0.0] && (self.zoom <= 0.0 || (self.zoom - 1.0).abs() < 1e-9)
    }
    pub fn apply(&self, view: View) -> View {
        let zoom = if self.zoom > 0.0 { self.zoom } else { 1.0 };
        let mut v = view;
        v.theta_deg -= self.turn[0];
        v.phi_deg = (v.phi_deg - self.turn[1]).clamp(deckcraft_model3d::camera::PHI_MIN_DEG, deckcraft_model3d::camera::PHI_MAX_DEG);
        v.zoomed(zoom)
    }
}

/// The camera a model is seen from: its configured start, then the live turn.
pub fn view_of(model: &Model, c: &ModelConfig, turn: &Turn) -> View {
    let pose = c.camera.map(|p| (p.orbit, p.target, p.fov_deg));
    turn.apply(deckcraft_model3d::view_for(model, c.initial_rotation, pose))
}

/// The pose to save as the configured camera.
pub fn pose_of(view: &View) -> CameraPose {
    CameraPose { orbit: [view.theta_deg, view.phi_deg, view.radius], target: view.target, fov_deg: view.fov_deg }
}

struct SoftTex {
    key: (u64, u32, u32, u64),
    tex: TextureHandle,
}

#[derive(Default)]
pub struct Models {
    loaded: HashMap<MediaId, Option<Arc<Model>>>,
    soft: HashMap<u64, SoftTex>,
    /// The editor's live turn per shape (by shape id).
    pub turns: HashMap<u32, Turn>,
    /// The last error, for the status line.
    pub error: Option<String>,
}

/// What was drawn for one model this frame.
pub struct Drawn {
    pub tex: TextureId,
    /// The software path was used (it is slower and lower-resolution while turning).
    pub soft: bool,
}

fn hash_key(parts: &[u64]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        h ^= p;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

fn view_key(v: &View, hidden: &[String]) -> u64 {
    let q = |x: f64| (x * 100.0).round() as i64 as u64;
    let mut parts = vec![q(v.theta_deg), q(v.phi_deg), q(v.radius * 1000.0), q(v.fov_deg), q(v.target[0]), q(v.target[1]), q(v.target[2])];
    parts.extend(v.rotation.iter().map(|r| q(*r * 1000.0)));
    for h in hidden {
        for b in h.bytes() {
            parts.push(b as u64);
        }
        parts.push(0xff);
    }
    hash_key(&parts)
}

impl Models {
    /// The decoded model for a media item, read on first use.
    pub fn model(&mut self, doc: &deckcraft_model::Presentation, media: MediaId) -> Option<Arc<Model>> {
        if let Some(m) = self.loaded.get(&media) {
            return m.clone();
        }
        let bytes = doc.media(media).map(|m| m.data.clone());
        let model = bytes.and_then(|b| match deckcraft_model3d::load(&b) {
            Ok(m) => Some(Arc::new(m)),
            Err(e) => {
                log::warn!("deckrt: the model could not be read: {e}");
                self.error = Some(format!("The 3D model could not be read: {e}"));
                None
            }
        });
        self.loaded.insert(media, model.clone());
        model
    }

    pub fn forget(&mut self, media: MediaId) {
        self.loaded.remove(&media);
    }

    /// Draw `model` (media `media`) into `slot` at `w` × `h` pixels. `turning` lowers the
    /// software path's resolution so a drag stays fluid.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        gpu: &mut Option<super::gpu::Gpu>,
        slot: u64,
        media: MediaId,
        model: &Arc<Model>,
        view: &View,
        w: u32,
        h: u32,
        hidden: &[String],
        turning: bool,
    ) -> Option<Drawn> {
        let key = media.0 as u64;
        if let Some(g) = gpu.as_mut() {
            if !g.has_model(key) {
                g.load(key, model.clone());
            }
            if let Some(tex) = g.draw(slot, key, view, w, h, hidden) {
                return Some(Drawn { tex, soft: false });
            }
        }
        // Software: a texture per slot, redrawn when the view, size or layers change.
        let cap = if turning { 360 } else { 1200 };
        let (w, h) = {
            let k = (cap as f64 / w.max(h).max(1) as f64).min(1.0);
            (((w as f64 * k).round() as u32).max(8), ((h as f64 * k).round() as u32).max(8))
        };
        let k = (view_key(view, hidden), w, h, key);
        if let Some(t) = self.soft.get(&slot)
            && t.key == k
        {
            return Some(Drawn { tex: t.tex.id(), soft: true });
        }
        let img = deckcraft_model3d::soft::render(model, view, w, h, hidden, None);
        let image = egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &img.rgba);
        let tex = ctx.load_texture(format!("deckrt-model-{slot}"), image, TextureOptions::LINEAR);
        let id = tex.id();
        self.soft.insert(slot, SoftTex { key: k, tex });
        Some(Drawn { tex: id, soft: true })
    }

    /// Paint a drawn model into `rect`.
    pub fn paint(painter: &egui::Painter, drawn: &Drawn, rect: Rect) {
        painter.image(drawn.tex, rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }
}
