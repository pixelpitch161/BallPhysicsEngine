//! The simulation is the song: random plinks in a ring, then a ball drops onto the stage every two bars, each
//! one a part of the beat (kick, hats, clap, bass, chords), until the bouncing balls play a finished song.

use bpe_core::{Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, Tone, choice, hex, n, text};
use crate::library::{Entry, Kind, Level, Payoff, PayoffEvent, SceneInfo, SoundLevel};
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};
use crate::things::balls::{GroupShape, Launch};
use crate::things::emitter::{Directions, EmitterSettings, FireWhen, Marker};
use crate::things::shape::ShapeFill;
use crate::things::text::TextSettings;
use crate::things::text::looks::Look;
use crate::{BallsSettings, BasicBounce, BasicMotion, SceneSettings, ShapeSettings};
use bpe_core::Settings;

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "the-simulation-is-the-song",
        name: "The simulation is the song",
        level: Level::Spectacle,
        kinds: &[Kind::Music],
        about: "It starts as random plinks in a ring. Then a ball drops onto the stage every two bars, each one a \
                part of the beat (kick, hats, clap, bass, chords), until the bouncing balls play a finished song.",
        uses: &["Balls", "Emitters: timed drops", "Shapes", "Rules: time, fired, count, text, camera", "Text", "Sound"],
        change_first: "Each player's notes and voice (its Sound card), the scene's Tempo, and when each player \
                       drops in (its emitter's After).",
        sound: SoundLevel::Designed,
        seconds: 36.0,
        poster_seconds: 26.0,
        preview_seconds: 15.0,
        swatch: [0x4fff9b, 0x0a0814],
        payoff: Some(Payoff { what: PayoffEvent::RuleFires(DROP), at_seconds: DROP_AT, within_seconds: 0.05 }),
    },
    build,
};

/// The beat: 120 a minute (a bar is 2 s), the first one an eighth of a second in.
const TEMPO: f32 = 120.0;
const FIRST_BEAT: f64 = 0.125;
const BEAT: f64 = 60.0 / TEMPO as f64;
const BAR: f64 = 4.0 * BEAT;

/// The rule for the drop, at the start of bar 13 when every player is in.
const DROP: &str = "The drop";
const DROP_AT: f64 = FIRST_BEAT + 12.0 * BAR;

/// The top of the stage.
const STAGE_TOP: f32 = 760.0;

/// One player: name, x, radius, Gravity ×, beats between landings, the bar it arrives in (from 1), the beat
/// of its first landing in that bar (from 0), colour, voice, its notes a landing (several at once a chord).
struct Player {
    name: &'static str,
    x: f32,
    radius: f32,
    gravity: f32,
    every: f64,
    bar: u32,
    beat: f64,
    colour: u32,
    tone: Tone,
    notes: &'static [&'static [u8]],
}

