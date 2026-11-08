//! Drawing a Trail layer for each ball: Streaks, Ribbons and Lines as strips, Dots and Smoke as
//! particles (Ghosts are copies of the ball, which the balls draw with themselves: see
//! [`ghosts_of`]). The layer's sub-layers work on what it draws: Recolour, Flash on hit and
//! Colour on hit change its colours; a Shadow, a Glow and an Outline draw more of it, under (the
//! first two) or over (the last) its own.

use bpe_core::Blend;
use bpe_gfx::{DrawCx, Frame, Particle, StreakStyle, push_trail};
use bpe_runtime::{Bodies, History};
use glam::Vec2;

use super::kinds::{self, Cut, Path};
use super::line;
use super::{TrailKind, TrailLayer, TrailSource, Twist};
use crate::modules::look::layers::around::{AroundKind, AroundLayer};
use crate::modules::look::layers::around_subs::{
    Extras, GLOW, HitInfo, OUTLINE, OWN, Pieces, SHADOW, coloured, darker, glow_light, like_layer, outline_reach,
    particle_with_extras, shadow_offset, sub_colour,
};
use crate::modules::look::layers::gpu::SubLayer;
use crate::modules::look::layers::particles::emit::MAX_PARTICLES;
use crate::modules::look::layers::trail::AroundPlace;
use crate::paint::{Follow, Inputs, Paint, colour_at, seed_of};

/// The margin each strip edge keeps for anti-aliasing, in video pixels. The preview shows the
/// video shrunk up to about four times, and the shader softens the edge over one output pixel.
pub const AA_MARGIN: f32 = 4.0;

/// The most points one trail layer's strips run through in a frame, across all its balls (see
/// [`strip_budget`]): a crowd of Lines is simplified more coarsely to fit, other trails are cut
/// to their newest share.
pub const MOST_STRIP_POINTS: usize = 200_000;

/// Particle shapes (as in sprites.wgsl).
const SMOKE: u32 = 1;
const DOTS: u32 = 6;

/// What the trails read about the balls they follow.
pub struct Balls<'a> {
    pub bodies: &'a Bodies,
    pub history: &'a History,
    /// Seconds between the history's samples.
    pub interval: f32,
    /// How old the history's newest sample is, in seconds.
    pub newest_ago: f32,
    /// How far before the history's newest sample the balls are drawn, in seconds: 0 but in a
    /// bake's sample between two steps (lighting part 3), whose trails leave out what is newer.
    pub ahead: f32,
    /// Ball `i`'s colour, linear light (alpha -1 adds its light).
    pub colour: &'a dyn Fn(usize) -> [f32; 4],
    /// Ball `i`'s number in its group.
    pub number: &'a dyn Fn(usize) -> u32,
    /// How far ball `i` had turned `ago` seconds back, in radians, when its look turns.
    pub turned: &'a dyn Fn(usize, f32) -> Option<f32>,
    /// Each ball's last hit: how many seconds ago (`bpe_gfx::NO_HIT`'s age without one) and how
    /// strong, by row.
    pub last_hits: &'a [(f32, f32)],
    /// Ball `i`'s own trails and particles, when the look's settings for them vary across the
    /// balls (see `layers::vary`); none when every ball wears them as the card says.
    pub own: Option<&'a dyn Fn(usize) -> std::rc::Rc<Vec<AroundLayer>>>,
    /// How many seconds ago ball `i` was caught, when it is gone and the emitter that caught it
    /// leaves trails behind (its trail runs out where it was caught); `None` for every other ball.
    pub gone: &'a dyn Fn(usize) -> Option<f32>,
}

impl Balls<'_> {
    /// Ball `i`'s own trails and particles, when they vary across the balls.
    pub fn own_layers(&self, i: usize) -> Option<std::rc::Rc<Vec<AroundLayer>>> {
        self.own.map(|f| f(i))
    }

    fn last_hit(&self, i: usize) -> Option<(f32, f32)> {
        self.last_hits.get(i).copied().filter(|h| h.0 < bpe_gfx::NO_HIT[0])
    }

    fn hit(&self, i: usize) -> HitInfo {
        HitInfo { last: self.last_hit(i), hits: self.bodies.hits.get(i).copied().unwrap_or(0) }
    }

    /// How far ball `i` has rolled along its way, to where it is now: what Dots and a Ribbon
    /// that follows the turning count from, so they stay put along the path wherever its trail
    /// is cut.
    fn rolled(&self, i: usize) -> f32 {
        let newest = self.history.tip(i).map_or(self.bodies.position[i], |t| t.1);
        self.history.travelled(i) + (self.bodies.position[i] - newest).length()
    }

    /// How ball `i`'s trail is cut. A ball caught `gone` seconds ago stands still where it was
    /// caught (its history keeps that place), so its trail runs out into it: `None` once it has,
    /// or once the history no longer reaches back to when it moved.
    fn cut(&self, trail: &TrailLayer, i: usize, gone: Option<f32>) -> Option<Cut> {
        let speed = self.bodies.velocity[i].length();
        let cut = Cut::of(trail, speed, self.last_hit(i).map(|h| h.0));
        let Some(gone) = gone else { return Some(cut) };
        let kept = self.history.cap().saturating_sub(1) as f32 * self.interval;
        if !trail.stays() && gone >= kept {
            return None;
        }
        cut.after_gone(gone, speed)
    }

    /// Ball `i`'s trail as `trail` cuts it, before any wobble; empty for a caught ball's trail
    /// that has run out (see [`Balls::cut`]). A caught ball's still head is left in: see
    /// [`Path::drop_still_head`].
    fn path(&self, trail: &TrailLayer, i: usize, gone: Option<f32>) -> Path {
        match self.cut(trail, i, gone) {
            Some(cut) => {
                let ago = self.newest_ago - self.ahead;
                kinds::path(self.bodies.position[i], self.history, i, ago, self.interval, cut)
            }
            None => Path::default(),
        }
    }
}

/// The trail's colour at a point from `base` (linear light) at its brightness and `fade`,
/// mixed in by the blend amount: covering with the brightness as opacity, or adding that much
/// light. An additive colour glows whatever the blend says. Other blends draw into a layer that
/// is mixed in at its amount later, so here they are plain: Normal at full amount.
pub fn shade(trail: &TrailLayer, base: [f32; 4], fade: f32) -> [f32; 4] {
    let k = trail.brightness.max(0.0);
    let amount = match trail.blend {
        Blend::Normal | Blend::Add => trail.blend_amount.clamp(0.0, 1.0),
        _ => 1.0,
    };
    if trail.blend != Blend::Add && base[3] > 0.0 {
        return [base[0], base[1], base[2], (base[3] * k).min(1.0) * amount * fade];
    }
    let k = k * amount * fade;
    [base[0] * k, base[1] * k, base[2] * k, -1.0]
}

/// A strip colour as a particle's: the sprites add light twice as bright, so half of it.
fn as_particle(c: [f32; 4]) -> [f32; 4] {
    if c[3] < 0.0 { [c[0], c[1], c[2], -0.5] } else { c }
}

