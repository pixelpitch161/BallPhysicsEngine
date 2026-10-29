//! Drawing an obstacle's pieces in its look's colour.

use bpe_gfx::{DrawCx, Frame, ShapeInstance, ThingDraw};
use bpe_runtime::{Outline, Solid};

use crate::modules::look::draw::{look_of, wall_glow};

pub struct ObstacleDraw;

impl ThingDraw for ObstacleDraw {
    fn kind(&self) -> &'static str {
        "obstacle"
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let Some(solids) = cx.world.solids.get(&cx.thing.id) else { return };
        let Some(look) = look_of(cx) else { return };
        let color = look.color.to_linear();
        let glow = wall_glow(&look);
        for part in &solids.parts {
            frame.shapes.push(match *part {
                Solid::Disc { center, radius } => ShapeInstance::disc(center, radius, color, glow),
                Solid::Capsule { from, to, radius } => ShapeInstance::capsule(from, to, radius, color, glow),
                Solid::Ring { center, radius, thickness } => ShapeInstance::outline(
                    &Outline::Ring { center, radius, sides: 0, angle: 0.0, thickness },
                    color,
                    glow,
                ),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use bpe_core::Value;
    use bpe_gfx::{Frame, build_frame};
    use bpe_runtime::{Log, World};

    use crate::testing::scene_with_obstacle;

    #[test]
    fn every_piece_is_drawn_as_a_shape() {
        let (scene, resolved, kit) = scene_with_obstacle(|p| {
            p.set("shape", Value::Choice("peg_grid".into()));
            p.set("columns", Value::Int(3));
            p.set("rows", Value::Int(2));
        });
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        // The arena's wall plus five pegs.
        assert_eq!(frame.shapes.len(), 6);
        assert!(frame.shapes[1..].iter().all(|s| s.c[0] == 0.0 && s.c[1] == s.b[0]), "filled discs");
        let (scene, resolved, kit) = scene_with_obstacle(|p| p.set("shape", Value::Choice("bar".into())));
        let world = World::build(&scene, &resolved, &kit.sims);
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        assert_eq!(frame.shapes.len(), 2);
        assert_eq!(frame.shapes[1].c[0], 2.0);
    }
}
