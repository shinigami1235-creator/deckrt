//! deckrt: Export to Video. Each slide is drawn once and held for its time: its recorded
//! narration's length, else its Advance After time, else the seconds asked for. A transition
//! becomes a fade. Narration is the sound. The MP4 is written on a thread of its own while the
//! editor stays in use.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use deckcraft_model::{MediaId, Presentation};
use deckcraft_video_out::{Options, SECOND, Writer, blend, nv12};
use egui::{Color32, RichText, Ui};

use crate::SlideApp;

pub const DIALOG: &str = "deckrt.video";

/// One slide's part of the video.
#[derive(Clone, Debug, PartialEq)]
pub struct Seg {
    pub slide: usize,
    pub secs: f64,
    /// The fade in from the slide before.
    pub fade: f64,
    pub narration: Option<MediaId>,
}

/// The slides in the video and how long each stays. Hidden slides are left out.
pub fn plan(doc: &Presentation, use_timings: bool, default_secs: f64) -> Vec<Seg> {
    let mut out = vec![];
    for (i, sl) in doc.slides.iter().enumerate() {
        if sl.hidden {
            continue;
        }
        let narr = deckcraft_engine::cmd::deckrt_record::narration_of(sl).map(|(_, c)| (c.media, c.duration_ms as f64 / 1000.0));
        let after = sl.transition.as_ref().and_then(|t| t.advance_after_ms).map(|ms| ms as f64 / 1000.0);
        let secs = match (use_timings, narr, after) {
            (true, Some((_, n)), Some(a)) => n.max(a),
            (true, Some((_, n)), None) => n,
            (true, None, Some(a)) => a,
            _ => default_secs,
        }
        .clamp(0.5, 3600.0);
        let fade = match &sl.transition {
            Some(t) if !out.is_empty() && t.kind != "none" && t.duration_ms > 0 => (t.duration_ms as f64 / 1000.0).min(2.0).min(secs / 2.0),
            _ => 0.0,
        };
        out.push(Seg { slide: i, secs, fade, narration: if use_timings { narr.map(|n| n.0) } else { None } });
    }
    out
}

#[derive(Default)]
struct Progress {
    done: usize,
    of: usize,
    result: Option<Result<PathBuf, String>>,
}

struct Job {
    progress: Arc<Mutex<Progress>>,
    cancel: Arc<AtomicBool>,
}

/// The dialog's choices and a running export.
pub struct Video {
    full_hd: bool,
    use_timings: bool,
    default_secs: f64,
    job: Option<Job>,
    pub last: Option<Result<PathBuf, String>>,
}

impl Default for Video {
    fn default() -> Self {
        Video { full_hd: true, use_timings: true, default_secs: 5.0, job: None, last: None }
    }
}

fn lock(m: &Mutex<Progress>) -> std::sync::MutexGuard<'_, Progress> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The Export to Video button on the Record tab.
pub fn ribbon_group(app: &mut SlideApp, ui: &mut Ui) {
    let en = app.session.active().is_some_and(|d| !d.doc.slides.is_empty());
    crate::widgets::group_named(ui, "Export", |ui| {
        if crate::widgets::big_button(ui, crate::icons::Icon::Video, "Export\nto Video", en)
            .on_hover_text("Save the deck as an MP4 with your recorded timings and narration")
            .clicked()
        {
            app.dialog = Some(crate::dialogs::Dialog::new(DIALOG));
        }
    });
}

/// Every frame: an export that ended says so on the status line.
pub fn poll(app: &mut SlideApp) {
    let Some(job) = &app.deckrt.video.job else { return };
    let r = lock(&job.progress).result.take();
    if let Some(r) = r {
        app.set_status(match &r {
            Ok(p) => format!("Video saved: {}", p.display()),
            Err(e) => e.clone(),
        });
        app.deckrt.video.last = Some(r);
        app.deckrt.video.job = None;
    }
}

