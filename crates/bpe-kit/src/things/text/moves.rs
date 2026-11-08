//! Letter moves (Text part 4): the Text's Look card's moves, worked out at the moment drawn for
//! each letter and folded into its [`LetterMove`] after the Show's. Pure in time: read from the
//! settings, the scene's beat, the log and the Follow sound reader. Drawing only: the wall, the
//! parts and the physics never see them.

use std::f32::consts::TAU;

use bpe_core::{
    Address, LayerId, PulseSource, ResolvedSlot, Scene, Settings, SlotId, Target, ThingId, Value, layered_key,
};
use bpe_runtime::{EventKind, Log, SoundAt, World};
use glam::Vec2;

use super::draw::{TextAt, TextNow};
use super::show::{LetterMove, Trace};
use crate::modules::look::shape::moves::{
    BEAT, BeatMove, DRAW_ON, DrawOnMove, DrawOrder, DrawThen, FLICKER, FlickerMove, HIT, HitLetters, HitMove, SCATTER,
    SHAKE, SOUND, SPIN, ScatterMove, ShakeMove, SoundMove, SpinMove, WAVE, WaveMove,
};
use crate::things::light::draw::{flicker, hash};

/// One shown move with its settings.
#[derive(Clone, Debug, PartialEq)]
pub enum Move {
    Wave(WaveMove),
    Beat(BeatMove),
    Hit(HitMove),
    Flicker(FlickerMove),
    Sound(SoundMove, LayerId),
    Shake(ShakeMove),
    Spin(SpinMove),
    Scatter(ScatterMove),
    DrawOn(DrawOnMove),
}

impl Move {
    fn amount(&self) -> f32 {
        let a = match self {
            Move::Wave(m) => m.amount,
            Move::Beat(m) => m.amount,
            Move::Hit(m) => m.amount,
            Move::Flicker(m) => m.amount,
            Move::Sound(m, _) => m.amount,
            Move::Shake(m) => m.amount,
            Move::Spin(m) => m.amount,
            Move::Scatter(m) => m.amount,
            Move::DrawOn(m) => m.amount,
        };
        if a.is_finite() { a.clamp(0.0, 1.0) } else { 0.0 }
    }
}

/// A Text's shown moves, in list order, each with the letters wearing it (empty: all).
pub fn moves_of(look: &ResolvedSlot) -> Vec<(Move, Vec<u32>)> {
    look.layers
        .iter()
        .filter_map(|l| {
            let p = look.layer_params(l.id);
            let m = match l.kind.as_str() {
                WAVE => Move::Wave(WaveMove::from_params(&p)),
                BEAT => Move::Beat(BeatMove::from_params(&p)),
                HIT => Move::Hit(HitMove::from_params(&p)),
                FLICKER => Move::Flicker(FlickerMove::from_params(&p)),
                SOUND => Move::Sound(SoundMove::from_params(&p), l.id),
                SHAKE => Move::Shake(ShakeMove::from_params(&p)),
                SPIN => Move::Spin(SpinMove::from_params(&p)),
                SCATTER => Move::Scatter(ScatterMove::from_params(&p)),
                DRAW_ON => Move::DrawOn(DrawOnMove::from_params(&p)),
                _ => return None,
            };
            Some((m, l.only.clone()))
        })
        .collect()
}

/// `x` when it is a finite number, else `or`.
fn finite(x: f32, or: f32) -> f32 {
    if x.is_finite() { x } else { or }
}

/// A seeded number from 0 to 1 for letter `k`'s `n`th dice.
fn dice(seed: u32, k: usize, n: i64) -> f32 {
    hash(k as i64 * 16 + n, seed ^ 0x7e57_1e77)
}

/// A hop over `u` from 0 to 1: up and back down, highest half way.
fn hop(u: f64) -> f32 {
    if (0.0..1.0).contains(&u) { (4.0 * u * (1.0 - u)) as f32 } else { 0.0 }
}

/// Smooth seeded wobble from −1 to 1: three sines with seeded phases.
pub(crate) fn wobble(t: f64, speed: f32, seed: u32, k: usize, axis: i64) -> f32 {
    let f = finite(speed, 0.0).max(0.0) as f64;
    let ph = |n: i64| dice(seed, k, axis * 3 + n) as f64 * std::f64::consts::TAU;
    let s = (std::f64::consts::TAU * f * t + ph(0)).sin()
        + 0.5 * (std::f64::consts::TAU * 2.31 * f * t + ph(1)).sin()
        + 0.25 * (std::f64::consts::TAU * 4.73 * f * t + ph(2)).sin();
    (s / 1.75) as f32
}

fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t)
}

fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// What a move does to one letter, in the letter's own frame (or to a whole shape, which wears the
/// moves as one letter).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Effect {
    /// How far it moves (a letter's: in its own frame; a shape's: in the picture's).
    pub offset: Vec2,
    /// Radians, clockwise.
    pub turn: f32,
    /// How much bigger it grows about its middle.
    pub scale: f32,
    /// How much wider and taller, in its own frame.
    pub stretch: Vec2,
    /// How much of it shows, 0 to 1.
    pub opacity: f32,
}

impl Effect {
    pub const NONE: Effect = Effect { offset: Vec2::ZERO, turn: 0.0, scale: 1.0, stretch: Vec2::ONE, opacity: 1.0 };

    /// This much of it: 0 does nothing, 1 all of it.
    fn times(self, a: f32) -> Effect {
        Effect {
            offset: self.offset * a,
            turn: self.turn * a,
            scale: 1.0 + (self.scale - 1.0) * a,
            stretch: Vec2::ONE + (self.stretch - Vec2::ONE) * a,
            opacity: 1.0 + (self.opacity - 1.0) * a,
        }
    }
}

/// The hits a Jump on hit move reads, oldest first: seconds, and the letter hit (a walled Text's
/// own letters), worked out once a copy.
struct Hits {
    times: Vec<(f64, Option<u32>)>,
    /// Each letter its own hits.
    by_letter: bool,
}

/// What the moves of a Text or a shape read at the moment drawn.
#[derive(Clone, Copy)]
pub struct MoveCx<'a> {
    /// The scene as saved (its pulses: what a Follow sound move follows).
    pub tree: &'a Scene,
    pub world: &'a World,
    pub log: Option<&'a Log>,
    pub sound: Option<&'a dyn SoundAt>,
    /// The thing wearing the moves.
    pub id: ThingId,
    /// Seconds: the video's clock.
    pub time: f64,
}

impl<'a> MoveCx<'a> {
    /// What a Text's moves read.
    pub fn of_text(at: &TextAt<'a>) -> Self {
        MoveCx { tree: at.tree, world: at.world, log: at.log, sound: at.sound, id: at.id, time: at.time }
    }

    /// What the moves of the thing `cx` draws read.
    pub fn of_draw(cx: &bpe_gfx::DrawCx<'a>) -> Self {
        MoveCx { tree: cx.tree, world: cx.world, log: Some(cx.log), sound: cx.sound, id: cx.thing.id, time: cx.time }
    }
}

/// The hits Jump on hit `m` reads for a copy riding `ball` (or none) with `letters` letters.
fn hits_for(cx: &MoveCx, m: &HitMove, ball: Option<(ThingId, usize)>, letters: usize) -> Hits {
    let mut out = Hits { times: Vec::new(), by_letter: false };
    let Some(log) = cx.log else { return out };
    let rate = cx.world.steps_per_second.max(1);
    let me = cx.id;
    enum Reads {
        Letters,
        Ball(bpe_core::ThingId, u32),
        Things(Vec<bpe_core::ThingId>),
    }
    let reads = match (&m.hits_of, ball) {
        (Target::Me, Some((thing, row))) => Reads::Ball(thing, row as u32),
        (Target::Me, None) if cx.world.parts.contains_key(&me) && m.which == HitLetters::TheHitLetter => Reads::Letters,
        (t, _) => Reads::Things(cx.world.matching(t, me)),
    };
    out.by_letter = matches!(reads, Reads::Letters);
    // Only hits a hop is still going from matter: back to the last letter's Each letter later, and a hop.
    let later = if m.which == HitLetters::EveryLetter { finite(m.later, 0.0).max(0.0) as f64 } else { 0.0 };
    let back = later * letters as f64 + finite(m.takes, 0.35).max(0.01) as f64;
    let from = ((cx.time - back) * rate as f64).floor().max(0.0) as u64;
    for e in log.since(from.saturating_sub(1)) {
        if e.kind != EventKind::Hit {
            continue;
        }
        let t = e.time(rate);
        if t > cx.time {
            break;
        }
        let hit = match &reads {
            Reads::Letters => e.b.filter(|b| b.thing == me).map(|b| Some(b.item)),
            Reads::Ball(thing, row) => {
                let on = |p: &bpe_runtime::Party| p.thing == *thing && p.item == *row;
                (on(&e.a) || e.b.as_ref().is_some_and(on)).then_some(None)
            }
            Reads::Things(things) => {
                (things.contains(&e.a.thing) || e.b.is_some_and(|b| things.contains(&b.thing))).then_some(None)
            }
        };
        if let Some(letter) = hit {
            out.times.push((t, letter));
        }
    }
    out
}

