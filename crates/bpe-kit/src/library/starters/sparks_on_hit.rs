//! Sparks on hit: every bounce on the floor throws sparks and a puff of smoke.

use bpe_core::{Registry, Scene, SlotId};
use glam::Vec2;

use crate::BasicBounce;
use crate::library::build::{Lib, hex};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::modules::look::layers::particles::{SHAPE_PARTICLES, ShapeMadeOf, ShapeParticlesLayer};
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "sparks-on-hit",
        name: "Sparks on hit",
        level: Level::Starter,
        kinds: &[Kind::LiquidSmokeSparks],
        about: "A glowing ember ball bounces round a ring, and every hit throws a burst of sparks and a puff of \
                smoke.",
        uses: &["Balls", "Shape: sparks and smoke", "Sound"],
        change_first: "How many sparks a hit throws, the smoke's size and life, and the ball's speed.",
        sound: SoundLevel::Simple,
        seconds: 15.0,
        poster_seconds: 4.75,
        preview_seconds: 12.0,
        swatch: [0xffb347, 0x120c08],
        payoff: None,
    },
    build,
};

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 15.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x120c08));
        s.bloom = 0.45;
    });
    let ring = lib.ring("Ring", Vec2::new(0.0, 40.0), 450.0, 10.0, "shape.sparks");
    // The look's sparks, more and brighter, then a puff of smoke.
    let sparks = lib.scene.card(ring, SlotId::Look).and_then(|c| c.layers.iter().find(|l| l.kind == SHAPE_PARTICLES));
    let sparks = sparks.expect("the Sparks look throws sparks").id;
    lib.layer_params(ring, SlotId::Look, sparks, |p: &mut ShapeParticlesLayer| {
        p.count = 36;
        p.speed = 520.0;
        p.size = 5.0;
        p.life = 0.7;
    });
    let smoke = lib.layer(ring, SlotId::Look, SHAPE_PARTICLES);
    lib.layer_params(ring, SlotId::Look, smoke, |p: &mut ShapeParticlesLayer| {
        p.made_of = ShapeMadeOf::Smoke;
        p.count = 5;
        p.size = 22.0;
        p.life = 1.4;
        p.speed = 70.0;
    });
    lib.notes(ring, &[52, 55, 57, 59, 62, 64], Plays::Random, Root::E, Scale::MinorPentatonic);
    let ball = lib.ball("Ball", Vec2::new(0.0, -200.0), 36.0, Vec2::new(520.0, 0.0), "ember");
    lib.card(ball, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);
    lib.finish()
}
