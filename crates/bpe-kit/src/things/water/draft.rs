//! A Water scene as the water engine's inputs (Water mode part 2b): the carved course and its
//! Rock's Wetting, the Pieces and Walls laid over it (part 3, [`course`]), the Liquids' Feel, the
//! Pours, Trapped air, gravity and Bubbles rise read once a frame from their keyframes, and a
//! fingerprint of
//! everything the physics reads ([`WaterSetup::print`]), so the editor's draft starts over for a
//! physics edit and never for a look, a lamp or the camera.
//!
//! Building a setup is cheap (no mask, no field): the editor makes one for every edit, and the
//! draft's own thread builds the carved field from it ([`WaterSetup::field`]).

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use bpe_core::{Params, Resolved, Scene, Settings, SlotId, Strokes, Target, ThingId, Track};
use bpe_water::{
    Drain, Drop, Feel, Layout, LumpKind, Lumps, MOST_DRAINS, MixWhen, Numbers, Pour, STOPS, SolidField, Throw, Timed,
    WATER, Water, WaterGpu,
};
use glam::Vec2;

use super::{DRAIN, LIQUID, PIECE, POUR, WALL, carving_of, course_of};
use crate::modules::water::{
    CarveMask, CourseEdge, DrainCard, DrainKind, FeelCard, MaterialCard, MixesWhen, MixingCard, PourCard, PourKind,
    WaterWorld, layout_for, mask_for,
};

pub mod course;
pub mod inputs;
pub mod slow;

pub use course::{Form, SolidSetup};
pub use inputs::{InputPart, WaterInputs};
pub use slow::Speeds;

pub use super::look::plain_look;

/// The most drops a draft holds (water spec, Detail): past it the pours stop.
pub const DRAFT_MOST: u32 = 500_000;
/// The most drops a final holds (water spec, Caps): past it the pours stop.
pub const FINAL_MOST: u32 = 8_000_000;

/// Which of a Water scene's details a setup is at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Detail {
    /// Draft detail: what the editor plays while editing.
    Draft,
    /// Final detail: what Make final works out and the video shows.
    Final,
}

/// The most liquids the water tells apart.
pub const MOST_LIQUIDS: usize = bpe_water::drops::numbers::MOST_LIQUIDS;

/// What the stage says about a pour with no Liquid to pour.
pub const NO_LIQUID: &str = "This pour has no Liquid to pour.";
/// What the stage says about a pour of a Liquid past the most the water tells apart.
pub const TOO_MANY_LIQUIDS: &str = "The water tells at most 16 liquids apart: this one isn't poured.";
/// What the stage says about a tap past the most the water pours at once.
pub const TOO_MANY_TAPS: &str = "The water pours at most 64 taps at once: this one isn't poured.";
/// What the stage says about a tap whose spout is in a solid.
pub const IN_ROCK: &str = "This tap's spout is inside a solid: carve the rock away round it, or move it, to pour.";
/// What the stage says about a Piece or Wall with a part under two draft cells thick: the draft
/// takes a thin wall as thicker and may let water through a thin block (neither is thickened past
/// what the physics needs), so the finer final can play it a little differently.
pub const THIN: &str = "Thinner than two draft cells: the draft can't hold it exactly, so the water may act a little differently in the final.";
/// What the stage says about a Drain past the most the water has.
pub const TOO_MANY_DRAINS: &str = "The water has at most 32 drains: this one takes nothing.";
/// What the stage says about a tap Fed by drains with no drain that Feeds pours.
pub const NO_FEEDING_DRAIN: &str = "This tap is fed by drains, but no Drain feeds pours: it pours nothing.";
/// How fast a Drain (and the Edge of the course) takes the liquid in its reach at Rate 1: the share
/// a second is `1 - exp(-pull)`, so most of what is in a hole is gone within a few frames.
pub const DRAIN_PULL: f32 = 30.0;
/// How fast a Leak takes it at Rate 1: about two thirds of what sits in it in a second.
pub const LEAK_PULL: f32 = 1.0;
/// What the stage says about a Piece or Wall whose Moves card asks it to move.
pub const MOVES_LATER: &str = "Moving pieces arrive in a later part of Water mode: this one stays where it is for now.";

