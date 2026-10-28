//! Shader graphs: effects built from blocks wired together instead of written as code.
//!
//! [`generate`] turns a [`Graph`] into an ordinary effect file, the same `fn effect(uv, p)` format
//! hand-written effects use, with a `// param` line for every input the user exposed as a slider.
//! The effect library then checks and runs it like any other effect, so keyframes, dice, the
//! effect chain and hot reload all work unchanged. Problems are reported against the block that
//! caused them, and a broken graph never panics.

pub mod catalog;

pub use bpe_graph::{Graph, Link, Node, Note, Param};
pub use catalog::{Block, Category, Control, Default, Input, Output, Ty};

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::sync::OnceLock;

/// What effect ids of node effects start with: a scene refers to graph `Name` as `graph:Name`.
pub const PREFIX: &str = "graph:";

/// Most sliders one node effect can expose.
pub const MAX_SLIDERS: usize = super::MAX_PARAMS;

/// Something wrong with a graph, on one block or (with `node: None`) the graph as a whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub node: Option<u32>,
    pub message: String,
}

impl Problem {
    fn on(node: u32, message: impl Into<String>) -> Self {
        Self { node: Some(node), message: message.into() }
    }

    fn graph(message: impl Into<String>) -> Self {
        Self { node: None, message: message.into() }
    }
}

/// A slider made from an exposed block input.
#[derive(Clone, Debug, PartialEq)]
pub struct Slider {
    /// The `// param` name, which effect cards, keyframes and dice use.
    pub name: String,
    pub node: u32,
    pub input: String,
}

/// The effect file made from a graph.
#[derive(Clone, Debug, PartialEq)]
pub struct Generated {
    /// A complete effect file, ready for [`super::EffectInfo::parse`].
    pub source: String,
    /// For each line of `source`, the block that wrote it.
    pub line_nodes: Vec<Option<u32>>,
    pub sliders: Vec<Slider>,
    /// Problems on blocks the output doesn't use, which don't stop the effect from running.
    pub warnings: Vec<Problem>,
    /// In thumbnail mode, the block shown when `fx.view` is `i + 1`.
    pub thumbnails: Vec<u32>,
}

impl Generated {
    /// The block that wrote a line of the source (counting from 1), if any.
    pub fn node_at_line(&self, line: usize) -> Option<u32> {
        line.checked_sub(1).and_then(|i| self.line_nodes.get(i).copied().flatten())
    }
}

/// Every block, looked up by kind. Also the catalog the node editor uses for shader graphs.
pub struct Catalog {
    blocks: Vec<Block>,
    by_kind: BTreeMap<&'static str, usize>,
    specs: Vec<bpe_graph::NodeSpec>,
}

impl Catalog {
    pub fn get() -> &'static Catalog {
        static CATALOG: OnceLock<Catalog> = OnceLock::new();
        CATALOG.get_or_init(|| {
            let blocks = catalog::blocks();
            let by_kind = blocks.iter().enumerate().map(|(i, b)| (b.kind, i)).collect();
            let specs = blocks.iter().map(node_spec).collect();
            Catalog { blocks, by_kind, specs }
        })
    }

    pub fn block(&self, kind: &str) -> Option<&Block> {
        self.by_kind.get(kind).map(|&i| &self.blocks[i])
    }

    pub fn all(&self) -> &[Block] {
        &self.blocks
    }
}

impl bpe_graph::Catalog for Catalog {
    fn spec(&self, kind: &str) -> Option<&bpe_graph::NodeSpec> {
        self.by_kind.get(kind).map(|&i| &self.specs[i])
    }

    fn all(&self) -> &[bpe_graph::NodeSpec] {
        &self.specs
    }

