//! Steam fog: wiped edges sometimes let a drop run down the glass, clearing a thin track.

use bpe_core::ThingId;
use bpe_runtime::{Mark, MarkShape, MarkTarget};
use glam::Vec2;

use super::grid::CanvasGrid;

/// The most drops running on one canvas.
pub const MAX_DROPS: usize = 64;

/// A drop running down the glass.
#[derive(Clone, Debug, PartialEq)]
pub struct FogDrop {
    /// Canvas pixels from the canvas's middle.
    pub at: Vec2,
    /// Pixels of run left.
    pub left: f32,
    /// Pixels a second.
    pub speed: f32,
}

/// The low 16 bits of `x` as 0..=1.
fn frac(x: u64) -> f32 {
    (x & 0xffff) as f32 / 65535.0
}

/// Starts drops from the `edges` noted this tick (a point and its hash seed) with probability
/// `chance` each, moves every drop `per_second`-th of a second down `gravity` (canvas frame), and
/// wipes its path out of `grid`. A drop ends when its run is used up or it leaves the canvas
/// (`half` is half its size).
pub fn tick(
    grid: &mut CanvasGrid,
    drops: &mut Vec<FogDrop>,
    edges: &[(Vec2, u64)],
    chance: f32,
    gravity: Vec2,
    per_second: f32,
    half: Vec2,
) {
    for &(at, seed) in edges {
        let u = (seed >> 40) as f32 / (1u64 << 24) as f32;
        if u < chance && drops.len() < MAX_DROPS {
            drops.push(FogDrop { at, left: 60.0 + 140.0 * frac(seed >> 8), speed: 40.0 + 60.0 * frac(seed >> 16) });
        }
    }
    let down = gravity.normalize_or_zero();
    if down == Vec2::ZERO {
        return;
    }
    let radius = 2.5_f32.max(grid.cell_size() * 0.5);
    for d in drops.iter_mut() {
        let step = d.speed / per_second;
        let from = d.at;
        let to = from + down * step;
        let mark = Mark {
            step: 0,
            target: MarkTarget::Canvas(ThingId::ROOT),
            shape: MarkShape::Scrape,
            from,
            to,
            radius,
            radius_to: radius,
            angle: 0.0,
            within: [1.0, 1.0],
            variant: 0,
            colour: [0.0, 0.0, 0.0, 1.0],
            colour_to: [0.0, 0.0, 0.0, 1.0],
        };
        grid.scrape(&mark, 0.0);
        d.at = to;
        d.left -= step;
    }
    drops.retain(|d| d.left > 0.0 && d.at.x.abs() <= half.x && d.at.y.abs() <= half.y);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::things::canvas::grid::REST_FOG;

    #[test]
    fn a_drop_wipes_a_track_and_ends() {
        let mut g = CanvasGrid::surface(Vec2::splat(400.0), REST_FOG, 4.0, 300_000);
        let mut drops = vec![FogDrop { at: Vec2::new(0.0, -100.0), left: 30.0, speed: 60.0 }];
        for _ in 0..30 {
            tick(&mut g, &mut drops, &[], 0.0, Vec2::new(0.0, 500.0), 30.0, Vec2::splat(200.0));
        }
        assert!(drops.is_empty());
        let (x, y) = g.cell_at(Vec2::new(0.0, -85.0)).unwrap();
        assert_eq!(g.get(x, y).amount, 0);
        let (x, y) = g.cell_at(Vec2::new(60.0, -85.0)).unwrap();
        assert_eq!(g.get(x, y).amount, 255);
    }

    #[test]
    fn drops_are_capped() {
        let mut g = CanvasGrid::surface(Vec2::splat(400.0), REST_FOG, 4.0, 300_000);
        let mut drops = Vec::new();
        let edges: Vec<(Vec2, u64)> = (0..200).map(|k| (Vec2::new(k as f32 - 100.0, 0.0), 0)).collect();
        tick(&mut g, &mut drops, &edges, 1.0, Vec2::new(0.0, 500.0), 30.0, Vec2::splat(200.0));
        assert_eq!(drops.len(), MAX_DROPS);
    }
}
