//! Drawing the arena's wall.

use bpe_gfx::{DrawCx, Frame, ShapeInstance, ThingDraw, thing_code};

use crate::modules::look::draw::{WallInputs, for_walls, look_of, wall_glow};

pub struct ArenaDraw;

impl ThingDraw for ArenaDraw {
    fn kind(&self) -> &'static str {
        "arena"
    }

    /// The wall in its look's Colours: one colour where the paint puts the wall, or a gradient
    /// along the line (Path) or across it (an inside pattern). The glow keeps the one colour.
    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let Some(wall) = cx.world.walls.get(&cx.thing.id) else { return };
        let Some(look) = look_of(cx) else { return };
        let fill = for_walls(look.fill.clone());
        let inputs = WallInputs::new(cx, &fill, 1, wall.velocity.length());
        let (color, paint) = inputs.paint(&fill, 0, wall.outline.center(), &mut frame.gradients);
        frame
            .shapes
            .push(ShapeInstance::outline(&wall.outline, paint, color, wall_glow(&look)).code(thing_code(cx.thing.id)));
    }
}

#[cfg(test)]
mod tests {
    use bpe_core::{Color, Gradient, SlotId, Value};
    use bpe_gfx::{Frame, build_frame};
    use bpe_runtime::{Log, World};

    use crate::testing::{ARENA, scene_with_balls};

    #[test]
    fn a_wall_paints_along_its_line_or_across_itself() {
        let (mut scene, _, kit) = scene_with_balls("single", |_| {});
        let look = scene.add_module(&kit.registry, "look.basic", "Wall colours").unwrap();
        let params = &mut scene.library.modules.get_mut(&look).unwrap().params;
        let colours = Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]);
        params.set("colours", Value::Gradient(colours.clone()));
        params.set("follow", Value::Choice("path".into()));
        scene.link(&kit.registry, ARENA, SlotId::Look, look).unwrap();
        let draw = |scene: &bpe_core::Scene| {
            let resolved = bpe_core::resolve(scene, &kit.registry);
            assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
            let world = World::build(scene, &resolved, &kit.sims);
            let mut frame = Frame::default();
            build_frame(scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
            frame
        };
        let frame = draw(&scene);
        let wall = frame.shapes[0];
        assert_eq!((wall.paint.row, wall.paint.pattern), (1.0, crate::paint::gpu::PATH_PATTERN));
        assert_eq!(wall.color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(frame.gradients.rows()[1].to_vec(), colours.bake(bpe_gfx::ATLAS_WIDTH));

        // One colour paints nothing on the line, as before.
        let params = &mut scene.library.modules.get_mut(&look).unwrap().params;
        params.set("colours", Value::Gradient(Gradient::solid(Color::rgb(0, 255, 0))));
        params.set("follow", Value::Choice("order".into()));
        let frame = draw(&scene);
        assert_eq!((frame.shapes[0].paint.row, frame.shapes[0].color), (-1.0, [0.0, 1.0, 0.0, 1.0]));
        assert!(frame.gradients.len() == 1);
    }
}
