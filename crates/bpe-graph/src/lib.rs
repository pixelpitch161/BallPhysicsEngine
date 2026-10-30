//! Node graphs: blocks wired together, shared by rules and shader effects.
//!
//! A [`Graph`] is plain data saved inside a scene. It knows nothing about what its blocks do.
//! Meaning comes from a [`Catalog`], which describes each kind of block: its typed input and
//! output pins and its settings. The rule engine interprets graphs with its catalog; shader
//! graphs compile to WGSL with theirs. The editor's node canvas works with any catalog.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A set of blocks and the wires between them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Graph {
    pub name: String,
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
    /// Sticky notes and group frames drawn behind the blocks.
    pub notes: Vec<Note>,
    /// A switched-off graph is kept but does nothing.
    pub enabled: bool,
}

/// One block.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Node {
    /// Unique within its graph. Removing the newest block frees its id for the next one.
    pub id: u32,
    /// Which block this is, as its catalog names it.
    pub kind: String,
    /// Where it sits on the canvas, in canvas units.
    pub pos: [f32; 2],
    /// Setting values by name. Any the catalog lists but the node leaves out use the default.
    pub params: BTreeMap<String, Param>,
    /// A switched-off block passes nothing on.
    pub enabled: bool,
    /// Settings shown as sliders outside the graph (on a shader's effect card, for example).
    pub exposed: BTreeSet<String>,
}

/// A wire from an output pin to an input pin, each given as (node id, pin index).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Link {
    pub from: (u32, u8),
    pub to: (u32, u8),
}

/// A sticky note, or a group frame when it has a size.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Note {
    pub pos: [f32; 2],
    /// Zero for a plain note; otherwise the frame's width and height.
    pub size: [f32; 2],
    pub text: String,
    /// Linear RGBA.
    pub color: [f32; 4],
}

/// A block setting's value.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Param {
    Number(f32),
    Bool(bool),
    Text(String),
    /// Linear RGBA.
    Color([f32; 4]),
    Vector([f32; 2]),
    /// One of a fixed list of words, as the catalog lists them.
    Choice(String),
}

/// What travels along a wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PinType {
    /// "This happened": rule events and the actions they start.
    Trigger,
    Number,
    Vector,
    Color,
    Bool,
    /// A ball, such as the one that hit a wall.
    Ball,
    Text,
    /// A value for every pixel, in shader graphs.
    Field,
    /// A stream of samples, in synth patches. Numbers and sound wire into each other: sound read
    /// as a number is its average over the block, and a number used as sound is held.
    Audio,
}

impl PinType {
    /// Whether an output of type `from` can be wired into an input of this type.
    pub fn accepts(self, from: PinType) -> bool {
        use PinType::*;
        self == from
            || matches!(
                (self, from),
                (Field, Number | Vector | Color) | (Number, Bool) | (Bool, Number) | (Text, Number)
                    | (Audio, Number)
                    | (Number, Audio)
            )
    }

    /// A short name for menus and tooltips.
    pub fn name(self) -> &'static str {
        match self {
            PinType::Trigger => "then",
            PinType::Number => "number",
            PinType::Vector => "position",
            PinType::Color => "colour",
            PinType::Bool => "yes/no",
            PinType::Ball => "ball",
            PinType::Text => "text",
            PinType::Field => "picture",
            PinType::Audio => "sound",
        }
    }
}

/// One input or output of a kind of block.
#[derive(Clone, Debug, PartialEq)]
pub struct PinSpec {
    pub name: String,
    pub ty: PinType,
    /// An input the block can't work without. Outputs ignore this.
    pub required: bool,
    pub help: String,
}

impl PinSpec {
    pub fn new(name: impl Into<String>, ty: PinType) -> Self {
        Self { name: name.into(), ty, required: false, help: String::new() }
    }

    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = help.into();
        self
    }
}

