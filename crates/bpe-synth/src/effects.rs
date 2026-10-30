//! Effects graphs: chains of effect blocks between an In and an Out, run over a whole track.
//! An empty graph is no effects at all, and the sound passes through untouched.

use crate::catalog::catalog;
use crate::program::{Program, compile_effects};
use bpe_graph::{Catalog, Graph, Link, Param};
use std::collections::{BTreeMap, BTreeSet};

/// The blocks the "Add effect" menu offers, in menu order.
pub fn effects_catalog_kinds() -> Vec<&'static str> {
    EFFECTS.to_vec()
}

const EFFECTS: &[&str] = &[
    "eq",
    "filter",
    "ladder",
    "drive",
    "bitcrush",
    "chorus",
    "delay",
    "reverb",
    "width",
    "compressor",
    "clipper",
    "limiter",
    "amp",
];

/// An effects graph ready to run: `(None, None)` for no effects, `(Some, None)` when it
/// compiles, and `(None, Some(first problem))` when it doesn't, so the sound passes through.
pub fn effects_or_through(graph: &Graph) -> (Option<Program>, Option<String>) {
    if graph.nodes.is_empty() {
        return (None, None);
    }
    match compile_effects(graph) {
        Ok(p) => (Some(p), None),
        Err(problems) => (None, Some(problems.first().map(|d| d.message.clone()).unwrap_or_default())),
    }
}

/// Whether a wire goes into a knob pin rather than one of the block's own inputs.
fn into_knob(graph: &Graph, link: &Link) -> bool {
    graph
        .node(link.to.0)
        .and_then(|n| catalog().spec(&n.kind).map(|s| (n, s)))
        .is_some_and(|(n, spec)| link.to.1 as usize >= spec.inputs.len() - crate::followable(&n.kind).len())
}

/// The "side" blocks: those whose outputs reach only knob pins, directly or through other side
/// blocks, such as the input and scale a Follow… adds.
fn side_blocks(graph: &Graph) -> BTreeSet<u32> {
    let mut side: BTreeSet<u32> = graph
        .nodes
        .iter()
        .filter(|n| !matches!(n.kind.as_str(), "fx_in" | "fx_out" | "voice_out"))
        .filter(|n| graph.links.iter().any(|l| l.from.0 == n.id))
        .map(|n| n.id)
        .collect();
    loop {
        let before = side.len();
        let keep: BTreeSet<u32> = side
            .iter()
            .copied()
            .filter(|id| {
                graph.links.iter().filter(|l| l.from.0 == *id).all(|l| into_knob(graph, l) || side.contains(&l.to.0))
            })
            .collect();
        side = keep;
        if side.len() == before {
            return side;
        }
    }
}

/// The blocks between In and Out in order, when the graph is a straight chain: each block
/// with one wire in and one wire out, nothing else wired. Blocks that only feed knob pins (a
/// followed knob's input) don't count. An empty graph is an empty chain.
pub fn straight_chain(graph: &Graph) -> Option<Vec<u32>> {
    if graph.nodes.is_empty() {
        return Some(Vec::new());
    }
    let side = side_blocks(graph);
    let mut graph = graph.clone();
    graph.nodes.retain(|n| !side.contains(&n.id));
    let knob_links: Vec<Link> = graph.links.iter().filter(|l| into_knob(&graph, l)).copied().collect();
    graph.links.retain(|l| !side.contains(&l.from.0) && !knob_links.contains(l));
    let graph = &graph;
    let one = |kind: &str| {
        let mut it = graph.nodes.iter().filter(|n| n.kind == kind);
        let first = it.next()?.id;
        it.next().is_none().then_some(first)
    };
    let (input, output) = (one("fx_in")?, one("fx_out")?);
    let mut chain = Vec::new();
    let mut at = input;
    loop {
        let mut next = graph.links.iter().filter(|l| l.from.0 == at);
        let link = next.next()?;
        if next.next().is_some() || link.from.1 != 0 || link.to.1 != 0 {
            return None;
        }
        if link.to.0 == output {
            break;
        }
        if link.to.0 == input || chain.contains(&link.to.0) {
            return None;
        }
        chain.push(link.to.0);
        at = link.to.0;
    }
    (chain.len() + 2 == graph.nodes.len() && graph.links.len() == chain.len() + 1).then_some(chain)
}

/// One block of an effects list: an existing block (`id`) or a new one, with its knobs.
#[derive(Clone, Debug, PartialEq)]
pub struct ChainBlock {
    pub id: Option<u32>,
    pub kind: String,
    pub params: BTreeMap<String, Param>,
    pub enabled: bool,
}

