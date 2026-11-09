//! The air (particles part 2): a soft 2D fluid over what the camera sees, which balls stir,
//! shapes block and heat lifts, and which particles that follow the air are carried by.
//!
//! One [`Air`] per world, kept in `World.states` under the scene's id ([`ThingId::ROOT`]), so
//! checkpoints and copies carry it. The scene puts a sleeping one there when it has an Air card;
//! a Particles thing that reads or heats the air wakes it (allocates the grid). Nothing steps it
//! on its own: before each particle step the Particles things bring it to that step
//! ([`Air::catch_up_to`]), so every thing reads the same air whatever their number.
//!
//! The grid is a lattice anchored in the world: cell `(i, j)` covers `[i·c, (i+1)·c) × [j·c,
//! (j+1)·c)`, cell centres at the middle. Each step it covers the camera's shot plus a margin;
//! when the camera moves, the cells both places share are kept. Outside the grid the air is the
//! ambient wind ([`AirSpec::ambient`]). Each step: calm towards the wind, balls stir, heat rises,
//! swirls are kept, then semi-Lagrangian advection and a Jacobi pressure projection. The work
//! splits rows into fixed bands on a lasting pool of threads (rayon's), every cell reading only the
//! previous buffers, so any number of threads gives the same air.

use bpe_core::ThingId;
use bpe_runtime::{Outline, World, wall_velocity};
use glam::{IVec2, Vec2};

use crate::modules::look::layers::particles::sim::noise;

/// Seconds an air step lasts: one particle step.
pub const DT: f32 = 1.0 / 120.0;
/// The most cells the grid has; a bigger view keeps its middle and reads the ambient wind
/// beyond.
/// How far from the world's middle the grid goes at most, px.
const FARTHEST: Vec2 = Vec2::splat(1e8);

pub const MOST_CELLS: usize = 160_000;
/// The most heat a cell holds.
pub const MOST_HEAT: f32 = 1000.0;
/// Jacobi iterations of the pressure solve.
const JACOBI: usize = 20;
/// From this many cells the step splits its rows over threads.
const THREADED_CELLS: usize = 32_768;
/// The most threads the air uses.
const MOST_THREADS: usize = 16;
/// How strongly a ball takes the air in its middle along, per second at Balls stir 1.
const STIR_RATE: f32 = 24.0;

/// How strong a gust is at `place` and `time`: 1 without gusts. Gusts of `gusts` (0..1) come
/// round every `every` seconds; with `waves` (px) above 0 they travel along `way` with that
/// distance between them, else the whole picture gusts at once.
pub fn gust_factor(gusts: f32, every: f32, waves: f32, time: f32, place: Vec2, way: Vec2) -> f32 {
    if gusts == 0.0 {
        return 1.0;
    }
    let mut phase = time / every.max(0.01);
    if waves > 0.0 && waves.is_finite() {
        phase -= place.dot(way) / waves;
    }
    1.0 + gusts * noise(Vec2::new(phase, 3.7))
}

