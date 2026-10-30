//! Drawing an obstacle's pieces in its look's colours.

use bpe_gfx::{DrawCx, Frame, ShapeInstance, ThingDraw};
use bpe_runtime::{Outline, Solid};

use crate::modules::look::draw::{WallInputs, for_walls, look_of, wall_glow};

pub struct ObstacleDraw;

impl ThingDraw for ObstacleDraw {
    fn kind(&self) -> &'static str {
        "obstacle"
    }

    /// Every piece in the look's Colours, spread over the pieces in order (a peg grid's pegs, a
    /// set's bars) or by where each one is, its own hits, the beats or the time; or painted along
    /// each piece's line (Path) or across it (an inside pattern), turning as the obstacle turns.
    /// The glow keeps each piece's one colour.
    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let Some(solids) = cx.world.solids.get(&cx.thing.id) else { return };
        let Some(look) = look_of(cx) else { return };
        let fill = for_walls(look.fill.clone());
        let inputs = WallInputs::new(cx, &fill, solids.parts.len(), solids.pose.velocity.length());
        let glow = wall_glow(&look);
        for (i, part) in solids.parts.iter().enumerate() {
            let place = part.bounds().0;
            let (color, paint) = inputs.paint(&fill, i, place, &mut frame.gradients);
            let mut shape = match *part {
                Solid::Disc { center, radius } => ShapeInstance::disc(center, radius, color, glow).painted(paint),
                Solid::Capsule { from, to, radius } => {
                    ShapeInstance::capsule(from, to, radius, color, glow).painted(paint)
                }
                Solid::Ring { center, radius, thickness } => ShapeInstance::outline(
                    &Outline::Ring { center, radius, sides: 0, angle: 0.0, thickness },
                    paint,
                    color,
                    glow,
                ),
            };
            // A round piece has no angle of its own; a painted one turns with the obstacle.
            if paint.row >= 0.0 && !matches!(part, Solid::Capsule { .. }) {
                shape.a[2] = solids.pose.angle;
            }
            frame.shapes.push(shape);
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

    #[test]
    fn peg_grid_order_spreads_colours() {
        use bpe_core::{Color, Gradient, SlotId};

        use crate::testing::OBSTACLE;

        let (mut scene, _, kit) = scene_with_obstacle(|p| {
            p.set("shape", Value::Choice("peg_grid".into()));
            p.set("columns", Value::Int(3));
            p.set("rows", Value::Int(2));
        });
        let look = scene.add_module(&kit.registry, "look.classic", "Peg colours").unwrap();
        let params = &mut scene.library.modules.get_mut(&look).unwrap().params;
        params.set("colours", Value::Gradient(Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)])));
        params.set("follow", Value::Choice("order".into()));
        scene.link(&kit.registry, OBSTACLE, SlotId::Look, look).unwrap();
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        let pegs = &frame.shapes[1..];
        assert_eq!(pegs.len(), 5);
        assert_eq!(pegs[0].color, [1.0, 0.0, 0.0, 1.0], "the first peg takes the first colour");
        assert_eq!(pegs[4].color, [0.0, 0.0, 1.0, 1.0], "the last peg takes the last colour");
        let middle = pegs[2].color;
        assert!(middle[0] > 0.05 && middle[2] > 0.05 && middle[1] < 1e-3, "{middle:?}: half way");
        assert!(pegs.iter().all(|s| s.paint.row == -1.0), "no inside pattern, so nothing is baked");
        assert!(frame.gradients.len() == 1);
    }

    #[test]
    fn pegs_follow_their_own_hits_and_paint_along_their_line() {
        use bpe_core::{Color, Gradient, SlotId, ThingId};
        use bpe_runtime::{Event, EventKind, Party};
        use glam::Vec2;

        use crate::testing::{BALLS, OBSTACLE};

        let (mut scene, _, kit) = scene_with_obstacle(|p| {
            p.set("shape", Value::Choice("peg_grid".into()));
            p.set("columns", Value::Int(3));
            p.set("rows", Value::Int(2));
        });
        let look = scene.add_module(&kit.registry, "look.classic", "Peg colours").unwrap();
        let params = &mut scene.library.modules.get_mut(&look).unwrap().params;
        params.set("colours", Value::Gradient(Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)])));
        params.set("follow", Value::Choice("step".into()));
        scene.link(&kit.registry, OBSTACLE, SlotId::Look, look).unwrap();
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let rate = world.steps_per_second;
        let hit = |step: u64, a: Party, b: Party| Event {
            step,
            kind: EventKind::Hit,
            a,
            b: Some(b),
            point: Vec2::ZERO,
            normal: Vec2::Y,
            speed: 100.0,
        };
        let ball = Party { thing: BALLS, item: 0 };
        let peg = |item| Party { thing: OBSTACLE, item };
        let mut log = Log::default();
        log.push(hit(rate as u64, ball, peg(1)));
        log.push(hit(2 * rate as u64, peg(3), ball));
        log.push(hit(3 * rate as u64, ball, peg(3)));
        // Hits on something else, and hits still to come, do not count.
        log.push(hit(3 * rate as u64, ball, Party { thing: ThingId(1), item: 0 }));
        log.push(hit(10 * rate as u64, ball, peg(0)));
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &log, 5.0, &kit.draws, &mut frame);
        let colours: Vec<[f32; 4]> = frame.shapes[1..].iter().map(|s| s.color).collect();
        let (red, blue) = ([1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(colours, vec![red, blue, red, red, red], "one hit steps to blue, two step round to red");

        scene.library.modules.get_mut(&look).unwrap().params.set("follow", Value::Choice("path".into()));
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        build_frame(&scene, &resolved, &world, &log, 5.0, &kit.draws, &mut frame);
        let pegs = &frame.shapes[1..];
        assert!(pegs.iter().all(|s| s.paint.row == 1.0 && s.paint.pattern == crate::paint::gpu::PATH_PATTERN));
        assert!(pegs.iter().all(|s| s.color == red), "the glow keeps the start colour");
        assert_eq!(frame.gradients.len(), 2, "every peg shares one row");
        assert_eq!(frame.shapes[0].paint.row, -1.0, "the arena keeps its own plain look");
    }

    /// How many of `colours` differ from all the others before them by more than a hair.
    fn distinct(colours: &[[f32; 4]]) -> usize {
        let mut seen: Vec<[f32; 4]> = Vec::new();
        for c in colours {
            if !seen.iter().any(|d| (0..3).all(|k| (d[k] - c[k]).abs() < 1e-4)) {
                seen.push(*c);
            }
        }
        seen.len()
    }

    #[test]
    fn wall_pieces_each_vary_their_colour() {
        use bpe_core::{Color, Gradient, SlotId};

        use crate::testing::OBSTACLE;

        let (mut scene, _, kit) = scene_with_obstacle(|p| {
            p.set("shape", Value::Choice("peg_grid".into()));
            p.set("columns", Value::Int(4));
            p.set("rows", Value::Int(2));
        });
        let look = scene.add_module(&kit.registry, "look.classic", "Peg colours").unwrap();
        let params = &mut scene.library.modules.get_mut(&look).unwrap().params;
        params.set("colours", Value::Gradient(Gradient::even(&[Color::rgb(255, 0, 0)])));
        params.set("vary_hue", Value::Number(60.0));
        scene.link(&kit.registry, OBSTACLE, SlotId::Look, look).unwrap();
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 1.0, &kit.draws, &mut frame);
        let pegs: Vec<[f32; 4]> = frame.shapes[1..].iter().map(|s| s.color).collect();
        assert!(pegs.len() >= 7, "{pegs:?}");
        assert!(distinct(&pegs) >= pegs.len() - 1, "{pegs:?}");
    }
}
