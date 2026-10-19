//! deckrt: questions from the phones. A phone asks (with its name or without), the room backs
//! the questions it wants answered, and the presenter puts one on the screen, marks it answered
//! or hides it. The list lives for the whole show, whatever slide is up.

use std::collections::BTreeSet;

use egui::{Align2, Color32, CornerRadius, FontId, Rect, Ui, pos2, vec2};
use serde_json::{Value, json};

/// Questions one phone may have waiting at once.
pub const PER_PHONE: usize = 5;
/// Questions kept for one show.
pub const MAX: usize = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QState {
    /// Asked, not dealt with yet.
    Open,
    /// Marked answered: phones still see it, greyed.
    Answered,
    /// Taken off the phones' list by the presenter.
    Hidden,
}

#[derive(Clone, Debug)]
pub struct Question {
    pub id: String,
    /// The phone that asked.
    pub from: String,
    /// Its name; none when it asked without one.
    pub name: Option<String>,
    pub text: String,
    /// Phones backing it.
    pub votes: BTreeSet<String>,
    pub state: QState,
    /// The clock when it came in.
    pub at: f64,
}

#[derive(Default)]
pub struct Questions {
    pub list: Vec<Question>,
    seq: u32,
    /// Phones may ask. On when a show starts.
    pub closed: bool,
    /// The question on the audience screen.
    pub shown: Option<String>,
}

impl Questions {
    /// Take a question from a phone. Returns false when it was refused (closed, too many).
    pub fn ask(&mut self, from: &str, name: Option<String>, text: &str, anon: bool, now: f64) -> bool {
        let text = deckcraft_interactive::poll::clean_text(text);
        if self.closed || text.is_empty() || self.list.len() >= MAX {
            return false;
        }
        let waiting = self.list.iter().filter(|q| q.from == from && q.state == QState::Open).count();
        if waiting >= PER_PHONE {
            return false;
        }
        if self.list.iter().any(|q| q.from == from && q.text.eq_ignore_ascii_case(&text)) {
            return false;
        }
        self.seq += 1;
        self.list.push(Question {
            id: format!("q{}", self.seq),
            from: from.to_string(),
            name: if anon { None } else { name },
            text,
            votes: BTreeSet::new(),
            state: QState::Open,
            at: now,
        });
        true
    }

    /// Drop everything from one phone: its questions and its votes on the others (Eject).
    pub fn forget(&mut self, from: &str) {
        self.list.retain(|q| q.from != from);
        for q in &mut self.list {
            q.votes.remove(from);
        }
        if self.shown.as_ref().is_some_and(|id| !self.list.iter().any(|q| &q.id == id)) {
            self.shown = None;
        }
    }