/// A number that is finite, else 0.
fn fin(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

/// What the air is set to: the Air card's settings, as the solver uses them.
#[derive(Clone, Debug, PartialEq)]
pub struct AirSpec {
    /// The steady wind, px/s.
    pub wind_speed: f32,
    /// The way it blows, degrees: 0 to the right, 90 down.
    pub wind_direction: f32,
    pub gusts: f32,
    pub gust_every: f32,
    /// The distance between gusts as they travel, px; 0 gusts everywhere at once.
    pub waves: f32,
    /// How fast the air settles back to the wind, per second.
    pub calm: f32,
    /// A cell's side, px.
    pub cell: f32,
    /// How strongly balls take the air along.
    pub stir: f32,
    /// How far round a ball it stirs, in its radii.
    pub stir_reach: f32,
    /// How hard a unit of heat lifts the air, px/s².
    pub rises: f32,
    /// How fast heat fades, per second.
    pub fades: f32,
    /// How much the air's swirls are kept from smoothing out (vorticity confinement).
    pub swirls: f32,
    /// Shapes are solid to the air.
    pub shapes_block: bool,
    /// How far past the camera's shot the grid reaches, px.
    pub margin: f32,
}

impl Default for AirSpec {
    fn default() -> Self {
        AirSpec {
            wind_speed: 0.0,
            wind_direction: 0.0,
            gusts: 0.0,
            gust_every: 2.0,
            waves: 600.0,
            calm: 1.0,
            cell: 16.0,
            stir: 1.0,
            stir_reach: 1.5,
            rises: 60.0,
            fades: 1.0,
            swirls: 0.3,
            shapes_block: true,
            margin: 200.0,
        }
    }
}

impl AirSpec {
    /// The way the wind blows.
    pub fn way(&self) -> Vec2 {
        Vec2::from_angle(fin(self.wind_direction).to_radians())
    }

    /// The wind at `place` and `time`, gusts and all: the air where nothing stirs it.
    pub fn ambient(&self, place: Vec2, time: f32) -> Vec2 {
        let way = self.way();
        let gust = gust_factor(fin(self.gusts), self.gust_every, fin(self.waves), time, place, way);
        way * fin(self.wind_speed) * gust
    }

    fn cell_size(&self) -> f32 {
        if self.cell.is_finite() { self.cell.clamp(2.0, 512.0) } else { 16.0 }
    }
}

/// The air of a world.
#[derive(Clone, Debug)]
pub struct Air {
    pub spec: AirSpec,
    /// Steps taken (particle steps: the world's time × 120).
    pub step: u32,
    awake: bool,
    /// The cell size the grid was made with.
    cell: f32,
    /// The lattice cell of column 0, row 0.
    origin: IVec2,
    cols: usize,
    rows: usize,
    /// Each cell's velocity, px/s, row by row.
    vel: Vec<Vec2>,
    /// Each cell's heat.
    heat: Vec<f32>,
    /// Heat put in since the last step, added at the next one.
    pending: Vec<f32>,
    /// Which cells a shape fills, and how fast the shape moves there.
    solid: Vec<bool>,
    solid_vel: Vec<Vec2>,
    /// What `solid` was worked out from: the walls' outlines and spins, and the grid's place.
    mask_of: Option<MaskKey>,
    /// How far the wind has carried the air since the start, px: the ambient wind summed over
    /// every world step, gusts and keys and all (the scene sim adds to it, awake or asleep). Moving
    /// haze drifts by it (Haze moves With the air, W13).
    pub drift: Vec2,
}

/// What a solid mask was worked out from.
#[derive(Clone, Debug, PartialEq)]
struct MaskKey {
    walls: Vec<(ThingId, Outline, f32, f32, Vec2)>,
    origin: IVec2,
    cols: usize,
    rows: usize,
}

impl Air {
    /// A sleeping air: its settings, no grid.
    pub fn new(spec: AirSpec) -> Air {
        Air {
            spec,
            step: 0,
            awake: false,
            cell: 0.0,
            origin: IVec2::ZERO,
            cols: 0,
            rows: 0,
            vel: Vec::new(),
            heat: Vec::new(),
            pending: Vec::new(),
            solid: Vec::new(),
            solid_vel: Vec::new(),
            mask_of: None,
            drift: Vec2::ZERO,
        }
    }

    /// Whether it has a grid and steps.
    pub fn awake(&self) -> bool {
        self.awake
    }

    /// Gives it a grid over what `world`'s camera sees, the ambient wind everywhere.
    pub fn wake(&mut self, world: &World) {
        if self.awake {
            return;
        }
        self.awake = true;
        self.cols = 0;
        self.rows = 0;
        self.vel.clear();
        self.heat.clear();
        self.pending.clear();
        self.place_grid(world);
    }

    /// Seconds since the start, as the air has got to.
    pub fn time(&self) -> f32 {
        self.step as f32 * DT
    }

    /// Bytes its grid takes, for playback budgets.
    pub fn memory(&self) -> usize {
        self.vel.capacity() * 8
            + (self.heat.capacity() + self.pending.capacity()) * 4
            + self.solid.capacity()
            + self.solid_vel.capacity() * 8
    }

    /// The grid's columns and rows.
    pub fn size(&self) -> (usize, usize) {
        (self.cols, self.rows)
    }

    /// The lattice cell of column 0, row 0: its top-left corner is `origin × cell_size`.
    pub fn origin(&self) -> IVec2 {
        self.origin
    }

    /// A cell's side, px.
    pub fn cell_size(&self) -> f32 {
        self.cell
    }

    /// Every cell's velocity, row by row (cell centres).
    pub fn velocities(&self) -> &[Vec2] {
        &self.vel
    }

    /// The middle of cell (`col`, `row`).
    pub fn centre(&self, col: usize, row: usize) -> Vec2 {
        (Vec2::new(col as f32, row as f32) + self.origin.as_vec2() + 0.5) * self.cell
    }

    /// The grid's continuous coordinates of `p` (0 at the first cell's middle), when it is over
    /// the grid.
    fn grid_at(&self, p: Vec2) -> Option<Vec2> {
        if !self.awake || self.cols == 0 || !p.is_finite() {
            return None;
        }
        let g = p / self.cell - self.origin.as_vec2();
        (g.x >= 0.0 && g.y >= 0.0 && g.x <= self.cols as f32 && g.y <= self.rows as f32).then(|| g - 0.5)
    }

    /// The air's velocity at `p`: between cell middles, and the ambient wind off the grid.
    pub fn at(&self, p: Vec2) -> Vec2 {
        match self.grid_at(p) {
            Some(g) => bilinear(&self.vel, self.cols, self.rows, g),
            None => self.spec.ambient(p, self.time()),
        }
    }

    /// The air's heat at `p`, 0 off the grid.
    pub fn heat_at(&self, p: Vec2) -> f32 {
        match self.grid_at(p) {
            Some(g) => bilinear(&self.heat, self.cols, self.rows, g),
            None => 0.0,
        }
    }

    /// Puts `amount` of heat into the cell at `p`, for the next step.
    pub fn add_heat(&mut self, p: Vec2, amount: f32) {
        if !(amount.is_finite() && amount > 0.0) {
            return;
        }
        if let Some(g) = self.grid_at(p) {
            let c = (g.x + 0.5).clamp(0.0, self.cols as f32 - 1.0) as usize;
            let r = (g.y + 0.5).clamp(0.0, self.rows as f32 - 1.0) as usize;
            let k = r * self.cols + c;
            self.pending[k] = (self.pending[k] + amount).min(MOST_HEAT);
        }
    }

    /// Takes the air steps due until it has taken `step`, over `threads` threads.
    pub fn catch_up_to(&mut self, step: u32, world: &World, threads: usize) {
        if !self.awake {
            self.step = self.step.max(step);
            return;
        }
        while self.step < step {
            self.step += 1;
            self.step_once(world, threads);
        }
    }

    /// Where the grid should be for `world`'s camera: its first lattice cell, columns and rows.
    fn wanted_place(&self, world: &World, cell: f32) -> (IVec2, usize, usize) {
        let margin = fin(self.spec.margin).max(0.0);
        let half = world.camera.shot_bounds_half(world.video) + Vec2::splat(margin);
        // Far out of the way, the grid stops at ±1e8 px (its cells still count in an i32).
        let centre =
            if world.camera.center.is_finite() { world.camera.center.clamp(-FARTHEST, FARTHEST) } else { Vec2::ZERO };
        let half = if half.is_finite() { half.clamp(Vec2::ONE, Vec2::splat(1e7)) } else { world.video * 0.5 };
        let lo = ((centre - half) / cell).floor();
        let hi = ((centre + half) / cell).ceil();
        let mut cols = (hi.x - lo.x).max(1.0) as usize;
        let mut rows = (hi.y - lo.y).max(1.0) as usize;
        let mut origin = IVec2::new(lo.x as i32, lo.y as i32);
        if cols * rows > MOST_CELLS {
            let shrink = (MOST_CELLS as f64 / (cols * rows) as f64).sqrt();
            let (c2, r2) = (((cols as f64 * shrink) as usize).max(1), ((rows as f64 * shrink) as usize).max(1));
            origin += IVec2::new(((cols - c2) / 2) as i32, ((rows - r2) / 2) as i32);
            cols = c2;
            rows = r2;
        }
        (origin, cols, rows)
    }

    /// Whole cell numbers `at` (counted from the world's middle) as the grid's own, held to one
    /// cell either side of it, so a ball or wall far off (or gone wrong) loops over nothing.
    fn on_grid(&self, at: Vec2) -> IVec2 {
        let lo = Vec2::splat(-1.0);
        let hi = Vec2::new(self.cols as f32, self.rows as f32);
        let at = at - self.origin.as_vec2();
        if at.is_nan() {
            return IVec2::splat(-1);
        }
        at.clamp(lo, hi).as_ivec2()
    }

    /// Moves the grid to where the camera is, keeping the cells the old and new places share.
    fn place_grid(&mut self, world: &World) {
        let cell = self.spec.cell_size();
        if cell != self.cell {
            // Another cell size: the air starts again as the wind.
            self.cell = cell;
            self.cols = 0;
            self.rows = 0;
        }
        let (origin, cols, rows) = self.wanted_place(world, cell);
        if origin == self.origin && cols == self.cols && rows == self.rows {
            return;
        }
        let time = self.time();
        let n = cols * rows;
        let mut vel = Vec::with_capacity(n);
        let mut heat = vec![0.0; n];
        let mut pending = vec![0.0; n];
        for r in 0..rows {
            for c in 0..cols {
                let at = origin + IVec2::new(c as i32, r as i32);
                let old = at - self.origin;
                if old.x >= 0 && old.y >= 0 && (old.x as usize) < self.cols && (old.y as usize) < self.rows {
                    let k = old.y as usize * self.cols + old.x as usize;
                    vel.push(self.vel[k]);
                    heat[r * cols + c] = self.heat[k];
                    pending[r * cols + c] = self.pending[k];
                } else {
                    vel.push(self.spec.ambient((at.as_vec2() + 0.5) * cell, time));
                }
            }
        }
        self.origin = origin;
        self.cols = cols;
        self.rows = rows;
        self.vel = vel;
        self.heat = heat;
        self.pending = pending;
        self.mask_of = None;
    }

    /// Works out which cells the shapes fill, when the walls or the grid moved.
    fn mask(&mut self, world: &World) {
        let walls: Vec<(ThingId, Outline, f32, f32, Vec2)> = if self.spec.shapes_block {
            world
                .walls
                .iter()
                .filter(|(_, w)| !w.passes)
                .map(|(id, w)| (*id, w.outline.clone(), w.spin, w.growth, w.velocity))
                .collect()
        } else {
            Vec::new()
        };
        let key = MaskKey { walls, origin: self.origin, cols: self.cols, rows: self.rows };
        if self.mask_of.as_ref() == Some(&key) {
            return;
        }
        let n = self.cols * self.rows;
        self.solid.clear();
        self.solid.resize(n, false);
        self.solid_vel.clear();
        self.solid_vel.resize(n, Vec2::ZERO);
        let half_cell = self.cell * 0.5;
        for (id, outline, spin, growth, velocity) in &key.walls {
            let Some(wall) = world.walls.get(id) else { continue };
            let centre = outline.center();
            let band = outline.thickness() * 0.5 + half_cell;
            let reach = outline.reach() + band + self.cell;
            let lo = self.on_grid(((centre - reach) / self.cell).floor());
            let hi = self.on_grid(((centre + reach) / self.cell).ceil());
            let (c0, r0) = (lo.x.max(0) as usize, lo.y.max(0) as usize);
            let (c1, r1) = (hi.x.min(self.cols as i32).max(0) as usize, hi.y.min(self.rows as i32).max(0) as usize);
            for r in r0..r1 {
                for c in c0..c1 {
                    let p = self.centre(c, r);
                    let (q, _) = outline.nearest(p);
                    let covered = (wall.solid && outline.inside(p)) || p.distance(q) < band;
                    if covered {
                        let k = r * self.cols + c;
                        self.solid[k] = true;
                        self.solid_vel[k] = *velocity + wall_velocity(p, centre, *spin, *growth);
                    }
                }
            }
        }
        self.mask_of = Some(key);
    }

    /// One air step at the world as it stands.
    fn step_once(&mut self, world: &World, threads: usize) {
        self.place_grid(world);
        self.mask(world);
        let (cols, rows, h) = (self.cols, self.rows, self.cell);
        let time = self.time();
        let ceiling = world.limits().ceiling;
        let threads = if cols * rows >= THREADED_CELLS { threads.clamp(1, MOST_THREADS) } else { 1 };

        // Calm towards the wind.
        let calm = (-fin(self.spec.calm).max(0.0) * DT).exp();
        {
            let (spec, origin) = (&self.spec, self.origin.as_vec2());
            let f = |r: usize, row: &mut [Vec2]| {
                for (c, v) in row.iter_mut().enumerate() {
                    let w = spec.ambient((Vec2::new(c as f32, r as f32) + origin + 0.5) * h, time);
                    *v = w + (*v - w) * calm;
                }
            };
            by_rows(&mut self.vel, cols, threads, &f);
        }

        // Balls stir the cells round them, in body-row order.
        let stir = fin(self.spec.stir);
        let reach_of = fin(self.spec.stir_reach).max(0.0);
        if stir != 0.0 && reach_of > 0.0 {
            for b in world.bodies.values() {
                for i in 0..b.len() {
                    let (p, v, radius) = (b.position[i], b.velocity[i], b.radius[i]);
                    if !b.alive[i] || !p.is_finite() || !v.is_finite() || !(radius.is_finite() && radius > 0.0) {
                        continue;
                    }
                    let v = v.clamp_length_max(ceiling);
                    let reach = radius * reach_of;
                    let lo = self.on_grid(((p - reach) / h - 0.5).ceil());
                    let hi = self.on_grid(((p + reach) / h - 0.5).floor());
                    for r in lo.y.max(0)..=hi.y.min(rows as i32 - 1) {
                        for c in lo.x.max(0)..=hi.x.min(cols as i32 - 1) {
                            let (c, r) = (c as usize, r as usize);
                            let d = self.centre(c, r).distance(p);
                            if d >= reach {
                                continue;
                            }
                            let f = (1.0 - d / reach).powi(2);
                            let take = 1.0 - (-STIR_RATE * stir * f * DT).exp();
                            let k = r * cols + c;
                            let now = self.vel[k];
                            self.vel[k] = now + (v - now) * take;
                        }
                    }
                }
            }
        }

        // Heat: what came in, lifting the air, then fading.
        let rises = fin(self.spec.rises);
        let fades = (-fin(self.spec.fades).max(0.0) * DT).exp();
        for k in 0..cols * rows {
            // No heat goes into a shape: a cell a solid shape covers stays cold.
            let t = if self.solid.get(k).copied().unwrap_or(false) {
                0.0
            } else {
                (self.heat[k] + self.pending[k]).clamp(0.0, MOST_HEAT)
            };
            self.pending[k] = 0.0;
            self.vel[k].y -= rises * t * DT;
            self.heat[k] = t * fades;
        }

        // Swirls kept: vorticity confinement.
        let swirls = fin(self.spec.swirls);
        if swirls != 0.0 && cols > 2 && rows > 2 {
            let curl = curl_of(&self.vel, cols, rows, h, threads);
            let at = |c: usize, r: usize| curl[r * cols + c].abs();
            // Each cell reads only the curl: the rows can go in any order.
            let f = |r: usize, row: &mut [Vec2]| {
                if r == 0 || r == rows - 1 {
                    return;
                }
                for c in 1..cols - 1 {
                    let grad = Vec2::new(at(c + 1, r) - at(c - 1, r), at(c, r + 1) - at(c, r - 1));
                    let Some(n) = grad.try_normalize() else { continue };
                    let w = curl[r * cols + c];
                    row[c] += Vec2::new(n.y * w, -n.x * w) * swirls * h * DT * 8.0;
                }
            };
            by_rows(&mut self.vel, cols, threads, &f);
        }

        // Carried along by itself.
        let spec = &self.spec;
        let (origin, vel, heat) = (self.origin, &self.vel, &self.heat);
        let mut new_vel = vec![Vec2::ZERO; cols * rows];
        let mut new_heat = vec![0.0f32; cols * rows];
        let advect = |r: usize, out_v: &mut [Vec2], out_h: &mut [f32]| {
            for c in 0..cols {
                let k = r * cols + c;
                let centre = (Vec2::new(c as f32, r as f32) + origin.as_vec2() + 0.5) * h;
                let from = centre - vel[k] * DT;
                let g = from / h - origin.as_vec2();
                if g.x >= 0.0 && g.y >= 0.0 && g.x <= cols as f32 && g.y <= rows as f32 {
                    out_v[c] = bilinear(vel, cols, rows, g - 0.5);
                    out_h[c] = bilinear(heat, cols, rows, g - 0.5);
                } else {
                    out_v[c] = spec.ambient(from, time);
                    out_h[c] = 0.0;
                }
            }
        };
        by_rows2(&mut new_vel, &mut new_heat, cols, threads, &advect);

        // Shapes move the air in them with themselves.
        for ((v, solid), moving) in new_vel.iter_mut().zip(&self.solid).zip(&self.solid_vel) {
            if *solid {
                *v = *moving;
            }
        }

        // Projected: no air piles up or thins out.
        project(&mut new_vel, &self.solid, cols, rows, h, threads, |c, r| {
            spec.ambient((Vec2::new(c as f32, r as f32) + origin.as_vec2() + 0.5) * h, time)
        });

        let (new_vel, new_heat) = (&new_vel, &new_heat);
        let f = |r: usize, vel: &mut [Vec2], heat: &mut [f32]| {
            for c in 0..cols {
                let k = r * cols + c;
                let mut v = new_vel[k];
                if !v.is_finite() {
                    v = spec.ambient((Vec2::new(c as f32, r as f32) + origin.as_vec2() + 0.5) * h, time);
                }
                vel[c] = v.clamp_length_max(ceiling);
                let t = new_heat[k];
                heat[c] = if t.is_finite() { t.clamp(0.0, MOST_HEAT) } else { 0.0 };
            }
        };
        by_rows2(&mut self.vel, &mut self.heat, cols, threads, &f);
    }

    /// Every cell's middle and velocity, for drawing (row by row).
    pub fn cells(&self) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
        (0..self.rows).flat_map(move |r| (0..self.cols).map(move |c| (self.centre(c, r), self.vel[r * self.cols + c])))
    }
}

