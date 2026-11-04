//! Balls: one ball or a group of them in a shape, launched together or one by one.

pub mod draw;
pub mod shapes;
pub mod sim;

use bpe_core::{Choice, SettingDef, Settings, SlotId, ThingKind};
use glam::Vec2;

pub struct BallsKind;

/// The shape a group of balls starts in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum GroupShape {
    /// Rows and columns, row by row from the top.
    #[default]
    Grid,
    Line,
    Ring,
    /// Rings of balls inside one another, the outer one at Radius, filling it to the middle.
    FilledRing,
    FilledCircle,
    Triangle,
    Honeycomb,
    Spiral,
    Heart,
    Scatter,
    Word,
    /// Wherever each ball is dragged to.
    Custom,
}

/// How the balls of a group are let go.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Release {
    #[default]
    AllAtOnce,
    OneByOne,
    Waves,
}

/// The order a group's balls are let go in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ReleaseOrder {
    #[default]
    FirstToLast,
    CentreOut,
    Random,
}

/// Which way each ball of a group is launched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Launch {
    /// Every ball at the launch speed.
    #[default]
    Same,
    /// Spread like a fan around the launch direction, first ball to last.
    Fan,
    /// Away from the middle of the group, at the launch speed.
    Outward,
    /// Each a different way, at the launch speed.
    Random,
}

/// The heading the group's placement settings sit under; the Group view shows them, the thing's
/// own settings card doesn't.
pub const GROUP: &str = "Group";
/// The headings of the group's other settings, shown in the Group view's folds.
pub const VARIETY: &str = "Variety";
pub const RELEASE: &str = "Release";
pub const LAUNCH: &str = "Launch";
/// Every heading the Group view shows instead of the thing's own card.
pub const GROUP_HEADINGS: [&str; 4] = [GROUP, VARIETY, RELEASE, LAUNCH];

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct BallsSettings {
    /// How big each ball is. Shown beside the Ball / Group switch.
    #[setting(label = "Size", range(0.5, 200), unit = "px", simple, keyframe)]
    pub radius: f32,
    /// How fast, and which way, the balls start moving. Shown in the Placement card.
    #[setting(label = "Launch speed", unit = "px/s", simple, span(100, 900))]
    pub velocity: Vec2,
    /// Off, one ball; on, a group of them in a shape.
    #[setting(label = "Group", group = "Group", simple)]
    pub group: bool,
    /// The shape the group starts in.
    #[setting(label = "Shape", group = "Group", when = "group", simple)]
    pub shape: GroupShape,
    /// How many balls the group has.
    #[setting(
        label = "How many",
        group = "Group",
        when = "group and shape!=word|custom|filled_ring",
        range(1, 5000),
        simple
    )]
    pub count: u32,
    /// How many balls across each row.
    #[setting(label = "Columns", group = "Group", when = "group and shape=grid|honeycomb", range(1, 500))]
    pub columns: u32,
    /// The gap between neighbouring balls' edges (below zero, they start overlapping).
    #[setting(
        label = "Space between",
        group = "Group",
        when = "group and shape!=ring|custom",
        range(-400, 400),
        span(0, 60),
        unit = "px"
    )]
    pub space: f32,
    /// How far the ring's balls are from its middle.
    #[setting(
        label = "Radius",
        group = "Group",
        when = "group and shape=ring|filled_ring",
        range(1, 4000),
        unit = "px"
    )]
    pub ring_radius: f32,
    /// The letters the balls spell out.
    #[setting(label = "Word", group = "Group", when = "group and shape=word")]
    pub word: String,
    /// How tall the capital letters are.
    #[setting(label = "Letter height", group = "Group", when = "group and shape=word", range(10, 4000), unit = "px")]
    pub letter_height: f32,
    /// Picks the scattered places and each ball's variety; another number, another pick.
    #[setting(label = "Seed", group = "Group", when = "group and shape=scatter")]
    pub seed: i32,
    /// How wide and tall the area the balls are scattered over is.
    #[setting(label = "Area", group = "Group", when = "group and shape=scatter", unit = "px", span(50, 2000))]
    pub area: Vec2,
    /// Each ball's place, from the thing, dragged in the Group view.
    #[setting(label = "Places", group = "Group", when = "group and shape=custom")]
    pub custom: Vec<[f32; 2]>,
    /// How far the whole shape is turned, clockwise.
    #[setting(label = "Turn", group = "Group", when = "group", span(-180, 180), unit = "°")]
    pub turn: f32,
    /// How much each ball's size differs, at random.
    #[setting(label = "Size spread", group = "Variety", when = "group", range(0, 1))]
    pub vary_size: f32,
    /// How much each ball's launch speed differs, at random.
    #[setting(label = "Speed spread", group = "Variety", when = "group", range(0, 1))]
    pub vary_speed: f32,
    /// How far round the colour wheel each ball's colour may turn, at random.
    #[setting(label = "Colour spread", group = "Variety", when = "group", range(0, 1))]
    pub vary_colour: f32,
    /// How far each ball's look may be turned, at random.
    #[setting(label = "Turn spread", group = "Variety", when = "group", range(0, 180), unit = "°")]
    pub vary_turn: f32,
    /// Let every ball go at once, one by one, or in waves.
    #[setting(label = "Release", group = "Release", when = "group")]
    pub release: Release,
    /// The time between one ball (or wave) and the next.
    #[setting(
        label = "Gap",
        group = "Release",
        when = "group and release!=all_at_once",
        range(0, 30),
        span(0, 2),
        unit = "s"
    )]
    pub release_gap: f32,
    /// How many balls go in each wave.
    #[setting(label = "Wave size", group = "Release", when = "group and release=waves", range(1, 5000))]
    pub wave_size: u32,
    /// Which balls go first.
    #[setting(label = "Order", group = "Release", when = "group and release!=all_at_once")]
    pub release_order: ReleaseOrder,
    /// Which way each ball is launched.
    #[setting(label = "Launch", group = "Launch", when = "group")]
    pub launch: Launch,
    /// How wide the fan opens.
    #[setting(label = "Fan angle", group = "Launch", when = "group and launch=fan", range(0, 360), unit = "°")]
    pub fan_angle: f32,
}

