//! The ready-made liquids (Water mode part 4.3a, decision 23): one table, one row a liquid, put on
//! a Liquid with **Use a liquid**. A row says how the Liquid's Feel card, its Mixing card's
//! *Meeting other liquids* and its Look card's *Its colour*, *Its body* and *Its surface* are set;
//! [`apply_liquid`] writes it as plain values the Liquid owns from then on. Water is a row too (the
//! cards' defaults), and adding a liquid is adding a row (Ferrofluid is part 4b's).
//!
//! A liquid never writes the Mixing card's Un-mix (what the video does, not what the liquid is),
//! the Look's Glow, Drawn and Extras (how the scene's ready-made look draws every liquid), or the
//! Heat card. Like a look, it sets each setting it writes to the row's value (removed where that is
//! the card's default), so one liquid after another never keeps what the first set, and a setting
//! with keyframes keeps them.

use bpe_core::{Color, Registry, Scene, SettingDef, SlotId, ThingId};

use super::liquid::{FeelCard, MixesWhen, MixingCard};
use super::look::WaterLook;
use super::looks::{ReadyLook, write_card};
use crate::things::water::LIQUID;

/// The ready-made liquids.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ReadyLiquid {
    #[default]
    Water,
    Oil,
    Honey,
    Syrup,
    Slime,
    Milk,
    Paint,
    Mercury,
    Ferrofluid,
}

/// A liquid's row: what it sets (only the groups in [`WRITTEN`] of each card).
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidRow {
    pub feel: FeelCard,
    pub mixing: MixingCard,
    pub look: WaterLook,
}

/// The groups of the Mixing and Look cards a liquid writes (all of the Feel card).
pub const WRITTEN: [&str; 4] = ["Meeting other liquids", "Its colour", "Its body", "Its surface"];

/// The groups it leaves as they are.
pub const LEFT: [&str; 4] = ["Un-mix", "Glow", "Drawn", "Extras"];

