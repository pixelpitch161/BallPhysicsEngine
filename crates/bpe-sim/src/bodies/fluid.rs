//! A small amount of liquid as position-based fluid: each particle is nudged until the
//! particles around it are as tightly packed as at the start, never more, which makes them
//! flow, splash and settle into a level surface.

use super::Point;
use crate::broadphase::Grid;
use bpe_scene::Vec2;
use rayon::prelude::*;
use std::f32::consts::PI;
use std::ops::Range;

/// Particles handled together by one thread. Each particle's result depends only on the
/// positions at the start of the loop, so splitting the work never changes the result.
const CHUNK: usize = 256;
/// Fewer particles than this are quicker on one thread.
const PARALLEL_FROM: usize = 1024;

/// Sets `out[i] = f(i)` for every `i`, on several threads when there are many.
fn each<T: Send>(out: &mut [T], f: impl Fn(usize) -> T + Sync) {
    if out.len() < PARALLEL_FROM {
        out.iter_mut().enumerate().for_each(|(i, v)| *v = f(i));
    } else {
        out.par_chunks_mut(CHUNK).enumerate().for_each(|(c, chunk)| {
            for (k, v) in chunk.iter_mut().enumerate() {
                *v = f(c * CHUNK + k);
            }
        });
    }
}

#[derive(Clone, Default)]
pub struct Fluid {
    /// The liquid's particles in the soft world's points.
    range: Range<u32>,
    /// How far a particle feels its neighbours, in pixels.
    reach: f32,
    /// Packing at rest, and a small number that keeps the solver steady.
    rest: f32,
    relax: f32,
    /// How much each particle takes on its neighbours' speed per pass, 0 to 1.
    stickiness: f32,
    grid: Grid,
    /// Each particle's neighbours (as indices into `range`), in one list.
    starts: Vec<u32>,
    neighbours: Vec<u32>,
    /// Scratch: the runs of grid entries around each particle.
    around: Vec<[Range<usize>; 3]>,
    /// Scratch: each chunk's neighbour lists, joined into `neighbours` in order.
    chunks: Vec<(Vec<u32>, Vec<u32>)>,
    lambda: Vec<f32>,
    change: Vec<Vec2>,
}

/// Smoothing kernel: how much a neighbour at `r` counts towards packing (2D poly6).
fn weight(r2: f32, h: f32) -> f32 {
    let h2 = h * h;
    if r2 >= h2 {
        return 0.0;
    }
    let d = h2 - r2;
    4.0 / (PI * h2 * h2 * h2 * h2) * d * d * d
}

/// Gradient of the spiky kernel at offset `d` (from the neighbour to the particle).
fn slope(d: Vec2, h: f32) -> Vec2 {
    let r = d.length();
    if r >= h || r < 1e-6 {
        return Vec2::ZERO;
    }
    let k = -30.0 / (PI * h.powi(5)) * (h - r) * (h - r);
    d * (k / r)
}

impl Fluid {
    pub fn count(&self) -> usize {
        self.range.len()
    }

    /// Sets up the liquid for particles of `radius` started evenly packed.
    pub fn setup(&mut self, range: Range<u32>, radius: f32, thickness: f32, points: &[Point]) {
        let _ = points;
        self.range = range;
        self.reach = radius * 3.2;
        // Packing and gradient size of a particle inside the starting lattice.
        let spacing = radius * 2.0;
        let (mut density, mut sum, mut squared) = (0.0, Vec2::ZERO, 0.0);
        for row in -4i32..=4 {
            for column in -4i32..=4 {
                let shift = if row.rem_euclid(2) == 1 { spacing * 0.5 } else { 0.0 };
                let p = Vec2::new(column as f32 * spacing + shift, row as f32 * spacing * 0.866);
                density += weight(p.length_squared(), self.reach);
                let g = slope(-p, self.reach);
                sum += g;
                squared += g.length_squared();
            }
        }
        self.rest = density.max(1e-12);
        let typical = (squared + sum.length_squared()) / (self.rest * self.rest);
        self.relax = typical * 0.05 + 1e-12;
        self.stickiness = 0.02 + 0.4 * thickness.clamp(0.0, 1.0);
    }

