//! Every block a CPU graph can use, by role.

use std::sync::OnceLock;

use bpe_core::SlotId;
use bpe_graph::{NodeSpec, Param, ParamSpec, PinSpec, PinType, SpecList};

/// What a graph is for, which decides its blocks and how it runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    /// Trigger chains that read events and queue actions.
    Rules,
    /// One hit in, a new velocity and size out.
    Bounce,
    /// One ball and step in, a new velocity out.
    Motion,
    /// One hit in, a note out.
    Sound,
}

use PinType::{Ball, Bool, Color, Number, Trigger, Vector};

const BOUNCE_IN: &[(&str, PinType)] = &[
    ("ball", Ball),
    ("velocity", Vector),
    ("normal", Vector),
    ("speed", Number),
    ("radius", Number),
    ("hits", Number),
    ("hit", Bool),
    ("time", Number),
];
const BOUNCE_OUT: &[(&str, PinType)] = &[("velocity", Vector), ("radius", Number), ("takes over", Bool)];
const MOTION_IN: &[(&str, PinType)] = &[
    ("ball", Ball),
    ("position", Vector),
    ("velocity", Vector),
    ("gravity", Vector),
    ("dt", Number),
    ("time", Number),
    ("radius", Number),
];
const MOTION_OUT: &[(&str, PinType)] = &[("velocity", Vector)];
const SOUND_IN: &[(&str, PinType)] = &[
    ("ball", Ball),
    ("speed", Number),
    ("index", Number),
    ("time", Number),
    ("x", Number),
    ("y", Number),
    ("hits", Number),
];
const SOUND_OUT: &[(&str, PinType)] = &[
    ("play", Bool),
    ("key", Number),
    ("velocity", Number),
    ("volume", Number),
    ("pan", Number),
    ("decay", Number),
    ("brightness", Number),
];

impl Role {
    /// The role a slot's graphs have, if its graphs run on the CPU.
    pub fn of_slot(slot: SlotId) -> Option<Role> {
        match slot {
            SlotId::Rules => Some(Role::Rules),
            SlotId::Bounce => Some(Role::Bounce),
            SlotId::Motion => Some(Role::Motion),
            SlotId::Sound => Some(Role::Sound),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Role::Rules => "Rules",
            Role::Bounce => "Bounce",
            Role::Motion => "Motion",
            Role::Sound => "Sound",
        }
    }

    /// The input block's output pins, in order: what one evaluation is given.
    pub fn inputs(self) -> &'static [(&'static str, PinType)] {
        match self {
            Role::Rules => &[],
            Role::Bounce => BOUNCE_IN,
            Role::Motion => MOTION_IN,
            Role::Sound => SOUND_IN,
        }
    }

    /// The output block's input pins, in order: what one evaluation gives back.
    pub fn outputs(self) -> &'static [(&'static str, PinType)] {
        match self {
            Role::Rules => &[],
            Role::Bounce => BOUNCE_OUT,
            Role::Motion => MOTION_OUT,
            Role::Sound => SOUND_OUT,
        }
    }

    fn in_kind(self) -> String {
        format!("{}.in", self.name().to_lowercase())
    }

    fn out_kind(self) -> String {
        format!("{}.out", self.name().to_lowercase())
    }

    /// The kind of this role's input block, if it has one.
    pub fn input_kind(self) -> Option<String> {
        (self != Role::Rules).then(|| self.in_kind())
    }

    pub fn output_kind(self) -> Option<String> {
        (self != Role::Rules).then(|| self.out_kind())
    }
}

/// The blocks whose value is a module setting shown on the Simple and More cards.
pub const SETTING_KINDS: [&str; 3] = ["setting", "setting.color", "setting.bool"];

pub fn is_setting(kind: &str) -> bool {
    SETTING_KINDS.contains(&kind)
}