const PLAYERS: [Player; 5] = [
    Player {
        name: "Kick",
        x: -380.0,
        radius: 56.0,
        gravity: 3.0,
        every: 1.0,
        bar: 3,
        beat: 0.0,
        colour: 0xff2f6d,
        tone: Tone::Kick,
        notes: &[&[33]],
    },
    Player {
        name: "Hats",
        x: -190.0,
        radius: 24.0,
        gravity: 2.0,
        every: 1.0,
        bar: 5,
        beat: 0.5,
        colour: 0xffe14a,
        tone: Tone::Hat,
        notes: &[&[86]],
    },
    Player {
        name: "Clap",
        x: 0.0,
        radius: 40.0,
        gravity: 1.5,
        every: 2.0,
        bar: 7,
        beat: 1.0,
        colour: 0x2fe8ff,
        tone: Tone::Clap,
        notes: &[&[62]],
    },
    Player {
        name: "Bass",
        x: 190.0,
        radius: 40.0,
        gravity: 4.0,
        every: 1.0,
        bar: 9,
        beat: 0.5,
        colour: 0x9b5cff,
        tone: Tone::Bass,
        // A, F, C and G, a bar each, on the off-beats.
        notes: &[
            &[45],
            &[45],
            &[45],
            &[45],
            &[41],
            &[41],
            &[41],
            &[41],
            &[48],
            &[48],
            &[48],
            &[48],
            &[43],
            &[43],
            &[43],
            &[43],
        ],
    },
    Player {
        name: "Chords",
        x: 380.0,
        radius: 50.0,
        gravity: 0.75,
        every: 4.0,
        bar: 11,
        beat: 0.0,
        colour: 0x4fff9b,
        tone: Tone::Pad,
        notes: &[&[57, 60, 64], &[53, 57, 60], &[55, 60, 64], &[55, 59, 62]],
    },
];

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 36.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x0a0814));
        s.bloom = 0.35;
        s.tempo = TEMPO;
        s.first_beat = FIRST_BEAT as f32;
        // Twice the usual gravity, so the fast bouncers stay inside Gravity ×'s range.
        s.gravity *= 2.0;
    });
    let gravity = SceneSettings::from_params(&lib.scene.things[&ThingId::ROOT].params).gravity.y;

    let ring = lib.ring("Plink ring", Vec2::new(0.0, -170.0), 280.0, 8.0, "shape.glow_ring");
    lib.notes(ring, &[69, 72, 74, 76, 79, 81, 84, 86], Plays::Random, Root::A, Scale::MinorPentatonic);
    lib.tone(ring, Tone::Bell);
    let plinks = lib.ball("Plinks", Vec2::new(0.0, -170.0), 16.0, Vec2::new(0.0, -380.0), "pearl");
    lib.params(plinks, |s: &mut BallsSettings| {
        s.group = true;
        s.shape = GroupShape::Line;
        s.count = 3;
        s.space = 40.0;
        s.launch = Launch::Random;
    });
    lib.card(plinks, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 0.0);
    lib.card(plinks, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);

    let stage = lib.add("shape", ThingId::ROOT, "Stage");
    lib.at(stage, Vec2::new(0.0, STAGE_TOP + 25.0));
    lib.params(stage, |s: &mut ShapeSettings| {
        *s = ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::rectangle(1000.0, 50.0, 4.0) }
    });
    lib.look_in(stage, "shape.chrome_edge", hex(0x2a2440));

    let title = lib.text(Look::HookTitle, "Title", "THE BALLS\nARE THE BAND");
    lib.params(title, |t: &mut TextSettings| t.size = 100.0);
    lib.at(title, Vec2::new(0.0, -740.0));
    let count = lib.text(Look::HitCounter, "Players", "{counter Players} OF 5 PLAYING");
    lib.params(count, |t: &mut TextSettings| t.size = 56.0);
    lib.at(count, Vec2::new(0.0, 230.0));

    let band = lib.add("folder", ThingId::ROOT, "Band");
    let mut droppers = Vec::new();
    for p in &PLAYERS {
        let g = gravity * p.gravity;
        let period = p.every * BEAT;
        // Dropped from the top of its bounce, so it lands half a bounce later and every bounce after that.
        let land = FIRST_BEAT + (p.bar - 1) as f64 * BAR + p.beat * BEAT;
        let height = g * (period as f32 / 2.0).powi(2) / 2.0;
        let dropper = lib.add(crate::things::emitter::KIND, band, p.name);
        lib.at(dropper, Vec2::new(p.x, STAGE_TOP - p.radius - height));
        lib.params(dropper, |s: &mut EmitterSettings| {
            s.radius = p.radius;
            s.directions = Directions::OneWay;
            s.aim_angle = 90.0;
            s.speed = 0.0;
            s.count = 1;
            s.fire_when = FireWhen::AtStart;
            s.start_delay = (land - period / 2.0) as f32;
            s.marker = Marker::None;
        });
        lib.card(dropper, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = p.gravity);
        lib.card(dropper, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);
        lib.look_in(dropper, "glow_orb", hex(p.colour));
        lib.melody(dropper, p.notes);
        lib.tone(dropper, p.tone);
        droppers.push(dropper);
    }
    for (p, dropper) in PLAYERS.iter().zip(&droppers) {
        lib.rule_doing(
            &format!("{} joins the band", p.name),
            &[("when.fired", vec![("who", lib.who(*dropper)), ("each", choice("a_ball"))])],
            &[("do.count", vec![("counter", text("Players")), ("amount", n(1.0))])],
        );
    }
    let t = lib.who(title);
    lib.rule_doing(
        DROP,
        &[("when.time", vec![("at", n(DROP_AT))])],
        &[
            ("do.text", vec![("who", t), ("words", text("IT'S A\nSONG!"))]),
            ("do.camera", vec![("mode", choice("zoom_punch")), ("punch", n(0.12))]),
            ("do.change", vec![("who", lib.who(ThingId::ROOT)), ("setting", text("bloom")), ("to_number", n(0.8))]),
        ],
    );
    lib.reverb(2.2, 24.0);
    lib.finish()
}
