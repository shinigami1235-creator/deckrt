//! deckrt: Insert › Screenshot, as PowerPoint has it. The menu shows a picture of each open window
//! of other programs; a click puts it on the slide. Screen Clipping copies the screen (without
//! deckrt's own windows), shows it over the display deckrt is on, and the part dragged over goes
//! on the slide.
//!
//! `DECKRT_SCREEN_TEST=<png>` stands in for the screen (and two windows cut from it) on a machine
//! without one to copy.

use std::sync::Arc;

use deckcraft_show_ext::capture::{self, Shot};
use egui::{Color32, CornerRadius, Pos2, Rect, RichText, Sense, Stroke, TextureHandle, Ui, pos2, vec2};
use serde_json::json;

use crate::SlideApp;

#[derive(Default)]
pub struct State {
    windows: Vec<(Arc<Shot>, TextureHandle)>,
    clip: Option<Clip>,
}

/// Screen Clipping in progress.
struct Clip {
    shot: Arc<Shot>,
    /// The part of `shot` on the display deckrt is on, in shot pixels: x, y, w, h.
    part: [u32; 4],
    /// That display, in points: x, y, w, h.
    frame: [f32; 4],
    tex: Option<TextureHandle>,
    from: Option<Pos2>,
    to: Option<Pos2>,
    focused: bool,
}

fn test_image() -> Option<Shot> {
    let path = std::env::var_os("DECKRT_SCREEN_TEST")?;
    let img = image::open(path).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    Some(Shot { title: "Screen".into(), w, h, rgba: img.into_raw(), origin: [0, 0] })
}

fn crop(s: &Shot, x: u32, y: u32, w: u32, h: u32) -> Option<Shot> {
    let img = image::RgbaImage::from_raw(s.w, s.h, s.rgba.clone())?;
    let (x, y) = (x.min(s.w.saturating_sub(1)), y.min(s.h.saturating_sub(1)));
    let (w, h) = (w.min(s.w - x).max(1), h.min(s.h - y).max(1));
    let part = image::imageops::crop_imm(&img, x, y, w, h).to_image();
    Some(Shot { title: s.title.clone(), w, h, rgba: part.into_raw(), origin: [s.origin[0] + x as i32, s.origin[1] + y as i32] })
}

fn take_windows() -> Vec<Shot> {
    match test_image() {
        Some(s) => {
            let (hw, hh) = (s.w / 2, s.h / 2);
            [crop(&s, 0, 0, hw, hh).map(|mut c| {
                c.title = "Test window one".into();
                c
            }), crop(&s, hw, hh, hw, hh).map(|mut c| {
                c.title = "Test window two".into();
                c
            })]
            .into_iter()
            .flatten()
            .collect()
        }
        None => capture::windows(12),
    }
}

fn take_screen() -> Option<Shot> {
    test_image().or_else(capture::screen)
}

fn texture(ctx: &egui::Context, name: &str, s: &Shot, max_w: u32) -> Option<TextureHandle> {
    let img = image::RgbaImage::from_raw(s.w, s.h, s.rgba.clone())?;
    let img = if s.w > max_w {
        let h = ((s.h as f64) * max_w as f64 / s.w as f64).round().max(1.0) as u32;
        image::imageops::resize(&img, max_w, h, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let (w, h) = img.dimensions();
    let ci = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], img.as_raw());
    Some(ctx.load_texture(name, ci, egui::TextureOptions::LINEAR))
}

/// Put `s` on the current slide.
fn insert(app: &mut SlideApp, s: &Shot) {
    let Some(img) = image::RgbaImage::from_raw(s.w, s.h, s.rgba.clone()) else { return };
    let mut png = std::io::Cursor::new(Vec::new());
    if img.write_to(&mut png, image::ImageFormat::Png).is_err() {
        app.set_status("The screenshot couldn't be saved as a picture.");
        return;
    }
    let data = deckcraft_engine::cmd::base64_encode(png.get_ref());
    if let Err(e) = app.run("insert.picture", json!({"name": "Screenshot.png", "data": data})) {
        app.set_status(e);
    }
}

/// The Insert tab's Screenshot button and its menu.
pub fn button(app: &mut SlideApp, ui: &mut Ui) {
    let en = app.session.active().is_some_and(|d| !d.doc.slides.is_empty());
    let r = crate::widgets::big_button(ui, crate::icons::Icon::Screenshot, "Screenshot", en);
    if r.clicked() {
        let ctx = ui.ctx().clone();
        app.deckrt.screenshot.windows = take_windows()
            .into_iter()
            .enumerate()
            .filter_map(|(i, s)| texture(&ctx, &format!("deckrt-window-{i}"), &s, 360).map(|t| (Arc::new(s), t)))
            .collect();
    }
    egui::Popup::menu(&r).width(540.0).show(|ui| menu(app, ui));
}

