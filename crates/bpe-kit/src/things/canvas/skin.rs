//! Paint skins: the paint an arena or obstacle carries on itself. A skin is part 1's canvas
//! state held by the thing (in [`World::skins`]), in the thing's own frame: its marks are logged
//! relative to the thing's pose each step, so they move and turn with it.
//!
//! Splats land on a skin when their brush says Lands on "Thing hit" or "Both": the mover's brush
//! paints the thing it hit, and a wall's or obstacle's own brush paints its own skin wherever
//! something hits it. Like the canvases, none of this changes the physics or enters the log's
//! hash.

use std::collections::BTreeMap;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

use bpe_core::{GroupOpts, Params, SettingDef, SettingGroup, Settings, SlotId, ThingId};
use bpe_runtime::{Event, EventKind, Log, Mark, MarkShape, MarkTarget, Outline, Pose, Solid, World};
use glam::Vec2;

use super::evolve::{self, Evolve, Tick};
use super::grid::{CanvasGrid, painter, tick_every};
use super::ribbon::{CONTINUED, CONTINUES};
use super::sim::{CanvasState, Colours, Stamp, hit_stamp, reaches, to_local};
use super::{CanvasSettings, CanvasStyle, Fit};
use crate::modules::marks::{MarksBrushes, SplatSkin, Turn};
use crate::things::arena::ArenaState;
use crate::things::obstacle::{ObstacleShape, ObstacleState};

/// How an arena or obstacle takes paint: its Paint skin group.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct SkinSettings {
    /// Whether splats that land on the thing hit paint this one.
    #[setting(label = "Takes paint", group = "Paint skin")]
    pub skin_on: bool,
    /// How far paint reaches past a thin line, in multiples of the line's half width. Solid
    /// shapes ignore it.
    #[setting(label = "Skin width", when = "skin_on", range(0, 3), group = "Paint skin")]
    pub skin_width: f32,
    /// How long the paint takes to fade away; 0 never fades.
    #[setting(label = "Fade", when = "skin_on", range(0, 60), unit = "s", group = "Paint skin")]
    pub skin_fade: f32,
    /// How much of the paint shows.
    #[setting(label = "Opacity", when = "skin_on", range(0, 1), group = "Paint skin", keyframe)]
    pub skin_opacity: f32,
    /// How the paint looks.
    #[setting(label = "Style", when = "skin_on", group = "Paint skin")]
    pub skin_style: CanvasStyle,
    /// Where the light comes from, in degrees clockwise from the top.
    #[setting(
        label = "Light angle",
        when = "skin_on and skin_style=wet_paint",
        range(0, 360),
        unit = "°",
        group = "Paint skin"
    )]
    pub skin_light_angle: f32,
    /// How shiny wet paint is.
    #[setting(label = "Gloss", when = "skin_on and skin_style=wet_paint", range(0, 1), group = "Paint skin", keyframe)]
    pub skin_gloss: f32,
    /// How grainy the washes are.
    #[setting(label = "Grain", when = "skin_on and skin_style=watercolour", range(0, 1), group = "Paint skin")]
    pub skin_grain: f32,
    /// How dark the edges of a wash are.
    #[setting(
        label = "Edge darkening",
        when = "skin_on and skin_style=watercolour",
        range(0, 1),
        group = "Paint skin"
    )]
    pub skin_edge_dark: f32,
    /// How far the glow reaches.
    #[setting(
        label = "Glow size",
        when = "skin_on and skin_style=neon_ink",
        range(2, 80),
        unit = "px",
        group = "Paint skin"
    )]
    pub skin_glow_size: f32,
    /// How bright the glow is.
    #[setting(
        label = "Glow strength",
        when = "skin_on and skin_style=neon_ink",
        range(0, 3),
        group = "Paint skin",
        keyframe
    )]
    pub skin_glow_strength: f32,
    /// How long paint stays wet; wet paint can bleed and drip.
    #[setting(label = "Dry time", when = "skin_on", range(0, 30), unit = "s", group = "Paint skin")]
    pub skin_dry_time: f32,
    /// How much wet colours run into each other.
    #[setting(label = "Bleed", when = "skin_on", range(0, 1), group = "Paint skin")]
    pub skin_bleed: f32,
    /// How much thick wet paint runs down the surface.
    #[setting(label = "Drips", when = "skin_on", range(0, 1), group = "Paint skin")]
    pub skin_drips: f32,
}

impl Default for SkinSettings {
    fn default() -> Self {
        Self {
            skin_on: true,
            skin_width: 2.0,
            skin_fade: 0.0,
            skin_opacity: 1.0,
            skin_style: CanvasStyle::Flat,
            skin_light_angle: 315.0,
            skin_gloss: 0.6,
            skin_grain: 0.5,
            skin_edge_dark: 0.5,
            skin_glow_size: 18.0,
            skin_glow_strength: 1.0,
            skin_dry_time: 1.0,
            skin_bleed: 0.0,
            skin_drips: 0.0,
        }
    }
}

/// Embedded whole in an arena's and an obstacle's settings, keys and group as they are.
impl SettingGroup for SkinSettings {
    fn group_defs(o: &GroupOpts, _default: &Self) -> Vec<SettingDef> {
        debug_assert!(o.prefix.is_empty() && o.when.is_empty(), "the skin group is embedded as it is");
        Self::defs().to_vec()
    }
    fn group_read(_o: &GroupOpts, _default: &Self, p: &Params) -> Self {
        Self::from_params(p)
    }
    fn group_write(&self, _o: &GroupOpts, _default: &Self, p: &mut Params) {
        for (key, value) in self.to_params().iter() {
            p.set(key, value.clone());
        }
    }
}

/// Segments a circle or ring is walked and drawn in.
const ROUND: usize = 64;
/// Segments of each rounded box corner.
const CORNER: usize = 16;
/// How far a print's centre is pushed into the surface, in splat radii.
const PRINT_DEPTH: f32 = 0.3;

/// A thing's Paint skin settings, from its own state: an arena or an obstacle.
pub fn skin_settings(world: &World, thing: ThingId) -> Option<&SkinSettings> {
    let st = world.states.get(&thing)?.as_any();
    st.downcast_ref::<ArenaState>()
        .map(|a| &a.settings.skin)
        .or_else(|| st.downcast_ref::<ObstacleState>().map(|o| &o.settings.skin))
}

/// A thing's skin, when it has one.
pub fn skin_state(world: &World, thing: ThingId) -> Option<&CanvasState> {
    world.skins.get(&thing)?.as_any().downcast_ref::<CanvasState>()
}

/// Where a thing's skin is and how big it is: the thing's pose (its skin's middle sits on it)
/// and the skin's width and height, like [`super::canvas_area`].
pub fn skin_area(world: &World, thing: ThingId) -> Option<(Pose, Vec2)> {
    let st = skin_state(world, thing)?;
    Some((world.poses.get(&thing).copied().unwrap_or_default(), st.size))
}

/// The thing's pose now.
fn pose_of(world: &World, thing: ThingId) -> Pose {
    world.poses.get(&thing).copied().unwrap_or_default()
}