    /// Why `ask` turned a question down, in the words the phone shows.
    pub fn why_refused(&self, from: &str, text: &str) -> &'static str {
        let text = deckcraft_interactive::poll::clean_text(text);
        if self.closed {
            "The presenter isn't taking questions right now."
        } else if text.is_empty() {
            "Type a question first."
        } else if self.list.len() >= MAX {
            "There are too many questions already."
        } else if self.list.iter().any(|q| q.from == from && q.text.eq_ignore_ascii_case(&text)) {
            "You asked that one already."
        } else {
            "You have five questions waiting. Ask again once one is answered."
        }
    }

    /// Back a question or take the backing back. A phone can't back a hidden question.
    pub fn vote(&mut self, from: &str, q: &str, on: bool) -> bool {
        let Some(item) = self.list.iter_mut().find(|x| x.id == q && x.state != QState::Hidden) else { return false };
        if on { item.votes.insert(from.to_string()) } else { item.votes.remove(from) }
    }

    /// Most backed first, then oldest first; answered and hidden ones after the open ones.
    pub fn sorted(&self) -> Vec<&Question> {
        let mut v: Vec<&Question> = self.list.iter().collect();
        let rank = |s: QState| match s {
            QState::Open => 0,
            QState::Answered => 1,
            QState::Hidden => 2,
        };
        v.sort_by(|a, b| {
            rank(a.state)
                .cmp(&rank(b.state))
                .then(b.votes.len().cmp(&a.votes.len()))
                .then(a.at.partial_cmp(&b.at).unwrap_or(std::cmp::Ordering::Equal))
        });
        v
    }

    pub fn set_state(&mut self, q: &str, state: QState) {
        if let Some(item) = self.list.iter_mut().find(|x| x.id == q) {
            item.state = state;
        }
        if state != QState::Open && self.shown.as_deref() == Some(q) {
            self.shown = None;
        }
    }

    pub fn shown_question(&self) -> Option<&Question> {
        let id = self.shown.as_deref()?;
        self.list.iter().find(|q| q.id == id)
    }

    /// What phones see: every question not hidden, in order.
    pub fn for_phones(&self) -> Value {
        json!(
            self.sorted()
                .into_iter()
                .filter(|q| q.state != QState::Hidden)
                .map(|q| json!({"id": q.id, "text": q.text, "name": q.name, "votes": q.votes.len(), "answered": q.state == QState::Answered, "shown": self.shown.as_deref() == Some(&q.id)}))
                .collect::<Vec<_>>()
        )
    }

    /// One phone's own standing: the questions it backed and asked.
    pub fn for_phone(&self, id: &str) -> Value {
        let backed: Vec<&str> = self.list.iter().filter(|q| q.votes.contains(id)).map(|q| q.id.as_str()).collect();
        let mine: Vec<&str> = self.list.iter().filter(|q| q.from == id).map(|q| q.id.as_str()).collect();
        json!({"backed": backed, "mine": mine})
    }

    /// The text for the results file.
    pub fn report(&self) -> Vec<Value> {
        self.list
            .iter()
            .map(|q| {
                json!({
                    "question": q.text,
                    "asked_by": q.name.clone().unwrap_or_else(|| "Anonymous".into()),
                    "votes": q.votes.len(),
                    "answered": q.state == QState::Answered,
                    "hidden": q.state == QState::Hidden,
                })
            })
            .collect()
    }
}

/// The presenter's Questions section. Returns whether anything a phone sees changed.
pub fn section(ui: &mut Ui, qs: &mut Questions) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Questions").size(13.0).color(Color32::from_gray(170)));
        let open = qs.list.iter().filter(|q| q.state == QState::Open).count();
        if open > 0 {
            ui.label(egui::RichText::new(format!("{open} waiting")).size(12.0).color(Color32::from_gray(140)));
        }
        let label = if qs.closed { "Let phones ask" } else { "Stop questions" };
        if ui.small_button(label).clicked() {
            qs.closed = !qs.closed;
            changed = true;
        }
    });
    if qs.list.is_empty() {
        ui.label(
            egui::RichText::new(if qs.closed { "Questions are off." } else { "No questions yet. Phones ask from the Questions tab." })
                .size(12.0)
                .color(Color32::from_gray(140)),
        );
        return changed;
    }
    let mut action: Option<(String, &'static str)> = None;
    let ids: Vec<(String, String, Option<String>, usize, QState)> = qs
        .sorted()
        .into_iter()
        .filter(|q| q.state != QState::Hidden)
        .map(|q| (q.id.clone(), q.text.clone(), q.name.clone(), q.votes.len(), q.state))
        .collect();
    egui::ScrollArea::vertical().id_salt("deckrt-questions").max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
        for (id, text, name, votes, state) in ids {
            let shown = qs.shown.as_deref() == Some(&id);
            let who = name.unwrap_or_else(|| "Anonymous".into());
            let col = if state == QState::Answered { Color32::from_gray(130) } else { Color32::WHITE };
            ui.label(egui::RichText::new(format!("{votes} · {text}")).size(13.0).color(col));
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(who).size(11.0).color(Color32::from_gray(140)));
                if state == QState::Open {
                    if ui.small_button(if shown { "Take off screen" } else { "Show on screen" }).clicked() {
                        action = Some((id.clone(), if shown { "unshow" } else { "show" }));
                    }
                    if ui.small_button("Answered").clicked() {
                        action = Some((id.clone(), "answered"));
                    }
                }
                if ui.small_button("Hide").on_hover_text("Take it off the phones' list").clicked() {
                    action = Some((id.clone(), "hide"));
                }
            });
            ui.add_space(3.0);
        }
    });
    if let Some((id, what)) = action {
        match what {
            "show" => qs.shown = Some(id),
            "unshow" => qs.shown = None,
            "answered" => qs.set_state(&id, QState::Answered),
            _ => qs.set_state(&id, QState::Hidden),
        }
        changed = true;
    }
    changed
}

