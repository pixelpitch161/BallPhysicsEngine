//! The shape looks: three families of six. A shape's outline is the thing's own, so none of them changes it.

use bpe_core::{Color, Gradient, Params, Value};

use super::{RLayer, ReadyLook, Recipe, Wears, own};
use crate::modules::look::layers::particles::{SHAPE_PARTICLES, ShapeMadeOf, ShapeParticlesLayer};
use crate::modules::look::layers::{ColourFrom, OutlineSits, ShineLayer, ShineMaterial, SubFlash, gives_light};
use crate::modules::look::shape::{self as s, PaintLayer, ShapeGlow, ShapeOutline};
use crate::modules::look::surface::{SurfaceKind, preset_values};

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::rgb(r, g, b)
}

fn solid(c: Color) -> Gradient {
    Gradient::solid(c)
}

fn even(cs: &[Color]) -> Gradient {
    Gradient::even(cs)
}

fn body() -> RLayer {
    RLayer::new(s::BODY, Params::default())
}

fn paint(colours: Gradient, opacity: f32) -> RLayer {
    RLayer::new(s::PAINT, own(&PaintLayer { colours, opacity, ..PaintLayer::default() }))
}

fn outline(width: f32, sits: OutlineSits, colour: Gradient) -> RLayer {
    let o = ShapeOutline { width, sits, colour_from: ColourFrom::Own, colour, ..ShapeOutline::default() };
    RLayer::new(s::OUTLINE, own(&o))
}

fn glow(strength: f32, size: f32, colour: Gradient) -> RLayer {
    let g = ShapeGlow { strength, size, colour_from: ColourFrom::Own, colour, ..ShapeGlow::default() };
    RLayer::new(s::GLOW, own(&g))
}

fn shine(material: ShineMaterial, strength: f32) -> RLayer {
    RLayer::new(s::SUB_SHINE, own(&ShineLayer { material, strength, ..ShineLayer::default() }))
}

fn plain(layers: Vec<RLayer>) -> Recipe {
    Recipe { layers, ..Recipe::default() }
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
    ReadyLook { id, name, family, help, wears: Wears::Shape, takes_colour, main, swatch, build }
}

const WHITE: Color = Color::WHITE;
const DARK: Color = rgb(0x10, 0x10, 0x14);
const PINK: Color = rgb(0xff, 0x6f, 0xa8);
const CYAN: Color = rgb(0x4f, 0xc3, 0xf0);
const VIOLET: Color = rgb(0xa9, 0x8b, 0xff);
const GOLD: Color = rgb(0xe8, 0xb9, 0x4a);
const ORANGE: Color = rgb(0xff, 0xaa, 0x45);
const YELLOW: Color = rgb(0xf2, 0xd2, 0x4b);
const TEAL: Color = rgb(0x3f, 0xd6, 0xb5);