/// An arena's wall as it is this step, in the arena's own frame.
pub fn wall_local(world: &World, thing: ThingId) -> Option<Outline> {
    let outline = world.walls.get(&thing)?.outline;
    let pose = pose_of(world, thing);
    let c = to_local(pose, outline.center());
    let a = outline.angle() - pose.angle;
    Some(match outline {
        Outline::Ring { radius, sides, thickness, .. } => {
            Outline::Ring { center: c, radius, sides, angle: a, thickness }
        }
        Outline::Box { half, thickness, .. } => Outline::Box { center: c, half, angle: a, thickness },
        Outline::Rounded { half, corner, thickness, .. } => {
            Outline::Rounded { center: c, half, corner, angle: a, thickness }
        }
    })
}

/// An obstacle's pieces as they are this step, in the obstacle's own frame.
pub fn parts_local(world: &World, thing: ThingId) -> Vec<Solid> {
    let Some(solids) = world.solids.get(&thing) else { return Vec::new() };
    let pose = pose_of(world, thing);
    let at = |p: Vec2| to_local(pose, p);
    solids
        .parts
        .iter()
        .map(|part| match *part {
            Solid::Disc { center, radius } => Solid::Disc { center: at(center), radius },
            Solid::Capsule { from, to, radius } => Solid::Capsule { from: at(from), to: at(to), radius },
            Solid::Ring { center, radius, thickness } => Solid::Ring { center: at(center), radius, thickness },
        })
        .collect()
}

/// Whether paint covers a piece of `thing` all over (a peg, a grid's peg, a bar) rather than
/// following it as a thin line (a ring, a polygon's side).
pub fn is_solid(world: &World, thing: ThingId, part: &Solid) -> bool {
    match part {
        Solid::Disc { .. } => true,
        Solid::Ring { .. } => false,
        Solid::Capsule { .. } => !world
            .states
            .get(&thing)
            .and_then(|st| st.as_any().downcast_ref::<ObstacleState>())
            .is_some_and(|o| o.settings.shape == ObstacleShape::Polygon),
    }
}

/// How far paint reaches either side of a thin line of `thickness`.
pub fn line_reach(thickness: f32, skin: &SkinSettings) -> f32 {
    thickness.max(0.0) * 0.5 * (1.0 + skin.skin_width.clamp(0.0, 3.0))
}

/// The direction `angle` radians clockwise from +x.
fn dir(angle: f32) -> Vec2 {
    Vec2::new(angle.cos(), angle.sin())
}

/// A closed circle of `n` segments, starting `start` radians round.
fn circle(center: Vec2, radius: f32, start: f32, n: usize) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = (0..n).map(|i| center + dir(start + TAU * i as f32 / n as f32) * radius).collect();
    out.push(out[0]);
    out
}

/// Closes a loop of points: the first point again at the end, with repeats left out.
fn closed(points: impl IntoIterator<Item = Vec2>) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::new();
    for p in points {
        if out.last().is_none_or(|q| (*q - p).length_squared() > 1e-8) {
            out.push(p);
        }
    }
    if out.len() > 1 && (out[0] - out[out.len() - 1]).length_squared() <= 1e-8 {
        out.pop();
    }
    if let Some(&first) = out.first() {
        out.push(first);
    }
    out
}

/// The centre line of a wall, as one closed polyline.
fn outline_line(outline: &Outline) -> Vec<Vec2> {
    match *outline {
        Outline::Ring { center, radius, sides, angle, .. } if sides < 3 => circle(center, radius, angle, ROUND),
        Outline::Ring { center, radius, sides, angle, .. } => {
            // A polygon's `angle` faces a side; its corners lie half a side round from there.
            closed((0..sides).map(|i| center + dir(angle + TAU * (i as f32 + 0.5) / sides as f32) * radius))
        }
        Outline::Box { center, half, angle, .. } => {
            let turn = dir(angle);
            let corners = [Vec2::new(-half.x, -half.y), Vec2::new(half.x, -half.y), half, Vec2::new(-half.x, half.y)];
            closed(corners.map(|p| center + turn.rotate(p)))
        }
        Outline::Rounded { center, half, corner, angle, .. } => {
            let turn = dir(angle);
            let inner = (half - Vec2::splat(corner)).max(Vec2::ZERO);
            let corners = [inner, Vec2::new(-inner.x, inner.y), -inner, Vec2::new(inner.x, -inner.y)];
            let mut points = Vec::with_capacity(4 * (CORNER + 1));
            for (k, c) in corners.into_iter().enumerate() {
                for i in 0..=CORNER {
                    let a = FRAC_PI_2 * (k as f32 + i as f32 / CORNER as f32);
                    points.push(center + turn.rotate(c + dir(a) * corner));
                }
            }
            closed(points)
        }
    }
}

/// The edge of a bar: both straight sides and both round ends, as one closed polyline.
fn capsule_edge(from: Vec2, to: Vec2, radius: f32) -> Vec<Vec2> {
    let along = to - from;
    if along.length_squared() < 1e-8 {
        return circle(from, radius, 0.0, ROUND);
    }
    let a = along.y.atan2(along.x);
    let half = ROUND / 2;
    let end = (0..=half).map(|i| to + dir(a - FRAC_PI_2 + PI * i as f32 / half as f32) * radius);
    let start = (0..=half).map(|i| from + dir(a + FRAC_PI_2 + PI * i as f32 / half as f32) * radius);
    closed(end.chain(start))
}

/// The lines along which `thing` takes paint, in its own frame, as polylines: a closed one
/// repeats its first point at the end. A wall's centre line; an obstacle's pieces: the edge of
/// a peg or bar, the centre line of a ring, and the sides of a polygon joined into one loop.
/// Circles and rings have 64 segments.
pub fn outline_local(world: &World, thing: ThingId) -> Vec<Vec<Vec2>> {
    if let Some(outline) = wall_local(world, thing) {
        return vec![outline_line(&outline)];
    }
    let mut out = Vec::new();
    // Thin sides that follow on from each other, gathered into one line.
    let mut chain: Vec<Vec2> = Vec::new();
    let flush = |chain: &mut Vec<Vec2>, out: &mut Vec<Vec<Vec2>>| {
        if chain.len() >= 2 {
            let looped = (chain[0] - chain[chain.len() - 1]).length() < 1e-3;
            out.push(if looped { closed(chain.drain(..)) } else { std::mem::take(chain) });
        }
        chain.clear();
    };
    for part in parts_local(world, thing) {
        match part {
            Solid::Capsule { from, to, .. } if !is_solid(world, thing, &part) => {
                if chain.last().is_some_and(|p| (*p - from).length() >= 1e-3) {
                    flush(&mut chain, &mut out);
                }
                if chain.is_empty() {
                    chain.push(from);
                }
                chain.push(to);
            }
            _ => {
                flush(&mut chain, &mut out);
                out.push(match part {
                    Solid::Disc { center, radius } => circle(center, radius, 0.0, ROUND),
                    Solid::Ring { center, radius, .. } => circle(center, radius, 0.0, ROUND),
                    Solid::Capsule { from, to, radius } => capsule_edge(from, to, radius),
                });
            }
        }
    }
    flush(&mut chain, &mut out);
    out
}

