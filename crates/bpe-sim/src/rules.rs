//! Rules: "when this happens, do that" graphs, run at the end of every step from that step's
//! events.
//!
//! Each of the scene's rule graphs ([`Scene::logic`]) is compiled once into a flat program. A
//! step's events (and the clock, beats and ball count) start chains of blocks; conditions pick
//! which way a chain goes, and actions change balls or the world, or emit an
//! [`EventKind::Action`] that the renderer, sound and captions pick up. Everything is
//! deterministic: the same scene always makes the same choices, so seeking and exporting agree
//! with the preview.

use crate::{Ball, Event, EventKind, NO_BALL};
use bpe_graph::{Catalog, Graph, Node, NodeSpec, Param, ParamSpec, PinSpec, PinType, SpecList};
use bpe_scene::{Scene, Vec2};
use std::sync::{Arc, LazyLock};

/// At most this many actions run in one step, however many rules fire, so a rule that
/// triggers itself (spawn on every spawn) can't stall the simulation.
pub const ACTION_BUDGET: u32 = 256;

/// What a rule's visual, sound or text action asks for. The renderer, soundtrack and captions
/// look these up from [`EventKind::Action`] events with [`RuleState::output`].
#[derive(Clone, Debug, PartialEq)]
pub enum RuleOutput {
    /// A flash over the whole picture, in linear RGBA, fading over about half a second.
    Flash { color: [f32; 4], strength: f32 },
    /// A note, in semitones above the scene's sound, at a volume from 0 to 1.
    Note { pitch: f32, volume: f32 },
    /// Text on screen for a while.
    Caption { text: String, seconds: f32 },
    /// A burst of confetti where the ball is (or the middle).
    Confetti { count: u32, color: [f32; 4] },
    /// A counter changed; the amount is in the event's `speed`.
    Counter { counter: usize },
}

/// The balls and world rules act on. The flat simulation provides one for its own balls, and
/// 3D mode for its balls, so the same rules work in both.
#[allow(clippy::len_without_is_empty)]
pub trait RuleBalls {
    /// Ball slots, removed ones included.
    fn len(&self) -> usize;
    fn alive(&self, i: usize) -> bool;
    fn alive_count(&self) -> usize;
    fn max_balls(&self) -> usize;
    fn position(&self, i: usize) -> Vec2;
    fn velocity(&self, i: usize) -> Vec2;
    fn set_velocity(&mut self, i: usize, velocity: Vec2);
    fn color(&self, i: usize) -> [f32; 4];
    fn set_color(&mut self, i: usize, color: [f32; 4]);
    fn radius(&self, i: usize) -> f32;
    fn set_radius(&mut self, i: usize, radius: f32);
    /// Takes the ball out and records a [`EventKind::Remove`] event.
    fn remove(&mut self, i: usize, time: f64, events: &mut Vec<Event>);
    /// Adds a ball split off `parent`, turned up to `spread` degrees, at `at` or where the
    /// parent is, and records a [`EventKind::Spawn`] event. Returns false if there was no room.
    fn spawn_from(&mut self, parent: usize, spread: f32, at: Option<Vec2>, time: f64, events: &mut Vec<Event>) -> bool;
    fn gravity(&self) -> Vec2;
    fn set_gravity(&mut self, gravity: Vec2);
    /// The middle of the arena, where global effects are placed.
    fn center(&self) -> Vec2;
}

// ---------- the blocks ----------

/// Every kind of rule block, in the order the catalog lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Start,
    Wall,
    Peg,
    BallsTouch,
    Escape,
    Appear,
    Beat,
    Timer,
    At,
    Count,
    CounterReaches,
    Chance,
    EveryNth,
    Once,
    Cooldown,
    Compare,
    Spawn,
    Remove,
    Color,
    Grow,
    Speed,
    Gravity,
    CounterAdd,
    Flash,
    Note,
    Caption,
    Confetti,
    Number,
    Random,
    BallCount,
    CounterValue,
    Time,
    Math,
    BallSpeed,
    RandomBall,
}

const KINDS: [(&str, Kind); 35] = [
    ("start", Kind::Start),
    ("wall", Kind::Wall),
    ("peg", Kind::Peg),
    ("balls_touch", Kind::BallsTouch),
    ("escape", Kind::Escape),
    ("appear", Kind::Appear),
    ("beat", Kind::Beat),
    ("timer", Kind::Timer),
    ("at", Kind::At),
    ("count", Kind::Count),
    ("counter_reaches", Kind::CounterReaches),
    ("chance", Kind::Chance),
    ("every_nth", Kind::EveryNth),
    ("once", Kind::Once),
    ("cooldown", Kind::Cooldown),
    ("compare", Kind::Compare),
    ("spawn", Kind::Spawn),
    ("remove", Kind::Remove),
    ("color", Kind::Color),
    ("grow", Kind::Grow),
    ("speed", Kind::Speed),
    ("gravity", Kind::Gravity),
    ("counter_add", Kind::CounterAdd),
    ("flash", Kind::Flash),
    ("note", Kind::Note),
    ("caption", Kind::Caption),
    ("confetti", Kind::Confetti),
    ("number", Kind::Number),
    ("random", Kind::Random),
    ("ball_count", Kind::BallCount),
    ("counter_value", Kind::CounterValue),
    ("time", Kind::Time),
    ("math", Kind::Math),
    ("ball_speed", Kind::BallSpeed),
    ("random_ball", Kind::RandomBall),
];

fn kind_of(name: &str) -> Option<Kind> {
    KINDS.iter().find(|(k, _)| *k == name).map(|(_, kind)| *kind)
}

pub const WHEN: &str = "When";
pub const IF: &str = "Only if";
pub const DO: &str = "Do";
pub const VALUES: &str = "Values";

