//! Drawing the liquid: with the Liquid surface look its particles go to the renderer's surface
//! layer; with any other look they are plain dots.

use bpe_core::{Settings, SlotId};
use bpe_gfx::{BallInstance, DrawCx, Frame, LiquidLayer, Points, ThingDraw};

use crate::modules::look::ClassicLook;
use crate::modules::look_liquid::LiquidLook;

pub struct LiquidDraw;

impl ThingDraw for LiquidDraw {
    fn kind(&self) -> &'static str {
        "liquid"
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let Some(bodies) = cx.world.bodies.get(&cx.thing.id) else { return };
        let Some(look) = cx.resolved.slots.get(&SlotId::Look) else { return };
        let alive = (0..bodies.len()).filter(|&i| bodies.alive[i]);
        if look.kind == "look.liquid" {
            let look = LiquidLook::from_params(&look.params);
            frame.liquids.push(LiquidLayer {
                thing: cx.thing.id,
                points: Points::Cpu(alive.map(|i| bodies.position[i].into()).collect()),
                radius: bodies.radius.first().copied().unwrap_or(0.0),
                color: look.color.to_linear(),
                clarity: look.clarity,
                refraction: look.refraction,
                highlight: look.highlight,
                smoothing: look.smoothing,
                glow: look.glow,
            });
        } else {
            let color = ClassicLook::from_params(&look.params).color.to_linear();
            frame.balls.extend(alive.map(|i| BallInstance {
                center: bodies.position[i].into(),
                radius: bodies.radius[i],
                glow: 0.0,
                color,
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::scene_with;
    use bpe_core::{Color, SlotId, Value};
    use bpe_gfx::{Frame, Points, build_frame};
    use bpe_runtime::{Log, World};
    use glam::Vec2;

    fn pool_with_look(look_kind: &str) -> (Frame, World, bpe_core::ThingId) {
        let (mut scene, _, kit, me) = scene_with("liquid", "Pool", Vec2::ZERO, |p| p.set("count", Value::Int(100)));
        let look = scene.add_module(&kit.registry, look_kind, "Blue").unwrap();
        scene.library.modules.get_mut(&look).unwrap().params.set("color", Value::Color(Color::rgb(0, 0, 255)));
        scene.link(&kit.registry, me, SlotId::Look, look).unwrap();
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        (frame, world, me)
    }

    #[test]
    fn a_cpu_liquid_with_the_liquid_look_fills_a_surface_layer() {
        let (frame, world, me) = pool_with_look("look.liquid");
        assert_eq!(frame.balls.len(), 1, "only the ball: the particles go to the layer");
        assert_eq!(frame.liquids.len(), 1);
        let layer = &frame.liquids[0];
        assert_eq!(layer.thing, me);
        let Points::Cpu(points) = &layer.points else { panic!("a CPU liquid sends its points") };
        assert_eq!(points.len(), 100);
        assert_eq!(points[0], <[f32; 2]>::from(world.bodies[&me].position[0]));
        assert_eq!(layer.color, [0.0, 0.0, 1.0, 1.0]);
        assert!((layer.radius - world.bodies[&me].radius[0]).abs() < 1e-5);
        assert_eq!((layer.clarity, layer.refraction, layer.highlight), (0.45, 0.5, 0.8));
        assert_eq!((layer.smoothing, layer.glow), (6.0, 0.1));
    }

    #[test]
    fn a_liquid_with_a_classic_look_draws_plain_dots() {
        let (frame, world, me) = pool_with_look("look.classic");
        assert!(frame.liquids.is_empty());
        assert_eq!(frame.balls.len(), 1 + 100, "the ball and every particle");
        let dot = frame.balls[1];
        assert_eq!(dot.color, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(dot.glow, 0.0);
        assert!((dot.radius - world.bodies[&me].radius[0]).abs() < 1e-5);
    }
}