/// The blocks a role's graphs can use.
pub fn catalog(role: Role) -> &'static SpecList {
    static RULES: OnceLock<SpecList> = OnceLock::new();
    static BOUNCE: OnceLock<SpecList> = OnceLock::new();
    static MOTION: OnceLock<SpecList> = OnceLock::new();
    static SOUND: OnceLock<SpecList> = OnceLock::new();
    let cell = match role {
        Role::Rules => &RULES,
        Role::Bounce => &BOUNCE,
        Role::Motion => &MOTION,
        Role::Sound => &SOUND,
    };
    cell.get_or_init(|| SpecList { specs: specs(role), cycles: false })
}

const VALUES: &str = "Values";
const SETTINGS: &str = "Settings";
const WHEN: &str = "When";
const ONLY_IF: &str = "Only if";
const DO: &str = "Do";
const MODULE: &str = "This module";

const COMPARE_OPS: &[&str] = &["less", "at most", "more", "at least", "equal", "different"];

fn spec(kind: &str, title: &str, category: &str, help: &str) -> NodeSpec {
    NodeSpec::new(kind, title, category, help)
}

fn pin(name: &str, ty: PinType) -> PinSpec {
    PinSpec::new(name, ty)
}

fn number(name: &str, label: &str, default: f32, min: f32, max: f32) -> ParamSpec {
    ParamSpec::number(name, label, default, min, max)
}

fn text(name: &str, label: &str, default: &str) -> ParamSpec {
    ParamSpec::new(name, label, Param::Text(default.into()))
}

fn target(name: &str, label: &str, default: &str) -> ParamSpec {
    text(name, label, default).help("Which things this is about: me, anything, a thing, a tag or a kind.")
}

fn choice(name: &str, label: &str, choices: &[&str]) -> ParamSpec {
    ParamSpec::choice(name, label, choices)
}

fn vector(name: &str, label: &str, default: [f32; 2]) -> ParamSpec {
    ParamSpec::new(name, label, Param::Vector(default))
}

fn color(name: &str, label: &str, default: [f32; 4]) -> ParamSpec {
    ParamSpec::new(name, label, Param::Color(default))
}

fn boolean(name: &str, label: &str, default: bool) -> ParamSpec {
    ParamSpec::new(name, label, Param::Bool(default))
}

/// A block of `kind` with the given input and output pins, each pin given as (name, type).
fn block(
    kind: &str,
    title: &str,
    category: &str,
    help: &str,
    ins: &[(&str, PinType)],
    outs: &[(&str, PinType)],
) -> NodeSpec {
    let mut s = spec(kind, title, category, help);
    for (name, ty) in ins {
        s = s.input(pin(name, *ty));
    }
    for (name, ty) in outs {
        s = s.output(pin(name, *ty));
    }
    s
}

