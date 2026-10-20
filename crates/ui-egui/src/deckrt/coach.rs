//! deckrt: Rehearse with Coach. A rehearsal from the first slide with the microphone on: what
//! is heard is timed against the slides, and when the show ends a report says how long each
//! slide took, the pace, the filler words, the long pauses and the slides read out word for
//! word. The pace can be kept in the deck, and in a real talk presenter view then says how far
//! ahead or behind it the talk is.

use deckcraft_captions::coach::{self, Heard, Report, Span};
use egui::{Align2, Color32, CornerRadius, FontId, Rect, RichText, Ui, vec2};
use serde_json::json;

use super::captions::Status;
use crate::SlideApp;
use crate::show::Show;

pub const DIALOG: &str = "deckrt.coach";

/// Said before each slide's words in the recogniser's prompt, so it writes the ums and uhs down
/// instead of tidying them away.
const FILLER_PROMPT: &str = "Um, so, uh, the, like, you know, er, hmm, เอ่อ, อ่า.";

/// Seconds the recogniser gets after the show ends to finish the last phrase.
const FINISH_SECS: f64 = 4.0;

/// The coach while a rehearsal runs, and while its last words are written up.
pub struct Live {
    captioner: Option<deckcraft_captions::Captioner>,
    pub status: Status,
    /// The show clock when the microphone opened: the captioner's times count from here.
    t0: f64,
    heard: Vec<Heard>,
    spans: Vec<Span>,
    /// The slide on screen and since when.
    cur: Option<(usize, f64)>,
    /// When the rehearsal began.
    start: f64,
    /// The last show clock seen.
    last: f64,
    pub fillers: usize,
    /// `DECKRT_CAPTIONS_TEST`: lines from a text file instead of the microphone.
    fake: Option<(Vec<String>, usize, f64)>,
    /// The words of each slide, for the read-out check and the prompt.
    texts: Vec<String>,
    prompts: Vec<String>,
    deck: Option<std::path::PathBuf>,
    titles: Vec<String>,
}

/// The rehearsal written up, for the report.
pub struct Done {
    live: Live,
    /// The dialog's clock when the show ended.
    ended: Option<f64>,
    pub report: Option<Report>,
    /// The row opened to show what was said.
    open: Option<usize>,
    saved: Option<String>,
    kept: bool,
    noted: bool,
}

fn model_ready(app: &mut SlideApp) -> bool {
    let dir = app.deckrt.settings_dir.clone();
    app.deckrt.captions.load_pub(dir.as_deref());
    let m = app.deckrt.captions.settings.model;
    std::env::var_os("DECKRT_CAPTIONS_TEST").is_some()
        || dir.as_deref().is_some_and(|d| m.ready(d) || deckcraft_captions::Model::ALL.iter().any(|x| x.ready(d)))
}

/// The button on the Slide Show tab.
pub fn ribbon_button(app: &mut SlideApp, ui: &mut Ui) {
    let en = app.session.active().is_some_and(|d| !d.doc.slides.is_empty());
    if crate::widgets::big_button(ui, crate::icons::Icon::Coach, "Rehearse\nwith Coach", en)
        .on_hover_text(
            "Rehearse out loud from the first slide. Afterwards: time per slide, pace, filler words, long pauses, slides read out word for word",
        )
        .clicked()
    {
        if !model_ready(app) {
            app.set_status("Rehearse with Coach listens with the speech model. Download one here first.");
            app.dialog = Some(crate::dialogs::Dialog::new(super::captions::DIALOG));
            return;
        }
        app.deckrt.coach = None;
        app.start_show(0, false);
        if let Some(s) = app.show.as_mut() {
            s.ext.coach_wanted = true;
        }
    }
}

/// A slide's own words: title and body, no notes.
fn slide_text(doc: &deckcraft_model::Presentation, idx: usize) -> String {
    let Some(sl) = doc.slides.get(idx) else { return String::new() };
    let mut out = vec![];
    deckcraft_model::walk(&sl.shapes, &mut |sh, _| {
        if let Some(t) = &sh.text {
            out.push(t.text());
        }
    });
    out.join("\n")
}

