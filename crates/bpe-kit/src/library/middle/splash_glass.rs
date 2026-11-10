//! Splash glass: balls drop into a tall glass of water and splash it about, while a lamp's light bends through
//! the water and the glass.

use bpe_core::{Color, Gradient, LayerId, Registry, Scene, SlotId, ThingId};
use glam::Vec2;

use crate::library::build::{Lib, Tone, hex};
use crate::library::{Entry, Kind, Level, SceneInfo, SoundLevel};
use crate::modules::lighting::LightingSettings;
use crate::modules::look::surface::SurfaceKind;
use crate::paint::{Follow, Paint};
use crate::sound::{Plays, Root, Scale};
use crate::things::emitter::{Directions, Earlier, EmitterSettings, FireOrder, FireWhen, Marker};
use crate::things::light::LightType;
use crate::things::particles::liquid::{LIQUID, LiquidLayer};
use crate::things::particles::looks::Look;
use crate::things::particles::meets::{AtBalls, AtShapes, MeetsSettings};
use crate::things::particles::more_forces::{SPRING, SpringForce};
use crate::things::particles::{BornFrom, Emit, GridBy, GridCovers, ParticlesSettings};
use crate::{BasicBounce, BasicMotion, ShapeSettings};

pub(crate) const ENTRY: Entry = Entry {
    info: SceneInfo {
        id: "splash-glass",
        name: "Splash glass",
        level: Level::InBetween,
        kinds: &[Kind::LiquidSmokeSparks],
        about: "Balls drop one by one into a tall glass of water and splash it about, ringing the glass as they \
                land. A lamp shines through the side, and the water and the glass bend its light.",
        uses: &["Particles: liquid", "Emitter", "Shape: glass", "Light", "Lighting: bent light", "Sound"],
        change_first: "How often balls drop and how big they are, the water's colour and how much it springs back \
                       (its Spring home force).",
        sound: SoundLevel::Designed,
        seconds: 26.0,
        poster_seconds: 6.3,
        preview_seconds: 14.0,
        swatch: [0x3aa0ff, 0x101820],
        payoff: None,
    },
    build,
};

/// The glass: its middle, inside width and height, and wall.
const GLASS_AT: Vec2 = Vec2::new(0.0, 260.0);
const GLASS: Vec2 = Vec2::new(560.0, 860.0);
const WALL: f32 = 14.0;
/// How deep the water is.
const DEPTH: f32 = 400.0;

fn build(reg: &Registry) -> Scene {
    let mut lib = Lib::new(reg, 26.0);
    lib.scene_settings(|s| {
        s.background = Paint::solid(hex(0x101820));
        s.bloom = 0.3;
    });
    let glass = lib.add("shape", ThingId::ROOT, "Glass");
    lib.at(glass, GLASS_AT);
    // Open at the top: the top side is the first side round, a share of the way round its width.
    let top = GLASS.x / (2.0 * (GLASS.x + GLASS.y));
    lib.params(glass, |s: &mut ShapeSettings| {
        *s = ShapeSettings {
            gaps: 1,
            gap_size: top * 0.98,
            gap_at: top / 2.0,
            ..ShapeSettings::rectangle(GLASS.x, GLASS.y, WALL)
        }
    });
    lib.look_in(glass, "shape.glass", hex(0xcfe8ff));
    lib.notes(glass, &[76, 79, 81, 84, 88], Plays::Random, Root::A, Scale::MinorPentatonic);
    lib.tone(glass, Tone::Bell);

    let water = lib.particles(Look::Empty, ThingId::ROOT, "Water");
    lib.at(water, GLASS_AT + Vec2::new(0.0, GLASS.y / 2.0 - WALL - DEPTH / 2.0));
    lib.params(water, |p: &mut ParticlesSettings| {
        *p = ParticlesSettings {
            from: BornFrom::AGrid,
            grid_covers: GridCovers::ARectangle,
            grid_by: GridBy::Spacing,
            width: GLASS.x - 2.0 * WALL - 24.0,
            height: DEPTH - 12.0,
            spacing: 13.0,
            emit: Emit::KeepACrowd,
            life: 1.0e6,
            life_spread: 0.0,
            speed: 0.0,
            speed_spread: 0.0,
            size: 6.0,
            size_spread: 0.0,
            ..p.clone()
        }
    });
    let spring = lib.layer(water, SlotId::Motion, SPRING);
    lib.layer_params(water, SlotId::Motion, spring, |f: &mut SpringForce| {
        f.stiffness = 5.0;
        f.damping = 1.1;
    });
    lib.card(water, SlotId::Bounce, |m: &mut MeetsSettings| {
        m.push = 0.6;
        m.reach = 1.8;
        m.at_balls = AtBalls::BounceOff;
        m.ball_bounce = 0.7;
        m.at_shapes = AtShapes::Bounce;
        m.bounciness = 0.3;
    });
    let dots: Vec<LayerId> = lib.scene.card(water, SlotId::Look).expect("a Look").layers.iter().map(|l| l.id).collect();
    let liquid = lib.layer(water, SlotId::Look, LIQUID);
    for l in dots {
        lib.scene.remove_layer(reg, water, SlotId::Look, l).expect("the dots go");
    }
    lib.layer_params(water, SlotId::Look, liquid, |l: &mut LiquidLayer| {
        l.blob = 4.0;
        l.surface = SurfaceKind::Glass;
        l.roundness = 30.0;
        l.colour = Paint {
            colours: Gradient::even(&[Color::rgb(60, 160, 255), Color::rgb(20, 90, 220)]),
            follow: Follow::Speed,
            ..Paint::default()
        };
    });

    let drops = lib.emitter("Balls", Vec2::new(0.0, -1000.0), |s: &mut EmitterSettings| {
        s.radius = 34.0;
        s.directions = Directions::Fan;
        s.aim_angle = 90.0;
        s.fan_angle = 20.0;
        s.speed = 200.0;
        // Three balls a round, one at a time, each down its own line of the fan.
        s.count = 3;
        s.gap = 2.2;
        s.order = FireOrder::Random;
        s.fire_when = FireWhen::Every;
        s.every = 6.6;
        s.endless = true;
        s.earlier = Earlier::FadeOut;
        s.fade_time = 6.0;
        s.marker = Marker::None;
    });
    lib.card(drops, SlotId::Motion, |m: &mut BasicMotion| m.gravity_scale = 1.6);
    lib.card(drops, SlotId::Bounce, |b: &mut BasicBounce| b.bounciness = 0.45);
    lib.look(drops, "glass");

    lib.light("Lamp", LightType::Spot, Vec2::new(-480.0, -420.0), |l| {
        l.direction = 50.0;
        l.cone = 34.0;
        l.size = 12.0;
        l.strength = 2.4;
        l.reach = 3000.0;
        l.colour = hex(0xfff4e0);
    });
    lib.card(ThingId::ROOT, SlotId::Lighting, |l: &mut LightingSettings| {
        l.ambient = 0.18;
        l.glass_caustics = 1.5;
    });
    lib.reverb(2.6, 30.0);
    lib.finish()
}
