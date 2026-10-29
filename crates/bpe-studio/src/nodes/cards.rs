//! The Simple and More cards of a module that runs its graph: the graph's own settings as
//! sliders, and rules as sentences with their numbers inline.

use super::params::param_control;
use crate::theme;
use bpe_core::{Depth, ModuleGraph, Scene};
use bpe_graph::{Catalog, Graph, Node, Param, ParamSpec};
use bpe_kit::graphs::value_param;
use bpe_kit::recipes;
use bpe_vm::{Role, catalog, is_setting, sentences};
use eframe::egui::{self, Id, RichText, Ui};
use std::collections::BTreeSet;
use std::hash::Hash;

/// One parameter a card shows: which block, which parameter, what to call it and its spec.
struct Shown {
    node: u32,
    name: String,
    label: String,
    help: String,
    spec: ParamSpec,
}

fn text_param<'a>(node: &'a Node, name: &str) -> &'a str {
    node.params.get(name).and_then(Param::text).unwrap_or("")
}

/// A setting block's value as the card shows it: named by the block's label, with the
/// block's own range.
fn setting_shown(node: &Node, catalog: &dyn Catalog) -> Option<Shown> {
    let name = value_param(&node.kind);
    let mut spec = catalog.spec(&node.kind)?.params.iter().find(|p| p.name == name)?.clone();
    let mut label = text_param(node, "label").to_owned();
    if label.is_empty() {
        label = text_param(node, "key").to_owned();
    }
    if label.is_empty() {
        label = spec.label.clone();
    }
    if node.kind == "setting" {
        let min = node.params.get("min").and_then(Param::number).unwrap_or(spec.range.0);
        let max = node.params.get("max").and_then(Param::number).unwrap_or(spec.range.1);
        if max > min {
            spec.range = (min, max);
        }
    }
    Some(Shown { node: node.id, name: name.to_owned(), label, help: String::new(), spec })
}

/// A block's own parameter, named "{block} · {parameter}" so blocks of one kind stay apart.
fn param_shown(node: &Node, spec: &bpe_graph::NodeSpec, p: &ParamSpec, with_title: bool) -> Shown {
    let label = if with_title { format!("{} · {}", spec.title, p.label) } else { p.label.clone() };
    Shown { node: node.id, name: p.name.clone(), label, help: p.help.clone(), spec: p.clone() }
}

/// The parameters a graph shows at a depth: at Simple the chosen pairs, at More every
/// setting block and every exposed parameter, in block order.
fn graph_shown(graph: &ModuleGraph, catalog: &dyn Catalog, depth: Depth) -> Vec<Shown> {
    let mut nodes: Vec<&Node> = graph.graph.nodes.iter().collect();
    nodes.sort_by_key(|n| n.id);
    let mut out = Vec::new();
    for node in nodes {
        let Some(spec) = catalog.spec(&node.kind) else { continue };
        if is_setting(&node.kind) {
            out.extend(setting_shown(node, catalog));
            continue;
        }
        for name in &node.exposed {
            if let Some(p) = spec.params.iter().find(|p| &p.name == name) {
                out.push(param_shown(node, spec, p, true));
            }
        }
    }
    at_depth(out, graph, depth)
}

/// Keeps every row at More, and at Simple only the pairs chosen for it.
fn at_depth(mut shown: Vec<Shown>, graph: &ModuleGraph, depth: Depth) -> Vec<Shown> {
    if depth == Depth::Simple {
        shown.retain(|s| graph.simple.contains(&(s.node, s.name.clone())));
    }
    shown
}

/// Whether a wire feeds the input pin a parameter stands in for.
fn wired(graph: &Graph, catalog: &dyn Catalog, node: u32, name: &str) -> bool {
    let Some(kind) = graph.node(node).map(|n| n.kind.as_str()) else { return false };
    let Some(pin) = catalog.spec(kind).and_then(|s| s.inputs.iter().position(|p| p.name == name)) else {
        return false;
    };
    graph.links.iter().any(|l| l.to == (node, pin as u8))
}

