//! Shrinking box: the square shrinks a little all the time, so the hits come faster and faster.

use bpe_core::{Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, hex};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};
use crate::{BasicBounce, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "shrinking-box",
        name: "Shrinking box",
        level: Level::Starter,
        kinds: &[Kind::BounceAndDrop],
        about: "A square that keeps shrinking around a bouncing ball, so the hits and notes come faster and faster.",
        uses: &["Balls", "Shape: shrink", "Sound: notes"],
        change_first: "How fast the box shrinks, its smallest size and the notes it plays.",
        sound: SoundLevel::Simple,
        seconds: 20.0,
        poster_seconds: 6.0,
        preview_seconds: 12.0,
        swatch: [0xa8d84a, 0x101410],
        payoff: None,
    },
    build,
};

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 20.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x101410));
        s.bloom = 0.3;
    });
    let square = lib.add("shape", ThingId::ROOT, "Box");
    lib.params(square, |s: &mut ShapeSettings| {
        *s = ShapeSettings { shrink: 0.07, min_size: 170.0, turn: 0.0, ..ShapeSettings::polygon(4, 520.0, 10.0) };
    });
    lib.look(square, "shape.neon_tube");
    // A minor pentatonic run that climbs as the hits speed up.
    lib.notes(square, &[57, 60, 62, 64, 67, 69, 72, 74], Plays::Up, Root::A, Scale::MinorPentatonic);
    let ball = lib.ball("Ball", Vec2::new(-60.0, -100.0), 26.0, Vec2::new(260.0, 120.0), "chrome");
    lib.card(ball, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);
    lib.finish()
}
