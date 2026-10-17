//! deckrt: live captions in the show. The Captions dialog on the Slide Show tab holds the
//! settings and the one-time model download; during a show the microphone is turned into lines
//! at the bottom of the audience screen and on the phones.

use std::path::{Path, PathBuf};

use deckcraft_captions::{Download, Model, Settings};
use egui::{Align2, Color32, CornerRadius, FontId, Rect, RichText, Ui, pos2, vec2};

use crate::SlideApp;
use crate::show::Show;

pub const DIALOG: &str = "deckrt.captions";
const SETTINGS_FILE: &str = "captions.json";

/// What the app keeps between shows.
#[derive(Default)]
pub struct Prefs {
    pub settings: Settings,
    /// Captions start with every show.
    pub on: bool,
    pub download: Option<(Model, Download)>,
    pub download_error: Option<String>,
    loaded: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
struct Saved {
    settings: Settings,
    on: bool,
}

impl Prefs {
    /// Read the saved settings once.
    pub fn load_pub(&mut self, dir: Option<&Path>) {
        self.load(dir);
    }

    fn load(&mut self, dir: Option<&Path>) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        if let Some(d) = dir
            && let Ok(t) = std::fs::read_to_string(d.join(SETTINGS_FILE))
            && let Ok(s) = serde_json::from_str::<Saved>(&t)
        {
            self.settings = s.settings;
            self.on = s.on;
        }
    }

    fn save(&self, dir: Option<&Path>) {
        let Some(d) = dir else { return };
        let _ = std::fs::create_dir_all(d);
        if let Ok(t) = serde_json::to_string_pretty(&Saved { settings: self.settings.clone(), on: self.on }) {
            let _ = std::fs::write(d.join(SETTINGS_FILE), t);
        }
    }
}

/// How captions are doing in this show, for presenter view.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Loading,
    Listening(String),
    Failed(String),
}

/// Captions while a show runs.
pub struct Live {
    captioner: Option<deckcraft_captions::Captioner>,
    pub status: Status,
    /// The last finished lines, newest last.
    pub lines: Vec<String>,
    /// The phrase being spoken.
    pub partial: Option<String>,
    /// When the last words came; the band fades out after a quiet spell.
    pub last_words: f64,
    /// Hidden from the screen and the phones by the presenter.
    pub paused: bool,
    slide: Option<usize>,
    on_screen: bool,
    on_phones: bool,
    /// `DECKRT_CAPTIONS_TEST`: lines from a text file instead of the microphone, for testing.
    fake: Option<(Vec<String>, usize, f64)>,
}

impl Live {
    /// The text the screen and phones show now.
    pub fn text(&self, now: f64) -> Option<String> {
        if self.paused || now - self.last_words > 8.0 {
            return None;
        }
        let mut parts: Vec<&str> = self.lines.iter().rev().take(if self.partial.is_some() { 1 } else { 2 }).rev().map(String::as_str).collect();
        if let Some(p) = &self.partial {
            parts.push(p);
        }
        let t = parts.join(" ");
        (!t.trim().is_empty()).then_some(t)
    }

    pub fn for_phones(&self, now: f64) -> Option<String> {
        if self.on_phones { self.text(now) } else { None }
    }
}

fn settings_dir(app: &SlideApp) -> Option<PathBuf> {
    app.deckrt.settings_dir.clone()
}

