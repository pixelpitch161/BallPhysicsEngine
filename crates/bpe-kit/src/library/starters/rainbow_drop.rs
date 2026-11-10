//! Rainbow drop: a grid of balls wearing one rainbow across the group pours into a round bowl.

use bpe_core::{Gradient, Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, hex};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::modules::look::layers::{GROUP_GRADIENT, GroupGradient, Spread};
use crate::paint::Paint;
use crate::sound::{Plays, Root, Scale};
use crate::{BallsSettings, BasicBounce, BasicMotion, GroupShape, Release};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "rainbow-drop",
        name: "Rainbow drop",
        level: Level::Starter,
        kinds: &[Kind::BounceAndDrop],
        about: "A block of balls wearing one rainbow across the whole group drops into a round bowl and piles up.",
        uses: &["Balls: a group", "Group gradient", "Shape", "Sound"],
        change_first: "The group's colours, how many balls there are, and how they are let go.",
        sound: SoundLevel::Simple,
        seconds: 15.0,
        poster_seconds: 4.0,
        preview_seconds: 10.0,
        swatch: [0xff4040, 0x4fc3f0],
        payoff: None,
    },
    build,
};

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 15.0);
    lib.scene_settings(|s| s.background = Paint::solid(hex(0x15151c)));
    let bowl = lib.ring("Bowl", Vec2::new(0.0, 280.0), 470.0, 10.0, "shape.chalk");
    lib.notes(bowl, &[60, 62, 64, 67, 69, 72, 74, 76], Plays::Random, Root::C, Scale::MajorPentatonic);
    let balls = lib.add("balls", ThingId::ROOT, "Rainbow balls");
    lib.at(balls, Vec2::new(0.0, 60.0));
    lib.params(balls, |s: &mut BallsSettings| {
        s.radius = 16.0;
        s.group = true;
        s.shape = GroupShape::Grid;
        s.count = 100;
        s.columns = 10;
        s.space = 6.0;
        s.release = Release::OneByOne;
        s.release_gap = 0.04;
    });
    lib.card(balls, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 1.0);
    lib.card(balls, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 0.6);
    lib.look(balls, "candy");
    let rainbow = lib.layer(balls, SlotId::Look, GROUP_GRADIENT);
    lib.layer_params(balls, SlotId::Look, rainbow, |g: &mut GroupGradient| {
        g.colours =
            Gradient::even(&[hex(0xff4040), hex(0xffa040), hex(0xffe040), hex(0x50e070), hex(0x4fc3f0), hex(0x9a6bff)]);
        g.spread = Spread::Order;
    });
    lib.finish()
}
