//! A software rasteriser: the poster picture of a model, thumbnails, PDF export and any machine
//! without a usable GPU. Z-buffered triangles, two lights, base-colour textures.

use glam::{Vec2, Vec3, Vec4, Vec4Swizzles};

use crate::camera::View;
use crate::model::{Model, Texture};

/// An RGBA8 picture.
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn to_png(&self) -> Option<Vec<u8>> {
        let img = image::RgbaImage::from_raw(self.width, self.height, self.rgba.clone())?;
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).ok()?;
        Some(out.into_inner())
    }
}

/// Shading shared with the GPU shader: ambient plus two directional lights, in camera space.
pub fn light(n: Vec3, view_dir: Vec3) -> f32 {
    let key = Vec3::new(0.4, 0.6, 0.7).normalize();
    let fill = Vec3::new(-0.6, 0.2, 0.5).normalize();
    let nd = if n.dot(view_dir) < 0.0 { -n } else { n };
    let d = 0.72 * nd.dot(key).max(0.0) + 0.3 * nd.dot(fill).max(0.0);
    let h = (key + view_dir).normalize();
    let s = nd.dot(h).max(0.0).powf(24.0) * 0.18;
    (0.32 + d).min(1.1) + s
}

fn sample(t: &Texture, uv: [f32; 2]) -> Vec4 {
    if t.width == 0 || t.height == 0 {
        return Vec4::ONE;
    }
    let u = uv[0].rem_euclid(1.0) * t.width as f32 - 0.5;
    let v = uv[1].rem_euclid(1.0) * t.height as f32 - 0.5;
    let (x0, y0) = (u.floor(), v.floor());
    let (fx, fy) = (u - x0, v - y0);
    let px = |x: i64, y: i64| -> Vec4 {
        let x = x.rem_euclid(t.width as i64) as usize;
        let y = y.rem_euclid(t.height as i64) as usize;
        let i = (y * t.width as usize + x) * 4;
        match t.rgba.get(i..i + 4) {
            Some(p) => Vec4::new(p[0] as f32, p[1] as f32, p[2] as f32, p[3] as f32) / 255.0,
            None => Vec4::ONE,
        }
    };
    let (x0, y0) = (x0 as i64, y0 as i64);
    let top = px(x0, y0).lerp(px(x0 + 1, y0), fx);
    let bottom = px(x0, y0 + 1).lerp(px(x0 + 1, y0 + 1), fx);
    top.lerp(bottom, fy)
}

