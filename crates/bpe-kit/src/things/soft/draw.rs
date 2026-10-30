//! Drawing soft bodies: a jelly ball filled and rimmed, a rope as rounded links, cloth as
//! shaded triangles. Each point takes its own colour from the look's paint, read as the point's
//! place, its order among the points (or how far round the skin it is, for Path) and its own
//! speed and hits; an inside pattern is painted across the body's bounding box.

use bpe_gfx::{DrawCx, Frame, PaintGpu, ShapeInstance, SoftVertex, ThingDraw, blend_alpha};
use glam::Vec2;

use super::sim::SoftState;
use super::{SoftShape, SoftWorld};
use crate::modules::look::draw::{WallInputs, for_walls, look_of};
use crate::paint::{Inside, colour_at, paint_gpu};

pub struct SoftDraw;

/// How far past a soft shape's edge its triangles reach, in world units, so the edge can fade.
const EDGE: f32 = 1.5;

fn lighter(c: [f32; 4], by: f32) -> [f32; 4] {
    [c[0] + (1.0 - c[0]) * by, c[1] + (1.0 - c[1]) * by, c[2] + (1.0 - c[2]) * by, c[3]]
}

/// Half way between `a` and `b`; exactly `a` when they are the same.
fn half(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, (a[2] + b[2]) * 0.5, (a[3] + b[3]) * 0.5]
}

fn vertex(position: Vec2, color: [f32; 4], inside: f32) -> SoftVertex {
    SoftVertex { position: position.into(), inside, _pad: 0.0, color, paint: PaintGpu::default(), local: [0.0; 2] }
}

impl ThingDraw for SoftDraw {
    fn kind(&self) -> &'static str {
        "soft"
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let Some(st) = cx.world.states.get(&cx.thing.id) else { return };
        let Some(st) = st.as_any().downcast_ref::<SoftState>() else { return };
        let Some(look) = look_of(cx) else { return };
        let w = &st.world;
        // A soft body never spawns, so Start reads as Place.
        let fill = for_walls(look.fill);
        let n = w.points.len();
        let inputs = WallInputs::new(cx, &fill, n, 0.0);
        // What the paint reads about point `k`: its place, its own speed, and how far round the
        // skin (or along the rope, or through the cloth) it is.
        let point_inputs = |k: usize| {
            let p = &w.points[k];
            let mut i = inputs.get(&fill, k, p.position);
            let speed = p.velocity.length();
            i.speed = if speed.is_finite() { speed } else { 0.0 };
            i.path = k as f32 / n.max(1) as f32;
            i
        };
        let colours: Vec<[f32; 4]> = (0..n).map(|k| blend_alpha(colour_at(&fill, &point_inputs(k)))).collect();
        let first = frame.soft.len();
        match w.shape {
            SoftShape::Jelly => jelly(w, &colours, look.glow, frame),
            SoftShape::Rope => {
                let radius = w.thickness * 0.5;
                for (a, b) in w.links_of() {
                    let color = half(colours[a as usize], colours[b as usize]);
                    let (a, b) = (w.points[a as usize].position, w.points[b as usize].position);
                    frame.shapes.push(ShapeInstance::capsule(a, b, radius, color, look.glow));
                }
            }
            SoftShape::Cloth => cloth(w, &colours, frame),
        }
        let body = &mut frame.soft[first..];
        if body.is_empty() {
            return;
        }
        // The body's bounding box, each corner placed in it from -1 to 1.
        let (lo, hi) = body.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), v| {
            let p = Vec2::from(v.position);
            (lo.min(p), hi.max(p))
        });
        let (centre, reach) = ((lo + hi) * 0.5, ((hi - lo) * 0.5).max(Vec2::splat(1e-3)));
        for v in body.iter_mut() {
            v.local = ((Vec2::from(v.position) - centre) / reach).into();
        }
        if fill.inside == Inside::None {
            return;
        }
        // The pattern starts where the paint puts the first point, read at the body's centre.
        let mut at = point_inputs(0);
        at.place = centre;
        at.place_screen = inputs.get(&fill, 0, centre).place_screen;
        let paint = paint_gpu(&fill, &at, &mut frame.gradients, cx.time as f32);
        for v in &mut frame.soft[first..] {
            v.paint = paint;
        }
    }
}

