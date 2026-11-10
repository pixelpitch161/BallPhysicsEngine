//! One to a million: a ball in a spinning ring with a gap. Every ball that gets out brings two new ones
//! in the middle, so the balls double and double. When the count reaches its top, the ring shatters and a Huge
//! swarm of a million sparks pours out of the middle.

use bpe_core::{Color, Gradient, Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, Tone, choice, hex, int, n, text};
use crate::library::{Entry, Kind, Level, Payoff, PayoffEvent, SceneInfo, SoundLevel};
use crate::paint::{Follow, Paint};
use crate::sound::{Plays, Root, Scale};
use crate::things::emitter::{EmitterSettings, FireWhen, Marker};
use crate::things::particles::forces::{DRAG, DragForce, GRAVITY, GravityForce};
use crate::things::particles::look::{ENGINE_KEY, SpritesLayer};
use crate::things::particles::looks::Look as Specks;
use crate::things::particles::{BornFrom, Emit, Heading, ParticlesSettings, RidesOn};
use crate::things::text::TextSettings;
use crate::things::text::looks::Look;
use crate::{BasicBounce, BasicMotion, SceneSettings, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "one-to-a-million",
        name: "One to a million",
        level: Level::Spectacle,
        kinds: &[Kind::Escape, Kind::LiquidSmokeSparks],
        about: "One ball in a spinning ring. Every ball that gets out brings two new ones into the middle, \
                so the balls double and double. When the count reaches its top, the ring shatters and a swarm of a \
                million sparks pours out.",
        uses: &[
            "Balls",
            "Shape: gap and spin",
            "Rules: leaves, spawn, count, break, fire, slow motion",
            "Emitter",
            "Particles: Huge swarm",
            "Text",
            "Sound",
        ],
        change_first: "The ring's gap (a bigger gap doubles faster), the count the million comes at (the rule \
                       \"A million\"), and the swarm's colours.",
        sound: SoundLevel::Designed,
        seconds: 30.0,
        poster_seconds: 24.0,
        preview_seconds: 15.0,
        swatch: [0xffb02f, 0x0b0814],
        payoff: Some(Payoff { what: PayoffEvent::Counter(BALLS, A_MILLION), at_seconds: MILLION, within_seconds: 0.1 }),
    },
    build,
};

/// The balls counter, and the count the million comes at.
const BALLS: &str = "Balls";
const TOP: f64 = 256.0;
const A_MILLION: f64 = 1_000_000.0;
/// When the count reaches the top (for the payoff check and the poster).
const MILLION: f64 = 16.8;