/// The Follow sound reading for a Sound move: the shape and part its Level follows and how much
/// the pulse adds, when it follows sound and the Text has a reader.
fn follows<'a>(cx: &MoveCx<'a>, layer: LayerId) -> Option<(bpe_core::ThingId, &'a bpe_core::SoundSource, f32)> {
    let addr = Address { thing: cx.id, slot: Some(SlotId::Look), key: layered_key(layer, "level") };
    let pulse = cx.tree.pulses.get(&addr)?;
    let PulseSource::Sound { shape, source } = &pulse.source else { return None };
    let Value::Number(amount) = pulse.amount else { return None };
    Some((*shape, source, amount as f32))
}

/// Draw on's state for one copy: each letter's share of the pen's way and when it is done.
struct DrawPlan {
    /// Each letter's (start, end) as shares of the whole tracing, 0 to 1.
    spans: Vec<(f32, f32)>,
}

impl DrawPlan {
    fn of(m: &DrawOnMove, now: &TextNow) -> DrawPlan {
        let n = now.layout.letters.len();
        if m.order == DrawOrder::AllTogether {
            return DrawPlan { spans: vec![(0.0, 1.0); n] };
        }
        let lengths: Vec<f32> = (0..n).map(|k| outline_length(&now.contours_placed(k))).collect();
        let total: f32 = lengths.iter().sum();
        if total <= 0.0 {
            return DrawPlan { spans: vec![(0.0, 1.0); n] };
        }
        let mut before = 0.0;
        let spans = lengths
            .iter()
            .map(|l| {
                let s = (before / total, (before + l) / total);
                before += l;
                s
            })
            .collect();
        DrawPlan { spans }
    }
}

/// The length of every contour of `contours`, closed.
pub fn outline_length(contours: &[Vec<Vec2>]) -> f32 {
    contours
        .iter()
        .filter(|c| c.len() >= 2)
        .map(|c| c.iter().zip(c.iter().cycle().skip(1)).map(|(a, b)| a.distance(*b)).sum::<f32>())
        .sum()
}

/// Letter `k`'s Draw on trace at `time` (`None`: the letter isn't shown yet).
fn trace_of(m: &DrawOnMove, plan: &DrawPlan, k: usize, time: f64, amount: f32) -> Option<Trace> {
    let takes = finite(m.takes, 3.0).max(0.01) as f64;
    let start = finite(m.start, 0.0).max(0.0) as f64;
    let whole = ((time - start) / takes).clamp(0.0, 1.0) as f32;
    let (lo, hi) = plan.spans.get(k).copied().unwrap_or((0.0, 1.0));
    let raw = if hi > lo {
        ((whole - lo) / (hi - lo)).clamp(0.0, 1.0)
    } else if whole >= hi {
        1.0
    } else {
        0.0
    };
    // Less than all of the move starts the letters part traced.
    let share = 1.0 - amount * (1.0 - raw);
    if share <= 0.0 {
        return None;
    }
    let done = start + hi as f64 * takes;
    let fill_takes = finite(m.fill, 0.5).max(0.0) as f64;
    let filled = if share < 1.0 {
        0.0
    } else if fill_takes <= 0.0 {
        1.0
    } else {
        (((time - done) / fill_takes).clamp(0.0, 1.0)) as f32
    };
    let (fill, band_shown) = match m.then {
        DrawThen::LinesOnly => (0.0, 1.0),
        DrawThen::LinesAndFill => (filled, 1.0),
        DrawThen::FillOnly => (filled, 1.0 - filled),
    };
    Some(Trace { share, band: 0.5 * finite(m.width, 6.0).max(0.1), fill, band_shown })
}

