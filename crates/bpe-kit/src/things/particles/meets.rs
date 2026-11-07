//! A Particles thing's Meets things card (`meets.particles`, in the Bounce slot): what its
//! particles do when they meet balls, shapes and the picture's edges. Balls push particles along;
//! on touching one a particle passes, bounces off, vanishes, pops or sticks to it. Shapes are the
//! world's walls, two-sided as for balls: particles bounce, stop, vanish, settle or pile up on
//! them. With **Two-way** on, the particles push back on the balls (`two_way.rs`).

use bpe_core::{Choice, ModuleKind, Params, SettingDef, Settings, SlotId, ThingId};
use bpe_runtime::{Outline, Touch, Wall, keep_clear_from};
use glam::Vec2;

use super::feeds::{FeedKind, Feeds, LAND_SPEED};
use super::moments::{MET_AT_BALL, MET_AT_SHAPE, MET_BALL, MET_LAND, MET_SHAPE};
use super::state::{DEAD, LANDED, LOOSE, RESTING, STUCK, Speck};

/// The card kind.
pub const CARD: &str = "meets.particles";

/// What a particle does on touching a ball.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum AtBalls {
    /// It goes on (still pushed by the ball's Push).
    #[default]
    Pass,
    BounceOff,
    Vanish,
    /// It vanishes with a pop (what a Particles thing fed on Pop is born from).
    Pop,
    /// It sticks to the ball where it touched, until shaken off.
    Stick,
}

/// What a particle does on meeting a shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum AtShapes {
    #[default]
    Pass,
    Bounce,
    /// It stops dead against the shape.
    Stop,
    Vanish,
    /// It stays where it lands, moving with the shape, until a ball kicks it up again.
    Settle,
    /// It joins a heap of sand on a still shape's top (or the floor), which slides down to its
    /// angle of repose.
    PileUp,
}

