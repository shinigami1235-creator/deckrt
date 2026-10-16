//! Interactive objects for deckrt: a 3D model, an image swap, a wipe, a zoom region, a popup, a
//! poll, a scrub slider, a measurement overlay and an injection map.
//!
//! Each object is an ordinary shape in the slide. Its settings are the JSON value of a shape tag
//! named [`object::TAG`], so the `.pptx` is the only file and PowerPoint carries the tag through
//! unchanged. Pictures and models the settings name are media items of the presentation,
//! referenced as `media:<id>` strings; the PPTX writer turns those into relationships from the
//! slide so the parts are kept.
//!
//! - [`object`]: the settings, their JSON form and the tag read/write.
//! - [`state`]: what an object is doing during a show.
//! - [`measure`], [`injection`], [`poll`]: the maths behind three of the objects.
//! - [`glb`]: node names of a `.glb`, the model's layer list.
//! - [`sidecar`]: import of the earlier deckrt `.deckrt` sidecar files.
//! - [`anchor`]: an object following the shape it is anchored to.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod anchor;
pub mod glb;
pub mod injection;
pub mod measure;
pub mod object;
pub mod poll;
pub mod sidecar;
pub mod state;

pub use object::{Anchor, Config, InteractiveObject, Kind, MediaRef, Trigger};
pub use state::ObjectState;