/// The blocks rules can use.
pub fn catalog() -> &'static SpecList {
    static CATALOG: LazyLock<SpecList> = LazyLock::new(build_catalog);
    &CATALOG
}

fn build_catalog() -> SpecList {
    let t = PinType::Trigger;
    let then = || PinSpec::new("then", t);
    let does = || PinSpec::new("do", t).required();
    let ball_in =
        || PinSpec::new("ball", PinType::Ball).help("Which ball. Left empty, it is the ball that started the rule.");
    let ball_out = || PinSpec::new("ball", PinType::Ball).help("The ball this happened to");
    let speed_out = || PinSpec::new("speed", PinType::Number).help("How hard it hit, in pixels per second");
    let num =
        |name: &str, label: &str, default: f32, min: f32, max: f32| ParamSpec::number(name, label, default, min, max);
    let color = |name: &str, label: &str, c: [f32; 4]| ParamSpec::new(name, label, Param::Color(c));
    let text = |name: &str, label: &str, value: &str| ParamSpec::new(name, label, Param::Text(value.into()));
    let flag = |name: &str, label: &str, value: bool| ParamSpec::new(name, label, Param::Bool(value));
    let choice = |name: &str, label: &str, options: &[&str]| ParamSpec::choice(name, label, options);
    let specs = vec![
        // When
        NodeSpec::new("start", "When the video starts", WHEN, "Once, at the very start.").output(then()),
        NodeSpec::new("wall", "When a ball hits the wall", WHEN, "Every bounce off the arena.")
            .output(then())
            .output(ball_out())
            .output(speed_out()),
        NodeSpec::new("peg", "When a ball hits a peg", WHEN, "Every bounce off a peg or wall inside the arena.")
            .output(then())
            .output(ball_out())
            .output(speed_out()),
        NodeSpec::new("balls_touch", "When two balls hit", WHEN, "Every time two balls bump into each other.")
            .output(then())
            .output(ball_out())
            .output(speed_out()),
        NodeSpec::new("escape", "When a ball escapes", WHEN, "A ball gets out through a gap in a ring.")
            .output(then())
            .output(ball_out()),
        NodeSpec::new("appear", "When a ball appears", WHEN, "A new ball is added, by the scene or by a rule.")
            .output(then())
            .output(ball_out()),
        NodeSpec::new("beat", "On the beat", WHEN, "On the song's beats, or the scene's tempo.")
            .output(then())
            .param(num("every", "Every … beats", 1.0, 1.0, 16.0))
            .param(flag("downbeats", "Only the first of a bar", false)),
        NodeSpec::new("timer", "Every few seconds", WHEN, "Again and again, on a clock.")
            .output(then())
            .param(num("every", "Every (s)", 1.0, 0.05, 60.0))
            .param(num("from", "Starting at (s)", 0.0, 0.0, 600.0)),
        NodeSpec::new("at", "At a moment", WHEN, "Once, at a set time.")
            .output(then())
            .param(num("time", "At (s)", 5.0, 0.0, 600.0)),
        NodeSpec::new(
            "count",
            "When there are this many balls",
            WHEN,
            "Once, when the ball count first reaches the number.",
        )
        .output(then())
        .param(num("balls", "Balls", 100.0, 1.0, 100000.0)),
        NodeSpec::new(
            "counter_reaches",
            "When a counter reaches",
            WHEN,
            "Once, when a counter first reaches the number.",
        )
        .output(then())
        .param(text("counter", "Counter", "score"))
        .param(num("value", "Value", 10.0, -1e6, 1e6)),
        // Only if
        NodeSpec::new("chance", "Sometimes", IF, "Goes on only some of the time.")
            .input(does())
            .output(then())
            .param(num("percent", "Chance (%)", 50.0, 0.0, 100.0)),
        NodeSpec::new("every_nth", "Every so many times", IF, "Goes on every Nth time, starting with the Nth.")
            .input(does())
            .output(then())
            .param(num("times", "Every", 2.0, 1.0, 1000.0)),
        NodeSpec::new("once", "Only the first time", IF, "Goes on once, then never again.")
            .input(does())
            .output(then()),
        NodeSpec::new("cooldown", "Not too often", IF, "Goes on at most once in this many seconds.")
            .input(does())
            .output(then())
            .param(num("seconds", "Wait (s)", 0.5, 0.0, 60.0)),
        NodeSpec::new("compare", "If a number is", IF, "Goes one way or the other by comparing two numbers.")
            .input(does())
            .input(PinSpec::new("value", PinType::Number))
            .output(then())
            .output(PinSpec::new("otherwise", t))
            .param(num("value", "Number", 0.0, -1e6, 1e6))
            .param(choice("test", "Is", &["more than", "less than", "equal to"]))
            .param(num("than", "Than", 10.0, -1e6, 1e6)),
        // Do
        NodeSpec::new(
            "spawn",
            "Spawn balls",
            DO,
            "Adds balls that split off the ball, like the scene's own splitting.",
        )
        .input(does())
        .input(ball_in())
        .input(PinSpec::new("count", PinType::Number))
        .output(then())
        .param(num("count", "How many", 3.0, 1.0, 64.0))
        .param(num("spread", "Spread (°)", 40.0, 0.0, 360.0))
        .param(choice("from", "Start", &["where the ball is", "in the middle"])),
        NodeSpec::new("remove", "Remove the ball", DO, "Takes the ball out of the scene.")
            .input(does())
            .input(ball_in())
            .output(then()),
        NodeSpec::new("color", "Colour the ball", DO, "Gives the ball a new colour.")
            .input(does())
            .input(ball_in())
            .output(then())
            .param(color("color", "Colour", [1.0, 0.1, 0.1, 1.0])),
        NodeSpec::new("grow", "Grow the ball", DO, "Makes the ball bigger (or smaller, with a negative amount).")
            .input(does())
            .input(ball_in())
            .input(PinSpec::new("amount", PinType::Number))
            .output(then())
            .param(num("amount", "By (px)", 1.0, -100.0, 100.0))
            .param(num("largest", "Up to (px)", 80.0, 1.0, 1000.0)),
        NodeSpec::new("speed", "Change the ball's speed", DO, "Multiplies the ball's speed.")
            .input(does())
            .input(ball_in())
            .input(PinSpec::new("times", PinType::Number))
            .output(then())
            .param(num("times", "Times", 1.1, 0.0, 10.0)),
        NodeSpec::new("gravity", "Change gravity", DO, "Adds to downward gravity, or sets it.")
            .input(does())
            .input(PinSpec::new("amount", PinType::Number))
            .output(then())
            .param(choice("mode", "How", &["add", "set"]))
            .param(num("amount", "Amount", 100.0, -10000.0, 10000.0)),
        NodeSpec::new("counter_add", "Add to a counter", DO, "Counters can be shown in captions or start other rules.")
            .input(does())
            .input(PinSpec::new("amount", PinType::Number))
            .output(then())
            .param(text("counter", "Counter", "score"))
            .param(num("amount", "Add", 1.0, -1e6, 1e6)),
        NodeSpec::new("flash", "Flash the screen", DO, "A quick flash of colour over the whole picture.")
            .input(does())
            .output(then())
            .param(color("color", "Colour", [1.0, 0.1, 0.1, 1.0]))
            .param(num("strength", "Strength", 1.0, 0.0, 4.0)),
        NodeSpec::new("note", "Play a note", DO, "Plays the scene's instrument.")
            .input(does())
            .input(ball_in())
            .output(then())
            .param(num("pitch", "Pitch (semitones)", 12.0, -36.0, 36.0))
            .param(num("volume", "Volume", 0.8, 0.0, 1.0)),
        NodeSpec::new("caption", "Show text", DO, "Puts words on screen for a while.")
            .input(does())
            .output(then())
            .param(text("text", "Text", "CHAOS!"))
            .param(num("seconds", "For (s)", 2.0, 0.1, 60.0)),
        NodeSpec::new("confetti", "Burst of confetti", DO, "A burst of particles where the ball is.")
            .input(does())
            .input(ball_in())
            .output(then())
            .param(num("count", "Pieces", 40.0, 1.0, 1000.0))
            .param(color("color", "Colour", [1.0, 0.85, 0.2, 1.0])),
        // Values
        NodeSpec::new("number", "Number", VALUES, "A fixed number.")
            .output(PinSpec::new("value", PinType::Number))
            .param(num("value", "Value", 1.0, -1e6, 1e6)),
        NodeSpec::new("random", "Random number", VALUES, "A new random number each time it is used.")
            .output(PinSpec::new("value", PinType::Number))
            .param(num("from", "From", 0.0, -1e6, 1e6))
            .param(num("to", "To", 1.0, -1e6, 1e6)),
        NodeSpec::new("ball_count", "Ball count", VALUES, "How many balls are in play.")
            .output(PinSpec::new("value", PinType::Number)),
        NodeSpec::new("counter_value", "Counter", VALUES, "A counter's value.")
            .output(PinSpec::new("value", PinType::Number))
            .param(text("counter", "Counter", "score")),
        NodeSpec::new("time", "Time", VALUES, "Seconds since the start.")
            .output(PinSpec::new("value", PinType::Number)),
        NodeSpec::new("math", "Math", VALUES, "Works out a number from two others.")
            .input(PinSpec::new("a", PinType::Number))
            .input(PinSpec::new("b", PinType::Number))
            .output(PinSpec::new("value", PinType::Number))
            .param(num("a", "A", 0.0, -1e6, 1e6))
            .param(choice("op", "Do", &["plus", "minus", "times", "divided by", "smaller of", "larger of"]))
            .param(num("b", "B", 1.0, -1e6, 1e6)),
        NodeSpec::new("ball_speed", "Ball speed", VALUES, "How fast the ball is going.")
            .input(ball_in())
            .output(PinSpec::new("value", PinType::Number)),
        NodeSpec::new("random_ball", "A random ball", VALUES, "Any ball still in play, picked at random.")
            .output(PinSpec::new("ball", PinType::Ball)),
    ];
    debug_assert_eq!(specs.len(), KINDS.len());
    SpecList { specs, cycles: false }
}

