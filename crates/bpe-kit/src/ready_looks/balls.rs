//! The ball looks (Balls and Emitters): five families of six.

use bpe_core::{Blend, Choice, Color, Gradient, Pulse, PulseSource, SoundSource, ThingId, Value};

use super::{RLayer, RPulse, ReadyLook, Recipe, Wears, own};
use crate::modules::look::layers::particles::{MadeOf, ParticlesLayer};
use crate::modules::look::layers::trail::{TrailFade, TrailKind, TrailLayer};
use crate::modules::look::layers::{
    self as l, BodyLayer, ColourFrom, ColourOverTime, FlashOnHit, GlowLayer, OutlineLayer, OutlineSits, PatternLayer,
    ShineLayer, ShineLight, ShineMaterial, Turning, arcs::ArcsLayer, gives_light, heat,
};
use crate::modules::look::surface::{SurfaceKind, preset_values};
use crate::paint::{Follow, InsideFrom, InsidePaint, Paint, Pattern};

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::rgb(r, g, b)
}

fn solid(c: Color) -> Gradient {
    Gradient::solid(c)
}

fn even(cs: &[Color]) -> Gradient {
    Gradient::even(cs)
}

/// A Body painted `colours` across each ball, `opacity` solid.
fn body(colours: Gradient, opacity: f32) -> RLayer {
    let fill = Paint { colours, follow: Follow::EachBall, ..Paint::default() };
    RLayer::new(l::BODY, own(&BodyLayer { fill, opacity, ..BodyLayer::default() }))
}

/// A Body whose colours run out from its middle.
fn round_body(colours: Gradient) -> RLayer {
    body(colours, 1.0).set("colours~form", Value::Choice("round".into()))
}

fn shine(material: ShineMaterial, strength: f32) -> ShineLayer {
    ShineLayer { material, strength, ..ShineLayer::default() }
}

fn shine_layer(s: ShineLayer) -> RLayer {
    RLayer::new(l::SHINE, own(&s))
}

fn outline(width: f32, sits: OutlineSits, colour: Gradient) -> RLayer {
    let o = OutlineLayer { width, sits, colour_from: ColourFrom::Own, colour, ..OutlineLayer::default() };
    RLayer::new(l::OUTLINE, own(&o))
}

fn glow(strength: f32, size: f32, colour: Gradient) -> RLayer {
    let g = GlowLayer { strength, size, colour_from: ColourFrom::Own, colour, ..GlowLayer::default() };
    RLayer::new(l::GLOW, own(&g))
}

fn pattern(p: InsidePaint, opacity: f32, blend: Blend) -> RLayer {
    RLayer::new(l::PATTERN, own(&PatternLayer { inside: p, opacity, blend }))
}

fn flash(brightness: f32) -> RLayer {
    RLayer::new(l::FLASH, own(&FlashOnHit { flash: brightness, ..FlashOnHit::default() }))
}

fn turning(spin: f32) -> RLayer {
    RLayer::new(l::TURNING, own(&Turning { spin, ..Turning::default() }))
}

fn shader(effect: &str) -> RLayer {
    RLayer::new(l::SHADER, bpe_core::Params::default()).set("effect", Value::Choice(effect.into()))
}

fn trail(t: TrailLayer) -> RLayer {
    RLayer::new(l::TRAIL, own(&t))
}

/// The card's Surface (how it takes light when the scene is lit).
fn surface(kind: SurfaceKind) -> bpe_core::Params {
    let mut p = bpe_core::Params::default();
    p.set("surface", Value::Choice(kind.name().into()));
    for (k, v) in preset_values(kind) {
        p.set(k, v);
    }
    p
}

fn plain(layers: Vec<RLayer>) -> Recipe {
    Recipe { layers, ..Recipe::default() }
}

fn with_surface(layers: Vec<RLayer>, kind: SurfaceKind) -> Recipe {
    Recipe { layers, card: surface(kind), ..Recipe::default() }
}

/// The first colour of `k`.
fn first(k: &Gradient) -> Color {
    k.first()
}

#[allow(clippy::too_many_arguments)]
const fn look(
    id: &'static str,
    name: &'static str,
    family: &'static str,
    help: &'static str,
    takes_colour: bool,
    main: Color,
    swatch: [Color; 2],
    build: fn(&Gradient) -> Recipe,
) -> ReadyLook {
    ReadyLook { id, name, family, help, wears: Wears::Balls, takes_colour, main, swatch, build }
}

