//! Will it escape?: a ball bounces in a spinning ring with one gap; each time one gets out, the
//! escape counter goes up and a new ball drops in.

use bpe_core::{Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, Tone, choice, hex, int, n, text};
use crate::library::{Entry, Kind, Level, Payoff, PayoffEvent, SceneInfo, SoundLevel};
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};
use crate::things::text::TextSettings;
use crate::things::text::looks::Look;
use crate::{BasicBounce, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "will-it-escape",
        name: "Will it escape?",
        level: Level::InBetween,
        kinds: &[Kind::Escape],
        about: "A ball bounces round a spinning ring with one gap. Every time a ball gets out the escape counter \
                goes up with a bright note, and a new ball drops in.",
        uses: &[
            "Balls",
            "Shape: a gap and spin",
            "Rules: leaves, only once, count, spawn",
            "Text: live words",
            "Sound",
        ],
        change_first: "How wide the gap is (the ring's Gap size), how fast the ring spins and the ball's bounciness.",
        sound: SoundLevel::Designed,
        seconds: 30.0,
        poster_seconds: 6.0,
        preview_seconds: 15.0,
        swatch: [0x7cf7ff, 0x0d0b1a],
        payoff: Some(Payoff { what: PayoffEvent::Counter("Escapes", 1.0), at_seconds: ESCAPE, within_seconds: 0.5 }),
    },
    build,
};

/// When the first ball gets out.
const ESCAPE: f64 = 2.1;

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 30.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x0d0b1a));
        s.bloom = 0.35;
    });
    let ring = lib.add("shape", ThingId::ROOT, "Ring");
    lib.at(ring, Vec2::new(0.0, 140.0));
    lib.params(ring, |s: &mut ShapeSettings| {
        *s = ShapeSettings { gaps: 1, gap_size: 0.15, spin: 60.0, ..ShapeSettings::circle(360.0, 12.0) };
    });
    lib.look(ring, "shape.glow_ring");
    lib.notes(ring, &[57, 60, 62, 64, 67, 69, 72], Plays::Up, Root::A, Scale::MinorPentatonic);
    lib.tone(ring, Tone::Mallet);
    let ball = lib.ball("Balls", Vec2::new(0.0, 60.0), 28.0, Vec2::new(900.0, -500.0), "electric");
    lib.card(ball, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);
    // Below the screen: balls that fall out of the picture are taken away.
    let drain = lib.add("shape", ThingId::ROOT, "Drain");
    lib.at(drain, Vec2::new(0.0, 1400.0));
    lib.params(drain, |s: &mut ShapeSettings| {
        *s = ShapeSettings { passes: true, ..ShapeSettings::rectangle(4000.0, 600.0, 4.0) };
    });
    let title = lib.text(Look::HookTitle, "Title", "WILL IT\nESCAPE?");
    lib.params(title, |t: &mut TextSettings| t.size = 110.0);
    lib.at(title, Vec2::new(0.0, -730.0));
    let counter = lib.text(Look::HitCounter, "Escape counter", "{counter Escapes} ESCAPED");
    lib.params(counter, |t: &mut TextSettings| t.size = 90.0);
    lib.at(counter, Vec2::new(0.0, -460.0));
    let (b, r, d) = (lib.who(ball), lib.who(ring), lib.who(drain));
    lib.rule_doing(
        "A ball gets out",
        &[
            ("when.enter", vec![("who", b.clone()), ("zone", r.clone()), ("way", choice("leaves"))]),
            // A ball that bounces back in through the gap and out again counts once.
            ("if.only", vec![("n", int(1)), ("per", choice("each_ball"))]),
        ],
        &[
            ("do.count", vec![("counter", text("Escapes")), ("amount", n(1.0))]),
            (
                "do.spawn",
                vec![
                    ("into", b.clone()),
                    ("count", int(1)),
                    ("at", choice("a_things_place")),
                    ("place", r.clone()),
                    ("way", choice("random")),
                    ("speed", n(650.0)),
                ],
            ),
            ("do.note", vec![("who", r), ("note", int(81)), ("velocity", n(0.9))]),
        ],
    );
    lib.rule(
        "Balls that fall off the screen go",
        &[("when.enter", vec![("who", b), ("zone", d)]), ("do.break", vec![])],
    );
    lib.reverb(2.5, 30.0);
    lib.finish()
}
