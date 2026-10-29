//! Running a Rules module: compile its graph once, then run the VM on every step's events.

use bpe_core::{ModuleGraph, Params, SlotId};
use bpe_runtime::{ModuleSim, RuleCx, ThingState, state_mut};
use bpe_vm::{Act, Happening, Host, Program, Vm};

use crate::graphs::{WorldHost, action, compile_module, happenings};

pub struct RulesSim;

/// What a Rules module keeps while the world runs.
#[derive(Clone, Debug)]
pub struct RulesState {
    /// The compiled graph; `None` when there is nothing to run.
    pub program: Option<Program>,
    /// The running VM, made on the first step so it can take the scene's seed.
    pub vm: Option<Vm>,
    /// Why the graph didn't compile, when it didn't.
    pub error: Option<String>,
}

impl ModuleSim for RulesSim {
    fn kind(&self) -> &'static str {
        "rules.graph"
    }

    fn prepare(&self, _params: &Params) -> Box<dyn ThingState> {
        Box::new(RulesState { program: None, vm: None, error: None })
    }

    fn prepare_graph(&self, _params: &Params, graph: Option<&ModuleGraph>) -> Box<dyn ThingState> {
        let mut st = RulesState { program: None, vm: None, error: None };
        if let Some(g) = graph {
            match compile_module(SlotId::Rules, &g.graph) {
                Ok(p) if !p.is_empty() => st.program = Some(p),
                Ok(_) => {}
                Err(e) => st.error = Some(e),
            }
        }
        Box::new(st)
    }

    fn rules(&self, state: &mut dyn ThingState, cx: &mut RuleCx) {
        let st = state_mut::<RulesState>(state);
        let Some(program) = &st.program else { return };
        let seed = cx.world.seed ^ cx.thing.0 as u64;
        let vm = st.vm.get_or_insert_with(|| Vm::new(seed));
        let host = WorldHost { world: cx.world, me: cx.thing };
        let happenings: Vec<Happening> = cx.events.iter().flat_map(|e| happenings(e, cx.world)).collect();
        let mut acts = Vec::new();
        vm.run(program, &host, cx.world.time() as f32, &happenings, &mut acts);
        for act in acts {
            let point = match &act {
                Act::Fire { ball, .. } => host.ball(*ball).map(|b| b.position).unwrap_or_default(),
                _ => glam::Vec2::ZERO,
            };
            cx.actions.push(action(act, point));
        }
    }
}
