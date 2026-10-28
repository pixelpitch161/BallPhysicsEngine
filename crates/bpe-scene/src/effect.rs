use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One step of the shader effect chain, applied to the whole picture after the scene is drawn.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Effect {
    /// A built-in effect such as "kaleidoscope", or the file name (without `.wgsl`) of a custom
    /// effect in an `effects` folder next to the scene file or the program.
    pub shader: String,
    pub enabled: bool,
    /// How much of the effect shows, from 0 (off) to 1 (full).
    pub amount: f32,
    /// Slider values by name. Any the shader declares but the scene leaves out use the shader's default.
    pub params: BTreeMap<String, f32>,
}

impl Effect {
    pub fn new(shader: impl Into<String>) -> Self {
        Self { shader: shader.into(), ..Self::default() }
    }

    pub fn with(mut self, name: &str, value: f32) -> Self {
        self.params.insert(name.into(), value);
        self
    }
}

impl Default for Effect {
    fn default() -> Self {
        Self { shader: String::new(), enabled: true, amount: 1.0, params: BTreeMap::new() }
    }
}
