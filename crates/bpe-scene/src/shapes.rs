//! Custom shapes: drawn, imported from SVG or traced from an image. Owned by the physics
//! workstream, which replaces this placeholder.

use serde::{Deserialize, Serialize};

/// A named outline that arenas and obstacles can refer to.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shape {
    pub name: String,
}
