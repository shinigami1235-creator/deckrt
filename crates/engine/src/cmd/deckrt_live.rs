//! deckrt: live sliders. While a slider is dragged each value runs as its command so the slide
//! shows it at once, and the whole drag is one undo step, as a drag on the canvas is.

use crate::Session;

/// Labels of live drags start with this, so a drag left open can be told apart from the canvas's.
pub const LIVE: &str = "deckrt-live:";

/// Start a live drag named `label` (the undo step's name) unless one is already running.
pub fn begin(s: &mut Session, label: &str) {
    s.begin(&format!("{LIVE}{label}"));
}

/// End the live drag: everything since `begin` becomes one undo step named `label`.
pub fn end(s: &mut Session) {
    let Some(label) = open_label(s) else { return };
    if let Some(st) = s.active_mut()
        && let Some(i) = st.interaction.as_mut()
    {
        i.label = label.trim_start_matches(LIVE).to_string();
    }
    let name = s.active().and_then(|st| st.interaction.as_ref()).map(|i| i.label.clone()).unwrap_or_default();
    s.commit_named(&name);
}

/// The open live drag's label, if any.
pub fn open_label(s: &Session) -> Option<String> {
    s.active().and_then(|st| st.interaction.as_ref()).map(|i| i.label.clone()).filter(|l| l.starts_with(LIVE))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use serde_json::json;

    use crate::Session;
    use crate::cmd::base64_encode;

    const PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
        0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf,
        0xc0, 0xf0, 0x1f, 0x00, 0x05, 0x00, 0x01, 0xff, 0x89, 0x99, 0x3d, 0x1d, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60,
        0x82,
    ];

    fn with_picture() -> Session {
        let mut s = Session::with_new();
        s.execute("slide.new", &json!({"layout": "blank"})).unwrap();
        s.execute("insert.picture", &json!({"data": base64_encode(PNG), "name": "a.png", "rect": [100.0, 100.0, 200.0, 150.0]})).unwrap();
        s
    }

    fn alpha(s: &Session) -> Option<f64> {
        s.doc().unwrap().shapes().iter().find_map(|sh| match &sh.kind {
            deckcraft_model::ShapeKind::Picture { fill } => Some(fill.alpha.unwrap_or(1.0)),
            _ => None,
        })
    }

    #[test]
    fn a_live_drag_is_one_undo_step() {
        let mut s = with_picture();
        super::begin(&mut s, "Transparency");
        for t in [0.1, 0.2, 0.35, 0.5] {
            s.execute("picture.adjust", &json!({"transparency": t})).unwrap();
        }
        assert_eq!(super::open_label(&s).as_deref(), Some("deckrt-live:Transparency"));
        super::end(&mut s);
        assert!(super::open_label(&s).is_none());
        assert!((alpha(&s).unwrap() - 0.5).abs() < 1e-9);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(alpha(&s), Some(1.0));
    }

    #[test]
    fn saturation_survives_saving_as_pptx() {
        let mut s = with_picture();
        s.execute("picture.adjust", &json!({"saturation": 2.5})).unwrap();
        let doc = s.doc().unwrap().doc.clone();
        let bytes = crate::cmd::file::save_bytes(&doc, "pptx").unwrap();
        let back = deckcraft_pptx::import(&bytes).unwrap();
        let sat = back.slides.iter().flat_map(|sl| sl.shapes.iter()).find_map(|sh| match &sh.kind {
            deckcraft_model::ShapeKind::Picture { fill } => fill.adjust.saturation,
            _ => None,
        });
        assert_eq!(sat, Some(2.5));
    }
}
