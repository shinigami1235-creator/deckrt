//! deckrt: Find a Slide. Every deck in the folders chosen (Documents, Desktop and Downloads at
//! first) is read once on a thread of its own: each slide's title, text and notes, and a small
//! picture of it, kept in the settings folder. A search shows the slides with all the words
//! typed, and the ones picked come into the open deck after the current slide.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use egui::{Color32, CornerRadius, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::SlideApp;

pub const DIALOG: &str = "deckrt.finder";
const INDEX_FILE: &str = "slide-index.json";
const THUMB_DIR: &str = "slide-thumbs";
const THUMB_W: u32 = 320;
const EXTS: [&str; 4] = ["pptx", "ppsx", "potx", "deckcraft"];
/// Bigger decks are left out: reading one would hold the index up for minutes.
const MAX_BYTES: u64 = 400 * 1024 * 1024;
const MAX_DECKS: usize = 3000;
const MAX_DEPTH: usize = 8;
const MAX_HITS: usize = 120;
const SKIP_DIRS: [&str; 8] = ["node_modules", ".git", "appdata", "$recycle.bin", "target", ".cache", "library", "windows"];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SlideEntry {
    pub title: String,
    pub text: String,
    pub notes: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DeckEntry {
    pub size: u64,
    pub modified: u64,
    pub slides: Vec<SlideEntry>,
    /// The folder under `slide-thumbs` holding `<n>.png`.
    pub thumbs: String,
    /// Why the deck couldn't be read (password, damaged).
    pub error: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    folders: Vec<PathBuf>,
    keep_look: bool,
    decks: BTreeMap<String, DeckEntry>,
}

#[derive(Default)]
struct Shared {
    saved: Saved,
    /// Reading: done, of, the deck being read.
    progress: Option<(usize, usize, String)>,
    stop: bool,
}

/// One slide found.
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub path: String,
    pub slide: usize,
    pub score: u32,
    pub modified: u64,
    pub snippet: String,
}

#[derive(Default)]
pub struct Finder {
    shared: Arc<Mutex<Shared>>,
    loaded: bool,
    /// The decks were read again since the dialog opened.
    refreshed: bool,
    /// The folders changed while a read was running: read again when it ends.
    again: bool,
    pub query: String,
    /// The query the hits are for.
    searched: Option<String>,
    hits: Vec<Hit>,
    picked: Vec<(String, usize)>,
    textures: HashMap<(String, usize), Option<egui::TextureHandle>>,
    focus: bool,
}

fn lock(m: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The folders searched at first.
fn default_folders() -> Vec<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from);
    let mut out = vec![];
    for base in [home, std::env::var_os("OneDrive").map(PathBuf::from)].into_iter().flatten() {
        for f in ["Documents", "Desktop", "Downloads"] {
            let p = base.join(f);
            if p.is_dir() && !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// A file kept in the cloud only (OneDrive "free up space"): reading it would download it.
#[cfg(windows)]
fn cloud_only(m: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    m.file_attributes() & (0x0040_0000 | 0x0004_0000 | 0x1000) != 0
}
#[cfg(not(windows))]
fn cloud_only(_: &std::fs::Metadata) -> bool {
    false
}

fn modified(m: &std::fs::Metadata) -> u64 {
    m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0)
}

/// The decks under `folders`, with their size and time.
fn find_decks(folders: &[PathBuf]) -> Vec<(PathBuf, u64, u64)> {
    let mut out = vec![];
    let mut stack: Vec<(PathBuf, usize)> = folders.iter().map(|f| (f.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name.starts_with("~$") {
                continue;
            }
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                if depth < MAX_DEPTH && !SKIP_DIRS.contains(&name.to_lowercase().as_str()) {
                    stack.push((p, depth + 1));
                }
                continue;
            }
            let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !EXTS.contains(&ext.as_str()) {
                continue;
            }
            let Ok(m) = e.metadata() else { continue };
            if m.len() == 0 || m.len() > MAX_BYTES || cloud_only(&m) {
                continue;
            }
            out.push((p, m.len(), modified(&m)));
            if out.len() >= MAX_DECKS {
                return out;
            }
        }
    }
    out
}

