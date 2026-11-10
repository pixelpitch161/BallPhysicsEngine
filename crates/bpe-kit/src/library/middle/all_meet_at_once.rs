//! All meet at once: balls fired every way from one focus of an ellipse all reach the other focus
//! at the same moment, and play a chord there, then fly back and meet again.

use bpe_core::{Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, Tone, choice, hex, n};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::modules::look::layers::trail::TRAIL;
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale, VoiceSettings};
use crate::things::emitter::Directions;
use crate::things::text::looks::Look;
use crate::{BasicBounce, BasicMotion, ShapeSettings, WallTouch};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "all-meet-at-once",
        name: "All meet at once",
        level: Level::InBetween,
        kinds: &[Kind::Hypnotic],
        about: "Balls fired every way from one focus of an ellipse all reach the other focus at the same moment and \
                play a chord there, then fly back and meet again.",
        uses: &["Emitter: sits on a focus", "Trail", "Shape: an ellipse and two zones", "Rules: enters, note", "Sound"],
        change_first: "How many balls (the emitter's How many), their speed and the chords the two foci play.",
        sound: SoundLevel::Designed,
        seconds: 25.0,
        poster_seconds: 2.5,
        preview_seconds: 12.0,
        swatch: [0xa98bff, 0x08060f],
        payoff: None,
    },
    build,
};

/// The ellipse's middle and size.
const AT: Vec2 = Vec2::new(0.0, 80.0);
const WIDE: f32 = 900.0;
const TALL: f32 = 1500.0;

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 25.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x08060f));
        s.bloom = 0.45;
    });
    let ellipse = lib.add("shape", ThingId::ROOT, "Ellipse");
    lib.at(ellipse, AT);
    lib.params(ellipse, |s: &mut ShapeSettings| *s = ShapeSettings::ellipse(WIDE, TALL, 6.0));
    lib.look(ellipse, "shape.glow_ring");
    lib.notes(ellipse, &[69, 72, 74, 76, 79, 81, 84], Plays::Random, Root::A, Scale::MinorPentatonic);
    lib.tone(ellipse, Tone::Bell);
    lib.card(ellipse, SlotId::Sound, |v: &mut VoiceSettings| v.out_volume = -16.0);
    // A tiny zone at each focus plays a chord when the balls come together there (a ball passing near a
    // focus on its own misses it).
    let focus = ((TALL * 0.5).powi(2) - (WIDE * 0.5).powi(2)).sqrt();
    let folder = lib.add("folder", ThingId::ROOT, "Foci");
    let mut zones = Vec::new();
    for (name, y, chord) in [("Top focus", -focus, [57u8, 64, 67, 72]), ("Bottom focus", focus, [53, 60, 64, 69])] {
        let zone = lib.add("shape", folder, name);
        lib.at(zone, AT + Vec2::new(0.0, y));
        lib.params(zone, |s: &mut ShapeSettings| {
            *s = ShapeSettings { passes: true, ..ShapeSettings::circle(4.0, 3.0) }
        });
        lib.look(zone, "shape.laser");
        lib.notes(zone, &chord, Plays::AllAtOnce, Root::A, Scale::Minor);
        lib.tone(zone, Tone::Pad);
        zones.push(zone);
    }
    let emitter = lib.emitter("Balls", AT, |e| {
        e.sits_on = ellipse.0 as i32;
        e.radius = 7.0;
        e.count = 72;
        e.directions = Directions::AllRound;
        e.speed = 560.0;
        // No ball straight along the long axis: that one would pass through each focus on its own too.
        e.aim_angle = -87.5;
    });
    lib.look(emitter, "glow_orb");
    lib.card(emitter, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 0.0);
    lib.card(emitter, SlotId::Bounce, |b: &mut BasicBounce| {
        b.bounciness = 1.0;
        b.touch = WallTouch::Middle;
    });
    lib.layer(emitter, SlotId::Look, TRAIL);
    let e = lib.who(emitter);
    for zone in zones {
        let name = lib.scene.things[&zone].name.clone();
        let z = lib.who(zone);
        lib.rule(
            &format!("{name} plays a chord when they meet"),
            &[
                ("when.enter", vec![("who", e.clone()), ("zone", z.clone())]),
                ("if.cooldown", vec![("seconds", n(1.0))]),
                ("do.note", vec![("who", z), ("pitch_from", choice("the_shapes_next_note")), ("velocity", n(0.9))]),
            ],
        );
    }
    let title = lib.text(Look::HookTitle, "Title", "THEY ALL\nMEET AT ONCE");
    lib.at(title, Vec2::new(0.0, -800.0));
    lib.params(title, |t: &mut crate::things::text::TextSettings| t.size = 90.0);
    lib.reverb(4.0, 35.0);
    lib.finish()
}
