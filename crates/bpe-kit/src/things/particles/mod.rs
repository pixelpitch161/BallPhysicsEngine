//! Particles: a thing that makes its own particles (particles spec, part 1): rain over the whole
//! picture, sparks from a point, fire riding a ball, specks thrown off where balls hit. Its own
//! settings are the **Source** (where and when particles are born, and what each starts as); its
//! cards are **Motion** (force layers, `forces.rs`), **Meets things** (balls, shapes and the
//! picture's edges, `meets.rs`) and **Look** (draw layers, `look.rs`).
//!
//! The particles are worked out exactly on the CPU, stepped with the physics at 120 Hz
//! (`sim.rs`, `step.rs`), their state in the world so checkpoints and seeks replay them. In part 1
//! a Particles thing is decoration only: it reads the balls and walls and changes nothing anything
//! else reads, so it is stepped beside the physics ([`bpe_runtime::ThingSim::in_main`]).

pub mod births;
pub mod draw;
pub mod forces;
pub mod look;
pub mod looks;
pub mod meets;
pub mod sim;
pub mod state;
pub mod step;
#[cfg(test)]
mod tests;
pub mod web;

use bpe_core::{Choice, SettingDef, Settings, SlotId, Target, ThingKind};
use glam::Vec2;

/// The thing kind's id.
pub const KIND: &str = "particles";

/// The thing kinds a Particles thing inside can ride.
pub const RIDES: [&str; 3] = ["balls", "emitter", "shape"];

/// Where particles are born.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum BornFrom {
    /// Over everything the camera sees, or along one of its edges.
    #[default]
    WholeScreen,
    /// Inside a rectangle, circle or ring round the thing's place.
    AnArea,
    /// At the thing's place.
    APoint,
    /// Along a line through the thing's place.
    ALine,
    /// On what the thing sits in: its balls or its shape.
    RidingItsParent,
}

/// Which part of the screen Whole screen births come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Edge {
    #[default]
    Everywhere,
    Top,
    Bottom,
    Left,
    Right,
}

/// The shape of an area source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum AreaShape {
    #[default]
    Rectangle,
    Circle,
    Ring,
}

/// What part of its parent a riding source is born on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum RidesOn {
    /// Every ball.
    #[default]
    EachBall,
    /// The middle of the balls, or of the shape.
    TheMiddle,
    /// One ball, by its number.
    BallNumber,
    /// Each ball's rim, or the shape's outline.
    TheEdge,
    /// Inside each ball, or inside the shape.
    Inside,
}

/// When particles are born.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Emit {
    /// A steady stream, Rate a second (a ball, when it rides each ball).
    #[default]
    Steady,
    /// Count at once, Every so often.
    Bursts,
    /// Count each time a ball hits something.
    OnHits,
    /// Count at all times: born at the start, and each one that dies is replaced.
    KeepACrowd,
    /// Count at the start, and no more.
    OnceAtTheStart,
}

/// The way a new particle flies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Heading {
    /// The Angle you set.
    #[default]
    AWay,
    /// Any way.
    AllRound,
    /// Away from what a ball hit (On hits), or away from the middle of what it rides.
    OffTheBounce,
    /// The way the ball it rides is going.
    AlongThePath,
    /// Back the way the ball it rides came from.
    Behind,
}

