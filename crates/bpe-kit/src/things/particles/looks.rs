//! The starting looks the Add thing menu's **Particles** heading offers: each adds a Particles
//! thing whose Source, forces, Meets things card and Look layers are filled in for that look.
//! They only fill settings when the owner picks one; there are no built-in scenes.

use bpe_core::{Color, EditError, Gradient, Mix, Params, Registry, Scene, Settings, SlotId, ThingId, layered_key};

use super::forces::{AIR, FollowAirForce, WANDER, WIND, WanderForce, WindForce};
use super::forces::{DRAG, DragForce, GRAVITY, GravityForce, RISE, RiseForce, SWAY, SWIRL, SwayForce, SwirlForce};
use super::look::{
    FLAME, FlameLayer, GLITTER, GlitterLayer, MoreShape, Only, SHAPES, SMOKE, SPRITES, STREAKS, SUB_GLOW, ShapesLayer,
    SmokeLayer, SpritesLayer, StreaksLayer, WEB, WebLayer,
};
use super::meets::{AtBalls, AtEdges, AtShapes, MeetsSettings};
use super::{AreaShape, BornFrom, Edge, Emit, Heading, KIND, ParticlesSettings, RIDES, RidesOn};
use crate::modules::look::layers::particles::{ParticleFade, ShapeMadeOf, SizeOverLife};
use crate::modules::look::shape::ShapeSubGlow;
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
    DustClouds,
    ThickFog,
    Glitter,
    WebOfLight,
    PixelGlitch,
}

/// The menu's groups of looks, in order, after Empty.
pub const GROUPS: [(&str, &[Look]); 4] = [
    (
        "Weather and nature",
        &[Look::Rain, Look::Snow, Look::Hail, Look::BlossomPetals, Look::Sandstorm, Look::DustClouds, Look::ThickFog],
    ),
    ("Fire and energy", &[Look::FireOnABall, Look::SparkFountain, Look::EmbersAndAsh]),
    ("Space and sparkle", &[Look::Fireflies, Look::Confetti, Look::Glitter]),
    ("Experimental and psychedelic", &[Look::RainbowSmokeTrail, Look::InkInWater, Look::WebOfLight, Look::PixelGlitch]),
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
            Look::PixelGlitch => "Pixel glitch",
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
            Look::ThickFog => "A bank of fog that drifts with the air and swirls round the balls passing through.",
            Look::Glitter => "Glitter drifting over the whole picture, its facets flashing as they turn.",
            Look::WebOfLight => "Drifting points of light joined by lines to their neighbours and the balls.",
            Look::PixelGlitch => "Square pixels on a grid flying off each ball (inside a group of balls) or a point.",
        }
    }

    /// Whether it rides what it sits in when it can.
    fn rides(self) -> bool {
        matches!(self, Look::Empty | Look::FireOnABall | Look::RainbowSmokeTrail | Look::PixelGlitch)
    }
}