/// What a particle does at the edge of the picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum AtEdges {
    /// It goes on out of sight.
    #[default]
    Pass,
    /// It comes back in at the other side.
    WrapRound,
    Bounce,
    Vanish,
}

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct MeetsSettings {
    /// How much a passing ball drags particles along with it: 0 not at all, 1 fully.
    #[setting(label = "Balls push", span(0, 1), simple, keyframe, pulse, group = "Other balls")]
    pub push: f32,
    /// How far from a ball its push reaches, in ball radii.
    #[setting(label = "Reach", when = "push", range(0, 1e9), span(1, 6), unit = "×", keyframe, group = "Other balls")]
    pub reach: f32,
    #[setting(label = "On touching a ball", simple, group = "Other balls")]
    pub at_balls: AtBalls,
    #[setting(label = "Bounciness", when = "at_balls=bounce_off", span(0, 1), keyframe, group = "Other balls")]
    pub ball_bounce: f32,
    /// The share of a ball's stuck particles each landing shakes off.
    #[setting(label = "Fall off when it lands", when = "at_balls=stick", range(0, 1), keyframe, group = "Other balls")]
    pub fall_off: f32,
    #[setting(label = "At shapes", simple, group = "Against shapes")]
    pub at_shapes: AtShapes,
    #[setting(label = "Bounciness", when = "at_shapes=bounce", span(0, 1), keyframe, group = "Against shapes")]
    pub bounciness: f32,
    /// How much sideways speed a bounce rubs off.
    #[setting(label = "Friction", when = "at_shapes=bounce", span(0, 1), keyframe, group = "Against shapes")]
    pub friction: f32,
    /// How long a settled particle stays before it fades away: 0 stays.
    #[setting(
        label = "Fade after",
        when = "at_shapes=settle|pile_up",
        range(0, 1e9),
        span(0, 30),
        unit = "s",
        keyframe,
        group = "Against shapes"
    )]
    pub fade_after: f32,
    /// The most settled particles kept: past it the oldest fade away.
    #[setting(
        label = "Most kept",
        when = "at_shapes=settle|pile_up",
        range(1, 1000000),
        span(1, 20000),
        group = "Against shapes"
    )]
    pub most_kept: u32,
    /// The steepest a pile's slope stands, in degrees.
    #[setting(
        label = "Angle of repose",
        when = "at_shapes=pile_up",
        range(0, 89),
        unit = "°",
        keyframe,
        group = "Against shapes"
    )]
    pub repose: f32,
    /// How much a pile grows for each particle that joins it, in pixels of height.
    #[setting(
        label = "Each adds",
        when = "at_shapes=pile_up",
        range(0, 1e9),
        span(0, 5),
        unit = "px",
        keyframe,
        group = "Against shapes"
    )]
    pub grain: f32,
    /// How much of the sand a ball cuts through flies back up as particles.
    #[setting(label = "Thrown up", when = "at_shapes=pile_up", span(0, 1), keyframe, group = "Against shapes")]
    pub thrown_up: f32,
    /// How much a ball bounces back off a pile (two-way).
    #[setting(
        label = "Bounciness on piles",
        when = "at_shapes=pile_up and two_way and holds_up",
        span(0, 1),
        keyframe,
        group = "Against shapes"
    )]
    pub pile_bounce: f32,
    #[setting(label = "At the edges", group = "Edges of the picture")]
    pub at_edges: AtEdges,
    /// How far past the picture's edge the edge is.
    #[setting(label = "Edge margin", when = "at_edges!=pass", span(-200, 400), unit = "px", group = "Edges of the picture")]
    pub edge_margin: f32,
    /// The particles push back on the balls: they change the physics. Not for particles fed by
    /// another Particles thing.
    #[setting(label = "Two-way", group = "Two-way")]
    pub two_way: bool,
    /// How heavy each particle is, as a share of a ball of its size.
    #[setting(label = "Weight", when = "two_way", range(0, 1e9), span(0, 10), keyframe, group = "Two-way")]
    pub weight: f32,
    /// Particles that hit a ball (and bounce off, vanish, pop or stick) push it.
    #[setting(label = "Pushes balls", when = "two_way", group = "Two-way")]
    pub pushes: bool,
    /// A ball pushing particles along is slowed by them, more the more it wades through.
    #[setting(label = "Slows balls", when = "two_way", group = "Two-way")]
    pub slows: bool,
    /// Balls rest on piles and bounce off them (hard landings dig in).
    #[setting(label = "Holds balls up", when = "two_way", group = "Two-way")]
    pub holds_up: bool,
    /// Particles stuck to a ball weigh it down.
    #[setting(label = "Weighs balls down", when = "two_way", group = "Two-way")]
    pub weighs: bool,
}

impl Default for MeetsSettings {
    fn default() -> Self {
        Self {
            push: 0.6,
            reach: 2.0,
            at_balls: AtBalls::Pass,
            ball_bounce: 0.5,
            at_shapes: AtShapes::Pass,
            bounciness: 0.4,
            friction: 0.1,
            at_edges: AtEdges::Pass,
            edge_margin: 40.0,
            fall_off: 0.5,
            fade_after: 0.0,
            most_kept: 3000,
            repose: 34.0,
            grain: 0.4,
            thrown_up: 0.5,
            pile_bounce: 0.1,
            two_way: false,
            weight: 1.0,
            pushes: true,
            slows: true,
            holds_up: true,
            weighs: true,
        }
    }
}

impl MeetsSettings {
    /// The card's settings; the defaults without a card.
    pub fn read(params: Option<&Params>) -> MeetsSettings {
        params.map(MeetsSettings::from_params).unwrap_or_default()
    }

    /// Whether the particles need to know where the balls are.
    pub fn reads_balls(&self) -> bool {
        self.push != 0.0 || self.at_balls != AtBalls::Pass || self.at_shapes == AtShapes::PileUp
    }

