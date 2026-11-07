//! What part 3's forces read, gathered once a particle step before the particles move
//! ([`Ready::new`]): the thing's middle, its parent's balls, a grid of the world's balls, each
//! Ripple's rings, each Pulse's latest beats, the homes, and for Flock a snapshot of every
//! particle in a grid. The chunks only read it, so any number of threads gives the same.

use bpe_core::ThingId;
use bpe_runtime::{EventKind, Log, World};
use glam::Vec2;

use super::beats::{Beats, Moments};
use super::forces::{Force, Forces};
use super::meets::{BallAt, BallGrid};
use super::more_forces::Toward;
use super::state::{HomeFrame, ParticlesState, Speck};
use super::{BornFrom, GridCovers};
use crate::modules::look::layers::particles::emit::impact;

/// The newest rings a Ripple keeps.
pub const MOST_RINGS: usize = 64;
/// The most neighbours a flocking particle looks at.
pub const MOST_MATES: usize = 32;
/// The most cells a side of the flock grid has.
const MOST_CELLS: usize = 512;

/// A Ripple's ring: where the hit was, how long ago (s) and how hard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ring {
    pub centre: Vec2,
    pub age: f32,
    pub impact: f32,
}

/// What one force of the list gathered.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum ForceReady {
    #[default]
    Nothing,
    /// The place (and its velocity) a pull, push or spirograph acts from this step; `None` when
    /// it is gone (a ball number with no ball).
    Centre(Option<(Vec2, Vec2)>),
    Rings(Vec<Ring>),
    /// The beats so far (`None` before the first).
    Beats(Option<Beats>),
}

/// Every particle's place and velocity as the step began, in a uniform grid.
#[derive(Clone, Debug, Default)]
pub struct Flockmates {
    place: Vec<Vec2>,
    velocity: Vec<Vec2>,
    origin: Vec2,
    cell: f32,
    cols: usize,
    rows: usize,
    starts: Vec<u32>,
    items: Vec<u32>,
}