    /// Wires may be type-checked loosely: the compiler converts numbers to positions and colours
    /// and colours to brightness, works out what flows through "any value" pins, and reports
    /// anything that still doesn't fit on the block.
    fn accepts(&self, to: bpe_graph::PinType, from: bpe_graph::PinType) -> bool {
        use bpe_graph::PinType::*;
        let value = |t| matches!(t, Number | Vector | Color | Field | Bool);
        // Numbers spread to positions and colours, and colours give their brightness; a
        // position has no sensible number or colour. "Any value" pins take anything.
        value(to) && value(from) && !matches!((to, from), (Number | Color | Bool, Vector) | (Vector, Color))
    }
}

fn pin_type(ty: Ty) -> bpe_graph::PinType {
    match ty {
        Ty::Number => bpe_graph::PinType::Number,
        Ty::Vector => bpe_graph::PinType::Vector,
        Ty::Color => bpe_graph::PinType::Color,
        Ty::Any => bpe_graph::PinType::Field,
    }
}

/// What the node editor needs to know about a block: its pins, and a setting for every input
/// that is used when nothing is plugged in.
fn node_spec(block: &Block) -> bpe_graph::NodeSpec {
    use bpe_graph::{NodeSpec, ParamSpec, PinSpec};
    let mut spec = NodeSpec::new(block.kind, block.title, block.category.name(), block.help);
    for input in &block.inputs {
        spec = spec.input(PinSpec::new(input.name, pin_type(input.ty)).help(input.help));
        let label = catalog_label(input.name);
        let param = match (input.default, input.control) {
            (Default::Number(v), Control::Toggle) => ParamSpec::new(input.name, label, Param::Bool(v > 0.5)),
            (Default::Number(v), _) => ParamSpec::number(input.name, &label, v, input.min, input.max),
            (Default::Vector(v), _) => {
                ParamSpec { range: (input.min, input.max), ..ParamSpec::new(input.name, label, Param::Vector(v)) }
            }
            (Default::Color([r, g, b]), _) => ParamSpec::new(input.name, label, Param::Color([r, g, b, 1.0])),
            // Unplugged, these use the pixel or the picture; there's nothing to set.
            (Default::Position | Default::Picture, _) => continue,
        };
        spec = spec.param(param.help(input.help));
    }
    for output in &block.outputs {
        spec = spec.output(PinSpec::new(output.name, pin_type(output.ty)));
    }
    spec
}

/// `in_low` to "In low".
fn catalog_label(name: &str) -> String {
    let text = name.replace('_', " ");
    let mut chars = text.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// The effect id a scene uses for the graph called `name`.
pub fn effect_id(name: &str) -> String {
    format!("{PREFIX}{name}")
}

/// The graph name in an effect id, if it is a node effect.
pub fn graph_name(effect_id: &str) -> Option<&str> {
    effect_id.strip_prefix(PREFIX)
}

/// Turns a graph into an effect file. With `thumbnails`, every working block is computed and
/// `fx.view` picks which one the effect shows, so one shader can draw a preview of every block.
pub fn generate(graph: &Graph, thumbnails: bool) -> Result<Generated, Vec<Problem>> {
    Compiler::new(graph).run(thumbnails)
}

/// The type flowing out of each output of each compiled block.
type OutTypes = BTreeMap<u32, Vec<Ty>>;

struct Compiler<'a> {
    graph: &'a Graph,
    catalog: &'static Catalog,
    nodes: BTreeMap<u32, (&'a Node, &'static Block)>,
    /// For each (node, input index), the (node, output index) wired into it.
    wires: BTreeMap<(u32, usize), (u32, usize)>,
    problems: Vec<Problem>,
}

impl<'a> Compiler<'a> {
    fn new(graph: &'a Graph) -> Self {
        Self { graph, catalog: Catalog::get(), nodes: BTreeMap::new(), wires: BTreeMap::new(), problems: Vec::new() }
    }

