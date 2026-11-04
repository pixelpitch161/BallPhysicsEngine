//! Names for things, plugin kinds and the slots a thing's cards fill.

use serde::{Deserialize, Serialize};

/// One thing in a scene. Never reused within a scene, even after the thing is removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThingId(pub u32);

impl ThingId {
    /// The scene settings, the root of every scene's tree.
    pub const ROOT: ThingId = ThingId(0);
}

/// A plugin's id, such as `"balls"` or `"look.plain"`.
pub type KindId = String;

/// A place on a thing that one of its cards fills.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum SlotId {
    #[default]
    Look,
    Trail,
    Motion,
    Bounce,
    Particles,
    Shader,
    /// A thing's sound (the Sound tab), not shown in the inspector's card list.
    Sound,
    /// The scene's mixer (the Mixer panel), not shown in the inspector's card list.
    Mixer,
}

impl SlotId {
    /// Every slot, in the order the inspector shows them. `slot as usize` indexes this list.
    pub const ALL: [SlotId; 8] = [
        SlotId::Look,
        SlotId::Trail,
        SlotId::Motion,
        SlotId::Bounce,
        SlotId::Particles,
        SlotId::Shader,
        SlotId::Sound,
        SlotId::Mixer,
    ];

    /// The name shown in the inspector.
    pub fn name(self) -> &'static str {
        match self {
            SlotId::Look => "Look",
            SlotId::Trail => "Trail",
            SlotId::Motion => "Motion",
            SlotId::Bounce => "Bounce",
            SlotId::Particles => "Particles",
            SlotId::Shader => "Shader",
            SlotId::Sound => "Sound",
            SlotId::Mixer => "Mixer",
        }
    }

    /// Whether the inspector lists this slot's card with the others. The sound and the mixer have
    /// panels of their own.
    pub fn in_card_list(self) -> bool {
        !matches!(self, SlotId::Sound | SlotId::Mixer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_and_mixer_slots_are_last_and_hidden() {
        assert_eq!(SlotId::ALL.len(), 8);
        assert_eq!(SlotId::ALL[6], SlotId::Sound);
        assert_eq!(SlotId::ALL[7], SlotId::Mixer);
        for (i, s) in SlotId::ALL.into_iter().enumerate() {
            assert_eq!(s as usize, i);
        }
        assert!(!SlotId::Sound.in_card_list());
        assert!(!SlotId::Mixer.in_card_list());
        assert!(SlotId::Look.in_card_list());
    }
}
