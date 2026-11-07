//! The light probe (lighting part 7): the direct light on a point, a ball or a thing, worked out
//! on the CPU from the very lights a frame hands the renderer, with analytic shadows from balls
//! and shapes. No bounce, light field or haze. It never reads itself: the scene it resolves for
//! the lights is animated with no light sensing in, and nothing that makes light reads it back.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use bpe_core::{Animation, Inputs, LightSource, Resolved, Scene, Settings, SlotId, ThingId, Value};
use bpe_gfx::{DrawCx, LightGpu, LightScene, ThingDraw};
use bpe_runtime::{BallLight, LightAt, LightPull, Log, SoundAt, World, level_of};
use glam::Vec2;

use crate::modules::lighting::{LIGHTING, LightingSettings};
use crate::modules::look::layers::gives_light::{Emit, GivesLight, SHAPE_SUB_LIGHT, SUB_LIGHT};
use crate::things::light::draw::{LightDraw, outline_points, places, point_round, shown_settings};
use crate::things::light::{LIGHT, LightSettings};

/// How many points round a shape's outline "Light on" a shape averages.
const SHAPE_POINTS: usize = 16;
/// The most grid cells across the balls' box, each way.
const MOST_CELLS: usize = 256;
/// In Hard and Toon shadows, the penumbra's half-width in world units (the shader's half a pixel).
const HARD_EDGE: f32 = 0.5;
/// A "Shadow" over this counts as in shadow for In shadow.
const IN_SHADOW: f32 = 0.5;

/// The things whose settings the probe reads besides the lights: their looks block or give light.
pub const LOOKED: [&str; 3] = ["balls", "emitter", "shape"];
/// The thing kinds whose own settings the probe reads.
pub const LIGHTING_KINDS: [&str; 2] = [LIGHT, crate::things::rig::RIG];

/// A scene's light probe.
pub struct Probe {
    scene: Arc<Scene>,
    /// The settings it reads, before any keys: the lights, rigs, the scene and the looks.
    resolved: Resolved,
    animation: Animation,
    /// What the scene's lights follow of its sound, when they do.
    sound: Option<Arc<dyn SoundAt>>,
    /// The lamps at the moments last asked (asked many times a step: once a ball), newest last.
    /// A few, so worlds of several runs asking in turn (test flights on threads) don't thrash.
    cache: Mutex<Vec<(Moment, Arc<Lamps>)>>,
}

/// How many moments the probe keeps.
const KEPT_MOMENTS: usize = 8;

/// A moment the probe is asked at: the world's run and step, the time and how much has been
/// logged.
#[derive(Clone, Copy, PartialEq)]
struct Moment {
    run: u64,
    step: u64,
    time: u64,
    logged: usize,
    balls: usize,
}

impl Probe {
    pub fn new(scene: &Scene, resolved: &Resolved, sound: Option<Arc<dyn SoundAt>>) -> Probe {
        let reads = |r: &bpe_core::ResolvedThing| {
            r.id == ThingId::ROOT || LIGHTING_KINDS.contains(&r.kind.as_str()) || LOOKED.contains(&r.kind.as_str())
        };
        let mut kept = Resolved { things: BTreeMap::new(), problems: Vec::new() };
        for (id, r) in &resolved.things {
            if !reads(r) {
                continue;
            }
            let mut r = r.clone();
            if LOOKED.contains(&r.kind.as_str()) {
                r.slots.retain(|slot, _| *slot == SlotId::Look);
            } else if r.id == ThingId::ROOT {
                r.slots.retain(|slot, _| *slot == SlotId::Lighting);
            }
            kept.things.insert(*id, r);
        }
        let mut animation = Animation::new(scene, &kept);
        animation.retain(|thing, slot, _| match kept.things.get(&thing) {
            Some(_) if thing == ThingId::ROOT => slot == Some(SlotId::Lighting),
            Some(r) if LOOKED.contains(&r.kind.as_str()) => slot == Some(SlotId::Look),
            Some(_) => true,
            None => false,
        });
        Probe { scene: Arc::new(scene.clone()), resolved: kept, animation, sound, cache: Mutex::new(Vec::new()) }
    }

    /// The lamps and blockers at `time`, with `world` and `log` as they are.
    fn lamps(&self, world: &World, log: &Log, time: f64) -> Arc<Lamps> {
        let balls = world.bodies.values().map(|b| b.len()).sum();
        let moment = Moment { run: world.run.0, step: world.step, time: time.to_bits(), logged: log.len(), balls };
        if let Ok(cache) = self.cache.lock()
            && let Some((_, lamps)) = cache.iter().rev().find(|(at, _)| *at == moment)
        {
            return lamps.clone();
        }
        let lamps = Arc::new(self.work_out(world, log, time));
        if let Ok(mut cache) = self.cache.lock() {
            if cache.len() >= KEPT_MOMENTS {
                cache.remove(0);
            }
            cache.push((moment, lamps.clone()));
        }
        lamps
    }