/// One setting of a kind of block.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamSpec {
    pub name: String,
    pub label: String,
    pub help: String,
    pub default: Param,
    /// Slider range for numbers.
    pub range: (f32, f32),
    /// The words a [`Param::Choice`] can be.
    pub choices: Vec<String>,
}

impl ParamSpec {
    pub fn new(name: impl Into<String>, label: impl Into<String>, default: Param) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            help: String::new(),
            default,
            range: (0.0, 1.0),
            choices: Vec::new(),
        }
    }

    pub fn number(name: &str, label: &str, default: f32, min: f32, max: f32) -> Self {
        Self { range: (min, max), ..Self::new(name, label, Param::Number(default)) }
    }

    pub fn choice(name: &str, label: &str, choices: &[&str]) -> Self {
        let first = choices.first().copied().unwrap_or_default();
        Self {
            choices: choices.iter().map(|c| c.to_string()).collect(),
            ..Self::new(name, label, Param::Choice(first.into()))
        }
    }

    pub fn help(mut self, help: impl Into<String>) -> Self {
        self.help = help.into();
        self
    }
}

/// What a kind of block looks like and how it can be wired.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeSpec {
    pub kind: String,
    /// The name shown on the block.
    pub title: String,
    /// Groups blocks in the search palette, such as "When" or "Do".
    pub category: String,
    /// One plain sentence on what it does.
    pub help: String,
    pub inputs: Vec<PinSpec>,
    pub outputs: Vec<PinSpec>,
    pub params: Vec<ParamSpec>,
}

impl NodeSpec {
    pub fn new(kind: &str, title: &str, category: &str, help: &str) -> Self {
        Self {
            kind: kind.into(),
            title: title.into(),
            category: category.into(),
            help: help.into(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            params: Vec::new(),
        }
    }

    pub fn input(mut self, pin: PinSpec) -> Self {
        self.inputs.push(pin);
        self
    }

    pub fn output(mut self, pin: PinSpec) -> Self {
        self.outputs.push(pin);
        self
    }

    pub fn param(mut self, param: ParamSpec) -> Self {
        self.params.push(param);
        self
    }

    pub fn param_spec(&self, name: &str) -> Option<&ParamSpec> {
        self.params.iter().find(|p| p.name == name)
    }

    /// A fresh block of this kind at `pos`, with every setting at its default.
    pub fn instantiate(&self, id: u32, pos: [f32; 2]) -> Node {
        Node {
            id,
            kind: self.kind.clone(),
            pos,
            params: self.params.iter().map(|p| (p.name.clone(), p.default.clone())).collect(),
            ..Node::default()
        }
    }
}

/// The kinds of block one sort of graph can use.
pub trait Catalog {
    fn spec(&self, kind: &str) -> Option<&NodeSpec>;
    fn all(&self) -> &[NodeSpec];
    /// Whether wires may loop back on themselves (shader feedback through the previous frame).
    fn allows_cycles(&self) -> bool {
        false
    }
    /// Whether an output of type `from` can be wired into an input of type `to`. Shader graphs
    /// loosen this for their generic blocks.
    fn accepts(&self, to: PinType, from: PinType) -> bool {
        to.accepts(from)
    }
}

/// A plain list of specs, the simplest catalog.
#[derive(Clone, Debug, Default)]
pub struct SpecList {
    pub specs: Vec<NodeSpec>,
    pub cycles: bool,
}

impl Catalog for SpecList {
    fn spec(&self, kind: &str) -> Option<&NodeSpec> {
        self.specs.iter().find(|s| s.kind == kind)
    }

    fn all(&self) -> &[NodeSpec] {
        &self.specs
    }

    fn allows_cycles(&self) -> bool {
        self.cycles
    }
}

/// Something wrong with a graph, in a plain sentence, pinned to a block or wire where possible.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub node: Option<u32>,
    /// Index into [`Graph::links`].
    pub link: Option<usize>,
    pub message: String,
}

/// The blocks that form a loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cycle {
    pub nodes: Vec<u32>,
}

