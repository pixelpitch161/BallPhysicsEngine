//! Polyrhythm rings: three rings, a ball in each going straight across, hitting 3, 4 and 5 times a
//! bar, so every bar starts with all three together.

use bpe_core::{Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, Tone, hex};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::modules::look::layers::trail::TRAIL;
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};
use crate::things::text::TextSettings;
use crate::things::text::looks::Look;
use crate::{BasicBounce, BasicMotion, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "polyrhythm-rings",
        name: "Polyrhythm rings",
        level: Level::InBetween,
        kinds: &[Kind::Music],
        about: "Three rings, a ball in each: they bounce 3, 4 and 5 times a bar, each its own note, and every bar \
                starts with all three hitting together.",
        uses: &["Balls", "Trail", "Shape", "Sound: three voices", "Text: the bar"],
        change_first: "The balls' speeds (the width a ball crosses inside its ring, times hits a bar, over a \
                       bar's seconds), the notes and the tempo.",
        sound: SoundLevel::Designed,
        seconds: 25.0,
        poster_seconds: 6.3,
        preview_seconds: 12.0,
        swatch: [0x3de8ff, 0x0a0a14],
        payoff: None,
    },
    build,
};

/// Seconds a bar takes at the scene's 120 beats a minute, four beats a bar.
const BAR: f32 = 2.0;
/// The rings' radius, thickness and the balls' size.
const RING: f32 = 250.0;
const BAND: f32 = 10.0;
const BALL: f32 = 26.0;

/// Each ring, top to bottom: hits a bar, its note, its tone, its colour.
const ROWS: [(u32, u8, Tone, u32); 3] =
    [(3, 48, Tone::Bass, 0xff5a5f), (4, 67, Tone::Mallet, 0xffd166), (5, 76, Tone::Bell, 0x3de8ff)];

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 25.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x0a0a14));
        s.bloom = 0.35;
        s.tempo = 60.0 * 4.0 / BAR;
    });
    let rings = lib.add("folder", ThingId::ROOT, "Rings");
    let balls = lib.add("folder", ThingId::ROOT, "Balls");
    // A ball's middle goes from one side of the ring's inside to the other.
    let across = 2.0 * (RING - BAND * 0.5 - BALL);
    for (k, (hits, note, tone, colour)) in ROWS.iter().enumerate() {
        let y = -580.0 + k as f32 * 560.0;
        let ring = lib.add("shape", rings, &format!("Ring of {hits}"));
        lib.at(ring, Vec2::new(0.0, y + 80.0));
        lib.params(ring, |s: &mut ShapeSettings| *s = ShapeSettings::circle(RING, BAND));
        lib.look_in(ring, "shape.neon_tube", hex(*colour));
        lib.notes(ring, &[*note], Plays::Up, Root::C, Scale::Major);
        lib.tone(ring, *tone);
        // Against the left side, it hits it at once, then each side after a beat of its own.
        let speed = across * *hits as f32 / BAR;
        let ball = lib.add("balls", balls, &format!("Ball of {hits}"));
        lib.at(ball, Vec2::new(-across * 0.5 + 0.5, y + 80.0));
        lib.params(ball, |s: &mut crate::BallsSettings| {
            s.radius = BALL;
            s.velocity = Vec2::new(-speed, 0.0);
        });
        lib.look_in(ball, "glow_orb", hex(*colour));
        lib.card(ball, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 0.0);
        lib.card(ball, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);
        lib.layer(ball, SlotId::Look, TRAIL);
    }
    let bar = lib.text(Look::HitCounter, "Bar counter", "BAR {bar}");
    lib.params(bar, |t: &mut TextSettings| t.size = 80.0);
    lib.at(bar, Vec2::new(0.0, -860.0));
    lib.reverb(2.0, 24.0);
    lib.finish()
}
