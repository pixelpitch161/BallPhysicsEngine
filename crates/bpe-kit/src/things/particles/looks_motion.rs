//! The starting looks part 3's motion makes possible (particles spec section 2): Galaxy round a
//! ball, Flow lines, Form a word, Flock, Shockwaves, Comet tail, Black hole, Iron filings, Beat
//! grid, Moiré and Spirograph. Like every look they only fill in settings.

use bpe_core::{Blend, Color, Params, Settings};
use glam::Vec2;

use super::forces::{DRAG, DragForce, OnlyFor};
use super::look::{MoreShape, Only, PointAlong, SHAPES, ShapesLayer, SpritesLayer, StreaksLayer};
use super::looks::{LayerRecipe, Look, Recipe, glow, layer, paint, rainbow, sprites, streaks};
use super::meets::{AtEdges, MeetsSettings};
use super::more_forces::{Around, Falloff};
use super::more_forces::{
    FLOCK, FLOW, FieldFrom, FlockForce, FlowForce, MAGNETIC, MagneticForce, PULL, PULSE, PUSH, PullForce, PulseForce,
    PushForce, RIPPLE, RippleForce, SPIROGRAPH, SPRING, SpirographForce, SpringForce, Toward,
};
use super::{AreaShape, BornFrom, Emit, GridCovers, Heading, ParticlesSettings, RidesOn};
use crate::modules::look::layers::particles::{ParticleFade, ShapeMadeOf, SizeOverLife};
use crate::paint::Follow;

/// Long enough to live for the whole video: crowds that hold a shape.
const FOREVER: f32 = 100_000.0;

/// A crowd that holds still and lasts: no speed, no fading, no shrinking.
fn still_crowd(s: ParticlesSettings) -> ParticlesSettings {
    ParticlesSettings { emit: Emit::KeepACrowd, life: FOREVER, life_spread: 0.0, speed: 0.0, speed_spread: 0.0, ..s }
}

/// A Sprites layer of `made_of` dots that never fade or shrink.
fn steady(made_of: ShapeMadeOf, softness: f32, colour: crate::paint::Paint) -> SpritesLayer {
    SpritesLayer {
        made_of,
        softness,
        colour,
        fade: ParticleFade::None,
        size_curve: SizeOverLife::Even,
        ..Default::default()
    }
}