impl Flockmates {
    /// The particles of `specks`, in cells at least `reach` wide.
    pub fn new(specks: &[Speck], reach: f32) -> Flockmates {
        let place: Vec<Vec2> = specks.iter().map(|s| s.place).collect();
        let velocity: Vec<Vec2> = specks.iter().map(|s| s.velocity).collect();
        let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
        for p in place.iter().filter(|p| p.is_finite()) {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        if lo.x > hi.x {
            return Flockmates { place, velocity, ..Flockmates::default() };
        }
        let span = (hi - lo).max(Vec2::ONE);
        let cell = reach.max(1.0).max(span.x / MOST_CELLS as f32).max(span.y / MOST_CELLS as f32);
        let cols = ((span.x / cell).floor() as usize + 1).clamp(1, MOST_CELLS);
        let rows = ((span.y / cell).floor() as usize + 1).clamp(1, MOST_CELLS);
        let index = |p: Vec2| {
            let c = (((p.x - lo.x) / cell) as usize).min(cols - 1);
            let r = (((p.y - lo.y) / cell) as usize).min(rows - 1);
            r * cols + c
        };
        let mut counts = vec![0u32; cols * rows + 1];
        for p in place.iter().filter(|p| p.is_finite()) {
            counts[index(*p)] += 1;
        }
        let mut starts = vec![0u32; cols * rows + 1];
        for k in 0..cols * rows {
            starts[k + 1] = starts[k] + counts[k];
        }
        let mut fill = starts.clone();
        let mut items = vec![0u32; starts[cols * rows] as usize];
        for (i, p) in place.iter().enumerate().filter(|(_, p)| p.is_finite()) {
            let k = index(*p);
            items[fill[k] as usize] = i as u32;
            fill[k] += 1;
        }
        Flockmates { place, velocity, origin: lo, cell, cols, rows, starts, items }
    }

    /// Calls `f` with the place and velocity of each particle within `reach` of `p` but particle
    /// `me`, at most [`MOST_MATES`], in a fixed order (cells row by row, then birth order).
    pub fn near(&self, me: usize, p: Vec2, reach: f32, mut f: impl FnMut(Vec2, Vec2)) {
        if self.cols == 0 || !p.is_finite() {
            return;
        }
        let rel = (p - self.origin) / self.cell;
        let (c, r) = (rel.x.floor() as i64, rel.y.floor() as i64);
        let mut seen = 0;
        for row in r - 1..=r + 1 {
            if row < 0 || row >= self.rows as i64 {
                continue;
            }
            for col in c - 1..=c + 1 {
                if col < 0 || col >= self.cols as i64 {
                    continue;
                }
                let k = row as usize * self.cols + col as usize;
                for &j in &self.items[self.starts[k] as usize..self.starts[k + 1] as usize] {
                    let j = j as usize;
                    if j == me || self.place[j].distance_squared(p) >= reach * reach {
                        continue;
                    }
                    f(self.place[j], self.velocity[j]);
                    seen += 1;
                    if seen >= MOST_MATES {
                        return;
                    }
                }
            }
        }
    }
}

/// What the step's forces read, besides the particle.
#[derive(Clone, Debug, Default)]
pub struct Ready {
    /// The particle clock's time at the end of the step, in seconds.
    pub time: f64,
    /// The thing's middle: its place, or the camera's middle for a source over the whole screen.
    pub middle: Vec2,
    /// Where a word's, picture's or grid's places are.
    pub homes: HomeFrame,
    /// Its parent's balls by body row, `None` for rows that aren't alive.
    pub parent_balls: Vec<Option<BallAt>>,
    /// The middle of its parent (its balls', or its shape's) and how fast it moves.
    pub parent_middle: Option<(Vec2, Vec2)>,
    /// The world's balls, when a force looks at the balls near each particle.
    pub balls: Option<BallGrid>,
    /// What each force of the list gathered, in list order.
    pub per_force: Vec<ForceReady>,
    /// The particles as the step began, when a force flocks.
    pub mates: Option<Flockmates>,
}

impl Ready {
    /// Gathers what `forces` read for `st`'s step in `world` (whose log is `log`), the homes
    /// being `homes`.
    pub fn new(st: &ParticlesState, forces: &Forces, world: &World, log: &Log, homes: HomeFrame) -> Ready {
        let time = st.step as f64 * super::step::DT as f64;
        let s = &st.settings;
        let screen =
            s.from == BornFrom::WholeScreen || (s.from == BornFrom::AGrid && s.grid_covers == GridCovers::TheScreen);
        let middle = if screen { world.camera.center } else { world.anchor(st.id).position };
        let mut ready = Ready { time, middle, homes, ..Ready::default() };
        if forces.list.iter().all(|f| !f.is_more()) {
            return ready;
        }
        // The parent's balls and middle.
        let parent = st.parent.filter(|p| *p != ThingId::ROOT);
        if let Some(b) = parent.and_then(|p| world.bodies.get(&p)) {
            ready.parent_balls = (0..b.len())
                .map(|r| {
                    (b.alive[r] && b.position[r].is_finite()).then(|| BallAt {
                        place: b.position[r],
                        velocity: b.velocity[r],
                        radius: b.radius[r].max(0.0),
                    })
                })
                .collect();
            let alive: Vec<BallAt> = ready.parent_balls.iter().flatten().copied().collect();
            if !alive.is_empty() {
                let n = alive.len() as f32;
                let at = alive.iter().map(|b| b.place).sum::<Vec2>() / n;
                let v = alive.iter().map(|b| b.velocity).sum::<Vec2>() / n;
                ready.parent_middle = Some((at, v));
            }
        } else if let Some(w) = parent.and_then(|p| world.walls.get(&p)) {
            ready.parent_middle = Some((w.outline.center(), w.velocity));
        }
        // The world's balls, as far as the furthest force looks.
        let mut reach: f32 = 0.0;
        for f in &forces.list {
            reach = reach.max(match f {
                Force::Pull(p) if p.toward == Toward::BallsNearby => p.ball_reach,
                Force::Push(p) if p.toward == Toward::BallsNearby => p.ball_reach,
                Force::Magnetic(m) if m.from == super::more_forces::FieldFrom::EachBall => 4.0 * m.size,
                _ => 0.0,
            });
        }
        if reach > 0.0 && reach.is_finite() {
            ready.balls = Some(BallGrid::new(super::step::balls_in(world), reach.max(1.0)));
        }
        // What each force reads.
        ready.per_force = forces.list.iter().map(|f| ready.gather(f, st, world, log)).collect();
        // The flock's neighbours.
        let flock = forces.list.iter().filter_map(|f| if let Force::Flock(k) = f { Some(k.reach) } else { None });
        if let Some(reach) = flock.filter(|r| r.is_finite()).reduce(f32::max) {
            ready.mates = Some(Flockmates::new(&st.specks, reach.max(1.0)));
        }
        ready
    }