/// Once a frame from the show's logic.
pub fn logic(app: &mut SlideApp, show: &mut Show, ctx: &egui::Context, doc: &deckcraft_model::Presentation, now: f64) {
    if show.ext.coach_wanted && show.ext.coach.is_none() {
        show.ext.coach_wanted = false;
        let dir = app.deckrt.settings_dir.clone();
        app.deckrt.captions.load_pub(dir.as_deref());
        let mut settings = app.deckrt.captions.settings.clone();
        settings.english = false;
        let fake = std::env::var_os("DECKRT_CAPTIONS_TEST")
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| (t.lines().map(str::to_string).filter(|l| !l.trim().is_empty()).collect::<Vec<_>>(), 0, now));
        let (captioner, status) = if fake.is_some() {
            (None, Status::Listening("test lines".into()))
        } else {
            let model = dir.as_deref().and_then(|d| {
                if settings.model.ready(d) { Some(settings.model) } else { deckcraft_captions::Model::ALL.into_iter().find(|m| m.ready(d)) }
            });
            match (dir.as_deref(), model) {
                (Some(d), Some(m)) => {
                    settings.model = m;
                    (Some(deckcraft_captions::Captioner::start(m.path(d), settings)), Status::Loading)
                }
                _ => (None, Status::Failed("The speech model isn't on this laptop. Download it in Slide Show › Live Captions.".into())),
            }
        };
        let n = doc.slides.len();
        let texts: Vec<String> = (0..n).map(|i| slide_text(doc, i)).collect();
        let prompts = (0..n).map(|i| format!("{FILLER_PROMPT} {}", deckcraft_captions::prompt_from(&super::captions::slide_texts(doc, i)))).collect();
        let titles = doc.slides.iter().map(|s| s.title()).collect();
        let deck = app.session.active().and_then(|d| d.path.clone()).map(std::path::PathBuf::from);
        show.ext.coach = Some(Live {
            captioner,
            status,
            t0: now,
            heard: vec![],
            spans: vec![],
            cur: None,
            start: now,
            last: now,
            fillers: 0,
            fake,
            texts,
            prompts,
            deck,
            titles,
        });
    }
    let Some(live) = show.ext.coach.as_mut() else { return };
    live.last = now;
    // The slide on screen.
    let on = (!show.ended).then_some(show.state.slide);
    if live.cur.map(|c| c.0) != on {
        if let Some((s, from)) = live.cur.take() {
            live.spans.push(Span { slide: s, from, to: now });
        }
        if let Some(s) = on {
            live.cur = Some((s, now));
            if let (Some(c), Some(p)) = (&live.captioner, live.prompts.get(s)) {
                c.set_prompt(p.clone());
            }
        }
    }
    live.take_events();
    if let Some((lines, next, at)) = live.fake.as_mut()
        && now - *at >= 2.5
        && !lines.is_empty()
        && *next < lines.len()
    {
        *at = now;
        let text = lines[*next].clone();
        *next += 1;
        live.fillers += coach::fillers(&text).len();
        live.heard.push(Heard { text, start: now - 2.2, end: now - 0.3 });
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(250));
}

impl Live {
    fn take_events(&mut self) {
        let Some(c) = &self.captioner else { return };
        for e in c.poll() {
            match e {
                deckcraft_captions::Event::Ready { microphone } => self.status = Status::Listening(microphone),
                deckcraft_captions::Event::Line { text, start, end } => {
                    self.fillers += coach::fillers(&text).len();
                    self.heard.push(Heard { text, start: self.t0 + start, end: self.t0 + end });
                }
                deckcraft_captions::Event::Partial(_) => {}
                deckcraft_captions::Event::Error(e) => self.status = Status::Failed(e),
            }
        }
    }

    fn close(&mut self) {
        if let Some((s, from)) = self.cur.take() {
            self.spans.push(Span { slide: s, from, to: self.last });
        }
    }
}

