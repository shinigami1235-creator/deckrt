//! deckrt: the room's reactions and the feedback form. A phone sends a reaction (love it,
//! great, wow, lost me) and it floats up the side of the screen; at the end the presenter opens
//! the feedback form and phones give the talk a rating and two comments. Both go in the results
//! file.

use std::collections::{BTreeMap, HashMap};

use egui::{Color32, Pos2, Rect, Shape, Stroke, Ui, pos2, vec2};
use serde_json::{Value, json};

/// How long a reaction floats.
const FLOAT_SECS: f64 = 3.0;
/// One reaction per phone in this long.
const PER_PHONE_SECS: f64 = 1.2;
/// Most on the screen at once; more are counted, not drawn.
const MAX_FLYING: usize = 40;

pub const KINDS: [(&str, &str); 4] = [("heart", "Love it"), ("star", "Great"), ("wow", "Wow"), ("lost", "Lost me")];

fn kind_index(kind: &str) -> Option<usize> {
    KINDS.iter().position(|(k, _)| *k == kind)
}

#[derive(Clone, Debug)]
pub struct Answer {
    pub name: Option<String>,
    pub rating: u8,
    pub useful: String,
    pub better: String,
}

#[derive(Clone, Debug)]
struct Flying {
    kind: usize,
    at: f64,
    /// 0 to 1 across the lane, so they don't stack.
    lane: f32,
}

pub struct Crowd {
    /// Phones see the feedback form.
    pub feedback_open: bool,
    /// Reactions float on the audience screen.
    pub reactions_on: bool,
    pub answers: BTreeMap<String, Answer>,
    /// Per slide, per kind.
    counts: BTreeMap<usize, [u32; 4]>,
    flying: Vec<Flying>,
    last: HashMap<String, f64>,
    seq: u32,
    /// The form opened by itself when the show reached its end.
    pub auto_asked: bool,
}

impl Default for Crowd {
    fn default() -> Self {
        Crowd {
            feedback_open: false,
            reactions_on: true,
            answers: BTreeMap::new(),
            counts: BTreeMap::new(),
            flying: vec![],
            last: HashMap::new(),
            seq: 0,
            auto_asked: false,
        }
    }
}

impl Crowd {
    /// A reaction from phone `from` on slide `slide`. False when it was too soon after its last
    /// or reactions are off.
    pub fn react(&mut self, from: &str, kind: &str, slide: usize, now: f64) -> bool {
        let Some(k) = kind_index(kind) else { return false };
        if !self.reactions_on || self.last.get(from).is_some_and(|t| now - t < PER_PHONE_SECS) {
            return false;
        }
        self.last.insert(from.to_string(), now);
        if let Some(c) = self.counts.entry(slide).or_default().get_mut(k) {
            *c += 1;
        }
        self.seq = self.seq.wrapping_add(1);
        if self.flying.len() < MAX_FLYING {
            // A spread that doesn't repeat for a while.
            let lane = ((self.seq as f32 * 0.618_034) % 1.0).abs();
            self.flying.push(Flying { kind: k, at: now, lane });
        }
        true
    }

    pub fn answer(&mut self, from: &str, a: Answer) {
        self.answers.insert(from.to_string(), a);
    }

    /// Drop reactions that have finished. True while some are still in the air.
    pub fn tick(&mut self, now: f64) -> bool {
        self.flying.retain(|f| now - f.at < FLOAT_SECS);
        !self.flying.is_empty()
    }

    pub fn counts_on(&self, slide: usize) -> [u32; 4] {
        self.counts.get(&slide).copied().unwrap_or_default()
    }

    pub fn average(&self) -> Option<f64> {
        let r: Vec<f64> = self.answers.values().filter(|a| a.rating > 0).map(|a| a.rating as f64).collect();
        (!r.is_empty()).then(|| r.iter().sum::<f64>() / r.len() as f64)
    }