/// What the trail's own Colours read at point `k` of `path` (the ball moving at `speed`):
/// Life runs from 0 at the ball to 1 at the tail; Place reads where the point is, Speed how fast
/// the ball went there, and Time when it was there, so each part keeps the colour it was drawn
/// in. `seed` is the ball's own, so the trail varies the way its ball does.
fn own_colour(cx: &DrawCx, paint: &Paint, path: &Path, k: usize, speed: f32, seed: u32) -> [f32; 4] {
    let n = path.len();
    let place = path.points[k];
    let gap = |a: usize, b: usize| {
        let dt = (path.ago[b] - path.ago[a]).max(1e-6);
        (path.points[a] - path.points[b]).length() / dt
    };
    let speed = match k {
        0 => speed,
        _ if k + 1 < n => gap(k, k + 1),
        _ if k >= 2 => gap(k - 1, k),
        _ => speed,
    };
    let screen = cx.resolved.screen;
    let inputs = Inputs {
        count: 1,
        place,
        place_screen: if screen { place } else { cx.world.camera.to_screen(place) },
        start: place,
        speed,
        life: path.along(k),
        time: cx.time as f32 - if paint.follow == Follow::Time { path.ago[k] } else { 0.0 },
        seed,
        ..Inputs::default()
    };
    colour_at(paint, &inputs)
}

/// One ball's trail to draw: which ball, its own copy of the layer when its settings vary, and
/// its path.
struct Drawn {
    i: usize,
    own: Option<std::rc::Rc<Vec<AroundLayer>>>,
    path: Path,
}

/// The layer and trail ball `own` copy says, or the card's own.
fn copy_of<'a>(
    own: &'a Option<std::rc::Rc<Vec<AroundLayer>>>,
    base: &'a AroundLayer,
    base_trail: &'a TrailLayer,
) -> (&'a AroundLayer, &'a TrailLayer) {
    match own.as_ref().and_then(|o| o.iter().find(|a| a.id == base.id)) {
        Some(mine @ AroundLayer { kind: AroundKind::Trail(t), .. }) => (mine, &**t),
        _ => (base, base_trail),
    }
}

/// How many strip points a trail layer may draw in a frame: [`MOST_STRIP_POINTS`], shared with
/// the strips its Shadow, Glow and Outline sub-layers draw again, so the frame never has to cut
/// any off.
pub fn strip_budget(subs: &[SubLayer]) -> usize {
    let again =
        subs.iter().filter(|s| matches!(s, SubLayer::Shadow(_) | SubLayer::Glow(_) | SubLayer::Outline(_))).count();
    MOST_STRIP_POINTS / (1 + again)
}

/// The coarsest a layer's Lines are simplified to fit the budget: at level `l` with `2^l` times
/// the tolerance and the grid spacing (8: 0.15 px to about 38 px).
pub const MOST_LEVEL: u32 = 8;

/// Picks the level a layer's Lines are drawn at: the finest whose points fit `budget`, up to
/// [`MOST_LEVEL`], looking from `start` (where the last frame ended up) so a frame usually works
/// out only its own level and the one finer. Which it picks doesn't hang on `start` (as long as
/// a coarser level never has more points than a finer one), so drawing a time again, scrubbing
/// or exporting draws the same. Gives what `gather` made at that level
/// and the level.
fn pick_level<T>(start: u32, budget: usize, mut gather: impl FnMut(u32) -> (T, usize)) -> (T, u32) {
    let mut level = start.min(MOST_LEVEL);
    let (mut made, mut total) = gather(level);
    // The finest level known not to fit.
    let mut over = None;
    while total > budget && level < MOST_LEVEL {
        over = Some(level);
        // A level usually at most halves the points, so one four times over the budget skips
        // ahead; the levels skipped are looked at again below.
        let skip = (total / budget.max(1)).max(1).ilog2().max(1);
        level = (level + skip).min(MOST_LEVEL);
        (made, total) = gather(level);
    }
    // Finer ones while they fit, down to the one known not to.
    while level > over.map_or(0, |o| o + 1) {
        let (finer, finer_total) = gather(level - 1);
        if finer_total > budget {
            break;
        }
        (made, level) = (finer, level - 1);
    }
    (made, level)
}

/// Cuts each of `drawn` to its newest share of `budget` points when together they are over it:
/// the last resort, once Lines are as coarse as they go (and for every other kind of trail).
fn cut_to(drawn: &mut [Drawn], budget: usize) {
    if drawn.iter().map(|t| t.path.len()).sum::<usize>() > budget {
        let each = budget / drawn.len().max(1);
        for t in drawn.iter_mut() {
            kinds::limit(&mut t.path, each);
        }
    }
}