const WHITE: Color = Color::WHITE;
const BLACK: Color = rgb(0, 0, 0);
const INK: Color = rgb(0x14, 0x14, 0x16);
const CYAN: Color = rgb(0x4f, 0xc3, 0xf0);
const PINK: Color = rgb(0xff, 0x5c, 0x8a);
const ORANGE: Color = rgb(0xff, 0x8a, 0x2a);
const GOLD: Color = rgb(0xf2, 0xc2, 0x4a);
const VIOLET: Color = rgb(0xa9, 0x8b, 0xff);

pub fn looks() -> Vec<ReadyLook> {
    vec![
        // ---------- Clean ----------
        look(
            "plain",
            "Plain white",
            "Clean",
            "A plain white ball, as a new ball starts.",
            true,
            WHITE,
            [WHITE, WHITE],
            |k| plain(vec![body(k.clone(), 1.0)]),
        ),
        look(
            "pastel",
            "Pastel",
            "Clean",
            "A soft pastel ball with a gentle matte sheen.",
            true,
            rgb(0xf6, 0xb3, 0xcd),
            [rgb(0xf6, 0xb3, 0xcd), rgb(0xff, 0xe0, 0xec)],
            |k| plain(vec![body(k.clone(), 1.0), shine_layer(shine(ShineMaterial::Matte, 0.5))]),
        ),
        look(
            "candy",
            "Candy",
            "Clean",
            "A bright glossy sweet with a sharp white highlight.",
            true,
            PINK,
            [PINK, WHITE],
            |k| {
                with_surface(
                    vec![
                        body(k.clone(), 1.0),
                        shine_layer(ShineLayer { softness: 0.3, ..shine(ShineMaterial::Glossy, 1.0) }),
                    ],
                    SurfaceKind::Plastic,
                )
            },
        ),
        look(
            "tennis",
            "Tennis",
            "Clean",
            "A fuzzy yellow-green ball with its curved seam.",
            true,
            rgb(0xc8, 0xe8, 0x3a),
            [rgb(0xc8, 0xe8, 0x3a), WHITE],
            |k| {
                let seam = InsidePaint { pattern: Pattern::Swirl, shade: 0.6, size: 0.6, ..InsidePaint::default() };
                plain(vec![
                    body(k.clone(), 1.0),
                    pattern(seam, 0.8, Blend::Normal),
                    shine_layer(shine(ShineMaterial::Matte, 0.7)),
                ])
            },
        ),
        look("ink", "Ink", "Clean", "A black ball drawn with a clean white ring.", true, WHITE, [INK, WHITE], |k| {
            plain(vec![body(solid(INK), 1.0), outline(3.0, OutlineSits::Inside, k.clone())])
        }),
        look(
            "billiard",
            "Billiard",
            "Clean",
            "A glossy pool ball with a white spot.",
            true,
            rgb(0xd0, 0x18, 0x18),
            [rgb(0xd0, 0x18, 0x18), WHITE],
            |k| {
                let spot = InsidePaint {
                    pattern: Pattern::Gradient,
                    form: bpe_core::GradientForm::Round,
                    from: InsideFrom::OwnGradient,
                    colours: hard(&[(0.0, WHITE), (0.42, WHITE), (0.42, first(k)), (1.0, first(k))]),
                    ..InsidePaint::default()
                };
                with_surface(
                    vec![
                        body(k.clone(), 1.0),
                        pattern(spot, 1.0, Blend::Normal),
                        shine_layer(shine(ShineMaterial::Glossy, 1.0)),
                    ],
                    SurfaceKind::Plastic,
                )
            },
        ),
        // ---------- Shiny ----------
        look(
            "chrome",
            "Chrome",
            "Shiny",
            "A mirror-like metal ball with a thin dark edge and a quick white flash on every hit.",
            false,
            rgb(0xc8, 0xc8, 0xc8),
            [rgb(0xe8, 0xe8, 0xe8), rgb(0x55, 0x55, 0x55)],
            |_| {
                let steel =
                    even(&[rgb(0xf4, 0xf4, 0xf4), rgb(0x9a, 0x9a, 0x9a), rgb(0x3a, 0x3a, 0x3a), rgb(0xd0, 0xd0, 0xd0)]);
                with_surface(
                    vec![
                        body(steel, 1.0),
                        shine_layer(ShineLayer { softness: 0.2, ..shine(ShineMaterial::Metal, 1.0) }),
                        outline(1.5, OutlineSits::Inside, solid(rgb(0x30, 0x30, 0x30))),
                        flash(0.6),
                    ],
                    SurfaceKind::Chrome,
                )
            },
        ),
        look(
            "gold",
            "Gold",
            "Shiny",
            "Warm polished gold with a soft golden halo.",
            true,
            GOLD,
            [GOLD, rgb(0x7a, 0x53, 0x10)],
            |k| {
                let warm = ShineLayer {
                    light: ShineLight::Own,
                    colour: solid(rgb(0xff, 0xf2, 0xc0)),
                    ..shine(ShineMaterial::Metal, 1.0)
                };
                with_surface(
                    vec![body(k.clone(), 1.0), shine_layer(warm), glow(0.35, 0.5, k.clone())],
                    SurfaceKind::Metal,
                )
            },
        ),
        look(
            "steel",
            "Steel",
            "Shiny",
            "Cool blue-grey brushed metal with a crisp edge.",
            true,
            rgb(0x8a, 0xa0, 0xb2),
            [rgb(0x8a, 0xa0, 0xb2), rgb(0x2f, 0x3b, 0x46)],
            |k| {
                with_surface(
                    vec![
                        body(k.clone(), 1.0),
                        shine_layer(ShineLayer { softness: 0.7, ..shine(ShineMaterial::Metal, 0.9) }),
                        outline(1.5, OutlineSits::Inside, solid(rgb(0x1c, 0x24, 0x2c))),
                    ],
                    SurfaceKind::Metal,
                )
            },
        ),
        look(
            "glass",
            "Glass",
            "Shiny",
            "A clear glass marble: see-through, with a bright edge.",
            true,
            rgb(0x9a, 0xd4, 0xff),
            [rgb(0x9a, 0xd4, 0xff), WHITE],
            |k| {
                with_surface(
                    vec![
                        body(k.clone(), 0.3),
                        shine_layer(shine(ShineMaterial::Glass, 1.0)),
                        outline(1.5, OutlineSits::Inside, solid(rgb(0xd8, 0xf0, 0xff))),
                    ],
                    SurfaceKind::Glass,
                )
            },
        ),
        look(
            "marble",
            "Marble",
            "Shiny",
            "A glass marble with a coloured swirl inside.",
            true,
            PINK,
            [rgb(0x9a, 0xd4, 0xff), PINK],
            |k| {
                let swirl = InsidePaint {
                    pattern: Pattern::Swirl,
                    from: InsideFrom::OwnGradient,
                    colours: even(&[first(k), rgb(0x9a, 0xd4, 0xff)]),
                    ..InsidePaint::default()
                };
                with_surface(
                    vec![
                        body(solid(rgb(0x9a, 0xd4, 0xff)), 0.35),
                        pattern(swirl, 0.85, Blend::Normal),
                        shine_layer(shine(ShineMaterial::Glass, 1.0)),
                    ],
                    SurfaceKind::Glass,
                )
            },
        ),
        look(
            "pearl",
            "Pearl",
            "Shiny",
            "A soft pearly sheen in pale shifting colours.",
            true,
            rgb(0xf3, 0xee, 0xe6),
            [rgb(0xf3, 0xee, 0xe6), rgb(0xcf, 0xc6, 0xd8)],
            |k| {
                let sheen = InsidePaint {
                    pattern: Pattern::Gradient,
                    form: bpe_core::GradientForm::Sweep,
                    from: InsideFrom::OwnGradient,
                    colours: even(&[
                        rgb(0xff, 0xe6, 0xf0),
                        rgb(0xe0, 0xf0, 0xff),
                        rgb(0xf0, 0xff, 0xe8),
                        rgb(0xff, 0xe6, 0xf0),
                    ]),
                    ..InsidePaint::default()
                };
                plain(vec![
                    body(k.clone(), 1.0),
                    pattern(sheen, 0.35, Blend::Multiply),
                    shine_layer(ShineLayer { softness: 0.85, size: 1.6, ..shine(ShineMaterial::Glossy, 0.8) }),
                ])
            },
        ),
        // ---------- Neon ----------
        look("neon_ring", "Neon ring", "Neon", "A dark ball with a glowing neon ring.", true, CYAN, [INK, CYAN], |k| {
            plain(vec![
                body(solid(rgb(0x0c, 0x10, 0x14)), 1.0),
                outline(3.0, OutlineSits::Inside, k.clone()),
                glow(1.0, 0.7, k.clone()),
            ])
        }),
        look(
            "glow_orb",
            "Glow orb",
            "Neon",
            "A white-hot core inside a big soft halo.",
            true,
            CYAN,
            [WHITE, CYAN],
            |k| plain(vec![round_body(even(&[WHITE, first(k)])), glow(1.2, 1.4, k.clone())]),
        ),
        look(
            "electric",
            "Electric",
            "Neon",
            "A glowing violet ball that throws lightning at balls near it.",
            true,
            VIOLET,
            [VIOLET, WHITE],
            |k| {
                let arcs = ArcsLayer { colour: first(k), ..ArcsLayer::default() };
                plain(vec![
                    round_body(even(&[rgb(0xf0, 0xe8, 0xff), first(k)])),
                    glow(1.0, 0.8, k.clone()),
                    RLayer::new(l::arcs::ARCS, own(&arcs)),
                ])
            },
        ),
        look(
            "laser",
            "Laser",
            "Neon",
            "A bright dot leaving a thin, long laser streak.",
            true,
            PINK,
            [PINK, WHITE],
            |k| {
                let t = TrailLayer {
                    kind: TrailKind::Streak,
                    seconds: 0.6,
                    width: 0.35,
                    brightness: 1.6,
                    blend: Blend::Add,
                    ..TrailLayer::default()
                };
                plain(vec![body(k.clone(), 1.0), glow(1.0, 0.6, k.clone()), trail(t)])
            },
        ),
        look(
            "pulse",
            "Pulse",
            "Neon",
            "A glowing ball whose glow beats with the music on Bus 1.",
            true,
            rgb(0x3f, 0xd6, 0xb5),
            [rgb(0x3f, 0xd6, 0xb5), WHITE],
            |k| {
                let pulse = Pulse {
                    source: PulseSource::Sound { shape: ThingId::ROOT, source: SoundSource::Bus(1) },
                    amount: Value::Number(1.5),
                    range: Some((0.0, 2.0)),
                    ..Pulse::default()
                };
                Recipe {
                    layers: vec![round_body(even(&[WHITE, first(k)])), glow(0.4, 1.0, k.clone())],
                    pulses: vec![RPulse { layer: 1, key: "strength", pulse }],
                    ..Recipe::default()
                }
            },
        ),
        look(
            "ghost",
            "Ghost",
            "Neon",
            "A pale see-through ball trailing fading copies of itself.",
            true,
            rgb(0xe8, 0xec, 0xff),
            [rgb(0xe8, 0xec, 0xff), rgb(0x80, 0x88, 0xb0)],
            |k| {
                let t = TrailLayer { kind: TrailKind::Ghosts, ghosts: 5, ghost_every: 0.06, ..TrailLayer::default() };
                plain(vec![body(k.clone(), 0.85), glow(0.4, 0.5, k.clone()), trail(t)])
            },
        ),
        // ---------- Fire ----------
        look(
            "comet",
            "Comet",
            "Fire",
            "A white-hot head with a long burning ribbon behind it.",
            false,
            ORANGE,
            [rgb(0xff, 0xd2, 0x7a), ORANGE],
            |_| {
                let t = TrailLayer {
                    kind: TrailKind::Streak,
                    seconds: 0.5,
                    width: 0.9,
                    fade: TrailFade::Out,
                    brightness: 1.4,
                    blend: Blend::Add,
                    ..TrailLayer::default()
                };
                plain(vec![
                    round_body(even(&[WHITE, rgb(0xff, 0xd2, 0x7a), ORANGE])),
                    glow(1.0, 0.8, solid(ORANGE)),
                    trail(t),
                ])
            },
        ),
        look(
            "lava",
            "Lava",
            "Fire",
            "Molten rock: glowing orange with dark crust swirling over it.",
            false,
            ORANGE,
            [rgb(0xff, 0x7a, 0x1a), rgb(0x3a, 0x0e, 0x00)],
            |_| {
                plain(vec![
                    round_body(even(&[rgb(0xff, 0xe2, 0x7a), rgb(0xff, 0x7a, 0x1a), rgb(0xb3, 0x1d, 0x00)])),
                    shader("lava"),
                    glow(0.7, 0.6, solid(rgb(0xff, 0x64, 0x14))),
                ])
            },
        ),
        look(
            "ember",
            "Ember",
            "Fire",
            "A dark smouldering coal leaving a wisp of smoke.",
            false,
            rgb(0xff, 0x8a, 0x3d),
            [rgb(0x5a, 0x15, 0x00), rgb(0xff, 0x9b, 0x3d)],
            |_| {
                let smoke = TrailLayer { kind: TrailKind::Smoke, ..TrailLayer::default() };
                plain(vec![
                    round_body(even(&[rgb(0xff, 0x9b, 0x3d), rgb(0x5a, 0x15, 0x00), rgb(0x1b, 0x07, 0x00)])),
                    glow(0.5, 0.4, solid(rgb(0xff, 0x78, 0x28))),
                    trail(smoke),
                ])
            },
        ),
        look(
            "sparks",
            "Sparks",
            "Fire",
            "A glossy yellow ball that throws sparks on every hit.",
            true,
            rgb(0xf2, 0xd2, 0x4b),
            [rgb(0xf2, 0xd2, 0x4b), WHITE],
            |k| {
                let sparks = ParticlesLayer { made_of: MadeOf::Sparks, count: 16, ..ParticlesLayer::default() };
                plain(vec![
                    body(k.clone(), 1.0),
                    shine_layer(shine(ShineMaterial::Glossy, 1.0)),
                    RLayer::new(l::PARTICLES, own(&sparks)),
                ])
            },
        ),
        look(
            "heat",
            "Heat",
            "Fire",
            "A ball that glows hotter, from red to white, with every hit.",
            true,
            rgb(0xd0, 0x2a, 0x00),
            [rgb(0xd0, 0x2a, 0x00), rgb(0xff, 0xf6, 0xd0)],
            |k| {
                let hot = heat::SubHeat { per_hit: 0.5, ..heat::SubHeat::default() };
                plain(vec![
                    body(k.clone(), 1.0).with(RLayer::new(heat::SUB_HEAT, own(&hot))),
                    shine_layer(shine(ShineMaterial::Matte, 0.6)),
                ])
            },
        ),
        look(
            "fireball",
            "Fireball",
            "Fire",
            "A blazing ball with a flame streak that lights up a lit scene.",
            false,
            rgb(0xff, 0x5a, 0x00),
            [rgb(0xff, 0xd2, 0x7a), rgb(0xff, 0x5a, 0x00)],
            |_| {
                let flame = TrailLayer {
                    kind: TrailKind::Streak,
                    seconds: 0.3,
                    width: 1.0,
                    brightness: 1.5,
                    blend: Blend::Add,
                    ..TrailLayer::default()
                };
                let light = gives_light::GivesLight { strength: 2.0, ..gives_light::GivesLight::default() };
                plain(vec![
                    round_body(even(&[WHITE, rgb(0xff, 0xd2, 0x7a), rgb(0xff, 0x5a, 0x00), rgb(0x7a, 0x10, 0x00)]))
                        .with(RLayer::new(gives_light::SUB_LIGHT, own(&light))),
                    glow(1.2, 1.0, solid(rgb(0xff, 0x6a, 0x10))),
                    trail(flame),
                ])
            },
        ),
        // ---------- Trippy ----------
        look(
            "rainbow",
            "Rainbow",
            "Trippy",
            "A ball whose colour keeps cycling round the rainbow.",
            false,
            rgb(0xff, 0x6f, 0xa8),
            [rgb(0xff, 0x6f, 0xa8), rgb(0x4f, 0xc3, 0xf0)],
            |_| {
                plain(vec![
                    body(solid(rgb(0xff, 0x40, 0x40)), 1.0),
                    shine_layer(shine(ShineMaterial::Glossy, 0.8)),
                    RLayer::new(l::RAINBOW, own(&ColourOverTime { rainbow: 0.4 })),
                ])
            },
        ),
        look("hypno", "Hypno", "Trippy", "Black and white rings that spin.", false, WHITE, [WHITE, BLACK], |_| {
            let rings = InsidePaint {
                pattern: Pattern::Gradient,
                form: bpe_core::GradientForm::Rings,
                from: InsideFrom::OwnGradient,
                colours: even(&[WHITE, BLACK]),
                repeat: bpe_core::Repeat::Repeat,
                size: 4.0,
                spin: 120.0,
                ..InsidePaint::default()
            };
            plain(vec![body(solid(WHITE), 1.0), pattern(rings, 1.0, Blend::Normal), turning(120.0)])
        }),
        look(
            "liquid",
            "Liquid",
            "Trippy",
            "Flowing colours that swirl inside the ball.",
            false,
            VIOLET,
            [VIOLET, PINK],
            |_| {
                let mut swirl = even(&[PINK, VIOLET, CYAN]);
                swirl.set_looped(true);
                let flow = InsidePaint {
                    pattern: Pattern::Gradient,
                    form: bpe_core::GradientForm::Marble,
                    from: InsideFrom::OwnGradient,
                    colours: swirl,
                    repeat: bpe_core::Repeat::Mirror,
                    size: 1.5,
                    scroll: 0.3,
                    ..InsidePaint::default()
                };
                plain(vec![body(solid(VIOLET), 1.0), pattern(flow, 1.0, Blend::Normal), turning(30.0)])
            },
        ),
        look(
            "glitch",
            "Glitch",
            "Trippy",
            "A pale ball that splits into red and blue copies.",
            true,
            rgb(0xe2, 0xe2, 0xe2),
            [rgb(0xff, 0x3c, 0x78), rgb(0x3c, 0xdc, 0xff)],
            |k| {
                let bars = InsidePaint {
                    pattern: Pattern::Gradient,
                    form: bpe_core::GradientForm::Stripes,
                    from: InsideFrom::OwnGradient,
                    colours: hard(&[
                        (0.0, rgb(0xff, 0x3c, 0x78)),
                        (0.33, rgb(0xff, 0x3c, 0x78)),
                        (0.34, rgb(0x3c, 0xdc, 0xff)),
                        (0.66, rgb(0x3c, 0xdc, 0xff)),
                        (0.67, rgb(0x20, 0x20, 0x28)),
                        (1.0, rgb(0x20, 0x20, 0x28)),
                    ]),
                    repeat: bpe_core::Repeat::Repeat,
                    angle: 0.0,
                    size: 3.0,
                    scroll: 0.6,
                    ..InsidePaint::default()
                };
                plain(vec![body(k.clone(), 1.0), pattern(bars, 0.45, Blend::Normal), shader("color_split")])
            },
        ),
        look(
            "bubble",
            "Bubble",
            "Trippy",
            "A thin soap bubble shimmering with rainbow colours.",
            false,
            rgb(0xd8, 0xf0, 0xff),
            [rgb(0xff, 0x78, 0xc8), rgb(0x78, 0xdc, 0xff)],
            |_| {
                let film = InsidePaint {
                    pattern: Pattern::Gradient,
                    form: bpe_core::GradientForm::Sweep,
                    from: InsideFrom::OwnGradient,
                    colours: even(&[PINK, GOLD, rgb(0xa8, 0xd8, 0x4a), CYAN, VIOLET, PINK]),
                    spin: 40.0,
                    ..InsidePaint::default()
                };
                plain(vec![
                    body(solid(rgb(0xd8, 0xf0, 0xff)), 0.12),
                    pattern(film, 0.35, Blend::Screen),
                    shine_layer(shine(ShineMaterial::Glass, 1.0)),
                    outline(1.0, OutlineSits::Inside, solid(rgb(0xf0, 0xf8, 0xff))),
                ])
            },
        ),
        look(
            "kaleido",
            "Kaleido",
            "Trippy",
            "A turning mirror pattern of bright colours.",
            false,
            PINK,
            [PINK, GOLD],
            |_| plain(vec![body(even(&[PINK, GOLD, CYAN, VIOLET]), 1.0), shader("kaleidoscope"), turning(60.0)]),
        ),
    ]
}

/// A gradient with these stops (two at one place make a hard edge).
fn hard(stops: &[(f32, Color)]) -> Gradient {
    let mut g = Gradient::solid(stops[0].1);
    g.stops = stops.iter().map(|&(at, c)| bpe_core::Stop::new(at, c)).collect();
    g
}
