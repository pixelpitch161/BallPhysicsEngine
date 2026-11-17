//! A Liquid thing's cards: **Fill** (the Motion slot, with its Pour layers), **Meets** (the Bounce
//! slot, with its Other liquid rules) and **Look**.

use bpe_core::{Choice, Color, LayerSection, ModuleKind, ResolvedSlot, SettingDef, Settings, SlotId, Target};
use glam::Vec2;

use crate::modules::look::layers::Kind;
use crate::paint::Paint;

use super::num;

pub const FILL: &str = "liquid.fill";
pub const POUR: &str = "liquid.pour";
pub const MEETS: &str = "liquid.meets";
pub const OTHER: &str = "liquid.other";
pub const LOOK: &str = "liquid.look";
pub const HEAT: &str = "liquid.heat";
pub const DRAIN: &str = "liquid.drain";
pub const CURRENT: &str = "liquid.current";
pub const MAGNET: &str = "liquid.magnet";

/// Water's blue.
const WATER: Color = Color { r: 47, g: 127, b: 224, a: 255 };

fn one_colour(c: Color) -> Paint {
    Paint::solid(c)
}

/// What the Fill at the start is made of.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum FillWith {
    /// Up to Level.
    #[default]
    Level,
    /// The solid parts of a picture, in its colours.
    APicture,
    /// The letters of a word.
    AWord,
}

/// The Fill card: liquid there from the start.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct FillSettings {
    /// Fill its shape (or the scene) up to Level at the start.
    #[setting(label = "Fill at the start", simple, group = "Fill")]
    pub fill: bool,
    /// Up to a Level, or the shape of a picture or a word, round the Liquid thing's place.
    #[setting(label = "Fill with", when = "fill", group = "Fill")]
    pub fill_with: FillWith,
    /// How full: 0 empty, 1 to the top. Raising it adds liquid at the top, lowering it takes it away.
    #[setting(
        label = "Level",
        when = "fill and fill_with=level",
        range(0, 1),
        span(0, 1),
        simple,
        keyframe,
        pulse,
        group = "Fill"
    )]
    pub level: f32,
    /// A PNG picture: its solid parts become liquid in its own colours.
    #[setting(label = "Picture", when = "fill and fill_with=a_picture", unit = "picture", group = "Fill")]
    pub picture: String,
    #[setting(
        label = "Picture width",
        when = "fill and fill_with=a_picture",
        range(1, 1e9),
        span(20, 2000),
        unit = "px",
        group = "Fill"
    )]
    pub picture_width: f32,
    #[setting(label = "Text", when = "fill and fill_with=a_word", group = "Fill")]
    pub text: String,
    #[setting(
        label = "Letter height",
        when = "fill and fill_with=a_word",
        range(1, 1e9),
        span(20, 1000),
        unit = "px",
        group = "Fill"
    )]
    pub letter_height: f32,
    #[setting(paint, label = "Colour", group = "Fill", prefix = "colour_", follows = "place|order|time")]
    pub colour: Paint,
    /// How much its colour takes over when it mixes: a strong colour wins, a weak one only tints.
    #[setting(label = "Colour strength", range(0, 1e9), span(0, 4), keyframe, group = "Fill")]
    pub strength: f32,
    /// The most drops this liquid keeps: past it the oldest go.
    #[setting(label = "Most drops", range(1, 800000), span(1000, 800000), group = "Fill")]
    pub most: u32,
    /// Raindrops a second falling from the top (0: no rain).
    #[setting(label = "Rain", range(0, 1e9), span(0, 200), unit = "/s", keyframe, pulse, group = "Rain")]
    pub rain: f32,
    #[setting(
        label = "Raindrop size",
        when = "rain",
        range(0, 1e9),
        span(1, 60),
        unit = "px",
        keyframe,
        pulse,
        group = "Rain"
    )]
    pub rain_size: f32,
    #[setting(
        label = "Rain speed",
        when = "rain",
        range(0, 1e9),
        span(0, 3000),
        unit = "px/s",
        keyframe,
        pulse,
        group = "Rain"
    )]
    pub rain_speed: f32,
    /// The balls that leak and burst into this liquid.
    #[setting(label = "Balls", group = "From balls")]
    pub from_balls: Target,
    /// Drops a second each moving ball leaks (more the faster it goes; 0: none).
    #[setting(label = "Leaks", range(0, 1e9), span(0, 200), unit = "/s", keyframe, pulse, group = "From balls")]
    pub leaks: f32,
    /// A ball that goes (removed or broken) bursts into this many times its own size of liquid
    /// (0: none).
    #[setting(label = "Bursts", range(0, 1e9), span(0, 4), unit = "×", keyframe, group = "From balls")]
    pub bursts: f32,
}