pub fn looks() -> Vec<ReadyLook> {
    vec![
        // ---------- Clean ----------
        look("shape.solid", "Solid", "Clean", "Filled with one plain colour.", true, WHITE, [WHITE, WHITE], |k| {
            plain(vec![body(), paint(k.clone(), 1.0)])
        }),
        look(
            "shape.outline",
            "Outline",
            "Clean",
            "A dark fill with a clean bright line round its edge.",
            true,
            WHITE,
            [DARK, WHITE],
            |k| plain(vec![body(), paint(solid(DARK), 1.0), outline(4.0, OutlineSits::Inside, k.clone())]),
        ),
        look(
            "shape.pastel",
            "Pastel",
            "Clean",
            "A soft pastel fill with a gentle sheen.",
            true,
            rgb(0xb8, 0xe6, 0xd8),
            [rgb(0xb8, 0xe6, 0xd8), WHITE],
            |k| plain(vec![body(), paint(k.clone(), 1.0).with(shine(ShineMaterial::Matte, 0.5))]),
        ),
        look(
            "shape.chalk",
            "Chalk",
            "Clean",
            "A dark fill edged with a soft, chalky light line.",
            true,
            rgb(0xf0, 0xf0, 0xf0),
            [DARK, rgb(0xf0, 0xf0, 0xf0)],
            |k| {
                let o = ShapeOutline {
                    width: 8.0,
                    sits: OutlineSits::Centre,
                    colour_from: ColourFrom::Own,
                    colour: k.clone(),
                    opacity: 0.75,
                    ..ShapeOutline::default()
                };
                plain(vec![body(), paint(solid(rgb(0x22, 0x26, 0x2a)), 1.0), RLayer::new(s::OUTLINE, own(&o))])
            },
        ),
        look(
            "shape.glass",
            "Glass",
            "Clean",
            "Clear glass with a bright rim; it bends light in a lit scene.",
            true,
            rgb(0x9a, 0xd4, 0xff),
            [rgb(0x9a, 0xd4, 0xff), WHITE],
            |k| {
                let mut card = Params::default();
                card.set("surface", Value::Choice(bpe_core::Choice::name(SurfaceKind::Glass).into()));
                for (key, v) in preset_values(SurfaceKind::Glass) {
                    card.set(key, v);
                }
                Recipe {
                    layers: vec![
                        body(),
                        paint(k.clone(), 0.55).with(shine(ShineMaterial::Glass, 1.0)),
                        outline(2.0, OutlineSits::Inside, solid(rgb(0xd8, 0xf0, 0xff))),
                    ],
                    card,
                    ..Recipe::default()
                }
            },
        ),
        look(
            "shape.frosted",
            "Frosted",
            "Clean",
            "A milky, frosted fill, lighter in the middle, with a fine white edge.",
            true,
            rgb(0xe6, 0xf0, 0xff),
            [rgb(0xe6, 0xf0, 0xff), WHITE],
            |k| {
                let milk =
                    PaintLayer { colours: even(&[k.stops[0].color, rgb(0xa8, 0xb8, 0xcc)]), ..PaintLayer::default() };
                plain(vec![
                    body(),
                    RLayer::new(s::PAINT, own(&milk)).set("colours~form", Value::Choice("round".into())),
                    outline(1.5, OutlineSits::Inside, solid(WHITE)),
                ])
            },
        ),
        // ---------- Glow ----------
        look(
            "shape.neon_tube",
            "Neon tube",
            "Glow",
            "A dark shape edged with a glowing neon tube.",
            true,
            PINK,
            [DARK, PINK],
            |k| {
                plain(vec![
                    body(),
                    paint(solid(DARK), 1.0),
                    outline(6.0, OutlineSits::Centre, k.clone()),
                    glow(1.0, 30.0, k.clone()),
                ])
            },
        ),
        look(
            "shape.glow_ring",
            "Glow ring",
            "Glow",
            "A dark shape with a wide glowing edge that lights up a lit scene.",
            true,
            CYAN,
            [DARK, CYAN],
            |k| {
                let light = gives_light::GivesLight { strength: 2.0, ..gives_light::GivesLight::default() };
                plain(vec![
                    body(),
                    paint(solid(DARK), 1.0),
                    outline(5.0, OutlineSits::Centre, k.clone())
                        .with(RLayer::new(gives_light::SHAPE_SUB_LIGHT, own(&light))),
                    glow(1.4, 70.0, k.clone()),
                ])
            },
        ),
        look(
            "shape.laser",
            "Laser",
            "Glow",
            "A dark shape with a thin white-hot edge and a violet glow.",
            true,
            VIOLET,
            [WHITE, VIOLET],
            |k| {
                plain(vec![
                    body(),
                    paint(solid(DARK), 1.0),
                    outline(2.0, OutlineSits::Centre, solid(WHITE)),
                    glow(1.2, 18.0, k.clone()),
                ])
            },
        ),
        look(
            "shape.hit_ripple",
            "Hit ripple",
            "Glow",
            "A coloured shape whose edge flashes white when hit.",
            true,
            TEAL,
            [TEAL, WHITE],
            |k| {
                let flash = SubFlash { strength: 1.2, length: 0.35, ..SubFlash::default() };
                plain(vec![
                    body(),
                    paint(k.clone(), 1.0),
                    outline(4.0, OutlineSits::Inside, solid(WHITE)).with(RLayer::new(s::SUB_FLASH, own(&flash))),
                    glow(0.4, 24.0, solid(WHITE)).with(RLayer::new(s::SUB_FLASH, own(&flash))),
                ])
            },
        ),
        look(
            "shape.chrome_edge",
            "Chrome edge",
            "Glow",
            "A dark shape with a polished metal rim.",
            false,
            rgb(0xbb, 0xbb, 0xbb),
            [DARK, rgb(0xbb, 0xbb, 0xbb)],
            |_| {
                let rim = even(&[rgb(0xf0, 0xf0, 0xf0), rgb(0x60, 0x60, 0x60), rgb(0xd0, 0xd0, 0xd0)]);
                plain(vec![
                    body(),
                    paint(solid(DARK), 1.0),
                    outline(8.0, OutlineSits::Inside, rim).with(shine(ShineMaterial::Metal, 1.0)),
                ])
            },
        ),
        look(
            "shape.gold_edge",
            "Gold edge",
            "Glow",
            "A dark shape with a warm golden rim.",
            true,
            GOLD,
            [DARK, GOLD],
            |k| {
                plain(vec![
                    body(),
                    paint(solid(rgb(0x1a, 0x14, 0x08)), 1.0),
                    outline(8.0, OutlineSits::Inside, k.clone()).with(shine(ShineMaterial::Metal, 1.0)),
                    glow(0.3, 20.0, k.clone()),
                ])
            },
        ),
        // ---------- Colour ----------
        look(
            "shape.sunset",
            "Sunset",
            "Colour",
            "Pink fading through orange to yellow.",
            false,
            ORANGE,
            [PINK, YELLOW],
            |_| plain(vec![body(), paint(even(&[PINK, ORANGE, YELLOW]), 1.0)]),
        ),
        look(
            "shape.ocean",
            "Ocean",
            "Colour",
            "Teal fading into deep blue.",
            false,
            CYAN,
            [TEAL, rgb(0x1d, 0x3d, 0x8a)],
            |_| plain(vec![body(), paint(even(&[TEAL, CYAN, rgb(0x1d, 0x3d, 0x8a)]), 1.0)]),
        ),
        look(
            "shape.rainbow_flow",
            "Rainbow flow",
            "Colour",
            "A rainbow that keeps flowing across it.",
            false,
            PINK,
            [PINK, CYAN],
            |_| {
                let mut rainbow = even(&[PINK, ORANGE, YELLOW, rgb(0xa8, 0xd8, 0x4a), CYAN, VIOLET]);
                rainbow.set_looped(true);
                let p = PaintLayer { colours: rainbow, flow: 0.3, ..PaintLayer::default() };
                plain(vec![body(), RLayer::new(s::PAINT, own(&p))])
            },
        ),
        look(
            "shape.aurora",
            "Aurora",
            "Colour",
            "Soft clouds of green and violet light drifting over it.",
            false,
            TEAL,
            [TEAL, VIOLET],
            |_| {
                let mut sky = even(&[rgb(0x0f, 0x18, 0x30), TEAL, rgb(0x0f, 0x18, 0x30), VIOLET]);
                sky.set_looped(true);
                let p = PaintLayer { colours: sky, flow: 0.12, ..PaintLayer::default() };
                plain(vec![body(), RLayer::new(s::PAINT, own(&p)).set("colours~form", Value::Choice("waves".into()))])
            },
        ),
        look(
            "shape.plasma",
            "Plasma",
            "Colour",
            "Bright colours boiling and flowing inside it.",
            false,
            VIOLET,
            [VIOLET, PINK],
            |_| {
                let mut hot = even(&[PINK, VIOLET, CYAN, YELLOW]);
                hot.set_looped(true);
                let p = PaintLayer { colours: hot, flow: 0.35, ..PaintLayer::default() };
                plain(vec![body(), RLayer::new(s::PAINT, own(&p)).set("colours~form", Value::Choice("marble".into()))])
            },
        ),
        look(
            "shape.sparks",
            "Sparks",
            "Colour",
            "A coloured shape that throws sparks where balls hit it.",
            true,
            YELLOW,
            [YELLOW, ORANGE],
            |k| {
                let sparks =
                    ShapeParticlesLayer { made_of: ShapeMadeOf::Sparks, count: 16, ..ShapeParticlesLayer::default() };
                plain(vec![body(), paint(k.clone(), 1.0), RLayer::new(SHAPE_PARTICLES, own(&sparks))])
            },
        ),
    ]
}