/// `values` between cell middles at grid place `g` (0 at the first cell's middle), clamped to the
/// grid.
fn bilinear<T>(values: &[T], cols: usize, rows: usize, g: Vec2) -> T
where
    T: Copy + std::ops::Mul<f32, Output = T> + std::ops::Add<Output = T>,
{
    let x = g.x.clamp(0.0, (cols - 1) as f32);
    let y = g.y.clamp(0.0, (rows - 1) as f32);
    let (c0, r0) = (x.floor() as usize, y.floor() as usize);
    let (c1, r1) = ((c0 + 1).min(cols - 1), (r0 + 1).min(rows - 1));
    let (fx, fy) = (x - c0 as f32, y - r0 as f32);
    let at = |c: usize, r: usize| values[r * cols + c];
    let top = at(c0, r0) * (1.0 - fx) + at(c1, r0) * fx;
    let bottom = at(c0, r1) * (1.0 - fx) + at(c1, r1) * fx;
    top * (1.0 - fy) + bottom * fy
}

/// Each cell's curl (how fast the air turns there), with the grid's edge cells' neighbours
/// clamped.
fn curl_of(vel: &[Vec2], cols: usize, rows: usize, h: f32, threads: usize) -> Vec<f32> {
    let at = |c: usize, r: usize| vel[r * cols + c];
    let mut out = vec![0.0; cols * rows];
    let f = |r: usize, row: &mut [f32]| {
        for (c, cell) in row.iter_mut().enumerate() {
            let (l, rt) = (c.saturating_sub(1), (c + 1).min(cols - 1));
            let (u, d) = (r.saturating_sub(1), (r + 1).min(rows - 1));
            let dvy_dx = (at(rt, r).y - at(l, r).y) / (h * (rt - l).max(1) as f32);
            let dvx_dy = (at(c, d).x - at(c, u).x) / (h * (d - u).max(1) as f32);
            *cell = dvy_dx - dvx_dy;
        }
    };
    by_rows(&mut out, cols, threads, &f);
    out
}