/// Once a frame from the show's logic: start the captioner when the show begins, read what it
/// heard, follow the slide for the recogniser's word list.
pub fn logic(app: &mut SlideApp, show: &mut Show, ctx: &egui::Context, doc: &deckcraft_model::Presentation, now: f64) {
    let dir = settings_dir(app);
    app.deckrt.captions.load(dir.as_deref());
    // One microphone at a time: a rehearsal with the coach has it.
    if show.reading || show.preview_only || show.ext.coach.is_some() || show.ext.coach_wanted {
        return;
    }
    if show.ext.captions.is_none() && !show.ext.captions_tried && app.deckrt.captions.on {
        show.ext.captions_tried = true;
        let s = app.deckrt.captions.settings.clone();
        let fake = std::env::var_os("DECKRT_CAPTIONS_TEST")
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| (t.lines().map(str::to_string).filter(|l| !l.trim().is_empty()).collect::<Vec<_>>(), 0, now));
        let (captioner, status) = if fake.is_some() {
            (None, Status::Listening("test lines".into()))
        } else {
            match dir.as_deref() {
                Some(d) if s.model.ready(d) => (Some(deckcraft_captions::Captioner::start(s.model.path(d), s.clone())), Status::Loading),
                _ => (None, Status::Failed("The speech model isn't on this laptop yet. Download it in Slide Show › Captions.".into())),
            }
        };
        show.ext.captions = Some(Live {
            captioner,
            status,
            lines: vec![],
            partial: None,
            last_words: f64::MIN,
            paused: false,
            slide: None,
            on_screen: s.on_screen,
            on_phones: s.on_phones,
            fake,
        });
    }
    let Some(live) = show.ext.captions.as_mut() else { return };
    let idx = show.state.slide;
    let mut changed = false;
    if live.slide != Some(idx) {
        live.slide = Some(idx);
        if let Some(c) = &live.captioner {
            c.set_prompt(deckcraft_captions::prompt_from(&slide_texts(doc, idx)));
        }
    }
    if let Some(c) = &live.captioner {
        for e in c.poll() {
            changed = true;
            match e {
                deckcraft_captions::Event::Ready { microphone } => live.status = Status::Listening(microphone),
                deckcraft_captions::Event::Partial(t) => {
                    live.partial = Some(t);
                    live.last_words = now;
                }
                deckcraft_captions::Event::Line { text: t, .. } => {
                    live.lines.push(t);
                    if live.lines.len() > 20 {
                        live.lines.remove(0);
                    }
                    live.partial = None;
                    live.last_words = now;
                }
                deckcraft_captions::Event::Error(e) => live.status = Status::Failed(e),
            }
        }
    }
    if let Some((lines, next, at)) = live.fake.as_mut()
        && now - *at >= 1.6
        && !lines.is_empty()
    {
        *at = now;
        live.lines.push(lines[*next % lines.len()].clone());
        *next += 1;
        live.partial = None;
        live.last_words = now;
        changed = true;
    }
    let for_phones = live.for_phones(now);
    if let Some(ph) = show.ext.phones.as_mut() {
        ph.set_caption(for_phones);
    }
    if changed {
        ctx.request_repaint();
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(300));
}

/// The slide's own words: title, body text and notes.
pub(crate) fn slide_texts(doc: &deckcraft_model::Presentation, idx: usize) -> Vec<String> {
    let Some(sl) = doc.slides.get(idx) else { return vec![] };
    let mut out = vec![sl.title()];
    deckcraft_model::walk(&sl.shapes, &mut |sh, _| {
        if let Some(t) = &sh.text {
            out.push(t.text());
        }
    });
    out
}

/// The caption band at the bottom of the audience screen. Returns its height (0 when nothing
/// shows), so a question band can sit above it.
pub fn draw(painter: &egui::Painter, slide: Rect, live: Option<&Live>, now: f64) -> f32 {
    let Some(live) = live.filter(|l| l.on_screen) else { return 0.0 };
    let Some(text) = live.text(now) else { return 0.0 };
    let size = (slide.height() * 0.042).clamp(16.0, 52.0);
    let width = slide.width() * 0.9;
    let galley = painter.layout(text, FontId::proportional(size), Color32::WHITE, width - size);
    // Two lines at most: the newest words stay, the start of a long phrase scrolls away.
    let rows = galley.rows.len();
    let line_h = galley.size().y / rows.max(1) as f32;
    let show_h = line_h * rows.min(2) as f32;
    let pad = size * 0.4;
    let h = show_h + pad * 2.0;
    let rect = Rect::from_center_size(pos2(slide.center().x, slide.max.y - h / 2.0 - slide.height() * 0.02), vec2(width, h));
    painter.rect_filled(rect, CornerRadius::same(8), Color32::from_black_alpha(200));
    let clip = rect.shrink2(vec2(pad, pad));
    let p = painter.with_clip_rect(clip);
    let y = clip.max.y - galley.size().y;
    p.galley(pos2(clip.min.x, y), galley, Color32::WHITE);
    h + slide.height() * 0.02
}

/// Presenter view: the captions button and how they are doing.
pub fn presenter_controls(ui: &mut Ui, show: &mut Show) {
    let Some(live) = show.ext.captions.as_mut() else { return };
    let label = if live.paused { "Captions off" } else { "Captions on" };
    if ui.add(egui::Button::new(label).selected(!live.paused)).on_hover_text("Hide or show the captions on the screen and the phones").clicked() {
        live.paused = !live.paused;
    }
    let (text, col) = match &live.status {
        Status::Loading => ("Loading the speech model".to_string(), Color32::from_gray(170)),
        Status::Listening(m) => (format!("Listening on {m}"), Color32::from_gray(170)),
        Status::Failed(e) => (e.clone(), Color32::from_rgb(0xff, 0x8a, 0x80)),
    };
    ui.label(RichText::new(text).size(12.0).color(col));
}

