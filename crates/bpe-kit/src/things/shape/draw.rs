//! Drawing a shape: its plain white Body, filled or as a band, with the paint layers of its Look
//! (see `modules::look::shape`) walked over it on the GPU.

use std::f32::consts::FRAC_PI_2;

use bpe_core::{Blend, ResolvedThing, ShapePath, SlotId};
use bpe_gfx::{Around, DrawCx, Frame, PathInstance, ThingDraw, thing_code};
use bpe_runtime::{EventKind, Outline, Wall};
use glam::Vec2;

use super::KIND;
use crate::modules::look::layers::around::{AroundKind, AroundLayer, around_layers};
use crate::modules::look::layers::around_subs::HitInfo;
use crate::modules::look::layers::trail::AroundPlace;
use crate::modules::look::shape::ShapeStack;
use crate::modules::look::shape::gpu::Geometry;
use crate::things::balls::draw::hit_strength;

/// A shape's body colour in linear light.
pub const BODY_WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

pub struct ShapeDraw;

impl ThingDraw for ShapeDraw {
    fn kind(&self) -> &'static str {
        KIND
    }

    /// A shape is a wall the balls play in front of: a blended one never tints them.
    fn behind(&self) -> bool {
        true
    }

    /// How the shape mixes with what is behind it: its Blend layer's (for the layers under it).
    fn blend(&self, resolved: &ResolvedThing) -> Option<(Blend, f32, bool)> {
        ShapeStack::of(resolved.slots.get(&SlotId::Look)?).map(|s| s.whole_blend())
    }

    /// The shape where its wall is this frame, wearing its Look's layers: all of them, or, when
    /// its Blend layer splits it, the ones under the Blend layer. A shape with no Look card draws
    /// nothing; a card of another kind draws it plain white.
    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let part = shape_stack(cx).and_then(|s| s.split()).unwrap_or(0);
        draw_part(cx, frame, part);
    }

    /// The layers above the Blend layer, painted normally over the blended rest.
    fn draw_over(&self, cx: &DrawCx, frame: &mut Frame) {
        if let Some(cut) = shape_stack(cx).and_then(|s| s.split()) {
            draw_part(cx, frame, cut | OVER);
        }
    }

    /// The shape's surface (and its layers' own), and its outline when it is a mirror, for a lit
    /// frame.
    fn light(&self, cx: &DrawCx, scene: &mut bpe_gfx::LightScene) {
        use crate::modules::look::surface;
        let Some(look) = cx.resolved.slots.get(&SlotId::Look).filter(|l| surface::has_surface(&l.kind)) else {
            return;
        };
        let index = surface::surface_index(cx.scene, cx.thing.id);
        let s = surface::of_card(look);
        scene.set_surface(index, s.gpu(bpe_gfx::SurfaceGpu::SHAPE));
        scene.refracts |= s.see_through();
        let stamps = surface::Stamps::of(cx.scene, cx.thing.id, look);
        stamps.fill(scene, bpe_gfx::SurfaceGpu::SHAPE);
        // Glass that bends lamp light (glass and light part 3): the shape's surface, and each of its
        // layers' own, as a bender over the shape's box, when its layer (the body for the whole
        // shape's) blocks light.
        let blocks = |layer: Option<bpe_core::LayerId>| {
            layer.is_some_and(|id| {
                let key = bpe_core::layered_key(id, crate::modules::look::layers::lighting::BLOCKS_LIGHT);
                matches!(look.params.get(&key), Some(bpe_core::Value::Bool(true)))
            })
        };
        let benders: Vec<u32> = std::iter::once((index, &s, look.layers.first().map(|b| b.id)))
            .chain(stamps.layers.iter().map(|(layer, i, ls)| (*i, ls, Some(*layer))))
            .filter(|(_, s, layer)| surface::bends(s) && blocks(*layer))
            .map(|(i, _, _)| i)
            .collect();
        if !benders.is_empty()
            && let Some(wall) = cx.world.walls.get(&cx.thing.id)
        {
            let form = wall_form(wall);
            if let Some(round) = bpe_gfx::Bender::round(outline_world(&form), form.half + 2.0, 0) {
                scene.benders.extend(benders.into_iter().map(|index| bpe_gfx::Bender { index, ..round }));
            }
        }
        // Glass and light part 4a: the shape's surface, and each of its layers' own, that throws
        // lamp light back like a mirror, when its layer blocks light; and a light pipe.
        let colour = surface::thing_colour(look);
        let throwers: Vec<(u32, [f32; 3], &surface::SurfaceSettings)> =
            std::iter::once((index, &s, look.layers.first().map(|b| b.id), colour))
                .chain(stamps.layers.iter().map(|(layer, i, ls)| {
                    (*i, ls, Some(*layer), crate::sensing::probe::layer_colour(&look.layer_params(*layer)))
                }))
                .filter(|(_, _, layer, _)| blocks(*layer))
                .filter_map(|(i, ls, _, c)| ls.throw_of(c).map(|back| (i, back, ls)))
                .collect();
        if (!throwers.is_empty() || s.light_pipe)
            && let Some(wall) = cx.world.walls.get(&cx.thing.id)
        {
            let form = wall_form(wall);
            let outline = outline_world(&form);
            if let Some(round) = bpe_gfx::Bender::round(outline.iter().copied(), form.half + 2.0, 0) {
                let finite = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
                for (index, colour, ls) in throwers {
                    scene.push_thrower(bpe_gfx::Thrower {
                        lo: round.lo,
                        hi: round.hi,
                        index,
                        colour,
                        widen: ls.throw_widen(),
                        facets: finite(ls.facets),
                        gaps: finite(ls.facet_gaps).min(1.0),
                        ball: false,
                    });
                }
            }
            if s.light_pipe {
                let most = colour[0].max(colour[1]).max(colour[2]);
                let tint = colour.map(|c| if most > 1e-4 { (c / most).clamp(0.0, 1.0) } else { 1.0 });
                let finite = |v: f32, or: f32| if v.is_finite() { v } else { or };
                scene.push_pipe(bpe_gfx::Pipe {
                    points: outline,
                    carries: finite(s.pipe_carries, 600.0).max(1.0),
                    glow: finite(s.pipe_glow, 0.0).max(0.0),
                    tint,
                    bends: finite(s.bends_light, 1.5).max(1.0001),
                    width: 2.0 * form.half,
                });
            }
        }
        if scene.mirrors.len() < bpe_gfx::MOST_MIRRORS
            && let Some(wall) = cx.world.walls.get(&cx.thing.id)
        {
            let form = wall_form(wall);
            if let Some(mut mirror) = s.mirror_gpu(form.scale) {
                let points = outline_world(&form);
                let first = scene.segments.len();
                for k in 0..points.len() {
                    let (a, b) = (points[k], points[(k + 1) % points.len()]);
                    scene.segments.push([a.x, a.y, b.x, b.y]);
                }
                mirror.a[0] = first as f32;
                mirror.a[1] = (scene.segments.len() - first) as f32;
                mirror.b[2] = form.half;
                scene.mirrors.push(mirror);
            }
        }
    }

    /// Its Look's particles.
    fn around(&self, resolved: &ResolvedThing) -> Vec<Around> {
        around_of(resolved)
            .iter()
            .map(|a| {
                let (blend, amount) = a.blend();
                Around { blend, amount, behind: a.place() == AroundPlace::Behind }
            })
            .collect()
    }

    fn draw_around(&self, cx: &DrawCx, index: usize, frame: &mut Frame) {
        let Some(layer) = around_of(cx.resolved).into_iter().nth(index) else { return };
        if let AroundKind::Particles(particles) = &layer.kind {
            let drew = crate::modules::look::layers::around::Drew::from(frame);
            let reads = layer.subs.iter().any(|s| s.reads_hits());
            let hit = if reads { shape_hits(cx) } else { bpe_gfx::NO_HIT };
            let last = (hit[0] < bpe_gfx::NO_HIT[0]).then_some((hit[0], hit[1]));
            let hits = HitInfo { last, hits: hit[2] as u32 };
            crate::modules::look::layers::particles::draw::draw_shape(cx, &layer, particles, hits, frame);
            drew.light(frame, &layer, cx);
        }
    }
}

