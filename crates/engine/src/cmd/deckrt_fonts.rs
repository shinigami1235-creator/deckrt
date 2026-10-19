//! deckrt: Add Fonts. Font files (and the .zip files font sites hand out) are copied into the
//! per-user fonts folder the system reads, the same one Windows' Install and macOS' Font Book
//! use, so Word and PowerPoint get them too. They draw in deckrt at once.
//!
//! On Windows a font in that folder also needs its entry under
//! `HKCU\Software\Microsoft\Windows NT\CurrentVersion\Fonts` before other programs list it: the
//! command returns what to register and the desktop app writes it.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::{CommandSpec, always, bad, cmd};
use crate::{Result, Session};

/// The font file extensions taken, by themselves or inside a .zip.
pub const FONT_EXTS: [&str; 4] = ["ttf", "otf", "ttc", "otc"];
/// A font file larger than this is refused.
const MAX_FONT: u64 = 64 << 20;
/// All the fonts taken out of one .zip together.
const MAX_ZIP_TOTAL: u64 = 256 << 20;
/// Fonts read from one .zip.
const MAX_ZIP_FONTS: usize = 400;

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(noundo "font.install", "Add Fonts", ["Home", "Font"], None, "{paths: [file], dir?} → {installed: [{family, styles, file}], skipped: [name]} (copies fonts into the user fonts folder)", always, install)]
}

fn is_font_name(name: &str) -> bool {
    Path::new(name).extension().and_then(|e| e.to_str()).is_some_and(|e| FONT_EXTS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Whether a dropped or picked file is something Add Fonts takes: a font, or a .zip with one in.
pub fn takes(name: &str, bytes: &[u8]) -> bool {
    if is_font_name(name) {
        return true;
    }
    name.to_ascii_lowercase().ends_with(".zip") && !fonts_in_zip(bytes).is_empty()
}

/// The fonts inside a .zip: (file name, bytes). Folders, macOS' `__MACOSX` copies and anything too
/// big are left out; names keep only their last part, so nothing in the zip picks a folder.
fn fonts_in_zip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    use std::io::Read;
    let Ok(mut z) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else { return vec![] };
    let mut out = vec![];
    let mut total = 0u64;
    for i in 0..z.len() {
        if out.len() >= MAX_ZIP_FONTS {
            break;
        }
        let Ok(mut f) = z.by_index(i) else { continue };
        let full = f.name().replace('\\', "/");
        if f.is_dir() || full.split('/').any(|p| p == "__MACOSX") {
            continue;
        }
        let Some(name) = full.rsplit('/').next().filter(|n| !n.starts_with('.') && is_font_name(n)).map(str::to_string) else { continue };
        let size = f.size();
        if size > MAX_FONT || total + size > MAX_ZIP_TOTAL {
            continue;
        }
        let mut data = Vec::with_capacity(size as usize);
        if f.by_ref().take(MAX_FONT + 1).read_to_end(&mut data).is_err() || data.len() as u64 > MAX_FONT {
            continue;
        }
        total += data.len() as u64;
        out.push((name, data));
    }
    out
}

/// A file name that is safe to write: the last part only, without characters Windows refuses.
fn clean_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let s: String = base.chars().map(|c| if c.is_control() || "<>:\"|?*".contains(c) { '_' } else { c }).collect();
    let s = s.trim().trim_start_matches('.').to_string();
    if s.is_empty() { "font.ttf".into() } else { s }
}

/// Where `name` goes in `dir`: the same name, or one with a number added when a different file is
/// there already. `None` when the very same font is there (nothing to copy).
fn target(dir: &Path, name: &str, data: &[u8]) -> Option<PathBuf> {
    let there = |p: &Path| crate::io::exists(&p.to_string_lossy());
    let p = dir.join(name);
    if !there(&p) {
        return Some(p);
    }
    if crate::io::read_file(&p.to_string_lossy()).is_ok_and(|old| old == data) {
        return None;
    }
    let stem = Path::new(name).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let ext = Path::new(name).extension().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    (2..1000).map(|n| dir.join(format!("{stem} ({n}).{ext}"))).find(|p| !there(p))
}

