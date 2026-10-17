//! Model files other than `.glb`, turned into `.glb` when they are inserted.
//!
//! A deck keeps every 3D model as glTF binary, since that is the only kind PowerPoint's own 3-D
//! model part carries. `.obj` (with its `.mtl` colours and textures) and `.stl` are converted
//! here; `.fbx` is refused with a line saying how to export it.

use std::collections::HashMap;

/// File extensions the model pickers offer.
pub const MODEL_EXTS: [&str; 3] = ["glb", "obj", "stl"];

/// Above this many triangles a file is refused rather than converted.
const MAX_TRIANGLES: usize = 5_000_000;

/// `bytes` as a `.glb`. `name` is the file name, which picks the format. `sibling` reads a file
/// named relative to the model's folder (an `.obj`'s `.mtl` and textures) and returns `None` when
/// it is missing.
pub fn to_glb(name: &str, bytes: &[u8], sibling: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Vec<u8>, String> {
    if bytes.starts_with(b"glTF") {
        return Ok(bytes.to_vec());
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let stem = stem_of(name);
    match ext.as_str() {
        "obj" => obj_to_glb(&stem, bytes, sibling),
        "stl" => stl_to_glb(&stem, bytes),
        "fbx" => Err(format!("{name} is an FBX file, which deckrt can't open. In Blender, import it and use File › Export › glTF 2.0 (.glb).")),
        "gltf" => Err(format!("{name} is a text glTF file. Export it as binary glTF (.glb) and insert that.")),
        "glb" => Err(format!("{name} is not a valid .glb file.")),
        _ => Err(format!("{name} isn't a 3D model deckrt can open. Use a .glb, .obj or .stl file.")),
    }
}

/// The name the converted model gets in the deck: the same stem with `.glb`.
pub fn glb_name(name: &str) -> String {
    if name.to_ascii_lowercase().ends_with(".glb") { name.to_string() } else { format!("{}.glb", stem_of(name)) }
}

fn stem_of(name: &str) -> String {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    match file.rsplit_once('.') {
        Some((s, _)) if !s.is_empty() => s.to_string(),
        _ => file.to_string(),
    }
}

// ---------------------------------------------------------------------------------------------
// Output

/// One primitive: triangles with one material.
#[derive(Default)]
struct Prim {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    has_uv: bool,
    idx: Vec<u32>,
    material: usize,
}

struct Node {
    name: String,
    prims: Vec<Prim>,
}

struct Mat {
    name: String,
    color: [f32; 4],
    texture: Option<(Vec<u8>, &'static str)>,
}

fn pad4(bin: &mut Vec<u8>) {
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
}

fn write_glb(nodes: &[Node], mats: &[Mat]) -> Result<Vec<u8>, String> {
    use serde_json::{Value, json};
    let mut bin: Vec<u8> = vec![];
    let mut views: Vec<Value> = vec![];
    let mut accessors: Vec<Value> = vec![];
    let mut view = |bin: &mut Vec<u8>, data: &[u8], target: Option<u32>| -> usize {
        pad4(bin);
        let off = bin.len();
        bin.extend_from_slice(data);
        let mut v = json!({"buffer": 0, "byteOffset": off, "byteLength": data.len()});
        if let Some(t) = target {
            v["target"] = json!(t);
        }
        views.push(v);
        views.len() - 1
    };
    let mut meshes = vec![];
    let mut json_nodes = vec![];
    for node in nodes {
        let mut prims = vec![];
        for p in &node.prims {
            if p.idx.is_empty() {
                continue;
            }
            let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
            let mut data = Vec::with_capacity(p.pos.len() * 12);
            for v in &p.pos {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                    data.extend_from_slice(&v[k].to_le_bytes());
                }
            }
            let pv = view(&mut bin, &data, Some(34962));
            accessors.push(json!({"bufferView": pv, "componentType": 5126, "count": p.pos.len(), "type": "VEC3", "min": lo, "max": hi}));
            let pa = accessors.len() - 1;
            let data: Vec<u8> = p.nrm.iter().flat_map(|n| n.iter().flat_map(|f| f.to_le_bytes())).collect();
            let nv = view(&mut bin, &data, Some(34962));
            accessors.push(json!({"bufferView": nv, "componentType": 5126, "count": p.nrm.len(), "type": "VEC3"}));
            let na = accessors.len() - 1;
            let mut attributes = json!({"POSITION": pa, "NORMAL": na});
            if p.has_uv {
                let data: Vec<u8> = p.uv.iter().flat_map(|n| n.iter().flat_map(|f| f.to_le_bytes())).collect();
                let tv = view(&mut bin, &data, Some(34962));
                accessors.push(json!({"bufferView": tv, "componentType": 5126, "count": p.uv.len(), "type": "VEC2"}));
                attributes["TEXCOORD_0"] = json!(accessors.len() - 1);
            }
            let data: Vec<u8> = p.idx.iter().flat_map(|i| i.to_le_bytes()).collect();
            let iv = view(&mut bin, &data, Some(34963));
            accessors.push(json!({"bufferView": iv, "componentType": 5125, "count": p.idx.len(), "type": "SCALAR"}));
            prims.push(json!({"attributes": attributes, "indices": accessors.len() - 1, "material": p.material}));
        }
        if prims.is_empty() {
            continue;
        }
        meshes.push(json!({"name": node.name, "primitives": prims}));
        json_nodes.push(json!({"name": node.name, "mesh": meshes.len() - 1}));
    }
    if json_nodes.is_empty() {
        return Err("the file has no faces to draw".into());
    }
    let mut images = vec![];
    let mut textures = vec![];
    let mut materials = vec![];
    for m in mats {
        let mut pbr = json!({"baseColorFactor": m.color, "metallicFactor": 0.0, "roughnessFactor": 0.8});
        if let Some((bytes, mime)) = &m.texture {
            let v = view(&mut bin, bytes, None);
            images.push(json!({"bufferView": v, "mimeType": mime}));
            textures.push(json!({"source": images.len() - 1}));
            pbr["baseColorTexture"] = json!({"index": textures.len() - 1});
        }
        let mut mat = json!({"name": m.name, "pbrMetallicRoughness": pbr, "doubleSided": true});
        if m.color[3] < 1.0 {
            mat["alphaMode"] = json!("BLEND");
        }
        materials.push(mat);
    }
    pad4(&mut bin);
    let roots: Vec<usize> = (0..json_nodes.len()).collect();
    let mut doc = json!({
        "asset": {"version": "2.0", "generator": "deckrt"},
        "scene": 0,
        "scenes": [{"nodes": roots}],
        "nodes": json_nodes,
        "meshes": meshes,
        "materials": materials,
        "buffers": [{"byteLength": bin.len()}],
        "bufferViews": views,
        "accessors": accessors,
    });
    if !images.is_empty() {
        doc["images"] = json!(images);
        doc["textures"] = json!(textures);
    }
    Ok(crate::model::sample::build_glb(&doc, &bin))
}

/// A texture as PNG or JPEG bytes, the two kinds glTF allows.
fn texture_bytes(bytes: Vec<u8>) -> Option<(Vec<u8>, &'static str)> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some((bytes, "image/png"));
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        return Some((bytes, "image/jpeg"));
    }
    let img = image::load_from_memory(&bytes).ok()?;
    let mut out = std::io::Cursor::new(vec![]);
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some((out.into_inner(), "image/png"))
}

fn face_normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
}

