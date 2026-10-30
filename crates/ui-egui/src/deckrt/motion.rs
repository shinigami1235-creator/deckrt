//! deckrt: a 3D model moving by itself (see [`Motion`]): the motion that repeats, the float, the
//! entrance as the slide opens, and what it does when the pointer comes near. [`Live`] keeps where
//! each model is in its motion; both show windows step the same one, so the audience screen and
//! presenter view agree.

use std::f64::consts::{PI, TAU};

use deckcraft_interactive::object::{Entrance, Hover, Motion, MotionMode};
use deckcraft_model3d::View;
use egui::{Pos2, Rect, Vec2};

/// How far either way a sway goes, in degrees.
const SWAY_DEG: f64 = 30.0;
/// How far a wobble tips, in degrees.
const WOBBLE_DEG: f64 = 12.0;
/// How far a showcase nods, in degrees.
const SHOWCASE_NOD_DEG: f64 = 12.0;
/// A bounce's height, as a share of the model's height.
const BOUNCE_SHARE: f32 = 0.08;
/// How much a breath and a beat change the size.
const BREATHE: f64 = 0.06;
const BEAT: f64 = 0.09;
/// A jiggle's wobble, in degrees, and how often it shakes a second.
const JIGGLE_DEG: f64 = 7.0;
const JIGGLE_HZ: f64 = 4.5;
/// How far the model turns towards (or away from) the pointer, in degrees.
const FOLLOW_DEG: [f64; 2] = [28.0, 16.0];
/// A hop's height (share of the height) and length in seconds.
const HOP_SHARE: f32 = 0.12;
const HOP_SECS: f64 = 0.45;
/// How much bigger Grow makes it near the pointer.
const GROW: f64 = 0.12;
/// How much faster Faster runs.
const FASTER: f64 = 3.0;
/// The float's rise, as a share of the model's height, and its period in seconds.
const FLOAT_SHARE: f32 = 0.035;
const FLOAT_SECS: f64 = 3.4;
/// How long each entrance takes, in seconds.
const ENTRANCE_SECS: [f64; 3] = [0.8, 1.2, 1.1];

/// Where one model is in its motion.
#[derive(Clone, Debug, Default)]
pub struct Live {
    /// Radians into the motion so far (a whole turn of a spin is TAU).
    phase: f64,
    /// How hard it is jiggling, 0 to 1.
    energy: f64,
    /// The turn towards the pointer now, in degrees: right and down.
    follow: [f64; 2],
    /// How far grown and how much faster now, 0 to 1.
    grown: f64,
    rush: f64,
    /// The pointer is on or near it.
    near: bool,
    /// When the last hop began.
    hop_at: Option<f64>,
    /// When it first drew: the entrance runs from here.
    born: Option<f64>,
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

/// 0 to 1, overshooting a little before it settles (a grow-in).
fn ease_out_back(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0) - 1.0;
    1.0 + t * t * (2.7 * t + 1.7)
}

/// 0 to 1, slowing to a stop.
fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// 0 to 1 with the bounces of something dropped.
fn ease_out_bounce(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    let (n, d) = (7.5625, 2.75);
    if t < 1.0 / d {
        n * t * t
    } else if t < 2.0 / d {
        let t = t - 1.5 / d;
        n * t * t + 0.75
    } else if t < 2.5 / d {
        let t = t - 2.25 / d;
        n * t * t + 0.9375
    } else {
        let t = t - 2.625 / d;
        n * t * t + 0.984375
    }
}

/// A heartbeat over one round (0 to 1): a beat, a smaller one, then rest.
fn beat(t: f64) -> f64 {
    let pulse = |c: f64, w: f64| (-((t - c) / w).powi(2)).exp();
    pulse(0.12, 0.05) + 0.6 * pulse(0.32, 0.05)
}

fn smooth(v: &mut f64, target: f64, dt: f64, rate: f64) {
    *v += (target - *v) * (1.0 - (-dt * rate).exp());
}

