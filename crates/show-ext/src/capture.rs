//! deckrt: Insert › Screenshot. Pictures of the open windows and of the whole screen.
//!
//! On Windows the windows are drawn with `PrintWindow` (with `PW_RENDERFULLCONTENT`, so browsers
//! and other GPU-drawn windows come out too) and the screen is copied with `BitBlt`. Elsewhere
//! both lists are empty.

/// One picture: RGBA, `w` × `h`, with where it was on the screen in physical pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Shot {
    pub title: String,
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
    pub origin: [i32; 2],
}

/// The open windows of other programs that can be seen, front to back, at most `max`.
pub fn windows(max: usize) -> Vec<Shot> {
    imp::windows(max)
}

/// Everything on every display, as one picture. This program's own windows are left out where
/// Windows allows it (Windows 10 version 2004 and later), so the picture shows what is behind them.
pub fn screen() -> Option<Shot> {
    imp::screen()
}

#[cfg(windows)]
mod imp {
    use super::Shot;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleBitmap, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject,
        GetDC, GetDIBits, HDC, HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject,
    };
    use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GWL_EXSTYLE, GetSystemMetrics, GetWindowLongW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
        SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
        WS_EX_TOOLWINDOW,
    };
    use windows::core::BOOL;

    /// `PrintWindow`'s flag for windows drawn by the GPU (Chrome, Edge, Office…).
    const PW_RENDERFULLCONTENT: PRINT_WINDOW_FLAGS = PRINT_WINDOW_FLAGS(2);
    /// Pictures larger than this on a side are not taken (a broken window size).
    const MAX_SIDE: i32 = 16_384;

    unsafe extern "system" fn each(h: HWND, l: LPARAM) -> BOOL {
        // SAFETY: `l` is the pointer to the `Vec` passed by `windows`, alive for the whole call.
        let out = unsafe { &mut *(l.0 as *mut Vec<HWND>) };
        out.push(h);
        BOOL(1)
    }

    fn title(h: HWND) -> String {
        let mut buf = [0u16; 256];
        // SAFETY: the buffer is valid for its length.
        let n = unsafe { GetWindowTextW(h, &mut buf) };
        String::from_utf16_lossy(buf.get(..n.max(0) as usize).unwrap_or(&[])).trim().to_string()
    }

    fn worth_showing(h: HWND, own: u32) -> Option<(String, RECT, RECT)> {
        // SAFETY: plain queries on a window handle EnumWindows just gave.
        unsafe {
            if !IsWindowVisible(h).as_bool() || IsIconic(h).as_bool() {
                return None;
            }
            if GetWindowLongW(h, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(h, Some(&mut pid as *mut u32));
            if pid == own {
                return None;
            }
            let mut cloaked = 0u32;
            if DwmGetWindowAttribute(h, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4).is_ok() && cloaked != 0 {
                return None;
            }
            let t = title(h);
            if t.is_empty() || t == "Program Manager" {
                return None;
            }
            let mut wr = RECT::default();
            GetWindowRect(h, &mut wr).ok()?;
            let mut fr = wr;
            if DwmGetWindowAttribute(h, DWMWA_EXTENDED_FRAME_BOUNDS, &mut fr as *mut RECT as *mut _, std::mem::size_of::<RECT>() as u32).is_err() {
                fr = wr;
            }
            let (w, hh) = (fr.right - fr.left, fr.bottom - fr.top);
            if w < 80 || hh < 60 || w > MAX_SIDE || hh > MAX_SIDE {
                return None;
            }
            Some((t, wr, fr))
        }
    }

    /// Read the bitmap selected into `dc` (`w` × `h`) as RGBA.
    fn read(dc: HDC, bmp: windows::Win32::Graphics::Gdi::HBITMAP, w: i32, h: i32) -> Option<Vec<u8>> {
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut px = vec![0u8; (w as usize) * (h as usize) * 4];
        // SAFETY: `px` holds w × h 32-bit pixels, the size `info` asks for.
        let n = unsafe { GetDIBits(dc, bmp, 0, h as u32, Some(px.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS) };
        if n == 0 {
            return None;
        }
        for p in px.chunks_exact_mut(4) {
            p.swap(0, 2);
            p[3] = 255;
        }
        Some(px)
    }

    /// Draw window `h` (its whole rect `wr`) and keep the visible frame `fr`.
    fn grab(h: HWND, wr: RECT, fr: RECT) -> Option<(u32, u32, Vec<u8>)> {
        let (w, hh) = (wr.right - wr.left, wr.bottom - wr.top);
        if w <= 0 || hh <= 0 || w > MAX_SIDE || hh > MAX_SIDE {
            return None;
        }
        // SAFETY: GDI objects are created, used and released here in order.
        let px = unsafe {
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            let bmp = CreateCompatibleBitmap(screen, w, hh);
            let old = SelectObject(dc, HGDIOBJ::from(bmp));
            let ok = PrintWindow(h, dc, PW_RENDERFULLCONTENT).as_bool();
            let px = if ok { read(dc, bmp, w, hh) } else { None };
            SelectObject(dc, old);
            let _ = DeleteObject(HGDIOBJ::from(bmp));
            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);
            px
        }?;
        // The visible frame inside the window rect (Windows 10 and 11 add invisible borders).
        let (ox, oy) = ((fr.left - wr.left).clamp(0, w), (fr.top - wr.top).clamp(0, hh));
        let (cw, ch) = ((fr.right - fr.left).clamp(1, w - ox), (fr.bottom - fr.top).clamp(1, hh - oy));
        let mut out = Vec::with_capacity((cw * ch * 4) as usize);
        for y in oy..oy + ch {
            let a = ((y * w + ox) * 4) as usize;
            out.extend_from_slice(px.get(a..a + (cw * 4) as usize)?);
        }
        Some((cw as u32, ch as u32, out))
    }

    fn all_windows() -> Vec<HWND> {
        let mut all: Vec<HWND> = vec![];
        // SAFETY: the callback only pushes into `all`, which outlives the call.
        let _ = unsafe { EnumWindows(Some(each), LPARAM(&mut all as *mut Vec<HWND> as isize)) };
        all
    }

    /// This program's visible top-level windows.
    fn own_windows() -> Vec<HWND> {
        let own = std::process::id();
        all_windows()
            .into_iter()
            .filter(|h| {
                let mut pid = 0u32;
                // SAFETY: plain queries on handles EnumWindows just gave.
                unsafe {
                    GetWindowThreadProcessId(*h, Some(&mut pid as *mut u32));
                    pid == own && IsWindowVisible(*h).as_bool()
                }
            })
            .collect()
    }

    pub fn windows(max: usize) -> Vec<Shot> {
        let all = all_windows();
        let own = std::process::id();
        let mut out = vec![];
        for h in all {
            if out.len() >= max {
                break;
            }
            let Some((t, wr, fr)) = worth_showing(h, own) else { continue };
            if let Some((w, hh, rgba)) = grab(h, wr, fr) {
                out.push(Shot { title: t, w, h: hh, rgba, origin: [fr.left, fr.top] });
            }
        }
        out
    }

    pub fn screen() -> Option<Shot> {
        let own = own_windows();
        // SAFETY: setting the capture affinity of this program's own windows.
        let hidden: Vec<HWND> = own.into_iter().filter(|h| unsafe { SetWindowDisplayAffinity(*h, WDA_EXCLUDEFROMCAPTURE) }.is_ok()).collect();
        if !hidden.is_empty() {
            // The compositor applies the affinity on its next frame.
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
        let shot = copy_screen();
        for h in hidden {
            // SAFETY: as above.
            let _ = unsafe { SetWindowDisplayAffinity(h, WDA_NONE) };
        }
        shot
    }

    fn copy_screen() -> Option<Shot> {
        // SAFETY: GDI objects are created, used and released here in order.
        unsafe {
            let (x, y) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
            let (w, h) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
            if w <= 0 || h <= 0 || w > MAX_SIDE || h > MAX_SIDE {
                return None;
            }
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            let bmp = CreateCompatibleBitmap(screen, w, h);
            let old = SelectObject(dc, HGDIOBJ::from(bmp));
            let ok = BitBlt(dc, 0, 0, w, h, Some(screen), x, y, SRCCOPY | CAPTUREBLT).is_ok();
            let px = if ok { read(dc, bmp, w, h) } else { None };
            SelectObject(dc, old);
            let _ = DeleteObject(HGDIOBJ::from(bmp));
            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);
            px.map(|rgba| Shot { title: "Screen".into(), w: w as u32, h: h as u32, rgba, origin: [x, y] })
        }
    }

    #[cfg(test)]
    mod tests {
        /// The screen copy has the virtual screen's size and opaque pixels. (CI runners have a
        /// desktop; the window list may be empty there.)
        #[test]
        fn the_screen_comes_back_whole() {
            if let Some(s) = super::screen() {
                assert_eq!(s.rgba.len(), (s.w * s.h * 4) as usize);
                assert!(s.rgba.chunks_exact(4).all(|p| p[3] == 255));
            }
            for w in super::windows(4) {
                assert_eq!(w.rgba.len(), (w.w * w.h * 4) as usize);
                assert!(!w.title.is_empty());
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Shot;

    pub fn windows(_max: usize) -> Vec<Shot> {
        vec![]
    }

    pub fn screen() -> Option<Shot> {
        None
    }
}
