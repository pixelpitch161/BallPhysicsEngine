//! A Text's experimental effects at the moment drawn (Text part 5; the kinds and settings are
//! `modules::look::shape::effects`). **Melt** reshapes each letter's contours before they are
//! pushed ([`Fx::melt`]); **Echo**, **Mirror**, **Colour split** and **Glitch** then work on the
//! Text's pushed shape instances ([`apply`]): copies moved, turned, scaled, flipped and faded,
//! cut to bands or wedges through their convex clips, and coloured through their style
//! (`bpe_gfx::PathStyle`). Pure in time; drawing only.

use std::f32::consts::{PI, TAU};

use bpe_core::{ResolvedSlot, Settings, Target, ThingId};
use bpe_gfx::{Frame, PathChannel, PathInstance, PathStyle};
use bpe_runtime::{CameraView, EventKind, Log, World};
use glam::Vec2;

use crate::modules::look::shape::effects::{
    ECHO, EchoEffect, GLITCH, GlitchEffect, MELT, MIRROR, MeltEffect, MirrorEffect, MirrorRound, SPLIT, SplitEffect,
};
use crate::things::light::draw::{hash, smooth_noise};

/// The most instances a Text's effects leave a frame (past it an effect makes no more copies).
pub const MOST_FX_INSTANCES: usize = 16384;
/// The most points a Text's effects add a frame (flipped copies and new clips).
pub const MOST_FX_POINTS: usize = 1_000_000;
/// The most bands Glitch cuts one instance into (past them it is left whole).
pub const MOST_BANDS: i64 = 64;
/// The most points a melted letter has after its contours are cut finer (`text::layout`'s cap).
const MOST_MELT_POINTS: usize = 2048;

/// One shown effect with its settings.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Echo(EchoEffect),
    Mirror(MirrorEffect),
    Split(SplitEffect),
    Glitch(GlitchEffect),
    Melt(MeltEffect),
}

/// A Text's shown effects, in list order, each with the letters wearing it (empty: all).
#[derive(Clone, Debug, Default)]
pub struct Fx {
    pub list: Vec<(Effect, Vec<u32>)>,
}

/// `x` when it is a finite number, else `or`.
fn finite(x: f32, or: f32) -> f32 {
    if x.is_finite() { x } else { or }
}

/// An effect's Amount, 0 to 1.
fn amount(a: f32) -> f32 {
    finite(a, 0.0).clamp(0.0, 1.0)
}

fn wears(only: &[u32], letter: u32) -> bool {
    only.is_empty() || only.contains(&letter)
}