/// The question on the audience screen: a band across the bottom of the slide.
/// `below` is the height taken at the bottom of the slide already (the caption band).
pub fn draw_on_screen(painter: &egui::Painter, slide: Rect, q: &Question, below: f32) {
    let size = (slide.height() * 0.034).clamp(14.0, 44.0);
    let pad = size * 0.6;
    let width = slide.width() * 0.86;
    let who = q.name.clone().unwrap_or_else(|| "Anonymous".into());
    let body = painter.layout(q.text.clone(), FontId::proportional(size), Color32::WHITE, width - pad * 2.0);
    let meta = format!("Question from {who}{}", if q.votes.is_empty() { String::new() } else { format!(" · {} backed it", q.votes.len()) });
    let meta_g = painter.layout_no_wrap(meta, FontId::proportional(size * 0.62), Color32::from_gray(200));
    let h = body.size().y + meta_g.size().y + pad * 2.0 + size * 0.25;
    let rect = Rect::from_center_size(pos2(slide.center().x, slide.max.y - below - h / 2.0 - slide.height() * 0.04), vec2(width, h));
    painter.rect_filled(rect, CornerRadius::same(10), Color32::from_rgba_unmultiplied(16, 22, 34, 235));
    painter.galley(rect.min + vec2(pad, pad), meta_g.clone(), Color32::from_gray(200));
    painter.galley(rect.min + vec2(pad, pad + meta_g.size().y + size * 0.25), body, Color32::WHITE);
    let _ = Align2::LEFT_TOP;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asking_backing_and_order() {
        let mut q = Questions::default();
        assert!(q.ask("a", Some("Sam".into()), "Why does elastin not regenerate?", false, 1.0));
        assert!(q.ask("b", Some("Ava".into()), "Dose for the glabella?", true, 2.0));
        assert!(!q.ask("b", Some("Ava".into()), "dose for the glabella?", true, 3.0), "the same question twice from one phone");
        assert!(q.vote("c", "q2", true));
        assert!(!q.vote("c", "q2", true), "backing twice counts once");
        let order: Vec<&str> = q.sorted().iter().map(|x| x.id.as_str()).collect();
        assert_eq!(order, vec!["q2", "q1"]);
        assert_eq!(q.list[1].name, None, "asked without a name");
        q.shown = Some("q2".into());
        q.set_state("q2", QState::Answered);
        assert_eq!(q.shown, None);
        q.closed = true;
        assert!(!q.ask("d", None, "Late", false, 4.0));
        let phones = q.for_phones();
        assert_eq!(phones.as_array().map(|a| a.len()), Some(2));
    }

    #[test]
    fn a_refusal_says_why_and_eject_forgets_the_phone() {
        let mut q = Questions::default();
        assert!(q.ask("a", None, "First?", false, 1.0));
        assert!(!q.ask("a", None, "first?", false, 2.0));
        assert_eq!(q.why_refused("a", "first?"), "You asked that one already.");
        assert!(q.ask("b", None, "Second?", false, 3.0));
        assert!(q.vote("a", "q2", true));
        q.shown = Some("q1".into());
        q.forget("a");
        assert_eq!(q.list.len(), 1);
        assert!(q.list[0].votes.is_empty(), "its backing goes too");
        assert_eq!(q.shown, None, "its question leaves the screen");
    }

    #[test]
    fn one_phone_cannot_flood() {
        let mut q = Questions::default();
        for i in 0..PER_PHONE {
            assert!(q.ask("a", None, &format!("Question {i}"), false, i as f64));
        }
        assert!(!q.ask("a", None, "One more", false, 99.0));
        assert!(q.ask("b", None, "Another phone", false, 100.0));
    }
}
