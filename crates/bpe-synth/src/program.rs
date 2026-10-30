//! Compiling a patch graph into a flat program over numbered buffers, and running it for one
//! voice.

use crate::catalog::{catalog, make};
use crate::{Block, Buf, Ctx, Knobs, SILENT};
use bpe_graph::{Catalog, Diagnostic, Graph, Link, Param, order, validate};
use std::collections::BTreeMap;
use std::sync::Arc;

/// Most input and output pins any block has.
const MAX_PINS: usize = 4;

/// The one validator message that is a warning rather than an error.
const EXTRA_WIRE: &str = "This input has more than one wire; only one is used.";

#[derive(Clone, Debug)]
struct Op {
    kind: String,
    knobs: Knobs,
    /// A register per input pin, or `None` when nothing is wired in.
    inputs: Vec<Option<usize>>,
    /// A register per output pin.
    outputs: Vec<usize>,
    /// A switched-off block passes its first input through.
    enabled: bool,
}

/// A compiled patch: blocks in running order, wired through registers.
#[derive(Clone, Debug)]
pub struct Program {
    ops: Arc<[Op]>,
    registers: usize,
    /// The Voice out block's register.
    out: usize,
}

/// One voice's running copy of a program.
pub struct Patch {
    program: Program,
    blocks: Vec<Option<Box<dyn Block>>>,
    regs: Vec<Buf>,
    /// Where a block writes its outputs before they go to their registers.
    scratch: [Buf; MAX_PINS],
}