fn thumbs_name(path: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// A slide's own words.
pub fn slide_entry(sl: &deckcraft_model::Slide) -> SlideEntry {
    let mut text = vec![];
    deckcraft_model::walk(&sl.shapes, &mut |sh, _| {
        if let Some(t) = &sh.text {
            let t = t.text();
            if !t.trim().is_empty() {
                text.push(t);
            }
        }
    });
    SlideEntry { title: sl.title(), text: text.join("\n"), notes: sl.notes.text() }
}

/// Read one deck: its slides' words, and a picture of each slide into `thumbs`.
fn read_deck(path: &Path, thumbs: &Path) -> Result<Vec<SlideEntry>, String> {
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let pres = deckcraft_engine::cmd::file::open_presentation(&name, &bytes).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(thumbs);
    std::fs::create_dir_all(thumbs).map_err(|e| e.to_string())?;
    let sw = pres.slide_size.width.max(1.0);
    let w = THUMB_W;
    let h = ((pres.slide_size.height / sw) * w as f64).round().max(1.0) as u32;
    let mut out = vec![];
    for (i, sl) in pres.slides.iter().enumerate() {
        out.push(slide_entry(sl));
        let img = deckcraft_render::render_slide(
            &pres,
            i,
            &deckcraft_render::RenderOpts { scale: w as f64 / sw, size: Some((w, h)), ..Default::default() },
        );
        let rgba = img.to_straight();
        if let Some(buf) = image::RgbaImage::from_raw(img.width, img.height, rgba) {
            let rgb = image::DynamicImage::ImageRgba8(buf).to_rgb8();
            let _ = rgb.save(thumbs.join(format!("{i}.jpg")));
        }
    }
    Ok(out)
}

/// Bring the index up to date on a thread of its own.
fn spawn_reader(shared: Arc<Mutex<Shared>>, settings: PathBuf) {
    let _ = std::thread::Builder::new().name("deckrt-find-slides".into()).spawn(move || {
        let folders = lock(&shared).saved.folders.clone();
        let found = find_decks(&folders);
        // Decks gone or outside the folders now leave the index.
        {
            let mut s = lock(&shared);
            let keep: std::collections::HashSet<String> = found.iter().map(|(p, _, _)| p.to_string_lossy().to_string()).collect();
            let gone: Vec<String> = s.saved.decks.keys().filter(|k| !keep.contains(*k)).cloned().collect();
            for k in gone {
                if let Some(d) = s.saved.decks.remove(&k) {
                    let _ = std::fs::remove_dir_all(settings.join(THUMB_DIR).join(d.thumbs));
                }
            }
        }
        let todo: Vec<&(PathBuf, u64, u64)> = {
            let s = lock(&shared);
            found
                .iter()
                .filter(|(p, size, m)| s.saved.decks.get(&p.to_string_lossy().to_string()).is_none_or(|d| d.size != *size || d.modified != *m))
                .collect()
        };
        let n = todo.len();
        for (i, (p, size, m)) in todo.into_iter().enumerate() {
            let key = p.to_string_lossy().to_string();
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            {
                let mut s = lock(&shared);
                if s.stop {
                    break;
                }
                s.progress = Some((i, n, name));
            }
            let thumbs = thumbs_name(&key);
            let entry = match read_deck(p, &settings.join(THUMB_DIR).join(&thumbs)) {
                Ok(slides) => DeckEntry { size: *size, modified: *m, slides, thumbs, error: None },
                Err(e) => DeckEntry { size: *size, modified: *m, slides: vec![], thumbs, error: Some(e) },
            };
            let mut s = lock(&shared);
            s.saved.decks.insert(key, entry);
            if i % 10 == 9 {
                save(&s.saved, &settings);
            }
        }
        let mut s = lock(&shared);
        s.progress = None;
        save(&s.saved, &settings);
    });
}

fn save(saved: &Saved, settings: &Path) {
    let _ = std::fs::create_dir_all(settings);
    if let Ok(t) = serde_json::to_string(saved) {
        let tmp = settings.join(format!("{INDEX_FILE}.tmp"));
        if std::fs::write(&tmp, t).is_ok() {
            let _ = std::fs::rename(&tmp, settings.join(INDEX_FILE));
        }
    }
}

/// The slides holding every word of `query`, best first: words in the title count most, then
/// the slide's text, then its notes; newer decks first among equals.
pub fn search(decks: &BTreeMap<String, DeckEntry>, query: &str, skip: Option<&str>) -> Vec<Hit> {
    let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).filter(|t| !t.is_empty()).collect();
    if terms.is_empty() {
        return vec![];
    }
    let mut hits = vec![];
    for (path, d) in decks {
        if Some(path.as_str()) == skip {
            continue;
        }
        for (i, s) in d.slides.iter().enumerate() {
            let (title, text, notes) = (s.title.to_lowercase(), s.text.to_lowercase(), s.notes.to_lowercase());
            let mut score = 0;
            let mut all = true;
            for t in &terms {
                let k = if title.contains(t.as_str()) {
                    3
                } else if text.contains(t.as_str()) {
                    2
                } else if notes.contains(t.as_str()) {
                    1
                } else {
                    0
                };
                if k == 0 {
                    all = false;
                    break;
                }
                score += k;
            }
            if all {
                hits.push(Hit { path: path.clone(), slide: i, score, modified: d.modified, snippet: snippet(s, &terms[0]) });
            }
        }
    }
    hits.sort_by(|a, b| b.score.cmp(&a.score).then(b.modified.cmp(&a.modified)).then(a.path.cmp(&b.path)).then(a.slide.cmp(&b.slide)));
    hits.truncate(MAX_HITS);
    hits
}

