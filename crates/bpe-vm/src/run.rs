//! Running a compiled program: values pulled through wires, triggers pushed along them.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};

use bpe_core::{SlotId, ThingId};
use bpe_graph::{Param, PinType};
use glam::Vec2;

use crate::target::target_from_text;
use crate::{Ball, Host, Program, Val};

/// How many wires a value may be pulled through before it reads as nothing.
pub const MAX_DEPTH: usize = 64;
/// The most actions one program may queue in one run.
pub const ACTION_BUDGET: usize = 256;

/// What happened, as a rules graph sees it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Trigger {
    Hit,
    Spawn,
    Remove,
    Enter,
    Leave,
    Beat,
    Note,
    Custom(String),
}

/// One event, as a rules graph sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Happening {
    pub kind: Trigger,
    pub a: Ball,
    pub b: Option<Ball>,
    pub point: Vec2,
    pub normal: Vec2,
    pub speed: f32,
}

/// What a rule asks the world to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Grow {
        ball: Ball,
        by: f32,
    },
    Speed {
        ball: Ball,
        factor: f32,
    },
    Kick {
        ball: Ball,
        velocity: Vec2,
    },
    /// Alpha is how strongly the tint applies.
    Tint {
        ball: Ball,
        color: [f32; 4],
    },
    Remove {
        ball: Ball,
    },
    Split {
        ball: Ball,
        pieces: u32,
    },
    Spawn {
        thing: ThingId,
        position: Vec2,
        velocity: Vec2,
        radius: f32,
        color: [f32; 4],
    },
    Gravity(Vec2),
    Set {
        thing: ThingId,
        slot: SlotId,
        key: String,
        value: f32,
    },
    Fire {
        name: String,
        ball: Ball,
    },
    Note {
        ball: Ball,
        loudness: f32,
    },
}

/// What an output block collected: one entry per input pin, `None` where nothing is wired.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outputs(pub Vec<Option<Val>>);

impl Outputs {
    fn get(&self, pin: usize) -> Option<&Val> {
        self.0.get(pin).and_then(|v| v.as_ref())
    }

    pub fn number(&self, pin: usize, default: f32) -> f32 {
        self.get(pin).map_or(default, Val::number)
    }

    pub fn vector(&self, pin: usize, default: Vec2) -> Vec2 {
        self.get(pin).map_or(default, Val::vector)
    }

    pub fn boolean(&self, pin: usize, default: bool) -> bool {
        self.get(pin).map_or(default, Val::boolean)
    }

    pub fn color(&self, pin: usize, default: [f32; 4]) -> [f32; 4] {
        self.get(pin).map_or(default, Val::color)
    }

    /// Whether anything is wired into the pin.
    pub fn wired(&self, pin: usize) -> bool {
        self.get(pin).is_some()
    }
}

/// What a When block hands the chain it fires.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Firing {
    pub ball: Option<Ball>,
    pub other: Option<Ball>,
    pub speed: f32,
    pub point: Vec2,
}

/// Where random numbers come from: a hash of the seed and salt (so a stateless evaluation
/// repeats), or a stream that advances with every read (for rules).
enum Rng {
    Salted(u64),
    Stream(Cell<u64>),
}

fn splitmix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

fn unit(bits: u64) -> f32 {
    (bits >> 40) as f32 / (1u64 << 24) as f32
}

impl Rng {
    /// A number in 0..1.
    fn next(&self, node: u32) -> f32 {
        match self {
            Rng::Salted(seed) => unit(splitmix(seed ^ (node as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15))),
            Rng::Stream(cell) => {
                let mut x = cell.get();
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                cell.set(x);
                unit(x.wrapping_mul(0x2545_f491_4f6c_dd1d))
            }
        }
    }
}

/// Everything one evaluation reads.
struct Cx<'a> {
    program: &'a Program,
    host: &'a dyn Host,
    time: f32,
    inputs: &'a [Val],
    firing: Firing,
    counters: &'a RefCell<BTreeMap<u32, f32>>,
    rng: Rng,
}

/// The state a rules graph keeps while the clip runs.
#[derive(Clone, Debug, PartialEq)]
pub struct Vm {
    seed: u64,
    rng: u64,
    started: bool,
    /// Blocks that have fired once (for Only once) and when every block last fired.
    once: BTreeSet<u32>,
    last: BTreeMap<u32, f32>,
    /// How many times each Every nth block was reached, and when each cooldown last let a
    /// chain through.
    reached: BTreeMap<u32, u32>,
    passed: BTreeMap<u32, f32>,
    /// Per timer, how many times it has fired.
    ticks: BTreeMap<u32, u32>,
    /// Per counter trigger, whether its counter is at or above the mark.
    above: BTreeSet<u32>,
    counters: BTreeMap<u32, f32>,
}

impl Vm {
    pub fn new(seed: u64) -> Vm {
        Vm {
            seed,
            rng: splitmix(seed) | 1,
            started: false,
            once: BTreeSet::new(),
            last: BTreeMap::new(),
            reached: BTreeMap::new(),
            passed: BTreeMap::new(),
            ticks: BTreeMap::new(),
            above: BTreeSet::new(),
            counters: BTreeMap::new(),
        }
    }

    /// Evaluates a Bounce, Motion or Sound program once: `inputs` are the role's input pins in
    /// order, `salt` makes random blocks differ between evaluations. Stateless, so a module can
    /// evaluate from a shared, read-only state.
    pub fn eval(program: &Program, host: &dyn Host, time: f32, inputs: &[Val], salt: u64) -> Outputs {
        let Some(out) = program.output else { return Outputs::default() };
        let counters = RefCell::new(BTreeMap::new());
        let cx = Cx {
            program,
            host,
            time,
            inputs,
            firing: Firing::default(),
            counters: &counters,
            rng: Rng::Salted(splitmix(host.seed() ^ salt)),
        };
        let block = &program.blocks[out];
        Outputs(
            (0..block.inputs.len())
                .map(|pin| block.inputs[pin].map(|(from, from_pin)| cx.value(from, from_pin, 0).finite()))
                .collect(),
        )
    }

