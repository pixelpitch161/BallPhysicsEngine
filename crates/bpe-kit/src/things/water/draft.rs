//! A Water scene as the water engine's inputs (Water mode part 2b): the carved course, the
//! Liquids' Feel, the Pours, gravity read once a frame from its keyframes, and a fingerprint of
//! everything the physics reads ([`WaterSetup::print`]), so the editor's draft starts over for a
//! physics edit and never for a look, a lamp or the camera.
//!
//! Building a setup is cheap (no mask, no field): the editor makes one for every edit, and the
//! draft's own thread builds the carved field from it ([`WaterSetup::field`]).

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use bpe_core::{Color, Params, Resolved, Scene, Settings, SlotId, Strokes, Target, ThingId, Track};
use bpe_water::draw::{PlainLiquid, PlainLook};
use bpe_water::{Drop, Feel, Layout, Numbers, Pour, STOPS, SolidField, WATER, Water, WaterGpu};
use glam::Vec2;

use super::{LIQUID, POUR, carving_of, course_of};
use crate::modules::water::{
    BackCard, BackShows, CarveMask, FeelCard, MaterialCard, PourCard, PourKind, WaterLook, WaterWorld, layout_for,
    mask_for,
};

/// The most drops a draft holds (water spec, Detail): past it the pours stop.
pub const DRAFT_MOST: u32 = 500_000;

/// The most liquids the water tells apart.
pub const MOST_LIQUIDS: usize = bpe_water::drops::numbers::MOST_LIQUIDS;

/// What the stage says about a pour with no Liquid to pour.
pub const NO_LIQUID: &str = "This pour has no Liquid to pour.";
/// What the stage says about a Rain, Drip or Burst pour.
pub const LATER_KINDS: &str = "Rain, drips and bursts arrive in a later part of Water mode.";
/// What the stage says about a pour of a Liquid past the most the water tells apart.
pub const TOO_MANY_LIQUIDS: &str = "The water tells at most 16 liquids apart: this one isn't poured.";
/// What the stage says about a tap past the most the water pours at once.
pub const TOO_MANY_TAPS: &str = "The water pours at most 64 taps at once: this one isn't poured.";

/// A card's settings at the start and the keyframes that move them.
#[derive(Clone, Debug, Default)]
struct Keyed {
    params: Params,
    tracks: Vec<(String, Track)>,
}

impl Keyed {
    /// The card in `slot` of `thing`, as resolved, with its keyframes; `None` when it has none.
    fn of(scene: &Scene, resolved: &Resolved, thing: ThingId, slot: SlotId) -> Option<Keyed> {
        let params = resolved.things.get(&thing)?.slots.get(&slot)?.params.clone();
        let tracks = scene
            .tracks
            .iter()
            .filter(|(a, t)| a.thing == thing && a.slot == Some(slot) && !t.keys.is_empty())
            .map(|(a, t)| (a.key.clone(), t.clone()))
            .collect();
        Some(Keyed { params, tracks })
    }

    /// The card's settings at `time` seconds.
    fn card<T: Settings>(&self, time: f64) -> T {
        if self.tracks.is_empty() {
            return T::from_params(&self.params);
        }
        let mut params = self.params.clone();
        for (key, track) in &self.tracks {
            if let Some(v) = track.value_at(time) {
                params.set(key, v);
            }
        }
        T::from_params(&params)
    }

    /// Adds the settings named in `keys` (all of them when `None`) and their keyframes to `h`.
    fn hash_into(&self, keys: Option<&[&str]>, h: &mut DefaultHasher) {
        let wanted = |k: &str| keys.is_none_or(|keys| keys.contains(&k));
        for (k, v) in self.params.0.iter().filter(|(k, _)| wanted(k)) {
            format!("{k}={v:?}").hash(h);
        }
        for (k, t) in self.tracks.iter().filter(|(k, _)| wanted(k)) {
            format!("{k}~{t:?}").hash(h);
        }
    }
}

/// One Liquid as the water sees it.
#[derive(Clone, Debug)]
pub struct LiquidSetup {
    pub id: ThingId,
    feel: Keyed,
}

/// One Pour as the water sees it.
#[derive(Clone, Debug)]
pub struct PourSetup {
    pub id: ThingId,
    /// Its Liquid's number (`None`: it pours nothing).
    pub liquid: Option<usize>,
    pub kind: PourKind,
    pub place: Vec2,
    card: Keyed,
}

/// One frame's physics inputs, read from the keyframes at the frame's start.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameInputs {
    pub gravity: Vec2,
    /// Each liquid's feel, by its number.
    pub feels: Vec<Feel>,
    /// Every tap, in the setup's order (which stays the same all run).
    pub pours: Vec<Pour>,
}

