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
    /// Kept for the test scenes, which still build old Trail cards and turn them into Look
    /// layers (`cards_to_layers`): no card kind fits this slot.
    Trail,
    Motion,
    Bounce,
    /// Kept for the test scenes, which still build old Particles cards and turn them into Look
    /// layers (`cards_to_layers`): no card kind fits this slot.
    Particles,
    Shader,
    /// The scene's lighting (the Lighting card): only the scene settings have one.
    Lighting,
    /// A thing's sound (the Sound tab), not shown in the inspector's card list.
    Sound,
    /// The scene's mixer (the Mixer panel), not shown in the inspector's card list.
    Mixer,
    /// A thing's health (rules part 4): a ball's, a group's balls', an emitter's balls' or a
    /// shape's. After the others, so no other slot's place in [`SlotId::ALL`] moves.
    Health,
    /// The scene's air (particles part 2, the Air card): only the scene settings have one. Last,
    /// so no other slot's place in [`SlotId::ALL`] moves.
    Air,
    /// The scene's colour grade (the Grade card, edited in Grade mode): only the scene settings
    /// have one, and it is not shown in the inspector's card list. Last, so no other slot's place
    /// in [`SlotId::ALL`] moves.
    Grade,
}

impl SlotId {
    /// Every slot, in the order the inspector shows them. `slot as usize` indexes this list.
    pub const ALL: [SlotId; 12] = [
        SlotId::Look,
        SlotId::Trail,
        SlotId::Motion,
        SlotId::Bounce,
        SlotId::Particles,
        SlotId::Shader,
        SlotId::Lighting,
        SlotId::Sound,
        SlotId::Mixer,
        SlotId::Health,
        SlotId::Air,
        SlotId::Grade,
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
            SlotId::Lighting => "Lighting",
            SlotId::Sound => "Sound",
            SlotId::Mixer => "Mixer",
            SlotId::Health => "Health",
            SlotId::Air => "Air",
            SlotId::Grade => "Grade",
        }
    }

    /// Whether the inspector lists this slot's card with the others. The sound, the mixer and the
    /// grade have panels of their own; Trail and Particles are kept only for the test scenes' old
    /// cards.
    pub fn in_card_list(self) -> bool {
        !matches!(self, SlotId::Sound | SlotId::Mixer | SlotId::Grade | SlotId::Trail | SlotId::Particles)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sound_and_mixer_slots_are_last_and_hidden() {
        assert_eq!(SlotId::ALL.len(), 12);
        assert_eq!(SlotId::ALL[9], SlotId::Health);
        assert_eq!(SlotId::ALL[10], SlotId::Air);
        assert_eq!(SlotId::ALL[11], SlotId::Grade);
        assert!(!SlotId::Grade.in_card_list(), "the grade has Grade mode of its own");
        assert!(SlotId::Air.in_card_list());
        assert!(SlotId::Health.in_card_list());
        assert_eq!(SlotId::ALL[7], SlotId::Sound);
        assert_eq!(SlotId::ALL[8], SlotId::Mixer);
        for (i, s) in SlotId::ALL.into_iter().enumerate() {
            assert_eq!(s as usize, i);
        }
        assert!(!SlotId::Sound.in_card_list());
        assert!(!SlotId::Mixer.in_card_list());
        assert!(SlotId::Look.in_card_list());
        assert!(!SlotId::Trail.in_card_list() && !SlotId::Particles.in_card_list(), "old cards, now Look layers");
    }
}
