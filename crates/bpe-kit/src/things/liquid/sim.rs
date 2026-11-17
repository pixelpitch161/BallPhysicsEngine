//! How Liquid things take part in the world. Each builds a state with its settings; the first
//! Liquid thing in tree order is the host: when the world is built it lays out the grid over the
//! Area, makes the body of drops on the graphics card and pours in every Liquid thing's Fill.
//! Then, last in each world step, it takes every liquid step due (240 or 120 a second), with
//! every Liquid thing's births, the walls band (again when the walls moved), the balls and the
//! meeting rules. A liquid that pushes balls back steps with the physics (and keeps every other
//! Liquid thing there with it): its card records what each ball felt, and the next world step's
//! first substep gives it to them (`feel.rs`). Otherwise the liquids are stepped beside it.

use std::sync::Arc;

use bpe_core::{ResolvedThing, Scene, SlotId, Thing, ThingId};
use bpe_gfx::liquid_sim::{
    BallGpu, Body, Drop, LiquidCard, LiquidGpu, MEET_LAYER, MEET_MIX, MIX_EVERY, MOST_LIQUIDS, MOST_NODES,
    MOST_SUBSTEPS, StepGpu, StepIn,
};
use bpe_runtime::{Log, ThingSim, ThingState, World, state_mut};

use super::feel::{self, Felt};
use glam::Vec2;

use super::births;
use super::cards::{
    FILL, FillSettings, LOOK, LookSettings, MEETS, Meeting, MeetsSettings, MixStyle, OtherLiquid, PourLayer, RuleStyle,
};
use super::state::{Host, Layout, LiquidState, MOST_BORN_A_STEP, share_most, state_of};
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
    st.sound = super::sound::LiquidSound::read(resolved.slots.get(&SlotId::Sound));
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