    /// What a counter has counted up to.
    pub fn counter(&self, id: u32) -> f32 {
        self.counters.get(&id).copied().unwrap_or(0.0)
    }

    /// When a block last fired, for the editor's live view.
    pub fn fired_at(&self, node: u32) -> Option<f32> {
        self.last.get(&node).copied()
    }

    /// Runs a Rules program for one step: the start (once), every happening, then the timers
    /// and counters. Actions go to `acts`, at most [`ACTION_BUDGET`] per run.
    pub fn run(
        &mut self,
        program: &Program,
        host: &dyn Host,
        time: f32,
        happenings: &[Happening],
        acts: &mut Vec<Act>,
    ) {
        if program.triggers.is_empty() {
            return;
        }
        let counters = RefCell::new(std::mem::take(&mut self.counters));
        let mut cx = Cx {
            program,
            host,
            time,
            inputs: &[],
            firing: Firing::default(),
            counters: &counters,
            rng: Rng::Stream(Cell::new(self.rng)),
        };
        let budget = acts.len() + ACTION_BUDGET;
        let mut walk = Walk { acts, budget };
        if !self.started {
            self.started = true;
            for &t in &program.triggers {
                if program.blocks[t].kind == "when.start" {
                    cx.firing = Firing::default();
                    self.fire(&cx, t, &mut walk);
                }
            }
        }
        for h in happenings {
            for &t in &program.triggers {
                let b = &program.blocks[t];
                let who = || host.matches(h.a, &target_from_text(b.text("who")));
                let what = || match h.b {
                    Some(other) => host.matches(other, &target_from_text(b.text("what"))),
                    None => matches!(target_from_text(b.text("what")), bpe_core::Target::Anything),
                };
                let fires = match (b.kind.as_str(), &h.kind) {
                    ("when.hit", Trigger::Hit) => who() && what(),
                    ("when.spawn", Trigger::Spawn) => who(),
                    ("when.remove", Trigger::Remove) => who(),
                    ("when.enter", Trigger::Enter) => who() && what(),
                    ("when.leave", Trigger::Leave) => who() && what(),
                    ("when.beat", Trigger::Beat) => true,
                    ("when.event", Trigger::Custom(name)) => name == b.text("name") && who(),
                    _ => false,
                };
                if fires {
                    cx.firing = Firing { ball: Some(h.a), other: h.b, speed: h.speed, point: h.point };
                    self.fire(&cx, t, &mut walk);
                }
            }
        }
        cx.firing = Firing::default();
        for &t in &program.triggers {
            let b = &program.blocks[t];
            match b.kind.as_str() {
                "when.timer" => {
                    let every = b.num("every").max(0.01);
                    let first = b.num("first").max(0.0);
                    let ticks = self.ticks.get(&b.id).copied().unwrap_or(0);
                    if time >= first + ticks as f32 * every {
                        // Several due ticks fire once, then wait for the next one.
                        let due = ((time - first) / every).floor() as u32 + 1;
                        self.ticks.insert(b.id, due.max(ticks + 1));
                        self.fire(&cx, t, &mut walk);
                    }
                }
                "when.at" => {
                    if time >= b.num("time") && self.once.insert(b.id) {
                        self.fire(&cx, t, &mut walk);
                    }
                }
                "when.counter" => {
                    let value = counters.borrow().get(&(b.num("id") as u32)).copied().unwrap_or(0.0);
                    if value >= b.num("reaches") {
                        if self.above.insert(b.id) {
                            self.fire(&cx, t, &mut walk);
                        }
                    } else {
                        self.above.remove(&b.id);
                    }
                }
                _ => {}
            }
        }
        if let Rng::Stream(cell) = &cx.rng {
            self.rng = cell.get();
        }
        self.counters = counters.into_inner();
    }

    /// A When block fires: everything on its `then` pin runs.
    fn fire(&mut self, cx: &Cx, trigger: usize, walk: &mut Walk) {
        self.last.insert(cx.program.blocks[trigger].id, cx.time);
        self.follow(cx, trigger, 0, walk, 0);
    }

    /// Runs every block wired to an output pin.
    fn follow(&mut self, cx: &Cx, block: usize, pin: usize, walk: &mut Walk, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        let next: Vec<usize> = cx.program.blocks[block].then.get(pin).cloned().unwrap_or_default();
        for b in next {
            self.reach(cx, b, walk, depth + 1);
        }
    }

