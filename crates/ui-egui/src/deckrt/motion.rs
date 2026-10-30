//! deckrt: a 3D model moving by itself (see [`Motion`]): the spin or sway, the float, and what
//! it does when the pointer comes near. [`Live`] keeps where each model is in its motion; both
//! show windows step the same one, so the audience screen and presenter view agree.

use deckcraft_interactive::object::{Hover, Motion, MotionMode};
use deckcraft_model3d::View;
use egui::{Pos2, Rect, Vec2};

/// How far either way a sway goes, in degrees.
const SWAY_DEG: f64 = 30.0;
/// A jiggle's wobble, in degrees, and how often it shakes a second.
const JIGGLE_DEG: f64 = 7.0;
const JIGGLE_HZ: f64 = 4.5;
/// How far the model turns towards the pointer, in degrees.
const FOLLOW_DEG: [f64; 2] = [28.0, 16.0];
/// The float's rise, as a share of the model's height, and its period in seconds.
const FLOAT_SHARE: f32 = 0.035;
const FLOAT_SECS: f64 = 3.4;

/// Where one model is in its motion.
#[derive(Clone, Debug, Default)]
pub struct Live {
    /// Radians round the axis so far (spin), or the sway's phase.
    phase: f64,
    /// How hard it is jiggling, 0 to 1.
    energy: f64,
    /// The turn towards the pointer now, in degrees: right and down.
    follow: [f64; 2],
    /// The pointer is on or near it.
    near: bool,
    last: Option<f64>,
}

/// How close `p` is to `rect`: 1 inside it, falling to 0 at half the model's size away.
pub fn closeness(rect: Rect, p: Pos2) -> f64 {
    let reach = (rect.width().max(rect.height()) * 0.5).max(1.0);
    let dx = (rect.min.x - p.x).max(p.x - rect.max.x).max(0.0);
    let dy = (rect.min.y - p.y).max(p.y - rect.max.y).max(0.0);
    let d = (dx * dx + dy * dy).sqrt();
    (1.0 - d / reach).clamp(0.0, 1.0) as f64
}

impl Live {
    /// Move on to `now`. `pointer` is the pointer in this window, when it is in it.
    pub fn step(&mut self, m: &Motion, now: f64, rect: Rect, pointer: Option<Pos2>) {
        let dt = self.last.map_or(0.0, |l| (now - l).clamp(0.0, 0.25));
        self.last = Some(now);
        // A window without the pointer leaves the reaction to the one that has it.
        let near = pointer.map(|p| closeness(rect, p));
        if let Some(n) = near {
            self.near = n > 0.0;
        }
        let paused = m.hover == Hover::Pause && self.near;
        if m.mode != MotionMode::None && !paused {
            let sign = if m.reverse { -1.0 } else { 1.0 };
            self.phase = (self.phase + sign * std::f64::consts::TAU / m.seconds.max(1.0) * dt) % (std::f64::consts::TAU * 1000.0);
        }
        match m.hover {
            Hover::Jiggle => {
                let target = near.unwrap_or(0.0);
                // It shakes as long as the pointer is close, and settles within a second after.
                self.energy = target.max(self.energy * (-dt * 3.5).exp());
            }
            Hover::Follow => {
                let target = match pointer {
                    Some(p) if self.near => {
                        let c = rect.center();
                        let half = Vec2::new(rect.width().max(1.0), rect.height().max(1.0)) * 0.5;
                        [
                            ((p.x - c.x) / half.x).clamp(-1.0, 1.0) as f64 * FOLLOW_DEG[0],
                            ((p.y - c.y) / half.y).clamp(-1.0, 1.0) as f64 * FOLLOW_DEG[1],
                        ]
                    }
                    Some(_) => [0.0, 0.0],
                    None => self.follow,
                };
                let k = 1.0 - (-dt * 6.0).exp();
                for (f, t) in self.follow.iter_mut().zip(target) {
                    *f += (t - *f) * k;
                }
            }
            _ => {}
        }
    }

    /// `view` with the motion now added.
    pub fn apply(&self, m: &Motion, now: f64, mut view: View) -> View {
        let axis = m.axis.index();
        match m.mode {
            MotionMode::None => {}
            MotionMode::Spin => view.rotation[axis] += self.phase,
            MotionMode::Sway => view.rotation[axis] += SWAY_DEG.to_radians() * (self.phase * 2.0).sin(),
        }
        if m.hover == Hover::Jiggle && self.energy > 0.001 {
            let w = std::f64::consts::TAU * JIGGLE_HZ * now;
            let a = JIGGLE_DEG.to_radians() * self.energy;
            view.rotation[0] += a * w.sin();
            view.rotation[1] += a * 0.6 * (w * 1.3 + 1.0).cos();
            view.radius *= 1.0 - 0.04 * self.energy * (w * 0.8 + 0.5).sin();
        }
        if m.hover == Hover::Follow {
            view.theta_deg += self.follow[0];
            view.phi_deg = (view.phi_deg + self.follow[1]).clamp(1.0, 179.0);
        }
        view
    }