/// Bands of rows each thread works on: fixed by the row count and thread count alone.
fn bands(rows: usize, threads: usize) -> usize {
    rows.div_ceil(threads.max(1)).max(1)
}

/// What fills a row of two buffers at once.
type RowsFn<'a, A, B> = &'a (dyn Fn(usize, &mut [A], &mut [B]) + Sync);

/// Fills `a` and `b` row by row with `f(row, a_row, b_row)`, over `threads` threads.
fn by_rows2<A: Send, B: Send>(a: &mut [A], b: &mut [B], cols: usize, threads: usize, f: RowsFn<'_, A, B>) {
    let rows = a.len() / cols.max(1);
    if threads <= 1 || rows < 2 {
        for (r, (ra, rb)) in a.chunks_mut(cols).zip(b.chunks_mut(cols)).enumerate() {
            f(r, ra, rb);
        }
        return;
    }
    let per = bands(rows, threads);
    // On the lasting pool: a step runs this some 25 times, too often to start threads for.
    rayon::scope(|scope| {
        for (band, (ba, bb)) in a.chunks_mut(per * cols).zip(b.chunks_mut(per * cols)).enumerate() {
            scope.spawn(move |_| {
                for (i, (ra, rb)) in ba.chunks_mut(cols).zip(bb.chunks_mut(cols)).enumerate() {
                    f(band * per + i, ra, rb);
                }
            });
        }
    });
}