/// A narration clip as 16-bit mono at `rate`.
fn narration_pcm(doc: &Presentation, id: MediaId, rate: u32) -> Vec<i16> {
    let Some(m) = doc.media(id) else { return vec![] };
    let Ok(pcm) = deckcraft_media::audio::decode(&m.data) else { return vec![] };
    let ch = pcm.channels.max(1) as usize;
    let mono: Vec<f32> = pcm.samples.chunks(ch).map(|c| c.iter().sum::<f32>() / c.len() as f32).collect();
    deckcraft_captions::resample_to(&mono, pcm.rate, rate).iter().map(|s| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16).collect()
}

fn render(doc: &Presentation, slide: usize, w: u32, h: u32) -> Vec<u8> {
    let sw = doc.slide_size.width.max(1.0);
    let img = deckcraft_render::render_slide(doc, slide, &deckcraft_render::RenderOpts { scale: w as f64 / sw, size: Some((w, h)), ..Default::default() });
    let rgba = img.to_straight();
    if img.width != w || img.height != h {
        return nv12(&vec![255u8; (w * h * 4) as usize], w as usize, h as usize);
    }
    nv12(&rgba, w as usize, h as usize)
}

/// Write the video. Runs on its own thread.
fn export(doc: Arc<Presentation>, segs: Vec<Seg>, path: PathBuf, opts: Options, progress: Arc<Mutex<Progress>>, cancel: Arc<AtomicBool>) -> Result<PathBuf, String> {
    // A failed export leaves no half-written MP4 behind.
    let r = export_to(doc, segs, path.clone(), opts, progress, cancel);
    if r.is_err() {
        let _ = std::fs::remove_file(&path);
    }
    r
}

fn export_to(doc: Arc<Presentation>, segs: Vec<Seg>, path: PathBuf, opts: Options, progress: Arc<Mutex<Progress>>, cancel: Arc<AtomicBool>) -> Result<PathBuf, String> {
    let mut w = Writer::create(&path, &opts)?;
    let (fw, fh) = (opts.width, opts.height);
    let dt = SECOND / opts.fps as i64;
    let rate = opts.audio_rate as i64;
    let mut t: i64 = 0;
    let mut prev: Option<Vec<u8>> = None;
    for (n, seg) in segs.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            drop(w);
            let _ = std::fs::remove_file(&path);
            return Err("Video export stopped.".into());
        }
        let frame = render(&doc, seg.slide, fw, fh);
        let audio = seg.narration.map(|m| narration_pcm(&doc, m, opts.audio_rate)).unwrap_or_default();
        let frames = ((seg.secs * opts.fps as f64).round() as i64).max(1);
        let fade = (seg.fade * opts.fps as f64).round() as i64;
        let seg_samples = (frames * dt * rate / SECOND) as usize;
        let mut written = 0usize;
        for k in 0..frames {
            match (&prev, k < fade) {
                (Some(p), true) => w.frame(&blend(p, &frame, (k + 1) as f32 / fade as f32), t + k * dt, dt)?,
                _ => w.frame(&frame, t + k * dt, dt)?,
            }
            // The sound up to the end of this frame, a second at a time.
            let until = (((k + 1) * dt * rate / SECOND) as usize).min(seg_samples);
            if until - written >= opts.audio_rate as usize || k + 1 == frames {
                let chunk: Vec<i16> = (written..until).map(|i| audio.get(i).copied().unwrap_or(0)).collect();
                w.audio(&chunk, t + written as i64 * SECOND / rate)?;
                written = until;
            }
        }
        t += frames * dt;
        prev = Some(frame);
        lock(&progress).done = n + 1;
    }
    w.finish()?;
    Ok(path)
}

pub fn title() -> &'static str {
    "Export to video"
}

