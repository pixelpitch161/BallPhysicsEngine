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
    #[setting(label = "Rate", range(0, 1e9), span(0, 4), unit = "×", simple, keyframe, group = "Pour")]
    pub rate: f32,
    #[setting(
        label = "Width",
        when = "kind!=fill_at_the_start",
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
    /// How far the stream fans out.
    #[setting(
        label = "Spread",
        when = "kind!=fill_at_the_start",
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
}

impl Default for DrainCard {
    fn default() -> Self {
        Self { kind: DrainKind::Drain, radius: 40.0, rate: 1.0, whirlpool: 0.0, edge: CourseEdge::Bottom }
    }
}

/// What a Force does to the liquid near it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum ForceKind {
    /// Pulls liquid in (or pushes it away, below zero), standing it up in spikes like ferrofluid.
    #[default]
    Magnet,
    /// Spins liquid round it (below zero, the other way).
    Whirlpool,
    /// Blows everywhere, one way: spray and drops more than a pool's top.
    Wind,
    /// Arrives with heat, in a later part of Water mode.
    Heater,
    /// Arrives with heat, in a later part of Water mode.
    Cooler,
}

/// A Force's card. Strength 1 is a gravity's pull for a Magnet, 600 px/s round a Whirlpool's core
/// and a 1000 px/s wind.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ForceCard {
    #[setting(label = "Kind", simple, group = "Force")]
    pub kind: ForceKind,
    /// Which Liquids it moves.
    #[setting(label = "Liquid", group = "Force")]
    pub liquid: Target,
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
    /// Which way the wind blows: 0 is to the right, 90 straight down.
    #[setting(label = "Aim", when = "kind=wind", span(-180, 180), unit = "°", keyframe, group = "Force")]
    pub aim: f32,
    /// How much the wind comes and goes.
    #[setting(label = "Gusts", when = "kind=wind", range(0, 1), span(0, 1), keyframe, group = "Force")]
    pub gusts: f32,
    /// How many spikes the liquid stands up in near the magnet (0: none).
    #[setting(label = "Spikes", when = "kind=magnet", range(0, 64), group = "Force")]
    pub spikes: u32,
    /// How deep the valleys between the spikes are.
    #[setting(
        label = "Spike size",
        when = "kind=magnet and spikes",
        range(0, 1),
        span(0, 1),
        keyframe,
        group = "Force"
    )]
    pub spike_size: f32,
    /// Which way the middle of the spikes points: -90 is straight up.
    #[setting(
        label = "Spikes point",
        when = "kind=magnet and spikes",
        span(-180, 180),
        unit = "°",
        keyframe,
        group = "Force"
    )]
    pub spike_aim: f32,
    /// How far round the magnet the spikes stand (360: all round).
    #[setting(
        label = "Spike spread",
        when = "kind=magnet and spikes",
        range(0, 360),
        span(0, 360),
        unit = "°",
        group = "Force"
    )]
    pub spike_spread: f32,
}

impl Default for ForceCard {
    fn default() -> Self {
        Self {
            kind: ForceKind::Magnet,
            liquid: Target::Kind(crate::things::water::LIQUID.into()),
            strength: 1.0,
            reach: 300.0,
            aim: 0.0,
            gusts: 0.0,
            spikes: 9,
            spike_size: 0.75,
            spike_aim: -90.0,
            spike_spread: 180.0,
        }
    }
}