/// The words around the first match, for the hover.
fn snippet(s: &SlideEntry, term: &str) -> String {
    for (label, t) in [("", &s.title), ("", &s.text), ("Notes: ", &s.notes)] {
        let flat = t.replace('\n', " ");
        let lower = flat.to_lowercase();
        if let Some(at) = lower.find(term) {
            // Byte positions in the lower-cased text may not fall on the original's characters;
            // count characters instead.
            let ci = lower[..at].chars().count();
            let chars: Vec<char> = flat.chars().collect();
            let from = ci.saturating_sub(50);
            let to = (ci + 90).min(chars.len());
            let mut out = String::from(label);
            if from > 0 {
                out.push('…');
            }
            out.extend(&chars[from..to]);
            if to < chars.len() {
                out.push('…');
            }
            return out;
        }
    }
    String::new()
}

fn deck_name(path: &str) -> String {
    Path::new(path).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string())
}

impl Finder {
    fn ensure_loaded(&mut self, settings: Option<&Path>, deck_dir: Option<PathBuf>) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let mut saved: Saved =
            settings.and_then(|d| std::fs::read_to_string(d.join(INDEX_FILE)).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        if saved.folders.is_empty() {
            saved.folders = default_folders();
            if let Some(d) = deck_dir
                && !saved.folders.iter().any(|f| d.starts_with(f))
            {
                saved.folders.push(d);
            }
        }
        lock(&self.shared).saved = saved;
    }

    fn refresh(&mut self, settings: Option<&Path>) {
        let Some(dir) = settings else { return };
        let mut s = lock(&self.shared);
        if s.progress.is_some() {
            self.again = true;
            return;
        }
        s.progress = Some((0, 0, String::new()));
        drop(s);
        spawn_reader(self.shared.clone(), dir.to_path_buf());
    }
}

/// The button on the Home tab, beside New Slide.
pub fn ribbon_button(app: &mut SlideApp, ui: &mut Ui) {
    let en = app.session.active().is_some() && app.services.pick_open.is_some();
    if crate::widgets::big_button(ui, crate::icons::Icon::Search, "Find a\nSlide", en)
        .on_hover_text("Search every deck in your folders for a slide and bring it into this one")
        .clicked()
    {
        app.deckrt.finder.focus = true;
        app.deckrt.finder.refreshed = false;
        app.dialog = Some(crate::dialogs::Dialog::new(DIALOG));
    }
}

pub fn title() -> &'static str {
    "Find a slide"
}