    fn work_out(&self, world: &World, log: &Log, time: f64) -> Lamps {
        let mut now = self.resolved.clone();
        if !self.animation.is_empty() {
            let levels = self.sound.as_deref().map(|sound| bpe_runtime::LevelsAt { sound, world, log });
            // No light sensing in: the probe never reads itself.
            let inputs = Inputs {
                hits: bpe_gfx::hit_strength(log, time, world.steps_per_second),
                sound: levels.as_ref().map(|l| l as &dyn bpe_core::SoundLevels),
                light: None,
                health: world.health_levels(),
            };
            self.animation.apply(&mut now, time, &inputs);
        }
        // What the rules changed, as the frame builder applies it.
        if let Some(rules) = world.rules().filter(|r| r.changes_picture()) {
            rules.apply_whole(&mut now, time, |_| true);
            // Every broken thing is gone at once here, even while its Break look still plays out
            // in the picture: the probe is also what the physics senses light with, and break
            // looks are drawn only.
            for id in rules.broken() {
                now.things.remove(&id);
            }
        }
        // Gradients' companions, as the frame builder applies them (light colours can be
        // gradients).
        if bpe_core::gradient_extras::any_in(&now) {
            bpe_core::gradient_extras::apply_moves(&mut now, time);
        }
        Lamps::of(&self.scene, &self.resolved, &now, world, log, time)
    }

    /// The light on body row `row` of `thing`.
    fn ball_sample(&self, lamps: &Lamps, world: &World, thing: ThingId, row: usize) -> Option<Sample> {
        let b = world.bodies.get(&thing)?;
        if !b.alive.get(row).copied().unwrap_or(false) {
            return None;
        }
        // Worked out once a moment: Follow light, the spreads, Moths and every Light modulator
        // ask about the same balls.
        let at = b.position[row];
        if let Ok(memo) = lamps.balls.lock()
            && let Some((was, s)) = memo.get(&(thing, row))
            && *was == at
        {
            return Some(*s);
        }
        let me = Some((thing, row as u32));
        let s = lamps.sample(at, Skip { ball: me, thing: None }, me);
        if let Ok(mut memo) = lamps.balls.lock() {
            memo.insert((thing, row), (at, s));
        }
        Some(s)
    }

    /// The light on every point `thing` is sensed at: its alive balls, round a shape's outline,
    /// or its place.
    fn samples(&self, lamps: &Lamps, world: &World, thing: ThingId) -> Vec<Sample> {
        if let Some(b) = world.bodies.get(&thing) {
            return (0..b.len()).filter_map(|row| self.ball_sample(lamps, world, thing, row)).collect();
        }
        if let Some(wall) = world.walls.get(&thing) {
            let points = outline_points(&wall.outline);
            return (0..SHAPE_POINTS)
                .map(|k| point_round(&points, k as f32 / SHAPE_POINTS as f32))
                .map(|p| lamps.sample(p, Skip { ball: None, thing: Some(thing) }, None))
                .collect();
        }
        let at = lamps.places.get(&thing).copied().or_else(|| world.poses.get(&thing).map(|p| p.position));
        at.map(|p| vec![lamps.sample(p, Skip::default(), None)]).unwrap_or_default()
    }
}

impl LightAt for Probe {
    fn level(&self, world: &World, log: &Log, thing: ThingId, source: &LightSource, time: f64) -> f32 {
        let lamps = self.lamps(world, log, time);
        if *source == LightSource::Strength {
            return lamps.strength.get(&thing).copied().unwrap_or(0.0);
        }
        let samples = self.samples(&lamps, world, thing);
        if samples.is_empty() {
            return 0.0;
        }
        let n = samples.len() as f32;
        let v = match source {
            LightSource::On => samples.iter().map(|s| level_of(s.light)).sum::<f32>() / n,
            LightSource::Brightest => samples.iter().map(|s| level_of(s.brightest)).fold(0.0, f32::max),
            LightSource::Shadow => samples.iter().map(Sample::shadow).sum::<f32>() / n,
            LightSource::InShadow => samples.iter().filter(|s| s.shadow() > IN_SHADOW).count() as f32 / n,
            LightSource::Strength => 0.0,
        };
        if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }
    }

    fn ball(&self, world: &World, log: &Log, thing: ThingId, row: usize, time: f64) -> BallLight {
        let lamps = self.lamps(world, log, time);
        self.ball_sample(&lamps, world, thing, row)
            .map(|s| BallLight { light: level_of(s.light), shadow: s.shadow() })
            .unwrap_or_default()
    }

    fn pull(&self, world: &World, log: &Log, thing: ThingId, row: usize, time: f64, pull: &LightPull) -> Vec2 {
        let Some(b) = world.bodies.get(&thing) else { return Vec2::ZERO };
        let Some(&p) = b.position.get(row) else { return Vec2::ZERO };
        let lamps = self.lamps(world, log, time);
        let me = Some((thing, row as u32));
        let mut sum = Vec2::ZERO;
        for lamp in &lamps.lamps {
            // A ball's own glow, or a light riding it, never pulls it.
            if lamp.from.is_some() && lamp.from == me {
                continue;
            }
            let Some(l) = lamps.one(lamp, p, Skip { ball: me, thing: None }) else { continue };
            let amount = if pull.shadows { l.lit } else { l.open };
            if amount == 0.0 || !amount.is_finite() {
                continue;
            }
            let soft = if l.dist.is_finite() && pull.near > 0.0 {
                let d2 = l.dist * l.dist;
                d2 / (d2 + pull.near * pull.near)
            } else {
                1.0
            };
            sum += l.toward * amount * soft;
        }
        let v = sum * pull.strength;
        if v.is_finite() { v } else { Vec2::ZERO }
    }
}

