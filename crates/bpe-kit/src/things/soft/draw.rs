//! Drawing soft bodies: a jelly ball filled and rimmed, a rope as rounded links, cloth as
//! shaded triangles.

use bpe_gfx::{DrawCx, Frame, ShapeInstance, SoftVertex, ThingDraw, blend_alpha};
use glam::Vec2;

use super::sim::SoftState;
use super::{SoftShape, SoftWorld};
use crate::modules::look::draw::{colors, look_of};

pub struct SoftDraw;

/// How far past a soft shape's edge its triangles reach, in world units, so the edge can fade.
const EDGE: f32 = 1.5;

fn lighter(c: [f32; 4], by: f32) -> [f32; 4] {
    [c[0] + (1.0 - c[0]) * by, c[1] + (1.0 - c[1]) * by, c[2] + (1.0 - c[2]) * by, c[3]]
}

fn vertex(position: Vec2, color: [f32; 4], inside: f32) -> SoftVertex {
    SoftVertex { position: position.into(), inside, _pad: 0.0, color }
}

impl ThingDraw for SoftDraw {
    fn kind(&self) -> &'static str {
        "soft"
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let Some(st) = cx.world.states.get(&cx.thing.id) else { return };
        let Some(st) = st.as_any().downcast_ref::<SoftState>() else { return };
        let Some(look) = look_of(cx) else { return };
        let color = blend_alpha(colors(&look, 1, |_| 0.0)(0));
        let w = &st.world;
        match w.shape {
            SoftShape::Jelly => jelly(w, color, look.glow, frame),
            SoftShape::Rope => {
                let radius = w.thickness * 0.5;
                for (a, b) in w.links_of() {
                    let (a, b) = (w.points[a as usize].position, w.points[b as usize].position);
                    frame.shapes.push(ShapeInstance::capsule(a, b, radius, color, look.glow));
                }
            }
            SoftShape::Cloth => cloth(w, color, frame),
        }
    }
}

/// A fan of triangles from the middle to the skin's outer edge, lighter inside, with a rim.
fn jelly(w: &SoftWorld, color: [f32; 4], glow: f32, frame: &mut Frame) {
    let n = w.points.len();
    if n < 3 {
        return;
    }
    let middle = w.middle();
    let edge = |k: usize| {
        let p = &w.points[k % n];
        p.position + (p.position - middle).normalize_or_zero() * p.radius
    };
    let reach = (0..n).map(|k| edge(k).distance(middle)).fold(0.0f32, f32::max);
    let inner = lighter(color, 0.35);
    for k in 0..n {
        let (a, b) = (edge(k), edge(k + 1));
        let out = |p: Vec2| p + (p - middle).normalize_or_zero() * EDGE;
        frame.soft.extend_from_slice(&[
            vertex(middle, inner, reach),
            vertex(out(a), color, -EDGE),
            vertex(out(b), color, -EDGE),
        ]);
    }
    let rim = lighter(color, 0.6);
    let width = (reach * 0.03).max(1.0);
    for k in 0..n {
        let (a, b) = (edge(k), edge(k + 1));
        let inset = |p: Vec2| p + (middle - p).normalize_or_zero() * width;
        frame.shapes.push(ShapeInstance::capsule(inset(a), inset(b), width, rim, glow));
    }
}

/// Two triangles per cell whose four links hold, darker where the cloth is squashed and lighter
/// where it is stretched, so folds show.
fn cloth(w: &SoftWorld, color: [f32; 4], frame: &mut Frame) {
    let (columns, rows) = (w.columns as usize, w.rows as usize);
    if columns < 2 || rows < 2 || w.points.len() < columns * rows {
        return;
    }
    let at = |c: usize, r: usize| w.points[r * columns + c].position;
    let mut across = vec![false; columns * rows];
    let mut down = vec![false; columns * rows];
    for (a, b) in w.links_of() {
        let (a, b) = (a.min(b) as usize, a.max(b) as usize);
        if b == a + 1 && b % columns != 0 {
            across[a] = true;
        } else if b == a + columns {
            down[a] = true;
        }
    }
    // The spacing of points along the top edge, which keeps its length.
    let rest = ((at(columns - 1, 0) - at(0, 0)).length() / (columns - 1) as f32).max(1e-3);
    let shade = |area: f32| {
        let stretch = (area.abs().sqrt() / rest).clamp(0.5, 1.5);
        0.55 + 0.45 * stretch.min(1.2) / 1.2
    };
    // Deep inside: no fading across the shared edges of cells.
    let deep = EDGE * 4.0;
    for r in 0..rows - 1 {
        for c in 0..columns - 1 {
            let k = r * columns + c;
            if !(across[k] && down[k] && across[k + columns] && down[k + 1]) {
                continue;
            }
            let (p00, p10, p01, p11) = (at(c, r), at(c + 1, r), at(c, r + 1), at(c + 1, r + 1));
            let s = shade((p10 - p00).perp_dot(p01 - p00));
            let checker = if (c + r) % 2 == 0 { 1.0 } else { 0.93 };
            let shaded = [color[0] * s * checker, color[1] * s * checker, color[2] * s * checker, color[3]];
            frame.soft.extend_from_slice(&[
                vertex(p00, shaded, deep),
                vertex(p10, shaded, deep),
                vertex(p11, shaded, deep),
                vertex(p00, shaded, deep),
                vertex(p11, shaded, deep),
                vertex(p01, shaded, deep),
            ]);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{kit, new_scene};
    use bpe_core::{ThingId, Value, resolve};
    use bpe_gfx::{Frame, build_frame};
    use bpe_runtime::{Log, World};

    fn frame_with(shape: &str, detail: i64) -> Frame {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let soft = scene.add_thing(&kit.registry, "soft", ThingId::ROOT).unwrap();
        let thing = scene.things.get_mut(&soft).unwrap();
        thing.params.set("shape", Value::Choice(shape.into()));
        thing.params.set("detail", Value::Int(detail));
        let resolved = resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        frame
    }

    #[test]
    fn a_jelly_draws_triangles_and_a_rim() {
        let frame = frame_with("jelly", 24);
        assert_eq!(frame.soft.len(), 24 * 3);
        // The arena's outline and one rim capsule per skin segment.
        assert_eq!(frame.shapes.len(), 1 + 24);
        assert!(frame.shapes[1..].iter().all(|s| s.c[0] == 2.0));
        assert_eq!(frame.balls.len(), 1);
        // The middle is at least as light as the skin (the default look is white).
        assert!(frame.soft[0].color[1] >= frame.soft[1].color[1]);
        assert!(frame.soft[0].inside > 100.0 && frame.soft[1].inside < 0.0);
    }

    #[test]
    fn a_rope_draws_a_capsule_per_link() {
        let frame = frame_with("rope", 10);
        assert!(frame.soft.is_empty());
        assert_eq!(frame.shapes.len(), 1 + 9);
        assert_eq!(frame.shapes[1].c[1], 4.0, "half the default thickness");
    }

    #[test]
    fn a_cloth_draws_two_triangles_per_cell() {
        let frame = frame_with("cloth", 5);
        // 5 columns × 20 rows: 4 × 19 cells.
        assert_eq!(frame.soft.len(), 4 * 19 * 6);
        assert_eq!(frame.shapes.len(), 1);
    }
}