    fn run(mut self, thumbnails: bool) -> Result<Generated, Vec<Problem>> {
        self.collect_nodes();
        self.collect_wires();
        let outputs: Vec<u32> = self
            .graph
            .nodes
            .iter()
            .filter(|n| n.kind == "output" && self.nodes.contains_key(&n.id))
            .map(|n| n.id)
            .collect();
        let mut extra_outputs = Vec::new();
        let output = match outputs.as_slice() {
            [] => {
                self.problems.push(Problem::graph("Add an Output block: it decides what the effect shows."));
                None
            }
            [first, rest @ ..] => {
                for &extra in rest {
                    self.problems.push(Problem::on(extra, "A node effect can only have one Output block."));
                    extra_outputs.push(extra);
                }
                Some(*first)
            }
        };

        let order = self.order();
        let mut types = OutTypes::new();
        let mut broken = BTreeSet::new();
        for &id in &order {
            match self.resolve(id, &types, &broken) {
                Ok(out) => {
                    types.insert(id, out);
                }
                Err(problem) => {
                    broken.insert(id);
                    if let Some(problem) = problem {
                        self.problems.push(problem);
                    }
                }
            }
        }

        // The blocks the output needs, so unused ones cost nothing on the GPU.
        let needed = match output {
            Some(output) => self.upstream(output),
            None => BTreeSet::new(),
        };
        let fatal: Vec<Problem> = self
            .problems
            .iter()
            .filter(|p| p.node.is_none_or(|n| needed.contains(&n) || extra_outputs.contains(&n)))
            .cloned()
            .collect();
        let fatal_needed = needed.iter().any(|n| broken.contains(n));
        if !fatal.is_empty() || fatal_needed || output.is_none() {
            let mut problems = self.problems;
            problems.dedup();
            return Err(problems);
        }
        let warnings = self.problems.clone();

        let emit: Vec<u32> = order
            .iter()
            .copied()
            .filter(|id| !broken.contains(id) && (needed.contains(id) || thumbnails) && Some(*id) != output)
            .collect();
        self.emit(&emit, &needed, output.unwrap(), &types, thumbnails, warnings)
    }

    fn collect_nodes(&mut self) {
        for node in &self.graph.nodes {
            if self.nodes.contains_key(&node.id) {
                self.problems.push(Problem::on(node.id, format!("Two blocks share the id {}.", node.id)));
                continue;
            }
            match self.catalog.block(&node.kind) {
                Some(block) => {
                    self.nodes.insert(node.id, (node, block));
                }
                None => self.problems.push(Problem::on(node.id, format!("Unknown block \"{}\".", node.kind))),
            }
        }
    }

    fn collect_wires(&mut self) {
        for link in &self.graph.links {
            let ((from, out), (to, input)) = ((link.from.0, link.from.1 as usize), (link.to.0, link.to.1 as usize));
            let (Some((_, from_block)), Some((_, to_block))) = (self.nodes.get(&from), self.nodes.get(&to)) else {
                // Wires to unknown or missing blocks are reported on the block itself, if it exists.
                continue;
            };
            if out >= from_block.outputs.len() || input >= to_block.inputs.len() {
                self.problems.push(Problem::on(to, "A wire goes to a pin this block doesn't have."));
                continue;
            }
            if self.wires.insert((to, input), (from, out)).is_some() {
                self.problems.push(Problem::on(
                    to,
                    format!("Two wires go into {}; only the last one is used.", to_block.inputs[input].name),
                ));
            }
        }
    }