fn menu(app: &mut SlideApp, ui: &mut Ui) {
    let t = crate::theme::Tokens::get(ui.ctx());
    ui.label(RichText::new("Available Windows").strong());
    let wins = app.deckrt.screenshot.windows.clone();
    if wins.is_empty() {
        ui.label(RichText::new("No other windows are open.").color(t.text_dim));
    }
    let mut picked = None;
    egui::Grid::new("deckrt-windows").spacing(vec2(6.0, 6.0)).show(ui, |ui| {
        for (i, (shot, tex)) in wins.iter().enumerate() {
            let (r, resp) = ui.allocate_exact_size(vec2(164.0, 124.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, CornerRadius::same(4), t.hover);
            }
            let pic = Rect::from_min_size(r.min + vec2(4.0, 4.0), vec2(156.0, 96.0));
            let [w, h] = tex.size();
            let k = (pic.width() / w.max(1) as f32).min(pic.height() / h.max(1) as f32);
            let fit = Rect::from_center_size(pic.center(), vec2(w as f32 * k, h as f32 * k));
            ui.painter().image(tex.id(), fit, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            ui.painter().rect_stroke(fit, CornerRadius::ZERO, Stroke::new(1.0, t.border), egui::StrokeKind::Outside);
            let g = ui.painter().layout(shot.title.clone(), crate::theme::font(11.0), t.text, r.width() - 8.0);
            let first_line = g.rows.first().map(|row| row.rect().height()).unwrap_or(14.0);
            let clip = ui.painter().with_clip_rect(Rect::from_min_size(pos2(r.min.x + 4.0, pic.max.y + 4.0), vec2(r.width() - 8.0, first_line + 1.0)));
            clip.galley(pos2(r.min.x + 4.0, pic.max.y + 4.0), g, t.text);
            if resp.on_hover_text(&shot.title).clicked() {
                picked = Some(shot.clone());
            }
            if i % 3 == 2 {
                ui.end_row();
            }
        }
    });
    if let Some(s) = picked {
        insert(app, &s);
        ui.close();
    }
    ui.separator();
    if ui.button("Screen Clipping").on_hover_text("Drag over part of the screen to put it on the slide").clicked() {
        start_clip(app, ui.ctx());
        ui.close();
    }
}

fn start_clip(app: &mut SlideApp, ctx: &egui::Context) {
    let Some(shot) = take_screen() else {
        app.set_status("The screen couldn't be copied.");
        return;
    };
    let ppp = ctx.pixels_per_point().max(0.1);
    // The display deckrt's window is on: its centre, in physical pixels.
    let centre = ctx.input(|i| i.viewport().outer_rect).map(|r| r.center() * ppp).unwrap_or(pos2(0.0, 0.0));
    let displays = app.deckrt.displays_now();
    let on = displays
        .iter()
        .find(|d| {
            let (x, y) = (d.origin[0] as f32, d.origin[1] as f32);
            centre.x >= x && centre.y >= y && centre.x < x + d.size_px[0] as f32 && centre.y < y + d.size_px[1] as f32
        })
        .or_else(|| displays.iter().find(|d| d.primary))
        .or(displays.first());
    let (origin, size, scale) = match on {
        Some(d) => (d.origin, d.size_px, if d.scale.is_finite() && d.scale > 0.0 { d.scale } else { ppp }),
        None => (shot.origin, [shot.w, shot.h], ppp),
    };
    let px = (origin[0] - shot.origin[0]).max(0) as u32;
    let py = (origin[1] - shot.origin[1]).max(0) as u32;
    let part = [px.min(shot.w.saturating_sub(1)), py.min(shot.h.saturating_sub(1)), size[0].min(shot.w - px.min(shot.w)), size[1].min(shot.h - py.min(shot.h))];
    let frame = [origin[0] as f32 / scale, origin[1] as f32 / scale, part[2] as f32 / scale, part[3] as f32 / scale];
    app.deckrt.screenshot.clip = Some(Clip { shot: Arc::new(shot), part, frame, tex: None, from: None, to: None, focused: false });
}

/// Once a frame: the Screen Clipping window while one is open.
pub fn frame(app: &mut SlideApp, ctx: &egui::Context) {
    let Some(mut clip) = app.deckrt.screenshot.clip.take() else { return };
    if clip.tex.is_none() {
        let [x, y, w, h] = clip.part;
        clip.tex = crop(&clip.shot, x, y, w, h).and_then(|s| texture(ctx, "deckrt-clip", &s, 8192));
    }
    let Some(tex) = clip.tex.clone() else { return };
    let [fx, fy, fw, fh] = clip.frame;
    let builder = egui::ViewportBuilder::default()
        .with_title("Screen Clipping")
        .with_decorations(false)
        .with_position(pos2(fx, fy))
        .with_inner_size(vec2(fw, fh))
        .with_always_on_top()
        .with_taskbar(false);
    let mut done: Option<Option<Rect>> = None;
    ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("deckrt-screen-clipping"), builder, |ui, _| {
        if !clip.focused {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
            clip.focused = true;
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(Color32::BLACK)).show(ui, |ui| {
            let full = ui.max_rect();
            let p = ui.painter();
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            p.image(tex.id(), full, uv, Color32::WHITE);
            p.rect_filled(full, CornerRadius::ZERO, Color32::from_white_alpha(110));
            let resp = ui.interact(full, ui.id().with("deckrt-clip"), Sense::drag());
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            if resp.drag_started() {
                clip.from = resp.interact_pointer_pos();
            }
            if resp.dragged() {
                clip.to = resp.interact_pointer_pos();
            }
            let sel = clip.from.zip(clip.to).map(|(a, b)| Rect::from_two_pos(a, b).intersect(full));
            if let Some(s) = sel {
                let k = vec2(1.0 / full.width().max(1.0), 1.0 / full.height().max(1.0));
                let suv = Rect::from_min_max(pos2((s.min.x - full.min.x) * k.x, (s.min.y - full.min.y) * k.y), pos2((s.max.x - full.min.x) * k.x, (s.max.y - full.min.y) * k.y));
                p.image(tex.id(), s, suv, Color32::WHITE);
                p.rect_stroke(s, CornerRadius::ZERO, Stroke::new(1.5, Color32::from_rgb(0xE0, 0x4E, 0x1B)), egui::StrokeKind::Outside);
            } else {
                let hint = "Drag over the part you want. Esc cancels.";
                let g = p.layout_no_wrap(hint.into(), crate::theme::font(16.0), Color32::WHITE);
                let box_r = Rect::from_center_size(pos2(full.center().x, full.min.y + 40.0), g.size() + vec2(28.0, 16.0));
                p.rect_filled(box_r, CornerRadius::same(6), Color32::from_black_alpha(190));
                p.galley(box_r.min + vec2(14.0, 8.0), g, Color32::WHITE);
            }
            if resp.drag_stopped() {
                done = Some(sel.filter(|s| s.width() >= 4.0 && s.height() >= 4.0).map(|s| {
                    Rect::from_min_max(
                        pos2((s.min.x - full.min.x) / full.width().max(1.0), (s.min.y - full.min.y) / full.height().max(1.0)),
                        pos2((s.max.x - full.min.x) / full.width().max(1.0), (s.max.y - full.min.y) / full.height().max(1.0)),
                    )
                }));
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape) || i.viewport().close_requested()) {
                done = Some(None);
            }
        });
    });
    match done {
        None => {
            app.deckrt.screenshot.clip = Some(clip);
            ctx.request_repaint();
        }
        Some(None) => {}
        Some(Some(f)) => {
            let [x, y, w, h] = clip.part;
            let (cx, cy) = (x + (f.min.x * w as f32) as u32, y + (f.min.y * h as f32) as u32);
            let (cw, ch) = (((f.width() * w as f32) as u32).max(1), ((f.height() * h as f32) as u32).max(1));
            if let Some(part) = crop(&clip.shot, cx, cy, cw, ch) {
                insert(app, &part);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(w: u32, h: u32) -> Shot {
        let rgba = (0..w * h).flat_map(|i| [(i % 256) as u8, (i / 256 % 256) as u8, 7, 255]).collect();
        Shot { title: "t".into(), w, h, rgba, origin: [10, 20] }
    }

    #[test]
    fn a_crop_keeps_its_place_and_stays_inside() {
        let s = shot(100, 50);
        let c = crop(&s, 10, 5, 30, 20).expect("crop");
        assert_eq!((c.w, c.h, c.origin), (30, 20, [20, 25]));
        assert_eq!(c.rgba.len(), 30 * 20 * 4);
        // The first pixel of the crop is the source's (10, 5).
        let i = (5 * 100 + 10) as usize * 4;
        assert_eq!(&c.rgba[..4], &s.rgba[i..i + 4]);
        let edge = crop(&s, 90, 40, 50, 50).expect("crop");
        assert_eq!((edge.w, edge.h), (10, 10));
    }
}
