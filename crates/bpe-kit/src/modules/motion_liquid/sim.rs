//! Scaled gravity on every particle that uses the module. Viscosity, stiffness and the ball
//! push are read by the liquid itself when it settles.

use bpe_core::{Params, Settings};
use bpe_runtime::{Bodies, ModuleSim, ThingState, state};
use glam::Vec2;

use super::Flowing;

pub struct FlowingSim;

impl ModuleSim for FlowingSim {
    fn kind(&self) -> &'static str {
        "motion.liquid"
    }

    fn prepare(&self, params: &Params) -> Box<dyn ThingState> {
        Box::new(Flowing::from_params(params))
    }

    fn forces(&self, st: &dyn ThingState, bodies: &mut Bodies, gravity: Vec2, dt: f32) {
        let pull = gravity * state::<Flowing>(st).gravity_scale * dt;
        for (v, alive) in bodies.velocity.iter_mut().zip(&bodies.alive) {
            if *alive {
                *v += pull;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Settings;
    use bpe_runtime::{Bodies, ModuleSim};
    use glam::Vec2;

    #[test]
    fn flowing_pulls_with_scaled_gravity_and_no_drag() {
        let sim = FlowingSim;
        let state = sim.prepare(&Flowing { gravity_scale: 2.0, ..Flowing::default() }.to_params());
        let mut bodies = Bodies::default();
        bodies.push(Vec2::ZERO, Vec2::new(10.0, 0.0), 1.0);
        bodies.push(Vec2::ZERO, Vec2::ZERO, 1.0);
        bodies.alive[1] = false;
        sim.forces(state.as_ref(), &mut bodies, Vec2::new(0.0, 100.0), 0.5);
        assert_eq!(bodies.velocity[0], Vec2::new(10.0, 100.0));
        assert_eq!(bodies.velocity[1], Vec2::ZERO, "dead particles are left alone");
    }
}
