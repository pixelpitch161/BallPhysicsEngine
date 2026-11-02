//! The canvas in the simulation: stamping brush strokes and hit splats into its grid and logging
//! every mark, so the graphics card can redraw them sharp.
//!
//! Everything here runs in the Events stage, after the physics, and never changes it: the grid
//! and the marks stay out of the log's hash.

use std::collections::BTreeMap;
use std::f32::consts::TAU;

use bpe_core::{ResolvedThing, Settings, SlotId, Thing, ThingId};
use bpe_runtime::{Event, EventKind, Log, Mark, MarkShape, MarkTarget, Pose, ThingSim, World, state};
use glam::Vec2;

use super::grid::{CanvasGrid, painter, tick_every};
use super::ribbon::{Piece, Ribbon, Tune};
use super::{CanvasSettings, Fit};
use crate::modules::look::SimLook;
use crate::modules::look::draw::ball_inputs;
use crate::modules::marks::{MarkColour, MarksBrushes, PictureColours, SplatShape, Tip, Turn, When, WidthBy};
use crate::paint::{Follow, Inputs, colour_at};
use crate::things::liquid::sim::bounds;

pub struct CanvasSim;

/// What a canvas keeps between steps.
#[derive(Clone)]
pub struct CanvasState {
    pub settings: CanvasSettings,
    /// The paint so far, centred on the canvas's middle.
    pub grid: CanvasGrid,
    /// The canvas's width and height in world pixels.
    pub size: Vec2,
    /// Where the canvas's middle sits from its anchor, before the anchor turns.
    pub centre: Vec2,
    /// Per brush thing and item: its trail's ribbon (last point, width, paint left in its load).
    pub ribbons: BTreeMap<(ThingId, u32), Ribbon>,
    /// Steps between strokes: 60 strokes a second.
    pub every: u64,
    /// Steps between wet-paint ticks: 30 a second.
    pub tick_every: u64,
}

/// Where a canvas is and how big it is: its pose (middle and turn) and its width and height.
pub fn canvas_area(world: &World, canvas: ThingId) -> Option<(Pose, Vec2)> {
    let st = world.states.get(&canvas)?.as_any().downcast_ref::<CanvasState>()?;
    Some((world.poses.get(&canvas).copied().unwrap_or_default(), st.size))
}

impl CanvasSim {
    /// Writes the canvas's pose: its anchor, moved on to the middle of its area.
    fn place(&self, thing: ThingId, world: &mut World) {
        let Some(st) = world.states.get(&thing) else { return };
        let centre = state::<CanvasState>(st.as_ref()).centre;
        let pose = world.anchor(thing).local(centre, true);
        world.poses.insert(thing, pose);
    }
}

impl ThingSim for CanvasSim {
    fn kind(&self) -> &'static str {
        "canvas"
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let settings = CanvasSettings::from_params(&resolved.params);
        // The first arena's box, when there is one and the canvas fits it; else the video. The
        // canvas's own place moves it off that middle, so at 0,0 it covers the box exactly.
        let arena = world
            .walls
            .iter()
            .find(|(id, _)| world.kind_of(**id) == Some("arena"))
            .map(|(_, wall)| bounds(&wall.outline));
        let (centre, size) = match (settings.fit, arena) {
            (Fit::Arena, Some(b)) => ((b.min + b.max) * 0.5, b.size()),
            (Fit::Arena, None) | (Fit::View, _) => (Vec2::ZERO, world.video),
            (Fit::Custom, _) => (Vec2::ZERO, settings.size),
        };
        let size = size.max(Vec2::ONE);
        let every = (world.steps_per_second / 60).max(1) as u64;
        let tick_every = tick_every(world.steps_per_second);
        let grid = CanvasGrid::new(size);
        world.state_memory.insert(thing.id, grid.bytes());
        world.states.insert(
            thing.id,
            Box::new(CanvasState { settings, grid, size, centre, ribbons: BTreeMap::new(), every, tick_every }),
        );
        self.place(thing.id, world);
    }

    fn after_build(&self, _thing: ThingId, world: &mut World) {
        crate::pictures::fill(world);
    }

    fn needs_walls(&self) -> bool {
        true
    }

    fn scripted(&self, thing: ThingId, world: &mut World) {
        self.place(thing, world);
    }

    fn after_step(&self, canvas: ThingId, world: &mut World, log: &mut Log) {
        // The state comes out while the brushes read the world, and goes back after.
        let Some(mut boxed) = world.states.remove(&canvas) else { return };
        if let Some(st) = boxed.as_any_mut().downcast_mut::<CanvasState>() {
            for m in marks(canvas, st, world, log) {
                log.push_mark(m);
            }
        }
        world.states.insert(canvas, boxed);
    }

    fn affects_physics(&self, key: &str) -> bool {
        !matches!(
            key,
            "layer"
                | "style"
                | "detail"
                | "fade"
                | "blend"
                | "opacity"
                | "start_colour"
                | "light_angle"
                | "gloss"
                | "grain"
                | "edge_dark"
                | "glow_size"
                | "glow_strength"
        )
    }
}

/// The shortest stroke worth a mark: shorter moves wait and add up into a later stroke.
const MIN_STROKE: f32 = 0.5;

/// What one step's marking on one canvas works with.
struct Cx<'a> {
    canvas: ThingId,
    pose: Pose,
    world: &'a World,
    log: &'a Log,
    colours: Colours<'a>,
    /// The marks made so far, in order.
    out: Vec<Mark>,
}

/// The colours marks take, with the looks of the balls marked this step read once each.
pub(crate) struct Colours<'a> {
    world: &'a World,
    /// `None` when the ball's look is unknown.
    looks: BTreeMap<ThingId, Option<SimLook>>,
}

/// This step's marks on `canvas`, already stamped into its grid: every matching brush's strokes
/// (60 times a second) and splats, brushes in the world's order.
fn marks(canvas: ThingId, st: &mut CanvasState, world: &World, log: &Log) -> Vec<Mark> {
    let pose = world.poses.get(&canvas).copied().unwrap_or_default();
    let mut cx = Cx { canvas, pose, world, log, colours: Colours::new(world), out: Vec::new() };
    let paint_now = world.step.is_multiple_of(st.every);
    for &thing in world.order() {
        let Some((_, brush)) = world.module_state(thing, SlotId::Marks) else { continue };
        let Some(b) = brush.as_any().downcast_ref::<MarksBrushes>() else { continue };
        if !(b.paint_on || b.splat_on) || !world.matches(canvas, thing, &st.settings.brushes_from) {
            continue;
        }
        if b.paint_on && paint_now {
            cx.strokes(st, thing, b);
        }
        // Splats meant for the thing hit come here too when it has no skin to take them.
        if b.splat_on {
            cx.splats(st, thing, b);
        }
    }
    cx.out
}

/// `p` in canvas pixels: from the canvas's middle, before the canvas turns.
pub(crate) fn to_local(pose: Pose, p: Vec2) -> Vec2 {
    Vec2::from_angle(-pose.angle).rotate(p - pose.position)
}

