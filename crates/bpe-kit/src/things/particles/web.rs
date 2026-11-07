//! Web lines' links (particles part 5): which particles are joined to which, and to which balls.
//! Pure and worked out afresh every frame from the places drawn, so scrubbing and export agree.

use std::collections::HashMap;

use glam::Vec2;

use super::meets::BallAt;

/// The most other particles one particle looks at for its links.
pub const MOST_LOOKED: usize = 128;
/// The most links one particle starts.
pub const MOST_LINKS: usize = 32;
/// The most lines one layer draws.
pub const MOST_WEB_LINES: usize = 200_000;

/// A uniform grid of indices by cell.
struct Grid {
    cell: f32,
    cells: HashMap<(i32, i32), Vec<usize>>,
}

impl Grid {
    fn new(cell: f32) -> Self {
        Self { cell: cell.max(1.0), cells: HashMap::new() }
    }

    fn key(&self, p: Vec2) -> (i32, i32) {
        let k = |v: f32| (v / self.cell).floor().clamp(i32::MIN as f32 / 2.0, i32::MAX as f32 / 2.0) as i32;
        (k(p.x), k(p.y))
    }

    /// Puts `i` in every cell the box round `p` reaching `r` touches.
    fn add(&mut self, i: usize, p: Vec2, r: f32) {
        let (a, b) = (self.key(p - r), self.key(p + r));
        // A huge reach covers so many cells it would be slow: such a ball is looked at from
        // the cells round its middle only.
        let (a, b) = if (b.0 - a.0) as i64 * (b.1 - a.1) as i64 > 64 { (self.key(p), self.key(p)) } else { (a, b) };
        for y in a.1..=b.1 {
            for x in a.0..=b.0 {
                self.cells.entry((x, y)).or_default().push(i);
            }
        }
    }

    /// The indices in the 3 × 3 cells round `p`, cell by cell, in the order they were added.
    fn round(&self, p: Vec2) -> impl Iterator<Item = usize> + '_ {
        let (cx, cy) = self.key(p);
        (-1..=1).flat_map(move |dy| {
            (-1..=1).flat_map(move |dx| self.cells.get(&(cx + dx, cy + dy)).into_iter().flatten().copied())
        })
    }
}

/// The lines between `points` within `reach` of each other: each point keeps its `links`
/// nearest (at most [`MOST_LINKS`], looking at most at [`MOST_LOOKED`] within reach; ties by
/// index), and a pair either kept is drawn once, as `(i, j, distance)` with `i < j`. At most
/// [`MOST_WEB_LINES`].
pub fn links(points: &[Vec2], reach: f32, links: usize) -> Vec<(usize, usize, f32)> {
    let most = links.clamp(1, MOST_LINKS);
    if points.len() < 2 || reach.is_nan() || reach <= 0.0 {
        return Vec::new();
    }
    let mut grid = Grid::new(reach);
    for (i, p) in points.iter().enumerate() {
        if p.is_finite() {
            grid.cells.entry(grid.key(*p)).or_default().push(i);
        }
    }
    let reach2 = reach * reach;
    let kept: Vec<Vec<(f32, usize)>> = points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            if !p.is_finite() {
                return Vec::new();
            }
            let mut near: Vec<(f32, usize)> = grid
                .round(*p)
                .filter(|&j| j != i)
                .filter_map(|j| {
                    let d2 = p.distance_squared(points[j]);
                    (d2 <= reach2).then_some((d2, j))
                })
                .take(MOST_LOOKED)
                .collect();
            near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            near.truncate(most);
            near
        })
        .collect();
    let mut out = Vec::new();
    'all: for (i, near) in kept.iter().enumerate() {
        for &(d2, j) in near {
            if i < j || !kept[j].iter().any(|&(_, k)| k == i) {
                out.push((i.min(j), i.max(j), d2.sqrt()));
                if out.len() >= MOST_WEB_LINES {
                    break 'all;
                }
            }
        }
    }
    out
}

/// The lines from `points` to the edges of `balls` within `reach` of them, as `(point, the
/// edge point, how far from the edge)`, point by point and ball by ball. At most `most`.
pub fn ball_links(points: &[Vec2], balls: &[BallAt], reach: f32, most: usize) -> Vec<(usize, Vec2, f32)> {
    if balls.is_empty() || points.is_empty() || reach.is_nan() || reach <= 0.0 {
        return Vec::new();
    }
    let biggest = balls.iter().map(|b| b.radius).fold(0.0f32, f32::max);
    let mut grid = Grid::new(reach + biggest);
    for (i, b) in balls.iter().enumerate() {
        grid.add(i, b.place, b.radius + reach);
    }
    let mut out = Vec::new();
    for (i, p) in points.iter().enumerate() {
        if !p.is_finite() {
            continue;
        }
        let mut seen = Vec::new();
        for k in grid.round(*p) {
            if seen.contains(&k) {
                continue;
            }
            seen.push(k);
            let b = &balls[k];
            let off = *p - b.place;
            let d = off.length();
            let gap = d - b.radius;
            if gap < reach && gap > 0.0 && d > 1e-4 {
                out.push((i, b.place + off / d * b.radius, gap));
                if out.len() >= most {
                    return out;
                }
            }
        }
    }
    out
}