    /// A block is reached by a trigger: a condition passes or fails, an action is queued.
    fn reach(&mut self, cx: &Cx, block: usize, walk: &mut Walk, depth: usize) {
        if walk.acts.len() >= walk.budget {
            return;
        }
        let b = &cx.program.blocks[block];
        self.last.insert(b.id, cx.time);
        let kind = b.kind.as_str();
        if let Some(cond) = kind.strip_prefix("if.") {
            let pass = match cond {
                "chance" => cx.rng.next(b.id) * 100.0 < cx.num(block, 1, 0),
                "every" => {
                    let n = (b.num("n") as u32).max(1);
                    let count = self.reached.entry(b.id).or_insert(0);
                    *count += 1;
                    (*count - 1).is_multiple_of(n)
                }
                "once" => self.once.insert(b.id),
                "cooldown" => {
                    let seconds = cx.num(block, 1, 0);
                    let ok = self.passed.get(&b.id).is_none_or(|last| cx.time - last >= seconds);
                    if ok {
                        self.passed.insert(b.id, cx.time);
                    }
                    ok
                }
                "compare" => compare(b.text("op"), cx.num(block, 1, 0), cx.num(block, 2, 0)),
                "is" => cx
                    .input(block, 1, 0)
                    .ball()
                    .is_some_and(|ball| cx.host.matches(ball, &target_from_text(b.text("target")))),
                _ => false,
            };
            self.follow(cx, block, if pass { 0 } else { 1 }, walk, depth);
            return;
        }
        if let Some(action) = kind.strip_prefix("do.") {
            let ball = || cx.input(block, 1, 0).ball();
            match action {
                "grow" => {
                    if let Some(ball) = ball() {
                        walk.push(Act::Grow { ball, by: cx.num(block, 2, 0) });
                    }
                }
                "speed" => {
                    if let Some(ball) = ball() {
                        walk.push(Act::Speed { ball, factor: cx.num(block, 2, 0) });
                    }
                }
                "kick" => {
                    if let Some(ball) = ball() {
                        walk.push(Act::Kick { ball, velocity: cx.vec(block, 2, 0) });
                    }
                }
                "color" => {
                    if let Some(ball) = ball() {
                        let mut color = cx.input(block, 2, 0).color();
                        color[3] = cx.num(block, 3, 0).clamp(0.0, 1.0);
                        walk.push(Act::Tint { ball, color });
                    }
                }
                "remove" => {
                    if let Some(ball) = ball() {
                        walk.push(Act::Remove { ball });
                    }
                }
                "split" => {
                    if let Some(ball) = ball() {
                        walk.push(Act::Split { ball, pieces: (cx.num(block, 2, 0).round() as u32).clamp(2, 8) });
                    }
                }
                "spawn" => {
                    if let Some(&thing) = cx.host.things_of(&target_from_text(b.text("into"))).first() {
                        walk.push(Act::Spawn {
                            thing,
                            position: cx.vec(block, 1, 0),
                            velocity: cx.vec(block, 2, 0),
                            radius: cx.num(block, 3, 0),
                            color: cx.input(block, 4, 0).color(),
                        });
                    }
                }
                "gravity" => walk.push(Act::Gravity(cx.vec(block, 1, 0))),
                "set" => {
                    let slot = if b.text("slot") == "Motion" { SlotId::Motion } else { SlotId::Bounce };
                    let value = cx.num(block, 1, 0);
                    for thing in cx.host.things_of(&target_from_text(b.text("target"))) {
                        walk.push(Act::Set { thing, slot, key: b.text("key").to_owned(), value });
                    }
                }
                "event" => {
                    if let Some(ball) = ball() {
                        walk.push(Act::Fire { name: b.text("name").to_owned(), ball });
                    }
                }
                "note" => {
                    if let Some(ball) = ball() {
                        walk.push(Act::Note { ball, loudness: cx.num(block, 2, 0) });
                    }
                }
                "counter" => {
                    let add = cx.num(block, 1, 0);
                    *cx.counters.borrow_mut().entry(b.num("id") as u32).or_insert(0.0) += add;
                }
                _ => {}
            }
            self.follow(cx, block, 0, walk, depth);
        }
    }
}

/// The actions queued so far in one run.
struct Walk<'a> {
    acts: &'a mut Vec<Act>,
    budget: usize,
}

impl Walk<'_> {
    fn push(&mut self, act: Act) {
        if self.acts.len() < self.budget {
            self.acts.push(act);
        }
    }
}

fn param_val(p: &Param) -> Option<Val> {
    match p {
        Param::Number(x) => Some(Val::Number(*x)),
        Param::Bool(b) => Some(Val::Bool(*b)),
        Param::Vector(v) => Some(Val::Vector(Vec2::from(*v))),
        Param::Color(c) => Some(Val::Color(*c)),
        Param::Text(_) | Param::Choice(_) => None,
    }
}

