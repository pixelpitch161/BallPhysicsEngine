//! Escape layers: a ball inside three spinning rings, each with a gap. Every ring the ball gets out of
//! shatters with a zoom punch, and the next ring out plays higher.

use bpe_core::{Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, Tone, choice, hex, int, n, text};
use crate::library::{Entry, Kind, Level, Payoff, PayoffEvent, SceneInfo, SoundLevel};
use crate::modules::look::layers::trail::TRAIL;
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};
use crate::things::text::looks::Look;
use crate::{BasicBounce, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "escape-layers",
        name: "Escape layers",
        level: Level::InBetween,
        kinds: &[Kind::Escape],
        about: "A ball inside three spinning rings, each with a gap. Every ring it gets out of shatters with a zoom \
                punch, and the next ring out plays higher.",
        uses: &["Balls", "Trail", "Shape: gaps and spin", "Rules: leaves, break, camera, note", "Text", "Sound"],
        change_first: "The rings' gap sizes and spins, and the ball's speed.",
        sound: SoundLevel::Designed,
        seconds: 26.0,
        poster_seconds: 3.9,
        preview_seconds: 15.0,
        swatch: [0xff4fd8, 0x0c0816],
        payoff: Some(Payoff { what: PayoffEvent::Breaks("Outer ring"), at_seconds: FREE, within_seconds: 0.5 }),
    },
    build,
};

/// When the ball gets out of the last ring.
const FREE: f64 = 20.75;

/// One of the rings.
struct Ring {
    name: &'static str,
    radius: f32,
    gap: f32,
    spin: f32,
    colour: u32,
    /// Its notes, higher than the ring inside it.
    notes: [u8; 3],
}

/// The rings, inside out.
const RINGS: [Ring; 3] = [
    Ring { name: "Inner ring", radius: 190.0, gap: 0.12, spin: 70.0, colour: 0x4fd18b, notes: [57, 60, 64] },
    Ring { name: "Middle ring", radius: 320.0, gap: 0.09, spin: -50.0, colour: 0x3de8ff, notes: [62, 65, 69] },
    Ring { name: "Outer ring", radius: 450.0, gap: 0.08, spin: 25.0, colour: 0xff4fd8, notes: [69, 72, 76] },
];

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 26.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x0c0816));
        s.bloom = 0.4;
    });
    let folder = lib.add("folder", ThingId::ROOT, "Rings");
    let mut rings = Vec::new();
    for (k, r) in RINGS.iter().enumerate() {
        let ring = lib.add("shape", folder, r.name);
        lib.at(ring, Vec2::new(0.0, 100.0));
        lib.params(ring, |s: &mut ShapeSettings| {
            *s = ShapeSettings {
                gaps: 1,
                gap_size: r.gap,
                gap_at: k as f32 * 0.33,
                spin: r.spin,
                ..ShapeSettings::circle(r.radius, 10.0)
            };
        });
        lib.look_in(ring, "shape.neon_tube", hex(r.colour));
        lib.notes(ring, &r.notes, Plays::Random, Root::A, Scale::Minor);
        lib.tone(ring, Tone::Pluck);
        rings.push(ring);
    }
    let ball = lib.ball("Ball", Vec2::new(0.0, 100.0), 22.0, Vec2::new(560.0, -420.0), "glow_orb");
    lib.card(ball, SlotId::Motion, |m: &mut crate::BasicMotion| m.gravity_scale = 0.0);
    lib.card(ball, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);
    lib.layer(ball, SlotId::Look, TRAIL);
    let title = lib.text(Look::HookTitle, "Title", "3 RINGS.\nCAN IT\nGET OUT?");
    lib.at(title, Vec2::new(0.0, -640.0));
    let b = lib.who(ball);
    for (k, ring) in rings.iter().enumerate() {
        let name = RINGS[k].name;
        let r = lib.who(*ring);
        let mut dos = vec![
            ("do.break", vec![("who", r.clone()), ("look", choice("shatter")), ("pieces", int(14))]),
            ("do.camera", vec![("mode", choice("zoom_punch")), ("punch", n(0.12))]),
        ];
        match rings.get(k + 1) {
            Some(next) => {
                let next_note = RINGS[k + 1].notes[2] as i64 + 12;
                dos.push(("do.note", vec![("who", lib.who(*next)), ("note", int(next_note)), ("velocity", n(1.0))]));
            }
            None => dos.push(("do.text", vec![("who", lib.who(title)), ("words", text("FREE!"))])),
        }
        lib.rule_doing(
            &format!("{name} breaks when the ball gets out"),
            &[("when.enter", vec![("who", b.clone()), ("zone", r), ("way", choice("leaves"))])],
            &dos,
        );
    }
    lib.reverb(3.0, 30.0);
    lib.finish()
}