/// What to leave out of a shadow ray: the ball being asked about, or a shape's own outline.
#[derive(Clone, Copy, Default)]
struct Skip {
    ball: Option<(ThingId, u32)>,
    thing: Option<ThingId>,
}

/// The light at one point: with ambient and every lamp's shadows, every lamp without shadows,
/// and the brightest single lamp (each the luminance of the light, in the shader's units).
#[derive(Clone, Copy, Debug, Default)]
struct Sample {
    light: f32,
    lamps_lit: f32,
    lamps_open: f32,
    brightest: f32,
}

impl Sample {
    /// How much of the lamps' light the shadows take, 0 to 1 (0 where no lamp reaches).
    fn shadow(&self) -> f32 {
        if self.lamps_open > 1e-6 { (1.0 - self.lamps_lit / self.lamps_open).clamp(0.0, 1.0) } else { 0.0 }
    }
}

/// One lamp's light at a point.
struct One {
    /// With its shadows and without (luminance).
    lit: f32,
    open: f32,
    /// The way to the light, and how far it is (a sun's: infinite).
    toward: Vec2,
    dist: f32,
}

/// One lamp: a Light thing's (one per place it shines from) or a glowing ball's or shape's.
struct Lamp {
    gpu: LightGpu,
    /// The ball it rides or glows on: its light doesn't fall on, shadow or pull that ball.
    from: Option<(ThingId, u32)>,
    /// The shape it glows along: that shape doesn't shadow it.
    shape: Option<ThingId>,
}

/// Everything the light at a moment is made of.
struct Lamps {
    lamps: Vec<Lamp>,
    segments: Vec<[f32; 4]>,
    /// Ambient light's luminance.
    ambient: f32,
    /// Shadow style (0 soft, 1 hard, 2 toon, 3 off), the card's softness and length, toon steps.
    style: f32,
    softness: f32,
    length: f32,
    toon: f32,
    blockers: Blockers,
    /// Each Light thing's strength now over its own (Its strength), 0 to 1.
    strength: BTreeMap<ThingId, f32>,
    /// Where each Light thing shines from first (Light on a light).
    places: BTreeMap<ThingId, Vec2>,
    /// The light on each ball asked about so far at this moment.
    balls: Mutex<std::collections::HashMap<(ThingId, usize), (Vec2, Sample)>>,
}

impl Lamps {
    fn of(scene: &Scene, base: &Resolved, now: &Resolved, world: &World, log: &Log, time: f64) -> Lamps {
        let card = now.things.get(&ThingId::ROOT).and_then(|r| r.slots.get(&SlotId::Lighting));
        let Some(card) = card.filter(|c| c.kind == LIGHTING) else {
            // No Lighting card: the picture is unlit, as if lit evenly at 1 everywhere.
            return Lamps {
                lamps: Vec::new(),
                segments: Vec::new(),
                ambient: 1.0,
                style: 3.0,
                softness: 0.0,
                length: 0.0,
                toon: 1.0,
                blockers: Blockers::default(),
                strength: BTreeMap::new(),
                places: BTreeMap::new(),
                balls: Mutex::default(),
            };
        };
        let settings = LightingSettings::from_params(&card.params).gpu(None);
        let mut scene_lights = LightScene { settings, ..Default::default() };
        let mut lamps = Vec::new();
        let mut strength = BTreeMap::new();
        let mut places_of = BTreeMap::new();
        for (id, r) in &now.things {
            if r.kind != LIGHT {
                continue;
            }
            let Some(thing) = scene.things.get(id) else { continue };
            let cx = DrawCx { thing, resolved: r, scene: now, world, log, time, tree: scene, light: None };
            let first = scene_lights.lights.len();
            LightDraw.light(&cx, &mut scene_lights);
            // One lamp per place, in the order the light pushed them.
            let shown = shown_settings(&cx, &crate::modules::light_scenes::now(&cx));
            let spots = places(&cx, &shown);
            // Its own Strength as set, before keys: a keyed fade shows in Its strength too.
            let own = LightSettings::from_params(&base.things.get(id).unwrap_or(r).params);
            let own_light = own.strength * luminance(own.linear_colour());
            let mut most: f32 = 0.0;
            for (k, gpu) in scene_lights.lights[first..].iter().enumerate() {
                let from = spots.get(k).and_then(|p| p.ball);
                lamps.push(Lamp { gpu: *gpu, from, shape: None });
                let now = luminance([gpu.p1[0], gpu.p1[1], gpu.p1[2]]);
                if own_light.abs() > 1e-9 {
                    most = most.max(now / own_light);
                }
            }
            strength.insert(*id, if most.is_finite() { most.clamp(0.0, 1.0) } else { 0.0 });
            if let Some(p) = spots.first() {
                places_of.insert(*id, p.at);
            }
        }
        let mut segments = scene_lights.segments;
        let reach = settings.glow[1].max(0.0);
        let mut blockers = Blockers::default();
        for (id, r) in &now.things {
            if !LOOKED.contains(&r.kind.as_str()) {
                continue;
            }
            let Some(look) = r.slots.get(&SlotId::Look) else { continue };
            let look = LookLight::of(look, settings.glass_colour());
            if let Some(b) = world.bodies.get(id) {
                for row in 0..b.len() {
                    if !b.alive[row] {
                        continue;
                    }
                    let number = b.number.get(row).copied().unwrap_or(row as u32);
                    if look.blocks {
                        blockers.balls.push(Ball {
                            at: b.position[row],
                            radius: b.radius[row],
                            from: (*id, row as u32),
                            passes: look.passes,
                        });
                    }
                    let glow = look.glow_of(number);
                    if luminance(glow).abs() > 1e-9 {
                        lamps.push(Lamp {
                            gpu: glow_lamp(b.position[row], b.radius[row], glow, reach),
                            from: Some((*id, row as u32)),
                            shape: None,
                        });
                    }
                }
            } else if let Some(wall) = world.walls.get(id) {
                let points = outline_points(&wall.outline);
                let n = points.len();
                if n < 2 {
                    continue;
                }
                let sides: Vec<[Vec2; 2]> = (0..n).map(|k| [points[k], points[(k + 1) % n]]).collect();
                let glow = look.glow_of(0);
                if luminance(glow).abs() > 1e-9 {
                    let first = segments.len() as f32;
                    segments.extend(sides.iter().map(|s| [s[0].x, s[0].y, s[1].x, s[1].y]));
                    let mut gpu = glow_lamp(wall.outline.center(), 0.0, glow, reach);
                    gpu.p0[3] = 4.0;
                    gpu.p4 = [0.0, 0.0, first, sides.len() as f32];
                    lamps.push(Lamp { gpu, from: None, shape: Some(*id) });
                }
                if look.blocks {
                    blockers.walls.push((*id, sides, look.passes, wall.solid));
                }
            }
        }
        blockers.build_grid();
        Lamps {
            lamps,
            segments,
            ambient: luminance([settings.ambient[0], settings.ambient[1], settings.ambient[2]]),
            style: settings.shadows[0],
            softness: settings.shadows[1].max(0.0),
            length: settings.shadows[2].max(0.0),
            toon: settings.more[1].max(1.0),
            blockers,
            strength,
            places: places_of,
            balls: Mutex::default(),
        }
    }

