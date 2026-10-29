//! The position-based fluid solver, ported from the old engine's `bpe-sim` liquid: each
//! particle is nudged until the particles around it are as tightly packed as at the start, never
//! more, which makes them flow, splash and settle into a level surface.

use std::collections::BTreeMap;
use std::f32::consts::PI;

use bpe_core::ThingId;
use bpe_runtime::{Bodies, Wall, keep_inside};
use glam::Vec2;

/// An axis-aligned region of the scene.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub min: Vec2,
    pub max: Vec2,
}

impl Rect {
    pub fn size(&self) -> Vec2 {
        self.max - self.min
    }

    pub fn area(&self) -> f32 {
        let s = self.size();
        (s.x * s.y).max(0.0)
    }

    pub fn center(&self) -> Vec2 {
        (self.min + self.max) * 0.5
    }
}

/// The height of a hexagonal lattice's rows, as a fraction of its spacing.
pub const ROW: f32 = 0.866_025_4;

/// Smoothing kernel: how much a neighbour at squared distance `r2` counts towards packing (2D
/// poly6).
pub fn weight(r2: f32, h: f32) -> f32 {
    let h2 = h * h;
    if r2 >= h2 {
        return 0.0;
    }
    let d = h2 - r2;
    4.0 / (PI * h2 * h2 * h2 * h2) * d * d * d
}

/// Gradient of the spiky kernel at offset `d` (from the neighbour to the particle).
pub fn slope(d: Vec2, h: f32) -> Vec2 {
    let r = d.length();
    if r >= h || r < 1e-6 {
        return Vec2::ZERO;
    }
    let k = -30.0 / (PI * h.powi(5)) * (h - r) * (h - r);
    d * (k / r)
}

/// Hexagonal rows from the bottom of `region` up, `spacing` apart, every other row shifted by
/// half a spacing, keeping the points `keep` accepts, until `count` are placed or the region
/// runs out.
pub fn lattice(region: &Rect, spacing: f32, keep: impl Fn(Vec2) -> bool, count: usize) -> Vec<Vec2> {
    let mut out = Vec::with_capacity(count);
    if spacing <= 0.0 || count == 0 {
        return out;
    }
    let mut row = 0u32;
    loop {
        let y = region.max.y - spacing * 0.5 - row as f32 * spacing * ROW;
        if y < region.min.y {
            break;
        }
        let shift = if row % 2 == 1 { spacing * 0.5 } else { 0.0 };
        let mut x = region.min.x + spacing * 0.5 + shift;
        while x <= region.max.x - spacing * 0.5 + 1e-3 {
            let p = Vec2::new(x, y);
            if keep(p) {
                out.push(p);
                if out.len() >= count {
                    return out;
                }
            }
            x += spacing;
        }
        row += 1;
    }
    out
}

/// The spacing of a hexagonal lattice that fits `count` particles into `area`.
pub fn spacing_for(count: u32, area: f32) -> f32 {
    (area.max(1.0) * ROW / count.max(1) as f32).sqrt()
}

/// A uniform grid of the particles, rebuilt every substep with a counting sort so the order
/// never depends on anything but the particle order.
#[derive(Clone, Debug, Default, PartialEq)]
struct Grid {
    cell: f32,
    origin: Vec2,
    columns: usize,
    rows: usize,
    /// `starts[c]..starts[c + 1]` indexes `entries` for cell `c`.
    starts: Vec<u32>,
    entries: Vec<u32>,
}