    /// Blocks in an order where every block comes after the blocks wired into it. Blocks in a
    /// loop are left out and reported.
    fn order(&mut self) -> Vec<u32> {
        let mut waiting: BTreeMap<u32, usize> = self.nodes.keys().map(|&id| (id, 0)).collect();
        let mut downstream: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for (&(to, _), &(from, _)) in &self.wires {
            *waiting.get_mut(&to).unwrap() += 1;
            downstream.entry(from).or_default().push(to);
        }
        // Keep the order of the graph's node list among ready blocks, so code is stable.
        let position: BTreeMap<u32, usize> = self.graph.nodes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
        let mut ready: BTreeSet<(usize, u32)> =
            waiting.iter().filter(|(_, n)| **n == 0).map(|(&id, _)| (position[&id], id)).collect();
        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(first) = ready.pop_first() {
            let id = first.1;
            order.push(id);
            for &next in downstream.get(&id).into_iter().flatten() {
                let count = waiting.get_mut(&next).unwrap();
                *count -= 1;
                if *count == 0 {
                    ready.insert((position[&next], next));
                }
            }
        }
        // Left over are blocks in a loop and blocks after one; only the first get the message.
        let left: BTreeSet<u32> = waiting.iter().filter(|(_, n)| **n > 0).map(|(&id, _)| id).collect();
        for &id in &left {
            let mut seen = BTreeSet::new();
            let mut stack: Vec<u32> = downstream.get(&id).cloned().unwrap_or_default();
            while let Some(next) = stack.pop() {
                if next == id {
                    self.problems.push(Problem::on(
                        id,
                        "This block is in a loop. Use Previous frame or Feedback to feed a picture back in.",
                    ));
                    break;
                }
                if left.contains(&next) && seen.insert(next) {
                    stack.extend(downstream.get(&next).into_iter().flatten().copied());
                }
            }
        }
        order
    }

