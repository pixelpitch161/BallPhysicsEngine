//! Parts that move (Water mode part 5): where a part is ([`Pose`]), its solid in its own frame
//! ([`MoverShape`]), and the course split into what stands still and what moves ([`Movers`]).
//!
//! **Liquid first** (water spec, section 4): the liquid's grid, its step and its method never
//! change because a part is there. A part is sampled onto the liquid's grid, thin walls thickened
//! for the physics only, and the only thing a part changes is how many steps a frame takes, and
//! only upward (no point of a part moves further in a step than the water's own fastest drop may).

mod movers;
mod pose;
mod shape;

pub use movers::{Mover, Movers};
pub use pose::{Pose, SCALE_RANGE};
pub use shape::MoverShape;

/// The most parts a course moves (water spec, section 4).
pub const MOST_MOVERS: usize = 64;