impl Grid {
    fn rebuild(&mut self, cell: f32, items: impl Iterator<Item = (u32, Vec2)> + Clone) {
        self.cell = cell.max(1e-3);
        let mut min = Vec2::splat(f32::MAX);
        let mut max = Vec2::splat(f32::MIN);
        let mut any = false;
        for (_, p) in items.clone() {
            min = min.min(p);
            max = max.max(p);
            any = true;
        }
        if !any {
            self.columns = 0;
            self.rows = 0;
            self.starts.clear();
            self.entries.clear();
            return;
        }
        self.origin = min - Vec2::splat(self.cell);
        let span = max - self.origin + Vec2::splat(self.cell);
        self.columns = ((span.x / self.cell).floor() as usize + 1).clamp(1, 1 << 14);
        self.rows = ((span.y / self.cell).floor() as usize + 1).clamp(1, 1 << 14);
        let cells = self.columns * self.rows;
        self.starts.clear();
        self.starts.resize(cells + 1, 0);
        for (_, p) in items.clone() {
            let c = self.index(p);
            self.starts[c + 1] += 1;
        }
        for c in 0..cells {
            self.starts[c + 1] += self.starts[c];
        }
        let total = self.starts[cells] as usize;
        self.entries.clear();
        self.entries.resize(total, 0);
        let mut fill = self.starts.clone();
        for (i, p) in items {
            let c = self.index(p);
            self.entries[fill[c] as usize] = i;
            fill[c] += 1;
        }
    }

    fn coords(&self, p: Vec2) -> (usize, usize) {
        let local = (p - self.origin) / self.cell;
        let x = (local.x.floor().max(0.0) as usize).min(self.columns.saturating_sub(1));
        let y = (local.y.floor().max(0.0) as usize).min(self.rows.saturating_sub(1));
        (x, y)
    }

    fn index(&self, p: Vec2) -> usize {
        let (x, y) = self.coords(p);
        y * self.columns + x
    }

    /// Every entry in the 3×3 cells around `p`, in cell order.
    fn around(&self, p: Vec2, mut f: impl FnMut(u32)) {
        if self.columns == 0 {
            return;
        }
        let (cx, cy) = self.coords(p);
        for y in cy.saturating_sub(1)..=(cy + 1).min(self.rows - 1) {
            for x in cx.saturating_sub(1)..=(cx + 1).min(self.columns - 1) {
                let c = y * self.columns + x;
                for &i in &self.entries[self.starts[c] as usize..self.starts[c + 1] as usize] {
                    f(i);
                }
            }
        }
    }
}

/// Scratch space the solver reuses between substeps.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scratch {
    grid: Grid,
    starts: Vec<u32>,
    neighbours: Vec<u32>,
    lambda: Vec<f32>,
    change: Vec<Vec2>,
    before: Vec<Vec2>,
    velocity_before: Vec<Vec2>,
    /// The wall normal that pushed each particle this substep, if one did.
    pushed: Vec<Option<Vec2>>,
}

/// The solver's constants for one liquid.
#[derive(Clone, Debug, PartialEq)]
pub struct Pbf {
    /// How far a particle feels its neighbours.
    pub h: f32,
    /// The particle radius.
    pub radius: f32,
    /// Packing at rest, and a small number that keeps the solver steady.
    rest: f32,
    relax: f32,
    /// Passes per substep.
    pub iterations: u32,
    /// How much each particle takes on its neighbours' speed per substep, 0 to 1.
    stickiness: f32,
}

impl Pbf {
    /// A solver for particles `spacing` apart at rest. `stiffness` and `viscosity` are 0 to 1.
    pub fn new(spacing: f32, stiffness: f32, viscosity: f32) -> Pbf {
        let radius = spacing * 0.5;
        let h = radius * 3.2;
        let (mut density, mut sum, mut squared) = (0.0, Vec2::ZERO, 0.0);
        for row in -4i32..=4 {
            for column in -4i32..=4 {
                let shift = if row.rem_euclid(2) == 1 { spacing * 0.5 } else { 0.0 };
                let p = Vec2::new(column as f32 * spacing + shift, row as f32 * spacing * ROW);
                density += weight(p.length_squared(), h);
                let g = slope(-p, h);
                sum += g;
                squared += g.length_squared();
            }
        }
        let rest = density.max(1e-12);
        let typical = (squared + sum.length_squared()) / (rest * rest);
        Pbf {
            h,
            radius,
            rest,
            relax: typical * 0.05 + 1e-12,
            iterations: 2 + (stiffness.clamp(0.0, 1.0) * 3.0).round() as u32,
            stickiness: 0.02 + 0.4 * viscosity.clamp(0.0, 1.0),
        }
    }