impl Fx {
    /// The effects of a Text's Look card.
    pub fn of(look: &ResolvedSlot) -> Fx {
        let list = look
            .layers
            .iter()
            .filter_map(|l| {
                let p = look.layer_params(l.id);
                let e = match l.kind.as_str() {
                    ECHO => Effect::Echo(EchoEffect::from_params(&p)),
                    MIRROR => Effect::Mirror(MirrorEffect::from_params(&p)),
                    SPLIT => Effect::Split(SplitEffect::from_params(&p)),
                    GLITCH => Effect::Glitch(GlitchEffect::from_params(&p)),
                    MELT => Effect::Melt(MeltEffect::from_params(&p)),
                    _ => return None,
                };
                Some((e, l.only.clone()))
            })
            .collect();
        Fx { list }
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Whether the letters must be drawn from their outlines (fast letters can be neither melted
    /// nor flipped).
    pub fn needs_outlines(&self) -> bool {
        self.list.iter().any(|(e, _)| match e {
            Effect::Melt(_) => true,
            Effect::Mirror(m) => m.mirrored,
            _ => false,
        })
    }

    /// Whether a Melt reshapes the letters.
    pub fn melts(&self) -> bool {
        self.list.iter().any(|(e, _)| matches!(e, Effect::Melt(_)))
    }

    /// Whether any effect works on the pushed instances.
    fn copies(&self) -> bool {
        self.list.iter().any(|(e, _)| !matches!(e, Effect::Melt(_)))
    }

    /// Letter `letter`'s contours (in the text's frame) melted by every Melt it wears at `time`;
    /// `down` is the picture's down in the text's frame.
    pub fn melt(&self, contours: Vec<Vec<Vec2>>, letter: u32, down: Vec2, time: f64) -> Vec<Vec<Vec2>> {
        let mut c = contours;
        for (e, only) in &self.list {
            if let Effect::Melt(m) = e
                && wears(only, letter)
                && let Some(now) = MeltNow::of(m, time)
                && now.length > 0.0
            {
                c = melt_contours(&c, &now, down);
            }
        }
        c
    }

    /// How much of letter `letter` shows at `time` after its Melts' fades (1 whole).
    pub fn opacity(&self, letter: u32, time: f64) -> f32 {
        self.list
            .iter()
            .filter(|(_, only)| wears(only, letter))
            .filter_map(|(e, _)| match e {
                Effect::Melt(m) => MeltNow::of(m, time).map(|n| n.opacity),
                _ => None,
            })
            .product()
    }
}

// ---------- Melt ----------

/// A Melt at a moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeltNow {
    /// The longest drip now, px.
    pub length: f32,
    pub sag: f32,
    pub width: f32,
    pub seed: u32,
    /// How much shows (fading before it starts again).
    pub opacity: f32,
}

impl MeltNow {
    /// Melt `m` at `time`; `None` before it starts.
    pub fn of(m: &MeltEffect, time: f64) -> Option<MeltNow> {
        let start = finite(m.start, 0.0).max(0.0) as f64;
        if !time.is_finite() || time < start {
            return None;
        }
        let every = finite(m.every, 0.0).max(0.0) as f64;
        let mut t = time - start;
        let mut opacity = 1.0;
        if every > 1e-3 {
            t = t.rem_euclid(every);
            let fade = (0.1 * every).min(0.5);
            if t > every - fade {
                opacity = ((every - t) / fade).clamp(0.0, 1.0) as f32;
            }
        }
        let u = (t * finite(m.speed, 0.0).max(0.0) as f64).clamp(0.0, 1.0) as f32;
        Some(MeltNow {
            length: amount(m.amount) * finite(m.drip, 0.0).max(0.0) * u * u,
            sag: finite(m.sag, 0.0).clamp(0.0, 1.0),
            width: finite(m.width, 12.0).max(0.5),
            seed: m.seed,
            opacity,
        })
    }

    /// How long the drip is at `x` across the picture (px).
    pub fn length_at(&self, x: f32) -> f32 {
        let n = smooth_noise((x / self.width) as f64, self.seed ^ 0x3e17);
        self.length * (0.1 + 0.9 * n * n)
    }
}

/// `contours` cut so no piece is longer than `most` (at most `MOST_MELT_POINTS` points in all).
fn finer(contours: &[Vec<Vec2>], most: f32) -> Vec<Vec<Vec2>> {
    let perimeter: f32 =
        contours.iter().map(|c| (0..c.len()).map(|k| c[k].distance(c[(k + 1) % c.len()])).sum::<f32>()).sum();
    let points: usize = contours.iter().map(Vec::len).sum();
    let room = MOST_MELT_POINTS.saturating_sub(points).max(1) as f32;
    let step = most.max(perimeter / room).max(1e-3);
    contours
        .iter()
        .map(|c| {
            let mut out = Vec::with_capacity(c.len());
            for k in 0..c.len() {
                let (a, b) = (c[k], c[(k + 1) % c.len()]);
                out.push(a);
                let n = ((a.distance(b) / step).ceil() as usize).clamp(1, MOST_MELT_POINTS);
                for i in 1..n {
                    out.push(a.lerp(b, i as f32 / n as f32));
                }
            }
            out
        })
        .collect()
}

/// A letter's contours melted: cut finer, then each point moved down by its column's drip,
/// the more the lower it sits in the letter.
pub fn melt_contours(contours: &[Vec<Vec2>], now: &MeltNow, down: Vec2) -> Vec<Vec<Vec2>> {
    let down = down.try_normalize().unwrap_or(Vec2::Y);
    let across = Vec2::new(-down.y, down.x);
    let fine = finer(contours, (now.width / 3.0).max(1.5));
    let (lo, hi) = fine.iter().flatten().fold((f32::MAX, f32::MIN), |(lo, hi), p| {
        let d = p.dot(down);
        (lo.min(d), hi.max(d))
    });
    let tall = hi - lo;
    fine.into_iter()
        .map(|c| {
            c.into_iter()
                .map(|p| {
                    let w = if tall > 1e-3 { ((p.dot(down) - lo) / tall).clamp(0.0, 1.0) } else { 1.0 };
                    p + down * now.length_at(p.dot(across)) * (now.sag + (1.0 - now.sag) * w)
                })
                .collect()
        })
        .collect()
}

// ---------- the instance effects ----------

/// One instance the effects work on: its copy and letter, and how far back it draws (0 the
/// letters, an Echo's copies further back).
#[derive(Clone, Copy, Debug)]
pub struct Inst {
    pub p: PathInstance,
    pub copy: usize,
    pub letter: u32,
    pub depth: i32,
    /// Whether its clip is still the letter's own box (grown round the letter by its layers'
    /// reach), which a smaller Echo copy may grow again; cut pieces' clips stay as they are.
    pub boxed: bool,
}

/// What the instance effects read.
pub struct FxCx<'a> {
    pub time: f64,
    /// Whether the Text draws on the screen (its frame's space is the screen's).
    pub screen: bool,
    pub camera: CameraView,
    /// Each copy's middle in the frame's space.
    pub middles: Vec<Vec2>,
    pub world: Option<&'a World>,
    pub log: Option<&'a Log>,
    pub id: ThingId,
}

