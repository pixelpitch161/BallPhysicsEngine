//! Drawing a Particles layer: each emission still alive (see `emit.rs`), each of its particles
//! moved to the moment drawn (`sim.rs`), coloured and drawn as a sprite, or as a small ball
//! wearing the ball's own look (Made of: Mini balls), with what its sub-layers add.

use bpe_core::{Blend, Thing};
use bpe_gfx::{DrawCx, Frame, LayeredRun, Particle, blend_alpha, thing_code};
use bpe_runtime::Wall;
use glam::Vec2;

use super::emit::{self, Emission, MAX_PARTICLES};
use super::sim::{Live, Motion, simulate};
use super::{MadeOf, ParticleSource, ParticlesLayer};
use crate::modules::look::layers::around::{AroundKind, AroundLayer};
use crate::modules::look::layers::around_subs::{Extras, HitInfo, Pieces, coloured, particle_with_extras};
use crate::modules::look::layers::gpu::Stack;
use crate::modules::look::layers::trail::AroundPlace;
use crate::modules::look::layers::trail::draw::Balls;
use crate::paint::{Inputs, colour_at};

/// A particle's colour from `base` (linear light) at `alpha`, mixed in by the blend amount:
/// covering, or adding its light (bright, so it blooms). An additive colour glows whatever the
/// blend says. Other blends draw into a layer mixed in at its amount later: here they are plain.
pub fn shade(layer: &ParticlesLayer, base: [f32; 4], alpha: f32) -> [f32; 4] {
    let amount = match layer.blend {
        Blend::Normal | Blend::Add => layer.blend_amount.clamp(0.0, 1.0),
        _ => 1.0,
    };
    if layer.blend != Blend::Add && base[3] > 0.0 {
        return [base[0], base[1], base[2], (base[3] * alpha * amount).min(1.0)];
    }
    [base[0], base[1], base[2], -(alpha * amount)]
}

/// The sprite for `live`, made of `made_of`, coloured `colour`.
pub fn sprite(made_of: MadeOf, live: &Live, colour: [f32; 4]) -> Particle {
    let mut p = Particle::new(made_of.code(), live.place, live.angle, live.size, colour);
    let speed = live.velocity.length();
    p.style[0] = match made_of {
        MadeOf::Sparks => live.size + speed * 0.03,
        MadeOf::Streaks => live.size * 2.0 + speed * 0.08,
        MadeOf::Confetti => live.size * 1.6,
        MadeOf::Shards => live.size * 1.4,
        _ => live.size,
    };
    p
}

/// What drawing one layer's particles reads besides the layer.
struct Source<'a> {
    cx: &'a DrawCx<'a>,
    /// The layer as the card has it: where it draws and what it bounces off.
    particles: &'a ParticlesLayer,
    /// The layer an emission's particles are (the card's, or their ball's own copy when the
    /// layer's settings vary across the balls) and its settings.
    pick: &'a dyn Fn(&Emission) -> (&'a AroundLayer, &'a ParticlesLayer),
    /// The colour an emission's particles take from the ball (Colour from: Ball).
    colour: &'a dyn Fn(&Emission) -> [f32; 4],
    /// The hits the colour sub-layers read for an emission's particles.
    hit: &'a dyn Fn(&Emission) -> HitInfo,
    /// World units to a sub-layer's size unit.
    unit: &'a dyn Fn(&Emission) -> f32,
    /// Where the ball that let an emission off was at a moment, for Pull back.
    pull_to: &'a dyn Fn(&Emission, f64) -> Option<Vec2>,
}

/// The walls `layer` bounces its particles off.
fn walls<'a>(cx: &'a DrawCx, layer: &ParticlesLayer) -> Vec<&'a Wall> {
    if layer.bounce { cx.world.walls.values().collect() } else { Vec::new() }
}

/// Each particle of `emissions` alive at the moment drawn, with its emission and number.
fn living<'a>(src: &Source, emissions: &'a [Emission], walls: &[&Wall], mut each: impl FnMut(&'a Emission, u32, Live)) {
    let cx = src.cx;
    let mut drawn = 0usize;
    for e in emissions {
        let pull = |t: f64| (src.pull_to)(e, t);
        let motion = Motion { walls, pull_to: Some(&pull) };
        for k in 0..e.count {
            if drawn >= MAX_PARTICLES {
                return;
            }
            if let Some(live) = simulate((src.pick)(e).1, e, k, cx.time, &motion) {
                drawn += 1;
                each(e, k, live);
            }
        }
    }
}