/// Compiles `graph`, or says in plain sentences what stops it.
pub fn compile(graph: &Graph) -> Result<Program, Vec<Diagnostic>> {
    let cat = catalog();
    let mut problems: Vec<Diagnostic> = validate(graph, cat).into_iter().filter(|d| d.message != EXTRA_WIRE).collect();
    let outs: Vec<u32> = graph.nodes.iter().filter(|n| n.kind == "voice_out").map(|n| n.id).collect();
    if outs.len() != 1 {
        let message = if outs.is_empty() { "Add a Voice out block" } else { "Use only one Voice out block" };
        // First, since it's the one to fix first.
        problems.insert(0, Diagnostic { node: outs.get(1).copied(), link: None, message: message.into() });
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let sequence = order(graph).map_err(|cycle| {
        cycle
            .nodes
            .iter()
            .map(|&n| Diagnostic { node: Some(n), link: None, message: "Wires go round in a loop".into() })
            .collect::<Vec<_>>()
    })?;

    // A register for every output pin, plus one for Voice out.
    let mut register = BTreeMap::<(u32, u8), usize>::new();
    let mut registers = 0;
    for node in &graph.nodes {
        let spec = cat.spec(&node.kind).expect("validated");
        let pins = if node.kind == "voice_out" { 1 } else { spec.outputs.len() };
        for pin in 0..pins {
            register.insert((node.id, pin as u8), registers);
            registers += 1;
        }
    }
    // The first wire into each input wins.
    let mut wire = BTreeMap::<(u32, u8), (u32, u8)>::new();
    for &Link { from, to } in &graph.links {
        wire.entry(to).or_insert(from);
    }
    let ops = sequence
        .iter()
        .map(|&id| {
            let node = graph.node(id).expect("ordered from the graph");
            let spec = cat.spec(&node.kind).expect("validated");
            let mut knobs = Knobs(spec.params.iter().map(|p| (p.name.clone(), p.default.clone())).collect());
            knobs.0.extend(node.params.iter().map(|(k, v)| (k.clone(), v.clone())));
            let pins = if node.kind == "voice_out" { 1 } else { spec.outputs.len() };
            Op {
                kind: node.kind.clone(),
                knobs,
                inputs: (0..spec.inputs.len())
                    .map(|pin| wire.get(&(id, pin as u8)).and_then(|from| register.get(from).copied()))
                    .collect(),
                outputs: (0..pins).map(|pin| register[&(id, pin as u8)]).collect(),
                enabled: node.enabled || node.kind == "voice_out",
            }
        })
        .collect();
    Ok(Program { ops, registers, out: register[&(outs[0], 0)] })
}

impl Program {
    /// A fresh voice of this patch, ready for a note.
    pub fn instantiate(&self, cx: &Ctx) -> Patch {
        let blocks = self
            .ops
            .iter()
            .map(|op| {
                let mut b = make(&op.kind, &op.knobs).filter(|_| op.enabled)?;
                b.reset(cx);
                Some(b)
            })
            .collect();
        Patch { program: self.clone(), blocks, regs: vec![SILENT; self.registers], scratch: [SILENT; MAX_PINS] }
    }

    /// The same program with every block of `kind` swapped for `by`.
    #[cfg(test)]
    pub(crate) fn with_block_replaced(&self, kind: &str, by: &str) -> Program {
        let ops: Vec<Op> = self
            .ops
            .iter()
            .map(|op| if op.kind == kind { Op { kind: by.into(), ..op.clone() } } else { op.clone() })
            .collect();
        Program { ops: ops.into(), ..self.clone() }
    }
}

impl Patch {
    /// Runs one block of the voice into `out`.
    pub fn process(&mut self, cx: &Ctx, out: &mut Buf) {
        for (op, block) in self.program.ops.iter().zip(&mut self.blocks) {
            let n = op.outputs.len().min(MAX_PINS);
            let outs = &mut self.scratch;
            for o in outs.iter_mut().take(n.max(1)) {
                *o = SILENT;
            }
            {
                let mut ins: [Option<&Buf>; MAX_PINS] = [None; MAX_PINS];
                for (slot, reg) in ins.iter_mut().zip(&op.inputs) {
                    *slot = reg.map(|r| &self.regs[r]);
                }
                let ins = &ins[..op.inputs.len().min(MAX_PINS)];
                match block {
                    Some(b) => b.process(cx, ins, &mut outs[..n.max(1)]),
                    None => outs[0] = ins.first().copied().flatten().copied().unwrap_or(SILENT),
                }
            }
            for (i, &reg) in op.outputs.iter().enumerate().take(MAX_PINS) {
                self.regs[reg] = outs[i];
            }
        }
        *out = self.regs[self.program.out];
    }

    /// Whether every block has finished (envelopes released).
    pub fn idle(&self) -> bool {
        self.blocks.iter().flatten().all(|b| b.idle())
    }
}

/// One exposed knob of a patch, shown on the module card as a slider.
#[derive(Clone, Debug, PartialEq)]
pub struct MacroKnob {
    pub node: u32,
    pub param: String,
    /// "<block title> <knob label>".
    pub label: String,
    pub min: f32,
    pub max: f32,
    pub value: f32,
}

impl MacroKnob {
    /// How a module's override of this macro is keyed.
    pub fn key(&self) -> String {
        format!("{}.{}", self.node, self.param)
    }
}

/// The patch's exposed number knobs, in block order.
pub fn macros(graph: &Graph) -> Vec<MacroKnob> {
    let cat = catalog();
    let mut out = Vec::new();
    for node in &graph.nodes {
        let Some(spec) = cat.spec(&node.kind) else { continue };
        for name in &node.exposed {
            let Some(p) = spec.param_spec(name) else { continue };
            let Param::Number(_) = p.default else { continue };
            out.push(MacroKnob {
                node: node.id,
                param: name.clone(),
                label: format!("{} {}", spec.title, p.label),
                min: p.range.0,
                max: p.range.1,
                value: node.number(spec, name),
            });
        }
    }
    out
}

/// `graph` with macro values overridden, keyed "<node>.<param>".
pub fn with_macros(graph: &Graph, values: &BTreeMap<String, f32>) -> Graph {
    let mut g = graph.clone();
    for (key, &v) in values {
        let Some((node, param)) = key.split_once('.') else { continue };
        let Some(node) = node.parse().ok().and_then(|id| g.node_mut(id)) else { continue };
        if node.exposed.contains(param) {
            node.params.insert(param.into(), Param::Number(v));
        }
    }
    g
}

/// A patch to start from: a saw through a filter and an envelope-shaped amp.
pub fn blank_patch() -> Graph {
    let cat = catalog();
    let mut g = Graph::new("Patch");
    let mut add = |kind: &str, x: f32, y: f32| g.add(cat.spec(kind).expect("in the catalog"), [x, y]);
    let osc = add("osc", 180.0, 0.0);
    let filter = add("filter", 360.0, 0.0);
    let env = add("adsr", 360.0, 160.0);
    let amp = add("amp", 540.0, 0.0);
    let out = add("voice_out", 720.0, 0.0);
    for link in [
        Link { from: (osc, 0), to: (filter, 0) },
        Link { from: (filter, 0), to: (amp, 0) },
        Link { from: (env, 0), to: (amp, 1) },
        Link { from: (amp, 0), to: (out, 0) },
    ] {
        g.connect(link, cat);
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BLOCK;

    /// Plays note 60 through `program` for `blocks` blocks, holding the gate for `gate` blocks.
    pub(crate) fn play(program: &Program, blocks: usize, gate: usize) -> Vec<f32> {
        let mut cx = Ctx::default();
        let mut patch = program.instantiate(&cx);
        let mut out = Vec::new();
        for b in 0..blocks {
            cx.gate = b < gate;
            let mut buf = SILENT;
            patch.process(&cx, &mut buf);
            for s in 0..BLOCK {
                out.extend([buf[0][s], buf[1][s]]);
            }
            cx.advance();
        }
        out
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn blank_patch_compiles_and_sounds() {
        let program = compile(&blank_patch()).unwrap();
        let out = play(&program, 375, 375);
        assert!(rms(&out) > 0.01, "rms {}", rms(&out));
    }

    #[test]
    fn no_voice_out_is_an_error_and_plays_silence() {
        let mut g = blank_patch();
        let out = g.nodes.iter().find(|n| n.kind == "voice_out").unwrap().id;
        g.remove(&[out].into());
        let problems = compile(&g).unwrap_err();
        assert!(problems.iter().any(|d| d.message == "Add a Voice out block"));
    }

    #[test]
    fn a_loop_is_rejected() {
        let cat = catalog();
        let mut g = blank_patch();
        let a = g.add(cat.spec("add").unwrap(), [0.0, 0.0]);
        let b = g.add(cat.spec("add").unwrap(), [0.0, 0.0]);
        // Wire them round in a loop without the validator's connect check.
        g.links.push(Link { from: (a, 0), to: (b, 0) });
        g.links.push(Link { from: (b, 0), to: (a, 0) });
        let problems = compile(&g).unwrap_err();
        let looped: std::collections::BTreeSet<u32> = problems.iter().filter_map(|d| d.node).collect();
        assert_eq!(looped, [a, b].into());
    }

    #[test]
    fn control_into_audio_and_back() {
        let cat = catalog();
        let mut g = blank_patch();
        let amp = g.nodes.iter().find(|n| n.kind == "amp").unwrap().id;
        let env = g.nodes.iter().find(|n| n.kind == "adsr").unwrap().id;
        let lfo = g.add(cat.spec("lfo").unwrap(), [0.0, 0.0]);
        let add = g.add(cat.spec("add").unwrap(), [0.0, 0.0]);
        g.links.retain(|l| l.from.0 != env);
        assert!(g.connect(Link { from: (lfo, 0), to: (amp, 1) }, cat));
        assert!(g.connect(Link { from: (amp, 0), to: (add, 0) }, cat));
        assert!(compile(&g).is_ok());
    }

    #[test]
    fn macros_list_exposed_knobs_and_override() {
        let mut g = blank_patch();
        let filter = g.nodes.iter().find(|n| n.kind == "filter").unwrap().id;
        g.node_mut(filter).unwrap().exposed.insert("cutoff".into());
        let knobs = macros(&g);
        assert_eq!(knobs.len(), 1);
        assert_eq!(knobs[0].label, "Filter Cutoff");
        assert_eq!(knobs[0].value, 2_000.0);
        assert_eq!((knobs[0].min, knobs[0].max), (20.0, 20_000.0));
        let changed = with_macros(&g, &[(knobs[0].key(), 500.0)].into());
        assert_eq!(macros(&changed)[0].value, 500.0);
        // Knobs that aren't exposed can't be overridden.
        let same = with_macros(&g, &[(format!("{filter}.resonance"), 1.0)].into());
        assert_eq!(same, g);
    }

    #[test]
    fn two_patches_render_identically() {
        let program = compile(&blank_patch()).unwrap();
        let a = play(&program, 200, 100);
        let b = play(&program, 200, 100);
        assert_eq!(
            a.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            b.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_switched_off_block_passes_its_input_through() {
        let mut g = blank_patch();
        let filter = g.nodes.iter().find(|n| n.kind == "filter").unwrap().id;
        g.node_mut(filter).unwrap().enabled = false;
        assert!(rms(&play(&compile(&g).unwrap(), 100, 100)) > 0.01);
    }
}
