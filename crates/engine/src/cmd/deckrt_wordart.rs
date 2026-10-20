//! deckrt: PowerPoint's twenty WordArt styles (Insert › WordArt), in its gallery order, four rows
//! of five. Each sets the text's fill, outline and shadow or glow from the theme colours, so the
//! style follows the deck's theme as PowerPoint's does. Bevels and the pattern fills of the last
//! row are drawn as shading and stripes, since deckrt's text has neither.

use deckcraft_color::{ColorTransform as T, SchemeSlot as S};
use deckcraft_model::style::{Glow, Gradient, GradientShape, GradientStop, Shadow};
use deckcraft_model::text::RunProps;
use deckcraft_model::{ColorRef, Fill, Line};

/// The styles' names, as PowerPoint's gallery tips them.
pub const NAMES: [&str; 20] = [
    "Fill: Black, Text color 1; Shadow",
    "Fill: Blue, Accent color 1; Shadow",
    "Fill: Orange, Accent color 2; Outline: Orange, Accent color 2",
    "Fill: White; Outline: Blue, Accent color 1; Glow: Blue, Accent color 1",
    "Fill: Gold, Accent color 4; Soft Bevel",
    "Fill: Gray, Accent color 3; Sharp Bevel",
    "Fill: Blue, Accent color 1; Shadow; Outline: White",
    "Fill: Gold, Accent color 4; Outline: Orange, Accent color 2",
    "Fill: White; Outline: Blue, Accent color 1",
    "Gradient Fill: Gray",
    "Fill: Black, Text color 1; Outline: White, Background color 1; Shadow",
    "Fill: White; Outline: Black, Text color 1; Shadow",
    "Gradient Fill: Blue, Accent color 1",
    "Fill: White; Outline: Orange, Accent color 2; Hard Shadow",
    "Fill: Light Gray, Background color 2; Inner Shadow",
    "Pattern Fill: Black, Text color 1, Dark Upward Diagonal",
    "Pattern Fill: Gray, Accent color 3, Narrow Horizontal",
    "Pattern Fill: Blue, Accent color 1, Light Downward Diagonal; Outline: Blue",
    "Pattern Fill: White; Outline: Blue, Accent color 1; Shadow",
    "Pattern Fill: Blue-Gray, Text color 2, Dark Upward Diagonal; Hard Shadow",
];

fn c(slot: S) -> ColorRef {
    ColorRef::scheme(slot)
}

fn lum(slot: S, m: i32, o: i32) -> ColorRef {
    let mut r = ColorRef::scheme(slot).with(T::LumMod(m));
    if o != 0 {
        r = r.with(T::LumOff(o));
    }
    r
}

fn grad(angle: f64, stops: &[(f64, ColorRef)]) -> Fill {
    Fill::Gradient(Gradient {
        stops: stops.iter().map(|(pos, color)| GradientStop { pos: *pos, color: color.clone() }).collect(),
        shape: GradientShape::Linear { angle, scaled: false },
        rotate_with_shape: true,
        tile: [0.0; 4],
    })
}

/// Stripes made of hard-stop bands: how deckrt draws the pattern fills of the last row.
fn stripes(angle: f64, bands: usize, fg: ColorRef, bg: ColorRef, fg_share: f64) -> Fill {
    let mut stops = vec![];
    for b in 0..bands {
        let a = b as f64 / bands as f64;
        let m = a + fg_share / bands as f64;
        let e = (b + 1) as f64 / bands as f64;
        stops.push((a, fg.clone()));
        stops.push((m, fg.clone()));
        stops.push((m, bg.clone()));
        stops.push((e, bg.clone()));
    }
    grad(angle, &stops)
}

fn shadow(color: ColorRef, blur: f64, dist: f64) -> Shadow {
    Shadow { color, blur, dist, dir: 45.0, inner: false, sx: 1.0, sy: 1.0, kx: 0.0, ky: 0.0, align: "tl".into(), rotate_with_shape: false }
}

fn soft() -> Shadow {
    shadow(c(S::Tx1).with(T::Alpha(40_000)), 3.0, 2.0)
}

fn hard(color: ColorRef) -> Shadow {
    shadow(color, 0.0, 3.0)
}

