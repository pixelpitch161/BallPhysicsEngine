//! Drawing the balls and their hit flashes.

use bpe_core::{Settings, SlotId, ThingId};
use bpe_gfx::{BallInstance, DrawCx, FlashInstance, Frame, HitMark, ThingDraw, blend_alpha, thing_code};
use bpe_runtime::EventKind;

use super::{Arrangement, BallsSettings};
use crate::modules::look::draw::{colors, look_of};
use crate::modules::look::styled::{self, StyledLook};

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

/// The linear-light colour of each ball of the thing being drawn, from its look's colour or
/// gradient (by row, order, speed or hits) and its hue shift per hit, when it has a look and
/// bodies. Trails and particles take their colours from here too.
pub fn ball_colors<'a>(cx: &DrawCx<'a>) -> Option<impl Fn(usize) -> [f32; 4] + 'a> {
    let look = look_of(cx)?;
    let bodies = cx.world.bodies.get(&cx.thing.id)?;
    let settings = BallsSettings::from_params(&cx.resolved.params);
    let row_of = move |i: usize| match settings.arrangement {
        Arrangement::Block if settings.rows > 1 => {
            (i / settings.columns.max(1) as usize) as f32 / (settings.rows - 1) as f32
        }
        _ => 0.0,
    };
    Some(colors(look, bodies, row_of))
}

impl ThingDraw for BallsDraw {
    fn kind(&self) -> &'static str {
        "balls"
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let id = cx.thing.id;
        let Some(bodies) = cx.world.bodies.get(&id) else { return };
        let Some(look) = look_of(cx) else { return };
        let Some(color) = ball_colors(cx) else { return };
        let styled = cx.resolved.slots.get(&SlotId::Look).filter(|s| s.kind == "look.styled");
        let styled = styled.map(|s| StyledLook::from_params(&s.params));
        for i in 0..bodies.len() {
            if !bodies.alive[i] {
                continue;
            }
            let seed = ball_seed(id, i);
            let code = thing_code(id) + seed * 0.5;
            if let Some(style) = &styled {
                let (center, velocity, radius) = (bodies.position[i], bodies.velocity[i], bodies.radius[i]);
                frame.looks.push(styled::instance(
                    style,
                    color(i),
                    center,
                    velocity,
                    radius,
                    cx.time as f32,
                    code,
                    seed,
                ));
                continue;
            }
            frame.balls.push(BallInstance {
                center: bodies.position[i].into(),
                radius: bodies.radius[i],
                glow: look.glow,
                color: blend_alpha(color(i)),
                code: [code, seed, 0.0, 0.0],
            });
        }