/// The Captions button on the Slide Show tab.
pub fn ribbon_button(app: &mut SlideApp, ui: &mut Ui) {
    let on = app.deckrt.captions.on;
    if crate::widgets::big_button_ex(ui, crate::icons::Icon::Subtitles, "Live\nCaptions", true, false, on)
        .0
        .on_hover_text("Captions from the microphone, worked out on this laptop")
        .clicked()
    {
        let dir = settings_dir(app);
        app.deckrt.captions.load(dir.as_deref());
        app.dialog = Some(crate::dialogs::Dialog::new(DIALOG));
    }
}

pub fn title() -> &'static str {
    "Live captions"
}

/// The dialog. Returns whether it closes.
pub fn body(app: &mut SlideApp, ui: &mut Ui) -> bool {
    let dir = settings_dir(app);
    let prefs = &mut app.deckrt.captions;
    prefs.load(dir.as_deref());
    ui.set_min_width(460.0);
    ui.set_max_width(520.0);
    let mut changed = false;
    ui.label("Captions are worked out on this laptop from the microphone. Nothing is sent online.");
    ui.add_space(8.0);

    ui.label(RichText::new("Speech model").strong());
    // Finish a download that ended.
    if let Some((m, d)) = prefs.download.as_mut()
        && let Some(r) = d.poll()
    {
        let m = *m;
        prefs.download = None;
        match r {
            Ok(_) => {
                prefs.settings.model = m;
                prefs.download_error = None;
                changed = true;
            }
            Err(e) => prefs.download_error = Some(e),
        }
    }
    for m in Model::ALL {
        ui.horizontal(|ui| {
            let ready = dir.as_deref().is_some_and(|d| m.ready(d));
            if ui.radio(prefs.settings.model == m, m.label()).clicked() && prefs.settings.model != m {
                prefs.settings.model = m;
                changed = true;
            }
            if ready {
                ui.label(RichText::new("On this laptop").size(12.0).color(Color32::from_rgb(0x2e, 0xa0, 0x5c)));
            } else if prefs.download.as_ref().is_some_and(|(dm, _)| *dm == m) {
                let p = prefs.download.as_ref().map(|(_, d)| d.progress()).unwrap_or(0.0);
                ui.add(egui::ProgressBar::new(p).desired_width(140.0).show_percentage());
                if ui.add(super::check::framed(ui, "Cancel", 0.0)).clicked()
                    && let Some((_, d)) = prefs.download.take()
                {
                    d.cancel();
                }
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
            } else if prefs.download.is_none() && ui.add(super::check::framed(ui, "Download", 0.0)).clicked() {
                match dir.as_deref().map(|d| Download::start(m, d)) {
                    Some(Ok(d)) => {
                        prefs.download = Some((m, d));
                        prefs.download_error = None;
                    }
                    Some(Err(e)) => prefs.download_error = Some(e),
                    None => prefs.download_error = Some("There is no settings folder to keep the model in.".into()),
                }
            }
        });
    }
    if let Some(e) = &prefs.download_error {
        ui.label(RichText::new(e).size(12.0).color(Color32::from_rgb(0xd8, 0x3f, 0x3f)));
    }
    ui.add_space(8.0);
    ui.label(RichText::new("Language spoken").strong());
    ui.horizontal(|ui| {
        for (id, label) in [("auto", "Detect"), ("en", "English"), ("th", "Thai")] {
            if ui.radio(prefs.settings.language == id, label).clicked() && prefs.settings.language != id {
                prefs.settings.language = id.to_string();
                changed = true;
            }
        }
    });
    changed |= ui.checkbox(&mut prefs.settings.english, "Write the captions in English").changed();
    ui.add_space(6.0);
    changed |= ui.checkbox(&mut prefs.settings.on_screen, "Show captions on the screen").changed();
    changed |= ui.checkbox(&mut prefs.settings.on_phones, "Send captions to phones").changed();
    ui.add_space(6.0);
    let ready = dir.as_deref().is_some_and(|d| prefs.settings.model.ready(d));
    let mut on = prefs.on;
    if ui.add_enabled(ready || prefs.on, egui::Checkbox::new(&mut on, "Turn captions on when a show starts")).changed() {
        prefs.on = on;
        changed = true;
    }
    if !ready {
        ui.label(RichText::new("Download the model first. It is needed once.").size(12.0).color(Color32::from_gray(140)));
    }
    if changed {
        prefs.save(dir.as_deref());
    }
    ui.add_space(10.0);
    let mut close = false;
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 26.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if ui.add(super::check::framed(ui, "Close", 72.0)).clicked() {
            close = true;
        }
    });
    let _ = Align2::LEFT_TOP;
    close
}