impl Default for FillSettings {
    fn default() -> Self {
        FillSettings {
            fill: true,
            fill_with: FillWith::Level,
            level: 0.35,
            picture: String::new(),
            picture_width: 400.0,
            text: "LIQUID".into(),
            letter_height: 200.0,
            colour: one_colour(WATER),
            strength: 1.0,
            most: 400_000,
            rain: 0.0,
            rain_size: 8.0,
            rain_speed: 500.0,
            from_balls: Target::Anything,
            leaks: 0.0,
            bursts: 0.0,
        }
    }
}

impl FillSettings {
    pub fn read(card: Option<&ResolvedSlot>) -> FillSettings {
        let s = card.map(|c| FillSettings::from_params(&c.params)).unwrap_or_default();
        let d = FillSettings::default();
        FillSettings {
            level: num(s.level, 0.35).clamp(0.0, 1.0),
            strength: num(s.strength, 1.0).max(0.0),
            picture_width: num(s.picture_width, d.picture_width).clamp(1.0, 100_000.0),
            letter_height: num(s.letter_height, d.letter_height).clamp(1.0, 100_000.0),
            rain: num(s.rain, 0.0).max(0.0),
            rain_size: num(s.rain_size, d.rain_size).clamp(0.0, 2000.0),
            rain_speed: num(s.rain_speed, d.rain_speed).max(0.0),
            leaks: num(s.leaks, 0.0).max(0.0),
            bursts: num(s.bursts, 0.0).max(0.0),
            ..s
        }
    }
}

/// A heat source (or, with Strength below 0, a cold one).
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct HeatLayer {
    #[setting(label = "On", simple)]
    pub on: bool,
    /// Where it is, from the Liquid thing's place.
    #[setting(label = "Place", unit = "px", keyframe)]
    pub place: Vec2,
    #[setting(label = "Radius", range(0, 1e9), span(1, 600), unit = "px", simple, keyframe, pulse)]
    pub radius: f32,
    /// How hot (1) or cold (below 0) it makes the liquid near it, with the liquid's Warms.
    #[setting(label = "Strength", span(-1, 1), simple, keyframe, pulse)]
    pub strength: f32,
}

impl Default for HeatLayer {
    fn default() -> Self {
        HeatLayer { on: true, place: Vec2::new(0.0, 200.0), radius: 80.0, strength: 1.0 }
    }
}

/// A hole the liquid drains away through.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct DrainLayer {
    #[setting(label = "On", simple)]
    pub on: bool,
    #[setting(label = "Place", unit = "px", keyframe)]
    pub place: Vec2,
    #[setting(label = "Radius", range(0, 1e9), span(1, 200), unit = "px", simple, keyframe, pulse)]
    pub radius: f32,
    /// How hard the liquid swirls round it as it goes down.
    #[setting(label = "Whirlpool", range(0, 1e9), span(0, 4), simple, keyframe, pulse)]
    pub whirlpool: f32,
}

impl Default for DrainLayer {
    fn default() -> Self {
        DrainLayer { on: true, place: Vec2::new(0.0, 200.0), radius: 16.0, whirlpool: 1.0 }
    }
}

/// What a current does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum CurrentKind {
    /// A push along a line.
    #[default]
    Pump,
    /// A spin round its place.
    Whirlpool,
    /// Wind across the liquid's surface (anywhere).
    Wind,
}

/// A current: a pump, a whirlpool or wind.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct CurrentLayer {
    #[setting(label = "On", simple)]
    pub on: bool,
    #[setting(label = "Kind", simple)]
    pub kind: CurrentKind,
    #[setting(label = "Place", unit = "px", when = "kind!=wind", keyframe)]
    pub place: Vec2,
    /// Which way it pushes (0 is to the right, 90 down); a whirlpool spins one way or the other
    /// by Strength's sign.
    #[setting(label = "Aim", span(-180, 180), unit = "°", when = "kind!=whirlpool", keyframe, pulse)]
    pub aim: f32,
    /// How far a pump pushes along, or how wide a whirlpool is.
    #[setting(label = "Length", range(0, 1e9), span(10, 1500), unit = "px", when = "kind!=wind", keyframe, pulse)]
    pub length: f32,
    #[setting(label = "Width", range(0, 1e9), span(4, 600), unit = "px", when = "kind=pump", keyframe, pulse)]
    pub width: f32,
    #[setting(label = "Strength", span(-4, 4), simple, keyframe, pulse)]
    pub strength: f32,
}