impl Default for Graph {
    fn default() -> Self {
        Self { name: String::new(), nodes: Vec::new(), links: Vec::new(), notes: Vec::new(), enabled: true }
    }
}

impl Default for Node {
    fn default() -> Self {
        Self {
            id: 0,
            kind: String::new(),
            pos: [0.0; 2],
            params: BTreeMap::new(),
            enabled: true,
            exposed: BTreeSet::new(),
        }
    }
}

impl Default for Note {
    fn default() -> Self {
        Self { pos: [0.0; 2], size: [0.0; 2], text: String::new(), color: [0.9, 0.8, 0.3, 1.0] }
    }
}

impl Param {
    pub fn number(&self) -> Option<f32> {
        match *self {
            Param::Number(v) => Some(v),
            Param::Bool(b) => Some(b as u8 as f32),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            Param::Bool(b) => Some(b),
            Param::Number(v) => Some(v != 0.0),
            _ => None,
        }
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Param::Text(t) | Param::Choice(t) => Some(t),
            _ => None,
        }
    }

    pub fn color(&self) -> Option<[f32; 4]> {
        match *self {
            Param::Color(c) => Some(c),
            _ => None,
        }
    }

    pub fn vector(&self) -> Option<[f32; 2]> {
        match *self {
            Param::Vector(v) => Some(v),
            _ => None,
        }
    }
}

impl Node {
    /// A setting's value, or the catalog's default when the node doesn't set it.
    pub fn param<'a>(&'a self, spec: &'a NodeSpec, name: &str) -> Option<&'a Param> {
        self.params.get(name).or_else(|| spec.param_spec(name).map(|p| &p.default))
    }

    pub fn number(&self, spec: &NodeSpec, name: &str) -> f32 {
        self.param(spec, name).and_then(Param::number).unwrap_or(0.0)
    }

    pub fn choice<'a>(&'a self, spec: &'a NodeSpec, name: &str) -> &'a str {
        self.param(spec, name).and_then(Param::text).unwrap_or_default()
    }
}

impl Graph {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ..Self::default() }
    }

    pub fn node(&self, id: u32) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: u32) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// An id no block in the graph has.
    pub fn next_id(&self) -> u32 {
        self.nodes.iter().map(|n| n.id.saturating_add(1)).max().unwrap_or(1)
    }

    /// Adds a block of `spec`'s kind and returns its id.
    pub fn add(&mut self, spec: &NodeSpec, pos: [f32; 2]) -> u32 {
        let id = self.next_id();
        self.nodes.push(spec.instantiate(id, pos));
        id
    }

    /// Removes blocks and every wire touching them.
    pub fn remove(&mut self, ids: &BTreeSet<u32>) {
        self.nodes.retain(|n| !ids.contains(&n.id));
        self.links.retain(|l| !ids.contains(&l.from.0) && !ids.contains(&l.to.0));
    }

    /// Wires an output to an input. Inputs other than triggers hold one wire, so an older wire
    /// into the same input is replaced. Returns false if the wire already existed.
    pub fn connect(&mut self, link: Link, catalog: &dyn Catalog) -> bool {
        if self.links.contains(&link) {
            return false;
        }
        let multiple = self.input_type(link.to, catalog) == Some(PinType::Trigger);
        if !multiple {
            self.links.retain(|l| l.to != link.to);
        }
        self.links.push(link);
        true
    }

    /// Wires into an input, as (link index, link).
    pub fn links_into(&self, to: (u32, u8)) -> impl Iterator<Item = (usize, &Link)> {
        self.links.iter().enumerate().filter(move |(_, l)| l.to == to)
    }

    /// Wires out of an output.
    pub fn links_from(&self, from: (u32, u8)) -> impl Iterator<Item = (usize, &Link)> {
        self.links.iter().enumerate().filter(move |(_, l)| l.from == from)
    }

    pub fn input_type(&self, pin: (u32, u8), catalog: &dyn Catalog) -> Option<PinType> {
        let spec = catalog.spec(&self.node(pin.0)?.kind)?;
        spec.inputs.get(pin.1 as usize).map(|p| p.ty)
    }

    pub fn output_type(&self, pin: (u32, u8), catalog: &dyn Catalog) -> Option<PinType> {
        let spec = catalog.spec(&self.node(pin.0)?.kind)?;
        spec.outputs.get(pin.1 as usize).map(|p| p.ty)
    }

    /// Copies of the given blocks and the wires between them, with fresh ids from `first_id`,
    /// moved by `offset`. Used for copy and paste.
    pub fn duplicate(&self, ids: &BTreeSet<u32>, first_id: u32, offset: [f32; 2]) -> (Vec<Node>, Vec<Link>) {
        let mut map = BTreeMap::new();
        let mut nodes = Vec::new();
        for node in self.nodes.iter().filter(|n| ids.contains(&n.id)) {
            let id = first_id + map.len() as u32;
            map.insert(node.id, id);
            let mut copy = node.clone();
            copy.id = id;
            copy.pos = [node.pos[0] + offset[0], node.pos[1] + offset[1]];
            nodes.push(copy);
        }
        let links = self
            .links
            .iter()
            .filter_map(|l| Some(Link { from: (*map.get(&l.from.0)?, l.from.1), to: (*map.get(&l.to.0)?, l.to.1) }))
            .collect();
        (nodes, links)
    }
}