// ---------- recipes ----------

/// A ready-made rule set: its name, a sentence about it, and the graph.
pub struct Recipe {
    pub name: &'static str,
    pub description: &'static str,
    pub graph: Graph,
}

/// Builds a chain of blocks left to right, each wired from the previous one's first output.
fn chain(name: &str, blocks: &[(&str, &[(&str, Param)])]) -> Graph {
    let catalog = catalog();
    let mut graph = Graph::new(name);
    let mut previous: Option<u32> = None;
    for (i, (kind, params)) in blocks.iter().enumerate() {
        let spec = catalog.spec(kind).expect("recipe block in catalog");
        let id = graph.add(spec, [i as f32 * 260.0, 0.0]);
        let node = graph.node_mut(id).expect("just added");
        for (key, value) in params.iter() {
            node.params.insert(key.to_string(), value.clone());
            node.exposed.insert(key.to_string());
        }
        if let Some(from) = previous {
            graph.connect(bpe_graph::Link { from: (from, 0), to: (id, 0) }, catalog);
        }
        previous = Some(id);
    }
    graph
}

pub fn recipes() -> Vec<Recipe> {
    use Param::{Choice, Color, Number, Text};
    vec![
        Recipe {
            name: "Escape and multiply",
            description: "When a ball escapes, two more start in the middle and the screen flashes red.",
            graph: chain(
                "Escape and multiply",
                &[
                    ("escape", &[]),
                    ("spawn", &[("count", Number(2.0)), ("from", Choice("in the middle".into()))]),
                    ("flash", &[("color", Color([1.0, 0.05, 0.05, 1.0])), ("strength", Number(0.6))]),
                ],
            ),
        },
        Recipe {
            name: "Last ball standing",
            description: "Each wall hit has a small chance to remove the ball, until one is left.",
            graph: {
                let mut graph = chain(
                    "Last ball standing",
                    &[
                        ("wall", &[]),
                        ("compare", &[("test", Choice("more than".into())), ("than", Number(1.0))]),
                        ("chance", &[("percent", Number(8.0))]),
                        ("remove", &[]),
                    ],
                );
                let count = graph.add(catalog().spec("ball_count").expect("in catalog"), [260.0, 200.0]);
                graph.connect(bpe_graph::Link { from: (count, 0), to: (2, 1) }, catalog());
                graph
            },
        },
        Recipe {
            name: "Grow on every hit",
            description: "Every wall hit makes the ball a little bigger.",
            graph: chain(
                "Grow on every hit",
                &[("wall", &[]), ("grow", &[("amount", Number(1.0)), ("largest", Number(80.0))])],
            ),
        },
        Recipe {
            name: "Beat confetti",
            description: "A burst of confetti on the first beat of every bar.",
            graph: chain(
                "Beat confetti",
                &[("beat", &[("downbeats", Param::Bool(true))]), ("confetti", &[("count", Number(60.0))])],
            ),
        },
        Recipe {
            name: "Chaos caption at 100 balls",
            description: "When there are 100 balls, CHAOS! flashes up on screen.",
            graph: chain(
                "Chaos caption",
                &[
                    ("count", &[("balls", Number(100.0))]),
                    ("caption", &[("text", Text("CHAOS!".into())), ("seconds", Number(3.0))]),
                    ("flash", &[("color", Color([1.0, 1.0, 1.0, 1.0])), ("strength", Number(0.8))]),
                ],
            ),
        },
    ]
}

