//! Gravity flips every bar: a box of balls, and on every bar of the song gravity turns over, so the whole pile
//! falls to the ceiling and crashes down again on the beat. The lights step with each bar, the balls' notes
//! follow a four-chord progression, and at the end gravity switches off and everything floats.

use bpe_core::{Color, Gradient, NoteSlot, Registry, Scene, SlotId, ThingId, Value};
use glam::Vec2;

use crate::library::build::{Lib, Tone, choice, hex, int, n, text};
use crate::library::{Entry, Kind, Level, Payoff, PayoffEvent, SceneInfo, SoundLevel};
use crate::modules::lighting::LightingSettings;
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale, VoiceSettings};
use crate::things::balls::GroupShape;
use crate::things::rig::{Pattern, RigColours, RigSettings, StepsOn, add_rig};
use crate::things::text::TextSettings;
use crate::things::text::looks::Look;
use crate::{BallsSettings, BasicBounce, BasicMotion, SceneSettings, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "gravity-flips-every-bar",
        name: "Gravity flips every bar",
        level: Level::Spectacle,
        kinds: &[Kind::Music, Kind::BounceAndDrop],
        about: "A box of balls, and on every bar gravity turns over: the whole pile falls to the ceiling and crashes \
                back down on the beat, the lights step with the bars and the balls play the chords. At the end \
                gravity switches off and everything floats.",
        uses: &[
            "Rules: on the bar, change the scene's gravity",
            "Rig: lights stepping on the bars",
            "Sound: chords a bar each",
            "Balls: a group",
            "Slow motion",
            "Text",
        ],
        change_first: "The tempo (Scene settings), how hard gravity pulls (Scene settings' Gravity and the flip rules' To), and the chords (the Box's \
                       Sound card).",
        sound: SoundLevel::Designed,
        seconds: 32.0,
        poster_seconds: 30.5,
        preview_seconds: 12.0,
        swatch: [0x9a5aff, 0x2a2638],
        payoff: Some(Payoff { what: PayoffEvent::RuleFires(ZERO_G), at_seconds: ZERO_AT, within_seconds: 0.02 }),
    },
    build,
};