/// The rehearsal clock on the audience screen.
pub fn draw(painter: &egui::Painter, slide: Rect, live: Option<&Live>) {
    let Some(live) = live else { return };
    let on_slide = live.cur.map(|c| live.last - c.1).unwrap_or(0.0);
    let total = live.last - live.start;
    let mut text = format!("Slide {}   Total {}   Fillers {}", coach::clock(on_slide), coach::clock(total), live.fillers);
    match &live.status {
        Status::Loading => text.push_str("   Loading the speech model"),
        Status::Failed(e) => text = format!("{text}   {e}"),
        Status::Listening(_) => {}
    }
    let g = painter.layout_no_wrap(text, FontId::proportional(14.0), Color32::WHITE);
    let r = Rect::from_min_size(slide.min + vec2(12.0, 12.0), g.size() + vec2(20.0, 12.0));
    painter.rect_filled(r, CornerRadius::same(6), Color32::from_black_alpha(170));
    painter.galley(r.min + vec2(10.0, 6.0), g, Color32::WHITE);
    let _ = Align2::LEFT_TOP;
}

/// Presenter view's toolbar: the coach listening, or the pace against the kept rehearsal.
pub fn presenter_status(ui: &mut Ui, show: &Show) {
    if let Some(live) = &show.ext.coach {
        let (t, col) = match &live.status {
            Status::Loading => ("Coach: loading the speech model".to_string(), Color32::from_gray(170)),
            Status::Listening(m) => (format!("Coach listening on {m} · fillers {}", live.fillers), Color32::from_gray(190)),
            Status::Failed(e) => (format!("Coach: {e}"), Color32::from_rgb(0xff, 0x8a, 0x80)),
        };
        ui.separator();
        ui.label(RichText::new(t).size(12.0).color(col));
    } else if let Some((t, behind)) = &show.ext.pace {
        ui.separator();
        let col = if *behind { Color32::from_rgb(0xff, 0xb6, 0x4d) } else { Color32::from_gray(190) };
        ui.label(RichText::new(t).size(12.0).color(col)).on_hover_text("Against the pace kept from Rehearse with Coach");
    }
}

/// The pace line for presenter view: how far the talk is from the kept rehearsal.
pub fn pace_line(doc: &deckcraft_model::Presentation, slide: usize, on_slide: f64, elapsed: f64) -> Option<(String, bool)> {
    let paces: Vec<Option<f64>> = doc.slides.iter().map(|s| deckcraft_engine::cmd::deckrt_coach::pace_of(s)).collect();
    if paces.iter().all(Option::is_none) {
        return None;
    }
    let before: f64 = doc.slides.iter().zip(&paces).take(slide).filter(|(s, _)| !s.hidden).filter_map(|(_, p)| *p).sum();
    let here = paces.get(slide).copied().flatten().unwrap_or(0.0);
    let behind = elapsed - (before + on_slide.min(here));
    let this = if here > 0.0 { format!(" · this slide {} of {}", coach::clock(on_slide), coach::clock(here)) } else { String::new() };
    Some(if behind.abs() < 20.0 {
        (format!("On your rehearsal pace{this}"), false)
    } else if behind > 0.0 {
        (format!("{} behind your rehearsal{this}", coach::clock(behind)), true)
    } else {
        (format!("{} ahead of your rehearsal{this}", coach::clock(-behind)), false)
    })
}

/// When the show ends: keep the coach for the report and open it.
pub fn end_show(app: &mut SlideApp, show: &mut Show) {
    if let Some(mut live) = show.ext.coach.take() {
        live.close();
        app.deckrt.coach = Some(Done { live, ended: None, report: None, open: None, saved: None, kept: false, noted: false });
        app.dialog = Some(crate::dialogs::Dialog::new(DIALOG));
    }
}

pub fn title() -> &'static str {
    "Rehearsal"
}

/// Once a frame: a rehearsal that's over keeps the microphone only while its report is being
/// written in the open dialog. Closed early (Esc, the window's ×), the microphone goes off and the
/// report is written from what was heard.
pub fn release_if_closed(app: &mut SlideApp) {
    let open = app.dialog.as_ref().is_some_and(|d| d.id == DIALOG);
    if open || app.show.is_some() {
        return;
    }
    let Some(done) = app.deckrt.coach.as_mut() else { return };
    if done.ended.is_some() && done.live.captioner.is_some() {
        done.live.take_events();
        done.live.captioner = None;
        if done.report.is_none() {
            done.report = Some(coach::analyse(&done.live.heard, &done.live.spans, &done.live.texts));
        }
    }
}

