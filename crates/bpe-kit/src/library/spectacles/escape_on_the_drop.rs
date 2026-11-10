//! Escape on the drop: a ball inside three spinning rings with gaps, over a soft build-up. When it gets out of the
//! last ring, the ring shatters in slow motion and on the next bar the beat drops: the light turns from cold blue
//! to hot pink, the rig lights up and the full beat comes in. The drop waits for the bar, so it lands on the beat
//! even when a changed ring lets the ball out at another time.

use bpe_core::{Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Block, Lib, Tone, choice, hex, int, n, text};
use crate::library::{Entry, Kind, Level, Payoff, PayoffEvent, SceneInfo, SoundLevel};
use crate::modules::lighting::LightingSettings;
use crate::modules::look::layers::trail::TRAIL;
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};
use crate::things::light::{LightSettings, LightType};
use crate::things::rig::{Pattern, RigColours, RigSettings, StepsOn, add_rig};
use crate::things::shape::ShapeFill;
use crate::things::text::TextSettings;
use crate::things::text::looks::Look;
use crate::{BasicBounce, BasicMotion, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "escape-on-the-drop",
        name: "Escape on the drop",
        level: Level::Spectacle,
        kinds: &[Kind::Escape, Kind::Music],
        about: "A ball inside three spinning rings, over a soft build-up. When it gets out of the last ring, the \
                ring shatters in slow motion and on the next bar the beat drops: the light turns from cold blue to \
                hot pink and the full beat comes in.",
        uses: &[
            "Balls",
            "Shape: gaps and spin",
            "Rules: leaves, beat, counter, break, slow motion, light",
            "Rig",
            "Lighting",
            "Text",
            "Sound",
        ],
        change_first: "The rings' gaps and spins, the ball's speed, and the colours the light turns at the drop \
                       (the rule \"The drop: on the bar after the escape\").",
        sound: SoundLevel::Designed,
        seconds: 32.0,
        poster_seconds: 25.0,
        preview_seconds: 15.0,
        swatch: [0xff2fa0, 0x080a18],
        payoff: Some(Payoff { what: PayoffEvent::Counter("Dropped", 1.0), at_seconds: DROP, within_seconds: 0.05 }),
    },
    build,
};

/// The ball gets out of the last ring at 20.34 s, so the drop comes on the bar after it, at 21 s.
const DROP: f64 = 21.0;
const TEMPO: f32 = 120.0;
/// Bars start every 2 s from 1 s (1, 3, 5 …), so 21 s is one.
const FIRST_BEAT: f32 = 1.0;
const THE_DROP: &str = "The drop: on the bar after the escape";

/// One of the rings, inside out: name, radius, gap, spin, its notes.
const RINGS: [(&str, f32, f32, f32, [u8; 3]); 3] = [
    ("Inner ring", 180.0, 0.13, 80.0, [57, 60, 64]),
    ("Middle ring", 300.0, 0.10, -55.0, [62, 65, 69]),
    ("Outer ring", 420.0, 0.08, 32.0, [69, 72, 76]),
];

/// The speakers' notes.
const KICK: i64 = 33;
const CLAP: i64 = 62;