fn normalize(n: [f32; 3]) -> [f32; 3] {
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if l > 1e-20 && l.is_finite() { [n[0] / l, n[1] / l, n[2] / l] } else { [0.0, 1.0, 0.0] }
}

// ---------------------------------------------------------------------------------------------
// OBJ

/// One corner of a face: position, texture and normal indices (0-based; texture and normal optional).
type Corner = (usize, Option<usize>, Option<usize>);

fn parse_index(tok: &str, count: usize) -> Option<usize> {
    let i: i64 = tok.parse().ok()?;
    let i = if i < 0 { count as i64 + i } else { i - 1 };
    (0..count as i64).contains(&i).then_some(i as usize)
}

fn parse_corner(tok: &str, np: usize, nt: usize, nn: usize) -> Option<Corner> {
    let mut it = tok.split('/');
    let v = parse_index(it.next()?, np)?;
    let t = it.next().filter(|s| !s.is_empty()).and_then(|s| parse_index(s, nt));
    let n = it.next().filter(|s| !s.is_empty()).and_then(|s| parse_index(s, nn));
    Some((v, t, n))
}

fn floats<const N: usize>(rest: &str) -> Option<[f32; N]> {
    let mut out = [0f32; N];
    let mut it = rest.split_whitespace();
    for o in out.iter_mut() {
        *o = it.next()?.parse().ok()?;
    }
    Some(out)
}