    /// The light at `p`, leaving `skip` out of the shadows and the lamps from `me` out.
    fn sample(&self, p: Vec2, skip: Skip, me: Option<(ThingId, u32)>) -> Sample {
        let mut s = Sample::default();
        for lamp in &self.lamps {
            if lamp.from.is_some() && lamp.from == me && lamp_is_glow(lamp) {
                // A glowing ball's own light doesn't fall on it (it shows its own colours).
                continue;
            }
            if let Some(one) = self.one(lamp, p, skip) {
                s.lamps_lit += one.lit;
                s.lamps_open += one.open;
                s.brightest = s.brightest.max(one.lit);
            }
        }
        s.light = self.ambient + s.lamps_lit;
        s
    }

    /// Lamp `lamp`'s light at `p`, as `lighting.wgsl::lamps` works it out on a flat surface.
    fn one(&self, lamp: &Lamp, p: Vec2, skip: Skip) -> Option<One> {
        let l = &lamp.gpu;
        let kind = l.p0[3];
        let (mut amount, toward, dist, angular, from);
        if kind > 1.5 && kind < 2.5 {
            // Sun: from far away, against its direction.
            amount = 1.0;
            toward = -Vec2::new(l.p3[0], l.p3[1]);
            dist = f32::INFINITY;
            angular = l.p6[1].max(0.0);
            from = None;
        } else {
            let at = self.source_point(l, p);
            let d = at - p;
            dist = d.length();
            amount = falloff(dist, l.p2[0], l.p2[1]);
            if amount == 0.0 {
                return None;
            }
            if kind > 0.5 && kind < 1.5 {
                let away = if dist > 1e-3 { -d / dist } else { Vec2::ZERO };
                let cosine = if dist <= 1e-3 { 1.0 } else { away.dot(Vec2::new(l.p3[0], l.p3[1])) };
                let e = ((cosine - l.p3[3]) / (l.p3[2] - l.p3[3]).max(1e-6)).clamp(0.0, 1.0);
                amount *= e * e * (3.0 - 2.0 * e);
                if amount == 0.0 {
                    return None;
                }
            }
            toward = if dist > 1e-3 { d / dist } else { Vec2::ZERO };
            angular = l.p2[2].max(0.0) / dist.max(1e-3);
            from = Some(at);
        }
        let mut tint = [1.0f32; 3];
        if l.p5[3] > 0.5 && self.style < 2.5 && dist > 1e-3 {
            let soft = if self.style > 0.5 {
                HARD_EDGE / dist.clamp(1e-3, 1e6)
            } else {
                angular * self.softness * l.p6[3].max(0.0)
            };
            let stop = l.p6[0] + l.p2[2].max(0.0);
            let v = self.blockers.visibility(p, toward, dist, soft, self.length, stop, skip, lamp, from);
            for ((t, c), v) in tint.iter_mut().zip(&l.p5[..3]).zip(v) {
                let k = ((1.0 - v) * l.p2[3]).clamp(0.0, 1.0);
                *t = 1.0 + (c - 1.0) * k;
            }
        }
        if self.style > 1.5 && self.style < 2.5 {
            amount = (amount * self.toon + 0.5).floor() / self.toon;
        }
        if l.p7[1] > 0.5 {
            // An ultraviolet light: only its share of ordinary light is seen.
            amount *= l.p7[2].max(0.0);
        }
        let rgb = [l.p1[0] * amount, l.p1[1] * amount, l.p1[2] * amount];
        let lit = luminance([rgb[0] * tint[0], rgb[1] * tint[1], rgb[2] * tint[2]]);
        Some(One { lit, open: luminance(rgb), toward, dist })
    }