impl Default for CurrentLayer {
    fn default() -> Self {
        CurrentLayer {
            on: true,
            kind: CurrentKind::Pump,
            place: Vec2::new(-200.0, 200.0),
            aim: 0.0,
            length: 400.0,
            width: 80.0,
            strength: 1.0,
        }
    }
}

/// Where a magnet is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum MagnetAt {
    #[default]
    ItsPlace,
    /// On every ball it names: the balls are the magnets.
    Balls,
}

/// A magnet that pulls the liquid, with spikes like a ferrofluid's.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct MagnetLayer {
    #[setting(label = "On", simple)]
    pub on: bool,
    #[setting(label = "At", simple)]
    pub at: MagnetAt,
    #[setting(label = "Balls", when = "at=balls")]
    pub balls: Target,
    #[setting(label = "Place", unit = "px", when = "at=its_place", keyframe)]
    pub place: Vec2,
    #[setting(label = "Strength", span(-4, 4), simple, keyframe, pulse)]
    pub strength: f32,
    #[setting(label = "Reach", range(0, 1e9), span(10, 1500), unit = "px", keyframe, pulse)]
    pub reach: f32,
    /// How many spikes it pulls the liquid into round it (0: a smooth heap).
    #[setting(label = "Spikes", range(0, 64), span(0, 24), keyframe)]
    pub spikes: u32,
    #[setting(label = "Spike size", when = "spikes", range(0, 1), span(0, 1), keyframe, pulse)]
    pub spike_size: f32,
}

impl Default for MagnetLayer {
    fn default() -> Self {
        MagnetLayer {
            on: true,
            at: MagnetAt::ItsPlace,
            balls: Target::Anything,
            place: Vec2::ZERO,
            strength: 1.0,
            reach: 250.0,
            spikes: 0,
            spike_size: 0.5,
        }
    }
}

/// Every force layer of a Fill card, bottom first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Forces {
    pub heats: Vec<HeatLayer>,
    pub drains: Vec<DrainLayer>,
    pub currents: Vec<CurrentLayer>,
    pub magnets: Vec<MagnetLayer>,
}

fn finite_place(p: Vec2) -> Vec2 {
    if p.is_finite() { p } else { Vec2::ZERO }
}

impl Forces {
    pub fn read(card: Option<&ResolvedSlot>) -> Forces {
        let mut out = Forces::default();
        let Some(card) = card else { return out };
        for l in &card.layers {
            let p = card.layer_params(l.id);
            match l.kind.as_str() {
                HEAT => {
                    let s = HeatLayer::from_params(&p);
                    out.heats.push(HeatLayer {
                        place: finite_place(s.place),
                        radius: num(s.radius, 80.0).max(0.0),
                        strength: num(s.strength, 1.0).clamp(-1.0, 1.0),
                        ..s
                    });
                }
                DRAIN => {
                    let s = DrainLayer::from_params(&p);
                    out.drains.push(DrainLayer {
                        place: finite_place(s.place),
                        radius: num(s.radius, 16.0).max(0.0),
                        whirlpool: num(s.whirlpool, 1.0).max(0.0),
                        ..s
                    });
                }
                CURRENT => {
                    let s = CurrentLayer::from_params(&p);
                    out.currents.push(CurrentLayer {
                        place: finite_place(s.place),
                        aim: num(s.aim, 0.0),
                        length: num(s.length, 400.0).max(0.0),
                        width: num(s.width, 80.0).max(0.0),
                        strength: num(s.strength, 1.0),
                        ..s
                    });
                }
                MAGNET => {
                    let s = MagnetLayer::from_params(&p);
                    out.magnets.push(MagnetLayer {
                        place: finite_place(s.place),
                        strength: num(s.strength, 1.0),
                        reach: num(s.reach, 250.0).max(0.0),
                        spikes: s.spikes.min(64),
                        spike_size: num(s.spike_size, 0.5).clamp(0.0, 1.0),
                        ..s
                    });
                }
                _ => {}
            }
        }
        out
    }
}

