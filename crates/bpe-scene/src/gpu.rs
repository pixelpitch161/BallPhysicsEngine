//! Swarms and big fluids that run on the graphics card. Owned by the GPU workstream, which
//! replaces this placeholder.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuLayers {}
