//! The wgpu renderer: a model drawn into an offscreen texture that the app shows through egui.
//! Nothing here knows about egui; the app registers the texture view with egui's renderer.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat4, Vec4};
use wgpu::util::DeviceExt;

use crate::camera::View;
use crate::model::Model;

const SHADER: &str = r#"
struct U {
    mvp: mat4x4<f32>,
    mv: mat4x4<f32>,
    nm: mat4x4<f32>,
    base: vec4<f32>,
    flags: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
@group(1) @binding(0) var t_color: texture_2d<f32>;
@group(1) @binding(1) var s_color: sampler;

struct VIn {
    @location(0) pos: vec3<f32>,
    @location(1) nrm: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) col: vec4<f32>,
};
struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) nrm: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) col: vec4<f32>,
    @location(3) eye: vec3<f32>,
};

@vertex
fn vs(in: VIn) -> VOut {
    var out: VOut;
    let p = vec4<f32>(in.pos, 1.0);
    out.clip = u.mvp * p;
    out.eye = (u.mv * p).xyz;
    out.nrm = normalize((u.nm * vec4<f32>(in.nrm, 0.0)).xyz);
    out.uv = in.uv;
    out.col = in.col;
    return out;
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
    var c = u.base * in.col;
    if (u.flags.x > 0.5) {
        c = c * textureSample(t_color, s_color, in.uv);
    }
    if (c.a < 0.5) {
        discard;
    }
    let v = normalize(-in.eye);
    var n = normalize(in.nrm);
    if (dot(n, v) < 0.0) {
        n = -n;
    }
    let key = normalize(vec3<f32>(0.4, 0.6, 0.7));
    let fill = normalize(vec3<f32>(-0.6, 0.2, 0.5));
    let d = 0.72 * max(dot(n, key), 0.0) + 0.3 * max(dot(n, fill), 0.0);
    let h = normalize(key + v);
    let s = pow(max(dot(n, h), 0.0), 24.0) * 0.18;
    let lit = min(0.32 + d, 1.1) + s;
    let lin = clamp(c.rgb * lit, vec3<f32>(0.0), vec3<f32>(1.0));
    // The target is a plain (non-sRGB) texture egui shows as it is, so encode here.
    let lo = lin * 12.92;
    let hi = 1.055 * pow(lin, vec3<f32>(1.0 / 2.4)) - 0.055;
    let enc = select(hi, lo, lin <= vec3<f32>(0.0031308));
    return vec4<f32>(enc, 1.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    pos: [f32; 3],
    nrm: [f32; 3],
    uv: [f32; 2],
    col: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    mvp: [[f32; 4]; 4],
    mv: [[f32; 4]; 4],
    nm: [[f32; 4]; 4],
    base: [f32; 4],
    flags: [f32; 4],
}

/// Uniform stride: 256 is the largest `min_uniform_buffer_offset_alignment` in common use.
const U_STRIDE: u64 = 256;

struct GpuPrimitive {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    count: u32,
    material: Option<usize>,
    node: usize,
}

struct GpuModel {
    primitives: Vec<GpuPrimitive>,
    /// One bind group per texture; `None` uses the white texture.
    textures: Vec<wgpu::BindGroup>,
    uniforms: wgpu::Buffer,
    uniform_group: wgpu::BindGroup,
    model: Arc<Model>,
}

struct Target {
    color: wgpu::Texture,
    view: wgpu::TextureView,
    depth: wgpu::TextureView,
    size: (u32, u32),
}

pub struct Renderer {
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    pipeline: wgpu::RenderPipeline,
    uniform_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    white: wgpu::BindGroup,
    models: HashMap<u64, GpuModel>,
    targets: HashMap<u64, Target>,
}

/// What [`Renderer::render`] gives back: the texture to show, and whether it is a new texture
/// (the app registers a new one with egui then) or the one from the last frame.
pub struct Rendered<'a> {
    pub view: &'a wgpu::TextureView,
    pub texture: &'a wgpu::Texture,
    pub recreated: bool,
    pub size: (u32, u32),
}

/// A plain RGBA target holding sRGB-encoded colour, the format egui takes for a user texture.
pub const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

