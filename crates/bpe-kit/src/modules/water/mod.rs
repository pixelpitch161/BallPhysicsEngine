//! A Water scene's cards (Water mode part 1): their settings as data. The draft in the editor
//! (part 2b, `things::water::draft`) reads the Water card, the Course's Carving and Rock, a
//! Liquid's Feel and Look, and a Pour; the rest wait for later parts.
//!
//! No setting here pulses (a Water scene has no sound to follow) and none varies across balls.

pub mod carve;
pub mod liquid;
pub mod look;
pub mod looks;
pub mod pieces;
pub mod places;
pub mod scene;
pub mod solids;

pub use carve::{CarveCard, CarveMask, CarvePaint, layout_for, mask_for};
pub use liquid::{FeelCard, HeatCard, MixesWhen, MixingCard};
pub use look::{
    BackCard, BackShows, ColourFrom, GlowFrom, LampCard, LampKind, LightQuality, WaterLightCard, WaterLook,
};
pub use looks::{Keep, ReadyLook, apply_look};
pub use pieces::{PALETTE, default_size, placed};
pub use places::{CourseEdge, DrainCard, DrainKind, ForceCard, ForceKind, PourCard, PourKind};
pub use scene::{DraftDetail, FinalDetail, SCREEN, WaterWorld};
pub use solids::{Joint, LineEnds, MaterialCard, MaterialKind, MovesCard, MovesKind, PieceKind, PieceShape, WallLine};

use bpe_core::{ModuleKind, Registry, SettingDef, Settings, SlotId};

use crate::things::water as things;

/// The scene's Water card.
pub const WORLD: &str = "water.world";
/// The scene's Light card.
pub const LIGHT: &str = "water.light";
/// The scene's Back card.
pub const BACK: &str = "water.back";
/// The Course's Carving card.
pub const CARVE: &str = "water.carve";
/// The Course's Rock card, and a Piece's or a Wall's Material card.
pub const MATERIAL: &str = "water.material";
/// A Piece's Shape card.
pub const SHAPE: &str = "water.shape";
/// A Wall's Line card.
pub const LINE: &str = "water.line";
/// A Piece's or a Wall's Moves card.
pub const MOVES: &str = "water.moves";
/// A Liquid's Feel card.
pub const FEEL: &str = "water.feel";
/// A Liquid's Look card.
pub const LOOK: &str = "water.look";
/// A Liquid's Mixing card.
pub const MIXING: &str = "water.mixing";
/// A Liquid's Heat card.
pub const HEAT: &str = "water.heat";
/// A Pour's card.
pub const POUR: &str = "water.pour";
/// A Drain's card.
pub const DRAIN: &str = "water.drain";
/// A Force's card.
pub const FORCE: &str = "water.force";
/// A Lamp's card.
pub const LAMP: &str = "water.lamp";

/// Every Water card kind's id.
pub const CARDS: [&str; 16] =
    [WORLD, LIGHT, BACK, CARVE, MATERIAL, SHAPE, LINE, MOVES, FEEL, LOOK, MIXING, HEAT, POUR, DRAIN, FORCE, LAMP];

/// One Water card kind: an id, a name, the slot it fills, its settings and the kinds of thing
/// it goes on.
pub struct WaterCard {
    id: &'static str,
    name: &'static str,
    help: &'static str,
    slot: SlotId,
    defs: fn() -> &'static [SettingDef],
    fits: &'static [&'static str],
}

impl ModuleKind for WaterCard {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.name
    }
    fn help(&self) -> &'static str {
        self.help
    }
    fn slot(&self) -> SlotId {
        self.slot
    }
    fn settings(&self) -> &'static [SettingDef] {
        (self.defs)()
    }
    fn fits(&self, thing_kind: &str) -> bool {
        self.fits.contains(&thing_kind)
    }
}

const fn card(
    id: &'static str,
    name: &'static str,
    help: &'static str,
    slot: SlotId,
    defs: fn() -> &'static [SettingDef],
    fits: &'static [&'static str],
) -> WaterCard {
    WaterCard { id, name, help, slot, defs, fits }
}

const SOLIDS: &[&str] = &[things::COURSE, things::PIECE, things::WALL];
const MOVERS: &[&str] = &[things::PIECE, things::WALL];
const LIQUID: &[&str] = &[things::LIQUID];

