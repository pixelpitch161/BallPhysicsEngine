//! The node editor: a canvas of blocks and wires for the Nodes depth of a module.

mod draw;
pub mod editor;
mod palette;
pub mod panel;
pub mod params;
#[cfg(test)]
mod tests;

pub use editor::take_wants_keyboard;
pub use panel::NodePanel;
