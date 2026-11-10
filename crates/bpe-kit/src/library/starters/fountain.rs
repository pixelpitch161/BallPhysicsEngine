//! Fountain: an emitter fires fans of balls up into the air; they bounce on the floor and fade away.

use bpe_core::{Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, hex};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};
use crate::things::emitter::{Directions, Earlier, FireWhen};
use crate::{BasicBounce, BasicMotion, EmitterSettings, ShapeFill, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "fountain",
        name: "Fountain",
        level: Level::Starter,
        kinds: &[Kind::BounceAndDrop],
        about: "An emitter at the bottom fires fans of glowing balls up into the air; they rain down, bounce on the \
                floor and fade away.",
        uses: &["Emitter", "Shape", "Sound: the emitter's own balls"],
        change_first: "How often it fires and how many, the fan's width and the balls' look.",
        sound: SoundLevel::Simple,
        seconds: 20.0,
        poster_seconds: 3.0,
        preview_seconds: 10.0,
        swatch: [0xffb03a, 0x0c0c14],
        payoff: None,
    },
    build,
};

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 20.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x0c0c14));
        s.bloom = 0.35;
    });
    let floor = lib.add("shape", ThingId::ROOT, "Floor");
    lib.at(floor, Vec2::new(0.0, 820.0));
    lib.params(floor, |s: &mut ShapeSettings| {
        *s = ShapeSettings { fill: ShapeFill::Filled, ..ShapeSettings::rectangle(1000.0, 40.0, 6.0) };
    });
    lib.look(floor, "shape.solid");
    let fountain = lib.add("emitter", ThingId::ROOT, "Fountain");
    lib.at(fountain, Vec2::new(0.0, 700.0));
    lib.params(fountain, |s: &mut EmitterSettings| {
        s.directions = Directions::Fan;
        s.aim_angle = -90.0;
        s.fan_angle = 50.0;
        s.speed = 950.0;
        s.vary_speed = 0.15;
        s.count = 7;
        s.fire_when = FireWhen::Every;
        s.every = 0.6;
        s.endless = true;
        s.earlier = Earlier::FadeOut;
        s.fade_time = 5.0;
    });
    lib.card(fountain, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 1.0);
    lib.card(fountain, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 0.55);
    lib.look(fountain, "comet");
    lib.notes(fountain, &[64, 67, 69, 71, 74, 76], Plays::Random, Root::E, Scale::MinorPentatonic);
    lib.finish()
}
