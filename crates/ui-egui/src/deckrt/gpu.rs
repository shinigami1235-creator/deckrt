//! The wgpu path for 3D models: the model is drawn into an offscreen texture by
//! [`deckcraft_model3d::gpu::Renderer`], and that texture is registered with egui's renderer so
//! the canvas and the show paint it like any image. The desktop app hands over the device, queue
//! and egui renderer at start-up ([`crate::SlideApp::set_gpu`]).

use std::collections::HashMap;
use std::sync::Arc;

use deckcraft_model3d::gpu::Renderer;
use deckcraft_model3d::{Model, View};
use egui_wgpu::RenderState;
use egui_wgpu::wgpu;

type RendererLock = egui::mutex::RwLock<egui_wgpu::Renderer>;

pub struct Gpu {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    renderer: Arc<RendererLock>,
    models: Renderer,
    textures: HashMap<u64, egui::TextureId>,
    /// Slots drawn this frame, so the rest can be released.
    used: Vec<u64>,
}

impl Gpu {
    pub fn new(rs: &RenderState) -> Self {
        let device = Arc::new(rs.device.clone());
        let queue = Arc::new(rs.queue.clone());
        let models = Renderer::new(device.clone(), queue.clone());
        Gpu { device, queue, renderer: rs.renderer.clone(), models, textures: HashMap::new(), used: vec![] }
    }

    pub fn adapter_name(rs: &RenderState) -> String {
        rs.adapter.get_info().name
    }

    pub fn has_model(&self, key: u64) -> bool {
        self.models.has(key)
    }

    pub fn load(&mut self, key: u64, model: Arc<Model>) {
        self.models.load(key, model);
    }

    pub fn unload(&mut self, key: u64) {
        self.models.unload(key);
    }

    /// Draw model `key` for `slot` at `w` × `h` pixels and give the egui texture to paint.
    pub fn draw(&mut self, slot: u64, key: u64, view: &View, w: u32, h: u32, hidden: &[String]) -> Option<egui::TextureId> {
        let w = w.clamp(1, 4096);
        let h = h.clamp(1, 4096);
        let out = self.models.render(slot, key, view, w, h, hidden, [0.0, 0.0, 0.0, 0.0])?;
        let recreated = out.recreated;
        let view_ref = out.view;
        let id = if recreated || !self.textures.contains_key(&slot) {
            let mut r = self.renderer.write();
            if let Some(old) = self.textures.remove(&slot) {
                r.free_texture(&old);
            }
            let id = r.register_native_texture(&self.device, view_ref, wgpu::FilterMode::Linear);
            self.textures.insert(slot, id);
            id
        } else {
            *self.textures.get(&slot)?
        };
        if !self.used.contains(&slot) {
            self.used.push(slot);
        }
        let _ = &self.queue;
        Some(id)
    }

    /// Pixels of `slot` as last drawn (screenshots, tests).
    pub fn read_back(&self, slot: u64) -> Option<deckcraft_model3d::soft::Image> {
        self.models.read_back(slot)
    }

    /// End of a frame: textures of slots not drawn this frame are released.
    pub fn end_frame(&mut self) {
        let stale: Vec<u64> = self.textures.keys().copied().filter(|s| !self.used.contains(s)).collect();
        if !stale.is_empty() {
            let mut r = self.renderer.write();
            for s in stale {
                if let Some(id) = self.textures.remove(&s) {
                    r.free_texture(&id);
                }
                self.models.release(s);
            }
        }
        self.used.clear();
    }
}
