//! The canvas in the simulation: stamping brush strokes and hit splats into its grid and logging
//! every mark, so the graphics card can redraw them sharp.
//!
//! Everything here runs in the Events stage, after the physics, and never changes it: the grid
//! and the marks stay out of the log's hash.

use std::collections::BTreeMap;
use std::f32::consts::TAU;

use bpe_core::{Color, ResolvedThing, Settings, SlotId, Thing, ThingId};
use bpe_runtime::{EventKind, Log, Mark, MarkShape, MarkTarget, Pose, ThingSim, World, state};
use glam::Vec2;

use super::grid::{CanvasGrid, Cell};
use super::{CanvasSettings, Fit};
use crate::modules::look::SimLook;
use crate::modules::look::draw::ball_inputs;
use crate::modules::marks::{MarkColour, MarksBrushes, SplatShape, Tip, Turn, When, WidthBy};
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
    /// Per brush thing and item: the last point a stroke reached (canvas pixels; `None` while
    /// it isn't painting), the paint left in its load, and its hits when that was noted.
    pub last: BTreeMap<(ThingId, u32), (Option<Vec2>, f32, u32)>,
    /// Steps between strokes: 60 strokes a second.
    pub every: u64,
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
        let grid = CanvasGrid::new(size);
        world.state_memory.insert(thing.id, grid.bytes());
        world
            .states
            .insert(thing.id, Box::new(CanvasState { settings, grid, size, centre, last: BTreeMap::new(), every }));
        self.place(thing.id, world);
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
        !matches!(key, "layer" | "style" | "detail" | "fade" | "blend" | "opacity" | "start_colour")
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
    /// The looks of the balls marked this step, read once each (`None` when unknown).
    looks: BTreeMap<ThingId, Option<SimLook>>,
    /// The marks made so far, in order.
    out: Vec<Mark>,
}

/// This step's marks on `canvas`, already stamped into its grid: every matching brush's strokes
/// (60 times a second) and splats, brushes in the world's order.
fn marks(canvas: ThingId, st: &mut CanvasState, world: &World, log: &Log) -> Vec<Mark> {
    let pose = world.poses.get(&canvas).copied().unwrap_or_default();
    let mut cx = Cx { canvas, pose, world, log, looks: BTreeMap::new(), out: Vec::new() };
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
        if b.splat_on {
            cx.splats(st, thing, b);
        }
    }
    cx.out
}

/// `p` in canvas pixels: from the canvas's middle, before the canvas turns.
fn to_local(pose: Pose, p: Vec2) -> Vec2 {
    Vec2::from_angle(-pose.angle).rotate(p - pose.position)
}