    pub fn feedback_report(&self) -> Option<deckcraft_phones::results::Feedback> {
        if self.answers.is_empty() {
            return None;
        }
        let mut ratings = [0u32; 5];
        for a in self.answers.values() {
            if (1..=5).contains(&a.rating) {
                ratings[a.rating as usize - 1] += 1;
            }
        }
        let comments = self
            .answers
            .values()
            .map(|a| json!({"name": a.name.clone().unwrap_or_else(|| "Anonymous".into()), "rating": a.rating, "useful": a.useful, "better": a.better}))
            .collect();
        Some(deckcraft_phones::results::Feedback { answers: self.answers.len() as u32, average: self.average(), ratings, comments })
    }

    /// `{"slide": 3, "heart": 12, ...}` per slide that got any, slides counted from 1.
    pub fn reactions_report(&self) -> Vec<Value> {
        self.counts
            .iter()
            .map(|(s, c)| {
                let mut o = serde_json::Map::new();
                o.insert("slide".into(), json!(s + 1));
                for (i, (k, _)) in KINDS.iter().enumerate() {
                    o.insert((*k).into(), json!(c[i]));
                }
                Value::Object(o)
            })
            .collect()
    }
}

fn color(kind: usize) -> Color32 {
    match kind {
        0 => Color32::from_rgb(0xe9, 0x4b, 0x5c),
        1 => Color32::from_rgb(0xf2, 0xb1, 0x2c),
        2 => Color32::from_rgb(0x8e, 0x5c, 0xe8),
        _ => Color32::from_rgb(0x4c, 0x8d, 0xff),
    }
}

/// One reaction's mark, drawn: a heart, a star, `!` or `?`.
pub fn mark(painter: &egui::Painter, c: Pos2, r: f32, kind: usize, alpha: f32) {
    let a = |col: Color32| col.gamma_multiply(alpha);
    let col = color(kind);
    painter.circle_filled(c, r, a(Color32::from_rgba_unmultiplied(255, 255, 255, 235)));
    painter.circle_stroke(c, r, Stroke::new(r * 0.08, a(col)));
    let s = r * 0.55;
    match kind {
        0 => {
            // A heart from its curve. Not convex at the top, so filled as one mesh from a point
            // inside it, with a smooth outline over the edge.
            let pts: Vec<Pos2> = (0..48)
                .map(|i| {
                    let t = i as f32 / 48.0 * std::f32::consts::TAU;
                    let x = 16.0 * t.sin().powi(3);
                    let y = 13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos();
                    pos2(c.x + x / 17.0 * s, c.y - y / 17.0 * s + s * 0.1)
                })
                .collect();
            fill_star_shaped(painter, pos2(c.x, c.y + s * 0.1), pts, a(col));
        }
        1 => {
            let pts: Vec<Pos2> = (0..10)
                .map(|i| {
                    let ang = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                    let rr = if i % 2 == 0 { s } else { s * 0.45 };
                    pos2(c.x + rr * ang.cos(), c.y + rr * ang.sin() + s * 0.08)
                })
                .collect();
            fill_star_shaped(painter, c + vec2(0.0, s * 0.08), pts, a(col));
        }
        k => {
            let text = if k == 2 { "!" } else { "?" };
            painter.text(c, egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(r * 1.25), a(col));
        }
    }
}

/// Fill a shape every edge of which `mid` can see, as one mesh, with an outline for a smooth edge.
fn fill_star_shaped(painter: &egui::Painter, mid: Pos2, pts: Vec<Pos2>, col: Color32) {
    let mut mesh = egui::epaint::Mesh::default();
    mesh.colored_vertex(mid, col);
    for p in &pts {
        mesh.colored_vertex(*p, col);
    }
    let n = pts.len() as u32;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    painter.add(Shape::mesh(mesh));
    painter.add(Shape::closed_line(pts, Stroke::new(1.0, col)));
}