fn hsv(h: f32, s: f32, v: f32) -> [f32; 4] {
    let h = (h.rem_euclid(1.0)) * 6.0;
    let i = h.floor();
    let f = h - i;
    let (s, v) = (s.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match i as i32 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [r, g, b, 1.0]
}

fn compare(op: &str, a: f32, b: f32) -> bool {
    match op {
        "less" => a < b,
        "at most" => a <= b,
        "more" => a > b,
        "at least" => a >= b,
        "equal" => a == b,
        "different" => a != b,
        _ => false,
    }
}

impl Cx<'_> {
    /// An input pin's value: what is wired into it, else the setting of the same name.
    fn input(&self, block: usize, pin: usize, depth: usize) -> Val {
        let b = &self.program.blocks[block];
        if let Some(Some((from, from_pin))) = b.inputs.get(pin) {
            return self.value(*from, *from_pin, depth + 1);
        }
        let ty = b.input_types.get(pin).copied().unwrap_or(PinType::Number);
        if ty == PinType::Ball && b.kind.starts_with("do.") || b.kind == "if.is" {
            // The ball a rule is about, unless another is wired in.
            return Val::Ball(self.firing.ball);
        }
        b.input_names.get(pin).and_then(|n| b.params.get(n)).and_then(param_val).unwrap_or(Val::zero(ty))
    }

    fn num(&self, block: usize, pin: usize, depth: usize) -> f32 {
        self.input(block, pin, depth).number()
    }

    fn vec(&self, block: usize, pin: usize, depth: usize) -> Vec2 {
        self.input(block, pin, depth).vector()
    }

    /// A block's output pin.
    fn value(&self, block: usize, pin: u8, depth: usize) -> Val {
        let b = &self.program.blocks[block];
        let ty = b.output_types.get(pin as usize).copied().unwrap_or(PinType::Number);
        if depth > MAX_DEPTH {
            return Val::zero(ty);
        }
        let pin = pin as usize;
        let v = match b.kind.as_str() {
            "number" => Val::Number(b.num("value")),
            "vector" => Val::Vector(b.vec("xy")),
            "color" => Val::Color(b.col("color")),
            "random" => {
                let (min, max) = (self.num(block, 0, depth), self.num(block, 1, depth));
                Val::Number(min + self.rng.next(b.id) * (max - min))
            }
            "time" => Val::Number(self.time),
            "gravity" => Val::Vector(self.host.gravity()),
            "math" => {
                let (a, c) = (self.num(block, 0, depth), self.num(block, 1, depth));
                Val::Number(match b.text("op") {
                    "add" => a + c,
                    "subtract" => a - c,
                    "multiply" => a * c,
                    "divide" => {
                        if c == 0.0 {
                            0.0
                        } else {
                            a / c
                        }
                    }
                    "min" => a.min(c),
                    "max" => a.max(c),
                    "power" => a.powf(c),
                    "modulo" => {
                        if c == 0.0 {
                            0.0
                        } else {
                            a.rem_euclid(c)
                        }
                    }
                    _ => 0.0,
                })
            }
            "fn" => {
                let x = self.num(block, 0, depth);
                Val::Number(match b.text("f") {
                    "sin" => x.sin(),
                    "cos" => x.cos(),
                    "abs" => x.abs(),
                    "floor" => x.floor(),
                    "round" => x.round(),
                    "sqrt" => x.max(0.0).sqrt(),
                    "sign" => x.signum() * (x != 0.0) as u8 as f32,
                    "negate" => -x,
                    _ => 0.0,
                })
            }
            "clamp" => {
                let (x, min, max) = (self.num(block, 0, depth), self.num(block, 1, depth), self.num(block, 2, depth));
                Val::Number(if min <= max { x.clamp(min, max) } else { x })
            }
            "compare" => Val::Bool(compare(b.text("op"), self.num(block, 0, depth), self.num(block, 1, depth))),
            "logic" => {
                let a = self.input(block, 0, depth).boolean();
                let c = self.input(block, 1, depth).boolean();
                Val::Bool(match b.text("op") {
                    "and" => a && c,
                    "or" => a || c,
                    "not" => !a,
                    "xor" => a != c,
                    _ => false,
                })
            }
            "choose" | "choose.vector" | "choose.color" => {
                let which = if self.input(block, 0, depth).boolean() { 1 } else { 2 };
                self.input(block, which, depth)
            }
            "vec.math" => {
                let (a, c) = (self.vec(block, 0, depth), self.vec(block, 1, depth));
                Val::Vector(match b.text("op") {
                    "subtract" => a - c,
                    _ => a + c,
                })
            }
            "vec.scale" => Val::Vector(self.vec(block, 0, depth) * self.num(block, 1, depth)),
            "vec.make" => Val::Vector(Vec2::new(self.num(block, 0, depth), self.num(block, 1, depth))),
            "vec.split" => {
                let v = self.vec(block, 0, depth);
                Val::Number(if pin == 0 { v.x } else { v.y })
            }
            "vec.length" => Val::Number(self.vec(block, 0, depth).length()),
            "vec.normalize" => Val::Vector(self.vec(block, 0, depth).normalize_or_zero()),
            "vec.rotate" => {
                let v = self.vec(block, 0, depth);
                let a = self.num(block, 1, depth).to_radians();
                Val::Vector(Vec2::new(v.x * a.cos() - v.y * a.sin(), v.x * a.sin() + v.y * a.cos()))
            }
            "vec.limit" => {
                let v = self.vec(block, 0, depth);
                let max = self.num(block, 1, depth).max(0.0);
                let len = v.length();
                Val::Vector(if len > max && len > 0.0 { v * (max / len) } else { v })
            }
            "reflect" => {
                let (v, n) = (self.vec(block, 0, depth), self.vec(block, 1, depth));
                let keep = self.num(block, 2, depth);
                // The same sums as the runtime's `reflect`, so the bounce template matches it.
                let surface = self.vec(block, 3, depth);
                let relative = v - surface;
                let into = (-relative.dot(n)).max(0.0);
                Val::Vector(relative + n * into * (1.0 + keep) + surface)
            }
            "color.mix" => {
                let (a, c) = (self.input(block, 0, depth).color(), self.input(block, 1, depth).color());
                let t = self.num(block, 2, depth).clamp(0.0, 1.0);
                Val::Color(std::array::from_fn(|i| a[i] + (c[i] - a[i]) * t))
            }
            "color.hsv" => {
                Val::Color(hsv(self.num(block, 0, depth), self.num(block, 1, depth), self.num(block, 2, depth)))
            }
            "setting" => Val::Number(b.num("value")),
            "setting.color" => Val::Color(b.col("color")),
            "setting.bool" => Val::Bool(b.on("on")),
            "ball" => {
                let info = self.input(block, 0, depth).ball().and_then(|ball| self.host.ball(ball));
                match (pin, info) {
                    (0, Some(i)) => Val::Vector(i.position),
                    (1, Some(i)) => Val::Vector(i.velocity),
                    (2, Some(i)) => Val::Number(i.velocity.length()),
                    (3, Some(i)) => Val::Number(i.radius),
                    (4, Some(i)) => Val::Number(i.hits as f32),
                    (5, _) => Val::Bool(info.is_some()),
                    _ => Val::zero(ty),
                }
            }
            "ball.is" => Val::Bool(
                self.input(block, 0, depth)
                    .ball()
                    .is_some_and(|ball| self.host.matches(ball, &target_from_text(b.text("target")))),
            ),
            "ball.pick" => {
                let balls = self.host.balls(&target_from_text(b.text("target")));
                let pick = (self.rng.next(b.id) * balls.len() as f32) as usize;
                Val::Ball(balls.get(pick.min(balls.len().saturating_sub(1))).copied())
            }
            "count" => Val::Number(self.host.balls(&target_from_text(b.text("target"))).len() as f32),
            "counter" => Val::Number(self.counters.borrow().get(&(b.num("id") as u32)).copied().unwrap_or(0.0)),
            "sound.chord" => {
                let note = self.host.chord_note(self.time, self.num(block, 0, depth).max(0.0) as u64);
                match pin {
                    0 => Val::Number(note.unwrap_or(0.0)),
                    _ => Val::Bool(note.is_some()),
                }
            }
            k if k.starts_with("when.") => match pin {
                1 => Val::Ball(self.firing.ball),
                2 => Val::Ball(self.firing.other),
                3 => Val::Number(self.firing.speed),
                4 => Val::Vector(self.firing.point),
                _ => Val::zero(ty),
            },
            k if k.ends_with(".in") => self.inputs.get(pin).copied().unwrap_or(Val::zero(ty)),
            _ => Val::zero(ty),
        };
        v.finite()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NoHost, Role, catalog};
    use bpe_core::ThingId;
    use bpe_graph::{Catalog, Graph, Link, Param};
    use glam::Vec2;

    /// A bounce graph: `build` gets the graph and the ids of the in and out blocks.
    fn bounce_graph(build: impl FnOnce(&mut Graph, u32, u32)) -> Program {
        let c = catalog(Role::Bounce);
        let mut g = Graph::new("b");
        let i = g.add(c.spec("bounce.in").unwrap(), [0.0, 0.0]);
        let o = g.add(c.spec("bounce.out").unwrap(), [400.0, 0.0]);
        build(&mut g, i, o);
        crate::compile(&g, Role::Bounce).unwrap()
    }

    fn set(g: &mut Graph, node: u32, name: &str, p: Param) {
        g.node_mut(node).unwrap().params.insert(name.into(), p);
    }

    fn wire(g: &mut Graph, from: (u32, u8), to: (u32, u8)) {
        assert!(g.connect(Link { from, to }, catalog(Role::Bounce)), "wire {from:?} -> {to:?}");
    }

    fn inputs() -> [Val; 8] {
        [
            Val::Ball(Some(Ball { thing: ThingId(2), item: 0 })),
            Val::Vector(Vec2::new(10.0, -4.0)),
            Val::Vector(Vec2::Y),
            Val::Number(5.0),
            Val::Number(24.0),
            Val::Number(0.0),
            Val::Bool(true),
            Val::Number(0.0),
        ]
    }

    #[test]
    fn a_wired_output_is_evaluated_and_an_unwired_one_defaults() {
        let p = bounce_graph(|g, i, o| {
            let c = catalog(Role::Bounce);
            let s = g.add(c.spec("vec.scale").unwrap(), [0.0, 0.0]);
            let n = g.add(c.spec("number").unwrap(), [0.0, 0.0]);
            set(g, n, "value", Param::Number(0.5));
            wire(g, (i, 1), (s, 0));
            wire(g, (n, 0), (s, 1));
            wire(g, (s, 0), (o, 0));
        });
        let out = Vm::eval(&p, &NoHost, 0.0, &inputs(), 0);
        assert_eq!(out.vector(0, Vec2::ZERO), Vec2::new(5.0, -2.0));
        assert_eq!(out.number(1, 24.0), 24.0);
        assert_eq!(out.0[1], None);
        assert!(!out.boolean(2, false));
    }

    #[test]
    fn unwired_inputs_fall_back_to_the_block_settings() {
        let p = bounce_graph(|g, _, o| {
            let c = catalog(Role::Bounce);
            let m = g.add(c.spec("math").unwrap(), [0.0, 0.0]);
            set(g, m, "op", Param::Choice("multiply".into()));
            set(g, m, "a", Param::Number(6.0));
            set(g, m, "b", Param::Number(7.0));
            wire(g, (m, 0), (o, 1));
        });
        assert_eq!(Vm::eval(&p, &NoHost, 0.0, &inputs(), 0).number(1, 0.0), 42.0);
    }

    #[test]
    fn bad_maths_gives_zero_not_nan() {
        let p = bounce_graph(|g, _, o| {
            let c = catalog(Role::Bounce);
            let d = g.add(c.spec("math").unwrap(), [0.0, 0.0]);
            set(g, d, "op", Param::Choice("divide".into()));
            set(g, d, "a", Param::Number(1.0));
            set(g, d, "b", Param::Number(0.0));
            let s = g.add(c.spec("fn").unwrap(), [0.0, 0.0]);
            set(g, s, "f", Param::Choice("sqrt".into()));
            set(g, s, "x", Param::Number(-4.0));
            let add = g.add(c.spec("math").unwrap(), [0.0, 0.0]);
            wire(g, (d, 0), (add, 0));
            wire(g, (s, 0), (add, 1));
            wire(g, (add, 0), (o, 1));
            let n = g.add(c.spec("vec.normalize").unwrap(), [0.0, 0.0]);
            wire(g, (n, 0), (o, 0));
        });
        let out = Vm::eval(&p, &NoHost, 0.0, &inputs(), 0);
        assert_eq!(out.number(1, 9.0), 0.0);
        assert_eq!(out.vector(0, Vec2::X), Vec2::ZERO);
    }

    #[test]
    fn random_is_the_same_for_the_same_seed_and_salt_and_differs_otherwise() {
        let p = bounce_graph(|g, _, o| {
            let c = catalog(Role::Bounce);
            let r = g.add(c.spec("random").unwrap(), [0.0, 0.0]);
            set(g, r, "min", Param::Number(10.0));
            set(g, r, "max", Param::Number(20.0));
            wire(g, (r, 0), (o, 1));
        });
        let a = Vm::eval(&p, &NoHost, 0.0, &inputs(), 3).number(1, 0.0);
        let b = Vm::eval(&p, &NoHost, 0.0, &inputs(), 3).number(1, 0.0);
        let c = Vm::eval(&p, &NoHost, 0.0, &inputs(), 4).number(1, 0.0);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!((10.0..20.0).contains(&a) && (10.0..20.0).contains(&c));
    }

    #[test]
    fn choose_compare_clamp_rotate_and_reflect_work() {
        let p = bounce_graph(|g, i, o| {
            let c = catalog(Role::Bounce);
            let cmp = g.add(c.spec("compare").unwrap(), [0.0, 0.0]);
            set(g, cmp, "op", Param::Choice("more".into()));
            set(g, cmp, "a", Param::Number(3.0));
            set(g, cmp, "b", Param::Number(2.0));
            let ch = g.add(c.spec("choose").unwrap(), [0.0, 0.0]);
            set(g, ch, "then", Param::Number(1.0));
            set(g, ch, "else", Param::Number(2.0));
            wire(g, (cmp, 0), (ch, 0));
            let cl = g.add(c.spec("clamp").unwrap(), [0.0, 0.0]);
            set(g, cl, "x", Param::Number(5.0));
            set(g, cl, "min", Param::Number(0.0));
            set(g, cl, "max", Param::Number(1.0));
            let sum = g.add(c.spec("math").unwrap(), [0.0, 0.0]);
            wire(g, (ch, 0), (sum, 0));
            wire(g, (cl, 0), (sum, 1));
            wire(g, (sum, 0), (o, 1));
            // reflect the hit's velocity off the normal keeping half, then rotate by 90.
            let r = g.add(c.spec("reflect").unwrap(), [0.0, 0.0]);
            set(g, r, "keep", Param::Number(0.5));
            wire(g, (i, 1), (r, 0));
            wire(g, (i, 2), (r, 1));
            let rot = g.add(c.spec("vec.rotate").unwrap(), [0.0, 0.0]);
            set(g, rot, "degrees", Param::Number(90.0));
            wire(g, (r, 0), (rot, 0));
            wire(g, (rot, 0), (o, 0));
        });
        let mut inputs = inputs();
        inputs[1] = Val::Vector(Vec2::new(0.0, 10.0));
        inputs[2] = Val::Vector(Vec2::new(0.0, -1.0));
        let out = Vm::eval(&p, &NoHost, 0.0, &inputs, 0);
        assert_eq!(out.number(1, 0.0), 2.0);
        // reflect gives (0, -5); rotated 90 degrees anticlockwise in scene space (+y down) → (5, 0)
        let v = out.vector(0, Vec2::ZERO);
        assert!((v - Vec2::new(5.0, 0.0)).length() < 1e-4, "{v:?}");
    }

    struct Chord;
    impl Host for Chord {
        fn chord_note(&self, _time: f32, index: u64) -> Option<f32> {
            (index == 0).then_some(60.0)
        }
    }

    #[test]
    fn a_chord_note_comes_from_the_host() {
        let c = catalog(Role::Sound);
        let mut g = Graph::new("s");
        let i = g.add(c.spec("sound.in").unwrap(), [0.0, 0.0]);
        let o = g.add(c.spec("sound.out").unwrap(), [0.0, 0.0]);
        let ch = g.add(c.spec("sound.chord").unwrap(), [0.0, 0.0]);
        g.connect(Link { from: (i, 2), to: (ch, 0) }, c);
        g.connect(Link { from: (ch, 0), to: (o, 1) }, c);
        g.connect(Link { from: (ch, 1), to: (o, 0) }, c);
        let p = crate::compile(&g, Role::Sound).unwrap();
        let mut inputs = vec![
            Val::Ball(None),
            Val::Number(100.0),
            Val::Number(0.0),
            Val::Number(0.0),
            Val::Number(0.0),
            Val::Number(0.0),
            Val::Number(0.0),
        ];
        let out = Vm::eval(&p, &Chord, 0.0, &inputs, 0);
        assert_eq!(out.number(1, 0.0), 60.0);
        assert!(out.boolean(0, false));
        inputs[2] = Val::Number(1.0);
        assert!(!Vm::eval(&p, &Chord, 0.0, &inputs, 0).boolean(0, true));
        assert!(!Vm::eval(&p, &NoHost, 0.0, &inputs, 0).boolean(0, true));
    }

    #[test]
    fn evaluation_stops_at_the_depth_limit() {
        let p = bounce_graph(|g, _, o| {
            let c = catalog(Role::Bounce);
            let mut prev = g.add(c.spec("number").unwrap(), [0.0, 0.0]);
            set(g, prev, "value", Param::Number(1.0));
            for _ in 0..70 {
                let m = g.add(c.spec("math").unwrap(), [0.0, 0.0]);
                set(g, m, "b", Param::Number(1.0));
                wire(g, (prev, 0), (m, 0));
                prev = m;
            }
            wire(g, (prev, 0), (o, 1));
        });
        let out = Vm::eval(&p, &NoHost, 0.0, &inputs(), 0);
        assert!(out.number(1, -1.0) < 71.0);
    }
}

