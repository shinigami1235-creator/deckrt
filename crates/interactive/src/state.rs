//! What an object is doing during a show. The presenter view changes it, the audience screen
//! draws it, and it is never written to the deck.

use std::collections::BTreeMap;

use crate::injection::{self, Point};
use crate::measure::{self, Handle, Preset};
use crate::object::{CameraPose, Config, InteractiveObject};

#[derive(Clone, Debug, PartialEq)]
pub enum TypeState {
    Model {
        /// Layer name → shown.
        layers: BTreeMap<String, bool>,
        /// The camera as turned during the show; `None` is the configured start.
        camera: Option<CameraPose>,
        /// Yaw and pitch in degrees applied by dragging, on top of the configured rotation.
        turn: [f64; 2],
        /// Zoom factor from the wheel, 1 at the start.
        zoom: f64,
    },
    Swap {
        index: usize,
        /// The strip of all pictures is open (on-click `picker`).
        picker: bool,
    },
    Wipe {
        /// Divider position, 0 to 1 of the object width.
        position: f64,
    },
    Zoom {
        zoomed: bool,
        /// Pan of the zoomed view in slide units, applied to the region centre.
        pan: [f64; 2],
        /// When the zoom started or ended, for the ease.
        since: f64,
    },
    Popup {
        open: bool,
    },
    Poll {
        /// Phones may vote.
        open: bool,
        /// The right answer is marked on the audience screen.
        revealed: bool,
        /// Phone id → option (a rating's value is stored as `value - 1`).
        votes: BTreeMap<String, usize>,
        /// Phone id → words (word cloud) or one answer (free text).
        texts: BTreeMap<String, Vec<String>>,
        /// Phone id → the answers in that phone's order (ranking).
        ranks: BTreeMap<String, Vec<usize>>,
        /// Phones whose free-text answer the presenter hid from the screen.
        hidden: std::collections::BTreeSet<String>,
    },
    Scrub {
        index: usize,
    },
    Measure {
        preset: Preset,
        handles: Vec<Handle>,
    },
    InjectionMap {
        points: Vec<Point>,
        /// The product the next point takes.
        product: usize,
        /// The dose the next point takes.
        dose: f64,
        /// The points stay across slide changes.
        pinned: bool,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObjectState {
    pub visible: bool,
    pub st: TypeState,
}

impl ObjectState {
    /// The state an object starts in when its slide is entered.
    pub fn initial(obj: &InteractiveObject) -> ObjectState {
        let st = match &obj.config {
            Config::Model(c) => TypeState::Model {
                layers: c.layers.iter().map(|l| (l.clone(), !c.hidden_layers.contains(l))).collect(),
                camera: c.camera,
                turn: [0.0, 0.0],
                zoom: 1.0,
            },
            Config::ImageSwap(c) => TypeState::Swap { index: c.index.min(c.images.len().saturating_sub(1)), picker: false },
            Config::Wipe(c) => TypeState::Wipe { position: c.initial_position.clamp(0.0, 1.0) },
            Config::Zoom(_) => TypeState::Zoom { zoomed: false, pan: [0.0, 0.0], since: f64::NEG_INFINITY },
            Config::Popup(_) => TypeState::Popup { open: false },
            Config::Poll(_) => TypeState::Poll { open: false, revealed: false, votes: BTreeMap::new(), texts: BTreeMap::new(), ranks: BTreeMap::new(), hidden: Default::default() },
            Config::Scrub(c) => TypeState::Scrub { index: c.initial_index.min(c.images.len().saturating_sub(1)) },
            Config::Measure(c) => TypeState::Measure { preset: c.preset, handles: measure::default_handles(c.preset) },
            Config::InjectionMap(c) => TypeState::InjectionMap { points: vec![], product: 0, dose: injection::default_dose(c, 0), pinned: false },
        };
        ObjectState { visible: obj.visible, st }
    }

    /// What a click, or the object's shortcut key, does. `now` is the show clock in seconds.
    /// Returns whether anything changed.
    pub fn fire(&mut self, obj: &InteractiveObject, now: f64) -> bool {
        match (&mut self.st, &obj.config) {
            (TypeState::Swap { index, picker }, Config::ImageSwap(c)) => {
                if c.on_click == "picker" {
                    *picker = !*picker;
                } else {
                    *index = wrap(*index + 1, c.images.len());
                }
                true
            }
            (TypeState::Zoom { zoomed, pan, since }, _) => {
                *zoomed = !*zoomed;
                *pan = [0.0, 0.0];
                *since = now;
                true
            }
            (TypeState::Popup { open }, _) => {
                *open = !*open;
                true
            }
            (TypeState::Poll { open, revealed, .. }, _) => {
                // The shortcut key opens voting, then closes it; a revealed poll stays closed.
                if !*revealed {
                    *open = !*open;
                    true
                } else {
                    false
                }
            }
            (TypeState::Scrub { index }, Config::Scrub(c)) => {
                // Comes round to the first after the last, so the key keeps working at the end.
                *index = if *index + 1 >= c.images.len() { 0 } else { *index + 1 };
                true
            }
            (TypeState::InjectionMap { points, .. }, _) => {
                // The shortcut takes the last point back; placing one needs a position.
                points.pop().is_some()
            }
            (TypeState::Model { layers, turn, zoom, .. }, Config::Model(c)) => {
                // The key peels the model: each press hides the next layer in the file's order;
                // after the last one is reached, every layer shows again. A model with no layers
                // goes back to its starting view.
                if c.layers.len() < 2 {
                    *turn = [0.0, 0.0];
                    *zoom = 1.0;
                    return true;
                }
                let hidden = c.layers.iter().filter(|l| layers.get(*l) == Some(&false)).count();
                if hidden + 1 >= c.layers.len() {
                    for l in &c.layers {
                        layers.insert(l.clone(), true);
                    }
                } else {
                    let next = c.layers.iter().find(|l| layers.get(*l) != Some(&false));
                    if let Some(l) = next {
                        layers.insert(l.clone(), false);
                    }
                }
                true
            }
            (TypeState::Model { .. }, _) | (TypeState::Wipe { .. }, _) | (TypeState::Measure { .. }, _) => false,
            _ => false,
        }
    }

    /// Reset to the start: the configured camera and layers, the first picture, no votes.
    pub fn reset(&mut self, obj: &InteractiveObject) {
        *self = ObjectState::initial(obj);
    }
}

pub fn wrap(i: usize, count: usize) -> usize {
    if count == 0 { 0 } else { i % count }
}

/// The transform that makes `source` fill the slide: the scale and the slide-space point that
/// lands at the slide centre.
pub struct ZoomView {
    pub scale: f64,
    pub cx: f64,
    pub cy: f64,
}

pub fn zoom_view(source: [f64; 4], max_scale: f64, pan: [f64; 2]) -> ZoomView {
    let scale = (1.0 / source[2].max(1e-6)).min(1.0 / source[3].max(1e-6)).min(max_scale).max(1.0);
    let half = 0.5 / scale;
    let cx = (source[0] + source[2] / 2.0 + pan[0]).clamp(half, 1.0 - half);
    let cy = (source[1] + source[3] / 2.0 + pan[1]).clamp(half, 1.0 - half);
    ZoomView { scale, cx, cy }
}

/// Ease-out progress of a zoom that started at `since`, 0 to 1.
pub fn zoom_progress(since: f64, now: f64, duration_ms: u32) -> f64 {
    let d = duration_ms as f64 / 1000.0;
    if d <= 0.0 || !since.is_finite() {
        return 1.0;
    }
    let t = ((now - since) / d).clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t)
}

/// After leaving a slide: every injection map that is not pinned drops its points.
pub fn clear_unpinned<K: Ord>(states: &mut BTreeMap<K, ObjectState>) {
    for s in states.values_mut() {
        if let TypeState::InjectionMap { points, pinned, .. } = &mut s.st
            && !*pinned
        {
            points.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{InteractiveObject, Kind, MediaRef, ScrubConfig, SwapConfig};
    use deckcraft_model::MediaId;

    fn obj(c: Config) -> InteractiveObject {
        InteractiveObject::new(c)
    }

    #[test]
    fn a_models_key_peels_its_layers_then_shows_them_all() {
        let o = obj(Config::Model(crate::object::ModelConfig { layers: vec!["skin".into(), "muscle".into(), "bone".into()], ..Default::default() }));
        let mut s = ObjectState::initial(&o);
        let shown = |s: &ObjectState| match &s.st {
            TypeState::Model { layers, .. } => layers.iter().filter(|(_, on)| **on).map(|(n, _)| n.clone()).collect::<Vec<_>>(),
            _ => vec![],
        };
        assert!(s.fire(&o, 0.0));
        assert_eq!(shown(&s), vec!["bone", "muscle"]);
        s.fire(&o, 0.0);
        assert_eq!(shown(&s), vec!["bone"]);
        s.fire(&o, 0.0);
        assert_eq!(shown(&s), vec!["bone", "muscle", "skin"]);
        // No layers: the key puts the view back.
        let o = obj(Config::Model(Default::default()));
        let mut s = ObjectState::initial(&o);
        if let TypeState::Model { turn, .. } = &mut s.st {
            *turn = [40.0, 0.0];
        }
        assert!(s.fire(&o, 0.0));
        assert!(matches!(s.st, TypeState::Model { turn: [0.0, 0.0], .. }));
    }

    #[test]
    fn a_swap_advances_and_wraps() {
        let o = obj(Config::ImageSwap(SwapConfig {
            images: vec![MediaRef(MediaId(1)), MediaRef(MediaId(2)), MediaRef(MediaId(3))],
            index: 2,
            on_click: "advance".into(),
        }));
        let mut s = ObjectState::initial(&o);
        assert_eq!(s.st, TypeState::Swap { index: 2, picker: false });
        assert!(s.fire(&o, 0.0));
        assert_eq!(s.st, TypeState::Swap { index: 0, picker: false });
    }

    #[test]
    fn a_scrub_comes_round_and_an_empty_one_stays_put() {
        let o = obj(Config::Scrub(ScrubConfig {
            images: vec![MediaRef(MediaId(1)), MediaRef(MediaId(2))],
            labels: vec!["a".into(), "b".into()],
            initial_index: 1,
        }));
        let mut s = ObjectState::initial(&o);
        s.fire(&o, 0.0);
        assert_eq!(s.st, TypeState::Scrub { index: 0 });
        let e = obj(Config::Scrub(ScrubConfig::default()));
        let mut s = ObjectState::initial(&e);
        s.fire(&e, 0.0);
        assert_eq!(s.st, TypeState::Scrub { index: 0 });
    }

    #[test]
    fn a_revealed_poll_refuses_to_reopen() {
        let o = obj(Config::default_for(Kind::Poll));
        let mut s = ObjectState::initial(&o);
        assert!(s.fire(&o, 0.0));
        if let TypeState::Poll { open, revealed, .. } = &mut s.st {
            assert!(*open);
            *revealed = true;
            *open = false;
        }
        assert!(!s.fire(&o, 0.0));
    }

    #[test]
    fn zoom_fills_the_slide_and_clamps_the_pan() {
        let v = zoom_view([0.25, 0.25, 0.5, 0.5], 2.5, [0.0, 0.0]);
        assert!((v.scale - 2.0).abs() < 1e-9);
        assert!((v.cx - 0.5).abs() < 1e-9);
        let v = zoom_view([0.25, 0.25, 0.5, 0.5], 2.5, [9.0, -9.0]);
        assert!((v.cx - 0.75).abs() < 1e-9);
        assert!((v.cy - 0.25).abs() < 1e-9);
        assert_eq!(zoom_progress(0.0, 1.0, 300), 1.0);
        assert_eq!(zoom_progress(0.0, 0.0, 300), 0.0);
    }

    #[test]
    fn unpinned_points_go_with_the_slide() {
        let o = obj(Config::default_for(Kind::InjectionMap));
        let mut s = ObjectState::initial(&o);
        if let TypeState::InjectionMap { points, .. } = &mut s.st {
            points.push(Point { x: 0.5, y: 0.5, product: 0, dose: 4.0 });
        }
        let mut all = BTreeMap::new();
        all.insert(1, s.clone());
        if let TypeState::InjectionMap { pinned, .. } = &mut s.st {
            *pinned = true;
        }
        all.insert(2, s);
        clear_unpinned(&mut all);
        let n = |k: u32| match &all[&k].st {
            TypeState::InjectionMap { points, .. } => points.len(),
            _ => 99,
        };
        assert_eq!((n(1), n(2)), (0, 1));
    }
}
