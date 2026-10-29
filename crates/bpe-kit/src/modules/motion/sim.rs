//! Gravity and drag on every ball that uses the module.

use bpe_core::{Params, Settings};
use bpe_runtime::{Bodies, ModuleSim, ThingState, state};
use glam::Vec2;

use super::BasicMotion;

pub struct BasicMotionSim;

impl ModuleSim for BasicMotionSim {
    fn kind(&self) -> &'static str {
        "motion.basic"
    }

    fn prepare(&self, params: &Params) -> Box<dyn ThingState> {
        Box::new(BasicMotion::from_params(params))
    }

    fn forces(&self, st: &dyn ThingState, bodies: &mut Bodies, gravity: Vec2, dt: f32) {
        let m = state::<BasicMotion>(st);
        let pull = (gravity * m.gravity_scale + m.wind) * dt;
        let keep = (1.0 - m.drag * dt).max(0.0);
        for (v, alive) in bodies.velocity.iter_mut().zip(&bodies.alive) {
            if *alive {
                *v = (*v + pull) * keep;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use bpe_core::Value;
    use bpe_runtime::{Log, World};

    use crate::testing::{BALLS, one_ball_with_motion};

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
