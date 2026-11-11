//! Stem battle: four balls fight in an arena, and each one is a part of the song (drums, bass, chords and
//! melody), played by its own speaker under the arena. Each hit takes health; a ball out of health shatters
//! and its speaker fades to silence and goes dark, until one part plays on alone.

use bpe_core::{Registry, Scene, SlotId, Target, ThingId, Value};
use glam::Vec2;

use crate::library::build::{Lib, Tone, choice, hex, int, n, text};
use crate::library::{Entry, Kind, Level, Payoff, PayoffEvent, SceneInfo, SoundLevel};
use crate::modules::health::{AtZero, HealthSettings, HitsTake};
use crate::modules::look::shape::GLOW;
use crate::paint::Paint;
use crate::rules::breaks::BreakLook;
use crate::sound::{Plays, Root, Scale, VoiceSettings};
use crate::things::shape::ShapeFill;
use crate::things::text::TextSettings;
use crate::things::text::looks::{Look, add_text};
use crate::{BallsSettings, BasicBounce, BasicMotion, SceneSettings, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "stem-battle",
        name: "Stem battle",
        level: Level::Spectacle,
        kinds: &[Kind::BattlesAndRaces, Kind::Music],
        about: "Four balls fight in an arena, and each is a part of the song: drums, bass, chords and melody, each \
                played by its own speaker. A ball out of health shatters and its part fades to silence, until one \
                part plays on alone.",
        uses: &[
            "Balls: hit each other",
            "Health",
            "Text: labels riding the balls",
            "Rules: on the beat, health, counter, change a sound's volume",
            "Follow sound",
            "Sound: four parts",
        ],
        change_first: "Each fighter's Starting health and how much a hit takes (its Health card), and the parts' \
                       notes (each voice's Sound card, in the Band folder).",
        sound: SoundLevel::Designed,
        seconds: 36.0,
        poster_seconds: 22.5,
        preview_seconds: 14.0,
        swatch: [0xffb03a, 0x0c0a14],
        payoff: Some(Payoff { what: PayoffEvent::RuleFires(WINNER), at_seconds: WON, within_seconds: 0.05 }),
    },
    build,
};

const PARTS_LEFT: &str = "Parts left";
const WINNER: &str = "The winner";
/// When the third ball goes (for the payoff check).
const WON: f64 = 26.52;
const TEMPO: f32 = 110.0;
/// What hurts a fighter when it hits it: the other fighters and the bumper, not the arena's wall.
const HURTS: &str = "hurts";
const ARENA_AT: Vec2 = Vec2::new(0.0, -130.0);

