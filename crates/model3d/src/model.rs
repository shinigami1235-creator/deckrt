//! A `.glb` loaded into flat triangle lists in model space, with its materials, textures and
//! node names.

use glam::{Mat4, Quat, Vec3};

/// Triangle limit per model: a scan far beyond this would not turn smoothly anyway.
pub const MAX_TRIANGLES: usize = 6_000_000;
const MAX_TEXTURE_SIDE: u32 = 8192;

#[derive(Clone, Debug)]
pub struct Primitive {
    /// Positions in model space (node transforms applied).
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Vertex colours, when the file has them (RGBA, 0 to 1).
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    pub material: Option<usize>,
    /// The node the primitive belongs to, for layer visibility.
    pub node: usize,
}

#[derive(Clone, Debug)]
pub struct Material {
    pub base_color: [f32; 4],
    pub texture: Option<usize>,
    pub double_sided: bool,
    pub metallic: f32,
    pub roughness: f32,
}

#[derive(Clone, Debug)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    /// RGBA8, row-major.
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct Model {
    pub primitives: Vec<Primitive>,
    pub materials: Vec<Material>,
    pub textures: Vec<Texture>,
    pub nodes: Vec<Node>,
    pub min: Vec3,
    pub max: Vec3,
    pub triangles: usize,
}

impl Model {
    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    /// Half the bounding box diagonal.
    pub fn radius(&self) -> f32 {
        ((self.max - self.min).length() * 0.5).max(1e-6)
    }

    /// Whether a node is shown given the hidden layer names: hidden when it or any ancestor is
    /// named in `hidden`.
    pub fn node_visible(&self, node: usize, hidden: &[String]) -> bool {
        if hidden.is_empty() {
            return true;
        }
        let mut cur = Some(node);
        let mut depth = 0;
        while let Some(i) = cur {
            let Some(n) = self.nodes.get(i) else { return true };
            if hidden.contains(&n.name) {
                return false;
            }
            cur = n.parent;
            depth += 1;
            if depth > 256 {
                break;
            }
        }
        true
    }

    /// The node names, each once, in file order (the layer list).
    pub fn layer_names(&self) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for n in &self.nodes {
            if !out.contains(&n.name) {
                out.push(n.name.clone());
            }
        }
        out
    }
}

fn node_local(n: &gltf::Node) -> Mat4 {
    match n.transform() {
        gltf::scene::Transform::Matrix { matrix } => Mat4::from_cols_array_2d(&matrix),
        gltf::scene::Transform::Decomposed { translation, rotation, scale } => {
            Mat4::from_scale_rotation_translation(Vec3::from(scale), Quat::from_array(rotation), Vec3::from(translation))
        }
    }
}

