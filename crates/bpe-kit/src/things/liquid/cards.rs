//! A Liquid thing's cards: **Fill** (the Motion slot, with its Pour layers), **Meets** (the Bounce
//! slot, with its Other liquid rules) and **Look**.

use bpe_core::{Choice, Color, LayerSection, ModuleKind, ResolvedSlot, SettingDef, Settings, SlotId, Target};
use glam::Vec2;

use crate::modules::look::layers::Kind;
use crate::modules::look::surface::SurfaceKind;
use crate::paint::Paint;

use super::num;

pub const FILL: &str = "liquid.fill";
pub const POUR: &str = "liquid.pour";
pub const MEETS: &str = "liquid.meets";
pub const OTHER: &str = "liquid.other";
pub const LOOK: &str = "liquid.look";

/// Water's blue.
const WATER: Color = Color { r: 47, g: 127, b: 224, a: 255 };

fn one_colour(c: Color) -> Paint {
    Paint::solid(c)
}

/// The Fill card: liquid there from the start.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct FillSettings {
    /// Fill its shape (or the scene) up to Level at the start.
    #[setting(label = "Fill at the start", simple, group = "Fill")]
    pub fill: bool,
    /// How full: 0 empty, 1 to the top. Raising it adds liquid at the top, lowering it takes it away.
    #[setting(label = "Level", when = "fill", range(0, 1), span(0, 1), simple, keyframe, pulse, group = "Fill")]
    pub level: f32,
    #[setting(paint, label = "Colour", group = "Fill", prefix = "colour_", follows = "place|order|time")]
    pub colour: Paint,
    /// How much its colour takes over when it mixes: a strong colour wins, a weak one only tints.
    #[setting(label = "Colour strength", range(0, 1e9), span(0, 4), keyframe, group = "Fill")]
    pub strength: f32,
    /// The most drops this liquid keeps: past it the oldest go.
    #[setting(label = "Most drops", range(1, 800000), span(1000, 800000), group = "Fill")]
    pub most: u32,
}

impl Default for FillSettings {
    fn default() -> Self {
        FillSettings { fill: true, level: 0.35, colour: one_colour(WATER), strength: 1.0, most: 400_000 }
    }
}

impl FillSettings {
    pub fn read(card: Option<&ResolvedSlot>) -> FillSettings {
        let s = card.map(|c| FillSettings::from_params(&c.params)).unwrap_or_default();
        FillSettings { level: num(s.level, 0.35).clamp(0.0, 1.0), strength: num(s.strength, 1.0).max(0.0), ..s }
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
            ..s
        }
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
}