/// Each part: name, colour, where its ball starts, which way it goes.
const PARTS: [(&str, u32, Vec2, f32); 4] = [
    ("Drums", 0xff4a5a, Vec2::new(-230.0, -330.0), 30.0),
    ("Bass", 0x4f8bff, Vec2::new(240.0, -360.0), 140.0),
    ("Chords", 0x3ee8a0, Vec2::new(-250.0, 90.0), -60.0),
    ("Melody", 0xffc43a, Vec2::new(230.0, 110.0), -150.0),
];
const KICK: u8 = 33;
const HAT: u8 = 90;
/// A bar each: A minor, F, C, G.
const BASS: [u8; 4] = [45, 41, 48, 43];
const CHORDS: [[u8; 3]; 4] = [[57, 60, 64], [57, 60, 65], [55, 60, 64], [55, 59, 62]];
/// Four bars of tune, a note a beat (A minor pentatonic).
const TUNE: [u8; 16] = [69, 72, 76, 74, 72, 69, 67, 69, 72, 76, 79, 76, 74, 72, 69, 67];

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 36.0);
    lib.scene_settings(|s: &mut SceneSettings| {
        s.background = Paint::solid(hex(0x0c0a14));
        s.bloom = 0.3;
        s.tempo = TEMPO;
    });
    let arena = lib.add("shape", ThingId::ROOT, "Arena");
    lib.at(arena, ARENA_AT);
    lib.params(arena, |s: &mut ShapeSettings| *s = ShapeSettings::circle(470.0, 12.0));
    lib.look(arena, "shape.chrome_edge");
    let bumper = lib.add("shape", ThingId::ROOT, "Bumper");
    lib.at(bumper, ARENA_AT);
    lib.params(bumper, |s: &mut ShapeSettings| {
        *s = ShapeSettings { fill: ShapeFill::Filled, spin: 70.0, ..ShapeSettings::polygon(3, 80.0, 4.0) };
    });
    lib.look_in(bumper, "shape.solid", hex(0xe8e8f0));
    lib.scene.things.get_mut(&bumper).expect("made").tags.insert(HURTS.into());

    // The speakers under the arena, each glowing with its own part, and the voices that play the parts.
    let speakers = lib.add("folder", ThingId::ROOT, "Speakers");
    let band = lib.add("folder", ThingId::ROOT, "Band");
    let voice_of = |lib: &mut Lib, name: &str| {
        let id = lib.add("shape", band, name);
        lib.at(id, Vec2::new(0.0, 1400.0));
        lib.params(id, |s: &mut ShapeSettings| {
            *s = ShapeSettings { passes: true, ..ShapeSettings::rectangle(40.0, 40.0, 4.0) }
        });
        lib.scene.things.get_mut(&id).expect("made").slots.remove(&SlotId::Look);
        id
    };
    let mut parts = Vec::new();
    for (k, (name, colour, at, way)) in PARTS.into_iter().enumerate() {
        let voice = voice_of(&mut lib, &format!("{name} voice"));
        let speaker = lib.add("shape", speakers, &format!("{name} speaker"));
        let x = -390.0 + 260.0 * k as f32;
        lib.at(speaker, Vec2::new(x, 560.0));
        lib.params(speaker, |s: &mut ShapeSettings| *s = ShapeSettings::rectangle(200.0, 130.0, 8.0));
        lib.look_in(speaker, "shape.neon_tube", hex(colour));
        let card = lib.scene.card(speaker, SlotId::Look).expect("a Look");
        let glow = card.layers.iter().find(|l| l.kind == GLOW).map(|l| l.id).expect("a glow");
        let strength = bpe_core::layered_key(glow, "strength");
        lib.scene.card_mut(speaker, SlotId::Look).expect("a Look").params.set(&strength, n(0.25));
        lib.follow_sound(speaker, Some(SlotId::Look), &strength, voice, 3.0, (0.0, 3.5));
        let label = lib.text(Look::HitCounter, &format!("{name} label"), &name.to_uppercase());
        lib.params(label, |t: &mut TextSettings| t.size = 40.0);
        lib.at(label, Vec2::new(x, 560.0));

        let ball = lib.add("balls", ThingId::ROOT, name);
        lib.at(ball, ARENA_AT + at);
        lib.params(ball, |s: &mut BallsSettings| {
            s.radius = 58.0;
            s.velocity = Vec2::from_angle(way.to_radians()) * (640.0 + 40.0 * k as f32);
        });
        lib.look_in(ball, "glow_orb", hex(colour));
        lib.card(ball, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 0.0);
        lib.card(ball, SlotId::Bounce, |b: &mut BasicBounce| {
            b.bounciness = 1.0;
            b.hit_balls = true;
        });
        lib.fill(ball, SlotId::Health);
        lib.card(ball, SlotId::Health, |h: &mut HealthSettings| {
            h.start = 115.0;
            h.takes = HitsTake::ByStrength;
            h.factor = 0.004;
            h.from = Target::Tag(HURTS.into());
            h.at_zero = AtZero::Break;
            h.look = BreakLook::Shatter;
        });
        lib.scene.things.get_mut(&ball).expect("made").tags.insert(HURTS.into());
        // Its name riding on it.
        let tag = add_text(&mut lib.scene, reg, Look::LabelsOnBalls, ball).expect("a label");
        lib.scene.rename_thing(tag, &format!("{name} name"));
        lib.params(tag, |t: &mut TextSettings| {
            t.words = name.to_uppercase();
            t.size = 34.0;
        });
        parts.push((ball, voice, speaker, label));
    }
    let [drums, bass, chords, melody] = [parts[0].1, parts[1].1, parts[2].1, parts[3].1];
    lib.tone(drums, Tone::Kick);
    lib.notes(drums, &[KICK], Plays::Random, Root::A, Scale::Minor);
    // The drums' hats have a voice of their own that goes quiet with the drums.
    let hats = voice_of(&mut lib, "Drums hats voice");
    lib.tone(hats, Tone::Hat);
    lib.notes(hats, &[HAT], Plays::Random, Root::A, Scale::Minor);
    lib.tone(bass, Tone::Bass);
    lib.melody(bass, &BASS.iter().flat_map(|n| [std::slice::from_ref(n); 4]).collect::<Vec<_>>());
    lib.tone(chords, Tone::Pad);
    lib.melody(chords, &CHORDS.iter().map(|c| c.as_slice()).collect::<Vec<_>>());
    lib.card(chords, SlotId::Sound, |v: &mut VoiceSettings| v.out_volume = -4.0);
    lib.tone(melody, Tone::Pluck);
    lib.melody(melody, &TUNE.iter().map(std::slice::from_ref).collect::<Vec<_>>());

    let title = lib.text(Look::HookTitle, "Title", "WHICH PART\nSURVIVES?");
    lib.params(title, |t: &mut TextSettings| t.size = 100.0);
    lib.at(title, Vec2::new(0.0, -800.0));
    let count = lib.text(Look::HitCounter, "Parts counter", "{counter Parts left} PARTS LEFT");
    lib.params(count, |t: &mut TextSettings| t.size = 80.0);
    lib.at(count, Vec2::new(0.0, 780.0));

    let who = |id: ThingId| Value::Who(bpe_core::Who::Thing { thing: id, ball: None });
    let next = |id: ThingId, velocity: f64| {
        ("do.note", vec![("who", who(id)), ("pitch_from", choice("the_shapes_next_note")), ("velocity", n(velocity))])
    };
    let note = |id: ThingId, pitch: u8, velocity: f64| {
        ("do.note", vec![("who", who(id)), ("note", int(pitch as i64)), ("velocity", n(velocity))])
    };
    lib.rule(
        "Four parts to start",
        &[
            ("when.time", vec![("at", n(0.0))]),
            ("do.count", vec![("counter", text(PARTS_LEFT)), ("how", choice("set")), ("amount", n(4.0))]),
        ],
    );
    lib.rule_doing("Drums: a kick on every beat", &[("when.beat", vec![])], &[note(drums, KICK, 1.0)]);
    lib.rule_doing("Drums: hats on the off-beats", &[("when.beat", vec![("offset", n(0.5))])], &[note(hats, HAT, 0.6)]);
    lib.rule_doing("Bass on the off-beats", &[("when.beat", vec![("offset", n(0.5))])], &[next(bass, 0.8)]);
    lib.rule_doing("Chords on every bar", &[("when.beat", vec![("every", choice("bar"))])], &[next(chords, 0.7)]);
    lib.rule_doing("Melody on every beat", &[("when.beat", vec![])], &[next(melody, 0.7)]);

    let fade = |id: ThingId| {
        (
            "do.change",
            vec![
                ("who", who(id)),
                ("card", choice("sound")),
                ("setting", text("out_volume")),
                ("to_number", n(-60.0)),
                ("glide", n(2.0)),
            ],
        )
    };
    for (k, &(ball, voice, speaker, label)) in parts.iter().enumerate() {
        let name = PARTS[k].0;
        let mut dos = vec![
            ("do.count", vec![("counter", text(PARTS_LEFT)), ("amount", n(-1.0))]),
            fade(voice),
            ("do.break", vec![("who", lib.who(speaker)), ("look", choice("fade_out")), ("fade_time", n(2.0))]),
            ("do.text", vec![("who", lib.who(label)), ("words", text("MUTED"))]),
            ("do.camera", vec![("mode", choice("zoom_punch")), ("punch", n(0.1))]),
        ];
        if voice == drums {
            dos.push(fade(hats));
        }
        lib.rule_doing(
            &format!("{name} is knocked out and goes quiet"),
            &[("when.health", vec![("who", lib.who(ball)), ("event", choice("reaches_zero"))])],
            &dos,
        );
    }
    lib.rule_doing(
        WINNER,
        &[(
            "when.count",
            vec![
                ("of", choice("a_counter")),
                ("counter", text(PARTS_LEFT)),
                ("reach", choice("falls_to")),
                ("amount", n(1.0)),
            ],
        )],
        &[
            ("do.text", vec![("who", lib.who(title)), ("words", text("ONE PART\nPLAYS ON"))]),
            ("do.text", vec![("who", lib.who(count)), ("words", text("LAST ONE STANDING"))]),
            ("do.slow", vec![("speed", n(0.3)), ("lasts", choice("for_a_time")), ("seconds", n(1.2))]),
            ("do.camera", vec![("mode", choice("shake")), ("amount", n(30.0))]),
            // The last one can't be hurt any more.
            (
                "do.change",
                vec![
                    ("who", Value::Who(bpe_core::Who::Kind("balls".into()))),
                    ("card", choice("health")),
                    ("setting", text("takes")),
                    ("to_choice", text("nothing")),
                ],
            ),
        ],
    );
    lib.reverb(2.2, 22.0);
    lib.finish()
}