// ---------- compiling ----------

/// One block, ready to run.
#[derive(Clone, Debug)]
struct Block {
    kind: Kind,
    rule: u16,
    id: u32,
    /// Numbers from the block's settings, in the order [`compile_block`] reads them.
    num: [f32; 4],
    color: [f32; 4],
    flag: bool,
    /// A counter index, a choice index or an output index, depending on the kind.
    pick: usize,
    /// For each input pin, the block and output pin wired into it.
    inputs: Vec<Option<(u32, u8)>>,
    /// For each trigger output pin, the blocks it starts, in wire order.
    outputs: Vec<Vec<u32>>,
}

#[derive(Debug, Default)]
struct Program {
    blocks: Vec<Block>,
    starts: Vec<u32>,
    walls: Vec<u32>,
    pegs: Vec<u32>,
    touches: Vec<u32>,
    escapes: Vec<u32>,
    appears: Vec<u32>,
    beats: Vec<u32>,
    clocks: Vec<u32>,
    watches: Vec<u32>,
    counters: Vec<String>,
    outputs: Vec<RuleOutput>,
    /// (rule, block id) to block index, sorted, for looking up actions from events.
    index: Vec<((u16, u32), u32)>,
}

fn counter_index(counters: &mut Vec<String>, name: &str) -> usize {
    let name = name.trim().to_lowercase();
    counters.iter().position(|c| *c == name).unwrap_or_else(|| {
        counters.push(name);
        counters.len() - 1
    })
}

fn compile(scene: &Scene) -> Option<Program> {
    let catalog = catalog();
    let mut program = Program::default();
    for (rule, graph) in scene.logic.iter().enumerate().filter(|(_, g)| g.enabled) {
        compile_graph(&mut program, rule as u16, graph, catalog);
    }
    if program.blocks.is_empty() {
        return None;
    }
    program.index.sort_by_key(|(key, _)| *key);
    Some(program)
}