/// The row's control, writing into the block. True when the value changed.
fn control(ui: &mut Ui, graph: &mut ModuleGraph, catalog: &dyn Catalog, s: &Shown, scene: &Scene, label: Id) -> bool {
    if wired(&graph.graph, catalog, s.node, &s.name) {
        ui.label(RichText::new("from wire").italics().color(theme::MUTED))
            .on_hover_text("A wire in the graph sets this; open Nodes to change it");
        return false;
    }
    let Some(node) = graph.graph.node_mut(s.node) else { return false };
    let mut value = node.params.get(&s.name).cloned().unwrap_or_else(|| s.spec.default.clone());
    let changed = param_control(ui, &s.spec, &mut value, scene, label);
    if changed {
        node.params.insert(s.name.clone(), value);
    }
    changed
}

fn rows(
    ui: &mut Ui,
    id: impl Hash + std::fmt::Debug,
    graph: &mut ModuleGraph,
    catalog: &dyn Catalog,
    shown: &[Shown],
    scene: &Scene,
) -> bool {
    let mut changed = false;
    egui::Grid::new(id).num_columns(2).spacing([12.0, 6.0]).min_col_width(96.0).show(ui, |ui| {
        for s in shown {
            let label = ui.add(egui::Label::new(&s.label).selectable(false));
            let label = if s.help.is_empty() { label } else { label.on_hover_text(&s.help) };
            changed |= control(ui, graph, catalog, s, scene, label.id);
            ui.end_row();
        }
    });
    changed
}

/// A grid of the parameters a module's graph shows at `depth`, edited in the graph. True
/// when the graph changed.
pub fn graph_settings(
    ui: &mut Ui,
    id: impl Hash + std::fmt::Debug,
    graph: &mut ModuleGraph,
    catalog: &dyn Catalog,
    depth: Depth,
    scene: &Scene,
) -> bool {
    let shown = graph_shown(graph, catalog, depth);
    if shown.is_empty() {
        let words = match depth {
            Depth::Simple => "Nothing is shown at Simple. Pick settings in the graph, or switch to More.",
            Depth::More => "This graph has no settings of its own.",
        };
        ui.label(RichText::new(words).small().color(theme::MUTED));
        return false;
    }
    rows(ui, id, graph, catalog, &shown, scene)
}

/// What a rules card asked for.
#[derive(Default)]
pub struct RulesOutcome {
    pub changed: bool,
    pub open_nodes: bool,
}

/// The parameters of one rule's chain of blocks.
fn chain_shown(graph: &ModuleGraph, catalog: &dyn Catalog, chain: &[u32], depth: Depth) -> Vec<Shown> {
    let mut out = Vec::new();
    for id in chain {
        let Some(node) = graph.graph.node(*id) else { continue };
        let Some(spec) = catalog.spec(&node.kind) else { continue };
        for p in &spec.params {
            out.push(param_shown(node, spec, p, depth == Depth::More));
        }
    }
    at_depth(out, graph, depth)
}

