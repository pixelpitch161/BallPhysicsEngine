//! Fire tornado, seen from above: a molten ball beats across the eye of a whirl of fire on every beat, and each
//! hit makes the fire hotter. The air whirls the flames round and into the eye and flings embers out, all throbbing
//! with the kick; when the heat reaches its top the firestorm breaks out: the whirl nearly doubles, the light flares and
//! the full beat comes in.

use bpe_core::{Blend, Color, Gradient, LayerId, Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, Tone, choice, hex, int, n, text};
use crate::library::{Entry, Kind, Level, Payoff, PayoffEvent, SceneInfo, SoundLevel};
use crate::modules::lighting::LightingSettings;
use crate::paint::{Follow, Paint};
use crate::sound::{Plays, Root, Scale};
use crate::things::light::LightType;
use crate::things::particles::forces::{DRAG, DragForce, RISE, SWIRL, SwirlForce};
use crate::things::particles::look::{FlameLayer, STREAKS, StreaksLayer};
use crate::things::particles::looks::Look as Specks;
use crate::things::particles::more_forces::{PULL, PUSH, PullForce, PushForce, Toward};
use crate::things::particles::{AreaShape, BornFrom, Heading, ParticlesSettings};
use crate::things::text::TextSettings;
use crate::things::text::looks::Look;
use crate::{BasicBounce, BasicMotion, SceneSettings, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "fire-tornado",
        name: "Fire tornado",
        level: Level::Spectacle,
        kinds: &[Kind::LiquidSmokeSparks],
        about: "Looking down into a fire tornado: a molten ball beats across its eye on every beat, and every hit \
                makes the fire hotter. The whirl of flames and embers throbs with the kick until the firestorm \
                breaks out.",
        uses: &[
            "Particles: flames, smoke and embers",
            "Forces: pull, push, swirl",
            "Follow sound",
            "Balls: timed hits",
            "Rules: hit, beat, counter, change",
            "Lighting",
            "Text",
            "Sound",
        ],
        change_first: "The flames' forces (Pull to's Strength and Orbit, and Swirl, on their Motion card), the heat \
                       each landing adds, and \
                       the scene's Tempo (then the ball's speed).",
        sound: SoundLevel::Designed,
        seconds: 32.0,
        poster_seconds: 26.0,
        preview_seconds: 15.0,
        swatch: [0xff7a1a, 0x0a0506],
        payoff: Some(Payoff { what: PayoffEvent::Counter(HEAT, TOP), at_seconds: FIRESTORM, within_seconds: 0.05 }),
    },
    build,
};

/// 100 beats a minute; the ball starts in the middle of the eye, so its first hit is half a beat in.
const TEMPO: f32 = 100.0;
const BEAT: f64 = 60.0 / TEMPO as f64;
const FIRST_BEAT: f64 = 0.3;

/// The heat counter, what each landing adds and the top, reached on the 40th landing.
const HEAT: &str = "Heat";
const STEP: f64 = 25.0;
const TOP: f64 = 1000.0;
const FIRESTORM: f64 = FIRST_BEAT + 39.0 * BEAT;

/// The eye: where, and how big; the ball in it.
const EYE_AT: Vec2 = Vec2::new(0.0, 160.0);
const EYE: f32 = 190.0;
const BALL: f32 = 34.0;

