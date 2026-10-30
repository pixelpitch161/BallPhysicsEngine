//! Compiling a patch graph into a flat program over numbered buffers, and running it for one
//! voice.

use crate::catalog::{catalog, make};
use crate::{BLOCK, Block, Buf, Ctx, Knobs, SILENT, Sources};
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
    /// An effects graph's In: its register holds the sound coming in, set before each block.
    input: bool,
    /// Knobs moved by wires into their knob pins.
    mods: Vec<KnobMod>,
}

/// A knob a wire adds to while the block runs.
#[derive(Clone, Debug)]
struct KnobMod {
    name: String,
    register: usize,
    /// The knob's own value, which the wire adds to.
    base: f32,
    min: f32,
    max: f32,
    /// Where the voice keeps the value it last applied.
    slot: usize,
}

/// A compiled patch: blocks in running order, wired through registers.
#[derive(Clone, Debug)]
pub struct Program {
    ops: Arc<[Op]>,
    registers: usize,
    /// The Voice out (or effects Out) block's register.
    out: usize,
    /// An effects graph's In register, when it has one.
    input: Option<usize>,
    /// Knob mods across all blocks.
    mod_slots: usize,
}

/// One voice's running copy of a program.
pub struct Patch {
    program: Program,
    blocks: Vec<Option<Box<dyn Block>>>,
    regs: Vec<Buf>,
    /// The value each knob mod last set, as bits; NaN before the first.
    applied: Vec<u32>,
    /// Where a block writes its outputs before they go to their registers.
    scratch: [Buf; MAX_PINS],
}

/// What a graph is compiled as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mode {
    /// A synth patch, played per voice, ending in Voice out.
    Patch,
    /// An effects graph, run over a whole track, from In to Out.
    Effects,
}

/// Compiles `graph`, or says in plain sentences what stops it.
pub fn compile(graph: &Graph) -> Result<Program, Vec<Diagnostic>> {
    compile_as(graph, Mode::Patch)
}

/// Compiles an effects graph, or says in plain sentences what stops it.
pub fn compile_effects(graph: &Graph) -> Result<Program, Vec<Diagnostic>> {
    compile_as(graph, Mode::Effects)
}

/// Whether a block kind ends a graph: one register, its right side optional.
fn is_end(kind: &str) -> bool {
    kind == "voice_out" || kind == "fx_out"
}

