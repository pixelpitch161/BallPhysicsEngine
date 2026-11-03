//! Drawing the balls and their hit flashes, through the layers of their Look.

use bpe_core::{Blend, ResolvedThing, SlotId, ThingId};
use bpe_gfx::{DrawCx, FlashInstance, Frame, HitMark, ThingDraw, blend_alpha, thing_code};
use bpe_runtime::{Bodies, EventKind};

use crate::modules::look::draw::{BallInputs, for_balls};
use crate::modules::look::layers::gpu::Stack;
use crate::paint::{colour_at, vary_gpu};

pub struct BallsDraw;

/// The speed at which a flash is at full strength.
const FULL_FLASH_SPEED: f32 = 600.0;
/// How far back hits are listed for particles and shaders, in seconds.
pub const HIT_MEMORY: f64 = 3.0;

/// A ball's own number in 0 to 1, the same every frame and on every machine, for patterns and
/// effects that vary from ball to ball.
pub fn ball_seed(thing: ThingId, index: usize) -> f32 {
    let mut x = (index as u32).wrapping_mul(0x9e37_79b9) ^ thing.0.wrapping_mul(0x85eb_ca6b) ^ 0x27d4_eb2f;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    x = x.wrapping_mul(0x8a7d_9c3b);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// The stack of layers the thing being drawn wears, when its Look card is a layered one.
fn stack_of(cx: &DrawCx) -> Option<Stack> {
    Stack::of(cx.resolved.slots.get(&SlotId::Look)?)
}

/// The linear-light colour of each ball of the thing being drawn: its Body's colours (painted by
/// place, start, order, speed, hits, steps or time), then turned by its colour reactions (Colour
/// on hit and Colour over time), when it has a layered look and bodies. Trails and particles take
/// their colours from here too.
pub fn ball_colors<'a>(cx: &DrawCx<'a>) -> Option<impl Fn(usize) -> [f32; 4] + 'a> {
    let stack = stack_of(cx)?;
    let bodies = cx.world.bodies.get(&cx.thing.id)?;
    Some(colours(&stack, cx, bodies))
}

/// [`ball_colors`] for `stack` and `bodies`.
fn colours<'a>(stack: &Stack, cx: &DrawCx<'a>, bodies: &'a Bodies) -> impl Fn(usize) -> [f32; 4] + use<'a> {
    let fill = for_balls(stack.body.fill.clone());
    let inputs = BallInputs::new(cx, bodies, &[&fill]);
    let (hue_per_hit, turn) = (stack.hue_per_hit(), stack.rainbow() * cx.time as f32);
    move |i| {
        let base = colour_at(&fill, &inputs.get(&fill, i));
        let hits = bodies.hits.get(i).copied().unwrap_or(0);
        crate::modules::look::layers::gpu::recoloured(base, hue_per_hit, turn, hits)
    }
}