/// A Particles thing's own settings: its Source.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ParticlesSettings {
    #[setting(label = "From", simple, group = "Where")]
    pub from: BornFrom,
    #[setting(label = "Edge", when = "from=whole_screen", group = "Where")]
    pub edge: Edge,
    /// How far past the screen's edge particles are born (and live on before they go).
    #[setting(label = "Margin", when = "from=whole_screen", span(0, 400), unit = "px", group = "Where")]
    pub margin: f32,
    #[setting(label = "Area shape", when = "from=an_area", group = "Where")]
    pub area: AreaShape,
    #[setting(
        label = "Width",
        when = "from=an_area and area=rectangle",
        range(0, 1e9),
        span(0, 2000),
        unit = "px",
        keyframe,
        group = "Where"
    )]
    pub width: f32,
    #[setting(
        label = "Height",
        when = "from=an_area and area=rectangle",
        range(0, 1e9),
        span(0, 2000),
        unit = "px",
        keyframe,
        group = "Where"
    )]
    pub height: f32,
    #[setting(
        label = "Radius",
        when = "from=an_area and area!=rectangle",
        range(0, 1e9),
        span(0, 1000),
        unit = "px",
        keyframe,
        group = "Where"
    )]
    pub radius: f32,
    #[setting(
        label = "Inner radius",
        when = "from=an_area and area=ring",
        range(0, 1e9),
        span(0, 1000),
        unit = "px",
        keyframe,
        group = "Where"
    )]
    pub inner: f32,
    #[setting(
        label = "Length",
        when = "from=a_line",
        range(0, 1e9),
        span(0, 2000),
        unit = "px",
        keyframe,
        group = "Where"
    )]
    pub length: f32,
    /// The way the line (or the area) turns, in degrees.
    #[setting(label = "Turn", when = "from=a_line|an_area", span(-180, 180), unit = "°", keyframe, group = "Where")]
    pub line_turn: f32,
    #[setting(label = "Rides on", when = "from=riding_its_parent", group = "Where")]
    pub rides_on: RidesOn,
    #[setting(
        label = "Ball number",
        when = "from=riding_its_parent and rides_on=ball_number",
        range(1, 1e9),
        span(1, 100),
        group = "Where"
    )]
    pub ball_number: u32,

    #[setting(label = "Emit", simple, group = "When")]
    pub emit: Emit,
    /// Particles a second (a ball, when they ride each ball).
    #[setting(
        label = "Rate",
        when = "emit=steady",
        range(0, 1e9),
        span(0, 2000),
        unit = "per s",
        simple,
        keyframe,
        pulse,
        group = "When"
    )]
    pub rate: f32,
    #[setting(
        label = "Every",
        when = "emit=bursts",
        range(0.001, 1e9),
        span(0.05, 10),
        unit = "s",
        keyframe,
        group = "When"
    )]
    pub every: f32,
    /// How many each burst, hit or crowd has.
    #[setting(label = "Count", when = "emit!=steady", range(0, 1e9), span(1, 2000), keyframe, pulse, group = "When")]
    pub count: u32,
    /// Whose balls' hits let particles off.
    #[setting(label = "Who hits", when = "emit=on_hits", group = "When")]
    pub who_hits: Target,
    #[setting(label = "Least speed", when = "emit=on_hits", span(0, 2000), unit = "px/s", keyframe, group = "When")]
    pub least_speed: f32,
    /// Harder hits let off more, faster particles.
    #[setting(label = "Follow impact", when = "emit=on_hits", group = "When")]
    pub follow_impact: bool,

    #[setting(
        label = "Life",
        range(0.001, 1e9),
        span(0.05, 20),
        unit = "s",
        simple,
        keyframe,
        group = "Each particle"
    )]
    pub life: f32,
    #[setting(label = "Life spread", span(0, 1), keyframe, group = "Each particle")]
    pub life_spread: f32,
    #[setting(label = "Speed", span(0, 2000), unit = "px/s", simple, keyframe, pulse, group = "Each particle")]
    pub speed: f32,
    #[setting(label = "Speed spread", span(0, 1), keyframe, group = "Each particle")]
    pub speed_spread: f32,
    #[setting(label = "Direction", group = "Each particle")]
    pub heading: Heading,
    /// The way they fly, in degrees: 0 to the right, 90 down, −90 up.
    #[setting(label = "Angle", when = "heading=a_way", span(-180, 180), unit = "°", keyframe, group = "Each particle")]
    pub angle: f32,
    /// How wide they fan out round their way.
    #[setting(
        label = "Spread",
        when = "heading!=all_round",
        span(0, 360),
        unit = "°",
        keyframe,
        group = "Each particle"
    )]
    pub spread: f32,
    /// How much of the speed of the ball they come from they take with them.
    #[setting(label = "Takes the parent's speed", span(0, 1), keyframe, group = "Each particle")]
    pub inherit: f32,
    /// Each one's half size.
    #[setting(
        label = "Size",
        range(0, 1e9),
        span(0.5, 60),
        unit = "px",
        simple,
        keyframe,
        pulse,
        group = "Each particle"
    )]
    pub size: f32,
    #[setting(label = "Size spread", span(0, 1), keyframe, group = "Each particle")]
    pub size_spread: f32,
    #[setting(label = "Turn", span(-180, 180), unit = "°", keyframe, group = "Each particle")]
    pub turn: f32,
    /// How much of a whole turn each one's start turn is spread by.
    #[setting(label = "Turn spread", span(0, 1), keyframe, group = "Each particle")]
    pub turn_spread: f32,
    #[setting(label = "Spin", span(-1440, 1440), unit = "°/s", keyframe, group = "Each particle")]
    pub spin: f32,
    #[setting(label = "Spin spread", span(0, 1), keyframe, group = "Each particle")]
    pub spin_spread: f32,
    /// Half of them spin the other way.
    #[setting(label = "Spin both ways", group = "Each particle")]
    pub spin_both: bool,
    /// Another seed gives other dice: other places, speeds and sizes.
    #[setting(label = "Seed", span(0, 1000), group = "Each particle")]
    pub seed: u32,

    /// When this many are alive, new ones aren't born until some die.
    #[setting(label = "Most particles", range(1, 1000000), span(1, 200000), group = "Limits")]
    pub most: u32,
}

