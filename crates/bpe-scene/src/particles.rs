//! Hit particles: sparks, smoke, confetti. Owned by the GPU workstream, which replaces this
//! placeholder.

use serde::{Deserialize, Serialize};

/// Particles started by one kind of event or rule action.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Emitter {}
