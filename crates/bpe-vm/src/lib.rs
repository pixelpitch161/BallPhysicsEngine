//! The graph VM: compiles a module's or a rule's node graph for the CPU and runs it.
//!
//! Graphs come in four roles. Rules graphs are trigger chains (When → Only if → Do) that read
//! the event log and queue actions. Bounce, Motion and Sound graphs are dataflow: an input block
//! gives the values of one hit, step or note, and an output block collects what the module
//! decides. Values are pulled through the wires; triggers are pushed along them.

pub mod catalog;
pub mod program;
pub mod target;
pub mod val;

pub use catalog::{Role, SETTING_KINDS, catalog, is_setting};
pub use program::{Block, Program, compile};
pub use target::{target_from_text, target_to_text};
pub use val::{Ball, BallInfo, Val};