/// The report. Returns whether the dialog closes.
pub fn body(app: &mut SlideApp, ui: &mut Ui) -> bool {
    let now = ui.input(|i| i.time);
    let Some(done) = app.deckrt.coach.as_mut() else { return true };
    ui.set_min_width(620.0);
    ui.set_max_width(680.0);
    // Let the recogniser finish the last phrase, then write it up.
    if done.report.is_none() {
        let ended = *done.ended.get_or_insert(now);
        done.live.take_events();
        let waiting = done.live.captioner.is_some() && now - ended < FINISH_SECS;
        if waiting {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Writing up the last few seconds…");
            });
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
            return false;
        }
        done.live.captioner = None;
        done.report = Some(coach::analyse(&done.live.heard, &done.live.spans, &done.live.texts));
    }
    let Some(report) = done.report.clone() else { return true };
    if report.slides.is_empty() || report.total < 1.0 {
        ui.label("The rehearsal ended before it began.");
        return close_row(ui);
    }
    if let Status::Failed(e) = &done.live.status {
        ui.label(RichText::new(e).color(Color32::from_rgb(0xd8, 0x3f, 0x3f)));
        ui.add_space(4.0);
    }
    for line in summary(&report, &done.live.titles) {
        ui.label(line);
    }
    ui.add_space(8.0);
    table(ui, &report, &done.live.titles, &mut done.open);
    if let Some(i) = done.open
        && let Some(r) = report.slides.get(i)
    {
        ui.add_space(6.0);
        ui.label(RichText::new(format!("What you said on slide {}", r.slide + 1)).strong());
        egui::ScrollArea::vertical().id_salt("coach-said").max_height(120.0).show(ui, |ui| {
            if r.said.is_empty() {
                ui.label(RichText::new("Nothing was heard.").color(Color32::from_gray(140)));
            } else {
                ui.label(r.said.join(" "));
            }
        });
    }
    if let Some(s) = &done.saved {
        ui.add_space(4.0);
        ui.label(RichText::new(s).size(12.0).color(Color32::from_gray(130)));
    }
    ui.add_space(10.0);
    let mut close = false;
    let mut keep = false;
    let mut save = false;
    let mut note = false;
    ui.horizontal(|ui| {
        let label = if done.kept { "Pace kept" } else { "Keep this pace" };
        if ui
            .add_enabled(!done.kept, super::check::framed(ui, label, 0.0))
            .on_hover_text("Presenter view then shows how far ahead or behind your rehearsal you are. Saved with the deck.")
            .clicked()
        {
            keep = true;
        }
        if ui.add(super::check::framed(ui, "Save report", 0.0)).on_hover_text("A text file next to the deck").clicked() {
            save = true;
        }
        let label = if done.noted { "Added to notes" } else { "Add to notes" };
        if ui
            .add_enabled(!done.noted && report.slides.iter().any(|s| !s.said.is_empty()), super::check::framed(ui, label, 0.0))
            .on_hover_text("What you said on each slide goes after its speaker notes")
            .clicked()
        {
            note = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(super::check::framed(ui, "Close", 72.0)).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
    });
    if keep {
        let slides: Vec<serde_json::Value> = report.slides.iter().map(|r| json!({"index": r.slide, "secs": r.secs})).collect();
        match app.session.execute("deckrt.keepPace", &json!({"slides": slides})) {
            Ok(_) => {
                if let Some(d) = app.deckrt.coach.as_mut() {
                    d.kept = true;
                }
                app.set_status("Pace kept. Save the deck to keep it for the talk.");
            }
            Err(e) => app.set_status(e.to_string()),
        }
        app.drain_requests();
    }
    if note {
        let slides: Vec<serde_json::Value> =
            report.slides.iter().filter(|r| !r.said.is_empty()).map(|r| json!({"index": r.slide, "text": r.said.join(" ")})).collect();
        match app.session.execute("deckrt.addNotes", &json!({"slides": slides})) {
            Ok(r) => {
                if let Some(d) = app.deckrt.coach.as_mut() {
                    d.noted = true;
                }
                let n = r["slides"].as_u64().unwrap_or(0);
                app.set_status(if n == 1 { "Added to the notes of 1 slide.".to_string() } else { format!("Added to the notes of {n} slides.") });
            }
            Err(e) => app.set_status(e.to_string()),
        }
        app.drain_requests();
    }
    if save && let Some(d) = app.deckrt.coach.as_mut() {
        d.saved = Some(match save_report(&report, &d.live) {
            Ok(p) => format!("Saved to {}", p.display()),
            Err(e) => e,
        });
    }
    if close {
        app.deckrt.coach = None;
    }
    close
}

