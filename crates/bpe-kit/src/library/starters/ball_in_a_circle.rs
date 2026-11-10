//! Ball in a circle: a ball in a ring, and each hit climbs a scale.

use bpe_core::{Registry, Scene, SlotId};
use glam::Vec2;

use crate::BasicBounce;
use crate::library::build::{Lib, hex};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "ball-in-a-circle",
        name: "Ball in a circle",
        level: Level::Starter,
        kinds: &[Kind::BounceAndDrop],
        about: "A glowing ball bouncing round inside a ring, and each hit climbs a scale.",
        uses: &["Balls", "Shape", "Sound: notes"],
        change_first: "The ring's size, the ball's look and the notes the ring plays.",
        sound: SoundLevel::Simple,
        seconds: 20.0,
        poster_seconds: 2.5,
        preview_seconds: 10.0,
        swatch: [0x4fc3f0, 0x0d0b14],
        payoff: None,
    },
    build,
};

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 20.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x0d0b14));
        s.bloom = 0.35;
    });
    let ring = lib.ring("Ring", Vec2::ZERO, 450.0, 10.0, "shape.neon_tube");
    // C major from middle C up an octave: each hit plays the next one up.
    lib.notes(ring, &[60, 62, 64, 65, 67, 69, 71, 72], Plays::Up, Root::C, Scale::Major);
    let ball = lib.ball("Ball", Vec2::new(0.0, -200.0), 32.0, Vec2::new(320.0, 0.0), "glow_orb");
    lib.card(ball, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);
    lib.finish()
}