/// Checks a graph against its catalog and says, in plain sentences, what is wrong.
pub fn validate(graph: &Graph, catalog: &dyn Catalog) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for node in &graph.nodes {
        if !seen.insert(node.id) {
            out.push(Diagnostic { node: Some(node.id), link: None, message: "Two blocks share this id.".into() });
        }
        if catalog.spec(&node.kind).is_none() {
            let message = format!("\"{}\" isn't a block this graph knows.", node.kind);
            out.push(Diagnostic { node: Some(node.id), link: None, message });
        }
    }
    let mut filled = BTreeMap::<(u32, u8), usize>::new();
    for (index, link) in graph.links.iter().enumerate() {
        let from = graph.output_type(link.from, catalog);
        let to = graph.input_type(link.to, catalog);
        let message = match (from, to) {
            (None, _) | (_, None) if graph.node(link.from.0).is_none() || graph.node(link.to.0).is_none() => {
                Some("This wire leads to a block that is gone.".to_string())
            }
            (None, _) | (_, None) => Some("This wire is plugged into a pin the block doesn't have.".to_string()),
            (Some(from), Some(to)) if !catalog.accepts(to, from) => {
                Some(format!("A {} can't go into a {} input.", from.name(), to.name()))
            }
            _ => None,
        };
        if let Some(message) = message {
            out.push(Diagnostic { node: Some(link.to.0), link: Some(index), message });
            continue;
        }
        if to != Some(PinType::Trigger) {
            *filled.entry(link.to).or_default() += 1;
            if filled[&link.to] > 1 {
                let message = "This input has more than one wire; only one is used.".into();
                out.push(Diagnostic { node: Some(link.to.0), link: Some(index), message });
            }
        }
    }
    for node in &graph.nodes {
        let Some(spec) = catalog.spec(&node.kind) else { continue };
        for (pin, input) in spec.inputs.iter().enumerate() {
            if input.required && graph.links_into((node.id, pin as u8)).next().is_none() {
                let message = format!("\"{}\" needs something wired into it.", input.name);
                out.push(Diagnostic { node: Some(node.id), link: None, message });
            }
        }
    }
    if !catalog.allows_cycles()
        && let Err(cycle) = order(graph)
    {
        for node in cycle.nodes {
            let message = "This block is part of a loop, so it would never finish.".into();
            out.push(Diagnostic { node: Some(node), link: None, message });
        }
    }
    out
}