    /// Every block the given one depends on, itself included.
    fn upstream(&self, id: u32) -> BTreeSet<u32> {
        let mut seen = BTreeSet::new();
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Some((_, block)) = self.nodes.get(&id) else { continue };
            for input in 0..block.inputs.len() {
                if let Some(&(from, _)) = self.wires.get(&(id, input)) {
                    stack.push(from);
                }
            }
        }
        seen
    }

    /// The type of a wire into an input: the source's output type.
    fn wire_type(&self, id: u32, input: usize, types: &OutTypes) -> Option<Ty> {
        let &(from, out) = self.wires.get(&(id, input))?;
        types.get(&from).map(|t| t[out])
    }

    /// Works out the shared type of a block's `Any` pins and the types of its outputs. `Err(None)`
    /// means a block it depends on is broken, which is already reported there.
    fn resolve(&self, id: u32, types: &OutTypes, broken: &BTreeSet<u32>) -> Result<Vec<Ty>, Option<Problem>> {
        let (_, block) = self.nodes[&id];
        let mut generic = Ty::Number;
        let mut seen = BTreeSet::new();
        for (index, input) in block.inputs.iter().enumerate() {
            let Some(&(from, _)) = self.wires.get(&(id, index)) else { continue };
            if broken.contains(&from) || !types.contains_key(&from) {
                return Err(None);
            }
            let source = self.wire_type(id, index, types).unwrap();
            if input.ty == Ty::Any {
                seen.insert(source);
                if source != Ty::Number {
                    generic = source;
                }
            } else if let Err(message) = convert("x", source, input.ty) {
                return Err(Some(Problem::on(id, format!("{}: {message}", input.name))));
            }
        }
        if seen.contains(&Ty::Vector) && seen.contains(&Ty::Color) {
            return Err(Some(Problem::on(id, "Can't mix a position and a colour in one block.")));
        }
        Ok(block.outputs.iter().map(|o| if o.ty == Ty::Any { generic } else { o.ty }).collect())
    }

    fn generic_type(&self, id: u32, types: &OutTypes) -> Ty {
        let (_, block) = self.nodes[&id];
        block
            .inputs
            .iter()
            .enumerate()
            .filter(|(_, input)| input.ty == Ty::Any)
            .filter_map(|(index, _)| self.wire_type(id, index, types))
            .fold(Ty::Number, |acc, ty| if ty == Ty::Number { acc } else { ty })
    }

    fn emit(
        &self,
        emit: &[u32],
        needed: &BTreeSet<u32>,
        output: u32,
        types: &OutTypes,
        thumbnails: bool,
        warnings: Vec<Problem>,
    ) -> Result<Generated, Vec<Problem>> {
        let mut sliders = Vec::new();
        let mut problems = Vec::new();
        let mut used_names = BTreeSet::new();
        // Sliders come first, in the order of the graph's node list, so their order is stable.
        let mut slider_of: BTreeMap<(u32, &str), String> = BTreeMap::new();
        let mut header = String::new();
        let name = one_line(&self.graph.name);
        let _ = writeln!(header, "// name: {}", if name.is_empty() { "Node effect" } else { &name });
        let _ = writeln!(header, "// about: Made from blocks in the Shaders workspace.");
        let compiled: BTreeSet<u32> = emit.iter().copied().chain([output]).collect();
        for node in &self.graph.nodes {
            if !compiled.contains(&node.id) {
                continue;
            }
            let (_, block) = self.nodes[&node.id];
            // Blocks drawn only for their previews keep fixed values, so the previews get the
            // same sliders as the effect itself.
            if node.id != output && !needed.contains(&node.id) {
                continue;
            }
            for (index, input) in block.inputs.iter().enumerate() {
                if !node.exposed.contains(input.name) || self.wires.contains_key(&(node.id, index)) {
                    continue;
                }
                if !matches!(input.ty, Ty::Number | Ty::Any) {
                    continue;
                }
                if sliders.len() == MAX_SLIDERS {
                    problems
                        .push(Problem::on(node.id, format!("A node effect can have at most {MAX_SLIDERS} sliders.")));
                    break;
                }
                let wanted = match node.params.get(&format!("{}:name", input.name)) {
                    Some(Param::Text(text)) => identifier(text),
                    _ => None,
                }
                .unwrap_or_else(|| input.name.to_string());
                let mut name = wanted.clone();
                let mut n = 2;
                while !used_names.insert(name.clone()) {
                    name = format!("{wanted}_{n}");
                    n += 1;
                }
                let bound = |key: &str, fallback: f32| match node.params.get(&format!("{}:{key}", input.name)) {
                    Some(Param::Number(v)) if v.is_finite() => *v,
                    _ => fallback,
                };
                let (min, max) = (bound("min", input.min), bound("max", input.max));
                let (min, max) = if min < max { (min, max) } else { (input.min, input.max) };
                let value = number_param(node, input).clamp(min, max);
                let control = match node.params.get(&format!("{}:control", input.name)) {
                    Some(Param::Choice(c)) if c == "toggle" => Control::Toggle,
                    Some(Param::Choice(c)) if c == "whole" => Control::Whole,
                    Some(Param::Choice(c)) if c == "slider" => Control::Slider,
                    _ => input.control,
                };
                let (min, max) = if control == Control::Toggle { (0.0, 1.0) } else { (min, max) };
                let value = value.clamp(min, max);
                let kind = match control {
                    Control::Toggle => "toggle".to_string(),
                    Control::Whole => format!("[{}, {}] int", number(min), number(max)),
                    Control::Slider => format!("[{}, {}]", number(min), number(max)),
                };
                let _ = writeln!(header, "// param {name} = {} {kind} {}", number(value), one_line(input.help));
                slider_of.insert((node.id, input.name), name.clone());
                sliders.push(Slider { name, node: node.id, input: input.name.to_string() });
            }
        }
        if !problems.is_empty() {
            return Err(problems);
        }

        let mut lines: Vec<(String, Option<u32>)> = header.lines().map(|l| (l.to_string(), None)).collect();
        lines.push((String::new(), None));
        let mut helpers = BTreeSet::new();
        for &id in emit.iter().chain([&output]) {
            helpers.extend(self.nodes[&id].1.helpers.iter().copied());
        }
        for (name, code) in catalog::HELPERS {
            if helpers.contains(name) {
                lines.extend(code.lines().map(|l| (l.to_string(), None)));
                lines.push((String::new(), None));
            }
        }
        lines.push(("fn effect(uv: vec2<f32>, p: Params) -> vec3<f32> {".into(), None));
        lines.push(("    let q0 = to_centered(uv);".into(), None));
        lines.push(("    let c0 = sample(uv);".into(), None));

        for &id in emit {
            self.emit_node(id, types, &slider_of, &mut lines);
        }
        let mut thumbs = Vec::new();
        if thumbnails {
            lines.push(("    switch fx.view {".into(), None));
            for &id in emit {
                let (_, block) = self.nodes[&id];
                let Some(first) = block.outputs.first() else { continue };
                let ty = types[&id][0];
                thumbs.push(id);
                let value = format!("n{id}_{}", first.name);
                let shown = match ty {
                    Ty::Number | Ty::Any => format!("vec3<f32>({value})"),
                    Ty::Vector => format!("vec3<f32>(fract({value} + 0.5), 0.5)"),
                    Ty::Color => value,
                };
                lines.push((format!("        case {}u: {{ return {shown}; }}", thumbs.len()), Some(id)));
            }
            lines.push(("        default: {}".into(), None));
            lines.push(("    }".into(), None));
        }
        self.emit_node(output, types, &slider_of, &mut lines);
        lines.push(("}".into(), None));

        let mut source = String::new();
        let mut line_nodes = Vec::with_capacity(lines.len());
        for (line, node) in lines {
            source.push_str(&line);
            source.push('\n');
            line_nodes.push(node);
        }
        Ok(Generated { source, line_nodes, sliders, warnings, thumbnails: thumbs })
    }

    fn emit_node(
        &self,
        id: u32,
        types: &OutTypes,
        sliders: &BTreeMap<(u32, &str), String>,
        lines: &mut Vec<(String, Option<u32>)>,
    ) {
        let (node, block) = self.nodes[&id];
        let generic = self.generic_type(id, types);
        let mut inputs = BTreeMap::new();
        for (index, input) in block.inputs.iter().enumerate() {
            let target = if input.ty == Ty::Any { generic } else { input.ty };
            let expr = match self.wires.get(&(id, index)) {
                Some(&(from, out)) => {
                    let source = types[&from][out];
                    let name = format!("n{from}_{}", self.nodes[&from].1.outputs[out].name);
                    // Checked in resolve, so this always succeeds.
                    convert(&name, source, target).unwrap_or(name)
                }
                None => match sliders.get(&(id, input.name)) {
                    Some(slider) => convert(&format!("p.{slider}"), Ty::Number, target).unwrap(),
                    None => unplugged(node, input, target),
                },
            };
            inputs.insert(input.name, expr);
        }
        lines.push((format!("    // {} #{id}", block.title), Some(id)));
        if !node.enabled {
            // A switched-off block passes its first matching input straight through.
            for (k, output) in block.outputs.iter().enumerate() {
                let ty = types[&id][k];
                let pass = block
                    .inputs
                    .iter()
                    .find(|i| (if i.ty == Ty::Any { generic } else { i.ty }) == ty)
                    .map(|i| inputs[i.name].clone())
                    .unwrap_or_else(|| zero(ty).to_string());
                if block.kind == "output" {
                    lines.push((format!("    return {pass};"), Some(id)));
                } else {
                    lines.push((format!("    let n{id}_{} = {pass};", output.name), Some(id)));
                }
            }
            if block.kind == "output" && block.outputs.is_empty() {
                lines.push(("    return c0;".into(), Some(id)));
            }
            return;
        }
        for line in block.code.lines() {
            lines.push((format!("    {}", substitute(line, id, block, &inputs)), Some(id)));
        }
    }
}

