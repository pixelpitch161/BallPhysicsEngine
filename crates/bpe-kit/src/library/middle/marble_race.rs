//! Marble race: six marbles drop through pegs, down zigzag ramps and past spinners to the finish,
//! with live standings and the camera riding with the pack.

use bpe_core::{Gradient, Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, Tone, choice, hex, int, n, text};
use crate::library::{Entry, Kind, Level, Payoff, PayoffEvent, SceneInfo, SoundLevel};
use crate::modules::look::layers::{GROUP_GRADIENT, GroupGradient, Spread};
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale, VoiceSettings};
use crate::things::balls::{GroupShape, Launch};
use crate::things::camera::CameraSettings;
use crate::things::shape::ShapeFill;
use crate::things::text::TextSettings;
use crate::things::text::looks::Look;
use crate::{BallsSettings, BasicBounce, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "marble-race",
        name: "Marble race",
        level: Level::InBetween,
        kinds: &[Kind::BattlesAndRaces],
        about: "Six marbles drop through pegs, down zigzag ramps and past two spinners to the finish line, with live \
                standings and the camera riding with the pack.",
        uses: &[
            "Balls: a group with a rainbow",
            "Shape: pegs, ramps, spinners and a finish zone",
            "Camera: follow",
            "Rules: enters, only once, set text, flash",
            "Text: standings",
            "Sound",
        ],
        change_first: "The course (move the pegs, ramps and spinners), the marbles' bounciness and how many race.",
        sound: SoundLevel::Designed,
        seconds: 25.0,
        poster_seconds: 4.0,
        preview_seconds: 15.0,
        swatch: [0xffd166, 0x16121e],
        payoff: Some(Payoff { what: PayoffEvent::RuleFires(FIRST), at_seconds: FINISH, within_seconds: 0.5 }),
    },
    build,
};

const FIRST: &str = "First across the line wins";
/// When the first marble crosses the line.
const FINISH: f64 = 15.9;

/// The course: its inside's width, top and bottom.
const WIDE: f32 = 980.0;
const TOP: f32 = -1000.0;
const BOTTOM: f32 = 4200.0;
/// Where the finish line is.
const LINE: f32 = 3900.0;