/// The speakers' notes: the kick, the bass line (a bar each of A, F, C, G) and the hats.
const KICK: u8 = 33;
const BASS: [u8; 4] = [45, 41, 48, 43];
const HAT: u8 = 90;
const CLAP: u8 = 62;

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 32.0);
    lib.scene_settings(|s: &mut SceneSettings| {
        s.background = Paint::solid(hex(0x0a0506));
        s.bloom = 0.5;
        s.tempo = TEMPO;
        s.first_beat = FIRST_BEAT as f32;
    });

    // The eye of the tornado: the molten ball beats across it, top to bottom, once a beat.
    let eye = lib.ring("Eye", EYE_AT, EYE, 10.0, "shape.neon_tube");
    lib.look_in(eye, "shape.neon_tube", hex(0xff7a1a));
    lib.notes(eye, &[KICK], Plays::Random, Root::A, Scale::Minor);
    lib.tone(eye, Tone::Kick);
    let across = 2.0 * (EYE - 5.0 - BALL);
    let ball = lib.ball("Molten ball", EYE_AT, BALL, Vec2::new(0.0, -(across as f64 / BEAT) as f32), "lava");
    lib.card(ball, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 0.0);
    lib.card(ball, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);

    let fire = lib.add("folder", ThingId::ROOT, "Fire");
    let flames = lib.particles(Specks::FireAndSmoke, fire, "Flames");
    lib.at(flames, EYE_AT);
    lib.params(flames, |p: &mut ParticlesSettings| {
        *p = ParticlesSettings {
            // Born along a turning bar through the eye, so the whirl has two arms.
            from: BornFrom::AnArea,
            area: AreaShape::Rectangle,
            width: 1150.0,
            height: 70.0,
            rate: 2200.0,
            life: 2.0,
            life_spread: 0.3,
            speed: 120.0,
            heading: Heading::RoundTheMiddle,
            size: 16.0,
            size_spread: 0.5,
            ..p.clone()
        }
    });
    lib.keys(flames, None, "line_turn", &[(0.0, n(0.0)), (32.0, n(-5760.0))]);
    // No rising: seen from above, the air whirls the flames round and into the eye.
    let rise = force(&mut lib, flames, RISE);
    lib.scene.remove_layer(reg, flames, SlotId::Motion, rise).expect("the rise");
    let drag = force(&mut lib, flames, DRAG);
    lib.layer_params(flames, SlotId::Motion, drag, |d: &mut DragForce| d.drag = 1.2);
    let whirl = lib.layer(flames, SlotId::Motion, PULL);
    lib.layer_params(flames, SlotId::Motion, whirl, |p: &mut PullForce| {
        p.toward = Toward::APoint;
        p.point = EYE_AT;
        p.strength = 500.0;
        p.at = 300.0;
        p.orbit = 2.0;
        p.swallow = EYE + 10.0;
    });
    let swirl = lib.layer(flames, SlotId::Motion, SWIRL);
    lib.layer_params(flames, SlotId::Motion, swirl, |w: &mut SwirlForce| {
        w.strength = 300.0;
        w.size = 140.0;
        w.drift = 0.0;
    });
    let flame = lib.scene.card(flames, SlotId::Look).expect("a Look").layers[0].id;
    lib.layer_params(flames, SlotId::Look, flame, |f: &mut FlameLayer| {
        f.heat = 0.6;
        f.brightness = 1.4;
    });
    // The flames throb with the kick.
    let heat_key = bpe_core::layered_key(flame, "heat");
    lib.follow_sound(flames, Some(SlotId::Look), &heat_key, eye, 1.2, (0.0, 6.0));

    // Embers flung out of the eye, spiralling away.
    let embers = lib.particles(Specks::EmbersAndAsh, fire, "Embers");
    lib.at(embers, EYE_AT);
    lib.params(embers, |p: &mut ParticlesSettings| {
        *p = ParticlesSettings {
            from: BornFrom::AnArea,
            area: AreaShape::Ring,
            radius: EYE + 30.0,
            inner: EYE + 10.0,
            rate: 300.0,
            life: 4.0,
            speed: 150.0,
            heading: Heading::RoundTheMiddle,
            size: 3.0,
            ..p.clone()
        }
    });
    let ember_rise = force(&mut lib, embers, RISE);
    lib.scene.remove_layer(reg, embers, SlotId::Motion, ember_rise).expect("the rise");
    let fling = lib.layer(embers, SlotId::Motion, PUSH);
    lib.layer_params(embers, SlotId::Motion, fling, |p: &mut PushForce| {
        p.toward = Toward::APoint;
        p.point = EYE_AT;
        p.strength = 260.0;
        p.at = 300.0;
        p.orbit = -1.5;
    });

    // Sparks streak round the whirl, showing which way it turns.
    let sparks = lib.layer(embers, SlotId::Look, STREAKS);
    lib.layer_params(embers, SlotId::Look, sparks, |l: &mut StreaksLayer| {
        l.stretch = 0.06;
        l.width = 0.6;
        l.colour = Paint {
            colours: Gradient::even(&[Color::additive(255, 220, 140), Color::additive(255, 90, 20)]),
            follow: Follow::Life,
            ..Paint::default()
        };
        l.blend = Blend::Add;
    });

    let glow = lib.light("Fire light", LightType::Point, EYE_AT, |l| {
        l.size = 120.0;
        l.strength = 1.2;
        l.reach = 1800.0;
        l.colour = hex(0xff8a3a);
    });
    lib.follow_sound(glow, None, "strength", eye, 0.8, (0.0, 4.0));
    lib.card(ThingId::ROOT, SlotId::Lighting, |l: &mut LightingSettings| l.ambient = 0.15);

    let title = lib.text(Look::HookTitle, "Title", "HOTTER ON\nEVERY BEAT");
    lib.params(title, |t: &mut TextSettings| t.size = 110.0);
    lib.at(title, Vec2::new(0.0, -760.0));
    lib.unlit(title);
    let count = lib.text(Look::HitCounter, "Heat counter", "{counter Heat}°");
    lib.params(count, |t: &mut TextSettings| t.size = 150.0);
    lib.at(count, Vec2::new(0.0, -500.0));
    lib.unlit(count);

    // The rest of the band: speakers with no look, off the picture.
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
    let bass = speaker(&mut lib, "Bass", Tone::Bass);
    lib.melody(bass, &BASS.iter().flat_map(|n| [std::slice::from_ref(n); 4]).collect::<Vec<_>>());
    let hats = speaker(&mut lib, "Hats", Tone::Hat);
    lib.notes(hats, &[HAT], Plays::Random, Root::A, Scale::Minor);
    let clap = speaker(&mut lib, "Clap", Tone::Clap);
    lib.notes(clap, &[CLAP], Plays::Random, Root::A, Scale::Minor);

    let heat_over = |amount: f64| {
        ("if.counter", vec![("counter", text(HEAT)), ("test", choice("over")), ("amount", n(amount - 0.5))])
    };
    lib.rule(
        "Each landing makes it hotter",
        &[
            ("when.hit", vec![("who", lib.who(ball)), ("whom", lib.who(eye))]),
            ("if.counter", vec![("counter", text(HEAT)), ("test", choice("under")), ("amount", n(TOP - 0.5))]),
            ("do.count", vec![("counter", text(HEAT)), ("amount", n(STEP))]),
        ],
    );
    lib.rule_doing(
        // From the ninth landing, so its first note starts a bar of four and the chords change on the bar.
        "The bass comes in at 225°",
        &[("when.beat", vec![("offset", n(0.5))]), heat_over(225.0)],
        &[(
            "do.note",
            vec![("who", lib.who(bass)), ("pitch_from", choice("the_shapes_next_note")), ("velocity", n(0.8))],
        )],
    );
    lib.rule_doing(
        "Hats at 600°",
        &[("when.beat", vec![("offset", n(0.5))]), heat_over(600.0)],
        &[("do.note", vec![("who", lib.who(hats)), ("note", int(HAT as i64)), ("velocity", n(0.6))])],
    );
    lib.rule_doing(
        "Claps on two and four in the firestorm",
        &[("when.beat", vec![]), ("if.every", vec![("n", int(2))]), heat_over(TOP)],
        &[("do.note", vec![("who", lib.who(clap)), ("note", int(CLAP as i64)), ("velocity", n(0.9))])],
    );
    let swirl_key = text(&bpe_core::layered_key(swirl, "strength"));
    let whirl_key = text(&bpe_core::layered_key(whirl, "strength"));
    lib.rule_doing(
        "The firestorm",
        &[(
            "when.count",
            vec![
                ("of", choice("a_counter")),
                ("counter", text(HEAT)),
                ("reach", choice("rises_to")),
                ("amount", n(TOP)),
            ],
        )],
        &[
            ("do.text", vec![("who", lib.who(title)), ("words", text("FIRESTORM!"))]),
            ("do.camera", vec![("mode", choice("zoom_punch")), ("punch", n(0.2))]),
            ("do.flash", vec![("who", lib.who(glow))]),
            // Fewer flames, so the stronger whirl doesn't blow out to white.
            ("do.change", vec![("who", lib.who(flames)), ("setting", text("rate")), ("to_number", n(1400.0))]),
            (
                "do.change",
                vec![
                    ("who", lib.who(flames)),
                    ("card", choice("motion")),
                    ("setting", swirl_key),
                    ("to_number", n(700.0)),
                ],
            ),
            (
                "do.change",
                vec![
                    ("who", lib.who(flames)),
                    ("card", choice("motion")),
                    ("setting", whirl_key),
                    ("to_number", n(900.0)),
                ],
            ),
            ("do.change", vec![("who", lib.who(glow)), ("setting", text("strength")), ("to_number", n(2.2))]),
        ],
    );
    lib.reverb(2.2, 22.0);
    lib.finish()
}

/// The force layer of `kind` on a Particles thing's Forces card (the look's own, else a new one).
fn force(lib: &mut Lib, id: ThingId, kind: &str) -> LayerId {
    let card = lib.scene.card(id, SlotId::Motion).expect("a Forces card");
    match card.layers.iter().find(|l| l.kind == kind) {
        Some(l) => l.id,
        None => lib.layer(id, SlotId::Motion, kind),
    }
}