/// Apply style `i` (0–19) to a run. Returns false for an index past the last.
pub fn apply(i: usize, r: &mut RunProps) -> bool {
    let (fill, outline, sh, glow): (Fill, Option<Line>, Option<Shadow>, Option<Glow>) = match i {
        0 => (Fill::solid(c(S::Tx1)), None, Some(soft()), None),
        1 => (Fill::solid(c(S::Accent1)), None, Some(soft()), None),
        2 => (grad(90.0, &[(0.0, lum(S::Accent2, 40_000, 60_000)), (1.0, c(S::Accent2))]), Some(Line::solid(c(S::Accent2), 1.25)), None, None),
        3 => (
            Fill::solid(c(S::Bg1)),
            Some(Line::solid(c(S::Accent1), 1.0)),
            None,
            Some(Glow { color: c(S::Accent1).with(T::Alpha(40_000)), radius: 5.0 }),
        ),
        4 => (
            grad(90.0, &[(0.0, lum(S::Accent4, 40_000, 60_000)), (0.55, c(S::Accent4)), (1.0, lum(S::Accent4, 75_000, 0))]),
            None,
            Some(soft()),
            None,
        ),
        5 => {
            (grad(90.0, &[(0.0, lum(S::Accent3, 60_000, 40_000)), (0.5, c(S::Accent3)), (1.0, lum(S::Accent3, 50_000, 0))]), None, Some(soft()), None)
        }
        6 => (Fill::solid(c(S::Accent1)), Some(Line::solid(c(S::Bg1), 1.0)), Some(shadow(c(S::Tx1).with(T::Alpha(55_000)), 4.0, 3.0)), None),
        7 => (Fill::solid(c(S::Accent4)), Some(Line::solid(c(S::Accent2), 1.0)), None, None),
        8 => (Fill::solid(c(S::Bg1)), Some(Line::solid(lum(S::Accent1, 60_000, 40_000), 1.0)), None, None),
        9 => (grad(90.0, &[(0.0, lum(S::Tx1, 50_000, 50_000)), (1.0, lum(S::Tx1, 75_000, 25_000))]), None, Some(soft()), None),
        10 => (Fill::solid(c(S::Tx1)), Some(Line::solid(c(S::Bg1), 1.5)), Some(shadow(c(S::Tx1).with(T::Alpha(60_000)), 5.0, 4.0)), None),
        11 => (Fill::solid(c(S::Bg1)), Some(Line::solid(c(S::Tx1), 1.25)), Some(soft()), None),
        12 => (grad(90.0, &[(0.0, lum(S::Accent1, 40_000, 60_000)), (0.5, c(S::Accent1)), (1.0, lum(S::Accent1, 50_000, 0))]), None, None, None),
        13 => (Fill::solid(c(S::Bg1)), Some(Line::solid(c(S::Accent2), 1.25)), Some(hard(c(S::Accent2))), None),
        14 => (
            Fill::solid(lum(S::Bg2, 90_000, 0)),
            Some(Line::solid(lum(S::Bg1, 65_000, 0), 0.75)),
            Some(shadow(c(S::Tx1).with(T::Alpha(25_000)), 2.0, 1.0)),
            None,
        ),
        15 => (stripes(-45.0, 14, c(S::Tx1), lum(S::Tx1, 50_000, 50_000), 0.6), None, None, None),
        16 => (stripes(90.0, 12, c(S::Accent3), lum(S::Accent3, 40_000, 60_000), 0.5), None, None, None),
        17 => (stripes(45.0, 14, c(S::Accent1), lum(S::Accent1, 20_000, 80_000), 0.35), Some(Line::solid(c(S::Accent1), 0.75)), None, None),
        18 => (stripes(90.0, 12, c(S::Bg1), lum(S::Accent1, 20_000, 80_000), 0.5), Some(Line::solid(c(S::Accent1), 1.0)), Some(soft()), None),
        19 => (stripes(-45.0, 14, c(S::Tx2), lum(S::Tx2, 40_000, 60_000), 0.6), None, Some(hard(lum(S::Tx2, 40_000, 60_000))), None),
        _ => return false,
    };
    r.fill = Some(fill);
    r.outline = outline;
    r.shadow = sh;
    r.glow = glow;
    r.bold = Some(matches!(i, 10 | 11 | 13 | 15..=19));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_style_applies_and_differs() {
        let mut seen: Vec<RunProps> = vec![];
        for i in 0..NAMES.len() {
            let mut r = RunProps::default();
            assert!(apply(i, &mut r), "style {i}");
            assert!(!seen.contains(&r), "style {i} looks like an earlier one");
            seen.push(r);
        }
        assert!(!apply(NAMES.len(), &mut RunProps::default()));
    }

    /// The fill, outline, shadow and glow come back after saving as .pptx and opening again.
    #[test]
    fn styles_survive_saving_as_pptx() {
        use serde_json::json;
        let mut s = crate::Session::with_new();
        let _ = s.execute("slide.new", &json!({"layout": "blank"}));
        for i in [3, 12, 13, 19] {
            s.execute("insert.wordArt", &json!({"style": i, "text": "A"})).expect("insert");
        }
        let runs = |s: &crate::Session| -> Vec<RunProps> {
            let d = s.active().expect("deck");
            d.shapes().iter().filter_map(|sh| sh.text.as_ref()?.paragraphs.first()?.runs.first().map(|r| r.props.clone())).collect()
        };
        let before = runs(&s);
        let saved = s.execute("file.saveBytes", &json!({"format": "pptx"})).expect("save");
        s.execute("file.openBytes", &json!({"name": "w.pptx", "data": saved["data"]})).expect("open");
        let after = runs(&s);
        assert_eq!(before.len(), 4);
        for (b, a) in before.iter().zip(&after) {
            assert_eq!(a.fill.is_some(), b.fill.is_some());
            assert_eq!(matches!(a.fill, Some(Fill::Gradient(_))), matches!(b.fill, Some(Fill::Gradient(_))));
            assert_eq!(a.outline.is_some(), b.outline.is_some());
            assert_eq!(a.shadow.is_some(), b.shadow.is_some());
            assert_eq!(a.glow.is_some(), b.glow.is_some());
        }
    }
}