/// The shape's shown particles layers, bottom first.
fn around_of(resolved: &ResolvedThing) -> Vec<AroundLayer> {
    match resolved.slots.get(&SlotId::Look) {
        Some(look) if look.kind == crate::modules::look::shape::CARD => around_layers(look, true),
        _ => Vec::new(),
    }
}

/// The bit of a shape instance's `layers[3]` that draws the layers above its Blend layer.
const OVER: u32 = 0x1_0000;

/// The stack of the shape `cx` is drawing, when its Look card is a shape one.
fn shape_stack(cx: &DrawCx) -> Option<ShapeStack> {
    ShapeStack::of(cx.resolved.slots.get(&SlotId::Look)?)
}

/// Draws the shape `cx` is drawing: whole (`part` 0), the layers under its Blend layer (`part`
/// the first record above it), or the layers above it (that with [`OVER`]): see shape_walk.wgsl.
fn draw_part(cx: &DrawCx, frame: &mut Frame, part: u32) {
    let Some(look) = cx.resolved.slots.get(&SlotId::Look) else { return };
    // A broken shape draws as it stood while its Break look plays out.
    let remains = crate::rules::remains::remains_now(cx, None);
    let Some(wall) = cx.world.walls.get(&cx.thing.id).or_else(|| remains.as_ref().and_then(|(r, _)| r.wall.as_ref()))
    else {
        return;
    };
    let mut form = wall_form(wall);
    let whole = remains.as_ref().and_then(|(r, l)| crate::rules::remains::draw::whole_look(r, l, cx.time));
    if let Some(s) = whole {
        // Pop swells it about its middle.
        form.scale *= s.scale.max(0.0);
        form.half *= s.scale.max(0.0);
    }
    let index = push_form(&form, BODY_WHITE, thing_code(cx.thing.id), frame);
    // Ripples measure from its outline (once, not again for the part over its Blend layer).
    // A shattered shape's outline is in pieces, so it gives no ripples.
    let shattered = remains.as_ref().is_some_and(|(_, l)| l.look == crate::rules::breaks::BreakLook::Shatter);
    if frame.ripples && part & OVER == 0 && !shattered {
        let points: Vec<[f32; 2]> = outline_world(&form).iter().map(|p| p.to_array()).collect();
        frame.push_ripple_outline(form.center.to_array(), &points, form.half);
    }
    wear_layers(cx, look, &form, index, part, frame);
    let path = &mut frame.paths[index];
    if let Some((r, l)) = &remains {
        use crate::rules::remains::{draw, shatter::Edge};
        match whole {
            Some(s) => path.clip[2] = 1.0 - s.alpha.clamp(0.0, 1.0),
            None if l.look == crate::rules::breaks::BreakLook::Shatter => {
                let edge = match (&form.points, form.ellipse) {
                    (Some(points), _) => Edge::Points(points),
                    (None, axes) => Edge::Ellipse(axes.unwrap_or(Vec2::ONE)),
                };
                let half = form.half / form.scale.max(1e-3);
                let walls = draw::walls_of(cx.world);
                draw::shatter_shape(r, l, cx.thing.id.0, edge, half, index, cx.time, &walls, frame);
            }
            None => {}
        }
    }
}