/// A fan of triangles from the middle to the skin's outer edge, lighter inside, with a rim. Each
/// skin point wears its own colour; the middle wears them all mixed.
fn jelly(w: &SoftWorld, colours: &[[f32; 4]], glow: f32, frame: &mut Frame) {
    let n = w.points.len();
    if n < 3 {
        return;
    }
    let middle = w.middle();
    let edge = |k: usize| {
        let p = &w.points[k % n];
        p.position + (p.position - middle).normalize_or_zero() * p.radius
    };
    let colour = |k: usize| colours[k % n];
    let reach = (0..n).map(|k| edge(k).distance(middle)).fold(0.0f32, f32::max);
    let mixed = if colours.iter().all(|c| *c == colours[0]) {
        colours[0]
    } else {
        let sum = colours.iter().fold([0.0f32; 4], |s, c| [s[0] + c[0], s[1] + c[1], s[2] + c[2], s[3] + c[3]]);
        sum.map(|x| x / n as f32)
    };
    let inner = lighter(mixed, 0.35);
    for k in 0..n {
        let (a, b) = (edge(k), edge(k + 1));
        let out = |p: Vec2| p + (p - middle).normalize_or_zero() * EDGE;
        frame.soft.extend_from_slice(&[
            vertex(middle, inner, reach),
            vertex(out(a), colour(k), -EDGE),
            vertex(out(b), colour(k + 1), -EDGE),
        ]);
    }
    let width = (reach * 0.03).max(1.0);
    for k in 0..n {
        let (a, b) = (edge(k), edge(k + 1));
        let rim = lighter(half(colour(k), colour(k + 1)), 0.6);
        let inset = |p: Vec2| p + (middle - p).normalize_or_zero() * width;
        frame.shapes.push(ShapeInstance::capsule(inset(a), inset(b), width, rim, glow));
    }
}

