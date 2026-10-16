//! deckrt: the connected displays, so a show can go to the projector and the presenter window
//! to the laptop. The order matches winit's `available_monitors()`, which is what
//! `egui::ViewportCommand::SetMonitor` takes: on Windows both come from `EnumDisplayMonitors`.
//! On X11 the list comes from `xrandr --listmonitors`; on anything else it is empty and the show
//! stays full screen on the current display.

use crate::DisplayInfo;

pub fn list() -> Vec<DisplayInfo> {
    let out = imp::list();
    log::info!("deckrt: {} display(s): {:?}", out.len(), out);
    out
}

#[cfg(windows)]
mod imp {
    use super::DisplayInfo;
    use windows::Win32::Foundation::{LPARAM, RECT};
    use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOF_PRIMARY};
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows::core::BOOL;

    unsafe extern "system" fn each(h: HMONITOR, _: HDC, _: *mut RECT, l: LPARAM) -> BOOL {
        // SAFETY: `l` is the pointer to the `Vec` passed by `list`, alive for the whole call.
        let out = unsafe { &mut *(l.0 as *mut Vec<DisplayInfo>) };
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        // SAFETY: `info` is a properly sized MONITORINFO.
        if unsafe { GetMonitorInfoW(h, &mut info) }.as_bool() {
            let r = info.rcMonitor;
            let (mut dx, mut dy) = (96u32, 96u32);
            // SAFETY: plain out-pointers to two u32s.
            if unsafe { GetDpiForMonitor(h, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) }.is_err() {
                dx = 96;
            }
            out.push(DisplayInfo {
                index: out.len(),
                origin: [r.left, r.top],
                size_px: [(r.right - r.left).max(0) as u32, (r.bottom - r.top).max(0) as u32],
                scale: (dx as f32 / 96.0).max(0.5),
                primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
            });
        }
        BOOL(1)
    }

    pub fn list() -> Vec<DisplayInfo> {
        let mut out: Vec<DisplayInfo> = vec![];
        // SAFETY: the callback only touches the `Vec` whose address it is given.
        let _ = unsafe { EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut out as *mut Vec<DisplayInfo> as isize)) };
        out
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
mod imp {
    use super::DisplayInfo;

    /// ` 0: +*DP-1 1920/527x1080/296+0+0  DP-1`
    pub(super) fn parse(listing: &str) -> Vec<DisplayInfo> {
        let mut out = vec![];
        for line in listing.lines() {
            let t = line.trim();
            let Some(rest) = t.split_once(':').map(|(_, r)| r.trim()) else { continue };
            let mut parts = rest.split_whitespace();
            let (Some(flags), Some(geom)) = (parts.next(), parts.next()) else { continue };
            // 1920/527x1080/296+0+0
            let Some((wh, xy)) = geom.split_once('+') else { continue };
            let Some((w, h)) = wh.split_once('x') else { continue };
            let Some((x, y)) = xy.split_once('+') else { continue };
            let num = |s: &str| s.split('/').next().and_then(|n| n.parse::<i64>().ok());
            let (Some(w), Some(h), Some(x), Some(y)) = (num(w), num(h), num(x), num(y)) else { continue };
            out.push(DisplayInfo {
                index: out.len(),
                origin: [x as i32, y as i32],
                size_px: [w.max(0) as u32, h.max(0) as u32],
                scale: 1.0,
                primary: flags.contains('*'),
            });
        }
        out
    }

    pub fn list() -> Vec<DisplayInfo> {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("DISPLAY").is_none() {
            return vec![];
        }
        let out = std::process::Command::new("xrandr").arg("--listmonitors").output();
        match out {
            Ok(o) if o.status.success() => parse(&String::from_utf8_lossy(&o.stdout)),
            _ => vec![],
        }
    }
}

#[cfg(not(any(windows, all(unix, not(target_os = "macos")))))]
mod imp {
    use super::DisplayInfo;
    pub fn list() -> Vec<DisplayInfo> {
        vec![]
    }
}

#[cfg(all(test, unix, not(target_os = "macos")))]
mod tests {
    #[test]
    fn xrandr_listing_parses() {
        let s = "Monitors: 2\n 0: +*eDP-1 2560/344x1600/215+0+0  eDP-1\n 1: +HDMI-1 1920/527x1080/296+2560+0  HDMI-1\n";
        let d = super::imp::parse(s);
        assert_eq!(d.len(), 2);
        assert!(d[0].primary && !d[1].primary);
        assert_eq!(d[1].origin, [2560, 0]);
        assert_eq!(d[1].size_px, [1920, 1080]);
    }
}
