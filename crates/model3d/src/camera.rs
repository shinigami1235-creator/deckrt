//! The orbit camera and the model's own rotation.

use glam::{Mat4, Vec3};

use crate::model::Model;

/// Where the camera is and which way the model is turned.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Azimuth in degrees: 0 looks at the model's front (+Z), positive turns to the right.
    pub theta_deg: f64,
    /// Polar angle in degrees from straight above: 90 is eye level.
    pub phi_deg: f64,
    /// Distance from the target, in model units.
    pub radius: f64,
    pub target: [f64; 3],
    /// Vertical field of view in degrees.
    pub fov_deg: f64,
    /// The model's own turn, in radians, as model-viewer's `orientation` (what the earlier
    /// deckrt saved): `[roll, pitch, yaw]`, about the model's Z, X and Y axes, applied in that
    /// order. With glTF's Y up, yaw turns the model round, pitch nods it, roll tilts it.
    pub rotation: [f64; 3],
}

/// model-viewer's limits, so the camera never flips over the pole.
pub const PHI_MIN_DEG: f64 = 22.5;
pub const PHI_MAX_DEG: f64 = 157.5;

impl View {
    /// A camera that frames the whole model.
    pub fn framing(model: &Model) -> View {
        let fov: f64 = 30.0;
        let radius = (model.radius() as f64 / (fov.to_radians() / 2.0).sin()) * 1.15;
        View { theta_deg: 0.0, phi_deg: 90.0, radius, target: model.center().as_dvec3().to_array(), fov_deg: fov, rotation: [0.0; 3] }
    }

    /// The pose after a drag of `dx`, `dy` fractions of the surface (deckrt's rule: 240 degrees
    /// per width, 150 per height).
    pub fn turned(mut self, dx: f64, dy: f64) -> View {
        self.theta_deg -= dx * 240.0;
        self.phi_deg = (self.phi_deg - dy * 150.0).clamp(PHI_MIN_DEG, PHI_MAX_DEG);
        self
    }

    pub fn zoomed(mut self, factor: f64) -> View {
        if factor.is_finite() && factor > 0.0 {
            self.radius = (self.radius / factor).max(1e-6);
        }
        self
    }

    pub fn eye(&self) -> Vec3 {
        let (t, p) = (self.theta_deg.to_radians(), self.phi_deg.clamp(1.0, 179.0).to_radians());
        let d = Vec3::new((p.sin() * t.sin()) as f32, p.cos() as f32, (p.sin() * t.cos()) as f32);
        Vec3::from(self.target.map(|v| v as f32)) + d * self.radius as f32
    }

    /// The model matrix: the configured rotation about the model's centre.
    pub fn model_matrix(&self, model: &Model) -> Mat4 {
        let c = model.center();
        let r = Mat4::from_rotation_y(self.rotation[2] as f32)
            * Mat4::from_rotation_x(self.rotation[1] as f32)
            * Mat4::from_rotation_z(self.rotation[0] as f32);
        Mat4::from_translation(c) * r * Mat4::from_translation(-c)
    }

    pub fn view_matrix(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye(), Vec3::from(self.target.map(|v| v as f32)), Vec3::Y)
    }

    pub fn projection(&self, aspect: f32) -> Mat4 {
        let r = self.radius.max(1e-6) as f32;
        Mat4::perspective_rh(self.fov_deg.clamp(5.0, 120.0).to_radians() as f32, aspect.max(1e-3), (r * 0.01).max(1e-4), r * 50.0)
    }

    /// Model → clip.
    pub fn mvp(&self, model: &Model, aspect: f32) -> Mat4 {
        self.projection(aspect) * self.view_matrix() * self.model_matrix(model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_eye_sits_in_front_at_the_start_and_turns_with_a_drag() {
        let v = View { theta_deg: 0.0, phi_deg: 90.0, radius: 10.0, target: [0.0; 3], fov_deg: 30.0, rotation: [0.0; 3] };
        let e = v.eye();
        assert!((e.z - 10.0).abs() < 1e-4 && e.x.abs() < 1e-4 && e.y.abs() < 1e-4);
        let t = v.turned(0.25, 0.0);
        assert!((t.theta_deg + 60.0).abs() < 1e-9);
        let t = v.turned(0.0, -1.0);
        assert_eq!(t.phi_deg, PHI_MAX_DEG);
        assert!((v.zoomed(2.0).radius - 5.0).abs() < 1e-9);
    }
}