/// Fills `a` row by row with `f(row, a_row)`, over `threads` threads.
fn by_rows<A: Send>(a: &mut [A], cols: usize, threads: usize, f: &(dyn Fn(usize, &mut [A]) + Sync)) {
    let rows = a.len() / cols.max(1);
    if threads <= 1 || rows < 2 {
        for (r, ra) in a.chunks_mut(cols).enumerate() {
            f(r, ra);
        }
        return;
    }
    let per = bands(rows, threads);
    rayon::scope(|scope| {
        for (band, ba) in a.chunks_mut(per * cols).enumerate() {
            scope.spawn(move |_| {
                for (i, ra) in ba.chunks_mut(cols).enumerate() {
                    f(band * per + i, ra);
                }
            });
        }
    });
}

/// Takes the part of `vel` that piles air up or thins it out away: a pressure from [`JACOBI`]
/// iterations, its gradient taken off every open cell. Solid cells keep their velocity and mirror
/// their neighbour's pressure; past the grid's edge the air is `outside(col, row)` and the
/// pressure 0, so wind blows through.
fn project(
    vel: &mut [Vec2],
    solid: &[bool],
    cols: usize,
    rows: usize,
    h: f32,
    threads: usize,
    outside: impl Fn(i64, i64) -> Vec2 + Sync,
) {
    let n = cols * rows;
    let v_at = |c: i64, r: i64| -> Vec2 {
        if c < 0 || r < 0 || c >= cols as i64 || r >= rows as i64 {
            outside(c, r)
        } else {
            vel[r as usize * cols + c as usize]
        }
    };
    let mut div = vec![0.0f32; n];
    {
        let f = |r: usize, out: &mut [f32]| {
            for c in 0..cols {
                if solid[r * cols + c] {
                    out[c] = 0.0;
                    continue;
                }
                let (ci, ri) = (c as i64, r as i64);
                out[c] =
                    (v_at(ci + 1, ri).x - v_at(ci - 1, ri).x + v_at(ci, ri + 1).y - v_at(ci, ri - 1).y) / (2.0 * h);
            }
        };
        by_rows(&mut div, cols, threads, &f);
    }
    let mut p = vec![0.0f32; n];
    let mut next = vec![0.0f32; n];
    for _ in 0..JACOBI {
        {
            let p = &p;
            let div = &div;
            // The busiest loop of the air: neighbours by index, past the grid's edge 0 and in a solid
            // cell the cell's own pressure, added in the same order as ever (left, right, up, down).
            let f = |r: usize, out: &mut [f32]| {
                let row = r * cols;
                let up = (r > 0).then(|| row - cols);
                let down = (r + 1 < rows).then(|| row + cols);
                for (c, cell) in out.iter_mut().enumerate() {
                    let k = row + c;
                    if solid[k] {
                        *cell = 0.0;
                        continue;
                    }
                    let mine = p[k];
                    let side = |k: usize| if solid[k] { mine } else { p[k] };
                    let left = if c > 0 { side(k - 1) } else { 0.0 };
                    let right = if c + 1 < cols { side(k + 1) } else { 0.0 };
                    let top = up.map_or(0.0, |u| side(u + c));
                    let bottom = down.map_or(0.0, |d| side(d + c));
                    *cell = (left + right + top + bottom - h * h * div[k]) * 0.25;
                }
            };
            by_rows(&mut next, cols, threads, &f);
        }
        std::mem::swap(&mut p, &mut next);
    }
    let p_at = |c: i64, r: i64, mine: f32| -> f32 {
        if c < 0 || r < 0 || c >= cols as i64 || r >= rows as i64 {
            0.0
        } else {
            let k = r as usize * cols + c as usize;
            if solid[k] { mine } else { p[k] }
        }
    };
    let f = |r: usize, row: &mut [Vec2]| {
        for (c, v) in row.iter_mut().enumerate() {
            let k = r * cols + c;
            if solid[k] {
                continue;
            }
            let (ci, ri) = (c as i64, r as i64);
            let mine = p[k];
            let grad = Vec2::new(
                p_at(ci + 1, ri, mine) - p_at(ci - 1, ri, mine),
                p_at(ci, ri + 1, mine) - p_at(ci, ri - 1, mine),
            ) / (2.0 * h);
            *v -= grad;
        }
    };
    by_rows(vel, cols, threads, &f);
}