/// A spout the liquid pours from.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct PourLayer {
    #[setting(label = "Pouring", simple)]
    pub on: bool,
    /// Where it is, from the Liquid thing's place.
    #[setting(label = "Place", unit = "px", keyframe)]
    pub place: Vec2,
    /// Which way it pours (90 is straight down).
    #[setting(label = "Aim", span(-180, 180), unit = "°", keyframe, pulse)]
    pub aim: f32,
    #[setting(label = "Width", range(0, 1e9), span(1, 400), unit = "px", simple, keyframe, pulse)]
    pub width: f32,
    /// How fast it comes out (with Width, how much comes out a second).
    #[setting(label = "Speed", range(0, 1e9), span(0, 2000), unit = "px/s", simple, keyframe, pulse)]
    pub speed: f32,
    /// How much comes out, times what Width and Speed pour (0 pours nothing; above 1 it comes out
    /// thicker than it would flow).
    #[setting(label = "Rate", range(0, 1e9), span(0, 4), unit = "×", keyframe, pulse)]
    pub rate: f32,
    /// How far each drop's way can stray from Aim.
    #[setting(label = "Spread", range(0, 180), span(0, 45), unit = "°", keyframe)]
    pub spread: f32,
    #[setting(paint, label = "Colour", prefix = "colour_", follows = "place|order|time")]
    pub colour: Paint,
    #[setting(label = "Colour strength", range(0, 1e9), span(0, 4), keyframe)]
    pub strength: f32,
}

impl Default for PourLayer {
    fn default() -> Self {
        PourLayer {
            on: true,
            place: Vec2::ZERO,
            aim: 90.0,
            width: 40.0,
            speed: 300.0,
            rate: 1.0,
            spread: 4.0,
            colour: one_colour(WATER),
            strength: 1.0,
        }
    }
}

impl PourLayer {
    /// The pours of a Fill card, bottom first (hidden ones left out by resolve).
    pub fn read_all(card: Option<&ResolvedSlot>) -> Vec<PourLayer> {
        let Some(card) = card else { return Vec::new() };
        card.layers
            .iter()
            .filter(|l| l.kind == POUR)
            .map(|l| {
                let s = PourLayer::from_params(&card.layer_params(l.id));
                let d = PourLayer::default();
                let place = if s.place.is_finite() { s.place } else { Vec2::ZERO };
                PourLayer {
                    place,
                    aim: num(s.aim, d.aim),
                    width: num(s.width, d.width).max(0.0),
                    speed: num(s.speed, d.speed).max(0.0),
                    rate: num(s.rate, d.rate).max(0.0),
                    spread: num(s.spread, d.spread).clamp(0.0, 180.0),
                    strength: num(s.strength, d.strength).max(0.0),
                    ..s
                }
            })
            .collect()
    }
}

/// How heavy the balls are to a liquid.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum Floats {
    /// As heavy as Ball weight (water's Weight is 1): lighter than the liquid floats.
    #[default]
    ByBallWeight,
    /// Always lighter than the liquid: they float high.
    Floats,
    /// Always heavier: they sink.
    Sinks,
    /// Nearly as heavy as the liquid: they float low and bob.
    Bobs,
}

/// Magenta ink.
const INK: Color = Color { r: 230, g: 40, b: 160, a: 255 };
/// Orange paint.
const PAINT: Color = Color { r: 255, g: 140, b: 30, a: 255 };

/// What a liquid does to the balls in it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum ChangesBall {
    #[default]
    Off,
    /// They shrink to nothing and go.
    Dissolves,
    /// They shrink, down to a third of their size.
    Shrinks,
    /// They grow, up to three times their size.
    Grows,
    /// They take its colour.
    TakesItsColour,
}