/// Draws the particles of `emissions` as sprites, with their sub-layers.
fn draw_sprites(src: &Source, emissions: &[Emission], frame: &mut Frame) {
    let cx = src.cx;
    let walls = walls(cx, src.particles);
    let mut pieces = Pieces::default();
    let screen = cx.resolved.screen;
    living(src, emissions, &walls, |e, k, live| {
        let (layer, p) = (src.pick)(e);
        let base = match p.source {
            ParticleSource::Ball => (src.colour)(e),
            ParticleSource::Own => {
                let inputs = Inputs {
                    index: k as usize,
                    count: e.count as usize,
                    place: live.place,
                    place_screen: if screen { live.place } else { cx.world.camera.to_screen(live.place) },
                    start: e.place,
                    speed: live.velocity.length(),
                    life: live.life,
                    time: cx.time as f32,
                    seed: emit::hash(&[e.seed, k]),
                    ..Inputs::default()
                };
                colour_at(&p.colour, &inputs)
            }
        };
        let hit = (src.hit)(e);
        let subs = &layer.subs;
        let own = shade(p, coloured(subs, subs.len(), base, hit), live.alpha);
        let extras =
            Extras { subs, base, s: live.life, fade: live.alpha, unit: (src.unit)(e), hit, outline: 1.0, added: 1.0 };
        particle_with_extras(sprite(p.made_of, &live, own), &extras, &mut pieces);
    });
    pieces.into_frame(frame, src.particles.place == AroundPlace::InFront);
}

/// Draws the balls' `layer` (a particles one) into `frame`.
pub fn draw_balls(
    cx: &DrawCx,
    layer: &AroundLayer,
    particles: &ParticlesLayer,
    balls: &Balls,
    stack: &Stack,
    frame: &mut Frame,
) {
    let wears = |i: usize| layer.applies((balls.number)(i));
    let rate = cx.world.steps_per_second;
    // When the layer's settings vary across the balls, each ball lets off particles of its own
    // copy: the balls wearing the same copy are worked out together, then the newest of all of
    // them kept.
    let mut copies: Vec<(std::rc::Rc<Vec<AroundLayer>>, usize)> = Vec::new();
    let mut copy_of: Vec<usize> = Vec::new();
    if balls.own.is_some() {
        for i in 0..balls.bodies.len() {
            let own = balls.own_layers(i).unwrap_or_default();
            let at = match copies.iter().position(|(c, _)| std::rc::Rc::ptr_eq(c, &own)) {
                Some(at) => at,
                None => {
                    let index = own.iter().position(|a| a.id == layer.id && matches!(a.kind, AroundKind::Particles(_)));
                    copies.push((own, index.unwrap_or(usize::MAX)));
                    copies.len() - 1
                }
            };
            copy_of.push(at);
        }
    }
    let mine = |at: usize| -> (&AroundLayer, &ParticlesLayer) {
        let Some((own, index)) = copies.get(at) else { return (layer, particles) };
        match own.get(*index) {
            Some(a @ AroundLayer { kind: AroundKind::Particles(p), .. }) => (a, &**p),
            _ => (layer, particles),
        }
    };
    let emissions = if copies.len() > 1 {
        let mut all = Vec::new();
        for at in 0..copies.len() {
            let wears = |i: usize| copy_of.get(i) == Some(&at) && wears(i);
            let p = mine(at).1;
            all.extend(emit::ball_emissions(p, cx.thing.id, balls, &wears, cx.log, rate, cx.time, MAX_PARTICLES));
        }
        all.sort_by(|a, b| a.time.total_cmp(&b.time));
        emit::capped(all, MAX_PARTICLES)
    } else {
        let p = mine(0).1;
        emit::ball_emissions(p, cx.thing.id, balls, &wears, cx.log, rate, cx.time, MAX_PARTICLES)
    };
    if emissions.is_empty() {
        return;
    }
    let row = |e: &Emission| e.row.unwrap_or(0).min(balls.bodies.len().saturating_sub(1));
    let colour = |e: &Emission| (balls.colour)(row(e));
    let hit = |e: &Emission| {
        let i = row(e);
        let last = balls.last_hits.get(i).copied().filter(|h| h.0 < bpe_gfx::NO_HIT[0]);
        HitInfo { last, hits: balls.bodies.hits.get(i).copied().unwrap_or(0) }
    };
    let unit = |e: &Emission| balls.bodies.radius.get(row(e)).copied().unwrap_or(1.0);
    let pull_to = |e: &Emission, t: f64| Some(emit::ball_at(balls, row(e), (cx.time - t).max(0.0) as f32));
    let pick = |e: &Emission| mine(e.row.and_then(|r| copy_of.get(r).copied()).unwrap_or(0));
    let src = Source { cx, particles, pick: &pick, colour: &colour, hit: &hit, unit: &unit, pull_to: &pull_to };
    if particles.made_of == MadeOf::MiniBalls {
        draw_mini_balls(&src, &emissions, balls, stack, frame);
    } else {
        draw_sprites(&src, &emissions, frame);
    }
}