/// Whether a capsule of `radius` around the segment `a`..`b` (canvas pixels) may reach onto a
/// canvas of `size`: the segment against the canvas grown by the radius.
pub(crate) fn reaches(size: Vec2, a: Vec2, b: Vec2, radius: f32) -> bool {
    let half = size * 0.5 + Vec2::splat(radius.max(0.0));
    // Clip the segment to the box one axis at a time (Liang-Barsky).
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [(-d.x, a.x + half.x), (d.x, half.x - a.x), (-d.y, a.y + half.y), (d.y, half.y - a.y)] {
        if p == 0.0 {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

impl<'a> Colours<'a> {
    pub(crate) fn new(world: &'a World) -> Self {
        Colours { world, looks: BTreeMap::new() }
    }

    /// The colour ball `i` of `thing` marks with under brush `b`: its own colour when the brush
    /// says Ball and the ball's look is known, else the brush's own Colours.
    pub(crate) fn of(&mut self, b: &MarksBrushes, thing: ThingId, i: usize) -> [f32; 4] {
        let world = self.world;
        if b.colour_from == MarkColour::Ball
            && let Some(look) =
                self.looks.entry(thing).or_insert_with(|| world.looks.get(&thing).map(|p| SimLook::new(p, world)))
        {
            return look.colour(world, thing, i);
        }
        let time = world.time();
        let inputs = match world.bodies.get(&thing) {
            Some(bodies) => {
                // The world's count: the log may have been trimmed.
                let beats = if b.colour.follow == Follow::Beat { world.beats as f32 } else { 0.0 };
                let place = bodies.position.get(i).copied().unwrap_or_default();
                ball_inputs(thing, bodies, i, world.camera.to_screen(place), time, beats)
            }
            None => Inputs { time: time as f32, ..Inputs::default() },
        };
        colour_at(&b.colour, &inputs)
    }
}

impl Cx<'_> {
    /// Each living ball of `thing` that may paint: its ribbon grows from its last point to where
    /// it is now, in short smooth pieces, or ends when the ball stops painting or dies.
    fn strokes(&mut self, st: &mut CanvasState, thing: ThingId, b: &MarksBrushes) {
        let Some(bodies) = self.world.bodies.get(&thing) else { return };
        let endless = b.paint_load <= 0.0;
        let tune = Tune {
            taper: b.paint_taper.max(0.0) * 40.0,
            follow: if b.paint_width_by == WidthBy::Speed { 1.0 - b.paint_smooth } else { 1.0 },
        };
        for i in 0..bodies.len() {
            let key = (thing, i as u32);
            let hits = bodies.hits[i];
            let mut ribbon = st.ribbons.remove(&key).unwrap_or_else(|| Ribbon::new(b.paint_load, hits));
            // It ends where the ball is, its tail no longer than the ball is wide.
            let end = (to_local(self.pose, bodies.position[i]), bodies.radius[i]);
            if !bodies.alive[i] {
                let pieces = ribbon.finish(end.0, end.1, &tune, endless);
                self.ends(st, b, key, &ribbon, &pieces);
                continue;
            }
            let speed = bodies.velocity[i].length();
            // The load refills only on a hit, which also bends the trail.
            if hits > ribbon.hits {
                ribbon.load = b.paint_load;
                ribbon.corner = ribbon.point.is_some();
                ribbon.hits = hits;
            }
            let paints = match b.paint_when {
                When::Always => true,
                When::AfterFirstHit => hits > 0,
                When::FasterThan => speed > b.paint_min_speed,
            } && (endless || ribbon.load > 0.0);
            if !paints {
                // It starts afresh wherever it next paints, with the paint it has left.
                let pieces = ribbon.finish(end.0, end.1, &tune, endless);
                self.ends(st, b, key, &ribbon, &pieces);
                st.ribbons.insert(key, ribbon);
                continue;
            }
            let here = end.0;
            let velocity = Vec2::from_angle(-self.pose.angle).rotate(bodies.velocity[i]);
            let radius = match b.paint_width_by {
                WidthBy::Size => b.paint_width * bodies.radius[i],
                WidthBy::Fixed => b.paint_width * 10.0 * 0.5,
                WidthBy::Speed => b.paint_width * speed / 100.0 * 0.5,
            };
            match ribbon.point {
                None => {
                    ribbon.begin(here, velocity, radius);
                    ribbon.colour = Some(self.colours.of(b, thing, i));
                }
                // Shorter moves wait and add up into a later piece.
                Some(from) if (here - from).length() < MIN_STROKE => {}
                Some(_) => {
                    // The colour blends from the last point's to this one's along the move.
                    let now = self.colours.of(b, thing, i);
                    let was = ribbon.colour.unwrap_or(now);
                    let pieces = ribbon.advance(here, velocity, radius, &tune, endless);
                    self.pieces(st, b, key, &pieces, [was, now]);
                    ribbon.colour = Some(now);
                }
            }
            st.ribbons.insert(key, ribbon);
        }
    }

    /// Logs and stamps the closing `pieces` of `ribbon`, the trail of ball `(thing, item)`, in
    /// the colour it had at its last point.
    fn ends(&mut self, st: &mut CanvasState, b: &MarksBrushes, key: (ThingId, u32), ribbon: &Ribbon, pieces: &[Piece]) {
        if pieces.is_empty() {
            return;
        }
        let colour = ribbon.colour.unwrap_or_else(|| self.colours.of(b, key.0, key.1 as usize));
        self.pieces(st, b, key, pieces, [colour, colour]);
    }

    /// Logs and stamps `pieces` of a trail, its colour blending from `was` to `now` over the
    /// move (by each piece's [`Piece::within`]).
    fn pieces(
        &mut self,
        st: &mut CanvasState,
        b: &MarksBrushes,
        key: (ThingId, u32),
        pieces: &[Piece],
        [was, now]: [[f32; 4]; 2],
    ) {
        let (tick, by) = ((self.world.step / st.tick_every) as u32, painter(key.0, key.1));
        let at = |t: f32| -> [f32; 4] {
            if t >= 1.0 {
                now
            } else if t <= 0.0 {
                was
            } else {
                std::array::from_fn(|c| was[c] + (now[c] - was[c]) * t)
            }
        };
        for p in pieces {
            if !reaches(st.size, p.from, p.to, p.radius.max(p.radius_to)) {
                continue;
            }
            let (colour, colour_to) = (at(p.within[0]), at(p.within[1]));
            let mark = Mark {
                step: self.world.step,
                target: MarkTarget::Canvas(self.canvas),
                shape: MarkShape::Stroke,
                from: p.from,
                to: p.to,
                radius: p.radius,
                radius_to: p.radius_to,
                angle: p.joins_at,
                within: p.within,
                // A stroke's variant: bit 0 is its tip (0 round, 1 square), bits 1 and 2 its joints.
                variant: (b.paint_tip == Tip::Square) as u8 | p.joints,
                colour,
                colour_to,
            };
            st.grid.stamp(&mark, tick, by);
            self.out.push(mark);
        }
    }

    /// A splat at each of this step's hits that `thing` made or took, fast enough to count, and
    /// meant for canvases (see [`SplatOnto`](crate::SplatOnto)).
    fn splats(&mut self, st: &mut CanvasState, thing: ThingId, b: &MarksBrushes) {
        let (world, log) = (self.world, self.log);
        for e in log.since(world.step) {
            let ours = e.a.thing == thing || e.b.is_some_and(|p| p.thing == thing);
            if e.kind != EventKind::Hit || !ours || e.speed < b.splat_min_speed {
                continue;
            }
            // Lands on the thing hit: only a hit with no skin to paint (a ball, or a thing that
            // takes no paint) splats the canvases, as if the brush said Canvases.
            if !b.splat_onto.canvases() && e.b.is_some_and(|p| world.skins.contains_key(&p.thing)) {
                continue;
            }
            let at = to_local(self.pose, e.point);
            let Stamp { shape, radius, angle, variant } = hit_stamp(world, e, b, self.pose.angle);
            if !reaches(st.size, at, at, radius) {
                continue;
            }
            // The mover's colour, whoever owns the brush.
            let colour = self.colours.of(b, e.a.thing, e.a.item as usize);
            let mark = Mark {
                step: world.step,
                target: MarkTarget::Canvas(self.canvas),
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
            st.grid.stamp(&mark, (world.step / st.tick_every) as u32, painter(e.a.thing, e.a.item));
            self.out.push(mark);
        }
    }
}

/// The stamp brush `b` leaves for hit `e` in a frame turned by `pose_angle`.
pub(crate) struct Stamp {
    pub shape: MarkShape,
    pub radius: f32,
    pub angle: f32,
    pub variant: u8,
}

/// The stamp brush `b` leaves for hit `e`, in the frame of a thing turned by `pose_angle`: its
/// shape, size, turn and variant. Only a splat leans with the ball and squashes; the other
/// shapes face the hit.
pub(crate) fn hit_stamp(world: &World, e: &Event, b: &MarksBrushes, pose_angle: f32) -> Stamp {
    if b.splat_shape == SplatShape::Picture && !b.splat_picture.trim().is_empty() {
        let mover = world.bodies.get(&e.a.thing).and_then(|bodies| bodies.velocity.get(e.a.item as usize)).copied();
        let (radius, random_angle, _) =
            splat_look(e, mover, b.splat_size, b.splat_by_speed, b.splat_variants, b.splat_turn, pose_angle);
        // A facing picture stands with its up (-y) along the hit normal, away from the surface.
        let angle = match b.splat_turn {
            Turn::FaceHit => {
                let n = e.normal.normalize_or_zero();
                n.x.atan2(-n.y) - pose_angle
            }
            Turn::Random => random_angle,
        };
        let tint = if b.splat_picture_colours == PictureColours::Tinted { crate::pictures::TINTED } else { 0 };
        let variant = crate::pictures::slot_of(world, &b.splat_picture) | tint;
        return Stamp { shape: MarkShape::Picture, radius, angle, variant };
    }
    let shape = match b.splat_shape {
        SplatShape::Splat | SplatShape::Picture => MarkShape::Splat,
        SplatShape::Ring => MarkShape::Ring,
        SplatShape::Star => MarkShape::Star,
        SplatShape::Crack => MarkShape::Crack,
        SplatShape::Burst => MarkShape::Burst,
        SplatShape::Dots => MarkShape::Dots,
    };
    let mover = world.bodies.get(&e.a.thing).and_then(|bodies| bodies.velocity.get(e.a.item as usize)).copied();
    let (radius, angle, variant) =
        splat_look(e, mover, b.splat_size, b.splat_by_speed, b.splat_variants, b.splat_turn, pose_angle);
    let (angle, variant) = if shape == MarkShape::Splat {
        (angle, variant)
    } else {
        let angle = match b.splat_turn {
            Turn::FaceHit => e.normal.y.atan2(e.normal.x) - pose_angle,
            Turn::Random => angle,
        };
        (angle, variant & 7)
    };
    Stamp { shape, radius, angle, variant }
}

/// How fast a hit squashes its splat at least (level 1 at 1000 px/s) and at most (level 3).
const SQUASH_FROM: f32 = 600.0;
const SQUASH_STEP: f32 = 400.0;

/// The size, turn and look of the splat a hit leaves, in the owning thing's frame (`pose_angle`
/// is its turn). The turn of a Face-hit splat is the ball's direction of travel: the mover's
/// velocity (already bounced by now) reflected about the hit normal, or against the normal when
/// the mover has no body. The variant is the layout (low 3 bits, one of `variants`) and the
/// squash level 0 to 3 (bits 3 and 4: 0 under 600 px/s, 3 from 1800 px/s). A Random turn picks
/// its own turn from the hit. Skins reuse this.
pub fn splat_look(
    e: &Event,
    mover_velocity: Option<Vec2>,
    size: f32,
    by_speed: f32,
    variants: u32,
    turn: Turn,
    pose_angle: f32,
) -> (f32, f32, u8) {
    let radius = size * (1.0 + by_speed * (e.speed / 1000.0).min(2.0));
    let h = mix(e.step, e.a.item);
    let angle = match turn {
        Turn::FaceHit => {
            let n = e.normal.normalize_or_zero();
            let travel = match mover_velocity.filter(|v| v.length_squared() > 1e-6) {
                Some(v) if n != Vec2::ZERO => v - 2.0 * v.dot(n) * n,
                Some(v) => v,
                None => -n,
            };
            travel.y.atan2(travel.x) - pose_angle
        }
        Turn::Random => (h >> 40) as f32 / (1u64 << 24) as f32 * TAU,
    };
    let squash = ((e.speed - SQUASH_FROM) / SQUASH_STEP).floor().clamp(0.0, 3.0) as u8;
    let variant = (h % variants.max(1) as u64) as u8 | squash << 3;
    (radius, angle, variant)
}

/// A well-mixed 64-bit number from a step and an item (SplitMix64's finaliser).
pub(crate) fn mix(step: u64, item: u32) -> u64 {
    let mut z = (step << 32 ^ item as u64).wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use bpe_core::{Attach, Color, Params, Resolved, Scene, SlotId, Target, Value, resolve};
    use bpe_runtime::{EventKind, MarkShape, MarkTarget, Playback};

    use super::*;
    use crate::testing::{ARENA, BALLS, OBSTACLE, module_of, run, scene_with_obstacle};
    use crate::{Kit, add_thing, fill_slot, kit, new_scene};

    /// The canvas the tests add after the arena and the ball.
    const CANVAS: ThingId = ThingId(3);

    /// Gives `thing` a Marks module and applies `f` to its settings.
    fn brushes(scene: &mut Scene, kit: &Kit, thing: ThingId, f: impl FnOnce(&mut Params)) {
        fill_slot(scene, &kit.registry, thing, SlotId::Marks).unwrap();
        let m = module_of(scene, &kit.registry, thing, SlotId::Marks);
        f(&mut scene.library.modules.get_mut(&m).unwrap().params);
    }

    /// Takes the brushes a new canvas gave the ball away again.
    fn bare_ball(scene: &mut Scene) {
        scene.things.get_mut(&BALLS).unwrap().slots.remove(&SlotId::Marks);
    }

    /// The new scene with a canvas fitting the arena and, when `brush` says so, a Marks module
    /// on the ball, with `f`'s changes.
    fn painted(brush: bool, f: impl FnOnce(&mut Params)) -> (Scene, Resolved, Kit) {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        assert_eq!(add_thing(&mut scene, &kit.registry, "canvas", ThingId::ROOT).unwrap(), CANVAS);
        if brush {
            brushes(&mut scene, &kit, BALLS, f);
        } else {
            bare_ball(&mut scene);
        }
        let resolved = resolve(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        (scene, resolved, kit)
    }

    fn canvas_state(world: &World, canvas: ThingId) -> &CanvasState {
        state::<CanvasState>(world.states[&canvas].as_ref())
    }

    /// `p` in the canvas's own pixels.
    fn local(pose: Pose, p: Vec2) -> Vec2 {
        Vec2::from_angle(-pose.angle).rotate(p - pose.position)
    }

    #[test]
    fn cells_remember_the_painting_ball() {
        let (scene, resolved, kit) = painted(true, |_| {});
        let mut pb = Playback::new(&scene, &resolved, kit.sims.clone());
        pb.seek(1.0);
        let grid = &canvas_state(pb.world(), CANVAS).grid;
        let (w, h) = grid.dims();
        let want = painter(BALLS, 0);
        let (mut mine, mut anonymous) = (false, false);
        for y in 0..h {
            for x in 0..w {
                let c = grid.get(x, y);
                if c.amount == 0 {
                    continue;
                }
                mine |= c.by == want && c.amount >= 96;
                anonymous |= c.by == 0;
            }
        }
        assert!(mine && !anonymous);
    }

    #[test]
    fn look_keys_never_restart_the_canvas() {
        let sim = CanvasSim;
        for k in ["light_angle", "gloss", "grain", "edge_dark", "glow_size", "glow_strength", "style"] {
            assert!(!sim.affects_physics(k), "{k}");
        }
        for k in ["dry_time", "bleed", "drips", "fit"] {
            assert!(sim.affects_physics(k), "{k}");
        }
    }

    #[test]
    fn canvas_kind_is_registered() {
        let kit = kit();
        let k = kit.registry.thing("canvas").expect("canvas kind");
        assert_eq!((k.name(), k.icon()), ("Canvas", "canvas"));
        assert!(k.slots().is_empty() && !k.sticks_to_screen() && !k.can_stick_to_screen());
        // A detail that could move during the video would restart the canvas's GPU layer.
        let detail = super::CanvasSettings::defs().iter().find(|d| d.key == "detail").unwrap();
        assert!(!detail.flags.keyframe && !detail.flags.pulse);
        let sim = kit.sims.thing("canvas").expect("canvas sim");
        assert!(sim.needs_walls());
        for key in ["layer", "style", "detail", "fade", "blend", "opacity", "start_colour"] {
            assert!(!sim.affects_physics(key), "{key}");
        }
        for key in ["kind", "fit", "size", "brushes_from", "starts_as"] {
            assert!(sim.affects_physics(key), "{key}");
        }
        for kind in ["arena", "obstacle", "emitter", "balls"] {
            let k = kit.registry.thing(kind).unwrap();
            assert!(k.slots().contains(&SlotId::Marks), "{kind}");
            assert!(kind == "balls" || !k.default_slots().contains(&SlotId::Marks), "{kind}");
        }
    }

    #[test]
    fn canvas_area_follows_the_fit() {
        let (scene, resolved, kit) = painted(false, |_| {});
        let w = World::build(&scene, &resolved, &kit.sims);
        let wall = w.walls[&ARENA].outline;
        let (pose, size) = canvas_area(&w, CANVAS).unwrap();
        let b = crate::things::liquid::sim::bounds(&wall);
        assert_eq!((pose.position, size), ((b.min + b.max) * 0.5, b.size()));

        let mut scene = scene;
        let p = &mut scene.things.get_mut(&CANVAS).unwrap().params;
        p.set("fit", Value::Choice("custom".into()));
        p.set("size", Value::Vec2([300.0, 200.0]));
        scene.things.get_mut(&CANVAS).unwrap().transform.position = Vec2::new(10.0, 20.0);
        let w = World::build(&scene, &resolve(&scene, &kit.registry), &kit.sims);
        let (pose, size) = canvas_area(&w, CANVAS).unwrap();
        assert_eq!((pose.position, size), (Vec2::new(10.0, 20.0), Vec2::new(300.0, 200.0)));
        assert_eq!(canvas_state(&w, CANVAS).grid.dims(), (75, 50));

        scene.things.get_mut(&CANVAS).unwrap().params.set("fit", Value::Choice("view".into()));
        let w = World::build(&scene, &resolve(&scene, &kit.registry), &kit.sims);
        assert_eq!(canvas_area(&w, CANVAS).unwrap().1, w.video);
    }

    /// A keyframed brush Width widens the trail as the video goes on, and moving its keys
    /// restarts the canvas.
    #[test]
    fn keyframed_width_changes_the_stroke() {
        let (mut scene, resolved, kit) = painted(true, |p| {
            p.set("paint_width_by", Value::Choice("fixed".into()));
            p.set("paint_taper", Value::Number(0.0));
        });
        let before = bpe_runtime::fingerprint(&scene, &resolved, &kit.sims);
        let m = module_of(&scene, &kit.registry, BALLS, SlotId::Marks);
        let a = bpe_core::Address { owner: bpe_core::Owner::Module(m), slot: None, key: "paint_width".into() };
        scene.set_key(&kit.registry, &a, 0.0, Value::Number(0.5)).unwrap();
        scene.set_key(&kit.registry, &a, 1.0, Value::Number(3.0)).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        assert_ne!(bpe_runtime::fingerprint(&scene, &resolved, &kit.sims), before);
        let (_, log) = run(&scene, &resolved, &kit, 1.5);
        let marks = log.marks();
        // Fixed: the radius is Width times 5 px.
        assert!((marks[0].radius - 2.5).abs() < 0.2, "first {}", marks[0].radius);
        assert_eq!(marks.last().unwrap().radius, 15.0);
        assert!(marks.windows(2).all(|w| w[1].radius >= w[0].radius), "the width only grows");
    }

    /// A ball whose Look colours are keyframed paints with the colour it has at that moment.
    #[test]
    fn keyframed_ball_colour_changes_the_mark_colour() {
        let (mut scene, _, kit) = painted(true, |_| {});
        let look = module_of(&scene, &kit.registry, BALLS, SlotId::Look);
        let (red, blue) = (Color::rgb(255, 0, 0), Color::rgb(0, 0, 255));
        let a = bpe_core::Address { owner: bpe_core::Owner::Module(look), slot: None, key: "colours".into() };
        scene.set_key(&kit.registry, &a, 0.0, Value::Gradient(bpe_core::Gradient::solid(red))).unwrap();
        scene.set_key(&kit.registry, &a, 1.0, Value::Gradient(bpe_core::Gradient::solid(blue))).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        let (_, log) = run(&scene, &resolved, &kit, 1.5);
        let marks = log.marks();
        let close = |c: [f32; 4], to: Color| c.iter().zip(to.to_linear()).all(|(a, b)| (a - b).abs() < 0.02);
        assert!(close(marks[0].colour, red), "first {:?}", marks[0].colour);
        assert!(close(marks.last().unwrap().colour, blue), "last {:?}", marks.last().unwrap().colour);
    }

    /// The graphics card's private world trims its log as it goes; marks that take a ball's
    /// beat-following colour must still count every beat so far, not just the ones left.
    #[test]
    fn beat_colours_survive_a_trimmed_log() {
        let (red, green, blue) = (Color::rgb(255, 0, 0), Color::rgb(0, 255, 0), Color::rgb(0, 0, 255));
        let (mut scene, _, kit) = painted(true, |_| {});
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("bpm", Value::Number(240.0));
        let look = module_of(&scene, &kit.registry, BALLS, SlotId::Look);
        let params = &mut scene.library.modules.get_mut(&look).unwrap().params;
        let mut colours = bpe_core::Gradient::even(&[red, green, blue]);
        colours.transition = bpe_core::Transition::Steps;
        params.set("colours", Value::Gradient(colours));
        params.set("follow", Value::Choice("beat".into()));
        params.set("hue_per_hit", Value::Number(0.0));
        let resolved = resolve(&scene, &kit.registry);
        let (_, full) = run(&scene, &resolved, &kit, 4.0);

        // The same run, trimmed the way the flow layers trim (with a much smaller limit).
        let mut world = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        let mut marks = Vec::new();
        let end = 4 * world.steps_per_second as u64;
        while world.step < end {
            world.step(&kit.sims, &mut log);
            marks.extend_from_slice(log.marks_between(world.step - 1, world.step));
            log.drop_marks_upto(world.step);
            if log.len() > 4 {
                let recent = log.since(world.step.saturating_sub(1)).to_vec();
                log = Log::default();
                for e in recent {
                    log.push(e);
                }
            }
        }
        let beats = |step| full.events().iter().filter(|e| e.kind == EventKind::Beat && e.step <= step).count();
        assert!(beats(end) >= 15, "{} beats", beats(end));
        let kept = log.events().iter().filter(|e| e.kind == EventKind::Beat).count();
        assert!(kept + 8 < beats(end), "the log was trimmed: {kept} beats kept");
        assert_eq!(marks.len(), full.marks().len());
        // A tick's last piece ends in the ball's colour then (the move blends into it).
        let ends: Vec<_> = full.marks().iter().filter(|m| m.within[1] == 1.0).collect();
        assert!(ends.len() > 100, "{} ticks", ends.len());
        for m in ends {
            let ball = [red, green, blue][beats(m.step) % 3].to_linear();
            assert_eq!(m.colour_to, ball, "the ball's colour at step {}", m.step);
        }
        for (trimmed, m) in marks.iter().zip(full.marks()) {
            assert_eq!((trimmed.colour, trimmed.colour_to), (m.colour, m.colour_to), "at step {}", m.step);
        }
    }

    #[test]
    fn paint_trail_logs_strokes_at_60_hz() {
        let (scene, resolved, kit) = painted(true, |_| {});
        let (w, log) = run(&scene, &resolved, &kit, 1.0);
        let marks = log.marks();
        // Every 60 Hz tick adds its move to the ribbon as one or more short pieces.
        let ticks: std::collections::BTreeSet<u64> = marks.iter().map(|m| m.step).collect();
        let every = canvas_state(&w, CANVAS).every;
        assert!((58..=60).contains(&ticks.len()), "{} ticks", ticks.len());
        assert!(ticks.iter().all(|s| s % every == 0));
        assert!(marks.len() >= ticks.len());
        assert!(marks.iter().all(|m| m.shape == MarkShape::Stroke && m.target == MarkTarget::Canvas(CANVAS)));
        assert!(marks.iter().all(|m| m.radius_to > 0.0 && m.colour[3] > 0.0));
        assert!(canvas_state(&w, CANVAS).grid.covered() > 0.0);
    }

    #[test]
    fn strokes_join_up() {
        let (scene, resolved, kit) = painted(true, |_| {});
        let (_, log) = run(&scene, &resolved, &kit, 2.0);
        let marks = log.marks();
        assert!(marks.len() > 100);
        for pair in marks.windows(2) {
            assert_eq!(pair[1].from, pair[0].to, "at step {}", pair[1].step);
        }
    }

    fn length(m: &Mark) -> f32 {
        (m.to - m.from).length()
    }

    /// A scene whose ball flies at `velocity` with no gravity, painting with `f`'s settings.
    fn flying(velocity: [f32; 2], f: impl FnOnce(&mut Params)) -> (Scene, Resolved, Kit) {
        let (mut scene, _, kit) = painted(true, f);
        let m = module_of(&scene, &kit.registry, BALLS, SlotId::Motion);
        scene.library.modules.get_mut(&m).unwrap().params.set("gravity_scale", Value::Number(0.0));
        let ball = scene.things.get_mut(&BALLS).unwrap();
        ball.transform.position = Vec2::ZERO;
        ball.params.set("velocity", Value::Vec2(velocity));
        let resolved = resolve(&scene, &kit.registry);
        (scene, resolved, kit)
    }

    #[test]
    fn ribbon_pieces_join_exactly() {
        let (scene, resolved, kit) = painted(true, |_| {});
        let (_, log) = run(&scene, &resolved, &kit, 4.0);
        let marks = log.marks();
        assert!(marks.len() > 200, "{} pieces", marks.len());
        for pair in marks.windows(2) {
            // Bit for bit, not nearly.
            assert_eq!(pair[1].from.to_array().map(f32::to_bits), pair[0].to.to_array().map(f32::to_bits));
            // The previous piece leaves its end cap out exactly when this one leaves its start out.
            assert_eq!(pair[0].variant & 4 != 0, pair[1].variant & 2 != 0, "at step {}", pair[1].step);
            assert_eq!(pair[0].radius_to, pair[1].radius, "the width carries on at step {}", pair[1].step);
        }
        assert_eq!(marks[0].variant & 2, 0, "a ribbon starts with a cap");
        let continuing = marks.iter().filter(|m| m.variant & 2 != 0).count();
        assert!(continuing + 10 > marks.len(), "{continuing} of {} continue", marks.len());
    }

    #[test]
    fn ribbon_colour_blends_along_its_pieces() {
        // A colour that follows the speed of a ball falling faster and faster: each piece ends
        // in the colour the next one starts with, so the ribbon never steps between ticks.
        let (scene, resolved, kit) = painted(true, |p| {
            p.set("colour_from", Value::Choice("own".into()));
            p.set("colour_follow", Value::Choice("speed".into()));
            let (red, blue) = (Color::rgb(255, 0, 0), Color::rgb(0, 0, 255));
            p.set("colour_colours", Value::Gradient(bpe_core::Gradient::even(&[red, blue])));
        });
        let (_, log) = run(&scene, &resolved, &kit, 1.0);
        let marks = log.marks();
        assert!(marks.len() > 50, "{} pieces", marks.len());
        let joined: Vec<_> = marks.windows(2).filter(|p| p[1].variant & 2 != 0).collect();
        assert!(joined.len() + 5 > marks.len());
        for pair in &joined {
            assert_eq!(pair[0].colour_to, pair[1].colour, "at step {}", pair[1].step);
        }
        let changing = marks.iter().filter(|m| m.colour_to != m.colour).count();
        assert!(changing * 2 > marks.len(), "only {changing} of {} pieces change colour", marks.len());
    }

    #[test]
    fn ribbon_pieces_are_short() {
        let (scene, resolved, kit) = flying([2000.0, 0.0], |_| {});
        let (_, log) = run(&scene, &resolved, &kit, 1.0);
        let marks = log.marks();
        let longest = marks.iter().map(length).fold(0.0, f32::max);
        assert!(longest <= 6.0, "a piece of {longest} px");
        assert!(marks.iter().map(length).sum::<f32>() > 1500.0, "the ball really flew fast");
        assert!(marks.iter().filter(|m| length(m) > 5.0).count() > 100, "the pieces are cut, not shrunk");
    }

    #[test]
    fn ribbon_bends_at_a_hit() {
        let (scene, resolved, kit) = painted(true, |p| p.set("paint_taper", Value::Number(0.0)));
        let (w, log) = run(&scene, &resolved, &kit, 5.0);
        let (pose, _) = canvas_area(&w, CANVAS).unwrap();
        let radius = w.bodies[&BALLS].radius[0];
        let marks = log.marks();
        let hits: Vec<_> = log.events().iter().filter(|e| e.kind == EventKind::Hit && e.a.thing == BALLS).collect();
        assert!(hits.len() >= 2, "{} hits", hits.len());
        let joints: Vec<usize> = (1..marks.len()).filter(|&k| marks[k].variant & 2 == 0).collect();
        assert!(joints.len() >= 2, "{} corners", joints.len());
        let every = canvas_state(&w, CANVAS).every;
        for k in joints {
            let (before, after) = (&marks[k - 1], &marks[k]);
            assert_eq!(before.to, after.from, "the corner is a joint");
            assert_eq!(before.variant & 4, 0, "both pieces keep their caps at a corner");
            // It is the bounce: a hit of the ball happened here, around then.
            let near = hits.iter().any(|e| {
                e.step.abs_diff(after.step) <= every && (local(pose, e.point) - after.from).length() <= radius + 12.0
            });
            assert!(near, "a corner at {:?} in step {} is not at a hit", after.from, after.step);
            // The path turns there, where it is otherwise smooth.
            let turn = (before.to - before.from).angle_to(after.to - after.from).abs();
            assert!(turn > 0.4, "turns {turn} rad at the corner");
        }
        let smooth = marks.windows(2).filter(|p| p[1].variant & 2 != 0 && length(&p[0]) > 0.1 && length(&p[1]) > 0.1);
        let sharp = smooth.filter(|p| (p[0].to - p[0].from).angle_to(p[1].to - p[1].from).abs() > 0.4).count();
        assert_eq!(sharp, 0, "no turn that sharp inside a smooth stretch");
    }

    #[test]
    fn taper_narrows_the_ends() {
        let (scene, resolved, kit) = painted(true, |p| p.set("paint_taper", Value::Number(0.3)));
        let (w, log) = run(&scene, &resolved, &kit, 1.0);
        let full = w.bodies[&BALLS].radius[0];
        let marks = log.marks();
        assert!(marks[0].radius < 0.2 * full, "{} of {full}", marks[0].radius);
        let mut run_length = 0.0;
        for m in marks {
            let before = run_length;
            run_length += length(m);
            if before < 6.0 {
                assert!(m.radius_to < full, "still narrow {before} px along: {}", m.radius_to);
            }
            if run_length >= 12.0 {
                assert!((m.radius_to - full).abs() < 1e-3, "full by 12 px, not {} of {full}", m.radius_to);
                assert!(marks.iter().skip_while(|n| *n != m).all(|n| n.radius_to <= full + 1e-3));
                break;
            }
        }
        assert!(run_length >= 12.0);
    }

    #[test]
    fn taper_closes_a_ribbon_that_stops() {
        // Thrown straight up, painting only while faster than 150 px/s: the trail stops near the
        // top of the throw and starts again on the way down.
        let (mut scene, _, kit) = painted(true, |p| {
            p.set("paint_when", Value::Choice("faster_than".into()));
            p.set("paint_min_speed", Value::Number(150.0));
            p.set("paint_taper", Value::Number(0.3));
        });
        let ball = scene.things.get_mut(&BALLS).unwrap();
        ball.transform.position = Vec2::ZERO;
        ball.params.set("velocity", Value::Vec2([0.0, -400.0]));
        let resolved = resolve(&scene, &kit.registry);
        let (w, log) = run(&scene, &resolved, &kit, 3.0);
        let full = w.bodies[&BALLS].radius[0];
        let marks = log.marks();
        let end = (0..marks.len() - 1).find(|&k| marks[k + 1].from != marks[k].to).expect("the trail stops once");
        assert!(marks[end].radius_to < 1e-4, "ends at {}", marks[end].radius_to);
        assert_eq!(marks[end].variant & 4, 0, "the last piece has an end cap");
        let tail: f32 =
            (0..=end).rev().take_while(|&k| marks[k].radius_to < full - 1e-3).map(|k| length(&marks[k])).sum();
        assert!((tail - 12.0).abs() < 1.0, "narrows over {tail} px");
    }

    #[test]
    fn taper_closes_a_ribbon_that_runs_out_of_paint() {
        let (scene, resolved, kit) = flying([0.0, -400.0], |p| {
            p.set("paint_load", Value::Number(80.0));
            p.set("paint_taper", Value::Number(0.3));
        });
        let (_, log) = run(&scene, &resolved, &kit, 1.0);
        let marks = log.marks();
        assert!((marks.iter().map(length).sum::<f32>() - 80.0).abs() < 0.01, "exactly the load");
        assert!(marks.last().unwrap().radius_to < 1e-4);
        assert_eq!(marks.last().unwrap().variant & 4, 0);
    }

    #[test]
    fn tail_starts_where_the_ball_ended() {
        // The ball ends 4 px past the last point: the ribbon runs on to it, then narrows away
        // over no more than its radius (8 px), not the whole taper (40 px).
        let tune = Tune { taper: 40.0, follow: 1.0 };
        let mut ribbon = Ribbon::new(0.0, 0);
        ribbon.begin(Vec2::ZERO, Vec2::new(600.0, 0.0), 5.0);
        ribbon.advance(Vec2::new(30.0, 0.0), Vec2::new(600.0, 0.0), 5.0, &tune, true);
        let pieces = ribbon.finish(Vec2::new(34.0, 0.0), 8.0, &tune, true);
        assert_eq!(pieces[0].from, Vec2::new(30.0, 0.0));
        let at = pieces.iter().position(|p| p.from == Vec2::new(34.0, 0.0)).expect("a piece from where it ended");
        let tail: f32 = pieces[at..].iter().map(|p| (p.to - p.from).length()).sum();
        assert!((tail - 8.0).abs() < 1e-3, "a tail of {tail} px");
        assert!((pieces.last().unwrap().to - Vec2::new(42.0, 0.0)).length() < 1e-3);
        assert!(pieces.last().unwrap().radius_to < 1e-6);
        for pair in pieces.windows(2) {
            assert_eq!(pair[0].to, pair[1].from);
        }
        assert!(ribbon.point.is_none());
    }

    #[test]
    fn speed_width_is_smoothed() {
        // Speed 600 px/s, then 1200: the width would jump from 3 px to 6 px.
        let tune = Tune { taper: 0.0, follow: 0.5 };
        let mut ribbon = Ribbon::new(0.0, 0);
        ribbon.begin(Vec2::ZERO, Vec2::new(600.0, 0.0), 3.0);
        ribbon.advance(Vec2::new(10.0, 0.0), Vec2::new(600.0, 0.0), 3.0, &tune, true);
        let pieces = ribbon.advance(Vec2::new(30.0, 0.0), Vec2::new(1200.0, 0.0), 6.0, &tune, true);
        assert!(pieces.len() >= 3);
        assert_eq!(pieces[0].radius, 3.0);
        assert!(pieces[0].radius_to - 3.0 < 1.5, "the first piece moved {}", pieces[0].radius_to - 3.0);
        // The tick as a whole moves half of the way; the next carries on.
        assert!((pieces.last().unwrap().radius_to - 4.5).abs() < 1e-4);
        let next = ribbon.advance(Vec2::new(50.0, 0.0), Vec2::new(1200.0, 0.0), 6.0, &tune, true);
        assert!((next.last().unwrap().radius_to - 5.25).abs() < 1e-4);
    }

    #[test]
    fn a_hit_refill_starts_a_new_ribbon_when_the_paint_had_run_out() {
        let (scene, resolved, kit) = painted(true, |p| p.set("paint_load", Value::Number(60.0)));
        let (_, log) = run(&scene, &resolved, &kit, 6.0);
        let marks = log.marks();
        let gaps = (0..marks.len() - 1).filter(|&k| marks[k + 1].from != marks[k].to).count();
        assert!(gaps >= 1, "the paint ran out and came back");
    }

    fn hit(speed: f32, normal: Vec2) -> bpe_runtime::Event {
        use bpe_runtime::Party;
        bpe_runtime::Event {
            step: 7,
            kind: EventKind::Hit,
            a: Party { thing: BALLS, item: 0 },
            b: None,
            point: Vec2::ZERO,
            normal,
            speed,
        }
    }

    #[test]
    fn a_splat_faces_the_way_the_ball_was_going() {
        // Bounced off a floor (normal up, y down on screen is positive: up is -y), now going up
        // and right; it came in going down and right.
        let e = hit(500.0, Vec2::new(0.0, -1.0));
        let (_, angle, _) = splat_look(&e, Some(Vec2::new(300.0, -400.0)), 30.0, 0.5, 4, Turn::FaceHit, 0.0);
        let want = Vec2::new(300.0, 400.0).to_angle();
        assert!((angle - want).abs() < 1e-4, "{angle} vs {want}");
        // In a turned canvas the angle is relative to it.
        let (_, turned, _) = splat_look(&e, Some(Vec2::new(300.0, -400.0)), 30.0, 0.5, 4, Turn::FaceHit, 0.5);
        assert!((turned - (want - 0.5)).abs() < 1e-4);
        // No body: against the normal.
        let (_, none, _) = splat_look(&e, None, 30.0, 0.5, 4, Turn::FaceHit, 0.0);
        assert!((none - Vec2::new(0.0, 1.0).to_angle()).abs() < 1e-4);
    }

    #[test]
    fn fast_hits_squash_their_splat() {
        let level = |speed: f32| splat_look(&hit(speed, Vec2::Y), None, 30.0, 0.0, 4, Turn::FaceHit, 0.0).2 >> 3;
        assert_eq!(
            [level(100.0), level(599.0), level(1000.0), level(1400.0), level(1800.0), level(5000.0)],
            [0, 0, 1, 2, 3, 3]
        );
        let (_, _, v) = splat_look(&hit(1800.0, Vec2::Y), None, 30.0, 0.0, 4, Turn::FaceHit, 0.0);
        assert!(v & 7 < 4);
    }

    #[test]
    fn splat_at_each_hit() {
        let (scene, resolved, kit) = painted(true, |p| {
            p.set("paint_on", Value::Bool(false));
            p.set("splat_on", Value::Bool(true));
            p.set("splat_onto", Value::Choice("canvases".into()));
            p.set("splat_min_speed", Value::Number(200.0));
        });
        let (w, log) = run(&scene, &resolved, &kit, 6.0);
        let hits: Vec<_> = log
            .events()
            .iter()
            .filter(|e| e.kind == EventKind::Hit && e.a.thing == BALLS && e.speed >= 200.0)
            .collect();
        assert!(hits.len() >= 2, "{} hits", hits.len());
        let marks = log.marks();
        assert_eq!(marks.len(), hits.len());
        let (pose, _) = canvas_area(&w, CANVAS).unwrap();
        for (m, e) in marks.iter().zip(&hits) {
            assert_eq!(m.shape, MarkShape::Splat);
            assert_eq!(m.step, e.step);
            assert_eq!((m.from, m.to), (local(pose, e.point), local(pose, e.point)));
            let radius = 30.0 * (1.0 + 0.5 * (e.speed / 1000.0).min(2.0));
            assert!((m.radius - radius).abs() < 1e-3, "{} vs {radius}", m.radius);
            assert!(m.variant & 7 < 4);
        }
    }

    #[test]
    fn splats_without_a_skin_land_on_canvases() {
        // Lands on the thing hit (the default): a hit with no skin to take it splats the
        // canvases instead, as if it said Canvases.
        let splats = |skin_on: bool, balls: bool| {
            let (mut scene, _, kit) = painted(true, |p| {
                p.set("paint_on", Value::Bool(false));
                p.set("splat_on", Value::Bool(true));
                p.set("splat_min_speed", Value::Number(200.0));
            });
            scene.things.get_mut(&ARENA).unwrap().params.set("skin_on", Value::Bool(skin_on));
            if balls {
                let params = &mut scene.things.get_mut(&BALLS).unwrap().params;
                params.set("arrangement", Value::Choice("block".into()));
                params.set("columns", Value::Int(4));
                params.set("rows", Value::Int(3));
                params.set("velocity", Value::Vec2([700.0, -300.0]));
                let bounce = module_of(&scene, &kit.registry, BALLS, SlotId::Bounce);
                scene.library.modules.get_mut(&bounce).unwrap().params.set("hit_balls", Value::Bool(true));
            }
            let resolved = resolve(&scene, &kit.registry);
            let (_, log) = run(&scene, &resolved, &kit, 6.0);
            let hits = |ball: bool| {
                log.events()
                    .iter()
                    .filter(|e| e.kind == EventKind::Hit && e.a.thing == BALLS && e.speed >= 200.0)
                    .filter(|e| e.b.is_some_and(|b| b.thing == BALLS) == ball)
                    .count()
            };
            let on_canvas = log.marks().iter().filter(|m| m.target == MarkTarget::Canvas(CANVAS)).count();
            let on_skin = log.marks().iter().filter(|m| m.target == MarkTarget::Skin(ARENA)).count();
            (hits(false), hits(true), on_canvas, on_skin)
        };
        // The arena takes no paint: its hits splat the canvas.
        let (walls, _, canvas, skin) = splats(false, false);
        assert!(walls >= 2, "{walls} hits");
        assert_eq!((canvas, skin), (walls, 0));
        // It takes paint: they splat its skin, never the canvas.
        let (walls, _, canvas, skin) = splats(true, false);
        assert_eq!((canvas, skin), (0, walls));
        // Balls hitting balls have no skin: those splat the canvas.
        let (walls, balls, canvas, skin) = splats(true, true);
        assert!(balls >= 2, "{balls} ball hits");
        assert_eq!((canvas, skin), (balls, walls));
    }

    #[test]
    fn riding_canvas_keeps_local_marks() {
        let (mut scene, _, kit) = scene_with_obstacle(|p| {
            p.set("shape", Value::Choice("bar".into()));
            p.set("spin", Value::Number(90.0));
        });
        let canvas = add_thing(&mut scene, &kit.registry, "canvas", OBSTACLE).unwrap();
        bare_ball(&mut scene);
        scene
            .set_attach(canvas, Attach::Ride { offset: Vec2::new(40.0, 0.0), turn_with_parent: true, item: None })
            .unwrap();
        let p = &mut scene.things.get_mut(&canvas).unwrap().params;
        p.set("fit", Value::Choice("custom".into()));
        p.set("size", Value::Vec2([600.0, 600.0]));
        brushes(&mut scene, &kit, OBSTACLE, |p| {
            p.set("paint_on", Value::Bool(false));
            p.set("splat_on", Value::Bool(true));
            p.set("splat_onto", Value::Choice("canvases".into()));
            p.set("splat_min_speed", Value::Number(0.0));
        });
        let resolved = resolve(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        while log.marks().is_empty() && w.step < 240 * 5 {
            w.step(&kit.sims, &mut log);
        }
        let m = log.marks()[0];
        let hit =
            log.since(w.step).iter().find(|e| e.kind == EventKind::Hit && e.b.is_some_and(|b| b.thing == OBSTACLE));
        let hit = hit.expect("the mark comes from a hit on the obstacle");
        let pose = w.poses[&canvas];
        assert!(pose.angle.abs() > 0.1, "the canvas turned with the obstacle: {pose:?}");
        assert_eq!(pose.angle, w.poses[&OBSTACLE].angle);
        assert_eq!(m.from, local(pose, hit.point));
        assert!((m.from - (hit.point - pose.position)).length() > 1.0, "the turn counts");
    }

    #[test]
    fn spawned_ball_starts_without_a_stroke() {
        let (mut scene, _, kit) = painted(true, |_| {});
        let spout = add_thing(&mut scene, &kit.registry, "emitter", ThingId::ROOT).unwrap();
        scene.things.get_mut(&spout).unwrap().transform.position = Vec2::new(0.0, 300.0);
        let p = &mut scene.things.get_mut(&spout).unwrap().params;
        p.set("target", Value::Target(Target::Thing(BALLS)));
        p.set("rate", Value::Number(10.0));
        p.set("spread", Value::Number(0.0));
        let resolved = resolve(&scene, &kit.registry);
        let (w, log) = run(&scene, &resolved, &kit, 1.0);
        let spawn = log.events().iter().find(|e| e.kind == EventKind::Spawn).expect("a spawn");
        assert!(log.marks().iter().all(|m| m.from.length() > 1.0), "no stroke starts at 0,0");
        // The tick that first sees the new ball only notes where it is; the next one strokes.
        let every = canvas_state(&w, CANVAS).every;
        let first_tick = spawn.step.div_ceil(every) * every;
        let at = |step: u64| log.marks().iter().filter(|m| m.step == step).collect::<Vec<_>>();
        // The pieces of one ball's move chain up; a new chain is a new ball.
        let chains = |marks: &[&Mark]| {
            let starts = (0..marks.len()).filter(|&k| k == 0 || marks[k].from != marks[k - 1].to);
            starts.map(|k| marks[k].from).collect::<Vec<_>>()
        };
        assert_eq!(chains(&at(first_tick)).len(), 1, "only the old ball strokes on the first tick");
        let second = chains(&at(first_tick + every));
        assert_eq!(second.len(), 2);
        let speed = w.bodies[&BALLS].velocity[1].length().max(500.0) + 500.0;
        let travel = speed * every as f32 / w.steps_per_second as f32 + 2.0;
        assert!((second[1] - spawn.point).length() <= travel, "{:?} from {:?}", second[1], spawn.point);
    }

    #[test]
    fn a_resting_ball_logs_nothing() {
        let (mut scene, _, kit) = painted(true, |_| {});
        let m = module_of(&scene, &kit.registry, BALLS, SlotId::Motion);
        scene.library.modules.get_mut(&m).unwrap().params.set("gravity_scale", Value::Number(0.0));
        scene.things.get_mut(&BALLS).unwrap().params.set("velocity", Value::Vec2([0.0, 0.0]));
        let resolved = resolve(&scene, &kit.registry);
        let (w, log) = run(&scene, &resolved, &kit, 1.0);
        assert_eq!(w.bodies[&BALLS].velocity[0], Vec2::ZERO);
        assert!(log.marks().is_empty(), "{} marks", log.marks().len());
        assert_eq!(canvas_state(&w, CANVAS).grid.covered(), 0.0);
    }

    #[test]
    fn slow_moments_keep_the_load() {
        // Thrown straight up with 100 px of paint, painting only above 150 px/s: the paint runs
        // out on the way up, and the slow top of the throw gives none back. Only a hit does.
        let (mut scene, _, kit) = painted(true, |p| {
            p.set("paint_load", Value::Number(100.0));
            p.set("paint_when", Value::Choice("faster_than".into()));
            p.set("paint_min_speed", Value::Number(150.0));
        });
        let ball = scene.things.get_mut(&BALLS).unwrap();
        ball.transform.position = Vec2::ZERO;
        ball.params.set("velocity", Value::Vec2([0.0, -400.0]));
        let resolved = resolve(&scene, &kit.registry);
        let (w, log) = run(&scene, &resolved, &kit, 4.0);
        let first_hit = log.events().iter().find(|e| e.kind == EventKind::Hit).expect("it lands").step;
        let before: f32 = log.marks().iter().filter(|m| m.step < first_hit).map(|m| (m.to - m.from).length()).sum();
        let tick = 400.0 * canvas_state(&w, CANVAS).every as f32 / w.steps_per_second as f32;
        assert!(before > 50.0 && before <= 100.0 + tick, "{before} px painted before the first hit");
        assert!(log.marks().iter().any(|m| m.step > first_hit), "the hit refills it");
    }

    #[test]
    fn a_stroke_across_the_canvas_reaches_it() {
        let size = Vec2::new(20.0, 20.0);
        assert!(reaches(size, Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0), 0.0), "both ends outside, crossing");
        assert!(reaches(size, Vec2::new(-100.0, 14.0), Vec2::new(100.0, 14.0), 5.0), "thick enough to touch");
        assert!(!reaches(size, Vec2::new(-100.0, 16.0), Vec2::new(100.0, 16.0), 5.0));
        assert!(!reaches(size, Vec2::new(30.0, -100.0), Vec2::new(100.0, 30.0), 1.0), "passes by the corner");
        assert!(reaches(size, Vec2::new(5.0, 5.0), Vec2::new(5.0, 5.0), 0.0));
        assert!(!reaches(size, Vec2::new(50.0, 5.0), Vec2::new(50.0, 5.0), 10.0));
    }

    #[test]
    fn world_memory_counts_the_grid() {
        let (scene, resolved, kit) = painted(false, |_| {});
        let with = World::build(&scene, &resolved, &kit.sims);
        let bytes = canvas_state(&with, CANVAS).grid.bytes();
        assert!(bytes > 0);
        let mut plain = new_scene(&kit.registry);
        plain.name = scene.name.clone();
        let without = World::build(&plain, &resolve(&plain, &kit.registry), &kit.sims);
        assert!(with.memory() >= without.memory() + bytes, "{} vs {} + {bytes}", with.memory(), without.memory());
    }

    #[test]
    fn no_brushes_no_marks() {
        let (scene, resolved, kit) = painted(false, |_| {});
        let (w, log) = run(&scene, &resolved, &kit, 2.0);
        assert!(log.marks().is_empty());
        assert_eq!(canvas_state(&w, CANVAS).grid.covered(), 0.0);
    }

    #[test]
    fn no_canvas_no_marks() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        brushes(&mut scene, &kit, BALLS, |p| {
            p.set("splat_on", Value::Bool(true));
            p.set("splat_onto", Value::Choice("canvases".into()));
        });
        let resolved = resolve(&scene, &kit.registry);
        let (_, log) = run(&scene, &resolved, &kit, 2.0);
        assert!(log.marks().is_empty());
    }

    #[test]
    fn brushes_from_picks_the_brushes() {
        let (mut scene, _, kit) = painted(true, |_| {});
        scene.things.get_mut(&CANVAS).unwrap().params.set("brushes_from", Value::Target(Target::Thing(ARENA)));
        let resolved = resolve(&scene, &kit.registry);
        let (_, log) = run(&scene, &resolved, &kit, 1.0);
        assert!(log.marks().is_empty());
    }

    #[test]
    fn walls_splat_what_hits_them() {
        let (mut scene, _, kit) = painted(false, |_| {});
        brushes(&mut scene, &kit, ARENA, |p| {
            p.set("paint_on", Value::Bool(false));
            p.set("splat_on", Value::Bool(true));
            p.set("splat_onto", Value::Choice("canvases".into()));
            p.set("splat_min_speed", Value::Number(200.0));
        });
        let resolved = resolve(&scene, &kit.registry);
        let (_, log) = run(&scene, &resolved, &kit, 6.0);
        let hits = log.events().iter().filter(|e| e.kind == EventKind::Hit && e.speed >= 200.0).count();
        assert!(hits >= 2);
        assert_eq!(log.marks().len(), hits);
    }

    #[test]
    fn an_obstacle_splats_in_the_colour_of_a_ball_without_marks() {
        let (mut scene, _, kit) = scene_with_obstacle(|_| {});
        assert_eq!(add_thing(&mut scene, &kit.registry, "canvas", ThingId::ROOT).unwrap(), ThingId(4));
        bare_ball(&mut scene);
        brushes(&mut scene, &kit, OBSTACLE, |p| {
            p.set("paint_on", Value::Bool(false));
            p.set("splat_on", Value::Bool(true));
            p.set("splat_onto", Value::Choice("canvases".into()));
            p.set("splat_min_speed", Value::Number(1.0));
        });
        let look = module_of(&scene, &kit.registry, BALLS, SlotId::Look);
        let red = bpe_core::Color::rgb(255, 0, 0);
        scene
            .library
            .modules
            .get_mut(&look)
            .unwrap()
            .params
            .set("colours", Value::Gradient(bpe_core::Gradient::even(&[red, red])));
        let resolved = resolve(&scene, &kit.registry);
        assert!(!resolved.things[&BALLS].slots.contains_key(&SlotId::Marks), "the ball has no brushes");
        let (_, log) = run(&scene, &resolved, &kit, 4.0);
        let marks = log.marks();
        assert!(!marks.is_empty(), "the obstacle was hit");
        for m in marks {
            assert!(
                m.colour[0] > 0.99 && m.colour[1] < 0.01 && m.colour[2] < 0.01,
                "{:?}: the ball's red, not white",
                m.colour
            );
        }
    }

    /// A ball that paints and splats, for the determinism tests.
    fn busy() -> (Scene, Resolved, Kit) {
        painted(true, |p| {
            p.set("splat_on", Value::Bool(true));
            p.set("splat_onto", Value::Choice("canvases".into()));
            p.set("splat_turn", Value::Choice("random".into()));
        })
    }

    #[test]
    fn same_run_same_grid() {
        let (scene, resolved, kit) = busy();
        let mut a = Playback::new(&scene, &resolved, kit.sims.clone());
        let mut b = Playback::new(&scene, &resolved, kit.sims.clone());
        a.seek(5.0);
        b.seek(5.0);
        let (ga, gb) = (&canvas_state(a.world(), CANVAS).grid, &canvas_state(b.world(), CANVAS).grid);
        assert!(ga.covered() > 0.0);
        assert_eq!(ga.hash(), gb.hash());
        assert!(!a.log().marks().is_empty());
        assert_eq!(a.log().marks(), b.log().marks());
    }

    #[test]
    fn seek_equals_straight() {
        let (scene, resolved, kit) = busy();
        let mut straight = Playback::new(&scene, &resolved, kit.sims.clone());
        straight.seek(5.0);
        let mut jumpy = Playback::new(&scene, &resolved, kit.sims.clone());
        jumpy.seek(8.0);
        jumpy.seek(5.0);
        assert_eq!(canvas_state(straight.world(), CANVAS).grid.hash(), canvas_state(jumpy.world(), CANVAS).grid.hash());
        assert_eq!(straight.log().marks(), jumpy.log().marks());
        assert_eq!(straight.log().hash(), jumpy.log().hash());
    }
}
