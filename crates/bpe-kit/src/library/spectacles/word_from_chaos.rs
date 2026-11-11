//! Word from chaos: thousands of specks whirl round the screen while glowing balls tear through them. A countdown
//! runs to the drop; on it every speck snaps home into the word ORDER and the beat comes in. Three bars later the
//! word is blown apart into chaos again.

use bpe_core::{LayerId, Registry, Scene, SlotId, ThingId, Value};
use glam::Vec2;

use crate::library::build::{Lib, Tone, choice, hex, int, n, text};
use crate::library::{Entry, Kind, Level, Payoff, PayoffEvent, SceneInfo, SoundLevel};
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};
use crate::things::particles::ParticlesSettings;
use crate::things::particles::forces::{DRAG, DragForce, SWIRL, SwirlForce, WIND};
use crate::things::particles::meets::{AtEdges, AtShapes, MeetsSettings};
use crate::things::particles::more_forces::{FLOW, Falloff, FlowForce, PUSH, Pattern, PushForce, SPRING, Toward};
use crate::things::text::TextSettings;
use crate::things::text::looks::Look;
use crate::{BallsSettings, BasicBounce, BasicMotion, SceneSettings, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "word-from-chaos",
        name: "Word from chaos",
        level: Level::Spectacle,
        kinds: &[Kind::Words],
        about: "Thousands of specks whirl round the screen while glowing balls tear through them. On the drop every \
                speck snaps home into the word ORDER and the beat comes in, until the word is blown apart again.",
        uses: &[
            "Particles: a word of specks",
            "Forces: spring home, flow, swirl, push",
            "Keyframes",
            "Balls",
            "Rules: time, beat, counter",
            "Text",
            "Sound",
        ],
        change_first: "The word (the Text \"Word\"), the drop's moment (the forces' keyframes and the rule \"The drop\"), \
                       and the specks' colours.",
        sound: SoundLevel::Designed,
        seconds: 30.0,
        poster_seconds: 18.5,
        preview_seconds: 15.0,
        swatch: [0x6fe8ff, 0x05060d],
        payoff: Some(Payoff { what: PayoffEvent::RuleFires(THE_DROP), at_seconds: DROP, within_seconds: 0.02 }),
    },
    build,
};

/// 120 beats a minute from 0 s, so a bar is 2 s.
const TEMPO: f32 = 120.0;
/// The drop, at the start of bar 9, and the scatter three bars later.
const DROP: f64 = 16.0;
const SCATTER: f64 = 22.0;
const THE_DROP: &str = "The drop";
const BEATS_LEFT: &str = "Beats to the drop";
const DROPPED: &str = "Dropped";

