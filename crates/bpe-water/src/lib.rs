//! Water mode's own engine (`docs/notes/water.md`).
//!
//! Part 2a holds the physics: drops on a grid, the pressure solved so the liquid cannot squash,
//! and a hold-together for thin streams and films ([`sim`]); the solid field the physics reads
//! ([`solid`]); and measurements for tests ([`probe`]). Nothing here knows about scenes, cards or
//! the editor: a later part turns a Water scene into a [`SolidField`], pours and [`Feel`]s.

pub mod probe;
#[cfg(feature = "test-scenes")]
pub mod scenes;
pub mod sim;
pub mod solid;

pub use sim::drop::{Drop, Pour, Poured, STOPS, WATER, fill};
pub use sim::numbers::{Feel, Numbers};
pub use sim::{MOST_DROPS, Water, WaterGpu};
pub use solid::{Layout, Solid, SolidField};
