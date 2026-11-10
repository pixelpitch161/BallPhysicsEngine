//! Every bounce makes two: each hit on the ring splits the ball in two, with a live ball count.

use bpe_core::{Registry, Scene, SlotId};
use glam::Vec2;

use crate::BasicBounce;
use crate::library::build::{Lib, choice, hex, int, n, text};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};
use crate::things::text::TextSettings;
use crate::things::text::looks::Look;

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "every-bounce-makes-two",
        name: "Every bounce makes two",
        level: Level::Starter,
        kinds: &[Kind::BounceAndDrop],
        about: "Each time a ball hits the ring it splits in two, until the ring is full of tiny balls. A live count \
                shows how many.",
        uses: &["Balls", "Shape", "Rules: split", "Text: live words", "Sound"],
        change_first: "How many each split makes (the rule's Into), the smallest size that still splits, and the \
                       ball's look.",
        sound: SoundLevel::Simple,
        seconds: 15.0,
        poster_seconds: 7.0,
        preview_seconds: 12.0,
        swatch: [0xff6fa8, 0x140d14],
        payoff: None,
    },
    build,
};

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 15.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x140d14));
        s.bloom = 0.25;
    });
    let ring = lib.ring("Ring", Vec2::new(0.0, 60.0), 450.0, 10.0, "shape.laser");
    lib.notes(ring, &[60, 64, 67, 71, 72, 76], Plays::Random, Root::C, Scale::Major);
    let ball = lib.ball("Ball", Vec2::new(0.0, -150.0), 64.0, Vec2::new(300.0, 0.0), "candy");
    lib.card(ball, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 0.98);
    let counter = lib.text(Look::HitCounter, "Ball count", "{balls left of Ball} BALLS");
    lib.params(counter, |t: &mut TextSettings| t.size = 140.0);
    let (b, r) = (lib.who(ball), lib.who(ring));
    lib.rule(
        "Every bounce makes two",
        &[
            ("when.hit", vec![("who", b), ("whom", r), ("min_strength", n(150.0))]),
            ("if.compare", vec![("value", text("size")), ("test", choice("over")), ("amount", n(9.0))]),
            ("do.split", vec![("into", int(2)), ("spread", n(30.0))]),
        ],
    );
    lib.finish()
}