/// Whether a capsule of `radius` around the segment `a`..`b` (canvas pixels) may reach onto a
/// canvas of `size`: the segment against the canvas grown by the radius.
fn reaches(size: Vec2, a: Vec2, b: Vec2, radius: f32) -> bool {
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

/// A grid cell fully painted in a linear-light colour.
fn cell_of(colour: [f32; 4]) -> Cell {
    let c = Color::from_linear(colour);
    Cell { rgb: [c.r, c.g, c.b], amount: 255, ..Cell::default() }
}

impl Cx<'_> {
    /// The colour ball `i` of `thing` marks with under brush `b`: its own colour when the brush
    /// says Ball and the ball's look is known, else the brush's own Colours.
    fn colour(&mut self, b: &MarksBrushes, thing: ThingId, i: usize) -> [f32; 4] {
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

    /// Each living ball of `thing` that may paint strokes from its last point to where it is now.
    fn strokes(&mut self, st: &mut CanvasState, thing: ThingId, b: &MarksBrushes) {
        let Some(bodies) = self.world.bodies.get(&thing) else { return };
        let endless = b.paint_load <= 0.0;
        for i in 0..bodies.len() {
            let key = (thing, i as u32);
            if !bodies.alive[i] {
                st.last.remove(&key);
                continue;
            }
            let speed = bodies.velocity[i].length();
            let hits = bodies.hits[i];
            let (point, load, seen) = st.last.get(&key).copied().unwrap_or((None, b.paint_load, hits));
            // The load refills only on a hit.
            let mut load = if hits > seen { b.paint_load } else { load };
            let paints = match b.paint_when {
                When::Always => true,
                When::AfterFirstHit => hits > 0,
                When::FasterThan => speed > b.paint_min_speed,
            };
            if !paints {
                // It starts afresh wherever it next paints, with the paint it has left.
                st.last.insert(key, (None, load, hits));
                continue;
            }
            let here = to_local(self.pose, bodies.position[i]);
            let Some(from) = point else {
                st.last.insert(key, (Some(here), load, hits));
                continue;
            };
            let length = (here - from).length();
            if length < MIN_STROKE {
                st.last.insert(key, (Some(from), load, hits));
                continue;
            }
            let radius = match b.paint_width_by {
                WidthBy::Size => b.paint_width * bodies.radius[i],
                WidthBy::Fixed => b.paint_width * 10.0 * 0.5,
                WidthBy::Speed => b.paint_width * speed / 100.0 * 0.5,
            };
            if (endless || load > 0.0) && reaches(st.size, from, here, radius) {
                let colour = self.colour(b, thing, i);
                st.grid.stamp_capsule(from, here, radius, cell_of(colour));
                self.out.push(Mark {
                    step: self.world.step,
                    target: MarkTarget::Canvas(self.canvas),
                    shape: MarkShape::Stroke,
                    from,
                    to: here,
                    radius,
                    radius_to: radius,
                    angle: 0.0,
                    // A stroke's variant is its tip: 0 round, 1 square.
                    variant: (b.paint_tip == Tip::Square) as u8,
                    colour,
                    colour_to: colour,
                });
                if !endless {
                    load = (load - length).max(0.0);
                }
            }
            st.last.insert(key, (Some(here), load, hits));
        }
    }

    /// A splat at each of this step's hits that `thing` made or took, fast enough to count.
    fn splats(&mut self, st: &mut CanvasState, thing: ThingId, b: &MarksBrushes) {
        let shape = match b.splat_shape {
            SplatShape::Splat => MarkShape::Splat,
            SplatShape::Ring => MarkShape::Ring,
            SplatShape::Star => MarkShape::Star,
            SplatShape::Crack => MarkShape::Crack,
            SplatShape::Burst => MarkShape::Burst,
            SplatShape::Dots => MarkShape::Dots,
        };
        let (world, log) = (self.world, self.log);
        for e in log.since(world.step) {
            let ours = e.a.thing == thing || e.b.is_some_and(|p| p.thing == thing);
            if e.kind != EventKind::Hit || !ours || e.speed < b.splat_min_speed {
                continue;
            }
            let at = to_local(self.pose, e.point);
            let radius = b.splat_size * (1.0 + b.splat_by_speed * (e.speed / 1000.0).min(2.0));
            if !reaches(st.size, at, at, radius) {
                continue;
            }
            let h = mix(e.step, e.a.item);
            let angle = match b.splat_turn {
                Turn::FaceHit => e.normal.y.atan2(e.normal.x) - self.pose.angle,
                Turn::Random => (h >> 40) as f32 / (1u64 << 24) as f32 * TAU,
            };
            let variant = (h % b.splat_variants.max(1) as u64) as u8;
            // The mover's colour, whoever owns the brush.
            let colour = self.colour(b, e.a.thing, e.a.item as usize);
            st.grid.stamp_disc(at, radius, cell_of(colour));
            self.out.push(Mark {
                step: world.step,
                target: MarkTarget::Canvas(self.canvas),
                shape,
                from: at,
                to: at,
                radius,
                radius_to: radius,
                angle,
                variant,
                colour,
                colour_to: colour,
            });
        }
    }
}

/// A well-mixed 64-bit number from a step and an item (SplitMix64's finaliser).
fn mix(step: u64, item: u32) -> u64 {
    let mut z = (step << 32 ^ item as u64).wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use bpe_core::{Attach, Params, Resolved, Scene, SlotId, Target, Value, resolve};
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
        let (mut scene, resolved, kit) = painted(true, |p| p.set("paint_width_by", Value::Choice("fixed".into())));
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
        for (trimmed, m) in marks.iter().zip(full.marks()) {
            let ball = [red, green, blue][beats(m.step) % 3].to_linear();
            assert_eq!(m.colour, ball, "the ball's colour at step {}", m.step);
            assert_eq!(trimmed.colour, m.colour, "at step {}", m.step);
        }
    }

    #[test]
    fn paint_trail_logs_strokes_at_60_hz() {
        let (scene, resolved, kit) = painted(true, |_| {});
        let (w, log) = run(&scene, &resolved, &kit, 1.0);
        let marks = log.marks();
        assert!((59..=61).contains(&marks.len()), "{} marks", marks.len());
        assert!(marks.iter().all(|m| m.shape == MarkShape::Stroke && m.target == MarkTarget::Canvas(CANVAS)));
        assert!(marks.iter().all(|m| m.radius > 0.0 && m.colour[3] > 0.0));
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

    #[test]
    fn splat_at_each_hit() {
        let (scene, resolved, kit) = painted(true, |p| {
            p.set("paint_on", Value::Bool(false));
            p.set("splat_on", Value::Bool(true));
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
            assert!(m.variant < 4);
        }
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
        assert_eq!(at(first_tick).len(), 1, "only the old ball strokes on the first tick");
        let second = at(first_tick + every);
        assert_eq!(second.len(), 2);
        let speed = w.bodies[&BALLS].velocity[1].length().max(500.0) + 500.0;
        let travel = speed * every as f32 / w.steps_per_second as f32 + 2.0;
        assert!((second[1].from - spawn.point).length() <= travel, "{:?} from {:?}", second[1].from, spawn.point);
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
        brushes(&mut scene, &kit, BALLS, |p| p.set("splat_on", Value::Bool(true)));
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