/// Draws the particles of `emissions` as small balls wearing `stack`, the ball's own look.
fn draw_mini_balls(src: &Source, emissions: &[Emission], balls: &Balls, stack: &Stack, frame: &mut Frame) {
    let cx = src.cx;
    let time = cx.time as f32;
    let walls = walls(cx, src.particles);
    let records = stack.records(&mut frame.gradients, &mut frame.mask_tiles, time);
    let first = frame.look_layers.len() as u32;
    let count = records.len() as u32;
    frame.look_layers.extend(records);
    let start = frame.layered.len() as u32;
    let mut made = Vec::new();
    living(src, emissions, &walls, |e, k, live| {
        let row = e.row.unwrap_or(0);
        let body = blend_alpha((balls.colour)(row));
        let seed = (emit::hash(&[e.seed, k, 3]) >> 8) as f32 / (1u32 << 24) as f32;
        let code = thing_code(cx.thing.id) + seed * 0.5;
        let mut ball = stack.instance(
            body,
            live.place,
            live.velocity,
            live.size,
            time,
            code,
            seed,
            bpe_gfx::NO_VARY,
            first,
            count,
        );
        ball.misc[0] *= live.alpha;
        made.push(ball);
    });
    frame.layered.extend(made);
    let program = stack.program().map(|p| match frame.programs.iter().position(|q| q.key == p.key) {
        Some(i) => i as u32,
        None => {
            frame.programs.push(p);
            frame.programs.len() as u32 - 1
        }
    });
    let end = frame.layered.len() as u32;
    if end > start {
        frame.layered_runs.push(LayeredRun { program, start, end });
    }
}

/// Draws a shape's `layer` (a particles one, read as a ball's) into `frame`: off its edge where
/// balls hit it, or steadily all along it.
pub fn draw_shape(cx: &DrawCx, layer: &AroundLayer, particles: &ParticlesLayer, hits: HitInfo, frame: &mut Frame) {
    let id = cx.thing.id;
    let wall = cx.world.walls.get(&id);
    let rate = cx.world.steps_per_second;
    let emissions = emit::shape_emissions(particles, id, wall, cx.log, rate, cx.time, MAX_PARTICLES);
    if emissions.is_empty() {
        return;
    }
    // The colour of the ball that hit the shape, as that ball draws it; white for a steady
    // shape's (the shape's body is white).
    let mut by_thing: std::collections::BTreeMap<bpe_core::ThingId, Vec<[f32; 4]>> = Default::default();
    if particles.source == ParticleSource::Ball {
        for e in &emissions {
            let Some(by) = e.by else { continue };
            by_thing.entry(by.thing).or_insert_with(|| ball_colours(cx, by.thing));
        }
    }
    let white = [1.0, 1.0, 1.0, 1.0];
    let colour = |e: &Emission| {
        e.by.and_then(|b| by_thing.get(&b.thing).and_then(|c| c.get(b.item as usize)).copied()).unwrap_or(white)
    };
    let hit = |_: &Emission| hits;
    let unit = |_: &Emission| 1.0;
    let pull_to = |_: &Emission, _: f64| None;
    let pick = |_: &Emission| (layer, particles);
    let src = Source { cx, particles, pick: &pick, colour: &colour, hit: &hit, unit: &unit, pull_to: &pull_to };
    draw_sprites(&src, &emissions, frame);
}

