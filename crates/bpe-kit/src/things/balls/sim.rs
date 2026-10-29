//! Placing the balls and keeping their settings.

use std::f32::consts::{FRAC_PI_2, TAU};

use bpe_core::{ResolvedThing, Settings, SlotId, Thing, ThingId};
use bpe_runtime::{Bodies, ThingSim, World, state_mut};
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
        let mut bodies = Bodies::default();
        for offset in offsets(&settings) {
            bodies.push(thing.transform.position + offset, settings.velocity, settings.radius);
        }
        world.bodies.insert(thing.id, bodies);
        world.states.insert(thing.id, Box::new(BallsState { settings }));
        // A trail needs to know where the balls have been.
        if resolved.slots.get(&SlotId::Trail).is_some_and(|t| t.kind.starts_with("trail.")) {
            world.keep_history(thing.id);
        }
    }

    /// A keyframe moved the size: every ball takes it. Placement and launch speed only matter
    /// at the start, so they stay.
    fn settings_changed(&self, thing: ThingId, resolved: &ResolvedThing, world: &mut World) {
        let settings = BallsSettings::from_params(&resolved.params);
        if let Some(bodies) = world.bodies.get_mut(&thing) {
            bodies.radius.iter_mut().for_each(|r| *r = settings.radius);
        }
        if let Some(st) = world.states.get_mut(&thing) {
            state_mut::<BallsState>(st.as_mut()).settings = settings;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::{BALLS, scene_with_balls};
    use bpe_core::{Address, Owner, Value, resolve};
    use bpe_runtime::{Log, World};
    use glam::Vec2;

    #[test]
    fn a_single_ball_sits_at_the_thing_with_its_launch_speed() {
        let (scene, resolved, kit) = scene_with_balls("single", |p| p.set("velocity", Value::Vec2([5.0, 6.0])));
        let w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(w.bodies[&BALLS].position, vec![Vec2::ZERO]);
        assert_eq!(w.bodies[&BALLS].velocity, vec![Vec2::new(5.0, 6.0)]);
        assert_eq!(w.bodies[&BALLS].radius, vec![24.0]);
    }

    #[test]
    fn a_size_key_resizes_the_balls() {
        let (mut scene, _, kit) = scene_with_balls("block", |p| {
            p.set("columns", Value::Int(2));
            p.set("rows", Value::Int(1));
        });
        let size = Address { owner: Owner::Thing(BALLS), slot: None, key: "radius".into() };
        scene.set_key(&kit.registry, &size, 0.0, Value::Number(24.0)).unwrap();
        scene.set_key(&kit.registry, &size, 1.0, Value::Number(48.0)).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        assert_eq!(w.bodies[&BALLS].radius, vec![24.0, 24.0]);
        w.advance_to(1.5, &kit.sims, &mut Log::default());
        assert_eq!(w.bodies[&BALLS].radius, vec![48.0, 48.0]);
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
