//! deckrt: Add Fonts in the app. Home › Font's list has Add Fonts…, and a font file or a .zip of
//! fonts dropped on the window is added the same way: copied into the user fonts folder (and
//! listed with Windows), so Word and PowerPoint have it too.

use serde_json::{Value, json};

use crate::SlideApp;

/// Home › Font › Add Fonts…: pick font files or .zip files and add them.
pub fn add_fonts_dialog(app: &mut SlideApp) {
    let Some(pick) = app.services.pick_open_many.as_mut() else {
        app.set_status("Adding fonts needs the desktop app".to_string());
        return;
    };
    let paths = pick("fonts");
    if !paths.is_empty() {
        install(app, paths);
    }
}

/// A file dropped on the window: added when it is a font or a .zip of fonts. Returns whether it
/// was taken.
pub fn dropped(app: &mut SlideApp, path: &std::path::Path, name: &str, bytes: &[u8]) -> bool {
    if !deckcraft_engine::cmd::deckrt_fonts::takes(name, bytes) || path.as_os_str().is_empty() {
        return false;
    }
    install(app, vec![path.to_string_lossy().to_string()]);
    true
}

/// Add the fonts in `paths` and say what came of it on the status line.
pub fn install(app: &mut SlideApp, paths: Vec<String>) {
    let r = match app.session.execute("font.install", &json!({ "paths": paths })) {
        Ok(r) => r,
        Err(e) => {
            app.set_status(crate::deckrt::messages::plain_error(&e.to_string()));
            return;
        }
    };
    let installed = r["installed"].as_array().cloned().unwrap_or_default();
    let mut families: Vec<(String, usize)> = vec![];
    let mut unlisted = 0;
    for f in &installed {
        let family = f["family"].as_str().unwrap_or_default().to_string();
        let styles: Vec<String> = f["styles"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
        let file = std::path::PathBuf::from(f["file"].as_str().unwrap_or_default());
        if deckcraft_show_ext::fonts::register(&family, &styles, &file).is_err() {
            unlisted += 1;
        }
        match families.iter_mut().find(|(n, _)| *n == family) {
            Some((_, n)) => *n += styles.len().max(1),
            None => families.push((family, styles.len().max(1))),
        }
    }
    let skipped: Vec<String> = r["skipped"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    app.set_status(summary(&families, &skipped, unlisted));
}

/// The status line after adding: "Added Lato (4 styles) and Inter. Word and PowerPoint have
/// them too."
fn summary(families: &[(String, usize)], skipped: &[String], unlisted: usize) -> String {
    let name = |(f, n): &(String, usize)| if *n > 1 { format!("{f} ({n} styles)") } else { f.clone() };
    let names: Vec<String> = families.iter().map(name).collect();
    let list = match names.as_slice() {
        [] => String::new(),
        [a] => a.clone(),
        [a, b] => format!("{a} and {b}"),
        [a, b, rest @ ..] => format!("{a}, {b} and {} more", rest.len()),
    };
    let mut s = format!("Added {list}.");
    if unlisted == 0 {
        s.push_str(if families.len() == 1 { " Word and PowerPoint have it too." } else { " Word and PowerPoint have them too." });
    } else {
        s.push_str(" Windows didn't list all of them, so other programs may not show them until you install them from the file.");
    }
    match skipped {
        [] => {}
        [one] => s.push_str(&format!(" {one} isn't a font.")),
        many => s.push_str(&format!(" {} files weren't fonts.", many.len())),
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_names_what_was_added() {
        assert_eq!(summary(&[("Lato".into(), 4)], &[], 0), "Added Lato (4 styles). Word and PowerPoint have it too.");
        assert_eq!(
            summary(&[("Lato".into(), 1), ("Inter".into(), 2)], &["OFL.txt".into()], 0),
            "Added Lato and Inter (2 styles). Word and PowerPoint have them too. OFL.txt isn't a font."
        );
    }
}
