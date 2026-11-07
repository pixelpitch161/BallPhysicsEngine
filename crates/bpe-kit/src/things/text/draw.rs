//! Drawing a Text: each letter one shape instance (`Frame::push_letter`), all sharing the text's
//! middle, turn and one set of layer records from its `look.shape` card, so a gradient runs across
//! the whole text and every shape layer and sub-layer works on it.

use std::sync::Arc;

use bpe_core::{Blend, Params, ResolvedThing, Scene, Settings, SlotId, ThingId};
use bpe_gfx::{DrawCx, Frame, PathInstance, ThingDraw, thing_code};
use bpe_runtime::{Pose, World};
use glam::Vec2;

use super::{KIND, TextPath, TextSettings};
use crate::modules::look::shape::ShapeStack;
use crate::modules::look::shape::gpu::Geometry;
use crate::text::{PathAt, Placed, TextLayout, arrange, fonts, layout, placed};
use crate::things::shape::draw::{BODY_WHITE, shape_hits, wall_form};

/// A Text at a moment: where it sits, its letters laid out and placed along its path, and what is
/// wrong with its font, if anything.
#[derive(Clone, Debug)]
pub struct TextNow {
    pub pose: Pose,
    pub layout: Arc<TextLayout>,
    /// Each letter's place in the text's own frame (as `layout.letters`).
    pub placed: Vec<Placed>,
    pub font_problem: Option<String>,
}

impl TextNow {
    /// Letter `k`'s contours in the text's own frame.
    pub fn contours(&self, k: usize) -> Vec<Vec<Vec2>> {
        placed(&self.layout.letters[k], self.placed[k])
    }

    /// The bounds of every placed letter in the text's own frame; `None` with no letters.
    pub fn bounds(&self) -> Option<(Vec2, Vec2)> {
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for k in 0..self.layout.letters.len() {
            for p in self.contours(k).iter().flatten() {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
        }
        (lo.x <= hi.x).then_some((lo, hi))
    }

    /// A point of the text's own frame in the world (or on the screen, for a screen text).
    pub fn to_world(&self, p: Vec2) -> Vec2 {
        self.pose.position + Vec2::from_angle(self.pose.angle).rotate(p)
    }

    /// A point of the world in the text's own frame.
    pub fn to_own(&self, p: Vec2) -> Vec2 {
        Vec2::from_angle(-self.pose.angle).rotate(p - self.pose.position)
    }
}

/// How many points a round shape's edge is cut into for a text to run along it.
const ROUND_EDGE_POINTS: usize = 256;

/// The Text `id` at `time`: `tree` the saved scene (its fonts), `params` its settings at that
/// moment and `world` the world there. The one way the draw and the editor see a Text.
pub fn text_now(tree: &Scene, params: &Params, world: &World, id: ThingId, time: f64) -> Option<TextNow> {
    let s = TextSettings::from_params(params);
    let found = fonts::find(&tree.fonts, &s.font, s.weight);
    let layout = layout(found.font(), &s.layout_spec());
    let pose = world.poses.get(&id).copied().unwrap_or_else(|| world.anchor(id));
    let path = match s.path {
        TextPath::Straight => PathAt::Straight,
        TextPath::RoundACircle => {
            PathAt::Circle { radius: s.radius, start: s.start_angle.to_radians(), inside: s.inside }
        }
        TextPath::AlongAWave => PathAt::Wave {
            height: s.wave_height,
            wavelength: s.wavelength,
            shift: s.wave_shift + s.wave_speed * time as f32,
            follow: s.follow_slope,
        },
        TextPath::AlongAShape => match edge_of(world, &s, id, pose, on_screen(tree, id)) {
            Some(points) => PathAt::Edge { points, start: s.along_start, gap: s.gap },
            None => PathAt::Straight,
        },
    };
    let placed = arrange(&layout, &path);
    Some(TextNow { pose, layout, placed, font_problem: found.problem() })
}

/// Whether Text `id` sits on the screen (a Text does unless it is set in the world).
fn on_screen(tree: &Scene, id: ThingId) -> bool {
    tree.things.get(&id).is_none_or(|t| t.screen.unwrap_or(true))
}

/// The edge of the first shape `s.along` names (not the text itself), in the text's own frame
/// at `pose`, going clockwise from its first corner. A text on the `screen` sees the shape where
/// the camera shows it.
fn edge_of(world: &World, s: &TextSettings, id: ThingId, pose: Pose, screen: bool) -> Option<Vec<Vec2>> {
    let wall = world.matching(&s.along, id).into_iter().filter(|t| *t != id).find_map(|t| world.walls.get(&t))?;
    let form = wall_form(wall);
    let unit: Vec<Vec2> = match (&form.points, form.ellipse) {
        (Some(points), _) => points.clone(),
        (None, axes) => {
            let axes = axes.unwrap_or(Vec2::ONE);
            (0..ROUND_EDGE_POINTS)
                .map(|k| {
                    let a = k as f32 / ROUND_EDGE_POINTS as f32 * std::f32::consts::TAU;
                    Vec2::new(a.sin() * axes.x, -a.cos() * axes.y)
                })
                .collect()
        }
    };
    let (turn, back) = (Vec2::from_angle(form.turn), Vec2::from_angle(-pose.angle));
    let seen = |p: Vec2| if screen { world.camera.to_screen(p) } else { p };
    Some(
        unit.into_iter()
            .map(|p| back.rotate(seen(form.center + turn.rotate(p * form.scale)) - pose.position))
            .collect(),
    )
}

pub struct TextDraw;

impl ThingDraw for TextDraw {
    fn kind(&self) -> &'static str {
        KIND
    }

