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
use super::{FastLetters, KIND, TextBalls, TextPath, TextRides, TextSettings};
use crate::modules::look::shape::ShapeStack;
use crate::modules::look::shape::gpu::Geometry;
use crate::modules::look::shape::units::{MOST_UNIT_RECORDS, MOST_UNIT_SETS, UnitLayer, UnitPlace, records_for_unit};
use crate::rules::breaks::{BreakLook, LookSettings};
use crate::rules::remains::draw::whole_look;
use crate::rules::remains::remains_now;
use crate::text::live::{self, BallNames, Filled, LiveCx, Piece};
use crate::text::{PathAt, Placed, TextLayout, arrange, fonts, layout, placed, tiles};
use crate::things::shape::draw::{BODY_WHITE, shape_hits, wall_form};
use bpe_runtime::Remains;

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
    /// Whether letter moves move its letters (a whole-text gradient then lies over them as laid out).
    pub moving: bool,
}

impl TextNow {
    /// Letter `k`'s contours in the text's own frame as it shows now (moved and scaled by its
    /// Show move); none when it isn't shown.
    pub fn contours(&self, k: usize) -> Vec<Vec<Vec2>> {
        let Some(m) = self.shown.get(k).copied().flatten() else { return Vec::new() };
        self.contours_moved(k, m)
    }

    /// Letter `k`'s contours in the text's own frame as placed (before any move).
    pub fn contours_placed(&self, k: usize) -> Vec<Vec<Vec2>> {
        placed(&self.layout.letters[k], self.placed[k])
    }

    /// Letter `k`'s contours in the text's own frame moved by `m` about its middle: scaled and
    /// stretched in the letter's own frame, turned, then moved (Draw on's trace isn't read here).
    pub fn contours_moved(&self, k: usize, m: LetterMove) -> Vec<Vec<Vec2>> {
        let c = placed(&self.layout.letters[k], self.placed[k]);
        if (LetterMove { trace: None, ..m }).is_whole() {
            return c;
        }
        let to = self.mover(k, m);
        c.into_iter().map(|c| c.into_iter().map(&to).collect()).collect()
    }

    /// Where a point of letter `k` as placed goes with move `m` (as `contours_moved`).
    pub fn mover(&self, k: usize, m: LetterMove) -> impl Fn(Vec2) -> Vec2 {
        let mid = self.middle(k);
        let own = Vec2::from_angle(self.placed[k].turn);
        let back = Vec2::new(own.x, -own.y);
        let turn = Vec2::from_angle(m.turn);
        let size = m.stretch * m.scale;
        // Even sizing needs no turn into the letter's own frame (and draws as before part 4).
        let even = size.x == size.y;
        move |p: Vec2| {
            let d = if even { (p - mid) * size } else { own.rotate(back.rotate(p - mid) * size) };
            mid + turn.rotate(d) + m.offset
        }
    }