/// Builds graphs in code, for templates and tests. Pins are named, so wiring reads clearly.
pub struct Builder {
    graph: Graph,
    next: u32,
}

impl Builder {
    pub fn new(name: &str) -> Self {
        Self { graph: Graph { name: name.into(), enabled: true, ..Graph::default() }, next: 1 }
    }

    /// Adds a block and returns its id. Blocks are laid out left to right in the order added.
    pub fn add(&mut self, kind: &str) -> u32 {
        let id = self.next;
        self.next += 1;
        let column = self.graph.nodes.len() as f32;
        self.graph.nodes.push(Node {
            id,
            kind: kind.into(),
            pos: [column * 190.0, (id % 3) as f32 * 70.0],
            enabled: true,
            ..Node::default()
        });
        id
    }

    fn node(&mut self, id: u32) -> &mut Node {
        self.graph.nodes.iter_mut().find(|n| n.id == id).expect("block added by this builder")
    }

    pub fn set(&mut self, id: u32, input: &str, value: f32) -> &mut Self {
        self.node(id).params.insert(input.into(), Param::Number(value));
        self
    }

    pub fn set_color(&mut self, id: u32, input: &str, [r, g, b]: [f32; 3]) -> &mut Self {
        self.node(id).params.insert(input.into(), Param::Color([r, g, b, 1.0]));
        self
    }