impl FxCx<'_> {
    fn to_screen(&self, p: Vec2) -> Vec2 {
        if self.screen { p } else { self.camera.to_screen(p) }
    }

    /// A move of `d` on the screen as a move in the frame's space.
    fn screen_move(&self, d: Vec2) -> Vec2 {
        if self.screen {
            return d;
        }
        let zoom = if self.camera.zoom.is_finite() && self.camera.zoom > 1e-6 { self.camera.zoom } else { 1.0 };
        Vec2::from_angle(self.camera.turn).rotate(d) / zoom
    }

    fn picture_middle(&self) -> Vec2 {
        if self.screen { Vec2::ZERO } else { self.camera.center }
    }

    fn middle(&self, copy: usize) -> Vec2 {
        self.middles.get(copy).copied().unwrap_or(Vec2::ZERO)
    }

    /// How strong the kick of the latest hit of `target` is now (1 right after it, settling over
    /// `settles` seconds), 0 with `harder` 0 or no hit.
    fn kick(&self, harder: f32, target: &Target, settles: f32) -> f32 {
        let harder = finite(harder, 0.0).clamp(0.0, 1.0);
        if harder <= 0.0 {
            return 0.0;
        }
        let settles = finite(settles, 0.4).max(0.01) as f64;
        match hit_age(self, target, 8.0 * settles) {
            Some(age) => harder * (-age / settles).exp() as f32,
            None => 0.0,
        }
    }
}

/// How long ago (seconds) the latest hit before the moment that involves a thing `target` names
/// was, looking back at most `back` seconds.
pub fn hit_age(cx: &FxCx, target: &Target, back: f64) -> Option<f64> {
    let (Some(world), Some(log)) = (cx.world, cx.log) else { return None };
    let rate = world.steps_per_second.max(1);
    let things = (*target != Target::Anything).then(|| world.matching(target, cx.id));
    let from = ((cx.time - back) * rate as f64).floor().max(0.0) as u64;
    let mut latest = None;
    for e in log.since(from) {
        if e.kind != EventKind::Hit {
            continue;
        }
        let t = e.time(rate);
        if t > cx.time {
            break;
        }
        let ours =
            things.as_ref().is_none_or(|ts| ts.contains(&e.a.thing) || e.b.is_some_and(|b| ts.contains(&b.thing)));
        if ours {
            latest = Some(cx.time - t);
        }
    }
    latest
}

/// How many instances and points the effects may still add.
struct Budget {
    instances: usize,
    points: usize,
}