    /// The centre `toward` names this step (not Its ball or Balls nearby, which are each
    /// particle's own).
    fn centre(&self, toward: Toward, point: Vec2, number: u32, st: &ParticlesState, world: &World) -> ForceReady {
        ForceReady::Centre(match toward {
            Toward::ItsPlace => Some((self.middle, Vec2::ZERO)),
            Toward::APoint => Some((point, Vec2::ZERO)),
            Toward::TheMiddleOfItsParent => Some(self.parent_middle.unwrap_or((self.middle, Vec2::ZERO))),
            Toward::BallNumber => {
                let bodies = st.parent.and_then(|p| world.bodies.get(&p));
                let row = bodies.and_then(|b| b.row_of(number.saturating_sub(1)));
                row.and_then(|r| self.parent_balls.get(r).copied().flatten()).map(|b| (b.place, b.velocity))
            }
            Toward::ItsBall | Toward::BallsNearby => return ForceReady::Nothing,
        })
    }

    fn gather(&self, f: &Force, st: &ParticlesState, world: &World, log: &Log) -> ForceReady {
        match f {
            Force::Pull(p) => self.centre(p.toward, p.point, p.ball_number, st, world),
            Force::Push(p) => self.centre(p.toward, p.point, p.ball_number, st, world),
            Force::Spirograph(p) => self.centre(p.round.toward(), p.point, p.ball_number, st, world),
            Force::Pulse(p) => ForceReady::Beats(Moments::of(world, p.every, p.bars, p.offset).beats_at(self.time)),
            Force::Ripple(r) => {
                let rate = world.steps_per_second.max(1);
                let lasts = if r.lasts.is_finite() { r.lasts.max(0.01) as f64 } else { 1.0 };
                let from = ((self.time - lasts) * rate as f64).floor().max(0.0) as u64;
                let events = log.events();
                let first = events.partition_point(|e| e.step < from);
                let named = |t: ThingId| world.names(&r.who_hits, st.id, t);
                let mut rings: Vec<Ring> = events[first..]
                    .iter()
                    .filter(|e| e.kind == EventKind::Hit && e.speed >= r.least_speed)
                    .filter(|e| named(e.a.thing) || e.b.is_some_and(|b| named(b.thing)))
                    .filter_map(|e| {
                        let age = self.time - e.time(rate);
                        (age >= 0.0 && age <= lasts).then(|| Ring {
                            centre: e.point,
                            age: age as f32,
                            impact: impact(r.follow_impact, e.speed),
                        })
                    })
                    .collect();
                if rings.len() > MOST_RINGS {
                    rings.drain(..rings.len() - MOST_RINGS);
                }
                ForceReady::Rings(rings)
            }
            _ => ForceReady::Nothing,
        }
    }

    /// What force `k` of the list gathered.
    pub fn of(&self, k: usize) -> &ForceReady {
        static NOTHING: ForceReady = ForceReady::Nothing;
        self.per_force.get(k).unwrap_or(&NOTHING)
    }
}
