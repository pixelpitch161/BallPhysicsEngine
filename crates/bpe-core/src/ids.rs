//! Names for things, modules, plugin kinds and slots.

use serde::{Deserialize, Serialize};

/// One thing in a scene. Never reused within a scene, even after the thing is removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThingId(pub u32);

impl ThingId {
    /// The scene settings, the root of every scene's tree.
    pub const ROOT: ThingId = ThingId(0);
}

/// One module in a scene's library. Never reused within a scene.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModuleId(pub u32);

/// A plugin's id, such as `"balls"` or `"look.plain"`.
pub type KindId = String;

/// A place on a thing that a module fills.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SlotId {
    #[default]
    Look,
    Trail,
    Motion,
    Bounce,
    Sound,
    Particles,
    Rules,
    Shader,
}

impl SlotId {
    /// Every slot, in the order the inspector shows them. `slot as usize` indexes this list.
    pub const ALL: [SlotId; 8] = [
        SlotId::Look,
        SlotId::Trail,
        SlotId::Motion,
        SlotId::Bounce,
        SlotId::Sound,
        SlotId::Particles,
        SlotId::Rules,
        SlotId::Shader,
    ];

    /// The name shown in the inspector.
    pub fn name(self) -> &'static str {
        match self {
            SlotId::Look => "Look",
            SlotId::Trail => "Trail",
            SlotId::Motion => "Motion",
            SlotId::Bounce => "Bounce",
            SlotId::Sound => "Sound",
            SlotId::Particles => "Particles",
            SlotId::Rules => "Rules",
            SlotId::Shader => "Shader",
        }
    }
}
