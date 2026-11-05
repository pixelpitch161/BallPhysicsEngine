//! The emitter test scenes: rounds that grow and fade, and two emitters that set each other off.

use bpe_core::{Color, Gradient, Registry, Scene, Settings, SlotId, ThingId, Value, layered_key};
use glam::Vec2;

use crate::modules::look::layers::trail::TRAIL;
use crate::modules::look::layers::wavefront::WAVEFRONT;
use crate::modules::look::layers::{GROUP_GRADIENT, PARTICLES};
use crate::things::emitter::{Catches, Directions, Earlier, EmitterSettings, FireWhen, KIND};
use crate::{ShapeSettings, new_scene};

const BALLS: ThingId = ThingId(2);

/// Both emitter scenes.
pub fn emitter_scenes(reg: &Registry) -> Vec<(String, Scene)> {
    vec![("Emitter rounds".into(), emitter_rounds(reg)), ("Emitter chain".into(), emitter_chain(reg))]
}

/// The new scene without its ball, named `name`.
fn empty(reg: &Registry, name: &str) -> Scene {
    let mut scene = new_scene(reg);
    scene.name = name.into();
    scene.remove_thing(BALLS).expect("the new scene's ball");
    scene.things.get_mut(&ThingId(1)).expect("the shape").params = ShapeSettings::circle(508.0, 3.0).to_params();
    scene
}

/// An emitter named `name` at `at` with settings `s`.
fn emitter(scene: &mut Scene, reg: &Registry, name: &str, at: Vec2, s: EmitterSettings) -> ThingId {
    let id = crate::add_thing(scene, reg, KIND, ThingId::ROOT).expect("kit kinds are registered");
    scene.rename_thing(id, name);
    let thing = scene.things.get_mut(&id).expect("just added");
    thing.transform.position = at;
    thing.params = s.to_params();
    id
}

/// Adds a `kind` layer to `id`'s Look with `values`.
fn layer(scene: &mut Scene, reg: &Registry, id: ThingId, kind: &str, values: &[(&str, Value)]) {
    let layer = scene.add_layer(reg, id, SlotId::Look, kind).expect("a ball look layer");
    let card = scene.card_mut(id, SlotId::Look).expect("a look");
    for (key, v) in values {
        card.params.set(&layered_key(layer, key), v.clone());
    }
}

/// An emitter in the middle of a circle: three rounds of ten (then twenty, then forty) a second
/// apart, the earlier ones fading, a wavefront through each round, coloured round the wheel by the
/// way each ball was fired.
fn emitter_rounds(reg: &Registry) -> Scene {
    let mut scene = empty(reg, "Emitter rounds");
    let s = EmitterSettings {
        fire_when: FireWhen::Every,
        every: 1.0,
        rounds: 3,
        count: 10,
        grow_times: 2.0,
        earlier: Earlier::FadeOut,
        fade_time: 0.8,
        speed: 500.0,
        radius: 5.0,
        ..EmitterSettings::default()
    };
    let id = emitter(&mut scene, reg, "Rounds", Vec2::ZERO, s);
    layer(&mut scene, reg, id, WAVEFRONT, &[("width", Value::Number(3.0))]);
    layer(
        &mut scene,
        reg,
        id,
        GROUP_GRADIENT,
        &[
            ("spread", Value::Choice("launch_angle".into())),
            (
                "colours",
                Value::Gradient(Gradient::even(&[
                    Color::rgb(0xff, 0x3e, 0x3e), // scene colour
                    Color::rgb(0x3e, 0xff, 0x8a), // scene colour
                    Color::rgb(0x3e, 0x6a, 0xff), // scene colour
                    Color::rgb(0xff, 0x3e, 0x3e), // scene colour
                ])),
            ),
        ],
    );
    scene
}

/// Two emitters in a circle that set each other off: each fires a round when a wave lands on it,
/// letting the balls fly through, without end but never more than 400 balls each.
fn emitter_chain(reg: &Registry) -> Scene {
    let mut scene = empty(reg, "Emitter chain");
    let base = EmitterSettings {
        fire_when: FireWhen::WhenAWaveLands,
        endless: true,
        count: 8,
        catches: Catches::PassThrough,
        catch_size: 40.0,
        most_balls: 400,
        speed: 700.0,
        radius: 4.0,
        ..EmitterSettings::default()
    };
    let left = emitter(&mut scene, reg, "Left", Vec2::new(-200.0, 0.0), base.clone());
    let right = EmitterSettings { wave_first: false, directions: Directions::Fan, aim_angle: 180.0, ..base };
    let right = emitter(&mut scene, reg, "Right", Vec2::new(200.0, 0.0), right);
    for id in [left, right] {
        layer(&mut scene, reg, id, TRAIL, &[("kind", Value::Choice("line".into())), ("seconds", Value::Number(0.6))]);
    }
    layer(&mut scene, reg, right, PARTICLES, &[]);
    scene
}