impl Budget {
    fn take(&mut self, instances: usize, points: usize) -> bool {
        if instances > self.instances || points > self.points {
            return false;
        }
        self.instances -= instances;
        self.points -= points;
        true
    }
}

/// Where a point of an instance's own units sits in the frame's space.
fn frame_of(p: &PathInstance, q: Vec2) -> Vec2 {
    Vec2::new(p.a[0], p.a[1]) + Vec2::from_angle(p.a[2]).rotate(q * p.a[3])
}

/// The half-plane `n · w ≥ d` of the frame's space in an instance's own units.
fn local_half_plane(p: &PathInstance, n: Vec2, d: f32) -> (Vec2, f32) {
    let centre = Vec2::new(p.a[0], p.a[1]);
    (Vec2::from_angle(-p.a[2]).rotate(n) * p.a[3], d - n.dot(centre))
}

/// An instance's convex clip corners, in its own units (none without one).
fn clip_corners(frame: &Frame, p: &PathInstance) -> Vec<Vec2> {
    let (first, n) = (p.clip[0].max(0.0) as usize, p.clip[1].max(0.0) as usize);
    if n < 3 || first + n > frame.path_points.len() {
        return Vec::new();
    }
    frame.path_points[first..first + n].iter().map(|c| Vec2::new(c[0], c[1])).collect()
}

/// `poly` cut to the half-plane `n · q ≥ d` (Sutherland–Hodgman; keeps its turning).
fn cut(poly: &[Vec2], n: Vec2, d: f32) -> Vec<Vec2> {
    let mut out = Vec::with_capacity(poly.len() + 1);
    for k in 0..poly.len() {
        let (prev, cur) = (poly[(k + poly.len() - 1) % poly.len()], poly[k]);
        let (dp, dc) = (n.dot(prev) - d, n.dot(cur) - d);
        if (dc >= 0.0) != (dp >= 0.0) {
            let t = dp / (dp - dc);
            out.push(prev.lerp(cur, t));
        }
        if dc >= 0.0 {
            out.push(cur);
        }
    }
    out
}

/// `poly` without corners repeated (where a corner sat on a cut, `cut` gives it twice), which
/// would make a clip edge of no length.
fn tidied(mut poly: Vec<Vec2>) -> Vec<Vec2> {
    let span = poly.iter().fold(0.0f32, |m, q| m.max(q.abs().max_element())).max(1.0);
    let near = 1e-5 * span;
    poly.dedup_by(|a, b| a.distance(*b) <= near);
    while poly.len() > 1 && poly[0].distance(poly[poly.len() - 1]) <= near {
        poly.pop();
    }
    poly
}

/// Instance `p`'s own-box clip grown `by` its own units on every side (`None` without room).
fn grown_box(frame: &mut Frame, p: &PathInstance, by: f32, budget: &mut Budget) -> Option<PathInstance> {
    let corners = clip_corners(frame, p);
    if corners.len() != 4 || !by.is_finite() || by <= 0.0 || !budget.take(0, 4) {
        return None;
    }
    let (lo, hi) =
        corners.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), q| (lo.min(*q), hi.max(*q)));
    let (lo, hi) = (lo - Vec2::splat(by), hi + Vec2::splat(by));
    let first = frame.path_points.len();
    // Round the positive way, as `push_letter` and `push_glyph` lay a box's corners.
    for q in [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)] {
        frame.path_points.push([q.x, q.y, 0.0, 0.0]);
    }
    let mut out = *p;
    out.clip[0] = first as f32;
    Some(out)
}