/// The Meets card.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct MeetsSettings {
    /// Balls push the liquid aside. Off, they pass through it.
    #[setting(label = "Balls push it", simple, group = "With the balls")]
    pub balls_push: bool,
    /// The liquid pushes back: balls float, sink, are slowed and carried. Every liquid then moves
    /// with the physics (a change to any of them restarts it).
    #[setting(label = "Pushes balls", when = "balls_push", simple, group = "With the balls")]
    pub pushes_balls: bool,
    /// Which balls it meets.
    #[setting(label = "Meets balls", when = "balls_push", group = "With the balls")]
    pub target: Target,
    #[setting(label = "Floats or sinks", when = "balls_push and pushes_balls", simple, group = "With the balls")]
    pub floats: Floats,
    /// How heavy a ball is to it (water's Weight is 1): lighter than the liquid floats.
    #[setting(
        label = "Ball weight",
        when = "balls_push and pushes_balls and floats=by_ball_weight",
        range(0.01, 1e9),
        span(0.05, 4),
        unit = "×",
        keyframe,
        pulse,
        group = "With the balls"
    )]
    pub ball_weight: f32,
    /// How much it slows balls moving through it (more in a thicker liquid).
    #[setting(
        label = "Drag",
        when = "balls_push and pushes_balls",
        range(0, 1e9),
        span(0, 4),
        keyframe,
        pulse,
        group = "With the balls"
    )]
    pub drag: f32,
    /// How much its currents and waves move balls along.
    #[setting(
        label = "Carries balls",
        when = "balls_push and pushes_balls",
        range(0, 1e9),
        span(0, 2),
        keyframe,
        pulse,
        group = "With the balls"
    )]
    pub carries: f32,
    /// How much a ball landing hard throws up (1 as it comes; more throws it higher).
    #[setting(
        label = "Splash size",
        when = "balls_push",
        range(0, 1e9),
        span(0, 4),
        keyframe,
        pulse,
        group = "With the balls"
    )]
    pub splash: f32,
    /// How fast a ball must go in to splash.
    #[setting(
        label = "Splashes from",
        when = "balls_push",
        range(0, 1e9),
        span(0, 3000),
        unit = "px/s",
        keyframe,
        group = "With the balls"
    )]
    pub splash_from: f32,
    /// The liquid clings to balls and drips off as they leave.
    #[setting(
        label = "Sticks to balls",
        when = "balls_push",
        range(0, 1e9),
        span(0, 1),
        keyframe,
        pulse,
        group = "With the balls"
    )]
    pub sticks: f32,
    /// Moving balls pull the liquid behind them along: wakes and waves.
    #[setting(
        label = "Wakes",
        when = "balls_push",
        range(0, 1e9),
        span(0, 4),
        keyframe,
        pulse,
        group = "With the balls"
    )]
    pub wakes: f32,
    /// Balls colour the liquid they touch with the Ink colour.
    #[setting(
        label = "Balls drop ink",
        when = "balls_push",
        range(0, 1e9),
        span(0, 1),
        keyframe,
        pulse,
        group = "With the balls"
    )]
    pub ink: f32,
    #[setting(label = "Ink colour", when = "balls_push and ink", keyframe, group = "With the balls")]
    pub ink_colour: Color,
    /// Balls come out tinted with the colour of the liquid they were in.
    #[setting(
        label = "Balls take its colour",
        when = "balls_push and pushes_balls",
        range(0, 1e9),
        span(0, 1),
        keyframe,
        pulse,
        group = "With the balls"
    )]
    pub take: f32,
    #[setting(label = "Changes the ball", when = "balls_push and pushes_balls", group = "With the balls")]
    pub changes: ChangesBall,
    #[setting(
        label = "Change speed",
        when = "balls_push and pushes_balls and changes!=off",
        range(0, 1e9),
        span(0, 2),
        unit = "/s",
        keyframe,
        pulse,
        group = "With the balls"
    )]
    pub change_speed: f32,
    /// The balls warm the liquid they touch.
    #[setting(
        label = "Balls heat it",
        when = "balls_push",
        range(0, 1e9),
        span(0, 1),
        keyframe,
        pulse,
        group = "With the balls"
    )]
    pub balls_heat: f32,
    /// Shapes colour the liquid touching them with the Paint colour.
    #[setting(label = "Shapes paint it", range(0, 1e9), span(0, 1), keyframe, pulse, group = "With the shapes")]
    pub paint: f32,
    #[setting(label = "Paint colour", when = "paint", keyframe, group = "With the shapes")]
    pub paint_colour: Color,
    #[setting(label = "Painting shapes", when = "paint", group = "With the shapes")]
    pub painting: Target,
}

impl Default for MeetsSettings {
    fn default() -> Self {
        MeetsSettings {
            balls_push: true,
            pushes_balls: true,
            target: Target::Anything,
            floats: Floats::ByBallWeight,
            ball_weight: 0.5,
            drag: 1.0,
            carries: 1.0,
            splash: 1.0,
            splash_from: 400.0,
            sticks: 0.0,
            wakes: 0.0,
            ink: 0.0,
            ink_colour: INK,
            take: 0.0,
            changes: ChangesBall::Off,
            change_speed: 0.5,
            balls_heat: 0.0,
            paint: 0.0,
            paint_colour: PAINT,
            painting: Target::Anything,
        }
    }
}

impl MeetsSettings {
    /// The card's settings, any number that isn't one read as its default.
    pub fn read(card: Option<&ResolvedSlot>) -> MeetsSettings {
        let s = card.map(|c| MeetsSettings::from_params(&c.params)).unwrap_or_default();
        MeetsSettings::fixed(s)
    }

    /// The settings of a saved card (`params` as saved: Pushes balls is never keyframed).
    pub fn of_saved(params: &bpe_core::Params) -> MeetsSettings {
        MeetsSettings::fixed(MeetsSettings::from_params(params))
    }

