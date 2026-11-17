//! How Liquid things take part in the world. Each builds a state with its settings; the first
//! Liquid thing in tree order is the host: when the world is built it lays out the grid over the
//! Area, makes the body of drops on the graphics card and pours in every Liquid thing's Fill.
//! Then, last in each world step, it takes every liquid step due (240 or 120 a second), with
//! every Liquid thing's births, the walls band (again when the walls moved), the balls and the
//! meeting rules. A Liquid thing reads the balls and walls and changes nothing anything else reads,
//! so it is stepped beside the physics.

use std::sync::Arc;

use bpe_core::{ResolvedThing, SlotId, Thing, ThingId};
use bpe_gfx::liquid_sim::{
    BallGpu, Body, Drop, LiquidCard, LiquidGpu, MEET_LAYER, MEET_MIX, MIX_EVERY, MOST_LIQUIDS, MOST_NODES,
    MOST_SUBSTEPS, StepGpu, StepIn,
};
use bpe_runtime::{Log, ThingSim, ThingState, World, state_mut};
use glam::Vec2;

use super::births;
use super::cards::{
    FILL, FillSettings, LOOK, LookSettings, MEETS, Meeting, MeetsSettings, MixStyle, OtherLiquid, PourLayer, RuleStyle,
};
use super::state::{Host, Layout, LiquidState, state_of};
use super::walls;
use super::{AreaFrom, KIND, LiquidSettings, exporting};

pub struct LiquidSim;

/// The card in `slot` when it is of kind `kind`.
fn card<'a>(resolved: &'a ResolvedThing, slot: SlotId, kind: &str) -> Option<&'a bpe_core::ResolvedSlot> {
    resolved.slots.get(&slot).filter(|c| c.kind == kind)
}

/// Reads the thing's settings and cards into `st`.
fn read_into(st: &mut LiquidState, resolved: &ResolvedThing) {
    st.settings = LiquidSettings::read(&resolved.params);
    let fill = card(resolved, SlotId::Motion, FILL);
    st.fill = FillSettings::read(fill);
    st.pours = PourLayer::read_all(fill);
    st.owed.resize(st.pours.len(), 0.0);
    let meets = card(resolved, SlotId::Bounce, MEETS);
    st.meets = MeetsSettings::read(meets);
    st.others = OtherLiquid::read_all(meets);
    st.look = LookSettings::read(card(resolved, SlotId::Look, LOOK));
}

/// The Liquid things of `world`, in tree order.
fn liquids(world: &World) -> Vec<ThingId> {
    world.order().iter().copied().filter(|t| world.kind_of(*t) == Some(KIND)).collect()
}

/// The part of the world the camera shows at the start, as a box.
fn shot(world: &World) -> (Vec2, Vec2) {
    let half = world.camera.shot_bounds_half(world.video);
    (world.camera.center - half, world.camera.center + half)
}

/// The biggest the Area can be each way, in pixels.
const MOST_AREA: f32 = 100_000.0;

/// The grid for the liquids `ids` (the host's Area and Detail).
fn layout(world: &World, host: &LiquidState, ids: &[ThingId]) -> Layout {
    let s = &host.settings;
    let detail = if exporting() { s.export_detail } else { s.detail };
    let shapes = ids
        .iter()
        .filter_map(|t| state_of(world, *t))
        .filter_map(|st| match births::region(st, world) {
            births::Region::Shape(w) => crate::outline::bounds(crate::outline::contours(&w.outline).iter().flatten()),
            births::Region::Free => None,
        })
        .reduce(|(a, b), (c, d)| (a.min(c), b.max(d)));
    let (lo, hi) = shot(world);
    let (lo, hi) = match (s.area, shapes) {
        (AreaFrom::FitTheShapes, Some(box_)) => box_,
        (AreaFrom::Custom, _) => {
            let m = Vec2::splat(s.margin.min(MOST_AREA));
            let (a, b) = (lo - m, hi + m);
            shapes.map_or((a, b), |(c, d)| (a.min(c), b.max(d)))
        }
        (_, _) => shapes.map_or((lo, hi), |(c, d)| (lo.min(c), hi.max(d))),
    };
    let fine = |v: Vec2, or: Vec2| if v.is_finite() { v } else { or };
    let (lo, hi) = (fine(lo, Vec2::splat(-540.0)), fine(hi, Vec2::splat(540.0)));
    let middle = (lo + hi) * 0.5;
    let half = ((hi - lo) * 0.5).clamp(Vec2::splat(16.0), Vec2::splat(MOST_AREA * 0.5));
    let (lo, hi) = (middle - half, middle + half);
    let size = hi - lo;
    // At most MOST_NODES nodes: a bigger area takes bigger cells.
    let mut cell = detail.cell_px();
    let nodes = |cell: f32| ((size.x / cell).ceil() + 5.0) * ((size.y / cell).ceil() + 5.0);
    if nodes(cell) > MOST_NODES as f32 {
        cell = (size.x * size.y / (MOST_NODES as f32 * 0.9)).sqrt().max(cell);
        while nodes(cell) > MOST_NODES as f32 {
            cell *= 1.05;
        }
    }
    let cols = (size.x / cell).ceil() as u32 + 5;
    let rows = (size.y / cell).ceil() as u32 + 5;
    Layout { origin: lo - Vec2::splat(2.0 * cell), cell, cols, rows }
}

