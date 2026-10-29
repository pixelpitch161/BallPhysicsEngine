//! Ready-made rules the inspector's "Add rule" menu drops into a graph.

use bpe_graph::{Catalog, Graph, Link, Param};
use bpe_vm::{Role, catalog};

/// A ready-made rule.
pub struct Recipe {
    pub name: &'static str,
    pub help: &'static str,
    /// Adds the rule's blocks below whatever is in the graph and returns the (block, setting)
    /// pairs worth showing at Simple depth.
    pub add: fn(&mut Graph) -> Vec<(u32, String)>,
}

/// Builds one chain of blocks left to right, below everything already in the graph.
struct Chain<'a> {
    g: &'a mut Graph,
    x: f32,
    y: f32,
    last: Option<u32>,
}

const STEP_X: f32 = 220.0;
const STEP_Y: f32 = 140.0;

impl<'a> Chain<'a> {
    fn new(g: &'a mut Graph) -> Self {
        let y = g.nodes.iter().map(|n| n.pos[1]).fold(f32::NEG_INFINITY, f32::max);
        let y = if y.is_finite() { y + STEP_Y } else { 40.0 };
        Chain { g, x: 40.0, y, last: None }
    }

    /// Adds a block and wires the previous block's trigger into it.
    fn then(&mut self, kind: &str) -> u32 {
        let c = catalog(Role::Rules);
        let id = self.g.add(c.spec(kind).expect("a recipe uses blocks the catalog has"), [self.x, self.y]);
        if let Some(last) = self.last {
            self.g.connect(Link { from: (last, 0), to: (id, 0) }, c);
        }
        self.last = Some(id);
        self.x += STEP_X;
        id
    }

    /// Adds a value block under the chain, wired into an input of `to`.
    fn feed(&mut self, kind: &str, to: u32, pin: u8) -> u32 {
        let c = catalog(Role::Rules);
        let id =
            self.g.add(c.spec(kind).expect("a recipe uses blocks the catalog has"), [self.x - STEP_X, self.y + 90.0]);
        self.g.connect(Link { from: (id, 0), to: (to, pin) }, c);
        id
    }

    fn set(&mut self, node: u32, name: &str, p: Param) {
        if let Some(n) = self.g.node_mut(node) {
            n.params.insert(name.into(), p);
        }
    }
}

fn simple(node: u32, param: &str) -> Vec<(u32, String)> {
    vec![(node, param.to_owned())]
}

fn grow(g: &mut Graph) -> Vec<(u32, String)> {
    let mut c = Chain::new(g);
    c.then("when.hit");
    let grow = c.then("do.grow");
    simple(grow, "by")
}

fn shrink(g: &mut Graph) -> Vec<(u32, String)> {
    let mut c = Chain::new(g);
    c.then("when.hit");
    let grow = c.then("do.grow");
    c.set(grow, "by", Param::Number(-2.0));
    simple(grow, "by")
}

fn speed_up(g: &mut Graph) -> Vec<(u32, String)> {
    let mut c = Chain::new(g);
    c.then("when.hit");
    let speed = c.then("do.speed");
    simple(speed, "factor")
}

fn split(g: &mut Graph) -> Vec<(u32, String)> {
    let mut c = Chain::new(g);
    c.then("when.hit");
    let only = c.then("if.compare");
    c.set(only, "op", Param::Choice("less".into()));
    c.set(only, "b", Param::Number(64.0));
    c.feed("count", only, 1);
    let split = c.then("do.split");
    vec![(split, "pieces".into()), (only, "b".into())]
}

fn colour(g: &mut Graph) -> Vec<(u32, String)> {
    let mut c = Chain::new(g);
    c.then("when.hit");
    let tint = c.then("do.color");
    let hsv = c.feed("color.hsv", tint, 2);
    let random = c.feed("random", hsv, 0);
    c.set(random, "max", Param::Number(1.0));
    simple(tint, "amount")
}

fn spawn(g: &mut Graph) -> Vec<(u32, String)> {
    let mut c = Chain::new(g);
    let timer = c.then("when.timer");
    let spawn = c.then("do.spawn");
    vec![(timer, "every".into()), (spawn, "size".into())]
}

fn remove_fast(g: &mut Graph) -> Vec<(u32, String)> {
    let mut c = Chain::new(g);
    let hit = c.then("when.hit");
    let only = c.then("if.compare");
    c.set(only, "op", Param::Choice("more".into()));
    c.set(only, "b", Param::Number(2000.0));
    c.g.connect(Link { from: (hit, 3), to: (only, 1) }, catalog(Role::Rules));
    c.then("do.remove");
    simple(only, "b")
}

fn note(g: &mut Graph) -> Vec<(u32, String)> {
    let mut c = Chain::new(g);
    c.then("when.hit");
    let note = c.then("do.note");
    simple(note, "loudness")
}

fn count(g: &mut Graph) -> Vec<(u32, String)> {
    let mut c = Chain::new(g);
    c.then("when.hit");
    let count = c.then("do.counter");
    c.set(count, "id", Param::Number(1.0));
    simple(count, "add")
}

fn pulse_gravity(g: &mut Graph) -> Vec<(u32, String)> {
    let mut c = Chain::new(g);
    let strong = c.then("when.timer");
    c.set(strong, "every", Param::Number(2.0));
    c.set(strong, "first", Param::Number(1.0));
    let heavy = c.then("do.gravity");
    c.set(heavy, "gravity", Param::Vector([0.0, 2000.0]));
    let mut c = Chain::new(g);
    let back = c.then("when.timer");
    c.set(back, "every", Param::Number(2.0));
    c.set(back, "first", Param::Number(2.0));
    let normal = c.then("do.gravity");
    c.set(normal, "gravity", Param::Vector([0.0, 900.0]));
    vec![(heavy, "gravity".into()), (strong, "every".into())]
}

/// Every ready-made rule, in menu order.
pub fn recipes() -> Vec<Recipe> {
    vec![
        Recipe { name: "Grow on hit", help: "Every hit makes the ball a little bigger.", add: grow },
        Recipe { name: "Shrink on hit", help: "Every hit makes the ball a little smaller.", add: shrink },
        Recipe { name: "Speed up on hit", help: "Every hit makes the ball a little faster.", add: speed_up },
        Recipe { name: "Split on hit", help: "Every hit splits the ball in two, until there are 64.", add: split },
        Recipe { name: "Colour on hit", help: "Every hit gives the ball a new random colour.", add: colour },
        Recipe { name: "Spawn every second", help: "A new ball appears at the thing every second.", add: spawn },
        Recipe {
            name: "Remove when fast",
            help: "A ball that hits faster than 2000 px/s is removed.",
            add: remove_fast,
        },
        Recipe { name: "Play a note on hit", help: "Every hit plays the thing's sound.", add: note },
        Recipe { name: "Count hits", help: "Counter 1 goes up by one on every hit.", add: count },
        Recipe { name: "Pulse gravity", help: "Gravity doubles every other second.", add: pulse_gravity },
    ]
}
