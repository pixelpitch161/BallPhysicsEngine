//! Stained glass: a lamp ball bounces through a rose window of coloured glass, and the panes throw coloured
//! shadows that sweep round the dark room as it moves.

use bpe_core::{Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, Tone, hex};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::modules::lighting::LightingSettings;
use crate::modules::look::shape::{PAINT, PaintLayer};
use crate::modules::look::surface::SurfaceSettings;
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};
use crate::things::balls::{GroupShape, Launch};
use crate::things::light::LightType;
use crate::things::shape::ShapeFill;
use crate::{BallsSettings, BasicBounce, BasicMotion, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "stained-glass",
        name: "Stained glass",
        level: Level::InBetween,
        kinds: &[Kind::LightAndGlass],
        about: "A ball carrying a lamp bounces through a rose window of coloured glass. Every pane throws its \
                colour across the dark room, and the coloured shadows sweep round as the lamp moves.",
        uses: &["Balls", "Light riding a ball", "Shape: glass panes", "Lighting: coloured shadows", "Sound"],
        change_first: "The panes' colours (each pane's Look), the lamp's colour and strength, and the ball's speed.",
        sound: SoundLevel::Designed,
        seconds: 28.0,
        poster_seconds: 6.0,
        preview_seconds: 14.0,
        swatch: [0xe0283c, 0x3a3644],
        payoff: None,
    },
    build,
};

/// The petals round the middle pane, clockwise from the top: colour and note.
const PETALS: [(u32, u8); 8] = [
    (0xd0102a, 69),
    (0xff7a00, 72),
    (0x08a83c, 74),
    (0x1240ff, 76),
    (0x8420f0, 79),
    (0xf0206e, 81),
    (0x00a8b0, 84),
    (0xffc400, 86),
];

/// The window's middle.
const WINDOW: Vec2 = Vec2::new(0.0, 60.0);

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 28.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x3a3644));
        s.bloom = 0.25;
    });
    let room = lib.add("shape", ThingId::ROOT, "Room");
    lib.params(room, |s: &mut ShapeSettings| *s = ShapeSettings::rectangle(1000.0, 1700.0, 24.0));
    lib.look_in(room, "shape.solid", hex(0x2c2a35));

    let window = lib.add("folder", ThingId::ROOT, "Rose window");
    let middle = lib.add("shape", window, "Middle pane");
    lib.at(middle, WINDOW);
    lib.params(middle, |s: &mut ShapeSettings| {
        *s = ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::polygon(6, 115.0, 4.0) }
    });
    lib.look_in(middle, "shape.glass", hex(0xfff2c8));
    lib.notes(middle, &[57], Plays::Random, Root::A, Scale::MinorPentatonic);
    lib.tone(middle, Tone::Pad);
    for (k, (colour, note)) in PETALS.iter().enumerate() {
        let turn = k as f32 / PETALS.len() as f32 * std::f32::consts::TAU;
        let pane = lib.add("shape", window, &format!("Pane {}", k + 1));
        lib.at(pane, WINDOW + Vec2::new(turn.sin(), -turn.cos()) * 320.0);
        lib.scene.things.get_mut(&pane).expect("a pane").transform.rotation = turn;
        lib.params(pane, |s: &mut ShapeSettings| {
            *s = ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::ellipse(130.0, 250.0, 4.0) }
        });
        lib.look_in(pane, "shape.glass", hex(*colour));
        // Flat panes that colour the light without bending it, in a deep colour.
        lib.card(pane, SlotId::Look, |g: &mut SurfaceSettings| {
            g.bends_light = 1.0;
            g.see_through = 0.9;
            g.tints = 1.0;
            g.thick_tint = 0.0;
        });
        let colour = lib.scene.card(pane, SlotId::Look).and_then(|c| c.layers.iter().find(|l| l.kind == PAINT));
        let colour = colour.expect("glass has a Colour layer").id;
        lib.layer_params(pane, SlotId::Look, colour, |p: &mut PaintLayer| p.opacity = 0.9);
        lib.notes(pane, &[*note], Plays::Random, Root::A, Scale::MinorPentatonic);
        lib.tone(pane, Tone::Bell);
    }

    let lamp_ball = lib.ball("Lamp ball", Vec2::new(-180.0, -560.0), 26.0, Vec2::new(520.0, 610.0), "glow_orb");
    lib.look_in(lamp_ball, "glow_orb", hex(0xfff1d6));
    lib.card(lamp_ball, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 0.0);
    lib.card(lamp_ball, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);
    lib.light_in(lamp_ball, "Lamp", LightType::Point, Vec2::ZERO, |l| {
        l.size = 16.0;
        l.strength = 2.4;
        l.reach = 2600.0;
        l.colour = hex(0xfff0d8);
    });
    let marbles = lib.ball("Glass marbles", Vec2::new(0.0, 640.0), 30.0, Vec2::new(0.0, -560.0), "glass");
    lib.params(marbles, |s: &mut BallsSettings| {
        s.group = true;
        s.shape = GroupShape::Line;
        s.count = 3;
        s.space = 50.0;
        s.launch = Launch::Random;
    });
    lib.card(marbles, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 0.0);
    lib.card(marbles, SlotId::Bounce, |b: &mut BasicBounce| {
        b.bounciness = 1.0;
        b.hit_balls = true;
    });
    lib.card(ThingId::ROOT, SlotId::Lighting, |l: &mut LightingSettings| {
        l.ambient = 0.07;
        l.contact = 0.3;
    });
    lib.reverb(3.5, 35.0);
    lib.finish()
}