/// The world's air, when it has one.
pub fn air_of(world: &World) -> Option<&Air> {
    (**world.states.get(&ThingId::ROOT)?).as_any().downcast_ref::<Air>()
}

/// The world's air to change, when it has one.
pub fn air_mut(world: &mut World) -> Option<&mut Air> {
    world.states.get_mut(&ThingId::ROOT)?.as_any_mut().downcast_mut::<Air>()
}

/// The world's air, taken out of it so it can step beside a `&World` ([`take_air`]).
pub struct TakenAir(Box<dyn bpe_runtime::ThingState>);

impl TakenAir {
    pub fn get(&self) -> &Air {
        bpe_runtime::state::<Air>(self.0.as_ref())
    }
    pub fn get_mut(&mut self) -> &mut Air {
        bpe_runtime::state_mut::<Air>(self.0.as_mut())
    }
}

/// Takes the world's air out of it, when it has one.
pub fn take_air(world: &mut World) -> Option<TakenAir> {
    let boxed = world.states.remove(&ThingId::ROOT)?;
    if (*boxed).as_any().is::<Air>() {
        Some(TakenAir(boxed))
    } else {
        world.states.insert(ThingId::ROOT, boxed);
        None
    }
}

/// Puts the air back into the world, noting how much memory it takes.
pub fn put_air(world: &mut World, air: TakenAir) {
    world.state_memory.insert(ThingId::ROOT, air.get().memory());
    world.states.insert(ThingId::ROOT, air.0);
}

