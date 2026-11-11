//! Club night: four glowing balls bounce on a dance floor in time, each one a part of a four-on-the-floor beat,
//! while a light rig chases on the beat, a second rig strobes on every bar and a mirror ball hangs in the haze.

use bpe_core::{Gradient, Registry, Scene, Settings, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, Tone, hex};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::modules::lighting::LightingSettings;
use crate::modules::look::surface::SurfaceKind;
use crate::paint::Paint;
use crate::things::light::LightSettings;
use crate::things::rig::{Across, Pattern, RigColours, RigSettings, StepsOn, add_rig};
use crate::things::shape::ShapeFill;
use crate::{BallsSettings, BasicBounce, BasicMotion, SceneSettings, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "club-night",
        name: "Club night",
        level: Level::InBetween,
        kinds: &[Kind::Music],
        about: "Four glowing balls bounce on a dance floor in time, each one part of a four-on-the-floor beat: \
                kick, clap, hats and bass. A light rig chases on the beat and strobes on every bar through the haze.",
        uses: &["Balls: timed bounces", "Rig: chase and strobe", "Lights", "Lighting: haze, mirror ball", "Sound"],
        change_first: "The rigs' Pattern and colours, the scene's Tempo (then the balls' heights), and the bass \
                       ball's melody.",
        sound: SoundLevel::Designed,
        seconds: 30.0,
        poster_seconds: 8.25,
        preview_seconds: 15.0,
        swatch: [0xff2fd0, 0x07050c],
        payoff: None,
    },
    build,
};

/// The beat: 120 a minute, the first one an eighth of a second in (so no ball starts on the floor).
const TEMPO: f32 = 120.0;
const FIRST_BEAT: f64 = 0.125;
const BEAT: f64 = 60.0 / TEMPO as f64;

/// The top of the dance floor.
const FLOOR_TOP: f32 = 700.0;

/// One dancer: name, x, radius, Gravity ×, beats between landings, beats to its first landing, colour, voice,
/// notes (a melody, one step a landing).
struct Dancer {
    name: &'static str,
    x: f32,
    radius: f32,
    gravity: f32,
    every: f64,
    first: f64,
    colour: u32,
    tone: Tone,
    notes: &'static [u8],
}

const DANCERS: [Dancer; 4] = [
    Dancer {
        name: "Kick",
        x: -330.0,
        radius: 64.0,
        gravity: 3.0,
        every: 1.0,
        first: 0.0,
        colour: 0xff2fd0,
        tone: Tone::Kick,
        notes: &[33],
    },
    Dancer {
        name: "Clap",
        x: -110.0,
        radius: 44.0,
        gravity: 1.5,
        every: 2.0,
        first: 1.0,
        colour: 0x2fe8ff,
        tone: Tone::Clap,
        notes: &[60],
    },
    Dancer {
        name: "Hats",
        x: 110.0,
        radius: 26.0,
        gravity: 2.0,
        every: 1.0,
        first: 0.5,
        colour: 0xffe14a,
        tone: Tone::Hat,
        notes: &[84],
    },
    Dancer {
        name: "Bass",
        x: 330.0,
        radius: 40.0,
        gravity: 4.0,
        every: 1.0,
        first: 0.5,
        colour: 0x9b5cff,
        tone: Tone::Bass,
        // A, F, C and G, a bar each, on the off-beats.
        notes: &[45, 45, 45, 45, 41, 41, 41, 41, 48, 48, 48, 48, 43, 43, 43, 43],
    },
];