/// Blocks in an order where every block comes after the blocks wired into it. Ties keep the
/// graph's own order, so the result is always the same for the same graph.
pub fn order(graph: &Graph) -> Result<Vec<u32>, Cycle> {
    let index: BTreeMap<u32, usize> = graph.nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
    let mut incoming = vec![0usize; graph.nodes.len()];
    let mut outgoing = vec![Vec::new(); graph.nodes.len()];
    let unique: BTreeSet<(u32, u32)> = graph.links.iter().map(|l| (l.from.0, l.to.0)).collect();
    for (from, to) in unique {
        let (Some(&a), Some(&b)) = (index.get(&from), index.get(&to)) else { continue };
        outgoing[a].push(b);
        incoming[b] += 1;
    }
    let mut ready: BTreeSet<usize> = (0..graph.nodes.len()).filter(|&i| incoming[i] == 0).collect();
    let mut result = Vec::with_capacity(graph.nodes.len());
    while let Some(i) = ready.pop_first() {
        result.push(graph.nodes[i].id);
        for &next in &outgoing[i] {
            incoming[next] -= 1;
            if incoming[next] == 0 {
                ready.insert(next);
            }
        }
    }
    if result.len() == graph.nodes.len() {
        Ok(result)
    } else {
        // What's left is the loops plus blocks that only hang off them. Peel off blocks that
        // lead nowhere inside what's left until only the loops remain.
        let mut left: Vec<bool> = incoming.iter().map(|&n| n > 0).collect();
        loop {
            let dead_ends: Vec<usize> =
                (0..left.len()).filter(|&i| left[i] && !outgoing[i].iter().any(|&j| left[j])).collect();
            if dead_ends.is_empty() {
                break;
            }
            for i in dead_ends {
                left[i] = false;
            }
        }
        let nodes = (0..graph.nodes.len()).filter(|&i| left[i]).map(|i| graph.nodes[i].id).collect();
        Err(Cycle { nodes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_and_number_pins_connect_both_ways() {
        assert!(PinType::Audio.accepts(PinType::Number));
        assert!(PinType::Number.accepts(PinType::Audio));
        assert!(!PinType::Audio.accepts(PinType::Color));
        assert_eq!(PinType::Audio.name(), "sound");
    }

    fn catalog() -> SpecList {
        let t = PinType::Trigger;
        SpecList {
            specs: vec![
                NodeSpec::new("start", "On start", "When", "").output(PinSpec::new("then", t)),
                NodeSpec::new("number", "Number", "Values", "")
                    .output(PinSpec::new("value", PinType::Number))
                    .param(ParamSpec::number("value", "Value", 3.0, 0.0, 10.0)),
                NodeSpec::new("spawn", "Spawn", "Do", "")
                    .input(PinSpec::new("do", t).required())
                    .input(PinSpec::new("count", PinType::Number))
                    .output(PinSpec::new("then", t)),
                NodeSpec::new("color", "Colour", "Values", "").output(PinSpec::new("c", PinType::Color)),
            ],
            cycles: false,
        }
    }

    fn sample() -> Graph {
        let c = catalog();
        let mut g = Graph::new("test");
        let start = g.add(c.spec("start").unwrap(), [0.0, 0.0]);
        let number = g.add(c.spec("number").unwrap(), [0.0, 100.0]);
        let spawn = g.add(c.spec("spawn").unwrap(), [200.0, 0.0]);
        g.connect(Link { from: (start, 0), to: (spawn, 0) }, &c);
        g.connect(Link { from: (number, 0), to: (spawn, 1) }, &c);
        g
    }

    #[test]
    fn a_good_graph_has_no_problems_and_orders_its_blocks() {
        let g = sample();
        assert!(validate(&g, &catalog()).is_empty());
        let order = order(&g).unwrap();
        let pos = |id| order.iter().position(|&x| x == id).unwrap();
        assert!(pos(1) < pos(3) && pos(2) < pos(3));
    }

    #[test]
    fn empty_graph_is_fine() {
        let g = Graph::default();
        assert!(validate(&g, &catalog()).is_empty());
        assert_eq!(order(&g).unwrap(), Vec::<u32>::new());
    }

    #[test]
    fn wrong_types_unknown_kinds_and_missing_inputs_are_reported() {
        let c = catalog();
        let mut g = sample();
        let color = g.add(c.spec("color").unwrap(), [0.0, 200.0]);
        g.links.retain(|l| l.to != (3, 1));
        g.links.push(Link { from: (color, 0), to: (3, 1) });
        g.nodes.push(Node { id: 9, kind: "mystery".into(), ..Node::default() });
        let lonely = g.add(c.spec("spawn").unwrap(), [0.0, 0.0]);
        let problems = validate(&g, &c);
        assert!(problems.iter().any(|d| d.message.contains("colour can't go into a number")));
        assert!(problems.iter().any(|d| d.node == Some(9) && d.message.contains("mystery")));
        assert!(problems.iter().any(|d| d.node == Some(lonely) && d.message.contains("needs")));
    }

    #[test]
    fn loops_are_found_unless_the_catalog_allows_them() {
        let mut c = catalog();
        let mut g = Graph::new("loop");
        let a = g.add(c.spec("spawn").unwrap(), [0.0, 0.0]);
        let b = g.add(c.spec("spawn").unwrap(), [0.0, 0.0]);
        g.connect(Link { from: (a, 0), to: (b, 0) }, &c);
        g.connect(Link { from: (b, 0), to: (a, 0) }, &c);
        assert_eq!(order(&g).unwrap_err().nodes, vec![a, b]);
        // A block that only hangs off the loop isn't part of it.
        let after = g.add(c.spec("spawn").unwrap(), [0.0, 0.0]);
        g.connect(Link { from: (b, 0), to: (after, 0) }, &c);
        assert_eq!(order(&g).unwrap_err().nodes, vec![a, b]);
        assert!(validate(&g, &c).iter().any(|d| d.message.contains("loop")));
        c.cycles = true;
        assert!(validate(&g, &c).iter().all(|d| !d.message.contains("loop")));
    }

    #[test]
    fn data_inputs_hold_one_wire_and_triggers_many() {
        let c = catalog();
        let mut g = sample();
        let other = g.add(c.spec("number").unwrap(), [0.0, 0.0]);
        g.connect(Link { from: (other, 0), to: (3, 1) }, &c);
        assert_eq!(g.links_into((3, 1)).count(), 1);
        let start2 = g.add(c.spec("start").unwrap(), [0.0, 0.0]);
        g.connect(Link { from: (start2, 0), to: (3, 0) }, &c);
        assert_eq!(g.links_into((3, 0)).count(), 2);
        assert!(!g.connect(Link { from: (start2, 0), to: (3, 0) }, &c));
    }

    #[test]
    fn removing_blocks_removes_their_wires_and_duplicates_get_new_ids() {
        let mut g = sample();
        let (nodes, links) = g.duplicate(&[1, 3].into(), 10, [5.0, 5.0]);
        assert_eq!(nodes.iter().map(|n| n.id).collect::<Vec<_>>(), vec![10, 11]);
        assert_eq!(links, vec![Link { from: (10, 0), to: (11, 0) }]);
        g.remove(&[3].into());
        assert!(g.links.is_empty());
        assert_eq!(g.next_id(), 3);
    }

    #[test]
    fn params_fall_back_to_defaults_and_graphs_save() {
        let c = catalog();
        let mut g = sample();
        let spec = c.spec("number").unwrap();
        g.node_mut(2).unwrap().params.clear();
        assert_eq!(g.node(2).unwrap().number(spec, "value"), 3.0);
        let text = ron::to_string(&g).unwrap();
        assert_eq!(ron::from_str::<Graph>(&text).unwrap(), g);
        let sparse: Graph = ron::from_str("(name: \"x\")").unwrap();
        assert!(sparse.enabled);
    }
}