/// `graph` relinked as the chain `blocks`, In to Out: blocks kept by id keep their followed
/// knobs, new ones are added, and blocks left out go with whatever only fed their knobs.
pub fn rechain(graph: &Graph, blocks: &[ChainBlock]) -> Graph {
    let find = |kind: &str| graph.nodes.iter().find(|n| n.kind == kind).map(|n| n.id);
    let (Some(input), Some(output)) = (find("fx_in"), find("fx_out")) else {
        let list: Vec<_> = blocks.iter().map(|b| (b.kind.clone(), b.params.clone(), b.enabled)).collect();
        return chain_graph(&list);
    };
    let cat = catalog();
    let side = side_blocks(graph);
    let kept: BTreeSet<u32> = blocks.iter().filter_map(|b| b.id).collect();
    let mut g = graph.clone();
    // Unwire the chain, keeping wires into knob pins and out of side blocks.
    let keep: Vec<bool> = g.links.iter().map(|l| side.contains(&l.from.0) || into_knob(graph, l)).collect();
    let mut i = 0;
    g.links.retain(|_| {
        i += 1;
        keep[i - 1]
    });
    let gone: BTreeSet<u32> = graph
        .nodes
        .iter()
        .filter(|n| n.id != input && n.id != output && !side.contains(&n.id) && !kept.contains(&n.id))
        .map(|n| n.id)
        .collect();
    g.remove(&gone);
    // Side blocks that now feed nothing go too.
    loop {
        let idle: BTreeSet<u32> = side
            .iter()
            .copied()
            .filter(|id| g.node(*id).is_some() && !g.links.iter().any(|l| l.from.0 == *id))
            .collect();
        if idle.is_empty() {
            break;
        }
        g.remove(&idle);
    }
    let mut at = input;
    let mut x = g.node(input).map_or(0.0, |n| n.pos[0]);
    for b in blocks {
        let id = match b.id.filter(|id| g.node(*id).is_some()) {
            Some(id) => id,
            None => {
                let Some(spec) = cat.spec(&b.kind) else { continue };
                x += 180.0;
                g.add(spec, [x, 0.0])
            }
        };
        let node = g.node_mut(id).expect("kept or just added");
        node.params = b.params.clone();
        node.enabled = b.enabled;
        g.links.push(Link { from: (at, 0), to: (id, 0) });
        at = id;
    }
    g.links.push(Link { from: (at, 0), to: (output, 0) });
    g
}

