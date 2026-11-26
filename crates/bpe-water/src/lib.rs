//! Water mode's own engine (`docs/notes/water.md`).
//!
//! The all-drops engine ([`Water`], [`drops`]: every drop feels its neighbours directly); the grid
//! core of parts 2a and 2a-2 ([`grid`]: drops on a grid, the pressure solved so the liquid cannot
//! squash), kept for comparisons and for part C2's pressure solve; where drops are born ([`drop`]);
//! the solid field the physics reads ([`solid`]); the plain drawing on the card, straight from the
//! drops' buffers ([`draw`]); and measurements for tests ([`probe`]). Nothing here knows about
//! scenes, cards or the editor: a later part turns a Water scene into a [`SolidField`], pours,
//! [`Feel`]s and a [`draw::PlainLook`].

pub mod draw;
pub mod drop;
pub mod drops;
pub mod grid;
pub mod probe;
#[cfg(feature = "test-scenes")]
pub mod scenes;
pub mod solid;

pub use drop::{Drop, Pour, Poured, STOPS, WATER, fill, fill_stacked};
pub use drops::numbers::{Feel, Numbers};
pub use drops::{DropBuffers, MOST_DROPS, Water, WaterGpu};
pub use solid::{Layout, Solid, SolidField};
