//! deckrt: Record. The show runs with the microphone on; when it ends, each slide gets what was
//! said while it was up as its narration (an audio clip that plays as the slide comes in), and
//! advances after that long. Slides passed in under half a second are left as they were.

use egui::{Color32, CornerRadius, RichText, Ui, vec2};
use serde_json::json;

use crate::SlideApp;
use crate::show::Show;

/// A slide shown for less than this is passed over, not recorded.
const MIN_SECS: f64 = 0.5;

/// The recording while the show runs.
pub struct Live {
    rec: deckcraft_captions::Recorder,
    /// (slide, from sample, to sample), in the order shown.
    spans: Vec<(usize, usize, usize)>,
    cur: Option<(usize, usize)>,
    /// The loudest the microphone has been, to notice a silent one.
    loudest: f32,
}

/// Recording or the coach is on: the slides' own timings and narration wait.
pub fn active(show: &Show) -> bool {
    show.ext.record.is_some() || show.ext.record_wanted || show.ext.coach.is_some() || show.ext.coach_wanted
}

/// A clip may start by itself: narration doesn't while recording or with Play Narrations off.
pub fn may_play(show: &Show, doc: &deckcraft_model::Presentation, id: deckcraft_model::ShapeId) -> bool {
    let narration = doc.slides.iter().any(|sl| sl.shapes.iter().any(|sh| sh.id == id && deckcraft_engine::cmd::deckrt_record::is_narration(sh)));
    !(narration && (doc.show.without_narration || active(show)))
}

fn start(app: &mut SlideApp, from: usize) {
    app.start_show(from, false);
    if let Some(s) = app.show.as_mut() {
        s.ext.record_wanted = true;
    }
}

fn current(app: &SlideApp) -> usize {
    app.session.active().map(|d| d.selection.slide).unwrap_or(0)
}

fn clear_menu(app: &mut SlideApp, ui: &mut Ui) {
    for (label, scope, timings) in [
        ("Clear Narration on Current Slide", "slide", false),
        ("Clear Narration on All Slides", "all", false),
        ("Clear Timings and Narration on All Slides", "all", true),
    ] {
        if ui.button(label).clicked() {
            match app.session.execute("deckrt.clearNarration", &json!({"scope": scope, "timings": timings})) {
                Ok(r) => {
                    let n = r["slides"].as_u64().unwrap_or(0);
                    app.set_status(match n {
                        0 if timings => "Timings cleared.".to_string(),
                        0 => "No narration to clear.".to_string(),
                        1 => "Narration cleared on 1 slide.".to_string(),
                        n => format!("Narration cleared on {n} slides."),
                    });
                }
                Err(e) => app.set_status(e.to_string()),
            }
            app.drain_requests();
            ui.close();
        }
    }
}

/// The Record tab. Takes the place of upstream's group.
pub fn ribbon(app: &mut SlideApp, ui: &mut Ui) {
    let en = app.session.active().is_some_and(|d| !d.doc.slides.is_empty());
    crate::widgets::group_named(ui, "Record", |ui| {
        if crate::widgets::big_button(ui, crate::icons::Icon::PlayFromStart, "Record from\nBeginning", en)
            .on_hover_text("Present from the first slide with the microphone on. Each slide keeps what you say while it's up.")
            .clicked()
        {
            start(app, 0);
        }
        if crate::widgets::big_button(ui, crate::icons::Icon::PlayFromCurrent, "Record from\nCurrent Slide", en)
            .on_hover_text("Record from this slide on. Slides before it keep their narration.")
            .clicked()
        {
            let i = current(app);
            start(app, i);
        }
        let (r, _) = crate::widgets::big_button_ex(ui, crate::icons::Icon::Eraser, "Clear", en, false, false);
        egui::Popup::menu(&r).width(300.0).show(|ui| clear_menu(app, ui));
    });
    crate::widgets::group_named(ui, "Settings", |ui| {
        crate::widgets::rows(ui, |ui| {
            let mut narr = app.session.active().is_none_or(|d| !d.doc.show.without_narration);
            if ui.checkbox(&mut narr, "Play Narrations").changed() {
                let _ = app.session.execute("show.setup", &json!({"noNarration": !narr}));
                app.drain_requests();
            }
            let mut use_t = app.session.active().is_none_or(|d| d.doc.show.use_timings);
            if ui.checkbox(&mut use_t, "Use Timings").changed() {
                let _ = app.session.execute("show.setup", &json!({"useTimings": use_t}));
                app.drain_requests();
            }
        });
    });
    super::video::ribbon_group(app, ui);
}

/// The Record button on the Slide Show tab: from the current slide, or the menu.
pub fn slide_show_button(app: &mut SlideApp, ui: &mut Ui) {
    let en = app.session.active().is_some_and(|d| !d.doc.slides.is_empty());
    let (main, arrow) = crate::widgets::big_button_ex(ui, crate::icons::Icon::Record, "Record", en, true, false);
    if main.on_hover_text("Record your voice from the current slide").clicked() {
        let i = current(app);
        start(app, i);
    }
    if let Some(a) = arrow {
        egui::Popup::menu(&a).width(300.0).show(|ui| {
            if ui.button("Record from Beginning").clicked() {
                start(app, 0);
                ui.close();
            }
            if ui.button("Record from Current Slide").clicked() {
                let i = current(app);
                start(app, i);
                ui.close();
            }
            ui.separator();
            clear_menu(app, ui);
        });
    }
}