    /// Whether the picture's bottom edge is a floor that particles stop, settle or pile up on.
    pub fn floor(&self) -> bool {
        self.at_edges == AtEdges::Bounce
            && matches!(self.at_shapes, AtShapes::Stop | AtShapes::Settle | AtShapes::PileUp)
    }
}

/// One ball as particles meet it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BallAt {
    pub place: Vec2,
    pub velocity: Vec2,
    pub radius: f32,
    /// Its thing (for the moments and sticking) and row.
    pub thing: ThingId,
    pub row: u32,
}

/// What the move of one chunk of particles gives back besides the particles: moments, grains
/// landing on piles and, two-way, what they do to the balls.
#[derive(Clone, Debug, Default)]
pub struct ChunkOut {
    pub feeds: Feeds,
    /// Grains that joined a pile (or start one): the surface's key, where they sit, and the
    /// pile and column when they landed on a pile's top.
    pub landings: Vec<Landing>,
    /// Two-way: the momentum particles gave each ball of the grid by touching it (mass × px/s).
    pub pushes: Vec<Vec2>,
    /// Two-way: the momentum balls lost pushing particles along.
    pub slows: Vec<Vec2>,
    /// Two-way: the mass stuck to each ball, by (thing, row).
    pub stuck: std::collections::BTreeMap<(u32, u32), f32>,
}

/// A grain joining a pile.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Landing {
    /// The shape's id, or [`super::piles::FLOOR`].
    pub key: u32,
    pub at: Vec2,
    /// The pile (its place in the list) and column it landed on the top of.
    pub pile: Option<(usize, i32)>,
}

/// What touching balls reads besides the particle.
#[derive(Clone, Copy, Debug, Default)]
pub struct TouchCx {
    /// Two-way pushes and slowing are worked out ([`MeetsSettings`]'s, when two-way acts).
    pub pushes: bool,
    pub slows: bool,
    /// Each particle's mass for its size² (Weight).
    pub weight: f32,
}

impl TouchCx {
    /// A particle's mass.
    pub fn mass(&self, s: &Speck) -> f32 {
        self.weight.max(0.0) * s.size.max(0.5).powi(2)
    }
}

/// How fast a ball must go past a settled particle to kick it up again, in pixels a second.
pub const KICK_SPEED: f32 = 40.0;

/// The most balls a particle looks at.
pub const MOST_NEAR: usize = 32;
/// The most cells a side of the ball grid has.
const MOST_CELLS: usize = 256;

/// Every alive ball, in a uniform grid, for the particles to look up the ones near them.
#[derive(Clone, Debug, Default)]
pub struct BallGrid {
    pub balls: Vec<BallAt>,
    origin: Vec2,
    cell: f32,
    cols: usize,
    rows: usize,
    /// Each cell's balls, as `starts[cell]..starts[cell + 1]` into `items`.
    starts: Vec<u32>,
    items: Vec<u32>,
    /// How far from a ball's middle anything is looked at, in its radii.
    reach: f32,
}