#[cfg(test)]
mod rules_tests {
    use super::*;
    use crate::{Role, catalog};
    use bpe_core::{Target, ThingId};
    use bpe_graph::{Catalog, Graph, Link, Param};
    use glam::Vec2;

    const B21: Ball = Ball { thing: ThingId(2), item: 1 };

    /// Thing 2 is me and has two balls.
    struct TwoBalls;
    impl Host for TwoBalls {
        fn ball(&self, ball: Ball) -> Option<crate::BallInfo> {
            (ball.thing == ThingId(2) && ball.item < 2).then_some(crate::BallInfo {
                position: Vec2::ZERO,
                velocity: Vec2::new(100.0, 0.0),
                radius: 24.0,
                hits: 1,
            })
        }
        fn matches(&self, ball: Ball, target: &Target) -> bool {
            match target {
                Target::Me => ball.thing == ThingId(2),
                Target::Thing(id) => ball.thing == *id,
                Target::Anything => true,
                _ => false,
            }
        }
        fn balls(&self, target: &Target) -> Vec<Ball> {
            match target {
                Target::Me | Target::Anything => vec![Ball { thing: ThingId(2), item: 0 }, B21],
                _ => vec![],
            }
        }
        fn things_of(&self, target: &Target) -> Vec<ThingId> {
            match target {
                Target::Me | Target::Anything => vec![ThingId(2)],
                _ => vec![],
            }
        }
    }

