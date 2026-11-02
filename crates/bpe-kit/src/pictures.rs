//! Picture splats: which picture files a scene uses, and the slot each gets.
//!
//! The simulation never reads an image. It only gives each picture file a slot in
//! [`World::pictures`], so a Picture mark can name its picture by that number. The renderer
//! decodes the files (Task 13 adds the file half of this module).

use std::collections::BTreeSet;

use bpe_core::SlotId;
use bpe_runtime::World;

use crate::modules::marks::{MarksBrushes, SplatShape};

/// Most pictures one scene can show.
pub const MAX_PICTURES: usize = 16;
/// A Picture mark's slot for a picture not in the table: drawn as the checker.
pub const NO_SLOT: u8 = 31;
/// A Picture mark's variant bit for Tinted.
pub const TINTED: u8 = 32;

/// Every distinct non-empty `splat_picture` of the world's brushes whose Splat is on with
/// Shape Picture, sorted (byte order of the strings as stored), the first MAX_PICTURES kept.
pub fn collect(world: &World) -> Vec<String> {
    let mut found = BTreeSet::new();
    for &thing in world.order() {
        let Some((_, state)) = world.module_state(thing, SlotId::Marks) else { continue };
        let Some(b) = state.as_any().downcast_ref::<MarksBrushes>() else { continue };
        if b.splat_on && b.splat_shape == SplatShape::Picture && !b.splat_picture.trim().is_empty() {
            found.insert(b.splat_picture.clone());
        }
    }
    found.into_iter().take(MAX_PICTURES).collect()
}

/// The slot of `path` in `world.pictures`, or NO_SLOT.
pub fn slot_of(world: &World, path: &str) -> u8 {
    world.pictures.iter().position(|p| p == path).map_or(NO_SLOT, |i| i as u8)
}