impl Default for BallsSettings {
    fn default() -> Self {
        Self {
            radius: 24.0,
            velocity: Vec2::new(300.0, -200.0),
            group: false,
            shape: GroupShape::Grid,
            count: 30,
            columns: 6,
            space: 8.0,
            ring_radius: 200.0,
            word: "BALLS".into(),
            letter_height: 240.0,
            seed: 1,
            area: Vec2::new(600.0, 600.0),
            custom: Vec::new(),
            turn: 0.0,
            vary_size: 0.0,
            vary_speed: 0.0,
            vary_colour: 0.0,
            vary_turn: 0.0,
            release: Release::AllAtOnce,
            release_gap: 0.1,
            wave_size: 10,
            release_order: ReleaseOrder::FirstToLast,
            launch: Launch::Same,
            fan_angle: 60.0,
        }
    }
}

impl BallsSettings {
    /// A grid of `columns` by `rows` with centres `spacing` apart, as the old Block arrangement
    /// laid it out (ball size set first: the gap depends on it).
    pub fn block(&mut self, columns: u32, rows: u32, spacing: f32) {
        self.group = true;
        self.shape = GroupShape::Grid;
        self.columns = columns;
        self.count = columns * rows;
        self.space = spacing - 2.0 * self.radius;
    }

    /// A ring of `count` balls `ring_radius` from the middle, as the old Ring arrangement.
    pub fn ring(&mut self, count: u32, ring_radius: f32) {
        self.group = true;
        self.shape = GroupShape::Ring;
        self.count = count;
        self.ring_radius = ring_radius;
    }

    /// From one ball's middle to its neighbour's.
    pub fn step(&self) -> f32 {
        2.0 * self.radius + self.space
    }
}

const SLOTS: &[SlotId] = &[SlotId::Look, SlotId::Trail, SlotId::Motion, SlotId::Bounce, SlotId::Particles];
const DEFAULT_SLOTS: &[SlotId] = &[SlotId::Look, SlotId::Motion, SlotId::Bounce];

impl ThingKind for BallsKind {
    fn id(&self) -> &'static str {
        "balls"
    }
    fn family(&self) -> &'static str {
        "Bouncers"
    }
    fn name(&self) -> &'static str {
        "Balls"
    }
    fn icon(&self) -> &'static str {
        "ball"
    }
    fn help(&self) -> &'static str {
        "One ball or a group of them, launched together or one by one."
    }
    fn settings(&self) -> &'static [SettingDef] {
        BallsSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        SLOTS
    }
    fn default_slots(&self) -> &'static [SlotId] {
        DEFAULT_SLOTS
    }
}
