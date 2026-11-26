//! Water mode in the editor (Water mode part 1): the New scene chooser, and a Water scene's own
//! stage in place of the ball preview; the water's draft playing there (part 2b).

pub mod chooser;
pub mod course_view;
pub mod draft;
pub mod picture;
pub mod rail;
pub mod stage;

pub use chooser::{Chooser, ChooserOutcome, NewScene};
pub use stage::{WaterStage, stage};

#[cfg(test)]
mod tests;
