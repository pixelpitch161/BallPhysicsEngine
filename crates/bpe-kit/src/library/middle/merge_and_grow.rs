//! Merge and grow: every pop makes a bigger pop. Two balls of the same size that meet become one
//! bigger ball, and bigger balls sound lower.

use bpe_core::{Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, Tone, hex, n};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale, VoiceSettings};
use crate::things::balls::{GroupShape, Launch};
use crate::things::text::TextSettings;
use crate::things::text::looks::Look;
use crate::{BallsSettings, BasicBounce, BasicMotion};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "merge-and-grow",
        name: "Merge and grow",
        level: Level::InBetween,
        kinds: &[Kind::BounceAndDrop],
        about: "Every pop makes a bigger pop: two balls of about the same size that meet become one bigger ball, \
                and the bigger the ball, the lower it sounds.",
        uses: &["Balls", "Shape", "Rules: hit, merge balls", "Sound: pitch follows size", "Text: live words"],
        change_first: "How many balls start (the group's How many), the Size match on the merge rule and how much \
                       lower big balls sound (the sound's Pitch follows size).",
        sound: SoundLevel::Designed,
        seconds: 30.0,
        poster_seconds: 9.0,
        preview_seconds: 15.0,
        swatch: [0x4fd18b, 0x0e1412],
        payoff: None,
    },
    build,
};

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 30.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x0e1412));
        s.bloom = 0.3;
    });
    lib.ring("Ring", Vec2::new(0.0, 140.0), 470.0, 12.0, "shape.glow_ring");
    let balls = lib.add("balls", ThingId::ROOT, "Balls");
    lib.at(balls, Vec2::new(0.0, 140.0));
    lib.params(balls, |s: &mut BallsSettings| {
        s.radius = 14.0;
        s.group = true;
        s.shape = GroupShape::FilledCircle;
        s.count = 64;
        s.space = 44.0;
        s.velocity = Vec2::new(0.0, -420.0);
        s.launch = Launch::Random;
    });
    lib.look(balls, "pearl");
    lib.card(balls, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 0.0);
    lib.card(balls, SlotId::Bounce, |b: &mut BasicBounce| {
        b.bounciness = 1.0;
        b.hit_balls = true;
        // A merge keeps the pair's momentum but not their speed: each hit gives a little back.
        b.speed_gain = 1.08;
        b.no_speed_limit = false;
        b.top_speed = 650.0;
    });
    lib.notes(balls, &[72, 76, 79, 83, 84], Plays::Random, Root::C, Scale::Major);
    lib.tone(balls, Tone::Bell);
    lib.card(balls, SlotId::Sound, |v: &mut VoiceSettings| {
        // An octave lower each time a ball's size doubles.
        v.play_size_pitch = 12.0;
        v.play_size_at = 14.0;
        v.play_retrigger = 50.0;
    });
    // Balls within 35% of each other's size merge, so the sizes keep climbing to a few big balls.
    let b = lib.who(balls);
    lib.rule(
        "Same-size balls merge when they meet",
        &[("when.hit", vec![("who", b.clone()), ("whom", b)]), ("do.merge", vec![("size_match", n(35.0))])],
    );
    let count = lib.text(Look::HitCounter, "Ball count", "{balls left of Balls} BALLS");
    lib.params(count, |t: &mut TextSettings| t.size = 120.0);
    lib.at(count, Vec2::new(0.0, -620.0));
    lib.reverb(2.5, 25.0);
    lib.finish()
}
