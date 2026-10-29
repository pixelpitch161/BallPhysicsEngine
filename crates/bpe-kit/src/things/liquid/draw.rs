//! Drawing the liquid: its particles as plain dots until the surface layer takes over.

use bpe_core::{Settings, SlotId};
use bpe_gfx::{BallInstance, DrawCx, Frame, ThingDraw};

use crate::modules::look::ClassicLook;
use crate::modules::look_liquid::LiquidLook;

pub struct LiquidDraw;

/// The linear-light colour of a liquid's look, whichever kind fills its Look slot.
pub fn liquid_color(cx: &DrawCx) -> Option<[f32; 4]> {
    let look = cx.resolved.slots.get(&SlotId::Look)?;
    Some(match look.kind.as_str() {
        "look.liquid" => LiquidLook::from_params(&look.params).color.to_linear(),
        _ => ClassicLook::from_params(&look.params).color.to_linear(),
    })
}

impl ThingDraw for LiquidDraw {
    fn kind(&self) -> &'static str {
        "liquid"
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let Some(bodies) = cx.world.bodies.get(&cx.thing.id) else { return };
        let Some(color) = liquid_color(cx) else { return };
        for i in 0..bodies.len() {
            if bodies.alive[i] {
                frame.balls.push(BallInstance {
                    center: bodies.position[i].into(),
                    radius: bodies.radius[i],
                    glow: 0.0,
                    color,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::scene_with;
    use bpe_core::{Color, SlotId, Value};
    use bpe_gfx::{Frame, build_frame};
    use bpe_runtime::{Log, World};
    use glam::Vec2;

    #[test]
    fn a_cpu_liquid_draws_each_particle_as_a_dot_in_its_look_colour() {
        let (mut scene, _, kit, me) = scene_with("liquid", "Pool", Vec2::ZERO, |p| p.set("count", Value::Int(100)));
        let look = scene.add_module(&kit.registry, "look.liquid", "Blue").unwrap();
        scene.library.modules.get_mut(&look).unwrap().params.set("color", Value::Color(Color::rgb(0, 0, 255)));
        scene.link(&kit.registry, me, SlotId::Look, look).unwrap();
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.balls.len(), 1 + 100, "the ball and every particle");
        let dot = frame.balls[1];
        assert_eq!(dot.color, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(dot.glow, 0.0);
        assert!((dot.radius - world.bodies[&me].radius[0]).abs() < 1e-5);
    }
}