/// What the moves read once a copy: each Jump on hit's hits and each Follow sound's pulse and
/// level now, by the move's place in the list.
struct Prepared {
    hits: Vec<Option<Hits>>,
    sounds: Vec<Option<(f32, f32)>>,
}

impl Prepared {
    fn of(cx: &MoveCx, moves: &[(Move, Vec<u32>)], ball: Option<(ThingId, usize)>, letters: usize) -> Prepared {
        let hits = moves
            .iter()
            .map(|(m, _)| if let Move::Hit(h) = m { Some(hits_for(cx, h, ball, letters)) } else { None })
            .collect();
        let sounds = moves
            .iter()
            .map(|(m, _)| {
                let Move::Sound(_, layer) = m else { return None };
                let (shape, source, add) = follows(cx, *layer)?;
                let (sound, log) = (cx.sound?, cx.log?);
                Some((add, sound.level(cx.world, log, shape, source, cx.time)))
            })
            .collect();
        Prepared { hits, sounds }
    }
}

/// What move `m` (number `i` in the list) does to letter `k` at full amount; Draw on does nothing
/// here (it traces, see [`apply`]).
fn effect_of(cx: &MoveCx, m: &Move, i: usize, k: usize, prepared: &Prepared) -> Effect {
    let t = cx.time;
    let beat = cx.world.beat_seconds;
    let kf = k as f32;
    match m {
        Move::Wave(w) => {
            let letters = finite(w.letters, 8.0).max(0.1);
            let phase = TAU * (finite(w.speed, 0.5) * t as f32 - kf / letters);
            Effect {
                offset: Vec2::new(0.0, -finite(w.height, 30.0) * phase.sin()),
                turn: finite(w.tilt, 0.0).to_radians() * phase.cos(),
                ..Effect::NONE
            }
        }
        Move::Beat(b) => {
            let every = finite(b.every, 1.0).max(0.01) as f64 * beat;
            let u = t - k as f64 * finite(b.later, 0.0).max(0.0) as f64;
            let j = if every > 0.0 && every.is_finite() && u >= 0.0 {
                let since = u - (u / every).floor() * every;
                hop(since / finite(b.takes, 0.3).max(0.01) as f64)
            } else {
                0.0
            };
            let squash = finite(b.squash, 0.3).clamp(0.0, 1.0) * j;
            Effect {
                offset: Vec2::new(0.0, -finite(b.height, 40.0) * j),
                stretch: Vec2::new(1.0 / (1.0 + 0.5 * squash), 1.0 + 0.5 * squash),
                ..Effect::NONE
            }
        }
        Move::Hit(h) => {
            let Some(hits) = prepared.hits[i].as_ref() else { return Effect::NONE };
            let later = if h.which == HitLetters::EveryLetter { finite(h.later, 0.0).max(0.0) } else { 0.0 };
            let seen = t - k as f64 * later as f64;
            let last = hits
                .times
                .iter()
                .rev()
                .filter(|(at, _)| *at <= seen)
                .find(|(_, letter)| !hits.by_letter || *letter == Some(k as u32));
            let j = last.map_or(0.0, |(at, _)| hop((seen - at) / finite(h.takes, 0.35).max(0.01) as f64));
            Effect { offset: Vec2::new(0.0, -finite(h.height, 50.0) * j), ..Effect::NONE }
        }
        Move::Flicker(f) => {
            let seed = if f.together { f.seed } else { f.seed.wrapping_add(k as u32) };
            // Flicker's own Amount is its depth.
            let o = flicker(t, 1.0, finite(f.speed, 8.0), f.style, seed);
            Effect { opacity: o.clamp(0.0, 1.0), ..Effect::NONE }
        }
        Move::Sound(s, layer) => {
            let mut level = finite(s.level, 0.0);
            if let (Some((add, now_level)), Some(sound), Some(log)) = (prepared.sounds[i], cx.sound, cx.log) {
                let d = k as f64 * finite(s.later, 0.0).max(0.0) as f64;
                if d > 0.0
                    && let Some((shape, source, _)) = follows(cx, *layer)
                {
                    let then = sound.level(cx.world, log, shape, source, (t - d).max(0.0));
                    // As the pulse reads it: 0 to 1.
                    level += add * (then.clamp(0.0, 1.0) - now_level.clamp(0.0, 1.0));
                }
            }
            let level = level.clamp(0.0, 1.0) * (1.0 - finite(s.own, 0.0).clamp(0.0, 1.0) * dice(s.seed, k, 0));
            let grow = 1.0 + finite(s.grow, 0.0) * level;
            Effect {
                offset: Vec2::new(0.0, -finite(s.jump, 60.0) * level),
                scale: grow.max(0.0),
                stretch: Vec2::new(1.0, (1.0 + finite(s.stretch, 0.0) * level).max(0.0)),
                ..Effect::NONE
            }
        }
        Move::Shake(s) => {
            let (speed, seed) = (finite(s.speed, 12.0), s.seed);
            let d = finite(s.distance, 6.0);
            Effect {
                offset: Vec2::new(wobble(t, speed, seed, k, 0), wobble(t, speed, seed, k, 1)) * d,
                turn: finite(s.turn, 4.0).to_radians() * wobble(t, speed, seed, k, 2),
                ..Effect::NONE
            }
        }
        Move::Spin(s) => {
            let (speed, later, swing) = (finite(s.speed, 90.0), finite(s.later, 0.0), finite(s.swing, 0.0));
            let turn = if swing > 0.0 {
                let phase = TAU * speed / 360.0 * t as f32 + (later * kf).to_radians();
                swing.to_radians() * phase.sin()
            } else {
                ((speed * t as f32 + later * kf) % 360.0).to_radians()
            };
            Effect { turn, ..Effect::NONE }
        }
        Move::Scatter(s) => {
            let out = scatter_share(s, t);
            let way = dice(s.seed, k, 0) * TAU;
            let far = finite(s.distance, 400.0) * (0.5 + 0.5 * dice(s.seed, k, 1));
            Effect {
                offset: Vec2::from_angle(way) * far * out,
                turn: finite(s.turn, 180.0).to_radians() * (2.0 * dice(s.seed, k, 2) - 1.0) * out,
                opacity: 1.0 - finite(s.fade, 0.0).clamp(0.0, 1.0) * out,
                ..Effect::NONE
            }
        }
        Move::DrawOn(_) => Effect::NONE,
    }
}

