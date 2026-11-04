//! Drawing a shape: plain white for now (its paint layers come later), filled or as a band.

use bpe_core::SlotId;
use bpe_gfx::{DrawCx, Frame, ThingDraw, thing_code};
use bpe_runtime::{Outline, Wall};
use glam::Vec2;

use super::KIND;

/// A shape's body colour in linear light.
pub const BODY_WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

pub struct ShapeDraw;

impl ThingDraw for ShapeDraw {
    fn kind(&self) -> &'static str {
        KIND
    }

    /// The shape where its wall is this frame. A shape with no Look card draws nothing.
    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        if !cx.resolved.slots.contains_key(&SlotId::Look) {
            return;
        }
        let Some(wall) = cx.world.walls.get(&cx.thing.id) else { return };
        push_wall(wall, BODY_WHITE, thing_code(cx.thing.id), frame);
    }
}

/// Adds `wall` to `frame` as one shape. A round ring is a circle; a ring polygon and a box are
/// their corners, already turned (angle 0, scaled by the ring's radius or the box's half
/// diagonal); a free outline keeps its own points, angle and scale.
pub fn push_wall(wall: &Wall, color: [f32; 4], code: f32, frame: &mut Frame) {
    let half = if wall.solid { 0.0 } else { wall.outline.thickness() * 0.5 };
    match &wall.outline {
        &Outline::Ring { center, radius, sides, .. } if sides < 3 => {
            frame.push_path(center, 0.0, radius, half, None, color, code);
        }
        Outline::Path { center, angle, scale, points, .. } => {
            frame.push_path(*center, *angle, *scale, half, Some(points), color, code);
        }
        outline => {
            let center = outline.center();
            let scale = match *outline {
                Outline::Box { half, .. } => half.length(),
                Outline::Ring { radius, .. } => radius,
                Outline::Path { scale, .. } => scale,
            }
            .max(1e-3);
            let corners: Vec<Vec2> = outline.corners().into_iter().map(|c| (c - center) / scale).collect();
            frame.push_path(center, 0.0, scale, half, Some(&corners), color, code);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{SHAPE, scene_with_balls};
    use crate::{ShapeFill, ShapeSettings};
    use bpe_core::{Scene, Settings, ShapePath};
    use bpe_gfx::build_frame;
    use bpe_runtime::{Log, World};

    fn frame_of(scene: &Scene) -> Frame {
        let kit = crate::kit();
        let resolved = bpe_core::resolve(scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let world = World::build(scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        frame
    }

    fn with(settings: ShapeSettings) -> Scene {
        let (mut scene, _, _) = scene_with_balls("single", |_| {});
        scene.things.get_mut(&SHAPE).unwrap().params = settings.to_params();
        scene
    }

    #[test]
    fn a_hollow_polygon_draws_one_shape_through_its_corners() {
        let mut scene = with(ShapeSettings::polygon(6, 450.0, 8.0));
        scene.things.get_mut(&SHAPE).unwrap().transform.position = Vec2::new(30.0, 40.0);
        let frame = frame_of(&scene);
        assert_eq!(frame.paths.len(), 1);
        let p = frame.paths[0];
        assert_eq!(p.a, [30.0, 40.0, 0.0, 450.0]);
        assert_eq!((p.b[0], p.b[1], p.b[3]), (4.0, 1.0, 6.0), "a band 8 px thick through 6 corners");
        assert_eq!((p.c[0], p.color), (thing_code(SHAPE), BODY_WHITE));
        let even = ShapePath::even(6);
        for k in 0..6 {
            let at = Vec2::new(frame.path_points[k][0], frame.path_points[k][1]);
            assert!((0..6).any(|j| (even.point(j) - at).length() < 1e-4), "{at}");
        }
    }

    #[test]
    fn a_filled_circle_draws_with_no_band() {
        let mut s = ShapeSettings::circle(300.0, 12.0);
        s.fill = ShapeFill::Filled;
        let frame = frame_of(&with(s));
        assert_eq!(frame.paths.len(), 1);
        assert_eq!((frame.paths[0].b[0], frame.paths[0].b[1], frame.paths[0].a[3]), (0.0, 0.0, 300.0));
        assert!(frame.path_points.is_empty());
        assert!(frame.shapes.is_empty(), "shapes don't use the old wall pipeline");
    }

    #[test]
    fn a_moulded_outline_draws_its_own_points() {
        let mut s = ShapeSettings::polygon(6, 200.0, 2.0);
        let mut path = ShapePath::even(5);
        path.create_curve(0);
        s.path = path.clone();
        let frame = frame_of(&with(s));
        let flat = path.flatten(bpe_core::shape_path::FLATTEN);
        assert_eq!(frame.path_points.len(), flat.len());
        assert_eq!(frame.paths[0].a[3], 200.0);
        // A rectangle draws its 4 corners.
        let frame = frame_of(&with(ShapeSettings::rectangle(300.0, 100.0, 2.0)));
        assert_eq!(frame.path_points.len(), 4);
    }

    #[test]
    fn a_shape_with_no_look_draws_nothing() {
        let mut scene = with(ShapeSettings::circle(300.0, 3.0));
        scene.things.get_mut(&SHAPE).unwrap().slots.remove(&SlotId::Look);
        assert!(frame_of(&scene).paths.is_empty());
    }
}