/// Two triangles per cell whose four links hold, darker where the cloth is squashed and lighter
/// where it is stretched, so folds show. Each corner wears its point's colour.
fn cloth(w: &SoftWorld, colours: &[[f32; 4]], frame: &mut Frame) {
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
            let corner = |p: Vec2, c: usize, r: usize| {
                let color = colours[r * columns + c];
                let shaded = [color[0] * s * checker, color[1] * s * checker, color[2] * s * checker, color[3]];
                vertex(p, shaded, deep)
            };
            frame.soft.extend_from_slice(&[
                corner(p00, c, r),
                corner(p10, c + 1, r),
                corner(p11, c + 1, r + 1),
                corner(p00, c, r),
                corner(p11, c + 1, r + 1),
                corner(p01, c, r + 1),
            ]);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{kit, new_scene};
    use bpe_core::{Color, Gradient, ThingId, Value, resolve};
    use bpe_gfx::{Frame, PaintGpu, build_frame};
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

    /// A soft body of `shape` wearing a classic look with `colours`, set up by `f`.
    fn painted_frame(shape: &str, detail: i64, f: impl FnOnce(&mut bpe_core::Params)) -> Frame {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let soft = scene.add_thing(&kit.registry, "soft", ThingId::ROOT).unwrap();
        let thing = scene.things.get_mut(&soft).unwrap();
        thing.params.set("shape", Value::Choice(shape.into()));
        thing.params.set("detail", Value::Int(detail));
        let look = scene.add_module(&kit.registry, "look.classic", "Soft colours").unwrap();
        f(&mut scene.library.modules.get_mut(&look).unwrap().params);
        scene.link(&kit.registry, soft, bpe_core::SlotId::Look, look).unwrap();
        let resolved = resolve(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, &world, &Log::default(), 0.0, &kit.draws, &mut frame);
        frame
    }

    const RED: Color = Color::rgb(255, 0, 0);
    const GREEN: Color = Color::rgb(0, 255, 0);
    const BLUE: Color = Color::rgb(0, 0, 255);

    #[test]
    fn soft_body_order_spreads_along_points() {
        let frame = painted_frame("jelly", 24, |p| {
            p.set("colours", Value::Gradient(Gradient::even(&[RED, GREEN, BLUE])));
            p.set("follow", Value::Choice("order".into()));
        });
        // Triangle k runs from the middle to the skin at point k, then point k + 1.
        let skin = |k: usize| frame.soft[k * 3 + 1].color;
        assert_eq!(skin(0), RED.to_linear(), "the first point takes the first colour");
        let middle = Gradient::even(&[RED, GREEN, BLUE]).sample(12.0 / 23.0);
        assert_eq!(skin(12), middle, "the points spread along the colours");
        assert_eq!(skin(23), BLUE.to_linear(), "the last point takes the last colour");
        assert_eq!(frame.soft[23 * 3 + 2].color, RED.to_linear(), "the skin closes back on the first point");
        assert!(frame.soft.iter().all(|v| v.paint == PaintGpu::default()), "no inside pattern");
        assert!(frame.gradients.len() == 1);
        // A rope's links blend the colours of their two ends.
        let frame = painted_frame("rope", 3, |p| {
            p.set("colours", Value::Gradient(Gradient::even(&[RED, BLUE])));
            p.set("follow", Value::Choice("order".into()));
        });
        let link = |k: usize| frame.shapes[1 + k].color;
        let half = |a: [f32; 4], b: [f32; 4]| [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, (a[2] + b[2]) * 0.5, 1.0];
        let mid = Gradient::even(&[RED, BLUE]).sample(0.5);
        assert_eq!(link(0), half(RED.to_linear(), mid));
        assert_eq!(link(1), half(mid, BLUE.to_linear()));
    }

    #[test]
    fn an_inside_pattern_paints_across_the_body_s_box() {
        let frame = painted_frame("cloth", 5, |p| {
            p.set("colours", Value::Gradient(Gradient::even(&[RED, BLUE])));
            p.set("inside", Value::Choice("stripes".into()));
        });
        assert_eq!(frame.gradients.len(), 2);
        assert!(frame.soft.iter().all(|v| v.paint.row == 1.0 && v.paint.pattern == 1.0));
        let (lo, hi) = frame.soft.iter().fold(([f32::MAX; 2], [f32::MIN; 2]), |(lo, hi), v| {
            ([lo[0].min(v.local[0]), lo[1].min(v.local[1])], [hi[0].max(v.local[0]), hi[1].max(v.local[1])])
        });
        let near = |a: [f32; 2], b: [f32; 2]| (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4;
        assert!(near(lo, [-1.0, -1.0]) && near(hi, [1.0, 1.0]), "{lo:?} {hi:?}: the box spans -1 to 1");
    }

    #[test]
    fn a_cloth_draws_two_triangles_per_cell() {
        let frame = frame_with("cloth", 5);
        // 5 columns × 20 rows: 4 × 19 cells.
        assert_eq!(frame.soft.len(), 4 * 19 * 6);
        assert_eq!(frame.shapes.len(), 1);
    }

    /// How many of `colours` differ from all the others before them by more than a hair.
    fn distinct(colours: &[[f32; 4]]) -> usize {
        let mut seen: Vec<[f32; 4]> = Vec::new();
        for c in colours {
            if !seen.iter().any(|d| (0..3).all(|k| (d[k] - c[k]).abs() < 1e-4)) {
                seen.push(*c);
            }
        }
        seen.len()
    }

    #[test]
    fn soft_points_each_vary_their_colour() {
        let one = |p: &mut bpe_core::Params| p.set("colours", Value::Gradient(Gradient::even(&[RED])));
        let plain = painted_frame("jelly", 12, one);
        let skin = |f: &Frame| (0..12).map(|k| f.soft[k * 3 + 1].color).collect::<Vec<_>>();
        assert_eq!(distinct(&skin(&plain)), 1);
        let frame = painted_frame("jelly", 12, |p| {
            one(p);
            p.set("vary_hue", Value::Number(60.0));
        });
        assert!(distinct(&skin(&frame)) >= 10, "{:?}", skin(&frame));
    }
}