impl Default for OtherLiquid {
    fn default() -> Self {
        OtherLiquid {
            liquid: Target::Kind(super::KIND.to_string()),
            meeting: Meeting::Mix,
            style: RuleStyle::AsThisLiquids,
            own_speed: false,
            speed: 0.5,
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
                OtherLiquid { speed: num(s.speed, 0.5).clamp(0.0, 1.0), ..s }
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
    /// Where the surface is cut from the drops: lower is fuller, higher leaner.
    #[setting(label = "Edge", range(0.01, 10), span(0.1, 1), keyframe, group = "Shape of the surface")]
    pub level: f32,
    #[setting(
        label = "Edge softness",
        range(0, 1e9),
        span(0, 10),
        unit = "px",
        keyframe,
        group = "Shape of the surface"
    )]
    pub softness: f32,
    /// How far in from the edge the surface rounds over.
    #[setting(label = "Roundness", range(0, 1e9), span(0, 40), unit = "px", keyframe, group = "Shape of the surface")]
    pub roundness: f32,
    /// How far its own light's highlight shines (only without a Lighting card, or at Lit 0).
    #[setting(label = "Shine", range(0, 1), span(0, 1), keyframe, pulse, group = "Its own light")]
    pub shine: f32,
    /// The way its own light comes from, on the screen (only without a Lighting card, or at Lit 0).
    #[setting(label = "Light from", span(-180, 180), unit = "°", keyframe, group = "Its own light")]
    pub light_angle: f32,
    #[setting(label = "Opacity", range(0, 1), span(0, 1), keyframe, pulse, group = "Shape of the surface")]
    pub opacity: f32,
    /// Drawn over the balls instead of behind them.
    #[setting(label = "In front of the balls", group = "Shape of the surface")]
    pub in_front: bool,
    // ---- Liquid part 3: looking real ----
    /// What its surface is like, as a ball's: picking one sets the numbers under it. With a
    /// Lighting card the real lights shade it so; without one it takes the nearest of its own
    /// pretend looks.
    #[setting(label = "Surface", group = "Surface", simple)]
    pub surface: SurfaceKind,
    /// How hard the surface hides the drops it is made of: higher is smoother and rounder, lower
    /// shows smaller drops and finer splashes.
    #[setting(label = "Smoothing", range(0, 1e9), span(0, 1), keyframe, group = "Shape of the surface")]
    pub smoothing: f32,
    /// How far in you see before its colour takes over: 0 is solid, more is clearer.
    #[setting(label = "Clarity", range(0, 1e9), span(0, 600), unit = "px", simple, keyframe, pulse, group = "Inside")]
    pub clarity: f32,
    /// What its colour turns to deep down (times its colour): darker is deeper water.
    #[setting(label = "Deep colour", when = "clarity", group = "Inside")]
    pub deep: Color,
    /// Soft bright edges where light soaks in (milk, jelly).
    #[setting(label = "Light soaks in", range(0, 1e9), span(0, 1), keyframe, pulse, group = "Inside")]
    pub soaks_in: f32,
    /// Its own light round its edge (only without a Lighting card, or at Lit 0).
    #[setting(label = "Rim light", range(0, 1), span(0, 1), keyframe, group = "Its own light")]
    pub own_rim: f32,
    /// How much of the sky it mirrors (only without a Lighting card, or at Lit 0).
    #[setting(label = "Reflects the sky", range(0, 1e9), span(0, 1), keyframe, group = "Its own light")]
    pub sky_reflects: f32,
    #[setting(label = "Sky top", group = "Its own light")]
    pub sky_top: Color,
    #[setting(label = "Sky bottom", group = "Its own light")]
    pub sky_bottom: Color,
    /// How far it bends what is behind it (only without a Lighting card, or at Lit 0, and only
    /// where it is clear or glass).
    #[setting(label = "Bend", range(0, 1e9), span(0, 60), unit = "px", keyframe, pulse, group = "Its own light")]
    pub bend: f32,
    /// How much glass takes on its colour.
    #[setting(label = "Tint", range(0, 1), span(0, 1), keyframe, group = "Its own light")]
    pub tint: f32,
    /// Splits what it bends into a rainbow.
    #[setting(label = "Rainbow split", when = "bend", range(0, 1), span(0, 1), keyframe, group = "Its own light")]
    pub split: f32,
    /// Fine ripples on its surface, smooth to choppy.
    #[setting(label = "Ripples", range(0, 1e9), span(0, 1), keyframe, pulse, group = "On its surface")]
    pub ripples: f32,
    #[setting(
        label = "Ripple size",
        when = "ripples",
        range(1, 1e9),
        span(2, 80),
        unit = "px",
        keyframe,
        group = "On its surface"
    )]
    pub ripple_size: f32,
    /// Rainbow colours that slide over it (soap, oil).
    #[setting(label = "Thin film", range(0, 1), span(0, 1), keyframe, pulse, group = "On its surface")]
    pub thin_film: f32,
    #[setting(
        label = "Film size",
        when = "thin_film",
        range(1, 1e9),
        span(5, 200),
        unit = "px",
        keyframe,
        group = "On its surface"
    )]
    pub film_size: f32,
    /// A line just inside its edge.
    #[setting(label = "Outline", range(0, 1e9), span(0, 10), unit = "px", keyframe, pulse, group = "On its surface")]
    pub outline: f32,
    #[setting(label = "Outline colour", when = "outline", group = "On its surface")]
    pub outline_colour: Color,
    /// Light it gives off (with a Lighting card it lights the things near it).
    #[setting(label = "Gives off light", range(0, 1e9), span(0, 4), keyframe, pulse, group = "On its surface")]
    pub glow: f32,
    /// Shows where colours are mixing.
    #[setting(label = "Mixing shows", range(0, 1), span(0, 1), keyframe, pulse, group = "Colour mixing")]
    pub mixing_shows: f32,
    #[setting(label = "Shows as", when = "mixing_shows", group = "Colour mixing")]
    pub shows_as: ShowsAs,
    /// How much the scene's lights light it: 0 keeps its own light.
    #[setting(label = "Lit", range(0, 1e9), span(0, 1), keyframe, group = "With lighting")]
    pub lit: f32,
    /// Whether it casts shadows (a see-through Surface lets its share of light through).
    #[setting(label = "Blocks light", group = "With lighting")]
    pub blocks_light: bool,
}