/// Load a `.glb`. Buffers must be embedded (the binary chunk); textures are decoded with `image`.
pub fn load(bytes: &[u8]) -> Result<Model, String> {
    let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| format!("the model file could not be read: {e}"))?;
    let blob = gltf.blob.clone().unwrap_or_default();
    let doc = &gltf.document;
    let buffer_data = |b: gltf::Buffer| -> Option<&[u8]> {
        match b.source() {
            gltf::buffer::Source::Bin => Some(&blob[..b.length().min(blob.len())]),
            gltf::buffer::Source::Uri(_) => None,
        }
    };
    let buffers: Vec<Option<&[u8]>> = doc.buffers().map(buffer_data).collect();
    let get_buffer = |b: gltf::Buffer| -> Option<&[u8]> { buffers.get(b.index()).copied().flatten() };

    // Nodes and their world transforms.
    let n_nodes = doc.nodes().count();
    let mut nodes: Vec<Node> = doc
        .nodes()
        .enumerate()
        .map(|(i, n)| Node { name: n.name().map(String::from).unwrap_or_else(|| format!("node_{i}")), parent: None })
        .collect();
    let mut world = vec![Mat4::IDENTITY; n_nodes];
    let mut stack: Vec<(usize, Mat4)> = vec![];
    for scene in doc.scenes() {
        for n in scene.nodes() {
            stack.push((n.index(), Mat4::IDENTITY));
        }
    }
    if stack.is_empty() {
        // No scene list: every root node.
        let has_parent: std::collections::HashSet<usize> = doc.nodes().flat_map(|n| n.children().map(|c| c.index())).collect();
        for n in doc.nodes() {
            if !has_parent.contains(&n.index()) {
                stack.push((n.index(), Mat4::IDENTITY));
            }
        }
    }
    let mut visited = vec![false; n_nodes];
    while let Some((i, parent)) = stack.pop() {
        let Some(n) = doc.nodes().nth(i) else { continue };
        if visited.get(i).copied().unwrap_or(true) {
            continue;
        }
        visited[i] = true;
        let m = parent * node_local(&n);
        world[i] = m;
        for c in n.children() {
            if let Some(cn) = nodes.get_mut(c.index()) {
                cn.parent = Some(i);
            }
            stack.push((c.index(), m));
        }
    }

    // Textures.
    let mut textures = vec![];
    let mut texture_index: Vec<Option<usize>> = vec![];
    for img in doc.images() {
        let data: Option<Vec<u8>> = match img.source() {
            gltf::image::Source::View { view, .. } => {
                get_buffer(view.buffer()).and_then(|b| b.get(view.offset()..view.offset() + view.length())).map(|s| s.to_vec())
            }
            gltf::image::Source::Uri { .. } => None,
        };
        let decoded = data.and_then(|d| image::load_from_memory(&d).ok()).filter(|i| i.width() <= MAX_TEXTURE_SIDE && i.height() <= MAX_TEXTURE_SIDE);
        match decoded {
            Some(i) => {
                let rgba = i.to_rgba8();
                textures.push(Texture { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() });
                texture_index.push(Some(textures.len() - 1));
            }
            None => {
                log::warn!("model: image {} could not be decoded", img.index());
                texture_index.push(None);
            }
        }
    }

    let materials: Vec<Material> = doc
        .materials()
        .map(|m| {
            let pbr = m.pbr_metallic_roughness();
            let texture = pbr.base_color_texture().and_then(|t| texture_index.get(t.texture().source().index()).copied().flatten());
            Material {
                base_color: pbr.base_color_factor(),
                texture,
                double_sided: m.double_sided(),
                metallic: pbr.metallic_factor(),
                roughness: pbr.roughness_factor(),
            }
        })
        .collect();

    let mut primitives = vec![];
    let mut min = Vec3::splat(f32::MAX);
    let mut max = Vec3::splat(f32::MIN);
    let mut triangles = 0usize;
    for n in doc.nodes() {
        let Some(mesh) = n.mesh() else { continue };
        let m = world.get(n.index()).copied().unwrap_or(Mat4::IDENTITY);
        let normal_m = m.inverse().transpose();
        for p in mesh.primitives() {
            if p.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = p.reader(get_buffer);
            let Some(pos) = reader.read_positions() else { continue };
            let positions: Vec<[f32; 3]> = pos
                .map(|v| {
                    let w = m.transform_point3(Vec3::from(v));
                    min = min.min(w);
                    max = max.max(w);
                    w.to_array()
                })
                .collect();
            let indices: Vec<u32> = match reader.read_indices() {
                Some(ix) => ix.into_u32().filter(|i| (*i as usize) < positions.len()).collect(),
                None => (0..positions.len() as u32).collect(),
            };
            let indices: Vec<u32> = indices[..indices.len() / 3 * 3].to_vec();
            let normals: Vec<[f32; 3]> = match reader.read_normals() {
                Some(ns) => ns.map(|v| normal_m.transform_vector3(Vec3::from(v)).normalize_or_zero().to_array()).collect(),
                None => flat_normals(&positions, &indices),
            };
            let uvs: Vec<[f32; 2]> = reader.read_tex_coords(0).map(|t| t.into_f32().collect()).unwrap_or_default();
            let colors: Vec<[f32; 4]> = reader.read_colors(0).map(|c| c.into_rgba_f32().collect()).unwrap_or_default();
            triangles += indices.len() / 3;
            if triangles > MAX_TRIANGLES {
                return Err(format!("the model has more than {MAX_TRIANGLES} triangles; reduce it before placing it"));
            }
            primitives.push(Primitive { positions, normals, uvs, colors, indices, material: p.material().index(), node: n.index() });
        }
    }
    if primitives.is_empty() {
        return Err("the model has no triangles to draw".into());
    }
    Ok(Model { primitives, materials, textures, nodes, min, max, triangles })
}

