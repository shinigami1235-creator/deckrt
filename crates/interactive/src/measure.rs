//! The measurement overlay's maths. Handles are points in the object's own box (0 to 1 across its
//! width and height); every length and angle is corrected by the box's pixel aspect, since a box
//! is rarely square.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    Thirds,
    Fifths,
    Angle,
    Ratio,
}

impl Preset {
    pub const ALL: [Preset; 4] = [Preset::Thirds, Preset::Fifths, Preset::Angle, Preset::Ratio];

    pub fn label(self) -> &'static str {
        match self {
            Preset::Thirds => "Thirds",
            Preset::Fifths => "Fifths",
            Preset::Angle => "Angle",
            Preset::Ratio => "Ratio",
        }
    }

    pub fn from_id(s: &str) -> Option<Preset> {
        match s {
            "thirds" => Some(Preset::Thirds),
            "fifths" => Some(Preset::Fifths),
            "angle" => Some(Preset::Angle),
            "ratio" => Some(Preset::Ratio),
            _ => None,
        }
    }
}

/// `[x, y]` in the object's box.
pub type Handle = [f64; 2];

/// Where the handles start. Thirds and fifths carry the two ends of the face and the dividers
/// between the bands, so the bands start equal and the dividers can be moved to where the landmarks
/// fall. An angle is two arms about a vertex. A ratio is two segments.
pub fn default_handles(preset: Preset) -> Vec<Handle> {
    match preset {
        Preset::Thirds => vec![[0.5, 0.08], [0.5, 0.08 + 0.84 / 3.0], [0.5, 0.08 + 2.0 * 0.84 / 3.0], [0.5, 0.92]],
        Preset::Fifths => (0..6).map(|i| [0.08 + 0.84 * i as f64 / 5.0, 0.5]).collect(),
        Preset::Angle => vec![[0.2, 0.25], [0.5, 0.75], [0.8, 0.25]],
        Preset::Ratio => vec![[0.3, 0.15], [0.3, 0.45], [0.65, 0.15], [0.65, 0.85]],
    }
}

fn clamp01(v: f64) -> f64 {
    if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }
}

/// Which axis the bands of a preset are measured along: `1` for thirds (y), `0` for fifths (x).
fn band_axis(preset: Preset) -> usize {
    if preset == Preset::Thirds { 1 } else { 0 }
}

/// Handle `i` moved to `to`. For thirds and fifths the two ends carry the dividers with them, so
/// the bands keep their proportions when the face is re-fitted, and a divider stays between its
/// neighbours.
pub fn move_handle(preset: Preset, handles: &[Handle], i: usize, to: Handle) -> Vec<Handle> {
    let mut next: Vec<Handle> = handles.to_vec();
    if i >= next.len() {
        return next;
    }
    let p = [clamp01(to[0]), clamp01(to[1])];
    if !matches!(preset, Preset::Thirds | Preset::Fifths) {
        next[i] = p;
        return next;
    }
    let a = band_axis(preset);
    let last = next.len() - 1;
    if i == 0 || i == last {
        let from0 = handles[0][a];
        let from1 = handles[last][a];
        let span = from1 - from0;
        let to0 = if i == 0 { p[a] } else { from0 };
        let to1 = if i == last { p[a] } else { from1 };
        for k in 1..last {
            let t = if span.abs() < 1e-9 { k as f64 / last as f64 } else { (handles[k][a] - from0) / span };
            next[k][a] = clamp01(to0 + t * (to1 - to0));
        }
        next[i] = p;
        return next;
    }
    let lo = next[i - 1][a].min(next[i + 1][a]);
    let hi = next[i - 1][a].max(next[i + 1][a]);
    next[i] = p;
    next[i][a] = p[a].clamp(lo, hi);
    next
}

/// Each band's share of the whole, in percent. Zero-length spans give equal shares.
pub fn band_shares(positions: &[f64]) -> Vec<f64> {
    let n = positions.len().saturating_sub(1);
    if n == 0 {
        return vec![];
    }
    let total = positions[n] - positions[0];
    if total.abs() < 1e-9 {
        return vec![100.0 / n as f64; n];
    }
    (1..=n).map(|k| (positions[k] - positions[k - 1]) * 100.0 / total).collect()
}

/// The angle at `v` between the arms to `a` and `b`, in degrees, in a box of pixel aspect
/// `aspect` (width over height).
pub fn angle_at(a: Handle, v: Handle, b: Handle, aspect: f64) -> f64 {
    let ax = (a[0] - v[0]) * aspect;
    let ay = a[1] - v[1];
    let bx = (b[0] - v[0]) * aspect;
    let by = b[1] - v[1];
    let la = ax.hypot(ay);
    let lb = bx.hypot(by);
    if la < 1e-9 || lb < 1e-9 {
        return 0.0;
    }
    ((ax * bx + ay * by) / (la * lb)).clamp(-1.0, 1.0).acos().to_degrees()
}

/// Length of a segment in units of the box height.
pub fn segment_length(a: Handle, b: Handle, aspect: f64) -> f64 {
    ((b[0] - a[0]) * aspect).hypot(b[1] - a[1])
}

pub struct Reading {
    /// `upper 33.4, middle 33.1, lower 33.5`, `132°`, `1 : 1.61`.
    pub text: String,
    /// The two lengths of a ratio.
    pub detail: Option<String>,
}