/// What Mixing shows draws where colours are mixing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Choice)]
pub enum ShowsAs {
    /// Sparkles.
    #[default]
    Shimmer,
    /// A soft glow (with a Lighting card it lights the scene).
    Glow,
    /// White froth.
    Foam,
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
            level: 0.5,
            softness: 1.5,
            roundness: 6.0,
            shine: 0.6,
            light_angle: -45.0,
            opacity: 1.0,
            in_front: false,
            surface: SurfaceKind::Glass,
            smoothing: 0.25,
            clarity: 160.0,
            deep: Color::rgb(110, 140, 170),
            soaks_in: 0.0,
            own_rim: 0.0,
            sky_reflects: 0.3,
            sky_top: Color::rgb(225, 235, 255),
            sky_bottom: Color::rgb(40, 40, 50),
            bend: 14.0,
            tint: 0.3,
            split: 0.0,
            ripples: 0.3,
            ripple_size: 18.0,
            thin_film: 0.0,
            film_size: 40.0,
            outline: 0.0,
            outline_colour: Color::rgb(255, 255, 255),
            glow: 0.0,
            mixing_shows: 0.0,
            shows_as: ShowsAs::Shimmer,
            lit: 1.0,
            blocks_light: true,
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
            level: num(s.level, d.level).clamp(0.01, 10.0),
            softness: num(s.softness, d.softness).max(0.0),
            roundness: num(s.roundness, d.roundness).max(0.0),
            shine: num(s.shine, d.shine).clamp(0.0, 1.0),
            light_angle: num(s.light_angle, d.light_angle),
            opacity: num(s.opacity, d.opacity).clamp(0.0, 1.0),
            smoothing: num(s.smoothing, d.smoothing).max(0.0),
            clarity: num(s.clarity, d.clarity).max(0.0),
            soaks_in: num(s.soaks_in, d.soaks_in).max(0.0),
            own_rim: num(s.own_rim, d.own_rim).clamp(0.0, 1.0),
            sky_reflects: num(s.sky_reflects, d.sky_reflects).max(0.0),
            bend: num(s.bend, d.bend).max(0.0),
            tint: num(s.tint, d.tint).clamp(0.0, 1.0),
            split: num(s.split, d.split).clamp(0.0, 1.0),
            ripples: num(s.ripples, d.ripples).max(0.0),
            ripple_size: num(s.ripple_size, d.ripple_size).max(1.0),
            thin_film: num(s.thin_film, d.thin_film).clamp(0.0, 1.0),
            film_size: num(s.film_size, d.film_size).max(1.0),
            outline: num(s.outline, d.outline).max(0.0),
            glow: num(s.glow, d.glow).max(0.0),
            mixing_shows: num(s.mixing_shows, d.mixing_shows).clamp(0.0, 1.0),
            lit: num(s.lit, d.lit).max(0.0),
            ..s
        }
    }

    /// What of it the liquid's stepping reads (the rest only draws).
    pub fn stepped(&self) -> (MixStyle, u32, MixesWhen) {
        (self.mix_style, self.mix_speed.to_bits(), self.mixes_when)
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
    "How the liquid looks: its colour, how colours mix, its surface, and layers of foam, bubbles and more.",
    SlotId::Look,
    super::look::look_defs()
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
            "What happens where this liquid meets another one: they mix or stay in layers.",
            MEETS,
            OtherLiquid::defs,
        ),
    ]
    .into_iter()
    .chain(super::look::kinds())
    .collect()
}