    /// Where lamp `l`'s light comes from for `p`: the lamp, or the nearest point of an area or
    /// a strip (`lighting.wgsl::source_point`).
    fn source_point(&self, l: &LightGpu, p: Vec2) -> Vec2 {
        let kind = l.p0[3];
        let at = Vec2::new(l.p0[0], l.p0[1]);
        if kind > 2.5 && kind < 3.5 {
            let half = 0.5 * Vec2::new(l.p4[0].max(0.0), l.p4[1].max(0.0));
            let dir = Vec2::new(l.p3[0], l.p3[1]);
            let side = Vec2::new(-dir.y, dir.x);
            let d = p - at;
            let local = Vec2::new(d.dot(dir), d.dot(side));
            let mut q = local.clamp(-half, half);
            if l.p6[2] > 0.5 {
                let r = local / half.max(Vec2::splat(1e-3));
                q = if r.dot(r) > 1.0 { local / r.length() } else { local };
            }
            return at + dir * q.x + side * q.y;
        }
        if kind > 3.5 {
            let first = l.p4[2].max(0.0) as usize;
            let n = l.p4[3].max(0.0) as usize;
            let mut best = at;
            let mut best_d = f32::INFINITY;
            for s in self.segments.iter().skip(first).take(n) {
                let q = nearest_on(p, Vec2::new(s[0], s[1]), Vec2::new(s[2], s[3]));
                let d = p.distance(q);
                if d < best_d {
                    best_d = d;
                    best = q;
                }
            }
            return best;
        }
        at
    }
}

/// Whether `lamp` is a glowing ball's own light (not a Light thing riding it).
fn lamp_is_glow(lamp: &Lamp) -> bool {
    lamp.gpu.p11[3] > 0.5
}

/// A glowing ball's (or shape's) light as a Point lamp of `colour` (linear, times strength).
fn glow_lamp(at: Vec2, radius: f32, colour: [f32; 3], reach: f32) -> LightGpu {
    LightGpu {
        p0: [at.x, at.y, 0.0, 0.0],
        p1: [colour[0], colour[1], colour[2], 0.0],
        p2: [reach, 0.0, radius.max(0.0), 1.0],
        p3: [1.0, 0.0, 1.0, 1.0],
        p4: [0.0; 4],
        p5: [0.0, 0.0, 0.0, 1.0],
        p6: [radius.max(0.0), 0.0, 0.0, 1.0],
        // p11.w (spare on the GPU) marks it as a glow.
        p11: [0.0, 0.0, 0.0, 1.0],
        ..LightGpu::ZERO
    }
}

/// The around layers (trails, particles, wavefronts, arcs, auras): what they give off is left out.
const AROUND_KINDS: [&str; 6] = [
    crate::modules::look::layers::TRAIL,
    crate::modules::look::layers::particles::PARTICLES,
    crate::modules::look::layers::particles::SHAPE_PARTICLES,
    crate::modules::look::layers::wavefront::WAVEFRONT,
    crate::modules::look::layers::arcs::ARCS,
    crate::modules::look::layers::aura::AURA,
];

/// What a thing's look says about light: whether it blocks it, and what it gives off.
struct LookLight {
    blocks: bool,
    /// What it lets through of each channel where it is in the way (glass and light part 1): its
    /// See-through share, coloured by its Body (a shape's first Colour and gradient layer) when
    /// light through glass takes its colour; 0 for a solid thing.
    passes: [f32; 3],
    /// Each shown Gives off light: the light (linear, times strength) and the balls whose layer
    /// it is on (empty: every ball).
    glows: Vec<([f32; 3], Vec<u32>)>,
}

impl LookLight {
    fn of(look: &bpe_core::ResolvedSlot, glass_colour: bool) -> LookLight {
        let mut blocks = false;
        let mut glows = Vec::new();
        let passes = passes_of(look, glass_colour);
        for layer in &look.layers {
            let params = look.layer_params(layer.id);
            if layer.parent.is_none()
                && matches!(params.get(crate::modules::look::layers::lighting::BLOCKS_LIGHT), Some(Value::Bool(true)))
            {
                blocks = true;
            }
            if layer.kind != SUB_LIGHT && layer.kind != SHAPE_SUB_LIGHT {
                continue;
            }
            let Some(parent) = layer.parent.and_then(|p| look.layers.iter().find(|l| l.id == p)) else { continue };
            if AROUND_KINDS.contains(&parent.kind.as_str()) {
                // Trails, particles and the like are many small or spread-out sources: left out.
                continue;
            }
            let e = Emit::of(&GivesLight::from_params(&params));
            let c = layer_colour(&look.layer_params(parent.id));
            let rgb = [e.own[0] + e.tint[0] * c[0], e.own[1] + e.tint[1] * c[1], e.own[2] + e.tint[2] * c[2]];
            glows.push((rgb, parent.only.clone()));
        }
        LookLight { blocks, passes, glows }
    }

