//! Water mode's own engine (`docs/notes/water.md`).
//!
//! The grid core of parts 2a and 2a-2 ([`grid`]: drops on a grid, the pressure solved so the liquid
//! cannot squash); where drops are born ([`drop`]); the solid field the physics reads ([`solid`]);
//! and measurements for tests ([`probe`]). Nothing here knows about scenes, cards or the editor: a
//! later part turns a Water scene into a [`SolidField`], pours and [`Feel`]s.

pub mod bridge;
pub mod drop;
pub mod drops;
pub mod grid;
pub mod probe;
#[cfg(feature = "test-scenes")]
pub mod scenes;
pub mod solid;

pub use drop::{Drop, Pour, Poured, STOPS, WATER, fill};
pub use drops::numbers::{Feel, Numbers};
pub use grid::{MOST_DROPS, Water, WaterGpu};
pub use solid::{Layout, Solid, SolidField};
