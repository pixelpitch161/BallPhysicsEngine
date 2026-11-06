//! One particle step (1/120 s): births, then every particle moved by the forces and the things it
//! meets, in chunks of [`CHUNK`] on threads, then the dead removed in order. Each chunk reads only
//! what is shared and read-only (the balls, the walls, the edges) and writes only its own
//! particles, so any number of threads gives the same particles.

use bpe_runtime::{Log, World};
use glam::Vec2;

use super::births::{self, BirthCx};
use super::forces::{ForceCx, Forces};
use super::meets::{AtEdges, BallAt, BallGrid, Edges, MeetsSettings, WallAt, meet_balls, meet_edges, meet_walls};
use super::state::{DEAD, ParticlesState, Speck};

/// Seconds a particle step lasts.
pub const DT: f32 = 1.0 / 120.0;
/// World steps a particle step lasts.
pub const WORLD_STEPS: u64 = 2;
/// Particles a thread works on at a time.
pub const CHUNK: usize = 4096;
/// The most threads a step uses.
const MOST_THREADS: usize = 16;

/// What the move reads, shared by every chunk.
pub struct MoveCx<'a> {
    pub forces: &'a Forces,
    pub meets: &'a MeetsSettings,
    pub grid: Option<&'a BallGrid>,
    pub walls: &'a [WallAt],
    pub edges: Option<Edges>,
    pub force: ForceCx,
    /// The particle step being taken.
    pub now: u32,
    /// The fastest a particle may go.
    pub ceiling: f32,
}

/// Moves one particle one step, marking it dead when its life is over or it vanishes.
pub fn move_one(s: &mut Speck, cx: &MoveCx) {
    let age = s.age(cx.now);
    if age >= s.life {
        s.flags |= DEAD;
        return;
    }
    s.previous = s.place;
    cx.forces.apply(s, age, &cx.force);
    let speed = s.velocity.length();
    if speed > cx.ceiling {
        s.velocity *= cx.ceiling / speed;
    }
    s.place += s.velocity * DT;
    s.turn += s.spin * DT;
    if let Some(grid) = cx.grid {
        meet_balls(s, grid, cx.meets, DT);
    }
    if s.flags & DEAD == 0 {
        meet_walls(s, cx.walls, cx.meets);
    }
    if s.flags & DEAD == 0
        && let Some(e) = &cx.edges
    {
        meet_edges(s, e, cx.meets);
    }
    if !(s.place.is_finite() && s.velocity.is_finite() && s.turn.is_finite() && s.flip.is_finite()) {
        s.flags |= DEAD;
    }
}

/// How many threads a step uses on this machine.
pub fn threads() -> usize {
    std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, MOST_THREADS)
}

/// Runs `f` on every particle of `specks`, in chunks of [`CHUNK`] over `threads` threads.
pub fn run_chunks(specks: &mut [Speck], threads: usize, f: &(dyn Fn(&mut Speck) + Sync)) {
    if specks.len() <= CHUNK || threads <= 1 {
        specks.iter_mut().for_each(f);
        return;
    }
    let chunks: Vec<&mut [Speck]> = specks.chunks_mut(CHUNK).collect();
    let per = chunks.len().div_ceil(threads);
    std::thread::scope(|scope| {
        let mut left = chunks.into_iter();
        loop {
            let group: Vec<&mut [Speck]> = left.by_ref().take(per).collect();
            if group.is_empty() {
                break;
            }
            scope.spawn(move || {
                for chunk in group {
                    chunk.iter_mut().for_each(f);
                }
            });
        }
    });
}

/// Every alive ball in the world, by thing then row, for the particles to meet.
pub fn balls_in(world: &World) -> Vec<BallAt> {
    let mut out = Vec::new();
    for b in world.bodies.values() {
        for r in 0..b.len() {
            if b.alive[r] && b.position[r].is_finite() {
                out.push(BallAt { place: b.position[r], velocity: b.velocity[r], radius: b.radius[r].max(0.0) });
            }
        }
    }
    out
}

/// One particle step of `st` in `world`, after world step `world.step` (whose log is `log`),
/// over `threads` threads.
pub fn step(st: &mut ParticlesState, world: &World, log: &Log, threads: usize) {
    let since = world.step.saturating_sub(WORLD_STEPS);
    st.step += 1;
    let anchor = world.anchor(st.id);
    let cx = BirthCx { world, log, anchor: anchor.position, angle: anchor.angle, offset: st.offset };
    births::births(st, &cx, since, false);
    let meets = st.meets.clone();
    let grid = meets.reads_balls().then(|| BallGrid::new(balls_in(world), meets.reach.max(1.0)));
    let walls: Vec<WallAt> = if meets.at_shapes == super::meets::AtShapes::Pass {
        Vec::new()
    } else {
        world
            .walls
            .iter()
            .map(|(id, w)| {
                let before = st.walls_before.get(id).cloned().unwrap_or_else(|| w.outline.clone());
                WallAt::new(w.clone(), before)
            })
            .collect()
    };
    let edges = (meets.at_edges != AtEdges::Pass).then(|| {
        let (centre, half) = births::screen(world, meets.edge_margin);
        Edges { centre, half }
    });
    let forces = st.forces.clone();
    let move_cx = MoveCx {
        forces: &forces,
        meets: &meets,
        grid: grid.as_ref(),
        walls: &walls,
        edges,
        force: ForceCx { time: world.time() as f32, dt: DT, gravity: world.gravity },
        now: st.step,
        ceiling: world.limits().ceiling,
    };
    run_chunks(&mut st.specks, threads, &|s| move_one(s, &move_cx));
    st.specks.retain(|s| s.flags & DEAD == 0);
    note_walls(st, world);
}

/// Notes every wall's outline as it is now, for the next step's sides.
pub fn note_walls(st: &mut ParticlesState, world: &World) {
    if st.meets.at_shapes == super::meets::AtShapes::Pass {
        st.walls_before.clear();
        return;
    }
    st.walls_before = world.walls.iter().map(|(id, w)| (*id, w.outline.clone())).collect();
}

/// The first births, when the world is built (step 0).
pub fn first(st: &mut ParticlesState, world: &World) {
    let anchor = world.anchor(st.id);
    let log = Log::default();
    let cx = BirthCx { world, log: &log, anchor: anchor.position, angle: anchor.angle, offset: st.offset };
    births::births(st, &cx, 0, true);
    note_walls(st, world);
}

/// The hash of every particle's place, velocity and life, for the goldens.
pub fn hash_of(st: &ParticlesState) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut put = |x: f32| {
        for b in x.to_bits().to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    };
    for s in &st.specks {
        put(s.place.x);
        put(s.place.y);
        put(s.velocity.x);
        put(s.velocity.y);
        put(s.life - s.age(st.step));
    }
    h
}

/// The middle of the particles, for tests and the editor.
pub fn middle(st: &ParticlesState) -> Option<Vec2> {
    let n = st.specks.len();
    (n > 0).then(|| st.specks.iter().map(|s| s.place).sum::<Vec2>() / n as f32)
}