/// Takes a rule out: its chain, and every block that fed nothing but that chain.
fn remove_chain(graph: &mut Graph, chain: &[u32]) -> BTreeSet<u32> {
    let mut doomed: BTreeSet<u32> = chain.iter().copied().collect();
    loop {
        let mut grew = false;
        for n in &graph.nodes {
            if doomed.contains(&n.id) {
                continue;
            }
            let mut others = graph
                .links
                .iter()
                .filter(|l| l.from.0 == n.id || l.to.0 == n.id)
                .map(|l| if l.from.0 == n.id { l.to.0 } else { l.from.0 })
                .peekable();
            if others.peek().is_some() && others.all(|o| doomed.contains(&o)) {
                doomed.insert(n.id);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    graph.remove(&doomed);
    doomed
}

/// Rules as sentences ("When hit › Grow"): one row per When block and what its trigger reaches, with the
/// numbers inline, an "Add rule" menu of recipes and a way into the graph.
pub fn rules_card(
    ui: &mut Ui,
    id: impl Hash + std::fmt::Debug,
    graph: &mut ModuleGraph,
    scene: &Scene,
    depth: Depth,
) -> RulesOutcome {
    let cat = catalog(Role::Rules);
    let id = ui.scope_id().with(id);
    let mut out = RulesOutcome::default();
    let chains = sentences(&graph.graph);
    if chains.is_empty() {
        ui.label(RichText::new("No rules yet.").color(theme::MUTED));
    }
    let mut remove = None;
    for (i, chain) in chains.iter().enumerate() {
        let words: Vec<String> = chain
            .iter()
            .filter_map(|n| graph.graph.node(*n))
            .map(|n| cat.spec(&n.kind).map_or_else(|| n.kind.clone(), |s| s.title.clone()))
            .collect();
        ui.horizontal(|ui| {
            ui.label(RichText::new(words.join(" › ")).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("Remove").on_hover_text("Take this rule out of the graph").clicked() {
                    remove = Some(chain.clone());
                }
            });
        });
        let shown = chain_shown(graph, cat, chain, depth);
        if !shown.is_empty() {
            out.changed |= rows(ui, (id, i), graph, cat, &shown, scene);
        }
        ui.add_space(4.0);
    }
    ui.horizontal(|ui| {
        ui.menu_button("Add rule", |ui| {
            for r in recipes() {
                if ui.button(r.name).on_hover_text(r.help).clicked() {
                    let pairs = (r.add)(&mut graph.graph);
                    graph.simple.extend(pairs);
                    out.changed = true;
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("Drop in a ready-made rule");
        if ui.button("Open in nodes").on_hover_text("See and edit the rules as blocks and wires").clicked() {
            out.open_nodes = true;
        }
    });
    if let Some(chain) = remove {
        let gone = remove_chain(&mut graph.graph, &chain);
        graph.simple.retain(|(n, _)| !gone.contains(n));
        out.changed = true;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_graph::Link;

    fn rules_graph() -> ModuleGraph {
        ModuleGraph {
            graph: Graph::new("Rules"),
            template: "rules.graph".into(),
            custom: true,
            simple: BTreeSet::new(),
        }
    }

    #[test]
    fn removing_a_rule_takes_the_blocks_that_only_fed_it() {
        let c = catalog(Role::Rules);
        let mut g = rules_graph();
        let grow = recipes().into_iter().find(|r| r.name == "Grow on hit").unwrap();
        g.simple.extend((grow.add)(&mut g.graph));
        let first = sentences(&g.graph)[0].clone();
        let (when, grow) = (first[0], first[1]);
        // A ball block feeding only the Grow, and a counter shared with a second rule.
        let only = g.graph.add(c.spec("ball.pick").unwrap(), [0.0, 300.0]);
        assert!(g.graph.connect(Link { from: (only, 0), to: (grow, 1) }, c));
        let shared = g.graph.add(c.spec("counter").unwrap(), [0.0, 400.0]);
        assert!(g.graph.connect(Link { from: (shared, 0), to: (grow, 2) }, c));
        let other_when = g.graph.add(c.spec("when.start").unwrap(), [0.0, 600.0]);
        let other_do = g.graph.add(c.spec("do.grow").unwrap(), [300.0, 600.0]);
        assert!(g.graph.connect(Link { from: (other_when, 0), to: (other_do, 0) }, c));
        assert!(g.graph.connect(Link { from: (shared, 0), to: (other_do, 2) }, c));
        assert_eq!(sentences(&g.graph)[0], vec![when, grow]);
        let gone = remove_chain(&mut g.graph, &first);
        assert!(gone.contains(&when) && gone.contains(&grow) && gone.contains(&only));
        assert!(!gone.contains(&shared), "a block another rule still uses stays");
        assert_eq!(sentences(&g.graph), vec![vec![other_when, other_do]]);
    }
}