    fn fixed(s: MeetsSettings) -> MeetsSettings {
        let d = MeetsSettings::default();
        MeetsSettings {
            ball_weight: num(s.ball_weight, d.ball_weight).max(0.01),
            drag: num(s.drag, d.drag).max(0.0),
            carries: num(s.carries, d.carries).max(0.0),
            splash: num(s.splash, d.splash).max(0.0),
            splash_from: num(s.splash_from, d.splash_from).max(0.0),
            sticks: num(s.sticks, d.sticks).max(0.0),
            wakes: num(s.wakes, d.wakes).max(0.0),
            ink: num(s.ink, d.ink).max(0.0),
            take: num(s.take, d.take).max(0.0),
            change_speed: num(s.change_speed, d.change_speed).max(0.0),
            balls_heat: num(s.balls_heat, 0.0).max(0.0),
            paint: num(s.paint, 0.0).max(0.0),
            ..s
        }
    }

    /// How fast it changes the balls in it: shrink rate a second (below 0 it grows them) and
    /// whether they dissolve; nothing unless it pushes balls.
    pub fn change(&self) -> (f32, bool) {
        if !self.two_way() {
            return (0.0, false);
        }
        match self.changes {
            ChangesBall::Dissolves => (self.change_speed, true),
            ChangesBall::Shrinks => (self.change_speed, false),
            ChangesBall::Grows => (-self.change_speed, false),
            ChangesBall::Off | ChangesBall::TakesItsColour => (0.0, false),
        }
    }

    /// Balls take its colour, lifted by Changes the ball: Takes its colour.
    pub fn take_now(&self) -> f32 {
        if self.changes == ChangesBall::TakesItsColour { self.take.max(self.change_speed) } else { self.take }
    }

    /// Whether the liquid pushes the balls back (it then steps with the physics).
    pub fn two_way(&self) -> bool {
        self.balls_push && self.pushes_balls
    }

    /// A ball's density to a liquid of `weight`.
    pub fn ball_density(&self, weight: f32) -> f32 {
        match self.floats {
            Floats::ByBallWeight => self.ball_weight,
            Floats::Floats => 0.4 * weight,
            Floats::Sinks => 2.5 * weight,
            Floats::Bobs => 0.85 * weight,
        }
        .max(0.01)
    }
}

/// What happens where two liquids meet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum Meeting {
    /// Their colours run together (and their thickness and weight).
    #[default]
    Mix,
    /// They stay apart, the lighter on top.
    Layer,
    /// Where they touch they turn into a third liquid.
    React,
}

/// The mix style a rule uses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum RuleStyle {
    /// This liquid's own Mix style.
    #[default]
    AsThisLiquids,
    Paint,
    Light,
    Blend,
    Marble,
    KeepApart,
}

/// A rule for meeting another liquid.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct OtherLiquid {
    /// The liquid (or liquids) it names.
    #[setting(label = "Liquid", simple)]
    pub liquid: Target,
    #[setting(label = "Meeting", simple)]
    pub meeting: Meeting,
    #[setting(label = "Mix style", when = "meeting=mix")]
    pub style: RuleStyle,
    /// Use Mix speed here instead of this liquid's.
    #[setting(label = "Own speed", when = "meeting=mix")]
    pub own_speed: bool,
    #[setting(label = "Mix speed", when = "meeting=mix and own_speed", range(0, 1), span(0, 1))]
    pub speed: f32,
    /// The Liquid thing they turn into where they touch.
    #[setting(label = "Makes", when = "meeting=react")]
    pub makes: Target,
    #[setting(label = "Reaction speed", when = "meeting=react", range(0, 1), span(0, 1))]
    pub react_speed: f32,
    /// How much the reaction heats what it makes.
    #[setting(label = "Heat", when = "meeting=react", range(0, 1), span(0, 1))]
    pub heat: f32,
    /// How hard what it makes fizzes up.
    #[setting(label = "Fizz", when = "meeting=react", range(0, 1), span(0, 1))]
    pub fizz: f32,
}

impl Default for OtherLiquid {
    fn default() -> Self {
        OtherLiquid {
            liquid: Target::Kind(super::KIND.to_string()),
            meeting: Meeting::Mix,
            style: RuleStyle::AsThisLiquids,
            own_speed: false,
            speed: 0.5,
            makes: Target::Kind(super::KIND.to_string()),
            react_speed: 0.3,
            heat: 0.0,
            fizz: 0.0,
        }
    }
}