/// Blocks every role has: constants, maths, vectors, colours and settings.
fn values() -> Vec<NodeSpec> {
    vec![
        block("number", "Number", VALUES, "A fixed number.", &[], &[("value", Number)])
            .param(number("value", "Value", 0.0, -10000.0, 10000.0)),
        block("vector", "Vector", VALUES, "A fixed position, direction or speed.", &[], &[("xy", Vector)])
            .param(vector("xy", "X and Y", [0.0, 0.0])),
        block("color", "Colour", VALUES, "A fixed colour.", &[], &[("colour", Color)]).param(color(
            "color",
            "Colour",
            [1.0, 1.0, 1.0, 1.0],
        )),
        block(
            "random",
            "Random",
            VALUES,
            "A random number between min and max, new every time it is read.",
            &[("min", Number), ("max", Number)],
            &[("value", Number)],
        )
        .param(number("min", "Min", 0.0, -10000.0, 10000.0))
        .param(number("max", "Max", 1.0, -10000.0, 10000.0)),
        block("time", "Time", VALUES, "Seconds since the start of the clip.", &[], &[("seconds", Number)]),
        block(
            "gravity",
            "Gravity",
            VALUES,
            "The scene's gravity, in pixels per second squared.",
            &[],
            &[("gravity", Vector)],
        ),
        block(
            "math",
            "Maths",
            VALUES,
            "Two numbers combined: add, subtract, multiply and so on.",
            &[("a", Number), ("b", Number)],
            &[("result", Number)],
        )
        .param(choice("op", "Operation", &["add", "subtract", "multiply", "divide", "min", "max", "power", "modulo"]))
        .param(number("a", "A", 0.0, -10000.0, 10000.0))
        .param(number("b", "B", 0.0, -10000.0, 10000.0)),
        block(
            "fn",
            "Function",
            VALUES,
            "One number changed: sine, absolute value, rounding and so on.",
            &[("x", Number)],
            &[("result", Number)],
        )
        .param(choice("f", "Function", &["sin", "cos", "abs", "floor", "round", "sqrt", "sign", "negate"]))
        .param(number("x", "X", 0.0, -10000.0, 10000.0)),
        block(
            "clamp",
            "Clamp",
            VALUES,
            "Keeps a number between min and max.",
            &[("x", Number), ("min", Number), ("max", Number)],
            &[("result", Number)],
        )
        .param(number("x", "X", 0.0, -10000.0, 10000.0))
        .param(number("min", "Min", 0.0, -10000.0, 10000.0))
        .param(number("max", "Max", 1.0, -10000.0, 10000.0)),
        block(
            "compare",
            "Compare",
            VALUES,
            "Yes when A compares to B the way you choose.",
            &[("a", Number), ("b", Number)],
            &[("yes", Bool)],
        )
        .param(choice("op", "Is", COMPARE_OPS))
        .param(number("a", "A", 0.0, -10000.0, 10000.0))
        .param(number("b", "B", 0.0, -10000.0, 10000.0)),
        block(
            "logic",
            "Logic",
            VALUES,
            "Two yes/no values combined: and, or, not, either.",
            &[("a", Bool), ("b", Bool)],
            &[("yes", Bool)],
        )
        .param(choice("op", "Operation", &["and", "or", "not", "xor"]))
        .param(boolean("a", "A", false))
        .param(boolean("b", "B", false)),
        block(
            "choose",
            "Choose number",
            VALUES,
            "One number when yes, the other when no.",
            &[("if", Bool), ("then", Number), ("else", Number)],
            &[("result", Number)],
        )
        .param(boolean("if", "If", false))
        .param(number("then", "Then", 1.0, -10000.0, 10000.0))
        .param(number("else", "Else", 0.0, -10000.0, 10000.0)),
        block(
            "choose.vector",
            "Choose vector",
            VALUES,
            "One vector when yes, the other when no.",
            &[("if", Bool), ("then", Vector), ("else", Vector)],
            &[("result", Vector)],
        )
        .param(boolean("if", "If", false))
        .param(vector("then", "Then", [0.0, 0.0]))
        .param(vector("else", "Else", [0.0, 0.0])),
        block(
            "choose.color",
            "Choose colour",
            VALUES,
            "One colour when yes, the other when no.",
            &[("if", Bool), ("then", Color), ("else", Color)],
            &[("result", Color)],
        )
        .param(boolean("if", "If", false))
        .param(color("then", "Then", [1.0, 1.0, 1.0, 1.0]))
        .param(color("else", "Else", [0.0, 0.0, 0.0, 1.0])),
        block(
            "vec.math",
            "Vector maths",
            VALUES,
            "Two vectors added or subtracted.",
            &[("a", Vector), ("b", Vector)],
            &[("result", Vector)],
        )
        .param(choice("op", "Operation", &["add", "subtract"]))
        .param(vector("a", "A", [0.0, 0.0]))
        .param(vector("b", "B", [0.0, 0.0])),
        block(
            "vec.scale",
            "Scale vector",
            VALUES,
            "A vector made longer or shorter.",
            &[("v", Vector), ("by", Number)],
            &[("result", Vector)],
        )
        .param(vector("v", "Vector", [0.0, 0.0]))
        .param(number("by", "By", 1.0, -100.0, 100.0)),
        block(
            "vec.make",
            "Make vector",
            VALUES,
            "A vector from an X and a Y.",
            &[("x", Number), ("y", Number)],
            &[("xy", Vector)],
        )
        .param(number("x", "X", 0.0, -10000.0, 10000.0))
        .param(number("y", "Y", 0.0, -10000.0, 10000.0)),
        block(
            "vec.split",
            "Split vector",
            VALUES,
            "A vector's X and Y as numbers.",
            &[("v", Vector)],
            &[("x", Number), ("y", Number)],
        )
        .param(vector("v", "Vector", [0.0, 0.0])),
        block(
            "vec.length",
            "Length",
            VALUES,
            "How long a vector is: a speed, a distance.",
            &[("v", Vector)],
            &[("length", Number)],
        )
        .param(vector("v", "Vector", [0.0, 0.0])),
        block(
            "vec.normalize",
            "Direction",
            VALUES,
            "A vector shrunk to length 1.",
            &[("v", Vector)],
            &[("direction", Vector)],
        )
        .param(vector("v", "Vector", [0.0, 0.0])),
        block(
            "vec.rotate",
            "Rotate",
            VALUES,
            "A vector turned by some degrees.",
            &[("v", Vector), ("degrees", Number)],
            &[("result", Vector)],
        )
        .param(vector("v", "Vector", [0.0, 0.0]))
        .param(number("degrees", "Degrees", 0.0, -360.0, 360.0)),
        block(
            "vec.limit",
            "Limit speed",
            VALUES,
            "A vector no longer than max.",
            &[("v", Vector), ("max", Number)],
            &[("result", Vector)],
        )
        .param(vector("v", "Vector", [0.0, 0.0]))
        .param(number("max", "Max", 1000.0, 0.0, 20000.0)),
        block(
            "reflect",
            "Bounce off",
            VALUES,
            "A velocity bounced off a surface, keeping some of its speed into it.",
            &[("v", Vector), ("normal", Vector), ("keep", Number)],
            &[("result", Vector)],
        )
        .param(vector("v", "Velocity", [0.0, 0.0]))
        .param(vector("normal", "Normal", [0.0, -1.0]))
        .param(number("keep", "Keep", 1.0, 0.0, 1.5)),
        block(
            "color.mix",
            "Mix colours",
            VALUES,
            "A blend of two colours: 0 is all A, 1 is all B.",
            &[("a", Color), ("b", Color), ("t", Number)],
            &[("colour", Color)],
        )
        .param(color("a", "A", [1.0, 1.0, 1.0, 1.0]))
        .param(color("b", "B", [0.0, 0.0, 0.0, 1.0]))
        .param(number("t", "Mix", 0.5, 0.0, 1.0)),
        block(
            "color.hsv",
            "Colour from hue",
            VALUES,
            "A colour from a hue (0 to 1 around the wheel), saturation and brightness.",
            &[("hue", Number), ("saturation", Number), ("value", Number)],
            &[("colour", Color)],
        )
        .param(number("hue", "Hue", 0.0, 0.0, 1.0))
        .param(number("saturation", "Saturation", 1.0, 0.0, 1.0))
        .param(number("value", "Brightness", 1.0, 0.0, 1.0)),
        block(
            "setting",
            "Setting",
            SETTINGS,
            "A number shown as a slider on the module's card.",
            &[],
            &[("value", Number)],
        )
        .param(text("key", "Key", ""))
        .param(text("label", "Label", "Setting"))
        .param(number("value", "Value", 0.0, -10000.0, 10000.0))
        .param(number("min", "Min", 0.0, -10000.0, 10000.0))
        .param(number("max", "Max", 1.0, -10000.0, 10000.0)),
        block(
            "setting.color",
            "Colour setting",
            SETTINGS,
            "A colour shown on the module's card.",
            &[],
            &[("colour", Color)],
        )
        .param(text("key", "Key", ""))
        .param(text("label", "Label", "Colour"))
        .param(color("color", "Colour", [1.0, 1.0, 1.0, 1.0])),
        block(
            "setting.bool",
            "Yes/no setting",
            SETTINGS,
            "A switch shown on the module's card.",
            &[],
            &[("yes", Bool)],
        )
        .param(text("key", "Key", ""))
        .param(text("label", "Label", "Switch"))
        .param(boolean("on", "On", false)),
    ]
}