impl BallGrid {
    /// A grid of `balls` for looking `reach` radii round each (at least its own radius).
    pub fn new(balls: Vec<BallAt>, reach: f32) -> BallGrid {
        let reach = reach.max(1.0);
        if balls.is_empty() {
            return BallGrid { reach, ..BallGrid::default() };
        }
        let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
        let mut biggest: f32 = 0.0;
        for b in &balls {
            let r = b.radius * reach;
            lo = lo.min(b.place - r);
            hi = hi.max(b.place + r);
            biggest = biggest.max(r);
        }
        let span = (hi - lo).max(Vec2::ONE);
        let cell = biggest.max(8.0).max(span.x / MOST_CELLS as f32).max(span.y / MOST_CELLS as f32);
        let cols = ((span.x / cell).ceil() as usize).clamp(1, MOST_CELLS);
        let rows = ((span.y / cell).ceil() as usize).clamp(1, MOST_CELLS);
        let index = |p: Vec2| {
            let c = (((p.x - lo.x) / cell) as isize).clamp(0, cols as isize - 1) as usize;
            let r = (((p.y - lo.y) / cell) as isize).clamp(0, rows as isize - 1) as usize;
            (c, r)
        };
        // Each ball in every cell its reach touches.
        let mut counts = vec![0u32; cols * rows + 1];
        let each = |f: &mut dyn FnMut(usize, usize)| {
            for (i, b) in balls.iter().enumerate() {
                let r = b.radius * reach;
                let (c0, r0) = index(b.place - r);
                let (c1, r1) = index(b.place + r);
                for row in r0..=r1 {
                    for col in c0..=c1 {
                        f(row * cols + col, i);
                    }
                }
            }
        };
        each(&mut |cell, _| counts[cell] += 1);
        let mut starts = vec![0u32; cols * rows + 1];
        for k in 0..cols * rows {
            starts[k + 1] = starts[k] + counts[k];
        }
        let mut fill = starts.clone();
        let mut items = vec![0u32; starts[cols * rows] as usize];
        each(&mut |cell, i| {
            items[fill[cell] as usize] = i as u32;
            fill[cell] += 1;
        });
        BallGrid { balls, origin: lo, cell, cols, rows, starts, items, reach }
    }

    /// Its parts, for a Huge swarm's step on the graphics card.
    pub fn parts(&self) -> GridParts<'_> {
        GridParts {
            balls: &self.balls,
            origin: self.origin,
            cell: self.cell,
            cols: self.cols,
            rows: self.rows,
            starts: &self.starts,
            items: &self.items,
            reach: self.reach,
        }
    }

    /// The balls whose reach may cover `p`, at most [`MOST_NEAR`], in ball order.
    pub fn near(&self, p: Vec2) -> &[u32] {
        if self.balls.is_empty() || !p.is_finite() {
            return &[];
        }
        let rel = (p - self.origin) / self.cell;
        if rel.x < 0.0 || rel.y < 0.0 || rel.x >= self.cols as f32 || rel.y >= self.rows as f32 {
            return &[];
        }
        let k = rel.y as usize * self.cols + rel.x as usize;
        let (a, b) = (self.starts[k] as usize, self.starts[k + 1] as usize);
        &self.items[a..b.min(a + MOST_NEAR)]
    }
}

/// A [`BallGrid`]'s parts: its balls, the grid's corner, cell size, columns and rows, each cell's
/// balls as `starts[cell]..starts[cell + 1]` into `items`, and the reach in radii.
pub struct GridParts<'a> {
    pub balls: &'a [BallAt],
    pub origin: Vec2,
    pub cell: f32,
    pub cols: usize,
    pub rows: usize,
    pub starts: &'a [u32],
    pub items: &'a [u32],
    pub reach: f32,
}

