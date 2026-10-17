//! deckrt: the check before a talk, in the app. Present runs it first; when it finds a problem
//! the list opens instead of the show, with a fix button on each line that has one and Present
//! anyway at the bottom. The Check button on the Slide Show tab opens the same list any time.

use egui::{Color32, RichText, Ui};
use serde_json::{Value, json};

use crate::SlideApp;

pub const DIALOG: &str = "deckrt.check";

/// A button with a visible frame: the app's buttons are frameless until hovered, and here a
/// fix has to read as something to click.
pub fn framed(ui: &Ui, text: &str, min_w: f32) -> egui::Button<'static> {
    let v = ui.visuals();
    egui::Button::new(text.to_string())
        .fill(v.widgets.inactive.weak_bg_fill)
        .stroke(egui::Stroke::new(1.0, v.widgets.noninteractive.bg_stroke.color))
        .min_size(egui::vec2(min_w, 24.0))
}

fn revision(app: &SlideApp) -> Option<u64> {
    app.session.active().map(|d| d.revision)
}

/// Called by `start_show`: returns true when the show must wait (the list opened instead).
pub fn gate(app: &mut SlideApp, from: usize, reading: bool) -> bool {
    if reading || app.dialog.is_some() {
        return false;
    }
    let rev = revision(app);
    if rev.is_some() && app.deckrt.check_passed == rev {
        return false;
    }
    let Ok(r) = app.session.execute("deckrt.check", &json!({})) else { return false };
    if r["problems"].as_u64().unwrap_or(0) == 0 {
        app.deckrt.check_passed = rev;
        return false;
    }
    open(app, r, Some(from));
    true
}

/// Open the list; `from` is the slide the show starts at when it was opened by Present.
pub fn open(app: &mut SlideApp, result: Value, from: Option<usize>) {
    let mut d = crate::dialogs::Dialog::new(DIALOG);
    d.params = json!({"result": result, "from": from});
    app.dialog = Some(d);
}

/// The Check button on the Slide Show tab.
pub fn ribbon_button(app: &mut SlideApp, ui: &mut Ui) {
    let en = app.session.active().is_some_and(|d| !d.doc.slides.is_empty());
    if crate::widgets::big_button(ui, crate::icons::Icon::Check, "Check\nDeck", en)
        .on_hover_text("Find videos that won't play, missing fonts and text that runs off the slide")
        .clicked()
        && let Ok(r) = app.session.execute("deckrt.check", &json!({}))
    {
        open(app, r, None);
    }
}

pub fn title() -> &'static str {
    "Check before showing"
}

/// The dialog body. Returns whether it closes.
pub fn body(app: &mut SlideApp, ui: &mut Ui, d: &mut crate::dialogs::Dialog) -> bool {
    let from = d.params["from"].as_u64().map(|v| v as usize);
    let result = d.params["result"].clone();
    let items: Vec<Value> = result["items"].as_array().cloned().unwrap_or_default();
    let problems = result["problems"].as_u64().unwrap_or(0);
    let when = d.params["when"].as_str().unwrap_or("the talk").to_string();
    let cmd = d.params["cmd"].as_str().unwrap_or("deckrt.check").to_string();
    let notes = result["notes"].as_u64().unwrap_or(0);
    ui.set_min_width(560.0);
    ui.set_max_width(640.0);
    let plural = |n: u64, one: &str, many: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {many}") };
    let summary = match (problems, notes) {
        (0, 0) => "Nothing to fix.".to_string(),
        (0, n) => format!("No problems. {} to look at.", plural(n, "note", "notes")),
        (p, 0) => format!("{} to fix before {when}.", plural(p, "problem", "problems")),
        (p, n) => format!("{} to fix before {when}, and {}.", plural(p, "problem", "problems"), plural(n, "note", "notes")),
    };
    ui.label(RichText::new(summary).strong());
    ui.add_space(6.0);

    let mut close = false;
    let mut rerun = false;
    let mut go: Option<(usize, Option<u64>)> = None;
    egui::ScrollArea::vertical().max_height(380.0).auto_shrink([false, true]).show(ui, |ui| {
        // Problems first, then notes, each in slide order.
        let mut sorted = items.clone();
        sorted.sort_by_key(|i| (i["severity"] != "problem", i["slide"].as_u64().unwrap_or(0)));
        for (k, it) in sorted.iter().enumerate() {
            let problem = it["severity"] == "problem";
            ui.horizontal_wrapped(|ui| {
                let (tag, color) = if problem { ("Problem", Color32::from_rgb(0xD8, 0x3F, 0x3F)) } else { ("Note", ui.visuals().weak_text_color()) };
                ui.label(RichText::new(tag).color(color).strong());
                if let Some(s) = it["slide"].as_u64() {
                    if ui.link(format!("Slide {}", s + 1)).on_hover_text("Go to it").clicked() {
                        go = Some((s as usize, it["shape"].as_u64()));
                    }
                }
                ui.label(it["text"].as_str().unwrap_or(""));
            });
            if let Some(fix) = it.get("fix").filter(|f| f.is_object()) {
                ui.horizontal(|ui| {
                    ui.add_space(52.0);
                    if ui.add(framed(ui, fix["label"].as_str().unwrap_or("Fix"), 0.0)).clicked() {
                        for c in fix["cmds"].as_array().cloned().unwrap_or_default() {
                            if let (Some(id), Some(p)) = (c.get(0).and_then(Value::as_str), c.get(1)) {
                                if let Err(e) = app.run(id, p.clone()) {
                                    app.set_status(e);
                                    break;
                                }
                            }
                        }
                        rerun = true;
                    }
                });
            }
            if k + 1 < sorted.len() {
                ui.add_space(4.0);
            }
        }
    });
    ui.add_space(10.0);
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 26.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if let Some(from) = from {
            let label = if problems > 0 { "Present anyway" } else { "Present" };
            if ui.add(framed(ui, label, 110.0)).clicked() {
                app.deckrt.check_passed = revision(app);
                close = true;
                app.dialog = None;
                app.start_show(from, false);
            }
            if ui.add(framed(ui, "Cancel", 72.0)).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        } else if ui.add(framed(ui, "Close", 72.0)).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::Enter)) {
            close = true;
        }
        if ui.add(framed(ui, "Check again", 0.0)).clicked() {
            rerun = true;
        }
    });
    if rerun && let Ok(r) = app.session.execute(&cmd, &json!({})) {
        d.params["result"] = r;
    }
    if let Some((slide, shape)) = go {
        let _ = app.run("slide.go", json!({"index": slide}));
        if let Some(id) = shape {
            let _ = app.run("edit.select", json!({"ids": [id]}));
        }
        close = true;
    }
    close
}