/// Every ball's colour of the balls thing `id`, as it draws them now.
fn ball_colours(cx: &DrawCx, id: bpe_core::ThingId) -> Vec<[f32; 4]> {
    let (Some(resolved), Some(bodies)) = (cx.scene.things.get(&id), cx.world.bodies.get(&id)) else {
        return Vec::new();
    };
    let thing = Thing { id, ..Thing::default() };
    let other = DrawCx { thing: &thing, resolved, ..*cx };
    match crate::things::balls::draw::ball_colors(&other) {
        Some(colour) => (0..bodies.len()).map(colour).collect(),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use crate::modules::look::layers::{PARTICLES, SHAPE_PARTICLES, SUB_GLOW};
    use crate::{kit, new_scene};
    use bpe_core::{LayerId, Scene, SlotId, ThingId, Value, layered_key};
    use bpe_gfx::{Frame, build_frame};
    use bpe_runtime::Playback;

    const SHAPE: ThingId = ThingId(1);
    const BALL: ThingId = ThingId(2);

    fn with_particles(thing: ThingId, kind: &str, f: impl FnOnce(&mut Scene, LayerId)) -> Scene {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let id = scene.add_layer(&kit.registry, thing, SlotId::Look, kind).unwrap();
        f(&mut scene, id);
        scene
    }

    fn set(scene: &mut Scene, thing: ThingId, id: LayerId, key: &str, v: Value) {
        scene.card_mut(thing, SlotId::Look).unwrap().params.set(&layered_key(id, key), v);
    }

    fn choice(s: &str) -> Value {
        Value::Choice(s.into())
    }

    /// The frames at each of `times`, drawn in that order from one playback.
    fn frames(scene: &Scene, times: &[f64]) -> Vec<Frame> {
        let kit = kit();
        let resolved = bpe_core::resolve(scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
        times
            .iter()
            .map(|&t| {
                playback.seek(t);
                let mut frame = Frame::default();
                build_frame(scene, &resolved, playback.world(), playback.log(), t, &kit.draws, &mut frame);
                frame
            })
            .collect()
    }

    fn hits_before(scene: &Scene, t: f64) -> usize {
        let kit = kit();
        let resolved = bpe_core::resolve(scene, &kit.registry);
        let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
        playback.seek(t);
        playback.log().events().iter().filter(|e| e.kind == bpe_runtime::EventKind::Hit).count()
    }

    #[test]
    fn particles_are_the_same_whenever_the_frame_is_drawn() {
        let scene = with_particles(BALL, PARTICLES, |s, id| {
            set(s, BALL, id, "life", Value::Number(2.5));
            set(s, BALL, id, "swirl", Value::Number(400.0));
            set(s, BALL, id, "bounce", Value::Bool(true));
        });
        assert!(hits_before(&scene, 4.0) > 0, "the ball hits the shape");
        let f = frames(&scene, &[4.0, 4.0, 6.0, 4.0, 1.0, 4.0]);
        assert!(!f[0].particles.is_empty());
        assert_eq!(f[0].particles, f[1].particles);
        assert_eq!(f[0].particles, f[3].particles, "after drawing a later time");
        assert_eq!(f[0].particles, f[5].particles, "after drawing an earlier time");
        assert!(f[0].particles.iter().all(|p| p.shape[0] == 0 && p.color[3] < 0.0), "added sparks");
    }

    #[test]
    fn moving_particles_come_only_from_a_moving_ball_and_steady_ones_at_fixed_times() {
        let moving = |s: &mut Scene, id| {
            set(s, BALL, id, "emit", choice("while_moving"));
            set(s, BALL, id, "rate", Value::Number(30.0));
        };
        let f = frames(&with_particles(BALL, PARTICLES, moving), &[2.0]);
        assert!(!f[0].particles.is_empty());
        let still = with_particles(BALL, PARTICLES, |s, id| {
            moving(s, id);
            s.things.get_mut(&BALL).unwrap().params.set("velocity", Value::Vec2([0.0, 0.0]));
            s.card_mut(BALL, SlotId::Motion).unwrap().params.set("gravity_scale", Value::Number(0.0));
        });
        assert!(frames(&still, &[2.0])[0].particles.is_empty(), "a still ball lets nothing off");
        // Steady: a still ball still lets them off, one every 1/rate seconds, wherever the frame
        // came from.
        let steady = with_particles(BALL, PARTICLES, |s, id| {
            moving(s, id);
            set(s, BALL, id, "emit", choice("steady"));
            set(s, BALL, id, "life", Value::Number(1.0));
            s.things.get_mut(&BALL).unwrap().params.set("velocity", Value::Vec2([0.0, 0.0]));
            s.card_mut(BALL, SlotId::Motion).unwrap().params.set("gravity_scale", Value::Number(0.0));
        });
        let f = frames(&steady, &[2.0, 0.5, 1.95, 2.0]);
        assert_eq!(f[0].particles.len(), 30, "a second's worth at 30 a second");
        assert_eq!(f[0].particles, f[3].particles);
    }

    #[test]
    fn a_layer_draws_at_most_4000() {
        let scene = with_particles(BALL, PARTICLES, |s, id| {
            set(s, BALL, id, "emit", choice("steady"));
            set(s, BALL, id, "rate", Value::Number(500.0));
            set(s, BALL, id, "life", Value::Number(3.0));
            let p = &mut s.things.get_mut(&BALL).unwrap().params;
            p.set("group", Value::Bool(true));
            p.set("count", Value::Int(4));
        });
        let f = frames(&scene, &[3.5]);
        assert_eq!(f[0].particles.len(), super::MAX_PARTICLES);
    }

    #[test]
    fn a_shape_s_particles_come_from_its_hits() {
        let scene = with_particles(SHAPE, SHAPE_PARTICLES, |s, id| set(s, SHAPE, id, "life", Value::Number(3.0)));
        let f = frames(&scene, &[4.0]);
        assert!(!f[0].particles.is_empty());
        // Off the shape's edge: near its outline, a circle 508 round.
        let r: Vec<f32> = f[0].particles.iter().map(|p| p.at().length()).collect();
        assert!(r.iter().any(|r| (r - 508.0).abs() < 120.0), "{r:?}");
        // Without hits, nothing.
        let early = frames(&scene, &[0.01]);
        assert!(early[0].particles.is_empty());
    }

    #[test]
    fn mini_balls_wear_the_ball_s_look() {
        let scene = with_particles(BALL, PARTICLES, |s, id| {
            set(s, BALL, id, "made_of", choice("mini_balls"));
            set(s, BALL, id, "life", Value::Number(3.0));
        });
        let f = frames(&scene, &[4.0]);
        assert!(f[0].particles.is_empty());
        assert!(f[0].layered.len() > 1, "the ball and its mini balls");
        let ball = f[0].layered[0];
        assert!(f[0].layered[1..].iter().all(|m| m.radius < ball.radius && m.form[0] == ball.form[0]));
        let covered: u32 = f[0].layered_runs.iter().map(|r| r.end - r.start).sum();
        assert_eq!(covered as usize, f[0].layered.len());
    }

    #[test]
    fn mini_balls_draw_with_their_balls_at_any_z_order() {
        let scene = with_particles(BALL, PARTICLES, |s, id| {
            set(s, BALL, id, "made_of", choice("mini_balls"));
            set(s, BALL, id, "life", Value::Number(3.0));
            s.things.get_mut(&BALL).unwrap().z_order = 5;
        });
        let f = frames(&scene, &[4.0]).remove(0);
        let holding: Vec<_> = f.layers.iter().filter(|l| !l.frame.layered.is_empty()).collect();
        assert_eq!(holding.len(), 1, "the balls and their mini balls in one layer, not behind the balls");
        assert!(holding[0].frame.layered.len() > 1);
    }

    #[test]
    fn a_glow_sub_layer_lights_each_particle_from_under_it() {
        let reg = crate::registry();
        let plain = frames(&with_particles(BALL, PARTICLES, |_, _| {}), &[4.0]).remove(0);
        let glowing = with_particles(BALL, PARTICLES, |s, id| {
            s.add_sub_layer(&reg, BALL, SlotId::Look, id, SUB_GLOW).unwrap();
        });
        let f = frames(&glowing, &[4.0]).remove(0);
        let n = plain.particles.len();
        assert!(n > 0);
        assert_eq!(f.particles.len(), 2 * n);
        assert!(f.particles[..n].iter().all(|p| p.style[2] == 1.0 && p.place[3] > 0.0), "soft glows first");
        assert_eq!(&f.particles[n..], &plain.particles[..], "then the particles as they were");
    }
}
