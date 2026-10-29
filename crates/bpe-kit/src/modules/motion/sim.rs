//! Gravity and drag on every ball that uses the module. A module whose graph has been edited
//! runs the graph on every ball instead.

use bpe_core::{ModuleGraph, Params, Settings, SlotId};
use bpe_runtime::{Bodies, ModuleSim, SimCx, ThingState, state};
use bpe_vm::{Ball, Program, Val, Vm};
use glam::Vec2;

use super::BasicMotion;
use crate::graphs::{SeedHost, bind, compile_module};

pub struct BasicMotionSim;

/// What a motion module keeps while the world runs.
#[derive(Clone, Debug)]
pub struct MotionState {
    pub settings: BasicMotion,
    /// The compiled graph, only for an edited module whose graph compiles.
    pub program: Option<Program>,
}

impl ModuleSim for BasicMotionSim {
    fn kind(&self) -> &'static str {
        "motion.basic"
    }

    fn prepare(&self, params: &Params) -> Box<dyn ThingState> {
        Box::new(MotionState { settings: BasicMotion::from_params(params), program: None })
    }

    fn prepare_graph(&self, params: &Params, graph: Option<&ModuleGraph>) -> Box<dyn ThingState> {
        let mut st = MotionState { settings: BasicMotion::from_params(params), program: None };
        if let Some(g) = graph
            && g.custom
            && let Ok(p) = compile_module(SlotId::Motion, &bind(&g.graph, params))
            && !p.is_empty()
        {
            st.program = Some(p);
        }
        Box::new(st)
    }

    fn forces(&self, st: &dyn ThingState, bodies: &mut Bodies, gravity: Vec2, dt: f32) {
        let m = &state::<MotionState>(st).settings;
        let pull = gravity * m.gravity_scale * dt;
        let keep = (1.0 - m.drag * dt).max(0.0);
        for (v, alive) in bodies.velocity.iter_mut().zip(&bodies.alive) {
            if *alive {
                *v = (*v + pull) * keep;
            }
        }
    }

    fn forces_at(&self, st: &dyn ThingState, bodies: &mut Bodies, gravity: Vec2, dt: f32, cx: &SimCx) {
        let st = state::<MotionState>(st);
        let Some(program) = &st.program else { return self.forces(st, bodies, gravity, dt) };
        let m = &st.settings;
        let pull = gravity * m.gravity_scale * dt;
        let keep = (1.0 - m.drag * dt).max(0.0);
        let host = SeedHost(cx.seed);
        for i in 0..bodies.len() {
            if !bodies.alive[i] {
                continue;
            }
            let v = bodies.velocity[i];
            let inputs = [
                Val::Ball(Some(Ball { thing: cx.thing, item: i as u32 })),
                Val::Vector(bodies.position[i]),
                Val::Vector(v),
                Val::Vector(gravity),
                Val::Number(dt),
                Val::Number(cx.time),
                Val::Number(bodies.radius[i]),
            ];
            let salt = (i as u64) << 32 | cx.time.to_bits() as u64;
            let out = Vm::eval(program, &host, cx.time, &inputs, salt);
            bodies.velocity[i] = if out.wired(0) { out.vector(0, v) } else { (v + pull) * keep };
        }
    }
}
