//! The ready-made looks (Water mode part 6c, decisions 38 to 41): one table, one row a look. A row
//! says how every Liquid's Look card, the Rock and every Material, the Back, the Light card, the
//! Lamps and the scene's Bloom and Exposure are set; [`apply_look`] writes it into a scene as one
//! edit. Plain is a row too (the cards' defaults), and adding a look is adding a row.
//!
//! A look sets each card it names to the card's defaults with the row's values on top, so one look
//! after another never keeps what the first set. It never writes Wetting (that is physics) or the
//! light's Quality (that is how long the picture takes, not how it looks), and a setting with
//! keyframes keeps them.

use bpe_core::{Color, Gradient, Params, Registry, Scene, SettingDef, Settings, SlotId, ThingId, Value, ValueType};
use glam::Vec2;

use super::look::{BackCard, BackShows, ColourFrom, GlowFrom, LampCard, LampKind, WaterLightCard, WaterLook};
use super::scene::SCREEN;
use super::solids::{MaterialCard, MaterialKind};
use crate::things::water::{COURSE, LAMP, LIQUID, PIECE, WALL};

/// The ready-made looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ReadyLook {
    #[default]
    Plain,
    LampLitRock,
    GlassLab,
    Neon,
    PaintOnPaper,
    Molten,
    Cartoon,
}

/// What [`apply_look`] leaves as it is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Keep {
    /// Every colour, gradient and Glow colour setting.
    pub colours: bool,
    /// The Lamps (else every Lamp goes and the look's are added).
    pub lamps: bool,
}

/// One of a look's lamps: its card, and where it is as a share of the camera's shot from its
/// middle (-0.5 to 0.5 across and down).
#[derive(Clone, Debug, PartialEq)]
pub struct LampRow {
    pub card: LampCard,
    pub share: Vec2,
}

/// A look's row: what it sets.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    /// Every Liquid's Look card.
    pub liquid: WaterLook,
    /// The Course's Rock and every Piece's and Wall's Material.
    pub material: MaterialCard,
    pub back: BackCard,
    pub light: WaterLightCard,
    pub lamps: Vec<LampRow>,
    /// The scene's Bloom and Exposure.
    pub bloom: f32,
    pub exposure: f32,
}

/// The settings a look never writes: Wetting is the physics', and Quality is how long the
/// picture takes to work out.
const NEVER: [&str; 2] = ["wetting", "quality"];

