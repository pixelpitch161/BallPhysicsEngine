//! The one card each of a Water scene's placed things: **Pour**, **Drain** and **Force** (a
//! **Lamp**'s is in [`super::look`]). Where each is, and which way it is turned, is the thing's own place.

use bpe_core::{Choice, Settings, Target};
use glam::Vec2;

/// How a Pour lets its liquid in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum PourKind {
    /// A steady stream.
    #[default]
    Tap,
    /// Drops falling over a width.
    Rain,
    /// One drop at a time.
    Drip,
    /// All at once, outwards.
    Burst,
    /// Liquid already there when the video starts.
    FillAtTheStart,
}

/// A Pour's card.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct PourCard {
    /// Which Liquid it pours.
    #[setting(label = "Liquid", group = "Pour")]
    pub liquid: Target,
    #[setting(label = "Kind", simple, group = "Pour")]
    pub kind: PourKind,
    /// How much comes out: 1 is a full stream of its Width.
    #[setting(
        label = "Rate",
        when = "kind=tap",
        range(0, 1e9),
        span(0, 4),
        unit = "×",
        simple,
        keyframe,
        group = "Pour"
    )]
    pub rate: f32,
    /// How many drops fall a second (rain over its Width, a drip from its point).
    #[setting(
        label = "Drops a second",
        when = "kind=rain or kind=drip",
        range(0, 10000),
        span(0, 60),
        simple,
        group = "Pour"
    )]
    pub drops_a_second: f32,
    /// How big each drop is across (rain's vary a little either way).
    #[setting(
        label = "Drop size",
        when = "kind=rain or kind=drip",
        range(1, 400),
        span(2, 60),
        unit = "px",
        group = "Pour"
    )]
    pub drop_size: f32,
    /// How big the lump a burst throws is across.
    #[setting(
        label = "Burst size",
        when = "kind=burst",
        range(1, 4000),
        span(10, 600),
        unit = "px",
        simple,
        group = "Pour"
    )]
    pub burst_size: f32,
    /// How long between bursts (0: one burst, at its Starts).
    #[setting(label = "Every", when = "kind=burst", range(0, 600), span(0, 10), unit = "s", group = "Pour")]
    pub every: f32,
    #[setting(
        label = "Width",
        when = "kind=tap or kind=rain",
        range(0, 1e9),
        span(1, 400),
        unit = "px",
        simple,
        keyframe,
        group = "Pour"
    )]
    pub width: f32,
    #[setting(
        label = "Speed",
        when = "kind!=fill_at_the_start",
        range(0, 1e9),
        span(0, 2000),
        unit = "px/s",
        keyframe,
        group = "Pour"
    )]
    pub speed: f32,
    /// Which way it pours: 90 is straight down.
    #[setting(label = "Aim", when = "kind!=fill_at_the_start", span(-180, 180), unit = "°", keyframe, group = "Pour")]
    pub aim: f32,
    /// How far the stream fans out (a burst's lump: 0 is thrown whole, 180 bursts outward).
    #[setting(
        label = "Spread",
        when = "kind=tap or kind=rain or kind=burst",
        range(0, 180),
        span(0, 45),
        unit = "°",
        keyframe,
        group = "Pour"
    )]
    pub spread: f32,
    /// How wide and tall the liquid there at the start is.
    #[setting(label = "Fill size", when = "kind=fill_at_the_start", unit = "px", group = "Pour")]
    pub fill_size: Vec2,
    #[setting(label = "Starts at", when = "kind!=fill_at_the_start", range(0, 600), unit = "s", group = "Pour")]
    pub starts: f32,
    #[setting(label = "Stops at", when = "kind!=fill_at_the_start", range(0, 600), unit = "s", group = "Pour")]
    pub stops: f32,
    /// It pours only what drains that feed pours have taken, so a course can run as long as the
    /// video does.
    #[setting(label = "Fed by drains", when = "kind=tap", group = "Pour")]
    pub fed_by_drains: bool,
}

