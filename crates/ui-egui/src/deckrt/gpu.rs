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
    /// The window's colour format, which egui's renderer draws in.
    format: wgpu::TextureFormat,
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
        Gpu { device, queue, renderer: rs.renderer.clone(), format: rs.target_format, models, textures: HashMap::new(), used: vec![] }
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

    /// egui shapes drawn into a picture of `size` pixels at `ppp` pixels per point, the shapes'
    /// (0, 0) at its top left, over white. The textures the shapes use must be uploaded already
    /// (shapes from an earlier frame). `None` when the window's format isn't 8-bit RGBA or BGRA.
    pub fn paint_offscreen(
        &self,
        ctx: &egui::Context,
        shapes: Vec<egui::epaint::ClippedShape>,
        size: [u32; 2],
        ppp: f32,
    ) -> Option<image::RgbaImage> {
        use wgpu::TextureFormat as F;
        let bgra = match self.format {
            F::Bgra8Unorm | F::Bgra8UnormSrgb => true,
            F::Rgba8Unorm | F::Rgba8UnormSrgb => false,
            _ => return None,
        };
        let [w, h] = [size[0].clamp(1, 4096), size[1].clamp(1, 4096)];
        let prims = ctx.tessellate(shapes, ppp);
        let extent = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("deckrt phone copy"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: self.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [w, h], pixels_per_point: ppp };
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("deckrt phone copy") });
        let extra = {
            let mut r = self.renderer.write();
            let extra = r.update_buffers(&self.device, &self.queue, &mut enc, &prims, &screen);
            let pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("deckrt phone copy"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let mut pass = pass.forget_lifetime();
            r.render(&mut pass, &prims, &screen);
            extra
        };
        let row = (w * 4).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("deckrt phone copy"),
            size: row as u64 * h as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) },
            },
            extent,
        );
        self.queue.submit(extra.into_iter().chain(Some(enc.finish())));
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r.is_ok());
        });
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        if !rx.recv().unwrap_or(false) {
            return None;
        }
        let data = slice.get_mapped_range().ok()?;
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h as usize {
            for px in data.get(y * row as usize..y * row as usize + (w * 4) as usize).unwrap_or(&[]).chunks_exact(4) {
                if bgra {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
                } else {
                    rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
                }
            }
        }
        drop(data);
        buf.unmap();
        image::RgbaImage::from_raw(w, h, rgba)
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
