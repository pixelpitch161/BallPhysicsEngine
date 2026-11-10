//! Prism rainbow: a beam of white light through a slowly spinning glass prism fans into a rainbow that sweeps
//! round the dark, while pearls drop onto the prism and bounce off through the colours.

use bpe_core::{Registry, Scene, SlotId, ThingId, Value};
use glam::Vec2;

use crate::library::build::{Lib, Tone, hex, n};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::modules::lighting::LightingSettings;
use crate::modules::look::surface::SurfaceKind;
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};
use crate::things::emitter::{Directions, Earlier, EmitterSettings, FireWhen, Marker};
use crate::things::light::{AtThings, LightType};
use crate::things::shape::ShapeFill;
use crate::{BasicBounce, BasicMotion, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "prism-rainbow",
        name: "Prism rainbow",
        level: Level::InBetween,
        kinds: &[Kind::LightAndGlass],
        about: "A beam of white light shines through a slowly spinning glass prism and fans out into a rainbow \
                that sweeps round the dark. Pearls drop onto the prism, ring it and bounce off through the colours.",
        uses: &["Light beam", "Shape: prism glass, spin", "Emitter", "Lighting", "Sound"],
        change_first: "The prism's Spin keys and Rainbow split, the beam's width and angle, and how often pearls drop.",
        sound: SoundLevel::Designed,
        seconds: 28.0,
        poster_seconds: 5.0,
        preview_seconds: 14.0,
        swatch: [0xff5a3c, 0x0c0c12],
        payoff: None,
    },
    build,
};

/// The prism's middle and size (its corners' distance from the middle).
const PRISM_AT: Vec2 = Vec2::new(-130.0, -260.0);
/// The whole set-up (prism and beam) is turned by this, so the rainbow pours down across the picture.
const TURN: f32 = 32.0;
const PRISM: f32 = 210.0;
/// The prism's Bends light (the Prism chip's).
const INDEX: f32 = 1.6;

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 28.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x0c0c12));
        s.bloom = 0.35;
    });
    let prism = lib.add("shape", ThingId::ROOT, "Prism");
    lib.at(prism, PRISM_AT);
    lib.params(prism, |s: &mut ShapeSettings| {
        *s = ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::polygon(3, PRISM, 4.0) }
    });
    // The prism rocks to and fro (its Spin swinging between -12 and 12 °/s), so the rainbow sweeps up and down.
    let rock: Vec<(f64, Value)> = (0..=9).map(|k| (k as f64 * 3.0, n(if k % 2 == 0 { 12.0 } else { -12.0 }))).collect();
    lib.keys(prism, None, "spin", &rock);
    lib.scene.things.get_mut(&prism).expect("the prism").transform.rotation = TURN.to_radians();
    lib.look_in(prism, "shape.glass", hex(0xe8f4ff));
    lib.surface(prism, SurfaceKind::Prism, |g| g.rainbow = 2.5);
    lib.notes(prism, &[69, 72, 74, 76, 79, 81, 84], Plays::Random, Root::A, Scale::MinorPentatonic);
    lib.tone(prism, Tone::Bell);

    // In at the left face at the least turn for the middle colour, so the rainbow comes out wide.
    let turn = Vec2::from_angle(TURN.to_radians());
    let dir = turn.rotate(Vec2::from_angle((30.0 - (INDEX * 0.5).asin().to_degrees()).to_radians()));
    let face = PRISM_AT + turn.rotate((Vec2::new(0.0, -PRISM) + Vec2::new(-0.866 * PRISM, 0.5 * PRISM)) * 0.5);
    lib.light("Light beam", LightType::Beam, face - dir * 420.0, |l| {
        l.direction = dir.y.atan2(dir.x).to_degrees();
        l.beam_width = 46.0;
        l.strength = 3.0;
        l.bounces = 8;
        l.beam_length = 1700.0;
        l.at_shapes = AtThings::PassThrough;
        l.at_balls = AtThings::PassThrough;
    });
    lib.card(ThingId::ROOT, SlotId::Lighting, |l: &mut LightingSettings| l.ambient = 0.14);

    let floor = lib.add("shape", ThingId::ROOT, "Floor");
    lib.at(floor, Vec2::new(0.0, 840.0));
    lib.params(floor, |s: &mut ShapeSettings| {
        *s = ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::rectangle(1040.0, 40.0, 6.0) }
    });
    lib.look_in(floor, "shape.solid", hex(0x3a3a46));
    lib.notes(floor, &[45, 52, 57], Plays::Random, Root::A, Scale::MinorPentatonic);
    lib.tone(floor, Tone::Mallet);

    let pearls = lib.emitter("Pearls", Vec2::new(PRISM_AT.x + 30.0, -880.0), |s: &mut EmitterSettings| {
        s.radius = 18.0;
        s.directions = Directions::Fan;
        s.aim_angle = 90.0;
        s.fan_angle = 16.0;
        s.speed = 140.0;
        s.vary_speed = 0.4;
        s.count = 1;
        s.fire_when = FireWhen::Every;
        s.every = 0.45;
        s.endless = true;
        s.earlier = Earlier::FadeOut;
        s.fade_time = 6.0;
        s.marker = Marker::None;
    });
    lib.card(pearls, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 1.0);
    lib.card(pearls, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 0.75);
    lib.look(pearls, "pearl");
    lib.reverb(3.0, 32.0);
    lib.finish()
}