fn rgb(hex: u32) -> Color {
    Color::rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl ReadyLook {
    pub const ALL: [ReadyLook; 7] = [
        ReadyLook::Plain,
        ReadyLook::LampLitRock,
        ReadyLook::GlassLab,
        ReadyLook::Neon,
        ReadyLook::PaintOnPaper,
        ReadyLook::Molten,
        ReadyLook::Cartoon,
    ];

    /// Its name, on its tile and in the Use a look menu.
    pub fn title(self) -> &'static str {
        match self {
            ReadyLook::Plain => "Plain",
            ReadyLook::LampLitRock => "Lamp-lit rock",
            ReadyLook::GlassLab => "Glass lab",
            ReadyLook::Neon => "Neon in the dark",
            ReadyLook::PaintOnPaper => "Paint on paper",
            ReadyLook::Molten => "Molten",
            ReadyLook::Cartoon => "Cartoon",
        }
    }

    /// One line saying what it looks like.
    pub fn words(self) -> &'static str {
        match self {
            ReadyLook::Plain => "Blue water in grey rock, nothing set up.",
            ReadyLook::LampLitRock => "Clear water in warm rock, lit by two lamps from above.",
            ReadyLook::GlassLab => "Coloured liquid in clean glass, bright and even.",
            ReadyLook::Neon => "Glowing liquid in the dark, lighting the walls round it.",
            ReadyLook::PaintOnPaper => "Thick glossy paint on paper cut-outs.",
            ReadyLook::Molten => "Lava that glows and throws sparks, lighting the dark rock round it.",
            ReadyLook::Cartoon => "Flat colours with outlines and banded shading.",
        }
    }

    /// The name a new scene's Liquid gets.
    pub fn liquid_name(self) -> &'static str {
        match self {
            ReadyLook::Plain | ReadyLook::LampLitRock | ReadyLook::Cartoon => "Water",
            ReadyLook::GlassLab => "Liquid",
            ReadyLook::Neon => "Neon",
            ReadyLook::PaintOnPaper => "Paint",
            ReadyLook::Molten => "Lava",
        }
    }

    /// Its liquid's colour and its solids' colour, for its tile's picture.
    pub fn swatch(self) -> (Color, Color) {
        let row = self.row();
        (row.liquid.colour, row.material.colour)
    }

    /// What it sets (decision 41's table, tuned with the full-size look check).
    pub fn row(self) -> Row {
        let mut row = Row {
            liquid: WaterLook::default(),
            material: MaterialCard::default(),
            back: BackCard::default(),
            light: WaterLightCard::default(),
            lamps: Vec::new(),
            bloom: 0.0,
            exposure: 1.0,
        };
        let (l, m, b, light) = (&mut row.liquid, &mut row.material, &mut row.back, &mut row.light);
        match self {
            ReadyLook::Plain => {}
            ReadyLook::LampLitRock => {
                (l.colour, l.deep, l.clarity, l.bend) = (rgb(0x4fb4e8), rgb(0x06223a), 320.0, 16.0);
                (l.shine, l.edge_sheen, l.ripples, l.foam, l.bubbles) = (0.6, 0.4, 0.5, 0.25, 0.2);
                (m.material, m.colour, m.second_colour) = (MaterialKind::Rock, rgb(0x7a5c44), rgb(0x4a3626));
                (m.texture_size, m.depth, m.roughness, m.wet_marks) = (160.0, 14.0, 0.7, 0.6);
                (b.shows, b.darker) = (BackShows::DarkerRock, 0.65);
                (light.ambient, light.bounce, light.caustics, light.shafts) = (0.12, 0.4, 1.2, 0.3);
                let spot = |aim: f32, x: f32| LampRow {
                    card: LampCard {
                        kind: LampKind::Spot,
                        colour: rgb(0xffd2a0),
                        brightness: 1.6,
                        aim,
                        spread: 70.0,
                        softness: 0.4,
                        ..LampCard::default()
                    },
                    share: Vec2::new(x, -0.42),
                };
                row.lamps = vec![spot(70.0, -0.3), spot(110.0, 0.3)];
                row.bloom = 0.25;
            }
            ReadyLook::GlassLab => {
                (l.colour, l.deep, l.clarity) = (rgb(0x35d0a8), rgb(0x0c5a6a), 140.0);
                (l.shine, l.shine_size, l.edge_sheen, l.bubbles) = (0.9, 0.12, 0.6, 0.6);
                (m.material, m.colour, m.depth, m.roughness) = (MaterialKind::Glass, rgb(0xa8c4d8), 8.0, 0.1);
                (b.shows, b.colour) = (BackShows::Colour, rgb(0xe4ebf1));
                (light.ambient, light.caustics) = (0.75, 0.6);
                // (A Sun is stopped by a closed course's ceiling unless it throws no shadows.)
                row.lamps = vec![LampRow {
                    card: LampCard {
                        kind: LampKind::Sun,
                        colour: Color::WHITE,
                        brightness: 0.25,
                        aim: 90.0,
                        softness: 0.8,
                        shadows: false,
                        ..LampCard::default()
                    },
                    share: Vec2::new(0.0, -0.45),
                }];
                row.bloom = 0.1;
            }
            ReadyLook::Neon => {
                (l.colour, l.colour_from) = (rgb(0xff3dc8), ColourFrom::Speed);
                l.colour_gradient = Gradient::even(&[rgb(0x2a0a4a), rgb(0xff3dc8), Color::WHITE]);
                (l.clarity, l.glow, l.glow_colour, l.glow_from) = (120.0, 0.2, rgb(0xff3dc8), GlowFrom::Always);
                (l.outline, l.outline_colour) = (3.0, rgb(0xff9ae8));
                (m.material, m.colour) = (MaterialKind::Flat, rgb(0x12101e));
                (b.shows, b.colour) = (BackShows::Colour, rgb(0x07060c));
                (light.ambient, light.bounce) = (0.05, 0.5);
                row.bloom = 0.8;
            }
            ReadyLook::PaintOnPaper => {
                (l.colour, l.solid, l.clarity) = (rgb(0xe84a3a), 1.0, 0.0);
                (l.shine, l.shine_size, l.streaks) = (0.7, 0.35, 0.5);
                (m.material, m.colour, m.second_colour) = (MaterialKind::Paper, rgb(0xefe7d6), rgb(0xd9cdb4));
                (m.texture_size, m.depth) = (40.0, 10.0);
                (b.shows, b.darker) = (BackShows::DarkerRock, 0.08);
                (light.ambient, light.caustics) = (0.95, 0.0);
                row.lamps = vec![LampRow {
                    card: LampCard {
                        kind: LampKind::Sun,
                        colour: rgb(0xfff4e0),
                        brightness: 0.2,
                        aim: 60.0,
                        softness: 1.0,
                        shadows: false,
                        ..LampCard::default()
                    },
                    share: Vec2::new(-0.4, -0.45),
                }];
            }
            ReadyLook::Molten => {
                (l.colour, l.colour_from, l.deep) = (rgb(0xff7a18), ColourFrom::Speed, rgb(0x3a0800));
                l.colour_gradient = Gradient::even(&[rgb(0x6e0e00), rgb(0xff5a10), rgb(0xffd060)]);
                (l.clarity, l.glow, l.glow_colour, l.sparks) = (40.0, 0.8, rgb(0xff6a1c), 0.5);
                (m.material, m.colour, m.second_colour) = (MaterialKind::Basalt, rgb(0x241c1a), rgb(0x0e0a09));
                m.roughness = 0.25;
                (b.shows, b.darker) = (BackShows::DarkerRock, 0.7);
                (light.ambient, light.bounce) = (0.03, 0.6);
                row.bloom = 0.6;
            }
            ReadyLook::Cartoon => {
                (l.colour, l.clarity, l.bands, l.band_softness) = (rgb(0x3aa0ff), 300.0, 3, 0.1);
                (l.outline, l.outline_colour, l.shine, l.foam) = (4.0, rgb(0x101018), 0.5, 0.6);
                (m.material, m.colour, m.outline, m.outline_colour) =
                    (MaterialKind::Flat, rgb(0xffd24a), 4.0, rgb(0x101018));
                (b.shows, b.colour) = (BackShows::Colour, rgb(0x8fd3ff));
                (light.ambient, light.caustics, light.shafts) = (1.0, 0.0, 0.0);
            }
        }
        row
    }
}

