//! Pictures of the deck's media as egui textures, decoded once per media item: what the show and
//! the presenter view draw for a swap, a scrub, a wipe, a popup picture or a model's poster.

use std::collections::HashMap;
use std::sync::Arc;

use deckcraft_model::{MediaId, Presentation};
use egui::{ColorImage, TextureHandle, TextureOptions};

/// Longest side a picture is kept at. Slides are shown at most a few thousand pixels wide.
const MAX_SIDE: u32 = 2048;

#[derive(Default)]
pub struct Pictures {
    textures: HashMap<(usize, MediaId), Option<(TextureHandle, [u32; 2])>>,
}

impl Pictures {
    /// Decode `media` to RGBA, at most [`MAX_SIDE`] on its longer side.
    pub fn decode(doc: &Presentation, media: MediaId) -> Option<image::RgbaImage> {
        let m = doc.media(media)?;
        let img = deckcraft_render::decode_image(&m.data)?;
        let (w, h) = (img.width(), img.height());
        if w.max(h) > MAX_SIDE {
            let k = MAX_SIDE as f64 / w.max(h) as f64;
            let nw = ((w as f64 * k).round() as u32).max(1);
            let nh = ((h as f64 * k).round() as u32).max(1);
            return Some(image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle));
        }
        Some(img)
    }

    /// The texture of `media`, decoded on first use; `None` when the picture cannot be read.
    /// The size is the picture's own pixel size (after the size cap).
    pub fn get(&mut self, ctx: &egui::Context, doc: &Arc<Presentation>, media: MediaId) -> Option<(egui::TextureId, [u32; 2])> {
        let key = (Arc::as_ptr(doc) as usize, media);
        if let Some(t) = self.textures.get(&key) {
            return t.as_ref().map(|(h, s)| (h.id(), *s));
        }
        // The same media in another revision of the deck is the same picture: reuse it.
        if let Some(found) = self.textures.iter().find(|(k, v)| k.1 == media && v.is_some()).map(|(_, v)| v.clone()) {
            let out = found.as_ref().map(|(h, s)| (h.id(), *s));
            self.textures.insert(key, found);
            return out;
        }
        let decoded = Self::decode(doc, media).map(|img| {
            let size = [img.width() as usize, img.height() as usize];
            let ci = ColorImage::from_rgba_unmultiplied(size, img.as_raw());
            let tex = ctx.load_texture(format!("deckrt-picture-{}", media.0), ci, TextureOptions::LINEAR);
            (tex, [img.width(), img.height()])
        });
        let out = decoded.as_ref().map(|(h, s)| (h.id(), *s));
        self.textures.insert(key, decoded);
        out
    }

    /// Drop every texture (the show ended, or the deck closed).
    pub fn clear(&mut self) {
        self.textures.clear();
    }
}

/// The largest rectangle of `aspect` (width over height) that fits inside `rect`, centred:
/// `object-fit: contain`.
pub fn contain(rect: egui::Rect, aspect: f32) -> egui::Rect {
    if !aspect.is_finite() || aspect <= 0.0 || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return rect;
    }
    let (w, h) = if rect.width() / rect.height() > aspect { (rect.height() * aspect, rect.height()) } else { (rect.width(), rect.width() / aspect) };
    egui::Rect::from_center_size(rect.center(), egui::vec2(w, h))
}

/// The part of a picture of `aspect` that fills `rect` without distortion, as UV coordinates:
/// `object-fit: cover`.
pub fn cover_uv(rect: egui::Rect, aspect: f32) -> egui::Rect {
    let unit = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    if !aspect.is_finite() || aspect <= 0.0 || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return unit;
    }
    let box_aspect = rect.width() / rect.height();
    if box_aspect > aspect {
        // The box is wider: crop the top and bottom of the picture.
        let h = aspect / box_aspect;
        egui::Rect::from_min_max(egui::pos2(0.0, 0.5 - h / 2.0), egui::pos2(1.0, 0.5 + h / 2.0))
    } else {
        let w = box_aspect / aspect;
        egui::Rect::from_min_max(egui::pos2(0.5 - w / 2.0, 0.0), egui::pos2(0.5 + w / 2.0, 1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contain_and_cover_keep_the_aspect() {
        let r = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 100.0));
        let c = contain(r, 1.0);
        assert_eq!((c.width(), c.height()), (100.0, 100.0));
        assert_eq!(c.center(), r.center());
        let uv = cover_uv(r, 1.0);
        assert!((uv.width() - 1.0).abs() < 1e-6 && (uv.height() - 0.5).abs() < 1e-6, "{uv:?}");
        let uv = cover_uv(r, 4.0);
        assert!((uv.height() - 1.0).abs() < 1e-6 && (uv.width() - 0.5).abs() < 1e-6, "{uv:?}");
    }
}