/// Every Water card kind.
fn cards() -> [WaterCard; 16] {
    [
        card(
            WORLD,
            "Water",
            "The water itself: how finely it is worked out, which way it falls and how big the course is.",
            SlotId::Water,
            WaterWorld::defs,
            &["scene"],
        ),
        card(
            LIGHT,
            "Light",
            "The light the lamps don't give: the glow everywhere, light bouncing round, and the patterns water throws.",
            SlotId::WaterLight,
            WaterLightCard::defs,
            &["scene"],
        ),
        card(
            BACK,
            "Back",
            "What shows where the course is carved away, behind the water.",
            SlotId::Back,
            BackCard::defs,
            &["scene"],
        ),
        card(
            CARVE,
            "Carving",
            "Where the course is dug out of the rock: every stroke of the Carve tool.",
            SlotId::Form,
            CarveCard::defs,
            &[things::COURSE],
        ),
        card(
            MATERIAL,
            "Material",
            "What it is made of and how it looks: rock, glass, paper, basalt or flat colour.",
            SlotId::Material,
            MaterialCard::defs,
            SOLIDS,
        ),
        card(
            SHAPE,
            "Shape",
            "Which piece it is, how big, and whether it adds solid or cuts a hole.",
            SlotId::Form,
            PieceShape::defs,
            &[things::PIECE],
        ),
        card(
            LINE,
            "Line",
            "The wall's line: its points, how thick it is and whether it curves.",
            SlotId::Form,
            WallLine::defs,
            &[things::WALL],
        ),
        card(
            MOVES,
            "Moves",
            "Whether it stays put, follows its keyframes, or is pushed round by the water.",
            SlotId::Moves,
            MovesCard::defs,
            MOVERS,
        ),
        card(
            FEEL,
            "Feel",
            "How the liquid flows: thin or thick, heavy or light, clinging or not.",
            SlotId::Feel,
            FeelCard::defs,
            LIQUID,
        ),
        card(
            LOOK,
            "Look",
            "How the liquid is drawn: its colour, how deep you see into it, its surface, glow and extras.",
            SlotId::Look,
            WaterLook::defs,
            LIQUID,
        ),
        card(
            MIXING,
            "Mixing",
            "How its colours run together with other liquids, and Un-mix.",
            SlotId::Mixing,
            MixingCard::defs,
            LIQUID,
        ),
        card(
            HEAT,
            "Heat",
            "Heat in the liquid: cooling, thickening, freezing and drying up.",
            SlotId::Heat,
            HeatCard::defs,
            LIQUID,
        ),
        card(
            POUR,
            "Pour",
            "Where liquid comes from: a tap, rain, a drip, a burst, or a fill at the start.",
            SlotId::Main,
            PourCard::defs,
            &[things::POUR],
        ),
        card(
            DRAIN,
            "Drain",
            "Where liquid leaves: a drain, a slow leak, or an edge of the course.",
            SlotId::Main,
            DrainCard::defs,
            &[things::DRAIN],
        ),
        card(
            FORCE,
            "Force",
            "Something that pushes or heats the liquid near it: a magnet, a whirlpool, wind, a heater or a cooler.",
            SlotId::Main,
            ForceCard::defs,
            &[things::FORCE],
        ),
        card(
            LAMP,
            "Lamp",
            "A light in the course: a spot, a strip or the sun.",
            SlotId::Main,
            LampCard::defs,
            &[things::LAMP],
        ),
    ]
}

/// Adds every Water card kind.
pub fn register(reg: &mut Registry) {
    for c in cards() {
        reg.add_module(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_card_is_registered_once_under_its_id() {
        let reg = crate::registry();
        let ids: Vec<&str> = cards().iter().map(|c| c.id).collect();
        assert_eq!(ids, CARDS);
        for id in CARDS {
            let kind = reg.module(id).unwrap_or_else(|| panic!("{id} is not registered"));
            assert!(kind.slot().in_sim(bpe_core::SimKind::Water), "{id}");
            assert!(!kind.settings().is_empty(), "{id}");
            assert!(!kind.fits("balls") && !kind.fits("liquid"), "{id} is a Water scene's");
        }
    }

    #[test]
    fn water_settings_follow_nothing_and_vary_nothing() {
        let reg = crate::registry();
        for id in CARDS {
            let kind = reg.module(id).unwrap();
            for d in kind.settings() {
                assert!(!d.flags.pulse, "{id} {}: a Water scene has nothing to follow yet", d.key);
                assert!(kind.vary(d.key).is_none(), "{id} {}", d.key);
            }
        }
    }

    #[test]
    fn the_course_is_at_most_sixteen_screens() {
        let w = WaterWorld { screens_wide: 8, screens_tall: 8, ..WaterWorld::default() };
        assert_eq!(w.screens(), (8, 2));
        assert_eq!(WaterWorld { screens_wide: 3, screens_tall: 8, ..WaterWorld::default() }.screens(), (3, 5));
        assert_eq!(WaterWorld::default().course_size(), glam::Vec2::new(1080.0, 1920.0));
        assert_eq!(WaterWorld { screens_wide: 0, ..WaterWorld::default() }.screens(), (1, 1));
        let wide = WaterWorld { screens_wide: 2, ..WaterWorld::default() };
        assert_eq!(wide.course_size(), glam::Vec2::new(2160.0, 1920.0));
    }

    #[test]
    fn detail_reads_as_the_design_says() {
        assert_eq!(
            [DraftDetail::Coarse, DraftDetail::Normal, DraftDetail::Fine].map(DraftDetail::cell_px),
            [12.0, 8.0, 6.0]
        );
        assert_eq!(
            [FinalDetail::Coarse, FinalDetail::Normal, FinalDetail::Fine].map(FinalDetail::cell_px),
            [3.0, 2.0, 1.5]
        );
        let w = WaterWorld::default();
        assert_eq!((w.draft_detail, w.final_detail), (DraftDetail::Normal, FinalDetail::Normal));
        assert_eq!(w.gravity, glam::Vec2::new(0.0, 4000.0), "as today's default liquid falls: 8 x 500");
    }

    #[test]
    fn the_looks_are_plain_and_the_six() {
        let titles = ReadyLook::ALL.map(ReadyLook::title);
        assert_eq!(
            titles,
            ["Plain", "Lamp-lit rock", "Glass lab", "Neon in the dark", "Paint on paper", "Molten", "Cartoon"]
        );
        assert_eq!(ReadyLook::default(), ReadyLook::Plain);
        for look in ReadyLook::ALL {
            assert!(!look.words().is_empty());
        }
    }
}
