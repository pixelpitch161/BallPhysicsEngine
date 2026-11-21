//! Fitting a shape to the grid when it is made or its outline starts afresh (plan
//! `docs/superpowers/plans/2026-11-21-snap-new-shapes.md`): a new shape's middle lands on a grid
//! point, an even polygon's corners move to the nearest grid points where that keeps a proper
//! outline, and a circle's radius or an ellipse's half-width and half-height become whole squares.
//! Only while the grid snaps; the callers pass the grid square, or nothing.

use bpe_core::{Address, Scene, Settings, ShapePath, ThingId, Value};
use bpe_kit::{ShapeSettings, ShapeStart};
use glam::Vec2;

use crate::grid::{snap_length, snap_point};

/// Snaps new shape `id`'s middle to the grid, then fits its outline ([`fit`]). True when it
/// changed the scene. Nothing for another kind or a shape riding on something.
pub fn place_new(scene: &mut Scene, id: ThingId, step: f32) -> bool {
    let Some(thing) = scene.things.get_mut(&id).filter(|t| t.kind == bpe_kit::things::shape::KIND) else {
        return false;
    };
    if thing.rides() || !usable(step) {
        return false;
    }
    let at = snap_point(thing.transform.position, Some(step));
    let moved = at != thing.transform.position;
    thing.transform.position = at;
    fit(scene, id, step) || moved
}

/// Fits shape `id`'s outline to a grid of `step` world pixels where it stands: an even polygon's
/// corners to grid points (left even when that would collapse or fold the outline), a circle's
/// Size and an ellipse's Width and Height to whole squares. A keyframed number is left alone, and
/// so is a moulded outline, a shape riding on something and any other kind. True when it changed
/// the scene.
pub fn fit(scene: &mut Scene, id: ThingId, step: f32) -> bool {
    let Some(thing) = scene.things.get(&id).filter(|t| t.kind == bpe_kit::things::shape::KIND) else {
        return false;
    };
    if thing.rides() || !usable(step) {
        return false;
    }
    let s = ShapeSettings::from_params(&thing.params);
    let (centre, turn) = (thing.transform.position, thing.transform.rotation);
    let keyed = |key: &str| scene.keyed(&Address::own(id, key));
    let mut set: Vec<(&str, Value)> = Vec::new();
    match s.start {
        ShapeStart::Polygon if s.moulded().is_none() && !keyed("size") => {
            if let Some(path) = snapped_corners(&s, centre, turn, step) {
                set.push(("path", Value::Path(path)));
            }
        }
        ShapeStart::Polygon => {}
        ShapeStart::Circle => {
            if !keyed("size") {
                set.push(("size", Value::Number(f64::from(snap_length(s.size, Some(step))))));
            }
        }
        ShapeStart::Ellipse => {
            for (key, length) in [("width", s.width), ("height", s.height)] {
                if !keyed(key) {
                    set.push((key, Value::Number(f64::from(2.0 * snap_length(length * 0.5, Some(step))))));
                }
            }
        }
    }
    let thing = scene.things.get_mut(&id).expect("looked up above");
    let mut changed = false;
    for (key, value) in set {
        if thing.params.get(key) != Some(&value) {
            thing.params.set(key, value);
            changed = true;
        }
    }
    changed
}

fn usable(step: f32) -> bool {
    step.is_finite() && step > 0.0
}