fn install(_s: &mut Session, p: &Value) -> Result<Value> {
    let paths: Vec<String> = p
        .get("paths")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .filter(|v: &Vec<String>| !v.is_empty())
        .ok_or_else(|| bad("font.install", "missing `paths`"))?;
    let dir = match p.get("dir").and_then(Value::as_str) {
        Some(d) => PathBuf::from(d),
        None => deckcraft_fonts::user_font_dir().ok_or_else(|| bad("font.install", "this computer has no fonts folder deckrt can write to"))?,
    };
    crate::io::make_dir(&dir.to_string_lossy()).map_err(|e| bad("font.install", format!("couldn't make the fonts folder: {e}")))?;
    let mut files: Vec<(String, Vec<u8>)> = vec![];
    let mut skipped: Vec<String> = vec![];
    for path in &paths {
        let name = Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.clone());
        let is_zip = name.to_ascii_lowercase().ends_with(".zip");
        if crate::io::file_len(path).is_none_or(|n| n > if is_zip { MAX_ZIP_TOTAL } else { MAX_FONT }) {
            skipped.push(name);
            continue;
        }
        let Ok(bytes) = crate::io::read_file(path) else {
            skipped.push(name);
            continue;
        };
        if is_zip {
            let inside = fonts_in_zip(&bytes);
            if inside.is_empty() {
                skipped.push(name);
            }
            files.extend(inside);
        } else {
            files.push((name, bytes));
        }
    }
    let mut installed: Vec<Value> = vec![];
    for (name, data) in files {
        let faces = deckcraft_fonts::faces_in(&data);
        let Some((family, _)) = faces.first().cloned() else {
            skipped.push(name);
            continue;
        };
        let styles: Vec<String> = faces.iter().map(|(_, s)| s.clone()).collect();
        let file = match target(&dir, &clean_name(&name), &data) {
            Some(to) => {
                crate::io::write_file(&to.to_string_lossy(), &data).map_err(|e| bad("font.install", format!("couldn't write the font: {e}")))?;
                to
            }
            // The very file is installed already: still added for this session and reported.
            None => dir.join(clean_name(&name)),
        };
        deckcraft_fonts::FontDb::global().add_font(data);
        installed.push(json!({"family": family, "styles": styles, "file": file.to_string_lossy()}));
    }
    if installed.is_empty() {
        return Err(bad("font.install", if paths.len() == 1 { "that file has no font deckrt can read" } else { "none of those files has a font deckrt can read" }));
    }
    Ok(json!({"installed": installed, "skipped": skipped}))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder of the test's own under the temp folder.
    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("deckrt-fonts-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("scratch");
        d
    }

    fn a_font() -> Vec<u8> {
        deckcraft_fonts::bundled().first().map(|b| b.to_vec()).unwrap_or_else(|| deckcraft_fonts::FontDb::global().face("Arial", "Regular").data().to_vec())
    }

    #[test]
    fn a_font_and_a_zip_of_fonts_are_copied_into_the_folder() {
        let tmp = scratch("copy");
        let font = a_font();
        assert!(!deckcraft_fonts::faces_in(&font).is_empty(), "the test font parses");
        let one = tmp.join("Mine.ttf");
        std::fs::write(&one, &font).expect("write");
        // A zip as a font site hands it out: a folder, the font, a licence, and macOS' copies.
        let zip_path = tmp.join("family.zip");
        {
            let f = std::fs::File::create(&zip_path).expect("zip");
            let mut z = zip::ZipWriter::new(f);
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("Family/static/Family-Bold.ttf", o).expect("entry");
            std::io::Write::write_all(&mut z, &font).expect("font");
            z.start_file("Family/OFL.txt", o).expect("entry");
            std::io::Write::write_all(&mut z, b"licence").expect("text");
            z.start_file("__MACOSX/Family/._Family-Bold.ttf", o).expect("entry");
            std::io::Write::write_all(&mut z, b"junk").expect("junk");
            z.finish().expect("finish");
        }
        let zip_bytes = std::fs::read(&zip_path).expect("read");
        assert!(takes("family.zip", &zip_bytes));
        assert!(!takes("notes.zip", b"PK"));
        let out_dir = tmp.join("fonts");
        let mut s = Session::new();
        let r = s
            .execute("font.install", &json!({"paths": [one.to_string_lossy(), zip_path.to_string_lossy(), tmp.join("gone.ttf").to_string_lossy()], "dir": out_dir.to_string_lossy()}))
            .expect("install");
        assert_eq!(r["installed"].as_array().map(Vec::len), Some(2), "{r}");
        assert!(out_dir.join("Mine.ttf").exists());
        assert!(out_dir.join("Family-Bold.ttf").exists(), "the zip's font, without its folders");
        assert_eq!(r["skipped"], json!(["gone.ttf"]));
        // The same files again: nothing new is written.
        let again = s.execute("font.install", &json!({"paths": [one.to_string_lossy()], "dir": out_dir.to_string_lossy()})).expect("again");
        assert_eq!(again["installed"].as_array().map(Vec::len), Some(1));
        assert_eq!(std::fs::read_dir(&out_dir).expect("dir").count(), 2);
    }

    #[test]
    fn a_file_that_is_not_a_font_is_refused() {
        let tmp = scratch("fake");
        let p = tmp.join("fake.ttf");
        std::fs::write(&p, b"not a font at all").expect("write");
        let mut s = Session::new();
        let r = s.execute("font.install", &json!({"paths": [p.to_string_lossy()], "dir": tmp.join("f").to_string_lossy()}));
        assert!(r.is_err());
        assert_eq!(clean_name("../../evil:name?.ttf"), "evil_name_.ttf");
    }
}
