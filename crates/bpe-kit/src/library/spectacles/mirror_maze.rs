//! Mirror maze: a red laser zig-zags round a room of mirrors and a crystal prism. Glowing balls knock into the
//! mirrors, and each one shatters after a few hits, so the beam keeps finding a new path. When the last one
//! breaks the laser turns white, and the prism fans it into a rainbow (a red laser has only red to split).

use bpe_core::{Registry, Scene, SlotId, ThingId, Value};
use glam::Vec2;

use crate::library::build::{Lib, Tone, choice, hex, int, n, text};
use crate::library::{Entry, Kind, Level, Payoff, PayoffEvent, SceneInfo, SoundLevel};
use crate::modules::health::{AtZero, HealthSettings};
use crate::modules::lighting::LightingSettings;
use crate::modules::look::surface::SurfaceKind;
use crate::paint::Paint;
use crate::rules::breaks::BreakLook;
use crate::sound::{Plays, Root, Scale};
use crate::things::balls::{GroupShape, Launch};
use crate::things::light::{AtThings, LightType};
use crate::things::shape::ShapeFill;
use crate::things::text::TextSettings;
use crate::things::text::looks::Look;
use crate::{BallsSettings, BasicBounce, BasicMotion, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "mirror-maze",
        name: "Mirror maze",
        level: Level::Spectacle,
        kinds: &[Kind::LightAndGlass],
        about: "A red laser zig-zags round a room of mirrors. Glowing balls knock into them and each mirror \
                shatters after seven hits, so the beam keeps finding a new path. When the last one breaks the \
                laser turns white, and a crystal prism fans it into a rainbow.",
        uses: &[
            "Light: laser bouncing off shapes",
            "Shapes: chrome mirrors, a glass prism, light pipes",
            "Health: break and shatter",
            "Balls",
            "Rules: health, count, text, slow motion",
            "Lighting",
            "Text",
            "Sound",
        ],
        change_first: "Where the mirrors stand and how many hits they take (each one's Health card), and the \
                       laser's colour.",
        sound: SoundLevel::Designed,
        seconds: 30.0,
        poster_seconds: 10.0,
        preview_seconds: 15.0,
        swatch: [0xff2f4a, 0x06060c],
        payoff: Some(Payoff { what: PayoffEvent::RuleFires(LAST), at_seconds: FREE, within_seconds: 0.1 }),
    },
    build,
};

/// The mirrors-left counter, and when the last mirror breaks.
const LEFT: &str = "Mirrors left";
const FREE: f64 = 22.0;
const LAST: &str = "The last mirror breaks";

/// The laser, and the prism it points at.
const LASER_AT: Vec2 = Vec2::new(-440.0, -560.0);
const PRISM_AT: Vec2 = Vec2::new(80.0, 330.0);

/// Each mirror: where along the laser's first line it stands (a share of the way to the prism, or `None` for the
/// place given), and its turn in degrees.
const MIRRORS: [(Option<f32>, Vec2, f32); 6] = [
    (Some(0.28), Vec2::ZERO, 35.0),
    (Some(0.62), Vec2::ZERO, -50.0),
    (None, Vec2::new(330.0, -380.0), 70.0),
    (None, Vec2::new(-320.0, 260.0), -20.0),
    (None, Vec2::new(360.0, 120.0), 15.0),
    (None, Vec2::new(-200.0, 520.0), 60.0),
];

/// The laser's colour, and the white it turns at the end.
const RED: u32 = 0xff2f4a;
const WHITE: u32 = 0xffffff;

