//! An object follows the shape it is anchored to. After an edit, the objects on a slide whose
//! anchor shape moved are moved by the same amount; an object moved by hand keeps its new place
//! and remembers the new offset.

use deckcraft_geom::Xfrm;
use deckcraft_model::{Shape, ShapeId};

use crate::object::{self, Anchor};

fn find(shapes: &[Shape], id: ShapeId) -> Option<&Shape> {
    shapes.iter().find(|s| s.id == id)
}

/// The offset of `obj_box` from `anchor_box`.
pub fn offset(anchor_box: &Xfrm, obj_box: &Xfrm) -> (f64, f64) {
    (obj_box.x - anchor_box.x, obj_box.y - anchor_box.y)
}

/// Bring the objects on `shapes` (the new slide) into line with their anchors, given the slide as
/// it was before the edit. Returns whether anything moved.
pub fn follow(before: &[Shape], shapes: &mut [Shape]) -> bool {
    let mut moved = false;
    let n = shapes.len();
    for i in 0..n {
        let Some(mut obj) = shapes.get(i).and_then(object::read) else { continue };
        let Some(a) = obj.anchor else { continue };
        let Some(anchor_now) = find(shapes, a.shape_id()).and_then(|s| s.xfrm) else { continue };
        let Some(mine_now) = shapes.get(i).and_then(|s| s.xfrm) else { continue };
        let id = shapes[i].id;
        let mine_before = find(before, id).and_then(|s| s.xfrm);
        let anchor_before = find(before, a.shape_id()).and_then(|s| s.xfrm);
        let i_moved = mine_before.is_some_and(|b| (b.x - mine_now.x).abs() > 1e-6 || (b.y - mine_now.y).abs() > 1e-6);
        let anchor_moved = anchor_before.is_some_and(|b| (b.x - anchor_now.x).abs() > 1e-6 || (b.y - anchor_now.y).abs() > 1e-6);
        if i_moved || (!anchor_moved && mine_before.is_none()) {
            // Moved by hand (or just placed): keep the place, learn the offset.
            let (dx, dy) = offset(&anchor_now, &mine_now);
            if (dx - a.dx).abs() > 1e-6 || (dy - a.dy).abs() > 1e-6 {
                obj.anchor = Some(Anchor { shape: a.shape, dx, dy });
                if let Some(s) = shapes.get_mut(i) {
                    object::write(s, &obj);
                }
            }
        } else if anchor_moved {
            let want = Xfrm { x: anchor_now.x + a.dx, y: anchor_now.y + a.dy, ..mine_now };
            if (want.x - mine_now.x).abs() > 1e-6 || (want.y - mine_now.y).abs() > 1e-6 {
                if let Some(s) = shapes.get_mut(i) {
                    s.xfrm = Some(want);
                }
                moved = true;
            }
        }
    }
    moved
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{Config, InteractiveObject, Kind};

    fn shape(id: u32, x: f64, y: f64) -> Shape {
        Shape { id: ShapeId(id), xfrm: Some(Xfrm::new(x, y, 100.0, 50.0)), ..Default::default() }
    }

    fn object(id: u32, x: f64, y: f64, anchor: u32, dx: f64, dy: f64) -> Shape {
        let mut s = shape(id, x, y);
        let mut o = InteractiveObject::new(Config::default_for(Kind::Zoom));
        o.anchor = Some(Anchor { shape: anchor, dx, dy });
        object::write(&mut s, &o);
        s
    }

    #[test]
    fn the_object_moves_with_its_anchor() {
        let before = vec![shape(2, 10.0, 10.0), object(3, 15.0, 20.0, 2, 5.0, 10.0)];
        let mut after = vec![shape(2, 110.0, 10.0), object(3, 15.0, 20.0, 2, 5.0, 10.0)];
        assert!(follow(&before, &mut after));
        assert_eq!(after[1].xfrm.map(|x| (x.x, x.y)), Some((115.0, 20.0)));
    }

    #[test]
    fn a_hand_moved_object_keeps_its_place_and_learns_the_offset() {
        let before = vec![shape(2, 10.0, 10.0), object(3, 15.0, 20.0, 2, 5.0, 10.0)];
        let mut after = vec![shape(2, 10.0, 10.0), object(3, 50.0, 60.0, 2, 5.0, 10.0)];
        assert!(!follow(&before, &mut after));
        assert_eq!(after[1].xfrm.map(|x| (x.x, x.y)), Some((50.0, 60.0)));
        assert_eq!(object::read(&after[1]).and_then(|o| o.anchor), Some(Anchor { shape: 2, dx: 40.0, dy: 50.0 }));
    }

    #[test]
    fn a_missing_anchor_changes_nothing() {
        let before = vec![object(3, 15.0, 20.0, 2, 5.0, 10.0)];
        let mut after = before.clone();
        assert!(!follow(&before, &mut after));
        assert_eq!(after, before);
    }
}