fn compile_graph(program: &mut Program, rule: u16, graph: &Graph, catalog: &SpecList) {
    let base = program.blocks.len() as u32;
    // Blocks that can't run (unknown or switched off) keep their slot so wires still line up,
    // but nothing reaches them. Action events name their block in 16 bits, so a block with a
    // larger id (only from a hand-edited file) doesn't run either.
    let usable = |node: &Node| {
        node.enabled
            && node.id <= u16::MAX as u32
            && kind_of(&node.kind).is_some()
            && catalog.spec(&node.kind).is_some()
    };
    let slot = |id: u32| graph.nodes.iter().position(|n| n.id == id).map(|i| base + i as u32);
    for node in &graph.nodes {
        let spec = catalog.spec(&node.kind);
        let (inputs, outputs) = spec.map_or((0, 0), |s| (s.inputs.len(), s.outputs.len()));
        let mut block = Block {
            kind: kind_of(&node.kind).unwrap_or(Kind::Number),
            rule,
            id: node.id,
            num: [0.0; 4],
            color: [1.0; 4],
            flag: false,
            pick: 0,
            inputs: vec![None; inputs],
            outputs: vec![Vec::new(); outputs],
        };
        if let Some(spec) = spec.filter(|_| usable(node)) {
            compile_block(program, &mut block, node, spec);
        }
        program.blocks.push(block);
    }
    for link in &graph.links {
        let (Some(from), Some(to)) = (slot(link.from.0), slot(link.to.0)) else { continue };
        let (from_node, to_node) = (&graph.nodes[(from - base) as usize], &graph.nodes[(to - base) as usize]);
        if !usable(from_node) || !usable(to_node) {
            continue;
        }
        let (Some(out_ty), Some(in_ty)) = (graph.output_type(link.from, catalog), graph.input_type(link.to, catalog))
        else {
            continue;
        };
        if !catalog.accepts(in_ty, out_ty) {
            continue;
        }
        if out_ty == PinType::Trigger {
            if let Some(list) = program.blocks[from as usize].outputs.get_mut(link.from.1 as usize) {
                list.push(to);
            }
        } else if let Some(input) = program.blocks[to as usize].inputs.get_mut(link.to.1 as usize) {
            *input = Some((from, link.from.1));
        }
    }
    // A graph with a loop would never finish a chain, so its data wires are dropped; trigger
    // chains are bounded by the action budget anyway.
    if bpe_graph::order(graph).is_err() {
        for block in &mut program.blocks[base as usize..] {
            block.inputs.iter_mut().for_each(|i| *i = None);
        }
    }
    for (i, node) in graph.nodes.iter().enumerate() {
        let index = base + i as u32;
        if !usable(node) {
            continue;
        }
        program.index.push(((rule, node.id), index));
        let list = match program.blocks[index as usize].kind {
            Kind::Start => &mut program.starts,
            Kind::Wall => &mut program.walls,
            Kind::Peg => &mut program.pegs,
            Kind::BallsTouch => &mut program.touches,
            Kind::Escape => &mut program.escapes,
            Kind::Appear => &mut program.appears,
            Kind::Beat => &mut program.beats,
            Kind::Timer | Kind::At => &mut program.clocks,
            Kind::Count | Kind::CounterReaches => &mut program.watches,
            _ => continue,
        };
        list.push(index);
    }
}

fn compile_block(program: &mut Program, block: &mut Block, node: &Node, spec: &NodeSpec) {
    let n = |name: &str| node.number(spec, name);
    let c = |name: &str| node.param(spec, name).and_then(Param::color).unwrap_or([1.0; 4]);
    let choice = |name: &str| {
        let value = node.choice(spec, name);
        spec.param_spec(name).and_then(|p| p.choices.iter().position(|c| c == value)).unwrap_or(0)
    };
    let text = |name: &str| node.param(spec, name).and_then(Param::text).unwrap_or_default().to_string();
    match block.kind {
        Kind::Beat => {
            block.num[0] = n("every").max(1.0).round();
            block.flag = node.param(spec, "downbeats").and_then(Param::as_bool).unwrap_or(false);
        }
        Kind::Timer => block.num = [n("every").max(1.0 / 240.0), n("from").max(0.0), 0.0, 0.0],
        Kind::At => block.num[0] = n("time"),
        Kind::Count => block.num[0] = n("balls"),
        Kind::CounterReaches => {
            block.pick = counter_index(&mut program.counters, &text("counter"));
            block.num[0] = n("value");
        }
        Kind::Chance => block.num[0] = n("percent") / 100.0,
        Kind::EveryNth => block.num[0] = n("times").max(1.0).round(),
        Kind::Cooldown => block.num[0] = n("seconds"),
        Kind::Compare => {
            block.num = [n("value"), n("than"), 0.0, 0.0];
            block.pick = choice("test");
        }
        Kind::Spawn => {
            block.num = [n("count"), n("spread"), 0.0, 0.0];
            block.flag = choice("from") == 1;
        }
        Kind::Color => block.color = c("color"),
        Kind::Grow => block.num = [n("amount"), n("largest"), 0.0, 0.0],
        Kind::Speed => block.num[0] = n("times"),
        Kind::Gravity => {
            block.num[0] = n("amount");
            block.pick = choice("mode");
        }
        Kind::CounterAdd => {
            let counter = counter_index(&mut program.counters, &text("counter"));
            block.num[0] = n("amount");
            block.pick = output(program, RuleOutput::Counter { counter });
        }
        Kind::Flash => {
            let output_index = output(program, RuleOutput::Flash { color: c("color"), strength: n("strength") });
            block.pick = output_index;
        }
        Kind::Note => {
            block.pick = output(program, RuleOutput::Note { pitch: n("pitch"), volume: n("volume").clamp(0.0, 1.0) });
        }
        Kind::Caption => {
            block.pick = output(program, RuleOutput::Caption { text: text("text"), seconds: n("seconds") });
        }
        Kind::Confetti => {
            let count = n("count").max(1.0) as u32;
            block.pick = output(program, RuleOutput::Confetti { count, color: c("color") });
        }
        Kind::Number => block.num[0] = n("value"),
        Kind::Random => block.num = [n("from"), n("to"), 0.0, 0.0],
        Kind::CounterValue => block.pick = counter_index(&mut program.counters, &text("counter")),
        Kind::Math => {
            block.num = [n("a"), n("b"), 0.0, 0.0];
            block.pick = choice("op");
        }
        _ => {}
    }
}

fn output(program: &mut Program, output: RuleOutput) -> usize {
    program.outputs.push(output);
    program.outputs.len() - 1
}

// ---------- running ----------

/// Who and what started a chain.
#[derive(Clone, Copy, Debug)]
struct Context {
    ball: u32,
    speed: f32,
}

const NOBODY: Context = Context { ball: NO_BALL, speed: 0.0 };

/// The scene's rules, compiled once, and what they have done so far. Cloned with the
/// simulation for seeking, so it holds only plain values.
#[derive(Clone, Debug, Default)]
pub struct RuleState {
    program: Option<Arc<Program>>,
    counters: Vec<f64>,
    /// How many times each block has fired, and when it last did.
    fired: Vec<u32>,
    last_fired: Vec<f64>,
    /// Per block: the next clock time, a run count, or whether a watch has gone off.
    memory: Vec<f64>,
    rng: u64,
    started: bool,
    /// Scratch for chains being run, kept to avoid allocating every step.
    stack: Vec<(u32, Context)>,
}