    /// Letter `k`'s middle as it shows with move `m`.
    pub fn middle_moved(&self, k: usize, m: LetterMove) -> Vec2 {
        self.middle(k) + m.offset
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
    /// How far each sound is up at a moment, for a Follow sound move (Text part 4).
    pub sound: Option<&'a dyn bpe_runtime::SoundAt>,
    /// The Text's Look card, whose letter moves move its letters (Text part 4); `None` draws
    /// them as laid out and shown.
    pub look: Option<&'a bpe_core::ResolvedSlot>,
}

impl<'a> TextAt<'a> {
    /// Text `id` with settings `params` at `time`, without the log (no hits seen) or the resolved
    /// scene (balls named by number).
    pub fn new(tree: &'a Scene, params: &'a Params, world: &'a World, id: ThingId, time: f64) -> Self {
        TextAt { tree, scene: None, params, world, log: None, id, time, sound: None, look: None }
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
    let moves = at.look.map(super::moves::moves_of).unwrap_or_default();
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
            let moving = !moves.is_empty();
            let mut now = TextNow { pose, layout, placed, font_problem: found.problem(), shown, ball, moving };
            super::moves::apply(at, &moves, &mut now);
            now
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
            sound: at.sound,
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
    // A shape's edge: another Text's letters are no edge to run along.
    let wall = world
        .matching(&s.along, id)
        .into_iter()
        .filter(|t| *t != id)
        .filter_map(|t| world.walls.get(&t))
        .find(|w| !matches!(w.outline, bpe_runtime::Outline::Letters { .. }))?;
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
        let stamps = surface::Stamps::of(cx.scene, cx.thing.id, look);
        stamps.fill(scene, bpe_gfx::SurfaceGpu::SHAPE);
        // Glass letters of a Text set in the world bend lamp light as a glass shape does (glass and
        // light part 3): a bender over its letters' box, for its surface and each layer's own,
        // when that layer (the body for the whole Text's) blocks light.
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
            let points = crate::things::light::draw::outline_contours(&wall.outline).into_iter().flatten();
            let half = 0.5 * wall.outline.thickness().max(0.0);
            if let Some(round) = bpe_gfx::Bender::round(points, half + 2.0, 0) {
                scene.benders.extend(benders.into_iter().map(|index| bpe_gfx::Bender { index, ..round }));
            }
        }
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

/// One letter to push: its copy and its index there, how it shows now, and its Shatter while it
/// breaks into pieces.
struct Shown<'a> {
    copy: usize,
    letter: usize,
    moved: LetterMove,
    shatter: Option<(&'a Remains, LookSettings)>,
}

/// What a rule's Break left of a Text: the whole Text's look while it plays, and which letters are
/// still standing (a walled Text's parts).
struct Breaks<'a> {
    whole: Option<(&'a Remains, LookSettings)>,
    alive: Option<&'a [bool]>,
}

/// How one letter shows after a break.
enum LetterBreak<'a> {
    Standing,
    /// Broken, its look over (or Vanish).
    Gone,
    Look(&'a Remains, LookSettings),
}

impl<'a> Breaks<'a> {
    fn of(cx: &DrawCx<'a>) -> Self {
        Breaks { whole: remains_now(cx, None), alive: cx.world.parts.get(&cx.thing.id).map(|p| p.alive.as_slice()) }
    }