impl ThingDraw for BallsDraw {
    fn kind(&self) -> &'static str {
        "balls"
    }

    fn blend(&self, resolved: &ResolvedThing) -> Option<(Blend, f32, bool)> {
        Stack::of(resolved.slots.get(&SlotId::Look)?).map(|s| s.whole_blend())
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let id = cx.thing.id;
        let Some(bodies) = cx.world.bodies.get(&id) else { return };
        let Some(stack) = stack_of(cx) else { return };
        let color = colours(&stack, cx, bodies);
        let time = cx.time as f32;
        let first = frame.look_layers.len() as u32;
        let records = stack.records(&mut frame.gradients, time);
        let count = records.len() as u32;
        frame.look_layers.extend(records);
        // Each ball's own variation, only read when a pattern paints from its own gradient.
        let fill = for_balls(stack.body.fill.clone());
        let vary = stack.varies().then(|| BallInputs::new(cx, bodies, &[&fill]));
        for i in 0..bodies.len() {
            if !bodies.alive[i] {
                continue;
            }
            let seed = ball_seed(id, i);
            let code = thing_code(id) + seed * 0.5;
            let own = vary.as_ref().map_or(bpe_gfx::NO_VARY, |v| {
                let at = v.get(&fill, i);
                vary_gpu(&fill, at.seed, at.time)
            });
            let (center, velocity, radius) = (bodies.position[i], bodies.velocity[i], bodies.radius[i]);
            let body = blend_alpha(color(i));
            frame.layered.push(stack.instance(body, center, velocity, radius, time, code, seed, own, first, count));
        }

        // Recent hits, for the flashes here and for the particle and shader modules. Only the
        // flashes' worth is scanned when nothing else wants them.
        let rate = cx.world.steps_per_second;
        let flash = stack.flash.as_ref().filter(|f| f.flash > 0.0 && f.flash_seconds > 0.0);
        // The scene settings' effects aren't passed down but still pulse on every hit.
        let scene_effects = cx.scene.things.get(&ThingId::ROOT).is_some_and(|r| r.slots.contains_key(&SlotId::Shader));
        let wants_hits =
            scene_effects || [SlotId::Particles, SlotId::Shader].iter().any(|s| cx.resolved.slots.contains_key(s));
        let memory = if wants_hits {
            HIT_MEMORY
        } else if let Some(f) = flash {
            f.flash_seconds as f64
        } else {
            return;
        };
        let since = ((cx.time - memory) * rate as f64).floor().max(0.0) as u64;
        for e in cx.log.since(since) {
            if e.kind != EventKind::Hit || e.a.thing != id {
                continue;
            }
            let time = e.time(rate);
            if time > cx.time {
                continue;
            }
            frame.hits.push(HitMark {
                thing: id,
                other: e.b.map(|b| b.thing),
                point: e.point.into(),
                normal: e.normal.into(),
                time: time as f32,
                speed: e.speed,
                color: color(e.a.item as usize),
            });
            let Some(f) = flash else { continue };
            let age = (cx.time - time) as f32 / f.flash_seconds;
            if !(0.0..=1.0).contains(&age) {
                continue;
            }
            let intensity = (e.speed / FULL_FLASH_SPEED).min(1.5) * (-3.0 * age).exp() * 0.35 * f.flash / 0.4;
            if intensity < 0.002 {
                continue;
            }
            let c = color(e.a.item as usize);
            frame.flashes.push(FlashInstance {
                center: e.point.into(),
                size: f.flash_size,
                color: [
                    (c[0] + 1.0) * 0.5 * intensity,
                    (c[1] + 1.0) * 0.5 * intensity,
                    (c[2] + 1.0) * 0.5 * intensity,
                    1.0,
                ],
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::modules::look::layers::{BODY, HUE, PATTERN};
    use crate::testing::set_layer;
    use crate::{kit, new_scene, registry};
    use bpe_core::{Scene, Slot, SlotId, ThingId, Value};
    use bpe_gfx::{Frame, LayerGpu, build_frame};
    use bpe_runtime::{Log, World};

    const BALL: ThingId = ThingId(2);

    #[test]
    fn a_look_of_an_unknown_kind_is_reported_and_leaves_the_balls_undrawn() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        scene.things.get_mut(&BALL).unwrap().slots.insert(SlotId::Look, Slot::new("look.hologram"));
        let resolved = bpe_core::resolve(&scene, &reg);
        assert!(!resolved.problems.is_empty());
        let kit = kit();
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert!(frame.layered.is_empty() && frame.look_layers.is_empty(), "no look, nothing to draw them with");
        assert_eq!(frame.shapes.len(), 1, "the arena still draws");
        assert_eq!((frame.width, frame.height), (1080, 1920));
    }

    /// The frame at `time` for the new scene's ball after `f`'s changes, played to `time`.
    fn frame_after(time: f64, f: impl FnOnce(&mut Scene)) -> Frame {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        f(&mut scene);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(time);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), time, &kit.draws, &mut frame);
        frame
    }

    #[test]
    fn the_classic_ball_draws_its_body_shine_and_glow_through_one_layered_instance() {
        let frame = frame_after(0.0, |_| {});
        assert!(frame.balls.is_empty() && frame.looks.is_empty(), "the old paths draw no ball");
        assert_eq!(frame.layered.len(), 1);
        let codes: Vec<u32> = frame.look_layers.iter().map(|l| l.head[0]).collect();
        assert_eq!(codes, [LayerGpu::BODY, LayerGpu::SHINE, LayerGpu::GLOW]);
        assert_eq!(frame.layered[0].range[..2], [0, 3]);
        let pink = bpe_core::Color::rgb(0xff, 0x5c, 0xa8).to_linear();
        assert_eq!(frame.layered[0].body, pink);
    }

    #[test]
    fn a_new_ball_is_one_white_body_record_and_does_not_flash() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        scene.remove_thing(BALL).unwrap();
        let ball = crate::add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        let thing = scene.things.get_mut(&ball).unwrap();
        thing.params.set("velocity", Value::Vec2([0.0, 900.0]));
        scene.add_slot(&reg, ball, SlotId::Particles, "particles.burst").unwrap();
        let resolved = bpe_core::resolve(&scene, &reg);
        let kit = kit();
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(4.0);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 4.0, &kit.draws, &mut frame);
        assert_eq!(frame.layered.len(), 1);
        assert_eq!(frame.layered[0].range[..2], [0, 1], "the body alone");
        assert_eq!(frame.look_layers[0].head[0], LayerGpu::BODY);
        assert_eq!(frame.layered[0].body, [1.0; 4], "white");
        assert!(!frame.hits.is_empty(), "the ball hit the arena");
        assert!(frame.flashes.is_empty(), "no Flash on hit, no flash");
    }

    #[test]
    fn a_ball_in_another_shape_still_flashes_and_carries_its_thing_code() {
        let frame = frame_after(3.0, |s| {
            set_layer(s, BALL, BODY, "ball_shape", Value::Choice("star".into()));
        });
        assert_eq!(frame.layered.len(), 1);
        let ball = frame.layered[0];
        assert_eq!(ball.form[0], 5.0);
        assert_eq!(ball.misc[1], bpe_gfx::thing_code(BALL) + ball.seed * 0.5);
        assert!(!frame.flashes.is_empty() || frame.hits.is_empty());
    }

    #[test]
    fn flashes_come_from_the_flash_on_hit_reaction_only() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(8.0);
        let rate = playback.world().steps_per_second;
        let first = playback.log().events().first().expect("the ball hits the arena").time(rate);
        // Just after the first hit, while it still flashes.
        let at = first + 0.05;
        playback.seek(at);
        let recent = playback.log().events().iter().filter(|e| e.time(rate) > at - 0.25 && e.time(rate) <= at).count();
        assert!(recent > 0);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), at, &kit.draws, &mut frame);
        assert_eq!(frame.flashes.len(), recent);
        // Without the reaction, no flash.
        let card = scene.card_mut(BALL, SlotId::Look).unwrap();
        let flash = card.layers.iter().find(|l| l.kind == crate::modules::look::layers::FLASH).unwrap().id;
        scene.remove_layer(&kit.registry, BALL, SlotId::Look, flash).unwrap();
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, playback.world(), playback.log(), at, &kit.draws, &mut frame);
        assert!(frame.flashes.is_empty());
    }

    #[test]
    fn a_hidden_reaction_does_nothing() {
        let frame = frame_after(0.0, |s| {
            let reg = registry();
            let hue = set_layer(s, BALL, HUE, "hue_per_hit", Value::Number(120.0));
            s.set_layer_enabled(&reg, BALL, SlotId::Look, hue, false).unwrap();
            s.things.get_mut(&BALL).unwrap().params.set("velocity", Value::Vec2([0.0, 0.0]));
        });
        let pink = bpe_core::Color::rgb(0xff, 0x5c, 0xa8).to_linear();
        assert_eq!(frame.layered[0].body, pink);
    }

    #[test]
    fn inside_linear_on_a_ball_adds_a_pattern_record() {
        let frame = frame_after(0.0, |s| {
            set_layer(s, BALL, PATTERN, "in_pattern", Value::Choice("linear".into()));
        });
        let pattern = frame.look_layers.iter().find(|l| l.head[0] == LayerGpu::PATTERN).unwrap();
        assert_eq!(pattern.paint.pattern, 1.0);
        assert_eq!(pattern.paint.row, -2.0, "a shade of the ball's own colour by default");
    }

    #[test]
    fn a_look_that_is_off_draws_nothing() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        scene.remove_slot(ThingId(1), SlotId::Look);
        scene.remove_slot(BALL, SlotId::Look);
        let resolved = bpe_core::resolve(&scene, &reg);
        let kit = kit();
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert!(frame.layered.is_empty() && frame.shapes.is_empty());
    }

    #[test]
    fn colours_can_follow_hits_and_shift_hue_per_hit() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", bpe_core::Value::Int(2));
            p.set("rows", bpe_core::Value::Int(1));
            p.set("velocity", bpe_core::Value::Vec2([0.0, 0.0]));
        });
        set_layer(&mut scene, BALL, BODY, "colours", Value::Gradient(bpe_core::Gradient::even(&[RED, BLUE])));
        set_layer(&mut scene, BALL, BODY, "follow", Value::Choice("hits".into()));
        set_layer(&mut scene, BALL, BODY, "span", Value::Number(2.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        world.bodies.get_mut(&BALL).unwrap().hits[1] = 2;
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.layered[0].body, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(frame.layered[1].body, [0.0, 0.0, 1.0, 1.0]);
        // By order: first and last colour.
        set_layer(&mut scene, BALL, BODY, "follow", Value::Choice("order".into()));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!((frame.layered[0].body, frame.layered[1].body), ([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]));
        // By speed: both still, so both red.
        set_layer(&mut scene, BALL, BODY, "follow", Value::Choice("speed".into()));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.layered[1].body, [1.0, 0.0, 0.0, 1.0]);
        // Colour on hit: red turned 240° (two hits of 120°) is blue; the first ball with no hits
        // stays red. Its hit marks and trails take the same colour.
        set_layer(&mut scene, BALL, HUE, "hue_per_hit", Value::Number(120.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.layered[0].body, [1.0, 0.0, 0.0, 1.0]);
        let b = frame.layered[1].body;
        assert!(b[2] > 0.9 && b[0] < 0.1, "{b:?}");
    }

    #[test]
    fn colour_over_time_turns_the_body_round_the_wheel() {
        let at = |time: f64| {
            frame_after(time, |s| {
                s.things.get_mut(&BALL).unwrap().params.set("velocity", Value::Vec2([0.0, 0.0]));
                set_layer(s, BALL, crate::modules::look::layers::RAINBOW, "rainbow", Value::Number(0.5));
            })
            .layered[0]
                .body
        };
        let pink = bpe_core::Color::rgb(0xff, 0x5c, 0xa8).to_linear();
        assert_eq!(at(0.0), pink);
        let half = crate::modules::look::basic::shift_linear_hue(pink, 0.5);
        assert_eq!(bpe_core::Color::from_linear(at(1.0)), bpe_core::Color::from_linear(half));
    }

    #[test]
    fn balls_leave_a_hit_mark_in_their_colour() {
        let (mut scene, _, kit) = crate::testing::one_ball_with_bounce(|_| {});
        // Hits are listed for the modules that use them: here the ball's own burst.
        scene.add_slot(&kit.registry, BALL, SlotId::Particles, "particles.burst").unwrap();

        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(4.0);
        let rate = playback.world().steps_per_second;
        let recent: Vec<_> = playback.log().events().iter().filter(|e| e.time(rate) > 4.0 - 3.0).collect();
        assert!(!recent.is_empty(), "the ball hit nothing in four seconds");
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 4.0, &kit.draws, &mut frame);
        assert_eq!(frame.hits.len(), recent.len());
        let mark = frame.hits[0];
        assert_eq!((mark.thing, mark.other), (BALL, Some(ThingId(1))));
        assert_eq!(mark.point, <[f32; 2]>::from(recent[0].point));
        assert_eq!(mark.speed, recent[0].speed);
        assert!((mark.time - recent[0].time(rate) as f32).abs() < 1e-6);
        let pink = bpe_core::Color::rgb(0xff, 0x5c, 0xa8).to_linear();
        assert_eq!(mark.color, pink);
        // A hit further back than the memory is not listed.
        playback.seek(10.0);
        let old: Vec<_> = playback.log().events().iter().filter(|e| e.time(rate) > 7.0).collect();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 10.0, &kit.draws, &mut frame);
        assert_eq!(frame.hits.len(), old.len());
        // Without a module that wants them, only the flashes' quarter second is listed.
        scene.remove_slot(BALL, SlotId::Particles);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, playback.world(), playback.log(), 10.0, &kit.draws, &mut frame);
        let flash_worth = playback.log().events().iter().filter(|e| e.time(rate) > 10.0 - 0.25).count();
        assert_eq!(frame.hits.len(), flash_worth);
    }

    #[test]
    fn an_effect_on_the_scene_settings_still_gets_the_balls_hits() {
        let (mut scene, _, kit) = crate::testing::one_ball_with_bounce(|_| {});
        scene.add_slot(&kit.registry, ThingId::ROOT, SlotId::Shader, "shader.kaleidoscope").unwrap();

        let resolved = bpe_core::resolve(&scene, &kit.registry);
        assert!(!resolved.things[&BALL].slots.contains_key(&SlotId::Shader), "the effect is not passed down");
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(4.0);
        let rate = playback.world().steps_per_second;
        let recent = playback.log().events().iter().filter(|e| e.time(rate) > 4.0 - 3.0).count();
        assert!(recent > 0, "the ball hit nothing in four seconds");
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 4.0, &kit.draws, &mut frame);
        assert_eq!(frame.hits.len(), recent, "the scene's effect pulses on every hit it remembers");
    }

    #[test]
    fn stepping_on_hits_reads_the_log_and_the_hit_looks_once_a_frame() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", Value::Int(4));
            p.set("rows", Value::Int(3));
        });
        set_layer(&mut scene, BALL, BODY, "colours", Value::Gradient(bpe_core::Gradient::even(&[RED, BLUE])));
        set_layer(&mut scene, BALL, BODY, "follow", Value::Choice("step".into()));
        set_layer(&mut scene, BALL, BODY, "take_hit_colour", Value::Bool(true));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(6.0);
        let hit = playback.world().bodies[&BALL].hits.iter().filter(|&&h| h > 0).count();
        assert!(hit >= 4, "only {hit} balls hit something");
        let (searches, reads) = (&crate::paint::eval::LOG_SEARCHES, &crate::modules::look::draw::LOOK_READS);
        let (s0, r0) = (searches.with(|n| n.get()), reads.with(|n| n.get()));
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 6.0, &kit.draws, &mut frame);
        let (s1, r1) = (searches.with(|n| n.get()) - s0, reads.with(|n| n.get()) - r0);
        // The balls' draw reads their colours once; that reads the log once, and each other
        // thing's look once, whatever the number of balls.
        assert!(s1 <= 2, "{s1} searches of the log for {hit} balls that hit");
        assert!(r1 <= 2, "{r1} reads of the hit things' looks for {hit} balls that hit");
    }

    const RED: bpe_core::Color = bpe_core::Color::rgb(255, 0, 0);
    const BLUE: bpe_core::Color = bpe_core::Color::rgb(0, 0, 255);

    #[test]
    fn row_follow_is_place_from_spawn_at_90_degrees() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", Value::Int(2));
            p.set("rows", Value::Int(3));
            p.set("spacing", Value::Number(40.0));
        });
        scene.things.get_mut(&BALL).unwrap().transform.position = glam::Vec2::new(30.0, -200.0);
        set_layer(&mut scene, BALL, BODY, "colours", Value::Gradient(bpe_core::Gradient::even(&[RED, BLUE])));
        set_layer(&mut scene, BALL, BODY, "follow", Value::Choice("start".into()));
        set_layer(&mut scene, BALL, BODY, "angle", Value::Number(90.0));
        set_layer(&mut scene, BALL, BODY, "centre", Value::Vec2([30.0, -200.0]));
        // From the first row's middle to the last's.
        set_layer(&mut scene, BALL, BODY, "span", Value::Number(80.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(3.0);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 3.0, &kit.draws, &mut frame);
        assert_eq!(frame.layered.len(), 6);
        let body: Vec<[f32; 4]> = frame.layered.iter().map(|b| b.body).collect();
        let near = |a: [f32; 4], b: [f32; 4]| (0..4).all(|k| (a[k] - b[k]).abs() < 1e-5);
        let (red, blue) = (RED.to_linear(), BLUE.to_linear());
        // Where the balls started counts, not where they are now.
        assert!(near(body[0], red) && near(body[1], red), "{body:?}");
        let middle = bpe_core::Gradient::even(&[RED, BLUE]).sample(0.5);
        assert!(near(body[2], middle) && near(body[3], middle), "{body:?}");
        assert!(near(body[4], blue) && near(body[5], blue), "{body:?}");
        // Hits flash: the flashes match the log.
        let rate = playback.world().steps_per_second;
        let recent = playback.log().events().iter().filter(|e| e.time(rate) > 3.0 - 0.25).count();
        assert_eq!(frame.flashes.len(), recent, "{:?}", playback.log().events());
    }

    #[test]
    fn a_plain_look_bakes_nothing_and_a_pattern_paints_from_the_ball_s_colour_or_its_own() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", Value::Int(3));
            p.set("rows", Value::Int(1));
            p.set("velocity", Value::Vec2([0.0, 0.0]));
        });
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert!(frame.look_layers.iter().all(|l| l.head[0] != LayerGpu::PATTERN));
        assert_eq!(frame.look_layers.len(), 3, "one stack of records shared by the three balls");
        assert!(frame.layered.iter().all(|b| b.range[..2] == [0, 3]));
        assert!(frame.gradients.len() == 1, "nothing is baked for a plain look");

        set_layer(&mut scene, BALL, BODY, "colours", Value::Gradient(bpe_core::Gradient::even(&[RED, BLUE])));
        set_layer(&mut scene, BALL, BODY, "follow", Value::Choice("order".into()));
        set_layer(&mut scene, BALL, PATTERN, "in_pattern", Value::Choice("rings".into()));
        set_layer(&mut scene, BALL, PATTERN, "in_spin", Value::Number(90.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &Log::default(), 2.0, &kit.draws, &mut frame);
        assert_eq!(frame.gradients.len(), 1, "a shade of each ball's own colour bakes nothing");
        let paint = frame.look_layers.iter().find(|l| l.head[0] == LayerGpu::PATTERN).unwrap().paint;
        assert_eq!((paint.row, paint.pattern, paint.angle), (-2.0, 6.0, 270.0));
        assert_eq!(paint.t0, -0.5, "the default shade");
        // Its own gradient takes one row the balls share.
        set_layer(&mut scene, BALL, PATTERN, "in_pattern", Value::Choice("dots".into()));
        set_layer(&mut scene, BALL, PATTERN, "in_from", Value::Choice("own_gradient".into()));
        set_layer(&mut scene, BALL, PATTERN, "in_colours", Value::Gradient(bpe_core::Gradient::even(&[RED, BLUE])));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &Log::default(), 2.0, &kit.draws, &mut frame);
        assert_eq!(frame.gradients.len(), 2, "the balls share one row");
        let paint = frame.look_layers.iter().find(|l| l.head[0] == LayerGpu::PATTERN).unwrap().paint;
        assert_eq!((paint.row, paint.pattern), (1.0, 7.0));
    }

    #[test]
    fn stepping_on_hits_can_take_the_colour_of_what_was_hit() {
        let (mut scene, _, kit) = crate::testing::one_ball_with_bounce(|_| {});
        set_layer(&mut scene, BALL, BODY, "colours", Value::Gradient(bpe_core::Gradient::even(&[RED, BLUE])));
        set_layer(&mut scene, BALL, BODY, "follow", Value::Choice("step".into()));
        set_layer(&mut scene, BALL, BODY, "take_hit_colour", Value::Bool(true));
        // Paint the arena's own look green.
        let green = bpe_core::Color::rgb(0, 255, 0);
        let arena_look = &mut scene.card_mut(ThingId(1), SlotId::Look).unwrap().params;
        arena_look.set("colours", Value::Gradient(bpe_core::Gradient::solid(green)));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.layered[0].body, RED.to_linear(), "no hits yet: the first colour");
        playback.seek(4.0);
        assert!(playback.world().bodies[&BALL].hits[0] > 0, "the ball hit the wall");
        build_frame(&scene, &resolved, playback.world(), playback.log(), 4.0, &kit.draws, &mut frame);
        assert_eq!(frame.layered[0].body, green.to_linear());
        // Without taking it, the ball steps through its own colours.
        set_layer(&mut scene, BALL, BODY, "take_hit_colour", Value::Bool(false));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, playback.world(), playback.log(), 4.0, &kit.draws, &mut frame);
        assert_ne!(frame.layered[0].body, green.to_linear());
    }

    #[test]
    fn a_ball_hit_by_a_layered_ball_takes_its_body_colour() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        let other = crate::add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        let green = bpe_core::Color::rgb(0, 255, 0);
        set_layer(&mut scene, other, BODY, "colours", Value::Gradient(bpe_core::Gradient::solid(green)));
        let resolved = bpe_core::resolve(&scene, &reg);
        let kit = kit();
        let world = World::build(&scene, &resolved, &kit.sims);
        let cx = bpe_gfx::DrawCx {
            scene: &resolved,
            resolved: &resolved.things[&BALL],
            thing: &scene.things[&BALL],
            world: &world,
            log: &Log::default(),
            time: 0.0,
        };
        let still = crate::modules::look::draw::StillColours::new(&cx);
        assert_eq!(still.at(other, glam::Vec2::ZERO), Some(green.to_linear()));
    }

    #[test]
    fn a_ball_group_with_vary_hue_has_different_colours() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", Value::Int(4));
            p.set("rows", Value::Int(2));
            p.set("velocity", Value::Vec2([0.0, 0.0]));
        });
        set_layer(&mut scene, BALL, BODY, "colours", Value::Gradient(bpe_core::Gradient::even(&[RED])));
        set_layer(&mut scene, BALL, BODY, "vary_hue", Value::Number(40.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let build = || {
            let mut frame = Frame::default();
            build_frame(&scene, &resolved, &world, &Log::default(), 1.0, &kit.draws, &mut frame);
            frame.layered.iter().map(|b| b.body).collect::<Vec<_>>()
        };
        let colours = build();
        assert_eq!(colours.len(), 8);
        let mut distinct: Vec<[f32; 4]> = Vec::new();
        for c in &colours {
            if !distinct.iter().any(|d| (0..3).all(|k| (d[k] - c[k]).abs() < 1e-4)) {
                distinct.push(*c);
            }
        }
        assert!(distinct.len() >= 6, "only {} distinct colours: {colours:?}", distinct.len());
        // The same frame built twice is the same.
        assert_eq!(build(), colours);
    }

    #[test]
    fn an_inside_pattern_carries_each_ball_s_variation_to_the_gpu() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", Value::Int(3));
            p.set("rows", Value::Int(1));
            p.set("velocity", Value::Vec2([0.0, 0.0]));
        });
        set_layer(&mut scene, BALL, PATTERN, "in_pattern", Value::Choice("stripes".into()));
        set_layer(&mut scene, BALL, PATTERN, "in_from", Value::Choice("own_gradient".into()));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 1.0, &kit.draws, &mut frame);
        assert!(frame.layered.iter().all(|b| b.vary == [0.0, 0.0, 1.0, 0.0]), "no variation is the identity");
        set_layer(&mut scene, BALL, BODY, "vary_hue", Value::Number(90.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &Log::default(), 1.0, &kit.draws, &mut frame);
        let turns: Vec<f32> = frame.layered.iter().map(|b| b.vary[0]).collect();
        assert!(turns.iter().all(|t| t.abs() <= 0.25), "{turns:?}");
        assert!(turns[0] != turns[1] && turns[1] != turns[2], "{turns:?}");
    }
}