/// How far paint on a thin line of `thing` reaches from the line, or `None` when the thing (or
/// its piece `part`, for an obstacle) is solid.
pub(crate) fn thin_reach(world: &World, thing: ThingId, part: u32, skin: &SkinSettings) -> Option<f32> {
    if let Some(wall) = world.walls.get(&thing) {
        return Some(line_reach(wall.outline.thickness(), skin));
    }
    let piece = world.solids.get(&thing)?.parts.get(part as usize)?;
    if is_solid(world, thing, piece) {
        return None;
    }
    Some(match *piece {
        Solid::Ring { thickness, .. } => line_reach(thickness, skin),
        Solid::Capsule { radius, .. } | Solid::Disc { radius, .. } => line_reach(radius * 2.0, skin),
    })
}

/// A brush whose splats land on the things they hit.
fn splats_things(b: &MarksBrushes) -> bool {
    b.splat_on && b.splat_onto.things()
}

/// The brushes in the world, in the world's order, with their owners.
pub(crate) fn brushes(world: &World) -> impl Iterator<Item = (ThingId, &MarksBrushes)> {
    world.order().iter().filter_map(|&thing| {
        let (_, brush) = world.module_state(thing, SlotId::Marks)?;
        Some((thing, brush.as_any().downcast_ref::<MarksBrushes>()?))
    })
}

/// Gives `thing` its skin when it takes paint and some brush splats onto the things it hits.
/// The skin covers the thing's outline, centred on the thing, with room for the biggest splat
/// and the widest line paint around it. Arenas and obstacles call it once everything is built.
pub fn build(thing: ThingId, world: &mut World) {
    // A scene with only skins still gets its picture table.
    crate::pictures::fill(world);
    let Some(skin) = skin_settings(world, thing).filter(|s| s.skin_on).cloned() else { return };
    let splat = brushes(world)
        .filter(|(_, b)| splats_things(b))
        .map(|(_, b)| b.splat_size.max(0.0) * (1.0 + b.splat_by_speed.max(0.0) * 2.0))
        .reduce(f32::max);
    let Some(splat) = splat else { return };
    let lines = outline_local(world, thing);
    let reach = lines.iter().flatten().fold(Vec2::ZERO, |m, p| m.max(p.abs()));
    if lines.is_empty() || !reach.is_finite() {
        return;
    }
    let line = match (world.walls.get(&thing), world.solids.get(&thing)) {
        (Some(wall), _) => line_reach(wall.outline.thickness(), &skin),
        (None, Some(solids)) => {
            (0..solids.parts.len() as u32).filter_map(|i| thin_reach(world, thing, i, &skin)).fold(0.0, f32::max)
        }
        (None, None) => 0.0,
    };
    let size = ((reach + Vec2::splat(splat.max(line))) * 2.0).max(Vec2::ONE);
    let grid = CanvasGrid::new(size);
    world.state_memory.insert(thing, grid.bytes());
    let settings = CanvasSettings { fit: Fit::Custom, size, ..CanvasSettings::default() };
    let every = (world.steps_per_second / 60).max(1) as u64;
    let tick_every = tick_every(world.steps_per_second);
    let evolve = Evolve::new(skin.skin_dry_time, skin.skin_bleed, skin.skin_drips, world.steps_per_second);
    let st = CanvasState {
        settings,
        grid,
        size,
        centre: Vec2::ZERO,
        ribbons: BTreeMap::new(),
        every,
        tick_every,
        evolve,
        runners: Vec::new(),
        ticks_run: 0,
    };
    world.skins.insert(thing, Box::new(st));
}

/// This step's splats on `thing`'s skin, stamped into its grid and logged, brushes in the
/// world's order. Arenas and obstacles call it in their Events stage.
pub fn after_step(thing: ThingId, world: &mut World, log: &mut Log) {
    let Some(mut boxed) = world.skins.remove(&thing) else { return };
    if let Some(st) = boxed.as_any_mut().downcast_mut::<CanvasState>() {
        for m in marks(thing, st, world, log) {
            log.push_mark(m);
        }
        if st.evolve.on() && world.step.is_multiple_of(st.tick_every) {
            let mut picked = std::mem::take(&mut world.picked);
            let pose = pose_of(world, thing);
            let cx = Tick {
                target: MarkTarget::Skin(thing),
                world,
                now: (world.step / st.tick_every) as u32,
                per_second: world.steps_per_second as f32 / st.tick_every as f32,
                gravity: Vec2::from_angle(-pose.angle).rotate(world.gravity),
            };
            // The outline is only worked out while something can drip (peg grids are big).
            let outline = (st.evolve.drips > 0.0
                && (!st.runners.is_empty() || !st.grid.wet_tiles(cx.now, st.evolve.dry_ticks).is_empty()))
            .then(|| outline_local(world, thing));
            for m in evolve::tick(&cx, st, &mut picked, outline.as_deref()) {
                log.push_mark(m);
            }
            world.picked = picked;
        }
    }
    world.skins.insert(thing, boxed);
}

/// The marks this step's hits on `thing` leave on its skin.
fn marks(thing: ThingId, st: &mut CanvasState, world: &World, log: &Log) -> Vec<Mark> {
    let hits: Vec<&Event> = log
        .since(world.step)
        .iter()
        .filter(|e| e.kind == EventKind::Hit && e.b.is_some_and(|b| b.thing == thing))
        .collect();
    if hits.is_empty() {
        return Vec::new();
    }
    let pose = pose_of(world, thing);
    let target = MarkTarget::Skin(thing);
    let tick = (world.step / st.tick_every) as u32;
    let skin = skin_settings(world, thing).cloned().unwrap_or_default();
    let mut colours = Colours::new(world);
    // The outline, worked out once a step and only when something spreads.
    let mut outline = None;
    let mut out = Vec::new();
    for (owner, b) in brushes(world) {
        if !splats_things(b) {
            continue;
        }
        // The mover's brush paints what it hit; the thing's own brush paints whatever hits it.
        for e in hits.iter().filter(|e| (owner == thing || e.a.thing == owner) && e.speed >= b.splat_min_speed) {
            // The mover's colour, whoever owns the brush.
            let colour = colours.of(b, e.a.thing, e.a.item as usize);
            match b.splat_skin {
                SplatSkin::Spread => {
                    let length = 2.0 * b.splat_size * (1.0 + b.splat_by_speed * (e.speed / 1000.0).min(2.0));
                    let item = e.b.map_or(0, |p| p.item);
                    let width = thin_reach(world, thing, item, &skin).unwrap_or(0.5 * b.splat_size);
                    let outline = outline.get_or_insert_with(|| outline_local(world, thing));
                    let at = to_local(pose, e.point);
                    for m in spread_marks(outline, at, length, width, colour, world.step, target) {
                        if reaches(st.size, m.from, m.to, m.radius) {
                            st.grid.stamp(&m, tick, painter(e.a.thing, e.a.item));
                            out.push(m);
                        }
                    }
                }
                SplatSkin::Print => {
                    let Stamp { shape, radius, mut angle, variant } = hit_stamp(world, e, b, pose.angle);
                    // A splat facing the hit throws its droplets along the surface, the way the
                    // ball was going along it, not into the wall where the skin hides them.
                    let n = e.normal.normalize_or_zero();
                    if shape == MarkShape::Splat && b.splat_turn == Turn::FaceHit && n != Vec2::ZERO {
                        let travel = Vec2::from_angle(angle + pose.angle);
                        angle = along_surface(n, travel).to_angle() - pose.angle;
                    }
                    // Pushed into the surface, so the print sits across the line.
                    let at = to_local(pose, e.point - e.normal.normalize_or_zero() * PRINT_DEPTH * radius);
                    if !reaches(st.size, at, at, radius) {
                        continue;
                    }
                    let mark = Mark {
                        step: world.step,
                        target,
                        shape,
                        from: at,
                        to: at,
                        radius,
                        radius_to: radius,
                        angle,
                        within: [1.0, 1.0],
                        variant,
                        colour,
                        colour_to: colour,
                    };
                    st.grid.stamp(&mark, tick, painter(e.a.thing, e.a.item));
                    out.push(mark);
                }
            }
        }
    }
    out
}