fn flat_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut n = vec![Vec3::ZERO; positions.len()];
    for t in indices.chunks_exact(3) {
        let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
        let (Some(pa), Some(pb), Some(pc)) = (positions.get(a), positions.get(b), positions.get(c)) else { continue };
        let fnrm = (Vec3::from(*pb) - Vec3::from(*pa)).cross(Vec3::from(*pc) - Vec3::from(*pa));
        for i in [a, b, c] {
            if let Some(v) = n.get_mut(i) {
                *v += fnrm;
            }
        }
    }
    n.into_iter().map(|v| v.normalize_or(Vec3::Y).to_array()).collect()
}

/// Small models for tests in this crate and the ones above it.
pub mod sample {
    /// A unit cube with two named nodes (a parent "body" with the cube, a child "cap").
    pub fn cube_glb() -> Vec<u8> {
        let p: [[f32; 3]; 8] =
            [[-1., -1., -1.], [1., -1., -1.], [1., 1., -1.], [-1., 1., -1.], [-1., -1., 1.], [1., -1., 1.], [1., 1., 1.], [-1., 1., 1.]];
        let idx: [u16; 36] = [0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 2, 3, 7, 2, 7, 6, 1, 2, 6, 1, 6, 5, 0, 7, 3, 0, 4, 7];
        let mut bin = vec![];
        for v in p {
            for f in v {
                bin.extend_from_slice(&f.to_le_bytes());
            }
        }
        let ioff = bin.len();
        for i in idx {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        let json = serde_json::json!({
            "asset": {"version": "2.0"},
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [{"name": "body", "mesh": 0, "children": [1]}, {"name": "cap", "mesh": 0, "translation": [0.0, 3.0, 0.0], "scale": [0.5, 0.5, 0.5]}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "indices": 1, "material": 0}]}],
            "materials": [{"pbrMetallicRoughness": {"baseColorFactor": [0.8, 0.2, 0.2, 1.0]}}],
            "buffers": [{"byteLength": bin.len()}],
            "bufferViews": [{"buffer": 0, "byteOffset": 0, "byteLength": ioff}, {"buffer": 0, "byteOffset": ioff, "byteLength": 72}],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": 8, "type": "VEC3", "min": [-1, -1, -1], "max": [1, 1, 1]},
                {"bufferView": 1, "componentType": 5123, "count": 36, "type": "SCALAR"}
            ]
        });
        build_glb(&json, &bin)
    }

    /// A `.glb` from a glTF JSON document and one binary buffer.
    pub fn build_glb(json: &serde_json::Value, bin: &[u8]) -> Vec<u8> {
        let mut json_bytes = serde_json::to_vec(json).unwrap_or_default();
        while !json_bytes.len().is_multiple_of(4) {
            json_bytes.push(b' ');
        }
        let mut bin_bytes = bin.to_vec();
        while !bin_bytes.len().is_multiple_of(4) {
            bin_bytes.push(0);
        }
        let total = 12 + 8 + json_bytes.len() + 8 + bin_bytes.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
        out.extend_from_slice(&json_bytes);
        out.extend_from_slice(&(bin_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x004E_4942u32.to_le_bytes());
        out.extend_from_slice(&bin_bytes);
        out
    }
}

#[cfg(test)]
pub(crate) mod tests {
    pub(crate) use super::sample::cube_glb;
    use super::*;

    #[test]
    fn a_cube_loads_with_its_nodes_bounds_and_normals() {
        let m = load(&cube_glb()).expect("loads");
        assert_eq!(m.primitives.len(), 2);
        assert_eq!(m.triangles, 24);
        assert_eq!(m.layer_names(), vec!["body", "cap"]);
        assert_eq!(m.nodes[1].parent, Some(0));
        assert_eq!(m.min, Vec3::new(-1.0, -1.0, -1.0));
        assert_eq!(m.max, Vec3::new(1.0, 3.5, 1.0));
        assert!(m.node_visible(1, &[]));
        assert!(!m.node_visible(1, &["body".into()]), "a child of a hidden node is hidden");
        assert!(m.node_visible(0, &["cap".into()]));
        assert_eq!(m.materials[0].base_color, [0.8, 0.2, 0.2, 1.0]);
        let n = m.primitives[0].normals[0];
        assert!((Vec3::from(n).length() - 1.0).abs() < 1e-5);
    }

    #[test]
    fn garbage_is_refused_with_a_message() {
        assert!(load(b"not a model").unwrap_err().contains("could not be read"));
    }
}
