//! Letter smash: the word SMASH is a wall, and balls rain down on it. Every letter can take a few hits; each
//! hit glitches the word, the letters melt more the more they are hurt, and a letter out of health shatters.
//! When the last one goes the word is gone.

use bpe_core::{Address, Pulse, PulseSource, Registry, Scene, SlotId, ThingId, Value, layered_key};
use glam::Vec2;

use crate::library::build::{Lib, Tone, choice, hex, int, n, text};
use crate::library::{Entry, Kind, Level, Payoff, PayoffEvent, SceneInfo, SoundLevel};
use crate::modules::look::shape::effects::MELT;
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};
use crate::things::emitter::{Directions, EmitterSettings, FireWhen, Marker};
use crate::things::text::TextSettings;
use crate::things::text::looks::Look;
use crate::{BasicBounce, SceneSettings, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "letter-smash",
        name: "Letter smash",
        level: Level::Spectacle,
        kinds: &[Kind::Words],
        about: "The word SMASH is a wall, and balls rain down on it. Every hit glitches the word, the letters melt \
                more the more they are hurt, and a letter out of health shatters, until the word is gone.",
        uses: &[
            "Text: real letters with health",
            "Text effects: glitch, colour split, melt",
            "Follow health",
            "Emitter",
            "Rules: health, counter, break, slow motion",
            "Sound",
        ],
        change_first: "The word (the Text \"Word\"), each letter's Starting health on its Health card, and how hard \
                       the Glitch and Melt layers go.",
        sound: SoundLevel::Designed,
        seconds: 30.0,
        poster_seconds: 19.5,
        preview_seconds: 15.0,
        swatch: [0xff5a4a, 0x0b0710],
        payoff: Some(Payoff { what: PayoffEvent::RuleFires(LAST), at_seconds: GONE, within_seconds: 0.05 }),
    },
    build,
};