impl OtherLiquid {
    pub fn read_all(card: Option<&ResolvedSlot>) -> Vec<OtherLiquid> {
        let Some(card) = card else { return Vec::new() };
        card.layers
            .iter()
            .filter(|l| l.kind == OTHER)
            .map(|l| {
                let s = OtherLiquid::from_params(&card.layer_params(l.id));
                OtherLiquid {
                    speed: num(s.speed, 0.5).clamp(0.0, 1.0),
                    react_speed: num(s.react_speed, 0.3).clamp(0.0, 1.0),
                    heat: num(s.heat, 0.0).clamp(0.0, 1.0),
                    fizz: num(s.fizz, 0.0).clamp(0.0, 1.0),
                    ..s
                }
            })
            .collect()
    }
}

/// Where a Liquid's colour comes from in the picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum ColourFrom {
    /// Each drop's own colour (from its Fill or Pour, as mixed).
    #[default]
    ItsOwn,
    /// One colour over all of it.
    OneColour,
}

/// How colours mix where drops meet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum MixStyle {
    /// Like paint: blue and yellow make green.
    Paint,
    /// Like light: red and green make yellow, brighter.
    Light,
    /// A straight, even blend.
    #[default]
    Blend,
    /// Never blends: only swirls into streaks.
    Marble,
    /// No colour passes between drops.
    KeepApart,
}

impl MixStyle {
    pub fn code(self) -> u32 {
        use bpe_gfx::liquid_sim as l;
        match self {
            MixStyle::Paint => l::STYLE_PAINT,
            MixStyle::Light => l::STYLE_LIGHT,
            MixStyle::Blend => l::STYLE_BLEND,
            MixStyle::Marble => l::STYLE_MARBLE,
            MixStyle::KeepApart => l::STYLE_KEEP_APART,
        }
    }
}

/// When drops mix.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum MixesWhen {
    #[default]
    Always,
    /// Only where it is stirred up (drops moving past each other).
    OnlyWhenStirred,
    /// Only where a ball has gone through it.
    OnlyOnHits,
}

impl MixesWhen {
    pub fn code(self) -> u32 {
        use bpe_gfx::liquid_sim as l;
        match self {
            MixesWhen::Always => l::WHEN_ALWAYS,
            MixesWhen::OnlyWhenStirred => l::WHEN_STIRRED,
            MixesWhen::OnlyOnHits => l::WHEN_HITS,
        }
    }
}

/// The Look card.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct LookSettings {
    #[setting(label = "Colour from", simple, group = "Its colour")]
    pub colour_from: ColourFrom,
    #[setting(label = "Colour", when = "colour_from=one_colour", group = "Its colour")]
    pub colour: Color,
    #[setting(label = "Mix style", simple, group = "Colour mixing")]
    pub mix_style: MixStyle,
    /// How fast colours run together where drops meet: 0 never, 1 at once.
    #[setting(label = "Mix speed", range(0, 1), span(0, 1), simple, group = "Colour mixing")]
    pub mix_speed: f32,
    #[setting(label = "Mixes when", group = "Colour mixing")]
    pub mixes_when: MixesWhen,
    /// How sharp the swirls of colour stay: 0 soft, 1 crisp streaks.
    #[setting(label = "Swirl detail", range(0, 1), span(0, 1), keyframe, group = "Colour mixing")]
    pub swirl_detail: f32,
    /// How fast its colours drift to Fades to (0: never).
    #[setting(label = "Fades", range(0, 1e9), span(0, 1), unit = "/s", group = "Colour mixing")]
    pub fades: f32,
    #[setting(label = "Fades to", when = "fades", group = "Colour mixing")]
    pub fades_to: Color,
    /// Where the surface is cut from the drops: lower is fuller, higher leaner.
    #[setting(label = "Edge", range(0.01, 10), span(0.1, 1), keyframe, group = "Surface")]
    pub level: f32,
    #[setting(label = "Edge softness", range(0, 1e9), span(0, 10), unit = "px", keyframe, group = "Surface")]
    pub softness: f32,
    /// How far in from the edge the surface rounds over.
    #[setting(label = "Roundness", range(0, 1e9), span(0, 40), unit = "px", keyframe, group = "Surface")]
    pub roundness: f32,
    #[setting(label = "Shine", range(0, 1), span(0, 1), keyframe, group = "Surface")]
    pub shine: f32,
    #[setting(label = "Light from", span(-180, 180), unit = "°", keyframe, group = "Surface")]
    pub light_angle: f32,
    #[setting(label = "Opacity", range(0, 1), span(0, 1), keyframe, group = "Surface")]
    pub opacity: f32,
    /// Drawn over the balls instead of behind them.
    #[setting(label = "In front of the balls", group = "Surface")]
    pub in_front: bool,
}

