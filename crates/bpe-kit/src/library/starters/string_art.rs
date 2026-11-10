//! String art: two balls in a circle leave lines that stay, building a pattern.

use bpe_core::{Registry, Scene, SlotId};
use glam::Vec2;

use crate::BasicBounce;
use crate::BasicMotion;
use crate::library::build::{Lib, hex};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::modules::look::layers::trail::{TRAIL, TrailKind, TrailLayer};
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "string-art",
        name: "String art",
        level: Level::Starter,
        kinds: &[Kind::Hypnotic],
        about: "Two balls fly round inside a circle with no gravity, each leaving a thin line that stays, so the \
                lines weave a pattern.",
        uses: &["Balls", "Trail: a line that stays", "Shape", "Sound"],
        change_first: "The balls' starting speeds (they decide the pattern), the line colours and thickness.",
        sound: SoundLevel::Simple,
        seconds: 25.0,
        poster_seconds: 18.0,
        preview_seconds: 15.0,
        swatch: [0xff6fa8, 0x4fc3f0],
        payoff: None,
    },
    build,
};

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 25.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x0a0a10));
        s.bloom = 0.25;
    });
    let ring = lib.ring("Ring", Vec2::ZERO, 470.0, 6.0, "shape.outline");
    lib.notes(ring, &[62, 64, 66, 69, 71, 74], Plays::Random, Root::D, Scale::MajorPentatonic);
    for (name, at, velocity, look) in [
        ("Pink ball", Vec2::new(-120.0, -80.0), Vec2::new(530.0, 310.0), "neon_ring"),
        ("Blue ball", Vec2::new(150.0, 120.0), Vec2::new(-390.0, 460.0), "glow_orb"),
    ] {
        let ball = lib.ball(name, at, 14.0, velocity, look);
        lib.card(ball, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 0.0);
        lib.card(ball, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);
        let line = lib.layer(ball, SlotId::Look, TRAIL);
        lib.layer_params(ball, SlotId::Look, line, |t: &mut TrailLayer| {
            t.kind = TrailKind::Line;
            t.forever = true;
            t.thickness = 1.5;
        });
    }
    lib.finish()
}