/// Where a ball of `radius` falling at `g` px/s² must be at 0 s, and how fast it goes, so that it lands on the
/// floor every `every` seconds, first at `first` seconds (as long as nothing else touches it).
pub(crate) fn on_the_beat(floor_top: f32, radius: f32, g: f32, every: f64, first: f64) -> (f32, f32) {
    let up = g * every as f32 / 2.0;
    // How long since it last left the floor.
    let since = (every - first.rem_euclid(every)).rem_euclid(every) as f32;
    let height = up * since - g * since * since / 2.0;
    (floor_top - radius - height, -(up - g * since))
}

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 30.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x07050c));
        s.bloom = 0.5;
        s.tempo = TEMPO;
        s.first_beat = FIRST_BEAT as f32;
        // Twice the usual gravity, so the fast bouncers stay inside Gravity ×'s range.
        s.gravity *= 2.0;
    });
    let gravity = SceneSettings::from_params(&lib.scene.things[&ThingId::ROOT].params).gravity.y;
    let floor = lib.add("shape", ThingId::ROOT, "Dance floor");
    lib.at(floor, Vec2::new(0.0, FLOOR_TOP + 30.0));
    lib.params(floor, |s: &mut ShapeSettings| {
        *s = ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::rectangle(1000.0, 60.0, 4.0) }
    });
    lib.look_in(floor, "shape.chrome_edge", hex(0x3a2a50));

    let dancers = lib.add("folder", ThingId::ROOT, "Dancers");
    for d in &DANCERS {
        let g = gravity * d.gravity;
        let (y, vy) = on_the_beat(FLOOR_TOP, d.radius, g, d.every * BEAT, FIRST_BEAT + d.first * BEAT);
        let ball = lib.add("balls", dancers, d.name);
        lib.at(ball, Vec2::new(d.x, y));
        lib.params(ball, |s: &mut BallsSettings| {
            s.radius = d.radius;
            s.velocity = Vec2::new(0.0, vy);
        });
        lib.card(ball, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = d.gravity);
        lib.card(ball, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);
        lib.look_in(ball, "glow_orb", hex(d.colour));
        lib.melody(ball, &d.notes.iter().map(std::slice::from_ref).collect::<Vec<_>>());
        lib.tone(ball, d.tone);
    }

    let mirror_ball = lib.ball("Mirror ball", Vec2::new(0.0, -520.0), 70.0, Vec2::ZERO, "chrome");
    lib.card(mirror_ball, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 0.0);
    lib.surface(mirror_ball, SurfaceKind::MirrorBall, |_| {});

    let chase = add_rig(&mut lib.scene, reg, ThingId::ROOT).expect("a rig");
    lib.scene.rename_thing(chase, "Chase rig");
    lib.params(chase, |r: &mut RigSettings| {
        r.pattern = Pattern::Chase;
        r.steps_on = StepsOn::Beats;
        r.every_beats = 1.0;
        r.lit_at_once = 2.0;
        r.tail = 0.5;
        r.colours_from = RigColours::TheList;
        r.colours = Gradient::even(&[hex(0xff2fd0), hex(0x2fe8ff), hex(0x9b5cff), hex(0xffb02f)]);
        r.off_level = 0.0;
    });
    let strobe = add_rig(&mut lib.scene, reg, ThingId::ROOT).expect("a rig");
    lib.scene.rename_thing(strobe, "Strobe rig");
    lib.params(strobe, |r: &mut RigSettings| {
        r.pattern = Pattern::Strobe;
        r.steps_on = StepsOn::Bars;
        r.every_bars = 1.0;
        r.across = Across::Lights;
        r.flash_length = 0.06;
        r.tail = 0.0;
        r.off_level = 0.0;
    });
    let spots: Vec<(ThingId, ThingId)> = lib
        .scene
        .things
        .values()
        .filter(|t| t.parent == Some(chase) || t.parent == Some(strobe))
        .map(|t| (t.id, t.parent.expect("in a rig")))
        .collect();
    for (k, (spot, rig)) in spots.into_iter().enumerate() {
        let strobing = rig == strobe;
        lib.scene.rename_thing(spot, &format!("{} {}", if strobing { "Strobe" } else { "Spot" }, k % 4 + 1));
        lib.params(spot, |l: &mut LightSettings| {
            l.cone = if strobing { 30.0 } else { 16.0 };
            l.reach = 2400.0;
            l.strength = if strobing { 1.1 } else { 1.3 };
            l.beam = 0.7;
            l.direction = 90.0 + (k % 4) as f32 * 8.0 - 12.0;
            if strobing {
                l.position.y = -880.0;
            }
        });
    }
    lib.card(ThingId::ROOT, SlotId::Lighting, |l: &mut LightingSettings| {
        l.ambient = 0.05;
        l.haze = 0.3;
        l.background_lit = 0.25;
    });
    lib.reverb(1.6, 18.0);
    lib.finish()
}