impl Live {
    /// Move on to `now`. `pointer` is the pointer in this window, when it is in it.
    pub fn step(&mut self, m: &Motion, now: f64, rect: Rect, pointer: Option<Pos2>) {
        let dt = self.last.map_or(0.0, |l| (now - l).clamp(0.0, 0.25));
        self.last = Some(now);
        self.born.get_or_insert(now);
        // A window without the pointer leaves the reaction to the one that has it.
        let near = pointer.map(|p| closeness(rect, p));
        if let Some(n) = near {
            let was = self.near;
            self.near = n > 0.0;
            if m.hover == Hover::Hop && self.near && !was && self.hop_at.is_none_or(|h| now - h > HOP_SECS) {
                self.hop_at = Some(now);
            }
        }
        let on = |h: Hover| m.hover == h && self.near;
        smooth(&mut self.rush, if on(Hover::Faster) { 1.0 } else { 0.0 }, dt, 4.0);
        smooth(&mut self.grown, if on(Hover::Grow) { 1.0 } else { 0.0 }, dt, 8.0);
        if m.mode != MotionMode::None && !on(Hover::Pause) {
            let sign = if m.reverse { -1.0 } else { 1.0 };
            let speed = 1.0 + (FASTER - 1.0) * self.rush;
            self.phase = (self.phase + sign * speed * TAU / m.seconds.max(0.3) * dt) % (TAU * 1000.0);
        }
        match m.hover {
            Hover::Jiggle => {
                let target = near.unwrap_or(0.0);
                // It shakes as long as the pointer is close, and settles within a second after.
                self.energy = target.max(self.energy * (-dt * 3.5).exp());
            }
            Hover::Follow | Hover::Shy => {
                let sign = if m.hover == Hover::Shy { -1.0 } else { 1.0 };
                let target = match pointer {
                    Some(p) if self.near => {
                        let c = rect.center();
                        let half = Vec2::new(rect.width().max(1.0), rect.height().max(1.0)) * 0.5;
                        [
                            sign * ((p.x - c.x) / half.x).clamp(-1.0, 1.0) as f64 * FOLLOW_DEG[0],
                            sign * ((p.y - c.y) / half.y).clamp(-1.0, 1.0) as f64 * FOLLOW_DEG[1],
                        ]
                    }
                    Some(_) => [0.0, 0.0],
                    None => self.follow,
                };
                for (f, t) in self.follow.iter_mut().zip(target) {
                    smooth(f, t, dt, 6.0);
                }
            }
            _ => {}
        }
    }

    /// How far into the entrance it is, 0 to 1 (1 once done or with none).
    fn entrance(&self, m: &Motion, now: f64) -> f64 {
        let secs = match m.entrance {
            Entrance::None => return 1.0,
            Entrance::Grow => ENTRANCE_SECS[0],
            Entrance::Spin => ENTRANCE_SECS[1],
            Entrance::Drop => ENTRANCE_SECS[2],
        };
        self.born.map_or(0.0, |b| ((now - b) / secs).clamp(0.0, 1.0))
    }

    /// `view` with the motion now added.
    pub fn apply(&self, m: &Motion, now: f64, mut view: View) -> View {
        let axis = m.axis.index();
        let round = (self.phase / TAU).rem_euclid(1.0);
        let mut size = 1.0;
        match m.mode {
            MotionMode::None | MotionMode::Bounce => {}
            MotionMode::Spin => view.rotation[axis] += self.phase,
            MotionMode::Sway => view.rotation[axis] += SWAY_DEG.to_radians() * self.phase.sin(),
            MotionMode::Wobble => {
                view.rotation[1] += WOBBLE_DEG.to_radians() * self.phase.sin();
                view.rotation[0] += WOBBLE_DEG.to_radians() * self.phase.cos();
            }
            MotionMode::Breathe => size *= 1.0 + BREATHE * (0.5 - 0.5 * self.phase.cos()),
            MotionMode::Heartbeat => size *= 1.0 + BEAT * beat(round),
            MotionMode::Showcase => {
                view.rotation[2] += self.phase;
                view.rotation[1] += SHOWCASE_NOD_DEG.to_radians() * (self.phase * 0.5).sin();
            }
        }
        if m.hover == Hover::Jiggle && self.energy > 0.001 {
            let w = TAU * JIGGLE_HZ * now;
            let a = JIGGLE_DEG.to_radians() * self.energy;
            view.rotation[0] += a * w.sin();
            view.rotation[1] += a * 0.6 * (w * 1.3 + 1.0).cos();
            size *= 1.0 + 0.04 * self.energy * (w * 0.8 + 0.5).sin();
        }
        if matches!(m.hover, Hover::Follow | Hover::Shy) {
            view.theta_deg += self.follow[0];
            view.phi_deg = (view.phi_deg + self.follow[1]).clamp(1.0, 179.0);
        }
        size *= 1.0 + GROW * self.grown;
        let e = self.entrance(m, now);
        match m.entrance {
            _ if e >= 1.0 => {}
            Entrance::Grow => size *= ease_out_back(e).max(0.02),
            Entrance::Spin => view.rotation[2] += (1.0 - ease_out_cubic(e)) * TAU,
            _ => {}
        }
        // Bigger on screen is the camera closer.
        view.radius /= size.max(0.02);
        view
    }

