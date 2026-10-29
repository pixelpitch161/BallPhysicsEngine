//! A graph compiled for running: blocks in evaluation order, with their wires resolved to
//! block indices and their settings filled in from the catalog.

use std::collections::BTreeMap;

use bpe_graph::{Catalog, Diagnostic, Graph, Param, PinType, order, validate};
use glam::Vec2;

use crate::catalog::{Role, catalog};

/// One block, ready to run.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    /// The node id in the graph it came from.
    pub id: u32,
    pub kind: String,
    /// Every setting, the catalog's defaults filled in.
    pub params: BTreeMap<String, Param>,
    /// Per input pin, the block index and output pin wired into it. Trigger pins are `None`.
    pub inputs: Vec<Option<(usize, u8)>>,
    /// Per output pin, the blocks its trigger wires lead to, in node id order.
    pub then: Vec<Vec<usize>>,
    /// Per input pin, its type.
    pub input_types: Vec<PinType>,
    /// Per output pin, its type.
    pub output_types: Vec<PinType>,
}

impl Block {
    pub fn num(&self, name: &str) -> f32 {
        self.params.get(name).and_then(Param::number).unwrap_or(0.0)
    }

    pub fn text(&self, name: &str) -> &str {
        self.params.get(name).and_then(Param::text).unwrap_or_default()
    }

    pub fn vec(&self, name: &str) -> Vec2 {
        self.params.get(name).and_then(Param::vector).map(Vec2::from).unwrap_or(Vec2::ZERO)
    }

    pub fn col(&self, name: &str) -> [f32; 4] {
        self.params.get(name).and_then(Param::color).unwrap_or([0.0; 4])
    }

    pub fn on(&self, name: &str) -> bool {
        self.params.get(name).and_then(Param::as_bool).unwrap_or(false)
    }
}

/// A compiled graph.
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub role: Role,
    /// Every enabled block, in an order where wires only point backwards.
    pub blocks: Vec<Block>,
    /// The When blocks, in node id order.
    pub triggers: Vec<usize>,
    /// The role's input block.
    pub input: Option<usize>,
    /// The role's output block.
    pub output: Option<usize>,
}

impl Program {
    /// The block index of a node id.
    pub fn index(&self, id: u32) -> Option<usize> {
        self.blocks.iter().position(|b| b.id == id)
    }

    /// Nothing would ever run: no triggers, or no output block.
    pub fn is_empty(&self) -> bool {
        match self.role {
            Role::Rules => self.triggers.is_empty(),
            _ => self.output.is_none(),
        }
    }
}