    pub fn set_point(&mut self, id: u32, input: &str, value: [f32; 2]) -> &mut Self {
        self.node(id).params.insert(input.into(), Param::Vector(value));
        self
    }

    /// Limits the slider of an exposed input to a range other than the block's usual one.
    pub fn range(&mut self, id: u32, input: &str, min: f32, max: f32) -> &mut Self {
        let node = self.node(id);
        node.params.insert(format!("{input}:min"), Param::Number(min));
        node.params.insert(format!("{input}:max"), Param::Number(max));
        self
    }

    /// Shows an exposed input as an on/off switch (or whole-number slider) instead.
    pub fn control(&mut self, id: u32, input: &str, control: Control) -> &mut Self {
        let name = match control {
            Control::Slider => "slider",
            Control::Whole => "whole",
            Control::Toggle => "toggle",
        };
        self.node(id).params.insert(format!("{input}:control"), Param::Choice(name.into()));
        self
    }

    /// Shows an input as a slider on the effect card, optionally under another name.
    pub fn expose(&mut self, id: u32, input: &str, name: Option<&str>) -> &mut Self {
        let node = self.node(id);
        node.exposed.insert(input.into());
        if let Some(name) = name {
            node.params.insert(format!("{input}:name"), Param::Text(name.into()));
        }
        self
    }

    /// Wires output `out` of block `from` into input `input` of block `to`.
    pub fn wire(&mut self, from: u32, out: &str, to: u32, input: &str) -> &mut Self {
        let catalog = Catalog::get();
        let pin = |id: u32, input: bool, name: &str| {
            let node = self.graph.nodes.iter().find(|n| n.id == id).expect("block added by this builder");
            let block = catalog.block(&node.kind).expect("known block");
            let index = if input {
                block.inputs.iter().position(|i| i.name == name)
            } else {
                block.outputs.iter().position(|o| o.name == name)
            };
            index.unwrap_or_else(|| panic!("{} has no pin {name}", node.kind)) as u8
        };
        let link = Link { from: (from, pin(from, false, out)), to: (to, pin(to, true, input)) };
        self.graph.links.push(link);
        self
    }

    pub fn finish(self) -> Graph {
        self.graph
    }
}

/// Replaces `$input`, `$output` and `$_temp` in a line of block code.
fn substitute(line: &str, id: u32, block: &Block, inputs: &BTreeMap<&str, String>) -> String {
    let mut result = String::with_capacity(line.len() + 16);
    let mut rest = line;
    while let Some(at) = rest.find('$') {
        result.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let end = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(after.len());
        let name = &after[..end];
        if let Some(temp) = name.strip_prefix('_') {
            let _ = write!(result, "n{id}__{temp}");
        } else if let Some(expr) = inputs.get(name) {
            let _ = write!(result, "({expr})");
        } else if block.outputs.iter().any(|o| o.name == name) {
            let _ = write!(result, "n{id}_{name}");
        } else {
            // Not a placeholder; keep it so naga reports it rather than silently dropping text.
            result.push('$');
            result.push_str(name);
        }
        rest = &after[end..];
    }
    result.push_str(rest);
    result
}