/// The liquid steps due by the end of world step `world_step`, at `rate` a second.
fn due(world_step: u64, steps_per_second: u32, rate: u32) -> u32 {
    (world_step * u64::from(rate) / u64::from(steps_per_second.max(1))) as u32
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// How the liquid of `st` meets the liquid of `other` (thing `other_id`): mode, style, when, speed.
fn rule(world: &World, st: &LiquidState, other: &LiquidState) -> (u32, MixStyle, f32) {
    let own = (MEET_MIX, st.look.mix_style, st.look.mix_speed);
    if st.id == other.id {
        return own;
    }
    let found = st.others.iter().find(|r| world.names(&r.liquid, st.id, other.id));
    let Some(r) = found else { return own };
    if r.meeting == Meeting::Layer {
        return (MEET_LAYER, st.look.mix_style, 0.0);
    }
    let style = match r.style {
        RuleStyle::AsThisLiquids => st.look.mix_style,
        RuleStyle::Paint => MixStyle::Paint,
        RuleStyle::Light => MixStyle::Light,
        RuleStyle::Blend => MixStyle::Blend,
        RuleStyle::Marble => MixStyle::Marble,
        RuleStyle::KeepApart => MixStyle::KeepApart,
    };
    (MEET_MIX, style, if r.own_speed { r.speed } else { st.look.mix_speed })
}

/// One liquid step's settings for the liquids `list` (in their numbers' order).
/// How far a pressure wave may travel in a substep, in cells. At 0.4 still water slowly gains
/// energy and tears itself into foam within a second; 0.15 stays still.
const CFL: f32 = 0.15;

fn step_settings(world: &World, host: &Host, list: &[&LiquidState], mixing: bool) -> StepGpu {
    let layout = &host.layout;
    let dt = 1.0 / host.rate as f32;
    let g = world.gravity / layout.cell;
    let g = if g.is_finite() { g } else { Vec2::ZERO };
    let mut step = StepGpu::default();
    // Each liquid's stiffness: its weight's pressure holds a column the grid's height within its
    // squeeze (Springiness), and never softer than a floor.
    let tall = layout.rows as f32;
    let stiff: Vec<f32> = list
        .iter()
        .map(|st| {
            let squeeze = lerp(0.12, 0.015, st.settings.springiness);
            let pull = g.length() * st.settings.gravity.abs();
            (pull * tall / squeeze).max(900.0)
        })
        .collect();
    let most_c = stiff.iter().fold(0.0f32, |m, e| m.max(e.sqrt()));
    let substeps = ((most_c * dt / CFL).ceil() as u32).clamp(1, MOST_SUBSTEPS);
    let dt_sub = dt / substeps as f32;
    let cap = (CFL / dt_sub).powi(2);
    step.grid[2] = substeps;
    step.grid[3] = list.len() as u32;
    step.time = [dt_sub, g.x, g.y, 0.45 / dt_sub];
    step.flags[0] = u32::from(mixing);
    step.more = [0.2 / dt_sub, MIX_EVERY as f32 * dt, 0.05, 0.08];
    let mut free = false;
    for (i, st) in list.iter().enumerate().take(MOST_LIQUIDS) {
        let s = &st.settings;
        free |= matches!(births::region(st, world), births::Region::Free);
        let viscosity = 25_600.0 * s.thickness * s.thickness / (layout.cell * layout.cell);
        let floor = st.born.saturating_sub(st.fill.most);
        step.liquids[i] = LiquidGpu {
            a: [
                stiff[i].min(cap),
                0.25 * s.surface_pull * s.surface_pull,
                lerp(0.3, 0.01, s.spray),
                8.0 * s.settles * s.settles,
            ],
            b: [viscosity, s.weight, s.gravity, if st.meets.balls_push { 1.0 } else { 0.0 }],
            c: [st.look.mix_style.code() as f32, 0.0, 0.0, st.cut.unwrap_or(LiquidGpu::NO_CUT)],
            d: [floor, 0, 0, 0],
        };
    }
    step.flags[3] = u32::from(free);
    for (i, a) in list.iter().enumerate().take(MOST_LIQUIDS) {
        for (j, b) in list.iter().enumerate().take(MOST_LIQUIDS) {
            let (mode, style, speed) = rule(world, a, b);
            // Layer wins: if either says they stay apart, they do.
            let (back, _, _) = rule(world, b, a);
            let mode = if back == MEET_LAYER { MEET_LAYER } else { mode };
            step.set_pair(i, j, mode, style.code(), a.look.mixes_when.code(), speed * speed);
        }
    }
    step
}

/// Every ball, in grid units.
fn balls(world: &World, layout: &Layout) -> Vec<BallGpu> {
    let mut out = Vec::new();
    for b in world.bodies.values() {
        for r in 0..b.len() {
            let (x, v, radius) = (b.position[r], b.velocity[r], b.radius[r]);
            if b.alive[r] && x.is_finite() && v.is_finite() && radius.is_finite() {
                let g = layout.to_grid(x);
                let v = v / layout.cell;
                out.push(BallGpu { x: g.to_array(), v: v.to_array(), r: radius.max(0.0) / layout.cell });
            }
        }
    }
    out
}

/// Takes the Liquid states `ids` out of the world (to work on several at once).
fn take(world: &mut World, ids: &[ThingId]) -> Vec<(ThingId, Box<dyn ThingState>)> {
    ids.iter().filter_map(|t| Some((*t, world.states.remove(t)?))).collect()
}

fn give_back(world: &mut World, taken: Vec<(ThingId, Box<dyn ThingState>)>) {
    for (t, b) in taken {
        world.states.insert(t, b);
    }
}

/// Notes the video memory the host's body takes.
fn note_memory(world: &mut World, id: ThingId) {
    if let Some(bytes) = state_of(world, id).map(|s| s.gpu_memory()).filter(|b| *b > 0) {
        world.gpu_memory.insert(id, bytes);
    }
}

/// One liquid step of the host (taken out of the world, at `taken[0]`) and every other liquid.
fn step_once(world: &World, taken: &mut [(ThingId, Box<dyn ThingState>)]) {
    let (host_part, rest) = taken.split_at_mut(1);
    let host_st = state_mut::<LiquidState>(host_part[0].1.as_mut());
    let Some(mut host) = host_st.host.take() else { return };
    let dt = 1.0 / host.rate as f32;
    let mut births: Vec<Drop> = Vec::new();
    {
        let mut all: Vec<&mut LiquidState> = vec![host_st];
        all.extend(rest.iter_mut().map(|(_, b)| state_mut::<LiquidState>(b.as_mut())));
        for st in all.iter_mut() {
            births.extend(births::level_moved(st, world, &host.layout));
            for k in 0..st.pours.len() {
                births.extend(births::pour(st, k, world, &host.layout, dt, host.step));
            }
        }
    }
    let host_st = state_mut::<LiquidState>(host_part[0].1.as_mut());
    let mut list: Vec<&LiquidState> = vec![&*host_st];
    list.extend(rest.iter().map(|(_, b)| (**b).as_any().downcast_ref::<LiquidState>().expect("a Liquid state")));
    list.retain(|s| s.index.is_some());
    list.sort_by_key(|s| s.index);
    let mixing = host.step % MIX_EVERY == MIX_EVERY - 1;
    let settings = step_settings(world, &host, &list, mixing);
    let any_push = list.iter().any(|s| s.meets.balls_push);
    drop(list);
    if let Some(body) = host.body.as_mut() {
        // A body shared with a copy of the world is copied first: the copy needs its walls again.
        let body = Arc::make_mut(body);
        let now = walls::walls_now(world);
        let changed = now != host.walls_seen || walls::moving(&now);
        let band = (changed || !body.walls_ready()).then(|| walls::band(&now, &host.layout));
        host.walls_seen = now;
        let balls = if any_push { balls(world, &host.layout) } else { Vec::new() };
        body.step(&StepIn { step: settings, births: &births, walls: band.as_deref(), balls: &balls });
    }
    host.step += 1;
    state_mut::<LiquidState>(host_part[0].1.as_mut()).host = Some(host);
}

impl ThingSim for LiquidSim {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let mut st = LiquidState::new(thing.id, thing.parent);
        read_into(&mut st, resolved);
        world.states.insert(thing.id, Box::new(st));
    }

    /// Reads the walls of the shape it fills.
    fn needs_walls(&self) -> bool {
        true
    }

    /// The first Liquid thing makes the body for all of them and pours in their Fills.
    fn after_build(&self, id: ThingId, world: &mut World) {
        let ids = liquids(world);
        if ids.first() != Some(&id) {
            return;
        }
        let Some(host_st) = state_of(world, id) else { return };
        let layout = layout(world, host_st, &ids);
        let rate = host_st.settings.steps.per_second();
        let body = LiquidCard::on_this_thread().map(|gpu| Arc::new(Body::new(gpu, layout.cols, layout.rows)));
        let mut taken = take(world, &ids);
        let mut births = Vec::new();
        for (k, (_, b)) in taken.iter_mut().enumerate() {
            let st = state_mut::<LiquidState>(b.as_mut());
            st.index = (k < MOST_LIQUIDS).then_some(k);
            st.born = 0;
            st.filled = 0.0;
            st.cut = None;
        }
        // The fills, with the other liquids out of the world: their regions read only the walls.
        for (_, b) in taken.iter_mut() {
            births.extend(births::first_fill(state_mut::<LiquidState>(b.as_mut()), world, &layout));
        }
        let mut host = Host { body, layout, rate, step: 0, walls_seen: walls::walls_now(world) };
        if let Some(body) = host.body.as_mut() {
            // Born still: a step with no time in it puts them on the card with the walls band.
            let mut settings = StepGpu::default();
            settings.grid[2] = 1;
            settings.grid[3] = taken.len().min(MOST_LIQUIDS) as u32;
            let band = walls::band(&host.walls_seen, &layout);
            Arc::make_mut(body).step(&StepIn { step: settings, births: &births, walls: Some(&band), balls: &[] });
        }
        state_mut::<LiquidState>(taken[0].1.as_mut()).host = Some(host);
        give_back(world, taken);
        note_memory(world, id);
    }

    /// The host takes every liquid step due, last in the world step.
    fn after_step(&self, id: ThingId, world: &mut World, _log: &mut Log) {
        let Some(rate) = state_of(world, id).and_then(|s| s.host.as_ref()).map(|h| h.rate) else { return };
        let due = due(world.step, world.steps_per_second, rate);
        let ids = liquids(world);
        // The host first.
        let mut ids: Vec<ThingId> = ids.into_iter().filter(|t| *t != id).collect();
        ids.insert(0, id);
        let mut taken = take(world, &ids);
        loop {
            let at = state_mut::<LiquidState>(taken[0].1.as_mut()).host.as_ref().map_or(due, |h| h.step);
            if at >= due {
                break;
            }
            step_once(world, &mut taken);
        }
        give_back(world, taken);
        note_memory(world, id);
    }

    fn settings_changed(&self, id: ThingId, resolved: &ResolvedThing, world: &mut World) {
        if let Some(st) = world.states.get_mut(&id).and_then(|s| (**s).as_any_mut().downcast_mut::<LiquidState>()) {
            read_into(st, resolved);
        }
    }

    /// Decoration: it reads the balls and walls and nothing reads it.
    fn in_main(&self, _resolved: &ResolvedThing) -> bool {
        false
    }

    /// Every setting moves the liquid; it is stepped beside the physics, so this only lets keys
    /// reach `settings_changed`.
    fn key_restarts_physics(&self, _key: &str) -> bool {
        true
    }

    /// The Look's mixing is read by the stepping.
    fn card_key_restarts_beside(&self, slot: SlotId, key: &str) -> bool {
        slot == SlotId::Look && matches!(key, "mix_style" | "mix_speed" | "mixes_when")
    }

    fn beside_print_extra(&self, resolved: &ResolvedThing) -> u64 {
        let (style, speed, when) = LookSettings::read(card(resolved, SlotId::Look, LOOK)).stepped();
        (style.code() as u64) | (u64::from(speed) << 8) | ((when.code() as u64) << 40) | (1 << 48)
    }

    /// Its Fill and Meets cards have no module sims: it reads them itself.
    fn steps_own_cards(&self) -> bool {
        true
    }

    fn pulses_on_hits(&self) -> bool {
        true
    }
}