/// Compiles a graph for a role. A disabled graph compiles to an empty program.
pub fn compile(graph: &Graph, role: Role) -> Result<Program, Vec<Diagnostic>> {
    let cat = catalog(role);
    let mut errors = validate(graph, cat);
    for kind in [role.input_kind(), role.output_kind()].into_iter().flatten() {
        let mut seen = false;
        for node in graph.nodes.iter().filter(|n| n.kind == kind && n.enabled) {
            if seen {
                let title = cat.spec(&kind).map(|s| s.title.clone()).unwrap_or(kind.clone());
                errors.push(Diagnostic {
                    node: Some(node.id),
                    link: None,
                    message: format!("Only one \"{title}\" block is allowed in a graph."),
                });
            }
            seen = true;
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let mut program = Program { role, blocks: Vec::new(), triggers: Vec::new(), input: None, output: None };
    if !graph.enabled {
        return Ok(program);
    }
    let ordered = order(graph).map_err(|cycle| {
        cycle
            .nodes
            .into_iter()
            .map(|n| Diagnostic { node: Some(n), link: None, message: "This block is part of a loop.".into() })
            .collect::<Vec<_>>()
    })?;
    let enabled = |id: u32| graph.node(id).is_some_and(|n| n.enabled);
    let mut index = BTreeMap::new();
    for id in ordered.into_iter().filter(|&id| enabled(id)) {
        let node = graph.node(id).expect("ordered ids come from the graph");
        let spec = cat.spec(&node.kind).expect("validated");
        let mut params: BTreeMap<String, Param> =
            spec.params.iter().map(|p| (p.name.clone(), p.default.clone())).collect();
        for (name, value) in &node.params {
            if params.contains_key(name) {
                params.insert(name.clone(), value.clone());
            }
        }
        index.insert(id, program.blocks.len());
        program.blocks.push(Block {
            id,
            kind: node.kind.clone(),
            params,
            inputs: vec![None; spec.inputs.len()],
            then: vec![Vec::new(); spec.outputs.len()],
            input_types: spec.inputs.iter().map(|p| p.ty).collect(),
            output_types: spec.outputs.iter().map(|p| p.ty).collect(),
        });
    }
    for link in &graph.links {
        let (Some(&from), Some(&to)) = (index.get(&link.from.0), index.get(&link.to.0)) else { continue };
        let block = &program.blocks[to];
        let Some(ty) = block.input_types.get(link.to.1 as usize) else { continue };
        if *ty == PinType::Trigger {
            if let Some(list) = program.blocks[from].then.get_mut(link.from.1 as usize) {
                list.push(to);
            }
        } else {
            program.blocks[to].inputs[link.to.1 as usize] = Some((from, link.from.1));
        }
    }
    // Trigger lists in node id order, not block order, each block once.
    let ids: Vec<u32> = program.blocks.iter().map(|b| b.id).collect();
    for list in program.blocks.iter_mut().flat_map(|b| b.then.iter_mut()) {
        list.sort_by_key(|&i| ids[i]);
        list.dedup();
    }
    let mut triggers: Vec<usize> =
        (0..program.blocks.len()).filter(|&i| program.blocks[i].kind.starts_with("when.")).collect();
    triggers.sort_by_key(|&i| ids[i]);
    program.triggers = triggers;
    program.input = role.input_kind().and_then(|k| program.blocks.iter().position(|b| b.kind == k));
    program.output = role.output_kind().and_then(|k| program.blocks.iter().position(|b| b.kind == k));
    Ok(program)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Role, catalog};
    use bpe_graph::{Catalog, Graph, Link};

    #[test]
    fn a_chain_compiles_into_trigger_lists() {
        let c = catalog(Role::Rules);
        let mut g = Graph::new("r");
        let hit = g.add(c.spec("when.hit").unwrap(), [0.0, 0.0]);
        let grow = g.add(c.spec("do.grow").unwrap(), [100.0, 0.0]);
        let n = g.add(c.spec("number").unwrap(), [0.0, 100.0]);
        assert!(g.connect(Link { from: (hit, 0), to: (grow, 0) }, c));
        assert!(g.connect(Link { from: (n, 0), to: (grow, 2) }, c));
        let p = compile(&g, Role::Rules).unwrap();
        let (hit_i, grow_i, n_i) = (p.index(hit).unwrap(), p.index(grow).unwrap(), p.index(n).unwrap());
        assert_eq!(p.triggers, vec![hit_i]);
        assert_eq!(p.blocks[hit_i].then[0], vec![grow_i]);
        assert_eq!(p.blocks[grow_i].inputs[2], Some((n_i, 0)));
        assert_eq!(p.blocks[grow_i].inputs[0], None, "trigger inputs are not value inputs");
        assert!(!p.is_empty());
        assert!(compile(&Graph::new("empty"), Role::Rules).unwrap().is_empty());
    }

    #[test]
    fn disabled_blocks_and_graphs_are_left_out() {
        let c = catalog(Role::Rules);
        let mut g = Graph::new("r");
        let hit = g.add(c.spec("when.hit").unwrap(), [0.0, 0.0]);
        let grow = g.add(c.spec("do.grow").unwrap(), [100.0, 0.0]);
        g.connect(Link { from: (hit, 0), to: (grow, 0) }, c);
        g.node_mut(grow).unwrap().enabled = false;
        let p = compile(&g, Role::Rules).unwrap();
        assert_eq!(p.blocks.len(), 1);
        assert!(p.blocks[0].then[0].is_empty());
        g.enabled = false;
        assert!(compile(&g, Role::Rules).unwrap().is_empty());
    }

    #[test]
    fn an_unknown_block_a_loop_and_two_outputs_are_errors() {
        let c = catalog(Role::Bounce);
        let mut g = Graph::new("b");
        g.add(c.spec("bounce.out").unwrap(), [0.0, 0.0]);
        g.add(c.spec("bounce.out").unwrap(), [0.0, 50.0]);
        let errors = compile(&g, Role::Bounce).unwrap_err();
        assert!(errors.iter().any(|d| d.message.contains("one")), "{errors:?}");

        let mut g = Graph::new("w");
        g.add(catalog(Role::Rules).spec("when.hit").unwrap(), [0.0, 0.0]);
        let errors = compile(&g, Role::Bounce).unwrap_err();
        assert!(errors.iter().any(|d| d.message.contains("isn't a block")), "{errors:?}");

        let mut g = Graph::new("loop");
        let a = g.add(c.spec("math").unwrap(), [0.0, 0.0]);
        let b = g.add(c.spec("math").unwrap(), [0.0, 0.0]);
        g.links.push(Link { from: (a, 0), to: (b, 0) });
        g.links.push(Link { from: (b, 0), to: (a, 0) });
        assert!(compile(&g, Role::Bounce).unwrap_err().iter().any(|d| d.message.contains("loop")));
    }

    #[test]
    fn params_are_filled_in_from_the_catalog_defaults() {
        let c = catalog(Role::Bounce);
        let mut g = Graph::new("b");
        let m = g.add(c.spec("math").unwrap(), [0.0, 0.0]);
        g.node_mut(m).unwrap().params.clear();
        let p = compile(&g, Role::Bounce).unwrap();
        assert_eq!(p.blocks[0].text("op"), "add");
        assert_eq!(p.blocks[0].num("a"), 0.0);
    }
}
