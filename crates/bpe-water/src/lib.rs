//! Water mode's own engine (`docs/notes/water.md`).
//!
//! The all-drops engine ([`Water`], [`drops`]: every drop feels its neighbours directly, and deep
//! water is held up by a pressure solved on a grid, the grid core of parts 2a and 2a-2 kept only for
//! that); where drops are born ([`drop`]); the solid field the physics reads ([`solid`]) and the
//! parts that move in it ([`parts`], part 5); the plain drawing on the card, straight from the
//! drops' buffers ([`draw`]); and measurements for tests ([`probe`]). Nothing here knows about
//! scenes, cards or the editor: a later part turns a Water scene into a [`SolidField`], pours,
//! [`Feel`]s and a [`draw::PlainLook`].

pub mod draw;
pub mod drop;
pub mod drops;
pub mod lumps;
pub mod parts;
pub mod probe;
#[cfg(feature = "test-scenes")]
pub mod scenes;
pub mod solid;
pub mod take;

pub use drop::{AIR, Drop, Pour, Poured, STOPS, WATER, fill, fill_stacked};
pub use drops::numbers::{Feel, Numbers};
pub use drops::{Drain, DropBuffers, MOST_DRAINS, MOST_DROPS, Water, WaterGpu, WaterState};
pub use lumps::{LumpKind, Lumps, Throw, Timed};
pub use parts::{MOST_MOVERS, Mover, MoverShape, Movers, Pose};
pub use solid::{DEFAULT_WETTING, Layout, Part, Solid, SolidField};