/// The grid for the liquids `ids` (the host's Area and Detail). A liquid that steps with the
/// physics exports at the preview's Detail, so the balls go as they were tuned, unless the host
/// asks for a Finer export.
fn layout(world: &World, host: &LiquidState, ids: &[ThingId], main: bool) -> Layout {
    let s = &host.settings;
    let detail = if exporting() && (!main || s.finer_export) { s.export_detail } else { s.detail };
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
/// The heaviest a liquid steps as (Weight goes higher, the card's fixed-point sums don't).
const MOST_WEIGHT: f32 = 20.0;

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
        // Clamped here, before mixing sums it in fixed point: a thick liquid meeting a thin one would
        // overflow the sums and flip its thickness.
        let viscosity = (25_600.0 * s.thickness * s.thickness / (layout.cell * layout.cell)).min(step.more[0]);
        // The card's mass and momentum sums are fixed point: past this they clamp.
        let weight = s.weight.clamp(0.05, MOST_WEIGHT);
        // The pull is a stiffness too: never past what the substeps keep stable.
        let pull = (0.25 * s.surface_pull * s.surface_pull).min(cap / stiff[i].min(cap).max(1e-6));
        let floor = st.born.saturating_sub(st.kept);
        let m = &st.meets;
        let two_way = m.two_way();
        // At most ten times the push water gives: the card sums each ball's push in fixed point.
        let push = if two_way { (1.0 / m.ball_density(weight)).min(10.0) } else { 0.0 };
        let only = |x: f32| if two_way { x } else { 0.0 };
        let ink = bpe_core::Color::to_linear(m.ink_colour);
        step.liquids[i] = LiquidGpu {
            a: [stiff[i].min(cap), pull, lerp(0.3, 0.01, s.spray), 8.0 * s.settles * s.settles],
            b: [viscosity, weight, s.gravity, if m.balls_push { 1.0 } else { 0.0 }],
            c: [st.look.mix_style.code() as f32, 0.0, 0.0, st.cut.unwrap_or(LiquidGpu::NO_CUT)],
            d: [floor, 0, 0, 0],
            e: [push, only(DRAG_RATE * m.drag * (1.0 + 8.0 * s.thickness.min(10.0))), only(m.carries), m.sticks],
            f: [ink[0], ink[1], ink[2], m.ink],
            g: [m.splash, m.splash_from / layout.cell, m.wakes, only(m.take)],
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

/// How fast a ball fully under a liquid of Drag 1 and Thickness 0 loses its speed through it, a
/// second.
const DRAG_RATE: f32 = 3.0;

/// Every ball some liquid of `list` meets (its Balls push it on and its Meets balls naming the
/// ball's thing), in grid units with the mask of those liquids, and where each came from. Balls
/// wholly outside the grid are left out.
fn balls(world: &World, layout: &Layout, list: &[&LiquidState]) -> (Vec<BallGpu>, Vec<(ThingId, u32)>) {
    let mut out = Vec::new();
    let mut from = Vec::new();
    let size = Vec2::new(layout.cols as f32, layout.rows as f32);
    for (&thing, b) in &world.bodies {
        let mut mask = 0u32;
        for st in list {
            if let Some(k) = st.index.filter(|_| st.meets.balls_push && world.names(&st.meets.target, st.id, thing)) {
                mask |= 1 << k;
            }
        }
        if mask == 0 {
            continue;
        }
        for r in b.live_rows() {
            let (x, v, radius) = (b.position[r], b.velocity[r], b.radius[r]);
            if !(b.alive[r] && x.is_finite() && v.is_finite() && radius.is_finite()) {
                continue;
            }
            let g = layout.to_grid(x);
            let rg = radius.max(0.0) / layout.cell;
            if g.x + rg < 0.0 || g.y + rg < 0.0 || g.x - rg > size.x || g.y - rg > size.y {
                continue;
            }
            let v = v / layout.cell;
            out.push(BallGpu { x: g.to_array(), v: v.to_array(), r: rg, mask });
            from.push((thing, r as u32));
        }
    }
    (out, from)
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

/// The Liquid states of `taken`, with a liquid number, in their numbers' order.
fn numbered(taken: &[(ThingId, Box<dyn ThingState>)]) -> Vec<&LiquidState> {
    let mut list: Vec<&LiquidState> =
        taken.iter().map(|(_, b)| (**b).as_any().downcast_ref::<LiquidState>().expect("a Liquid state")).collect();
    list.retain(|s| s.index.is_some());
    list.sort_by_key(|s| s.index);
    list
}

/// What one liquid step tells the card about the balls.
struct BallsIn<'a> {
    balls: &'a [BallGpu],
    /// Record what they feel, and start afresh.
    feel: bool,
    fresh: bool,
}

/// One liquid step of the host (taken out of the world, at `taken[0]`) and every other liquid.
fn step_once(world: &World, taken: &mut [(ThingId, Box<dyn ThingState>)], balls: &BallsIn) {
    let (host_part, rest) = taken.split_at_mut(1);
    let host_st = state_mut::<LiquidState>(host_part[0].1.as_mut());
    let Some(mut host) = host_st.host.take() else { return };
    let dt = 1.0 / host.rate as f32;
    let mut births: Vec<Drop> = Vec::new();
    // Its sound hears a pour pouring about POUR_RATE times a second (part 5).
    let pour_every = (host.rate / super::sound::POUR_RATE).max(1);
    let hear_pours = host.step % pour_every == 0;
    {
        let mut all: Vec<&mut LiquidState> = vec![host_st];
        all.extend(rest.iter_mut().map(|(_, b)| state_mut::<LiquidState>(b.as_mut())));
        share_most(all.iter_mut().map(|s| &mut **s));
        for st in all.iter_mut() {
            births.extend(births::level_moved(st, world, &host.layout));
            for k in 0..st.pours.len() {
                let born = births::pour(st, k, world, &host.layout, dt, host.step);
                if hear_pours && st.sound.pours && !born.is_empty() {
                    let mid = born.iter().map(|d| Vec2::from(d.x)).sum::<Vec2>() / born.len() as f32;
                    let v = born.iter().map(|d| Vec2::from(d.v)).sum::<Vec2>() / born.len() as f32;
                    let (place, v) = (host.layout.to_world(mid), v * host.layout.cell);
                    host.news.push(super::sound::pour_event(world.step, st.id, k, place, v));
                }
                births.extend(born);
            }
        }
    }
    births.truncate(MOST_BORN_A_STEP);
    let list = numbered(taken);
    let mixing = host.step % MIX_EVERY == MIX_EVERY - 1;
    let settings = step_settings(world, &host, &list, mixing);
    drop(list);
    if let Some(body) = host.body.as_mut() {
        // A body shared with a copy of the world is copied first: the copy needs its walls again.
        let body = Arc::make_mut(body);
        let now = walls::walls_now(world);
        let changed = now != host.walls_seen || walls::moving(&now);
        if changed || host.band.is_none() {
            host.band = Some(Arc::new(walls::band(&now, &host.layout)));
        }
        let band = (changed || !body.walls_ready()).then(|| host.band.clone()).flatten();
        host.walls_seen = now;
        body.step(&StepIn {
            step: settings,
            births: &births,
            walls: band.as_deref().map(|b| b.as_slice()),
            balls: balls.balls,
            feel: balls.feel,
            fresh: balls.fresh,
        });
    }
    host.step += 1;
    state_mut::<LiquidState>(taken[0].1.as_mut()).host = Some(host);
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
        let layout = layout(world, host_st, &ids, !world.beside.contains(&id));
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
        share_most(taken.iter_mut().map(|(_, b)| state_mut::<LiquidState>(b.as_mut())));
        // The fills, with the other liquids out of the world: their regions read only the walls.
        for (_, b) in taken.iter_mut() {
            births.extend(births::first_fill(state_mut::<LiquidState>(b.as_mut()), world, &layout));
        }
        let mut host = Host {
            body,
            layout,
            rate,
            step: 0,
            walls_seen: walls::walls_now(world),
            band: None,
            pending: None,
            inside: Default::default(),
            touching: Default::default(),
            numbers: Vec::new(),
            news: Vec::new(),
            taken: Default::default(),
            wet: Default::default(),
            sums: None,
        };
        if let Some(body) = host.body.as_mut() {
            // Born still: a step with no time in it puts them on the card with the walls band.
            let mut settings = StepGpu::default();
            settings.grid[2] = 1;
            settings.grid[3] = taken.len().min(MOST_LIQUIDS) as u32;
            let band = Arc::new(walls::band(&host.walls_seen, &layout));
            let still =
                StepIn { step: settings, births: &births, walls: Some(&band), balls: &[], feel: false, fresh: false };
            Arc::make_mut(body).step(&still);
            host.band = Some(band);
        }
        state_mut::<LiquidState>(taken[0].1.as_mut()).host = Some(host);
        give_back(world, taken);
        note_memory(world, id);
    }

    /// The host logs what the balls did in the liquid, then takes every liquid step due, last in
    /// the world step; in the main world it then sends back what the balls felt.
    fn after_step(&self, id: ThingId, world: &mut World, log: &mut Log) {
        let Some(rate) = state_of(world, id).and_then(|s| s.host.as_ref()).map(|h| h.rate) else { return };
        if let Some(host) = world
            .states
            .get_mut(&id)
            .and_then(|s| (**s).as_any_mut().downcast_mut::<LiquidState>())
            .and_then(|s| s.host.as_mut())
        {
            for e in std::mem::take(&mut host.news) {
                log.push(e);
            }
        }
        let due = due(world.step, world.steps_per_second, rate);
        let main = !world.beside.contains(&id);
        let ids = liquids(world);
        // The host first.
        let mut ids: Vec<ThingId> = ids.into_iter().filter(|t| *t != id).collect();
        ids.insert(0, id);
        let mut taken = take(world, &ids);
        let Some(layout) = state_mut::<LiquidState>(taken[0].1.as_mut()).host.as_ref().map(|h| h.layout) else {
            give_back(world, taken);
            return;
        };
        // The balls as this world step left them, the same for every liquid step it takes.
        let (balls, from) = balls(world, &layout, &numbered(&taken));
        let mut stepped = 0u32;
        loop {
            let at = state_mut::<LiquidState>(taken[0].1.as_mut()).host.as_ref().map_or(due, |h| h.step);
            if at >= due {
                break;
            }
            // A later liquid step of the same world step sees the balls moved on along their speed.
            let ahead = stepped as f32 / rate as f32;
            let moved: Vec<BallGpu>;
            let now = if stepped == 0 {
                &balls[..]
            } else {
                moved = balls
                    .iter()
                    .map(|b| BallGpu { x: [b.x[0] + b.v[0] * ahead, b.x[1] + b.v[1] * ahead], ..*b })
                    .collect();
                &moved[..]
            };
            let with = BallsIn { balls: now, feel: main, fresh: stepped == 0 };
            step_once(world, &mut taken, &with);
            stepped += 1;
        }
        if main && stepped > 0 && balls.is_empty() {
            // No ball is met any more: the ones that were in a liquid have left it.
            let host = state_mut::<LiquidState>(taken[0].1.as_mut()).host.as_mut().expect("the host");
            host.pending = None;
            feel::all_left(host, world);
        }
        // The feel goes back after every batch, with no balls too: the liquids' sums come with it
        // (part 5).
        if main && stepped > 0 {
            let liquids: Vec<(usize, ThingId, f32)> =
                numbered(&taken).iter().filter_map(|s| Some((s.index?, s.id, s.meets.splash_from))).collect();
            let bubbles = numbered(&taken)
                .iter()
                .filter(|s| s.sound.bubbles)
                .filter_map(|s| s.index)
                .fold(0u16, |m, l| m | (1 << l));
            let host = state_mut::<LiquidState>(taken[0].1.as_mut()).host.as_mut().expect("the host");
            if let Some(body) = host.body.as_mut() {
                let felt =
                    Felt { balls: from, liquids, cell: layout.cell, span: stepped as f32 / rate as f32, bubbles };
                host.pending = Arc::make_mut(body).send_feel(balls.len() as u32).map(|f| (f, Arc::new(felt)));
            }
        }
        give_back(world, taken);
        note_memory(world, id);
    }

    /// In the main world: the balls get what they felt in the last liquid steps, once, at the
    /// first substep.
    fn forces(&self, id: ThingId, world: &mut World, _dt: f32) {
        if !state_of(world, id).and_then(|s| s.host.as_ref()).is_some_and(|h| h.pending.is_some()) {
            return;
        }
        let Some(mut boxed) = world.states.remove(&id) else { return };
        if let Some(host) = state_mut::<LiquidState>(boxed.as_mut()).host.as_mut() {
            feel::apply(host, world);
        }
        world.states.insert(id, boxed);
    }

    fn settings_changed(&self, id: ThingId, resolved: &ResolvedThing, world: &mut World) {
        if let Some(st) = world.states.get_mut(&id).and_then(|s| (**s).as_any_mut().downcast_mut::<LiquidState>()) {
            read_into(st, resolved);
        }
    }

    /// It steps with the physics when it pushes the balls back; otherwise it is decoration (it
    /// reads the balls and walls and nothing reads it).
    fn in_main(&self, resolved: &ResolvedThing) -> bool {
        MeetsSettings::read(card(resolved, SlotId::Bounce, MEETS)).two_way()
    }

    /// A liquid that pushes balls keeps every other Liquid thing in the main world with it: they
    /// share one body of liquid.
    fn keeps(&self, scene: &Scene, me: &Thing, other: ThingId) -> bool {
        let pushes = me
            .slots
            .get(&SlotId::Bounce)
            .filter(|c| c.kind == MEETS)
            .map_or_else(MeetsSettings::default, |c| MeetsSettings::of_saved(&c.params))
            .two_way();
        pushes && me.enabled && scene.things.get(&other).is_some_and(|t| t.kind == KIND)
    }

    /// A scene whose sound hears a liquid keeps every Liquid thing in the main world, so their
    /// events and readings are in the world the sound reads (part 5).
    fn kept_by_scene(&self, scene: &Scene, _id: ThingId) -> bool {
        super::sound::scene_hears_liquid(scene)
    }

    /// Every setting moves the liquid (beside the physics, this only lets keys reach
    /// `settings_changed`).
    fn key_restarts_physics(&self, _key: &str) -> bool {
        true
    }

    /// In the main world its Look's mixing moves the drops, and the first Liquid thing sets the
    /// area: both count in the physics fingerprint.
    fn physics_print_extra(&self, scene: &Scene, thing: &Thing, resolved: &ResolvedThing) -> u64 {
        let place = scene
            .order
            .iter()
            .filter(|t| scene.things.get(t).is_some_and(|t| t.kind == KIND))
            .position(|t| *t == thing.id);
        // The moments its sound logs change the log (part 5).
        let sound = super::sound::LiquidSound::read(resolved.slots.get(&SlotId::Sound)).code();
        self.beside_print_extra(resolved) ^ (place.map_or(u64::MAX, |p| p as u64) << 52) ^ (sound << 44)
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