/// Draws `trail` (the around layer `layer`) for every ball it is worn by, into `frame`.
pub fn draw(cx: &DrawCx, layer: &AroundLayer, trail: &TrailLayer, balls: &Balls, frame: &mut Frame) {
    if trail.kind == TrailKind::Ghosts || (trail.brightness <= 0.0 && balls.own.is_none()) {
        return;
    }
    let mut pieces = Pieces::default();
    let bodies = balls.bodies;
    let time = cx.time as f32;
    let newest = balls.history.newest_step().unwrap_or(0) / balls.history.every();
    // How far a Line's points may stray, in the units it is drawn in. The zoom is taken to the
    // power of two at or below it, so a zooming camera doesn't make the lines start again every
    // frame.
    let tol = if cx.resolved.screen {
        kinds::LINE_TOLERANCE
    } else {
        kinds::LINE_TOLERANCE / cx.world.camera.zoom.abs().clamp(1.0 / 64.0, 64.0).log2().floor().exp2()
    };
    // Dots and puffs share the particles' cap.
    let full = |pieces: &Pieces| pieces.particles[OWN].len() >= MAX_PARTICLES;
    // How many puffs Smoke may fill a gap with: as many as keep every ball's smoke under the cap.
    let puffs_each = (trail.seconds.max(0.0) * trail.puffs.max(0.1)).max(1.0);
    let fill_most = (MAX_PARTICLES as f32 / (bodies.len().max(1) as f32 * puffs_each)).floor().clamp(1.0, 8.0) as usize;
    let (base_layer, base_trail) = (layer, trail);
    let newest_step = balls.history.newest_step().unwrap_or(0);
    // Seconds a step.
    let step_dt = balls.interval / balls.history.every() as f32;
    // Dots and Smoke draw no strips: their paths only place the particles.
    let budget = if matches!(trail.kind, TrailKind::Dots | TrailKind::Smoke) {
        MOST_STRIP_POINTS
    } else {
        strip_budget(&base_layer.subs)
    };
    // Only Lines remember anything between frames.
    let mut memos = (trail.kind == TrailKind::Line).then(line::memos);
    // Every ball's path first, so a crowd can share the points out; Lines at `level`.
    let mut gather = |level: u32| -> (Vec<Drawn>, usize) {
        let (tol, scale) = (tol * (1u32 << level) as f32, 1u64 << level);
        let mut drawn = Vec::new();
        let mut total = 0;
        for i in 0..bodies.len() {
            let gone = if bodies.alive[i] { None } else { (balls.gone)(i) };
            if !(bodies.alive[i] || gone.is_some()) || !base_layer.applies((balls.number)(i)) {
                continue;
            }
            // The ball's own copy of the layer, when its settings vary across the balls.
            let own = balls.own_layers(i);
            let (_, trail) = copy_of(&own, base_layer, base_trail);
            if trail.brightness <= 0.0 || bodies.velocity[i].length() < trail.least_speed {
                continue;
            }
            let waves = (trail.wobble_amount != 0.0).then_some(trail.wobble_waves);
            let line = trail.kind == TrailKind::Line;
            let mut path = if line && trail.stays() && !trail.restart {
                // The whole way since the start, carried on from what earlier frames settled.
                // The grid is spaced by the time it spans while moving: a caught ball's line
                // keeps the same points as it stands.
                let span = balls.history.first(i).map_or(0.0, |(t, _)| {
                    (newest_step as f64 - t) as f32 * step_dt + balls.newest_ago - gone.unwrap_or(0.0)
                });
                let grid = kinds::grid_for(span, balls.interval, waves) * scale;
                let key = (cx.thing.id, base_layer.id, i, level);
                let head = bodies.position[i];
                let (ago, every) = (balls.newest_ago, balls.interval);
                match memos.as_deref_mut() {
                    Some(memos) => line::remembered_line(memos, key, balls.history, i, head, ago, every, tol, grid),
                    None => line::staying_line(&mut None, balls.history, i, head, ago, every, tol, grid),
                }
            } else {
                balls.path(trail, i, gone)
            };
            // A caught ball's trail starts where it last moved: the samples since are all one
            // place.
            if gone.is_some() {
                path.drop_still_head();
            }
            if path.len() < 2 {
                continue;
            }
            if line && !(trail.stays() && !trail.restart) {
                // Over the time it spans, as above.
                let grid = kinds::grid_for(path.reach() - path.ago[0], balls.interval, waves) * scale;
                path = kinds::simplify(&path, tol, grid, newest, balls.newest_ago, balls.interval);
            }
            total += path.len();
            drawn.push(Drawn { i, own, path });
        }
        (drawn, total)
    };
    let mut drawn = if trail.kind == TrailKind::Line {
        let key = (cx.thing.id, base_layer.id);
        let start = line::level_of(key);
        let (drawn, level) = pick_level(start, budget, &mut gather);
        line::set_level(key, level);
        drawn
    } else {
        gather(0).0
    };
    drop(memos);
    cut_to(&mut drawn, budget);
    for Drawn { i, own, mut path, .. } in drawn {
        if full(&pieces) {
            break;
        }
        let (layer, trail) = copy_of(&own, base_layer, base_trail);
        let speed = bodies.velocity[i].length();
        let rolled = balls.rolled(i);
        if matches!(trail.kind, TrailKind::Streak | TrailKind::Ribbon | TrailKind::Line | TrailKind::Dots) {
            kinds::wobble(&mut path, trail.wobble_amount, trail.wobble_waves, trail.wobble_speed, time);
        }
        let radius = bodies.radius[i];
        let seed = seed_of(cx.thing.id, i);
        let ball = (balls.colour)(i);
        let base = |k: usize| match trail.source {
            TrailSource::Ball => ball,
            TrailSource::Own => own_colour(cx, &trail.colour, &path, k, speed, seed),
        };
        let ball_cx = BallCx { trail, layer, balls, i, radius };
        match trail.kind {
            TrailKind::Streak | TrailKind::Ribbon | TrailKind::Line => {
                let n = path.len();
                let mut widths = Vec::with_capacity(n);
                let mut backs = Vec::with_capacity(n);
                for k in 0..n {
                    let s = path.along(k);
                    let (w, back) = match trail.kind {
                        TrailKind::Line => (trail.thickness.max(0.0) * 0.5, false),
                        TrailKind::Ribbon => {
                            let full = kinds::width_at(trail.width * radius, trail.tail_width, trail.width_curve, s);
                            let angle = match trail.twist {
                                Twist::Spins => trail.twist_speed * (time - path.ago[k]) * std::f32::consts::TAU,
                                Twist::FollowsTurning => {
                                    (balls.turned)(i, path.ago[k]).unwrap_or((rolled - path.arc[k]) / radius.max(1e-3))
                                }
                            };
                            let (share, back) = kinds::twist(angle);
                            (full * share, back)
                        }
                        _ => (kinds::width_at(trail.width * radius, trail.tail_width, trail.width_curve, s), false),
                    };
                    widths.push(w);
                    backs.push(back);
                }
                let bases: Vec<[f32; 4]> = (0..n)
                    .map(|k| {
                        let c = base(k);
                        if backs[k] { [c[0] * 0.6, c[1] * 0.6, c[2] * 0.6, c[3]] } else { c }
                    })
                    .collect();
                ball_cx.strips(&path, &widths, &bases, &mut pieces);
            }
            TrailKind::Dots => {
                let length = path.length().max(1e-3);
                let size = trail.dot_size.max(0.0) * radius;
                for arc in kinds::dots(&path, rolled, trail.dot_every) {
                    if full(&pieces) {
                        break;
                    }
                    let Some((at, k)) = path.at_arc(arc) else { continue };
                    ball_cx.particle(at, size, arc / length, base(k), DOTS, 0.0, &mut pieces);
                }
            }
            TrailKind::Smoke => {
                let reach = path.reach().max(1e-3);
                let softness = trail.softness.clamp(0.0, 1.0);
                // A fast ball's puffs are further apart than they are wide, a string of beads:
                // fill each gap with puffs at fixed shares of it, as many as the ball's path
                // between the two puffs needs (that path never changes, so neither do they).
                let every = 1.0 / trail.puffs.max(0.1);
                let born = (trail.width.max(0.0) * radius).max(0.5);
                let mut agos = Vec::new();
                for ago in kinds::puffs(cx.time, trail.puffs, path.reach()) {
                    let older = ago + every;
                    let fill = match (older <= path.reach() && ago >= path.ago[0], path.at_ago(ago), path.at_ago(older))
                    {
                        (true, Some(a), Some(b)) => {
                            ((a.distance(b) / (0.6 * born)).ceil() as usize).clamp(1, fill_most)
                        }
                        _ => 1,
                    };
                    agos.extend((0..fill).map(|j| (ago + every * j as f32 / fill as f32, fill)));
                }
                for (ago, fill) in agos {
                    if full(&pieces) {
                        break;
                    }
                    // No puffs from a caught ball standing still.
                    if ago < path.ago[0] {
                        continue;
                    }
                    let Some(at) = path.at_ago(ago) else { continue };
                    let k = path.ago.partition_point(|&a| a <= ago).clamp(1, path.len()) - 1;
                    let size = trail.width.max(0.0) * radius * (1.0 + trail.spreads.max(0.0) * ago);
                    let at = at - Vec2::new(0.0, trail.rises * ago);
                    // The puffs filling a gap share the one puff's cover between them.
                    let mut colour = base(k);
                    if trail.blend == Blend::Add || colour[3] < 0.0 {
                        for c in &mut colour[..3] {
                            *c /= fill as f32;
                        }
                    } else {
                        colour[3] = 1.0 - (1.0 - colour[3].min(1.0)).powf(1.0 / fill as f32);
                    }
                    ball_cx.particle(at, size, ago / reach, colour, SMOKE, softness, &mut pieces);
                }
            }
            TrailKind::Ghosts => {}
        }
    }
    pieces.give_light(layer.emit);
    pieces.into_frame(frame, base_trail.place == AroundPlace::InFront);
}

/// One ball's part of a trail layer.
struct BallCx<'a> {
    trail: &'a TrailLayer,
    layer: &'a AroundLayer,
    balls: &'a Balls<'a>,
    i: usize,
    radius: f32,
}