/// WGSL for a value of type `from` used where `to` is wanted.
fn convert(expr: &str, from: Ty, to: Ty) -> Result<String, String> {
    let from = if from == Ty::Any { Ty::Number } else { from };
    let to = if to == Ty::Any { Ty::Number } else { to };
    Ok(match (from, to) {
        _ if from == to => expr.to_string(),
        (Ty::Number, Ty::Vector) => format!("vec2<f32>({expr})"),
        (Ty::Number, Ty::Color) => format!("vec3<f32>({expr})"),
        (Ty::Color, Ty::Number) => format!("luma({expr})"),
        (Ty::Vector, Ty::Number) => {
            return Err("a position can't go into a number. Use Split position or Length.".into());
        }
        (Ty::Vector, Ty::Color) => return Err("a position can't go into a colour. Use Make colour.".into()),
        (Ty::Color, Ty::Vector) => return Err("a colour can't go into a position. Use Split colour.".into()),
        _ => unreachable!(),
    })
}

fn zero(ty: Ty) -> &'static str {
    match ty {
        Ty::Number | Ty::Any => "0.0",
        Ty::Vector => "vec2<f32>(0.0)",
        Ty::Color => "vec3<f32>(0.0)",
    }
}

/// The value of an unplugged number input: the block's setting, else the default.
fn number_param(node: &Node, input: &Input) -> f32 {
    let value = match node.params.get(input.name) {
        Some(Param::Number(v)) => Some(*v),
        Some(Param::Bool(b)) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    };
    match (value.filter(|v| v.is_finite()), input.default) {
        (Some(v), _) => v,
        (None, Default::Number(v)) => v,
        _ => 0.0,
    }
}

/// WGSL for an input with nothing plugged in.
fn unplugged(node: &Node, input: &Input, target: Ty) -> String {
    let finite = |v: &[f32]| v.iter().all(|x| x.is_finite());
    match (node.params.get(input.name), input.default) {
        (Some(Param::Color(c)), _) if input.ty == Ty::Color && finite(c) => {
            format!("vec3<f32>({}, {}, {})", literal(c[0]), literal(c[1]), literal(c[2]))
        }
        (Some(Param::Vector(v)), _) if input.ty == Ty::Vector && finite(v) => {
            format!("vec2<f32>({}, {})", literal(v[0]), literal(v[1]))
        }
        (_, Default::Position) => "q0".into(),
        (_, Default::Picture) => "c0".into(),
        (_, Default::Color(c)) => format!("vec3<f32>({}, {}, {})", literal(c[0]), literal(c[1]), literal(c[2])),
        (_, Default::Vector(v)) => format!("vec2<f32>({}, {})", literal(v[0]), literal(v[1])),
        (_, Default::Number(_)) => convert(&literal(number_param(node, input)), Ty::Number, target).unwrap(),
    }
}

/// A number for a `// param` line.
fn number(v: f32) -> String {
    if v.is_finite() { format!("{v:?}") } else { "0".into() }
}

/// A WGSL float literal.
fn literal(v: f32) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    // The `f` suffix keeps calls like `mix(0.0f, 1.0f, 0.5f)` concrete.
    let text = format!("{v:?}");
    if text.contains(['.', 'e']) { format!("{text}f") } else { format!("{text}.0f") }
}

/// A slider name made from a user's label: lower case letters, digits and `_`.
fn identifier(text: &str) -> Option<String> {
    let mut name: String =
        text.trim().to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    while name.contains("__") {
        name = name.replace("__", "_");
    }
    let name = name.trim_matches('_').to_string();
    match name.chars().next() {
        None => None,
        Some(c) if c.is_ascii_digit() => Some(format!("n_{name}")),
        Some(_) => Some(name),
    }
}

/// Text safe to put in a one-line comment.
fn one_line(text: &str) -> String {
    text.lines().next().unwrap_or("").trim().to_string()
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod gpu_tests;