const TEMPO: f32 = 120.0;
const BAR: f64 = 2.0;
/// How hard gravity pulls: from floor to ceiling in about two beats.
const PULL: f32 = 2600.0;
const ZERO_G: &str = "Zero gravity";
/// Gravity goes away half a second into the thirteenth bar's flip, with the pile in mid-air.
const ZERO_AT: f64 = 13.0 * BAR + 0.5;
const FLIPS: &str = "Flips";
const KICK: u8 = 33;
const HAT: u8 = 90;
/// A bar each: A minor, F, C, G.
const CHORDS: [[u8; 4]; 4] = [[57, 60, 64, 69], [57, 60, 65, 69], [55, 60, 64, 67], [55, 59, 62, 67]];

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 32.0);
    lib.scene_settings(|s: &mut SceneSettings| {
        s.background = Paint::solid(hex(0x2a2638));
        s.bloom = 0.25;
        s.tempo = TEMPO;
        s.gravity = Vec2::new(0.0, PULL);
    });
    let box_ = lib.add("shape", ThingId::ROOT, "Box");
    lib.at(box_, Vec2::new(0.0, 60.0));
    lib.params(box_, |s: &mut ShapeSettings| *s = ShapeSettings::rectangle(940.0, 1400.0, 12.0));
    lib.look_in(box_, "shape.chrome_edge", hex(0x3a3050));
    lib.tone(box_, Tone::Mallet);
    lib.card(box_, SlotId::Sound, |v: &mut VoiceSettings| {
        // A chord a bar (the starter sound's Slots change by tempo, a bar each), a note of it a hit.
        v.notes_slots = CHORDS.iter().map(|c| NoteSlot { notes: c.to_vec(), order_clicked: Vec::new() }).collect();
        v.notes_plays = Plays::Random;
        v.notes_root = Root::A;
        v.notes_scale = Scale::Minor;
        v.out_volume = -6.0;
        // A crash of a few notes when the pile lands, not one for every ball.
        v.play_voices = 6;
        v.play_min_strength = 25.0;
    });

    let balls = lib.add("balls", ThingId::ROOT, "Balls");
    lib.at(balls, Vec2::new(0.0, 100.0));
    lib.params(balls, |s: &mut BallsSettings| {
        s.radius = 34.0;
        s.group = true;
        s.shape = GroupShape::Honeycomb;
        s.count = 24;
        s.columns = 6;
        s.space = 22.0;
        s.vary_size = 0.25;
    });
    lib.look_in(balls, "plain", hex(0xe8e4f0));
    lib.card(balls, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 1.0);
    lib.card(balls, SlotId::Bounce, |b: &mut BasicBounce| {
        b.bounciness = 0.45;
        b.hit_balls = true;
    });

    // The lights over the box, a colour each, stepping along on every bar.
    let rig = add_rig(&mut lib.scene, reg, ThingId::ROOT).expect("a rig");
    lib.scene.rename_thing(rig, "Bar lights");
    // The rig's spots hang just inside the box's lid, so the lid doesn't shade them.
    let spots: Vec<ThingId> = lib.scene.things.values().filter(|t| t.parent == Some(rig)).map(|t| t.id).collect();
    for spot in spots {
        lib.params(spot, |l: &mut crate::things::light::LightSettings| {
            l.position.y = -600.0;
            l.cone = 55.0;
        });
    }
    lib.params(rig, |r: &mut RigSettings| {
        r.pattern = Pattern::AlternateColours;
        r.steps_on = StepsOn::Bars;
        r.every_bars = 1.0;
        r.colours_from = RigColours::TheList;
        r.colours = Gradient::even(&[
            Color::rgb(170, 90, 255),
            Color::rgb(60, 200, 255),
            Color::rgb(255, 70, 160),
            Color::rgb(255, 190, 60),
        ]);
        r.brightness = 3.0;
    });
    lib.card(ThingId::ROOT, SlotId::Lighting, |l: &mut LightingSettings| {
        l.ambient = 0.15;
        // The beams show on the back wall too.
        l.background_lit = 0.8;
    });

    let title = lib.text(Look::HookTitle, "Title", "GRAVITY FLIPS\nEVERY BAR");
    lib.params(title, |t: &mut TextSettings| t.size = 85.0);
    lib.at(title, Vec2::new(0.0, -790.0));
    lib.unlit(title);
    let count = lib.text(Look::HitCounter, "Flip counter", "FLIPS {counter Flips}");
    lib.params(count, |t: &mut TextSettings| t.size = 80.0);
    lib.at(count, Vec2::new(0.0, 850.0));
    lib.unlit(count);

    // The beat: look-less speakers off the picture.
    let band = lib.add("folder", ThingId::ROOT, "Band");
    let speaker = |lib: &mut Lib, name: &str, tone: Tone, note: u8| {
        let id = lib.add("shape", band, name);
        lib.at(id, Vec2::new(0.0, 1400.0));
        lib.params(id, |s: &mut ShapeSettings| {
            *s = ShapeSettings { passes: true, ..ShapeSettings::rectangle(40.0, 40.0, 4.0) }
        });
        lib.scene.things.get_mut(&id).expect("made").slots.remove(&SlotId::Look);
        lib.tone(id, tone);
        lib.notes(id, &[note], Plays::Random, Root::A, Scale::Minor);
        id
    };
    let kick = speaker(&mut lib, "Kick", Tone::Kick, KICK);
    let hats = speaker(&mut lib, "Hats", Tone::Hat, HAT);

    let who = |id: ThingId| Value::Who(bpe_core::Who::Thing { thing: id, ball: None });
    let note = |id: ThingId, pitch: u8, velocity: f64| {
        ("do.note", vec![("who", who(id)), ("note", int(pitch as i64)), ("velocity", n(velocity))])
    };
    let before_zero = ("if.between", vec![("from", n(0.0)), ("to", n(ZERO_AT - 0.01))]);
    let gravity = |y: f32| {
        (
            "do.change",
            vec![("who", who(ThingId::ROOT)), ("setting", text("gravity")), ("to_pair", Value::Vec2([0.0, y]))],
        )
    };
    let flip = ("do.count", vec![("counter", text(FLIPS)), ("amount", n(1.0))]);
    // Up on the odd bars, down on the even ones.
    lib.rule_doing(
        "Gravity flips up",
        &[("when.beat", vec![("every", choice("bars")), ("bars", int(2)), ("offset", n(4.0))]), before_zero.clone()],
        &[gravity(-PULL), flip.clone(), note(kick, KICK, 1.0)],
    );
    lib.rule_doing(
        "Gravity flips down",
        &[("when.beat", vec![("every", choice("bars")), ("bars", int(2)), ("offset", n(8.0))]), before_zero.clone()],
        &[gravity(PULL), flip, note(kick, KICK, 1.0)],
    );
    lib.rule_doing(
        "Hats on the off-beats",
        &[("when.beat", vec![("offset", n(0.5))]), before_zero],
        &[note(hats, HAT, 0.5)],
    );
    lib.rule_doing(
        ZERO_G,
        &[("when.time", vec![("at", n(ZERO_AT))])],
        &[
            gravity(0.0),
            // Nothing slows them now: the pile drifts apart and floats round the box.
            (
                "do.change",
                vec![
                    ("who", who(balls)),
                    ("card", choice("bounce")),
                    ("setting", text("bounciness")),
                    ("to_number", n(1.0)),
                ],
            ),
            ("do.text", vec![("who", lib.who(title)), ("words", text("ZERO\nGRAVITY"))]),
            ("do.slow", vec![("speed", n(0.4)), ("lasts", choice("for_a_time")), ("seconds", n(2.0))]),
            ("do.camera", vec![("mode", choice("zoom_punch")), ("punch", n(0.12))]),
            note(kick, KICK, 1.0),
        ],
    );
    lib.reverb(2.6, 24.0);
    lib.finish()
}