/// Puts the shape's Look layers on its instance at `index` (pushed for `form`).
fn wear_layers(
    cx: &DrawCx,
    look: &bpe_core::ResolvedSlot,
    form: &WallForm,
    index: usize,
    part: u32,
    frame: &mut Frame,
) {
    let Some(stack) = ShapeStack::of(look) else { return };
    let geometry =
        Geometry { points: form.points.clone(), ellipse: form.ellipse, half: form.half / form.scale.max(1e-3) };
    let mut records = stack.records(&geometry, &mut frame.gradients);
    let link = crate::things::light::links::number_of(cx.scene, cx.thing.id);
    crate::modules::look::layers::lighting::stamp(&mut records, link, frame.light.as_mut());
    let first = frame.look_layers.len() as u32;
    let count = records.len() as u32;
    // Every record names the shape's surface, or its layer's own (read only by a lit frame's light
    // passes), and the body carries the Surface's Lens: how much the shape bulges.
    use crate::modules::look::surface;
    surface::Stamps::of(cx.scene, cx.thing.id, look).apply(&mut records, |layer| stack.record_of(layer));
    if surface::has_surface(&look.kind)
        && let Some(body) = records.first_mut()
    {
        let lens = surface::of_card(look).lens;
        body.light[2] = if lens.is_finite() { lens.max(0.0) } else { 0.0 };
    }
    frame.look_layers.extend(records);
    // Its Shader layers' program, named once per frame, as balls name theirs.
    let program = stack.program().map_or(PathInstance::NO_PROGRAM, |p| {
        match frame.programs.iter().position(|q| q.key == p.key) {
            Some(i) => i as u32,
            None => {
                frame.programs.push(p);
                frame.programs.len() as u32 - 1
            }
        }
    });
    let mut hit = if stack.needs_hits() { shape_hits(cx) } else { bpe_gfx::NO_HIT };
    if let Some(heat) = stack.heat() {
        hit[3] = crate::modules::look::layers::heat::shape_heat(cx, heat);
    }
    let path = &mut frame.paths[index];
    path.layers = [first, count, program, part];
    path.c[2] = stack.reach_past();
    path.hit = hit;
}