    /// Whether the next frame looks different (so the window keeps drawing).
    pub fn moving(&self, m: &Motion) -> bool {
        m.mode != MotionMode::None
            || m.float
            || self.energy > 0.001
            || self.follow.iter().any(|f| f.abs() > 0.05)
            || (m.hover != Hover::None && self.near)
    }
}

/// Where the model is drawn now: its box, risen by the float.
pub fn floated(m: &Motion, now: f64, rect: Rect) -> Rect {
    if !m.float {
        return rect;
    }
    let dy = -(rect.height() * FLOAT_SHARE) * (std::f64::consts::TAU * now / FLOAT_SECS).sin() as f32;
    rect.translate(Vec2::new(0.0, dy))
}

#[cfg(test)]
mod tests {
    use super::*;
    use deckcraft_interactive::object::MotionAxis;

    fn view() -> View {
        View { theta_deg: 0.0, phi_deg: 90.0, radius: 10.0, target: [0.0; 3], fov_deg: 30.0, rotation: [0.0; 3] }
    }

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::new(200.0, 200.0))
    }

    #[test]
    fn a_spin_turns_once_in_its_seconds_about_its_axis() {
        let m = Motion { mode: MotionMode::Spin, axis: MotionAxis::Tilt, seconds: 4.0, ..Default::default() };
        let mut l = Live::default();
        for i in 0..=40 {
            l.step(&m, i as f64 * 0.05, rect(), None);
        }
        let v = l.apply(&m, 2.0, view());
        assert!((v.rotation[0] - std::f64::consts::PI).abs() < 1e-6, "half a turn in 2 s: {:?}", v.rotation);
        assert_eq!((v.rotation[1], v.rotation[2]), (0.0, 0.0));
        let back = Motion { reverse: true, ..m.clone() };
        let mut l = Live::default();
        l.step(&back, 0.0, rect(), None);
        l.step(&back, 0.2, rect(), None);
        assert!(l.apply(&back, 0.2, view()).rotation[0] < 0.0);
    }

    #[test]
    fn a_jiggle_starts_near_the_pointer_and_settles_after_it_leaves() {
        let m = Motion { hover: Hover::Jiggle, ..Default::default() };
        let mut l = Live::default();
        l.step(&m, 0.0, rect(), Some(Pos2::new(900.0, 900.0)));
        assert!(!l.moving(&m), "far away: still");
        l.step(&m, 0.1, rect(), Some(Pos2::new(200.0, 200.0)));
        assert!(l.moving(&m));
        assert!(l.apply(&m, 0.155, view()) != view(), "it shakes");
        // The pointer leaves; within two seconds it has settled.
        let mut t = 0.1;
        while t < 2.2 {
            t += 0.05;
            l.step(&m, t, rect(), Some(Pos2::new(900.0, 900.0)));
        }
        assert!(!l.moving(&m), "settled");
        // The other window, without the pointer, doesn't stop a jiggle the first one started.
        l.step(&m, 3.0, rect(), Some(Pos2::new(150.0, 150.0)));
        l.step(&m, 3.0, rect(), None);
        assert!(l.moving(&m));
    }

    #[test]
    fn pause_stops_the_spin_while_the_pointer_is_on_it() {
        let m = Motion { mode: MotionMode::Spin, hover: Hover::Pause, seconds: 2.0, ..Default::default() };
        let mut l = Live::default();
        l.step(&m, 0.0, rect(), Some(Pos2::new(150.0, 150.0)));
        l.step(&m, 0.2, rect(), Some(Pos2::new(150.0, 150.0)));
        assert_eq!(l.apply(&m, 0.2, view()).rotation[2], 0.0);
        l.step(&m, 0.4, rect(), Some(Pos2::new(900.0, 900.0)));
        assert!(l.apply(&m, 0.4, view()).rotation[2] > 0.0);
    }

    #[test]
    fn follow_turns_towards_the_pointer_and_float_rises_and_sinks() {
        let m = Motion { hover: Hover::Follow, float: true, ..Default::default() };
        let mut l = Live::default();
        for i in 0..40 {
            l.step(&m, i as f64 * 0.05, rect(), Some(Pos2::new(300.0, 200.0)));
        }
        let v = l.apply(&m, 2.0, view());
        assert!(v.theta_deg > 20.0, "turned right: {}", v.theta_deg);
        let a = floated(&m, FLOAT_SECS / 4.0, rect());
        let b = floated(&m, FLOAT_SECS * 0.75, rect());
        assert!(a.min.y < rect().min.y && b.min.y > rect().min.y);
        assert_eq!(floated(&Motion::default(), 1.0, rect()), rect());
    }
}
