//! Liquid escape: a tap in the middle of a ring pours water drops that pile up inside it. The level climbs and
//! climbs until the water reaches the gap high in the ring's side and spills out, pouring down into a pool.

use bpe_core::{Color, Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, Tone, choice, hex, int, n, text};
use crate::library::{Entry, Kind, Level, Payoff, PayoffEvent, SceneInfo, SoundLevel};
use crate::modules::look::layers::particles::ParticleFade;
use crate::modules::look::surface::SurfaceKind;
use crate::paint::{Follow, Paint};
use crate::sound::{Plays, Root, Scale};
use crate::things::emitter::{Directions, EmitterSettings, FireOrder, FireWhen, Marker};
use crate::things::particles::liquid::{LIQUID, LiquidLayer};
use crate::things::particles::looks::Look as Specks;
use crate::things::particles::{BornFrom, Emit, ParticlesSettings, RidesOn};
use crate::things::shape::ShapeFill;
use crate::things::text::TextSettings;
use crate::things::text::looks::Look;
use crate::{BasicBounce, BasicMotion, SceneSettings, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "liquid-escape",
        name: "Liquid escape",
        level: Level::Spectacle,
        kinds: &[Kind::Escape, Kind::LiquidSmokeSparks],
        about: "A tap in the middle of a ring pours water that piles up inside it. The level climbs and climbs until \
                the water reaches the gap high in the ring's side and spills out, pouring down into a pool.",
        uses: &[
            "Emitter: a steady pour",
            "Balls: hit each other",
            "Shape: a gap",
            "Particles: liquid riding each drop",
            "Rules: fired, leaves, counter",
            "Text",
            "Sound",
        ],
        change_first: "The gap's place and size (lower spills sooner), the tap's pour (its Every), and the water's \
                       colours (the Water's Liquid layer).",
        sound: SoundLevel::Designed,
        seconds: 32.0,
        poster_seconds: 27.0,
        preview_seconds: 15.0,
        swatch: [0x3aa8ff, 0x060b14],
        payoff: Some(Payoff { what: PayoffEvent::Counter(SPILLED, 1.0), at_seconds: SPILL, within_seconds: 0.1 }),
    },
    build,
};

const SPILLED: &str = "Spilled";
/// When the first drop gets out (for the payoff check and the poster).
const SPILL: f64 = 21.89;

/// The ring.
const RING_AT: Vec2 = Vec2::new(-80.0, 60.0);
const RING: f32 = 360.0;
const DROP: f32 = 13.0;

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 32.0);
    lib.scene_settings(|s: &mut SceneSettings| {
        s.background = Paint::solid(hex(0x060b14));
        s.bloom = 0.35;
    });
    let ring = lib.add("shape", ThingId::ROOT, "Ring");
    lib.at(ring, RING_AT);
    lib.params(ring, |s: &mut ShapeSettings| {
        // One gap at about two o'clock.
        *s = ShapeSettings { gaps: 1, gap_size: 0.06, gap_at: 0.19, ..ShapeSettings::circle(RING, 14.0) };
    });
    lib.look_in(ring, "shape.glass", hex(0xbfe4ff));
    lib.notes(ring, &[64, 67, 69, 71, 74, 76, 79], Plays::Random, Root::E, Scale::MinorPentatonic);
    lib.tone(ring, Tone::Mallet);
    let pool = lib.add("shape", ThingId::ROOT, "Pool");
    lib.at(pool, Vec2::new(0.0, 900.0));
    lib.params(pool, |s: &mut ShapeSettings| {
        *s = ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::rectangle(1100.0, 80.0, 4.0) }
    });
    lib.look_in(pool, "shape.solid", hex(0x0d1a2c));
    lib.notes(pool, &[40, 47, 52], Plays::Random, Root::E, Scale::MinorPentatonic);
    lib.tone(pool, Tone::Pad);

    // High in the ring, above where the water stops.
    let tap = lib.emitter("Tap", RING_AT + Vec2::new(0.0, -290.0), |s: &mut EmitterSettings| {
        s.radius = DROP;
        s.directions = Directions::Fan;
        s.aim_angle = 90.0;
        s.fan_angle = 50.0;
        s.speed = 260.0;
        s.vary_speed = 0.3;
        // Three drops a round, one after another, each down its own line of the fan.
        s.count = 3;
        s.gap = 0.035;
        s.order = FireOrder::Random;
        s.fire_when = FireWhen::Every;
        s.every = 0.105;
        s.endless = true;
        s.most_balls = 800;
        s.marker = Marker::None;
    });
    lib.card(tap, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 1.0);
    lib.card(tap, SlotId::Bounce, |b: &mut BasicBounce| {
        b.bounciness = 0.15;
        b.friction = 0.3;
        b.hit_balls = true;
    });
    // The drops show only as the water they leave.
    lib.scene.things.get_mut(&tap).expect("made").slots.remove(&SlotId::Look);

    // The water: short-lived blobs left by every drop, drawn as one liquid.
    let water = lib.particles(Specks::Empty, tap, "Water");
    lib.params(water, |p: &mut ParticlesSettings| {
        *p = ParticlesSettings {
            from: BornFrom::RidingItsParent,
            rides_on: RidesOn::EachBall,
            emit: Emit::Steady,
            rate: 40.0,
            life: 0.12,
            life_spread: 0.0,
            speed: 0.0,
            speed_spread: 0.0,
            size: DROP,
            size_spread: 0.0,
            ..p.clone()
        }
    });
    let dots = lib.scene.card(water, SlotId::Look).expect("a Look").layers[0].id;
    lib.scene.remove_layer(reg, water, SlotId::Look, dots).expect("the dots");
    let liquid = lib.layer(water, SlotId::Look, LIQUID);
    lib.layer_params(water, SlotId::Look, liquid, |l: &mut LiquidLayer| {
        l.blob = 1.7;
        l.surface = SurfaceKind::Plastic;
        l.colour = Paint {
            colours: bpe_core::Gradient::even(&[Color::rgb(110, 200, 255), Color::rgb(30, 100, 230)]),
            follow: Follow::Speed,
            ..Paint::default()
        };
        l.fade = ParticleFade::None;
    });

    let title = lib.text(Look::HookTitle, "Title", "WILL IT\nOVERFLOW?");
    lib.params(title, |t: &mut TextSettings| t.size = 105.0);
    lib.at(title, Vec2::new(0.0, -790.0));
    let count = lib.text(Look::HitCounter, "Drop count", "{counter Drops} DROPS");
    lib.params(count, |t: &mut TextSettings| t.size = 80.0);
    lib.at(count, Vec2::new(0.0, -480.0));

    let (t, r) = (lib.who(tap), lib.who(ring));
    lib.rule(
        "Count every drop",
        &[("when.fired", vec![("who", t.clone())]), ("do.count", vec![("counter", text("Drops")), ("amount", n(1.0))])],
    );
    lib.rule_doing(
        "The first drop spills out",
        &[
            ("when.enter", vec![("who", t), ("zone", r.clone()), ("way", choice("leaves"))]),
            ("if.only", vec![("n", int(1))]),
        ],
        &[
            ("do.count", vec![("counter", text(SPILLED)), ("amount", n(1.0))]),
            ("do.text", vec![("who", lib.who(title)), ("words", text("IT'S\nOVERFLOWING!"))]),
            ("do.camera", vec![("mode", choice("zoom_punch")), ("punch", n(0.15))]),
            ("do.note", vec![("who", r), ("note", int(88)), ("velocity", n(1.0))]),
        ],
    );
    lib.reverb(2.4, 26.0);
    lib.finish()
}