/// Folds `moves` into every shown letter of `now` at `at`'s moment.
pub fn apply(at: &TextAt, moves: &[(Move, Vec<u32>)], now: &mut TextNow) {
    if moves.is_empty() {
        return;
    }
    let cx = MoveCx::of_text(at);
    let t = at.time;
    let n = now.layout.letters.len();
    // Worked out once a copy.
    let prepared = Prepared::of(&cx, moves, now.ball, n);
    let plans: Vec<Option<DrawPlan>> =
        moves.iter().map(|(m, _)| if let Move::DrawOn(d) = m { Some(DrawPlan::of(d, now)) } else { None }).collect();
    for k in 0..n {
        let Some(mut lm) = now.shown[k] else { continue };
        let turn_of_letter = now.placed[k].turn;
        let mut traced = lm.trace.is_some();
        let mut hidden = false;
        for (i, (m, only)) in moves.iter().enumerate() {
            if !only.is_empty() && !only.contains(&(k as u32)) {
                continue;
            }
            let a = m.amount();
            if a <= 0.0 {
                continue;
            }
            if let Move::DrawOn(d) = m {
                // The first Draw on wins.
                if traced {
                    continue;
                }
                traced = true;
                let plan = plans[i].as_ref().expect("worked out above");
                match trace_of(d, plan, k, t, a) {
                    None => hidden = true,
                    // Done and filled in with no line left: the letter as it is.
                    Some(tr) if tr.share >= 1.0 && tr.fill >= 1.0 && tr.band_shown <= 0.0 => {}
                    Some(tr) => lm.trace = Some(tr),
                }
                continue;
            }
            let e = effect_of(&cx, m, i, k, &prepared).times(a);
            lm.offset += Vec2::from_angle(turn_of_letter).rotate(e.offset);
            lm.turn += e.turn;
            lm.scale *= e.scale;
            lm.stretch *= e.stretch;
            lm.opacity *= e.opacity;
        }
        now.shown[k] = if hidden { None } else { Some(lm) };
    }
}

