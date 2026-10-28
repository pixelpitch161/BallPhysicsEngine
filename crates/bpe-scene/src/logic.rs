//! Rules: "when this happens, do that" graphs that run inside the simulation.
//!
//! Each graph in [`crate::Scene::logic`] is one rule, built from the blocks in the simulation's
//! rule catalog. The scene only stores the wiring; `bpe-sim` compiles and runs it.

pub use bpe_graph::{Graph, Link, Node, Note, Param};