/// Balls push `s` and, on touching, it does what `m` says, keeping moments and two-way pushes in
/// `out`.
pub fn meet_balls(s: &mut Speck, grid: &BallGrid, m: &MeetsSettings, dt: f32, t: &TouchCx, out: &mut ChunkOut) {
    let mut touching = false;
    for &i in grid.near(s.place) {
        let b = grid.balls[i as usize];
        let off = s.place - b.place;
        let d = off.length();
        let reach = b.radius * grid.reach;
        if m.push != 0.0 && d < reach {
            let w = m.push * (1.0 - d / reach.max(1e-3)).powi(2);
            let take = 1.0 - (-12.0 * w * dt).exp();
            let before = s.velocity;
            s.velocity += (b.velocity - s.velocity) * take;
            // Inside the ball it is shoved straight out, at the ball's speed.
            if d < b.radius {
                let out_way = off.try_normalize().unwrap_or(Vec2::NEG_Y);
                s.velocity += out_way * b.velocity.length() * m.push * take;
            }
            if t.slows && s.velocity.is_finite() {
                out.slows[i as usize] -= (s.velocity - before) * t.mass(s);
            }
        }
        let touch = b.radius + s.size.max(0.0);
        if d >= touch {
            continue;
        }
        touching = true;
        if m.at_balls == AtBalls::Pass || s.flags & LOOSE != 0 {
            continue;
        }
        let n = off.try_normalize().unwrap_or(Vec2::NEG_Y);
        let before = s.velocity;
        let into = (s.velocity - b.velocity).dot(n);
        if -into > LAND_SPEED {
            out.feeds.note(FeedKind::HitBall, s, before, n);
        }
        match m.at_balls {
            AtBalls::Pass => {}
            AtBalls::Vanish | AtBalls::Pop => {
                s.flags |= DEAD;
                mark(s, MET_BALL, b.thing.0, -into);
                if m.at_balls == AtBalls::Pop {
                    out.feeds.note(FeedKind::Pop, s, before, n);
                }
            }
            AtBalls::BounceOff => {
                s.place = b.place + n * touch;
                let rel = s.velocity - b.velocity;
                let into = rel.dot(n);
                if into < 0.0 {
                    s.velocity = b.velocity + rel - n * into * (1.0 + m.ball_bounce.clamp(0.0, 1.0));
                    mark(s, MET_BALL, b.thing.0, -into);
                }
            }
            AtBalls::Stick => {
                mark(s, MET_BALL, b.thing.0, -into);
                s.place = b.place + n * b.radius;
                s.velocity = b.velocity;
                s.flags |= RESTING | STUCK;
                s.rest = out.feeds.now;
                s.on = b.thing.0;
                s.on_row = b.row;
                s.offset = n;
            }
        }
        // What it gave the ball (it ends at the ball's speed when it goes or sticks).
        if t.pushes {
            let after = if s.flags & DEAD != 0 { b.velocity } else { s.velocity };
            out.pushes[i as usize] += (before - after) * t.mass(s);
        }
        if s.flags & (DEAD | STUCK) != 0 {
            return;
        }
    }
    if !touching {
        s.flags &= !LOOSE;
    }
}

/// Whether a ball near the settled particle `s` kicks it up: within its push's reach, going at
/// least [`KICK_SPEED`] past it.
pub fn kicked(s: &Speck, grid: &BallGrid, m: &MeetsSettings) -> bool {
    if m.push == 0.0 {
        return false;
    }
    grid.near(s.place).iter().any(|&i| {
        let b = grid.balls[i as usize];
        s.place.distance(b.place) < b.radius * grid.reach && (b.velocity - s.velocity).length() >= KICK_SPEED
    })
}

/// Notes that `s` met `what` (a [`MET_BALL`], [`MET_SHAPE`] or [`MET_LAND`] mark) of thing
/// `thing`, going into it at `speed`: the first ball and the first shape it meets in a step are
/// the ones its moments tell.
fn mark(s: &mut Speck, what: u8, thing: u32, speed: f32) {
    let (k, kin) = if what == MET_BALL { (MET_AT_BALL, MET_BALL) } else { (MET_AT_SHAPE, MET_SHAPE | MET_LAND) };
    if s.met & kin == 0 {
        s.with[k] = thing;
        s.impact[k] = if speed.is_finite() { speed.max(0.0) } else { 0.0 };
    }
    s.met |= what;
}

/// A wall with a box round everything near it, for skipping walls far from a particle.
#[derive(Clone, Debug)]
pub struct WallAt {
    /// The shape's id.
    pub id: u32,
    pub wall: Wall,
    /// Its outline as the step began (which side a particle is on is judged against it).
    pub before: Outline,
    lo: Vec2,
    hi: Vec2,
}