    /// How letter `k` of copy `copy` shows: a broken letter (only the first copy is a wall) its
    /// own look, else the whole Text's.
    fn letter(&self, cx: &DrawCx<'a>, copy: usize, k: usize) -> LetterBreak<'a> {
        if copy == 0 && self.alive.is_some_and(|a| a.get(k) == Some(&false)) {
            return match remains_now(cx, Some(k as u32)) {
                Some((r, l)) => LetterBreak::Look(r, l),
                None => LetterBreak::Gone,
            };
        }
        match &self.whole {
            Some((r, l)) => LetterBreak::Look(r, l.clone()),
            None => LetterBreak::Standing,
        }
    }
}

/// Each letter's hit data (as `shape_hits`) for the first copy of a Text whose letters are parts:
/// hits by a ball on that letter and a rule's flashes of it. `None` for a Text without parts,
/// where every letter shares the Text's.
fn letter_hits(cx: &DrawCx, letters: usize) -> Option<Vec<[f32; 4]>> {
    use bpe_runtime::EventKind;
    let id = cx.thing.id;
    cx.world.parts.get(&id)?;
    let rate = cx.world.steps_per_second;
    let mut hits = vec![bpe_gfx::NO_HIT; letters];
    for e in cx.log.since(0) {
        let item = match e.kind {
            EventKind::Hit => e.b.filter(|b| b.thing == id).map(|b| b.item),
            EventKind::Flash => (e.a.thing == id).then_some(e.a.item),
            _ => None,
        };
        let Some(item) = item else { continue };
        let time = e.time(rate);
        if time > cx.time {
            break;
        }
        if let Some(h) = hits.get_mut(item as usize) {
            *h = [(cx.time - time) as f32, crate::things::balls::draw::hit_strength(e.speed), h[2] + 1.0, 0.0];
        }
    }
    Some(hits)
}

/// Half the band a Text's letters draw as: half its Thickness while balls bounce inside its
/// letters (a walled Text), else 0 (filled).
fn band_half(cx: &DrawCx, s: &TextSettings) -> f32 {
    let walled = cx.world.states.get(&cx.thing.id).and_then(|st| st.as_any().downcast_ref::<super::TextState>());
    if s.balls == TextBalls::BounceInside && walled.is_some_and(|st| st.walled) && s.thickness.is_finite() {
        0.5 * s.thickness.max(0.0)
    } else {
        0.0
    }
}

/// The patched records for each of `shown` (as `outlines` lays them) whose units or worn layers
/// differ from the whole text's: `base` copied once a distinct unit into `layers`, at most
/// [`MOST_UNIT_SETS`] copies (the rest keep the whole records). Each letter's first record, `None`
/// for the whole text's.
fn unit_records(
    copies: &[TextNow],
    shown: &[Shown],
    outlines: &[Vec<Vec<Vec2>>],
    unit_layers: &[UnitLayer],
    base: &[bpe_gfx::LayerGpu],
    layers: &mut Vec<bpe_gfx::LayerGpu>,
) -> Vec<Option<u32>> {
    use crate::modules::look::shape::Across;
    use std::collections::HashMap;
    let by_letter = unit_layers.iter().any(|u| u.across == Across::EachLetter);
    let by_line = unit_layers.iter().any(|u| u.across == Across::EachLine);
    // Each line's points, middle (its box's) by (copy, line).
    let mut lines: HashMap<(usize, u32), Vec<Vec2>> = HashMap::new();
    if by_line {
        for (l, outline) in shown.iter().zip(outlines) {
            let line = copies[l.copy].layout.letters[l.letter].line;
            lines.entry((l.copy, line)).or_default().extend(outline.iter().flatten().copied());
        }
    }
    let box_middle = |ps: &[Vec2]| {
        let (lo, hi) =
            ps.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        if lo.x <= hi.x { 0.5 * (lo + hi) } else { Vec2::ZERO }
    };
    let line_middles: HashMap<(usize, u32), Vec2> = lines.iter().map(|(k, ps)| (*k, box_middle(ps))).collect();
    let mut sets: HashMap<(usize, usize, u32, Vec<bool>), u32> = HashMap::new();
    // At most MOST_UNIT_SETS sets, and fewer the more records a set has (past it, letters wear
    // the whole text's).
    let most_sets = MOST_UNIT_SETS.min((MOST_UNIT_RECORDS / base.len().max(1)).max(64));
    shown
        .iter()
        .zip(outlines)
        .map(|(l, outline)| {
            let now = &copies[l.copy];
            let number = l.letter as u32;
            let line = now.layout.letters[l.letter].line;
            let worn: Vec<bool> = unit_layers.iter().map(|u| u.only.is_empty() || u.only.contains(&number)).collect();
            let key = (
                if by_letter { l.copy } else { usize::MAX },
                if by_letter { l.letter } else { usize::MAX },
                if by_line { line } else { u32::MAX },
                worn,
            );
            // Every layer worn whole by every letter: the whole text's records.
            if !by_letter && !by_line && key.3.iter().all(|w| *w) {
                return None;
            }
            let key = if by_line && !by_letter { (l.copy, key.1, key.2, key.3) } else { key };
            if let Some(first) = sets.get(&key) {
                return Some(*first);
            }
            if sets.len() >= most_sets {
                return None;
            }
            let points: Vec<Vec2> = outline.iter().flatten().copied().collect();
            let letter = UnitPlace {
                points: &points,
                middle: now.middle_moved(l.letter, l.moved),
                turn: now.placed[l.letter].turn + l.moved.turn,
            };
            let line_points = lines.get(&(l.copy, line)).map_or(&[][..], |v| v.as_slice());
            let line_place = UnitPlace {
                points: line_points,
                middle: line_middles.get(&(l.copy, line)).copied().unwrap_or(Vec2::ZERO),
                turn: 0.0,
            };
            let first = layers.len() as u32;
            layers.extend(records_for_unit(base, unit_layers, number, &letter, &line_place));
            sets.insert(key, first);
            Some(first)
        })
        .collect()
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
        sound: cx.sound,
        look: Some(look),
    };
    let copies = text_copies(&at);
    let s = TextSettings::from_params(&cx.resolved.params);
    let fast = fast_letters(&s, &copies);
    let font = fonts::find(&cx.tree.fonts, &s.font, s.weight).font().clone();
    let breaks = Breaks::of(cx);
    let mut shown: Vec<Shown> = Vec::new();
    for (copy, now) in copies.iter().enumerate() {
        for letter in 0..now.layout.letters.len() {
            let Some(mut moved) = now.shown[letter] else { continue };
            let mut shatter = None;
            match breaks.letter(cx, copy, letter) {
                LetterBreak::Standing => {}
                LetterBreak::Gone => continue,
                LetterBreak::Look(r, l) => match whole_look(r, &l, cx.time) {
                    // Fade out and Pop: about the letter's middle.
                    Some(w) => {
                        moved.scale *= w.scale.max(0.0);
                        moved.opacity *= w.alpha.clamp(0.0, 1.0);
                    }
                    None if l.look == BreakLook::Shatter => shatter = Some((r, l)),
                    None => continue,
                },
            }
            shown.push(Shown { copy, letter, moved, shatter });
        }
    }
    let half = band_half(cx, &s);
    // A letter Draw on traces walks its outline; so does one breaking into pieces (its pieces clip
    // it to their cells).
    let tiled = |l: &Shown| fast && l.shatter.is_none() && l.moved.trace.is_none();
    // Each letter's outline as it shows (fast letters' are their moved boxes), laying the gradient.
    let outlines: Vec<Vec<Vec<Vec2>>> = shown
        .iter()
        .map(|l| {
            let now = &copies[l.copy];
            if tiled(l) {
                let (letter, p) = (&now.layout.letters[l.letter], now.placed[l.letter]);
                let turn = Vec2::from_angle(p.turn);
                let (a, b) = (letter.min - letter.origin, letter.max - letter.origin);
                let corners = [a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)].map(|q| p.at + turn.rotate(q));
                let to = now.mover(l.letter, l.moved);
                vec![corners.into_iter().map(to).collect()]
            } else {
                now.contours_moved(l.letter, l.moved)
            }
        })
        .collect();
    // Fast letters lay the gradient over their boxes as laid out (as before letter moves); with
    // letter moves, drawn ones lay it over every letter as laid out, so it stays put while the
    // letters move through it.
    let moving = copies.iter().any(|c| c.moving);
    let points: Vec<Vec2> = if moving && !fast {
        // Every letter of each copy showing, so letters drawing on or scattering in don't move it.
        let mut seen: Vec<usize> = shown.iter().map(|l| l.copy).collect();
        seen.sort_unstable();
        seen.dedup();
        seen.iter()
            .flat_map(|&c| (0..copies[c].layout.letters.len()).map(move |k| (c, k)))
            .flat_map(|(c, k)| copies[c].contours_placed(k).into_iter().flatten())
            .collect()
    } else if fast {
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
    let geometry = Geometry { points: Some(points), ellipse: None, half };
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
    // Layers laid over each letter or line, or worn by only some letters: a patched copy of the
    // records a unit, each letter naming its own.
    frame.look_layers.extend_from_slice(&records);
    let unit_layers = stack.unit_layers(look);
    let unit_firsts = if unit_layers.is_empty() {
        Vec::new()
    } else {
        unit_records(&copies, &shown, &outlines, &unit_layers, &records, &mut frame.look_layers)
    };
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
    let heat = stack.heat().map(|heat| crate::modules::look::layers::heat::shape_heat(cx, heat));
    if let Some(heat) = heat {
        hit[3] = heat;
    }
    // A walled Text's letters each flash and count their own hits.
    let own_hits =
        if stack.needs_hits() { copies.first().and_then(|c| letter_hits(cx, c.layout.letters.len())) } else { None };
    let past = stack.reach_past();
    let code = thing_code(cx.thing.id);
    let tile_scale = s.size / tiles::TILE_CAP;
    // Each pushed instance with its letter, how much of it shows, how far its layers may reach
    // past it and half its band.
    let mut pushed: Vec<(usize, usize, f32, f32, f32)> = Vec::new();
    for (i, l) in shown.iter().enumerate() {
        let now = &copies[l.copy];
        let m = l.moved;
        let letter = &now.layout.letters[l.letter];
        let glyph = tiled(l).then(|| tiles::tile_for(&font, letter.glyph, s.weight, s.slant)).flatten();
        let tile = glyph.as_ref().and_then(|t| frame.mask_tile(t.hash, &t.bytes).map(|i| (t, i)));
        if let Some((t, index)) = tile {
            let p = now.placed[l.letter];
            let centre = now.mover(l.letter, m)(p.at + Vec2::from_angle(p.turn).rotate(t.centre * tile_scale));
            // A stretched letter reads its tile at the stretch's mean size.
            let size = m.scale * (m.stretch.x * m.stretch.y).abs().sqrt();
            let half_tile = t.half * tile_scale * size;
            let reach = half_tile * tiles::reach_share();
            let turn = p.turn + m.turn;
            if let Some(k) =
                frame.push_glyph(now.pose.position, now.pose.angle, centre, turn, half_tile, index, BODY_WHITE, code)
            {
                pushed.push((k, i, m.opacity, past.min(reach), half));
            }
            continue;
        }
        // The pieces it draws as: whole, or Draw on's traced line and its fill.
        let full = if fast { now.contours_moved(l.letter, m) } else { outlines[i].clone() };
        let mut pieces: Vec<(Vec<Vec<Vec2>>, f32, f32)> = Vec::new();
        match m.trace {
            None => pieces.push((full, half, m.opacity)),
            Some(tr) if tr.share < 1.0 => {
                pieces.push((super::moves::traced(&full, tr.share).0, tr.band, m.opacity * tr.band_shown))
            }
            Some(tr) => {
                if tr.fill > 0.0 {
                    pieces.push((full.clone(), half, m.opacity * tr.fill));
                }
                if tr.band_shown > 0.0 {
                    pieces.push((full, tr.band, m.opacity * tr.band_shown));
                }
            }
        }
        for (contours, band, opacity) in pieces {
            let margin = past + band + 2.0;
            if let Some(k) = frame.push_letter(now.pose.position, now.pose.angle, &contours, margin, BODY_WHITE, code) {
                pushed.push((k, i, opacity, past, band));
            }
        }
    }
    // Layers running from the middle measure out to the whole text's reach.
    let reach = pushed.iter().map(|&(i, ..)| frame.paths[i].c[1]).fold(0.0, f32::max);
    for &(i, l, opacity, reach_past, band) in &pushed {
        let records_first = unit_firsts.get(l).copied().flatten().unwrap_or(first);
        let l = &shown[l];
        let path = &mut frame.paths[i];
        path.layers = [records_first, count, program, part];
        path.b[0] = band;
        path.c[2] = reach_past;
        path.c[3] = reach;
        path.hit = match own_hits.as_ref().filter(|_| l.copy == 0).and_then(|h| h.get(l.letter)) {
            Some(own) => [own[0], own[1], own[2], heat.unwrap_or(own[3])],
            None => hit,
        };
        if opacity < 1.0 {
            path.clip[2] = (1.0 - opacity).clamp(0.0, 1.0);
        }
    }
    // Shattering letters become their pieces (after every letter has its records, which each
    // piece keeps).
    let walls = crate::rules::remains::draw::walls_of(cx.world);
    for &(i, l, ..) in &pushed {
        let l = &shown[l];
        let Some((r, look)) = &l.shatter else { continue };
        let now = &copies[l.copy];
        let middle = now.middle(l.letter);
        let contours = now.contours_moved(l.letter, l.moved);
        // The outer contour (the longest) round the letter's middle: what the pieces are made of.
        let Some(outer) = contours.iter().max_by_key(|c| c.len()) else { continue };
        let edge: Vec<Vec2> = outer.iter().map(|p| *p - middle).collect();
        let letter = crate::rules::remains::draw::LetterPiece { part: l.letter as u32, edge: &edge, half, middle };
        crate::rules::remains::draw::shatter_letter(r, look, cx.thing.id.0, &letter, i, cx.time, &walls, frame);
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

    /// The test scene (its ring and ball) with "II" in the world at the middle, balls doing
    /// `balls` at it, the ball falling onto the second I; and that Text's id.
    fn walled(balls: &str) -> (Scene, ThingId) {
        let reg = crate::registry();
        let make = |ball: Vec2| {
            let mut scene = crate::new_scene(&reg);
            let id = crate::add_thing(&mut scene, &reg, KIND, ThingId::ROOT).unwrap();
            let t = scene.things.get_mut(&id).unwrap();
            t.screen = Some(false);
            t.transform.position = Vec2::ZERO;
            t.params.set("words", Value::Text("II".into()));
            t.params.set("size", Value::Number(300.0));
            t.params.set("balls", Value::Choice(balls.into()));
            let b = scene.things.get_mut(&crate::testing::BALLS).unwrap();
            b.transform.position = ball;
            b.params.set("velocity", Value::Vec2([0.0, 0.0]));
            (scene, id)
        };
        let (scene, id) = make(Vec2::ZERO);
        let kit = crate::kit();
        let resolved = resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let bpe_runtime::Outline::Letters { letters, .. } = &world.walls[&id].outline else { panic!() };
        let second = &letters.list[1];
        make(Vec2::new(0.5 * (second.lo.x + second.hi.x), second.lo.y - 200.0))
    }

    /// Text `id`'s letters in `frame` (the test scene's ring is a path too).
    fn letters_of(frame: &Frame, id: ThingId) -> Vec<PathInstance> {
        paths(frame).into_iter().filter(|p| p.c[0] == thing_code(id)).collect()
    }

    #[test]
    fn each_walled_letter_has_its_own_hits() {
        let (mut scene, id) = walled("bounce_off");
        let reg = crate::registry();
        use crate::modules::look::shape::{PAINT, SUB_FLASH};
        let paint = scene.add_layer(&reg, id, SlotId::Look, PAINT).unwrap();
        scene.add_sub_layer(&reg, id, SlotId::Look, paint, SUB_FLASH).unwrap();
        let ps = letters_of(&frame_at(&scene, 2.0), id);
        assert_eq!(ps.len(), 2);
        assert_eq!(ps[0].hit, bpe_gfx::NO_HIT, "the first I was never hit");
        assert!(ps[1].hit[2] >= 1.0 && ps[1].hit[0] < 2.0, "{:?}", ps[1].hit);
    }

    #[test]
    fn bounce_inside_letters_draw_as_bands_of_their_thickness() {
        let (mut scene, id) = walled("bounce_inside");
        scene.things.get_mut(&id).unwrap().params.set("thickness", Value::Number(16.0));
        let ps = letters_of(&frame_at(&scene, 0.0), id);
        assert!(ps.len() == 2 && ps.iter().all(|p| p.b[0] == 8.0), "{:?}", ps.iter().map(|p| p.b).collect::<Vec<_>>());
        let (scene, id) = walled("bounce_off");
        assert!(letters_of(&frame_at(&scene, 0.0), id).iter().all(|p| p.b[0] == 0.0), "filled");
        // A screen Text doesn't bounce, so it stays filled.
        let (mut scene, id) = walled("bounce_inside");
        scene.things.get_mut(&id).unwrap().screen = Some(true);
        let ps = letters_of(&frame_at(&scene, 0.0), id);
        assert!(ps.len() == 2 && ps.iter().all(|p| p.b[0] == 0.0));
    }

    /// `walled` with a rule breaking the letter a ball hits, with the look `look` (and its
    /// settings).
    fn breaking(look: &str, settings: &[(&str, Value)]) -> (Scene, ThingId) {
        let reg = crate::registry();
        let (mut scene, id) = walled("bounce_off");
        let rule = scene.add_rule("Break");
        let when = scene.add_block(&reg, rule, "when.hit", None).unwrap();
        let text = Value::Who(bpe_core::Who::Thing { thing: id, ball: None });
        scene.set_block_param(&reg, rule, when, "whom", text).unwrap();
        let brk = scene.add_block(&reg, rule, "do.break", Some(when)).unwrap();
        scene.set_block_param(&reg, rule, brk, "who", Value::Who(bpe_core::Who::Other)).unwrap();
        scene.set_block_param(&reg, rule, brk, "look", Value::Choice(look.into())).unwrap();
        for (k, v) in settings {
            scene.set_block_param(&reg, rule, brk, k, v.clone()).unwrap();
        }
        (scene, id)
    }

    /// When Text `id`'s letter `part` broke in `scene`, in seconds.
    fn broke_at(scene: &Scene, id: ThingId) -> f64 {
        let kit = crate::kit();
        let resolved = resolve(scene, &kit.registry);
        let mut world = World::build(scene, &resolved, &kit.sims);
        let mut log = Log::default();
        world.advance_to(2.0, &kit.sims, &mut log);
        let e = log.events().iter().find(|e| e.kind == bpe_runtime::EventKind::Break && e.a.thing == id).unwrap();
        e.time(world.steps_per_second)
    }

    #[test]
    fn a_broken_letter_fades_out_then_is_gone() {
        let (scene, id) = breaking("fade_out", &[("fade_time", Value::Number(1.0))]);
        let at = broke_at(&scene, id);
        let ps = letters_of(&frame_at(&scene, at + 0.5), id);
        assert_eq!(ps.len(), 2, "still showing");
        assert_eq!(ps[0].clip[2], 0.0, "the standing letter shows whole");
        assert!(ps[1].clip[2] > 0.2 && ps[1].clip[2] < 0.8, "{}", ps[1].clip[2]);
        assert_eq!(letters_of(&frame_at(&scene, at + 1.5), id).len(), 1, "its look is over");
        let (scene, id) = breaking("vanish", &[]);
        let at = broke_at(&scene, id);
        assert_eq!(letters_of(&frame_at(&scene, at + 0.05), id).len(), 1, "Vanish leaves nothing");
    }

    #[test]
    fn a_shattered_letter_is_its_pieces_flying_apart() {
        let (scene, id) = breaking("shatter", &[("pieces", Value::Int(8)), ("fade_after", Value::Number(5.0))]);
        let at = broke_at(&scene, id);
        let early = letters_of(&frame_at(&scene, at + 0.02), id);
        let late = letters_of(&frame_at(&scene, at + 0.5), id);
        for f in [&early, &late] {
            assert_eq!(f[0].clip[1], 4.0, "the standing letter keeps its box");
            assert!(f.len() >= 6, "{}", f.len());
            assert!(f[1..].iter().all(|p| p.layers == f[0].layers), "every piece wears the letter's look");
        }
        // Each piece is clipped to its own cell, and they move apart.
        let spread = |f: &[PathInstance]| {
            let ps: Vec<Vec2> = f[1..].iter().map(|p| Vec2::new(p.a[0], p.a[1])).collect();
            let mid = ps.iter().copied().sum::<Vec2>() / ps.len() as f32;
            ps.iter().map(|p| p.distance(mid)).sum::<f32>() / ps.len() as f32
        };
        assert!(spread(&late) > spread(&early) + 10.0, "{} {}", spread(&early), spread(&late));
        let cells: Vec<f32> = late[1..].iter().map(|p| p.clip[0]).collect();
        assert!(cells.windows(2).all(|w| w[0] != w[1]));
    }

    /// "ABCD" in the world with `add` run on its Look card (given the registry and the Text).
    fn look_with(add: impl FnOnce(&mut Scene, &bpe_core::Registry, ThingId)) -> (Scene, ThingId) {
        let (mut scene, id) = scene_with("ABCD");
        scene.things.get_mut(&id).unwrap().screen = Some(false);
        let reg = crate::registry();
        add(&mut scene, &reg, id);
        (scene, id)
    }

    fn set(scene: &mut Scene, id: ThingId, layer: bpe_core::LayerId, key: &str, v: Value) {
        scene.card_mut(id, SlotId::Look).unwrap().params.set(&bpe_core::layered_key(layer, key), v);
    }

    #[test]
    fn a_letter_being_drawn_on_is_a_band_of_the_pen_and_then_its_fill() {
        use crate::modules::look::shape::moves::DRAW_ON;
        let (scene, _) = look_with(|scene, reg, id| {
            let m = scene.add_layer(reg, id, SlotId::Look, DRAW_ON).unwrap();
            set(scene, id, m, "takes", Value::Number(4.0));
            set(scene, id, m, "width", Value::Number(8.0));
            set(scene, id, m, "then", Value::Choice("lines_and_fill".into()));
            set(scene, id, m, "fill", Value::Number(0.0));
        });
        let whole = frame_at(&scene, 10.0);
        // Done: each letter its fill and its band.
        assert_eq!(whole.paths.len(), 8);
        assert_eq!(whole.paths.iter().filter(|p| p.b[0] == 4.0).count(), 4, "bands half the pen wide");
        assert!(frame_at(&scene, 2.0).paths.len() < 8);
        // Early on only the first letter shows, as the part of it traced: fewer points than its
        // whole band.
        let early = frame_at(&scene, 0.2);
        assert_eq!(early.paths.len(), 1);
        assert_eq!(early.paths[0].b[0], 4.0);
        let first_band = whole.paths.iter().find(|p| p.b[0] == 4.0).unwrap();
        assert!(early.paths[0].b[3] < first_band.b[3], "{} {}", early.paths[0].b[3], first_band.b[3]);
    }

    #[test]
    fn a_gradient_laid_over_each_letter_gets_a_record_set_a_letter() {
        use crate::modules::look::shape as look;
        let (scene, _) = look_with(|scene, reg, id| {
            let p = scene.add_layer(reg, id, SlotId::Look, look::PAINT).unwrap();
            let g = bpe_core::Gradient::even(&[bpe_core::Color::WHITE, bpe_core::Color::rgb(255, 0, 0)]);
            set(scene, id, p, "colours", Value::Gradient(g));
            set(scene, id, p, "across", Value::Choice("each_letter".into()));
            set(scene, id, p, "colours~turn", Value::Number(0.0));
        });
        let frame = frame_at(&scene, 0.0);
        assert_eq!(frame.paths.len(), 4);
        let firsts: Vec<u32> = frame.paths.iter().map(|p| p.layers[0]).collect();
        assert!(firsts.windows(2).all(|w| w[0] != w[1]), "{firsts:?}");
        assert_eq!(frame.look_layers.len(), 2 * 5, "the whole text's and one a letter");
        // Each letter's Straight runs over its own box.
        for (k, p) in frame.paths.iter().enumerate() {
            let r = frame.look_layers[p.layers[0] as usize + 1];
            let width = r.p1[3] - r.p1[2];
            let whole = frame.look_layers[1].p1[3] - frame.look_layers[1].p1[2];
            assert!(width > 10.0 && width < 0.5 * whole, "letter {k}: {width} of {whole}");
        }
    }

    #[test]
    fn a_layer_worn_by_some_letters_is_off_on_the_others() {
        use crate::modules::look::shape as look;
        let (scene, _) = look_with(|scene, reg, id| {
            let o = scene.add_layer(reg, id, SlotId::Look, look::OUTLINE).unwrap();
            scene.set_layer_only(reg, id, SlotId::Look, o, vec![1]).unwrap();
        });
        let frame = frame_at(&scene, 0.0);
        let outline_of = |k: usize| frame.look_layers[frame.paths[k].layers[0] as usize + 1];
        assert_eq!(outline_of(1).head[2] & bpe_gfx::LayerGpu::UNWORN, 0);
        for k in [0, 2, 3] {
            assert_ne!(outline_of(k).head[2] & bpe_gfx::LayerGpu::UNWORN, 0, "letter {k}");
            assert_eq!(outline_of(k).amount[0], 0.0);
        }
        assert_eq!(frame.look_layers.len(), 2 * 2, "the whole text's (worn by letter 1) and one set not worn");
        assert_eq!(frame.paths[1].layers[0], 0);
        assert!([0, 2, 3].iter().all(|k| frame.paths[*k].layers[0] == 2));
    }

    #[test]
    fn a_spinning_letter_turns_its_outline_and_fast_letters_their_tile() {
        use crate::modules::look::shape::moves::SPIN;
        let (mut scene, id) = look_with(|scene, reg, id| {
            let m = scene.add_layer(reg, id, SlotId::Look, SPIN).unwrap();
            set(scene, id, m, "speed", Value::Number(90.0));
        });
        let still = frame_at(&scene, 0.0);
        let turned = frame_at(&scene, 1.0);
        assert_ne!(still.path_points, turned.path_points);
        scene.things.get_mut(&id).unwrap().params.set("fast", Value::Choice("always".into()));
        let fast = frame_at(&scene, 1.0);
        let glyph = fast.paths.iter().find(|p| p.b[1] > 2.5).expect("a glyph");
        assert!(glyph.clip.iter().all(|c| c.is_finite()));
    }
}
