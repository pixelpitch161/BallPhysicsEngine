//! Diamond fire: a big five-sided diamond (no two sides parallel, so the light fans out) spins in the dark, a white light beam through it fans rainbow rays round the room,
//! and diamond balls bounce off it, throwing star glints. On the hundredth hit it shatters in slow motion.

use bpe_core::{Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, Tone, choice, hex, int, n, text};
use crate::library::{Entry, Kind, Level, Payoff, PayoffEvent, SceneInfo, SoundLevel};
use crate::modules::lighting::LightingSettings;
use crate::modules::look::surface::SurfaceKind;
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};
use crate::things::balls::{GroupShape, Launch};
use crate::things::light::{AtThings, LightType};
use crate::things::shape::ShapeFill;
use crate::things::text::TextSettings;
use crate::things::text::looks::Look;
use crate::{BallsSettings, BasicBounce, BasicMotion, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "diamond-fire",
        name: "Diamond fire",
        level: Level::Spectacle,
        kinds: &[Kind::LightAndGlass],
        about: "A big diamond spins in the dark. A white light beam through it fans rainbow rays round the room, \
                and diamond balls bounce off it, throwing star glints. On the hundredth hit it shatters in slow \
                motion.",
        uses: &[
            "Shape: glass with Fire, Dichroic and Rainbow",
            "Light beam",
            "Balls: diamond",
            "Rules: hit, count, break, slow motion, flash",
            "Lighting",
            "Text",
            "Sound",
        ],
        change_first: "The diamond's Surface (Fire, Dichroic and Rainbow split), its spin, and how many hits it \
                       takes (the rule \"The hundredth hit\").",
        sound: SoundLevel::Designed,
        seconds: 30.0,
        poster_seconds: 6.0,
        preview_seconds: 15.0,
        swatch: [0x9fe8ff, 0x05040a],
        payoff: Some(Payoff { what: PayoffEvent::Counter(HITS, TOP), at_seconds: SHATTER, within_seconds: 0.1 }),
    },
    build,
};

/// The hits counter, how many hits the diamond takes, and when the last one comes.
const HITS: &str = "Hits";
const TOP: f64 = 100.0;
const SHATTER: f64 = 18.233;

/// The diamond: its middle and size.
const DIAMOND_AT: Vec2 = Vec2::new(0.0, 80.0);
const DIAMOND: f32 = 270.0;

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 30.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x05040a));
        s.bloom = 0.5;
    });
    let room = lib.add("shape", ThingId::ROOT, "Room");
    lib.params(room, |s: &mut ShapeSettings| *s = ShapeSettings::rectangle(1000.0, 1700.0, 10.0));
    lib.look_in(room, "shape.chrome_edge", hex(0x1a1828));

    let diamond = lib.add("shape", ThingId::ROOT, "Diamond");
    lib.at(diamond, DIAMOND_AT);
    lib.params(diamond, |s: &mut ShapeSettings| {
        *s = ShapeSettings { fill: ShapeFill::Filled, spin: 18.0, ..ShapeSettings::polygon(5, DIAMOND, 4.0) }
    });
    lib.look_in(diamond, "shape.glass", hex(0xf2fbff));
    lib.surface(diamond, SurfaceKind::Prism, |g| {
        g.fire = 1.0;
        g.dichroic = 0.35;
        g.rainbow = 2.0;
    });
    lib.notes(diamond, &[76, 79, 81, 84, 86, 88, 91, 93], Plays::Random, Root::A, Scale::MinorPentatonic);
    lib.tone(diamond, Tone::Bell);

    // The beam comes in low from the left and fans out of the diamond's far side.
    lib.light("Light beam", LightType::Beam, Vec2::new(-520.0, 360.0), |l| {
        l.direction = -28.0;
        l.beam_width = 34.0;
        l.strength = 3.0;
        l.bounces = 10;
        l.beam_length = 2400.0;
        l.at_shapes = AtThings::PassThrough;
        l.at_balls = AtThings::PassThrough;
    });
    lib.card(ThingId::ROOT, SlotId::Lighting, |l: &mut LightingSettings| {
        l.ambient = 0.05;
        l.haze = 0.15;
        l.glass_caustics = 1.2;
    });

    let balls = lib.ball("Diamonds", Vec2::new(0.0, -520.0), 26.0, Vec2::new(0.0, 950.0), "glass");
    lib.params(balls, |s: &mut BallsSettings| {
        s.group = true;
        s.shape = GroupShape::Line;
        s.count = 18;
        s.space = 52.0;
        s.launch = Launch::Random;
    });
    lib.card(balls, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 1.0);
    lib.card(balls, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);
    lib.surface(balls, SurfaceKind::Glass, |g| {
        g.fire = 1.0;
        g.dichroic = 0.5;
    });

    let title = lib.text(Look::HookTitle, "Title", "DIAMOND\nFIRE");
    lib.params(title, |t: &mut TextSettings| t.size = 110.0);
    lib.at(title, Vec2::new(0.0, -700.0));
    let count = lib.text(Look::HitCounter, "Hit count", "{counter Hits}/100");
    lib.params(count, |t: &mut TextSettings| t.size = 80.0);
    lib.at(count, Vec2::new(0.0, 640.0));
    lib.unlit(title);
    lib.unlit(count);

    let (b, d) = (lib.who(balls), lib.who(diamond));
    lib.rule_doing(
        "A diamond ball hits the diamond",
        &[
            ("when.hit", vec![("who", b.clone()), ("whom", d.clone())]),
            ("if.counter", vec![("counter", text(HITS)), ("test", choice("under")), ("amount", n(TOP))]),
        ],
        &[("do.count", vec![("counter", text(HITS)), ("amount", n(1.0))])],
    );
    lib.rule_doing(
        "The hundredth hit",
        &[(
            "when.count",
            vec![
                ("of", choice("a_counter")),
                ("counter", text(HITS)),
                ("reach", choice("rises_to")),
                ("amount", n(TOP)),
            ],
        )],
        &[
            ("do.break", vec![("who", d), ("look", choice("shatter")), ("pieces", int(30))]),
            ("do.slow", vec![("speed", n(0.25)), ("lasts", choice("for_a_time")), ("seconds", n(1.5))]),
            ("do.flash", vec![("who", b), ("strength", n(1.0))]),
            ("do.camera", vec![("mode", choice("zoom_punch")), ("punch", n(0.15))]),
            ("do.text", vec![("who", lib.who(title)), ("words", text("SHATTERED"))]),
        ],
    );
    lib.reverb(3.2, 34.0);
    lib.finish()
}