/// Join a path named inside a file with the folder of that file (both `/`-separated or `\`).
fn relative(base: &str, name: &str) -> String {
    let name = name.trim().trim_matches('"');
    match base.rfind(['/', '\\']) {
        Some(i) => format!("{}/{}", &base[..i], name),
        None => name.to_string(),
    }
}

fn parse_mtl(text: &str, mtl_path: &str, sibling: &dyn Fn(&str) -> Option<Vec<u8>>, mats: &mut Vec<Mat>, by_name: &mut HashMap<String, usize>) {
    let mut cur: Option<usize> = None;
    for line in text.lines() {
        let line = line.trim();
        let (key, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = rest.trim();
        match key {
            "newmtl" => {
                let name = rest.to_string();
                let i = *by_name.entry(name.clone()).or_insert_with(|| {
                    mats.push(Mat { name, color: [0.8, 0.8, 0.8, 1.0], texture: None });
                    mats.len() - 1
                });
                cur = Some(i);
            }
            "Kd" => {
                if let (Some(i), Some(c)) = (cur, floats::<3>(rest)) {
                    mats[i].color[..3].copy_from_slice(&c);
                }
            }
            "d" => {
                if let (Some(i), Some([d])) = (cur, floats::<1>(rest)) {
                    mats[i].color[3] = d.clamp(0.0, 1.0);
                }
            }
            "Tr" => {
                if let (Some(i), Some([t])) = (cur, floats::<1>(rest)) {
                    mats[i].color[3] = (1.0 - t).clamp(0.0, 1.0);
                }
            }
            "map_Kd" => {
                let Some(i) = cur else { continue };
                // Options (`-s 1 1 1`, `-bm 0.5`) come first; the file name is what follows them.
                // Try the whole rest first, then the last word, for names with spaces.
                let last = rest.split_whitespace().last().unwrap_or("");
                let found = [rest, last].iter().find_map(|n| sibling(&relative(mtl_path, n)));
                if let Some(bytes) = found {
                    mats[i].texture = texture_bytes(bytes);
                    if mats[i].texture.is_some() {
                        // A texture is multiplied by Kd; most exporters leave Kd at grey.
                        mats[i].color[..3].copy_from_slice(&[1.0, 1.0, 1.0]);
                    }
                }
            }
            _ => {}
        }
    }
}

fn obj_to_glb(stem: &str, bytes: &[u8], sibling: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Vec<u8>, String> {
    let text = String::from_utf8_lossy(bytes);
    let mut pos: Vec<[f32; 3]> = vec![];
    let mut uvs: Vec<[f32; 2]> = vec![];
    let mut nrms: Vec<[f32; 3]> = vec![];
    let mut mats: Vec<Mat> = vec![];
    let mut mat_by_name: HashMap<String, usize> = HashMap::new();
    // Faces grouped by (group, material), groups in the order they first appear.
    let mut groups: Vec<String> = vec![];
    let mut group_ix: HashMap<String, usize> = HashMap::new();
    let mut faces: Vec<Vec<(usize, Vec<[Corner; 3]>)>> = vec![];
    let mut cur_group: Option<usize> = None;
    let mut cur_mat_name = String::new();
    let mut triangles = 0usize;

    let mut group_named = |name: &str, groups: &mut Vec<String>, faces: &mut Vec<Vec<(usize, Vec<[Corner; 3]>)>>| -> usize {
        *group_ix.entry(name.to_string()).or_insert_with(|| {
            groups.push(name.to_string());
            faces.push(vec![]);
            groups.len() - 1
        })
    };

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, rest) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let rest = rest.trim();
        match key {
            "v" => pos.push(floats::<3>(rest).ok_or("a vertex line can't be read")?),
            "vt" => {
                let mut it = rest.split_whitespace().map(|s| s.parse::<f32>().unwrap_or(0.0));
                let u = it.next().unwrap_or(0.0);
                let v = it.next().unwrap_or(0.0);
                uvs.push([u, 1.0 - v]);
            }
            "vn" => nrms.push(floats::<3>(rest).unwrap_or([0.0, 1.0, 0.0])),
            "o" | "g" => {
                let name = if rest.is_empty() { stem.to_string() } else { rest.to_string() };
                cur_group = Some(group_named(&name, &mut groups, &mut faces));
            }
            "usemtl" => cur_mat_name = rest.to_string(),
            "mtllib" => {
                // Several files may be named; names with spaces are rare, so try the whole rest first.
                let names: Vec<&str> = std::iter::once(rest).chain(rest.split_whitespace()).collect();
                for n in names {
                    if let Some(b) = sibling(n) {
                        parse_mtl(&String::from_utf8_lossy(&b), n, sibling, &mut mats, &mut mat_by_name);
                        break;
                    }
                }
            }
            "f" => {
                let corners: Vec<Corner> =
                    rest.split_whitespace().filter_map(|t| parse_corner(t, pos.len(), uvs.len(), nrms.len())).collect();
                if corners.len() < 3 {
                    continue;
                }
                let g = match cur_group {
                    Some(g) => g,
                    None => {
                        let g = group_named(stem, &mut groups, &mut faces);
                        cur_group = Some(g);
                        g
                    }
                };
                let m = *mat_by_name.entry(cur_mat_name.clone()).or_insert_with(|| {
                    let name = if cur_mat_name.is_empty() { "default".to_string() } else { cur_mat_name.clone() };
                    mats.push(Mat { name, color: [0.8, 0.8, 0.8, 1.0], texture: None });
                    mats.len() - 1
                });
                let list = &mut faces[g];
                let slot = match list.iter().position(|(mm, _)| *mm == m) {
                    Some(i) => i,
                    None => {
                        list.push((m, vec![]));
                        list.len() - 1
                    }
                };
                for k in 1..corners.len() - 1 {
                    list[slot].1.push([corners[0], corners[k], corners[k + 1]]);
                }
                triangles += corners.len() - 2;
                if triangles > MAX_TRIANGLES {
                    return Err(format!("the model has more than {MAX_TRIANGLES} triangles"));
                }
            }
            _ => {}
        }
    }
    if triangles == 0 {
        return Err("the file has no faces to draw".into());
    }

    // Smooth normals per position, for corners the file gave no normal.
    let mut smooth = vec![[0f32; 3]; pos.len()];
    let needs_smooth = faces.iter().flatten().flat_map(|(_, t)| t).any(|tri| tri.iter().any(|c| c.2.is_none()));
    if needs_smooth {
        for tri in faces.iter().flatten().flat_map(|(_, t)| t) {
            let n = face_normal(pos[tri[0].0], pos[tri[1].0], pos[tri[2].0]);
            for c in tri {
                for k in 0..3 {
                    smooth[c.0][k] += n[k];
                }
            }
        }
        for n in smooth.iter_mut() {
            *n = normalize(*n);
        }
    }

    let mut nodes = vec![];
    for (g, name) in groups.iter().enumerate() {
        let mut prims = vec![];
        for (m, tris) in &faces[g] {
            let mut p = Prim { material: *m, ..Default::default() };
            p.has_uv = tris.iter().any(|t| t.iter().any(|c| c.1.is_some()));
            let mut seen: HashMap<Corner, u32> = HashMap::new();
            for tri in tris {
                for c in tri {
                    let i = *seen.entry(*c).or_insert_with(|| {
                        p.pos.push(pos[c.0]);
                        p.nrm.push(match c.2 {
                            Some(n) => normalize(nrms[n]),
                            None => smooth[c.0],
                        });
                        p.uv.push(c.1.map(|t| uvs[t]).unwrap_or([0.0, 0.0]));
                        (p.pos.len() - 1) as u32
                    });
                    p.idx.push(i);
                }
            }
            prims.push(p);
        }
        nodes.push(Node { name: name.clone(), prims });
    }
    write_glb(&nodes, &mats)
}