    /// The light ball `number` gives off.
    fn glow_of(&self, number: u32) -> [f32; 3] {
        let mut out = [0.0f32; 3];
        for (rgb, only) in &self.glows {
            if only.is_empty() || only.binary_search(&number).is_ok() {
                for (o, c) in out.iter_mut().zip(rgb) {
                    *o += c;
                }
            }
        }
        out
    }
}

/// What a thing with Look card `look` lets through of each channel where it is in the way: its
/// See-through share (less where its Body is see-through too: `1 - (1 - see-through) x opacity`),
/// times, when `glass_colour`, its colour's hue at full brightness by its Tints light.
fn passes_of(look: &bpe_core::ResolvedSlot, glass_colour: bool) -> [f32; 3] {
    use crate::modules::look::surface;
    if !surface::has_surface(&look.kind) {
        return [0.0; 3];
    }
    let card = surface::of_card(look);
    let see = surface::passes(&card);
    if see <= 0.0 {
        return [0.0; 3];
    }
    let body = look.layers.first().map(|l| look.layer_params(l.id)).unwrap_or_default();
    let opacity = match body.get("opacity") {
        Some(Value::Number(o)) if o.is_finite() => (*o as f32).clamp(0.0, 1.0),
        _ => 1.0,
    };
    let share = 1.0 - (1.0 - see) * opacity;
    if !glass_colour {
        return [share; 3];
    }
    // A shape's colour is its first Colour and gradient layer's, else its Body's.
    let paint = look.layers.iter().find(|l| l.parent.is_none() && l.kind == crate::modules::look::shape::PAINT);
    let colour = layer_colour(&paint.map(|l| look.layer_params(l.id)).unwrap_or(body));
    let most = colour[0].max(colour[1]).max(colour[2]);
    let tints = if card.tints.is_finite() { card.tints.clamp(0.0, 1.0) } else { 0.0 } * opacity;
    colour.map(|c| {
        let hue = if most > 1e-4 { (c / most).clamp(0.0, 1.0) } else { 1.0 };
        share * (1.0 + (hue - 1.0) * tints)
    })
}

/// A layer's own colour, linear: the middle of its colours, its colour, else white.
pub(crate) fn layer_colour(params: &bpe_core::Params) -> [f32; 3] {
    let lin = |c: &bpe_core::Color| {
        let l = c.to_linear();
        [l[0], l[1], l[2]]
    };
    match params.get("colours").or_else(|| params.get("colour")) {
        // A moving gradient: the average of its moved colours.
        Some(Value::Gradient(g)) if !g.moves.is_still() => {
            let baked = g.bake(16);
            let mut sum = [0.0f32; 3];
            let n = baked.len() as f32;
            for c in &baked {
                for (o, c) in sum.iter_mut().zip(c) {
                    *o += c / n;
                }
            }
            sum
        }
        Some(Value::Gradient(g)) if !g.stops.is_empty() => {
            let n = g.stops.len() as f32;
            let mut sum = [0.0f32; 3];
            for s in &g.stops {
                let c = lin(&s.color);
                for (o, c) in sum.iter_mut().zip(c) {
                    *o += c / n;
                }
            }
            sum
        }
        Some(Value::Color(c)) => lin(c),
        _ => [1.0; 3],
    }
}

/// One ball that blocks light, and what it lets through of each channel (0 solid).
#[derive(Clone, Copy)]
struct Ball {
    at: Vec2,
    radius: f32,
    from: (ThingId, u32),
    passes: [f32; 3],
}

/// What blocks light: balls in a uniform grid, shapes' outlines as segments, each with what it
/// lets through of each channel (0 solid).
/// A shape that blocks light: its thing, its outline's sides, what it lets through of each channel
/// and whether it is filled.
type Wall = (ThingId, Vec<[Vec2; 2]>, [f32; 3], bool);

#[derive(Default)]
struct Blockers {
    balls: Vec<Ball>,
    walls: Vec<Wall>,
    grid: Option<Grid>,
}

/// The balls by grid cell.
struct Grid {
    origin: Vec2,
    cell: f32,
    size: (usize, usize),
    /// Cell `k`'s balls are `items[starts[k]..starts[k + 1]]`.
    starts: Vec<u32>,
    items: Vec<u32>,
}

impl Blockers {
    fn build_grid(&mut self) {
        if self.balls.len() < 16 {
            return;
        }
        let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
        let mut radii: Vec<f32> = Vec::with_capacity(self.balls.len());
        for b in &self.balls {
            lo = lo.min(b.at - b.radius);
            hi = hi.max(b.at + b.radius);
            radii.push(b.radius);
        }
        if !(lo.is_finite() && hi.is_finite()) {
            return;
        }
        radii.sort_by(f32::total_cmp);
        let span = (hi - lo).max(Vec2::splat(1.0));
        let cell = (2.0 * radii[radii.len() / 2]).max(span.x.max(span.y) / MOST_CELLS as f32).max(1.0);
        let size = (((span.x / cell).ceil() as usize).max(1), ((span.y / cell).ceil() as usize).max(1));
        let cell_of = |p: Vec2| -> (usize, usize) {
            let c = (p - lo) / cell;
            ((c.x.max(0.0) as usize).min(size.0 - 1), (c.y.max(0.0) as usize).min(size.1 - 1))
        };
        let mut counts = vec![0u32; size.0 * size.1 + 1];
        let each = |b: &Ball, f: &mut dyn FnMut(usize)| {
            let (a, z) = (cell_of(b.at - b.radius), cell_of(b.at + b.radius));
            for y in a.1..=z.1 {
                for x in a.0..=z.0 {
                    f(y * size.0 + x);
                }
            }
        };
        for b in &self.balls {
            each(b, &mut |k| counts[k + 1] += 1);
        }
        for k in 1..counts.len() {
            counts[k] += counts[k - 1];
        }
        let mut fill = counts.clone();
        let mut items = vec![0u32; *counts.last().unwrap_or(&0) as usize];
        for (i, b) in self.balls.iter().enumerate() {
            each(b, &mut |k| {
                items[fill[k] as usize] = i as u32;
                fill[k] += 1;
            });
        }
        self.grid = Some(Grid { origin: lo, cell, size, starts: counts, items });
    }

