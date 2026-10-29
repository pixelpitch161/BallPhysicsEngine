//! The node editor: a canvas of blocks and wires for the Nodes depth of a module.

mod draw;
pub mod editor;
mod palette;
pub mod params;
#[cfg(test)]
mod tests;

pub use editor::{Editor, LiveView, Outcome, PinRef, category_color, pin_color, take_wants_keyboard};
pub use params::{param_control, target_words};