/// The speakers' notes.
const KICK: u8 = 33;
const HAT: u8 = 90;
/// Bass and chords a bar each: A minor, F, C, G.
const BASS: [u8; 4] = [45, 41, 48, 43];
const CHORDS: [[u8; 3]; 4] = [[57, 60, 64], [57, 60, 65], [55, 60, 64], [55, 59, 62]];

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 30.0);
    lib.scene_settings(|s: &mut SceneSettings| {
        s.background = Paint::solid(hex(0x05060d));
        s.bloom = 0.25;
        s.tempo = TEMPO;
    });
    let room = lib.add("shape", ThingId::ROOT, "Room");
    lib.params(room, |s: &mut ShapeSettings| *s = ShapeSettings::rectangle(1000.0, 1800.0, 10.0));
    lib.look_in(room, "shape.chrome_edge", hex(0x2a3b6a));
    lib.notes(room, &[69, 72, 74, 76, 79, 81, 84], Plays::Random, Root::A, Scale::MinorPentatonic);
    lib.tone(room, Tone::Bell);

    let racers = lib.add("folder", ThingId::ROOT, "Balls");
    for (k, (name, colour, at, way)) in [
        ("Cyan ball", 0x6fe8ff, Vec2::new(-300.0, 500.0), 35.0),
        ("Pink ball", 0xff4fb8, Vec2::new(250.0, -400.0), 160.0),
        ("Gold ball", 0xffc43a, Vec2::new(100.0, 650.0), -70.0),
    ]
    .into_iter()
    .enumerate()
    {
        let ball = lib.add("balls", racers, name);
        lib.at(ball, at);
        lib.params(ball, |s: &mut BallsSettings| {
            s.radius = 24.0;
            s.velocity = Vec2::from_angle(f32::to_radians(way)) * (620.0 + 60.0 * k as f32);
        });
        lib.card(ball, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);
        lib.card(ball, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 0.0);
        lib.look_in(ball, "glow_orb", hex(colour));
    }

    let word = lib.text_saying(Look::ParticleWord, "Word", "ORDER");
    lib.params(word, |t: &mut TextSettings| t.size = 180.0);
    lib.at(word, Vec2::new(0.0, 60.0));
    let specks =
        lib.scene.things.values().find(|t| t.kind == crate::things::particles::KIND).map(|t| t.id).expect("the specks");
    lib.scene.rename_thing(specks, "Specks");
    lib.params(specks, |p: &mut ParticlesSettings| {
        p.spacing = 4.0;
        p.size *= 1.4;
    });
    // The room's walls keep the chaos in.
    lib.card(specks, SlotId::Bounce, |m: &mut MeetsSettings| {
        m.at_shapes = AtShapes::Bounce;
        m.bounciness = 0.6;
        m.at_edges = AtEdges::Bounce;
    });
    // The look's gusts of wind make way for the chaos below.
    let wind = layer_of(&lib, specks, SlotId::Motion, WIND);
    lib.scene.remove_layer(reg, specks, SlotId::Motion, wind).expect("the wind");

    // Chaos: a flow that keeps changing and swirling noise, on until the drop and again after the scatter.
    let chaos = [(0.0, n(1.0)), (DROP - 0.05, n(1.0)), (DROP, n(0.0)), (SCATTER, n(0.0)), (SCATTER + 0.4, n(1.0))];
    let flow = lib.layer(specks, SlotId::Motion, FLOW);
    lib.layer_params(specks, SlotId::Motion, flow, |f: &mut FlowForce| {
        f.pattern = Pattern::Noise;
        f.speed = 220.0;
        f.size = 320.0;
        f.grip = 1.2;
        f.changes = 0.3;
    });
    let swirl = lib.layer(specks, SlotId::Motion, SWIRL);
    lib.layer_params(specks, SlotId::Motion, swirl, |w: &mut SwirlForce| {
        w.strength = 1500.0;
        w.size = 170.0;
        w.drift = 40.0;
    });
    let drag = lib.layer(specks, SlotId::Motion, DRAG);
    lib.layer_params(specks, SlotId::Motion, drag, |d: &mut DragForce| d.drag = 0.8);
    // Blasts out from the word's middle: at the start (so the word is never seen whole) and at the scatter.
    let blast = lib.layer(specks, SlotId::Motion, PUSH);
    lib.layer_params(specks, SlotId::Motion, blast, |p: &mut PushForce| {
        p.toward = Toward::APoint;
        p.point = Vec2::new(0.0, 60.0);
        p.strength = 6000.0;
        p.falloff = Falloff::Even;
        p.reach = 0.0;
    });
    // Home: off in the chaos, on from the drop to the scatter.
    let spring = layer_of(&lib, specks, SlotId::Motion, SPRING);
    let home = [(DROP - 0.05, n(0.0)), (DROP, n(1.0)), (SCATTER, n(1.0)), (SCATTER + 0.05, n(0.0))];
    let amount = |lib: &mut Lib, layer: LayerId, keys: &[(f64, Value)]| {
        let key = bpe_core::layered_key(layer, "amount");
        lib.keys(specks, Some(SlotId::Motion), &key, keys);
    };
    amount(&mut lib, flow, &chaos);
    amount(&mut lib, swirl, &chaos);
    amount(&mut lib, drag, &chaos);
    amount(&mut lib, spring, &home);
    let blasts = [(0.0, n(1.0)), (0.3, n(0.0)), (SCATTER, n(0.0)), (SCATTER + 0.05, n(1.0)), (SCATTER + 0.3, n(0.0))];
    amount(&mut lib, blast, &blasts);

    // Hidden until the first blast has torn the word apart.
    let dots = lib.scene.card(specks, SlotId::Look).expect("a Look").layers[0].id;
    let opacity = bpe_core::layered_key(dots, "opacity");
    lib.keys(specks, Some(SlotId::Look), &opacity, &[(0.0, n(0.0)), (0.25, n(0.0)), (0.8, n(0.85))]);

    let title = lib.text(Look::HookTitle, "Title", "CAN CHAOS\nSPELL A WORD?");
    lib.params(title, |t: &mut TextSettings| t.size = 100.0);
    lib.at(title, Vec2::new(0.0, -720.0));
    let count = lib.text(Look::HitCounter, "Countdown", "{counter Beats to the drop}");
    lib.params(count, |t: &mut TextSettings| t.size = 130.0);
    lib.at(count, Vec2::new(0.0, 650.0));

    // The band: speakers with no look, off the picture.
    let band = lib.add("folder", ThingId::ROOT, "Band");
    let speaker = |lib: &mut Lib, name: &str, tone: Tone| {
        let id = lib.add("shape", band, name);
        lib.at(id, Vec2::new(0.0, 1400.0));
        lib.params(id, |s: &mut ShapeSettings| {
            *s = ShapeSettings { passes: true, ..ShapeSettings::rectangle(40.0, 40.0, 4.0) }
        });
        lib.scene.things.get_mut(&id).expect("made").slots.remove(&SlotId::Look);
        lib.tone(id, tone);
        id
    };
    let kick = speaker(&mut lib, "Kick", Tone::Kick);
    lib.notes(kick, &[KICK], Plays::Random, Root::A, Scale::Minor);
    let hats = speaker(&mut lib, "Hats", Tone::Hat);
    lib.notes(hats, &[HAT], Plays::Random, Root::A, Scale::Minor);
    let bass = speaker(&mut lib, "Bass", Tone::Bass);
    lib.melody(bass, &BASS.iter().flat_map(|n| [std::slice::from_ref(n); 4]).collect::<Vec<_>>());
    let chords = speaker(&mut lib, "Chords", Tone::Pad);
    lib.melody(chords, &CHORDS.iter().map(|c| c.as_slice()).collect::<Vec<_>>());

    let counter = |name: &str, test: &str, amount: f64| {
        ("if.counter", vec![("counter", text(name)), ("test", choice(test)), ("amount", n(amount))])
    };
    let note = |id: ThingId, pitch: u8, velocity: f64| {
        (
            "do.note",
            vec![
                ("who", Value::Who(bpe_core::Who::Thing { thing: id, ball: None })),
                ("note", int(pitch as i64)),
                ("velocity", n(velocity)),
            ],
        )
    };
    let next = |id: ThingId, velocity: f64| {
        (
            "do.note",
            vec![
                ("who", Value::Who(bpe_core::Who::Thing { thing: id, ball: None })),
                ("pitch_from", choice("the_shapes_next_note")),
                ("velocity", n(velocity)),
            ],
        )
    };
    lib.rule(
        "Thirty-two beats to the drop",
        &[
            ("when.time", vec![("at", n(0.0))]),
            ("do.count", vec![("counter", text(BEATS_LEFT)), ("how", choice("set")), ("amount", n(32.0))]),
        ],
    );
    lib.rule_doing(
        "Each beat brings the drop closer",
        &[("when.beat", vec![]), counter(BEATS_LEFT, "over", 0.5)],
        &[("do.count", vec![("counter", text(BEATS_LEFT)), ("amount", n(-1.0))])],
    );
    lib.rule_doing(
        "A soft kick on each bar before the drop",
        &[("when.beat", vec![("every", choice("bar"))]), counter(DROPPED, "under", 0.5)],
        &[note(kick, KICK, 0.45)],
    );
    lib.rule_doing(
        "Hats in the last four bars before the drop",
        &[("when.beat", vec![("offset", n(0.5))]), counter(BEATS_LEFT, "under", 16.5), counter(DROPPED, "under", 0.5)],
        &[note(hats, HAT, 0.45)],
    );
    lib.rule_doing(
        THE_DROP,
        &[("when.time", vec![("at", n(DROP))])],
        &[
            ("do.count", vec![("counter", text(DROPPED)), ("how", choice("set")), ("amount", n(1.0))]),
            ("do.text", vec![("who", lib.who(title)), ("words", text("IT CAN."))]),
            ("do.text", vec![("who", lib.who(count)), ("how", choice("clear"))]),
            ("do.camera", vec![("mode", choice("zoom_punch")), ("punch", n(0.15))]),
            note(kick, KICK, 1.0),
        ],
    );
    lib.rule_doing(
        "Four on the floor after the drop",
        &[("when.beat", vec![]), counter(DROPPED, "over", 0.5)],
        &[note(kick, KICK, 1.0)],
    );
    lib.rule_doing(
        "Hats on the off-beats after the drop",
        &[("when.beat", vec![("offset", n(0.5))]), counter(DROPPED, "over", 0.5)],
        &[note(hats, HAT, 0.7)],
    );
    lib.rule_doing(
        "Bass on the off-beats after the drop",
        &[("when.beat", vec![("offset", n(0.5))]), counter(DROPPED, "over", 0.5)],
        &[next(bass, 0.8)],
    );
    lib.rule_doing(
        "A chord on each bar from the drop",
        &[
            ("when.beat", vec![("every", choice("bar"))]),
            ("if.between", vec![("from", n(DROP - 0.01)), ("to", n(30.0))]),
        ],
        &[next(chords, 0.7)],
    );
    lib.rule_doing(
        "The word scatters",
        &[("when.time", vec![("at", n(SCATTER))])],
        &[
            ("do.text", vec![("who", lib.who(title)), ("words", text("…AND IT'S GONE"))]),
            ("do.camera", vec![("mode", choice("shake")), ("amount", n(30.0))]),
        ],
    );
    lib.reverb(2.8, 28.0);
    lib.finish()
}

/// The first layer of `kind` on the card in `slot`.
fn layer_of(lib: &Lib, id: ThingId, slot: SlotId, kind: &str) -> LayerId {
    let card = lib.scene.card(id, slot).expect("a card");
    card.layers.iter().find(|l| l.kind == kind).map(|l| l.id).unwrap_or_else(|| panic!("no {kind} layer"))
}