impl RuleState {
    pub fn new(scene: &Scene) -> Self {
        let Some(program) = compile(scene) else { return Self::default() };
        let blocks = program.blocks.len();
        let mut memory = vec![0.0; blocks];
        for &i in &program.clocks {
            let block = &program.blocks[i as usize];
            memory[i as usize] = if block.kind == Kind::Timer { block.num[1] as f64 } else { block.num[0] as f64 };
        }
        Self {
            counters: vec![0.0; program.counters.len()],
            fired: vec![0; blocks],
            last_fired: vec![f64::NEG_INFINITY; blocks],
            memory,
            rng: 0x9e37_79b9_7f4a_7c15,
            started: false,
            stack: Vec::with_capacity(64),
            program: Some(Arc::new(program)),
        }
    }

    /// Whether the scene has any rules that can run.
    pub fn is_empty(&self) -> bool {
        self.program.is_none()
    }

    /// For an "add to counter" action event: which counter it changes and by how much.
    pub fn counter_change(&self, event: &Event) -> Option<(usize, f64)> {
        match self.output(event)? {
            RuleOutput::Counter { counter } => Some((*counter, event.speed as f64)),
            _ => None,
        }
    }

    /// What an [`EventKind::Action`] event asks the picture, sound or captions to do.
    pub fn output(&self, event: &Event) -> Option<&RuleOutput> {
        let EventKind::Action { rule, action } = event.kind else { return None };
        let program = self.program.as_ref()?;
        let index = self.block_index(rule, action as u32)?;
        program.outputs.get(program.blocks[index].pick)
    }

    fn block_index(&self, rule: u16, id: u32) -> Option<usize> {
        let program = self.program.as_ref()?;
        let found = program.index.binary_search_by_key(&(rule, id), |(key, _)| *key).ok()?;
        Some(program.index[found].1 as usize)
    }

    /// Counter names, lower case, in counter order.
    pub fn counter_names(&self) -> &[String] {
        self.program.as_ref().map_or(&[], |p| &p.counters)
    }

    /// Counter values now.
    pub fn counters(&self) -> &[f64] {
        &self.counters
    }

    /// How many times a block of rule set `rule` has fired, and when it last did.
    pub fn fired(&self, rule: u16, node: u32) -> Option<(u32, f64)> {
        let index = self.block_index(rule, node)?;
        Some((self.fired[index], self.last_fired[index]))
    }