    fn hit(a: Ball) -> Happening {
        Happening {
            kind: Trigger::Hit,
            a,
            b: Some(Ball { thing: ThingId(1), item: 0 }),
            point: Vec2::new(1.0, 2.0),
            normal: Vec2::Y,
            speed: 300.0,
        }
    }

    /// A rules graph: `build` returns the chain's blocks in order and the program wires each to the next.
    fn chain(kinds: &[&str], set: impl Fn(&mut Graph, &[u32])) -> (Program, Vec<u32>) {
        let c = catalog(Role::Rules);
        let mut g = Graph::new("r");
        let ids: Vec<u32> = kinds.iter().map(|k| g.add(c.spec(k).unwrap(), [0.0, 0.0])).collect();
        for w in ids.windows(2) {
            assert!(g.connect(Link { from: (w[0], 0), to: (w[1], 0) }, c));
        }
        set(&mut g, &ids);
        (crate::compile(&g, Role::Rules).unwrap(), ids)
    }

    fn param(g: &mut Graph, node: u32, name: &str, p: Param) {
        g.node_mut(node).unwrap().params.insert(name.into(), p);
    }

    #[test]
    fn a_hit_grows_the_ball_and_plays_a_note() {
        let (p, ids) = chain(&["when.hit", "do.grow", "do.note"], |g, ids| param(g, ids[1], "by", Param::Number(3.0)));
        let mut vm = Vm::new(1);
        let mut acts = Vec::new();
        vm.run(&p, &TwoBalls, 0.5, &[hit(B21)], &mut acts);
        assert_eq!(acts, vec![Act::Grow { ball: B21, by: 3.0 }, Act::Note { ball: B21, loudness: 600.0 }]);
        assert_eq!(vm.fired_at(ids[0]), Some(0.5));
        assert_eq!(vm.fired_at(ids[2]), Some(0.5));
    }