/// Draw `model` through `view` into a `width` × `height` picture. `hidden` names the layers left
/// out; `background` is the colour behind the model, or transparent when `None`.
pub fn render(model: &Model, view: &View, width: u32, height: u32, hidden: &[String], background: Option<[u8; 4]>) -> Image {
    let (w, h) = (width.clamp(1, 8192) as usize, height.clamp(1, 8192) as usize);
    let mut rgba = vec![0u8; w * h * 4];
    if let Some(bg) = background {
        for p in rgba.chunks_exact_mut(4) {
            p.copy_from_slice(&bg);
        }
    }
    let mut depth = vec![f32::INFINITY; w * h];
    let mvp = view.mvp(model, w as f32 / h as f32);
    let mv = view.view_matrix() * view.model_matrix(model);
    let normal_m = mv.inverse().transpose();
    let srgb_to_lin = |c: f32| if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
    let lin_to_srgb = |c: f32| if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    for prim in &model.primitives {
        if !model.node_visible(prim.node, hidden) {
            continue;
        }
        let mat = prim.material.and_then(|i| model.materials.get(i));
        let base = mat.map(|m| Vec4::from(m.base_color)).unwrap_or(Vec4::ONE);
        let tex = mat.and_then(|m| m.texture).and_then(|i| model.textures.get(i));
        // Clip-space positions and camera-space normals, once per vertex.
        let clip: Vec<Vec4> = prim.positions.iter().map(|p| mvp * Vec4::new(p[0], p[1], p[2], 1.0)).collect();
        let eye_pos: Vec<Vec3> = prim.positions.iter().map(|p| (mv * Vec4::new(p[0], p[1], p[2], 1.0)).xyz()).collect();
        let nrm: Vec<Vec3> = prim.normals.iter().map(|n| normal_m.transform_vector3(Vec3::from(*n)).normalize_or(Vec3::Z)).collect();
        for t in prim.indices.chunks_exact(3) {
            let (ia, ib, ic) = (t[0] as usize, t[1] as usize, t[2] as usize);
            let (Some(ca), Some(cb), Some(cc)) = (clip.get(ia), clip.get(ib), clip.get(ic)) else { continue };
            if ca.w <= 1e-6 || cb.w <= 1e-6 || cc.w <= 1e-6 {
                continue;
            }
            let to_screen = |c: &Vec4| Vec3::new((c.x / c.w * 0.5 + 0.5) * w as f32, (0.5 - c.y / c.w * 0.5) * h as f32, c.z / c.w);
            let (sa, sb, sc) = (to_screen(ca), to_screen(cb), to_screen(cc));
            let area = (sb.x - sa.x) * (sc.y - sa.y) - (sb.y - sa.y) * (sc.x - sa.x);
            if area.abs() < 1e-9 {
                continue;
            }
            let x0 = sa.x.min(sb.x).min(sc.x).floor().max(0.0) as usize;
            let x1 = (sa.x.max(sb.x).max(sc.x).ceil().min(w as f32 - 1.0)).max(0.0) as usize;
            let y0 = sa.y.min(sb.y).min(sc.y).floor().max(0.0) as usize;
            let y1 = (sa.y.max(sb.y).max(sc.y).ceil().min(h as f32 - 1.0)).max(0.0) as usize;
            if x1 < x0 || y1 < y0 {
                continue;
            }
            let (wa, wb, wc) = (1.0 / ca.w, 1.0 / cb.w, 1.0 / cc.w);
            let inv_area = 1.0 / area;
            for y in y0..=y1 {
                let py = y as f32 + 0.5;
                for x in x0..=x1 {
                    let px = x as f32 + 0.5;
                    let mut l0 = ((sb.x - px) * (sc.y - py) - (sb.y - py) * (sc.x - px)) * inv_area;
                    let mut l1 = ((sc.x - px) * (sa.y - py) - (sc.y - py) * (sa.x - px)) * inv_area;
                    let mut l2 = 1.0 - l0 - l1;
                    if l0 < -1e-5 || l1 < -1e-5 || l2 < -1e-5 {
                        continue;
                    }
                    l0 = l0.max(0.0);
                    l1 = l1.max(0.0);
                    l2 = l2.max(0.0);
                    let z = l0 * sa.z + l1 * sb.z + l2 * sc.z;
                    let di = y * w + x;
                    if z >= depth[di] || z < -1.0 {
                        continue;
                    }
                    // Perspective-correct weights.
                    let (p0, p1, p2) = (l0 * wa, l1 * wb, l2 * wc);
                    let sum = p0 + p1 + p2;
                    if sum <= 0.0 {
                        continue;
                    }
                    let (q0, q1, q2) = (p0 / sum, p1 / sum, p2 / sum);
                    let mut color = base;
                    if let Some(t) = tex
                        && let (Some(ua), Some(ub), Some(uc)) = (prim.uvs.get(ia), prim.uvs.get(ib), prim.uvs.get(ic))
                    {
                        let uv = [q0 * ua[0] + q1 * ub[0] + q2 * uc[0], q0 * ua[1] + q1 * ub[1] + q2 * uc[1]];
                        color *= sample(t, uv);
                    }
                    if let (Some(va), Some(vb), Some(vc)) = (prim.colors.get(ia), prim.colors.get(ib), prim.colors.get(ic)) {
                        color *= Vec4::from(*va) * q0 + Vec4::from(*vb) * q1 + Vec4::from(*vc) * q2;
                    }
                    if color.w < 0.5 {
                        continue;
                    }
                    let n = (nrm.get(ia).copied().unwrap_or(Vec3::Z) * q0
                        + nrm.get(ib).copied().unwrap_or(Vec3::Z) * q1
                        + nrm.get(ic).copied().unwrap_or(Vec3::Z) * q2)
                        .normalize_or(Vec3::Z);
                    let pos = eye_pos.get(ia).copied().unwrap_or(Vec3::ZERO) * q0
                        + eye_pos.get(ib).copied().unwrap_or(Vec3::ZERO) * q1
                        + eye_pos.get(ic).copied().unwrap_or(Vec3::ZERO) * q2;
                    let view_dir = (-pos).normalize_or(Vec3::Z);
                    let lit = light(n, view_dir);
                    depth[di] = z;
                    let o = di * 4;
                    let c = |v: f32| (lin_to_srgb((srgb_to_lin(v.clamp(0.0, 1.0)) * lit).clamp(0.0, 1.0)) * 255.0).round() as u8;
                    rgba[o] = c(color.x);
                    rgba[o + 1] = c(color.y);
                    rgba[o + 2] = c(color.z);
                    rgba[o + 3] = 255;
                }
            }
        }
    }
    let _ = Vec2::ZERO;
    Image { width: w as u32, height: h as u32, rgba }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::cube_glb;

    #[test]
    fn the_cube_is_drawn_in_the_middle_and_hidden_layers_are_left_out() {
        let m = crate::model::load(&cube_glb()).expect("loads");
        let v = View::framing(&m);
        let img = render(&m, &v, 120, 90, &[], None);
        let px = |x: u32, y: u32| {
            let i = ((y * 120 + x) * 4) as usize;
            [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2], img.rgba[i + 3]]
        };
        assert_eq!(px(0, 0)[3], 0, "the corner is transparent");
        let c = px(60, 50);
        assert_eq!(c[3], 255, "the middle is covered");
        assert!(c[0] > c[1] && c[0] > c[2], "the cube is red: {c:?}");
        // The cap sits above the body: hiding it clears the top rows.
        let with_cap = render(&m, &v, 120, 90, &[], None);
        let without = render(&m, &v, 120, 90, &["cap".into()], None);
        let covered = |img: &Image| img.rgba.chunks_exact(4).filter(|p| p[3] == 255).count();
        assert!(covered(&with_cap) > covered(&without));
        assert!(img.to_png().is_some());
    }
}
