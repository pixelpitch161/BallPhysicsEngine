//! How a Water scene looks, read from its cards (Water mode part 6): [`plain_look`] for the plain
//! drawing, and [`look_of`] for the look drawing. Left out of the take's print (the kit's `build.rs`), so a
//! change here never makes a final again.

use bpe_core::{Address, Color, Gradient, Resolved, Scene, Settings, SlotId, ThingId};
use bpe_water::draw::look::{
    self, BackLook, FinishLook, GRADIENT_SAMPLES, Lamp, LightLook, LiquidLook, Look, MaterialLook, Quality,
};
use bpe_water::draw::{PlainLiquid, PlainLook};
use glam::Vec2;

use super::draft::{Keyed, MOST_LIQUIDS};
use super::{LAMP, LIQUID, PIECE, WALL, course_of};
use crate::modules::water::look::{ColourFrom, GlowFrom, LampCard, LampKind, LightQuality, WaterLightCard};
use crate::modules::water::{BackCard, BackShows, MaterialCard, MaterialKind, WaterLook};
use crate::things::scene::SceneSettings;

/// What the plain drawing is told by the cards at `time`: each Liquid's Colour, Deep colour and
/// Clarity, the Course's Rock colour, each Piece's and Wall's Material Colour (in the order they are
/// laid over the rock), and the Back (darker rock, a colour, a gradient's middle; a Picture shows as
/// darker rock until pictures are drawn).
pub fn plain_look(scene: &Scene, resolved: &Resolved, time: f64) -> PlainLook {
    let rock = course_of(scene)
        .and_then(|id| Keyed::of(scene, resolved, id, SlotId::Material))
        .map_or_else(|| MaterialCard::default().colour, |k| k.card::<MaterialCard>(time).colour);
    let rock = srgb(rock);
    let back: BackCard = Keyed::of(scene, resolved, ThingId::ROOT, SlotId::Back).unwrap_or_default().card(time);
    let darker = rock.map(|c| c * (1.0 - back.darker.clamp(0.0, 1.0)));
    let back = match back.shows {
        BackShows::DarkerRock | BackShows::Picture => darker,
        BackShows::Colour => srgb(back.colour),
        BackShows::Gradient => srgb(Color::from_linear(back.gradient.sample(0.5))),
    };
    let liquids = scene
        .order
        .iter()
        .filter(|id| scene.things[*id].kind == LIQUID && resolved.things.contains_key(*id))
        .take(MOST_LIQUIDS)
        .map(|id| {
            let look: WaterLook = Keyed::of(scene, resolved, *id, SlotId::Look).unwrap_or_default().card(time);
            PlainLiquid { colour: srgb(look.colour), deep: srgb(look.deep), clarity: look.clarity }
        })
        .collect();
    let solids = scene
        .order
        .iter()
        .filter(|id| matches!(scene.things[*id].kind.as_str(), PIECE | WALL) && resolved.things.contains_key(*id))
        .take(bpe_water::draw::MOST_COLOURED)
        .map(|id| {
            let m: MaterialCard = Keyed::of(scene, resolved, *id, SlotId::Material).unwrap_or_default().card(time);
            srgb(m.colour)
        })
        .collect();
    PlainLook { back, rock, liquids, solids }
}

