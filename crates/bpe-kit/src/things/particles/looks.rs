//! The starting looks the Add thing menu's **Particles** heading offers: each adds a Particles
//! thing whose Source, forces, Meets things card and Look layers are filled in for that look.
//! They only fill settings when the owner picks one; there are no built-in scenes.

use bpe_core::{Color, EditError, Gradient, Mix, Params, Registry, Scene, Settings, SlotId, ThingId, layered_key};

use super::forces::{AIR, FollowAirForce, WANDER, WIND, WanderForce, WindForce};
use super::forces::{DRAG, DragForce, GRAVITY, GravityForce, RISE, RiseForce, SWAY, SWIRL, SwayForce, SwirlForce};
use super::liquid::{LIQUID, LiquidLayer, params_of};
use super::look::{
    ColourFrom, FLAME, FlameLayer, GLITTER, GlitterLayer, MoreShape, Only, PILES, PilesLayer, SHAPES, SMOKE, SPRITES,
    STREAKS, SUB_GLOW, SUB_LIGHT, ShapesLayer, SmokeLayer, SpritesLayer, StreaksLayer, WEB, WebLayer,
};
use super::marks::{MARKS, MarksLayer};
use super::meets::{AtBalls, AtEdges, AtShapes, MeetsSettings};
use super::modifiers::{Mirror, MirrorRound, Modifiers};
use super::trail::{TRAILS, TrailsLayer};
use super::{AreaShape, BornFrom, Edge, Emit, FedWhen, Heading, KIND, ParticlesSettings, RidesOn};
use crate::modules::look::layers::gives_light::GivesLight;
use crate::modules::look::layers::particles::{ParticleFade, ShapeMadeOf, SizeOverLife};
use crate::modules::look::shape::ShapeSubGlow;
use crate::modules::look::surface::SurfaceKind;
use crate::paint::{Follow, Paint};

/// A starting look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    /// A plain Particles thing: one Sprites layer and no forces.
    Empty,
    Rain,
    Snow,
    Hail,
    BlossomPetals,
    Sandstorm,
    FireOnABall,
    SparkFountain,
    EmbersAndAsh,
    Fireflies,
    Confetti,
    RainbowSmokeTrail,
    InkInWater,
    // Part 2 and part 5.
    DustClouds,
    ThickFog,
    Glitter,
    WebOfLight,
    // Part 6 (Liquid splashes is part 4's, drawn as a liquid since part 6).
    LightStreams,
    Kaleidoscope,
    ColourSplitDust,
    LiquidMetal,
    PaintDrips,
    PixelGlitch,
    // Part 3 (`looks_motion.rs`).
    Galaxy,
    FlowLines,
    FormAWord,
    Flock,
    Shockwaves,
    CometTail,
    BlackHole,
    IronFilings,
    BeatGrid,
    Moire,
    Spirograph,
    FireAndSmoke,
    Bubbles,
    SandThatPilesUp,
    FallingLeaves,
    WaterfallAndMist,
    LiquidSplashes,
    Fireworks,
    StickyDust,
}

/// The menu's groups of looks, in order, after Empty.
pub const GROUPS: [(&str, &[Look]); 4] = [
    (
        "Weather and nature",
        &[
            Look::Rain,
            Look::Snow,
            Look::Hail,
            Look::BlossomPetals,
            Look::Sandstorm,
            Look::DustClouds,
            Look::Bubbles,
            Look::SandThatPilesUp,
            Look::ThickFog,
            Look::FallingLeaves,
            Look::WaterfallAndMist,
            Look::LiquidSplashes,
            Look::Flock,
        ],
    ),
    (
        "Fire and energy",
        &[
            Look::FireOnABall,
            Look::FireAndSmoke,
            Look::SparkFountain,
            Look::EmbersAndAsh,
            Look::Fireworks,
            Look::Shockwaves,
        ],
    ),
    (
        "Space and sparkle",
        &[
            Look::Fireflies,
            Look::Confetti,
            Look::Glitter,
            Look::StickyDust,
            Look::Galaxy,
            Look::CometTail,
            Look::BlackHole,
            Look::IronFilings,
        ],
    ),
    (
        "Experimental and psychedelic",
        &[
            Look::RainbowSmokeTrail,
            Look::InkInWater,
            Look::WebOfLight,
            Look::PixelGlitch,
            Look::FlowLines,
            Look::FormAWord,
            Look::BeatGrid,
            Look::Moire,
            Look::Spirograph,
            Look::LightStreams,
            Look::Kaleidoscope,
            Look::ColourSplitDust,
            Look::LiquidMetal,
            Look::PaintDrips,
        ],
    ),
];

impl Look {
    /// Every look, Empty first.
    pub fn all() -> Vec<Look> {
        std::iter::once(Look::Empty).chain(GROUPS.iter().flat_map(|(_, l)| l.iter().copied())).collect()
    }

    /// Its name in the menu (and the new thing's name).
    pub fn name(self) -> &'static str {
        match self {
            Look::Empty => "Particles",
            Look::Rain => "Rain",
            Look::Snow => "Snow",
            Look::Hail => "Hail",
            Look::BlossomPetals => "Blossom petals",
            Look::Sandstorm => "Sandstorm",
            Look::FireOnABall => "Fire on a ball",
            Look::SparkFountain => "Spark fountain",
            Look::EmbersAndAsh => "Embers and ash",
            Look::Fireflies => "Fireflies",
            Look::Confetti => "Confetti",
            Look::RainbowSmokeTrail => "Rainbow smoke trail",
            Look::InkInWater => "Ink in water",
            Look::DustClouds => "Dust clouds",
            Look::ThickFog => "Thick fog",
            Look::Glitter => "Glitter",
            Look::WebOfLight => "Web of light",
            Look::LightStreams => "Light streams",
            Look::Kaleidoscope => "Kaleidoscope",
            Look::ColourSplitDust => "Colour split dust",
            Look::LiquidMetal => "Liquid metal",
            Look::PaintDrips => "Paint drips",
            Look::PixelGlitch => "Pixel glitch",
            Look::Galaxy => "Galaxy round a ball",
            Look::FlowLines => "Flow lines",
            Look::FormAWord => "Form a word",
            Look::Flock => "Flock",
            Look::Shockwaves => "Shockwaves",
            Look::CometTail => "Comet tail",
            Look::BlackHole => "Black hole",
            Look::IronFilings => "Iron filings",
            Look::BeatGrid => "Beat grid",
            Look::Moire => "Moiré",
            Look::Spirograph => "Spirograph",
            Look::FireAndSmoke => "Fire and smoke",
            Look::Bubbles => "Bubbles",
            Look::SandThatPilesUp => "Sand that piles up",
            Look::FallingLeaves => "Falling leaves",
            Look::WaterfallAndMist => "Waterfall and mist",
            Look::LiquidSplashes => "Liquid splashes",
            Look::Fireworks => "Fireworks",
            Look::StickyDust => "Sticky dust",
        }
    }

    /// The menu's one line of help.
    pub fn help(self) -> &'static str {
        match self {
            Look::Empty => "Particles with nothing set up yet: one layer of dots and no forces.",
            Look::Rain => "Streaks of rain falling over the whole picture, blown by the wind.",
            Look::Snow => "Soft flakes drifting down, swaying as they fall.",
            Look::Hail => "Hailstones that bounce off shapes and balls.",
            Look::BlossomPetals => "Pink petals fluttering down on a breeze.",
            Look::Sandstorm => "Sand blown across the picture in gusts and swirls.",
            Look::FireOnABall => "Flames rising off each ball (inside a group of balls) or off a point.",
            Look::SparkFountain => "A fountain of sparks that fall and bounce off what they meet.",
            Look::EmbersAndAsh => "Glowing embers and grey ash rising from the bottom of the picture.",
            Look::Fireflies => "A crowd of fireflies wandering and blinking.",
            Look::Confetti => "Bursts of confetti, flipping over as it falls.",
            Look::RainbowSmokeTrail => "Rainbow smoke left behind each ball (inside a group of balls) or a point.",
            Look::InkInWater => "Clouds of ink swirling out, bursting again and again.",
            Look::DustClouds => "Puffs of dust thrown up where balls land, swept along by the air they stir.",
            Look::ThickFog => {
                "A bank of fog that drifts with the air, swirls round the balls and slows them as they wade through."
            }
            Look::Glitter => "Glitter drifting over the whole picture, its facets flashing as they turn.",
            Look::WebOfLight => "Drifting points of light joined by lines to their neighbours and the balls.",
            Look::PixelGlitch => {
                "Square pixels on a grid flying off each ball (inside a group of balls) or a point, torn by glitches."
            }
            Look::LightStreams => "Lines of light flowing round the picture in long swirls.",
            Look::Kaleidoscope => "Sparkles round each ball (inside a group of balls), mirrored six ways.",
            Look::ColourSplitDust => "Swirling dust that splits into red, green and blue as it moves.",
            Look::LiquidMetal => "Drops of liquid chrome raining down and running over shapes.",
            Look::PaintDrips => "Paint flung off each ball (inside a group of balls), dripping where it lands.",
            Look::Galaxy => "Stars orbiting each ball (inside a group of balls) or a point, pulled along as it moves.",
            Look::FlowLines => "Thousands of specks following an invisible current; the balls stir it.",
            Look::FormAWord => "Specks holding the shape of a word; the balls blast them apart and they spring back.",
            Look::Flock => "A flock of birds or a school of fish that swoops together and splits round the balls.",
            Look::Shockwaves => "Dust hanging in the air that every hit sends a ring through.",
            Look::CometTail => "A blue gas tail and a golden dust tail blown off each ball by a far-off sun.",
            Look::BlackHole => "A disc of dust spiralling into a point and swallowed, stretched into streaks.",
            Look::IronFilings => "A grid of needles lining up with the balls' magnetic field.",
            Look::BeatGrid => "A grid of dots that jumps on every beat and ripples out from every hit.",
            Look::Moire => "Two fine grids over each other, one slowly turning and pushed by the balls.",
            Look::Spirograph => {
                "Specks looping round each ball (inside a group of balls) or a point on spirograph curves."
            }
            Look::FireAndSmoke => "Flames rising from a point, each one leaving a puff of smoke as it dies.",
            Look::Bubbles => "Bubbles rising from the bottom that pop when a ball touches them.",
            Look::SandThatPilesUp => "Sand pouring from a point into heaps on the shapes; balls land on the heaps.",
            Look::FallingLeaves => "Leaves fluttering down and settling, kicked up again by the balls.",
            Look::WaterfallAndMist => {
                "A waterfall that splits round the balls, pushing them down, with mist where it hits."
            }
            Look::LiquidSplashes => "Water splashing up where balls hit, running together and pooling on shapes.",
            Look::Fireworks => "Rockets that burst into sparks of their own colour at the top of their climb.",
            Look::StickyDust => "Dust hanging in the air that sticks to the balls and weighs them down.",
        }
    }

    /// Whether its particles come only where balls hit (a scene without balls shows none).
    pub fn needs_hits(self) -> bool {
        matches!(self, Look::LiquidSplashes)
    }

    /// Whether it rides what it sits in when it can.
    pub fn rides(self) -> bool {
        matches!(
            self,
            Look::Empty
                | Look::FireOnABall
                | Look::RainbowSmokeTrail
                | Look::Kaleidoscope
                | Look::PaintDrips
                | Look::PixelGlitch
                | Look::Galaxy
                | Look::CometTail
                | Look::Spirograph
        )
    }
}