    /// Like a shape: a blended Text never tints the balls in front of it.
    fn behind(&self) -> bool {
        true
    }

    fn blend(&self, resolved: &ResolvedThing) -> Option<(Blend, f32, bool)> {
        ShapeStack::of(resolved.slots.get(&SlotId::Look)?).map(|s| s.whole_blend())
    }

    fn draw(&self, cx: &DrawCx, frame: &mut Frame) {
        let part = stack_of(cx).and_then(|s| s.split()).unwrap_or(0);
        draw_part(cx, frame, part);
    }

    fn draw_over(&self, cx: &DrawCx, frame: &mut Frame) {
        if let Some(cut) = stack_of(cx).and_then(|s| s.split()) {
            draw_part(cx, frame, cut | OVER);
        }
    }

    /// Its surface (and its layers' own) for a lit frame, as a shape's.
    fn light(&self, cx: &DrawCx, scene: &mut bpe_gfx::LightScene) {
        use crate::modules::look::surface;
        let Some(look) = cx.resolved.slots.get(&SlotId::Look).filter(|l| surface::has_surface(&l.kind)) else {
            return;
        };
        let index = surface::surface_index(cx.scene, cx.thing.id);
        let s = surface::of_card(look);
        scene.set_surface(index, s.gpu(bpe_gfx::SurfaceGpu::SHAPE));
        scene.refracts |= s.see_through();
        surface::Stamps::of(cx.scene, cx.thing.id, look).fill(scene, bpe_gfx::SurfaceGpu::SHAPE);
    }
}

/// The bit of an instance's `layers[3]` that draws the layers above its Blend layer (as shapes').
const OVER: u32 = 0x1_0000;

fn stack_of(cx: &DrawCx) -> Option<ShapeStack> {
    ShapeStack::of(cx.resolved.slots.get(&SlotId::Look)?)
}