const WHEN_OUTS: &[(&str, PinType)] =
    &[("then", Trigger), ("ball", Ball), ("other", Ball), ("speed", Number), ("where", Vector)];

fn when(kind: &str, title: &str, help: &str) -> NodeSpec {
    block(kind, title, WHEN, help, &[], WHEN_OUTS)
}

fn only_if(kind: &str, title: &str, help: &str, ins: &[(&str, PinType)]) -> NodeSpec {
    let mut all = vec![("when", Trigger)];
    all.extend_from_slice(ins);
    block(kind, title, ONLY_IF, help, &all, &[("then", Trigger), ("else", Trigger)])
}

fn act(kind: &str, title: &str, help: &str, ins: &[(&str, PinType)]) -> NodeSpec {
    let mut all = vec![("when", Trigger)];
    all.extend_from_slice(ins);
    block(kind, title, DO, help, &all, &[("then", Trigger)])
}

/// The rule blocks: what happens, what has to hold, what to do, plus ball values.
fn rules() -> Vec<NodeSpec> {
    vec![
        block(
            "ball",
            "About a ball",
            VALUES,
            "Where a ball is, how fast it goes, how big it is and how often it has hit.",
            &[("ball", Ball)],
            &[
                ("position", Vector),
                ("velocity", Vector),
                ("speed", Number),
                ("radius", Number),
                ("hits", Number),
                ("exists", Bool),
            ],
        ),
        block(
            "ball.is",
            "Ball is",
            VALUES,
            "Yes when the ball belongs to the target.",
            &[("ball", Ball)],
            &[("yes", Bool)],
        )
        .param(target("target", "Target", "anything")),
        block("ball.pick", "A ball", VALUES, "One ball of the target, picked at random.", &[], &[("ball", Ball)])
            .param(target("target", "Target", "me")),
        block("count", "How many", VALUES, "How many balls the target has right now.", &[], &[("count", Number)])
            .param(target("target", "Target", "me")),
        block("counter", "Counter value", VALUES, "What a counter has counted up to.", &[], &[("value", Number)])
            .param(number("id", "Counter", 0.0, 0.0, 15.0)),
        when("when.start", "When the clip starts", "Fires once, at the very start."),
        when("when.hit", "When hit", "Fires when one of these things hits what you choose.")
            .param(target("who", "Who", "me"))
            .param(target("what", "Hits what", "anything")),
        when("when.spawn", "When a ball appears", "Fires when a ball of these things appears after the start.")
            .param(target("who", "Who", "me")),
        when("when.remove", "When a ball is removed", "Fires when a ball of these things is removed.")
            .param(target("who", "Who", "me")),
        when("when.enter", "When entering", "Fires when a ball of these things enters a zone.")
            .param(target("who", "Who", "me"))
            .param(target("what", "Enters what", "anything")),
        when("when.leave", "When leaving", "Fires when a ball of these things leaves a zone.")
            .param(target("who", "Who", "me"))
            .param(target("what", "Leaves what", "anything")),
        when("when.beat", "On the beat", "Fires on every beat of the music."),
        when(
            "when.event",
            "When an event fires",
            "Fires on a custom event with this name, sent by a Fire event block.",
        )
        .param(target("who", "Who", "anything"))
        .param(text("name", "Name", "custom")),
        when("when.timer", "Every few seconds", "Fires on a timer: first at the first time, then every so often.")
            .param(number("every", "Every (s)", 1.0, 0.05, 60.0))
            .param(number("first", "First at (s)", 1.0, 0.0, 60.0)),
        when("when.at", "At a time", "Fires once, at this time.").param(number("time", "Time (s)", 1.0, 0.0, 600.0)),
        when("when.counter", "When a counter reaches", "Fires each time the counter climbs to this number.")
            .param(number("id", "Counter", 0.0, 0.0, 15.0))
            .param(number("reaches", "Reaches", 10.0, 1.0, 100000.0)),
        only_if("if.chance", "Chance", "Carries on this often, at random.", &[("percent", Number)])
            .param(number("percent", "Percent", 50.0, 0.0, 100.0)),
        only_if("if.every", "Every nth time", "Carries on every nth time it is reached.", &[])
            .param(number("n", "Every", 2.0, 1.0, 1000.0)),
        only_if("if.once", "Only once", "Carries on the first time only.", &[]),
        only_if("if.cooldown", "Not too often", "Carries on at most once every few seconds.", &[("seconds", Number)])
            .param(number("seconds", "Seconds", 1.0, 0.01, 60.0)),
        only_if(
            "if.compare",
            "Only if",
            "Carries on when A compares to B the way you choose.",
            &[("a", Number), ("b", Number)],
        )
        .param(choice("op", "Is", COMPARE_OPS))
        .param(number("a", "A", 0.0, -10000.0, 10000.0))
        .param(number("b", "B", 0.0, -10000.0, 10000.0)),
        only_if("if.is", "Only if the ball is", "Carries on when the ball belongs to the target.", &[("ball", Ball)])
            .param(target("target", "Target", "anything")),
        act(
            "do.grow",
            "Grow",
            "Makes the ball bigger by some pixels, or smaller when negative.",
            &[("ball", Ball), ("by", Number)],
        )
        .param(number("by", "By (px)", 2.0, -50.0, 50.0)),
        act("do.speed", "Change speed", "Multiplies the ball's speed.", &[("ball", Ball), ("factor", Number)])
            .param(number("factor", "Factor", 1.1, 0.0, 3.0)),
        act("do.kick", "Kick", "Adds a velocity to the ball.", &[("ball", Ball), ("velocity", Vector)]).param(vector(
            "velocity",
            "Velocity",
            [0.0, -500.0],
        )),
        act(
            "do.color",
            "Change colour",
            "Tints the ball towards a colour.",
            &[("ball", Ball), ("color", Color), ("amount", Number)],
        )
        .param(color("color", "Colour", [1.0, 0.3, 0.6, 1.0]))
        .param(number("amount", "Amount", 1.0, 0.0, 1.0)),
        act("do.remove", "Remove", "Takes the ball out of the scene.", &[("ball", Ball)]),
        act(
            "do.split",
            "Split",
            "Splits the ball into pieces that share its area.",
            &[("ball", Ball), ("pieces", Number)],
        )
        .param(number("pieces", "Pieces", 2.0, 2.0, 8.0)),
        act(
            "do.spawn",
            "Spawn",
            "Adds a new ball to a thing.",
            &[("at", Vector), ("velocity", Vector), ("size", Number), ("color", Color)],
        )
        .param(target("into", "Into", "me"))
        .param(vector("at", "At", [0.0, 0.0]))
        .param(vector("velocity", "Velocity", [0.0, 0.0]))
        .param(number("size", "Size (px)", 24.0, 0.5, 400.0))
        .param(color("color", "Colour", [0.0, 0.0, 0.0, 0.0])),
        act("do.gravity", "Set gravity", "Changes the scene's gravity from now on.", &[("gravity", Vector)])
            .param(vector("gravity", "Gravity", [0.0, 900.0])),
        act(
            "do.set",
            "Set a setting",
            "Changes a Motion or Bounce setting of the target's module, for the target only.",
            &[("value", Number)],
        )
        .param(target("target", "Target", "me"))
        .param(choice("slot", "Slot", &["Bounce", "Motion"]))
        .param(text("key", "Setting", "bounciness"))
        .param(number("value", "Value", 1.0, -10000.0, 10000.0)),
        act("do.event", "Fire event", "Sends a custom event other rules can wait for.", &[("ball", Ball)])
            .param(text("name", "Name", "custom")),
        act(
            "do.note",
            "Play a note",
            "Plays the thing's sound as if it had been hit this hard.",
            &[("ball", Ball), ("loudness", Number)],
        )
        .param(number("loudness", "Loudness", 600.0, 0.0, 10000.0)),
        act("do.counter", "Count", "Adds to a counter.", &[("add", Number)])
            .param(number("id", "Counter", 0.0, 0.0, 15.0))
            .param(number("add", "Add", 1.0, -1000.0, 1000.0)),
    ]
}