fn compile_as(graph: &Graph, mode: Mode) -> Result<Program, Vec<Diagnostic>> {
    let cat = catalog();
    let mut problems: Vec<Diagnostic> = validate(graph, cat).into_iter().filter(|d| d.message != EXTRA_WIRE).collect();
    let ids = |kind: &str| -> Vec<u32> { graph.nodes.iter().filter(|n| n.kind == kind).map(|n| n.id).collect() };
    let (end, wrong_end, ins) = match mode {
        Mode::Patch => ("voice_out", "fx_out", Vec::new()),
        Mode::Effects => ("fx_out", "voice_out", ids("fx_in")),
    };
    let outs = ids(end);
    let mut first = Vec::new();
    if let Some(&n) = ids(wrong_end).first() {
        let message = match mode {
            Mode::Patch => "Out is for effects; use Voice out",
            Mode::Effects => "Use In and Out, not Voice out, in effects",
        };
        first.push(Diagnostic { node: Some(n), link: None, message: message.into() });
    }
    if mode == Mode::Patch
        && let Some(&n) = ids("fx_in").first()
    {
        first.push(Diagnostic { node: Some(n), link: None, message: "In is for effects; use Note".into() });
    }
    if mode == Mode::Effects {
        for n in graph.nodes.iter().filter(|n| n.kind == "hit_input" || n.kind == "live_input") {
            let message = "At-the-hit and live inputs only work in instrument patches".into();
            first.push(Diagnostic { node: Some(n.id), link: None, message });
        }
    }
    if outs.len() != 1 {
        let message = match (mode, outs.is_empty()) {
            (Mode::Patch, true) => "Add a Voice out block",
            (Mode::Patch, false) => "Use only one Voice out block",
            (Mode::Effects, true) => "Add an Out block",
            (Mode::Effects, false) => "Use only one Out block",
        };
        first.insert(0, Diagnostic { node: outs.get(1).copied(), link: None, message: message.into() });
    }
    if ins.len() > 1 {
        first.push(Diagnostic { node: Some(ins[1]), link: None, message: "Use only one In block".into() });
    }
    // First, since they're the ones to fix first.
    problems.splice(0..0, first);
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

    // A register for every output pin, plus one for the end block.
    let mut register = BTreeMap::<(u32, u8), usize>::new();
    let mut registers = 0;
    for node in &graph.nodes {
        let spec = cat.spec(&node.kind).expect("validated");
        let pins = if is_end(&node.kind) { 1 } else { spec.outputs.len() };
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
    let mut mod_slots = 0;
    let ops = sequence
        .iter()
        .map(|&id| {
            let node = graph.node(id).expect("ordered from the graph");
            let spec = cat.spec(&node.kind).expect("validated");
            let mut knobs = Knobs(spec.params.iter().map(|p| (p.name.clone(), p.default.clone())).collect());
            knobs.0.extend(node.params.iter().map(|(k, v)| (k.clone(), v.clone())));
            let pins = if is_end(&node.kind) { 1 } else { spec.outputs.len() };
            let followable = crate::followable(&node.kind);
            let own = spec.inputs.len() - followable.len();
            let from = |pin: usize| wire.get(&(id, pin as u8)).and_then(|from| register.get(from).copied());
            let mods = followable
                .iter()
                .enumerate()
                .filter_map(|(i, name)| {
                    let register = from(own + i)?;
                    let range = spec.param_spec(name).map_or((f32::MIN, f32::MAX), |p| p.range);
                    let slot = mod_slots;
                    mod_slots += 1;
                    Some(KnobMod {
                        name: name.to_string(),
                        register,
                        base: knobs.number(name, 0.0),
                        min: range.0,
                        max: range.1,
                        slot,
                    })
                })
                .collect();
            Op {
                kind: node.kind.clone(),
                inputs: (0..own).map(from).collect(),
                mods,
                knobs,
                outputs: (0..pins).map(|pin| register[&(id, pin as u8)]).collect(),
                enabled: node.enabled || is_end(&node.kind) || node.kind == "fx_in",
                input: node.kind == "fx_in",
            }
        })
        .collect();
    let input = ins.first().map(|&id| register[&(id, 0)]);
    Ok(Program { ops, registers, out: register[&(outs[0], 0)], input, mod_slots })
}

impl Program {
    /// A fresh voice of this patch, ready for a note.
    pub fn instantiate(&self, cx: &Ctx) -> Patch {
        self.instantiate_with(cx, &Sources::default())
    }

    /// A fresh voice reading `sources`.
    pub fn instantiate_with(&self, cx: &Ctx, sources: &Sources) -> Patch {
        let blocks = self
            .ops
            .iter()
            .map(|op| {
                let mut b = make(&op.kind, &op.knobs).filter(|_| op.enabled)?;
                b.reset(cx);
                b.bind(sources);
                Some(b)
            })
            .collect();
        Patch {
            program: self.clone(),
            blocks,
            regs: vec![SILENT; self.registers],
            applied: vec![f32::NAN.to_bits(); self.mod_slots],
            scratch: [SILENT; MAX_PINS],
        }
    }

    /// Runs an effects program over interleaved stereo `audio` in place, a block at a time from
    /// `cx`'s time. The last part block is padded with silence.
    pub fn run(&self, audio: &mut [f32], cx: Ctx) {
        self.run_with(audio, cx, &Sources::default())
    }

    /// [`Program::run`], with input blocks reading `sources`.
    pub fn run_with(&self, audio: &mut [f32], mut cx: Ctx, sources: &Sources) {
        let mut patch = self.instantiate_with(&cx, sources);
        for chunk in audio.chunks_mut(BLOCK * 2) {
            let mut input = SILENT;
            for (s, frame) in chunk.chunks_exact(2).enumerate() {
                input[0][s] = frame[0];
                input[1][s] = frame[1];
            }
            patch.set_input(&input);
            let mut out = SILENT;
            patch.process(&cx, &mut out);
            for (s, frame) in chunk.chunks_exact_mut(2).enumerate() {
                frame[0] = out[0][s];
                frame[1] = out[1][s];
            }
            cx.advance();
        }
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
            if op.input {
                continue;
            }
            if let Some(b) = block {
                for m in &op.mods {
                    let wired = &self.regs[m.register][0];
                    let add = wired.iter().sum::<f32>() / BLOCK as f32;
                    let v = if add.is_finite() { (m.base + add).clamp(m.min, m.max) } else { m.base };
                    if self.applied[m.slot] != v.to_bits() {
                        self.applied[m.slot] = v.to_bits();
                        b.set_knob(&m.name, v);
                    }
                }
            }
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

    /// Hands every block new sources, as a mono voice does on a new note.
    pub fn bind(&mut self, sources: &Sources) {
        for b in self.blocks.iter_mut().flatten() {
            b.bind(sources);
        }
    }

    /// Sets the sound coming into an effects graph's In for the next [`Patch::process`].
    pub fn set_input(&mut self, input: &Buf) {
        if let Some(r) = self.program.input {
            self.regs[r] = *input;
        }
    }

    /// Whether every block has finished (envelopes released).
    pub fn idle(&self) -> bool {
        self.blocks.iter().flatten().all(|b| b.idle())
    }
}

/// Everything about a patch graph that changes how it sounds, as text to hash: its blocks,
/// knobs, exposed macros and wires, but not where blocks sit, its notes or its name.
pub fn sound_key(graph: &Graph) -> String {
    let nodes: Vec<_> = graph.nodes.iter().map(|n| (n.id, &n.kind, &n.params, n.enabled, &n.exposed)).collect();
    format!("{:?} {:?} {}", nodes, graph.links, graph.enabled)
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
    fn hit_inputs_are_patch_only() {
        let cat = catalog();
        let message = "At-the-hit and live inputs only work in instrument patches";
        let mut fx = crate::chain_graph(&[("amp".into(), Default::default(), true)]);
        let amp = fx.nodes.iter().find(|n| n.kind == "amp").unwrap().id;
        let input = fx.add(cat.spec("hit_input").unwrap(), [0.0, 0.0]);
        assert!(fx.connect(Link { from: (input, 0), to: (amp, 1) }, cat));
        let problems = compile_effects(&fx).unwrap_err();
        assert!(problems.iter().any(|d| d.message == message && d.node == Some(input)), "{problems:?}");

        let mut patch = blank_patch();
        let amp = patch.nodes.iter().find(|n| n.kind == "amp").unwrap().id;
        let input = patch.add(cat.spec("hit_input").unwrap(), [0.0, 0.0]);
        assert!(patch.connect(Link { from: (input, 0), to: (amp, 1) }, cat));
        assert!(compile(&patch).is_ok());
    }

    #[test]
    fn uses_lists_inputs() {
        let cat = catalog();
        let mut g = blank_patch();
        g.add(cat.spec("live_input").unwrap(), [0.0, 0.0]);
        let s = g.add(cat.spec("scene_input").unwrap(), [0.0, 0.0]);
        let node = g.node_mut(s).unwrap();
        node.params.insert("input".into(), Param::Choice("channel".into()));
        node.params.insert("channel".into(), Param::Text("Kick".into()));
        let expected = crate::Uses { hit: false, live: true, scene: true, channels: ["Kick".to_string()].into() };
        assert_eq!(crate::uses(&g), expected);
    }

    /// `kind` between In and Out, with a macro (value 1) through a scale of `amount` into the
    /// knob pin for `knob`.
    fn followed_chain(kind: &str, knob: &str, amount: f32, knobs: &[(&str, Param)]) -> Graph {
        let cat = catalog();
        let mut g =
            crate::chain_graph(&[(kind.into(), knobs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(), true)]);
        let target = g.nodes.iter().find(|n| n.kind == kind).unwrap().id;
        let m = g.add(cat.spec("macro").unwrap(), [0.0, 200.0]);
        g.node_mut(m).unwrap().params.insert("value".into(), Param::Number(1.0));
        let sc = g.add(cat.spec("scale").unwrap(), [0.0, 200.0]);
        g.node_mut(sc).unwrap().params.insert("scale".into(), Param::Number(amount));
        let spec = cat.spec(kind).unwrap();
        let pin = spec.inputs.len() - crate::followable(kind).len()
            + crate::followable(kind).iter().position(|k| *k == knob).unwrap();
        assert!(g.connect(Link { from: (m, 0), to: (sc, 0) }, cat));
        assert!(g.connect(Link { from: (sc, 0), to: (target, pin as u8) }, cat), "{kind}.{knob}");
        if kind == "osc" {
            // The FM knob only matters with something in the FM input.
            let input = g.nodes.iter().find(|n| n.kind == "fx_in").unwrap().id;
            g.connect(Link { from: (input, 0), to: (target, 1) }, cat);
        }
        g
    }

    fn noise(frames: usize) -> Vec<f32> {
        let mut rng = crate::Rng::new(7);
        (0..frames * 2).map(|_| rng.next() * 0.5).collect()
    }

    #[test]
    fn every_followable_knob_moves_the_sound() {
        let cat = catalog();
        for spec in &cat.specs {
            let kind = spec.kind.as_str();
            if matches!(kind, "macro" | "lfo") {
                continue;
            }
            for knob in crate::followable(kind) {
                let p = spec.param_spec(knob).unwrap();
                // Wet effects so their mix knobs and tails are heard.
                let extra: Vec<(&str, Param)> = match kind {
                    "delay" => vec![("ms", Param::Number(20.0)), ("feedback", Param::Number(0.5))],
                    "osc" => vec![("unison", Param::Number(3.0)), ("wave", Param::Choice("pulse".into()))],
                    _ => vec![],
                };
                let render = |amount: f32| {
                    let g = followed_chain(kind, knob, amount, &extra);
                    let program = compile_effects(&g).unwrap_or_else(|e| panic!("{kind}.{knob}: {e:?}"));
                    let mut audio = noise(24_000);
                    program.run(&mut audio, Ctx::default());
                    audio
                };
                let span = p.range.1 - p.range.0;
                let (a, b) = (render(-span), render(span));
                let diff = a.iter().zip(&b).fold(0.0f32, |m, (x, y)| m.max((x - y).abs()));
                assert!(diff > 1e-4, "{kind}.{knob} doesn't move the sound ({diff})");
            }
        }
        // An LFO's rate and a macro's value, through an amp's mod.
        for (kind, knob) in [("lfo", "rate"), ("macro", "value")] {
            let render = |amount: f32| {
                let mut g = followed_chain("amp", "gain", 0.0, &[]);
                let amp = g.nodes.iter().find(|n| n.kind == "amp").unwrap().id;
                let source = g.add(cat.spec(kind).unwrap(), [0.0, 400.0]);
                let m = g.add(cat.spec("macro").unwrap(), [0.0, 400.0]);
                g.node_mut(m).unwrap().params.insert("value".into(), Param::Number(1.0));
                let sc = g.add(cat.spec("scale").unwrap(), [0.0, 400.0]);
                g.node_mut(sc).unwrap().params.insert("scale".into(), Param::Number(amount));
                let pin = cat.spec(kind).unwrap().inputs.len() - 1;
                g.connect(Link { from: (m, 0), to: (sc, 0) }, cat);
                g.connect(Link { from: (sc, 0), to: (source, pin as u8) }, cat);
                g.connect(Link { from: (source, 0), to: (amp, 1) }, cat);
                let mut audio = noise(24_000);
                compile_effects(&g).unwrap().run(&mut audio, Ctx::default());
                audio
            };
            let (a, b) = (render(0.0), render(if kind == "lfo" { 20.0 } else { -0.5 }));
            let diff = a.iter().zip(&b).fold(0.0f32, |m, (x, y)| m.max((x - y).abs()));
            assert!(diff > 1e-4, "{kind}.{knob} doesn't move the sound ({diff})");
        }
    }

    #[test]
    fn a_knob_pin_adds_to_the_knob() {
        let rms = |g: &Graph| {
            let mut audio = noise(24_000);
            compile_effects(g).unwrap().run(&mut audio, Ctx::default());
            rms(&audio)
        };
        let plain = rms(&crate::chain_graph(&[("amp".into(), Default::default(), true)]));
        let low = [("gain", Param::Number(-60.0))];
        let lifted = rms(&followed_chain("amp", "gain", 60.0, &low));
        assert!((lifted / plain - 1.0).abs() < 0.01, "{lifted} vs {plain}");
        let capped = rms(&followed_chain("amp", "gain", 100.0, &low));
        assert!((capped / plain / 3.981 - 1.0).abs() < 0.01, "{}", capped / plain);
    }

    #[test]
    fn a_switched_off_block_passes_its_input_through() {
        let mut g = blank_patch();
        let filter = g.nodes.iter().find(|n| n.kind == "filter").unwrap().id;
        g.node_mut(filter).unwrap().enabled = false;
        assert!(rms(&play(&compile(&g).unwrap(), 100, 100)) > 0.01);
    }
}