impl BallCx<'_> {
    /// The strip through `path`, `widths` half wide and `bases` coloured at each point, with
    /// what its sub-layers add.
    fn strips(&self, path: &Path, widths: &[f32], bases: &[[f32; 4]], out: &mut Pieces) {
        let look =
            StripLook { trail: self.trail, subs: &self.layer.subs, hit: self.balls.hit(self.i), radius: self.radius };
        strip_with_subs(&look, path, widths, bases, 0.0, out);
    }

    /// One particle of the trail (a dot or a puff) at `at`, `size` half wide, `s` of the way
    /// back, coloured `base`, with what its sub-layers add.
    #[allow(clippy::too_many_arguments)]
    fn particle(&self, at: Vec2, size: f32, s: f32, base: [f32; 4], shape: u32, soft: f32, out: &mut Pieces) {
        let (trail, subs) = (self.trail, &self.layer.subs);
        let fade = trail.fade.at(s);
        let hit = self.balls.hit(self.i);
        let own = shade(trail, coloured(subs, subs.len(), base, hit), fade);
        let mut p = Particle::new(shape, at, 0.0, size, as_particle(own));
        p.style[2] = soft;
        let outline = trail.brightness.clamp(0.0, 1.0) * 2.0;
        let extras = Extras { subs, base, s, fade, unit: self.radius, hit, outline, added: 0.5 };
        particle_with_extras(p, &extras, out);
    }
}

/// What a strip and its sub-layers are drawn with: the trail's look (or one standing in for
/// it), its shown sub-layers, the hit its colour sub-layers read, and the size its sub-layers'
/// reaches are shares of.
pub(crate) struct StripLook<'a> {
    pub trail: &'a TrailLayer,
    pub subs: &'a [SubLayer],
    pub hit: HitInfo,
    pub radius: f32,
}

/// The strip through `path`, `widths` half wide and `bases` coloured at each point, its own edge
/// `softness` soft, with what its sub-layers add.
pub(crate) fn strip_with_subs(
    look: &StripLook,
    path: &Path,
    widths: &[f32],
    bases: &[[f32; 4]],
    softness: f32,
    out: &mut Pieces,
) {
    let (trail, subs) = (look.trail, look.subs);
    let n = path.len();
    let fade: Vec<f32> = (0..n).map(|k| trail.fade.at(path.along(k))).collect();
    let head = widths.first().copied().unwrap_or(0.0).max(1e-3);
    let colours = |upto: usize| -> Vec<[f32; 4]> { (0..n).map(|k| coloured(subs, upto, bases[k], look.hit)).collect() };
    let own = colours(subs.len());
    let shaded: Vec<[f32; 4]> = (0..n).map(|k| shade(trail, own[k], fade[k])).collect();
    let adds = shaded.first().is_some_and(|c| c[3] < 0.0);
    let plain = StreakStyle { softness, band: 0.0 };
    push_trail(&mut out.strips[OWN], &path.points, widths, &shaded, plain, AA_MARGIN);
    for (at, sub) in subs.iter().enumerate() {
        match sub {
            SubLayer::Shadow(s) if s.opacity > 0.0 => {
                let off = shadow_offset(s, look.radius);
                let points: Vec<Vec2> = path.points.iter().map(|p| *p + off).collect();
                let c = s.colour.to_linear();
                let colours: Vec<[f32; 4]> =
                    fade.iter().map(|f| [c[0], c[1], c[2], s.opacity.clamp(0.0, 1.0) * f]).collect();
                let softness = (s.softness.max(0.0) * look.radius / head).min(1.0);
                let style = StreakStyle { softness, band: 0.0 };
                push_trail(&mut out.strips[SHADOW], &points, widths, &colours, style, AA_MARGIN);
            }
            SubLayer::Glow(g) if g.strength > 0.0 && g.opacity > 0.0 => {
                let same = colours(at);
                let reach = g.size.max(0.0) * look.radius;
                let wide: Vec<f32> = widths.iter().map(|w| w + reach).collect();
                let light: Vec<[f32; 4]> = (0..n)
                    .map(|k| glow_light(g, sub_colour(g.colour_from, &g.colour, path.along(k), same[k]), fade[k]))
                    .collect();
                let style = StreakStyle { softness: 1.0, band: 0.0 };
                push_trail(&mut out.strips[GLOW], &path.points, &wide, &light, style, AA_MARGIN);
            }
            SubLayer::Outline(o) if o.width > 0.0 && o.opacity > 0.0 => {
                let same = colours(at);
                let (_, band) = outline_reach(o, head);
                let wide: Vec<f32> = widths.iter().map(|w| outline_reach(o, *w).0).collect();
                let cs: Vec<[f32; 4]> = (0..n)
                    .map(|k| {
                        let c = sub_colour(o.colour_from, &o.colour, path.along(k), darker(same[k]));
                        let alpha = o.opacity.clamp(0.0, 1.0) * fade[k] * trail.brightness.clamp(0.0, 1.0) * 2.0;
                        like_layer(adds, c, alpha.min(1.0))
                    })
                    .collect();
                let style = StreakStyle { softness: 0.0, band };
                push_trail(&mut out.strips[OUTLINE], &path.points, &wide, &cs, style, AA_MARGIN);
            }
            _ => {}
        }
    }
}

/// One Ghost: where it stands and how opaque it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ghost {
    pub at: Vec2,
    pub opacity: f32,
}

/// Ball `i`'s Ghosts for `trail`, oldest first (so newer ones draw over older), none when it
/// doesn't wear them or is too slow.
pub fn ghosts_of(layer: &AroundLayer, trail: &TrailLayer, balls: &Balls, i: usize) -> Vec<Ghost> {
    if trail.kind != TrailKind::Ghosts || !layer.applies((balls.number)(i)) {
        return Vec::new();
    }
    if balls.bodies.velocity[i].length() < trail.least_speed {
        return Vec::new();
    }
    let path = balls.path(trail, i, None);
    let strength = (trail.brightness.max(0.0) / 0.5).min(1.0);
    let mut out: Vec<Ghost> = kinds::ghosts(trail.ghosts, trail.ghost_every)
        .into_iter()
        .filter_map(|(ago, s)| {
            let at = path.at_ago(ago)?;
            let opacity = trail.fade.at(s) * strength;
            (opacity > 1e-3).then_some(Ghost { at, opacity })
        })
        .collect();
    out.reverse();
    out
}