fn rgb(hex: u32) -> Color {
    Color::rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl ReadyLiquid {
    pub const ALL: [ReadyLiquid; 9] = [
        ReadyLiquid::Water,
        ReadyLiquid::Oil,
        ReadyLiquid::Honey,
        ReadyLiquid::Syrup,
        ReadyLiquid::Slime,
        ReadyLiquid::Milk,
        ReadyLiquid::Paint,
        ReadyLiquid::Mercury,
        ReadyLiquid::Ferrofluid,
    ];

    /// Its name, in the Use a liquid menu and on a Liquid that takes it.
    pub fn title(self) -> &'static str {
        match self {
            ReadyLiquid::Water => "Water",
            ReadyLiquid::Oil => "Oil",
            ReadyLiquid::Honey => "Honey",
            ReadyLiquid::Syrup => "Syrup",
            ReadyLiquid::Slime => "Slime",
            ReadyLiquid::Milk => "Milk",
            ReadyLiquid::Paint => "Paint",
            ReadyLiquid::Mercury => "Mercury",
            ReadyLiquid::Ferrofluid => "Ferrofluid",
        }
    }

    /// One line saying what it is.
    pub fn words(self) -> &'static str {
        match self {
            ReadyLiquid::Water => "Clear blue water that runs and splashes.",
            ReadyLiquid::Oil => "Clear gold oil that floats on water and never mixes with it.",
            ReadyLiquid::Honey => "Thick amber honey that sinks, clings and mixes slowly.",
            ReadyLiquid::Syrup => "Dark syrup, thick and sticky, a little runnier than honey.",
            ReadyLiquid::Slime => "Green slime that holds together in blobs and clings.",
            ReadyLiquid::Milk => "White milk that runs like water and clouds into it.",
            ReadyLiquid::Paint => "Glossy red paint whose colour runs into the paint it meets.",
            ReadyLiquid::Mercury => "Heavy silver mercury that beads up and sinks under everything.",
            ReadyLiquid::Ferrofluid => "Black glossy ferrofluid that a Magnet pulls into spikes.",
        }
    }

    /// What it sets (decision 23's table, its look tuned with the full-size look check).
    pub fn row(self) -> LiquidRow {
        let mut row =
            LiquidRow { feel: FeelCard::default(), mixing: MixingCard::default(), look: WaterLook::default() };
        let (f, m, l) = (&mut row.feel, &mut row.mixing, &mut row.look);
        match self {
            ReadyLiquid::Water => {}
            ReadyLiquid::Oil => {
                (f.thickness, f.surface_pull, f.weight, f.stickiness) = (0.08, 0.35, 0.9, 0.2);
                (m.keeps_apart, m.mix_speed) = (1.0, 0.0);
                (l.colour, l.deep, l.clarity, l.bend) = (rgb(0xe8b830), rgb(0x6a4608), 220.0, 10.0);
                (l.shine, l.shine_size, l.edge_sheen, l.thin_film) = (0.8, 0.2, 0.5, 0.12);
                (l.smoothing, l.streaks) = (0.6, 0.6);
            }
            ReadyLiquid::Honey => {
                (f.thickness, f.surface_pull, f.weight, f.stickiness) = (0.6, 0.6, 1.4, 0.6);
                (m.keeps_apart, m.mix_speed) = (0.2, 0.15);
                (l.colour, l.deep, l.clarity, l.solid, l.bend) = (rgb(0xe8961c), rgb(0x7a3404), 90.0, 0.1, 16.0);
                (l.shine, l.shine_size, l.edge_sheen) = (0.8, 0.25, 0.5);
                (l.smoothing, l.streaks) = (0.8, 0.4);
            }
            ReadyLiquid::Syrup => {
                (f.thickness, f.surface_pull, f.weight, f.stickiness) = (0.35, 0.55, 1.3, 0.5);
                (m.keeps_apart, m.mix_speed) = (0.1, 0.25);
                (l.colour, l.deep, l.clarity, l.bend) = (rgb(0xb4501a), rgb(0x3e1004), 60.0, 14.0);
                (l.shine, l.shine_size, l.edge_sheen) = (0.75, 0.25, 0.45);
                (l.smoothing, l.streaks) = (0.75, 0.5);
            }
            ReadyLiquid::Slime => {
                (f.thickness, f.surface_pull, f.weight, f.stickiness) = (0.45, 0.85, 1.1, 0.7);
                (m.keeps_apart, m.mix_speed) = (0.6, 0.1);
                (l.colour, l.deep, l.clarity, l.solid, l.bend) = (rgb(0x7ee03a), rgb(0x1f6a10), 50.0, 0.35, 6.0);
                (l.shine, l.shine_size, l.edge_sheen) = (0.7, 0.35, 0.6);
                (l.smoothing, l.streaks) = (0.9, 0.3);
            }
            ReadyLiquid::Milk => {
                (f.thickness, f.surface_pull, f.weight, f.stickiness) = (0.03, 0.5, 1.03, 0.05);
                (m.keeps_apart, m.mix_speed) = (0.0, 0.6);
                (l.colour, l.deep, l.clarity, l.solid, l.bend) = (rgb(0xf4f1ea), rgb(0xd6cfc0), 8.0, 0.85, 2.0);
                (l.shine, l.shine_size, l.edge_sheen) = (0.35, 0.45, 0.2);
                (l.smoothing, l.streaks) = (0.6, 0.3);
            }
            ReadyLiquid::Paint => {
                (f.thickness, f.surface_pull, f.weight, f.stickiness) = (0.25, 0.45, 1.2, 0.4);
                (m.keeps_apart, m.mix_speed) = (0.0, 0.8);
                (l.colour, l.deep, l.clarity, l.solid, l.bend) = (rgb(0xe84a3a), rgb(0x8a1a12), 0.0, 1.0, 0.0);
                (l.shine, l.shine_size, l.smoothing, l.streaks) = (0.7, 0.35, 0.7, 0.5);
            }
            ReadyLiquid::Mercury => {
                (f.thickness, f.surface_pull, f.weight, f.stickiness) = (0.01, 1.0, 10.0, 0.0);
                (m.keeps_apart, m.mix_speed) = (1.0, 0.0);
                (l.colour, l.deep, l.clarity, l.solid, l.bend) = (rgb(0xa4abb5), rgb(0x2e3238), 40.0, 1.0, 0.0);
                (l.shine, l.shine_size, l.edge_sheen) = (1.0, 0.12, 1.0);
                (l.smoothing, l.streaks) = (0.9, 0.2);
            }
            // (Part 4b's: water's feel, which the Magnet's spikes are tuned with; oil-based, so it keeps
            // apart from water.)
            ReadyLiquid::Ferrofluid => {
                (m.keeps_apart, m.mix_speed) = (1.0, 0.0);
                (l.colour, l.deep, l.clarity, l.solid, l.bend) = (rgb(0x08080b), rgb(0x000000), 0.0, 1.0, 0.0);
                (l.shine, l.shine_size, l.edge_sheen) = (1.0, 0.6, 0.12);
                (l.smoothing, l.streaks) = (0.15, 0.0);
            }
        }
        row.mixing.mixes_when = MixesWhen::Always;
        row
    }
}