/// What part 3's `look` fills in; `rides` when it sits in something it can ride.
pub(super) fn recipe(look: Look, rides: bool) -> Recipe {
    let d = ParticlesSettings::default();
    let push = |push: f32, reach: f32| MeetsSettings { push, reach, ..MeetsSettings::default() };
    let screen_grid = |spacing: f32| ParticlesSettings {
        from: BornFrom::AGrid,
        grid_covers: GridCovers::TheScreen,
        spacing,
        ..still_crowd(d.clone())
    };
    match look {
        Look::Galaxy => Recipe {
            position: None,
            children: Vec::new(),
            source: ParticlesSettings {
                emit: Emit::KeepACrowd,
                count: 900,
                life: 6.0,
                life_spread: 0.5,
                speed: 300.0,
                speed_spread: 0.35,
                heading: Heading::RoundTheMiddle,
                spread: 20.0,
                inherit: 1.0,
                size: 2.2,
                size_spread: 0.5,
                ..if rides {
                    ParticlesSettings { from: BornFrom::RidingItsParent, rides_on: RidesOn::TheEdge, ..d.clone() }
                } else {
                    ParticlesSettings {
                        from: BornFrom::AnArea,
                        area: AreaShape::Ring,
                        radius: 320.0,
                        inner: 60.0,
                        ..d.clone()
                    }
                }
            },
            forces: vec![(
                PULL,
                PullForce {
                    toward: if rides { Toward::ItsBall } else { Toward::ItsPlace },
                    strength: 900.0,
                    falloff: Falloff::WeakerFurther,
                    at: 120.0,
                    ..Default::default()
                }
                .to_params(),
            )],
            meets: push(0.5, 2.0),
            layers: vec![sprites(
                SpritesLayer {
                    made_of: ShapeMadeOf::Dots,
                    softness: 0.6,
                    colour: paint(
                        &[Color::rgb(200, 220, 255), Color::rgb(170, 140, 255), Color::rgb(220, 90, 255)],
                        Follow::Order,
                    ),
                    opacity: 0.8,
                    fade: ParticleFade::Both,
                    size_curve: SizeOverLife::Even,
                    blend: Blend::Add,
                    ..Default::default()
                },
                vec![glow(6.0, 0.5)],
            )],
        },
        Look::FlowLines => Recipe {
            position: None,
            children: Vec::new(),
            source: ParticlesSettings {
                emit: Emit::KeepACrowd,
                count: 4000,
                life: 8.0,
                life_spread: 0.5,
                speed: 0.0,
                size: 1.4,
                ..d.clone()
            },
            forces: vec![(FLOW, FlowForce::default().to_params())],
            meets: MeetsSettings { at_edges: AtEdges::WrapRound, ..push(0.9, 3.0) },
            layers: vec![streaks(
                StreaksLayer {
                    stretch: 0.12,
                    width: 0.8,
                    colour: rainbow(Follow::Place),
                    opacity: 0.6,
                    fade: ParticleFade::Both,
                    blend: Blend::Add,
                    ..Default::default()
                },
                Vec::new(),
            )],
        },
        Look::FormAWord => Recipe {
            position: None,
            children: Vec::new(),
            source: ParticlesSettings {
                from: BornFrom::AWordOrPicture,
                letter_height: 220.0,
                spacing: 6.0,
                size: 2.2,
                size_spread: 0.2,
                ..still_crowd(d.clone())
            },
            forces: vec![(SPRING, SpringForce { stiffness: 26.0, damping: 4.0, ..Default::default() }.to_params())],
            meets: push(1.0, 2.6),
            layers: vec![sprites(
                SpritesLayer {
                    opacity: 0.85,
                    blend: Blend::Add,
                    ..steady(
                        ShapeMadeOf::Dots,
                        0.5,
                        paint(&[Color::rgb(120, 230, 255), Color::rgb(255, 80, 200)], Follow::Speed),
                    )
                },
                Vec::new(),
            )],
        },
        Look::Flock => Recipe {
            position: None,
            children: Vec::new(),
            source: ParticlesSettings {
                from: BornFrom::AnArea,
                area: AreaShape::Rectangle,
                width: 900.0,
                height: 800.0,
                emit: Emit::KeepACrowd,
                count: 180,
                life: FOREVER,
                life_spread: 0.0,
                speed: 300.0,
                heading: Heading::AllRound,
                size: 6.0,
                size_spread: 0.2,
                ..d.clone()
            },
            forces: vec![
                (FLOCK, FlockForce::default().to_params()),
                (PUSH, PushForce { strength: 3000.0, ball_reach: 5.0, ..Default::default() }.to_params()),
            ],
            meets: MeetsSettings { at_edges: AtEdges::Bounce, edge_margin: -60.0, ..push(0.0, 2.0) },
            // Birds that point the way they fly (part 5's More shapes).
            layers: vec![layer(
                SHAPES,
                ShapesLayer {
                    shape: MoreShape::Birds,
                    point_along: PointAlong::WayItGoes,
                    softness: 0.2,
                    colour: paint(&[Color::rgb(200, 244, 255)], Follow::Life),
                    fade: ParticleFade::None,
                    size_curve: SizeOverLife::Even,
                    ..Default::default()
                }
                .to_params(),
                Vec::new(),
            )],
        },
        Look::Shockwaves => Recipe {
            position: None,
            children: Vec::new(),
            source: ParticlesSettings { size: 2.4, size_spread: 0.3, count: 1500, ..still_crowd(d.clone()) },
            forces: vec![
                (RIPPLE, RippleForce { width: 50.0, strength: 5000.0, ..Default::default() }.to_params()),
                (SPRING, SpringForce { stiffness: 3.0, damping: 2.5, ..Default::default() }.to_params()),
            ],
            meets: push(0.6, 2.2),
            layers: vec![sprites(
                SpritesLayer {
                    blend: Blend::Add,
                    opacity: 0.8,
                    ..steady(
                        ShapeMadeOf::Dots,
                        0.6,
                        paint(&[Color::rgb(70, 130, 170), Color::rgb(220, 250, 255)], Follow::Speed),
                    )
                },
                vec![glow(5.0, 0.4)],
            )],
        },
        Look::CometTail => {
            let sun = Vec2::new(-1500.0, -1500.0);
            let away = |strength: f32, only: OnlyFor| {
                PushForce {
                    toward: Toward::APoint,
                    point: sun,
                    strength,
                    falloff: Falloff::Even,
                    only,
                    share: 0.5,
                    ..Default::default()
                }
                .to_params()
            };
            let tail = |only: Only, made_of: ShapeMadeOf, colour: Color, opacity: f32| {
                sprites(
                    SpritesLayer {
                        made_of,
                        softness: 1.0,
                        only,
                        share: 0.5,
                        colour: paint(&[colour], Follow::Life),
                        opacity,
                        size_curve: SizeOverLife::Grow,
                        blend: Blend::Add,
                        ..Default::default()
                    },
                    Vec::new(),
                )
            };
            Recipe {
                position: None,
                children: Vec::new(),
                source: ParticlesSettings {
                    rate: 240.0,
                    life: 1.8,
                    life_spread: 0.4,
                    speed: 40.0,
                    speed_spread: 0.5,
                    heading: Heading::AllRound,
                    inherit: 0.6,
                    size: 5.0,
                    size_spread: 0.4,
                    ..if rides {
                        ParticlesSettings { from: BornFrom::RidingItsParent, rides_on: RidesOn::EachBall, ..d.clone() }
                    } else {
                        ParticlesSettings { from: BornFrom::APoint, ..d.clone() }
                    }
                },
                forces: vec![(PUSH, away(1100.0, OnlyFor::AShare)), (PUSH, away(140.0, OnlyFor::TheRest))],
                meets: MeetsSettings::default(),
                layers: vec![
                    tail(Only::AShare, ShapeMadeOf::Smoke, Color::rgb(110, 170, 255), 0.35),
                    tail(Only::TheRest, ShapeMadeOf::Dots, Color::rgb(255, 205, 120), 0.3),
                ],
            }
        }
        Look::BlackHole => Recipe {
            position: None,
            children: Vec::new(),
            source: ParticlesSettings {
                from: BornFrom::AnArea,
                area: AreaShape::Ring,
                radius: 340.0,
                inner: 60.0,
                emit: Emit::KeepACrowd,
                count: 2500,
                life: 8.0,
                life_spread: 0.5,
                speed: 380.0,
                speed_spread: 0.3,
                heading: Heading::RoundTheMiddle,
                spread: 10.0,
                size: 1.5,
                ..d.clone()
            },
            forces: vec![
                (
                    PULL,
                    PullForce {
                        strength: 1500.0,
                        falloff: Falloff::MuchWeakerFurther,
                        at: 120.0,
                        swallow: 28.0,
                        ..Default::default()
                    }
                    .to_params(),
                ),
                (DRAG, DragForce { drag: 0.15, ..Default::default() }.to_params()),
            ],
            meets: push(0.6, 2.0),
            layers: vec![streaks(
                StreaksLayer {
                    stretch: 0.03,
                    width: 0.8,
                    colour: paint(
                        &[
                            Color::rgb(80, 30, 140),
                            Color::rgb(200, 60, 120),
                            Color::rgb(255, 170, 60),
                            Color::rgb(255, 250, 230),
                        ],
                        Follow::Speed,
                    ),
                    opacity: 0.6,
                    fade: ParticleFade::Both,
                    blend: Blend::Add,
                    ..Default::default()
                },
                Vec::new(),
            )],
        },
        Look::IronFilings => Recipe {
            position: None,
            children: Vec::new(),
            source: ParticlesSettings { jitter: 0.3, size: 2.2, turn_spread: 1.0, ..screen_grid(30.0) },
            forces: vec![
                (MAGNETIC, MagneticForce { from: FieldFrom::EachBall, pull: 240.0, ..Default::default() }.to_params()),
                (SPRING, SpringForce { stiffness: 20.0, damping: 5.0, ..Default::default() }.to_params()),
            ],
            meets: push(0.0, 2.0),
            layers: vec![sprites(
                steady(ShapeMadeOf::Streaks, 0.1, paint(&[Color::rgb(201, 206, 216)], Follow::Life)),
                Vec::new(),
            )],
        },
        Look::BeatGrid => Recipe {
            position: None,
            children: Vec::new(),
            source: ParticlesSettings { size: 5.0, ..screen_grid(52.0) },
            forces: vec![
                (PULSE, PulseForce { strength: 260.0, decay: 0.06, ..Default::default() }.to_params()),
                (
                    RIPPLE,
                    RippleForce { speed: 640.0, width: 60.0, strength: 3000.0, lasts: 1.5, ..Default::default() }
                        .to_params(),
                ),
                (SPRING, SpringForce { stiffness: 60.0, damping: 6.0, ..Default::default() }.to_params()),
            ],
            meets: push(0.6, 2.4),
            layers: vec![sprites(steady(ShapeMadeOf::Dots, 0.3, rainbow(Follow::Place)), Vec::new())],
        },
        Look::Moire => moire(false),
        Look::Spirograph => Recipe {
            position: None,
            children: Vec::new(),
            source: ParticlesSettings {
                count: 1500,
                size: 2.0,
                ..still_crowd(if rides {
                    ParticlesSettings { from: BornFrom::RidingItsParent, rides_on: RidesOn::EachBall, ..d.clone() }
                } else {
                    ParticlesSettings { from: BornFrom::APoint, ..d.clone() }
                })
            },
            forces: vec![(
                SPIROGRAPH,
                SpirographForce {
                    round: if rides { Around::ItsBall } else { Around::ItsPlace },
                    speed: 0.15,
                    differ: 0.03,
                    ..Default::default()
                }
                .to_params(),
            )],
            meets: MeetsSettings { push: 0.0, ..MeetsSettings::default() },
            layers: vec![sprites(
                SpritesLayer { blend: Blend::Add, ..steady(ShapeMadeOf::Dots, 0.5, rainbow(Follow::Order)) },
                vec![glow(4.0, 0.4)],
            )],
        },
        _ => unreachable!("{look:?} is a part 1 look"),
    }
}