/// Fills `world.pictures` (idempotent: the same world always gives the same table).
pub fn fill(world: &mut World) {
    world.pictures = collect(world);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{ARENA, BALLS, module_of};
    use crate::{add_thing, fill_slot, kit, new_scene, scenes};
    use bpe_core::{Params, Scene, ThingId, Value, resolve};
    use bpe_runtime::{Log, MarkShape, MarkTarget};
    use glam::Vec2;

    const CANVAS: ThingId = ThingId(3);

    fn brush(scene: &mut Scene, thing: ThingId, f: impl FnOnce(&mut Params)) {
        let kit = kit();
        fill_slot(scene, &kit.registry, thing, SlotId::Marks).unwrap();
        let m = module_of(scene, &kit.registry, thing, SlotId::Marks);
        let p = &mut scene.library.modules.get_mut(&m).unwrap().params;
        p.set("paint_on", Value::Bool(false));
        p.set("splat_on", Value::Bool(true));
        p.set("splat_shape", Value::Choice("picture".into()));
        f(p);
    }

    fn world_of(scene: &Scene, seconds: f64) -> (World, Log) {
        let kit = kit();
        let resolved = resolve(scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        crate::testing::run(scene, &resolved, &kit, seconds)
    }

    /// The new scene with a canvas (thing 3) and `f` to set things up.
    fn with_canvas(f: impl FnOnce(&mut Scene)) -> Scene {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        assert_eq!(add_thing(&mut scene, &kit.registry, "canvas", ThingId::ROOT).unwrap(), CANVAS);
        f(&mut scene);
        scene
    }

    #[test]
    fn pictures_are_collected_sorted_and_capped() {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let mut ids = Vec::new();
        for _ in 0..17 {
            ids.push(add_thing(&mut scene, &kit.registry, "obstacle", ThingId::ROOT).unwrap());
        }
        // Out of order on purpose.
        for (k, &id) in ids.iter().enumerate().rev() {
            brush(&mut scene, id, |p| p.set("splat_picture", Value::Text(format!("p{k:02}.png"))));
        }
        brush(&mut scene, BALLS, |p| p.set("splat_picture", Value::Text("p17.png".into())));
        let (world, _) = world_of(&scene, 0.0);
        let want: Vec<String> = (0..16).map(|k| format!("p{k:02}.png")).collect();
        assert_eq!(collect(&world), want);
        assert_eq!(world.pictures, want);
    }

    #[test]
    fn a_scene_without_pictures_keeps_an_empty_table() {
        let kit = kit();
        let (_, scene) = scenes::marks(&kit.registry).into_iter().next().unwrap();
        let (world, _) = world_of(&scene, 0.0);
        assert!(world.pictures.is_empty());
    }

    #[test]
    fn picture_marks_carry_their_slot_and_tint() {
        let scene = with_canvas(|scene| {
            brush(scene, BALLS, |p| {
                p.set("splat_picture", Value::Text("b.png".into()));
                p.set("splat_picture_colours", Value::Choice("tinted".into()));
                p.set("splat_onto", Value::Choice("canvases".into()));
            });
            // Its own picture sorts first, so the ball's is slot 1. It lands on its own skin.
            brush(scene, ARENA, |p| p.set("splat_picture", Value::Text("a.png".into())));
        });
        let (world, log) = world_of(&scene, 3.0);
        assert_eq!(world.pictures, ["a.png", "b.png"]);
        let pictures: Vec<_> = log
            .marks()
            .iter()
            .filter(|m| m.shape == MarkShape::Picture && m.target == MarkTarget::Canvas(CANVAS))
            .collect();
        assert!(!pictures.is_empty());
        for m in pictures {
            assert_eq!(m.variant, 1 | TINTED);
        }
    }

    #[test]
    fn an_empty_picture_splats_as_a_splat() {
        let scene = with_canvas(|scene| {
            brush(scene, BALLS, |p| p.set("splat_onto", Value::Choice("canvases".into())));
        });
        let (world, log) = world_of(&scene, 3.0);
        assert!(world.pictures.is_empty());
        let marks: Vec<_> = log.marks().iter().filter(|m| m.target == MarkTarget::Canvas(CANVAS)).collect();
        assert!(!marks.is_empty());
        assert!(marks.iter().all(|m| m.shape == MarkShape::Splat));
    }

    #[test]
    fn a_picture_facing_a_floor_hit_stands_upright() {
        let scene = with_canvas(|scene| {
            // A box arena and a ball dropped straight down onto its flat floor.
            scene.things.get_mut(&ARENA).unwrap().params.set("shape", Value::Choice("box".into()));
            let ball = scene.things.get_mut(&BALLS).unwrap();
            ball.transform.position = Vec2::new(0.0, 0.0);
            ball.params.set("velocity", Value::Vec2([0.0, 0.0]));
            brush(scene, BALLS, |p| {
                p.set("splat_picture", Value::Text("x.png".into()));
                p.set("splat_turn", Value::Choice("face_hit".into()));
                p.set("splat_onto", Value::Choice("canvases".into()));
            });
        });
        let (_, log) = world_of(&scene, 4.0);
        let first = log.marks().iter().find(|m| m.shape == MarkShape::Picture).expect("a picture mark");
        let wrapped = (first.angle + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
        assert!(wrapped.abs() < 0.05, "angle {wrapped}");
    }

    #[test]
    fn a_picture_past_the_table_gets_no_slot() {
        let scene = with_canvas(|scene| {
            brush(scene, BALLS, |p| p.set("splat_picture", Value::Text("x.png".into())));
        });
        let (world, _) = world_of(&scene, 0.0);
        assert_eq!(slot_of(&world, "x.png"), 0);
        assert_eq!(slot_of(&world, "zzz.png"), NO_SLOT);
    }

    #[test]
    fn picture_splats_paint_a_smaller_disc_on_the_grid() {
        use crate::things::canvas::sim::CanvasState;
        let scene = with_canvas(|scene| {
            brush(scene, BALLS, |p| {
                p.set("splat_picture", Value::Text("x.png".into()));
                p.set("splat_onto", Value::Choice("canvases".into()));
            });
        });
        let (world, _) = world_of(&scene, 3.0);
        let st = bpe_runtime::state::<CanvasState>(world.states[&CANVAS].as_ref());
        assert!(st.grid.covered() > 0.0);
    }
}
