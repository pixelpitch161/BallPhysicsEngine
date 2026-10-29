//! Drawing the balls and their hit flashes.

use bpe_core::Settings;
use bpe_gfx::{BallInstance, DrawCx, FlashInstance, Frame, ThingDraw, blend_alpha};
use bpe_runtime::EventKind;

use super::{Arrangement, BallsSettings};
use crate::modules::look::draw::{colors, look_of};

pub struct BallsDraw;

/// The speed at which a flash is at full strength.
const FULL_FLASH_SPEED: f32 = 600.0;

impl ThingDraw for BallsDraw {
    fn kind(&self) -> &'static str {
        "balls"
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let id = cx.thing.id;
        let Some(bodies) = cx.world.bodies.get(&id) else { return };
        let Some(look) = look_of(cx) else { return };
        let settings = BallsSettings::from_params(&cx.resolved.params);
        let row_of = move |i: usize| match settings.arrangement {
            Arrangement::Block if settings.rows > 1 => {
                (i / settings.columns.max(1) as usize) as f32 / (settings.rows - 1) as f32
            }
            _ => 0.0,
        };
        let color = colors(&look, bodies.len(), row_of);
        for i in 0..bodies.len() {
            if !bodies.alive[i] {
                continue;
            }
            frame.balls.push(BallInstance {
                center: bodies.position[i].into(),
                radius: bodies.radius[i],
                glow: look.glow,
                color: blend_alpha(color(i)),
            });
        }

        if look.flash <= 0.0 || look.flash_seconds <= 0.0 {
            return;
        }
        let rate = cx.world.steps_per_second;
        let since = ((cx.time - look.flash_seconds as f64) * rate as f64).floor().max(0.0) as u64;
        for e in cx.log.since(since) {
            if e.kind != EventKind::Hit || e.a.thing != id {
                continue;
            }
            let age = (cx.time - e.time(rate)) as f32 / look.flash_seconds;
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
    fn a_gradient_colours_a_block_by_row_and_hits_flash() {
        let (scene, resolved, kit) = crate::testing::scene_with_balls("block", |p| {
            p.set("columns", bpe_core::Value::Int(2));
            p.set("rows", bpe_core::Value::Int(3));
        });
        let mut scene = scene;
        let m = bpe_core::resolve_slot(&scene, &kit.registry, ThingId(2), SlotId::Look, &mut Vec::new()).unwrap().module;
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