/// What a Ghost's sub-layers add round it: a soft shadow and a glow under it (`under`), an
/// outline ring over it (`over`). `colour` is the ball's, `radius` its size.
pub fn ghost_extras(
    layer: &AroundLayer,
    ghost: Ghost,
    radius: f32,
    colour: [f32; 4],
    under: &mut Vec<Particle>,
    over: &mut Vec<Particle>,
) {
    for sub in &layer.subs {
        match sub {
            SubLayer::Shadow(s) if s.opacity > 0.0 => {
                let c = s.colour.to_linear();
                let colour = [c[0], c[1], c[2], s.opacity.clamp(0.0, 1.0) * ghost.opacity];
                let mut q = Particle::new(DOTS, ghost.at + shadow_offset(s, radius), 0.0, radius, colour);
                q.style[2] = s.softness.clamp(0.0, 1.0);
                under.push(q);
            }
            SubLayer::Glow(g) if g.strength > 0.0 && g.opacity > 0.0 => {
                let c = sub_colour(g.colour_from, &g.colour, 0.0, colour);
                let light = glow_light(g, c, ghost.opacity);
                let mut q = Particle::new(DOTS, ghost.at, 0.0, radius * (1.0 + g.size.max(0.0)), as_particle(light));
                q.style[2] = 1.0;
                under.push(q);
            }
            SubLayer::Outline(o) if o.width > 0.0 && o.opacity > 0.0 => {
                let (reach, band) = outline_reach(o, radius);
                let c = sub_colour(o.colour_from, &o.colour, 0.0, darker(colour));
                let mut q = Particle::new(DOTS, ghost.at, 0.0, reach, [c[0], c[1], c[2], o.opacity * ghost.opacity]);
                q.style[3] = band;
                over.push(q);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::modules::look::layers::{SUB_GLOW, SUB_OUTLINE, SUB_RECOLOUR, SUB_SHADOW, TRAIL};
    use crate::{kit, new_scene};
    use bpe_core::{LayerId, Scene, SlotId, ThingId, Value, layered_key};
    use bpe_gfx::{Frame, build_frame};
    use bpe_runtime::Playback;

    const BALL: ThingId = ThingId(2);

    /// The test scene with a trail layer on its ball, set by `f`.
    fn with_trail(f: impl FnOnce(&mut Scene, LayerId)) -> Scene {
        let kit = kit();
        let mut scene = new_scene(&kit.registry);
        let id = scene.add_layer(&kit.registry, BALL, SlotId::Look, TRAIL).unwrap();
        f(&mut scene, id);
        scene
    }

    fn set(scene: &mut Scene, id: LayerId, key: &str, v: Value) {
        scene.card_mut(BALL, SlotId::Look).unwrap().params.set(&layered_key(id, key), v);
    }

    fn choice(s: &str) -> Value {
        Value::Choice(s.into())
    }

    /// The frame at `time`, with the playback that made it.
    fn frame_at(scene: &Scene, time: f64) -> (Frame, Playback) {
        let kit = kit();
        let resolved = bpe_core::resolve(scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
        playback.seek(time);
        let mut frame = Frame::default();
        build_frame(scene, &resolved, playback.world(), playback.log(), time, &kit.draws, &mut frame);
        (frame, playback)
    }

    fn trail_frame(time: f64, f: impl FnOnce(&mut Scene, LayerId)) -> Frame {
        frame_at(&with_trail(f), time).0
    }

    /// How far a ball has rolled counts to where it is, however short its trail is cut: a trail
    /// that restarts at a bounce a moment ago doesn't move its Dots along.
    #[test]
    fn how_far_a_ball_rolled_does_not_hang_on_the_cut() {
        use super::{Balls, TrailLayer};
        use glam::Vec2;
        let mut bodies = bpe_runtime::Bodies::default();
        bodies.push(Vec2::ZERO, Vec2::new(600.0, 0.0), 10.0);
        let mut history = bpe_runtime::History::new(240, bpe_runtime::HistoryWant { seconds: 2.0, forever: false });
        history.note(0, 1);
        for step in 1..=40u64 {
            bodies.position[0] = Vec2::new(step as f32 * 2.5, 0.0);
            history.record(step, &bodies.position);
        }
        // Two steps on from the newest sample, 5 px further.
        bodies.position[0] = Vec2::new(105.0, 0.0);
        let colour = |_: usize| [1.0; 4];
        let number = |i: usize| i as u32;
        let turned = |_: usize, _: f32| None;
        let hits = [(0.001, 1.0)];
        let balls = Balls {
            bodies: &bodies,
            history: &history,
            interval: 1.0 / 60.0,
            newest_ago: 2.0 / 240.0,
            ahead: 0.0,
            colour: &colour,
            number: &number,
            turned: &turned,
            last_hits: &hits,
            own: None,
            gone: &|_| None,
        };
        let trail = TrailLayer { restart: true, ..TrailLayer::default() };
        let path = balls.path(&trail, 0, None);
        assert_eq!(path.len(), 2, "cut short of the newest sample");
        let rolled = balls.rolled(0);
        // From the first sample (10 px) to the newest (100 px), then on to the ball.
        assert!((rolled - 95.0).abs() < 1e-3, "{rolled}");
    }

    /// A bake's sample between two steps draws the ball moved back (lighting part 3): its trail
    /// starts there and leaves out the newest sample, which is ahead of it.
    #[test]
    fn a_ball_drawn_between_steps_has_no_trail_ahead_of_it() {
        use super::{Balls, TrailLayer};
        use glam::Vec2;
        let mut bodies = bpe_runtime::Bodies::default();
        bodies.push(Vec2::ZERO, Vec2::new(600.0, 0.0), 10.0);
        let mut history = bpe_runtime::History::new(240, bpe_runtime::HistoryWant { seconds: 2.0, forever: false });
        history.note(0, 1);
        for step in 1..=40u64 {
            bodies.position[0] = Vec2::new(step as f32 * 2.5, 0.0);
            history.record(step, &bodies.position);
        }
        // Half a step before the newest sample (100 px).
        bodies.position[0] = Vec2::new(98.75, 0.0);
        let colour = |_: usize| [1.0; 4];
        let number = |i: usize| i as u32;
        let turned = |_: usize, _: f32| None;
        let balls = Balls {
            bodies: &bodies,
            history: &history,
            interval: 1.0 / 60.0,
            newest_ago: 0.0,
            ahead: 0.5 / 240.0,
            colour: &colour,
            number: &number,
            turned: &turned,
            last_hits: &[],
            own: None,
            gone: &|_| None,
        };
        let path = balls.path(&TrailLayer::default(), 0, None);
        assert_eq!(path.points[0], Vec2::new(98.75, 0.0));
        assert!(path.points.iter().all(|p| p.x <= 98.75), "nothing ahead: {:?}", &path.points[..3]);
        assert!(path.len() > 2, "the rest of the trail is there");
        assert!(path.ago.windows(2).all(|w| w[1] >= w[0]), "older along the trail");
    }

    /// A fast ball's trail turns where the ball bounced: through the very place it touched the
    /// shape, coming in the way it came, not cutting across from the sample before the bounce to
    /// the one after.
    #[test]
    fn a_fast_ball_s_trail_turns_where_it_bounced() {
        use super::kinds::{self, Cut};
        let mut scene = with_trail(|s, id| set(s, id, "seconds", Value::Number(1.0)));
        scene.things.get_mut(&BALL).unwrap().params.set("velocity", Value::Vec2([2800.0, -1900.0]));
        for time in [1.0, 1.51, 2.37] {
            let (_, playback) = frame_at(&scene, time);
            let world = playback.world();
            let history = &world.history[&BALL];
            let dt = world.dt();
            let newest_ago = (time - history.newest_step().unwrap() as f64 * dt as f64) as f32;
            let cut = Cut { seconds: Some(0.9), ..Cut::default() };
            let path = kinds::path(world.bodies[&BALL].position[0], history, 0, newest_ago, history.interval(dt), cut);
            let radius = world.bodies[&BALL].radius[0];
            let rate = world.steps_per_second;
            let hits: Vec<_> = playback
                .log()
                .since(0)
                .iter()
                .filter(|e| e.kind == bpe_runtime::EventKind::Hit && e.a.thing == BALL)
                .filter(|e| (0.05..0.85).contains(&(time - e.time(rate))))
                .collect();
            assert!(hits.len() >= 2, "{} bounces before {time} s", hits.len());
            for e in hits {
                let centre = e.point + e.normal * radius;
                let (k, gap) = path
                    .points
                    .iter()
                    .enumerate()
                    .map(|(k, p)| (k, (*p - centre).length()))
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .unwrap();
                assert!(gap < 1.0, "the trail misses the bounce at {:.3} s by {gap} px", e.time(rate));
                // The moment it touched, on the way it came in, just before.
                let mut m = k;
                while path.corner.get(m + 1) == Some(&true) && (path.points[m + 1] - centre).length() < 20.0 {
                    m += 1;
                }
                let touch = path.points[m];
                assert!(((touch - centre).dot(e.normal)).abs() < 0.5, "it touched the shape there");
                // Coming in: from the point before to the touch, the way the two points before
                // that went.
                let (a, b) = (path.points[m + 2], path.points[m + 1]);
                let turn = (b - a).normalize().dot((touch - b).normalize());
                assert!(turn > 0.999, "the way in turns {:.1}° before the bounce", turn.acos().to_degrees());
                // And the trail goes out of the bounce away from the wall.
                assert!((path.points[k - 1] - path.points[k]).dot(e.normal) > 0.0);
            }
        }
    }

    #[test]
    fn a_streak_follows_its_length_by_time_or_distance_and_grows_when_fast() {
        let half = trail_frame(2.0, |_, _| {});
        let quarter = trail_frame(2.0, |s, id| set(s, id, "seconds", Value::Number(0.25)));
        assert!(!half.trails.is_empty() && half.trails.len() % 6 == 0);
        // Half a second at 60 samples a second: about 31 segments of 6 vertices.
        assert!(half.trails.len() <= 32 * 6, "{}", half.trails.len());
        let ratio = quarter.trails.len() as f32 / half.trails.len() as f32;
        assert!((0.4..0.6).contains(&ratio), "{ratio}");
        let near = trail_frame(2.0, |s, id| {
            set(s, id, "length_by", choice("distance"));
            set(s, id, "pixels", Value::Number(100.0));
        });
        let far = trail_frame(2.0, |s, id| {
            set(s, id, "length_by", choice("distance"));
            set(s, id, "pixels", Value::Number(400.0));
        });
        assert!(far.trails.len() > 2 * near.trails.len(), "{} {}", far.trails.len(), near.trails.len());
        let fast = trail_frame(2.0, |s, id| set(s, id, "faster", Value::Number(2.0)));
        assert!(fast.trails.len() > half.trails.len());
        // Too slow to show.
        let hidden = trail_frame(2.0, |s, id| set(s, id, "least_speed", Value::Number(100_000.0)));
        assert!(hidden.trails.is_empty());
    }

    #[test]
    fn a_trail_that_restarts_at_each_bounce_starts_at_the_last_hit() {
        let scene = with_trail(|s, id| set(s, id, "seconds", Value::Number(2.0)));
        let (_, playback) = frame_at(&scene, 6.0);
        let rate = playback.world().steps_per_second;
        let hit = playback
            .log()
            .since(0)
            .iter()
            .find(|e| e.kind == bpe_runtime::EventKind::Hit && e.a.thing == BALL)
            .map(|e| e.time(rate))
            .expect("the ball hits the shape in 6 s");
        let at = hit + 0.1;
        let whole = frame_at(&scene, at).0;
        let restart = with_trail(|s, id| {
            set(s, id, "seconds", Value::Number(2.0));
            set(s, id, "restart", Value::Bool(true));
        });
        let cut = frame_at(&restart, at).0;
        assert!(!cut.trails.is_empty());
        assert!(cut.trails.len() <= 8 * 6, "{} vertices in 0.1 s", cut.trails.len());
        assert!(whole.trails.len() > cut.trails.len());
    }

    #[test]
    fn a_ribbon_twists_its_width_by_cos() {
        let plain = trail_frame(2.0, |_, _| {});
        let ribbon = trail_frame(2.0, |s, id| {
            set(s, id, "kind", choice("ribbon"));
            set(s, id, "twist", choice("spins"));
            set(s, id, "twist_speed", Value::Number(2.0));
        });
        let widths = |f: &Frame| f.trails.iter().map(|v| v.edge[1]).collect::<Vec<f32>>();
        let full = widths(&plain).into_iter().fold(0.0, f32::max);
        let w = widths(&ribbon);
        assert!(w.iter().all(|x| *x <= full + 1e-3));
        assert!(w.iter().any(|x| *x < 0.3 * full), "it turns edge on");
        assert!(w.iter().any(|x| *x > 0.9 * full), "and face on");
    }

    #[test]
    fn dots_sit_evenly_along_the_way() {
        let f = trail_frame(2.0, |s, id| {
            set(s, id, "kind", choice("dots"));
            set(s, id, "dot_every", Value::Number(40.0));
        });
        assert!(f.trails.is_empty());
        assert!(f.particles.len() >= 3, "{}", f.particles.len());
        let mut gaps: Vec<f32> = f.particles.windows(2).map(|w| (w[0].at() - w[1].at()).length()).collect();
        gaps.sort_by(f32::total_cmp);
        let median = gaps[gaps.len() / 2];
        assert!((median - 40.0).abs() < 4.0, "{gaps:?}");
        assert!(f.particles.iter().all(|p| p.shape[0] == super::DOTS));
    }

    #[test]
    fn smoke_on_a_crowd_stops_at_the_particles_cap() {
        let f = trail_frame(3.0, |s, id| {
            set(s, id, "kind", choice("smoke"));
            set(s, id, "puffs", Value::Number(120.0));
            set(s, id, "seconds", Value::Number(2.0));
            let p = &mut s.things.get_mut(&BALL).unwrap().params;
            p.set("group", Value::Bool(true));
            p.set("count", Value::Int(40));
        });
        assert_eq!(f.particles.len(), super::MAX_PARTICLES);
    }

    #[test]
    fn smoke_puffs_are_the_same_whenever_the_frame_is_drawn() {
        let scene = with_trail(|s, id| set(s, id, "kind", choice("smoke")));
        let first = frame_at(&scene, 2.0).0;
        assert!(!first.particles.is_empty());
        assert!(first.particles.iter().all(|p| p.shape[0] == super::SMOKE && p.style[2] > 0.0));
        let again = frame_at(&scene, 2.0).0;
        assert_eq!(first.particles, again.particles);
        // Drawing an earlier time first changes nothing.
        let kit = kit();
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        let mut frame = Frame::default();
        for t in [1.5, 2.0] {
            playback.seek(t);
            build_frame(&scene, &resolved, playback.world(), playback.log(), t, &kit.draws, &mut frame);
        }
        assert_eq!(frame.particles, first.particles);
        // Older puffs are bigger and higher than where the ball was.
        let sizes: Vec<f32> = first.particles.iter().map(|p| p.place[3]).collect();
        assert!(sizes.windows(2).all(|w| w[0] <= w[1] + 1e-4), "{sizes:?}");
    }

    #[test]
    fn smoke_on_a_fast_ball_is_a_trail_not_a_string_of_beads() {
        let f = trail_frame(2.0, |s, id| {
            set(s, id, "kind", choice("smoke"));
            set(s, id, "spreads", Value::Number(0.0));
            set(s, id, "rises", Value::Number(0.0));
            s.things.get_mut(&BALL).unwrap().params.set("velocity", Value::Vec2([1800.0, -600.0]));
        });
        assert!(f.particles.len() > 10, "{}", f.particles.len());
        // Each puff overlaps the next: no gap wider than a puff is round.
        for w in f.particles.windows(2) {
            let gap = (w[0].at() - w[1].at()).length();
            assert!(gap <= w[0].place[3].min(w[1].place[3]), "a gap of {gap} between puffs {:?}", w);
        }
    }

    #[test]
    fn ghosts_are_fading_copies_of_the_ball_behind_or_in_front() {
        let f = trail_frame(2.0, |s, id| {
            set(s, id, "kind", choice("ghosts"));
            set(s, id, "ghosts", Value::Int(4));
        });
        assert!(f.trails.is_empty() && f.particles.is_empty());
        assert_eq!(f.layered.len(), 5, "four ghosts and the ball");
        let opacity: Vec<f32> = f.layered.iter().map(|l| l.misc[0]).collect();
        assert_eq!(opacity[4], 1.0, "the ball last, over its ghosts");
        assert!(opacity.windows(2).all(|w| w[0] < w[1]), "older ones fainter: {opacity:?}");
        assert!((opacity[3] - 0.8).abs() < 1e-4, "{opacity:?}");
        assert_eq!(f.layered_runs.len(), 1);
        assert_eq!((f.layered_runs[0].start, f.layered_runs[0].end), (0, 5));
        let front = trail_frame(2.0, |s, id| {
            set(s, id, "kind", choice("ghosts"));
            set(s, id, "ghosts", Value::Int(4));
            set(s, id, "place", choice("in_front"));
        });
        assert_eq!(front.layered[0].misc[0], 1.0, "the ball first, its ghosts over it");
    }

    #[test]
    fn behind_and_in_front_land_in_their_own_lists() {
        let behind = trail_frame(2.0, |_, _| {});
        assert!(!behind.trails.is_empty() && behind.trails_front.is_empty());
        let front = trail_frame(2.0, |s, id| set(s, id, "place", choice("in_front")));
        assert!(front.trails.is_empty() && !front.trails_front.is_empty());
        assert_eq!(front.trails_front, behind.trails);
    }

    #[test]
    fn sub_layers_add_a_shadow_and_glow_under_and_an_outline_over() {
        let reg = crate::registry();
        let plain = trail_frame(2.0, |_, _| {});
        let n = plain.trails.len();
        let f = trail_frame(2.0, |s, id| {
            // In the box: outline first, so order in the list is not what decides.
            s.add_sub_layer(&reg, BALL, SlotId::Look, id, SUB_OUTLINE).unwrap();
            s.add_sub_layer(&reg, BALL, SlotId::Look, id, SUB_GLOW).unwrap();
            s.add_sub_layer(&reg, BALL, SlotId::Look, id, SUB_SHADOW).unwrap();
        });
        assert_eq!(f.trails.len(), 4 * n);
        let part = |k: usize| &f.trails[k * n..(k + 1) * n];
        // The shadow: black, covering, soft.
        assert!(part(0).iter().all(|v| v.color[3] >= 0.0 && v.color[..3] == [0.0; 3]));
        // The glow: added light, soft from the middle out, wider.
        assert!(part(1).iter().all(|v| v.color[3] < 0.0 && v.style == [1.0, 0.0]));
        assert!(part(1)[0].edge[1] > plain.trails[0].edge[1]);
        // Its own, as it was.
        assert_eq!(part(2), &plain.trails[..]);
        // The outline: a band, wider than the trail.
        assert!(part(3).iter().all(|v| v.style[1] > 0.0));
        assert!(part(3)[0].edge[1] > plain.trails[0].edge[1]);
    }

    #[test]
    fn a_recolour_sub_layer_changes_the_trail_s_colours() {
        let reg = crate::registry();
        let f = trail_frame(2.0, |s, id| {
            let sub = s.add_sub_layer(&reg, BALL, SlotId::Look, id, SUB_RECOLOUR).unwrap();
            set(s, sub, "tint", Value::Color(bpe_core::Color::rgb(0, 0, 255)));
            set(s, sub, "amount", Value::Number(1.0));
        });
        let c = f.trails[0].color;
        assert_eq!(c[3], -1.0, "still added light");
        assert!(c[0] < 1e-4 && c[1] < 1e-4 && c[2] > 0.3, "{c:?}");
    }

    /// A shooter firing three balls at a catcher that takes them out (at about 0.43 s), its balls
    /// wearing a half-second Streak set by `f`; the catcher leaves trails when `leave` says so.
    fn caught_trails(leave: bool, f: impl FnOnce(&mut dyn FnMut(&str, Value))) -> Scene {
        use crate::things::emitter::test_scenes::shooter_and_catcher;
        let (mut scene, shooter, catcher, kit) = shooter_and_catcher(3, |_| {});
        scene.things.get_mut(&catcher).unwrap().params.set("leave_trails", Value::Bool(leave));
        let id = scene.add_layer(&kit.registry, shooter, SlotId::Look, TRAIL).unwrap();
        let card = scene.card_mut(shooter, SlotId::Look).unwrap();
        card.params.set(&layered_key(id, "seconds"), Value::Number(0.5));
        card.params.set(&layered_key(id, "faster"), Value::Number(0.0));
        f(&mut |key, v| card.params.set(&layered_key(id, key), v));
        scene
    }

    fn vertices(scene: &Scene, time: f64) -> usize {
        let f = frame_at(scene, time).0;
        f.trails.len() + f.trails_front.len()
    }

    #[test]
    fn a_caught_ball_s_trail_goes_with_it_unless_the_catcher_leaves_trails() {
        let off = caught_trails(false, |_| {});
        let on = caught_trails(true, |_| {});
        assert!(vertices(&off, 0.4) > 0 && vertices(&off, 0.4) == vertices(&on, 0.4), "the same before the catch");
        assert_eq!(vertices(&off, 0.5), 0, "gone with the balls");
        // It runs out into the catch point over its half second, then is gone.
        let (a, b, c) = (vertices(&on, 0.5), vertices(&on, 0.75), vertices(&on, 1.0));
        assert!(a > b && b > 0, "{a} {b}");
        assert_eq!(c, 0);
        // What is left stays where the balls were caught: it reaches no further on.
        let right = |time: f64| {
            let f = frame_at(&on, time).0;
            f.trails.iter().chain(&f.trails_front).map(|v| v.position[0]).fold(f32::MIN, f32::max)
        };
        assert!(right(0.7) <= right(0.5) + 1e-3, "{} {}", right(0.7), right(0.5));
    }

    #[test]
    fn a_left_trail_cut_by_distance_runs_out_at_the_speed_its_ball_went() {
        // 100 px at 400 px/s: run out a quarter of a second after the catch.
        let on = caught_trails(true, |set| {
            set("length_by", Value::Choice("distance".into()));
            set("pixels", Value::Number(100.0));
        });
        assert!(vertices(&on, 0.5) > vertices(&on, 0.6));
        assert!(vertices(&on, 0.6) > 0);
        assert_eq!(vertices(&on, 0.75), 0);
    }

    /// Being caught isn't a bounce a left trail restarts at: a trail that restarts at each bounce
    /// still runs out, and a Line that restarts and stays still stays.
    #[test]
    fn a_left_trail_that_restarts_at_bounces_doesnt_restart_at_the_catch() {
        let streak = caught_trails(true, |set| set("restart", Value::Bool(true)));
        assert!(vertices(&streak, 0.6) > 0);
        let line = caught_trails(true, |set| {
            set("kind", Value::Choice("line".into()));
            set("forever", Value::Bool(true));
            set("restart", Value::Bool(true));
        });
        assert!(vertices(&line, 2.0) > 0);
    }

    #[test]
    fn a_left_line_that_stays_stays() {
        let line = |set: &mut dyn FnMut(&str, Value)| {
            set("kind", Value::Choice("line".into()));
            set("forever", Value::Bool(true));
        };
        let (on, off) = (caught_trails(true, line), caught_trails(false, line));
        assert!(vertices(&on, 3.0) > 0);
        assert_eq!(vertices(&on, 3.0), vertices(&on, 1.0));
        assert_eq!(vertices(&off, 3.0), 0);
    }

    /// 1200 balls launched out from a ring, each with a Line that stays, and `f`'s changes.
    fn crowd_of_lines(f: impl FnOnce(&mut Scene, LayerId)) -> Scene {
        with_trail(|s, id| {
            set(s, id, "kind", choice("line"));
            set(s, id, "forever", Value::Bool(true));
            let p = &mut s.things.get_mut(&BALL).unwrap().params;
            p.set("group", Value::Bool(true));
            p.set("shape", choice("ring"));
            p.set("count", Value::Int(1200));
            p.set("ring_radius", Value::Number(420.0));
            p.set("radius", Value::Number(1.0));
            p.set("launch", choice("outward"));
            p.set("velocity", Value::Vec2([700.0, 0.0]));
            f(s, id);
        })
    }

    /// How many of the 1200 balls' first samples have no drawn trail vertex near them.
    fn starts_missed(frame: &Frame, playback: &Playback) -> usize {
        let history = &playback.world().history[&BALL];
        let starts: Vec<glam::Vec2> = (0..1200).map(|i| history.points(i).last().unwrap()).collect();
        let drawn: Vec<glam::Vec2> =
            frame.trails.iter().chain(&frame.trails_front).map(|v| glam::Vec2::from(v.position)).collect();
        assert!(!drawn.is_empty());
        starts.iter().filter(|s| !drawn.iter().any(|d| d.distance(**s) < 6.0)).count()
    }

    /// A crowd of Lines that stay all reach back to where their balls started, however many
    /// balls there are: none is cut short to share out the points.
    #[test]
    fn a_crowd_of_lines_that_stay_all_reach_their_start() {
        let (frame, playback) = frame_at(&crowd_of_lines(|_, _| {}), 12.0);
        let missing = starts_missed(&frame, &playback);
        assert_eq!(missing, 0, "{missing} of 1200 lines don't reach their start");
    }

    /// With a Glow and an Outline drawing each line twice more, the crowd still fits what the
    /// frame can hold, so the frame's last guard cuts none of it off.
    #[test]
    fn a_crowd_of_lines_with_sub_layers_fits_the_frame() {
        let reg = kit().registry;
        let scene = crowd_of_lines(|s, id| {
            s.add_sub_layer(&reg, BALL, SlotId::Look, id, SUB_GLOW).unwrap();
            s.add_sub_layer(&reg, BALL, SlotId::Look, id, SUB_OUTLINE).unwrap();
        });
        let (frame, playback) = frame_at(&scene, 12.0);
        let vertices = frame.trails.len() + frame.trails_front.len();
        assert!(vertices <= bpe_gfx::MOST_TRAIL_VERTICES, "{vertices}");
        assert_eq!(starts_missed(&frame, &playback), 0);
    }

    /// How long a frame of the crowd takes to build a minute in (run by hand, in release).
    #[test]
    #[ignore]
    fn time_a_crowd_of_staying_lines() {
        let scene = crowd_of_lines(|_, _| {});
        let kit = kit();
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let mut playback = Playback::new(&scene, &resolved, kit.sims.clone());
        let mut frame = Frame::default();
        for time in [60.0, 60.0, 60.02, 60.04, 59.5, 55.0, 60.06] {
            playback.seek(time);
            let start = std::time::Instant::now();
            build_frame(&scene, &resolved, playback.world(), playback.log(), time, &kit.draws, &mut frame);
            eprintln!("frame at {time}: {:?}, {} trail vertices", start.elapsed(), frame.trails.len());
        }
        assert_eq!(starts_missed(&frame, &playback), 0);
        // The same as a frame worked out afresh.
        super::line::memos().clear();
        let carried = frame.trails.clone();
        build_frame(&scene, &resolved, playback.world(), playback.log(), 60.06, &kit.draws, &mut frame);
        assert!(carried == frame.trails);
    }

    /// The level a crowd's lines are drawn at a minute in, with and without gravity (run by
    /// hand, in release).
    #[test]
    #[ignore]
    fn levels_of_a_crowd_a_minute_in() {
        for gravity in [true, false] {
            let scene = crowd_of_lines(|s, _| {
                if !gravity {
                    s.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 0.0]));
                }
            });
            let (frame, playback) = frame_at(&scene, 60.0);
            let layer = scene.card(BALL, SlotId::Look).unwrap().layers.iter().find(|l| l.kind == TRAIL).unwrap().id;
            let level = super::line::level_of((BALL, layer));
            eprintln!("gravity {gravity}: level {level}, {} vertices", frame.trails.len());
            assert_eq!(starts_missed(&frame, &playback), 0);
        }
    }

    /// Lines over the budget are drawn more coarsely until they fit, at the finest level that
    /// does wherever the search starts; other trails are cut to their newest share.
    #[test]
    fn a_crowd_over_the_budget_is_simplified_more_not_cut() {
        use super::{Drawn, MOST_LEVEL, cut_to, kinds, pick_level};
        // Points at each level: halving, more or less, as the lines get coarser.
        let totals = [900_000, 610_000, 400_000, 260_000, 190_000, 120_000, 90_000, 70_000, 60_000];
        for start in 0..=MOST_LEVEL {
            let mut asked = Vec::new();
            let (made, level) = pick_level(start, 200_000, |l| {
                asked.push(l);
                (l, totals[l as usize])
            });
            assert_eq!((made, level), (4, 4), "from {start}: asked {asked:?}");
        }
        // From where it was, it only looks at its own level and the one finer.
        let mut asked = Vec::new();
        pick_level(4, 200_000, |l| {
            asked.push(l);
            ((), totals[l as usize])
        });
        assert_eq!(asked, vec![4, 3]);
        // Past the coarsest, it stops there.
        assert_eq!(pick_level(0, 10, |l| ((), totals[l as usize])).1, MOST_LEVEL);
        // A level that drops far more than half isn't skipped past.
        let steep = [900_000, 150_000, 100_000, 90_000, 80_000, 70_000, 60_000, 50_000, 40_000];
        for start in 0..=MOST_LEVEL {
            assert_eq!(pick_level(start, 200_000, |l| ((), steep[l as usize])).1, 1, "from {start}");
        }
        // Cut to their newest share as a last resort.
        let long = |i: usize| Drawn {
            i,
            own: None,
            path: kinds::Path {
                points: vec![glam::Vec2::ZERO; 1000],
                ago: vec![0.0; 1000],
                arc: vec![0.0; 1000],
                corner: vec![false; 1000],
            },
        };
        let mut drawn: Vec<Drawn> = (0..100).map(long).collect();
        cut_to(&mut drawn, 200_000);
        assert!(drawn.iter().all(|d| d.path.len() == 1000), "under the budget: left alone");
        cut_to(&mut drawn, 20_000);
        assert!(drawn.iter().all(|d| d.path.len() == 200));
    }

    #[test]
    fn a_still_head_drops_to_the_last_moment_it_moved() {
        use super::kinds::Path;
        use glam::Vec2;
        let p = |x: f32| Vec2::new(x, 0.0);
        let mut path = Path {
            points: vec![p(5.0), p(5.0), p(5.0), p(3.0), p(1.0)],
            ago: vec![0.0, 0.1, 0.2, 0.3, 0.4],
            arc: vec![0.0, 0.0, 0.0, 2.0, 4.0],
            corner: vec![false; 5],
        };
        assert_eq!(path.drop_still_head(), 2);
        assert_eq!(path.points, vec![p(5.0), p(3.0), p(1.0)]);
        assert_eq!(path.ago, vec![0.2, 0.3, 0.4]);
        assert_eq!(path.arc, vec![0.0, 2.0, 4.0]);
        // A path standing still all the way is left alone.
        let mut still =
            Path { points: vec![p(1.0); 3], ago: vec![0.0, 0.1, 0.2], arc: vec![0.0; 3], corner: vec![false; 3] };
        assert_eq!(still.drop_still_head(), 0);
        assert_eq!(still.len(), 3);
    }
}