const LETTERS_LEFT: &str = "Letters left";
const LAST: &str = "The last letter breaks";
/// When the last letter goes (for the payoff check).
const GONE: f64 = 25.31;
/// Hits each letter takes.
const HEALTH: f64 = 26.0;

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 30.0);
    lib.scene_settings(|s: &mut SceneSettings| {
        s.background = Paint::solid(hex(0x0b0710));
        s.bloom = 0.3;
        s.tempo = 110.0;
    });
    let room = lib.add("shape", ThingId::ROOT, "Room");
    lib.params(room, |s: &mut ShapeSettings| *s = ShapeSettings::rectangle(1020.0, 1860.0, 10.0));
    lib.look_in(room, "shape.chrome_edge", hex(0x2a2236));
    lib.notes(room, &[45, 52, 57], Plays::Random, Root::A, Scale::Minor);
    lib.tone(room, Tone::Kick);

    let word = lib.text_saying(Look::LettersBreakApart, "Word", "SMASH");
    // Each letter its own note, low to high.
    lib.notes(word, &[57, 60, 64, 67, 69], Plays::ByLetter, Root::A, Scale::Minor);
    lib.tone(word, Tone::Pluck);
    lib.card(word, SlotId::Health, |h: &mut crate::modules::health::HealthSettings| {
        h.start = HEALTH;
        // Only real hits count, not a ball rolling on a letter.
        h.least = 250.0;
    });
    // Glitch and colour split go harder on hits; the melt runs further as the word is hurt.
    let glitch = lib.layer(word, SlotId::Look, crate::modules::look::shape::effects::GLITCH);
    set(&mut lib, word, glitch, &[("amount", n(0.08)), ("band", n(16.0)), ("tear", n(40.0)), ("harder", n(1.0))]);
    let split = lib.layer(word, SlotId::Look, crate::modules::look::shape::effects::SPLIT);
    set(&mut lib, word, split, &[("distance", n(3.0)), ("harder", n(1.0))]);
    let melt = lib.layer(word, SlotId::Look, MELT);
    set(&mut lib, word, melt, &[("amount", n(1.0)), ("drip", n(260.0)), ("speed", n(0.15)), ("width", n(14.0))]);
    lib.scene.pulses.insert(
        Address { thing: word, slot: Some(SlotId::Look), key: layered_key(melt, "amount") },
        Pulse {
            source: PulseSource::Health { thing: word },
            amount: n(-1.0),
            range: Some((0.0, 1.0)),
            ..Default::default()
        },
    );

    // The look's balls, and more raining in from the top.
    let balls = lib
        .scene
        .things
        .values()
        .find(|t| t.kind == crate::things::balls::KIND)
        .map(|t| t.id)
        .expect("the look's balls");
    lib.scene.rename_thing(balls, "Wrecking balls");
    lib.card(balls, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 0.95);
    lib.look_in(balls, "glow_orb", hex(0xffd23a));
    let rain = lib.emitter("Rain of balls", Vec2::new(0.0, -860.0), |s: &mut EmitterSettings| {
        s.radius = 20.0;
        s.directions = Directions::Fan;
        s.aim_angle = 90.0;
        s.fan_angle = 70.0;
        s.speed = 500.0;
        s.count = 2;
        s.fire_when = FireWhen::Every;
        s.every = 1.5;
        s.start_delay = 2.0;
        s.endless = true;
        s.most_balls = 24;
        s.marker = Marker::None;
    });
    lib.card(rain, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 0.95);
    lib.look_in(rain, "glow_orb", hex(0x4fd8ff));

    let title = lib.text(Look::HookTitle, "Title", "SMASH\nTHE WORD");
    lib.params(title, |t: &mut TextSettings| t.size = 110.0);
    lib.at(title, Vec2::new(0.0, -740.0));
    let count = lib.text(Look::HitCounter, "Letters counter", "LETTERS LEFT: {counter Letters left}");
    lib.params(count, |t: &mut TextSettings| t.size = 70.0);
    lib.at(count, Vec2::new(0.0, 700.0));

    let w = lib.who(word);
    lib.rule(
        "Five letters to start",
        &[
            ("when.time", vec![("at", n(0.0))]),
            ("do.count", vec![("counter", text(LETTERS_LEFT)), ("how", choice("set")), ("amount", n(5.0))]),
        ],
    );
    lib.rule_doing(
        "A letter shatters",
        &[("when.health", vec![("who", w.clone()), ("event", choice("reaches_zero"))])],
        &[
            ("do.count", vec![("counter", text(LETTERS_LEFT)), ("amount", n(-1.0))]),
            ("do.camera", vec![("mode", choice("zoom_punch")), ("punch", n(0.1))]),
            ("do.note", vec![("who", lib.who(room)), ("note", int(33)), ("velocity", n(1.0))]),
        ],
    );
    lib.rule_doing(
        LAST,
        &[(
            "when.count",
            vec![
                ("of", choice("a_counter")),
                ("counter", text(LETTERS_LEFT)),
                ("reach", choice("falls_to")),
                ("amount", n(0.0)),
            ],
        )],
        &[
            ("do.text", vec![("who", lib.who(title)), ("words", text("SMASHED!"))]),
            ("do.text", vec![("who", lib.who(count)), ("words", text("NO LETTERS LEFT"))]),
            ("do.slow", vec![("speed", n(0.3)), ("lasts", choice("for_a_time")), ("seconds", n(1.0))]),
            ("do.camera", vec![("mode", choice("shake")), ("amount", n(40.0))]),
        ],
    );
    lib.reverb(2.0, 22.0);
    lib.finish()
}

/// Sets a layer's settings on `id`'s Look card.
fn set(lib: &mut Lib, id: ThingId, layer: bpe_core::LayerId, params: &[(&str, Value)]) {
    let card = lib.scene.card_mut(id, SlotId::Look).expect("a Look card");
    for (k, v) in params {
        card.params.set(&layered_key(layer, k), v.clone());
    }
}