/// What the look drawing is told by the cards at water time `time` (decision 29): each enabled
/// Liquid's Look (in the scene's order, at most [`MOST_LIQUIDS`]), the Course's Rock and each enabled
/// Piece's and Wall's Material (in the order the solid field numbers them), the Back, the Light card,
/// every enabled Lamp (the drawing lights with the first [`bpe_water::draw::look::MOST_LAMPS`]), and
/// the scene's Bloom and Exposure. Reads no file: a Back picture is the PNG the scene keeps under its
/// name, or else its name as a path.
pub fn look_of(scene: &Scene, resolved: &Resolved, time: f64) -> Look {
    let on = |id: &ThingId| resolved.things.contains_key(id);
    let of_kind = |kinds: &[&str]| -> Vec<ThingId> {
        scene.order.iter().copied().filter(|id| kinds.contains(&scene.things[id].kind.as_str()) && on(id)).collect()
    };
    let card = |id: ThingId, slot: SlotId| Keyed::of(scene, resolved, id, slot).unwrap_or_default();
    let liquids = of_kind(&[LIQUID])
        .into_iter()
        .take(MOST_LIQUIDS)
        .map(|id| liquid_look(&card(id, SlotId::Look).card(time)))
        .collect();
    let rock = course_of(scene).map_or_else(MaterialCard::default, |id| card(id, SlotId::Material).card(time));
    let solids = of_kind(&[PIECE, WALL])
        .into_iter()
        .take(bpe_water::draw::MOST_COLOURED)
        .map(|id| material_look(&card(id, SlotId::Material).card(time)))
        .collect();
    let back: BackCard = card(ThingId::ROOT, SlotId::Back).card(time);
    let light: WaterLightCard = card(ThingId::ROOT, SlotId::WaterLight).card(time);
    let lamps = of_kind(&[LAMP])
        .into_iter()
        .map(|id| lamp_look(&card(id, SlotId::Main).card(time), scene.things[&id].transform.position))
        .collect();
    Look {
        liquids,
        rock: material_look(&rock),
        solids,
        back: back_look(scene, &back),
        light: LightLook {
            ambient: light.ambient,
            bounce: light.bounce,
            caustics: light.caustics,
            shafts: light.shafts,
            quality: match light.quality {
                LightQuality::Draft => Quality::Draft,
                LightQuality::Good => Quality::Good,
                LightQuality::Best => Quality::Best,
            },
        },
        lamps,
        time: time as f32,
        finish: finish_of(scene, resolved, time),
    }
}

fn srgb(c: Color) -> [f32; 3] {
    [c.r, c.g, c.b].map(|v| f32::from(v) / 255.0)
}

/// A gradient's [`GRADIENT_SAMPLES`] linear samples.
fn samples(g: &Gradient) -> Vec<[f32; 3]> {
    (0..GRADIENT_SAMPLES)
        .map(|i| {
            let [r, g2, b, _] = g.sample(i as f32 / (GRADIENT_SAMPLES - 1) as f32);
            [r, g2, b]
        })
        .collect()
}

fn liquid_look(l: &WaterLook) -> LiquidLook {
    LiquidLook {
        colour: srgb(l.colour),
        colour_from: match l.colour_from {
            ColourFrom::ItsOwn => look::ColourFrom::ItsOwn,
            ColourFrom::Speed => look::ColourFrom::Speed,
            ColourFrom::Depth => look::ColourFrom::Depth,
        },
        // (Read only when the colour is from it.)
        gradient: if l.colour_from == ColourFrom::ItsOwn { Vec::new() } else { samples(&l.colour_gradient) },
        clarity: l.clarity,
        deep: srgb(l.deep),
        solid: l.solid,
        bend: l.bend,
        split: l.split,
        smoothing: l.smoothing,
        streaks: l.streaks,
        shine: l.shine,
        shine_size: l.shine_size,
        edge_sheen: l.edge_sheen,
        ripples: l.ripples,
        thin_film: l.thin_film,
        glow: l.glow,
        glow_colour: srgb(l.glow_colour),
        glow_from: match l.glow_from {
            GlowFrom::Always => look::GlowFrom::Always,
            GlowFrom::Speed => look::GlowFrom::Speed,
        },
        outline: l.outline,
        outline_colour: srgb(l.outline_colour),
        bands: l.bands,
        band_softness: l.band_softness,
        foam: l.foam,
        foam_size: l.foam_size,
        foam_colour: srgb(l.foam_colour),
        bubbles: l.bubbles,
        bubble_size: l.bubbles_size,
        bubble_colour: srgb(l.bubbles_colour),
        bubbles_life: l.bubbles_life,
        mist: l.mist,
        mist_size: l.mist_size,
        mist_colour: srgb(l.mist_colour),
        sparks: l.sparks,
        spark_size: l.sparks_size,
        spark_colour: srgb(l.sparks_colour),
        sparks_life: l.sparks_life,
    }
}

