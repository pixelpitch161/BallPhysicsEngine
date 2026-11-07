//! Drawing a Text: each letter one shape instance (`Frame::push_letter`), all sharing the text's
//! middle, turn and one set of layer records from its `look.shape` card, so a gradient runs across
//! the whole text and every shape layer and sub-layer works on it. Text part 2: the words' values
//! are read at the moment drawn, letters show and move as the Show settings say, a Text inside a
//! group of balls draws a copy on each, and fast letters read a distance tile
//! (`Frame::push_glyph`) instead of walking their outline.

use std::sync::Arc;

use bpe_core::{Blend, Params, Resolved, ResolvedThing, Scene, Settings, SlotId, ThingId};
use bpe_gfx::{DrawCx, Frame, PathInstance, ThingDraw, thing_code};
use bpe_runtime::{Log, Pose, World};
use glam::Vec2;

use super::show::{LetterMove, appear_times, letter_move, shows_whole, unit_of};
use super::{FastLetters, KIND, TextPath, TextRides, TextSettings};
use crate::modules::look::shape::ShapeStack;
use crate::modules::look::shape::gpu::Geometry;
use crate::text::live::{self, BallNames, Filled, LiveCx, Piece};
use crate::text::{PathAt, Placed, TextLayout, arrange, fonts, layout, placed, tiles};
use crate::things::shape::draw::{BODY_WHITE, shape_hits, wall_form};

/// A Text at a moment: where it sits, its letters laid out and placed along its path, how each
/// shows, and what is wrong with its font, if anything. A Text riding every ball of a group is one
/// of these a ball.
#[derive(Clone, Debug)]
pub struct TextNow {
    pub pose: Pose,
    pub layout: Arc<TextLayout>,
    /// Each letter's place in the text's own frame (as `layout.letters`).
    pub placed: Vec<Placed>,
    pub font_problem: Option<String>,
    /// How each letter shows now (as `layout.letters`); `None` isn't shown.
    pub shown: Vec<Option<LetterMove>>,
    /// The ball this copy rides (thing, body row).
    pub ball: Option<(ThingId, usize)>,
}

impl TextNow {
    /// Letter `k`'s contours in the text's own frame as it shows now (moved and scaled by its
    /// Show move); none when it isn't shown.
    pub fn contours(&self, k: usize) -> Vec<Vec<Vec2>> {
        let Some(m) = self.shown.get(k).copied().flatten() else { return Vec::new() };
        let c = placed(&self.layout.letters[k], self.placed[k]);
        if m.is_whole() {
            return c;
        }
        let mid = self.middle(k);
        c.into_iter().map(|c| c.into_iter().map(|p| mid + (p - mid) * m.scale + m.offset).collect()).collect()
    }

    /// The middle of letter `k`'s box in the text's own frame, as placed (before its move).
    pub fn middle(&self, k: usize) -> Vec2 {
        let l = &self.layout.letters[k];
        let p = self.placed[k];
        p.at + Vec2::from_angle(p.turn).rotate(0.5 * (l.min + l.max) - l.origin)
    }