    /// Where the model is drawn now: its box moved by a bounce, a hop, a drop and the float.
    pub fn place(&self, m: &Motion, now: f64, rect: Rect) -> Rect {
        let h = rect.height();
        let mut dy = 0.0f32;
        if m.mode == MotionMode::Bounce {
            dy -= h * BOUNCE_SHARE * self.phase.sin().abs() as f32;
        }
        if m.float {
            dy -= h * FLOAT_SHARE * (TAU * now / FLOAT_SECS).sin() as f32;
        }
        if let Some(at) = self.hop_at.filter(|at| now - at < HOP_SECS) {
            dy -= h * HOP_SHARE * (PI * (now - at) / HOP_SECS).sin() as f32;
        }
        if m.entrance == Entrance::Drop && self.entrance(m, now) < 1.0 {
            // From just above its own top, bouncing to rest.
            dy -= (rect.max.y + h * 0.2) * (1.0 - ease_out_bounce(self.entrance(m, now))) as f32;
        }
        rect.translate(Vec2::new(0.0, dy))
    }

    /// Whether the next frame looks different (so the window keeps drawing).
    pub fn moving(&self, m: &Motion, now: f64) -> bool {
        m.mode != MotionMode::None
            || m.float
            || self.entrance(m, now) < 1.0
            || self.energy > 0.001
            || self.follow.iter().any(|f| f.abs() > 0.05)
            || self.grown > 0.002
            || self.hop_at.is_some_and(|at| now - at < HOP_SECS)
            || (m.hover != Hover::None && self.near)
    }
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

    fn run(m: &Motion, until: f64, pointer: Option<Pos2>) -> Live {
        let mut l = Live::default();
        let mut t = 0.0;
        while t <= until + 1e-9 {
            l.step(m, t, rect(), pointer);
            t += 0.05;
        }
        l
    }

    #[test]
    fn a_spin_turns_once_in_its_seconds_about_its_axis() {
        let m = Motion { mode: MotionMode::Spin, axis: MotionAxis::Tilt, seconds: 4.0, ..Default::default() };
        let v = run(&m, 2.0, None).apply(&m, 2.0, view());
        assert!((v.rotation[0] - PI).abs() < 1e-6, "half a turn in 2 s: {:?}", v.rotation);
        assert_eq!((v.rotation[1], v.rotation[2]), (0.0, 0.0));
        let back = Motion { reverse: true, ..m.clone() };
        assert!(run(&back, 0.2, None).apply(&back, 0.2, view()).rotation[0] < 0.0);
    }

    #[test]
    fn every_motion_moves_the_model_and_comes_back_round() {
        for mode in [
            MotionMode::Spin,
            MotionMode::Sway,
            MotionMode::Wobble,
            MotionMode::Bounce,
            MotionMode::Breathe,
            MotionMode::Heartbeat,
            MotionMode::Showcase,
        ] {
            let m = Motion { mode, seconds: 2.0, ..Default::default() };
            let mut differs = false;
            for k in 1..20 {
                let t = k as f64 * 0.1;
                let l = run(&m, t, None);
                differs |= l.apply(&m, t, view()) != view() || l.place(&m, t, rect()) != rect();
            }
            assert!(differs, "{mode:?} moves");
            assert!(run(&m, 1.0, None).moving(&m, 1.0));
        }
        // A bounce goes up, never down through the floor.
        let m = Motion { mode: MotionMode::Bounce, seconds: 1.0, ..Default::default() };
        for k in 0..20 {
            let t = k as f64 * 0.05;
            assert!(run(&m, t, None).place(&m, t, rect()).min.y <= rect().min.y);
        }
    }

