//! Rule chains as the inspector shows them: one sentence per When block.

use std::collections::BTreeSet;

use bpe_graph::{Catalog, Graph, PinType};

use crate::catalog::{Role, catalog};

/// One list per enabled When block, in node id order: the block followed by every block its
/// trigger wires reach, depth first, each block at most once.
pub fn sentences(graph: &Graph) -> Vec<Vec<u32>> {
    let cat = catalog(Role::Rules);
    let enabled = |id: u32| graph.node(id).is_some_and(|n| n.enabled);
    let mut whens: Vec<u32> =
        graph.nodes.iter().filter(|n| n.enabled && n.kind.starts_with("when.")).map(|n| n.id).collect();
    whens.sort();
    whens
        .into_iter()
        .map(|when| {
            let mut out = Vec::new();
            let mut seen = BTreeSet::new();
            let mut stack = vec![when];
            while let Some(id) = stack.pop() {
                if !seen.insert(id) || !enabled(id) {
                    continue;
                }
                out.push(id);
                let Some(node) = graph.node(id) else { continue };
                let Some(spec) = cat.spec(&node.kind) else { continue };
                let mut next: Vec<u32> = Vec::new();
                for (pin, out_pin) in spec.outputs.iter().enumerate() {
                    if out_pin.ty != PinType::Trigger {
                        continue;
                    }
                    let mut targets: Vec<u32> = graph.links_from((id, pin as u8)).map(|(_, l)| l.to.0).collect();
                    targets.sort();
                    next.extend(targets);
                }
                // Push in reverse so the first wire is walked first.
                for n in next.into_iter().rev() {
                    stack.push(n);
                }
            }
            out
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Role, catalog};
    use bpe_graph::{Catalog, Graph, Link};

    #[test]
    fn sentences_follow_trigger_wires() {
        let c = catalog(Role::Rules);
        let mut g = Graph::new("r");
        let hit = g.add(c.spec("when.hit").unwrap(), [0.0, 0.0]);
        let once = g.add(c.spec("if.once").unwrap(), [0.0, 0.0]);
        let grow = g.add(c.spec("do.grow").unwrap(), [0.0, 0.0]);
        let timer = g.add(c.spec("when.timer").unwrap(), [0.0, 0.0]);
        let spawn = g.add(c.spec("do.spawn").unwrap(), [0.0, 0.0]);
        let n = g.add(c.spec("number").unwrap(), [0.0, 0.0]);
        g.connect(Link { from: (hit, 0), to: (once, 0) }, c);
        g.connect(Link { from: (once, 0), to: (grow, 0) }, c);
        g.connect(Link { from: (timer, 0), to: (spawn, 0) }, c);
        g.connect(Link { from: (n, 0), to: (grow, 2) }, c);
        assert_eq!(sentences(&g), vec![vec![hit, once, grow], vec![timer, spawn]]);
        g.node_mut(timer).unwrap().enabled = false;
        assert_eq!(sentences(&g), vec![vec![hit, once, grow]]);
    }
}
