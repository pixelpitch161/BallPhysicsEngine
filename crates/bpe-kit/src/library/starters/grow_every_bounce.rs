//! Grow every bounce: the ball gets bigger on every hit until it fills the ring.

use bpe_core::{Registry, Scene, SlotId};
use glam::Vec2;

use crate::BasicBounce;
use crate::library::build::{Lib, choice, hex, n, text};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};
use crate::things::text::TextSettings;
use crate::things::text::looks::Look;

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "grow-every-bounce",
        name: "Grow every bounce",
        level: Level::Starter,
        kinds: &[Kind::BounceAndDrop],
        about: "The ball grows a little on every bounce until it nearly fills the ring, with a bounce counter.",
        uses: &["Balls", "Shape", "Rules: change a setting", "Text: live words", "Sound"],
        change_first: "How much it grows per bounce (the rule's Times), the ring's size and the counter's words.",
        sound: SoundLevel::Simple,
        seconds: 20.0,
        poster_seconds: 9.0,
        preview_seconds: 12.0,
        swatch: [0xffa040, 0x1a1410],
        payoff: None,
    },
    build,
};

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 20.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x1a1410));
        s.bloom = 0.2;
    });
    let ring = lib.ring("Ring", Vec2::new(0.0, 80.0), 320.0, 12.0, "shape.gold_edge");
    lib.notes(ring, &[57, 60, 64], Plays::Up, Root::A, Scale::Minor);
    let ball = lib.ball("Ball", Vec2::new(0.0, -40.0), 24.0, Vec2::new(600.0, -200.0), "lava");
    lib.card(ball, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);
    let counter = lib.text(Look::HitCounter, "Bounce counter", "{hits of Ball} BOUNCES");
    lib.params(counter, |t: &mut TextSettings| t.size = 120.0);
    let (b, r) = (lib.who(ball), lib.who(ring));
    lib.rule(
        "Ball grows on every bounce",
        &[
            ("when.hit", vec![("who", b), ("whom", r)]),
            ("if.compare", vec![("value", text("size")), ("test", choice("under")), ("amount", n(195.0))]),
            (
                "do.change",
                vec![("setting", text("radius")), ("how", choice("multiply")), ("times", n(1.08)), ("glide", n(0.15))],
            ),
        ],
    );
    lib.finish()
}