    #[test]
    fn entrances_run_once_from_the_first_frame() {
        let grow = Motion { entrance: Entrance::Grow, ..Default::default() };
        let l = run(&grow, 0.0, None);
        assert!(l.apply(&grow, 0.0, view()).radius > 100.0, "starts tiny");
        assert!(l.moving(&grow, 0.4));
        assert_eq!(l.apply(&grow, 2.0, view()), view(), "full size after");
        assert!(!l.moving(&grow, 2.0));
        let drop = Motion { entrance: Entrance::Drop, ..Default::default() };
        let l = run(&drop, 0.0, None);
        assert!(l.place(&drop, 0.0, rect()).max.y < 0.0, "starts above the slide");
        assert_eq!(l.place(&drop, 2.0, rect()), rect());
        let spin = Motion { entrance: Entrance::Spin, ..Default::default() };
        let l = run(&spin, 0.0, None);
        assert!((l.apply(&spin, 0.0, view()).rotation[2] - TAU).abs() < 1e-9);
        assert_eq!(l.apply(&spin, 2.0, view()), view());
    }

    #[test]
    fn a_jiggle_starts_near_the_pointer_and_settles_after_it_leaves() {
        let m = Motion { hover: Hover::Jiggle, ..Default::default() };
        let mut l = Live::default();
        l.step(&m, 0.0, rect(), Some(Pos2::new(900.0, 900.0)));
        assert!(!l.moving(&m, 0.0), "far away: still");
        l.step(&m, 0.1, rect(), Some(Pos2::new(200.0, 200.0)));
        assert!(l.moving(&m, 0.1));
        assert!(l.apply(&m, 0.155, view()) != view(), "it shakes");
        let mut t = 0.1;
        while t < 2.2 {
            t += 0.05;
            l.step(&m, t, rect(), Some(Pos2::new(900.0, 900.0)));
        }
        assert!(!l.moving(&m, t), "settled");
        // The other window, without the pointer, doesn't stop a jiggle the first one started.
        l.step(&m, 3.0, rect(), Some(Pos2::new(150.0, 150.0)));
        l.step(&m, 3.0, rect(), None);
        assert!(l.moving(&m, 3.0));
    }

    #[test]
    fn the_pointer_reactions() {
        let at = Some(Pos2::new(300.0, 200.0));
        // Pause stops the motion; Faster runs it three times as fast.
        let spin = |hover| Motion { mode: MotionMode::Spin, hover, seconds: 2.0, ..Default::default() };
        assert_eq!(run(&spin(Hover::Pause), 1.0, at).apply(&spin(Hover::Pause), 1.0, view()).rotation[2], 0.0);
        let plain = run(&spin(Hover::None), 1.0, at).phase;
        let fast = run(&spin(Hover::Faster), 1.0, at).phase;
        assert!(fast > plain * 2.0, "{fast} vs {plain}");
        // Follow turns towards the pointer, Shy away from it.
        let follow = Motion { hover: Hover::Follow, ..Default::default() };
        assert!(run(&follow, 2.0, at).apply(&follow, 2.0, view()).theta_deg > 20.0);
        let shy = Motion { hover: Hover::Shy, ..Default::default() };
        assert!(run(&shy, 2.0, at).apply(&shy, 2.0, view()).theta_deg < -20.0);
        // Grow gets bigger (the camera closer).
        let grow = Motion { hover: Hover::Grow, ..Default::default() };
        assert!(run(&grow, 1.0, at).apply(&grow, 1.0, view()).radius < 9.5);
        // Hop jumps once as the pointer arrives.
        let hop = Motion { hover: Hover::Hop, ..Default::default() };
        let mut l = Live::default();
        l.step(&hop, 0.0, rect(), Some(Pos2::new(900.0, 900.0)));
        l.step(&hop, 0.1, rect(), at);
        assert!(l.place(&hop, 0.3, rect()).min.y < rect().min.y);
        assert_eq!(l.place(&hop, 1.0, rect()), rect(), "once");
    }

    #[test]
    fn float_rises_and_sinks() {
        let m = Motion { float: true, ..Default::default() };
        let l = Live::default();
        let a = l.place(&m, FLOAT_SECS / 4.0, rect());
        let b = l.place(&m, FLOAT_SECS * 0.75, rect());
        assert!(a.min.y < rect().min.y && b.min.y > rect().min.y);
        assert_eq!(l.place(&Motion::default(), 1.0, rect()), rect());
    }
}
