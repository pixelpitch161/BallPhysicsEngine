//! Breakable bricks: a wall of bricks with health; a ball knocks it down, each brick shattering when its
//! health runs out.

use bpe_core::{Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, hex};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::modules::health::{AtZero, HealthSettings};
use crate::paint::Paint;
use crate::rules::breaks::BreakLook;
use crate::sound::{Plays, Root, Scale};
use crate::things::balls::{GroupShape, Launch};
use crate::things::shape::ShapeFill;
use crate::{BallsSettings, BasicBounce, BasicMotion, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "breakable-bricks",
        name: "Breakable bricks",
        level: Level::Starter,
        kinds: &[Kind::BounceAndDrop],
        about: "Three fast balls knock down a wall of fifteen bricks. Each brick takes two hits, then shatters into \
                pieces.",
        uses: &["Balls", "Shape: filled bricks in a folder", "Health: break and shatter", "Sound"],
        change_first: "How many hits a brick takes (its Health card's Starting health), the break look and the \
                       balls' speed.",
        sound: SoundLevel::Simple,
        seconds: 25.0,
        poster_seconds: 9.0,
        preview_seconds: 15.0,
        swatch: [0xff7a45, 0x14121a],
        payoff: None,
    },
    build,
};

/// The bricks' colours, a row each.
const ROWS: [u32; 3] = [0xff5a5f, 0xffb347, 0x4fd18b];

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 25.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x14121a));
        s.bloom = 0.2;
    });
    let room = lib.add("shape", ThingId::ROOT, "Room");
    lib.params(room, |s: &mut ShapeSettings| *s = ShapeSettings::rectangle(960.0, 1700.0, 12.0));
    lib.look(room, "shape.outline");
    let folder = lib.add("folder", ThingId::ROOT, "Bricks");
    for (row, colour) in ROWS.iter().enumerate() {
        for col in 0..5 {
            let brick = lib.add("shape", folder, &format!("Brick {}", row * 5 + col + 1));
            lib.at(brick, Vec2::new(-340.0 + col as f32 * 170.0, -560.0 + row as f32 * 90.0));
            lib.params(brick, |s: &mut ShapeSettings| {
                *s = ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::rectangle(150.0, 66.0, 4.0) };
            });
            lib.look_in(brick, "shape.solid", hex(*colour));
            lib.fill(brick, SlotId::Health);
            lib.card(brick, SlotId::Health, |h: &mut HealthSettings| {
                h.start = 2.0;
                h.at_zero = AtZero::Break;
                h.look = BreakLook::Shatter;
            });
        }
    }
    let ball = lib.ball("Balls", Vec2::new(0.0, 500.0), 24.0, Vec2::new(0.0, -1200.0), "steel");
    lib.params(ball, |s: &mut BallsSettings| {
        s.group = true;
        s.shape = GroupShape::Line;
        s.count = 3;
        s.space = 60.0;
        s.launch = Launch::Fan;
    });
    lib.card(ball, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 0.0);
    lib.card(ball, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);
    lib.notes(ball, &[60, 63, 65, 67, 70, 72], Plays::Random, Root::C, Scale::MinorPentatonic);
    lib.finish()
}