    fn random(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        (x >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// Runs the rules on the events from `first` onwards, which happened during the step ending at
/// `now`, plus the clock, beat and count triggers.
pub fn run(state: &mut RuleState, now: f64, first: usize, events: &mut Vec<Event>, balls: &mut dyn RuleBalls) {
    let Some(program) = state.program.clone() else { return };
    let mut budget = ACTION_BUDGET;
    if !state.started {
        state.started = true;
        for &block in &program.starts {
            fire(state, &program, block, NOBODY, now, events, balls, &mut budget);
        }
    }
    for &block in &program.clocks {
        let b = &program.blocks[block as usize];
        let due = state.memory[block as usize];
        if now + 1e-9 < due || due.is_infinite() {
            continue;
        }
        // A timer fires at most once a step; one that falls behind skips ahead.
        state.memory[block as usize] = match b.kind {
            Kind::Timer => {
                let every = b.num[0] as f64;
                let next = due + every;
                if next <= now { now + every } else { next }
            }
            _ => f64::INFINITY,
        };
        fire(state, &program, block, NOBODY, now, events, balls, &mut budget);
    }
    let mut i = first;
    while i < events.len() {
        let event = events[i];
        i += 1;
        let list = match event.kind {
            EventKind::Wall => &program.walls,
            EventKind::Obstacle(_) => &program.pegs,
            EventKind::Ball(_) => &program.touches,
            EventKind::Escape(_) => &program.escapes,
            EventKind::Spawn => &program.appears,
            EventKind::Beat { downbeat, .. } => {
                for &block in &program.beats {
                    let b = &program.blocks[block as usize];
                    let counted = if b.flag { downbeat } else { true };
                    if !counted {
                        continue;
                    }
                    let n = state.memory[block as usize] as u64;
                    state.memory[block as usize] += 1.0;
                    if n.is_multiple_of(b.num[0] as u64) {
                        // Logged at the end of the step like every other action, so the log stays in time order.
                        fire(state, &program, block, NOBODY, now, events, balls, &mut budget);
                    }
                }
                continue;
            }
            _ => continue,
        };
        let context = Context { ball: event.ball, speed: event.speed };
        for &block in list {
            fire(state, &program, block, context, now, events, balls, &mut budget);
        }
    }
    for &block in &program.watches {
        if state.memory[block as usize] != 0.0 {
            continue;
        }
        let b = &program.blocks[block as usize];
        let reached = match b.kind {
            Kind::Count => balls.alive_count() as f32 >= b.num[0],
            _ => state.counters.get(b.pick).is_some_and(|&v| v >= b.num[0] as f64),
        };
        if reached {
            state.memory[block as usize] = 1.0;
            fire(state, &program, block, NOBODY, now, events, balls, &mut budget);
        }
    }
}

/// Starts the chains wired to a trigger block's "then".
#[allow(clippy::too_many_arguments)]
fn fire(
    state: &mut RuleState,
    program: &Program,
    trigger: u32,
    context: Context,
    now: f64,
    events: &mut Vec<Event>,
    balls: &mut dyn RuleBalls,
    budget: &mut u32,
) {
    mark(state, trigger, now);
    let start = state.stack.len();
    if let Some(next) = program.blocks[trigger as usize].outputs.first() {
        state.stack.extend(next.iter().rev().map(|&b| (b, context)));
    }
    while state.stack.len() > start {
        let (index, context) = state.stack.pop().expect("stack has entries");
        if *budget == 0 {
            state.stack.truncate(start);
            return;
        }
        let pin = step(state, program, index, context, now, events, balls, budget);
        if let Some(pin) = pin
            && let Some(next) = program.blocks[index as usize].outputs.get(pin)
        {
            state.stack.extend(next.iter().rev().map(|&b| (b, context)));
        }
    }
}

fn mark(state: &mut RuleState, block: u32, now: f64) {
    state.fired[block as usize] = state.fired[block as usize].saturating_add(1);
    state.last_fired[block as usize] = now;
}

/// Runs one condition or action and says which trigger output to carry on through, if any.
#[allow(clippy::too_many_arguments)]
fn step(
    state: &mut RuleState,
    program: &Program,
    index: u32,
    context: Context,
    now: f64,
    events: &mut Vec<Event>,
    balls: &mut dyn RuleBalls,
    budget: &mut u32,
) -> Option<usize> {
    let block = &program.blocks[index as usize];
    let i = index as usize;
    let go = match block.kind {
        Kind::Chance => state.random() < block.num[0],
        Kind::EveryNth => {
            state.memory[i] += 1.0;
            (state.memory[i] as u64).is_multiple_of(block.num[0] as u64)
        }
        Kind::Once => {
            let first = state.memory[i] == 0.0;
            state.memory[i] = 1.0;
            first
        }
        Kind::Cooldown => {
            let ready = state.fired[i] == 0 || now - state.last_fired[i] >= block.num[0] as f64 - 1e-9;
            if ready {
                mark(state, index, now);
            }
            return ready.then_some(0);
        }
        Kind::Compare => {
            let value = number(state, program, index, 1, block.num[0], context, now, balls);
            let than = block.num[1];
            let yes = match block.pick {
                0 => value > than,
                1 => value < than,
                _ => (value - than).abs() < 1e-4,
            };
            mark(state, index, now);
            return Some(if yes { 0 } else { 1 });
        }
        _ => {
            if is_action(block.kind) {
                *budget -= 1;
                act(state, program, index, context, now, events, balls);
            }
            true
        }
    };
    if go {
        mark(state, index, now);
    }
    go.then_some(0)
}

fn is_action(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Spawn
            | Kind::Remove
            | Kind::Color
            | Kind::Grow
            | Kind::Speed
            | Kind::Gravity
            | Kind::CounterAdd
            | Kind::Flash
            | Kind::Note
            | Kind::Caption
            | Kind::Confetti
    )
}

fn act(
    state: &mut RuleState,
    program: &Program,
    index: u32,
    context: Context,
    now: f64,
    events: &mut Vec<Event>,
    balls: &mut dyn RuleBalls,
) {
    let block = &program.blocks[index as usize];
    let target = ball_input(state, program, index, 1, context, balls);
    let emit = |events: &mut Vec<Event>, ball: Option<usize>, amount: f32, balls: &dyn RuleBalls| {
        let position = ball.map_or_else(|| balls.center(), |b| balls.position(b));
        let ball = ball.map_or(NO_BALL, |b| b as u32);
        let kind = EventKind::Action { rule: block.rule, action: block.id as u16 };
        events.push(Event { time: now, ball, position, depth: 0.0, speed: amount, kind });
    };
    match block.kind {
        Kind::Spawn => {
            let count = number(state, program, index, 2, block.num[0], context, now, balls).clamp(0.0, 64.0) as u32;
            let parent = target.or_else(|| random_ball(state, balls));
            if let Some(parent) = parent {
                for _ in 0..count {
                    let at = block.flag.then(|| balls.center());
                    if balls.alive_count() >= balls.max_balls()
                        || !balls.spawn_from(parent, block.num[1], at, now, events)
                    {
                        break;
                    }
                }
            }
        }
        Kind::Remove => {
            if let Some(b) = target {
                balls.remove(b, now, events);
            }
        }
        Kind::Color => {
            if let Some(b) = target {
                balls.set_color(b, block.color);
            }
        }
        Kind::Grow => {
            if let Some(b) = target {
                let amount = number(state, program, index, 2, block.num[0], context, now, balls);
                let radius = (balls.radius(b) + amount).clamp(0.5, block.num[1].max(0.5));
                balls.set_radius(b, radius);
            }
        }
        Kind::Speed => {
            if let Some(b) = target {
                let times = number(state, program, index, 2, block.num[0], context, now, balls);
                if times.is_finite() {
                    balls.set_velocity(b, balls.velocity(b) * times.clamp(0.0, 10.0));
                }
            }
        }
        Kind::Gravity => {
            let amount = number(state, program, index, 1, block.num[0], context, now, balls);
            if amount.is_finite() {
                let mut gravity = balls.gravity();
                gravity.y = if block.pick == 1 { amount } else { gravity.y + amount };
                balls.set_gravity(gravity);
            }
        }
        Kind::CounterAdd => {
            let amount = number(state, program, index, 1, block.num[0], context, now, balls);
            if let Some(RuleOutput::Counter { counter }) = program.outputs.get(block.pick)
                && amount.is_finite()
            {
                state.counters[*counter] += amount as f64;
                emit(events, None, amount, balls);
            }
        }
        Kind::Flash | Kind::Caption => emit(events, None, 0.0, balls),
        Kind::Note | Kind::Confetti => {
            let speed = target.map_or(0.0, |b| balls.velocity(b).length());
            emit(events, target, speed, balls);
        }
        _ => {}
    }
}

/// The ball wired into input `pin`, or the ball that started the chain.
fn ball_input(
    state: &mut RuleState,
    program: &Program,
    index: u32,
    pin: usize,
    context: Context,
    balls: &dyn RuleBalls,
) -> Option<usize> {
    let block = &program.blocks[index as usize];
    let wired = block.inputs.get(pin).copied().flatten();
    let ball = match wired {
        Some((from, _)) if program.blocks[from as usize].kind == Kind::RandomBall => random_ball(state, balls)?,
        _ => context.ball as usize,
    };
    (context.ball != NO_BALL || wired.is_some()).then_some(ball).filter(|&b| b < balls.len() && balls.alive(b))
}

fn random_ball(state: &mut RuleState, balls: &dyn RuleBalls) -> Option<usize> {
    let alive = balls.alive_count();
    if alive == 0 {
        return None;
    }
    let mut pick = ((state.random() * alive as f32) as usize).min(alive - 1);
    (0..balls.len()).find(|&b| {
        if !balls.alive(b) {
            return false;
        }
        if pick == 0 {
            return true;
        }
        pick -= 1;
        false
    })
}

/// The number wired into input `pin`, or `fallback` (the block's own setting).
#[allow(clippy::too_many_arguments)]
fn number(
    state: &mut RuleState,
    program: &Program,
    index: u32,
    pin: usize,
    fallback: f32,
    context: Context,
    now: f64,
    balls: &dyn RuleBalls,
) -> f32 {
    match program.blocks[index as usize].inputs.get(pin).copied().flatten() {
        Some((from, out)) => value(state, program, from, out, context, now, balls, 0),
        None => fallback,
    }
}

#[allow(clippy::too_many_arguments)]
fn value(
    state: &mut RuleState,
    program: &Program,
    index: u32,
    out: u8,
    context: Context,
    now: f64,
    balls: &dyn RuleBalls,
    depth: u32,
) -> f32 {
    if depth > 64 {
        return 0.0;
    }
    let block = &program.blocks[index as usize];
    let input = |state: &mut RuleState, pin: usize, fallback: f32| match block.inputs.get(pin).copied().flatten() {
        Some((from, out)) => value(state, program, from, out, context, now, balls, depth + 1),
        None => fallback,
    };
    match block.kind {
        // Trigger outputs: 1 is the ball, 2 its speed.
        Kind::Wall | Kind::Peg | Kind::BallsTouch | Kind::Escape | Kind::Appear => match out {
            2 => context.speed,
            _ => context.ball as f32,
        },
        Kind::Number => block.num[0],
        Kind::Random => {
            let t = state.random();
            block.num[0] + (block.num[1] - block.num[0]) * t
        }
        Kind::BallCount => balls.alive_count() as f32,
        Kind::CounterValue => state.counters.get(block.pick).copied().unwrap_or(0.0) as f32,
        Kind::Time => now as f32,
        Kind::Math => {
            let a = input(state, 0, block.num[0]);
            let b = input(state, 1, block.num[1]);
            match block.pick {
                0 => a + b,
                1 => a - b,
                2 => a * b,
                3 if b != 0.0 => a / b,
                3 => 0.0,
                4 => a.min(b),
                _ => a.max(b),
            }
        }
        Kind::BallSpeed => {
            let ball = match block.inputs.first().copied().flatten() {
                Some((from, _)) if program.blocks[from as usize].kind == Kind::RandomBall => random_ball(state, balls),
                _ => (context.ball != NO_BALL).then_some(context.ball as usize),
            };
            ball.filter(|&b| b < balls.len()).map_or(0.0, |b| balls.velocity(b).length())
        }
        _ => 0.0,
    }
}

/// The flat simulation's balls, for rules.
pub(crate) struct FlatBalls<'a>(pub &'a mut crate::Simulation);

impl RuleBalls for FlatBalls<'_> {
    fn len(&self) -> usize {
        self.0.balls.len()
    }
    fn alive(&self, i: usize) -> bool {
        self.0.balls.get(i).is_some_and(|b| b.alive)
    }
    fn alive_count(&self) -> usize {
        self.0.alive_balls()
    }
    fn max_balls(&self) -> usize {
        self.0.scene.rules.max_balls as usize
    }
    fn position(&self, i: usize) -> Vec2 {
        self.0.balls[i].position
    }
    fn velocity(&self, i: usize) -> Vec2 {
        self.0.balls[i].velocity
    }
    fn set_velocity(&mut self, i: usize, velocity: Vec2) {
        self.0.balls[i].velocity = velocity;
    }
    fn color(&self, i: usize) -> [f32; 4] {
        self.0.balls[i].color
    }
    fn set_color(&mut self, i: usize, color: [f32; 4]) {
        self.0.balls[i].color = color;
    }
    fn radius(&self, i: usize) -> f32 {
        self.0.balls[i].radius
    }
    fn set_radius(&mut self, i: usize, radius: f32) {
        self.0.balls[i].radius = radius;
    }
    fn remove(&mut self, i: usize, time: f64, events: &mut Vec<Event>) {
        self.0.remove(i, time, events);
    }
    fn spawn_from(&mut self, parent: usize, spread: f32, at: Option<Vec2>, time: f64, events: &mut Vec<Event>) -> bool {
        let Some(parent) = self.0.balls.get(parent) else { return false };
        let mut child: Ball = crate::split_ball(parent, self.0.balls.len(), spread);
        if let Some(at) = at {
            child.position = at;
        }
        self.0.spawn(child, time, events);
        true
    }
    fn gravity(&self) -> Vec2 {
        self.0.scene.physics.gravity
    }
    fn set_gravity(&mut self, gravity: Vec2) {
        self.0.scene.physics.gravity = gravity;
    }
    fn center(&self) -> Vec2 {
        self.0.scene.container.center()
    }
}

#[cfg(test)]
mod tests;