/// One Look layer of a recipe, with its sub-layers.
struct LayerRecipe {
    kind: &'static str,
    params: Params,
    subs: Vec<(&'static str, Params)>,
}

/// What a look fills in.
struct Recipe {
    source: ParticlesSettings,
    forces: Vec<(&'static str, Params)>,
    meets: MeetsSettings,
    layers: Vec<LayerRecipe>,
}

fn gradient(colors: &[Color]) -> Gradient {
    Gradient::spread(colors, Mix::Even, false)
}

fn paint(colors: &[Color], follow: Follow) -> Paint {
    Paint { colours: gradient(colors), follow, ..Paint::default() }
}

fn rainbow(follow: Follow) -> Paint {
    let colours = [
        Color::rgb(255, 60, 60),
        Color::rgb(255, 200, 40),
        Color::rgb(60, 230, 90),
        Color::rgb(40, 160, 255),
        Color::rgb(190, 70, 255),
    ];
    Paint { colours: Gradient::spread(&colours, Mix::RoundTheWheel, true), follow, ..Paint::default() }
}

fn glow(size: f32, strength: f32) -> (&'static str, Params) {
    (SUB_GLOW, ShapeSubGlow { size, strength, ..ShapeSubGlow::default() }.to_params())
}

fn sprites(s: SpritesLayer, subs: Vec<(&'static str, Params)>) -> LayerRecipe {
    LayerRecipe { kind: SPRITES, params: s.to_params(), subs }
}

fn streaks(s: StreaksLayer, subs: Vec<(&'static str, Params)>) -> LayerRecipe {
    LayerRecipe { kind: STREAKS, params: s.to_params(), subs }
}

fn layer(kind: &'static str, params: Params, subs: Vec<(&'static str, Params)>) -> LayerRecipe {
    LayerRecipe { kind, params, subs }
}

/// What `look` fills in; `rides` when it sits in something it can ride.
fn recipe(look: Look, rides: bool) -> Recipe {
    let d = ParticlesSettings::default();
    let riding = |s: ParticlesSettings| {
        if rides { ParticlesSettings { from: BornFrom::RidingItsParent, rides_on: RidesOn::EachBall, ..s } } else { s }
    };
    let top = ParticlesSettings { from: BornFrom::WholeScreen, edge: Edge::Top, ..d.clone() };
    let vanish = MeetsSettings { at_edges: AtEdges::Vanish, ..MeetsSettings::default() };
    match look {
        Look::Empty => Recipe {
            source: riding(d.clone()),
            forces: Vec::new(),
            meets: MeetsSettings::default(),
            layers: vec![sprites(SpritesLayer::default(), Vec::new())],
        },
        Look::Rain => Recipe {
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
                ..top.clone()
            },
            forces: vec![
                (GRAVITY, GravityForce { strength: 1200.0, ..Default::default() }.to_params()),
                (WIND, WindForce { speed: 120.0, grip: 0.5, gusts: 0.3, ..Default::default() }.to_params()),
            ],
            meets: MeetsSettings { push: 0.8, ..vanish.clone() },
            layers: vec![streaks(
                StreaksLayer {
                    stretch: 0.025,
                    width: 0.7,
                    colour: paint(&[Color::rgb(190, 215, 255)], Follow::Life),
                    opacity: 0.7,
                    fade: ParticleFade::None,
                    ..Default::default()
                },
                Vec::new(),
            )],
        },
        Look::Snow => Recipe {
            source: ParticlesSettings {
                rate: 80.0,
                life: 14.0,
                speed: 60.0,
                angle: 90.0,
                spread: 30.0,
                size: 3.0,
                size_spread: 0.5,
                ..top.clone()
            },
            forces: vec![
                (GRAVITY, GravityForce { strength: 40.0, ..Default::default() }.to_params()),
                (SWAY, SwayForce { sway: 25.0, sway_speed: 0.6, ..Default::default() }.to_params()),
                (WANDER, WanderForce { strength: 30.0, turns: 0.5, ..Default::default() }.to_params()),
                (DRAG, DragForce { drag: 0.5, ..Default::default() }.to_params()),
            ],
            meets: MeetsSettings { push: 0.8, ..vanish.clone() },
            layers: vec![sprites(
                SpritesLayer { softness: 0.7, fade: ParticleFade::None, ..Default::default() },
                Vec::new(),
            )],
        },
        Look::Hail => Recipe {
            source: ParticlesSettings {
                rate: 120.0,
                life: 4.0,
                speed: 700.0,
                angle: 95.0,
                spread: 6.0,
                size: 3.5,
                size_spread: 0.4,
                ..top.clone()
            },
            forces: vec![(GRAVITY, GravityForce { strength: 1400.0, ..Default::default() }.to_params())],
            meets: MeetsSettings {
                at_balls: AtBalls::BounceOff,
                at_shapes: AtShapes::Bounce,
                bounciness: 0.45,
                friction: 0.2,
                ..vanish.clone()
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
        },
        Look::BlossomPetals => Recipe {
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
                ..top.clone()
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
            meets: vanish.clone(),
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
        },
        Look::Sandstorm => Recipe {
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
                ..d.clone()
            },
            forces: vec![
                (
                    WIND,
                    WindForce {
                        speed: 600.0,
                        grip: 1.5,
                        gusts: 0.6,
                        gust_every: 1.5,
                        waves: 700.0,
                        ..Default::default()
                    }
                    .to_params(),
                ),
                (SWIRL, SwirlForce { strength: 250.0, size: 220.0, drift: 0.0, ..Default::default() }.to_params()),
                (GRAVITY, GravityForce { strength: 30.0, ..Default::default() }.to_params()),
            ],
            meets: MeetsSettings { push: 0.6, ..vanish.clone() },
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
        },
        Look::FireOnABall => Recipe {
            source: riding(ParticlesSettings {
                from: BornFrom::APoint,
                rate: 220.0,
                life: 0.8,
                life_spread: 0.4,
                speed: 60.0,
                heading: Heading::AllRound,
                inherit: 0.3,
                size: 7.0,
                size_spread: 0.5,
                ..d.clone()
            }),
            forces: vec![
                (RISE, RiseForce { strength: 500.0, cools: 0.6, ..Default::default() }.to_params()),
                (WANDER, WanderForce { strength: 300.0, turns: 3.0, ..Default::default() }.to_params()),
                (DRAG, DragForce { drag: 1.5, ..Default::default() }.to_params()),
            ],
            meets: MeetsSettings::default(),
            layers: vec![
                layer(FLAME, FlameLayer::default().to_params(), Vec::new()),
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
        },
        Look::SparkFountain => Recipe {
            source: ParticlesSettings {
                from: BornFrom::APoint,
                rate: 150.0,
                life: 2.2,
                speed: 700.0,
                speed_spread: 0.3,
                angle: -90.0,
                spread: 30.0,
                size: 1.5,
                ..d.clone()
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
                vec![glow(4.0, 0.5)],
            )],
        },
        Look::EmbersAndAsh => Recipe {
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
                ..d.clone()
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
                    vec![glow(5.0, 0.6)],
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
        },
        Look::Fireflies => Recipe {
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
                ..d.clone()
            },
            forces: vec![
                (WANDER, WanderForce { strength: 60.0, turns: 0.5, ..Default::default() }.to_params()),
                (DRAG, DragForce { drag: 0.8, ..Default::default() }.to_params()),
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
                vec![glow(10.0, 0.7)],
            )],
        },
        Look::Confetti => Recipe {
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
                ..d.clone()
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
        },
        Look::RainbowSmokeTrail => Recipe {
            source: riding(ParticlesSettings {
                from: BornFrom::APoint,
                rate: 120.0,
                life: 2.5,
                speed: 20.0,
                heading: Heading::AllRound,
                size: 10.0,
                size_spread: 0.4,
                ..d.clone()
            }),
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
                    colour: rainbow(Follow::Time),
                    opacity: 0.7,
                    shading: 0.4,
                    dark_inside: 0.2,
                    ..Default::default()
                }
                .to_params(),
                Vec::new(),
            )],
        },
        Look::InkInWater => Recipe {
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
                ..d.clone()
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
        },
        Look::Glitter => Recipe {
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
                ..d.clone()
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
        },
        Look::WebOfLight => Recipe {
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
                ..d.clone()
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
        },
        Look::PixelGlitch => Recipe {
            source: riding(ParticlesSettings {
                from: BornFrom::APoint,
                rate: 160.0,
                life: 1.2,
                life_spread: 0.5,
                speed: 70.0,
                heading: Heading::AllRound,
                inherit: 0.2,
                size: 4.0,
                size_spread: 0.3,
                ..d.clone()
            }),
            forces: vec![
                (WANDER, WanderForce { strength: 250.0, turns: 4.0, ..Default::default() }.to_params()),
                (DRAG, DragForce { drag: 2.0, ..Default::default() }.to_params()),
            ],
            meets: MeetsSettings::default(),
            layers: vec![layer(
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
            )],
        },
        Look::DustClouds => Recipe {
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
                ..d.clone()
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
        },
        Look::ThickFog => Recipe {
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
                ..d.clone()
            },
            forces: vec![
                (AIR, FollowAirForce { grip: 1.5, ..Default::default() }.to_params()),
                (WANDER, WanderForce { strength: 8.0, turns: 0.3, ..Default::default() }.to_params()),
                (DRAG, DragForce { drag: 0.6, ..Default::default() }.to_params()),
            ],
            meets: MeetsSettings { push: 0.3, ..MeetsSettings::default() },
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
        },
    }
}

