//! Flat 2D or 3D space. Owned by the 3D workstream, which replaces this placeholder.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum View {
    /// The 2D picture every scene has used so far.
    #[default]
    Flat,
}
