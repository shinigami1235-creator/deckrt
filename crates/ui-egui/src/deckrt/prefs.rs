//! deckrt: small settings kept between runs in `<settings>/deckrt-prefs.json`: the presenter
//! view's layout and notes size, and whether picking an effect previews it.

use serde::{Deserialize, Serialize};

use crate::SlideApp;

const FILE: &str = "deckrt-prefs.json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Prefs {
    /// Picking a transition or an animation plays it on the slide.
    pub auto_preview: bool,
    /// Speaker notes size in presenter view, in points.
    pub notes_px: f32,
    /// Presenter view: the share of the width the current slide's column takes.
    pub notes_split: f32,
    /// Presenter view: the share of the right column the next slide takes.
    pub next_share: f32,
    /// Presenter view: the next slide is hidden so the notes get the column.
    pub hide_next: bool,
    /// Presenter view: the Phones section is folded to one line.
    pub fold_phones: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs { auto_preview: true, notes_px: 24.0, notes_split: 0.55, next_share: 0.32, hide_next: false, fold_phones: false }
    }
}

/// The settings, read from disk the first time.
pub fn get(app: &mut SlideApp) -> &mut Prefs {
    let dir = app.deckrt.settings_dir.clone();
    app.deckrt
        .prefs
        .get_or_insert_with(|| dir.and_then(|d| std::fs::read(d.join(FILE)).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default())
}

/// Write the settings when they changed.
pub fn set(app: &mut SlideApp, p: Prefs) {
    if app.deckrt.prefs.as_ref() == Some(&p) {
        return;
    }
    if let Some(d) = app.deckrt.settings_dir.as_ref()
        && let Ok(b) = serde_json::to_vec_pretty(&p)
    {
        let _ = std::fs::create_dir_all(d);
        let _ = std::fs::write(d.join(FILE), b);
    }
    app.deckrt.prefs = Some(p);
}