    /// Finds each particle's neighbours for this step.
    pub fn find_neighbours(&mut self, points: &[Point], _shared: &Grid) {
        let n = self.count();
        self.starts.clear();
        self.neighbours.clear();
        if n == 0 {
            return;
        }
        let first = self.range.start as usize;
        let liquid = &points[first..first + n];
        // A little more than the kernel's reach, as particles move during the step.
        let reach = self.reach * 1.1;
        self.grid.rebuild(reach, liquid.iter().map(|p| p.position));
        // The cells around each particle, worked out once per cell rather than per particle.
        self.around.clear();
        self.around.resize(n, [0..0, 0..0, 0..0]);
        for ((cx, cy), run) in self.grid.cells() {
            let runs = self.grid.runs_around(cx, cy);
            for i in self.grid.run(run) {
                self.around[i as usize] = runs.clone();
            }
        }
        let chunks = n.div_ceil(CHUNK);
        self.chunks.resize_with(chunks, Default::default);
        let (grid, around) = (&self.grid, &self.around);
        let find = |c: usize, (counts, found): &mut (Vec<u32>, Vec<u32>)| {
            counts.clear();
            found.clear();
            for i in c * CHUNK..((c + 1) * CHUNK).min(n) {
                let p = liquid[i].position;
                for run in around[i].clone() {
                    for e in run {
                        let (j, at) = grid.entry(e);
                        if j as usize != i && at.distance_squared(p) < reach * reach {
                            found.push(j);
                        }
                    }
                }
                counts.push(found.len() as u32);
            }
        };
        if n < PARALLEL_FROM {
            self.chunks.iter_mut().enumerate().for_each(|(c, chunk)| find(c, chunk));
        } else {
            self.chunks[..chunks].par_iter_mut().enumerate().for_each(|(c, chunk)| find(c, chunk));
        }
        for (counts, found) in &self.chunks[..chunks] {
            let base = self.neighbours.len() as u32;
            let mut start = 0;
            for &end in counts {
                self.starts.push(base + start);
                start = end;
            }
            self.neighbours.extend_from_slice(found);
        }
        self.starts.push(self.neighbours.len() as u32);
        self.lambda.resize(n, 0.0);
        self.change.resize(n, Vec2::ZERO);
    }

    /// One pass of keeping the particles evenly packed.
    pub fn solve(&mut self, points: &mut [Point], _dt: f32) {
        let n = self.count();
        if n == 0 || self.starts.len() != n + 1 {
            return;
        }
        let first = self.range.start as usize;
        let h = self.reach;
        let (rest, relax) = (self.rest, self.relax);
        let liquid = &points[first..first + n];
        let (starts, neighbours) = (&self.starts, &self.neighbours);
        let around = |i: usize| &neighbours[starts[i] as usize..starts[i + 1] as usize];
        each(&mut self.lambda, |i| {
            let p = liquid[i].position;
            let mut density = weight(0.0, h);
            let (mut sum, mut squared) = (Vec2::ZERO, 0.0);
            for &j in around(i) {
                let d = p - liquid[j as usize].position;
                density += weight(d.length_squared(), h);
                let g = slope(d, h) / rest;
                sum += g;
                squared += g.length_squared();
            }
            let pressure = density / rest - 1.0;
            // Only push apart: letting particles pull together makes them clump into strings.
            if pressure > 0.0 { -pressure / (squared + sum.length_squared() + relax) } else { 0.0 }
        });
        let lambda = &self.lambda;
        each(&mut self.change, |i| {
            let p = liquid[i].position;
            let mut change = Vec2::ZERO;
            for &j in around(i) {
                let d = p - liquid[j as usize].position;
                change += slope(d, h) * (lambda[i] + lambda[j as usize]);
            }
            // Never more than half a particle per pass, so a crushed liquid can't explode.
            (change / rest).clamp_length_max(h * 0.125)
        });
        for i in 0..n {
            if points[first + i].inverse_mass > 0.0 {
                points[first + i].position += self.change[i];
            }
        }
    }

    /// Blends each particle's speed towards its neighbours', which makes thick liquids ooze.
    pub fn viscosity(&mut self, points: &mut [Point]) {
        let n = self.count();
        if n == 0 || self.starts.len() != n + 1 {
            return;
        }
        let first = self.range.start as usize;
        let h = self.reach;
        let scale = self.stickiness / weight(0.0, h);
        let liquid = &points[first..first + n];
        let (starts, neighbours) = (&self.starts, &self.neighbours);
        each(&mut self.change, |i| {
            let p = &liquid[i];
            let mut change = Vec2::ZERO;
            for &j in &neighbours[starts[i] as usize..starts[i + 1] as usize] {
                let q = &liquid[j as usize];
                change += (q.velocity - p.velocity) * weight(p.position.distance_squared(q.position), h);
            }
            change * scale
        });
        for i in 0..n {
            points[first + i].velocity += self.change[i];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernels_vanish_at_their_reach() {
        assert_eq!(weight(16.0, 4.0), 0.0);
        assert!(weight(0.0, 4.0) > weight(4.0, 4.0));
        assert_eq!(slope(Vec2::new(4.0, 0.0), 4.0), Vec2::ZERO);
        assert_eq!(slope(Vec2::ZERO, 4.0), Vec2::ZERO);
        // The gradient points from the particle towards its neighbour (packing rises that way).
        assert!(slope(Vec2::new(1.0, 0.0), 4.0).x < 0.0);
    }
}