/// A Water scene as the water engine's inputs, at draft detail.
#[derive(Clone, Debug)]
pub struct WaterSetup {
    /// The course's size, pixels.
    pub size: Vec2,
    /// Pixels a cell.
    pub cell: f32,
    pub strokes: Arc<Strokes>,
    pub liquids: Vec<LiquidSetup>,
    pub pours: Vec<PourSetup>,
    /// What the stage says about a pour that can't pour.
    pub notes: Vec<(ThingId, &'static str)>,
    /// Changes exactly when what the physics reads changes.
    pub print: u64,
    world: Keyed,
}

impl WaterSetup {
    /// The scene's water at its Draft detail.
    pub fn of(scene: &Scene, resolved: &Resolved) -> WaterSetup {
        let world = Keyed::of(scene, resolved, ThingId::ROOT, SlotId::Water).unwrap_or_default();
        let w: WaterWorld = world.card(0.0);
        let (size, cell) = (w.course_size(), w.draft_detail.cell_px());
        let strokes = Arc::new(if resolved.things.contains_key(&course_of(scene).unwrap_or(ThingId::ROOT)) {
            carving_of(scene)
        } else {
            Strokes::default()
        });
        let on = |id: &ThingId| resolved.things.contains_key(id);
        let of_kind = |kind: &str| -> Vec<ThingId> {
            scene.order.iter().copied().filter(|id| scene.things[id].kind == kind && on(id)).collect()
        };
        let liquid_ids = of_kind(LIQUID);
        let liquids: Vec<LiquidSetup> = liquid_ids
            .iter()
            .take(MOST_LIQUIDS)
            .map(|id| LiquidSetup { id: *id, feel: Keyed::of(scene, resolved, *id, SlotId::Feel).unwrap_or_default() })
            .collect();
        let mut pours = Vec::new();
        let mut notes = Vec::new();
        let mut taps = 0;
        for id in of_kind(POUR) {
            let Some(card) = Keyed::of(scene, resolved, id, SlotId::Main) else { continue };
            let c: PourCard = card.card(0.0);
            let found = liquid_index(scene, &liquid_ids, &c.liquid);
            let mut liquid = found.filter(|i| *i < MOST_LIQUIDS);
            match (found, c.kind) {
                (_, PourKind::Rain | PourKind::Drip | PourKind::Burst) => notes.push((id, LATER_KINDS)),
                (None, _) => notes.push((id, NO_LIQUID)),
                (Some(i), _) if i >= MOST_LIQUIDS => notes.push((id, TOO_MANY_LIQUIDS)),
                (Some(_), PourKind::Tap) if taps >= bpe_water::drops::MOST_POURS => {
                    notes.push((id, TOO_MANY_TAPS));
                    liquid = None;
                }
                (Some(_), PourKind::Tap) => taps += 1,
                _ => {}
            }
            let place = scene.things[&id].transform.position;
            pours.push(PourSetup { id, liquid, kind: c.kind, place, card });
        }
        let mut h = DefaultHasher::new();
        world.hash_into(Some(&["gravity", "screens_wide", "screens_tall", "draft_detail"]), &mut h);
        strokes.hash64().hash(&mut h);
        for l in &liquids {
            l.id.hash(&mut h);
            l.feel.hash_into(None, &mut h);
        }
        for p in &pours {
            (p.id, p.liquid, p.place.x.to_bits(), p.place.y.to_bits()).hash(&mut h);
            p.card.hash_into(None, &mut h);
        }
        let print = h.finish();
        WaterSetup { size, cell, strokes, liquids, pours, notes, print, world }
    }

    /// The grid the water runs on.
    pub fn layout(&self) -> Layout {
        layout_for(self.size, self.cell)
    }

    /// The carved course (from the cache of recent ones).
    pub fn mask(&self) -> Arc<CarveMask> {
        mask_for(self.size, &self.strokes)
    }

    /// The solid field the water feels and the drawing shows.
    pub fn field(&self) -> SolidField {
        self.mask().field(self.cell)
    }

    /// The engine's numbers for this detail.
    pub fn numbers(&self) -> Numbers {
        Numbers::for_cell(self.cell)
    }

    /// What the stage says about pour `id`, if it can't pour.
    pub fn note_for(&self, id: ThingId) -> Option<&'static str> {
        self.notes.iter().find(|(i, _)| *i == id).map(|(_, n)| *n)
    }