        // Recent hits, for the flashes here and for the particle and shader modules. Only the
        // flashes' worth is scanned when nothing else wants them.
        let rate = cx.world.steps_per_second;
        let flashing = look.flash > 0.0 && look.flash_seconds > 0.0;
        let wants_hits = [SlotId::Particles, SlotId::Shader].iter().any(|s| cx.resolved.slots.contains_key(s));
        let memory = if wants_hits {
            HIT_MEMORY
        } else if flashing {
            look.flash_seconds as f64
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
            if !flashing {
                continue;
            }
            let age = (cx.time - time) as f32 / look.flash_seconds;
            if !(0.0..=1.0).contains(&age) {
                continue;
            }
            let intensity = (e.speed / FULL_FLASH_SPEED).min(1.5) * (-3.0 * age).exp() * 0.35 * look.flash / 0.4;
            if intensity < 0.002 {
                continue;
            }
            let c = color(e.a.item as usize);
            frame.flashes.push(FlashInstance {
                center: e.point.into(),
                size: look.flash_size,
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
    use crate::{kit, new_scene, registry};
    use bpe_core::{ModuleId, Params, SlotId, SlotLink, ThingId};
    use bpe_gfx::{Frame, build_frame};
    use bpe_runtime::{Log, World};

    #[test]
    fn a_missing_look_module_still_draws() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        // Point the ball at a module that doesn't exist.
        scene
            .things
            .get_mut(&ThingId(2))
            .unwrap()
            .slots
            .insert(SlotId::Look, SlotLink::Linked { module: ModuleId(99), overrides: Params::default() });
        let resolved = bpe_core::resolve(&scene, &reg);
        assert!(!resolved.problems.is_empty());
        let kit = kit();
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.balls.len(), 1);
        assert_eq!(frame.shapes.len(), 1);
        assert_eq!(frame.balls[0].color, [1.0, 1.0, 1.0, 1.0]); // falls back to the root's Plain look
        assert_eq!(frame.balls[0].glow, 0.12);
        assert_eq!((frame.width, frame.height), (1080, 1920));
    }

    #[test]
    fn a_styled_look_draws_looks_instead_of_balls_and_still_flashes() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        let styled = scene.add_module(&reg, "look.styled", "Star look").unwrap();
        let module = scene.library.modules.get_mut(&styled).unwrap();
        module.params.set("shape", bpe_core::Value::Choice("star".into()));
        module.params.set("flash", bpe_core::Value::Number(1.0));
        scene.link(&reg, ThingId(2), SlotId::Look, styled).unwrap();
        let resolved = bpe_core::resolve(&scene, &reg);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let kit = kit();
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(3.0);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 3.0, &kit.draws, &mut frame);
        assert!(frame.balls.is_empty());
        assert_eq!(frame.looks.len(), 1);
        assert_eq!(frame.looks[0].form[0], 5.0);
        assert_eq!(frame.looks[0].extent[1], bpe_gfx::thing_code(ThingId(2)) + frame.looks[0].seed * 0.5);
        let rate = playback.world().steps_per_second;
        let recent = playback.log().events().iter().filter(|e| e.time(rate) > 3.0 - 0.25).count();
        assert_eq!(frame.flashes.len(), recent);
    }

    #[test]
    fn a_look_that_is_off_draws_nothing() {
        let reg = registry();
        let mut scene = new_scene(&reg);
        scene.set_slot_off(ThingId(1), SlotId::Look);
        scene.set_slot_off(ThingId(2), SlotId::Look);
        let resolved = bpe_core::resolve(&scene, &reg);
        let kit = kit();
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert!(frame.balls.is_empty() && frame.shapes.is_empty());
    }

    #[test]
    fn colours_can_follow_hits_and_shift_hue_per_hit() {
        let (mut scene, _, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", bpe_core::Value::Int(2));
            p.set("rows", bpe_core::Value::Int(1));
            p.set("velocity", bpe_core::Value::Vec2([0.0, 0.0]));
        });
        let m =
            bpe_core::resolve_slot(&scene, &kit.registry, ThingId(2), SlotId::Look, &mut Vec::new()).unwrap().module;
        let look = scene.library.modules.get_mut(&m).unwrap();
        look.params.set(
            "gradient",
            bpe_core::Value::Colors(vec![bpe_core::Color::rgb(255, 0, 0), bpe_core::Color::rgb(0, 0, 255)]),
        );
        look.params.set("gradient_by", bpe_core::Value::Choice("hits".into()));
        look.params.set("gradient_full", bpe_core::Value::Number(2.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        world.bodies.get_mut(&ThingId(2)).unwrap().hits[1] = 2;
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.balls[0].color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(frame.balls[1].color, [0.0, 0.0, 1.0, 1.0]);
        // By order: first and last colour.
        scene.library.modules.get_mut(&m).unwrap().params.set("gradient_by", bpe_core::Value::Choice("order".into()));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!((frame.balls[0].color, frame.balls[1].color), ([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]));
        // By speed: both still, so both red.
        scene.library.modules.get_mut(&m).unwrap().params.set("gradient_by", bpe_core::Value::Choice("speed".into()));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.balls[1].color, [1.0, 0.0, 0.0, 1.0]);
        // Red turned 240° (two hits of 120°) is blue; the first ball with no hits stays red.
        scene.library.modules.get_mut(&m).unwrap().params.set("hue_per_hit", bpe_core::Value::Number(120.0));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.balls[0].color, [1.0, 0.0, 0.0, 1.0]);
        assert!(frame.balls[1].color[2] > 0.9 && frame.balls[1].color[0] < 0.1, "{:?}", frame.balls[1].color);
    }

    #[test]
    fn balls_leave_a_hit_mark_in_their_colour() {
        let (mut scene, _, kit) = crate::testing::one_ball_with_bounce(|_| {});
        // Hits are listed for the modules that use them: here a scene-wide burst.
        let m = scene.add_module(&kit.registry, "particles.burst", "Sparks").unwrap();
        scene.link(&kit.registry, ThingId::ROOT, SlotId::Particles, m).unwrap();
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
        assert_eq!((mark.thing, mark.other), (ThingId(2), Some(ThingId(1))));
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
        scene.set_slot_off(ThingId::ROOT, SlotId::Particles);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, playback.world(), playback.log(), 10.0, &kit.draws, &mut frame);
        let flash_worth = playback.log().events().iter().filter(|e| e.time(rate) > 10.0 - 0.25).count();
        assert_eq!(frame.hits.len(), flash_worth);
    }

    #[test]
    fn a_gradient_colours_a_block_by_row_and_hits_flash() {
        let (scene, resolved, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", bpe_core::Value::Int(2));
            p.set("rows", bpe_core::Value::Int(3));
        });
        let mut scene = scene;
        let m =
            bpe_core::resolve_slot(&scene, &kit.registry, ThingId(2), SlotId::Look, &mut Vec::new()).unwrap().module;
        scene.library.modules.get_mut(&m).unwrap().params.set(
            "gradient",
            bpe_core::Value::Colors(vec![bpe_core::Color::rgb(255, 0, 0), bpe_core::Color::rgb(0, 0, 255)]),
        );
        let _ = resolved;
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(3.0);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 3.0, &kit.draws, &mut frame);
        assert_eq!(frame.balls.len(), 6);
        assert_eq!(frame.balls[0].color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(frame.balls[1].color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(frame.balls[5].color, [0.0, 0.0, 1.0, 1.0]);
        // Something hit the wall in the last quarter second, or nothing did; either way the
        // flashes match the log.
        let rate = playback.world().steps_per_second;
        let recent = playback.log().events().iter().filter(|e| e.time(rate) > 3.0 - 0.25).count();
        assert_eq!(frame.flashes.len(), recent, "{:?}", playback.log().events());
    }
}