/// The hit data of the shape `cx` is drawing, for its Flash and Colour on hit sub-layers: how long
/// ago its last hit was (by a ball against it), that hit's strength and its hits so far.
pub(crate) fn shape_hits(cx: &DrawCx) -> [f32; 4] {
    let id = cx.thing.id;
    let rate = cx.world.steps_per_second;
    let mut hit = bpe_gfx::NO_HIT;
    for e in cx.log.since(0) {
        // Hits by a ball against it, and a rule's flashes of it (rules part 6).
        let ours = match e.kind {
            EventKind::Hit => e.b.is_some_and(|b| b.thing == id),
            EventKind::Flash => e.a.thing == id,
            _ => false,
        };
        if !ours {
            continue;
        }
        let time = e.time(rate);
        if time > cx.time {
            break;
        }
        hit = [(cx.time - time) as f32, hit_strength(e.speed), hit[2] + 1.0, 0.0];
    }
    hit
}

/// A wall as it is drawn, in the shape's own frame, so its layers turn with it.
#[derive(Clone, Debug, PartialEq)]
pub struct WallForm {
    pub center: Vec2,
    /// How far the shape has turned, in radians clockwise.
    pub turn: f32,
    /// World pixels per shape unit.
    pub scale: f32,
    /// Half the band's thickness in world pixels (0 filled).
    pub half: f32,
    /// The outline's points in shape units, from the top (an even polygon's top-most corner, the
    /// left of two; a rectangle's top-left corner) going clockwise; `None` for a circle of radius
    /// 1, which starts at its top, or an ellipse.
    pub points: Option<Vec<Vec2>>,
    /// An exact ellipse's half-axes in shape units (the larger one 1).
    pub ellipse: Option<Vec2>,
}

/// How `wall` is drawn: a round ring is a circle; a ring polygon is an even polygon's corners
/// (`ShapePath::even`, the corners its Ring has) turned by the ring's angle less the quarter turn
/// `base_outline` adds; a box is its corners over its half diagonal; an ellipse is exact, scaled by
/// its larger half-axis; a free outline keeps its own points, angle and scale.
pub fn wall_form(wall: &Wall) -> WallForm {
    let half = if wall.solid { 0.0 } else { wall.outline.thickness() * 0.5 };
    match &wall.outline {
        &Outline::Ring { center, radius, sides, angle, .. } if sides < 3 => {
            WallForm { center, turn: angle, scale: radius, half, points: None, ellipse: None }
        }
        &Outline::Ring { center, radius, sides, angle, .. } => {
            let even = ShapePath::even(sides);
            let points = (0..even.points.len()).map(|k| even.point(k)).collect();
            WallForm { center, turn: angle - FRAC_PI_2, scale: radius, half, points: Some(points), ellipse: None }
        }
        &Outline::Box { center, half: size, angle, .. } => {
            let scale = size.length().max(1e-3);
            let corner = size / scale;
            let points = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .into_iter()
                .map(|(sx, sy)| Vec2::new(corner.x * sx, corner.y * sy))
                .collect();
            WallForm { center, turn: angle, scale, half, points: Some(points), ellipse: None }
        }
        &Outline::Ellipse { center, half: axes, angle, .. } => {
            let scale = axes.x.max(axes.y).max(1e-3);
            WallForm { center, turn: angle, scale, half, points: None, ellipse: Some(axes / scale) }
        }
        Outline::Path { center, angle, scale, points, .. } => WallForm {
            center: *center,
            turn: *angle,
            scale: *scale,
            half,
            points: Some(points.to_vec()),
            ellipse: None,
        },
    }
}

/// The most corners a mirror wall's outline takes (a circle or an ellipse is cut into this many).
const MIRROR_POINTS: usize = 128;

