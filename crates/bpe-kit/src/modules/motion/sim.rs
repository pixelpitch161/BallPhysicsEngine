//! Gravity, drag and a steady push on every ball that uses the module (the air it follows is the
//! world's: `bpe_runtime::World` carries such balls along, see `ModuleSim::air_grip`).

use bpe_core::{Params, Settings};
use bpe_runtime::{Bodies, ModuleSim, SimCx, ThingState, state};
use glam::Vec2;

use super::BasicMotion;

pub struct BasicMotionSim;

/// What a motion module keeps while the world runs.
#[derive(Clone, Debug)]
pub struct MotionState {
    pub settings: BasicMotion,
}

impl ModuleSim for BasicMotionSim {
    fn kind(&self) -> &'static str {
        "motion.basic"
    }

    fn prepare(&self, params: &Params) -> Box<dyn ThingState> {
        Box::new(MotionState { settings: BasicMotion::from_params(params) })
    }

    fn pull(&self, st: &dyn ThingState, gravity: Vec2) -> Option<Vec2> {
        let m = &state::<MotionState>(st).settings;
        Some(gravity * m.gravity_scale + m.wind)
    }

    fn forces(&self, st: &dyn ThingState, bodies: &mut Bodies, gravity: Vec2, dt: f32) {
        let m = &state::<MotionState>(st).settings;
        let pull = (gravity * m.gravity_scale + m.wind) * dt;
        let keep = (1.0 - m.drag * dt).max(0.0);
        for (v, alive) in bodies.velocity.iter_mut().zip(&bodies.alive) {
            if *alive {
                *v = (*v + pull) * keep;
            }
        }
    }

    fn light_pull(&self, st: &dyn ThingState) -> Option<bpe_runtime::LightPull> {
        state::<MotionState>(st).settings.light_pull()
    }

    fn air_grip(&self, st: &dyn ThingState) -> f32 {
        state::<MotionState>(st).settings.air_grip()
    }

    fn air_key(&self) -> Option<&'static str> {
        Some(super::FOLLOW_AIR)
    }

    fn per_ball(&self) -> bool {
        true
    }

    /// The same as `forces`, on one ball with its own settings.
    fn forces_one(&self, st: &dyn ThingState, bodies: &mut Bodies, item: usize, gravity: Vec2, dt: f32, _cx: &SimCx) {
        let m = &state::<MotionState>(st).settings;
        let pull = (gravity * m.gravity_scale + m.wind) * dt;
        let keep = (1.0 - m.drag * dt).max(0.0);
        let v = &mut bodies.velocity[item];
        *v = (*v + pull) * keep;
    }
}

#[cfg(test)]
mod tests {
    use bpe_core::{SlotId, ThingId, Value, resolve};
    use bpe_runtime::{EventKind, Log, World};

    use crate::testing::{BALLS, one_ball_with_motion};

    /// A ball pulled down five times as hard as the scene's gravity, with almost no bounce, lies
    /// on the floor without hitting it every step.
    #[test]
    fn a_ball_pulled_hard_comes_to_rest_without_hitting_every_step() {
        let (mut scene, _, kit) = one_ball_with_motion(|p| p.set("gravity_scale", Value::Number(5.0)));
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 500.0]));
        let ball = scene.things.get_mut(&BALLS).unwrap();
        ball.params.set("velocity", Value::Vec2([0.0, 0.0]));
        scene.card_mut(BALLS, SlotId::Bounce).unwrap().params.set("bounciness", Value::Number(0.1));
        let resolved = resolve(&scene, &kit.registry);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        w.advance_to(3.0, &kit.sims, &mut log);
        let late = log.events().iter().filter(|e| e.kind == EventKind::Hit && e.step > 2 * 240).count();
        assert!(late < 5, "{late} hits in the last second of lying still");
        assert!(w.bodies[&BALLS].velocity[0].length() < 50.0, "it lies still");
    }

    /// Wind pressing a ball into a wall counts too.
    #[test]
    fn a_ball_pushed_by_wind_into_a_wall_rests_there() {
        let (mut scene, _, kit) = one_ball_with_motion(|p| {
            p.set("gravity_scale", Value::Number(0.0));
            p.set("wind", Value::Vec2([3000.0, 0.0]));
        });
        scene.things.get_mut(&BALLS).unwrap().params.set("velocity", Value::Vec2([0.0, 0.0]));
        scene.card_mut(BALLS, SlotId::Bounce).unwrap().params.set("bounciness", Value::Number(0.1));
        let resolved = resolve(&scene, &kit.registry);
        let mut w = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        w.advance_to(3.0, &kit.sims, &mut log);
        let late = log.events().iter().filter(|e| e.kind == EventKind::Hit && e.step > 2 * 240).count();
        assert!(late < 5, "{late} hits in the last second of lying still");
    }

    #[test]
    fn wind_pushes_balls_sideways() {
        let (scene, resolved, kit) = one_ball_with_motion(|p| p.set("wind", Value::Vec2([400.0, 0.0])));
        let mut w = World::build(&scene, &resolved, &kit.sims);
        let x0 = w.bodies[&BALLS].position[0].x;
        w.advance_to(0.25, &kit.sims, &mut Log::default());
        // 300 px/s launch plus 400 px/s² wind for a quarter second: about 75 + 12.5 px.
        assert!((w.bodies[&BALLS].position[0].x - x0 - 87.5).abs() < 2.0, "{}", w.bodies[&BALLS].position[0].x - x0);
    }
}