    /// One substep of settling: the particles are packed no tighter than at rest, pushed out of
    /// the `pushers` (balls: centre and radius) by `ball_push`, kept inside every wall, given the
    /// velocity of their moves (walls answer with `bounciness`), then blended with their
    /// neighbours' speeds.
    #[allow(clippy::too_many_arguments)]
    pub fn relax(
        &self,
        bodies: &mut Bodies,
        dt: f32,
        walls: &BTreeMap<ThingId, Wall>,
        pushers: &[(Vec2, f32)],
        ball_push: f32,
        bounciness: f32,
        s: &mut Scratch,
    ) {
        let n = bodies.len();
        if n == 0 || dt <= 0.0 {
            return;
        }
        let h = self.h;
        let reach = h * 1.1;
        let alive = |i: usize| bodies.alive[i];
        s.grid.rebuild(reach, (0..n as u32).filter(|&i| alive(i as usize)).map(|i| (i, bodies.position[i as usize])));

        // Neighbours within reach, once per substep.
        s.starts.clear();
        s.neighbours.clear();
        for i in 0..n {
            s.starts.push(s.neighbours.len() as u32);
            if !alive(i) {
                continue;
            }
            let p = bodies.position[i];
            let (grid, neighbours) = (&s.grid, &mut s.neighbours);
            grid.around(p, |j| {
                if j as usize != i && bodies.position[j as usize].distance_squared(p) < reach * reach {
                    neighbours.push(j);
                }
            });
        }
        s.starts.push(s.neighbours.len() as u32);
        s.lambda.clear();
        s.lambda.resize(n, 0.0);
        s.change.clear();
        s.change.resize(n, Vec2::ZERO);
        s.before.clear();
        s.before.extend_from_slice(&bodies.position);
        s.velocity_before.clear();
        s.velocity_before.extend_from_slice(&bodies.velocity);
        s.pushed.clear();
        s.pushed.resize(n, None);

        let (rest, relax) = (self.rest, self.relax);
        for _ in 0..self.iterations {
            let positions = &bodies.position;
            let around = |i: usize| &s.neighbours[s.starts[i] as usize..s.starts[i + 1] as usize];
            for i in 0..n {
                if !alive(i) {
                    continue;
                }
                let p = positions[i];
                let mut density = weight(0.0, h);
                let (mut sum, mut squared) = (Vec2::ZERO, 0.0);
                for &j in around(i) {
                    let d = p - positions[j as usize];
                    density += weight(d.length_squared(), h);
                    let g = slope(d, h) / rest;
                    sum += g;
                    squared += g.length_squared();
                }
                let pressure = density / rest - 1.0;
                // Only push apart: letting particles pull together makes them clump into strings.
                s.lambda[i] = if pressure > 0.0 { -pressure / (squared + sum.length_squared() + relax) } else { 0.0 };
            }
            for i in 0..n {
                if !alive(i) {
                    continue;
                }
                let p = positions[i];
                let mut change = Vec2::ZERO;
                for &j in around(i) {
                    let d = p - positions[j as usize];
                    change += slope(d, h) * (s.lambda[i] + s.lambda[j as usize]);
                }
                // Never more than an eighth of the reach per pass, so a crushed liquid can't explode.
                s.change[i] = (change / rest).clamp_length_max(h * 0.125);
            }
            for i in 0..n {
                if alive(i) {
                    bodies.position[i] += s.change[i];
                }
            }

            // Balls shove the particles aside, one way.
            if ball_push > 0.0 {
                let push = ball_push.min(1.0);
                for &(center, radius) in pushers {
                    let limit = radius + self.radius;
                    let (grid, positions) = (&s.grid, &mut bodies.position);
                    // Every cell the ball can touch.
                    let corners = [
                        center - Vec2::splat(limit),
                        center + Vec2::new(limit, -limit),
                        center + Vec2::new(-limit, limit),
                        center + Vec2::splat(limit),
                    ];
                    let mut seen = Vec::new();
                    for corner in corners {
                        grid.around(corner, |j| seen.push(j));
                    }
                    grid.around(center, |j| seen.push(j));
                    seen.sort_unstable();
                    seen.dedup();
                    for j in seen {
                        let j = j as usize;
                        let offset = positions[j] - center;
                        let distance = offset.length();
                        if distance < limit && distance > 1e-6 {
                            positions[j] += offset / distance * ((limit - distance) * push);
                        }
                    }
                }
            }

            // Kept inside every wall.
            for i in 0..n {
                if !alive(i) {
                    continue;
                }
                for wall in walls.values() {
                    let was = bodies.position[i];
                    keep_inside(&mut bodies.position[i], Vec2::ZERO, self.radius, wall);
                    let moved = bodies.position[i] - was;
                    if moved.length_squared() > 1e-10 {
                        s.pushed[i] = Some(moved.normalize());
                    }
                }
            }
        }

        // The moves become speed, and a wall's push bounces.
        for i in 0..n {
            if !alive(i) {
                continue;
            }
            let mut v = bodies.velocity[i] + (bodies.position[i] - s.before[i]) / dt;
            if let Some(normal) = s.pushed[i] {
                let into = (-s.velocity_before[i].dot(normal)).max(0.0);
                v -= normal * v.dot(normal).min(0.0);
                v += normal * into * bounciness;
            }
            bodies.velocity[i] = v;
        }

        // Each particle takes on a little of its neighbours' speed, which makes thick liquids ooze.
        let scale = self.stickiness / weight(0.0, h);
        for i in 0..n {
            if !alive(i) {
                continue;
            }
            let p = bodies.position[i];
            let v = bodies.velocity[i];
            let mut change = Vec2::ZERO;
            for &j in &s.neighbours[s.starts[i] as usize..s.starts[i + 1] as usize] {
                let j = j as usize;
                change += (bodies.velocity[j] - v) * weight(p.distance_squared(bodies.position[j]), h);
            }
            s.change[i] = change * scale;
        }
        for i in 0..n {
            if alive(i) {
                bodies.velocity[i] += s.change[i];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;

    #[test]
    fn kernels_vanish_at_their_reach() {
        assert_eq!(weight(16.0, 4.0), 0.0);
        assert!(weight(0.0, 4.0) > weight(4.0, 4.0));
        assert_eq!(slope(Vec2::new(4.0, 0.0), 4.0), Vec2::ZERO);
        assert_eq!(slope(Vec2::ZERO, 4.0), Vec2::ZERO);
        assert!(slope(Vec2::new(1.0, 0.0), 4.0).x < 0.0);
    }

    #[test]
    fn the_lattice_is_hexagonal_and_stops_at_count() {
        let region = Rect { min: Vec2::ZERO, max: Vec2::new(100.0, 100.0) };
        let points = lattice(&region, 10.0, |_| true, 25);
        assert_eq!(points.len(), 25);
        assert_eq!(points[0], Vec2::new(5.0, 95.0));
        assert_eq!(points[9], Vec2::new(95.0, 95.0));
        assert!((points[10] - Vec2::new(10.0, 95.0 - 8.660254)).length() < 1e-4, "{:?}", points[10]);
        let few = lattice(&region, 10.0, |p| p.x < 50.0, 1000);
        assert!(few.len() < 60 && few.iter().all(|p| p.x < 50.0), "{}", few.len());
    }

    #[test]
    fn a_relaxed_block_spreads_out_and_keeps_its_count() {
        // A block packed twice too tightly pushes itself apart.
        let spacing = 10.0;
        let pbf = Pbf::new(spacing, 0.5, 0.2);
        let mut bodies = bpe_runtime::Bodies::default();
        for p in lattice(&Rect { min: Vec2::ZERO, max: Vec2::new(50.0, 50.0) }, spacing * 0.5, |_| true, 100) {
            bodies.push(p, Vec2::ZERO, spacing * 0.5);
        }
        let before = bodies.position.clone();
        let mut scratch = Scratch::default();
        pbf.relax(&mut bodies, 1.0 / 240.0, &std::collections::BTreeMap::new(), &[], 1.0, 0.0, &mut scratch);
        let spread = |ps: &[Vec2]| ps.iter().fold(0.0f32, |m, p| m.max(p.distance(Vec2::new(25.0, 25.0))));
        assert!(spread(&bodies.position) > spread(&before));
        assert_eq!(bodies.len(), 100);
        assert!(bodies.position.iter().all(|p| p.is_finite()));
        assert!(bodies.velocity.iter().all(|v| v.length() > 0.0 && v.is_finite()), "velocities follow the moves");
    }
}