fn close_row(ui: &mut Ui) -> bool {
    ui.add_space(8.0);
    let mut close = false;
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        close = ui.add(super::check::framed(ui, "Close", 72.0)).clicked();
    });
    close
}

/// The lines above the table.
fn summary(r: &Report, titles: &[String]) -> Vec<String> {
    let mut out = vec![];
    match r.pace() {
        Some(p) => {
            let note = if p < 110.0 {
                " That's slow; 120 to 160 is easy to follow."
            } else if p > 170.0 {
                " That's fast; 120 to 160 is easy to follow."
            } else {
                ""
            };
            out.push(format!("{} in all, {:.0} words a minute.{note}", coach::clock(r.total), p));
        }
        None if r.thai => out.push(format!("{} in all. Pace isn't counted for Thai.", coach::clock(r.total))),
        None => out.push(format!("{} in all.", coach::clock(r.total))),
    }
    let n = r.fillers_total();
    if n == 0 {
        out.push("No filler words heard.".into());
    } else {
        let list: Vec<String> = r.fillers_ranked().iter().map(|(w, k)| format!("{w} {k}")).collect();
        out.push(format!("{n} filler words: {}.", list.join(", ")));
    }
    let read: Vec<String> = r.slides.iter().filter(|s| s.read_aloud()).map(|s| (s.slide + 1).to_string()).collect();
    if !read.is_empty() {
        out.push(format!("Read out word for word: slide{} {}.", if read.len() == 1 { "" } else { "s" }, read.join(", ")));
    }
    if let Some(long) = r.slides.iter().max_by(|a, b| a.secs.partial_cmp(&b.secs).unwrap_or(std::cmp::Ordering::Equal))
        && r.slides.len() > 1
    {
        let t = titles.get(long.slide).map(|t| t.trim()).filter(|t| !t.is_empty()).map(|t| format!(", {}", short(t, 40))).unwrap_or_default();
        out.push(format!("Longest: slide {}{t}, {}.", long.slide + 1, coach::clock(long.secs)));
    }
    out
}

fn short(s: &str, n: usize) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() <= n { s } else { format!("{}…", s.chars().take(n - 1).collect::<String>()) }
}

/// One row per slide. A click opens what was said.
fn table(ui: &mut Ui, r: &Report, titles: &[String], open: &mut Option<usize>) {
    let head = |ui: &mut Ui, t: &str| {
        ui.label(RichText::new(t).size(12.0).color(Color32::from_gray(120)));
    };
    egui::ScrollArea::vertical().id_salt("coach-table").max_height(280.0).show(ui, |ui| {
        egui::Grid::new("coach-grid").num_columns(6).spacing(vec2(14.0, 4.0)).striped(true).show(ui, |ui| {
            for h in ["Slide", "Time", "Words a minute", "Fillers", "Pauses", ""] {
                head(ui, h);
            }
            ui.end_row();
            for (i, s) in r.slides.iter().enumerate() {
                let title = titles.get(s.slide).map(|t| short(t.trim(), 28)).unwrap_or_default();
                let label = format!("{}  {}", s.slide + 1, title);
                if ui.selectable_label(*open == Some(i), label).on_hover_text("What you said").clicked() {
                    *open = if *open == Some(i) { None } else { Some(i) };
                }
                ui.label(coach::clock(s.secs));
                match s.pace() {
                    Some(p) => {
                        let col = if !(110.0..=170.0).contains(&p) { Color32::from_rgb(0xd0, 0x80, 0x20) } else { ui.visuals().text_color() };
                        ui.label(RichText::new(format!("{p:.0}")).color(col));
                    }
                    None => {
                        ui.label(RichText::new("–").color(Color32::from_gray(140)));
                    }
                }
                let f = s.fillers_total();
                ui.label(if f == 0 { "0".to_string() } else { f.to_string() });
                ui.label(match s.pauses.len() {
                    0 => String::new(),
                    1 => format!("1 ({:.0} s)", s.pauses[0]),
                    n => format!("{n} (longest {:.0} s)", s.pauses.iter().cloned().fold(0.0, f64::max)),
                });
                let note = if s.read_aloud() {
                    "Read out the slide"
                } else if s.said.is_empty() && s.secs >= 10.0 {
                    "Nothing heard"
                } else {
                    ""
                };
                ui.label(RichText::new(note).color(Color32::from_rgb(0xd0, 0x80, 0x20)));
                ui.end_row();
            }
        });
    });
}