/// Instance `p` cut to the half-planes `planes` (each `n · w ≥ d` in the frame's space): its clip
/// made smaller and pushed as new corners. `None` when nothing is left (or no room); an instance
/// without a clip comes back whole.
fn clip_to(frame: &mut Frame, p: &PathInstance, planes: &[(Vec2, f32)], budget: &mut Budget) -> Option<PathInstance> {
    let mut poly = clip_corners(frame, p);
    if poly.is_empty() {
        return Some(*p);
    }
    for &(n, d) in planes {
        let (n, d) = local_half_plane(p, n, d);
        poly = tidied(cut(&poly, n, d));
        if poly.len() < 3 {
            return None;
        }
    }
    // A sliver with no room inside would draw as a faint hairline.
    let twice_area: f32 = (0..poly.len()).map(|k| poly[k].perp_dot(poly[(k + 1) % poly.len()])).sum();
    let span = poly.iter().fold(0.0f32, |m, q| m.max(q.abs().max_element())).max(1e-6);
    if twice_area.abs() <= 1e-6 * span * span {
        return None;
    }
    if !budget.take(0, poly.len()) {
        return None;
    }
    let first = frame.path_points.len();
    frame.path_points.extend(poly.iter().map(|q| [q.x, q.y, 0.0, 0.0]));
    let mut out = *p;
    out.clip[0] = first as f32;
    out.clip[1] = poly.len() as f32;
    Some(out)
}

/// `p` with `opacity` more of it faded.
fn faded(mut p: PathInstance, opacity: f32) -> PathInstance {
    let keep = (1.0 - p.clip[2].clamp(0.0, 1.0)) * opacity.clamp(0.0, 1.0);
    p.clip[2] = 1.0 - keep;
    p
}

/// `p` turned a further `hue` turns round the colour wheel and colourised at least `colourise`.
fn coloured(mut p: PathInstance, hue: f32, colourise: f32) -> PathInstance {
    if hue == 0.0 && colourise <= 0.0 {
        return p;
    }
    let s = p.style();
    p.set_style(PathStyle { hue: s.hue + finite(hue, 0.0), colourise: s.colourise.max(colourise), ..s });
    p
}

/// `p` drawing only `channel`.
fn channelled(mut p: PathInstance, channel: PathChannel) -> PathInstance {
    let s = p.style();
    p.set_style(PathStyle { channel, ..s });
    p
}

fn moved(mut p: PathInstance, by: Vec2) -> PathInstance {
    p.a[0] += by.x;
    p.a[1] += by.y;
    p
}

/// `p` and its red and blue copies moved `by` either way: a green covering copy in place and two
/// adding light. An instance already drawing one channel stays as it is.
fn split(p: PathInstance, by: Vec2) -> [Option<PathInstance>; 3] {
    if p.style().channel != PathChannel::All {
        return [Some(p), None, None];
    }
    [
        Some(channelled(p, PathChannel::GreenCovering)),
        Some(channelled(moved(p, by), PathChannel::RedLight)),
        Some(channelled(moved(p, -by), PathChannel::BlueLight)),
    ]
}

/// `p` flipped over its own x axis: its points (and clip corners, kept turning the same way)
/// pushed again with y the other way. A fast letter can't flip and stays as it is.
fn flipped(frame: &mut Frame, p: &PathInstance, budget: &mut Budget) -> Option<PathInstance> {
    let mut out = *p;
    let kind = p.b[1];
    let has_points = (0.5..1.5).contains(&kind);
    let (first, n) = (p.b[2].max(0.0) as usize, p.b[3].max(0.0) as usize);
    let corners = clip_corners(frame, p);
    let points = if has_points { n } else { 0 };
    if !budget.take(0, points + corners.len()) {
        return None;
    }
    if has_points && first + n <= frame.path_points.len() {
        let start = frame.path_points.len();
        for k in first..first + n {
            let q = frame.path_points[k];
            frame.path_points.push([q[0], -q[1], q[2], q[3]]);
        }
        out.b[2] = start as f32;
    }
    if !corners.is_empty() {
        let start = frame.path_points.len();
        frame.path_points.extend(corners.iter().rev().map(|q| [q.x, -q.y, 0.0, 0.0]));
        out.clip[0] = start as f32;
    }
    Some(out)
}

