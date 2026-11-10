//! Bounce off words: a ball bounces round inside each hollow letter of a big word.

use bpe_core::{Registry, Scene};

use crate::library::build::{Lib, hex};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};
use crate::things::text::looks::Look;

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "bounce-off-words",
        name: "Bounce off words",
        level: Level::Starter,
        kinds: &[Kind::Words],
        about: "A ball bounces round inside each hollow letter of the big word BOUNCE, and the letters play notes \
                as they are hit.",
        uses: &["Text: real letters", "Balls", "Sound"],
        change_first: "The word (the Text's Words), its font and the letters' colours.",
        sound: SoundLevel::Simple,
        seconds: 20.0,
        poster_seconds: 5.0,
        preview_seconds: 12.0,
        swatch: [0x3de8ff, 0xff4fd8],
        payoff: None,
    },
    build,
};

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 20.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x0c0a14));
        s.bloom = 0.0;
    });
    let word = lib.text_saying(Look::BallsInsideAWord, "Bounce word", "BOUNCE");
    lib.notes(word, &[60, 62, 64, 67, 69, 72], Plays::Random, Root::C, Scale::MajorPentatonic);
    lib.finish()
}