fn role_blocks(role: Role) -> Vec<NodeSpec> {
    let (title, in_help, out_help) = match role {
        Role::Rules => return Vec::new(),
        Role::Bounce => (
            "hit",
            "The ball and the surface it hit, as the module sees them.",
            "What the bounce does: the new velocity and size. Pins left unwired keep what the Basic bounce would do.",
        ),
        Role::Motion => (
            "step",
            "One ball at one step: where it is, how it moves, the gravity and the time step.",
            "The ball's velocity after this step. Unwired, the Basic motion applies.",
        ),
        Role::Sound => (
            "hit",
            "The hit that is about to make a sound.",
            "The note to play. Pins left unwired take what the Notes sound would choose.",
        ),
    };
    let name = role.name().to_lowercase();
    let mut out = vec![
        block(&format!("{name}.in"), &format!("The {title}"), MODULE, in_help, &[], role.inputs()),
        block(&format!("{name}.out"), &format!("{} result", role.name()), MODULE, out_help, role.outputs(), &[]),
    ];
    if role == Role::Sound {
        out.push(
            block(
                "sound.chord",
                "Note of the chord",
                MODULE,
                "A note of the chord playing right now, picked by the hit's number.",
                &[("index", Number)],
                &[("key", Number), ("has", Bool)],
            )
            .param(number("index", "Pick", 0.0, 0.0, 100000.0)),
        );
    }
    out
}