/// The reactions floating up the right of the audience screen.
pub fn draw(painter: &egui::Painter, slide: Rect, crowd: &Crowd, now: f64) {
    if !crowd.reactions_on {
        return;
    }
    let r = (slide.height() * 0.035).clamp(14.0, 40.0);
    let lane_w = slide.width() * 0.08;
    for f in &crowd.flying {
        let t = ((now - f.at) / FLOAT_SECS) as f32;
        if !(0.0..1.0).contains(&t) {
            continue;
        }
        let x = slide.max.x - r * 1.6 - f.lane * lane_w + (t * 9.0 + f.lane * 6.0).sin() * r * 0.3;
        let y = slide.max.y - r * 1.5 - t * slide.height() * 0.55;
        let alpha = if t < 0.1 { t / 0.1 } else { 1.0 - ((t - 0.6) / 0.4).clamp(0.0, 1.0) };
        mark(painter, pos2(x, y), r, f.kind, alpha);
    }
}

/// Presenter view's rows for reactions and feedback. True when phones need telling.
pub fn section(ui: &mut Ui, crowd: &mut Crowd, slide: usize) -> bool {
    let mut changed = false;
    let dim = |ui: &mut Ui, t: &str| {
        ui.label(egui::RichText::new(t).size(12.0).color(Color32::from_gray(150)));
    };
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Reactions").size(13.0).color(Color32::from_gray(170)));
        let c = crowd.counts_on(slide);
        for (i, n) in c.iter().enumerate() {
            let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), egui::Sense::hover());
            mark(ui.painter(), r.center(), 7.5, i, 1.0);
            ui.label(egui::RichText::new(n.to_string()).size(12.0).color(Color32::from_gray(200))).on_hover_text(KINDS[i].1);
        }
        // Off means the phones can't send them, not just that the screen doesn't show them.
        if ui.small_button(if crowd.reactions_on { "Turn off" } else { "Turn on" }).on_hover_text("Reactions from the phones").clicked() {
            crowd.reactions_on = !crowd.reactions_on;
            changed = true;
        }
    });
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Feedback").size(13.0).color(Color32::from_gray(170)));
        let label = if crowd.feedback_open { "Close the form" } else { "Ask for feedback" };
        if ui.small_button(label).on_hover_text("Phones get a rating and two comment boxes").clicked() {
            crowd.feedback_open = !crowd.feedback_open;
            changed = true;
        }
        match (crowd.answers.len(), crowd.average()) {
            (0, _) => {}
            (n, Some(avg)) => dim(ui, &format!("{n} answered, {avg:.1} of 5")),
            (n, None) => dim(ui, &format!("{n} answered")),
        }
    });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reactions_are_limited_counted_and_float_away() {
        let mut c = Crowd::default();
        assert!(c.react("a", "heart", 2, 0.0));
        assert!(!c.react("a", "star", 2, 0.5), "too soon after the last");
        assert!(c.react("a", "star", 2, 1.5));
        assert!(c.react("b", "lost", 2, 1.5));
        assert!(!c.react("b", "thumbs", 2, 9.0), "not a reaction");
        assert_eq!(c.counts_on(2), [1, 1, 0, 1]);
        assert!(c.tick(2.0));
        assert!(!c.tick(10.0));
        c.reactions_on = false;
        assert!(!c.react("c", "wow", 3, 20.0));
        assert_eq!(c.reactions_report(), vec![json!({"slide": 3, "heart": 1, "star": 1, "wow": 0, "lost": 1})]);
    }

    #[test]
    fn feedback_is_one_answer_per_phone() {
        let mut c = Crowd::default();
        assert!(c.feedback_report().is_none());
        c.answer("a", Answer { name: Some("Sam".into()), rating: 4, useful: "The ranking poll".into(), better: String::new() });
        c.answer("a", Answer { name: Some("Sam".into()), rating: 5, useful: "The ranking poll".into(), better: String::new() });
        c.answer("b", Answer { name: None, rating: 0, useful: String::new(), better: "Slower on slide 7".into() });
        let f = c.feedback_report().unwrap_or_else(|| deckcraft_phones::results::Feedback { answers: 0, average: None, ratings: [0; 5], comments: vec![] });
        assert_eq!(f.answers, 2);
        assert_eq!(f.average, Some(5.0), "a blank rating isn't counted");
        assert_eq!(f.ratings, [0, 0, 0, 0, 1]);
        assert_eq!(f.comments[1]["name"], "Anonymous");
    }
}