/// Echo: each instance's copies toward the point, behind it.
fn echo(e: &EchoEffect, only: &[u32], list: &mut Vec<Inst>, frame: &mut Frame, cx: &FxCx, budget: &mut Budget) {
    let n = e.copies.clamp(1, 64);
    let amount = amount(e.amount);
    if amount <= 0.0 {
        return;
    }
    let t = cx.time as f32;
    let wander = finite(e.wander, 0.0).max(0.0) * Vec2::from_angle(TAU * finite(e.wander_speed, 0.0) * t);
    let reach = finite(e.reach, 0.8).clamp(0.0, 1.0);
    let last = finite(e.last_size, 0.5).max(0.0);
    let last_opacity = finite(e.last_opacity, 0.4).clamp(0.0, 1.0);
    let mut made = Vec::new();
    'all: for inst in list.iter().filter(|i| wears(only, i.letter)) {
        let m = cx.middle(inst.copy);
        let towards = e.toward + wander;
        for j in 1..=n {
            let f = j as f32 / n as f32;
            let s = 1.0 + (last - 1.0) * f;
            let opacity = amount * (1.0 + (last_opacity - 1.0) * f);
            if s <= 1e-4 || opacity <= 0.0 {
                continue;
            }
            if !budget.take(1, 0) {
                break 'all;
            }
            let mut p = inst.p;
            // The layers reach as far in px on a smaller copy, so its own box grows to hold them.
            if inst.boxed && s < 1.0 {
                let reach_px = p.c[2].max(0.0) + p.b[0].max(0.0) + 2.0;
                let unit = if p.a[3].abs() > 1e-6 { p.a[3].abs() } else { 1.0 };
                let by = reach_px / unit * (1.0 / s.max(1.0 / 16.0) - 1.0);
                if let Some(q) = grown_box(frame, &p, by, budget) {
                    p = q;
                }
            }
            let centre = m + (Vec2::new(p.a[0], p.a[1]) - m) * s + towards * f * reach;
            p.a = [centre.x, centre.y, p.a[2], p.a[3] * s];
            p.b[0] *= s;
            p.c[2] *= s;
            let hue = (j as f32 * finite(e.hue, 0.0) + finite(e.hue_spin, 0.0) * t) / 360.0;
            let p = coloured(faded(p, opacity), hue, finite(e.colourise, 0.0).clamp(0.0, 1.0));
            made.push(Inst { p, depth: inst.depth - j as i32, ..*inst });
        }
    }
    list.extend(made);
}

/// Mirror: each instance turned round the centre `ways` ways, odd ones flipped when mirrored,
/// each cut to its wedge when asked.
fn mirror(e: &MirrorEffect, only: &[u32], list: &mut Vec<Inst>, frame: &mut Frame, cx: &FxCx, budget: &mut Budget) {
    let ways = e.ways.clamp(2, 12);
    let amount = amount(e.amount);
    // At no Amount the word shows as it is (not turned, nor cut to its wedge).
    if amount <= 0.0 {
        return;
    }
    let turn = (finite(e.turn, 0.0) + finite(e.spin, 0.0) * cx.time as f32).to_radians();
    let half = PI / ways as f32;
    let mut out = Vec::with_capacity(list.len());
    for inst in list.drain(..) {
        if !wears(only, inst.letter) {
            out.push(inst);
            continue;
        }
        let c = match e.round {
            MirrorRound::TheTextsMiddle => cx.middle(inst.copy),
            MirrorRound::TheMiddleOfThePicture => cx.picture_middle(),
            MirrorRound::APoint => e.centre,
        };
        for k in 0..ways {
            if k > 0 && !budget.take(1, 0) {
                break;
            }
            let beta = turn + k as f32 * TAU / ways as f32;
            let flip = e.mirrored && k % 2 == 1;
            let mut p = inst.p;
            let from = Vec2::new(p.a[0], p.a[1]) - c;
            let r = Vec2::from_angle(beta);
            if flip {
                let Some(f) = flipped(frame, &p, budget) else { continue };
                p = f;
                let centre = c + r.rotate(Vec2::new(from.x, -from.y));
                p.a = [centre.x, centre.y, beta - p.a[2], p.a[3]];
            } else {
                let centre = c + r.rotate(from);
                p.a = [centre.x, centre.y, p.a[2] + beta, p.a[3]];
            }
            if e.wedges {
                let (u1, u2) = (Vec2::from_angle(beta - half), Vec2::from_angle(beta + half));
                let (n1, n2) = (Vec2::new(-u1.y, u1.x), Vec2::new(u2.y, -u2.x));
                // With no room left the letter itself still shows, uncut.
                if k == 0 && budget.points < clip_corners(frame, &p).len() + 2 {
                    out.push(Inst { p, ..inst });
                    continue;
                }
                match clip_to(frame, &p, &[(n1, n1.dot(c)), (n2, n2.dot(c))], budget) {
                    Some(q) => p = q,
                    None => continue,
                }
            }
            if k > 0 {
                p = coloured(
                    faded(p, amount),
                    k as f32 * finite(e.hue, 0.0) / 360.0,
                    finite(e.colourise, 0.0).clamp(0.0, 1.0),
                );
            }
            out.push(Inst { p, boxed: inst.boxed && !e.wedges, ..inst });
        }
    }
    *list = out;
}