impl Default for PourCard {
    fn default() -> Self {
        Self {
            liquid: Target::Kind(crate::things::water::LIQUID.into()),
            kind: PourKind::Tap,
            rate: 1.0,
            width: 40.0,
            speed: 300.0,
            aim: 90.0,
            spread: 0.0,
            fill_size: Vec2::new(300.0, 200.0),
            starts: 0.0,
            stops: 600.0,
            drops_a_second: 8.0,
            drop_size: 12.0,
            burst_size: 160.0,
            every: 0.0,
            fed_by_drains: false,
        }
    }
}

/// How a Drain takes liquid away.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum DrainKind {
    /// A hole the liquid runs out of.
    #[default]
    Drain,
    /// A slow seep.
    Leak,
    /// Liquid leaves where it reaches an edge of the course.
    EdgeOfTheCourse,
}

/// Which edge of the course lets liquid out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum CourseEdge {
    #[default]
    Bottom,
    Top,
    Left,
    Right,
    All,
}

/// A Drain's card.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct DrainCard {
    #[setting(label = "Kind", simple, group = "Drain")]
    pub kind: DrainKind,
    #[setting(
        label = "Radius",
        when = "kind!=edge_of_the_course",
        range(0, 1e9),
        span(1, 200),
        unit = "px",
        simple,
        keyframe,
        group = "Drain"
    )]
    pub radius: f32,
    /// How fast it takes liquid away.
    #[setting(label = "Rate", range(0, 1e9), span(0, 4), unit = "×", simple, keyframe, group = "Drain")]
    pub rate: f32,
    /// How much the liquid spins as it goes down.
    #[setting(label = "Whirlpool", when = "kind=drain", range(0, 1e9), span(0, 4), keyframe, group = "Drain")]
    pub whirlpool: f32,
    #[setting(label = "Edge", when = "kind=edge_of_the_course", group = "Drain")]
    pub edge: CourseEdge,
    /// What it takes comes back at the Pours that are Fed by drains.
    #[setting(label = "Feeds pours", group = "Drain")]
    pub feeds_pours: bool,
}

impl Default for DrainCard {
    fn default() -> Self {
        Self {
            kind: DrainKind::Drain,
            radius: 40.0,
            rate: 1.0,
            whirlpool: 0.0,
            edge: CourseEdge::Bottom,
            feeds_pours: true,
        }
    }
}

/// What a Force does to the liquid near it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum ForceKind {
    /// Pulls liquid in (or pushes it away, below zero).
    #[default]
    Magnet,
    /// Spins liquid round it.
    Whirlpool,
    /// Blows everywhere, one way.
    Wind,
    Heater,
    Cooler,
}

/// A Force's card.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ForceCard {
    #[setting(label = "Kind", simple, group = "Force")]
    pub kind: ForceKind,
    #[setting(label = "Strength", span(-4, 4), simple, keyframe, group = "Force")]
    pub strength: f32,
    /// How far from it the liquid feels it.
    #[setting(
        label = "Reach",
        when = "kind!=wind",
        range(0, 1e9),
        span(10, 1500),
        unit = "px",
        simple,
        keyframe,
        group = "Force"
    )]
    pub reach: f32,
    /// Which way the wind blows.
    #[setting(label = "Aim", when = "kind=wind", span(-180, 180), unit = "°", keyframe, group = "Force")]
    pub aim: f32,
    /// How many spikes the liquid stands up in near the magnet (0: none).
    #[setting(label = "Spikes", when = "kind=magnet", range(0, 64), group = "Force")]
    pub spikes: u32,
    #[setting(label = "Spike size", when = "kind=magnet and spikes", range(0, 1), span(0, 1), group = "Force")]
    pub spike_size: f32,
}

impl Default for ForceCard {
    fn default() -> Self {
        Self { kind: ForceKind::Magnet, strength: 1.0, reach: 300.0, aim: 0.0, spikes: 0, spike_size: 0.5 }
    }
}