    /// The physics inputs of frame `frame` (read at its start, `frame / 60` seconds).
    pub fn inputs_at(&self, frame: u32) -> FrameInputs {
        let time = f64::from(frame) / 60.0;
        let w: WaterWorld = self.world.card(time);
        let feels = self
            .liquids
            .iter()
            .map(|l| {
                let f: FeelCard = l.feel.card(time);
                Feel {
                    gravity: 1.0,
                    surface_pull: f.surface_pull,
                    thickness: f.thickness,
                    stickiness: f.stickiness,
                    weight: f.weight,
                    settles: 0.0,
                }
            })
            .collect();
        let pours = self
            .pours
            .iter()
            .filter(|p| p.kind == PourKind::Tap)
            .filter_map(|p| {
                let liquid = p.liquid?;
                let c: PourCard = p.card.card(time);
                Some(Pour {
                    place: p.place,
                    aim: c.aim,
                    width: (c.rate * c.width).max(0.0),
                    speed: c.speed,
                    spread: c.spread,
                    from: c.starts,
                    until: c.stops,
                    kind: liquid as u32 | STOPS,
                    colour: WATER,
                })
            })
            .collect();
        FrameInputs { gravity: w.gravity, feels, pours }
    }

    /// The drops of every Fill at the start, on `layout`, where `field` is open: each box filled
    /// from its floor, and a later box's drops left out where an earlier box already put water.
    pub fn fills(&self, layout: &Layout, field: &SolidField) -> Vec<Drop> {
        let rest = self.numbers().wall_off;
        let mut out: Vec<Drop> = Vec::new();
        let mut taken: HashMap<(i32, i32), Vec<Vec2>> = HashMap::new();
        let bin = |p: Vec2| ((p.x * 2.0).floor() as i32, (p.y * 2.0).floor() as i32);
        for p in self.pours.iter().filter(|p| p.kind == PourKind::FillAtTheStart) {
            let Some(liquid) = p.liquid else { continue };
            let c: PourCard = p.card.card(0.0);
            let half = c.fill_size.max(Vec2::ZERO) / 2.0;
            let kind = liquid as u32 | STOPS;
            let mine = bpe_water::fill_stacked(layout, field, p.place - half, p.place + half, kind, rest);
            let kept: Vec<Drop> = mine
                .into_iter()
                .filter(|d| {
                    let x = Vec2::from(d.x);
                    let (bx, by) = bin(x);
                    let near = (-1..=1).any(|i| {
                        (-1..=1).any(|j| {
                            taken.get(&(bx + i, by + j)).is_some_and(|ps| ps.iter().any(|q| q.distance(x) < 0.35))
                        })
                    });
                    !near
                })
                .collect();
            for d in &kept {
                taken.entry(bin(Vec2::from(d.x))).or_default().push(Vec2::from(d.x));
            }
            out.extend(kept);
        }
        out
    }

    /// A new body on `field` (which must be this setup's): the fills added (at most `most` drops)
    /// and frame 0's inputs set.
    pub fn start(&self, gpu: Arc<WaterGpu>, field: &SolidField, most: u32) -> Water {
        let inputs = self.inputs_at(0);
        let feels = if inputs.feels.is_empty() { vec![Feel::default()] } else { inputs.feels.clone() };
        let mut water = Water::new(gpu, field, inputs.gravity, self.numbers(), &feels);
        let mut fills = self.fills(&field.layout, field);
        fills.truncate(most as usize);
        if !fills.is_empty() {
            water.add(&fills);
        }
        water.set_pours(&inputs.pours);
        water
    }
}

/// Which of `liquids` (the enabled Liquids, in the scene's order) `target` names: a Liquid named
/// by id, the first for its kind or for anything, the first with a tag.
fn liquid_index(scene: &Scene, liquids: &[ThingId], target: &Target) -> Option<usize> {
    match target {
        Target::Thing(id) => liquids.iter().position(|l| l == id),
        Target::Kind(kind) if kind.as_str() == LIQUID => (!liquids.is_empty()).then_some(0),
        Target::Anything => (!liquids.is_empty()).then_some(0),
        Target::Tag(tag) => liquids.iter().position(|l| scene.things[l].tags.contains(tag)),
        _ => None,
    }
}

/// What the plain drawing is told by the cards at `time`: each Liquid's Colour, Deep colour and
/// Clarity, the Course's Rock colour, and the Back (darker rock, a colour, a gradient's middle;
/// a Picture shows as darker rock until pictures are drawn).
pub fn plain_look(scene: &Scene, resolved: &Resolved, time: f64) -> PlainLook {
    let srgb = |c: Color| [c.r, c.g, c.b].map(|v| f32::from(v) / 255.0);
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
    PlainLook { back, rock, liquids }
}

#[cfg(test)]
mod tests;