/// Whether a Liquid called `name` takes a liquid's title: it is still called what it was given
/// (Liquid, Water, a ready look's liquid or another ready liquid, with or without a number after
/// it), not a name the person typed.
pub fn takes_title(name: &str) -> bool {
    let name = name.trim();
    let bare = match name.rsplit_once(' ') {
        Some((head, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => head.trim_end(),
        _ => name,
    };
    bare == "Liquid"
        || ReadyLiquid::ALL.iter().any(|l| l.title() == bare)
        || ReadyLook::ALL.iter().any(|l| l.liquid_name() == bare)
}

/// Puts `liquid` on Liquid `id` (decision 23): its Feel card, its Mixing card's Meeting other
/// liquids and its Look's Its colour, Its body and Its surface, and its title when [`takes_title`].
/// False, and nothing changed, when `id` is not a Water Liquid.
pub fn apply_liquid(scene: &mut Scene, reg: &Registry, id: ThingId, liquid: ReadyLiquid) -> bool {
    use super::{FEEL, LOOK, MIXING};
    let Some(thing) = scene.things.get_mut(&id).filter(|t| t.kind == LIQUID) else { return false };
    if takes_title(&thing.name) {
        thing.name = liquid.title().into();
    }
    let row = liquid.row();
    let takes = |d: &SettingDef| WRITTEN.contains(&d.group);
    write_card(scene, reg, id, SlotId::Feel, FEEL, &row.feel, |_| true);
    write_card(scene, reg, id, SlotId::Mixing, MIXING, &row.mixing, takes);
    write_card(scene, reg, id, SlotId::Look, LOOK, &row.look, takes);
    true
}

#[cfg(test)]
mod tests {
    use super::super::HeatCard;
    use super::*;
    use crate::things::water::{POUR, new_water_scene};
    use bpe_core::{Params, Resolved, Settings, Value};

    fn scene_with_liquid() -> (Scene, Registry, ThingId) {
        let reg = crate::registry();
        let mut scene = new_water_scene(&reg, "");
        let id = crate::add_thing(&mut scene, &reg, LIQUID, ThingId::ROOT).unwrap();
        (scene, reg, id)
    }

    fn card<T: Settings>(resolved: &Resolved, id: ThingId, slot: SlotId) -> T {
        T::from_params(&resolved.things[&id].slots[&slot].params)
    }

    /// `card`'s settings in `groups` (every one when `None`).
    fn only<T: Settings>(card: &T, groups: Option<&[&str]>) -> Params {
        let all = card.to_params();
        let mut out = Params::default();
        for def in T::defs() {
            if groups.is_none_or(|g| g.contains(&def.group))
                && let Some(v) = all.get(def.key)
            {
                out.set(def.key, v.clone());
            }
        }
        out
    }

    #[test]
    fn each_liquid_writes_its_row() {
        for liquid in ReadyLiquid::ALL {
            let (mut scene, reg, id) = scene_with_liquid();
            // (Another liquid first, so every setting this one leaves at its default is set back.)
            let other = if liquid == ReadyLiquid::Mercury { ReadyLiquid::Honey } else { ReadyLiquid::Mercury };
            assert!(apply_liquid(&mut scene, &reg, id, other));
            assert!(apply_liquid(&mut scene, &reg, id, liquid));
            let r = bpe_core::resolve(&scene, &reg);
            assert!(r.problems.is_empty(), "{liquid:?}: {:?}", r.problems);
            let row = liquid.row();
            assert_eq!(card::<FeelCard>(&r, id, SlotId::Feel), row.feel, "{liquid:?}'s feel");
            let mixing: MixingCard = card(&r, id, SlotId::Mixing);
            assert_eq!(only(&mixing, Some(&WRITTEN)), only(&row.mixing, Some(&WRITTEN)), "{liquid:?}'s mixing");
            let look: WaterLook = card(&r, id, SlotId::Look);
            assert_eq!(only(&look, Some(&WRITTEN)), only(&row.look, Some(&WRITTEN)), "{liquid:?}'s look");
            assert_eq!(scene.things[&id].name, liquid.title());
        }
        // Water is the cards' defaults.
        let water = ReadyLiquid::Water.row();
        assert_eq!((water.feel, water.mixing), (FeelCard::default(), MixingCard::default()));
        assert_eq!(water.look, WaterLook::default());
    }

    #[test]
    fn a_liquid_leaves_un_mix_heat_and_the_scenes_drawing() {
        let (mut scene, reg, id) = scene_with_liquid();
        scene.add_slot(&reg, id, SlotId::Heat, super::super::HEAT).unwrap();
        let set = |scene: &mut Scene, slot: SlotId, key: &str, v: Value| {
            scene.card_mut(id, slot).unwrap().params.set(key, v);
        };
        set(&mut scene, SlotId::Mixing, "remember", Value::Bool(true));
        set(&mut scene, SlotId::Mixing, "unmix_seconds", Value::Number(9.0));
        set(&mut scene, SlotId::Heat, "temperature", Value::Number(0.9));
        set(&mut scene, SlotId::Look, "outline", Value::Number(4.0));
        set(&mut scene, SlotId::Look, "glow", Value::Number(0.5));
        set(&mut scene, SlotId::Look, "foam", Value::Number(0.6));
        let before = bpe_core::resolve(&scene, &reg);
        for liquid in ReadyLiquid::ALL {
            apply_liquid(&mut scene, &reg, id, liquid);
            let r = bpe_core::resolve(&scene, &reg);
            let (m, was): (MixingCard, MixingCard) = (card(&r, id, SlotId::Mixing), card(&before, id, SlotId::Mixing));
            assert_eq!(only(&m, Some(&LEFT)), only(&was, Some(&LEFT)), "{liquid:?} leaves Un-mix");
            let (l, was): (WaterLook, WaterLook) = (card(&r, id, SlotId::Look), card(&before, id, SlotId::Look));
            assert_eq!(only(&l, Some(&LEFT)), only(&was, Some(&LEFT)), "{liquid:?} leaves the drawing");
            let heat: HeatCard = card(&r, id, SlotId::Heat);
            assert_eq!(heat.temperature, 0.9, "{liquid:?} leaves Heat");
        }
    }

    #[test]
    fn every_setting_is_written_or_left_on_purpose() {
        fn groups<T: Settings>() -> Vec<&'static str> {
            T::defs().iter().map(|d| d.group).collect()
        }
        for g in [groups::<MixingCard>(), groups::<WaterLook>()].concat() {
            assert!(WRITTEN.contains(&g) || LEFT.contains(&g), "the group {g:?} is neither written nor left");
        }
        for g in WRITTEN.iter().chain(&LEFT) {
            let all = [groups::<MixingCard>(), groups::<WaterLook>()].concat();
            assert!(all.contains(g), "no setting is in {g:?} any more");
        }
    }

    #[test]
    fn a_liquid_takes_its_title_unless_it_was_named() {
        for given in ["Liquid", "Liquid 2", "Water", "Neon", "Lava 3", "Honey", "Oil 12"] {
            assert!(takes_title(given), "{given}");
        }
        for named in ["River", "Liquid two", "My honey", "Honey2", "", "2"] {
            assert!(!takes_title(named), "{named:?}");
        }
        let (mut scene, reg, id) = scene_with_liquid();
        scene.things.get_mut(&id).unwrap().name = "River".into();
        apply_liquid(&mut scene, &reg, id, ReadyLiquid::Oil);
        assert_eq!(scene.things[&id].name, "River");
    }

    #[test]
    fn only_a_liquid_takes_one() {
        let (mut scene, reg, _) = scene_with_liquid();
        let pour = crate::add_thing(&mut scene, &reg, POUR, ThingId::ROOT).unwrap();
        let before = scene.clone();
        assert!(!apply_liquid(&mut scene, &reg, pour, ReadyLiquid::Honey));
        assert!(!apply_liquid(&mut scene, &reg, ThingId(9999), ReadyLiquid::Honey));
        assert_eq!(bpe_core::save::save(&scene), bpe_core::save::save(&before));
    }

    #[test]
    fn the_liquids_say_what_they_are() {
        assert_eq!(ReadyLiquid::Oil.words(), "Clear gold oil that floats on water and never mixes with it.");
        assert_eq!(ReadyLiquid::Mercury.title(), "Mercury");
        // Oil floats on water and Mercury sinks under every other liquid.
        let weight = |l: ReadyLiquid| l.row().feel.weight;
        assert!(weight(ReadyLiquid::Oil) < weight(ReadyLiquid::Water));
        assert!(
            ReadyLiquid::ALL.iter().all(|l| *l == ReadyLiquid::Mercury || weight(*l) < weight(ReadyLiquid::Mercury))
        );
    }
}
