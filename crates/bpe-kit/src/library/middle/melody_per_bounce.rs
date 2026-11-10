//! Melody per bounce: every bounce plays the next note of Ode to Joy (Beethoven, public domain).
//! Guess the song before the answer shows.

use bpe_core::{Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::ShapeSettings;
use crate::library::build::{Lib, Tone, hex, n, text};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::modules::look::layers::trail::TRAIL;
use crate::paint::Paint;
use crate::things::text::looks::Look;
use crate::{BasicBounce, BasicMotion};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "melody-per-bounce",
        name: "Melody per bounce",
        level: Level::InBetween,
        kinds: &[Kind::Music],
        about: "Every bounce plays the next note of a famous tune. Guess the song before the answer shows at the \
                end.",
        uses: &["Balls", "Trail", "Shape", "Sound: Plays Melody", "Rules: set text", "Text"],
        change_first: "The melody (the box's Sound card, Plays Melody, or Import MIDI as a melody), the ball's \
                       speed and the box's size.",
        sound: SoundLevel::Designed,
        seconds: 28.0,
        poster_seconds: 8.0,
        preview_seconds: 15.0,
        swatch: [0xffd166, 0x101826],
        payoff: None,
    },
    build,
};

const C: u8 = 60;
const D: u8 = 62;
const E: u8 = 64;
const F: u8 = 65;
const G: u8 = 67;
const LOW_G: u8 = 55;

/// Ode to Joy, a note a bounce; each bar's first note has its chord under it.
const TUNE: [&[u8]; 62] = [
    // E E F G G F E D C C D E E D D
    &[E, C - 12],
    &[E],
    &[F],
    &[G],
    &[G, C - 12],
    &[F],
    &[E],
    &[D],
    &[C, F - 12],
    &[C],
    &[D],
    &[E],
    &[E, G - 24],
    &[D],
    &[D],
    // E E F G G F E D C C D E D C C
    &[E, C - 12],
    &[E],
    &[F],
    &[G],
    &[G, C - 12],
    &[F],
    &[E],
    &[D],
    &[C, F - 12],
    &[C],
    &[D],
    &[E],
    &[D, G - 24],
    &[C],
    &[C],
    // D D E C D E F E C D E F E D C D G
    &[D, G - 24],
    &[D],
    &[E],
    &[C],
    &[D, G - 24],
    &[E],
    &[F],
    &[E],
    &[C],
    &[D, G - 24],
    &[E],
    &[F],
    &[E],
    &[D],
    &[C],
    &[D],
    &[LOW_G],
    // E E F G G F E D C C D E D C C
    &[E, C - 12],
    &[E],
    &[F],
    &[G],
    &[G, C - 12],
    &[F],
    &[E],
    &[D],
    &[C, F - 12],
    &[C],
    &[D],
    &[E],
    &[D, G - 24],
    &[C],
    &[C, C - 12],
];

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 28.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x101826));
        s.bloom = 0.3;
    });
    let bbox = lib.add("shape", ThingId::ROOT, "Box");
    lib.at(bbox, Vec2::new(0.0, 160.0));
    lib.params(bbox, |s: &mut ShapeSettings| *s = ShapeSettings::rectangle(720.0, 940.0, 12.0));
    lib.look(bbox, "shape.gold_edge");
    lib.melody(bbox, &TUNE);
    lib.tone(bbox, Tone::Bell);
    // No gravity: a fast ball on a steady zigzag, so the notes come evenly.
    let ball = lib.ball("Ball", Vec2::new(-120.0, 60.0), 30.0, Vec2::new(830.0, -1000.0), "gold");
    lib.card(ball, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 0.0);
    lib.card(ball, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);
    lib.layer(ball, SlotId::Look, TRAIL);
    let title = lib.text(Look::HookTitle, "Title", "GUESS\nTHE SONG");
    lib.at(title, Vec2::new(0.0, -680.0));
    lib.rule(
        "The answer shows",
        &[
            ("when.time", vec![("at", n(ANSWER))]),
            ("do.text", vec![("who", lib.who(title)), ("words", text("ODE TO JOY"))]),
        ],
    );
    lib.reverb(2.8, 28.0);
    lib.finish()
}

/// When the title gives the answer.
const ANSWER: f64 = 25.5;
