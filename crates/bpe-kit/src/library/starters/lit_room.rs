//! Lit room: one lamp over a few balls in a room, throwing soft shadows on the walls.

use bpe_core::{Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, hex};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::modules::lighting::LightingSettings;
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};
use crate::things::balls::{GroupShape, Launch};
use crate::things::light::LightType;
use crate::{BallsSettings, BasicBounce, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "lit-room",
        name: "Lit room",
        level: Level::Starter,
        kinds: &[Kind::LightAndGlass],
        about: "One lamp hangs over five marble balls tumbling round a room, and they throw real soft shadows on \
                the walls and on each other.",
        uses: &["Balls: a group", "Shape", "Light", "Lighting", "Sound"],
        change_first: "The lamp's colour, size (softer shadows when bigger) and place, and how dark the room is \
                       (the Lighting card's Ambient).",
        sound: SoundLevel::Simple,
        seconds: 20.0,
        poster_seconds: 5.0,
        preview_seconds: 12.0,
        swatch: [0xffc88a, 0x2a2622],
        payoff: None,
    },
    build,
};

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 20.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x2a2622));
        s.bloom = 0.15;
    });
    let room = lib.add("shape", ThingId::ROOT, "Room");
    lib.params(room, |s: &mut ShapeSettings| *s = ShapeSettings::rectangle(900.0, 1500.0, 30.0));
    lib.look(room, "shape.solid");
    lib.notes(room, &[48, 55, 60, 64, 67], Plays::Random, Root::C, Scale::Major);
    lib.light("Lamp", LightType::Point, Vec2::new(0.0, -560.0), |l| {
        l.size = 60.0;
        l.strength = 1.6;
        l.reach = 2400.0;
        l.colour = hex(0xffd6a0);
    });
    lib.card(ThingId::ROOT, SlotId::Lighting, |l: &mut LightingSettings| {
        l.ambient = 0.12;
        l.contact = 0.4;
    });
    let balls = lib.ball("Balls", Vec2::new(0.0, -100.0), 46.0, Vec2::new(0.0, -650.0), "marble");
    lib.params(balls, |s: &mut BallsSettings| {
        s.group = true;
        s.shape = GroupShape::Line;
        s.count = 5;
        s.space = 40.0;
        s.launch = Launch::Random;
    });
    lib.card(balls, SlotId::Bounce, |b: &mut BasicBounce| {
        b.bounciness = 1.0;
        b.hit_balls = true;
    });
    lib.finish()
}