/// The settings that say how a colour is picked, kept with the colours: a liquid of one colour
/// stays one colour (Neon and Molten colour theirs by speed).
const COLOUR_WAYS: [&str; 1] = ["colour_from"];

/// Whether a look writes `def`: none in [`NEVER`], and no colour, gradient or [`COLOUR_WAYS`] when
/// `keep_colours`.
fn a_look_writes(def: &SettingDef, keep_colours: bool) -> bool {
    let colour = matches!(def.ty, ValueType::Color | ValueType::Gradient) || COLOUR_WAYS.contains(&def.key);
    !NEVER.contains(&def.key) && !(keep_colours && colour)
}

/// Sets `params` to `card`: each of its settings that `takes` to the card's value (left out where
/// that is the default).
pub(super) fn write<T: Settings>(params: &mut Params, card: &T, takes: impl Fn(&SettingDef) -> bool) {
    let set = card.to_params();
    for def in T::defs() {
        if !takes(def) {
            continue;
        }
        match set.get(def.key) {
            Some(v) => params.set(def.key, v.clone()),
            None => {
                params.remove(def.key);
            }
        }
    }
}

/// Writes `card`'s settings that `takes` into `id`'s `slot`, adding the card (of kind `kind`) when
/// the thing has none.
pub(super) fn write_card<T: Settings>(
    scene: &mut Scene,
    reg: &Registry,
    id: ThingId,
    slot: SlotId,
    kind: &str,
    card: &T,
    takes: impl Fn(&SettingDef) -> bool,
) {
    let has = scene.things.get(&id).is_some_and(|t| t.slots.contains_key(&slot));
    if !has && scene.add_slot(reg, id, slot, kind).is_err() {
        return;
    }
    if let Some(s) = scene.things.get_mut(&id).and_then(|t| t.slots.get_mut(&slot)) {
        write(&mut s.params, card, takes);
    }
}