    #[test]
    fn hits_by_other_things_do_not_fire_a_me_rule() {
        let (p, _) = chain(&["when.hit", "do.grow"], |_, _| {});
        let mut acts = Vec::new();
        Vm::new(1).run(&p, &TwoBalls, 0.0, &[hit(Ball { thing: ThingId(3), item: 0 })], &mut acts);
        assert!(acts.is_empty());
        // "what" narrows the other side.
        let (p, _) = chain(&["when.hit", "do.grow"], |g, ids| param(g, ids[0], "what", Param::Text("thing:9".into())));
        Vm::new(1).run(&p, &TwoBalls, 0.0, &[hit(B21)], &mut acts);
        assert!(acts.is_empty());
    }

    #[test]
    fn timers_fire_on_schedule_and_skip_ahead() {
        let c = catalog(Role::Rules);
        let mut g = Graph::new("r");
        let timer = g.add(c.spec("when.timer").unwrap(), [0.0, 0.0]);
        let count0 = g.add(c.spec("do.counter").unwrap(), [0.0, 0.0]);
        let start = g.add(c.spec("when.start").unwrap(), [0.0, 0.0]);
        let count1 = g.add(c.spec("do.counter").unwrap(), [0.0, 0.0]);
        param(&mut g, count1, "id", Param::Number(1.0));
        g.connect(Link { from: (timer, 0), to: (count0, 0) }, c);
        g.connect(Link { from: (start, 0), to: (count1, 0) }, c);
        let p = crate::compile(&g, Role::Rules).unwrap();
        let mut vm = Vm::new(1);
        let mut acts = Vec::new();
        vm.run(&p, &TwoBalls, 0.5, &[], &mut acts);
        assert_eq!((vm.counter(0), vm.counter(1)), (0.0, 1.0));
        vm.run(&p, &TwoBalls, 1.0, &[], &mut acts);
        assert_eq!((vm.counter(0), vm.counter(1)), (1.0, 1.0));
        vm.run(&p, &TwoBalls, 3.7, &[], &mut acts);
        assert_eq!(vm.counter(0), 2.0, "several due ticks fire once");
        vm.run(&p, &TwoBalls, 4.0, &[], &mut acts);
        assert_eq!(vm.counter(0), 3.0);
        assert!(acts.is_empty());
    }

    #[test]
    fn chance_once_every_and_cooldown_gate_a_chain() {
        let grows = |acts: &[Act]| acts.iter().filter(|a| matches!(a, Act::Grow { .. })).count();
        let (p, _) = chain(&["when.hit", "if.once", "do.grow"], |_, _| {});
        let mut acts = Vec::new();
        Vm::new(1).run(&p, &TwoBalls, 0.0, &[hit(B21), hit(B21)], &mut acts);
        assert_eq!(grows(&acts), 1);

        let (p, _) = chain(&["when.hit", "if.every", "do.grow"], |_, _| {});
        acts.clear();
        Vm::new(1).run(&p, &TwoBalls, 0.0, &[hit(B21), hit(B21), hit(B21), hit(B21)], &mut acts);
        assert_eq!(grows(&acts), 2);

        let (p, _) = chain(&["when.hit", "if.cooldown", "do.grow"], |_, _| {});
        acts.clear();
        let mut vm = Vm::new(1);
        vm.run(&p, &TwoBalls, 0.0, &[hit(B21)], &mut acts);
        vm.run(&p, &TwoBalls, 0.5, &[hit(B21)], &mut acts);
        assert_eq!(grows(&acts), 1);
        vm.run(&p, &TwoBalls, 1.5, &[hit(B21)], &mut acts);
        assert_eq!(grows(&acts), 2);

        for (percent, expect) in [(100.0, 10), (0.0, 0)] {
            let (p, _) = chain(&["when.hit", "if.chance", "do.grow"], |g, ids| {
                param(g, ids[1], "percent", Param::Number(percent))
            });
            acts.clear();
            Vm::new(1).run(&p, &TwoBalls, 0.0, &vec![hit(B21); 10], &mut acts);
            assert_eq!(grows(&acts), expect);
        }
        // The else pin fires when the condition fails.
        let c = catalog(Role::Rules);
        let mut g = Graph::new("r");
        let h = g.add(c.spec("when.hit").unwrap(), [0.0, 0.0]);
        let once = g.add(c.spec("if.once").unwrap(), [0.0, 0.0]);
        let shrink = g.add(c.spec("do.grow").unwrap(), [0.0, 0.0]);
        param(&mut g, shrink, "by", Param::Number(-1.0));
        g.connect(Link { from: (h, 0), to: (once, 0) }, c);
        g.connect(Link { from: (once, 1), to: (shrink, 0) }, c);
        let p = crate::compile(&g, Role::Rules).unwrap();
        acts.clear();
        Vm::new(1).run(&p, &TwoBalls, 0.0, &[hit(B21), hit(B21), hit(B21)], &mut acts);
        assert_eq!(acts, vec![Act::Grow { ball: B21, by: -1.0 }; 2]);
    }