impl Renderer {
    pub fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Renderer {
        let shader = device
            .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("deckrt model"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("deckrt model uniforms"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64),
                },
                count: None,
            }],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("deckrt model texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("deckrt model"),
            bind_group_layouts: &[Some(&uniform_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("deckrt model"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 3 => Float32x4],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: TARGET_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("deckrt model"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let white = Self::texture_group(&device, &queue, &texture_layout, &sampler, 1, 1, &[255, 255, 255, 255]);
        Renderer { device, queue, pipeline, uniform_layout, texture_layout, sampler, white, models: HashMap::new(), targets: HashMap::new() }
    }

    fn texture_group(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        w: u32,
        h: u32,
        rgba: &[u8],
    ) -> wgpu::BindGroup {
        let size = wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 };
        let tex = device.create_texture_with_data(
            queue,
            &wgpu::TextureDescriptor {
                label: Some("deckrt model texture"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            rgba,
        );
        let view = tex.create_view(&Default::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("deckrt model texture"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
            ],
        })
    }

    pub fn has(&self, key: u64) -> bool {
        self.models.contains_key(&key)
    }

    pub fn model(&self, key: u64) -> Option<&Arc<Model>> {
        self.models.get(&key).map(|m| &m.model)
    }

    /// Upload a model under `key` (replacing one already there).
    pub fn load(&mut self, key: u64, model: Arc<Model>) {
        let mut primitives = vec![];
        for p in &model.primitives {
            let verts: Vec<Vertex> = p
                .positions
                .iter()
                .enumerate()
                .map(|(i, pos)| Vertex {
                    pos: *pos,
                    nrm: p.normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]),
                    uv: p.uvs.get(i).copied().unwrap_or([0.0, 0.0]),
                    col: p.colors.get(i).copied().unwrap_or([1.0; 4]),
                })
                .collect();
            let vertices = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("deckrt model vertices"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let indices = self.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("deckrt model indices"),
                contents: bytemuck::cast_slice(&p.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            primitives.push(GpuPrimitive { vertices, indices, count: p.indices.len() as u32, material: p.material, node: p.node });
        }
        let textures = model
            .textures
            .iter()
            .map(|t| Self::texture_group(&self.device, &self.queue, &self.texture_layout, &self.sampler, t.width, t.height, &t.rgba))
            .collect();
        let uniforms = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("deckrt model uniforms"),
            size: U_STRIDE * primitives.len().max(1) as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("deckrt model uniforms"),
            layout: &self.uniform_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &uniforms,
                    offset: 0,
                    size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64),
                }),
            }],
        });
        self.models.insert(key, GpuModel { primitives, textures, uniforms, uniform_group, model });
    }

    pub fn unload(&mut self, key: u64) {
        self.models.remove(&key);
    }

    /// Drop the offscreen texture of `slot` (an object no longer shown).
    pub fn release(&mut self, slot: u64) {
        self.targets.remove(&slot);
    }

    fn target(&mut self, slot: u64, size: (u32, u32)) -> bool {
        let size = (size.0.clamp(1, 8192), size.1.clamp(1, 8192));
        if self.targets.get(&slot).is_some_and(|t| t.size == size) {
            return false;
        }
        let extent = wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 };
        let color = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("deckrt model target"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TARGET_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let depth = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("deckrt model depth"),
            size: extent,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = color.create_view(&Default::default());
        let depth = depth.create_view(&Default::default());
        self.targets.insert(slot, Target { color, view, depth, size });
        true
    }

    /// Draw model `key` through `view` into the texture of `slot`, `width` × `height` pixels.
    /// `background` is sRGB-encoded RGBA, 0 to 1 (alpha 0 leaves the slide showing through).
    pub fn render(
        &mut self,
        slot: u64,
        key: u64,
        view: &View,
        width: u32,
        height: u32,
        hidden: &[String],
        background: [f64; 4],
    ) -> Option<Rendered<'_>> {
        if !self.models.contains_key(&key) {
            return None;
        }
        let recreated = self.target(slot, (width, height));
        let gm = self.models.get(&key)?;
        let t = self.targets.get(&slot)?;
        let aspect = t.size.0 as f32 / t.size.1 as f32;
        let model = &gm.model;
        let mvp = view.mvp(model, aspect);
        let mv = view.view_matrix() * view.model_matrix(model);
        let nm = mv.inverse().transpose();
        // One uniform block per primitive, at its stride.
        let mut data = vec![0u8; U_STRIDE as usize * gm.primitives.len().max(1)];
        for (i, p) in gm.primitives.iter().enumerate() {
            let mat = p.material.and_then(|m| model.materials.get(m));
            let u = Uniforms {
                mvp: mvp.to_cols_array_2d(),
                mv: mv.to_cols_array_2d(),
                nm: nm.to_cols_array_2d(),
                base: mat.map(|m| m.base_color).unwrap_or([1.0; 4]),
                flags: [if mat.and_then(|m| m.texture).is_some() { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
            };
            let at = i * U_STRIDE as usize;
            if let Some(dst) = data.get_mut(at..at + std::mem::size_of::<Uniforms>()) {
                dst.copy_from_slice(bytemuck::bytes_of(&u));
            }
        }
        self.queue.write_buffer(&gm.uniforms, 0, &data);
        let _ = Mat4::IDENTITY;
        let _ = Vec4::ONE;
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("deckrt model") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("deckrt model"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: background[0], g: background[1], b: background[2], a: background[3] }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            for (i, p) in gm.primitives.iter().enumerate() {
                if !model.node_visible(p.node, hidden) {
                    continue;
                }
                let tex =
                    p.material.and_then(|m| model.materials.get(m)).and_then(|m| m.texture).and_then(|t| gm.textures.get(t)).unwrap_or(&self.white);
                pass.set_bind_group(0, &gm.uniform_group, &[(i as u64 * U_STRIDE) as u32]);
                pass.set_bind_group(1, tex, &[]);
                pass.set_vertex_buffer(0, p.vertices.slice(..));
                pass.set_index_buffer(p.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..p.count, 0, 0..1);
            }
        }
        self.queue.submit(Some(encoder.finish()));
        let t = self.targets.get(&slot)?;
        Some(Rendered { view: &t.view, texture: &t.color, recreated, size: t.size })
    }

    /// Read the pixels of `slot` back (tests and screenshots): RGBA8, sRGB-encoded.
    pub fn read_back(&self, slot: u64) -> Option<crate::soft::Image> {
        let t = self.targets.get(&slot)?;
        let (w, h) = t.size;
        let row = (w * 4).div_ceil(256) * 256;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("deckrt readback"),
            size: row as u64 * h as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &t.color, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) },
            },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit(Some(enc.finish()));
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
            rgba.extend_from_slice(data.get(y * row as usize..y * row as usize + (w * 4) as usize).unwrap_or(&[]));
        }
        drop(data);
        buf.unmap();
        Some(crate::soft::Image { width: w, height: h, rgba })
    }
}