/// The middle and size of the camera's shot at the start (its Position and Zoom).
fn shot(scene: &Scene, reg: &Registry) -> (Vec2, Vec2) {
    let Some(camera) = crate::camera_of(scene) else { return (Vec2::ZERO, SCREEN) };
    let at = |key: &str| scene.value_at_time(reg, &bpe_core::Address::own(camera, key), 0.0);
    let centre = match at("position") {
        Some(Value::Vec2([x, y])) => Vec2::new(x, y),
        _ => Vec2::ZERO,
    };
    let zoom = match at("zoom") {
        Some(Value::Number(z)) if z > 0.0 => z as f32,
        _ => 1.0,
    };
    (centre, SCREEN / zoom)
}

/// Writes `look`'s row into `scene` (decision 38): every Liquid's Look, the Rock and every
/// Material, the Back, the Light card, the scene's Bloom and Exposure and (unless `keep.lamps`)
/// the Lamps, every Lamp going and the look's added in the camera's shot.
pub fn apply_look(scene: &mut Scene, reg: &Registry, look: ReadyLook, keep: Keep) {
    use super::{BACK, LIGHT, LOOK, MATERIAL};
    let row = look.row();
    let of_kind = |scene: &Scene, kinds: &[&str]| -> Vec<ThingId> {
        scene.order.iter().copied().filter(|id| kinds.contains(&scene.things[id].kind.as_str())).collect()
    };
    let takes = |d: &SettingDef| a_look_writes(d, keep.colours);
    for id in of_kind(scene, &[LIQUID]) {
        write_card(scene, reg, id, SlotId::Look, LOOK, &row.liquid, takes);
    }
    for id in of_kind(scene, &[COURSE, PIECE, WALL]) {
        write_card(scene, reg, id, SlotId::Material, MATERIAL, &row.material, takes);
    }
    write_card(scene, reg, ThingId::ROOT, SlotId::Back, BACK, &row.back, takes);
    write_card(scene, reg, ThingId::ROOT, SlotId::WaterLight, LIGHT, &row.light, takes);
    if let Some(root) = scene.things.get_mut(&ThingId::ROOT) {
        root.params.set("bloom", Value::Number(f64::from(row.bloom)));
        root.params.set("exposure", Value::Number(f64::from(row.exposure)));
    }
    if keep.lamps {
        return;
    }
    for id in of_kind(scene, &[LAMP]) {
        let _ = scene.remove_thing(id);
    }
    let (centre, size) = shot(scene, reg);
    for (k, lamp) in row.lamps.iter().enumerate() {
        let Ok(id) = crate::add_thing(scene, reg, LAMP, ThingId::ROOT) else { continue };
        let thing = scene.things.get_mut(&id).expect("just added");
        thing.name = if row.lamps.len() > 1 { format!("Lamp {}", k + 1) } else { "Lamp".into() };
        thing.transform.position = centre + lamp.share * size;
        // (A new lamp has no colours of its own to keep.)
        if let Some(s) = thing.slots.get_mut(&SlotId::Main) {
            write(&mut s.params, &lamp.card, |d| a_look_writes(d, false));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::things::water::{new_water_scene, start_water_scene};
    use bpe_core::Resolved;

    /// A new Water scene with a Liquid, a Piece, a Wall and a Lamp.
    fn scene_with_all() -> (Scene, Registry) {
        let reg = crate::registry();
        let mut scene = new_water_scene(&reg, "");
        for kind in [LIQUID, PIECE, WALL, LAMP] {
            crate::add_thing(&mut scene, &reg, kind, ThingId::ROOT).unwrap();
        }
        (scene, reg)
    }

    fn ids(scene: &Scene, kind: &str) -> Vec<ThingId> {
        scene.order.iter().copied().filter(|id| scene.things[id].kind == kind).collect()
    }

    fn card<T: Settings>(resolved: &Resolved, id: ThingId, slot: SlotId) -> T {
        T::from_params(&resolved.things[&id].slots[&slot].params)
    }

    /// `card` with Wetting and Quality as `like` has them (a look never writes them).
    fn as_written<T: Settings>(card: &T, like: &T) -> Params {
        let mut p = card.to_params();
        let like = like.to_params();
        for key in NEVER {
            match like.get(key) {
                Some(v) => p.set(key, v.clone()),
                None => {
                    p.remove(key);
                }
            }
        }
        p
    }

    #[test]
    fn each_look_writes_its_row() {
        for look in ReadyLook::ALL {
            let (mut scene, reg) = scene_with_all();
            // (Another look first, so every setting this one leaves at its default is set back.)
            let other = if look == ReadyLook::Neon { ReadyLook::Molten } else { ReadyLook::Neon };
            apply_look(&mut scene, &reg, other, Keep::default());
            apply_look(&mut scene, &reg, look, Keep::default());
            let r = bpe_core::resolve(&scene, &reg);
            assert!(r.problems.is_empty(), "{look:?}: {:?}", r.problems);
            let row = look.row();
            let liquid: WaterLook = card(&r, ids(&scene, LIQUID)[0], SlotId::Look);
            assert_eq!(liquid, row.liquid, "{look:?}'s liquid");
            for id in [ids(&scene, COURSE), ids(&scene, PIECE), ids(&scene, WALL)].concat() {
                let m: MaterialCard = card(&r, id, SlotId::Material);
                assert_eq!(as_written(&m, &row.material), as_written(&row.material, &m), "{look:?}'s material");
            }
            let back: BackCard = card(&r, ThingId::ROOT, SlotId::Back);
            assert_eq!(back, row.back, "{look:?}'s back");
            let light: WaterLightCard = card(&r, ThingId::ROOT, SlotId::WaterLight);
            assert_eq!(light, row.light, "{look:?}'s light");
            let root = crate::things::scene::SceneSettings::from_params(&r.things[&ThingId::ROOT].params);
            assert_eq!((root.bloom, root.exposure), (row.bloom, row.exposure), "{look:?}'s finish");
            let lamps = ids(&scene, LAMP);
            assert_eq!(lamps.len(), row.lamps.len(), "{look:?}: only its own lamps");
            for (id, want) in lamps.iter().zip(&row.lamps) {
                let lamp: LampCard = card(&r, *id, SlotId::Main);
                assert_eq!(lamp, want.card, "{look:?}'s lamp");
                // (The camera's shot of a new scene is one screen round the middle.)
                assert_eq!(scene.things[id].transform.position, want.share * SCREEN);
            }
        }
    }

    #[test]
    fn keep_my_colours_and_keep_my_lamps() {
        let (mut scene, reg) = scene_with_all();
        let liquid = ids(&scene, LIQUID)[0];
        let lamp = ids(&scene, LAMP)[0];
        let mine = Color::rgb(1, 2, 3);
        let look = scene.things.get_mut(&liquid).unwrap().slots.get_mut(&SlotId::Look).unwrap();
        look.params.set("colour", Value::Color(mine));
        look.params.set("glow_colour", Value::Color(mine));
        let keep = Keep { colours: true, lamps: true };
        apply_look(&mut scene, &reg, ReadyLook::Molten, keep);
        let r = bpe_core::resolve(&scene, &reg);
        let l: WaterLook = card(&r, liquid, SlotId::Look);
        assert_eq!((l.colour, l.glow_colour), (mine, mine), "my colours stay");
        assert_eq!(l.colour_gradient, WaterLook::default().colour_gradient, "and so does the gradient");
        assert_eq!(l.colour_from, WaterLook::default().colour_from, "and how the colour is picked");
        let row = ReadyLook::Molten.row().liquid;
        assert_eq!((l.glow, l.sparks), (row.glow, row.sparks), "the rest is the look's");
        let rock: MaterialCard = card(&r, ids(&scene, COURSE)[0], SlotId::Material);
        assert_eq!((rock.colour, rock.material), (MaterialCard::default().colour, MaterialKind::Basalt));
        assert_eq!(ids(&scene, LAMP), [lamp], "my lamp stays, and none is added");
        // Without the switches: the look's colours, my lamp goes and the look's two come.
        apply_look(&mut scene, &reg, ReadyLook::LampLitRock, Keep::default());
        let r = bpe_core::resolve(&scene, &reg);
        assert_eq!(card::<WaterLook>(&r, liquid, SlotId::Look).colour, ReadyLook::LampLitRock.row().liquid.colour);
        let lamps = ids(&scene, LAMP);
        assert!(!lamps.contains(&lamp) && lamps.len() == 2);
        let names: Vec<&str> = lamps.iter().map(|id| scene.things[id].name.as_str()).collect();
        assert_eq!(names, ["Lamp 1", "Lamp 2"]);
        // Keeping colours, the new lamps still get the look's.
        apply_look(&mut scene, &reg, ReadyLook::PaintOnPaper, Keep { colours: true, lamps: false });
        let r = bpe_core::resolve(&scene, &reg);
        let sun: LampCard = card(&r, ids(&scene, LAMP)[0], SlotId::Main);
        assert_eq!(sun, ReadyLook::PaintOnPaper.row().lamps[0].card);
    }

    #[test]
    fn a_look_never_writes_wetting_or_quality() {
        let (mut scene, reg) = scene_with_all();
        let piece = ids(&scene, PIECE)[0];
        fn slot(scene: &mut Scene, id: ThingId, s: SlotId) -> &mut Params {
            &mut scene.things.get_mut(&id).unwrap().slots.get_mut(&s).unwrap().params
        }
        slot(&mut scene, piece, SlotId::Material).set("wetting", Value::Number(0.3));
        slot(&mut scene, ThingId::ROOT, SlotId::WaterLight).set("quality", Value::Choice("best".into()));
        for look in ReadyLook::ALL {
            apply_look(&mut scene, &reg, look, Keep::default());
            let r = bpe_core::resolve(&scene, &reg);
            assert_eq!(card::<MaterialCard>(&r, piece, SlotId::Material).wetting, 0.3, "{look:?}");
            let light: WaterLightCard = card(&r, ThingId::ROOT, SlotId::WaterLight);
            assert_eq!(light.quality, super::super::LightQuality::Best, "{look:?}");
        }
    }

    #[test]
    fn a_scene_started_in_a_look_has_its_liquid_and_lamps() {
        let reg = crate::registry();
        for look in ReadyLook::ALL {
            let scene = start_water_scene(&reg, look, "");
            let r = bpe_core::resolve(&scene, &reg);
            assert!(r.problems.is_empty(), "{look:?}: {:?}", r.problems);
            let liquids = ids(&scene, LIQUID);
            assert_eq!(liquids.len(), 1, "{look:?}");
            assert_eq!(scene.things[&liquids[0]].name, look.liquid_name());
            assert_eq!(card::<WaterLook>(&r, liquids[0], SlotId::Look), look.row().liquid);
            assert_eq!(ids(&scene, LAMP).len(), look.row().lamps.len(), "{look:?}");
        }
        let plain = start_water_scene(&reg, ReadyLook::Plain, "");
        assert!(ids(&plain, LAMP).is_empty());
        assert_eq!(plain.things[&ids(&plain, LIQUID)[0]].name, "Water");
    }

    #[test]
    fn the_looks_say_what_they_are() {
        assert_eq!(ReadyLook::Molten.words(), "Lava that glows and throws sparks, lighting the dark rock round it.");
        assert_eq!(ReadyLook::PaintOnPaper.words(), "Thick glossy paint on paper cut-outs.");
        assert_eq!(ReadyLook::Molten.liquid_name(), "Lava");
        assert_eq!(ReadyLook::Cartoon.swatch(), (rgb(0x3aa0ff), rgb(0xffd24a)));
    }
}
