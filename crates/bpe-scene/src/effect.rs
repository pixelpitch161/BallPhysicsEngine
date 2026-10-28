use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One step of the shader effect chain, applied after the scene is drawn: to the whole picture, or
/// like a material to only some things in it, such as one group of balls or the arena's walls.
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
    /// What the effect changes. Effects on things run before effects on the whole picture.
    #[serde(skip_serializing_if = "EffectTarget::is_picture")]
    pub target: EffectTarget,
}

/// What an effect changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectTarget {
    /// The whole picture.
    #[default]
    Picture,
    /// Every ball.
    Balls,
    /// The balls of one group: the spawner at this place in the scene's list.
    Group(u32),
    /// The arena's walls and rings.
    Walls,
    /// Pegs, lines and shaped obstacles.
    Obstacles,
}

impl EffectTarget {
    pub fn is_picture(&self) -> bool {
        *self == EffectTarget::Picture
    }

    /// The code the effect shaders compare with what the object picture says is at a pixel:
    /// 0 the whole picture, 1 every ball, 2 walls, 3 obstacles, 10 + n the balls of group n.
    pub fn code(self) -> f32 {
        match self {
            EffectTarget::Picture => 0.0,
            EffectTarget::Balls => 1.0,
            EffectTarget::Walls => 2.0,
            EffectTarget::Obstacles => 3.0,
            EffectTarget::Group(n) => 10.0 + n.min(1_000_000) as f32,
        }
    }

    /// The target after the spawner at `removed` was deleted: groups after it move up one. None
    /// for the deleted group itself, whose effects should go too.
    pub fn after_removing_group(self, removed: u32) -> Option<Self> {
        match self {
            EffectTarget::Group(n) if n == removed => None,
            EffectTarget::Group(n) if n > removed => Some(EffectTarget::Group(n - 1)),
            other => Some(other),
        }
    }
}

impl Effect {
    pub fn new(shader: impl Into<String>) -> Self {
        Self { shader: shader.into(), ..Self::default() }
    }

    pub fn with(mut self, name: &str, value: f32) -> Self {
        self.params.insert(name.into(), value);
        self
    }

    /// The same effect on `target` only.
    pub fn on(mut self, target: EffectTarget) -> Self {
        self.target = target;
        self
    }
}

impl Default for Effect {
    fn default() -> Self {
        Self {
            shader: String::new(),
            enabled: true,
            amount: 1.0,
            params: BTreeMap::new(),
            target: EffectTarget::Picture,
        }
    }
}