/// Once a frame from the show's logic.
pub fn logic(show: &mut Show, ctx: &egui::Context) {
    if show.ext.record_wanted && show.ext.record.is_none() {
        show.ext.record_wanted = false;
        show.ext.record = Some(Live { rec: deckcraft_captions::Recorder::start(), spans: vec![], cur: None, loudest: 0.0 });
    }
    let Some(live) = show.ext.record.as_mut() else { return };
    live.rec.poll();
    live.loudest = live.loudest.max(live.rec.level());
    let at = live.rec.len();
    let on = (!show.ended).then_some(show.state.slide);
    if live.cur.map(|c| c.0) != on {
        if let Some((s, from)) = live.cur.take() {
            live.spans.push((s, from, at));
        }
        if let Some(s) = on {
            live.cur = Some((s, at));
        }
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
}

/// Presenter view's toolbar while recording: the clock, the level, Pause.
pub fn presenter_controls(ui: &mut Ui, show: &mut Show) {
    let Some(live) = show.ext.record.as_mut() else { return };
    ui.separator();
    let paused = live.rec.paused();
    let (dot, text) = if paused { (Color32::from_gray(150), "Paused") } else { (Color32::from_rgb(0xe5, 0x39, 0x35), "Recording") };
    let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), egui::Sense::hover());
    ui.painter().circle_filled(r.center(), 5.5, dot);
    ui.label(RichText::new(format!("{text} {}", deckcraft_captions::coach::clock(live.rec.secs()))).size(12.0).color(Color32::from_gray(220)));
    // The level meter.
    let (m, _) = ui.allocate_exact_size(vec2(60.0, 8.0), egui::Sense::hover());
    ui.painter().rect_filled(m, CornerRadius::same(2), Color32::from_gray(60));
    let lvl = (live.rec.level() * 3.0).clamp(0.0, 1.0);
    let mut f = m;
    f.set_width(m.width() * lvl);
    ui.painter().rect_filled(
        f,
        CornerRadius::same(2),
        if lvl > 0.95 { Color32::from_rgb(0xe5, 0x39, 0x35) } else { Color32::from_rgb(0x4c, 0xaf, 0x50) },
    );
    if ui.button(if paused { "Resume" } else { "Pause" }).on_hover_text("Pause stops the recording and the slide's clock").clicked() {
        live.rec.set_paused(!paused);
    }
    match &live.rec.status {
        Some(Err(e)) => {
            ui.label(RichText::new(e).size(12.0).color(Color32::from_rgb(0xff, 0x8a, 0x80)));
        }
        Some(Ok(_)) if live.rec.secs() > 3.0 && live.loudest < 0.002 => {
            ui.label(
                RichText::new("No sound from the microphone. Check Windows Settings › Privacy › Microphone.")
                    .size(12.0)
                    .color(Color32::from_rgb(0xff, 0xb6, 0x4d)),
            );
        }
        _ => {}
    }
}

/// When the show ends: each slide's part of the recording becomes its narration.
pub fn end_show(app: &mut SlideApp, show: &mut Show) {
    let Some(mut live) = show.ext.record.take() else { return };
    live.rec.poll();
    let at = live.rec.len();
    if let Some((s, from)) = live.cur.take() {
        live.spans.push((s, from, at));
    }
    let rate = deckcraft_captions::Recorder::RATE;
    // Every visit to a slide, in order.
    let mut by_slide: std::collections::BTreeMap<usize, Vec<f32>> = std::collections::BTreeMap::new();
    for (s, from, to) in &live.spans {
        by_slide.entry(*s).or_default().extend_from_slice(live.rec.slice(*from, *to));
    }
    let silent = live.loudest < 0.002 || matches!(live.rec.status, Some(Err(_)) | None);
    let dir = std::env::temp_dir();
    let mut files = vec![];
    let mut items = vec![];
    let mut total = 0.0;
    for (s, samples) in by_slide {
        let secs = samples.len() as f64 / rate as f64;
        if secs < MIN_SECS {
            continue;
        }
        total += secs;
        let ms = (secs * 1000.0).round() as u64;
        if silent {
            items.push(json!({"index": s, "ms": ms}));
            continue;
        }
        let path = dir.join(format!("deckrt-narration-{}-{}.wav", std::process::id(), s + 1));
        if std::fs::write(&path, deckcraft_captions::wav(&samples, rate)).is_err() {
            items.push(json!({"index": s, "ms": ms}));
            continue;
        }
        items.push(json!({"index": s, "path": path.to_string_lossy(), "name": format!("Narration slide {}.wav", s + 1), "ms": ms}));
        files.push(path);
    }
    drop(live);
    if items.is_empty() {
        return;
    }
    let n = items.len();
    match app.session.execute("deckrt.setNarration", &json!({"slides": items})) {
        Ok(r) => {
            let voiced = r["slides"].as_u64().unwrap_or(0);
            let t = deckcraft_captions::coach::clock(total);
            app.set_status(if voiced == 0 {
                format!("Timings recorded on {n} slides, {t} in all. The microphone sent no sound.")
            } else {
                format!("Recorded {voiced} slide{}, {t} in all. Save the deck to keep it.", if voiced == 1 { "" } else { "s" })
            });
        }
        Err(e) => app.set_status(format!("The recording couldn't go into the deck: {e}")),
    }
    app.drain_requests();
    for f in files {
        let _ = std::fs::remove_file(f);
    }
}
