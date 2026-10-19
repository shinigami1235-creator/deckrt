//! deckrt: telling Windows about a font Add Fonts put in the user fonts folder, so Word,
//! PowerPoint and the font pickers of other programs list it. On other systems a font in the
//! folder is enough and these do nothing.

use std::path::Path;

/// The name Windows gives a font in its list: "Lato Bold (TrueType)", "Inter (OpenType)".
pub fn listing_name(family: &str, styles: &[String], file: &Path) -> String {
    let shown: Vec<&str> = styles.iter().map(String::as_str).filter(|s| !s.eq_ignore_ascii_case("Regular")).collect();
    let face = if shown.is_empty() || styles.len() > 4 { family.to_string() } else { format!("{family} {}", shown.join(" & ")) };
    let kind = if file.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("otf") || e.eq_ignore_ascii_case("otc")) {
        "OpenType"
    } else {
        "TrueType"
    };
    format!("{face} ({kind})")
}

/// Register the font at `file` for this user and load it, so programs already open can use it.
#[cfg(windows)]
pub fn register(family: &str, styles: &[String], file: &Path) -> Result<(), String> {
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::Graphics::Gdi::AddFontResourceW;
    use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegSetKeyValueW};
    use windows::Win32::UI::WindowsAndMessaging::{HWND_BROADCAST, SendNotifyMessageW, WM_FONTCHANGE};
    use windows::core::PCWSTR;
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let key = wide("Software\\Microsoft\\Windows NT\\CurrentVersion\\Fonts");
    let name = wide(&listing_name(family, styles, file));
    let path_text = file.to_string_lossy().to_string();
    let value = wide(&path_text);
    // SAFETY: every pointer is to a live, NUL-terminated UTF-16 buffer, and the data size is the
    // value's length in bytes, terminator included.
    let err = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            REG_SZ.0,
            Some(value.as_ptr().cast()),
            (value.len() * 2) as u32,
        )
    };
    if err.0 != 0 {
        return Err(format!("Windows didn't list the font (error {})", err.0));
    }
    // SAFETY: a NUL-terminated path; the broadcast only posts a notice to every top window.
    unsafe {
        let _ = AddFontResourceW(PCWSTR(value.as_ptr()));
        let _ = SendNotifyMessageW(HWND_BROADCAST, WM_FONTCHANGE, WPARAM(0), LPARAM(0));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn register(_family: &str, _styles: &[String], _file: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_windows_fonts_list() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(listing_name("Lato", &s(&["Bold"]), Path::new("Lato-Bold.ttf")), "Lato Bold (TrueType)");
        assert_eq!(listing_name("Lato", &s(&["Regular"]), Path::new("Lato-Regular.ttf")), "Lato (TrueType)");
        assert_eq!(listing_name("Inter", &s(&["Thin", "Light", "Regular", "Medium", "Bold", "Black"]), Path::new("Inter.otf")), "Inter (OpenType)");
    }
}