    #[test]
    fn counters_add_compare_and_fire_once_on_reaching() {
        let c = catalog(Role::Rules);
        let mut g = Graph::new("r");
        let h = g.add(c.spec("when.hit").unwrap(), [0.0, 0.0]);
        let add = g.add(c.spec("do.counter").unwrap(), [0.0, 0.0]);
        param(&mut g, add, "id", Param::Number(1.0));
        let reach = g.add(c.spec("when.counter").unwrap(), [0.0, 0.0]);
        param(&mut g, reach, "id", Param::Number(1.0));
        param(&mut g, reach, "reaches", Param::Number(3.0));
        let remove = g.add(c.spec("do.remove").unwrap(), [0.0, 0.0]);
        let pick = g.add(c.spec("ball.pick").unwrap(), [0.0, 0.0]);
        g.connect(Link { from: (h, 0), to: (add, 0) }, c);
        g.connect(Link { from: (reach, 0), to: (remove, 0) }, c);
        g.connect(Link { from: (pick, 0), to: (remove, 1) }, c);
        let p = crate::compile(&g, Role::Rules).unwrap();
        let mut vm = Vm::new(1);
        let mut acts = Vec::new();
        for i in 0..5 {
            vm.run(&p, &TwoBalls, i as f32, &[hit(B21)], &mut acts);
        }
        assert_eq!(vm.counter(1), 5.0);
        let removes: Vec<&Act> = acts.iter().filter(|a| matches!(a, Act::Remove { .. })).collect();
        assert_eq!(removes.len(), 1);
        assert_eq!(vm.fired_at(reach), Some(2.0));
    }

    #[test]
    fn a_run_stops_at_the_action_budget() {
        let (p, _) = chain(&["when.hit", "do.grow"], |_, _| {});
        let mut acts = Vec::new();
        Vm::new(1).run(&p, &TwoBalls, 0.0, &vec![hit(B21); 300], &mut acts);
        assert_eq!(acts.len(), ACTION_BUDGET);
    }

    #[test]
    fn spawn_set_and_event_actions_name_their_things() {
        let (p, _) = chain(
            &[
                "when.start",
                "do.spawn",
                "do.set",
                "do.event",
                "do.gravity",
                "do.split",
                "do.color",
                "do.kick",
                "do.speed",
            ],
            |g, ids| {
                param(g, ids[2], "slot", Param::Choice("Motion".into()));
                param(g, ids[2], "key", Param::Text("drag".into()));
                param(g, ids[2], "value", Param::Number(0.5));
                param(g, ids[3], "name", Param::Text("boom".into()));
            },
        );
        let mut acts = Vec::new();
        Vm::new(1).run(&p, &TwoBalls, 0.0, &[], &mut acts);
        assert_eq!(
            acts[0],
            Act::Spawn { thing: ThingId(2), position: Vec2::ZERO, velocity: Vec2::ZERO, radius: 24.0, color: [0.0; 4] }
        );
        assert_eq!(
            acts[1],
            Act::Set { thing: ThingId(2), slot: bpe_core::SlotId::Motion, key: "drag".into(), value: 0.5 }
        );
        // Without a ball the ball actions do nothing, but gravity still applies.
        assert_eq!(acts[2], Act::Gravity(Vec2::new(0.0, 900.0)));
        assert_eq!(acts.len(), 3);
        let (p, _) = chain(&["when.hit", "do.event", "do.split", "do.color", "do.kick", "do.speed"], |g, ids| {
            param(g, ids[1], "name", Param::Text("boom".into()))
        });
        acts.clear();
        Vm::new(1).run(&p, &TwoBalls, 0.0, &[hit(B21)], &mut acts);
        assert_eq!(acts[0], Act::Fire { name: "boom".into(), ball: B21 });
        assert_eq!(acts[1], Act::Split { ball: B21, pieces: 2 });
        assert_eq!(acts[2], Act::Tint { ball: B21, color: [1.0, 0.3, 0.6, 1.0] });
        assert_eq!(acts[3], Act::Kick { ball: B21, velocity: Vec2::new(0.0, -500.0) });
        assert_eq!(acts[4], Act::Speed { ball: B21, factor: 1.1 });
    }

    #[test]
    fn custom_events_wake_the_rules_that_wait_for_them() {
        let (p, _) = chain(&["when.event", "do.grow"], |g, ids| param(g, ids[0], "name", Param::Text("boom".into())));
        let mut acts = Vec::new();
        let boom = Happening { kind: Trigger::Custom("boom".into()), ..hit(B21) };
        let other = Happening { kind: Trigger::Custom("bang".into()), ..hit(B21) };
        Vm::new(1).run(&p, &TwoBalls, 0.0, &[other, boom], &mut acts);
        assert_eq!(acts, vec![Act::Grow { ball: B21, by: 2.0 }]);
    }
}
