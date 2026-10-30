//! Effects graphs: chains of effect blocks between an In and an Out, run over a whole track.
//! An empty graph is no effects at all, and the sound passes through untouched.

use crate::catalog::catalog;
use crate::program::{Program, compile_effects};
use bpe_graph::{Catalog, Graph, Link, Param};
use std::collections::BTreeMap;

/// The blocks the "Add effect" menu offers, in menu order.
pub fn effects_catalog_kinds() -> Vec<&'static str> {
    EFFECTS.to_vec()
}

const EFFECTS: &[&str] = &["filter", "ladder", "drive", "amp"];

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

/// The blocks between In and Out in order, when the graph is a straight chain: each block
/// with one wire in and one wire out, nothing else wired. An empty graph is an empty chain.
pub fn straight_chain(graph: &Graph) -> Option<Vec<u32>> {
    if graph.nodes.is_empty() {
        return Some(Vec::new());
    }
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
    fn a_drive_chain_changes_the_sound() {
        let program = compile_effects(&chain_graph(&[block("drive", &[("amount", 1.0)], true)])).unwrap();
        let dry: Vec<f32> = (0..4_800).flat_map(|i| [0.5 * (i as f32 * 0.05).sin(); 2]).collect();
        let mut wet = dry.clone();
        program.run(&mut wet, Ctx::default());
        let peak = |x: &[f32]| x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak(&wet) > peak(&dry) + 0.3, "{} vs {}", peak(&wet), peak(&dry));
    }
}