fn specs(role: Role) -> Vec<NodeSpec> {
    let mut out = role_blocks(role);
    if role == Role::Rules {
        out.extend(rules());
    }
    out.extend(values());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::SlotId;
    use bpe_graph::{Catalog, PinType};

    #[test]
    fn every_role_has_its_blocks_and_only_rules_has_when_blocks() {
        assert!(catalog(Role::Rules).spec("when.hit").is_some());
        assert!(catalog(Role::Bounce).spec("when.hit").is_none());
        assert!(catalog(Role::Bounce).spec("bounce.in").is_some());
        assert!(catalog(Role::Motion).spec("bounce.in").is_none());
        assert!(catalog(Role::Sound).spec("sound.chord").is_some());
        assert!(catalog(Role::Rules).spec("ball").is_some());
        assert!(catalog(Role::Motion).spec("ball").is_none());
        assert_eq!(Role::of_slot(SlotId::Look), None);
        assert_eq!(Role::of_slot(SlotId::Rules), Some(Role::Rules));
        assert_eq!(Role::Bounce.inputs().len(), 8);
        assert_eq!(Role::Sound.outputs()[0], ("play", PinType::Bool));
        assert_eq!(Role::Rules.inputs(), &[]);
        assert!(is_setting("setting.color") && !is_setting("number"));
        for role in [Role::Rules, Role::Bounce, Role::Motion, Role::Sound] {
            for spec in catalog(role).all() {
                assert!(!spec.help.is_empty(), "{} has no help", spec.kind);
                assert!(!spec.category.is_empty(), "{} has no category", spec.kind);
                for p in &spec.params {
                    assert!(!p.label.is_empty(), "{}.{} has no label", spec.kind, p.name);
                }
            }
        }
    }

    #[test]
    fn the_in_and_out_blocks_match_the_role_pin_lists() {
        for role in [Role::Bounce, Role::Motion, Role::Sound] {
            let c = catalog(role);
            let input = c.spec(&format!("{}.in", role.name().to_lowercase())).unwrap();
            let output = c.spec(&format!("{}.out", role.name().to_lowercase())).unwrap();
            let outs: Vec<(&str, PinType)> = input.outputs.iter().map(|p| (p.name.as_str(), p.ty)).collect();
            assert_eq!(outs, role.inputs());
            let ins: Vec<(&str, PinType)> = output.inputs.iter().map(|p| (p.name.as_str(), p.ty)).collect();
            assert_eq!(ins, role.outputs());
            assert!(output.params.is_empty(), "unwired output pins must stay unset");
        }
    }
}
