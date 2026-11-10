//! Neon tube: a glowing ball in a glowing hexagon in a dark room, the bloom turned up.

use bpe_core::{Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, hex};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};
use crate::{BasicBounce, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "neon-tube",
        name: "Neon tube",
        level: Level::Starter,
        kinds: &[Kind::LightAndGlass],
        about: "A glowing ball bounces inside a hexagon of neon tube in a dark room, the glow blooming round both.",
        uses: &["Balls", "Shape: neon tube look", "Bloom", "Sound"],
        change_first: "The tube's colours (its Look), the scene's Bloom and the ball's look.",
        sound: SoundLevel::Simple,
        seconds: 20.0,
        poster_seconds: 6.0,
        preview_seconds: 12.0,
        swatch: [0xff4fd8, 0x07060a],
        payoff: None,
    },
    build,
};

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 20.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x07060a));
        s.bloom = 0.6;
    });
    let hexagon = lib.add("shape", ThingId::ROOT, "Hexagon");
    lib.params(hexagon, |s: &mut ShapeSettings| *s = ShapeSettings::polygon(6, 430.0, 12.0));
    lib.look(hexagon, "shape.neon_tube");
    lib.notes(hexagon, &[60, 62, 64, 67, 69, 72, 74], Plays::Random, Root::C, Scale::MajorPentatonic);
    let ball = lib.ball("Neon ball", Vec2::new(-40.0, -120.0), 34.0, Vec2::new(340.0, 80.0), "glow_orb");
    lib.card(ball, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 0.97);
    lib.finish()
}