/// The light before and after the drop.
const COLD: u32 = 0x4a8cff;
const HOT: u32 = 0xff2fa0;

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 32.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x080a18));
        s.bloom = 0.4;
        s.tempo = TEMPO;
        s.first_beat = FIRST_BEAT;
    });
    let folder = lib.add("folder", ThingId::ROOT, "Rings");
    let mut rings = Vec::new();
    for (k, (name, radius, gap, spin, notes)) in RINGS.iter().enumerate() {
        let ring = lib.add("shape", folder, name);
        lib.at(ring, Vec2::new(0.0, 80.0));
        lib.params(ring, |s: &mut ShapeSettings| {
            *s = ShapeSettings {
                gaps: 1,
                gap_size: *gap,
                gap_at: k as f32 * 0.37,
                spin: *spin,
                ..ShapeSettings::circle(*radius, 10.0)
            };
        });
        lib.look_in(ring, "shape.neon_tube", hex(0x8fd0ff));
        lib.notes(ring, notes, Plays::Random, Root::A, Scale::Minor);
        lib.tone(ring, Tone::Pluck);
        rings.push(ring);
    }
    let ball = lib.ball("Ball", Vec2::new(0.0, 80.0), 22.0, Vec2::from_angle(40f32.to_radians()) * 640.0, "glow_orb");
    lib.card(ball, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 0.0);
    lib.card(ball, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);
    lib.layer(ball, SlotId::Look, TRAIL);
    // Once out, the ball bounces round the room for the rest of the song.
    let room = lib.add("shape", ThingId::ROOT, "Room");
    lib.params(room, |s: &mut ShapeSettings| *s = ShapeSettings::rectangle(1040.0, 1880.0, 12.0));
    lib.look_in(room, "shape.chrome_edge", hex(0x1a1c2c));

    // The speakers play the beat (balls pass through them).
    let speakers = lib.add("folder", ThingId::ROOT, "Speakers");
    let speaker = |lib: &mut Lib, name: &str, x: f32, tone: Tone, note: u8| {
        let id = lib.add("shape", speakers, name);
        lib.at(id, Vec2::new(x, 800.0));
        lib.params(id, |s: &mut ShapeSettings| {
            *s = ShapeSettings { fill: ShapeFill::Filled, passes: true, ..ShapeSettings::rectangle(160.0, 220.0, 4.0) }
        });
        lib.look_in(id, "shape.chrome_edge", hex(0x1a1c2c));
        lib.notes(id, &[note], Plays::Random, Root::A, Scale::Minor);
        lib.tone(id, tone);
        id
    };
    let kick = speaker(&mut lib, "Kick speaker", -400.0, Tone::Kick, KICK as u8);
    let clap = speaker(&mut lib, "Clap speaker", 400.0, Tone::Clap, CLAP as u8);

    let title = lib.text(Look::HookTitle, "Title", "WAIT FOR\nTHE DROP");
    lib.params(title, |t: &mut TextSettings| t.size = 110.0);
    lib.at(title, Vec2::new(0.0, -720.0));
    let count = lib.text(Look::HitCounter, "Rings left", "RINGS LEFT: {counter Rings left}");
    lib.params(count, |t: &mut TextSettings| t.size = 70.0);
    lib.at(count, Vec2::new(0.0, -470.0));

    let lamp = lib.light("Lamp", LightType::Point, Vec2::new(0.0, -300.0), |l| {
        l.size = 40.0;
        l.strength = 1.4;
        l.reach = 2600.0;
        l.colour = hex(COLD);
    });
    let rig = add_rig(&mut lib.scene, reg, ThingId::ROOT).expect("a rig");
    lib.scene.rename_thing(rig, "Drop lights");
    lib.params(rig, |r: &mut RigSettings| {
        r.pattern = Pattern::Chase;
        r.steps_on = StepsOn::Beats;
        r.colours_from = RigColours::TheList;
        r.colours = bpe_core::Gradient::even(&[hex(HOT), hex(0xffb02f), hex(0x9b5cff)]);
        r.off_level = 0.0;
        r.tail = 0.4;
        r.brightness = 0.0;
    });
    let spots: Vec<ThingId> = lib.scene.things.values().filter(|t| t.parent == Some(rig)).map(|t| t.id).collect();
    for (k, spot) in spots.into_iter().enumerate() {
        lib.scene.rename_thing(spot, &format!("Spot {}", k + 1));
        lib.params(spot, |l: &mut LightSettings| {
            l.cone = 20.0;
            l.strength = 1.6;
            l.beam = 0.8;
        });
    }
    lib.card(ThingId::ROOT, SlotId::Lighting, |l: &mut LightingSettings| {
        l.ambient = 0.25;
        l.haze = 0.15;
    });

    lib.rule_doing(
        "Three rings to start",
        &[("when.time", vec![("at", n(0.0))])],
        &[("do.count", vec![("counter", text("Rings left")), ("how", choice("set")), ("amount", n(3.0))])],
    );
    let b = lib.who(ball);
    for (k, ring) in rings.iter().enumerate() {
        let name = RINGS[k].0;
        let r = lib.who(*ring);
        let mut dos: Vec<Block> = vec![
            ("do.break", vec![("who", r.clone()), ("look", choice("shatter")), ("pieces", int(16))]),
            ("do.count", vec![("counter", text("Rings left")), ("amount", n(-1.0))]),
        ];
        match rings.get(k + 1) {
            Some(next) => {
                dos.push(("do.camera", vec![("mode", choice("zoom_punch")), ("punch", n(0.08))]));
                let note = RINGS[k + 1].4[2] as i64 + 12;
                dos.push(("do.note", vec![("who", lib.who(*next)), ("note", int(note)), ("velocity", n(1.0))]));
            }
            // The last escape: the ring shatters in slow motion and the drop waits for the bar.
            None => dos.extend([
                ("do.count", vec![("counter", text("Escaped")), ("amount", n(1.0))]),
                ("do.text", vec![("who", lib.who(title)), ("words", text("HERE IT\nCOMES"))]),
                ("do.slow", vec![("speed", n(0.3)), ("lasts", choice("for_a_time")), ("seconds", n(0.6))]),
            ]),
        }
        lib.rule_doing(
            &format!("{name} breaks when the ball gets out"),
            &[("when.enter", vec![("who", b.clone()), ("zone", r), ("way", choice("leaves"))])],
            &dos,
        );
    }
    let counter = |name: &str, test: &str| {
        ("if.counter", vec![("counter", text(name)), ("test", choice(test)), ("amount", n(0.5))])
    };
    lib.rule_doing(
        THE_DROP,
        &[("when.beat", vec![("every", choice("bar"))]), counter("Escaped", "over"), counter("Dropped", "under")],
        &the_drop(&lib, title, lamp, rig, kick),
    );
    let dropped = |test: &str| counter("Dropped", test);
    lib.rule_doing(
        "A soft beat before the drop",
        &[("when.beat", vec![("every", choice("bar"))]), counter("Escaped", "under")],
        &[("do.note", vec![("who", lib.who(kick)), ("note", int(KICK)), ("velocity", n(0.5))])],
    );
    lib.rule_doing(
        "Four on the floor after the drop",
        &[("when.beat", vec![]), dropped("over")],
        &[("do.note", vec![("who", lib.who(kick)), ("note", int(KICK)), ("velocity", n(1.0))])],
    );
    lib.rule_doing(
        "Claps on two and four after the drop",
        // Every second beat from the first bar's first: beats 2 and 4.
        &[("when.beat", vec![]), ("if.every", vec![("n", int(2))]), dropped("over")],
        &[("do.note", vec![("who", lib.who(clap)), ("note", int(CLAP)), ("velocity", n(0.9))])],
    );
    lib.reverb(3.0, 30.0);
    lib.finish()
}

/// The drop, on the bar after the last escape: everything it changes.
fn the_drop<'k>(lib: &Lib, title: ThingId, lamp: ThingId, rig: ThingId, kick: ThingId) -> Vec<Block<'k>> {
    vec![
        ("do.count", vec![("counter", text("Dropped")), ("amount", n(1.0))]),
        // The counter changes from the next step, so the drop plays its own downbeat.
        ("do.note", vec![("who", lib.who(kick)), ("note", int(KICK)), ("velocity", n(1.0))]),
        ("do.text", vec![("who", lib.who(title)), ("words", text("DROP!"))]),
        ("do.camera", vec![("mode", choice("zoom_punch")), ("punch", n(0.2))]),
        (
            "do.change",
            vec![("who", lib.who(lamp)), ("setting", text("colour")), ("to_colour", bpe_core::Value::Color(hex(HOT)))],
        ),
        ("do.change", vec![("who", lib.who(lamp)), ("setting", text("strength")), ("to_number", n(0.5))]),
        ("do.change", vec![("who", lib.who(rig)), ("setting", text("brightness")), ("to_number", n(1.0))]),
    ]
}