    /// The bounds of every letter as placed (shown now or not) in the text's own frame; `None`
    /// with no letters.
    pub fn bounds(&self) -> Option<(Vec2, Vec2)> {
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        for k in 0..self.layout.letters.len() {
            for p in placed(&self.layout.letters[k], self.placed[k]).iter().flatten() {
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

/// What a Text is read with at a moment.
#[derive(Clone, Copy)]
pub struct TextAt<'a> {
    /// The scene as saved (its fonts, names and tree).
    pub tree: &'a Scene,
    /// The scene resolved at this moment, for the balls' colours (their names); `None` names balls
    /// by number.
    pub scene: Option<&'a Resolved>,
    /// The Text's settings at this moment.
    pub params: &'a Params,
    pub world: &'a World,
    pub log: Option<&'a Log>,
    pub id: ThingId,
    /// Seconds: the video's clock.
    pub time: f64,
}

impl<'a> TextAt<'a> {
    /// Text `id` with settings `params` at `time`, without the log (no hits seen) or the resolved
    /// scene (balls named by number).
    pub fn new(tree: &'a Scene, params: &'a Params, world: &'a World, id: ThingId, time: f64) -> Self {
        TextAt { tree, scene: None, params, world, log: None, id, time }
    }
}

/// The Text at `at` (its first copy, for one riding several balls): the one way the editor sees a
/// Text. `None` when it rides balls and none are there.
pub fn text_now(at: &TextAt) -> Option<TextNow> {
    text_copies(at).into_iter().next()
}

/// Every copy of the Text at `at`: one, or one a ball for a Text riding every ball of its group.
pub fn text_copies(at: &TextAt) -> Vec<TextNow> {
    let s = TextSettings::from_params(at.params);
    let found = fonts::find(&at.tree.fonts, &s.font, s.weight);
    let screen = on_screen(at.tree, at.id);
    // A rule's words in place of its own; one line of them while a rule moves through its lines.
    let rule = at.world.rules().and_then(|r| r.text(at.id));
    let mut words = rule.map_or_else(|| s.words.clone(), |r| r.words_for(&s.words));
    let mut rule_line = None;
    if let Some(r) = rule.filter(|r| !r.nexts.is_empty()) {
        words = words.split('\n').nth(r.nexts.len() - 1).unwrap_or("").to_string();
        rule_line = r.nexts.last().copied();
    }
    let pieces = if words.contains('{') { live::parse(&words) } else { Vec::new() };
    let reads = pieces.iter().any(|p| matches!(p, Piece::Value(..)));
    let names = if reads && needs_names(&pieces) { ball_names(at) } else { BallNames::default() };
    let own = at.world.poses.get(&at.id).copied().unwrap_or_else(|| at.world.anchor(at.id));
    let mut times = Vec::new();
    riding_places(at, &s, screen, own)
        .into_iter()
        .map(|(pose, ball)| {
            let filled = if reads {
                let cx = LiveCx {
                    tree: at.tree,
                    world: at.world,
                    log: at.log,
                    time: at.time,
                    me: at.id,
                    ball,
                    names: Some(&names),
                    resort: s.resort,
                };
                live::fill(&pieces, &cx)
            } else {
                Filled { words: words.clone(), moves: Vec::new() }
            };
            let layout = layout(found.font(), &s.layout_spec_of(&filled.words));
            let path = path_of(at, &s, pose, screen);
            let mut placed = arrange(&layout, &path);
            // Standings lines on their way to their new places.
            if !filled.moves.is_empty() {
                for (l, p) in layout.letters.iter().zip(&mut placed) {
                    if let Some((_, off)) = filled.moves.iter().find(|(para, _)| *para == l.paragraph) {
                        p.at.y += off * layout.line_step;
                    }
                }
            }
            let shown = shown_of(at, &s, &layout, rule_line, &mut times);
            TextNow { pose, layout, placed, font_problem: found.problem(), shown, ball }
        })
        .collect()
}

/// Whether `pieces` read the balls' names.
fn needs_names(pieces: &[Piece]) -> bool {
    use live::Token;
    pieces.iter().any(|p| matches!(p, Piece::Value(Token::BallName | Token::Standings { .. }, _)))
}

/// Every ball's name from its colour, read through its thing's drawing.
fn ball_names(at: &TextAt) -> BallNames {
    let (Some(resolved), Some(log)) = (at.scene, at.log) else { return BallNames::default() };
    let mut out = BallNames::default();
    for &id in at.world.bodies.keys() {
        let (Some(thing), Some(r)) = (at.tree.things.get(&id), resolved.things.get(&id)) else { continue };
        let cx = DrawCx {
            thing,
            resolved: r,
            scene: resolved,
            world: at.world,
            log,
            time: at.time,
            tree: at.tree,
            light: None,
        };
        let Some(colour) = crate::things::balls::draw::ball_colors(&cx) else { continue };
        let named = BallNames::from_colours(at.world, &[id], &|_, row| Some(colour(row)));
        out.0.extend(named.0);
    }
    out
}

/// Where each copy sits: the Text's own pose, or, inside a group of balls, an emitter or a shape,
/// on what it rides (moved by its Position), with the ball each rides.
fn riding_places(at: &TextAt, s: &TextSettings, screen: bool, own: Pose) -> Vec<(Pose, Option<(ThingId, usize)>)> {
    let Some(thing) = at.tree.things.get(&at.id) else { return vec![(own, None)] };
    let parent = thing
        .parent
        .filter(|p| *p != ThingId::ROOT)
        .filter(|p| at.tree.things.get(p).is_some_and(|t| crate::things::light::RIDES.contains(&t.kind.as_str())));
    let Some(parent) = parent else { return vec![(own, None)] };
    // A group of balls that isn't there (switched off): no labels.
    let balls = at.tree.things.get(&parent).is_some_and(|t| matches!(t.kind.as_str(), "balls" | "emitter"));
    if balls && !at.world.bodies.contains_key(&parent) {
        return Vec::new();
    }
    let offset = thing.transform.position;
    let seen = |p: Vec2| if screen { at.world.camera.to_screen(p) } else { p };
    let pose_at = |p: Vec2| Pose { position: seen(p) + offset, ..own };
    if let Some(b) = at.world.bodies.get(&parent) {
        let alive = || (0..b.len()).filter(|&r| b.alive[r]);
        return match s.rides_on {
            TextRides::EachBall => alive().map(|r| (pose_at(b.position[r]), Some((parent, r)))).collect(),
            TextRides::BallNumber => b
                .row_of(s.ball_number.saturating_sub(1))
                .filter(|&r| b.alive[r])
                .map(|r| (pose_at(b.position[r]), Some((parent, r))))
                .into_iter()
                .collect(),
            TextRides::TheMiddle => {
                let n = alive().count();
                if n == 0 {
                    return Vec::new();
                }
                let sum = alive().fold(Vec2::ZERO, |a, r| a + b.position[r]);
                vec![(pose_at(sum / n as f32), None)]
            }
        };
    }
    if let Some(wall) = at.world.walls.get(&parent) {
        return vec![(pose_at(crate::things::light::draw::outline_centre(&wall.outline)), None)];
    }
    vec![(own, None)]
}

/// How each letter of `layout` shows at `at`'s moment; `rule_line` the moment a rule showed the
/// line laid out, when one did.
/// `times` keeps the units' moments worked out for an earlier copy (they don't depend on the ball).
fn shown_of(
    at: &TextAt,
    s: &TextSettings,
    layout: &TextLayout,
    rule_line: Option<f64>,
    times: &mut Vec<Option<f64>>,
) -> Vec<Option<LetterMove>> {
    let n = layout.letters.len();
    if let Some(t) = rule_line {
        let m = letter_move(s, Some(t), at.time, s.size);
        return vec![m; n];
    }
    if shows_whole(s) {
        return vec![Some(LetterMove::WHOLE); n];
    }
    let units = layout.letters.iter().map(|l| unit_of(l, s.show) as usize + 1).max().unwrap_or(0);
    if times.len() < units {
        *times = appear_times(s, units, at.world, at.log, at.id);
    }
    layout
        .letters
        .iter()
        .map(|l| letter_move(s, times.get(unit_of(l, s.show) as usize).copied().flatten(), at.time, s.size))
        .collect()
}

/// The path the letters follow for a copy at `pose`.
fn path_of(at: &TextAt, s: &TextSettings, pose: Pose, screen: bool) -> PathAt {
    match s.path {
        TextPath::Straight => PathAt::Straight,
        TextPath::RoundACircle => {
            PathAt::Circle { radius: s.radius, start: s.start_angle.to_radians(), inside: s.inside }
        }
        TextPath::AlongAWave => PathAt::Wave {
            height: s.wave_height,
            wavelength: s.wavelength,
            shift: s.wave_shift + s.wave_speed * at.time as f32,
            follow: s.follow_slope,
        },
        TextPath::AlongAShape => match edge_of(at.world, s, at.id, pose, screen) {
            Some(points) => PathAt::Edge { points, start: s.along_start, gap: s.gap },
            None => PathAt::Straight,
        },
    }
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

/// Whether a Text with settings `s` and `copies` draws fast letters.
fn fast_letters(s: &TextSettings, copies: &[TextNow]) -> bool {
    match s.fast {
        FastLetters::Always => true,
        FastLetters::Never => false,
        FastLetters::Automatic => {
            (s.rides_on == TextRides::EachBall && copies.iter().any(|c| c.ball.is_some()))
                || !live::standings_in(&s.words).is_empty()
        }
    }
}

/// One letter to push: its copy and its index there.
struct Shown {
    copy: usize,
    letter: usize,
}

/// Draws the Text: every letter of every copy, wearing its Look's layers whole (`part` 0), or the
/// layers under or over its Blend layer.
fn draw_part(cx: &DrawCx, frame: &mut Frame, part: u32) {
    let Some(look) = cx.resolved.slots.get(&SlotId::Look) else { return };
    let Some(stack) = ShapeStack::of(look) else { return };
    let at = TextAt {
        tree: cx.tree,
        scene: Some(cx.scene),
        params: &cx.resolved.params,
        world: cx.world,
        log: Some(cx.log),
        id: cx.thing.id,
        time: cx.time,
    };
    let copies = text_copies(&at);
    let s = TextSettings::from_params(&cx.resolved.params);
    let fast = fast_letters(&s, &copies);
    let font = fonts::find(&cx.tree.fonts, &s.font, s.weight).font().clone();
    let shown: Vec<Shown> = copies
        .iter()
        .enumerate()
        .flat_map(|(copy, now)| {
            (0..now.layout.letters.len())
                .filter(move |&k| now.shown[k].is_some())
                .map(move |letter| Shown { copy, letter })
        })
        .collect();
    // Fast letters lay the gradient over their boxes; drawn ones over their outlines.
    let outlines: Vec<Vec<Vec<Vec2>>> =
        if fast { Vec::new() } else { shown.iter().map(|l| copies[l.copy].contours(l.letter)).collect() };
    let points: Vec<Vec2> = if fast {
        shown
            .iter()
            .flat_map(|l| {
                let now = &copies[l.copy];
                let (letter, p) = (&now.layout.letters[l.letter], now.placed[l.letter]);
                let turn = Vec2::from_angle(p.turn);
                let (a, b) = (letter.min - letter.origin, letter.max - letter.origin);
                [a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)].map(|q| p.at + turn.rotate(q))
            })
            .collect()
    } else {
        outlines.iter().flatten().flatten().copied().collect()
    };
    if points.is_empty() {
        return;
    }
    // One set of records for the whole text, laid over every letter's points (so a gradient runs
    // across the words).
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
    let tile_scale = s.size / tiles::TILE_CAP;
    // Each pushed instance with how much of it shows and how far its layers may reach past it.
    let mut pushed: Vec<(usize, f32, f32)> = Vec::new();
    for (i, l) in shown.iter().enumerate() {
        let now = &copies[l.copy];
        let m = now.shown[l.letter].unwrap_or(LetterMove::WHOLE);
        let letter = &now.layout.letters[l.letter];
        let glyph = fast.then(|| tiles::tile_for(&font, letter.glyph, s.weight, s.slant)).flatten();
        let tile = glyph.as_ref().and_then(|t| frame.mask_tile(t.hash, &t.bytes).map(|i| (t, i)));
        let index = match tile {
            Some((t, index)) => {
                let p = now.placed[l.letter];
                let mid = now.middle(l.letter);
                let centre = p.at + Vec2::from_angle(p.turn).rotate(t.centre * tile_scale);
                let centre = mid + (centre - mid) * m.scale + m.offset;
                let half = t.half * tile_scale * m.scale;
                let reach = half * tiles::reach_share();
                frame
                    .push_glyph(now.pose.position, now.pose.angle, centre, p.turn, half, index, BODY_WHITE, code)
                    .map(|k| (k, past.min(reach)))
            }
            None => {
                let contours = if fast { now.contours(l.letter) } else { outlines[i].clone() };
                frame
                    .push_letter(now.pose.position, now.pose.angle, &contours, margin, BODY_WHITE, code)
                    .map(|k| (k, past))
            }
        };
        if let Some((k, reach_past)) = index {
            pushed.push((k, m.opacity, reach_past));
        }
    }
    // Layers running from the middle measure out to the whole text's reach.
    let reach = pushed.iter().map(|&(i, _, _)| frame.paths[i].c[1]).fold(0.0, f32::max);
    for (i, opacity, reach_past) in pushed {
        let path = &mut frame.paths[i];
        path.layers = [first, count, program, part];
        path.c[2] = reach_past;
        path.c[3] = reach;
        path.hit = hit;
        if opacity < 1.0 {
            path.clip[2] = (1.0 - opacity).clamp(0.0, 1.0);
        }
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
        let now = text_now(&TextAt::new(&scene, &resolved.things[&id].params, &world, id, 0.0)).unwrap();
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
        let now = text_now(&TextAt::new(&scene, &resolved.things[&id].params, &world, id, 0.0)).unwrap();
        assert_eq!(now.layout.letters.len(), 2, "drawn in the fallback");
        assert!(now.font_problem.unwrap().contains("not found"));
        assert_eq!(paths(&frame_at(&scene, 0.0)).len(), 2);
    }

    /// The letters' characters of every copy of Text `id` at `time`.
    fn words_of(scene: &Scene, id: ThingId, time: f64) -> Vec<String> {
        let kit = crate::kit();
        let resolved = resolve(scene, &kit.registry);
        let mut world = World::build(scene, &resolved, &kit.sims);
        let mut log = Log::default();
        world.advance_to(time, &kit.sims, &mut log);
        let at = TextAt {
            scene: Some(&resolved),
            log: Some(&log),
            ..TextAt::new(scene, &resolved.things[&id].params, &world, id, time)
        };
        text_copies(&at).iter().map(|c| c.layout.letters.iter().map(|l| l.ch).collect()).collect()
    }

    #[test]
    fn values_in_the_words_are_read_as_it_plays() {
        let (scene, id) = scene_with("T{time}");
        assert_eq!(words_of(&scene, id, 0.0), ["T0.0"]);
        assert_eq!(words_of(&scene, id, 1.5), ["T1.5"]);
        // A text without braces is laid out as typed (part 1).
        let (scene, id) = scene_with("HI");
        assert_eq!(words_of(&scene, id, 1.0), ["HI"]);
    }

    /// A ring of `count` balls with a Text inside it saying `words`.
    fn riding(words: &str, count: u32) -> (Scene, ThingId, ThingId) {
        let reg = crate::registry();
        let mut scene = Scene::empty(&reg);
        let balls = crate::add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        let mut b = crate::BallsSettings::from_params(&scene.things[&balls].params);
        b.ring(count, 200.0);
        scene.things.get_mut(&balls).unwrap().params = b.to_params();
        let id = crate::add_thing(&mut scene, &reg, KIND, balls).unwrap();
        let t = scene.things.get_mut(&id).unwrap();
        t.params.set("words", Value::Text(words.into()));
        t.screen = Some(false);
        (scene, balls, id)
    }

    #[test]
    fn a_text_inside_a_group_rides_each_ball() {
        let (scene, balls, id) = riding("{ball number}", 3);
        assert_eq!(words_of(&scene, id, 0.0), ["1", "2", "3"]);
        let frame = frame_at(&scene, 0.0);
        assert_eq!(frame.paths.len(), 3, "a label on each ball");
        assert!(frame.paths.iter().all(|p| p.b[1] == bpe_gfx::PATH_GLYPH), "riding each ball draws fast letters");
        // Each sits on its ball.
        let kit = crate::kit();
        let resolved = resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let copies = text_copies(&TextAt::new(&scene, &resolved.things[&id].params, &world, id, 0.0));
        let b = &world.bodies[&balls];
        for (c, p) in copies.iter().zip(&b.position) {
            assert!((c.pose.position - *p).length() < 1e-3);
        }
        // The middle: one copy.
        let mut scene = scene;
        scene.things.get_mut(&id).unwrap().params.set("rides_on", Value::Choice("the_middle".into()));
        assert_eq!(words_of(&scene, id, 0.0).len(), 1);
    }

    #[test]
    fn fast_letters_follow_the_setting() {
        let (mut scene, id) = scene_with("HI");
        let frame = frame_at(&scene, 0.0);
        assert!(paths(&frame).iter().all(|p| p.b[1] != bpe_gfx::PATH_GLYPH && p.clip[2] == 0.0), "part 1's letters");
        scene.things.get_mut(&id).unwrap().params.set("fast", Value::Choice("always".into()));
        let frame = frame_at(&scene, 0.0);
        let ps = paths(&frame);
        assert_eq!(ps.len(), 2);
        assert!(ps.iter().all(|p| p.b[1] == bpe_gfx::PATH_GLYPH));
        assert_eq!(frame.overlay.as_ref().unwrap().mask_tiles.len(), 2, "a tile a glyph, in the overlay's own atlas");
    }

    #[test]
    fn letters_show_one_by_one_and_fade_in() {
        let (mut scene, id) = scene_with("ABCD");
        let t = scene.things.get_mut(&id).unwrap();
        t.params.set("show", Value::Choice("letter_by_letter".into()));
        t.params.set("each", Value::Number(0.5));
        assert_eq!(paths(&frame_at(&scene, 0.0)).len(), 1);
        assert_eq!(paths(&frame_at(&scene, 0.6)).len(), 2);
        assert_eq!(paths(&frame_at(&scene, 5.0)).len(), 4);
        let t = scene.things.get_mut(&id).unwrap();
        t.params.set("appear", Value::Choice("fade".into()));
        t.params.set("move_time", Value::Number(0.4));
        let ps = paths(&frame_at(&scene, 0.7));
        assert_eq!(ps.len(), 2);
        assert_eq!(ps[0].clip[2], 0.0, "the first is in");
        assert!(ps[1].clip[2] > 0.2 && ps[1].clip[2] < 0.8, "the second is half in: {}", ps[1].clip[2]);
    }

    #[test]
    fn a_rule_moves_a_text_through_its_lines() {
        let reg = crate::registry();
        let (mut scene, id) = scene_with("ONE\nTWO");
        for t in [0.1, 0.2, 0.3] {
            let rule = scene.add_rule("Next");
            let when = scene.add_block(&reg, rule, "when.time", None).unwrap();
            scene.set_block_param(&reg, rule, when, "at", Value::Number(t)).unwrap();
            let set = scene.add_block(&reg, rule, "do.text", Some(when)).unwrap();
            scene.set_block_param(&reg, rule, set, "how", Value::Choice("show_the_next_line".into())).unwrap();
        }
        assert_eq!(words_of(&scene, id, 0.05), ["ONETWO"], "its own words before the first");
        assert_eq!(words_of(&scene, id, 0.15), ["ONE"]);
        assert_eq!(words_of(&scene, id, 0.25), ["TWO"]);
        assert_eq!(words_of(&scene, id, 0.35), [""], "past the last line nothing shows");
    }

    #[test]
    fn standings_measure_from_where_each_ball_started() {
        let reg = crate::registry();
        let mut scene = crate::new_scene(&reg);
        let balls = crate::add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
        let mut b = crate::BallsSettings::from_params(&scene.things[&balls].params);
        b.ring(3, 200.0);
        scene.things.get_mut(&balls).unwrap().params = b.to_params();
        let id = crate::add_thing(&mut scene, &reg, KIND, ThingId::ROOT).unwrap();
        scene.things.get_mut(&id).unwrap().params.set("words", Value::Text("{standings down 3}".into()));
        let kit = crate::kit();
        let resolved = resolve(&scene, &kit.registry);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        world.advance_to(1.0, &kit.sims, &mut log);
        let s = TextSettings::from_params(&resolved.things[&id].params);
        let filled = live::fill(
            &live::parse(&s.words),
            &LiveCx {
                tree: &scene,
                world: &world,
                log: Some(&log),
                time: 1.0,
                me: id,
                ball: None,
                names: None,
                resort: s.resort,
            },
        );
        let lines: Vec<&str> = filled.words.lines().collect();
        assert_eq!(lines.len(), 3, "{:?}", filled.words);
        // Measured from where each started a second ago, not one substep's worth; best first.
        let metres: Vec<f64> =
            lines.iter().map(|l| l.rsplit(' ').next().unwrap().trim_end_matches('m').parse().unwrap()).collect();
        assert!(metres.iter().any(|m| m.abs() > 0.3), "{:?}", filled.words);
        assert!(metres.windows(2).all(|w| w[0] >= w[1]), "{:?}", filled.words);
    }

    #[test]
    fn keyed_words_change_at_their_keys() {
        let reg = crate::registry();
        let (mut scene, id) = scene_with("A");
        let a = bpe_core::Address::own(id, "words");
        scene.set_key(&reg, &a, 0.0, Value::Text("A".into())).unwrap();
        scene.set_key(&reg, &a, 1.0, Value::Text("ABC".into())).unwrap();
        assert_eq!(paths(&frame_at(&scene, 0.9)).len(), 1, "held until the next key");
        assert_eq!(paths(&frame_at(&scene, 1.1)).len(), 3);
    }
}