/// The air as the physics reads it (W13): balls that follow the air find it awake from the start
/// and brought to each step before the forces, every world alike. The particles' own catch-up then
/// finds it there already.
pub struct KitAir;

impl bpe_runtime::AirSim for KitAir {
    fn wake(&self, world: &mut World) {
        let mut air = match take_air(world) {
            Some(air) => air,
            None => {
                world.states.insert(ThingId::ROOT, Box::new(Air::new(AirSpec::default())));
                take_air(world).expect("just put there")
            }
        };
        if !air.get().awake() {
            let at = crate::things::particles::step::due(world.step, world.steps_per_second);
            let a = air.get_mut();
            a.wake(world);
            a.step = a.step.max(at);
        }
        put_air(world, air);
    }

    fn catch_up(&self, world: &mut World) {
        let Some(mut air) = take_air(world) else { return };
        let due = crate::things::particles::step::due(world.step, world.steps_per_second);
        air.get_mut().catch_up_to(due, world, crate::things::particles::step::threads());
        put_air(world, air);
    }

    fn velocities(&self, world: &World, points: &[Vec2]) -> Option<Vec<Vec2>> {
        let air = air_of(world)?;
        Some(points.iter().map(|p| air.at(*p)).collect())
    }
}

/// The cells of the air in a map by lattice place, for tests.
#[cfg(test)]
pub(crate) fn by_place(air: &Air) -> std::collections::BTreeMap<(i32, i32), Vec2> {
    let mut out = std::collections::BTreeMap::new();
    for r in 0..air.rows {
        for c in 0..air.cols {
            out.insert((air.origin.x + c as i32, air.origin.y + r as i32), air.vel[r * air.cols + c]);
        }
    }
    out
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod follow_tests;