/// The dialog. Returns whether it closes.
pub fn body(app: &mut SlideApp, ui: &mut Ui) -> bool {
    let settings = app.deckrt.settings_dir.clone();
    let deck_path = app.session.active().and_then(|d| d.path.clone());
    let deck_dir = deck_path.as_ref().and_then(|p| Path::new(p).parent().map(Path::to_path_buf));
    let f = &mut app.deckrt.finder;
    f.ensure_loaded(settings.as_deref(), deck_dir);
    if !f.refreshed {
        f.refreshed = true;
        f.refresh(settings.as_deref());
    }
    ui.set_min_width(900.0);
    ui.set_max_width(900.0);
    if settings.is_none() {
        ui.label("Find a Slide needs the desktop app.");
    }

    let r =
        ui.add(egui::TextEdit::singleline(&mut f.query).hint_text("Search your slides: collagen, HA filler, case 3").desired_width(f32::INFINITY));
    if f.focus {
        r.request_focus();
        f.focus = false;
    }

    // Folders.
    let mut remove: Option<usize> = None;
    let mut add = false;
    let (progress, folders, decks, slides, keep_look) = {
        let s = lock(&f.shared);
        (
            s.progress.clone(),
            s.saved.folders.clone(),
            s.saved.decks.len(),
            s.saved.decks.values().map(|d| d.slides.len()).sum::<usize>(),
            s.saved.keep_look,
        )
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Looking in").color(Color32::from_gray(130)));
        for (i, p) in folders.iter().enumerate() {
            let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.display().to_string());
            if ui.add(egui::Button::new(format!("{name}  ×")).small()).on_hover_text(format!("{}\nClick to stop looking here", p.display())).clicked()
            {
                remove = Some(i);
            }
        }
        if ui.add(super::check::framed(ui, "Add folder…", 0.0)).clicked() {
            add = true;
        }
    });
    match &progress {
        Some((done, of, name)) if *of > 0 => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(format!("Reading decks: {} of {of}, {name}", done + 1)).color(Color32::from_gray(130)));
            });
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
        }
        Some(_) => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Looking for decks").color(Color32::from_gray(130)));
            });
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
        }
        None => {
            ui.label(RichText::new(format!("{slides} slides in {decks} decks.")).color(Color32::from_gray(130)));
        }
    }
    if progress.is_none() && f.again {
        f.again = false;
        f.refresh(settings.as_deref());
    }
    let mut changed_folders = false;
    if let Some(i) = remove {
        lock(&f.shared).saved.folders.remove(i);
        changed_folders = true;
    }
    if add && let Some(p) = app.services.pick_open.as_mut().and_then(|pick| pick("folder")) {
        let p = PathBuf::from(p);
        let f = &mut app.deckrt.finder;
        let mut s = lock(&f.shared);
        if p.is_dir() && !s.saved.folders.contains(&p) {
            s.saved.folders.push(p);
            changed_folders = true;
        }
    }
    let f = &mut app.deckrt.finder;
    if changed_folders {
        if let Some(d) = settings.as_deref() {
            save(&lock(&f.shared).saved, d);
        }
        f.searched = None;
        f.refresh(settings.as_deref());
    }

    // Search again when the words or the index changed.
    let q = f.query.trim().to_string();
    let stale = f.searched.as_deref() != Some(q.as_str()) || progress.is_some();
    if stale {
        let s = lock(&f.shared);
        f.hits = search(&s.saved.decks, &q, deck_path.as_deref());
        drop(s);
        f.searched = Some(q.clone());
    }
    ui.add_space(6.0);

    let mut insert_now: Option<(String, usize)> = None;
    let card_w = 205.0;
    egui::ScrollArea::vertical().id_salt("finder-hits").min_scrolled_height(420.0).max_height(420.0).auto_shrink([false, false]).show(ui, |ui| {
        if q.is_empty() {
            ui.label(RichText::new("Type a word from the slide's title, text or notes.").color(Color32::from_gray(130)));
            return;
        }
        if f.hits.is_empty() {
            ui.label(RichText::new(format!("No slide has all of “{q}”.")).color(Color32::from_gray(130)));
            return;
        }
        let mut loads = 0;
        let thumbs_root = settings.as_deref().map(|d| d.join(THUMB_DIR));
        let thumb_dirs: HashMap<String, String> = {
            let s = lock(&f.shared);
            f.hits.iter().filter_map(|h| s.saved.decks.get(&h.path).map(|d| (h.path.clone(), d.thumbs.clone()))).collect()
        };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
            for h in f.hits.clone() {
                let key = (h.path.clone(), h.slide);
                if !f.textures.contains_key(&key) && loads < 6 {
                    loads += 1;
                    let tex = thumbs_root.as_ref().zip(thumb_dirs.get(&h.path)).and_then(|(root, dir)| {
                        let bytes = std::fs::read(root.join(dir).join(format!("{}.jpg", h.slide))).ok()?;
                        let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
                        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
                        Some(ui.ctx().load_texture(format!("finder-{}-{}", h.path, h.slide), ci, egui::TextureOptions::LINEAR))
                    });
                    f.textures.insert(key.clone(), tex);
                }
                let picked = f.picked.iter().position(|p| *p == key);
                let tex = f.textures.get(&key).and_then(|t| t.clone());
                let img_h = tex.as_ref().map(|t| card_w * t.size()[1] as f32 / t.size()[0].max(1) as f32).unwrap_or(card_w * 9.0 / 16.0);
                let (rect, resp) = ui.allocate_exact_size(vec2(card_w, img_h + 34.0), Sense::click());
                let img_rect = egui::Rect::from_min_size(rect.min, vec2(card_w, img_h));
                match &tex {
                    Some(t) => {
                        ui.painter().image(t.id(), img_rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                    }
                    None => {
                        ui.painter().rect_filled(img_rect, CornerRadius::same(2), Color32::from_gray(230));
                    }
                }
                let border = if picked.is_some() {
                    Stroke::new(3.0, Color32::from_rgb(0xe8, 0x6a, 0x1c))
                } else if resp.hovered() {
                    Stroke::new(1.5, Color32::from_gray(150))
                } else {
                    Stroke::new(1.0, Color32::from_gray(200))
                };
                ui.painter().rect_stroke(img_rect, CornerRadius::same(2), border, StrokeKind::Outside);
                if let Some(n) = picked {
                    let c = img_rect.right_top() + vec2(-14.0, 14.0);
                    ui.painter().circle_filled(c, 11.0, Color32::from_rgb(0xe8, 0x6a, 0x1c));
                    ui.painter().text(c, egui::Align2::CENTER_CENTER, (n + 1).to_string(), egui::FontId::proportional(12.0), Color32::WHITE);
                }
                let cap = format!("{} · {}", deck_name(&h.path), h.slide + 1);
                let g = ui.painter().layout(cap, egui::FontId::proportional(12.0), ui.visuals().text_color(), card_w);
                ui.painter().with_clip_rect(rect).galley(egui::pos2(rect.min.x, img_rect.max.y + 5.0), g, ui.visuals().text_color());
                let resp = resp.on_hover_text(format!("{}\n{}", h.path, h.snippet));
                if resp.double_clicked() {
                    insert_now = Some(key.clone());
                } else if resp.clicked() {
                    match picked {
                        Some(i) => {
                            f.picked.remove(i);
                        }
                        None => f.picked.push(key.clone()),
                    }
                }
            }
        });
        if loads > 0 {
            ui.ctx().request_repaint();
        }
    });

    ui.add_space(8.0);
    let mut keep = keep_look;
    let mut close = false;
    let mut insert = false;
    let mut open = false;
    ui.horizontal(|ui| {
        if ui.checkbox(&mut keep, "Keep their own look").on_hover_text("Off: the slides take this deck's theme").changed() {
            lock(&f.shared).saved.keep_look = keep;
            if let Some(d) = settings.as_deref() {
                save(&lock(&f.shared).saved, d);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(super::check::framed(ui, "Close", 72.0)).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
            let n = f.picked.len();
            let label = match n {
                0 => "Insert".to_string(),
                1 => "Insert 1 slide".to_string(),
                n => format!("Insert {n} slides"),
            };
            if ui.add_enabled(n > 0, super::check::framed(ui, &label, 110.0)).clicked() {
                insert = true;
            }
            if ui.add_enabled(n == 1, super::check::framed(ui, "Open its deck", 0.0)).clicked() {
                open = true;
            }
        });
    });

    let picks: Vec<(String, usize)> = match insert_now {
        Some(k) => vec![k],
        None if insert => f.picked.clone(),
        None => vec![],
    };
    if !picks.is_empty() {
        // One reuse per deck, in the order picked, each after the slide before it.
        let mut groups: Vec<(String, Vec<usize>)> = vec![];
        for (p, i) in picks {
            match groups.last_mut() {
                Some((gp, v)) if *gp == p => v.push(i),
                _ => groups.push((p, vec![i])),
            }
        }
        let mut done = 0;
        for (path, slides) in groups {
            let n = slides.len();
            match app.session.execute("slide.reuse", &json!({"path": path, "slides": slides, "keepFormatting": keep})) {
                Ok(_) => done += n,
                Err(e) => {
                    app.set_status(format!("{}: {e}", deck_name(&path)));
                    break;
                }
            }
        }
        app.drain_requests();
        if done > 0 {
            app.set_status(if done == 1 { "1 slide added.".to_string() } else { format!("{done} slides added.") });
            app.deckrt.finder.picked.clear();
            return true;
        }
    }
    if open && let Some((path, slide)) = app.deckrt.finder.picked.first().cloned() {
        let _ = app.run("file.open", json!({"path": path}));
        let _ = app.run("slide.go", json!({"index": slide}));
        app.deckrt.finder.picked.clear();
        return true;
    }
    close
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deck(modified: u64, slides: &[(&str, &str, &str)]) -> DeckEntry {
        DeckEntry {
            modified,
            slides: slides.iter().map(|(t, x, n)| SlideEntry { title: t.to_string(), text: x.to_string(), notes: n.to_string() }).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn every_word_must_match_and_titles_come_first() {
        let mut decks = BTreeMap::new();
        decks.insert(
            "a.pptx".to_string(),
            deck(
                10,
                &[
                    ("Collagen in the dermis", "Type I and III", ""),
                    ("Fillers", "HA filler and collagen stimulators", ""),
                    ("Case 3", "", "HA filler, 1 ml"),
                ],
            ),
        );
        decks.insert("b.pptx".to_string(), deck(20, &[("HA filler basics", "Cross-linked hyaluronic acid", "")]));
        let h = search(&decks, "ha filler", None);
        let order: Vec<(&str, usize)> = h.iter().map(|x| (x.path.as_str(), x.slide)).collect();
        assert_eq!(order, vec![("b.pptx", 0), ("a.pptx", 1), ("a.pptx", 2)]);
        assert_eq!(search(&decks, "collagen", None).first().map(|x| x.slide), Some(0));
        assert!(search(&decks, "collagen botox", None).is_empty());
        assert!(search(&decks, "  ", None).is_empty());
        assert_eq!(search(&decks, "filler", Some("b.pptx")).len(), 2, "the open deck is left out");
        assert_eq!(h[2].snippet, "Notes: HA filler, 1 ml");
    }

    #[test]
    fn thai_words_are_found_inside_text() {
        let mut decks = BTreeMap::new();
        decks.insert("t.pptx".to_string(), deck(1, &[("คอลลาเจนในผิวหนัง", "ไฟโบรบลาสต์สร้างคอลลาเจน", "")]));
        assert_eq!(search(&decks, "คอลลาเจน", None).len(), 1);
    }

    #[test]
    fn decks_are_found_and_reread_only_when_changed() {
        let root = std::env::temp_dir().join(format!("deckrt-finder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let sub = root.join("talks");
        std::fs::create_dir_all(sub.join("node_modules")).expect("dir");
        let mut s = deckcraft_engine::Session::with_new();
        s.execute("slide.new", &json!({})).expect("slide");
        let doc = &s.doc().expect("doc").doc;
        let bytes = deckcraft_engine::cmd::file::save_bytes(doc, "pptx").expect("save");
        std::fs::write(sub.join("Lecture.pptx"), &bytes).expect("write");
        std::fs::write(sub.join("~$Lecture.pptx"), b"lock").expect("write");
        std::fs::write(sub.join("node_modules").join("x.pptx"), &bytes).expect("write");
        let found = find_decks(std::slice::from_ref(&root));
        assert_eq!(found.len(), 1, "{found:?}");
        let thumbs = root.join("thumbs");
        let slides = read_deck(&found[0].0, &thumbs).expect("read");
        assert_eq!(slides.len(), doc.slides.len());
        assert!(thumbs.join("0.jpg").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }
}