/// The way along a surface with unit `normal` that `travel` heads (either way when it heads
/// straight in).
fn along_surface(normal: Vec2, travel: Vec2) -> Vec2 {
    let t = normal.perp();
    if travel.dot(t) < 0.0 { -t } else { t }
}

/// The longest piece a spread is cut into, in pixels.
const SPREAD_PIECE: f32 = 8.0;

/// Where `at` falls nearest on `outline`: the polyline, the segment's index and the point.
pub(crate) fn nearest(outline: &[Vec<Vec2>], at: Vec2) -> Option<(usize, usize, Vec2)> {
    let mut best: Option<(f32, usize, usize, Vec2)> = None;
    for (l, line) in outline.iter().enumerate() {
        for (k, seg) in line.windows(2).enumerate() {
            let d = seg[1] - seg[0];
            let t = if d.length_squared() > 0.0 {
                ((at - seg[0]).dot(d) / d.length_squared()).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let p = seg[0] + d * t;
            let gap = (at - p).length_squared();
            if best.is_none_or(|b| gap < b.0) {
                best = Some((gap, l, k, p));
            }
        }
    }
    best.map(|(_, l, k, p)| (l, k, p))
}

/// The paint a Spread hit at `at` leaves: colour smeared along the nearest of `outline`'s
/// polylines (a closed one repeats its first point), half of `length` each way from the point
/// nearest `at`, in pieces of at most 8 px that end at every corner. Each half's radius falls
/// from `width` at the hit to 0 at its end. A closed line is walked round, never past its far
/// side; an open one stops at its ends. The forward half's pieces come first, each half from
/// the hit outwards.
pub fn spread_marks(
    outline: &[Vec<Vec2>],
    at: Vec2,
    length: f32,
    width: f32,
    colour: [f32; 4],
    step: u64,
    target: MarkTarget,
) -> Vec<Mark> {
    let Some((l, k, start)) = nearest(outline, at) else { return Vec::new() };
    let line = &outline[l];
    let n = line.len();
    let looped = n > 2 && line[0] == line[n - 1];
    let around: f32 = line.windows(2).map(|s| (s[1] - s[0]).length()).sum();
    let half = if looped { (length * 0.5).min(around * 0.5) } else { length * 0.5 };
    // Nothing to spread, also for NaN sizes.
    if !(half.is_finite() && width.is_finite() && half > 0.0 && width > 0.0) {
        return Vec::new();
    }
    let mut out: Vec<Mark> = Vec::new();
    let mut firsts = [None, None];
    for (half_index, forward) in [true, false].into_iter().enumerate() {
        let (mut point, mut k) = (start, k);
        let mut gone = 0.0f32;
        // The direction of the last piece, while the next may join on to it.
        let mut before: Option<Vec2> = None;
        loop {
            // The corner this way along the current segment.
            let corner = if forward { line[k + 1] } else { line[k] };
            let left = half - gone;
            let span = (corner - point).length();
            let take = span.min(left);
            if take > 0.0 {
                let end = if take >= span { corner } else { point + (corner - point) * (take / span) };
                let pieces = (take / SPREAD_PIECE).ceil().max(1.0) as usize;
                for i in 0..pieces {
                    let from = point.lerp(end, i as f32 / pieces as f32);
                    let to = if i + 1 == pieces { end } else { point.lerp(end, (i + 1) as f32 / pieces as f32) };
                    let (s0, s1) =
                        (gone + take * i as f32 / pieces as f32, gone + take * (i + 1) as f32 / pieces as f32);
                    let last = take >= left && i + 1 == pieces;
                    let d = (to - from).normalize_or_zero();
                    // Like a ribbon's pieces, the pieces of a straight run or a gentle bend are
                    // cut apart at their joint, so see-through paint is never drawn twice there.
                    let (variant, angle) = match before {
                        Some(b) if b.angle_to(d).abs() < SMEAR_BEND => {
                            if let Some(previous) = out.last_mut() {
                                previous.variant |= CONTINUED;
                            }
                            (CONTINUES, b.to_angle())
                        }
                        _ => (0, 0.0),
                    };
                    if firsts[half_index].is_none() {
                        firsts[half_index] = Some(out.len());
                    }
                    out.push(Mark {
                        step,
                        target,
                        shape: MarkShape::Smear,
                        from,
                        to,
                        radius: width * (1.0 - s0 / half).max(0.0),
                        radius_to: if last { 0.0 } else { width * (1.0 - s1 / half).max(0.0) },
                        angle,
                        within: [1.0, 1.0],
                        variant,
                        colour,
                        colour_to: colour,
                    });
                    before = (d != Vec2::ZERO).then_some(d);
                }
                gone += take;
            }
            if take >= left {
                break;
            }
            // On round the next corner, wrapping a closed line; an open one ends here.
            match (forward, k) {
                (true, k2) if k2 + 2 < n => k += 1,
                (true, _) if looped => k = 0,
                (false, k2) if k2 > 0 => k -= 1,
                (false, _) if looped => k = n - 2,
                _ => break,
            }
            point = if forward { line[k] } else { line[k + 1] };
        }
    }
    // The two halves set off back to back from the hit: each is cut at the hit across its own
    // way, which splits the paint there exactly between them.
    if let [Some(f), Some(b)] = firsts {
        let way = |m: &Mark| (m.to - m.from).normalize_or_zero();
        let (fw, bw) = (way(&out[f]), way(&out[b]));
        if fw.dot(bw) < -0.999 {
            for (i, w) in [(f, fw), (b, bw)] {
                out[i].variant |= CONTINUES;
                out[i].angle = w.to_angle();
            }
        }
    }
    out
}

/// The sharpest turn in radians at which a Spread's pieces join without their round caps; a
/// sharper corner keeps them, as a ribbon does at a hit.
const SMEAR_BEND: f32 = 0.5;

#[cfg(test)]
mod tests {
    use bpe_core::{Address, Owner, Params, Resolved, Scene, Value, resolve};
    use bpe_runtime::{MarkShape, Party, Playback, fingerprint};

    use super::*;
    use crate::testing::{ARENA, BALLS, OBSTACLE, module_of, run, scene_with_obstacle};
    use crate::{Kit, add_thing, fill_slot, kit, new_scene};

    /// Gives `thing` a Marks module that splats and doesn't paint, with `f`'s changes.
    fn splatter(scene: &mut Scene, kit: &Kit, thing: ThingId, f: impl FnOnce(&mut Params)) {
        fill_slot(scene, &kit.registry, thing, SlotId::Marks).unwrap();
        let m = module_of(scene, &kit.registry, thing, SlotId::Marks);
        let p = &mut scene.library.modules.get_mut(&m).unwrap().params;
        p.set("paint_on", Value::Bool(false));
        p.set("splat_on", Value::Bool(true));
        p.set("splat_min_speed", Value::Number(100.0));
        f(p);
    }

    /// The new scene with a splatting ball and `f`'s changes to its brush.
    fn splatting(f: impl FnOnce(&mut Params)) -> (Scene, Resolved, Kit) {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        splatter(&mut scene, &kit, BALLS, f);
        let resolved = resolve(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        (scene, resolved, kit)
    }

    fn built(scene: &Scene, kit: &Kit) -> World {
        World::build(scene, &resolve(scene, &kit.registry), &kit.sims)
    }

    /// `p`, in a thing's own frame at `pose`, back in the world.
    fn world_point(pose: Pose, p: Vec2) -> Vec2 {
        pose.position + Vec2::from_angle(pose.angle).rotate(p)
    }

    #[test]
    fn skin_look_keys_mirror_the_canvas() {
        let canvas = CanvasSettings::defs();
        let skin = SkinSettings::defs();
        for k in [
            "style",
            "light_angle",
            "gloss",
            "grain",
            "edge_dark",
            "glow_size",
            "glow_strength",
            "dry_time",
            "bleed",
            "drips",
        ] {
            let c = canvas.iter().find(|d| d.key == k).unwrap_or_else(|| panic!("canvas {k}"));
            let s = skin.iter().find(|d| d.key == format!("skin_{k}")).unwrap_or_else(|| panic!("skin_{k}"));
            assert_eq!((s.label, s.range, s.group), (c.label, c.range, "Paint skin"), "{k}");
            assert!(s.when.starts_with("skin_on"), "{k}: {}", s.when);
        }
    }

    #[test]
    fn skin_settings_sit_in_arena_and_obstacle_settings() {
        let kit = kit();
        for kind in ["arena", "obstacle"] {
            let defs = kit.registry.thing(kind).unwrap().settings();
            for key in ["skin_on", "skin_width", "skin_fade", "skin_opacity"] {
                let d = defs.iter().find(|d| d.key == key).unwrap_or_else(|| panic!("{kind}: no {key}"));
                assert_eq!(d.group, "Paint skin");
                let sim = kit.sims.thing(kind).unwrap();
                assert!(!sim.affects_physics(key), "{kind} {key}");
            }
            let opacity = defs.iter().find(|d| d.key == "skin_opacity").unwrap();
            assert!(opacity.flags.keyframe);
            assert!(kit.sims.thing(kind).unwrap().affects_physics("thickness"));
        }
        assert!(bpe_core::section_rank("Paint skin").is_some());
        let d = SkinSettings::default();
        assert!(d.skin_on && d.skin_width == 2.0 && d.skin_fade == 0.0 && d.skin_opacity == 1.0);
        let mut a = crate::ArenaSettings::default();
        a.skin.skin_width = 2.5;
        assert_eq!(crate::ArenaSettings::from_params(&a.to_params()), a);
    }

    #[test]
    fn skins_only_where_splats_land_on_things() {
        // No Marks at all: no skins, and skin settings don't count.
        let kit = kit();
        let mut plain = new_scene(&kit.registry);
        assert!(built(&plain, &kit).skins.is_empty());
        let print = |s: &Scene| fingerprint(s, &resolve(s, &kit.registry), &kit.sims);
        let before = print(&plain);
        plain.things.get_mut(&ARENA).unwrap().params.set("skin_width", Value::Number(2.0));
        plain.things.get_mut(&ARENA).unwrap().params.set("skin_on", Value::Bool(false));
        assert_eq!(print(&plain), before);

        // Splats onto canvases only: no skins.
        let (scene, _, kit) = splatting(|p| p.set("splat_onto", Value::Choice("canvases".into())));
        assert!(built(&scene, &kit).skins.is_empty());
        // A brush that only paints trails: no skins.
        let (scene, _, kit) = splatting(|p| p.set("splat_on", Value::Bool(false)));
        assert!(built(&scene, &kit).skins.is_empty());

        // The thing hit (the default) and both: the arena has a skin, counted in memory.
        for onto in [None, Some("both")] {
            let (scene, _, kit) = splatting(|p| {
                if let Some(onto) = onto {
                    p.set("splat_onto", Value::Choice(onto.into()));
                }
            });
            let w = built(&scene, &kit);
            assert_eq!(w.skins.keys().copied().collect::<Vec<_>>(), [ARENA], "{onto:?}");
            let (pose, size) = skin_area(&w, ARENA).unwrap();
            assert_eq!(pose, w.poses[&ARENA]);
            // The ring of radius 508 and a splat's reach of 30 x (1 + 0.5 x 2) around it.
            assert!((size - Vec2::splat((508.0 + 60.0) * 2.0)).abs().max_element() < 0.5, "{size}");
            let bytes = skin_state(&w, ARENA).unwrap().grid.bytes();
            assert_eq!(w.state_memory[&ARENA], bytes);
        }

        // With marks, the skin settings that change the skin count; how it looks does not.
        let (mut scene, _, kit) = splatting(|_| {});
        let before = print(&scene);
        let arena = &mut scene.things.get_mut(&ARENA).unwrap().params;
        arena.set("skin_fade", Value::Number(3.0));
        arena.set("skin_opacity", Value::Number(0.5));
        assert_eq!(print(&scene), before);
        scene.things.get_mut(&ARENA).unwrap().params.set("skin_width", Value::Number(3.0));
        assert_ne!(print(&scene), before);

        // Takes paint off: no skin.
        scene.things.get_mut(&ARENA).unwrap().params.set("skin_on", Value::Bool(false));
        assert!(built(&scene, &kit).skins.is_empty());
    }

    #[test]
    fn prints_follow_the_ball_along_the_surface() {
        let up = Vec2::new(0.0, -1.0);
        // Bouncing off a floor going right and up: along the floor to the right.
        assert_eq!(along_surface(up, Vec2::new(3.0, -4.0)), Vec2::new(1.0, 0.0));
        assert_eq!(along_surface(up, Vec2::new(-3.0, -4.0)), Vec2::new(-1.0, 0.0));
        // Straight back up: one way or the other, still along the floor.
        assert_eq!(along_surface(up, up).dot(up), 0.0);
    }

    #[test]
    fn print_lands_on_the_hit_thing() {
        let (mut scene, _, kit) = splatting(|_| {});
        let arena = &mut scene.things.get_mut(&ARENA).unwrap().params;
        arena.set("shape", Value::Choice("box".into()));
        arena.set("width", Value::Number(600.0));
        arena.set("height", Value::Number(800.0));
        let resolved = resolve(&scene, &kit.registry);
        let (w, log) = run(&scene, &resolved, &kit, 6.0);
        let hits: Vec<_> = log
            .events()
            .iter()
            .filter(|e| e.kind == EventKind::Hit && e.b.is_some_and(|b| b.thing == ARENA) && e.speed >= 100.0)
            .collect();
        assert!(hits.len() >= 3, "{} hits", hits.len());
        let marks = log.marks();
        assert_eq!(marks.len(), hits.len(), "one print per hit");
        let pose = w.poses[&ARENA];
        for (m, e) in marks.iter().zip(&hits) {
            assert_eq!((m.target, m.shape, m.step), (MarkTarget::Skin(ARENA), MarkShape::Splat, e.step));
            assert_eq!(m.from, m.to);
            let radius = 30.0 * (1.0 + 0.5 * (e.speed / 1000.0).min(2.0));
            assert!((m.radius - radius).abs() < 1e-3);
            // Pushed 0.3 radii into the wall from where the ball touched it.
            let centre = world_point(pose, m.from);
            assert!(((centre - e.point).length() - 0.3 * radius).abs() < 1.0, "{centre} from {}", e.point);
            assert!((centre - e.point).dot(e.normal) < 0.0, "into the wall, away from the ball");
            // Its droplets fly along the wall, not into it.
            let way = Vec2::from_angle(m.angle + pose.angle);
            assert!(way.dot(e.normal.normalize()).abs() < 1e-4, "{way} across a wall facing {}", e.normal);
        }
        assert!(skin_state(&w, ARENA).unwrap().grid.covered() > 0.0);
    }

    /// A ball resting near a spinning bar, with no gravity: hits are pushed in by hand.
    fn spinning_bar() -> (Scene, Resolved, Kit) {
        let (mut scene, _, kit) = scene_with_obstacle(|p| {
            p.set("shape", Value::Choice("bar".into()));
            p.set("spin", Value::Number(90.0));
        });
        let m = module_of(&scene, &kit.registry, BALLS, SlotId::Motion);
        scene.library.modules.get_mut(&m).unwrap().params.set("gravity_scale", Value::Number(0.0));
        splatter(&mut scene, &kit, BALLS, |p| p.set("splat_min_speed", Value::Number(0.0)));
        let resolved = resolve(&scene, &kit.registry);
        (scene, resolved, kit)
    }

    #[test]
    fn skin_marks_follow_a_spinning_bar() {
        let (scene, resolved, kit) = spinning_bar();
        let mut w = World::build(&scene, &resolved, &kit.sims);
        assert!(w.skins.contains_key(&OBSTACLE));
        let mut log = Log::default();
        // The same spot of the bar, 150 px along its top side, at two moments.
        let spot = Vec2::new(150.0, -4.0);
        let mut seen = Vec::new();
        for time in [0.25, 0.75] {
            w.advance_to(time, &kit.sims, &mut log);
            let pose = w.poses[&OBSTACLE];
            let normal = Vec2::from_angle(pose.angle).rotate(Vec2::new(0.0, -1.0));
            let e = Event {
                step: w.step,
                kind: EventKind::Hit,
                a: Party { thing: BALLS, item: 0 },
                b: Some(Party { thing: OBSTACLE, item: 0 }),
                point: world_point(pose, spot),
                normal,
                speed: 500.0,
            };
            let point = e.point;
            log.push(e);
            let before = log.marks().len();
            after_step(OBSTACLE, &mut w, &mut log);
            assert_eq!(log.marks().len(), before + 1);
            seen.push((point, *log.marks().last().unwrap()));
        }
        let ((p1, m1), (p2, m2)) = (seen[0], seen[1]);
        assert!((p1 - p2).length() > 100.0, "the bar turned: {p1} then {p2}");
        assert!((m1.from - m2.from).length() < 1e-3, "{} vs {}", m1.from, m2.from);
        let radius = 30.0 * 1.25;
        assert!((m1.from - (spot + Vec2::new(0.0, 0.3 * radius))).length() < 1e-3, "{}", m1.from);
        assert_eq!(m1.target, MarkTarget::Skin(OBSTACLE));
    }

    #[test]
    fn real_hits_on_a_spinning_bar_land_in_its_frame() {
        let (mut scene, _, kit) = scene_with_obstacle(|p| {
            p.set("shape", Value::Choice("bar".into()));
            p.set("spin", Value::Number(90.0));
        });
        splatter(&mut scene, &kit, BALLS, |p| p.set("splat_min_speed", Value::Number(0.0)));
        let resolved = resolve(&scene, &kit.registry);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        while !log.marks().iter().any(|m| m.target == MarkTarget::Skin(OBSTACLE)) && w.step < 240 * 5 {
            w.step(&kit.sims, &mut log);
        }
        let m = *log.marks().iter().find(|m| m.target == MarkTarget::Skin(OBSTACLE)).expect("the bar was hit");
        let e = log.since(w.step).iter().find(|e| e.b.is_some_and(|b| b.thing == OBSTACLE)).unwrap();
        let pose = w.poses[&OBSTACLE];
        assert!(pose.angle.abs() > 0.1, "{pose:?}");
        let into = e.point - e.normal * 0.3 * m.radius;
        assert!((world_point(pose, m.from) - into).length() < 1e-2);
        // On the bar, in its frame: near its centre line.
        assert!(m.from.y.abs() < 4.0 + 0.3 * m.radius + 0.5 && m.from.x.abs() < 205.0, "{}", m.from);
    }

    #[test]
    fn wall_brush_paints_its_own_skin() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        splatter(&mut scene, &kit, ARENA, |_| {});
        let look = module_of(&scene, &kit.registry, BALLS, SlotId::Look);
        let red = bpe_core::Color::rgb(255, 0, 0);
        let p = &mut scene.library.modules.get_mut(&look).unwrap().params;
        p.set("colours", Value::Gradient(bpe_core::Gradient::even(&[red, red])));
        let resolved = resolve(&scene, &kit.registry);
        assert!(!resolved.things[&BALLS].slots.contains_key(&SlotId::Marks), "the ball has no brushes");
        let (_, log) = run(&scene, &resolved, &kit, 6.0);
        let hits = log.events().iter().filter(|e| e.kind == EventKind::Hit && e.speed >= 100.0).count();
        assert!(hits >= 2);
        assert_eq!(log.marks().len(), hits);
        for m in log.marks() {
            assert_eq!(m.target, MarkTarget::Skin(ARENA));
            assert!(m.colour[0] > 0.99 && m.colour[1] < 0.01, "{:?}: the ball's red", m.colour);
        }
    }

    #[test]
    fn ball_hits_paint_no_skin() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let ball = scene.things.get_mut(&BALLS).unwrap();
        ball.params.set("arrangement", Value::Choice("block".into()));
        ball.params.set("columns", Value::Int(4));
        ball.params.set("rows", Value::Int(3));
        let bounce = module_of(&scene, &kit.registry, BALLS, SlotId::Bounce);
        scene.library.modules.get_mut(&bounce).unwrap().params.set("hit_balls", Value::Bool(true));
        splatter(&mut scene, &kit, BALLS, |p| p.set("splat_min_speed", Value::Number(0.0)));
        let resolved = resolve(&scene, &kit.registry);
        let (_, log) = run(&scene, &resolved, &kit, 6.0);
        let hit = |e: &&Event| e.kind == EventKind::Hit;
        let balls = log.events().iter().filter(hit).filter(|e| e.b.is_some_and(|b| b.thing == BALLS)).count();
        let walls = log.events().iter().filter(hit).filter(|e| e.b.is_some_and(|b| b.thing == ARENA)).count();
        assert!(balls > 0 && walls > 0, "{balls} ball hits, {walls} wall hits");
        assert_eq!(log.marks().len(), walls);
        assert!(log.marks().iter().all(|m| m.target == MarkTarget::Skin(ARENA)));
    }

    #[test]
    fn both_paints_the_canvas_and_the_skin() {
        let (mut scene, _, kit) = splatting(|p| p.set("splat_onto", Value::Choice("both".into())));
        let canvas = add_thing(&mut scene, &kit.registry, "canvas", ThingId::ROOT).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        let (_, log) = run(&scene, &resolved, &kit, 6.0);
        let on = |t: MarkTarget| log.marks().iter().filter(|m| m.target == t).count();
        let hits = log.events().iter().filter(|e| e.kind == EventKind::Hit && e.speed >= 100.0).count();
        assert!(hits >= 2);
        assert_eq!((on(MarkTarget::Canvas(canvas)), on(MarkTarget::Skin(ARENA))), (hits, hits));
    }

    /// A ball that splats the arena with random turns, for the determinism tests.
    fn busy() -> (Scene, Resolved, Kit) {
        splatting(|p| {
            p.set("splat_turn", Value::Choice("random".into()));
            p.set("splat_min_speed", Value::Number(0.0));
        })
    }

    #[test]
    fn same_run_same_skins() {
        let (scene, resolved, kit) = busy();
        let mut a = Playback::new(&scene, &resolved, kit.sims.clone());
        let mut b = Playback::new(&scene, &resolved, kit.sims.clone());
        a.seek(5.0);
        b.seek(5.0);
        let (ga, gb) = (&skin_state(a.world(), ARENA).unwrap().grid, &skin_state(b.world(), ARENA).unwrap().grid);
        assert!(ga.covered() > 0.0);
        assert_eq!(ga.hash(), gb.hash());
        assert!(!a.log().marks().is_empty());
        assert_eq!(a.log().marks(), b.log().marks());
    }

    #[test]
    fn seek_equals_straight_for_skins() {
        let (scene, resolved, kit) = busy();
        let mut straight = Playback::new(&scene, &resolved, kit.sims.clone());
        straight.seek(5.0);
        let mut jumpy = Playback::new(&scene, &resolved, kit.sims.clone());
        jumpy.seek(8.0);
        jumpy.seek(5.0);
        let grid = |p: &Playback| skin_state(p.world(), ARENA).unwrap().grid.hash();
        assert_eq!(grid(&straight), grid(&jumpy));
        assert_eq!(straight.log().marks(), jumpy.log().marks());
        assert_eq!(straight.log().hash(), jumpy.log().hash());
    }

    #[test]
    fn skins_leave_the_physics_alone() {
        let (scene, resolved, kit) = busy();
        let (_, with) = run(&scene, &resolved, &kit, 4.0);
        let mut off = scene.clone();
        off.things.get_mut(&ARENA).unwrap().params.set("skin_on", Value::Bool(false));
        let (_, without) = run(&off, &resolve(&off, &kit.registry), &kit, 4.0);
        assert!(!with.marks().is_empty() && without.marks().is_empty());
        assert_eq!(with.hash(), without.hash());
        // A keyframed opacity is the picture's, not the simulation's.
        let mut keyed = scene.clone();
        let a = Address { owner: Owner::Thing(ARENA), slot: None, key: "skin_opacity".into() };
        keyed.set_key(&kit.registry, &a, 0.0, Value::Number(0.0)).unwrap();
        keyed.set_key(&kit.registry, &a, 1.0, Value::Number(1.0)).unwrap();
        let keyed_resolved = resolve(&keyed, &kit.registry);
        assert_eq!(fingerprint(&keyed, &keyed_resolved, &kit.sims), fingerprint(&scene, &resolved, &kit.sims));
    }

    /// The pieces' total length.
    fn total(marks: &[Mark]) -> f32 {
        marks.iter().map(|m| (m.to - m.from).length()).sum()
    }

    const WHITE: [f32; 4] = [1.0; 4];

    /// A ball splatting with Spread in the new scene, with `f`'s changes to the arena.
    fn spreading(f: impl FnOnce(&mut Params)) -> (Scene, Resolved, Kit) {
        let (mut scene, _, kit) = splatting(|p| p.set("splat_skin", Value::Choice("spread".into())));
        f(&mut scene.things.get_mut(&ARENA).unwrap().params);
        let resolved = resolve(&scene, &kit.registry);
        (scene, resolved, kit)
    }

    #[test]
    fn spread_follows_a_circle() {
        let (scene, resolved, kit) = spreading(|p| p.set("radius", Value::Number(300.0)));
        let (w, log) = run(&scene, &resolved, &kit, 6.0);
        let marks = log.marks();
        assert!(marks.len() > 20, "{} pieces", marks.len());
        let centre = to_local(w.poses[&ARENA], w.walls[&ARENA].outline.center());
        for m in marks {
            assert_eq!((m.target, m.shape), (MarkTarget::Skin(ARENA), MarkShape::Smear));
            for p in [m.from, m.to] {
                let off = ((p - centre).length() - 300.0).abs();
                assert!(off <= 0.5, "{p} is {off} px off the circle");
            }
        }
        assert!(skin_state(&w, ARENA).unwrap().grid.covered() > 0.0);
    }

    #[test]
    fn spread_turns_a_box_corner() {
        let half = Vec2::new(300.0, 400.0);
        let outline = vec![outline_line(&Outline::Box { center: Vec2::ZERO, half, angle: 0.0, thickness: 3.0 })];
        // 5 px from the top right corner, along the top.
        let at = Vec2::new(295.0, -401.0);
        let marks = spread_marks(&outline, at, 80.0, 3.0, WHITE, 9, MarkTarget::Skin(ARENA));
        let on_top = |p: Vec2| (p.y + half.y).abs() < 1e-3 && p.x <= half.x + 1e-3;
        let on_side = |p: Vec2| (p.x - half.x).abs() < 1e-3 && p.y >= -half.y - 1e-3;
        assert!(marks.iter().any(|m| on_side(m.from) && on_side(m.to) && m.to.y > -half.y + 1.0), "down the side");
        assert!(marks.iter().any(|m| on_top(m.from) && on_top(m.to)), "along the top");
        for m in &marks {
            let along = (on_top(m.from) && on_top(m.to)) || (on_side(m.from) && on_side(m.to));
            assert!(along, "{} to {} cuts across", m.from, m.to);
            assert!((m.to - m.from).length() <= 8.0 + 1e-3);
            assert_eq!((m.step, m.target), (9, MarkTarget::Skin(ARENA)));
        }
        assert!((total(&marks) - 80.0).abs() < 1e-2);
    }

    #[test]
    fn spread_pieces_join_like_a_ribbon() {
        // Round a circle (gentle bends): every joint is cut, both halves at the hit too, so
        // see-through paint has no beads; only the two far ends keep a cap.
        let circle = vec![circle(Vec2::ZERO, 300.0, 0.0, ROUND)];
        let marks = spread_marks(&circle, Vec2::new(20.0, 300.0), 120.0, 5.0, WHITE, 1, MarkTarget::Skin(ARENA));
        let second = marks.iter().position(|m| m.radius_to == 0.0).unwrap() + 1;
        for (k, pair) in marks.windows(2).enumerate() {
            if k + 1 != second {
                assert_ne!(pair[0].variant & CONTINUED, 0, "piece {k} leaves its end to the next");
                assert_ne!(pair[1].variant & CONTINUES, 0, "piece {} leaves its start to the last", k + 1);
                assert!((pair[1].angle - (pair[0].to - pair[0].from).to_angle()).abs() < 1e-5);
            }
        }
        for start in [0, second] {
            let m = &marks[start];
            assert_ne!(m.variant & CONTINUES, 0, "the halves meet at the hit without caps");
            assert!((m.angle - (m.to - m.from).to_angle()).abs() < 1e-5, "each cut across its own way");
        }
        for end in [second - 1, marks.len() - 1] {
            assert_eq!(marks[end].variant & CONTINUED, 0, "a far end keeps its cap");
        }

        // A box corner is sharp: the pieces either side of it keep their caps.
        let half = Vec2::new(300.0, 400.0);
        let outline = vec![outline_line(&Outline::Box { center: Vec2::ZERO, half, angle: 0.0, thickness: 3.0 })];
        let marks = spread_marks(&outline, Vec2::new(295.0, -401.0), 80.0, 3.0, WHITE, 9, MarkTarget::Skin(ARENA));
        let corner = Vec2::new(300.0, -400.0);
        let at = marks.iter().position(|m| (m.from - corner).length() < 1e-3).expect("a piece from the corner");
        assert_eq!(marks[at].variant & CONTINUES, 0);
        assert_eq!(marks[at - 1].variant & CONTINUED, 0);
    }

    #[test]
    fn spread_length_matches() {
        let circle = vec![circle(Vec2::ZERO, 300.0, 0.0, ROUND)];
        let marks = spread_marks(&circle, Vec2::new(0.0, 310.0), 150.0, 4.0, WHITE, 1, MarkTarget::Skin(ARENA));
        assert!((total(&marks) - 150.0).abs() <= 8.0, "{}", total(&marks));
        // Longer than the loop: once round, never over itself.
        let marks = spread_marks(&circle, Vec2::new(0.0, 310.0), 5000.0, 4.0, WHITE, 1, MarkTarget::Skin(ARENA));
        let round: f32 = circle[0].windows(2).map(|s| (s[1] - s[0]).length()).sum();
        assert!((total(&marks) - round).abs() < 0.1, "{} of {round}", total(&marks));

        // In a run, each hit spreads 2 x size x (1 + by speed x min(speed / 1000, 2)).
        let (scene, resolved, kit) = spreading(|_| {});
        let (_, log) = run(&scene, &resolved, &kit, 6.0);
        let hits: Vec<_> = log.events().iter().filter(|e| e.kind == EventKind::Hit && e.speed >= 100.0).collect();
        assert!(hits.len() >= 2);
        for e in hits {
            let mine: Vec<Mark> = log.marks().iter().filter(|m| m.step == e.step).copied().collect();
            let length = 2.0 * 30.0 * (1.0 + 0.5 * (e.speed / 1000.0).min(2.0));
            assert!((total(&mine) - length).abs() <= 8.0, "{} vs {length}", total(&mine));
        }
    }

    #[test]
    fn spread_tapers() {
        let circle = vec![circle(Vec2::ZERO, 300.0, 0.0, ROUND)];
        let marks = spread_marks(&circle, Vec2::new(20.0, 300.0), 120.0, 5.0, WHITE, 1, MarkTarget::Skin(ARENA));
        // Two halves, each from the hit outwards: full width at the hit, none at the far end.
        let ends: Vec<usize> = (0..marks.len()).filter(|&k| marks[k].radius_to == 0.0).collect();
        assert_eq!(ends.len(), 2, "{ends:?}");
        assert_eq!(*ends.last().unwrap(), marks.len() - 1);
        let second = ends[0] + 1;
        for start in [0, second] {
            assert_eq!(marks[start].radius, 5.0);
            assert!((marks[start].from - marks[0].from).length() < 1e-3, "both halves start at the hit");
        }
        for (k, pair) in marks.windows(2).enumerate() {
            if k + 1 != second {
                assert_eq!(pair[0].to, pair[1].from);
                assert_eq!(pair[0].radius_to, pair[1].radius);
                assert!(pair[1].radius < pair[0].radius);
            }
        }
        // Linearly: halfway along a half, half the width.
        let mut gone = 0.0;
        for m in &marks[..second] {
            assert!((m.radius - 5.0 * (1.0 - gone / 60.0)).abs() < 1e-3);
            gone += (m.to - m.from).length();
        }
    }

    #[test]
    fn spread_width_on_solids() {
        let (mut scene, _, kit) = scene_with_obstacle(|p| p.set("radius", Value::Number(40.0)));
        splatter(&mut scene, &kit, BALLS, |p| {
            p.set("splat_skin", Value::Choice("spread".into()));
            p.set("splat_min_speed", Value::Number(0.0));
        });
        let resolved = resolve(&scene, &kit.registry);
        let (_, log) = run(&scene, &resolved, &kit, 3.0);
        let on_peg: Vec<_> = log.marks().iter().filter(|m| m.target == MarkTarget::Skin(OBSTACLE)).collect();
        assert!(!on_peg.is_empty(), "the peg was hit");
        let widest = on_peg.iter().map(|m| m.radius).fold(0.0, f32::max);
        assert_eq!(widest, 0.5 * 30.0, "half the splat size");
        // Along the peg's edge, in its frame.
        for m in &on_peg {
            assert!((m.from.length() - 40.0).abs() < 0.5, "{}", m.from);
        }
        // A thin line spreads the line's reach: the arena's 3 px wall, Skin width 2 (the default).
        let (scene, resolved, kit) = spreading(|_| {});
        let (_, log) = run(&scene, &resolved, &kit, 6.0);
        assert!(!log.marks().is_empty());
        let widest = log.marks().iter().map(|m| m.radius).fold(0.0, f32::max);
        assert_eq!(widest, 1.5 * 3.0);
    }
}
