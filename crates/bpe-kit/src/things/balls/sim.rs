//! Placing the balls and keeping their settings.

use std::f32::consts::{FRAC_PI_2, TAU};

use bpe_core::{ResolvedThing, Settings, Thing};
use bpe_runtime::{Bodies, ThingSim, World};
use glam::Vec2;

use super::{Arrangement, BallsSettings};

pub struct BallsSim;

/// What the balls keep between steps.
#[derive(Clone, Debug, PartialEq)]
pub struct BallsState {
    pub settings: BallsSettings,
}

/// Where each ball starts, relative to the thing: one at the middle, a block row by row from
/// the top, or a ring clockwise from the top.
pub fn offsets(s: &BallsSettings) -> Vec<Vec2> {
    match s.arrangement {
        Arrangement::Single => vec![Vec2::ZERO],
        Arrangement::Block => {
            let (columns, rows) = (s.columns.max(1), s.rows.max(1));
            let size = Vec2::new((columns - 1) as f32, (rows - 1) as f32) * s.spacing;
            let origin = -size * 0.5;
            let mut out = Vec::with_capacity((columns * rows) as usize);
            for row in 0..rows {
                for column in 0..columns {
                    out.push(origin + Vec2::new(column as f32, row as f32) * s.spacing);
                }
            }
            out
        }
        Arrangement::Ring => {
            let count = s.count.max(1);
            (0..count)
                .map(|i| {
                    let angle = -FRAC_PI_2 + TAU * i as f32 / count as f32;
                    Vec2::new(angle.cos(), angle.sin()) * s.ring_radius
                })
                .collect()
        }
    }
}

impl ThingSim for BallsSim {
    fn kind(&self) -> &'static str {
        "balls"
    }

    fn build(&self, thing: &Thing, resolved: &ResolvedThing, world: &mut World) {
        let settings = BallsSettings::from_params(&resolved.params);
        // Riders start where their parent is and move with it; everything else sits where its
        // own transform says.
        let at = world.anchor(thing.id);
        let turn = Vec2::from_angle(at.angle);
        let mut bodies = Bodies::default();
        for offset in offsets(&settings) {
            let position = at.position + turn.rotate(offset);
            let velocity = settings.velocity + at.velocity + (position - at.position).perp() * at.spin;
            bodies.push(position, velocity, settings.radius);
        }
        world.bodies.insert(thing.id, bodies);
        world.states.insert(thing.id, Box::new(BallsState { settings }));
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{ARENA, BALLS, scene_with_balls};
    use crate::{kit, new_scene};
    use bpe_core::{Attach, Value, resolve};
    use bpe_runtime::World;
    use glam::Vec2;
    use std::f32::consts::FRAC_PI_2;

    #[test]
    fn balls_riding_a_spinning_arena_start_with_its_rim_speed() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        scene.things.get_mut(&ARENA).unwrap().params.set("spin", Value::Number(90.0));
        scene
            .set_parent(
                BALLS,
                ARENA,
                Attach::Ride { offset: Vec2::new(0.0, -300.0), turn_with_parent: true, item: None },
            )
            .unwrap();
        scene.things.get_mut(&BALLS).unwrap().params.set("velocity", Value::Vec2([0.0, 0.0]));
        let resolved = resolve(&scene, &kit.registry);
        let w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(w.bodies[&BALLS].position[0], Vec2::new(0.0, -300.0));
        let v = w.bodies[&BALLS].velocity[0];
        // The rim at (0, -300) turning clockwise (y down) moves in +x at 300 * pi/2 px/s.
        assert!((v.x - 300.0 * FRAC_PI_2).abs() < 0.5 && v.y.abs() < 1e-3, "{v:?}");
    }

    #[test]
    fn a_turned_block_turns_its_layout() {
        let (mut scene, _, kit) = scene_with_balls("block", |p| {
            p.set("columns", Value::Int(2));
            p.set("rows", Value::Int(1));
            p.set("spacing", Value::Number(10.0));
        });
        scene.things.get_mut(&BALLS).unwrap().transform.rotation = FRAC_PI_2;
        let resolved = resolve(&scene, &kit.registry);
        let w = World::build(&scene, &resolved, &kit.sims);
        let p = &w.bodies[&BALLS].position;
        assert!((p[0] - Vec2::new(0.0, -5.0)).length() < 1e-4 && (p[1] - Vec2::new(0.0, 5.0)).length() < 1e-4, "{p:?}");
    }

    #[test]
    fn a_single_ball_sits_at_the_thing_with_its_launch_speed() {
        let (scene, resolved, kit) = scene_with_balls("single", |p| p.set("velocity", Value::Vec2([5.0, 6.0])));
        let w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(w.bodies[&BALLS].position, vec![Vec2::ZERO]);
        assert_eq!(w.bodies[&BALLS].velocity, vec![Vec2::new(5.0, 6.0)]);
        assert_eq!(w.bodies[&BALLS].radius, vec![24.0]);
    }

    #[test]
    fn a_block_is_laid_out_row_by_row_and_a_ring_clockwise_from_the_top() {
        let (scene, resolved, kit) = scene_with_balls("block", |p| {
            p.set("columns", Value::Int(2));
            p.set("rows", Value::Int(2));
            p.set("spacing", Value::Number(10.0));
        });
        let w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(
            w.bodies[&BALLS].position,
            vec![Vec2::new(-5.0, -5.0), Vec2::new(5.0, -5.0), Vec2::new(-5.0, 5.0), Vec2::new(5.0, 5.0)]
        );
        let (scene, resolved, kit) = scene_with_balls("ring", |p| {
            p.set("count", Value::Int(4));
            p.set("ring_radius", Value::Number(10.0));
        });
        let w = World::build(&scene, &resolved, &kit.sims);
        let p = &w.bodies[&BALLS].position;
        assert_eq!(p.len(), 4);
        assert!((p[0] - Vec2::new(0.0, -10.0)).length() < 1e-4, "{p:?}");
        assert!((p[1] - Vec2::new(10.0, 0.0)).length() < 1e-4, "{p:?}");
    }
}
