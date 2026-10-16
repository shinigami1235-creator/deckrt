//! Drawing glTF binary (`.glb`) models.
//!
//! - [`model`]: the file loaded into triangle lists, materials, textures and node names.
//! - [`camera`]: the orbit camera.
//! - [`soft`]: a software rasteriser for posters, thumbnails, PDF export and machines without a
//!   usable GPU.
//! - [`gpu`] (feature `gpu`): the wgpu renderer the editor canvas and the slide show use.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod camera;
#[cfg(feature = "gpu")]
pub mod gpu;
pub mod model;
pub mod soft;
pub use model::sample;

pub use camera::View;
pub use model::{Model, load};

/// The poster picture of a model: the whole model framed, from the front, on a transparent
/// background, as a PNG. The picture PowerPoint and the CPU renderer show in place of the mesh.
pub fn poster_png(bytes: &[u8], width: u32, height: u32, rotation: [f64; 3], hidden: &[String]) -> Result<Vec<u8>, String> {
    poster_png_with(bytes, width, height, rotation, hidden, None)
}

/// A camera pose as the objects store it: `[theta_deg, phi_deg, radius]`, the target, the fov.
pub type Pose = ([f64; 3], [f64; 3], f64);

/// [`poster_png`] from a stored camera pose instead of the framing camera.
pub fn poster_png_with(bytes: &[u8], width: u32, height: u32, rotation: [f64; 3], hidden: &[String], pose: Option<Pose>) -> Result<Vec<u8>, String> {
    let model = load(bytes)?;
    let view = view_for(&model, rotation, pose);
    let img = soft::render(&model, &view, width.clamp(16, 4096), height.clamp(16, 4096), hidden, None);
    img.to_png().ok_or_else(|| "the poster could not be encoded".to_string())
}

/// The camera a model starts from: the stored pose when there is one, else the whole model framed.
pub fn view_for(model: &Model, rotation: [f64; 3], pose: Option<Pose>) -> View {
    let mut view = View::framing(model);
    view.rotation = rotation;
    if let Some((orbit, target, fov)) = pose
        && orbit.iter().chain(target.iter()).all(|v| v.is_finite())
        && fov.is_finite()
        && orbit[2] > 0.0
    {
        view.theta_deg = orbit[0];
        view.phi_deg = orbit[1].clamp(camera::PHI_MIN_DEG, camera::PHI_MAX_DEG);
        view.radius = orbit[2];
        view.target = target;
        view.fov_deg = fov.clamp(5.0, 120.0);
    }
    view
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `DECKRT_GLB=<file> DECKRT_GLB_OUT=<png>`: draw a real model with the software path, for
    /// looking at by eye. Skipped when the variables are not set.
    #[test]
    fn draw_a_real_model_when_asked() {
        let (Some(path), Some(out)) = (std::env::var_os("DECKRT_GLB"), std::env::var_os("DECKRT_GLB_OUT")) else { return };
        let bytes = std::fs::read(&path).expect("read");
        let t = std::time::Instant::now();
        let model = load(&bytes).expect("load");
        eprintln!("loaded {} triangles, {} nodes, {} textures in {:?}", model.triangles, model.nodes.len(), model.textures.len(), t.elapsed());
        let t = std::time::Instant::now();
        let png = poster_png(&bytes, 800, 600, [0.0; 3], &[]).expect("poster");
        eprintln!("poster in {:?}", t.elapsed());
        std::fs::write(out, png).expect("write");
    }
}
