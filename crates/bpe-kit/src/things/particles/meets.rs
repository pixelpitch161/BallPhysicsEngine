//! A Particles thing's Meets things card (`meets.particles`, in the Bounce slot): what its
//! particles do when they meet balls, shapes and the picture's edges. Balls push particles along
//! (never the other way in part 1); on touching one a particle passes, bounces off or vanishes.
//! Shapes are the world's walls, two-sided as for balls.

use bpe_core::{Choice, ModuleKind, Params, SettingDef, Settings, SlotId};
use bpe_runtime::{Outline, Touch, Wall, keep_clear_from};
use glam::Vec2;

use super::state::{DEAD, Speck};

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
    #[setting(label = "At shapes", simple, group = "Against shapes")]
    pub at_shapes: AtShapes,
    #[setting(label = "Bounciness", when = "at_shapes=bounce", span(0, 1), keyframe, group = "Against shapes")]
    pub bounciness: f32,
    /// How much sideways speed a bounce rubs off.
    #[setting(label = "Friction", when = "at_shapes=bounce", span(0, 1), keyframe, group = "Against shapes")]
    pub friction: f32,
    #[setting(label = "At the edges", group = "Edges of the picture")]
    pub at_edges: AtEdges,
    /// How far past the picture's edge the edge is.
    #[setting(label = "Edge margin", when = "at_edges!=pass", span(-200, 400), unit = "px", group = "Edges of the picture")]
    pub edge_margin: f32,
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
        self.push != 0.0 || self.at_balls != AtBalls::Pass
    }
}

/// One ball as particles meet it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BallAt {
    pub place: Vec2,
    pub velocity: Vec2,
    pub radius: f32,
}

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

/// Balls push `s` and, on touching, it does what `m` says.
pub fn meet_balls(s: &mut Speck, grid: &BallGrid, m: &MeetsSettings, dt: f32) {
    for &i in grid.near(s.place) {
        let b = grid.balls[i as usize];
        let off = s.place - b.place;
        let d = off.length();
        let reach = b.radius * grid.reach;
        if m.push != 0.0 && d < reach {
            let w = m.push * (1.0 - d / reach.max(1e-3)).powi(2);
            let take = 1.0 - (-12.0 * w * dt).exp();
            s.velocity += (b.velocity - s.velocity) * take;
            // Inside the ball it is shoved straight out, at the ball's speed.
            if d < b.radius {
                let out = off.try_normalize().unwrap_or(Vec2::NEG_Y);
                s.velocity += out * b.velocity.length() * m.push * take;
            }
        }
        let touch = b.radius + s.size.max(0.0);
        if d < touch {
            match m.at_balls {
                AtBalls::Pass => {}
                AtBalls::Vanish => s.flags |= DEAD,
                AtBalls::BounceOff => {
                    let n = off.try_normalize().unwrap_or(Vec2::NEG_Y);
                    s.place = b.place + n * touch;
                    let rel = s.velocity - b.velocity;
                    let into = rel.dot(n);
                    if into < 0.0 {
                        s.velocity = b.velocity + rel - n * into * (1.0 + m.ball_bounce.clamp(0.0, 1.0));
                    }
                }
            }
        }
    }
}

/// A wall with a box round everything near it, for skipping walls far from a particle.
#[derive(Clone, Debug)]
pub struct WallAt {
    pub wall: Wall,
    /// Its outline as the step began (which side a particle is on is judged against it).
    pub before: Outline,
    lo: Vec2,
    hi: Vec2,
}

impl WallAt {
    pub fn new(wall: Wall, before: Outline) -> WallAt {
        let grow = |o: &Outline| o.reach() + o.thickness() * 0.5 + 2.0;
        let (c0, c1) = (wall.outline.center(), before.center());
        let (r0, r1) = (grow(&wall.outline), grow(&before));
        let lo = (c0 - Vec2::splat(r0)).min(c1 - Vec2::splat(r1));
        let hi = (c0 + Vec2::splat(r0)).max(c1 + Vec2::splat(r1));
        WallAt { wall, before, lo, hi }
    }

    /// Whether a step from `a` to `b` may touch it.
    fn near(&self, a: Vec2, b: Vec2) -> bool {
        let (lo, hi) = (a.min(b), a.max(b));
        !(hi.x < self.lo.x || hi.y < self.lo.y || lo.x > self.hi.x || lo.y > self.hi.y)
    }
}

/// How big a particle is to the walls.
const SPECK_RADIUS: f32 = 0.5;

/// Keeps `s` on its own side of every wall, doing what `m` says where it meets one.
pub fn meet_walls(s: &mut Speck, walls: &[WallAt], m: &MeetsSettings) {
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
        match m.at_shapes {
            AtShapes::Pass => {}
            AtShapes::Vanish => {
                s.flags |= DEAD;
                return;
            }
            // It stops dead against the wall (moving with it); gravity can still slide it along.
            AtShapes::Stop => s.velocity = contact.wall_velocity,
            AtShapes::Bounce => {
                let rel = s.velocity - contact.wall_velocity;
                let n = contact.normal;
                let into = rel.dot(n);
                let normal = n * into;
                let side = (rel - normal) * (1.0 - m.friction.clamp(0.0, 1.0));
                let out = if into < 0.0 { -normal * m.bounciness.clamp(0.0, 1.0) } else { normal };
                s.velocity = contact.wall_velocity + side + out;
            }
        }
    }
}

/// The picture's edges as the particles meet them: its middle and half size, margin added.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edges {
    pub centre: Vec2,
    pub half: Vec2,
}

/// Does at the picture's edges what `m` says.
pub fn meet_edges(s: &mut Speck, e: &Edges, m: &MeetsSettings) {
    let lo = e.centre - e.half;
    let hi = e.centre + e.half;
    match m.at_edges {
        AtEdges::Pass => {}
        AtEdges::Vanish => {
            if s.place.x < lo.x || s.place.y < lo.y || s.place.x > hi.x || s.place.y > hi.y {
                s.flags |= DEAD;
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
            }
        }
        AtEdges::Bounce => {
            for k in 0..2 {
                if s.place[k] < lo[k] {
                    s.place[k] = lo[k] + (lo[k] - s.place[k]).min(hi[k] - lo[k]);
                    s.velocity[k] = s.velocity[k].abs();
                } else if s.place[k] > hi[k] {
                    s.place[k] = hi[k] - (s.place[k] - hi[k]).min(hi[k] - lo[k]);
                    s.velocity[k] = -s.velocity[k].abs();
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