/// What `moves` (a Shape's Look's) do to the whole shape at `cx`'s moment: the shape wears them as
/// one letter, number 0, whatever letters a move is limited to, and Draw on (a Text's alone) does
/// nothing. `None` when no move is worn.
pub fn shape_move(cx: &MoveCx, moves: &[(Move, Vec<u32>)]) -> Option<Effect> {
    let worn: Vec<(usize, &Move, f32)> = moves
        .iter()
        .enumerate()
        .filter(|(_, (m, _))| !matches!(m, Move::DrawOn(_)))
        .map(|(i, (m, _))| (i, m, m.amount()))
        .filter(|(_, _, a)| *a > 0.0)
        .collect();
    if worn.is_empty() {
        return None;
    }
    let prepared = Prepared::of(cx, moves, None, 1);
    let mut out = Effect::NONE;
    for (i, m, a) in worn {
        let e = effect_of(cx, m, i, 0, &prepared).times(a);
        out.offset += e.offset;
        out.turn += e.turn;
        out.scale *= e.scale;
        out.stretch *= e.stretch;
        out.opacity *= e.opacity;
    }
    Some(out)
}

/// How far out Scatter has the letters at `t`, 0 home to 1 furthest.
fn scatter_share(s: &ScatterMove, t: f64) -> f32 {
    let start = finite(s.start, 1.0).max(0.0) as f64;
    if t < start {
        return 0.0;
    }
    let (out, stay, back) = (finite(s.out, 0.6).max(0.01), finite(s.stay, 0.4).max(0.0), finite(s.back, 0.8).max(0.01));
    let mut u = (t - start) as f32;
    let every = finite(s.every, 0.0);
    if every > 0.0 {
        u %= every.max(out + stay + back);
    }
    if u < out {
        ease_out(u / out)
    } else if u < out + stay {
        1.0
    } else if u < out + stay + back {
        1.0 - ease_in_out((u - out - stay) / back)
    } else {
        0.0
    }
}

/// The part of `contours` (a letter's, closed) a pen has traced at `share` of their whole length,
/// contour after contour: each traced piece as an out-and-back loop (a whole contour as itself),
/// and where the pen is (`None` once it is all traced).
pub fn traced(contours: &[Vec<Vec2>], share: f32) -> (Vec<Vec<Vec2>>, Option<Vec2>) {
    let total = outline_length(contours);
    let mut left = share.clamp(0.0, 1.0) * total;
    let mut out = Vec::new();
    let mut pen = None;
    for c in contours.iter().filter(|c| c.len() >= 2) {
        if left <= 0.0 {
            break;
        }
        let len = outline_length(std::slice::from_ref(c));
        if left >= len {
            out.push(c.clone());
            left -= len;
            continue;
        }
        // Part of this one: its corners up to the pen, then back.
        let mut piece = vec![c[0]];
        for (a, b) in c.iter().zip(c.iter().cycle().skip(1)) {
            let d = a.distance(*b);
            if left <= d {
                let p = if d > 0.0 { *a + (*b - *a) * (left / d) } else { *a };
                piece.push(p);
                pen = Some(p);
                break;
            }
            piece.push(*b);
            left -= d;
        }
        if piece.len() >= 2 {
            let back: Vec<Vec2> = piece[1..piece.len() - 1].iter().rev().copied().collect();
            piece.extend(back);
            // A band needs three corners: a single straight stroke doubles its end.
            if piece.len() < 3 {
                piece.push(piece[1]);
            }
            out.push(piece);
        }
        left = 0.0;
    }
    (out, pen)
}

/// Where Draw on's pen is now on Text `at` (its first copy), in the world (a screen Text's mapped
/// through the camera); `None` before it sets off, once it is done, or without Draw on.
pub fn pen(at: &TextAt) -> Option<Vec2> {
    let now = super::draw::text_now(at)?;
    let screen = at.tree.things.get(&at.id).is_none_or(|t| t.screen.unwrap_or(true));
    (0..now.layout.letters.len()).find_map(|k| {
        let m = now.shown[k]?;
        let tr = m.trace.filter(|tr| tr.share < 1.0)?;
        let (_, p) = traced(&now.contours_moved(k, LetterMove { trace: None, ..m }), tr.share);
        let p = now.to_world(p?);
        Some(if screen { at.world.camera.to_world(p) } else { p })
    })
}

#[cfg(test)]
#[path = "moves_tests.rs"]
mod tests;