/// A straight chain of `blocks` (kind, knobs, switched on) from In to Out, laid out left to
/// right. Kinds the catalog doesn't know are left out.
pub fn chain_graph(blocks: &[(String, BTreeMap<String, Param>, bool)]) -> Graph {
    let cat = catalog();
    let mut g = Graph::new("Effects");
    let mut x = 0.0;
    let mut at = g.add(cat.spec("fx_in").expect("in the catalog"), [x, 0.0]);
    for (kind, knobs, enabled) in blocks {
        let Some(spec) = cat.spec(kind) else { continue };
        x += 180.0;
        let id = g.add(spec, [x, 0.0]);
        let node = g.node_mut(id).expect("just added");
        node.params.extend(knobs.iter().map(|(k, v)| (k.clone(), v.clone())));
        node.enabled = *enabled;
        g.links.push(Link { from: (at, 0), to: (id, 0) });
        at = id;
    }
    let out = g.add(cat.spec("fx_out").expect("in the catalog"), [x + 180.0, 0.0]);
    g.links.push(Link { from: (at, 0), to: (out, 0) });
    g
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Ctx, Rng};

    fn noise(frames: usize) -> Vec<f32> {
        let mut rng = Rng::new(7);
        (0..frames * 2).map(|_| rng.next() * 0.5).collect()
    }

    fn block(kind: &str, knobs: &[(&str, f32)], enabled: bool) -> (String, BTreeMap<String, Param>, bool) {
        (kind.into(), knobs.iter().map(|(k, v)| (k.to_string(), Param::Number(*v))).collect(), enabled)
    }

    #[test]
    fn empty_and_bare_graphs_pass_through() {
        let (program, problem) = effects_or_through(&Graph::new("x"));
        assert!(program.is_none() && problem.is_none());
        let program = compile_effects(&chain_graph(&[])).unwrap();
        let dry = noise(1_000);
        let mut wet = dry.clone();
        program.run(&mut wet, Ctx::default());
        assert_eq!(dry, wet);
        assert_eq!(straight_chain(&Graph::new("x")), Some(vec![]));
    }

    #[test]
    fn chain_round_trips() {
        let g = chain_graph(&[block("drive", &[("amount", 0.5)], true), block("amp", &[], false)]);
        let chain = straight_chain(&g).unwrap();
        let kinds: Vec<&str> = chain.iter().map(|id| g.node(*id).unwrap().kind.as_str()).collect();
        assert_eq!(kinds, ["drive", "amp"]);
        assert_eq!(g.node(chain[0]).unwrap().params.get("amount"), Some(&Param::Number(0.5)));
        assert!(!g.node(chain[1]).unwrap().enabled);
        // A second wire from In straight into the amp is no longer a chain.
        let mut forked = g.clone();
        let input = forked.nodes.iter().find(|n| n.kind == "fx_in").unwrap().id;
        let mix = forked.add(crate::catalog().spec("mix").unwrap(), [0.0, 0.0]);
        forked.links.push(Link { from: (input, 0), to: (mix, 0) });
        assert_eq!(straight_chain(&forked), None);
    }

    #[test]
    fn wrong_ends_are_rejected() {
        let mut g = chain_graph(&[]);
        g.add(crate::catalog().spec("voice_out").unwrap(), [0.0, 0.0]);
        let problems = compile_effects(&g).unwrap_err();
        assert!(problems.iter().any(|d| d.message == "Use In and Out, not Voice out, in effects"), "{problems:?}");

        let mut patch = crate::blank_patch();
        patch.add(crate::catalog().spec("fx_out").unwrap(), [0.0, 0.0]);
        let problems = crate::compile(&patch).unwrap_err();
        assert!(problems.iter().any(|d| d.message == "Out is for effects; use Voice out"), "{problems:?}");

        let mut two = chain_graph(&[]);
        two.add(crate::catalog().spec("fx_out").unwrap(), [0.0, 0.0]);
        let problems = compile_effects(&two).unwrap_err();
        assert!(problems.iter().any(|d| d.message == "Use only one Out block"), "{problems:?}");
        let (program, problem) = effects_or_through(&two);
        assert!(program.is_none());
        assert_eq!(problem.as_deref(), Some("Use only one Out block"));
    }

    #[test]
    fn rechain_keeps_follows() {
        let g = chain_graph(&[block("drive", &[], true), block("delay", &[], true)]);
        let ids = straight_chain(&g).unwrap();
        let (drive, delay) = (ids[0], ids[1]);
        let beat = crate::FollowInput { kind: "scene_input".into(), input: "beat".into(), channel: String::new() };
        let f = crate::follow(&g, delay, "mix", &beat).unwrap();
        assert_eq!(straight_chain(&f), Some(vec![drive, delay]), "a follow keeps the list");
        let entry = |id: u32| {
            let n = f.node(id).unwrap();
            ChainBlock { id: Some(id), kind: n.kind.clone(), params: n.params.clone(), enabled: n.enabled }
        };
        let swapped = rechain(&f, &[entry(delay), entry(drive)]);
        assert_eq!(straight_chain(&swapped), Some(vec![delay, drive]));
        assert_eq!(crate::followed(&swapped, delay, "mix").as_deref(), Some("Beat phase"));
        assert!(compile_effects(&swapped).is_ok());

        let without = rechain(&f, &[entry(drive)]);
        assert!(without.nodes.iter().all(|n| n.kind != "scene_input" && n.kind != "scale"), "{:?}", without.nodes);
        assert_eq!(straight_chain(&without), Some(vec![drive]));

        let reverb = ChainBlock { id: None, kind: "reverb".into(), params: BTreeMap::new(), enabled: true };
        let more = rechain(&f, &[entry(drive), entry(delay), reverb]);
        let chain = straight_chain(&more).unwrap();
        assert_eq!(chain.len(), 3);
        assert_eq!(more.node(chain[2]).unwrap().kind, "reverb");
        assert_eq!(crate::followed(&more, delay, "mix").as_deref(), Some("Beat phase"));
    }

    #[test]
    fn a_drive_chain_changes_the_sound() {
        let program = compile_effects(&chain_graph(&[block("drive", &[("amount", 1.0)], true)])).unwrap();
        let dry: Vec<f32> = (0..4_800).flat_map(|i| [0.5 * (i as f32 * 0.05).sin(); 2]).collect();
        let mut wet = dry.clone();
        program.run(&mut wet, Ctx::default());
        let peak = |x: &[f32]| x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak(&wet) > peak(&dry) + 0.3, "{} vs {}", peak(&wet), peak(&dry));
    }
}