/// The dialog. Returns whether it closes.
pub fn body(app: &mut SlideApp, ui: &mut Ui) -> bool {
    ui.set_min_width(460.0);
    ui.set_max_width(520.0);
    let Some(st) = app.session.active() else { return true };
    let doc = &st.doc;
    let name = st.path.as_deref().and_then(|p| std::path::Path::new(p).file_stem().map(|s| s.to_string_lossy().to_string())).unwrap_or_else(|| "Presentation".into());
    let v = &mut app.deckrt.video;
    let timed = doc.slides.iter().any(|s| s.transition.as_ref().is_some_and(|t| t.advance_after_ms.is_some()));
    let narrated = doc.slides.iter().filter(|s| !s.hidden && deckcraft_engine::cmd::deckrt_record::narration_of(s).is_some()).count();
    let segs = plan(doc, v.use_timings && (timed || narrated > 0), v.default_secs);
    let total: f64 = segs.iter().map(|s| s.secs).sum();
    let running = v.job.is_some();

    ui.add_enabled_ui(!running, |ui| {
        ui.horizontal(|ui| {
            ui.label("Quality");
            ui.radio_value(&mut v.full_hd, true, "Full HD (1080p)");
            ui.radio_value(&mut v.full_hd, false, "HD (720p)");
        });
        ui.add_enabled(timed || narrated > 0, egui::Checkbox::new(&mut v.use_timings, "Use recorded timings and narration"));
        ui.horizontal(|ui| {
            ui.label("Seconds on a slide with no timing");
            ui.add(egui::DragValue::new(&mut v.default_secs).update_while_editing(false).range(1.0..=120.0).speed(0.5));
        });
    });
    ui.add_space(6.0);
    let narr_line = match narrated {
        0 => String::new(),
        1 => ", 1 with narration".into(),
        n => format!(", {n} with narration"),
    };
    ui.label(format!("{} slides, {} long{narr_line}.", segs.len(), deckcraft_captions::coach::clock(total)));
    ui.label(RichText::new("Animations show finished, transitions become fades, and videos on slides show their first frame.").size(12.0).color(Color32::from_gray(130)));
    ui.add_space(8.0);

    let mut start: Option<PathBuf> = None;
    let mut close = false;
    if let Some(job) = &v.job {
        let (done, of) = {
            let p = lock(&job.progress);
            (p.done, p.of)
        };
        ui.add(egui::ProgressBar::new(done as f32 / of.max(1) as f32).text(format!("Slide {} of {of}", (done + 1).min(of))));
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        ui.horizontal(|ui| {
            if ui.add(super::check::framed(ui, "Stop", 72.0)).clicked() {
                job.cancel.store(true, Ordering::Relaxed);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(super::check::framed(ui, "Close", 72.0)).on_hover_text("The export carries on; the status line says when it's done").clicked() {
                    close = true;
                }
            });
        });
        return close;
    }
    if let Some(r) = &v.last {
        let (t, c) = match r {
            Ok(p) => (format!("Saved to {}", p.display()), Color32::from_rgb(0x2e, 0xa0, 0x5c)),
            Err(e) => (e.clone(), Color32::from_rgb(0xd8, 0x3f, 0x3f)),
        };
        ui.label(RichText::new(t).size(12.0).color(c));
        ui.add_space(4.0);
    }
    let windows = cfg!(windows);
    ui.horizontal(|ui| {
        let r = ui.add_enabled(windows && !segs.is_empty(), super::check::framed(ui, "Export…", 90.0));
        if !windows {
            r.on_disabled_hover_text("Export to Video works on Windows.");
        } else if r.clicked()
            && let Some(p) = app.services.pick_save.as_mut().and_then(|f| f(&format!("{name}.mp4")))
        {
            let mut p = PathBuf::from(p);
            if p.extension().is_none_or(|e| !e.eq_ignore_ascii_case("mp4")) {
                p.set_extension("mp4");
            }
            start = Some(p);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(super::check::framed(ui, "Close", 72.0)).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
    });
    if let Some(path) = start
        && let Some(st) = app.session.active()
    {
        let doc = st.doc.clone();
        let v = &mut app.deckrt.video;
        let h: u32 = if v.full_hd { 1080 } else { 720 };
        let aspect = doc.slide_size.width / doc.slide_size.height.max(1.0);
        let w = (((h as f64 * aspect) / 2.0).round() as u32 * 2).clamp(16, 4096);
        let opts = Options { width: w, height: h, ..Default::default() };
        let segs = plan(&doc, v.use_timings && (timed || narrated > 0), v.default_secs);
        let progress = Arc::new(Mutex::new(Progress { done: 0, of: segs.len(), result: None }));
        let cancel = Arc::new(AtomicBool::new(false));
        let (p2, c2) = (progress.clone(), cancel.clone());
        let spawned = std::thread::Builder::new().name("deckrt-video".into()).spawn(move || {
            let r = export(doc, segs, path, opts, p2.clone(), c2);
            lock(&p2).result = Some(r);
        });
        match spawned {
            Ok(_) => {
                v.last = None;
                v.job = Some(Job { progress, cancel });
            }
            Err(e) => v.last = Some(Err(format!("The export couldn't start: {e}"))),
        }
    }
    close
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slides_take_narration_then_timings_then_the_default() {
        let mut doc = Presentation::default();
        doc.slides.clear();
        let mut a = deckcraft_model::Slide::default();
        a.transition = Some(deckcraft_model::Transition { advance_after_ms: Some(7000), kind: "fade".into(), duration_ms: 800, ..Default::default() });
        let mut b = deckcraft_model::Slide::default();
        b.transition = Some(deckcraft_model::Transition { kind: "fade".into(), duration_ms: 800, ..Default::default() });
        let c = deckcraft_model::Slide { hidden: true, ..Default::default() };
        for s in [a, b, c] {
            doc.slides.push(Arc::new(s));
        }
        let p = plan(&doc, true, 5.0);
        assert_eq!(p.len(), 2, "the hidden slide is left out");
        assert_eq!((p[0].secs, p[0].fade), (7.0, 0.0), "no fade into the first slide");
        assert_eq!((p[1].secs, p[1].fade), (5.0, 0.8));
        let p = plan(&doc, false, 3.0);
        assert_eq!(p[0].secs, 3.0);
    }

    /// Windows only: a two-slide deck with narration on the first becomes an MP4.
    #[cfg(windows)]
    #[test]
    fn a_deck_becomes_an_mp4_on_windows() {
        use serde_json::json;
        let mut s = deckcraft_engine::Session::with_new();
        s.execute("slide.new", &json!({})).expect("slide");
        let tone: Vec<f32> = (0..22_050 * 3 / 2).map(|i| (i as f32 * 0.05).sin() * 0.3).collect();
        let wav = deckcraft_captions::wav(&tone, 22_050);
        let path = std::env::temp_dir().join(format!("deckrt-narr-test-{}.wav", std::process::id()));
        std::fs::write(&path, wav).expect("wav");
        s.execute("deckrt.setNarration", &json!({"slides": [{"index": 0, "path": path.to_string_lossy(), "ms": 1500}]})).expect("narrate");
        s.execute("transition.timing", &json!({"index": 1, "after": 2000})).expect("timing");
        let doc = s.doc().expect("doc").doc.clone();
        let segs = plan(&doc, true, 1.0);
        assert_eq!(segs.iter().map(|g| g.secs).collect::<Vec<_>>(), vec![1.5, 2.0]);
        let out = std::env::temp_dir().join(format!("deckrt-export-test-{}.mp4", std::process::id()));
        let opts = Options { width: 640, height: 360, ..Default::default() };
        let progress = Arc::new(Mutex::new(Progress { done: 0, of: segs.len(), result: None }));
        let r = export(doc, segs, out.clone(), opts, progress.clone(), Arc::new(AtomicBool::new(false)));
        assert_eq!(r.as_ref().ok(), Some(&out), "{r:?}");
        assert_eq!(lock(&progress).done, 2);
        let bytes = Arc::new(std::fs::read(&out).expect("read"));
        let info = deckcraft_media::probe(&bytes).expect("probe");
        assert_eq!(info.video.as_ref().map(|v| (v.width, v.height)), Some((640, 360)));
        assert!(info.audio.as_ref().is_some_and(|a| a.codec.to_lowercase().contains("aac")), "{:?}", info.audio);
        assert!((3200..=3900).contains(&info.duration_ms), "{}", info.duration_ms);
        let _ = std::fs::remove_file(&out);
        let _ = std::fs::remove_file(&path);
    }
}