const THIRDS: [&str; 3] = ["upper", "middle", "lower"];
const FIFTHS: [&str; 5] = ["left", "left eye", "centre", "right eye", "right"];

/// What the handles measure, as the room reads it.
pub fn reading(preset: Preset, handles: &[Handle], aspect: f64) -> Reading {
    let h = |i: usize| handles.get(i).copied().unwrap_or([0.5, 0.5]);
    match preset {
        Preset::Thirds | Preset::Fifths => {
            let names: &[&str] = if preset == Preset::Thirds { &THIRDS } else { &FIFTHS };
            let a = band_axis(preset);
            let shares = band_shares(&handles.iter().map(|h| h[a]).collect::<Vec<_>>());
            let text =
                shares.iter().enumerate().map(|(k, s)| format!("{} {:.1}", names.get(k).unwrap_or(&"band"), s.abs())).collect::<Vec<_>>().join(", ");
            Reading { text, detail: None }
        }
        Preset::Angle => Reading { text: format!("{}°", angle_at(h(0), h(1), h(2), aspect).round()), detail: None },
        Preset::Ratio => {
            let a = segment_length(h(0), h(1), aspect) * 100.0;
            let b = segment_length(h(2), h(3), aspect) * 100.0;
            let ratio = if a < 1e-6 { "1 : ?".to_string() } else { format!("1 : {:.2}", b / a) };
            Reading { text: ratio, detail: Some(format!("{a:.1} and {b:.1}")) }
        }
    }
}

/// The lines to draw, as pairs of points in the box: bands span the box across their axis; an
/// angle is two arms; a ratio two segments.
pub fn lines(preset: Preset, handles: &[Handle]) -> Vec<(Handle, Handle)> {
    let h = |i: usize| handles.get(i).copied().unwrap_or([0.5, 0.5]);
    match preset {
        Preset::Thirds => handles.iter().map(|h| ([0.0, h[1]], [1.0, h[1]])).collect(),
        Preset::Fifths => handles.iter().map(|h| ([h[0], 0.0], [h[0], 1.0])).collect(),
        Preset::Angle => vec![(h(1), h(0)), (h(1), h(2))],
        Preset::Ratio => vec![(h(0), h(1)), (h(2), h(3))],
    }
}

/// A well-formed handle list for the preset: the right count, finite numbers inside the box.
pub fn sanitize(preset: Preset, raw: &[Handle]) -> Option<Vec<Handle>> {
    if raw.len() != default_handles(preset).len() || raw.iter().any(|h| !h[0].is_finite() || !h[1].is_finite()) {
        return None;
    }
    Some(raw.iter().map(|h| [clamp01(h[0]), clamp01(h[1])]).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_thirds_read_a_third_each() {
        let r = reading(Preset::Thirds, &default_handles(Preset::Thirds), 1.0);
        assert_eq!(r.text, "upper 33.3, middle 33.3, lower 33.3");
    }

    #[test]
    fn a_moved_divider_changes_the_shares_and_stays_between_its_neighbours() {
        let h = default_handles(Preset::Thirds);
        let moved = move_handle(Preset::Thirds, &h, 1, [0.5, 0.2]);
        let r = reading(Preset::Thirds, &moved, 1.0);
        assert!(r.text.starts_with("upper 14.3"), "{}", r.text);
        let pinned = move_handle(Preset::Thirds, &h, 1, [0.5, 0.0]);
        assert_eq!(pinned[1][1], h[0][1], "a divider cannot pass the end");
    }

    #[test]
    fn an_end_carries_the_dividers_by_proportion() {
        let h = default_handles(Preset::Fifths);
        let moved = move_handle(Preset::Fifths, &h, 5, [0.5, 0.5]);
        let r = reading(Preset::Fifths, &moved, 1.0);
        assert_eq!(r.text, "left 20.0, left eye 20.0, centre 20.0, right eye 20.0, right 20.0");
    }

    #[test]
    fn a_right_angle_opens_in_a_wide_box() {
        let a = angle_at([0.0, 0.5], [0.5, 0.5], [0.5, 0.0], 1.0);
        assert!((a - 90.0).abs() < 1e-9);
        let wide = angle_at([0.0, 0.5], [0.5, 0.5], [0.5, 0.0], 2.0);
        assert!((wide - 90.0).abs() < 1e-9, "a right angle between an axis-aligned pair stays right");
        let diag = angle_at([0.0, 0.0], [0.5, 0.5], [1.0, 0.0], 2.0);
        assert!((diag - 127.0).abs() < 1.0, "{diag}");
    }

    #[test]
    fn ratio_reading() {
        let r = reading(Preset::Ratio, &[[0.0, 0.0], [0.0, 0.5], [0.5, 0.0], [0.5, 0.8]], 1.0);
        assert_eq!(r.text, "1 : 1.60");
        assert_eq!(r.detail.as_deref(), Some("50.0 and 80.0"));
    }

    #[test]
    fn sanitize_rejects_the_wrong_count() {
        assert!(sanitize(Preset::Angle, &[[0.0, 0.0]]).is_none());
        assert_eq!(sanitize(Preset::Angle, &[[2.0, -1.0], [0.5, 0.5], [0.1, 0.1]]).map(|h| h[0]), Some([1.0, 0.0]));
    }
}
