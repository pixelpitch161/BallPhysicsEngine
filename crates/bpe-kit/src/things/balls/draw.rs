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

/// A ball's own number in 0 to 1, the same every frame, for patterns and effects that vary
/// from ball to ball.
pub fn ball_seed(thing: ThingId, index: usize) -> f32 {
    let x = (thing.0 as f32 * 7919.0 + index as f32 * 104_729.0).sin() * 43_758.547;
    (x - x.floor()).clamp(0.0, 0.999_999)
}

/// The linear-light colour of each ball of the thing being drawn, from its look's colour or
/// gradient, when it has a look. Trails and particles take their colours from here too.
pub fn ball_colors(cx: &DrawCx) -> Option<impl Fn(usize) -> [f32; 4] + use<>> {
    let look = look_of(cx)?;
    let settings = BallsSettings::from_params(&cx.resolved.params);
    let row_of = move |i: usize| match settings.arrangement {
        Arrangement::Block if settings.rows > 1 => {
            (i / settings.columns.max(1) as usize) as f32 / (settings.rows - 1) as f32
        }
        _ => 0.0,
    };
    let count = cx.world.bodies.get(&cx.thing.id).map_or(0, |b| b.len());
    Some(colors(&look, count, row_of))
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

        // Recent hits, for the flashes here and for the particle and shader modules.
        let rate = cx.world.steps_per_second;
        let since = ((cx.time - HIT_MEMORY) * rate as f64).floor().max(0.0) as u64;
        let flashing = look.flash > 0.0 && look.flash_seconds > 0.0;
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
    fn balls_leave_a_hit_mark_in_their_colour() {
        let (scene, resolved, kit) = crate::testing::one_ball_with_bounce(|_| {});
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