impl WallAt {
    pub fn new(id: ThingId, wall: Wall, before: Outline) -> WallAt {
        let grow = |o: &Outline| o.reach() + o.thickness() * 0.5 + 2.0;
        let (c0, c1) = (wall.outline.center(), before.center());
        let (r0, r1) = (grow(&wall.outline), grow(&before));
        let lo = (c0 - Vec2::splat(r0)).min(c1 - Vec2::splat(r1));
        let hi = (c0 + Vec2::splat(r0)).max(c1 + Vec2::splat(r1));
        WallAt { id: id.0, wall, before, lo, hi }
    }

    /// Whether a step from `a` to `b` may touch it.
    fn near(&self, a: Vec2, b: Vec2) -> bool {
        let (lo, hi) = (a.min(b), a.max(b));
        !(hi.x < self.lo.x || hi.y < self.lo.y || lo.x > self.hi.x || lo.y > self.hi.y)
    }
}

/// How big a particle is to the walls.
const SPECK_RADIUS: f32 = 0.5;

/// Keeps `s` on its own side of every wall, doing what `m` says where it meets one; moments and
/// grains landing on piles go into `out`.
pub fn meet_walls(s: &mut Speck, walls: &[WallAt], m: &MeetsSettings, out: &mut ChunkOut) {
    if m.at_shapes == AtShapes::Pass {
        return;
    }
    for w in walls {
        if !w.near(s.previous, s.place) {
            continue;
        }
        let mut at = s.place;
        let Some(contact) =
            keep_clear_from(&mut at, s.previous, &w.before, s.velocity, SPECK_RADIUS, Touch::Edge, &w.wall)
        else {
            continue;
        };
        if !at.is_finite() {
            s.flags |= DEAD;
            return;
        }
        s.place = at;
        s.flags |= super::marks::TOUCHED_SHAPE;
        let rel = s.velocity - contact.wall_velocity;
        let into = rel.dot(contact.normal);
        let before = s.velocity;
        let hard = contact.speed > LAND_SPEED;
        if hard {
            out.feeds.note(FeedKind::HitShape, s, before, contact.normal);
        }
        // Piles form on a still shape's top; on a moving one sand settles, on a side it stops.
        let up = contact.normal.y < -0.5;
        let outcome = match m.at_shapes {
            AtShapes::PileUp if up && super::piles::still(&w.wall) => AtShapes::PileUp,
            AtShapes::PileUp if up => AtShapes::Settle,
            AtShapes::PileUp => AtShapes::Stop,
            other => other,
        };
        match outcome {
            AtShapes::Pass => {}
            AtShapes::Vanish => {
                s.flags |= DEAD;
                mark(s, MET_SHAPE, w.id, -into);
                return;
            }
            // It stops dead against the wall (moving with it); gravity can still slide it along.
            // It lands the first time only.
            AtShapes::Stop => {
                if s.flags & LANDED == 0 {
                    mark(s, MET_LAND, w.id, -into);
                }
                s.velocity = contact.wall_velocity;
                if hard {
                    out.feeds.note(FeedKind::Land, s, before, contact.normal);
                }
            }
            AtShapes::Bounce => {
                let n = contact.normal;
                if into < 0.0 {
                    mark(s, MET_SHAPE, w.id, -into);
                }
                let normal = n * into;
                let side = (rel - normal) * (1.0 - m.friction.clamp(0.0, 1.0));
                let out_way = if into < 0.0 { -normal * m.bounciness.clamp(0.0, 1.0) } else { normal };
                s.velocity = contact.wall_velocity + side + out_way;
            }
            AtShapes::Settle => {
                if s.flags & LANDED == 0 {
                    mark(s, MET_LAND, w.id, -into);
                }
                settle(s, w.id, contact.wall_velocity, out.feeds.now);
                out.feeds.note(FeedKind::Land, s, before, contact.normal);
                return;
            }
            AtShapes::PileUp => {
                s.flags |= DEAD;
                mark(s, MET_LAND, w.id, -into);
                out.feeds.note(FeedKind::Land, s, before, contact.normal);
                out.landings.push(Landing { key: w.id, at: s.place, pile: None });
                return;
            }
        }
    }
}