/// Whether a Particles thing under `parent` can ride it.
pub fn can_ride(scene: &Scene, parent: ThingId) -> bool {
    scene.things.get(&parent).is_some_and(|t| RIDES.contains(&t.kind.as_str()))
}

/// Writes `params` into layer `layer` of `id`'s card `slot`.
fn put_layer(scene: &mut Scene, id: ThingId, slot: SlotId, layer: bpe_core::LayerId, params: Params) {
    if let Some(card) = scene.things.get_mut(&id).and_then(|t| t.slots.get_mut(&slot)) {
        for (key, value) in params.0 {
            card.params.0.insert(layered_key(layer, &key), value);
        }
    }
}

/// Adds a Particles thing with `look` under `parent`, named after the look.
pub fn add_particles(scene: &mut Scene, reg: &Registry, look: Look, parent: ThingId) -> Result<ThingId, EditError> {
    let id = crate::add_thing(scene, reg, KIND, parent)?;
    let rides = look.rides() && can_ride(scene, parent);
    let r = recipe(look, rides);
    if let Some(t) = scene.things.get_mut(&id) {
        t.name.clear();
    }
    let name = scene.free_name(look.name());
    if let Some(t) = scene.things.get_mut(&id) {
        t.params = r.source.to_params();
        t.name = name;
    }
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
    Ok(id)
}