/// A filled bar `width` × `height` at `at`, turned by `degrees` clockwise.
fn bar(lib: &mut Lib, folder: ThingId, name: &str, at: Vec2, width: f32, degrees: f32, spin: f32) -> ThingId {
    let id = lib.add("shape", folder, name);
    lib.at(id, at);
    lib.scene.things.get_mut(&id).expect("a bar").transform.rotation = degrees.to_radians();
    lib.params(id, |s: &mut ShapeSettings| {
        *s = ShapeSettings { fill: ShapeFill::Filled, spin, ..ShapeSettings::rectangle(width, 26.0, 4.0) };
    });
    id
}

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 25.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x16121e));
        s.bloom = 0.25;
    });
    let course = lib.add("folder", ThingId::ROOT, "Course");
    let walls = lib.add("shape", course, "Walls");
    lib.at(walls, Vec2::new(0.0, (TOP + BOTTOM) * 0.5));
    lib.params(walls, |s: &mut ShapeSettings| *s = ShapeSettings::rectangle(WIDE, BOTTOM - TOP, 16.0));
    lib.look(walls, "shape.chrome_edge");
    // Pegs: staggered rows under the start.
    let pegs = lib.add("folder", course, "Pegs");
    let mut k = 0;
    for row in 0..7 {
        let shift = if row % 2 == 0 { 0.0 } else { 70.0 };
        for col in 0..7 {
            let x = -420.0 + shift + col as f32 * 140.0;
            if x > 440.0 {
                continue;
            }
            k += 1;
            let peg = lib.add("shape", pegs, &format!("Peg {k}"));
            lib.at(peg, Vec2::new(x, -500.0 + row as f32 * 150.0));
            lib.params(peg, |s: &mut ShapeSettings| {
                *s = ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::circle(16.0, 4.0) };
            });
            lib.look_in(peg, "shape.solid", hex(0xffd166));
        }
    }
    // Ramps: zigzag down, each leaving a gap at its low end.
    let ramps = lib.add("folder", course, "Ramps");
    let mut look_of = Vec::new();
    for r in 0..4 {
        let left = r % 2 == 0;
        let x = if left { -110.0 } else { 110.0 };
        let id = bar(
            &mut lib,
            ramps,
            &format!("Ramp {}", r + 1),
            Vec2::new(x, 800.0 + r as f32 * 420.0),
            780.0,
            if left { 22.0 } else { -22.0 },
            0.0,
        );
        look_of.push(id);
    }
    // Spinners: two turning bars the marbles have to get past.
    let spinners = lib.add("folder", course, "Spinners");
    for (s, (at, spin)) in
        [(Vec2::new(-200.0, 2700.0), 120.0), (Vec2::new(200.0, 3200.0), -120.0)].into_iter().enumerate()
    {
        let id = bar(&mut lib, spinners, &format!("Spinner {}", s + 1), at, 380.0, 0.0, spin);
        look_of.push(id);
    }
    for id in look_of {
        lib.look_in(id, "shape.solid", hex(0x8fd3ff));
    }
    let finish = lib.add("shape", course, "Finish line");
    lib.at(finish, Vec2::new(0.0, LINE));
    lib.params(finish, |s: &mut ShapeSettings| {
        *s = ShapeSettings { passes: true, fill: ShapeFill::Filled, ..ShapeSettings::rectangle(WIDE, 40.0, 4.0) };
    });
    // Its edge flashes when the winner crosses (a zone is never hit, so only the rule flashes it).
    lib.look_in(finish, "shape.hit_ripple", hex(0x6bffb8));
    // The marbles.
    let marbles = lib.add("balls", ThingId::ROOT, "Marbles");
    lib.at(marbles, Vec2::new(0.0, -850.0));
    lib.params(marbles, |s: &mut BallsSettings| {
        s.radius = 22.0;
        s.group = true;
        s.shape = GroupShape::Line;
        s.count = 6;
        s.space = 40.0;
        s.velocity = Vec2::new(0.0, 80.0);
        s.launch = Launch::Random;
    });
    lib.card(marbles, SlotId::Motion, |m: &mut crate::BasicMotion| m.gravity_scale = 1.0);
    lib.card(marbles, SlotId::Bounce, |b: &mut BasicBounce| {
        b.bounciness = 0.55;
        b.hit_balls = true;
    });
    lib.look(marbles, "marble");
    let rainbow = lib.layer(marbles, SlotId::Look, GROUP_GRADIENT);
    lib.layer_params(marbles, SlotId::Look, rainbow, |g: &mut GroupGradient| {
        g.colours =
            Gradient::even(&[hex(0xff3b3b), hex(0xff9a1f), hex(0xffe14a), hex(0x3ddc63), hex(0x3d8bff), hex(0xb04dff)]);
        g.spread = Spread::Order;
    });
    lib.notes(marbles, &[60, 62, 64, 67, 69, 72, 74, 76], Plays::Random, Root::C, Scale::MajorPentatonic);
    lib.tone(marbles, Tone::Mallet);
    lib.card(marbles, SlotId::Sound, |v: &mut VoiceSettings| v.play_retrigger = 60.0);
    // The camera rides with the pack, kept inside the course.
    let camera = *lib.scene.things.iter().find(|(_, t)| t.kind == "camera").expect("a camera").0;
    let m = lib.who(marbles);
    lib.params(camera, |c: &mut CameraSettings| {
        c.follow = true;
        c.follow_target = bpe_core::Target::Thing(marbles);
        c.smoothness = 0.5;
        c.stay_inside = true;
        c.box_size = Vec2::new(1080.0, BOTTOM - TOP + 200.0);
        c.box_centre = Vec2::new(0.0, (TOP + BOTTOM) * 0.5);
    });
    let standings = lib.text(Look::RaceLeaderboard, "Standings", "{standings down 6 of Marbles}");
    // Smaller than the look's own, and under the title: its left edge about 40 px in from the screen's.
    lib.params(standings, |t: &mut TextSettings| t.size = 30.0);
    lib.at(standings, Vec2::new(-190.0, -470.0));
    let title = lib.text(Look::HookTitle, "Title", "WHO WINS?");
    lib.params(title, |t: &mut TextSettings| t.size = 110.0);
    lib.at(title, Vec2::new(0.0, -800.0));
    lib.rule_doing(
        FIRST,
        &[("when.enter", vec![("who", m), ("zone", lib.who(finish))]), ("if.only", vec![("n", int(1))])],
        &[
            ("do.text", vec![("who", lib.who(title)), ("words", text("WINNER!"))]),
            ("do.flash", vec![("who", lib.who(finish)), ("strength", n(1.5))]),
            ("do.camera", vec![("mode", choice("zoom_punch")), ("punch", n(0.12))]),
        ],
    );
    lib.reverb(1.8, 22.0);
    lib.finish()
}