/// One of Moiré's two grids: the still one, or the `turning` one the balls push.
fn moire(turning: bool) -> Recipe {
    let colour = if turning { Color::rgb(255, 79, 160) } else { Color::rgb(233, 240, 255) };
    Recipe {
        position: None,
        children: Vec::new(),
        source: ParticlesSettings {
            from: BornFrom::AGrid,
            grid_covers: GridCovers::TheScreen,
            spacing: 22.0,
            margin: 600.0,
            size: 2.2,
            size_spread: 0.0,
            homes_turn: if turning { 1.5 } else { 0.0 },
            most: 40_000,
            ..still_crowd(ParticlesSettings::default())
        },
        forces: if turning {
            vec![(SPRING, SpringForce { stiffness: 40.0, damping: 5.0, ..Default::default() }.to_params())]
        } else {
            Vec::new()
        },
        meets: MeetsSettings { push: if turning { 0.8 } else { 0.0 }, reach: 3.0, ..MeetsSettings::default() },
        layers: vec![LayerRecipe {
            kind: super::look::SPRITES,
            params: steady(ShapeMadeOf::Dots, 0.1, paint(&[colour], Follow::Life)).to_params(),
            subs: Vec::<(&'static str, Params)>::new(),
        }],
    }
}

/// The other things a look adds beside its first, with their names.
pub(super) fn also(look: Look) -> Vec<(&'static str, Recipe)> {
    match look {
        Look::Moire => vec![("Moiré 2", moire(true))],
        _ => Vec::new(),
    }
}