/// One Look layer of a recipe, with its sub-layers.
pub(super) struct LayerRecipe {
    pub(super) kind: &'static str,
    pub(super) params: Params,
    pub(super) subs: Vec<(&'static str, Params)>,
}

/// What a look fills in.
pub(super) struct Recipe {
    pub(super) source: ParticlesSettings,
    pub(super) forces: Vec<(&'static str, Params)>,
    pub(super) meets: MeetsSettings,
    pub(super) layers: Vec<LayerRecipe>,
    /// Where the thing is put, when the look says.
    pub(super) position: Option<glam::Vec2>,
    /// Particles things inside it, fed by its particles: their names and looks.
    pub(super) children: Vec<(&'static str, Recipe)>,
}

impl Recipe {
    fn new(
        source: ParticlesSettings,
        forces: Vec<(&'static str, Params)>,
        meets: MeetsSettings,
        layers: Vec<LayerRecipe>,
    ) -> Recipe {
        Recipe { source, forces, meets, layers, position: None, children: Vec::new() }
    }

    fn at(self, x: f32, y: f32) -> Recipe {
        Recipe { position: Some(glam::Vec2::new(x, y)), ..self }
    }

    fn feeding(self, name: &'static str, child: Recipe) -> Recipe {
        let mut r = self;
        r.children.push((name, child));
        r
    }
}

/// A fed source: born where its parent's particles do `when`, `count` at a time.
fn fed(when: FedWhen, count: u32, s: ParticlesSettings) -> ParticlesSettings {
    ParticlesSettings { from: BornFrom::ItsParentsParticles, fed_when: when, count, ..s }
}

fn gradient(colors: &[Color]) -> Gradient {
    Gradient::spread(colors, Mix::Even, false)
}

pub(super) fn paint(colors: &[Color], follow: Follow) -> Paint {
    Paint { colours: gradient(colors), follow, ..Paint::default() }
}

pub(super) fn rainbow(follow: Follow) -> Paint {
    let colours = [
        Color::rgb(255, 60, 60),
        Color::rgb(255, 200, 40),
        Color::rgb(60, 230, 90),
        Color::rgb(40, 160, 255),
        Color::rgb(190, 70, 255),
    ];
    Paint { colours: Gradient::spread(&colours, Mix::RoundTheWheel, true), follow, ..Paint::default() }
}

pub(super) fn glow(size: f32, strength: f32) -> (&'static str, Params) {
    (SUB_GLOW, ShapeSubGlow { size, strength, ..ShapeSubGlow::default() }.to_params())
}

/// A Gives off light sub-layer of `strength`: the layer lights the scene round it (only with a
/// Lighting card; without one nothing changes).
fn light(strength: f32) -> (&'static str, Params) {
    (SUB_LIGHT, GivesLight { strength, ..GivesLight::default() }.to_params())
}

pub(super) fn sprites(s: SpritesLayer, subs: Vec<(&'static str, Params)>) -> LayerRecipe {
    LayerRecipe { kind: SPRITES, params: s.to_params(), subs }
}

/// `layer` with its Lighting fold's Bends light at `index` (glass and light part 3): its particles
/// focus lamp light like bubbles, in a lit scene with Glass caustics.
fn bending(mut layer: LayerRecipe, index: f32) -> LayerRecipe {
    use crate::modules::look::layers::lighting::BENDS_LIGHT;
    layer.params.set(BENDS_LIGHT, bpe_core::Value::Number(index as f64));
    layer
}

pub(super) fn streaks(s: StreaksLayer, subs: Vec<(&'static str, Params)>) -> LayerRecipe {
    LayerRecipe { kind: STREAKS, params: s.to_params(), subs }
}

pub(super) fn layer(kind: &'static str, params: Params, subs: Vec<(&'static str, Params)>) -> LayerRecipe {
    LayerRecipe { kind, params, subs }
}

/// What `look` fills in; `rides` when it sits in something it can ride.
fn trails(s: TrailsLayer, subs: Vec<(&'static str, Params)>) -> LayerRecipe {
    LayerRecipe { kind: TRAILS, params: s.to_params(), subs }
}

fn marks(s: MarksLayer, subs: Vec<(&'static str, Params)>) -> LayerRecipe {
    LayerRecipe { kind: MARKS, params: s.to_params(), subs }
}

fn liquid(s: LiquidLayer) -> LayerRecipe {
    LayerRecipe { kind: LIQUID, params: params_of(&s), subs: Vec::new() }
}

/// `layer` with the modifiers `m` too.
fn modified(mut layer: LayerRecipe, m: Modifiers) -> LayerRecipe {
    layer.params.0.extend(m.to_params().0);
    layer
}

/// What `look` fills in; `rides` when it sits in something it can ride.
fn recipe(look: Look, rides: bool, fed_by: bool) -> Recipe {
    // Inside another Particles thing an Empty is born where its parent's particles die.
    if look == Look::Empty && fed_by {
        return Recipe::new(
            fed(
                FedWhen::Die,
                3,
                ParticlesSettings {
                    life: 0.6,
                    speed: 60.0,
                    heading: Heading::AllRound,
                    ..ParticlesSettings::default()
                },
            ),
            Vec::new(),
            MeetsSettings::default(),
            vec![sprites(SpritesLayer::default(), Vec::new())],
        );
    }
    match look {
        Look::Galaxy
        | Look::FlowLines
        | Look::FormAWord
        | Look::Flock
        | Look::Shockwaves
        | Look::CometTail
        | Look::BlackHole
        | Look::IronFilings
        | Look::BeatGrid
        | Look::Moire
        | Look::Spirograph => super::looks_motion::recipe(look, rides),
        Look::Empty => empty(rides),
        Look::Rain => rain(),
        Look::Snow => snow(),
        Look::Hail => hail(),
        Look::BlossomPetals => blossom_petals(),
        Look::Sandstorm => sandstorm(),
        Look::FireOnABall => fire_on_a_ball(rides),
        Look::SparkFountain => spark_fountain(),
        Look::EmbersAndAsh => embers_and_ash(),
        Look::Fireflies => fireflies(),
        Look::Confetti => confetti(),
        Look::RainbowSmokeTrail => rainbow_smoke_trail(rides),
        Look::InkInWater => ink_in_water(),
        Look::Glitter => glitter(),
        Look::WebOfLight => web_of_light(),
        Look::LightStreams => light_streams(),
        Look::Kaleidoscope => kaleidoscope(rides),
        Look::ColourSplitDust => colour_split_dust(),
        Look::LiquidMetal => liquid_metal(),
        Look::PaintDrips => paint_drips(rides),
        Look::PixelGlitch => pixel_glitch(rides),
        Look::DustClouds => dust_clouds(),
        Look::ThickFog => thick_fog(),
        Look::FireAndSmoke => fire_and_smoke(),
        Look::Bubbles => bubbles(),
        Look::SandThatPilesUp => sand_that_piles_up(),
        Look::FallingLeaves => falling_leaves(),
        Look::WaterfallAndMist => waterfall_and_mist(),
        Look::LiquidSplashes => liquid_splashes(),
        Look::Fireworks => fireworks(),
        Look::StickyDust => sticky_dust(),
    }
}

/// `s` born riding each of its parent's balls, when `rides`.
fn riding(rides: bool, s: ParticlesSettings) -> ParticlesSettings {
    if rides { ParticlesSettings { from: BornFrom::RidingItsParent, rides_on: RidesOn::EachBall, ..s } } else { s }
}

/// Born across the top of the screen.
fn top() -> ParticlesSettings {
    ParticlesSettings { from: BornFrom::WholeScreen, edge: Edge::Top, ..ParticlesSettings::default() }
}

/// Gone at the screen's edges.
fn vanish() -> MeetsSettings {
    MeetsSettings { at_edges: AtEdges::Vanish, ..MeetsSettings::default() }
}

fn empty(rides: bool) -> Recipe {
    Recipe {
        source: riding(rides, ParticlesSettings::default()),
        forces: Vec::new(),
        meets: MeetsSettings::default(),
        layers: vec![sprites(SpritesLayer::default(), Vec::new())],
        position: None,
        children: Vec::new(),
    }
}

fn rain() -> Recipe {
    Recipe {
        source: ParticlesSettings {
            rate: 500.0,
            life: 3.0,
            life_spread: 0.1,
            speed: 900.0,
            speed_spread: 0.15,
            angle: 100.0,
            spread: 4.0,
            size: 1.0,
            size_spread: 0.4,
            ..top()
        },
        forces: vec![
            (GRAVITY, GravityForce { strength: 1200.0, ..Default::default() }.to_params()),
            (WIND, WindForce { speed: 120.0, grip: 0.5, gusts: 0.3, ..Default::default() }.to_params()),
        ],
        meets: MeetsSettings { push: 0.8, at_balls: AtBalls::Vanish, at_shapes: AtShapes::Vanish, ..vanish() },
        layers: vec![streaks(
            StreaksLayer {
                stretch: 0.025,
                // About 2 px across: thinner rain fades out once a phone video is compressed.
                width: 1.1,
                colour: paint(&[Color::rgb(190, 215, 255)], Follow::Life),
                opacity: 0.7,
                fade: ParticleFade::None,
                ..Default::default()
            },
            Vec::new(),
        )],
        position: None,
        children: vec![("Splashes", splashes())],
    }
}

fn snow() -> Recipe {
    Recipe {
        source: ParticlesSettings {
            rate: 80.0,
            life: 14.0,
            speed: 60.0,
            angle: 90.0,
            spread: 30.0,
            size: 3.0,
            size_spread: 0.5,
            ..top()
        },
        forces: vec![
            (GRAVITY, GravityForce { strength: 40.0, ..Default::default() }.to_params()),
            (SWAY, SwayForce { sway: 25.0, sway_speed: 0.6, ..Default::default() }.to_params()),
            (WANDER, WanderForce { strength: 30.0, turns: 0.5, ..Default::default() }.to_params()),
            (DRAG, DragForce { drag: 0.5, ..Default::default() }.to_params()),
        ],
        meets: MeetsSettings { push: 0.8, ..vanish() },
        layers: vec![sprites(
            SpritesLayer { softness: 0.7, fade: ParticleFade::None, ..Default::default() },
            Vec::new(),
        )],
        position: None,
        children: Vec::new(),
    }
}

fn hail() -> Recipe {
    Recipe {
        source: ParticlesSettings {
            rate: 120.0,
            life: 4.0,
            speed: 700.0,
            angle: 95.0,
            spread: 6.0,
            size: 3.5,
            size_spread: 0.4,
            ..top()
        },
        forces: vec![(GRAVITY, GravityForce { strength: 1400.0, ..Default::default() }.to_params())],
        meets: MeetsSettings {
            at_balls: AtBalls::BounceOff,
            at_shapes: AtShapes::Bounce,
            bounciness: 0.45,
            friction: 0.2,
            ..vanish()
        },
        layers: vec![sprites(
            SpritesLayer {
                softness: 0.15,
                colour: paint(&[Color::rgb(235, 245, 255)], Follow::Life),
                fade: ParticleFade::None,
                ..Default::default()
            },
            Vec::new(),
        )],
        position: None,
        children: Vec::new(),
    }
}

fn blossom_petals() -> Recipe {
    Recipe {
        source: ParticlesSettings {
            rate: 25.0,
            life: 16.0,
            speed: 50.0,
            angle: 100.0,
            spread: 40.0,
            size: 6.0,
            size_spread: 0.4,
            spin: 90.0,
            spin_spread: 0.8,
            ..top()
        },
        forces: vec![
            (GRAVITY, GravityForce { strength: 35.0, ..Default::default() }.to_params()),
            (WIND, WindForce { speed: 60.0, grip: 0.6, gusts: 0.5, ..Default::default() }.to_params()),
            (
                SWAY,
                SwayForce { sway: 40.0, sway_speed: 0.5, flutter: 1.5, flutter_drift: 40.0, ..Default::default() }
                    .to_params(),
            ),
        ],
        meets: vanish(),
        layers: vec![layer(
            SHAPES,
            ShapesLayer {
                shape: MoreShape::Petals,
                softness: 0.25,
                colour: paint(
                    &[Color::rgb(255, 196, 214), Color::rgb(255, 150, 190), Color::rgb(255, 236, 242)],
                    Follow::Order,
                ),
                fade: ParticleFade::None,
                ..Default::default()
            }
            .to_params(),
            Vec::new(),
        )],
        position: None,
        children: Vec::new(),
    }
}

fn sandstorm() -> Recipe {
    Recipe {
        source: ParticlesSettings {
            from: BornFrom::WholeScreen,
            edge: Edge::Left,
            rate: 900.0,
            life: 5.0,
            speed: 500.0,
            speed_spread: 0.4,
            angle: 0.0,
            spread: 10.0,
            size: 1.2,
            size_spread: 0.6,
            ..ParticlesSettings::default()
        },
        forces: vec![
            (
                WIND,
                WindForce { speed: 600.0, grip: 1.5, gusts: 0.6, gust_every: 1.5, waves: 700.0, ..Default::default() }
                    .to_params(),
            ),
            (SWIRL, SwirlForce { strength: 250.0, size: 220.0, drift: 0.0, ..Default::default() }.to_params()),
            (GRAVITY, GravityForce { strength: 30.0, ..Default::default() }.to_params()),
        ],
        meets: MeetsSettings { push: 0.6, ..vanish() },
        layers: vec![
            streaks(
                StreaksLayer {
                    stretch: 0.015,
                    width: 0.5,
                    colour: paint(&[Color::rgb(214, 170, 110)], Follow::Life),
                    opacity: 0.35,
                    fade: ParticleFade::None,
                    ..Default::default()
                },
                Vec::new(),
            ),
            sprites(
                SpritesLayer {
                    softness: 0.5,
                    colour: paint(&[Color::rgb(230, 190, 130), Color::rgb(180, 130, 80)], Follow::Order),
                    opacity: 0.6,
                    fade: ParticleFade::None,
                    ..Default::default()
                },
                Vec::new(),
            ),
            // The haze: a few of the grains drawn as big soft puffs.
            sprites(
                SpritesLayer {
                    made_of: ShapeMadeOf::Smoke,
                    softness: 1.0,
                    colour: paint(&[Color::rgb(222, 180, 120)], Follow::Life),
                    opacity: 0.05,
                    size: 24.0,
                    fade: ParticleFade::Both,
                    only: Only::AShare,
                    share: 0.15,
                    ..Default::default()
                },
                Vec::new(),
            ),
        ],
        position: None,
        children: Vec::new(),
    }
}

fn fire_on_a_ball(rides: bool) -> Recipe {
    Recipe {
        source: riding(
            rides,
            ParticlesSettings {
                from: BornFrom::APoint,
                rate: 220.0,
                life: 0.8,
                life_spread: 0.4,
                speed: 60.0,
                heading: Heading::AllRound,
                inherit: 0.3,
                size: 7.0,
                size_spread: 0.5,
                ..ParticlesSettings::default()
            },
        ),
        forces: vec![
            (RISE, RiseForce { strength: 500.0, cools: 0.6, ..Default::default() }.to_params()),
            (WANDER, WanderForce { strength: 300.0, turns: 3.0, ..Default::default() }.to_params()),
            (DRAG, DragForce { drag: 1.5, ..Default::default() }.to_params()),
        ],
        meets: MeetsSettings::default(),
        layers: vec![
            layer(FLAME, FlameLayer::default().to_params(), vec![light(1.5)]),
            // A few of them as embers, bright specks in the flames.
            sprites(
                SpritesLayer {
                    only: Only::AShare,
                    share: 0.08,
                    softness: 0.5,
                    size: 0.25,
                    colour: paint(&[Color::additive(255, 230, 150), Color::additive(255, 120, 30)], Follow::Life),
                    twinkle: 0.5,
                    twinkle_speed: 6.0,
                    size_curve: SizeOverLife::Even,
                    blend: bpe_core::Blend::Add,
                    ..Default::default()
                },
                vec![glow(4.0, 0.5)],
            ),
        ],
        position: None,
        children: Vec::new(),
    }
}

fn spark_fountain() -> Recipe {
    Recipe {
        source: ParticlesSettings {
            from: BornFrom::APoint,
            rate: 150.0,
            life: 2.2,
            speed: 700.0,
            speed_spread: 0.3,
            angle: -90.0,
            spread: 30.0,
            size: 1.5,
            ..ParticlesSettings::default()
        },
        forces: vec![
            (GRAVITY, GravityForce { strength: 900.0, ..Default::default() }.to_params()),
            (DRAG, DragForce { drag: 0.3, ..Default::default() }.to_params()),
        ],
        meets: MeetsSettings {
            at_balls: AtBalls::BounceOff,
            at_shapes: AtShapes::Bounce,
            bounciness: 0.5,
            ..MeetsSettings::default()
        },
        layers: vec![streaks(
            StreaksLayer {
                stretch: 0.025,
                width: 0.8,
                colour: paint(
                    &[Color::additive(255, 250, 210), Color::additive(255, 180, 50), Color::additive(230, 70, 10)],
                    Follow::Life,
                ),
                blend: bpe_core::Blend::Add,
                ..Default::default()
            },
            vec![glow(4.0, 0.5), light(1.0)],
        )],
        position: None,
        children: Vec::new(),
    }
}

fn embers_and_ash() -> Recipe {
    Recipe {
        source: ParticlesSettings {
            from: BornFrom::WholeScreen,
            edge: Edge::Bottom,
            rate: 40.0,
            life: 6.0,
            life_spread: 0.5,
            speed: 60.0,
            angle: -90.0,
            spread: 60.0,
            size: 2.0,
            ..ParticlesSettings::default()
        },
        forces: vec![
            (RISE, RiseForce { strength: 120.0, cools: 3.0, ..Default::default() }.to_params()),
            (WANDER, WanderForce { strength: 80.0, ..Default::default() }.to_params()),
            (SWIRL, SwirlForce { strength: 120.0, size: 300.0, drift: 20.0, ..Default::default() }.to_params()),
            (DRAG, DragForce { drag: 0.4, ..Default::default() }.to_params()),
        ],
        meets: MeetsSettings::default(),
        layers: vec![
            sprites(
                SpritesLayer {
                    only: Only::AShare,
                    softness: 0.6,
                    colour: paint(&[Color::additive(255, 200, 90), Color::additive(240, 90, 20)], Follow::Life),
                    twinkle: 0.6,
                    twinkle_speed: 4.0,
                    blend: bpe_core::Blend::Add,
                    ..Default::default()
                },
                vec![glow(5.0, 0.6), light(0.8)],
            ),
            sprites(
                SpritesLayer {
                    only: Only::TheRest,
                    made_of: ShapeMadeOf::Shards,
                    softness: 0.2,
                    colour: paint(&[Color::rgb(120, 118, 115), Color::rgb(70, 68, 66)], Follow::Order),
                    opacity: 0.7,
                    ..Default::default()
                },
                Vec::new(),
            ),
        ],
        position: None,
        children: Vec::new(),
    }
}

fn fireflies() -> Recipe {
    Recipe {
        source: ParticlesSettings {
            from: BornFrom::AnArea,
            area: AreaShape::Rectangle,
            width: 900.0,
            height: 1400.0,
            emit: Emit::KeepACrowd,
            count: 40,
            life: 6.0,
            life_spread: 0.4,
            speed: 20.0,
            heading: Heading::AllRound,
            size: 2.5,
            ..ParticlesSettings::default()
        },
        forces: vec![
            (WANDER, WanderForce { strength: 60.0, turns: 0.5, ..Default::default() }.to_params()),
            (DRAG, DragForce { drag: 0.8, ..Default::default() }.to_params()),
            // They flee the balls (part 3).
            (
                super::more_forces::PUSH,
                super::more_forces::PushForce { strength: 1500.0, ball_reach: 5.0, ..Default::default() }.to_params(),
            ),
        ],
        meets: MeetsSettings::default(),
        layers: vec![sprites(
            SpritesLayer {
                softness: 0.8,
                colour: paint(&[Color::additive(220, 255, 120)], Follow::Life),
                twinkle: 0.8,
                twinkle_speed: 1.5,
                fade: ParticleFade::Both,
                blend: bpe_core::Blend::Add,
                ..Default::default()
            },
            vec![glow(10.0, 0.7), light(1.0)],
        )],
        position: None,
        children: Vec::new(),
    }
}

fn confetti() -> Recipe {
    Recipe {
        source: ParticlesSettings {
            from: BornFrom::APoint,
            emit: Emit::Bursts,
            count: 150,
            every: 3.0,
            life: 5.0,
            speed: 900.0,
            speed_spread: 0.4,
            angle: -90.0,
            spread: 70.0,
            size: 5.0,
            spin: 360.0,
            spin_spread: 1.0,
            ..ParticlesSettings::default()
        },
        forces: vec![
            (GRAVITY, GravityForce { strength: 600.0, ..Default::default() }.to_params()),
            (DRAG, DragForce { drag: 1.2, ..Default::default() }.to_params()),
            (SWAY, SwayForce { sway: 15.0, flutter: 4.0, flutter_drift: 40.0, ..Default::default() }.to_params()),
        ],
        meets: MeetsSettings::default(),
        layers: vec![sprites(
            SpritesLayer {
                made_of: ShapeMadeOf::Confetti,
                softness: 0.0,
                colour: rainbow(Follow::Order),
                ..Default::default()
            },
            Vec::new(),
        )],
        position: None,
        children: Vec::new(),
    }
}

fn rainbow_smoke_trail(rides: bool) -> Recipe {
    Recipe {
        source: riding(
            rides,
            ParticlesSettings {
                from: BornFrom::APoint,
                rate: 120.0,
                life: 2.5,
                speed: 20.0,
                heading: Heading::AllRound,
                size: 10.0,
                size_spread: 0.4,
                ..ParticlesSettings::default()
            },
        ),
        forces: vec![
            (RISE, RiseForce { strength: 40.0, ..Default::default() }.to_params()),
            (WANDER, WanderForce { strength: 60.0, ..Default::default() }.to_params()),
            (SWIRL, SwirlForce { strength: 80.0, size: 120.0, ..Default::default() }.to_params()),
            (DRAG, DragForce { drag: 1.0, ..Default::default() }.to_params()),
        ],
        meets: MeetsSettings::default(),
        layers: vec![layer(
            SMOKE,
            SmokeLayer {
                thickness: 1.2,
                puff: 2.2,
                colour: rainbow(Follow::Life),
                opacity: 0.7,
                shading: 0.4,
                dark_inside: 0.2,
                ..Default::default()
            }
            .to_params(),
            Vec::new(),
        )],
        position: None,
        children: Vec::new(),
    }
}

fn ink_in_water() -> Recipe {
    Recipe {
        position: None,
        children: Vec::new(),
        source: ParticlesSettings {
            from: BornFrom::APoint,
            emit: Emit::Bursts,
            count: 400,
            every: 4.0,
            life: 6.0,
            speed: 220.0,
            speed_spread: 0.6,
            heading: Heading::AllRound,
            size: 6.0,
            size_spread: 0.6,
            ..ParticlesSettings::default()
        },
        forces: vec![
            (DRAG, DragForce { drag: 2.5, ..Default::default() }.to_params()),
            (SWIRL, SwirlForce { strength: 200.0, size: 120.0, drift: 0.0, ..Default::default() }.to_params()),
            (WANDER, WanderForce { strength: 40.0, ..Default::default() }.to_params()),
            (GRAVITY, GravityForce { strength: 20.0, ..Default::default() }.to_params()),
        ],
        meets: MeetsSettings::default(),
        layers: vec![layer(
            SMOKE,
            SmokeLayer {
                thickness: 1.0,
                puff: 2.5,
                detail: 0.7,
                detail_size: 50.0,
                drift: 8.0,
                shading: 0.2,
                dark_inside: 0.6,
                colour: paint(
                    &[Color::rgb(30, 40, 160), Color::rgb(110, 40, 170), Color::rgb(20, 120, 170)],
                    Follow::Order,
                ),
                opacity: 0.8,
                ..Default::default()
            }
            .to_params(),
            Vec::new(),
        )],
    }
}

fn glitter() -> Recipe {
    Recipe {
        position: None,
        children: Vec::new(),
        source: ParticlesSettings {
            emit: Emit::KeepACrowd,
            count: 400,
            life: 8.0,
            life_spread: 0.5,
            speed: 15.0,
            heading: Heading::AllRound,
            size: 3.0,
            size_spread: 0.5,
            spin: 120.0,
            spin_spread: 1.0,
            ..ParticlesSettings::default()
        },
        forces: vec![
            (GRAVITY, GravityForce { strength: 10.0, ..Default::default() }.to_params()),
            (WANDER, WanderForce { strength: 20.0, turns: 0.5, ..Default::default() }.to_params()),
            (DRAG, DragForce { drag: 0.5, ..Default::default() }.to_params()),
        ],
        meets: MeetsSettings { push: 0.6, ..MeetsSettings::default() },
        layers: vec![layer(
            GLITTER,
            GlitterLayer {
                colour: paint(
                    &[Color::rgb(255, 214, 120), Color::rgb(230, 230, 240), Color::rgb(255, 160, 210)],
                    Follow::Order,
                ),
                ..Default::default()
            }
            .to_params(),
            vec![glow(3.0, 0.3)],
        )],
    }
}

fn web_of_light() -> Recipe {
    Recipe {
        position: None,
        children: Vec::new(),
        source: ParticlesSettings {
            from: BornFrom::AnArea,
            area: AreaShape::Rectangle,
            width: 900.0,
            height: 1500.0,
            emit: Emit::KeepACrowd,
            count: 90,
            life: 12.0,
            life_spread: 0.4,
            speed: 30.0,
            heading: Heading::AllRound,
            size: 2.5,
            ..ParticlesSettings::default()
        },
        forces: vec![
            (WANDER, WanderForce { strength: 40.0, turns: 0.4, ..Default::default() }.to_params()),
            (DRAG, DragForce { drag: 0.4, ..Default::default() }.to_params()),
        ],
        meets: MeetsSettings { push: 0.5, at_edges: AtEdges::Bounce, ..MeetsSettings::default() },
        layers: vec![layer(
            WEB,
            WebLayer {
                reach: 170.0,
                links: 5,
                to_balls: true,
                colour: paint(&[Color::additive(90, 220, 255), Color::additive(190, 120, 255)], Follow::Order),
                fade: ParticleFade::Both,
                ..Default::default()
            }
            .to_params(),
            vec![glow(3.0, 0.4)],
        )],
    }
}

fn light_streams() -> Recipe {
    Recipe {
        source: ParticlesSettings {
            from: BornFrom::WholeScreen,
            emit: Emit::KeepACrowd,
            count: 500,
            life: 6.0,
            life_spread: 0.5,
            speed: 40.0,
            heading: Heading::AllRound,
            size: 1.2,
            size_spread: 0.3,
            ..ParticlesSettings::default()
        },
        forces: vec![
            (SWIRL, SwirlForce { strength: 420.0, size: 280.0, drift: 40.0, ..Default::default() }.to_params()),
            (DRAG, DragForce { drag: 1.0, ..Default::default() }.to_params()),
        ],
        meets: MeetsSettings { push: 0.6, at_edges: AtEdges::WrapRound, ..MeetsSettings::default() },
        layers: vec![trails(
            TrailsLayer {
                length: 48,
                every: 2,
                taper: 0.6,
                colour: rainbow(Follow::Place),
                fade: ParticleFade::Both,
                blend: bpe_core::Blend::Add,
                ..Default::default()
            },
            Vec::new(),
        )],
        position: None,
        children: Vec::new(),
    }
}

fn kaleidoscope(rides: bool) -> Recipe {
    Recipe {
        source: riding(
            rides,
            ParticlesSettings {
                from: BornFrom::APoint,
                rate: 60.0,
                life: 2.5,
                life_spread: 0.3,
                speed: 140.0,
                speed_spread: 0.4,
                heading: Heading::AllRound,
                size: 4.0,
                size_spread: 0.5,
                spin: 180.0,
                spin_spread: 1.0,
                ..ParticlesSettings::default()
            },
        ),
        forces: vec![
            (SWIRL, SwirlForce { strength: 220.0, size: 140.0, drift: 0.0, ..Default::default() }.to_params()),
            (DRAG, DragForce { drag: 0.8, ..Default::default() }.to_params()),
        ],
        meets: MeetsSettings::default(),
        layers: vec![modified(
            sprites(
                SpritesLayer {
                    made_of: ShapeMadeOf::Stars,
                    softness: 0.3,
                    colour: rainbow(Follow::Life),
                    blend: bpe_core::Blend::Add,
                    ..Default::default()
                },
                vec![glow(4.0, 0.5)],
            ),
            Modifiers {
                mirror: Mirror::SixWays,
                round: MirrorRound::TheThingTheyRide,
                flipped: true,
                ..Default::default()
            },
        )],
        position: None,
        children: Vec::new(),
    }
}

fn colour_split_dust() -> Recipe {
    Recipe {
        source: ParticlesSettings {
            from: BornFrom::WholeScreen,
            emit: Emit::KeepACrowd,
            count: 900,
            life: 8.0,
            life_spread: 0.5,
            speed: 30.0,
            heading: Heading::AllRound,
            size: 2.0,
            size_spread: 0.5,
            ..ParticlesSettings::default()
        },
        forces: vec![
            (SWIRL, SwirlForce { strength: 320.0, size: 220.0, drift: 30.0, ..Default::default() }.to_params()),
            (DRAG, DragForce { drag: 0.7, ..Default::default() }.to_params()),
        ],
        meets: MeetsSettings { push: 0.9, at_edges: AtEdges::WrapRound, ..MeetsSettings::default() },
        layers: vec![modified(
            sprites(
                SpritesLayer {
                    softness: 0.6,
                    colour: paint(&[Color::additive(255, 255, 255)], Follow::Life),
                    fade: ParticleFade::Both,
                    blend: bpe_core::Blend::Add,
                    ..Default::default()
                },
                Vec::new(),
            ),
            Modifiers { split: 0.04, offset: 1.5, ..Default::default() },
        )],
        position: None,
        children: Vec::new(),
    }
}

fn liquid_metal() -> Recipe {
    Recipe {
        source: ParticlesSettings {
            rate: 50.0,
            life: 8.0,
            speed: 120.0,
            angle: 90.0,
            spread: 8.0,
            size: 5.0,
            size_spread: 0.4,
            ..top()
        },
        forces: vec![(GRAVITY, GravityForce { strength: 900.0, ..Default::default() }.to_params())],
        meets: MeetsSettings {
            push: 0.5,
            at_balls: AtBalls::BounceOff,
            ball_bounce: 0.2,
            at_shapes: AtShapes::Stop,
            ..vanish()
        },
        layers: vec![liquid(LiquidLayer {
            blob: 3.5,
            level: 0.45,
            roundness: 18.0,
            surface: SurfaceKind::Chrome,
            shine: 0.8,
            colour: paint(&[Color::rgb(200, 205, 215)], Follow::Life),
            fade: ParticleFade::None,
            ..Default::default()
        })],
        position: None,
        children: Vec::new(),
    }
}

fn paint_drips(rides: bool) -> Recipe {
    Recipe {
        source: riding(
            rides,
            ParticlesSettings {
                from: BornFrom::APoint,
                rate: 25.0,
                life: 5.0,
                speed: 160.0,
                speed_spread: 0.5,
                heading: Heading::AllRound,
                inherit: 0.5,
                size: 4.0,
                size_spread: 0.5,
                ..ParticlesSettings::default()
            },
        ),
        forces: vec![(GRAVITY, GravityForce { strength: 900.0, ..Default::default() }.to_params())],
        meets: MeetsSettings { at_shapes: AtShapes::Vanish, ..vanish() },
        layers: vec![
            marks(
                MarksLayer {
                    on_edges: true,
                    splats: 5,
                    drip: 70.0,
                    drip_time: 3.0,
                    size: 1.6,
                    colour: paint(
                        &[
                            Color::rgb(255, 60, 110),
                            Color::rgb(255, 200, 40),
                            Color::rgb(40, 200, 255),
                            Color::rgb(130, 255, 90),
                        ],
                        Follow::Order,
                    ),
                    fade: ParticleFade::None,
                    ..Default::default()
                },
                Vec::new(),
            ),
            sprites(
                SpritesLayer {
                    softness: 0.2,
                    colour: paint(
                        &[
                            Color::rgb(255, 60, 110),
                            Color::rgb(255, 200, 40),
                            Color::rgb(40, 200, 255),
                            Color::rgb(130, 255, 90),
                        ],
                        Follow::Order,
                    ),
                    fade: ParticleFade::None,
                    ..Default::default()
                },
                Vec::new(),
            ),
        ],
        position: None,
        children: Vec::new(),
    }
}

fn pixel_glitch(rides: bool) -> Recipe {
    Recipe {
        source: riding(
            rides,
            ParticlesSettings {
                from: BornFrom::APoint,
                rate: 160.0,
                life: 1.2,
                life_spread: 0.5,
                speed: 70.0,
                heading: Heading::AllRound,
                inherit: 0.2,
                size: 4.0,
                size_spread: 0.3,
                ..ParticlesSettings::default()
            },
        ),
        forces: vec![
            (WANDER, WanderForce { strength: 250.0, turns: 4.0, ..Default::default() }.to_params()),
            (DRAG, DragForce { drag: 2.0, ..Default::default() }.to_params()),
        ],
        meets: MeetsSettings::default(),
        layers: vec![modified(
            layer(
                SHAPES,
                ShapesLayer {
                    shape: MoreShape::Pixels,
                    grid: 8.0,
                    softness: 0.0,
                    colour: paint(
                        &[Color::additive(0, 255, 230), Color::additive(255, 40, 200), Color::additive(120, 255, 60)],
                        Follow::Order,
                    ),
                    blend: bpe_core::Blend::Add,
                    ..Default::default()
                }
                .to_params(),
                Vec::new(),
            ),
            Modifiers { glitch: 0.25, band: 16.0, tear: 50.0, rate: 12.0, tear_split: 5.0, ..Default::default() },
        )],
        position: None,
        children: Vec::new(),
    }
}

fn dust_clouds() -> Recipe {
    Recipe {
        position: None,
        children: Vec::new(),
        source: ParticlesSettings {
            emit: Emit::OnHits,
            count: 30,
            least_speed: 80.0,
            follow_impact: true,
            life: 2.5,
            life_spread: 0.4,
            speed: 160.0,
            speed_spread: 0.5,
            heading: Heading::OffTheBounce,
            spread: 120.0,
            size: 9.0,
            size_spread: 0.5,
            ..ParticlesSettings::default()
        },
        forces: vec![
            (AIR, FollowAirForce { grip: 3.0, ..Default::default() }.to_params()),
            (DRAG, DragForce { drag: 2.5, ..Default::default() }.to_params()),
            (SWIRL, SwirlForce { strength: 60.0, size: 100.0, drift: 10.0, ..Default::default() }.to_params()),
            (GRAVITY, GravityForce { strength: 25.0, ..Default::default() }.to_params()),
        ],
        meets: MeetsSettings::default(),
        layers: vec![sprites(
            SpritesLayer {
                made_of: ShapeMadeOf::Smoke,
                softness: 1.0,
                colour: paint(&[Color::rgb(196, 170, 130), Color::rgb(150, 128, 100)], Follow::Life),
                opacity: 0.45,
                size_curve: SizeOverLife::Grow,
                ..Default::default()
            },
            Vec::new(),
        )],
    }
}

fn thick_fog() -> Recipe {
    Recipe {
        position: None,
        children: Vec::new(),
        source: ParticlesSettings {
            from: BornFrom::AnArea,
            area: AreaShape::Rectangle,
            width: 1100.0,
            height: 700.0,
            emit: Emit::KeepACrowd,
            count: 450,
            life: 9.0,
            life_spread: 0.3,
            speed: 8.0,
            heading: Heading::AllRound,
            size: 45.0,
            size_spread: 0.4,
            ..ParticlesSettings::default()
        },
        forces: vec![
            (AIR, FollowAirForce { grip: 1.5, ..Default::default() }.to_params()),
            (WANDER, WanderForce { strength: 8.0, turns: 0.3, ..Default::default() }.to_params()),
            (DRAG, DragForce { drag: 0.6, ..Default::default() }.to_params()),
        ],
        // Two-way: the balls wading through it are slowed.
        meets: MeetsSettings { push: 0.9, reach: 1.5, two_way: true, weight: 0.02, ..MeetsSettings::default() },
        layers: vec![sprites(
            SpritesLayer {
                made_of: ShapeMadeOf::Smoke,
                softness: 1.0,
                colour: paint(&[Color::rgb(214, 222, 232)], Follow::Life),
                opacity: 0.09,
                fade: ParticleFade::Both,
                ..Default::default()
            },
            Vec::new(),
        )],
    }
}

fn fire_and_smoke() -> Recipe {
    Recipe::new(
        ParticlesSettings {
            from: BornFrom::APoint,
            rate: 200.0,
            life: 0.7,
            life_spread: 0.4,
            speed: 70.0,
            heading: Heading::AllRound,
            size: 8.0,
            size_spread: 0.5,
            ..ParticlesSettings::default()
        },
        vec![
            (RISE, RiseForce { strength: 600.0, cools: 0.5, ..Default::default() }.to_params()),
            (WANDER, WanderForce { strength: 300.0, turns: 3.0, ..Default::default() }.to_params()),
            (DRAG, DragForce { drag: 1.5, ..Default::default() }.to_params()),
        ],
        MeetsSettings::default(),
        vec![layer(FLAME, FlameLayer::default().to_params(), Vec::new())],
    )
    .feeding(
        "Smoke",
        Recipe::new(
            fed(
                FedWhen::Die,
                1,
                ParticlesSettings {
                    life: 3.0,
                    life_spread: 0.3,
                    speed: 20.0,
                    heading: Heading::AllRound,
                    inherit: 0.5,
                    size: 10.0,
                    size_spread: 0.4,
                    ..ParticlesSettings::default()
                },
            ),
            vec![
                (RISE, RiseForce { strength: 120.0, ..Default::default() }.to_params()),
                (WANDER, WanderForce { strength: 60.0, ..Default::default() }.to_params()),
                (SWIRL, SwirlForce { strength: 80.0, size: 140.0, ..Default::default() }.to_params()),
                (DRAG, DragForce { drag: 0.8, ..Default::default() }.to_params()),
            ],
            MeetsSettings::default(),
            vec![layer(
                SMOKE,
                SmokeLayer {
                    thickness: 1.0,
                    colour: paint(&[Color::rgb(120, 116, 112), Color::rgb(70, 68, 66)], Follow::Life),
                    opacity: 0.5,
                    ..SmokeLayer::default()
                }
                .to_params(),
                Vec::new(),
            )],
        ),
    )
}

fn bubbles() -> Recipe {
    Recipe::new(
        ParticlesSettings {
            from: BornFrom::WholeScreen,
            edge: Edge::Bottom,
            rate: 12.0,
            life: 14.0,
            speed: 40.0,
            angle: -90.0,
            spread: 20.0,
            size: 9.0,
            size_spread: 0.5,
            ..ParticlesSettings::default()
        },
        vec![
            (RISE, RiseForce { strength: 60.0, ..Default::default() }.to_params()),
            (SWAY, SwayForce { sway: 18.0, sway_speed: 0.7, ..Default::default() }.to_params()),
            (DRAG, DragForce { drag: 0.6, ..Default::default() }.to_params()),
        ],
        MeetsSettings { push: 0.6, at_balls: AtBalls::Pop, ..vanish() },
        vec![bending(
            sprites(
                SpritesLayer {
                    made_of: ShapeMadeOf::Bubbles,
                    softness: 0.3,
                    colour: paint(&[Color::rgb(200, 235, 255)], Follow::Life),
                    opacity: 0.8,
                    fade: ParticleFade::None,
                    ..Default::default()
                },
                Vec::new(),
            ),
            1.33,
        )],
    )
    .feeding(
        "Pops",
        Recipe::new(
            fed(
                FedWhen::Pop,
                8,
                ParticlesSettings {
                    life: 0.25,
                    speed: 140.0,
                    heading: Heading::AllRound,
                    size: 1.2,
                    ..ParticlesSettings::default()
                },
            ),
            vec![(DRAG, DragForce { drag: 3.0, ..Default::default() }.to_params())],
            MeetsSettings::default(),
            vec![sprites(
                SpritesLayer {
                    colour: paint(&[Color::additive(230, 245, 255)], Follow::Life),
                    blend: bpe_core::Blend::Add,
                    ..Default::default()
                },
                Vec::new(),
            )],
        ),
    )
}

fn sand_that_piles_up() -> Recipe {
    Recipe::new(
        ParticlesSettings {
            from: BornFrom::APoint,
            rate: 300.0,
            life: 30.0,
            life_spread: 0.0,
            speed: 20.0,
            angle: 90.0,
            spread: 8.0,
            size: 1.2,
            size_spread: 0.3,
            ..ParticlesSettings::default()
        },
        vec![(GRAVITY, GravityForce { strength: 900.0, ..Default::default() }.to_params())],
        MeetsSettings {
            push: 0.3,
            at_shapes: AtShapes::PileUp,
            at_edges: AtEdges::Bounce,
            edge_margin: 0.0,
            grain: 0.35,
            two_way: true,
            ..MeetsSettings::default()
        },
        vec![
            LayerRecipe { kind: PILES, params: PilesLayer::default().to_params(), subs: Vec::new() },
            sprites(
                SpritesLayer {
                    softness: 0.2,
                    colour: paint(&[Color::rgb(222, 188, 128), Color::rgb(196, 156, 96)], Follow::Order),
                    fade: ParticleFade::None,
                    ..Default::default()
                },
                Vec::new(),
            ),
        ],
    )
    .at(0.0, -700.0)
}

fn falling_leaves() -> Recipe {
    Recipe::new(
        ParticlesSettings {
            rate: 6.0,
            life: 30.0,
            speed: 40.0,
            angle: 95.0,
            spread: 30.0,
            size: 7.0,
            size_spread: 0.4,
            spin: 60.0,
            spin_spread: 0.8,
            ..top()
        },
        vec![
            (GRAVITY, GravityForce { strength: 60.0, ..Default::default() }.to_params()),
            (
                SWAY,
                SwayForce { sway: 40.0, sway_speed: 0.4, flutter: 1.2, flutter_drift: 50.0, ..Default::default() }
                    .to_params(),
            ),
            (WIND, WindForce { speed: 40.0, grip: 0.4, gusts: 0.6, ..Default::default() }.to_params()),
            (DRAG, DragForce { drag: 0.4, ..Default::default() }.to_params()),
        ],
        MeetsSettings {
            push: 0.8,
            reach: 2.5,
            at_shapes: AtShapes::Settle,
            fade_after: 12.0,
            most_kept: 400,
            at_edges: AtEdges::Bounce,
            edge_margin: 0.0,
            ..MeetsSettings::default()
        },
        vec![layer(
            SHAPES,
            ShapesLayer {
                shape: MoreShape::Leaves,
                softness: 0.2,
                colour: paint(
                    &[Color::rgb(214, 96, 34), Color::rgb(232, 160, 40), Color::rgb(160, 60, 30)],
                    Follow::Order,
                ),
                fade: ParticleFade::None,
                ..Default::default()
            }
            .to_params(),
            Vec::new(),
        )],
    )
}

fn waterfall_and_mist() -> Recipe {
    Recipe::new(
        ParticlesSettings {
            from: BornFrom::ALine,
            length: 420.0,
            rate: 700.0,
            life: 3.0,
            speed: 150.0,
            angle: 90.0,
            spread: 3.0,
            size: 1.6,
            size_spread: 0.3,
            ..ParticlesSettings::default()
        },
        vec![(GRAVITY, GravityForce { strength: 1300.0, ..Default::default() }.to_params())],
        MeetsSettings {
            push: 0.9,
            reach: 1.3,
            at_balls: AtBalls::BounceOff,
            ball_bounce: 0.2,
            at_shapes: AtShapes::Vanish,
            two_way: true,
            weight: 0.5,
            ..vanish()
        },
        vec![streaks(
            StreaksLayer {
                stretch: 0.02,
                width: 0.8,
                colour: paint(&[Color::rgb(200, 230, 255), Color::rgb(150, 200, 250)], Follow::Order),
                opacity: 0.6,
                fade: ParticleFade::None,
                ..Default::default()
            },
            Vec::new(),
        )],
    )
    .at(0.0, -1000.0)
    .feeding(
        "Mist",
        Recipe::new(
            fed(
                FedWhen::HitAnything,
                1,
                ParticlesSettings {
                    life: 1.2,
                    speed: 60.0,
                    heading: Heading::OffTheBounce,
                    spread: 160.0,
                    size: 14.0,
                    size_spread: 0.5,
                    ..ParticlesSettings::default()
                },
            ),
            vec![
                (RISE, RiseForce { strength: 40.0, ..Default::default() }.to_params()),
                (WANDER, WanderForce { strength: 50.0, ..Default::default() }.to_params()),
                (DRAG, DragForce { drag: 2.0, ..Default::default() }.to_params()),
            ],
            MeetsSettings::default(),
            vec![sprites(
                SpritesLayer {
                    made_of: ShapeMadeOf::Smoke,
                    softness: 1.0,
                    colour: paint(&[Color::rgb(235, 245, 255)], Follow::Life),
                    opacity: 0.15,
                    size_curve: SizeOverLife::Grow,
                    ..Default::default()
                },
                Vec::new(),
            )],
        ),
    )
}

fn liquid_splashes() -> Recipe {
    Recipe::new(
        ParticlesSettings {
            from: BornFrom::APoint,
            emit: Emit::OnHits,
            count: 25,
            life: 4.0,
            speed: 350.0,
            speed_spread: 0.5,
            heading: Heading::OffTheBounce,
            spread: 140.0,
            size: 3.0,
            size_spread: 0.5,
            ..ParticlesSettings::default()
        },
        vec![
            (GRAVITY, GravityForce { strength: 1000.0, ..Default::default() }.to_params()),
            (DRAG, DragForce { drag: 0.3, ..Default::default() }.to_params()),
        ],
        MeetsSettings {
            at_balls: AtBalls::BounceOff,
            ball_bounce: 0.2,
            at_shapes: AtShapes::Settle,
            fade_after: 2.5,
            ..vanish()
        },
        vec![liquid(LiquidLayer {
            blob: 4.5,
            surface: SurfaceKind::Plastic,
            colour: paint(&[Color::rgb(90, 170, 255), Color::rgb(40, 110, 230)], Follow::Speed),
            fade: ParticleFade::None,
            ..Default::default()
        })],
    )
}

fn fireworks() -> Recipe {
    Recipe::new(
        ParticlesSettings {
            from: BornFrom::WholeScreen,
            edge: Edge::Bottom,
            emit: Emit::Bursts,
            every: 0.7,
            count: 1,
            life: 3.0,
            speed: 1100.0,
            speed_spread: 0.15,
            angle: -90.0,
            spread: 12.0,
            size: 1.5,
            ..ParticlesSettings::default()
        },
        vec![
            (GRAVITY, GravityForce { strength: 600.0, ..Default::default() }.to_params()),
            (DRAG, DragForce { drag: 0.2, ..Default::default() }.to_params()),
        ],
        MeetsSettings::default(),
        vec![streaks(
            StreaksLayer { stretch: 0.03, width: 0.8, colour: rainbow(Follow::Order), ..Default::default() },
            vec![glow(4.0, 0.5)],
        )],
    )
    .feeding(
        "Bursts",
        Recipe::new(
            fed(
                FedWhen::StopRising,
                90,
                ParticlesSettings {
                    life: 1.6,
                    life_spread: 0.3,
                    speed: 260.0,
                    speed_spread: 0.25,
                    heading: Heading::AllRound,
                    size: 1.8,
                    ..ParticlesSettings::default()
                },
            ),
            vec![
                (GRAVITY, GravityForce { strength: 150.0, ..Default::default() }.to_params()),
                (DRAG, DragForce { drag: 1.2, ..Default::default() }.to_params()),
            ],
            MeetsSettings::default(),
            vec![sprites(
                SpritesLayer {
                    made_of: ShapeMadeOf::Sparks,
                    colour_from: ColourFrom::ParentParticle,
                    twinkle: 0.3,
                    blend: bpe_core::Blend::Add,
                    ..Default::default()
                },
                vec![glow(5.0, 0.6)],
            )],
        ),
    )
}

fn sticky_dust() -> Recipe {
    Recipe::new(
        ParticlesSettings {
            from: BornFrom::AnArea,
            area: AreaShape::Rectangle,
            width: 1000.0,
            height: 1600.0,
            emit: Emit::KeepACrowd,
            count: 500,
            life: 20.0,
            life_spread: 0.3,
            speed: 5.0,
            heading: Heading::AllRound,
            size: 1.6,
            size_spread: 0.5,
            ..ParticlesSettings::default()
        },
        vec![
            (WANDER, WanderForce { strength: 15.0, turns: 0.3, ..Default::default() }.to_params()),
            (DRAG, DragForce { drag: 1.0, ..Default::default() }.to_params()),
        ],
        MeetsSettings {
            push: 0.4,
            at_balls: AtBalls::Stick,
            fall_off: 0.35,
            two_way: true,
            weight: 3.0,
            slows: false,
            ..MeetsSettings::default()
        },
        vec![sprites(
            SpritesLayer {
                softness: 0.4,
                colour: paint(&[Color::rgb(205, 195, 175), Color::rgb(170, 160, 140)], Follow::Order),
                opacity: 0.85,
                fade: ParticleFade::None,
                ..Default::default()
            },
            Vec::new(),
        )],
    )
}

/// Rain's splashes: a few drops thrown off where it hits a ball or a shape.
fn splashes() -> Recipe {
    Recipe::new(
        fed(
            FedWhen::HitAnything,
            3,
            ParticlesSettings {
                life: 0.35,
                life_spread: 0.3,
                speed: 160.0,
                speed_spread: 0.5,
                heading: Heading::OffTheBounce,
                spread: 120.0,
                size: 1.0,
                size_spread: 0.3,
                ..ParticlesSettings::default()
            },
        ),
        vec![(GRAVITY, GravityForce { strength: 1200.0, ..Default::default() }.to_params())],
        MeetsSettings::default(),
        vec![sprites(
            SpritesLayer {
                colour: paint(&[Color::rgb(200, 222, 255)], Follow::Life),
                opacity: 0.8,
                ..Default::default()
            },
            Vec::new(),
        )],
    )
}

/// Whether a Particles thing under `parent` can ride it.
pub fn can_ride(scene: &Scene, parent: ThingId) -> bool {
    scene.things.get(&parent).is_some_and(|t| crate::riding::can_ride(KIND, &t.kind))
}

/// Writes `params` into layer `layer` of `id`'s card `slot`.
fn put_layer(scene: &mut Scene, id: ThingId, slot: SlotId, layer: bpe_core::LayerId, params: Params) {
    if let Some(card) = scene.things.get_mut(&id).and_then(|t| t.slots.get_mut(&slot)) {
        for (key, value) in params.0 {
            card.params.0.insert(layered_key(layer, &key), value);
        }
    }
}

/// Whether a Particles thing under `parent` is inside another Particles thing (and can be fed).
pub fn can_be_fed(scene: &Scene, parent: ThingId) -> bool {
    scene.things.get(&parent).is_some_and(|t| t.kind == KIND)
}

/// Adds a Particles thing with `look` under `parent`, named after the look, with the fed
/// Particles things the look has inside it (and, for a look made of two, the second beside it).
/// The first is given back.
pub fn add_particles(scene: &mut Scene, reg: &Registry, look: Look, parent: ThingId) -> Result<ThingId, EditError> {
    let rides = look.rides() && can_ride(scene, parent);
    let r = recipe(look, rides, can_be_fed(scene, parent));
    let id = add_recipe(scene, reg, r, look.name(), parent)?;
    for (name, r) in super::looks_motion::also(look) {
        add_recipe(scene, reg, r, name, parent)?;
    }
    Ok(id)
}

/// Adds a Particles thing filled in by `r`, named `name`, under `parent` (and its children
/// under it).
fn add_recipe(scene: &mut Scene, reg: &Registry, r: Recipe, name: &str, parent: ThingId) -> Result<ThingId, EditError> {
    let id = crate::add_thing(scene, reg, KIND, parent)?;
    if let Some(t) = scene.things.get_mut(&id) {
        t.name.clear();
    }
    let name = scene.free_name(name);
    if let Some(t) = scene.things.get_mut(&id) {
        t.params = r.source.to_params();
        t.name = name;
        if let Some(at) = r.position {
            t.transform.position = at;
        }
    }
    crate::riding::fit_choice(scene, id);
    if let Some(card) = scene.things.get_mut(&id).and_then(|t| t.slots.get_mut(&SlotId::Bounce)) {
        card.params = r.meets.to_params();
    }
    // The first thing that follows the air gives the scene its Air card.
    if r.forces.iter().any(|(kind, _)| super::air::wants_air_card(kind)) {
        super::air::ensure_air_card(scene, reg)?;
    }
    for (kind, params) in r.forces {
        let layer = scene.add_layer(reg, id, SlotId::Motion, kind)?;
        put_layer(scene, id, SlotId::Motion, layer, params);
    }
    for l in r.layers {
        let layer = scene.add_layer(reg, id, SlotId::Look, l.kind)?;
        put_layer(scene, id, SlotId::Look, layer, l.params);
        for (kind, params) in l.subs {
            let sub = scene.add_sub_layer(reg, id, SlotId::Look, layer, kind)?;
            put_layer(scene, id, SlotId::Look, sub, params);
        }
    }
    for (name, child) in r.children {
        add_recipe(scene, reg, child, name, id)?;
    }
    Ok(id)
}