/// Colour split: every instance a green covering copy and red and blue copies pulled apart.
fn colour_split(e: &SplitEffect, only: &[u32], list: &mut Vec<Inst>, cx: &FxCx, budget: &mut Budget) {
    let kick = cx.kick(e.harder, &e.hits_of, e.settles);
    let wobble = finite(e.wobble, 0.0).max(0.0) * super::moves::wobble(cx.time, e.wobble_speed, e.seed, 0, 0);
    let d = amount(e.amount) * (finite(e.distance, 0.0).max(0.0) + wobble) * (1.0 + 3.0 * kick);
    if d.abs() < 1e-3 {
        return;
    }
    let by = cx.screen_move(Vec2::from_angle(finite(e.angle, 0.0).to_radians()) * d);
    let mut out = Vec::with_capacity(list.len() * 3);
    for inst in list.drain(..) {
        if !wears(only, inst.letter) || !budget.take(2, 0) {
            out.push(inst);
            continue;
        }
        out.extend(split(inst.p, by).into_iter().flatten().map(|p| Inst { p, ..inst }));
    }
    *list = out;
}

/// Glitch: each instance cut along the torn bands of the screen it spans, the torn ones moved
/// sideways (and split into red and blue).
fn glitch(e: &GlitchEffect, only: &[u32], list: &mut Vec<Inst>, frame: &mut Frame, cx: &FxCx, budget: &mut Budget) {
    let kick = cx.kick(e.harder, &e.hits_of, e.settles);
    let a = amount(e.amount);
    let chance = a + (1.0 - a) * kick;
    let tear = finite(e.tear, 0.0).max(0.0) * (1.0 + 2.0 * kick);
    let tear_split = finite(e.tear_split, 0.0).max(0.0);
    if chance <= 0.0 || (tear <= 0.0 && tear_split <= 0.0) {
        return;
    }
    let band = finite(e.band, 16.0).max(1.0);
    let slot = (cx.time.max(0.0) * finite(e.rate, 0.0).max(0.0) as f64).floor().min(1e12) as i64;
    let dice = |b: i64, n: i64| {
        hash(b.wrapping_mul(1_000_003).wrapping_add(slot.wrapping_mul(64)).wrapping_add(n), e.seed ^ 0x611c)
    };
    let torn = |b: i64| (dice(b, 0) < chance).then(|| (dice(b, 1) * 2.0 - 1.0) * tear);
    // The screen's y as a straight function of the frame's place: y(w) = g · w + h.
    let h0 = cx.to_screen(Vec2::ZERO).y;
    let g = Vec2::new(cx.to_screen(Vec2::X).y - h0, cx.to_screen(Vec2::Y).y - h0);
    let at_least = |y: f32| (g, y - h0);
    let below = |y: f32| (-g, h0 - y);
    let mut out = Vec::with_capacity(list.len());
    for inst in list.drain(..) {
        let corners = clip_corners(frame, &inst.p);
        if !wears(only, inst.letter) || corners.is_empty() {
            out.push(inst);
            continue;
        }
        let ys = corners.iter().map(|q| cx.to_screen(frame_of(&inst.p, *q)).y);
        let (lo, hi) = ys.fold((f32::MAX, f32::MIN), |(lo, hi), y| (lo.min(y), hi.max(y)));
        if !(lo.is_finite() && hi.is_finite()) {
            out.push(inst);
            continue;
        }
        let (b0, b_last) = ((lo / band).floor() as i64, (hi / band).floor() as i64);
        // Past MOST_BANDS bands the rest of the instance stays whole, as one more piece.
        let b1 = b_last.min(b0.saturating_add(MOST_BANDS - 1));
        let capped = b_last > b1;
        let tears: Vec<Option<f32>> = (b0..=b1).map(torn).collect();
        if tears.iter().all(Option::is_none) {
            out.push(inst);
            continue;
        }
        // Room for every piece this instance becomes, or it stays whole.
        let runs = tears.iter().enumerate().filter(|&(k, t)| t.is_some() || k == 0 || tears[k - 1].is_some()).count()
            + capped as usize;
        let torn_count = tears.iter().filter(|t| t.is_some()).count();
        let instances = runs + if tear_split > 0.0 { 2 * torn_count } else { 0 };
        if instances > budget.instances || runs * (corners.len() + 2) > budget.points {
            out.push(inst);
            continue;
        }
        // Runs of untorn bands stay as one piece; each torn band is its own.
        let mut k = 0;
        while k < tears.len() {
            let b = b0 + k as i64;
            let (end, by) = match tears[k] {
                Some(by) => (k, Some(by)),
                None => (k + tears[k..].iter().take_while(|t| t.is_none()).count() - 1, None),
            };
            let mut planes = Vec::with_capacity(2);
            if k > 0 {
                planes.push(at_least(b as f32 * band));
            }
            if end + 1 < tears.len() || capped {
                planes.push(below((b0 + end as i64 + 1) as f32 * band));
            }
            k = end + 1;
            if !budget.take(1, 0) {
                break;
            }
            let Some(piece) = clip_to(frame, &inst.p, &planes, budget) else { continue };
            let inst = Inst { boxed: false, ..inst };
            match by {
                None => out.push(Inst { p: piece, ..inst }),
                Some(by) => {
                    let piece = moved(piece, cx.screen_move(Vec2::new(by, 0.0)));
                    if tear_split > 0.0 && budget.take(2, 0) {
                        let parts = split(piece, cx.screen_move(Vec2::new(tear_split, 0.0)));
                        out.extend(parts.into_iter().flatten().map(|p| Inst { p, ..inst }));
                    } else {
                        out.push(Inst { p: piece, ..inst });
                    }
                }
            }
        }
        if capped
            && budget.take(1, 0)
            && let Some(piece) = clip_to(frame, &inst.p, &[at_least((b1 + 1) as f32 * band)], budget)
        {
            out.push(Inst { p: piece, boxed: false, ..inst });
        }
    }
    *list = out;
}

