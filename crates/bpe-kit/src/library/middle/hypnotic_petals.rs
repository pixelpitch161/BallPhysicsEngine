//! Hypnotic petals: hundreds of balls fired out from the middle of a slowly turning flower, a rainbow
//! round them that slowly turns.

use bpe_core::{Gradient, Registry, Scene, ShapePath, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, Tone, hex, n};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::modules::look::layers::{GROUP_GRADIENT, GroupGradient, Spread};
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale, VoiceSettings};
use crate::things::emitter::Directions;
use crate::things::shape::ShapeStart;
use crate::{BasicBounce, BasicMotion, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "hypnotic-petals",
        name: "Hypnotic petals",
        level: Level::InBetween,
        kinds: &[Kind::Hypnotic],
        about: "Three hundred balls burst out of the middle of a slowly turning six-petal flower, a rainbow round \
                them that slowly turns.",
        uses: &["Emitter", "Group gradient: by launch angle, keyframed", "Shape: a moulded flower", "Sound"],
        change_first: "How many balls (the emitter's How many), the flower's Size and Spin, and the rainbow's \
                       colours.",
        sound: SoundLevel::Designed,
        seconds: 30.0,
        poster_seconds: 1.1,
        preview_seconds: 15.0,
        swatch: [0xff4fd8, 0x0a0614],
        payoff: None,
    },
    build,
};

/// How many petals, and how deep the dips between them are (as a share of the size).
const PETALS: f32 = 6.0;
const DIP: f32 = 0.42;

/// The flower's outline in shape units: a petal's tip 1 from the middle.
fn flower() -> ShapePath {
    const POINTS: usize = 144;
    let points: Vec<[f32; 2]> = (0..POINTS)
        .map(|k| {
            let a = k as f32 / POINTS as f32 * std::f32::consts::TAU;
            // Tips up, and round petals: |cos| keeps each petal fat at the end.
            let r = 1.0 - DIP * (1.0 - (PETALS * 0.5 * a).cos().abs().powf(0.6));
            [r * a.sin(), -r * a.cos()]
        })
        .collect();
    ShapePath { bends: vec![None; points.len()], points }
}

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 30.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x0a0614));
        s.bloom = 0.5;
    });
    let flower_at = Vec2::new(0.0, 0.0);
    let petals = lib.add("shape", ThingId::ROOT, "Flower");
    lib.at(petals, flower_at);
    lib.params(petals, |s: &mut ShapeSettings| {
        *s = ShapeSettings {
            start: ShapeStart::Polygon,
            sides: 6,
            size: 520.0,
            thickness: 8.0,
            path: flower(),
            spin: 6.0,
            ..ShapeSettings::default()
        };
    });
    lib.look(petals, "shape.aurora");
    lib.notes(petals, &[60, 62, 64, 67, 69, 72, 74, 76], Plays::Random, Root::C, Scale::MajorPentatonic);
    lib.tone(petals, Tone::Pad);
    lib.card(petals, SlotId::Sound, |v: &mut VoiceSettings| {
        // Three hundred balls hit it: a note at most every eighth of a second.
        v.play_retrigger = 125.0;
        v.out_volume = -6.0;
    });
    let emitter = lib.emitter("Balls", flower_at, |e| {
        e.radius = 6.0;
        e.count = 300;
        e.directions = Directions::AllRound;
        e.speed = 420.0;
    });
    lib.look(emitter, "glow_orb");
    lib.card(emitter, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 0.0);
    lib.card(emitter, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 1.0);
    let rainbow = lib.layer(emitter, SlotId::Look, GROUP_GRADIENT);
    lib.layer_params(emitter, SlotId::Look, rainbow, |g: &mut GroupGradient| {
        let mut colours =
            Gradient::even(&[hex(0xff4fd8), hex(0xffa040), hex(0xffe040), hex(0x50e070), hex(0x3de8ff), hex(0x9a6bff)]);
        colours.looped = true;
        g.colours = colours;
        g.spread = Spread::LaunchAngle;
    });
    // The rainbow turns once round over the video.
    lib.keys(emitter, Some(SlotId::Look), &format!("{}/angle", rainbow.0), &[(0.0, n(-180.0)), (30.0, n(180.0))]);
    lib.reverb(5.0, 40.0);
    lib.finish()
}