/// The notes the mirrors ring.
const NOTES: [u8; 6] = [69, 72, 74, 76, 79, 81];

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 30.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x06060c));
        s.bloom = 0.5;
    });
    let room = lib.add("shape", ThingId::ROOT, "Room");
    lib.params(room, |s: &mut ShapeSettings| *s = ShapeSettings::rectangle(1000.0, 1700.0, 10.0));
    lib.look_in(room, "shape.chrome_edge", hex(0x1a1a26));

    let prism = lib.add("shape", ThingId::ROOT, "Prism");
    lib.at(prism, PRISM_AT);
    lib.params(prism, |s: &mut ShapeSettings| {
        *s = ShapeSettings { fill: ShapeFill::Filled, spin: 10.0, ..ShapeSettings::polygon(3, 150.0, 4.0) }
    });
    lib.look_in(prism, "shape.glass", hex(0xeef6ff));
    lib.surface(prism, SurfaceKind::Prism, |g| g.rainbow = 2.5);
    lib.notes(prism, &[57, 64, 69], Plays::Random, Root::A, Scale::Minor);
    lib.tone(prism, Tone::Bell);

    // Two light pipes on the side walls glow where the beam ends on them.
    for (k, x) in [-470.0f32, 470.0].into_iter().enumerate() {
        let pipe = lib.add("shape", ThingId::ROOT, &format!("Light pipe {}", k + 1));
        lib.at(pipe, Vec2::new(x, -100.0 + k as f32 * 300.0));
        lib.params(pipe, |s: &mut ShapeSettings| {
            *s = ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::rectangle(24.0, 420.0, 4.0) }
        });
        lib.look_in(pipe, "shape.glass", hex(0xbfe4ff));
        lib.surface(pipe, SurfaceKind::Frosted, |g| {
            g.light_pipe = true;
            g.pipe_glow = 2.0;
        });
    }

    let line = PRISM_AT - LASER_AT;
    let folder = lib.add("folder", ThingId::ROOT, "Mirrors");
    let mut mirrors = Vec::new();
    for (k, (along, at, turn)) in MIRRORS.iter().enumerate() {
        let mirror = lib.add("shape", folder, &format!("Mirror {}", k + 1));
        lib.at(mirror, along.map_or(*at, |a| LASER_AT + line * a));
        lib.params(mirror, |s: &mut ShapeSettings| {
            *s = ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::rectangle(210.0, 16.0, 4.0) }
        });
        lib.scene.things.get_mut(&mirror).expect("the mirror").transform.rotation = turn.to_radians();
        lib.look_in(mirror, "shape.neon_tube", hex(0xbcd4ff));
        lib.surface(mirror, SurfaceKind::Chrome, |_| {});
        lib.fill(mirror, SlotId::Health);
        lib.card(mirror, SlotId::Health, |h: &mut HealthSettings| {
            h.start = 7.0;
            h.at_zero = AtZero::Break;
            h.look = BreakLook::Shatter;
        });
        lib.notes(mirror, &[NOTES[k]], Plays::Random, Root::A, Scale::MinorPentatonic);
        lib.tone(mirror, Tone::Mallet);
        mirrors.push(mirror);
    }

    let laser = lib.light("Laser", LightType::Laser, LASER_AT, |l| {
        l.direction = line.y.atan2(line.x).to_degrees();
        l.colour = hex(RED);
        l.strength = 3.0;
        l.laser_width = 6.0;
        l.bounces = 12;
        l.beam_length = 6000.0;
        l.at_shapes = AtThings::Bounce;
        l.at_balls = AtThings::PassThrough;
    });
    lib.card(ThingId::ROOT, SlotId::Lighting, |l: &mut LightingSettings| {
        l.ambient = 0.12;
        l.haze = 0.15;
    });

    let balls = lib.ball("Balls", Vec2::new(0.0, -200.0), 24.0, Vec2::new(0.0, 700.0), "glow_orb");
    lib.params(balls, |s: &mut BallsSettings| {
        s.group = true;
        s.shape = GroupShape::Line;
        s.count = 4;
        s.space = 70.0;
        s.launch = Launch::Random;
    });
    lib.card(balls, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 0.0);
    lib.card(balls, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);

    let title = lib.text(Look::HookTitle, "Title", "MIRROR\nMAZE");
    lib.params(title, |t: &mut TextSettings| t.size = 110.0);
    lib.at(title, Vec2::new(0.0, -720.0));
    let count = lib.text(Look::HitCounter, "Mirrors left", "MIRRORS LEFT: {counter Mirrors left}");
    lib.params(count, |t: &mut TextSettings| t.size = 60.0);
    lib.at(count, Vec2::new(0.0, -540.0));
    lib.unlit(title);
    lib.unlit(count);

    lib.rule_doing(
        "Six mirrors to start",
        &[("when.time", vec![("at", n(0.0))])],
        &[("do.count", vec![("counter", text(LEFT)), ("how", choice("set")), ("amount", n(MIRRORS.len() as f64))])],
    );
    for (k, mirror) in mirrors.iter().enumerate() {
        lib.rule_doing(
            &format!("Mirror {} breaks", k + 1),
            &[("when.health", vec![("who", lib.who(*mirror)), ("event", choice("reaches_zero"))])],
            &[("do.count", vec![("counter", text(LEFT)), ("amount", n(-1.0))])],
        );
    }
    lib.rule_doing(
        LAST,
        &[(
            "when.count",
            vec![
                ("of", choice("a_counter")),
                ("counter", text(LEFT)),
                ("reach", choice("falls_to")),
                ("amount", n(0.0)),
            ],
        )],
        &[
            ("do.text", vec![("who", lib.who(title)), ("words", text("SET\nFREE"))]),
            ("do.slow", vec![("speed", n(0.3)), ("lasts", choice("for_a_time")), ("seconds", n(1.2))]),
            ("do.camera", vec![("mode", choice("zoom_punch")), ("punch", n(0.15))]),
            ("do.note", vec![("who", lib.who(prism)), ("note", int(81)), ("velocity", n(1.0))]),
            (
                "do.change",
                vec![("who", lib.who(laser)), ("setting", text("colour")), ("to_colour", Value::Color(hex(WHITE)))],
            ),
            ("do.change", vec![("who", lib.who(laser)), ("setting", text("strength")), ("to_number", n(4.0))]),
        ],
    );
    lib.reverb(3.0, 32.0);
    lib.finish()
}
