//! First bounce: one ball, one floor, a soft pluck on every hit.

use bpe_core::{Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, hex};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::paint::Paint;
use crate::{BallsSettings, BasicBounce, BasicMotion, ShapeFill, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "first-bounce",
        name: "First bounce",
        level: Level::Starter,
        kinds: &[Kind::BounceAndDrop],
        about: "One ball, one floor, a soft pluck on every hit. The simplest scene to pull apart.",
        uses: &["Balls", "Shape", "Sound"],
        change_first: "The ball's look, how bouncy it is, and the note the floor plays.",
        sound: SoundLevel::Simple,
        seconds: 12.0,
        poster_seconds: 1.2,
        preview_seconds: 8.0,
        swatch: [0xff5a8c, 0x202020],
        payoff: None,
    },
    build,
};

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 12.0);
    lib.scene_settings(|s| s.background = Paint::solid(hex(0x202020)));

    let floor = lib.add("shape", ThingId::ROOT, "Floor");
    lib.at(floor, Vec2::new(0.0, 700.0));
    lib.params(floor, |s: &mut ShapeSettings| {
        *s = ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::rectangle(900.0, 40.0, 6.0) };
    });
    lib.look(floor, "shape.solid");
    lib.sound(floor);

    let ball = lib.add("balls", ThingId::ROOT, "Ball");
    lib.at(ball, Vec2::new(-150.0, -550.0));
    lib.params(ball, |s: &mut BallsSettings| {
        s.radius = 45.0;
        s.velocity = Vec2::new(25.0, 0.0);
    });
    lib.card(ball, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 1.0);
    lib.card(ball, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 0.85);
    lib.look(ball, "candy");
    lib.finish()
}