/// Brings `s` to rest on shape `on` (0 the floor), moving at `velocity`, at particle step `now`.
pub fn settle(s: &mut Speck, on: u32, velocity: Vec2, now: u32) {
    s.flags |= RESTING;
    s.flags &= !STUCK;
    s.rest = now.max(1);
    s.on = on;
    s.velocity = velocity;
}

/// The picture's edges as the particles meet them: its middle and half size, margin added.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edges {
    pub centre: Vec2,
    pub half: Vec2,
}

/// Does at the picture's edges what `m` says (the bottom edge a floor when [`MeetsSettings::floor`]).
pub fn meet_edges(s: &mut Speck, e: &Edges, m: &MeetsSettings, out: &mut ChunkOut) {
    let lo = e.centre - e.half;
    let hi = e.centre + e.half;
    if m.floor() && s.place.y > hi.y {
        let before = s.velocity;
        s.place.y = hi.y;
        let normal = Vec2::NEG_Y;
        if before.y > LAND_SPEED {
            out.feeds.note(FeedKind::HitShape, s, before, normal);
            out.feeds.note(FeedKind::Land, s, before, normal);
        }
        match m.at_shapes {
            AtShapes::PileUp => {
                s.flags |= DEAD;
                out.landings.push(Landing { key: super::piles::FLOOR, at: s.place, pile: None });
                return;
            }
            AtShapes::Settle => {
                settle(s, 0, Vec2::ZERO, out.feeds.now);
                return;
            }
            _ => s.velocity.y = s.velocity.y.min(0.0),
        }
    }
    match m.at_edges {
        AtEdges::Pass => {}
        AtEdges::Vanish => {
            if s.place.x < lo.x || s.place.y < lo.y || s.place.x > hi.x || s.place.y > hi.y {
                s.flags |= DEAD | super::marks::TOUCHED_EDGE;
            }
        }
        AtEdges::WrapRound => {
            let size = (hi - lo).max(Vec2::ONE);
            let wrap = |x: f32, lo: f32, size: f32| lo + (x - lo).rem_euclid(size);
            let before = s.place;
            s.place = Vec2::new(wrap(s.place.x, lo.x, size.x), wrap(s.place.y, lo.y, size.y));
            // No streak across the picture: it was where it is now.
            if s.place != before {
                s.previous = s.place - (before - s.previous);
                // Its mark goes where it comes back in: the edge it is beside now.
                s.flags |= super::marks::TOUCHED_EDGE;
            }
        }
        AtEdges::Bounce => {
            for k in 0..2 {
                if s.place[k] < lo[k] {
                    s.place[k] = lo[k] + (lo[k] - s.place[k]).min(hi[k] - lo[k]);
                    s.velocity[k] = s.velocity[k].abs();
                    s.flags |= super::marks::TOUCHED_EDGE;
                } else if s.place[k] > hi[k] {
                    s.place[k] = hi[k] - (s.place[k] - hi[k]).min(hi[k] - lo[k]);
                    s.velocity[k] = -s.velocity[k].abs();
                    s.flags |= super::marks::TOUCHED_EDGE;
                }
            }
        }
    }
}

/// The Meets things card of a Particles thing.
pub struct MeetsCard;

impl ModuleKind for MeetsCard {
    fn id(&self) -> &'static str {
        CARD
    }
    fn name(&self) -> &'static str {
        "Meets things"
    }
    fn help(&self) -> &'static str {
        "What the particles do when they meet balls, shapes and the edges of the picture."
    }
    fn slot(&self) -> SlotId {
        SlotId::Bounce
    }
    fn settings(&self) -> &'static [SettingDef] {
        MeetsSettings::defs()
    }
    fn fits(&self, thing_kind: &str) -> bool {
        thing_kind == super::KIND
    }
}