/// The even polygon of `s` with each corner moved to its nearest grid point, as the world places
/// it (middle `centre`, turned `turn` radians clockwise, `s.size` world pixels per shape unit),
/// in shape units again. `None` when two corners would meet, or the outline would no longer turn
/// the same way at every corner (a corner flattened into a side, or the outline folded).
fn snapped_corners(s: &ShapeSettings, centre: Vec2, turn: f32, step: f32) -> Option<ShapePath> {
    let size = s.size;
    if !(size.is_finite() && size > 0.0) {
        return None;
    }
    let even = ShapePath::even(s.sides.clamp(3, bpe_runtime::MAX_SIDES as u32));
    let rot = Vec2::from_angle(turn);
    let back = Vec2::from_angle(-turn);
    let world: Vec<Vec2> =
        even.points.iter().map(|&p| snap_point(centre + rot.rotate(Vec2::from(p) * size), Some(step))).collect();
    let n = world.len();
    // The even polygon's corners all turn one way; the snapped ones must too, each strictly.
    let turns = |pts: &[Vec2], k: usize| {
        let (a, b, c) = (pts[k], pts[(k + 1) % n], pts[(k + 2) % n]);
        (b - a).perp_dot(c - b)
    };
    let original: Vec<Vec2> = even.points.iter().map(|&p| Vec2::from(p)).collect();
    let sign = turns(&original, 0).signum();
    if (0..n).any(|k| world[k] == world[(k + 1) % n] || turns(&world, k) * sign <= 0.0) {
        return None;
    }
    // Turning the same way at every corner and going round once (the corners stay near where
    // the even polygon's were, at most half a square's diagonal away) keeps it convex.
    let points: Vec<[f32; 2]> = world.iter().map(|&w| (back.rotate(w - centre) / size).to_array()).collect();
    let path = ShapePath { bends: vec![None; points.len()], points };
    (path.is_valid() && path != even).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::Scene;

    const STEP: f32 = 60.0;

    /// A scene with one shape, its params set by `edit`, at `at`.
    fn scene_with(at: Vec2, edit: impl FnOnce(&mut bpe_core::Params)) -> (Scene, ThingId) {
        let reg = bpe_kit::registry();
        let mut scene = Scene::empty(&reg);
        let id = bpe_kit::add_thing(&mut scene, &reg, bpe_kit::things::shape::KIND, ThingId::ROOT).unwrap();
        let thing = scene.things.get_mut(&id).unwrap();
        thing.transform.position = at;
        edit(&mut thing.params);
        (scene, id)
    }

    fn polygon(sides: i64, size: f64) -> impl FnOnce(&mut bpe_core::Params) {
        move |p| {
            p.set("start", Value::Choice("polygon".into()));
            p.set("sides", Value::Int(sides));
            p.set("size", Value::Number(size));
        }
    }

    fn settings(scene: &Scene, id: ThingId) -> ShapeSettings {
        ShapeSettings::from_params(&scene.things[&id].params)
    }

    fn on_grid(p: Vec2) -> bool {
        (p / STEP - (p / STEP).round()).abs().max_element() < 1e-3
    }

    /// Where shape `id`'s corners are in the world, as a world built from the scene has them.
    fn world_corners(scene: &Scene, id: ThingId) -> Vec<Vec2> {
        let kit = bpe_kit::kit();
        let resolved = bpe_core::resolve(scene, &kit.registry);
        let world = bpe_runtime::World::build(scene, &resolved, &kit.sims);
        let seen = crate::shape_handles::seen(&world, id).unwrap();
        seen.path.points.iter().map(|&p| seen.frame.to_world(Vec2::from(p))).collect()
    }

    #[test]
    fn a_hexagons_corners_land_on_grid_points() {
        let (mut scene, id) = scene_with(Vec2::new(0.0, 0.0), polygon(6, 400.0));
        assert!(fit(&mut scene, id, STEP));
        let s = settings(&scene, id);
        assert_eq!(s.moulded().map(|p| p.points.len()), Some(6));
        assert_eq!(s.size, 400.0, "the Size is kept; the corners carry the snapping");
        for c in world_corners(&scene, id) {
            assert!(on_grid(c), "{c}");
        }
        assert!(!fit(&mut scene, id, STEP), "a moulded outline is left as it is");
    }

    #[test]
    fn a_turned_triangle_off_the_grid_still_lands_on_it() {
        let (mut scene, id) = scene_with(Vec2::new(17.0, -23.0), polygon(3, 333.0));
        scene.things.get_mut(&id).unwrap().transform.rotation = 0.4;
        assert!(fit(&mut scene, id, STEP));
        assert_eq!(scene.things[&id].transform.position, Vec2::new(17.0, -23.0), "an existing shape isn't moved");
        for c in world_corners(&scene, id) {
            assert!(on_grid(c), "{c}");
        }
    }

    #[test]
    fn a_polygon_too_small_for_the_grid_stays_even() {
        let (mut scene, id) = scene_with(Vec2::ZERO, polygon(64, 100.0));
        assert!(!fit(&mut scene, id, STEP));
        assert_eq!(scene.things[&id].params.get("path"), None);
        // Four corners 30 px out all round to the middle.
        let (mut scene, id) = scene_with(Vec2::ZERO, polygon(4, 30.0));
        assert!(!fit(&mut scene, id, STEP));
    }

    #[test]
    fn a_circle_and_an_ellipse_round_to_whole_squares() {
        let (mut scene, id) = scene_with(Vec2::ZERO, |p| p.set("size", Value::Number(400.0)));
        assert!(fit(&mut scene, id, STEP));
        assert_eq!(settings(&scene, id).size, 420.0);
        let (mut scene, id) = scene_with(Vec2::ZERO, |p| {
            p.set("start", Value::Choice("ellipse".into()));
            p.set("width", Value::Number(900.0));
            p.set("height", Value::Number(1300.0));
        });
        assert!(fit(&mut scene, id, STEP));
        let s = settings(&scene, id);
        assert_eq!((s.width, s.height), (960.0, 1320.0));
        assert!(!fit(&mut scene, id, STEP), "already fitted");
    }

    #[test]
    fn a_new_shape_lands_in_a_grid_square_corner() {
        let (mut scene, id) = scene_with(Vec2::new(29.0, -95.0), polygon(4, 300.0));
        assert!(place_new(&mut scene, id, STEP));
        assert_eq!(scene.things[&id].transform.position, Vec2::new(0.0, -120.0));
        for c in world_corners(&scene, id) {
            assert!(on_grid(c), "{c}");
        }
    }

    #[test]
    fn keyed_sizes_riders_and_bad_steps_are_left_alone() {
        let reg = bpe_kit::registry();
        let (mut scene, id) = scene_with(Vec2::ZERO, polygon(6, 400.0));
        let size = Address::own(id, "size");
        scene.set_key(&reg, &size, 0.0, Value::Number(400.0)).unwrap();
        assert!(scene.keyed(&size));
        assert!(!fit(&mut scene, id, STEP));
        let (mut scene, id) = scene_with(Vec2::ZERO, polygon(6, 400.0));
        assert!(!fit(&mut scene, id, 0.0));
        assert!(!fit(&mut scene, id, f32::NAN));
        scene.things.get_mut(&id).unwrap().attach =
            bpe_core::Attach::Ride { offset: Vec2::ZERO, turn_with_parent: true, item: None };
        assert!(!fit(&mut scene, id, STEP));
        assert!(!place_new(&mut scene, id, STEP));
    }
}
