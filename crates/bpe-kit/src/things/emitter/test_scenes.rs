//! Emitter scenes for this crate's tests.

use bpe_core::{Resolved, Scene, Settings, ThingId, resolve};
use glam::Vec2;

use super::{Catches, Directions, EmitterSettings, KIND, StartFrom};
use crate::{Kit, add_thing, kit, new_scene};

/// The new scene without its ball, with an emitter at `at` set by `f`. Gives back the
/// emitter's id.
pub fn scene_with(at: Vec2, f: impl FnOnce(&mut EmitterSettings)) -> (Scene, ThingId, Kit) {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    scene.remove_thing(crate::testing::BALLS).unwrap();
    let id = add_emitter(&mut scene, &kit, at, f);
    (scene, id, kit)
}

/// Adds an emitter at `at`, set by `f`.
pub fn add_emitter(scene: &mut Scene, kit: &Kit, at: Vec2, f: impl FnOnce(&mut EmitterSettings)) -> ThingId {
    let id = add_thing(scene, &kit.registry, KIND, ThingId::ROOT).unwrap();
    let mut s = EmitterSettings::default();
    f(&mut s);
    let thing = scene.things.get_mut(&id).unwrap();
    thing.params = s.to_params();
    thing.transform.position = at;
    id
}

/// `scene` resolved, with no problems.
pub fn resolved(scene: &Scene, kit: &Kit) -> Resolved {
    let r = resolve(scene, &kit.registry);
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    r
}

/// An emitter at the origin firing `count` balls one way to the right at a catcher 200 px off,
/// set by `f`.
pub fn shooter_and_catcher(count: u32, f: impl FnOnce(&mut EmitterSettings)) -> (Scene, ThingId, ThingId, Kit) {
    let (mut scene, shooter, kit) = scene_with(Vec2::ZERO, |s| {
        s.count = count;
        s.directions = Directions::OneWay;
        s.aim_angle = 0.0;
        s.start_from = StartFrom::Line;
        s.start_length = 40.0;
        s.speed = 400.0;
    });
    let catcher = add_emitter(&mut scene, &kit, Vec2::new(200.0, 0.0), |s| {
        s.emits = false;
        s.catches = Catches::Remove;
        s.catch_size = 30.0;
        f(s);
    });
    (scene, shooter, catcher, kit)
}
