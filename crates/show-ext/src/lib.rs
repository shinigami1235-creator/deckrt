//! deckrt's slide show extras that need the platform.
//!
//! - [`capture`]: pictures of the open windows and the screen, for Insert › Screenshot.
//! - [`displays`]: the connected displays, in the order `egui::ViewportCommand::SetMonitor` takes.
//! - [`split`], [`presenter_frame`]: which display the audience screen and the presenter window
//!   go to.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod capture;
pub mod displays;

/// One connected display, in `available_monitors()` order (the index
/// `egui::ViewportCommand::SetMonitor` takes).
#[derive(Clone, Debug, PartialEq)]
pub struct DisplayInfo {
    pub index: usize,
    /// Top-left corner in physical pixels.
    pub origin: [i32; 2],
    pub size_px: [u32; 2],
    /// Physical pixels per point on this display.
    pub scale: f32,
    pub primary: bool,
}

impl DisplayInfo {
    pub fn label(&self) -> String {
        format!("Display {} ({}×{}){}", self.index + 1, self.size_px[0], self.size_px[1], if self.primary { ", main" } else { "" })
    }
}

/// The audience display when two or more are connected: the first one that is not the primary.
/// The presenter window stays on the primary. Returns `(audience, presenter)`.
pub fn split(displays: &[DisplayInfo]) -> Option<(usize, usize)> {
    if displays.len() < 2 {
        return None;
    }
    let presenter = displays.iter().find(|d| d.primary).or(displays.first())?;
    let audience = displays.iter().find(|d| d.index != presenter.index)?;
    Some((audience.index, presenter.index))
}

/// The presenter window frame covering `display`, in points: `(x, y, w, h)`.
pub fn presenter_frame(display: &DisplayInfo) -> Option<[f32; 4]> {
    let scale = display.scale;
    if !scale.is_finite() || scale <= 0.0 || display.size_px[0] == 0 || display.size_px[1] == 0 {
        return None;
    }
    let f =
        [display.origin[0] as f32 / scale, display.origin[1] as f32 / scale, display.size_px[0] as f32 / scale, display.size_px[1] as f32 / scale];
    (f.iter().all(|v| v.is_finite()) && f[2] > 1.0 && f[3] > 1.0).then_some(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(index: usize, origin: [i32; 2], size_px: [u32; 2], scale: f32, primary: bool) -> DisplayInfo {
        DisplayInfo { index, origin, size_px, scale, primary }
    }

    #[test]
    fn one_display_keeps_the_show_where_it_is() {
        assert_eq!(split(&[]), None);
        assert_eq!(split(&[display(0, [0, 0], [1920, 1080], 1.0, true)]), None);
    }

    #[test]
    fn the_show_goes_to_the_other_display_and_the_presenter_stays_on_the_primary() {
        let laptop = display(0, [0, 0], [3024, 1964], 2.0, true);
        let projector = display(1, [3024, 0], [1920, 1080], 1.0, false);
        assert_eq!(split(&[laptop.clone(), projector.clone()]), Some((1, 0)));
        assert_eq!(split(&[projector.clone(), laptop.clone()]), Some((1, 0)));
        assert_eq!(presenter_frame(&laptop), Some([0.0, 0.0, 1512.0, 982.0]));
        assert!(presenter_frame(&display(0, [0, 0], [0, 10], 1.0, true)).is_none());
        assert_eq!(laptop.label(), "Display 1 (3024×1964), main");
    }

    #[test]
    fn without_a_primary_the_first_display_is_the_presenters() {
        let a = display(0, [0, 0], [800, 600], 1.0, false);
        let b = display(1, [800, 0], [1920, 1080], 1.0, false);
        assert_eq!(split(&[a, b]), Some((1, 0)));
    }
}