/// The report as text, next to the deck (or in the settings folder for an unsaved deck).
fn save_report(r: &Report, live: &Live) -> Result<std::path::PathBuf, String> {
    let stamp = chrono::Local::now().format("%Y-%m-%d %H%M").to_string();
    let (dir, stem) = match &live.deck {
        Some(p) => (
            p.parent().map(std::path::Path::to_path_buf).unwrap_or_default(),
            p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Deck".into()),
        ),
        None => (dirs_home(), "Deck".into()),
    };
    let path = dir.join(format!("{stem} rehearsal {stamp}.txt"));
    let mut out = format!("Rehearsal of {stem}, {stamp}\n\n");
    for l in summary(r, &live.titles) {
        out.push_str(&l);
        out.push('\n');
    }
    for s in &r.slides {
        let title = live.titles.get(s.slide).map(|t| short(t.trim(), 60)).unwrap_or_default();
        out.push_str(&format!("\nSlide {}: {title}\n", s.slide + 1));
        out.push_str(&format!("Time {}", coach::clock(s.secs)));
        if let Some(p) = s.pace() {
            out.push_str(&format!(", {p:.0} words a minute"));
        }
        if s.fillers_total() > 0 {
            let f: Vec<String> = s.fillers.iter().map(|(w, n)| format!("{w} {n}")).collect();
            out.push_str(&format!(", fillers: {}", f.join(", ")));
        }
        if !s.pauses.is_empty() {
            let p: Vec<String> = s.pauses.iter().map(|p| format!("{p:.0} s")).collect();
            out.push_str(&format!(", pauses: {}", p.join(", ")));
        }
        if s.read_aloud() {
            out.push_str(", read out the slide");
        }
        out.push('\n');
        if !s.said.is_empty() {
            out.push_str(&s.said.join(" "));
            out.push('\n');
        }
    }
    std::fs::write(&path, out).map_err(|e| format!("Couldn't save the report: {e}"))?;
    Ok(path)
}

fn dirs_home() -> std::path::PathBuf {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(std::path::PathBuf::from).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pace_line_says_ahead_or_behind() {
        let mut doc = deckcraft_model::Presentation::default();
        doc.slides.clear();
        assert_eq!(pace_line(&doc, 0, 0.0, 0.0), None);
        for secs in [Some("60"), Some("120"), None] {
            let mut s = deckcraft_model::Slide::default();
            if let Some(v) = secs {
                s.tags.push((deckcraft_engine::cmd::deckrt_coach::PACE_TAG.into(), v.into()));
            }
            doc.slides.push(std::sync::Arc::new(s));
        }
        // On slide 2, 30 s in, 100 s since the start: 70 s on slide 1 against 60.
        let (t, behind) = pace_line(&doc, 1, 30.0, 100.0).unwrap_or_default();
        assert!(t.starts_with("On your rehearsal pace"), "{t}");
        assert!(!behind);
        let (t, behind) = pace_line(&doc, 1, 30.0, 150.0).unwrap_or_default();
        assert!(t.starts_with("1:00 behind"), "{t}");
        assert!(behind);
        let (t, _) = pace_line(&doc, 2, 5.0, 120.0).unwrap_or_default();
        assert!(t.starts_with("1:00 ahead"), "{t}");
    }
}
