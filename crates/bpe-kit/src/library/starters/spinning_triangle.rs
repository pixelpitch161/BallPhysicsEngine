//! Spinning triangle: a ball ricochets inside a slowly turning triangle, a glowing trail behind it.

use bpe_core::{Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, hex};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::modules::look::layers::trail::TRAIL;
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};
use crate::{BasicBounce, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "spinning-triangle",
        name: "Spinning triangle",
        level: Level::Starter,
        kinds: &[Kind::Hypnotic],
        about: "A ball ricochets inside a slowly spinning triangle, flung off its corners as they sweep round, with \
                a glowing trail behind it.",
        uses: &["Balls", "Trail", "Shape: spin", "Sound"],
        change_first: "How fast the triangle spins (its Spin), how many sides it has and the ball's speed.",
        sound: SoundLevel::Simple,
        seconds: 20.0,
        poster_seconds: 8.0,
        preview_seconds: 12.0,
        swatch: [0x7cf7ff, 0x0b0b16],
        payoff: None,
    },
    build,
};

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 20.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x0b0b16));
        s.bloom = 0.35;
    });
    let triangle = lib.add("shape", ThingId::ROOT, "Triangle");
    lib.at(triangle, Vec2::new(0.0, 40.0));
    lib.params(triangle, |s: &mut ShapeSettings| {
        *s = ShapeSettings { spin: 20.0, ..ShapeSettings::polygon(3, 440.0, 10.0) };
    });
    lib.look(triangle, "shape.glow_ring");
    lib.notes(triangle, &[57, 60, 62, 64, 67, 69, 72], Plays::Up, Root::A, Scale::MinorPentatonic);
    let ball = lib.ball("Ball", Vec2::new(0.0, 60.0), 30.0, Vec2::new(520.0, -390.0), "electric");
    lib.card(ball, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);
    lib.layer(ball, SlotId::Look, TRAIL);
    lib.finish()
}