impl Default for LookSettings {
    fn default() -> Self {
        LookSettings {
            colour_from: ColourFrom::ItsOwn,
            colour: WATER,
            mix_style: MixStyle::Blend,
            mix_speed: 0.3,
            mixes_when: MixesWhen::Always,
            swirl_detail: 0.5,
            fades: 0.0,
            fades_to: Color { r: 255, g: 255, b: 255, a: 255 },
            level: 0.5,
            softness: 1.5,
            roundness: 6.0,
            shine: 0.6,
            light_angle: -45.0,
            opacity: 1.0,
            in_front: false,
        }
    }
}

impl LookSettings {
    pub fn read(card: Option<&ResolvedSlot>) -> LookSettings {
        let s = card.map(|c| LookSettings::from_params(&c.params)).unwrap_or_default();
        let d = LookSettings::default();
        LookSettings {
            mix_speed: num(s.mix_speed, d.mix_speed).clamp(0.0, 1.0),
            swirl_detail: num(s.swirl_detail, d.swirl_detail).clamp(0.0, 1.0),
            fades: num(s.fades, 0.0).max(0.0),
            level: num(s.level, d.level).clamp(0.01, 10.0),
            softness: num(s.softness, d.softness).max(0.0),
            roundness: num(s.roundness, d.roundness).max(0.0),
            shine: num(s.shine, d.shine).clamp(0.0, 1.0),
            light_angle: num(s.light_angle, d.light_angle),
            opacity: num(s.opacity, d.opacity).clamp(0.0, 1.0),
            ..s
        }
    }

    /// What of it the liquid's stepping reads (the rest only draws).
    pub fn stepped(&self) -> (MixStyle, u32, MixesWhen, u32, Color) {
        (self.mix_style, self.mix_speed.to_bits(), self.mixes_when, self.fades.to_bits(), self.fades_to)
    }
}

macro_rules! card {
    ($name:ident, $id:expr, $label:expr, $help:expr, $slot:expr, $defs:expr) => {
        pub struct $name;

        impl ModuleKind for $name {
            fn id(&self) -> &'static str {
                $id
            }
            fn name(&self) -> &'static str {
                $label
            }
            fn help(&self) -> &'static str {
                $help
            }
            fn slot(&self) -> SlotId {
                $slot
            }
            fn settings(&self) -> &'static [SettingDef] {
                $defs
            }
            fn fits(&self, thing_kind: &str) -> bool {
                thing_kind == super::KIND
            }
        }
    };
}

card!(
    FillCard,
    FILL,
    "Fill",
    "Where the liquid comes from: filled at the start, and poured from spouts.",
    SlotId::Motion,
    FillSettings::defs()
);
card!(
    MeetsCard,
    MEETS,
    "Meets",
    "What the liquid does with the balls and with other liquids.",
    SlotId::Bounce,
    MeetsSettings::defs()
);
card!(
    LookCard,
    LOOK,
    "Look",
    "How the liquid looks: its colour, how colours mix, its surface.",
    SlotId::Look,
    LookSettings::defs()
);

/// The Pour and Other liquid layer kinds.
pub fn kinds() -> Vec<Kind> {
    let list = |id, name, help, card, settings| Kind {
        id,
        name,
        help,
        section: LayerSection::Force,
        repeats: true,
        base: false,
        card,
        takes_subs: false,
        settings,
        dynamic: crate::modules::look::layers::no_more,
    };
    vec![
        list(
            POUR,
            "Pour",
            "A spout the liquid pours from: where, which way, how wide and how fast.",
            FILL,
            PourLayer::defs,
        ),
        list(
            OTHER,
            "Other liquid",
            "What happens where this liquid meets another one: they mix, stay in layers or react into a third.",
            MEETS,
            OtherLiquid::defs,
        ),
        list(HEAT, "Heat", "A hot (or cold) place that heats the liquid near it.", FILL, HeatLayer::defs),
        list(DRAIN, "Drain", "A hole the liquid drains away through, swirling round it.", FILL, DrainLayer::defs),
        list(CURRENT, "Current", "A pump, a whirlpool or wind that moves the liquid.", FILL, CurrentLayer::defs),
        list(
            MAGNET,
            "Magnet",
            "Pulls the liquid in, with spikes like a ferrofluid's; a ball can be the magnet.",
            FILL,
            MagnetLayer::defs,
        ),
    ]
}