/// Draws the Text: every letter, wearing its Look's layers whole (`part` 0), or the layers under
/// or over its Blend layer.
fn draw_part(cx: &DrawCx, frame: &mut Frame, part: u32) {
    let Some(look) = cx.resolved.slots.get(&SlotId::Look) else { return };
    let Some(stack) = ShapeStack::of(look) else { return };
    let Some(now) = text_now(cx.tree, &cx.resolved.params, cx.world, cx.thing.id, cx.time) else { return };
    let letters: Vec<Vec<Vec<Vec2>>> = (0..now.layout.letters.len()).map(|k| now.contours(k)).collect();
    if letters.iter().all(Vec::is_empty) {
        return;
    }
    // One set of records for the whole text, laid over every letter's points (so a gradient runs
    // across the words).
    let points: Vec<Vec2> = letters.iter().flatten().flatten().copied().collect();
    let geometry = Geometry { points: Some(points), ellipse: None, half: 0.0 };
    let mut records = stack.records(&geometry, &mut frame.gradients);
    let link = crate::things::light::links::number_of(cx.scene, cx.thing.id);
    crate::modules::look::layers::lighting::stamp(&mut records, link, frame.light.as_mut());
    use crate::modules::look::surface;
    surface::Stamps::of(cx.scene, cx.thing.id, look).apply(&mut records, |layer| stack.record_of(layer));
    if surface::has_surface(&look.kind)
        && let Some(body) = records.first_mut()
    {
        let lens = surface::of_card(look).lens;
        body.light[2] = if lens.is_finite() { lens.max(0.0) } else { 0.0 };
    }
    let first = frame.look_layers.len() as u32;
    let count = records.len() as u32;
    frame.look_layers.extend(records);
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
    let past = stack.reach_past();
    let margin = past + 2.0;
    let code = thing_code(cx.thing.id);
    let pushed: Vec<usize> = letters
        .iter()
        .filter_map(|c| frame.push_letter(now.pose.position, now.pose.angle, c, margin, BODY_WHITE, code))
        .collect();
    // Layers running from the middle measure out to the whole text's reach.
    let reach = pushed.iter().map(|&i| frame.paths[i].c[1]).fold(0.0, f32::max);
    for i in pushed {
        let path = &mut frame.paths[i];
        path.layers = [first, count, program, part];
        path.c[2] = past;
        path.c[3] = reach;
        path.hit = hit;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Value, resolve};
    use bpe_gfx::build_frame;
    use bpe_runtime::Log;

    fn scene_with(words: &str) -> (Scene, ThingId) {
        let reg = crate::registry();
        let mut scene = Scene::empty(&reg);
        let id = crate::add_thing(&mut scene, &reg, KIND, ThingId::ROOT).unwrap();
        scene.things.get_mut(&id).unwrap().params.set("words", Value::Text(words.into()));
        (scene, id)
    }

    fn frame_at(scene: &Scene, time: f64) -> Frame {
        let kit = crate::kit();
        let resolved = resolve(scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let mut world = World::build(scene, &resolved, &kit.sims);
        let mut log = Log::default();
        world.advance_to(time, &kit.sims, &mut log);
        let mut frame = Frame::default();
        build_frame(scene, &resolved, &world, &log, time, &kit.draws, &mut frame);
        frame
    }

    /// The letters' paths: the screen's, or the world's for a text in the world.
    fn paths(frame: &Frame) -> Vec<PathInstance> {
        frame.paths.iter().chain(frame.overlay.iter().flat_map(|o| o.paths.iter())).copied().collect()
    }

    #[test]
    fn a_text_draws_one_shape_per_letter() {
        let (scene, _) = scene_with("HI");
        let frame = frame_at(&scene, 0.0);
        let ps = paths(&frame);
        assert_eq!(ps.len(), 2);
        assert_eq!((ps[0].a, ps[0].layers), (ps[1].a, ps[1].layers), "the same middle, turn and records");
        assert_eq!(ps[0].clip[1], 4.0);
        let (scene, _) = scene_with("A B C");
        assert_eq!(paths(&frame_at(&scene, 0.0)).len(), 3);
    }

    #[test]
    fn an_empty_text_draws_nothing() {
        for words in ["", "  \n "] {
            let (scene, _) = scene_with(words);
            assert!(paths(&frame_at(&scene, 0.0)).is_empty(), "{words:?}");
        }
    }

    #[test]
    fn a_text_with_no_look_draws_nothing() {
        let (mut scene, id) = scene_with("HI");
        scene.things.get_mut(&id).unwrap().slots.remove(&SlotId::Look);
        assert!(paths(&frame_at(&scene, 0.0)).is_empty());
    }

    #[test]
    fn the_records_are_shared_and_the_gradient_spans_the_word() {
        use crate::modules::look::shape as look;
        let (mut scene, id) = scene_with("WIDE");
        scene.things.get_mut(&id).unwrap().screen = Some(false);
        let reg = crate::registry();
        scene.add_layer(&reg, id, SlotId::Look, look::PAINT).unwrap();
        let frame = frame_at(&scene, 0.0);
        assert_eq!(frame.paths.len(), 4);
        assert_eq!(frame.look_layers.len(), 2, "the body and the paint, once for every letter");
        assert!(frame.paths.iter().all(|p| p.layers == frame.paths[0].layers && p.layers[1] == 2));
        let reach = frame.paths.iter().map(|p| p.c[1]).fold(0.0, f32::max);
        assert!(frame.paths.iter().all(|p| p.c[3] == reach), "from the middle out to the whole word's reach");
        // The gradient's Across runs over the whole word: the extent it was laid over covers the
        // first letter's left and the last letter's right.
        let s = TextSettings::from_params(&resolve(&scene, &reg).things[&id].params);
        let l = layout(fonts::find(&scene.fonts, &s.font, s.weight).font(), &s.layout_spec());
        let geometry = Geometry {
            points: Some(l.letters.iter().flat_map(|x| x.contours.iter().flatten().map(|p| *p + x.origin)).collect()),
            ellipse: None,
            half: 0.0,
        };
        let (lo, hi) = geometry.extent(Vec2::X);
        assert!(lo <= l.letters[0].min.x + 1e-3 && hi >= l.letters[3].max.x - 1e-3);
    }

    #[test]
    fn a_screen_text_draws_into_the_overlay() {
        let (mut scene, id) = scene_with("HI");
        let frame = frame_at(&scene, 0.0);
        assert!(frame.paths.is_empty());
        assert_eq!(frame.overlay.as_ref().map(|o| o.paths.len()), Some(2));
        scene.things.get_mut(&id).unwrap().screen = Some(false);
        let frame = frame_at(&scene, 0.0);
        assert_eq!(frame.paths.len(), 2);
    }

    #[test]
    fn spin_turns_the_pose() {
        let (mut scene, id) = scene_with("HI");
        scene.things.get_mut(&id).unwrap().params.set("spin", Value::Number(90.0));
        let kit = crate::kit();
        let resolved = resolve(&scene, &kit.registry);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        world.advance_to(1.0, &kit.sims, &mut Log::default());
        let angle = world.poses[&id].angle;
        assert!((angle - std::f32::consts::FRAC_PI_2).abs() < 1e-3, "{angle}");
        let frame = frame_at(&scene, 1.0);
        assert!((paths(&frame)[0].a[2] - std::f32::consts::FRAC_PI_2).abs() < 1e-3);
    }

    #[test]
    fn edge_path_follows_a_shape() {
        let reg = crate::registry();
        let (mut scene, id) = scene_with("ROUND THE EDGE");
        let shape = crate::add_thing(&mut scene, &reg, crate::things::shape::KIND, ThingId::ROOT).unwrap();
        scene.things.get_mut(&shape).unwrap().params = crate::ShapeSettings::circle(300.0, 4.0).to_params();
        scene.things.get_mut(&shape).unwrap().transform.position = Vec2::new(40.0, -60.0);
        let t = scene.things.get_mut(&id).unwrap();
        t.screen = Some(false);
        t.transform.position = Vec2::new(-100.0, 30.0);
        t.params.set("path", Value::Choice("along_a_shape".into()));
        t.params.set("gap", Value::Number(0.0));
        t.params.set("size", Value::Number(40.0));
        let kit = crate::kit();
        let resolved = resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let now = text_now(&scene, &resolved.things[&id].params, &world, id, 0.0).unwrap();
        assert_eq!(now.placed.len(), 12);
        for (letter, p) in now.layout.letters.iter().zip(&now.placed) {
            let middle = now.to_world(p.at + Vec2::from_angle(p.turn) * (letter.advance * 0.5));
            let off = (middle - Vec2::new(40.0, -60.0)).length() - 300.0;
            assert!(off.abs() < 1.0, "'{}' is {off} px off the circle", letter.ch);
        }
    }

    #[test]
    fn a_missing_font_falls_back_and_says_so() {
        let (mut scene, id) = scene_with("HI");
        scene.things.get_mut(&id).unwrap().params.set("font", Value::Text("Nope".into()));
        let kit = crate::kit();
        let resolved = resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let now = text_now(&scene, &resolved.things[&id].params, &world, id, 0.0).unwrap();
        assert_eq!(now.layout.letters.len(), 2, "drawn in the fallback");
        assert!(now.font_problem.unwrap().contains("not found"));
        assert_eq!(paths(&frame_at(&scene, 0.0)).len(), 2);
    }
}