    /// How much of the light gets from the lamp to `p` along `toward` (1 all, 0 none) of each
    /// channel, the shader's `visibility` worked out against circles and segments: a blocker
    /// farther than `length` from `p`, or within `stop` of the lamp, casts nothing on it. A
    /// see-through ball in the way lets through what it passes (once, though the grid may meet it
    /// twice), a see-through shape what it passes (a filled one once, a band each time crossed).
    #[allow(clippy::too_many_arguments)]
    fn visibility(
        &self,
        p: Vec2,
        toward: Vec2,
        dist: f32,
        soft: f32,
        length: f32,
        stop: f32,
        skip: Skip,
        lamp: &Lamp,
        source: Option<Vec2>,
    ) -> [f32; 3] {
        let end = (dist - stop).min(length);
        if end <= 0.0 {
            return [1.0; 3];
        }
        let ang = soft.max(1e-4);
        let mut v: f32 = 1.0;
        let mut through = [1.0f32; 3];
        let smooth = |v: f32| v * v * (3.0 - 2.0 * v);
        // The see-through balls met so far (the grid can give a ball twice): each lets its share
        // through once.
        let mut met: Vec<(ThingId, u32)> = Vec::new();
        let mut test = |b: &Ball| {
            if Some(b.from) == skip.ball || Some(b.from) == lamp.from {
                return;
            }
            let rel = b.at - p;
            // A ball the point is inside doesn't shadow it (the shader leaves the blocker first).
            if rel.length_squared() <= b.radius * b.radius {
                return;
            }
            let along = rel.dot(toward);
            if along <= 0.0 || along - b.radius >= end {
                return;
            }
            let miss = (rel - toward * along).length();
            let d = miss - b.radius;
            let vb = (0.5 + 0.5 * d / (along.max(1.0) * ang)).clamp(0.0, 1.0);
            if b.passes.iter().all(|c| *c <= 0.0) {
                v = v.min(vb);
            } else {
                if met.contains(&b.from) {
                    return;
                }
                met.push(b.from);
                let s = smooth(vb);
                for (t, c) in through.iter_mut().zip(b.passes) {
                    *t *= c + (1.0 - c) * s;
                }
            }
        };
        match &self.grid {
            Some(grid) => grid.walk(p, toward, end, ang, |i| test(&self.balls[i as usize])),
            None => self.balls.iter().for_each(&mut test),
        }
        if v > 0.0 {
            let far = match source {
                Some(_) => p + toward * end,
                None => p + toward * end.min(1e6),
            };
            for (id, sides, passes, solid) in &self.walls {
                if Some(*id) == skip.thing || Some(*id) == lamp.shape {
                    continue;
                }
                let crossed = sides.iter().filter(|s| crosses(p, far, s[0], s[1])).count();
                if crossed == 0 {
                    continue;
                }
                if passes.iter().all(|c| *c <= 0.0) {
                    v = 0.0;
                    break;
                }
                // A filled shape is passed through once going in and out; a band each time.
                let times = if *solid { crossed.div_ceil(2) } else { crossed };
                for (t, c) in through.iter_mut().zip(passes) {
                    *t *= c.powi(times as i32);
                }
            }
        }
        let shown = smooth(v);
        through.map(|t| shown * t)
    }
}

