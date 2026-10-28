//! Captions, counters and timers drawn over the video. Owned by the batch and captions
//! workstream, which replaces this placeholder.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Caption {}