/// `form`'s outline in the world, as corners: a circle and an ellipse cut into [`MIRROR_POINTS`].
fn outline_world(form: &WallForm) -> Vec<Vec2> {
    let unit: Vec<Vec2> = match (&form.points, form.ellipse) {
        // Thinned to at most 512 (the most a mirror's outline tests), every step-th corner.
        (Some(points), _) => points.iter().step_by(points.len().div_ceil(512).max(1)).copied().collect(),
        (None, axes) => {
            let axes = axes.unwrap_or(Vec2::ONE);
            (0..MIRROR_POINTS)
                .map(|k| {
                    let a = k as f32 / MIRROR_POINTS as f32 * std::f32::consts::TAU;
                    Vec2::new(a.cos() * axes.x, a.sin() * axes.y)
                })
                .collect()
        }
    };
    let (sin, cos) = form.turn.sin_cos();
    unit.into_iter()
        .map(|p| {
            let q = p * form.scale;
            form.center + Vec2::new(q.x * cos - q.y * sin, q.x * sin + q.y * cos)
        })
        .collect()
}

/// Adds `form` to `frame` as one plain shape in `color`, giving its index in `frame.paths`.
fn push_form(form: &WallForm, color: [f32; 4], code: f32, frame: &mut Frame) -> usize {
    if let Some(axes) = form.ellipse {
        return frame.push_ellipse(form.center, form.turn, form.scale, form.half, axes, color, code);
    }
    frame.push_path(form.center, form.turn, form.scale, form.half, form.points.as_deref(), color, code)
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
    fn an_ellipse_draws_one_exact_shape() {
        let mut s = ShapeSettings::ellipse(900.0, 1300.0, 6.0);
        s.turn = 20.0;
        let frame = frame_of(&with(s));
        assert_eq!(frame.paths.len(), 1);
        let p = frame.paths[0];
        assert_eq!((p.b[0], p.b[1], p.a[3]), (3.0, 2.0, 650.0), "a band 6 px thick, kind ellipse, scale 650");
        assert!((p.b[2] - 450.0 / 650.0).abs() < 1e-6 && p.b[3] == 1.0, "{:?}", p.b);
        assert!((p.a[2] - 20f32.to_radians()).abs() < 1e-6);
        assert!(frame.path_points.is_empty());
        assert!(p.reach() >= 650.0 + 3.0);
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

    #[test]
    fn a_spinning_hexagon_turns_its_instance_not_its_points() {
        let mut s = ShapeSettings::polygon(6, 300.0, 8.0);
        s.spin = 90.0;
        let scene = with(s);
        let kit = crate::kit();
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        let before = wall_form(&world.walls[&SHAPE]);
        world.advance_to(1.0, &kit.sims, &mut Log::default());
        let after = wall_form(&world.walls[&SHAPE]);
        assert!(before.turn.abs() < 1e-6, "{}", before.turn);
        assert!((after.turn - std::f32::consts::FRAC_PI_2).abs() < 1e-3, "{}", after.turn);
        assert_eq!(before.points, after.points, "the corners stay put in the shape's own frame");
        // A hexagon has a flat top: its first corner is the left of the two up there.
        let ps = before.points.as_ref().unwrap();
        let top = ps.iter().map(|p| p.y).fold(f32::MAX, f32::min);
        assert!((ps[0].y - top).abs() < 1e-6 && ps[0].x < 0.0 && ps[1].x > 0.0, "{ps:?}");
    }

    #[test]
    fn a_box_draws_its_corners_from_the_top_left() {
        let scene = with(ShapeSettings::rectangle(300.0, 100.0, 2.0));
        let form = wall_form(&World::build(&scene, &resolve_of(&scene), &crate::kit().sims).walls[&SHAPE]);
        let ps = form.points.unwrap();
        assert_eq!(ps.len(), 4);
        assert!(ps[0].x < 0.0 && ps[0].y < 0.0 && ps[2].x > 0.0 && ps[2].y > 0.0);
        assert!((ps[0] * form.scale).abs_diff_eq(Vec2::new(-150.0, -50.0), 1e-3), "{}", ps[0] * form.scale);
    }

    fn resolve_of(scene: &Scene) -> bpe_core::Resolved {
        bpe_core::resolve(scene, &crate::kit().registry)
    }

    fn add(scene: &mut Scene, kind: &str) -> bpe_core::LayerId {
        let kit = crate::kit();
        scene.add_layer(&kit.registry, SHAPE, SlotId::Look, kind).unwrap()
    }

    #[test]
    fn a_shape_names_its_records_and_its_shader_program() {
        use crate::modules::look::shape as look;
        let mut scene = with(ShapeSettings::circle(300.0, 12.0));
        let frame = frame_of(&scene);
        assert_eq!(frame.paths[0].layers, [0, 1, PathInstance::NO_PROGRAM, 0], "the Body alone");
        assert_eq!(frame.look_layers[0].head[0], bpe_gfx::LayerGpu::BODY);
        add(&mut scene, look::PAINT);
        add(&mut scene, look::GLOW);
        add(&mut scene, look::SHADER);
        let frame = frame_of(&scene);
        let p = frame.paths[0];
        assert_eq!((p.layers[0], p.layers[1], p.layers[2]), (0, 4, 0));
        assert_eq!(frame.programs.len(), 1);
        let own = &frame.look_layers[p.layers[0] as usize..(p.layers[0] + p.layers[1]) as usize];
        let codes: Vec<u32> = own.iter().map(|l| l.head[0]).collect();
        assert_eq!(codes, [0, bpe_gfx::LayerGpu::SHAPE_PAINT, 4, 6]);
        assert_eq!(p.c[2], crate::modules::look::shape::gpu::GLOW_REACH * 30.0, "the glow widens its quad");
        assert!(p.reach() > 300.0 + 6.0 + 60.0);
    }

    #[test]
    fn a_shape_with_a_hit_sub_layer_knows_its_last_hit_and_its_hits_so_far() {
        use crate::modules::look::shape as look;
        let mut scene = with(ShapeSettings::circle(300.0, 12.0));
        let kit = crate::kit();
        let reg = &kit.registry;
        let paint = add(&mut scene, look::PAINT);
        scene.add_sub_layer(reg, SHAPE, SlotId::Look, paint, look::SUB_HUE).unwrap();
        let resolved = bpe_core::resolve(&scene, reg);
        let mut playback = bpe_runtime::Playback::new(&scene, &resolved, kit.sims.clone());
        playback.seek(10.0);
        let rate = playback.world().steps_per_second;
        let on_me: Vec<f64> = playback
            .log()
            .events()
            .iter()
            .filter(|e| e.kind == EventKind::Hit && e.b.is_some_and(|b| b.thing == SHAPE))
            .map(|e| e.time(rate))
            .collect();
        assert!(on_me.len() >= 3, "the balls hit the shape: {on_me:?}");
        let at = on_me[2] + 0.01;
        let mut frame = Frame::default();
        build_frame(&scene, &resolved, playback.world(), playback.log(), at, &kit.draws, &mut frame);
        let hit = frame.paths[0].hit;
        assert_eq!(hit[2], on_me.iter().filter(|t| **t <= at).count() as f32);
        assert!((hit[0] - 0.01).abs() < 0.006, "just after its last hit: {hit:?}");
        assert_eq!(frame.look_layers[1].head[3], 1, "the paint layer has one sub-layer");
        assert_eq!(frame.look_layers[2].head[0], bpe_gfx::LayerGpu::SUB_HUE);
    }

    #[test]
    fn a_blend_layer_mixes_the_whole_shape_with_what_is_behind() {
        use crate::modules::look::shape as look;
        let mut scene = with(ShapeSettings::circle(300.0, 12.0));
        assert!(frame_of(&scene).layers.is_empty());
        add(&mut scene, look::BLEND);
        let resolved = resolve_of(&scene);
        assert_eq!(ShapeDraw.blend(&resolved.things[&SHAPE]).map(|b| b.0), Some(bpe_core::Blend::Add));
        let frame = frame_of(&scene);
        assert!(frame.paths.is_empty(), "drawn into its blend layer");
        assert_eq!(frame.layers.len(), 1);
        assert_eq!(frame.layers[0].frame.paths.len(), 1);
        assert!(frame.layers[0].behind(), "mixed in before the balls are drawn");
    }

    #[test]
    fn a_blend_layer_blends_only_the_layers_under_it() {
        use crate::modules::look::shape as look;
        let mut scene = with(ShapeSettings::circle(300.0, 12.0));
        add(&mut scene, look::PAINT);
        add(&mut scene, look::BLEND);
        add(&mut scene, look::OUTLINE);
        let resolved = resolve_of(&scene);
        let stack = ShapeStack::of(&resolved.things[&SHAPE].slots[&SlotId::Look]).unwrap();
        assert_eq!(stack.split(), Some(2), "the outline's record, after the body's and the paint's");
        let frame = frame_of(&scene);
        assert!(frame.paths.is_empty());
        let places: Vec<bpe_gfx::Place> = frame.layers.iter().map(|l| l.place).collect();
        assert_eq!(places, [bpe_gfx::Place::Behind, bpe_gfx::Place::BehindOver], "both behind the balls");
        assert_eq!(frame.layers[0].blend, bpe_core::Blend::Add);
        assert_eq!(frame.layers[0].frame.paths[0].layers[3], 2, "the lower part: the layers under the Blend layer");
        assert_eq!(frame.layers[1].blend, bpe_core::Blend::Normal);
        assert_eq!(frame.layers[1].frame.paths[0].layers[3], 2 | OVER, "the upper part, painted normally");
        // With nothing drawn above it, the Blend layer mixes the whole shape.
        let mut top = with(ShapeSettings::circle(300.0, 12.0));
        add(&mut top, look::PAINT);
        add(&mut top, look::BLEND);
        let frame = frame_of(&top);
        assert_eq!(frame.layers.len(), 1);
        assert_eq!(frame.layers[0].frame.paths[0].layers[3], 0);
    }

    #[test]
    fn a_raised_shape_draws_over_the_balls_and_a_lowered_ball_under_everything() {
        let mut scene = with(ShapeSettings::circle(300.0, 12.0));
        scene.things.get_mut(&SHAPE).unwrap().z_order = 2;
        let frame = frame_of(&scene);
        assert!(frame.paths.is_empty(), "not with the things at 0");
        assert!(!frame.layered.is_empty(), "the balls stay at 0");
        let (z, place) = (frame.layers[0].z, frame.layers[0].place);
        assert_eq!((z, place, frame.layers[0].frame.paths.len()), (2, bpe_gfx::Place::Back, 1));
        assert!(frame.layers[0].plain());
        // A lowered ball goes to a plain layer below, its trail and particles with it.
        let ball = crate::testing::BALLS;
        scene.things.get_mut(&ball).unwrap().z_order = -1;
        let frame = frame_of(&scene);
        let zs: Vec<(i32, bpe_gfx::Place)> = frame.layers.iter().map(|l| (l.z, l.place)).collect();
        assert_eq!(zs, [(-1, bpe_gfx::Place::Front), (2, bpe_gfx::Place::Back)], "in drawing order");
        assert!(frame.layered.is_empty() && !frame.layers[0].frame.layered.is_empty());
    }

    /// The lit room's frame with its shape's (and ball's) Look Surface set to `see` See-through
    /// and `bends` Bends light.
    fn lit_benders(shape: (f32, f32), ball: (f32, f32)) -> Vec<bpe_gfx::Bender> {
        use bpe_core::{SlotId, Value};
        let kit = crate::kit();
        let mut scene = crate::scenes::lit_room(&kit.registry);
        let shape_id = scene.things.values().find(|t| t.kind == "shape").map(|t| t.id).expect("a shape");
        let ball_id = scene.things.values().find(|t| t.kind == "balls").map(|t| t.id).expect("a ball");
        for (id, (see, bends)) in [(shape_id, shape), (ball_id, ball)] {
            let look = scene.card_mut(id, SlotId::Look).unwrap();
            look.params.set("see_through", Value::Number(see.into()));
            look.params.set("bends_light", Value::Number(bends.into()));
        }
        frame_of(&scene).light.expect("a lit frame").benders
    }

    #[test]
    fn a_glass_shape_bends_lamp_light_and_a_glass_ball_doesnt() {
        let benders = lit_benders((0.85, 1.5), (0.85, 1.5));
        assert_eq!(benders.len(), 1, "the shape only: balls keep the sphere caustics");
        let b = benders[0];
        assert!(b.index >= 1, "the shape's surface index");
        assert!(b.lo[0] < b.hi[0] && b.lo[1] < b.hi[1], "{b:?}");
        // Matte see-through glass, solid glass and nearly straight glass don't bend.
        assert!(lit_benders((0.85, 1.0), (0.0, 1.0)).is_empty());
        assert!(lit_benders((0.0, 1.5), (0.0, 1.0)).is_empty());
        assert!(lit_benders((0.85, 1.00005), (0.0, 1.0)).is_empty());
    }

    #[test]
    fn a_glass_shape_that_doesnt_block_light_doesnt_bend_it() {
        use bpe_core::{SlotId, Value, layered_key};
        let kit = crate::kit();
        let mut scene = crate::scenes::lit_room(&kit.registry);
        let id = scene.things.values().find(|t| t.kind == "shape").map(|t| t.id).expect("a shape");
        let body = scene.things[&id].slots[&SlotId::Look].layers[0].id;
        let look = scene.card_mut(id, SlotId::Look).unwrap();
        look.params.set("see_through", Value::Number(0.85));
        look.params.set("bends_light", Value::Number(1.5));
        look.params.set(&layered_key(body, "blocks_light"), Value::Bool(false));
        assert!(frame_of(&scene).light.expect("a lit frame").benders.is_empty());
    }

    /// The lit room with its shape and ball wearing Surface `shape` and `ball` (their presets).
    fn lit_with(
        shape: crate::modules::look::surface::SurfaceKind,
        ball: crate::modules::look::surface::SurfaceKind,
    ) -> Scene {
        use bpe_core::{SlotId, Value};
        let kit = crate::kit();
        let mut scene = crate::scenes::lit_room(&kit.registry);
        let shape_id = scene.things.values().find(|t| t.kind == "shape").map(|t| t.id).expect("a shape");
        let ball_id = scene.things.values().find(|t| t.kind == "balls").map(|t| t.id).expect("a ball");
        for (id, kind) in [(shape_id, shape), (ball_id, ball)] {
            let look = scene.card_mut(id, SlotId::Look).unwrap();
            look.params.set("surface", Value::Choice(bpe_core::Choice::name(&kind).into()));
            for (k, v) in crate::modules::look::surface::preset_values(kind) {
                look.params.set(k, v);
            }
        }
        scene
    }

    #[test]
    fn chrome_shapes_and_mirror_balls_throw_light_and_matte_and_glass_dont() {
        use crate::modules::look::surface::SurfaceKind;
        let lit = |shape, ball| frame_of(&lit_with(shape, ball)).light.expect("a lit frame");
        let scene = lit(SurfaceKind::Chrome, SurfaceKind::MirrorBall);
        assert_eq!(scene.throwers.len(), 2, "{:?}", scene.throwers);
        let shape = scene.throwers.iter().find(|t| !t.ball).expect("the shape");
        assert!(shape.index >= 1 && shape.colour.iter().all(|c| *c > 0.5), "{shape:?}");
        assert!(shape.lo[0] < shape.hi[0] && shape.facets == 0.0);
        let ball = scene.throwers.iter().find(|t| t.ball).expect("the ball");
        assert_eq!((ball.facets, ball.gaps), (120.0, 0.12));
        let sphere = scene.spheres.iter().find(|s| s.c[0] as u32 == ball.index).expect("its sphere");
        assert!(ball.lo[0] <= sphere.a[0] - sphere.a[2] && ball.hi[1] >= sphere.a[1] + sphere.a[2], "{ball:?}");
        assert!(lit(SurfaceKind::Matte, SurfaceKind::Glass).throwers.is_empty());
        assert!(lit(SurfaceKind::Glass, SurfaceKind::Plastic).throwers.is_empty());
        assert!(lit(SurfaceKind::Chrome, SurfaceKind::Matte).pipes.is_empty(), "no light pipe unless asked");
    }

    #[test]
    fn a_light_pipe_lists_its_outline() {
        use crate::modules::look::surface::SurfaceKind;
        use bpe_core::{SlotId, Value};
        let mut scene = lit_with(SurfaceKind::Glass, SurfaceKind::Matte);
        let id = scene.things.values().find(|t| t.kind == "shape").map(|t| t.id).expect("a shape");
        let look = scene.card_mut(id, SlotId::Look).unwrap();
        look.params.set("light_pipe", Value::Bool(true));
        look.params.set("pipe_carries", Value::Number(300.0));
        let frame = frame_of(&scene);
        let lit = frame.light.expect("a lit frame");
        assert_eq!(lit.pipes.len(), 1);
        let pipe = &lit.pipes[0];
        assert!(pipe.points.len() >= 3 && pipe.carries == 300.0 && pipe.bends == 1.5, "{pipe:?}");
        assert!(pipe.tint.iter().cloned().fold(0.0, f32::max) == 1.0);
        // The room's lamp is near: the pipe glows with strip lights of its own.
        assert!(lit.lights.iter().filter(|l| l.p0[3] == 4.0).count() > 0, "pipe glow strips");
    }
}