/// Runs the instance effects of `fx` over the Text's instances pushed from `start` on, each
/// owned by `(copy, letter, clip still its own box)` in `owners`, and puts back what they made: copies further back
/// first, and each covering instance before the light added onto it.
pub fn apply(fx: &Fx, cx: &FxCx, frame: &mut Frame, start: usize, owners: &[(usize, u32, bool)]) {
    if !fx.copies() || start >= frame.paths.len() {
        return;
    }
    let mut list: Vec<Inst> = frame
        .paths
        .drain(start..)
        .zip(owners.iter().copied().chain(std::iter::repeat((0, u32::MAX, false))))
        .map(|(p, (copy, letter, boxed))| Inst { p, copy, letter, depth: 0, boxed })
        .collect();
    let mut budget = Budget { instances: MOST_FX_INSTANCES.saturating_sub(list.len()), points: MOST_FX_POINTS };
    for (e, only) in &fx.list {
        match e {
            Effect::Echo(e) => echo(e, only, &mut list, frame, cx, &mut budget),
            Effect::Mirror(e) => mirror(e, only, &mut list, frame, cx, &mut budget),
            Effect::Split(e) => colour_split(e, only, &mut list, cx, &mut budget),
            Effect::Glitch(e) => glitch(e, only, &mut list, frame, cx, &mut budget),
            Effect::Melt(_) => {}
        }
    }
    list.sort_by_key(|i| (i.depth, i.p.style().channel.adds_light()));
    frame.paths.extend(list.into_iter().map(|i| i.p));
}