/// A card's settings at the start and the keyframes that move them.
#[derive(Clone, Debug, Default)]
pub(crate) struct Keyed {
    params: Params,
    tracks: Vec<(String, Track)>,
}

impl Keyed {
    /// The card in `slot` of `thing`, as resolved, with its keyframes; `None` when it has none.
    pub(crate) fn of(scene: &Scene, resolved: &Resolved, thing: ThingId, slot: SlotId) -> Option<Keyed> {
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
    pub(crate) fn card<T: Settings>(&self, time: f64) -> T {
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
    /// Its Mixing card (part 4.2: [`MIXING_KEYS`] reach the physics; Un-mix is part 4.4's). With
    /// none, its colour mixes with nothing and it keeps nothing apart.
    mixing: Option<Keyed>,
}

/// The Mixing card's settings the physics reads.
pub(super) const MIXING_KEYS: [&str; 3] = ["keeps_apart", "mix_speed", "mixes_when"];

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

/// One Drain as the water sees it.
#[derive(Clone, Debug)]
pub struct DrainSetup {
    pub id: ThingId,
    pub place: Vec2,
    card: Keyed,
}

/// One frame's physics inputs, read from the keyframes at the frame's start.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameInputs {
    pub gravity: Vec2,
    /// How many times the gravity trapped air rises at (the Water card's Bubbles rise).
    pub air_lift: f32,
    /// Each liquid's feel, by its number.
    pub feels: Vec<Feel>,
    /// Every tap, in the setup's order (which stays the same all run).
    pub pours: Vec<Pour>,
    /// The drops the frame's rain, drips and bursts give birth to, each at its moment (only with
    /// the field: [`WaterSetup::inputs_on`]).
    pub timed: Vec<Timed>,
    /// Every drain, in the setup's order.
    pub drains: Vec<Drain>,
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
    /// The enabled Drains, in the scene's order (at most [`MOST_DRAINS`]).
    pub drains: Vec<DrainSetup>,
    /// The enabled Pieces and Walls, in the scene's order: the order they are laid over the rock.
    pub solids: Vec<SolidSetup>,
    /// What the stage says about a pour that can't pour, or a piece or wall that is not as it will be.
    pub notes: Vec<(ThingId, &'static str)>,
    /// Changes exactly when what the physics reads changes.
    pub print: u64,
    /// Draft or Final.
    pub detail: Detail,
    world: Keyed,
    /// The Course's Rock card.
    rock: Keyed,
}

impl WaterSetup {
    /// The scene's water at its Draft detail.
    pub fn of(scene: &Scene, resolved: &Resolved) -> WaterSetup {
        WaterSetup::of_detail(scene, resolved, Detail::Draft)
    }

    /// The scene's water at its Final detail (Make final, part 8).
    pub fn final_of(scene: &Scene, resolved: &Resolved) -> WaterSetup {
        WaterSetup::of_detail(scene, resolved, Detail::Final)
    }

    /// The same at `cell` pixels a cell (tests, and the measuring runs).
    pub fn with_cell(mut self, cell: f32) -> WaterSetup {
        let mut h = DefaultHasher::new();
        (self.print, cell.to_bits()).hash(&mut h);
        self.print = h.finish();
        self.cell = cell;
        self
    }

    fn of_detail(scene: &Scene, resolved: &Resolved, detail: Detail) -> WaterSetup {
        let world = Keyed::of(scene, resolved, ThingId::ROOT, SlotId::Water).unwrap_or_default();
        let w: WaterWorld = world.card(0.0);
        let cell = match detail {
            Detail::Draft => w.draft_detail.cell_px(),
            Detail::Final => w.final_detail.cell_px(),
        };
        let size = w.course_size();
        let course = course_of(scene).filter(|id| resolved.things.contains_key(id));
        let strokes = Arc::new(if course.is_some() { carving_of(scene) } else { Strokes::default() });
        let rock = course.and_then(|id| Keyed::of(scene, resolved, id, SlotId::Material)).unwrap_or_default();
        let on = |id: &ThingId| resolved.things.contains_key(id);
        let of_kind = |kind: &str| -> Vec<ThingId> {
            scene.order.iter().copied().filter(|id| scene.things[id].kind == kind && on(id)).collect()
        };
        let liquid_ids = of_kind(LIQUID);
        let liquids: Vec<LiquidSetup> = liquid_ids
            .iter()
            .take(MOST_LIQUIDS)
            .map(|id| LiquidSetup {
                id: *id,
                feel: Keyed::of(scene, resolved, *id, SlotId::Feel).unwrap_or_default(),
                mixing: Keyed::of(scene, resolved, *id, SlotId::Mixing),
            })
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
        let mut drains = Vec::new();
        for id in of_kind(DRAIN) {
            let Some(card) = Keyed::of(scene, resolved, id, SlotId::Main) else { continue };
            if drains.len() >= MOST_DRAINS {
                notes.push((id, TOO_MANY_DRAINS));
                continue;
            }
            drains.push(DrainSetup { id, place: scene.things[&id].transform.position, card });
        }
        let feeding = drains.iter().any(|d| d.card.card::<DrainCard>(0.0).feeds_pours);
        for p in &pours {
            let c: PourCard = p.card.card(0.0);
            if p.kind == PourKind::Tap && p.liquid.is_some() && c.fed_by_drains && !feeding {
                notes.push((p.id, NO_FEEDING_DRAIN));
            }
        }
        let solids: Vec<SolidSetup> = scene
            .order
            .iter()
            .filter(|id| matches!(scene.things[*id].kind.as_str(), PIECE | WALL) && on(id))
            .filter_map(|id| SolidSetup::of(scene, resolved, *id))
            .collect();
        for s in &solids {
            if s.thinnest() < 2.0 * cell {
                notes.push((s.id, THIN));
            }
            if s.moves() {
                notes.push((s.id, MOVES_LATER));
            }
        }
        let mut h = DefaultHasher::new();
        let detail_key = match detail {
            Detail::Draft => "draft_detail",
            Detail::Final => "final_detail",
        };
        world.hash_into(Some(&world_keys(detail_key)), &mut h);
        strokes.hash64().hash(&mut h);
        rock.hash_into(Some(&["wetting"]), &mut h);
        for l in &liquids {
            l.id.hash(&mut h);
            l.feel.hash_into(None, &mut h);
            l.mixing.is_some().hash(&mut h);
            if let Some(m) = &l.mixing {
                m.hash_into(Some(&MIXING_KEYS), &mut h);
            }
        }
        for p in &pours {
            (p.id, p.liquid, p.place.x.to_bits(), p.place.y.to_bits()).hash(&mut h);
            p.card.hash_into(None, &mut h);
        }
        for d in &drains {
            (d.id, d.place.x.to_bits(), d.place.y.to_bits()).hash(&mut h);
            d.card.hash_into(None, &mut h);
        }
        for s in &solids {
            (s.id, s.kind, s.at.x.to_bits(), s.at.y.to_bits(), s.turn.to_bits()).hash(&mut h);
            s.form.hash_into(None, &mut h);
            s.material.hash_into(Some(&["wetting"]), &mut h);
        }
        let print = h.finish();
        WaterSetup { size, cell, strokes, liquids, pours, drains, solids, notes, print, detail, world, rock }
    }

    /// The grid the water runs on.
    pub fn layout(&self) -> Layout {
        layout_for(self.size, self.cell)
    }

    /// The carved course (from the cache of recent ones).
    pub fn mask(&self) -> Arc<CarveMask> {
        mask_for(self.size, &self.strokes)
    }

    /// The solid field the water feels and the drawing shows: the carved rock, as wet as the
    /// Course's Rock, with every Piece and Wall laid over it in order, each as wet as its Material.
    pub fn field(&self) -> SolidField {
        self.field_with(&self.mask())
    }

    /// The same with the rock carved as `mask` says (the Carve tool's stroke being painted).
    pub fn field_with(&self, mask: &CarveMask) -> SolidField {
        let mut field = mask.field(self.cell).with_wetting(self.wetting());
        let least = bpe_water::drops::LEAST_WALL * self.cell;
        for (k, s) in self.solids.iter().enumerate() {
            if let Some(part) = s.part() {
                field.add_part(&part, u16::try_from(k + 1).unwrap_or(u16::MAX), least);
            }
        }
        field
    }

    /// The top-most Piece or Wall whose body is within `reach` pixels of world point `p`.
    pub fn solid_at(&self, p: Vec2, reach: f32) -> Option<ThingId> {
        self.solids.iter().rev().find(|s| s.distance(p) <= reach).map(|s| s.id)
    }

    /// Where a piece or a pen point snaps to (world pixels): every Piece's and Wall's corners but
    /// `except`'s.
    pub fn snap_points(&self, except: Option<ThingId>) -> Vec<Vec2> {
        self.solids.iter().filter(|s| Some(s.id) != except).flat_map(SolidSetup::corners).collect()
    }

    /// Piece or Wall `id`, if it is in the course.
    pub fn solid(&self, id: ThingId) -> Option<&SolidSetup> {
        self.solids.iter().find(|s| s.id == id)
    }

    /// The Course's Rock's Wetting (the Material card's default with no Course).
    pub fn wetting(&self) -> f32 {
        self.rock.card::<MaterialCard>(0.0).wetting
    }

    /// The engine's numbers for this detail, with trapped air as the Water card says.
    pub fn numbers(&self) -> Numbers {
        let w: WaterWorld = self.world.card(0.0);
        Numbers { air: u32::from(w.trapped_air), ..Numbers::for_cell(self.cell) }
    }

    /// What the stage says about pour, piece or wall `id`, if anything.
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
                // (No Mixing card: no Mix speed, nothing kept apart.)
                let m: MixingCard = match &l.mixing {
                    Some(m) => m.card(time),
                    None => MixingCard { keeps_apart: 0.0, mix_speed: 0.0, ..MixingCard::default() },
                };
                Feel {
                    gravity: 1.0,
                    surface_pull: f.surface_pull,
                    thickness: f.thickness,
                    stickiness: f.stickiness,
                    weight: f.weight,
                    settles: 0.0,
                    keeps_apart: m.keeps_apart,
                    mix_speed: m.mix_speed,
                    mixes_when: match m.mixes_when {
                        MixesWhen::Always => MixWhen::Always,
                        MixesWhen::OnlyWhenStirred => MixWhen::Stirred,
                        MixesWhen::OnlyOnHits => MixWhen::Hits,
                    },
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
                    fed: c.fed_by_drains,
                })
            })
            .collect();
        let drains = self.drains.iter().map(|d| drain_of(d, d.card.card(time))).collect();
        FrameInputs { gravity: w.gravity, air_lift: w.bubble_rise, feels, pours, timed: Vec::new(), drains }
    }

    /// Pour `p`'s lumps as they are at `time` seconds, if it is a Rain, Drip or Burst with a Liquid
    /// to pour (its Drops a second, Drop size, Burst size, Every, Starts and Stops are read at the
    /// start; its Width at `time`).
    fn lumps_of(&self, p: &PourSetup, time: f64) -> Option<Lumps> {
        let liquid = p.liquid?;
        let c0: PourCard = p.card.card(0.0);
        let c: PourCard = p.card.card(time);
        let finite = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
        let per_second = f64::from(finite(c0.drops_a_second));
        let (kind, width, size, every) = match p.kind {
            PourKind::Rain => (LumpKind::Rain, finite(c.width), c0.drop_size, 1.0 / per_second),
            PourKind::Drip => (LumpKind::Drip, 0.0, c0.drop_size, 1.0 / per_second),
            PourKind::Burst => (LumpKind::Burst, 0.0, c0.burst_size, f64::from(finite(c0.every))),
            PourKind::Tap | PourKind::FillAtTheStart => return None,
        };
        if p.kind != PourKind::Burst && per_second <= 0.0 {
            return None;
        }
        Some(Lumps {
            kind,
            place: p.place,
            width,
            size: finite(size),
            every,
            from: f64::from(c0.starts),
            until: f64::from(c0.stops),
            seed: u64::from(p.id.0).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0x5851_F42D_4C95_7F2D,
            kind_bits: liquid as u32 | STOPS,
            colour: WATER,
        })
    }

    /// The drops frame `frame`'s rain, drips and bursts give birth to on `field`, each at its
    /// moment, set off by its pour's Aim, Speed and Spread then.
    pub fn timed_on(&self, frame: u32, field: &SolidField) -> Vec<Timed> {
        let (t0, t1) = (f64::from(frame) / 60.0, f64::from(frame + 1) / 60.0);
        let clear = self.numbers().wall_off * self.cell;
        let mut out = Vec::new();
        for p in &self.pours {
            let Some(lumps) = self.lumps_of(p, t0) else { continue };
            let throw = |at: f64| {
                let c: PourCard = p.card.card(at);
                Throw { aim: c.aim, speed: c.speed, spread: c.spread }
            };
            out.extend(lumps.born(&field.layout, field, clear, t0, t1, throw));
        }
        out
    }

    /// The physics inputs of frame `frame` on `field` (this setup's): each tap's stream as wide as
    /// its spout has room for in the carved course, none for a spout in the rock.
    pub fn inputs_on(&self, frame: u32, field: &SolidField) -> FrameInputs {
        let mut inputs = self.inputs_at(frame);
        let clear = self.numbers().wall_off * self.cell;
        for pour in &mut inputs.pours {
            pour.width = open_width(pour, field, clear);
        }
        inputs.timed = self.timed_on(frame, field);
        inputs
    }

    /// Sets `water`'s inputs for its next frame on `field` (this setup's): gravity, Bubbles rise,
    /// the liquids' feel, the drains, and the taps and the rain, drips and bursts, none of those
    /// while the body holds `most` liquid drops or more (by the liquid: trapped air comes and goes,
    /// and would stop and start the pours). The draft, the final and a draft's export all play a
    /// frame this way.
    pub fn set_inputs(&self, water: &mut Water, field: &SolidField, most: u32) {
        let inputs = self.inputs_on(water.frames(), field);
        water.set_gravity(inputs.gravity);
        water.set_air_lift(inputs.air_lift);
        if !inputs.feels.is_empty() {
            water.set_feels(&inputs.feels);
        }
        let full = water.liquid() >= most;
        water.set_pours(if full { &[] } else { &inputs.pours });
        water.set_timed(if full { &[] } else { &inputs.timed });
        water.set_drains(&inputs.drains);
    }

    /// A body on `field` (this setup's) from a restart point's `state`, made with the inputs of the
    /// frame it is at.
    pub fn resume(
        &self,
        gpu: Arc<WaterGpu>,
        field: &SolidField,
        state: &bpe_water::WaterState,
    ) -> anyhow::Result<Water> {
        let inputs = self.inputs_on(state.frames, field);
        let feels = if inputs.feels.is_empty() { vec![Feel::default()] } else { inputs.feels.clone() };
        let numbers = Numbers { air_lift: inputs.air_lift, ..self.numbers() };
        Water::from_state(gpu, field, inputs.gravity, numbers, &feels, state)
    }

    /// About how many drops the final holds at most: its fills and what its taps pour in `seconds`
    /// (an estimate: drops that leave the course, or a full course, make it less).
    pub fn drops_about(&self, field: &SolidField, seconds: f64) -> u32 {
        let fills = self.fills(&field.layout, field).len() as f64;
        let spacing = f64::from(self.cell) * 0.5;
        let mut poured = 0.0;
        for p in self.pours.iter().filter(|p| p.kind == PourKind::Tap && p.liquid.is_some()) {
            let c: PourCard = p.card.card(0.0);
            let from = f64::from(c.starts).clamp(0.0, seconds);
            let until = f64::from(c.stops).clamp(from, seconds);
            let width = f64::from((c.rate * c.width).max(0.0));
            poured += width * f64::from(c.speed.max(0.0)) * (until - from) / (spacing * spacing);
        }
        let layout = self.layout();
        for p in &self.pours {
            if let Some(lumps) = self.lumps_of(p, 0.0) {
                poured += lumps.amount(&layout, 0.0, seconds);
            }
        }
        (fills + poured).min(f64::from(FINAL_MOST)) as u32
    }

    /// The taps (with a Liquid to pour) whose spouts are in the solid of `field`.
    pub fn taps_in_rock(&self, field: &SolidField) -> Vec<ThingId> {
        let clear = self.numbers().wall_off * self.cell;
        let taps = self.pours.iter().filter(|p| p.kind == PourKind::Tap && p.liquid.is_some());
        taps.filter(|p| !open_at(field, p.place, clear)).map(|p| p.id).collect()
    }

    /// The drops of every Fill at the start, on `layout`, where `field` is open: each box (kept to
    /// the course) filled from its floor, and a later box's drops left out where an earlier box
    /// already put water.
    pub fn fills(&self, layout: &Layout, field: &SolidField) -> Vec<Drop> {
        let rest = self.numbers().wall_off;
        let mut out: Vec<Drop> = Vec::new();
        let mut taken: HashMap<(i32, i32), Vec<Vec2>> = HashMap::new();
        let bin = |p: Vec2| ((p.x * 2.0).floor() as i32, (p.y * 2.0).floor() as i32);
        for p in self.pours.iter().filter(|p| p.kind == PourKind::FillAtTheStart) {
            let Some(liquid) = p.liquid else { continue };
            let c: PourCard = p.card.card(0.0);
            let half = c.fill_size.max(Vec2::ZERO) / 2.0;
            let (lo, hi) = ((p.place - half).max(-self.size / 2.0), (p.place + half).min(self.size / 2.0));
            if !(lo.x < hi.x && lo.y < hi.y) {
                continue;
            }
            let kind = liquid as u32 | STOPS;
            let mine = bpe_water::fill_stacked(layout, field, lo, hi, kind, rest);
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
        let inputs = self.inputs_on(0, field);
        let feels = if inputs.feels.is_empty() { vec![Feel::default()] } else { inputs.feels.clone() };
        let numbers = Numbers { air_lift: inputs.air_lift, ..self.numbers() };
        let mut water = Water::new(gpu, field, inputs.gravity, numbers, &feels);
        let mut fills = self.fills(&field.layout, field);
        fills.truncate(most as usize);
        if !fills.is_empty() {
            water.add(&fills);
        }
        water.set_pours(&inputs.pours);
        water.set_timed(&inputs.timed);
        water.set_drains(&inputs.drains);
        water
    }
}