impl Default for ParticlesSettings {
    fn default() -> Self {
        Self {
            from: BornFrom::WholeScreen,
            edge: Edge::Everywhere,
            margin: 40.0,
            area: AreaShape::Rectangle,
            width: 400.0,
            height: 300.0,
            radius: 200.0,
            inner: 100.0,
            length: 400.0,
            line_turn: 0.0,
            rides_on: RidesOn::EachBall,
            ball_number: 1,
            emit: Emit::Steady,
            rate: 60.0,
            every: 1.0,
            count: 30,
            who_hits: Target::Anything,
            least_speed: 0.0,
            follow_impact: true,
            life: 2.0,
            life_spread: 0.3,
            speed: 100.0,
            speed_spread: 0.3,
            heading: Heading::AWay,
            angle: -90.0,
            spread: 30.0,
            inherit: 0.0,
            size: 3.0,
            size_spread: 0.3,
            turn: 0.0,
            turn_spread: 1.0,
            spin: 0.0,
            spin_spread: 0.5,
            spin_both: true,
            seed: 0,
            most: 20_000,
        }
    }
}

/// The hard cap on Most particles.
pub const MOST_EVER: u32 = 1_000_000;

impl ParticlesSettings {
    /// The way `angle` (degrees) points.
    pub fn way(angle: f32) -> Vec2 {
        Vec2::from_angle(angle.to_radians())
    }
}

pub struct ParticlesKind;

impl ThingKind for ParticlesKind {
    fn id(&self) -> &'static str {
        KIND
    }
    fn name(&self) -> &'static str {
        "Particles"
    }
    fn icon(&self) -> &'static str {
        "particles"
    }
    fn help(&self) -> &'static str {
        "Makes its own particles: rain, sparks, smoke, specks. Inside a group of balls or a shape it can ride on it."
    }
    fn settings(&self) -> &'static [SettingDef] {
        ParticlesSettings::defs()
    }
    fn slots(&self) -> &'static [SlotId] {
        &[SlotId::Motion, SlotId::Bounce, SlotId::Look]
    }
    fn default_slots(&self) -> &'static [SlotId] {
        &[SlotId::Motion, SlotId::Bounce, SlotId::Look]
    }
    fn family(&self) -> &'static str {
        "Particles"
    }
    fn slot_label(&self, slot: SlotId) -> &'static str {
        match slot {
            SlotId::Bounce => "Meets things",
            other => other.name(),
        }
    }
    fn can_stick_to_screen(&self) -> bool {
        false
    }
}