// ---------------------------------------------------------------------------------------------
// STL

fn stl_to_glb(stem: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut tris: Vec<[[f32; 3]; 3]> = vec![];
    let binary_len = (bytes.len() >= 84).then(|| u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize);
    let is_binary = binary_len.is_some_and(|n| 84 + n.saturating_mul(50) == bytes.len());
    if is_binary {
        let n = binary_len.unwrap_or(0);
        if n > MAX_TRIANGLES {
            return Err(format!("the model has more than {MAX_TRIANGLES} triangles"));
        }
        for t in 0..n {
            let base = 84 + t * 50 + 12;
            let mut tri = [[0f32; 3]; 3];
            for (v, corner) in tri.iter_mut().enumerate() {
                for (k, c) in corner.iter_mut().enumerate() {
                    let o = base + v * 12 + k * 4;
                    *c = f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
                }
            }
            tris.push(tri);
        }
    } else {
        let text = String::from_utf8_lossy(bytes);
        let mut corner = vec![];
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("vertex") {
                corner.push(floats::<3>(rest).ok_or("a vertex line can't be read")?);
                if corner.len() == 3 {
                    tris.push([corner[0], corner[1], corner[2]]);
                    corner.clear();
                    if tris.len() > MAX_TRIANGLES {
                        return Err(format!("the model has more than {MAX_TRIANGLES} triangles"));
                    }
                }
            }
        }
    }
    if tris.is_empty() {
        return Err("the file has no faces to draw".into());
    }
    // STL files are almost always Z-up; glTF is Y-up.
    let up = |v: [f32; 3]| [v[0], v[2], -v[1]];
    let mut p = Prim::default();
    for t in &tris {
        let [a, b, c] = [up(t[0]), up(t[1]), up(t[2])];
        let n = normalize(face_normal(a, b, c));
        for v in [a, b, c] {
            p.pos.push(v);
            p.nrm.push(n);
            p.uv.push([0.0, 0.0]);
            p.idx.push((p.pos.len() - 1) as u32);
        }
    }
    let mats = [Mat { name: "default".into(), color: [0.8, 0.8, 0.8, 1.0], texture: None }];
    write_glb(&[Node { name: stem.to_string(), prims: vec![p] }], &mats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<Vec<u8>> {
        None
    }

    #[test]
    fn an_obj_with_groups_and_quads_becomes_a_glb_with_layers() {
        let obj = "mtllib m.mtl\no skin\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nusemtl red\nf 1 2 3 4\no bone\nv 0 0 1\nv 1 0 1\nv 1 1 1\nf -3 -2 -1\n";
        let mtl = "newmtl red\nKd 1 0 0\n";
        let sib = |n: &str| (n == "m.mtl").then(|| mtl.as_bytes().to_vec());
        let glb = to_glb("head.obj", obj.as_bytes(), &sib).expect("converts");
        let model = crate::load(&glb).expect("loads");
        assert_eq!(model.layer_names(), vec!["skin".to_string(), "bone".to_string()]);
        assert!(model.radius() > 0.5);
    }

    #[test]
    fn an_obj_without_its_mtl_still_converts() {
        let obj = "mtllib missing.mtl\nusemtl defaultMat\nv 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvt 1 0\nvt 0 1\nf 1/1 2/2 3/3\n";
        let glb = to_glb("Head.obj", obj.as_bytes(), &none).expect("converts");
        assert!(crate::load(&glb).is_ok());
    }

    #[test]
    fn ascii_and_binary_stl_convert() {
        let ascii = "solid t\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nvertex 1 0 0\nvertex 0 1 0\nendloop\nendfacet\nendsolid t\n";
        assert!(crate::load(&to_glb("t.stl", ascii.as_bytes(), &none).expect("ascii")).is_ok());
        let mut bin = vec![0u8; 80];
        bin.extend_from_slice(&1u32.to_le_bytes());
        bin.extend_from_slice(&[0u8; 12]);
        for v in [[0f32, 0., 0.], [1., 0., 0.], [0., 1., 0.]] {
            for f in v {
                bin.extend_from_slice(&f.to_le_bytes());
            }
        }
        bin.extend_from_slice(&[0, 0]);
        assert!(crate::load(&to_glb("t.stl", &bin, &none).expect("binary")).is_ok());
    }

    #[test]
    fn fbx_and_unknown_files_are_refused_with_a_reason() {
        assert!(to_glb("a.fbx", b"Kaydara FBX Binary", &none).unwrap_err().contains("Blender"));
        assert!(to_glb("a.txt", b"hello", &none).unwrap_err().contains(".obj"));
        assert_eq!(glb_name("C:\\x\\Head.obj"), "Head.glb");
    }
}