/// Drain `d` as the water takes it, its card `c` as it is in the frame.
fn drain_of(d: &DrainSetup, c: DrainCard) -> Drain {
    let rate = if c.rate.is_finite() { c.rate.max(0.0) } else { 0.0 };
    let reach = if c.radius.is_finite() { c.radius.max(0.0) } else { 0.0 };
    let disc = |pull: f32| Drain { place: d.place, reach, pull: pull * rate, edges: 0, feeds: c.feeds_pours };
    match c.kind {
        DrainKind::Drain => disc(DRAIN_PULL),
        DrainKind::Leak => disc(LEAK_PULL),
        DrainKind::EdgeOfTheCourse => {
            let edges = match c.edge {
                CourseEdge::Bottom => Drain::BOTTOM,
                CourseEdge::Top => Drain::TOP,
                CourseEdge::Left => Drain::LEFT,
                CourseEdge::Right => Drain::RIGHT,
                CourseEdge::All => Drain::ALL_EDGES,
            };
            Drain { edges, feeds: c.feeds_pours, ..Drain::edge(edges, DRAIN_PULL * rate) }
        }
    }
}

/// The Water card's settings the physics reads (with the detail's own).
fn world_keys(detail: &str) -> [&str; 6] {
    ["gravity", "screens_wide", "screens_tall", detail, "trapped_air", "bubble_rise"]
}

/// Whether world point `p` is more than `clear` pixels from the rock of `field` (a field that
/// isn't a number there counts as rock).
fn open_at(field: &SolidField, p: Vec2, clear: f32) -> bool {
    field.at(p) > clear
}

/// How wide `pour`'s stream can be with every drop born at least `clear` pixels from the rock of
/// `field`: the width asked for, or less where the rock is closer to the middle of its spout on
/// either side, and 0 for a spout in the rock. (Never wider than the field, whatever is asked.)
fn open_width(pour: &Pour, field: &SolidField, clear: f32) -> f32 {
    if !open_at(field, pour.place, clear) {
        return 0.0;
    }
    let l = &field.layout;
    let room = Vec2::new(l.cols as f32, l.rows as f32).length() * l.cell;
    let most = (pour.width.max(0.0) / 2.0).min(room);
    let across = pour.way().perp();
    let step = l.cell / 2.0;
    let open =
        |d: f32| open_at(field, pour.place + across * d, clear) && open_at(field, pour.place - across * d, clear);
    let mut half = 0.0;
    while half < most {
        let next = (half + step).min(most);
        if !open(next) {
            break;
        }
        half = next;
    }
    2.0 * half
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

#[cfg(test)]
mod tests;
