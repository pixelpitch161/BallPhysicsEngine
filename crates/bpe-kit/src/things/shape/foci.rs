//! An ellipse's two foci: every path from one to the outline and on to the other is the same
//! length, so balls fired at one speed from one focus all reach the other together.

use bpe_core::ThingId;
use bpe_runtime::{Outline, World};
use glam::Vec2;

/// Shape `shape`'s two foci as it stands in `world` now (its spin, shrink and keyed Width and
/// Height at this moment, and wherever it rides): an ellipse's lie on its long axis, `c` either
/// side of its middle with `c² = a² − b²`, the first towards its top (or its left, when it is
/// wider than tall) before it turns; a circle's are both its middle. `None` for a polygon or a
/// moulded outline, or when `shape` is not a wall in `world`.
pub fn foci(world: &World, shape: ThingId) -> Option<[Vec2; 2]> {
    match world.walls.get(&shape)?.outline {
        Outline::Ring { center, sides: 0, .. } => Some([center, center]),
        Outline::Ellipse { center, half, angle, .. } => {
            let (long, short, axis) =
                if half.x >= half.y { (half.x, half.y, Vec2::X) } else { (half.y, half.x, Vec2::Y) };
            let c = (long * long - short * short).max(0.0).sqrt();
            let along = Vec2::from_angle(angle).rotate(axis) * c;
            Some([center - along, center + along])
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{BALLS, SHAPE};
    use crate::{ShapeSettings, kit, new_scene};
    use bpe_core::{Scene, Settings, resolve};

    fn with(settings: ShapeSettings, edit: impl FnOnce(&mut Scene)) -> World {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        scene.things.get_mut(&SHAPE).unwrap().params = settings.to_params();
        scene.things.get_mut(&SHAPE).unwrap().transform.position = Vec2::ZERO;
        edit(&mut scene);
        World::build(&scene, &resolve(&scene, &kit.registry), &kit.sims)
    }

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).length() < 1e-2
    }

    #[test]
    fn an_ellipse_has_its_foci_on_its_long_axis() {
        let c = (650f32 * 650.0 - 450.0 * 450.0).sqrt();
        let [f1, f2] = foci(&with(ShapeSettings::ellipse(900.0, 1300.0, 3.0), |_| {}), SHAPE).unwrap();
        assert!(close(f1, Vec2::new(0.0, -c)) && close(f2, Vec2::new(0.0, c)), "{f1} {f2}");
        let [f1, f2] = foci(&with(ShapeSettings::ellipse(1300.0, 900.0, 3.0), |_| {}), SHAPE).unwrap();
        assert!(close(f1, Vec2::new(-c, 0.0)) && close(f2, Vec2::new(c, 0.0)), "{f1} {f2}");
        // Turned 30° clockwise by its Turn, and 10° more by the thing's Rotation.
        let mut turned = ShapeSettings::ellipse(900.0, 1300.0, 3.0);
        turned.turn = 30.0;
        let w = with(turned, |s| s.things.get_mut(&SHAPE).unwrap().transform.rotation = 10f32.to_radians());
        let [_, f2] = foci(&w, SHAPE).unwrap();
        assert!(close(f2, Vec2::from_angle(40f32.to_radians()).rotate(Vec2::new(0.0, c))), "{f2}");
    }

    #[test]
    fn a_circle_has_both_at_its_middle_and_a_polygon_none() {
        let w = with(ShapeSettings::circle(300.0, 3.0), |s| {
            s.things.get_mut(&SHAPE).unwrap().transform.position = Vec2::new(5.0, 6.0)
        });
        assert_eq!(foci(&w, SHAPE), Some([Vec2::new(5.0, 6.0); 2]));
        // A round ellipse is a circle.
        let w = with(ShapeSettings::ellipse(600.0, 600.0, 3.0), |_| {});
        assert_eq!(foci(&w, SHAPE), Some([Vec2::ZERO; 2]));
        assert_eq!(foci(&with(ShapeSettings::polygon(6, 300.0, 3.0), |_| {}), SHAPE), None);
        assert_eq!(foci(&with(ShapeSettings::circle(300.0, 3.0), |_| {}), BALLS), None, "not a wall");
    }

    #[test]
    fn the_foci_turn_and_shrink_with_the_shape() {
        let kit = kit();
        let mut s = ShapeSettings::ellipse(300.0, 500.0, 3.0);
        s.spin = 90.0;
        s.shrink = 0.5;
        s.min_size = 125.0;
        let mut w = with(s, |_| {});
        w.advance_to(2.0, &kit.sims, &mut bpe_runtime::Log::default());
        let [f1, f2] = foci(&w, SHAPE).unwrap();
        // Half a turn: the focus that was at the top is at the bottom; and shrunk to its smallest
        // (half its size): 2c is 400 at full size.
        assert!(f1.y > 0.0 && f1.x.abs() < 1.0 && (f1 + f2).length() < 1e-2, "{f1} {f2}");
        assert!(((f2 - f1).length() - 200.0).abs() < 0.5, "{}", (f2 - f1).length());
    }

    /// The focus property, end to end: 360 balls fired at one speed from one focus, bouncing by
    /// their middles, all pass the other focus together, within half a pixel.
    #[test]
    fn balls_from_one_focus_meet_at_the_other() {
        use bpe_core::{SlotId, Value};
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        scene.things.get_mut(&bpe_core::ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 0.0]));
        let shape = scene.things.get_mut(&SHAPE).unwrap();
        shape.params = ShapeSettings::ellipse(900.0, 1300.0, 8.0).to_params();
        shape.transform.position = Vec2::ZERO;
        let c = (650f32 * 650.0 - 450.0 * 450.0).sqrt();
        let (from, to) = (Vec2::new(0.0, -c), Vec2::new(0.0, c));
        let mut balls = crate::BallsSettings { radius: 1.0, velocity: Vec2::new(600.0, 0.0), ..Default::default() };
        balls.ring(360, 1.0);
        balls.launch = crate::things::balls::Launch::Outward;
        let thing = scene.things.get_mut(&BALLS).unwrap();
        thing.params = balls.to_params();
        thing.transform.position = from;
        scene.card_mut(BALLS, SlotId::Bounce).unwrap().params.set("touch", Value::Choice("middle".into()));
        let resolved = resolve(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        let mut log = bpe_runtime::Log::default();
        // Each ball's closest pass by the far focus: how near, and on which step.
        let mut best = vec![(f32::INFINITY, 0u64); 360];
        let rate = w.steps_per_second as f64;
        let arrive = (1300.0 - 1.0) / 600.0;
        let mut last: Vec<Vec2> = w.bodies[&BALLS].position.clone();
        let mut step = 0u64;
        while (step as f64) < (arrive + 0.2) * rate {
            step += 1;
            w.advance_to(step as f64 / rate, &kit.sims, &mut log);
            let now = &w.bodies[&BALLS].position;
            for (i, (a, b)) in last.iter().zip(now).enumerate() {
                let (_, _, d) = bpe_core::shape_path::nearest_on_segment(*a, *b, to);
                if d < best[i].0 {
                    best[i] = (d, step);
                }
            }
            last = now.clone();
        }
        assert_eq!(w.bodies[&BALLS].len(), 360);
        let worst = best.iter().map(|b| b.0).fold(0.0, f32::max);
        assert!(worst < 0.5, "every ball passes within half a pixel of the far focus: worst {worst}");
        let mut steps: Vec<u64> = best.iter().map(|b| b.1).collect();
        steps.sort_unstable();
        let median = steps[180];
        assert!(steps[0] + 1 >= median && steps[359] <= median + 1, "together: {} to {}", steps[0], steps[359]);
        assert!(((median as f64 / rate) - arrive).abs() < 0.02, "on time: {median}");
    }
}