/// The ring.
const RING_AT: Vec2 = Vec2::new(0.0, 120.0);
const RING: f32 = 340.0;

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 30.0);
    lib.scene_settings(|s: &mut SceneSettings| {
        s.background = Paint::solid(hex(0x0b0814));
        s.bloom = 0.45;
    });
    let ring = lib.add("shape", ThingId::ROOT, "Ring");
    lib.at(ring, RING_AT);
    lib.params(ring, |s: &mut ShapeSettings| {
        *s = ShapeSettings { gaps: 1, gap_size: 0.16, spin: 45.0, ..ShapeSettings::circle(RING, 12.0) };
    });
    lib.look_in(ring, "shape.glow_ring", hex(0xffb02f));
    lib.notes(ring, &[57, 60, 62, 64, 67, 69, 72, 74, 76], Plays::Random, Root::A, Scale::MinorPentatonic);
    lib.tone(ring, Tone::Pluck);

    let balls = lib.ball("Balls", RING_AT, 14.0, Vec2::new(780.0, -450.0), "electric");
    lib.card(balls, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 0.0);
    lib.card(balls, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);

    // A circle round the whole screen: balls that fly out of it are taken away.
    let drain = lib.add("shape", ThingId::ROOT, "Edge of the screen");
    lib.params(drain, |s: &mut ShapeSettings| {
        *s = ShapeSettings { passes: true, ..ShapeSettings::circle(1150.0, 4.0) }
    });

    let title = lib.text(Look::HookTitle, "Title", "ONE TO A\nMILLION");
    lib.params(title, |t: &mut TextSettings| t.size = 110.0);
    lib.at(title, Vec2::new(0.0, -730.0));
    let count = lib.text(Look::HitCounter, "Ball count", "{counter Balls}");
    lib.params(count, |t: &mut TextSettings| t.size = 120.0);
    lib.at(count, Vec2::new(0.0, -450.0));

    // The million: an emitter fired by the rule, its one still ball carrying the swarm.
    let core = lib.emitter("Core", RING_AT, |s: &mut EmitterSettings| {
        s.radius = 30.0;
        s.speed = 0.0;
        s.count = 1;
        s.fire_when = FireWhen::ByARule;
        s.marker = Marker::None;
    });
    lib.card(core, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 0.0);
    lib.look_in(core, "glow_orb", hex(0xfff1d6));
    let swarm = lib.particles(Specks::Empty, core, "A million sparks");
    lib.params(swarm, |p: &mut ParticlesSettings| {
        *p = ParticlesSettings {
            from: BornFrom::RidingItsParent,
            rides_on: RidesOn::EachBall,
            emit: Emit::Steady,
            rate: 160_000.0,
            life: 3.5,
            life_spread: 0.4,
            speed: 1500.0,
            speed_spread: 0.6,
            heading: Heading::AWay,
            angle: -90.0,
            spread: 50.0,
            size: 1.0,
            size_spread: 0.5,
            most: 1_000_000,
            ..p.clone()
        }
    });
    lib.scene.card_mut(swarm, SlotId::Look).expect("a Look").params.set(ENGINE_KEY, choice("huge_swarm"));
    let gravity = lib.layer(swarm, SlotId::Motion, GRAVITY);
    lib.layer_params(swarm, SlotId::Motion, gravity, |g: &mut GravityForce| g.strength = 1100.0);
    let drag = lib.layer(swarm, SlotId::Motion, DRAG);
    lib.layer_params(swarm, SlotId::Motion, drag, |d: &mut DragForce| d.amount = 0.3);
    let sprites = lib.scene.card(swarm, SlotId::Look).expect("a Look").layers[0].id;
    lib.layer_params(swarm, SlotId::Look, sprites, |l: &mut SpritesLayer| {
        l.colour = Paint {
            colours: Gradient::even(&[Color::rgb(255, 250, 220), hex(0xffb02f), hex(0xff2f6d), hex(0x9b5cff)]),
            follow: Follow::Life,
            ..Paint::default()
        };
        l.opacity = 0.35;
    });

    let (b, r, d) = (lib.who(balls), lib.who(ring), lib.who(drain));
    lib.rule_doing(
        "One ball to start",
        &[("when.time", vec![("at", n(0.0))])],
        &[("do.count", vec![("counter", text(BALLS)), ("how", choice("set")), ("amount", n(1.0))])],
    );
    lib.rule_doing(
        "A ball gets out: two more",
        &[
            ("when.enter", vec![("who", b.clone()), ("zone", r.clone()), ("way", choice("leaves"))]),
            // A ball that bounces back in through the gap and out again counts once.
            ("if.only", vec![("n", int(1)), ("per", choice("each_ball"))]),
            // Until the million (whatever count "A million" waits for).
            ("if.counter", vec![("counter", text(BALLS)), ("test", choice("under")), ("amount", n(A_MILLION))]),
        ],
        &[
            ("do.count", vec![("counter", text(BALLS)), ("amount", n(2.0))]),
            (
                "do.spawn",
                vec![
                    ("into", b.clone()),
                    ("count", int(2)),
                    ("at", choice("a_things_place")),
                    ("place", r.clone()),
                    ("way", choice("random")),
                    ("speed", n(900.0)),
                ],
            ),
        ],
    );
    lib.rule_doing(
        "A million",
        &[
            (
                "when.count",
                vec![
                    ("of", choice("a_counter")),
                    ("counter", text(BALLS)),
                    ("reach", choice("rises_to")),
                    ("amount", n(TOP)),
                ],
            ),
            ("if.only", vec![("n", int(1))]),
        ],
        &[
            ("do.count", vec![("counter", text(BALLS)), ("how", choice("set")), ("amount", n(A_MILLION))]),
            ("do.break", vec![("who", r), ("look", choice("shatter")), ("pieces", int(24))]),
            ("do.fire", vec![("who", lib.who(core))]),
            ("do.text", vec![("who", lib.who(title)), ("words", text("A\nMILLION!"))]),
            ("do.text", vec![("who", lib.who(count)), ("words", text("1,000,000"))]),
            ("do.slow", vec![("speed", n(0.3)), ("lasts", choice("for_a_time")), ("seconds", n(1.2))]),
            ("do.camera", vec![("mode", choice("zoom_punch")), ("punch", n(0.2))]),
        ],
    );
    lib.rule(
        "Balls that fly off the screen go",
        &[("when.enter", vec![("who", b.clone()), ("zone", d), ("way", choice("leaves"))]), ("do.break", vec![])],
    );
    lib.reverb(2.6, 30.0);
    lib.finish()
}
