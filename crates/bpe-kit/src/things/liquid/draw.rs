//! Drawing the liquid: with the Liquid surface look its particles go to the renderer's surface
//! layer; with any other look they are plain dots. A liquid on the graphics card has no
//! particles here, so it sends what the renderer needs to run it, and is always a surface.

use bpe_core::{Settings, SlotId};
use bpe_gfx::{BallInstance, DrawCx, Frame, GpuLiquid, LiquidLayer, Points, ThingDraw};
use bpe_runtime::state;

use super::sim::LiquidState;
use crate::modules::bounce::sim::BounceState;
use crate::modules::look::ClassicLook;
use crate::modules::look_liquid::LiquidLook;
use crate::modules::motion_liquid::Flowing;

pub struct LiquidDraw;

/// The surface look: the Liquid surface module's settings, or that module's defaults in the
/// classic look's colour when the liquid wears any other look.
fn surface_look(cx: &DrawCx) -> Option<LiquidLook> {
    let look = cx.resolved.slots.get(&SlotId::Look)?;
    Some(if look.kind == "look.liquid" {
        LiquidLook::from_params(&look.params)
    } else {
        LiquidLook { color: ClassicLook::from_params(&look.params).fill.colours.first(), ..LiquidLook::default() }
    })
}

fn layer(cx: &DrawCx, look: &LiquidLook, points: Points, radius: f32) -> LiquidLayer {
    LiquidLayer {
        thing: cx.thing.id,
        points,
        radius,
        color: look.color.to_linear(),
        clarity: look.clarity,
        refraction: look.refraction,
        highlight: look.highlight,
        smoothing: look.smoothing,
        glow: look.glow,
    }
}

impl ThingDraw for LiquidDraw {
    fn kind(&self) -> &'static str {
        "liquid"
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let Some(look) = cx.resolved.slots.get(&SlotId::Look) else { return };
        let Some(st) = cx.world.states.get(&cx.thing.id) else { return };
        let st = state::<LiquidState>(st.as_ref());
        if st.gpu {
            let flowing = match cx.world.module_state(cx.thing.id, SlotId::Motion) {
                Some(("motion.liquid", m)) => state::<Flowing>(m).clone(),
                _ => Flowing::default(),
            };
            let bounciness = match cx.world.module_state(cx.thing.id, SlotId::Bounce) {
                Some(("bounce.basic", b)) => state::<BounceState>(b).settings.bounciness,
                _ => 0.0,
            };
            let gpu = GpuLiquid {
                count: st.settings.count,
                region: [st.region.min.x, st.region.min.y, st.region.max.x, st.region.max.y],
                pour_seconds: st.settings.pour_seconds,
                pour_from: st.position.into(),
                seed: st.settings.seed,
                viscosity: flowing.viscosity,
                stiffness: flowing.stiffness,
                ball_push: flowing.ball_push,
                gravity_scale: flowing.gravity_scale,
                bounciness,
            };
            let Some(look) = surface_look(cx) else { return };
            frame.liquids.push(layer(cx, &look, Points::Gpu(gpu), st.radius()));
            return;
        }
        let Some(bodies) = cx.world.bodies.get(&cx.thing.id) else { return };
        let alive = (0..bodies.len()).filter(|&i| bodies.alive[i]);
        if look.kind == "look.liquid" {
            let look = LiquidLook::from_params(&look.params);
            let points = Points::Cpu(alive.map(|i| bodies.position[i].into()).collect());
            frame.liquids.push(layer(cx, &look, points, bodies.radius.first().copied().unwrap_or(0.0)));
        } else {
            let color = ClassicLook::from_params(&look.params).first_colour();
            let code = bpe_gfx::thing_code(cx.thing.id);
            frame.balls.extend(alive.map(|i| BallInstance {
                center: bodies.position[i].into(),
                radius: bodies.radius[i],
                glow: 0.0,
                color,
                code: [code, 0.0, 0.0, 0.0],
                paint: bpe_gfx::PaintGpu::default(),
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
        let params = &mut scene.library.modules.get_mut(&look).unwrap().params;
        let blue = Color::rgb(0, 0, 255);
        if look_kind == "look.liquid" {
            params.set("color", Value::Color(blue));
        } else {
            params.set("colours", Value::Gradient(bpe_core::Gradient::solid(blue)));
        }
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

    #[test]
    fn a_big_liquid_draws_a_gpu_layer_with_its_bounce() {
        let (mut scene, _, kit, me) = scene_with("liquid", "Deep pool", Vec2::new(0.0, -500.0), |p| {
            p.set("count", Value::Int(5000));
            p.set("pour_seconds", Value::Number(2.0));
            p.set("seed", Value::Int(7));
        });
        let look = scene.add_module(&kit.registry, "look.liquid", "Blue").unwrap();
        scene.link(&kit.registry, me, SlotId::Look, look).unwrap();
        let bounce = scene.add_module(&kit.registry, "bounce.basic", "Soft landing").unwrap();
        scene.library.modules.get_mut(&bounce).unwrap().params.set("bounciness", Value::Number(0.0));
        scene.link(&kit.registry, me, SlotId::Bounce, bounce).unwrap();
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        assert!(!world.bodies.contains_key(&me), "a GPU liquid has no bodies here");
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.liquids.len(), 1);
        let layer = &frame.liquids[0];
        assert_eq!(layer.thing, me);
        let Points::Gpu(gpu) = &layer.points else { panic!("a big liquid runs on the graphics card") };
        assert_eq!(gpu.count, 5000);
        assert_eq!((gpu.pour_seconds, gpu.pour_from, gpu.seed), (2.0, [0.0, -500.0], 7));
        assert_eq!(gpu.bounciness, 0.0);
        let flowing = crate::Flowing::default();
        assert_eq!(
            (gpu.viscosity, gpu.stiffness, gpu.ball_push, gpu.gravity_scale),
            (flowing.viscosity, flowing.stiffness, flowing.ball_push, flowing.gravity_scale)
        );
        // A pool fills the bottom 35% of the arena's box.
        assert!(gpu.region[1] > 0.0 && gpu.region[3] > gpu.region[1], "{:?}", gpu.region);
        assert!((layer.clarity, layer.smoothing) == (0.45, 6.0));
    }
}