fn material_look(m: &MaterialCard) -> MaterialLook {
    MaterialLook {
        kind: match m.material {
            MaterialKind::Rock => look::MaterialKind::Rock,
            MaterialKind::Glass => look::MaterialKind::Glass,
            MaterialKind::Paper => look::MaterialKind::Paper,
            MaterialKind::Basalt => look::MaterialKind::Basalt,
            MaterialKind::Flat => look::MaterialKind::Flat,
        },
        colour: srgb(m.colour),
        second: srgb(m.second_colour),
        texture: m.texture_size,
        depth: m.depth,
        roughness: m.roughness,
        outline: m.outline,
        outline_colour: srgb(m.outline_colour),
        wet_marks: m.wet_marks,
    }
}

fn back_look(scene: &Scene, b: &BackCard) -> BackLook {
    match b.shows {
        BackShows::DarkerRock => BackLook::DarkerRock(b.darker),
        BackShows::Colour => BackLook::Colour(srgb(b.colour)),
        BackShows::Gradient => BackLook::Gradient { samples: samples(&b.gradient), angle: b.angle },
        BackShows::Picture => BackLook::Picture(look::BackPicture {
            name: b.picture.clone(),
            kept: scene.pictures.get(&b.picture).map(|p| p.bytes.clone()),
        }),
    }
}

/// Lamp `id`'s card at water time `time` (with its keyframes), for the stage's markers.
pub fn lamp_at(scene: &Scene, resolved: &Resolved, id: ThingId, time: f64) -> Option<LampCard> {
    Keyed::of(scene, resolved, id, SlotId::Main).map(|k| k.card(time))
}

fn lamp_look(l: &LampCard, place: Vec2) -> Lamp {
    Lamp {
        kind: match l.kind {
            LampKind::Spot => look::LampKind::Spot,
            LampKind::Strip => look::LampKind::Strip,
            LampKind::Sun => look::LampKind::Sun,
        },
        place,
        aim: l.aim,
        colour: srgb(l.colour),
        brightness: l.brightness,
        spread: l.spread,
        length: l.length,
        softness: l.softness,
        shadows: l.shadows,
    }
}

/// The scene's Bloom and Exposure (its Picture settings) at `time`, its Effects over the picture
/// and its Grade (decision 42).
fn finish_of(scene: &Scene, resolved: &Resolved, time: f64) -> FinishLook {
    let Some(root) = resolved.things.get(&ThingId::ROOT) else { return FinishLook::default() };
    let mut params = root.params.clone();
    for key in ["bloom", "exposure"] {
        if let Some(v) = scene.tracks.get(&Address::own(ThingId::ROOT, key)).and_then(|t| t.value_at(time)) {
            params.set(key, v);
        }
    }
    let s = SceneSettings::from_params(&params);
    // (Each card with its keyframes at `time`.)
    let at_time = |slot: SlotId| {
        let mut card = root.slots.get(&slot)?.clone();
        for (a, track) in scene.tracks.iter().filter(|(a, _)| a.thing == ThingId::ROOT && a.slot == Some(slot)) {
            if let Some(v) = track.value_at(time) {
                card.params.set(&a.key, v);
            }
        }
        Some(card)
    };
    let effects = at_time(SlotId::Shader)
        .filter(|c| c.kind == crate::modules::effects::EFFECTS)
        .map(|c| crate::modules::effects::draw::calls(&c, ThingId::ROOT, SlotId::Shader))
        .unwrap_or_default()
        .into_iter()
        .filter(|e| e.stage == bpe_gfx::Stage::Picture)
        .collect();
    let grade = crate::modules::grade::stack(at_time(SlotId::Grade).as_ref(), None);
    FinishLook { bloom: s.bloom, exposure: s.exposure, effects, grade, video: (s.width.max(1), s.height.max(1)) }
}