/// A device and queue on whatever adapter wgpu finds, for tests and tools.
pub fn headless_device() -> Option<(Arc<wgpu::Device>, Arc<wgpu::Queue>, String)> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor { backends: wgpu::Backends::all(), ..wgpu::InstanceDescriptor::new_without_display_handle() });
    let adapter = pollster_block(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
        apply_limit_buckets: false,
    }))
    .ok()?;
    let name = adapter.get_info().name;
    let (device, queue) =
        pollster_block(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("deckrt headless"), ..Default::default() })).ok()?;
    Some((Arc::new(device), Arc::new(queue), name))
}

/// Poll without sleeping, for when the condition variable is poisoned.
fn pollster_spin<F: std::future::Future>(mut f: std::pin::Pin<&mut F>) -> F::Output {
    use std::task::{Context, Poll, Waker};
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Run a future to completion on this thread (a tiny executor, so no pollster dependency).
pub fn pollster_block<F: std::future::Future>(f: F) -> F::Output {
    use std::sync::{Condvar, Mutex};
    use std::task::{Context, Poll, Wake, Waker};
    struct Flag(Mutex<bool>, Condvar);
    impl Wake for Flag {
        fn wake(self: Arc<Self>) {
            if let Ok(mut g) = self.0.lock() {
                *g = true;
            }
            self.1.notify_one();
        }
    }
    let flag = Arc::new(Flag(Mutex::new(false), Condvar::new()));
    let waker = Waker::from(flag.clone());
    let mut cx = Context::from_waker(&waker);
    let mut f = std::pin::pin!(f);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
        if let Ok(mut g) = flag.0.lock() {
            while !*g {
                match flag.1.wait(g) {
                    Ok(next) => g = next,
                    Err(_) => return pollster_spin(f),
                }
            }
            *g = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gpu_draws_the_cube_like_the_software_path() {
        let Some((device, queue, name)) = headless_device() else {
            eprintln!("no GPU adapter here; skipped");
            return;
        };
        eprintln!("adapter: {name}");
        let model = Arc::new(crate::model::load(&crate::model::tests::cube_glb()).expect("loads"));
        let mut r = Renderer::new(device, queue);
        r.load(1, model.clone());
        let v = View::framing(&model);
        let out = r.render(7, 1, &v, 120, 90, &[], [0.0, 0.0, 0.0, 0.0]).expect("renders");
        assert!(out.recreated);
        let img = r.read_back(7).expect("reads back");
        let px = |x: u32, y: u32| {
            let i = ((y * 120 + x) * 4) as usize;
            [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2], img.rgba[i + 3]]
        };
        assert_eq!(px(0, 0)[3], 0);
        let c = px(60, 50);
        assert_eq!(c[3], 255);
        assert!(c[0] > c[1] && c[0] > c[2], "{c:?}");
        let soft = crate::soft::render(&model, &v, 120, 90, &[], None);
        let s = {
            let i = ((50 * 120 + 60) * 4) as usize;
            [soft.rgba[i], soft.rgba[i + 1], soft.rgba[i + 2]]
        };
        assert!((c[0] as i32 - s[0] as i32).abs() < 40, "gpu {c:?} vs soft {s:?}");
    }
}