impl Grid {
    /// Calls `f` with every ball in the cells within `ang × t` (the penumbra, `t` how far along)
    /// of the ray from `p` along `dir` for `len` (a ball can come more than once).
    fn walk(&self, p: Vec2, dir: Vec2, len: f32, ang: f32, mut f: impl FnMut(u32)) {
        let span = Vec2::new(self.size.0 as f32, self.size.1 as f32) * self.cell;
        let widest = (len * ang).min(span.x.max(span.y));
        let (lo, hi) = (self.origin - widest, self.origin + span + widest);
        // Clip the ray to the grid's box, grown by the widest penumbra.
        let (mut t0, mut t1) = (0.0f32, len.min(1e7));
        for axis in 0..2 {
            let (o, d) = (p[axis], dir[axis]);
            if d.abs() < 1e-12 {
                if o < lo[axis] || o > hi[axis] {
                    return;
                }
            } else {
                let (a, b) = ((lo[axis] - o) / d, (hi[axis] - o) / d);
                t0 = t0.max(a.min(b));
                t1 = t1.min(a.max(b));
            }
        }
        if t0 > t1 {
            return;
        }
        let cell_of = |q: Vec2| -> (i64, i64) {
            let c = (q - self.origin) / self.cell;
            (c.x.floor() as i64, c.y.floor() as i64)
        };
        let (w, h) = (self.size.0 as i64, self.size.1 as i64);
        let step = 0.5 * self.cell;
        // The last box visited (none yet: an empty one).
        let mut last = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
        let mut t = t0;
        loop {
            let at = p + dir * t;
            let reach = Vec2::splat(t.max(0.0) * ang + step);
            let (a, z) = (cell_of(at - reach), cell_of(at + reach));
            let (ax, ay, zx, zy) = (a.0.max(0), a.1.max(0), z.0.min(w - 1), z.1.min(h - 1));
            if (ax, ay, zx, zy) != last && ax <= zx && ay <= zy {
                // Each edge of the box moves one way only along the ray, so a cell is in the
                // boxes for one run of steps: only the cells new since the last box are visited.
                let (px, py, qx, qy) = last;
                let mut visit = |x: i64, y: i64| {
                    let k = (y * w + x) as usize;
                    for &i in &self.items[self.starts[k] as usize..self.starts[k + 1] as usize] {
                        f(i);
                    }
                };
                for y in ay..=zy {
                    if y < py || y > qy {
                        (ax..=zx).for_each(|x| visit(x, y));
                    } else {
                        (ax..=zx.min(px - 1)).for_each(|x| visit(x, y));
                        ((qx + 1).max(ax)..=zx).for_each(|x| visit(x, y));
                    }
                }
                last = (ax, ay, zx, zy);
            }
            if t >= t1 {
                return;
            }
            t = (t + step).min(t1);
        }
    }
}

/// How a light fades with distance `d` against its reach (`lighting.wgsl::falloff`).
fn falloff(d: f32, reach: f32, kind: f32) -> f32 {
    if reach <= 0.0 {
        return 0.0;
    }
    let x = d / reach;
    if x >= 1.0 {
        return 0.0;
    }
    if kind < 0.5 {
        let w = 1.0 - x * x * x * x;
        return w * w / (1.0 + 16.0 * x * x);
    }
    if kind < 1.5 {
        let w = 1.0 - x * x;
        return w * w;
    }
    if kind < 2.5 {
        return 1.0 - x;
    }
    1.0
}

/// The nearest point to `p` of segment `a`-`b`.
fn nearest_on(p: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    a + ab * t
}

/// Whether segment `p`-`q` crosses segment `a`-`b`.
fn crosses(p: Vec2, q: Vec2, a: Vec2, b: Vec2) -> bool {
    let d1 = (q - p).perp_dot(a - p);
    let d2 = (q - p).perp_dot(b - p);
    let d3 = (b - a).perp_dot(p - a);
    let d4 = (b - a).perp_dot(q - a);
    (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0)
}

/// A linear colour's brightness.
pub fn luminance(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The grid finds the same shadows as trying every ball.
    #[test]
    fn the_grid_casts_the_same_shadows_as_every_ball() {
        let mut rng = 12345u32;
        let mut next = || {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            rng as f32 / u32::MAX as f32
        };
        let mut b = Blockers::default();
        for i in 0..400 {
            let at = Vec2::new(next() * 1000.0 - 500.0, next() * 1600.0 - 800.0);
            let passes = if i % 3 == 0 { [0.5, 0.2, 0.1] } else { [0.0; 3] };
            b.balls.push(Ball { at, radius: 4.0 + next() * 30.0, from: (ThingId(2), i), passes });
        }
        let lamp = Lamp { gpu: glow_lamp(Vec2::ZERO, 0.0, [1.0; 3], 1.0), from: None, shape: None };
        let rays: Vec<(Vec2, Vec2, f32, f32)> = (0..300)
            .map(|_| {
                let p = Vec2::new(next() * 1400.0 - 700.0, next() * 2000.0 - 1000.0);
                let q = Vec2::new(next() * 1400.0 - 700.0, next() * 2000.0 - 1000.0);
                (p, (q - p).normalize(), p.distance(q), 0.002 + next() * 0.05)
            })
            .collect();
        let each: Vec<[f32; 3]> = rays
            .iter()
            .map(|r| b.visibility(r.0, r.1, r.2, r.3, 1e9, 0.0, Skip::default(), &lamp, Some(r.0)))
            .collect();
        b.build_grid();
        assert!(b.grid.is_some());
        let grid: Vec<[f32; 3]> = rays
            .iter()
            .map(|r| b.visibility(r.0, r.1, r.2, r.3, 1e9, 0.0, Skip::default(), &lamp, Some(r.0)))
            .collect();
        // The same, but for the order see-through balls' shares multiply in.
        for (k, (e, g)) in each.iter().zip(&grid).enumerate() {
            for c in 0..3 {
                assert!((e[c] - g[c]).abs() <= 1e-6 * e[c].max(1.0), "ray {k}: {e:?} against {g:?}");
            }
        }
        assert!(each.iter().any(|v| v[0] < 0.5) && each.iter().any(|v| v[0] > 0.99), "a mix of open and blocked rays");
        assert!(each.iter().any(|v| v[0] > v[2] + 0.1), "some through coloured balls");
    }
}
