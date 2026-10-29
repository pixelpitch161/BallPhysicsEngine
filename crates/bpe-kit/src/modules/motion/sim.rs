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
        let pull = gravity * m.gravity_scale * dt;
        let keep = (1.0 - m.drag * dt).max(0.0);
        for (v, alive) in bodies.velocity.iter_mut().zip(&bodies.alive) {
            if *alive {
                *v = (*v + pull) * keep;
            }
        }
    }
}
