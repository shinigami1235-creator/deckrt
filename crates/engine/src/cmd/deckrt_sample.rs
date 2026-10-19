//! deckrt: the sample deck that comes with the program, one slide for each feature, with speaker
//! notes to read aloud. It is built by `packaging/sample/build_sample.py` from pictures and a
//! model drawn by `packaging/sample/make_assets.py`, and opens as a new, unsaved deck.

use serde_json::Value;

use super::*;
use crate::{Result, Session};

/// The sample deck, as saved by `build_sample.py`.
pub const SAMPLE: &[u8] = include_bytes!("../../../../assets/sample/deckrt-feature-tour.pptx");

/// Its name in the window title and on File › New.
pub const NAME: &str = "deckrt feature tour";

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(noundo "deckrt.openSample", "deckrt Feature Tour", ["File"], None, "{} → {document, slides}; opens as a new deck", always, open_sample)]
}

fn open_sample(s: &mut Session, _p: &Value) -> Result<Value> {
    let r = s.execute("file.openBytes", &serde_json::json!({"name": NAME, "data": base64_encode(SAMPLE)}))?;
    let slides = s.doc()?.doc.slides.len();
    Ok(serde_json::json!({"document": r.get("document").cloned().unwrap_or(Value::Null), "slides": slides}))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use serde_json::json;

    use crate::Session;

    /// The sample opens whole: every slide, its objects and the notes the script reads.
    #[test]
    fn the_sample_deck_opens_with_its_objects_and_notes() {
        let mut s = Session::new();
        let r = s.execute("deckrt.openSample", &json!({})).unwrap();
        assert_eq!(r["slides"], json!(25));
        let st = s.doc().unwrap();
        assert!(st.path.is_none(), "the sample opens unsaved");
        let objects = st.doc.slides.iter().flat_map(|sl| sl.shapes.iter()).filter(|sh| deckcraft_interactive::object::is_live(sh)).count();
        assert!(objects >= 13, "{objects} objects");
        assert!(st.doc.slides.iter().all(|sl| !sl.notes_text().trim().is_empty()), "every slide has its script");
    }
}
