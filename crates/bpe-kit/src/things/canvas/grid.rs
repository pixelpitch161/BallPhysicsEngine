//! The canvas grid: the simulation's coarse, exact copy of a paint canvas.
//!
//! The grid is a plain array of 8-byte cells, split into 32x32 tiles that are
//! shared between clones until written. Ball strokes and splats are stamped in
//! with 16.16 fixed-point integer maths, so the result is identical on every
//! machine. Splat shapes are stamped as a disc of the mark's radius; only the
//! picture draws the real shape.

use std::sync::Arc;

use bpe_core::{Color, ThingId};
use bpe_runtime::{Mark, MarkShape};
use glam::Vec2;

/// Cells along one side of a tile.
const TILE: u32 = 32;
/// Cells in one tile.
const TILE_CELLS: usize = (TILE * TILE) as usize;
/// The cell size a grid starts from, in world pixels.
const START_CELL_SIZE: f32 = 4.0;
/// The most cells a grid may have before the cell size doubles.
const MAX_CELLS: u64 = 300_000;
/// One cell unit in 16.16 fixed point.
const ONE: i64 = 1 << 16;
/// World coordinates are clamped to this before conversion, so absurd inputs
/// cannot overflow the integer maths.
const WORLD_LIMIT: f32 = 1.0e7;

/// One cell of the canvas: 8 bytes.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Cell {
    /// Paint colour, sRGB.
    pub rgb: [u8; 3],
    /// How much paint is here: its thickness. 0 means untouched.
    pub amount: u8,
    /// The 30 Hz tick it was last painted at, wrapping.
    pub painted: u16,
    /// Which team laid it (part 4's Claim); 0 for now.
    pub team: u8,
    /// Which ball laid it: [`painter`] of its thing and row; 0 for drips and bleed.
    pub by: u8,
}

/// Sentinel: a tile that was never painted.
pub const NEVER: u32 = u32::MAX;
/// A Drip mark's variant: leave out the start disc and the half-plane behind the start.
pub const DRIP_JOINED: u8 = 8;
/// Ticks a second the wet-paint tick runs at.
pub const TICK_HZ: u32 = 30;

/// World steps between ticks.
pub fn tick_every(steps_per_second: u32) -> u64 {
    (steps_per_second / TICK_HZ).max(1) as u64
}

/// The number a ball's cells carry: 1..=255, from its thing and row.
pub fn painter(thing: ThingId, item: u32) -> u8 {
    ((thing.0 as u64 * 31 + item as u64) % 255) as u8 + 1
}

/// The ink a mark of `shape` adds to a cell (and, over 255, to the GPU's thickness).
pub fn ink_of(shape: MarkShape) -> u8 {
    match shape {
        MarkShape::Stroke => 96,
        MarkShape::Smear => 160,
        MarkShape::Splat
        | MarkShape::Ring
        | MarkShape::Star
        | MarkShape::Crack
        | MarkShape::Burst
        | MarkShape::Dots => 200,
        MarkShape::Picture => 96,
        MarkShape::Drip => 40,
        MarkShape::Lift | MarkShape::Bleed | MarkShape::Scrape | MarkShape::Chip => 0,
    }
}

/// The most ticks paint can stay wet.
const MAX_DRY_TICKS: u32 = 60_000;

/// Whether `c` is wet at full tick `now`: painted and `(now as u16).wrapping_sub(c.painted) <
/// dry_ticks` (dry_ticks clamped to 60_000; 0 is never wet).
pub fn wet(c: &Cell, now: u32, dry_ticks: u32) -> bool {
    c.amount > 0 && ((now as u16).wrapping_sub(c.painted) as u32) < dry_ticks.min(MAX_DRY_TICKS)
}

/// A grid of paint cells with copy-on-write tiles.
#[derive(Clone)]
pub struct CanvasGrid {
    cell_size: f32,
    width: u32,
    height: u32,
    tiles_x: u32,
    tiles: Vec<Arc<[Cell; TILE_CELLS]>>,
    /// Per tile, the full tick it was last painted at, or [`NEVER`]. Not hashed.
    tile_tick: Vec<u32>,
}

impl CanvasGrid {
    /// An empty grid covering `size` world pixels, centred on the origin.
    /// Cells start at 4 px and double until there are at most 300,000.
    pub fn new(size: Vec2) -> Self {
        let size = size.max(Vec2::ONE);
        let mut cell_size = START_CELL_SIZE;
        let dims = |cs: f32| ((size.x / cs).ceil().max(1.0) as u64, (size.y / cs).ceil().max(1.0) as u64);
        loop {
            let (w, h) = dims(cell_size);
            if w * h <= MAX_CELLS {
                break;
            }
            cell_size *= 2.0;
        }
        let (w, h) = dims(cell_size);
        let (width, height) = (w as u32, h as u32);
        let tiles_x = width.div_ceil(TILE);
        let tiles_y = height.div_ceil(TILE);
        let blank = Arc::new([Cell::default(); TILE_CELLS]);
        let count = (tiles_x * tiles_y) as usize;
        CanvasGrid { cell_size, width, height, tiles_x, tiles: vec![blank; count], tile_tick: vec![NEVER; count] }
    }

    /// World pixels along one side of a cell.
    pub fn cell_size(&self) -> f32 {
        self.cell_size
    }

    /// Grid size in cells, width then height.
    pub fn dims(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The cell at column `x`, row `y`. Panics if out of range.
    pub fn get(&self, x: u32, y: u32) -> Cell {
        assert!(x < self.width && y < self.height, "cell out of range");
        let (tile, slot) = self.locate(x, y);
        self.tiles[tile][slot]
    }

    /// The grid cell whose square holds canvas point `p`, or None off the grid.
    pub fn cell_at(&self, p: Vec2) -> Option<(u32, u32)> {
        if !(p.x.is_finite() && p.y.is_finite()) {
            return None;
        }
        let x = (p.x / self.cell_size + self.width as f32 / 2.0).floor();
        let y = (p.y / self.cell_size + self.height as f32 / 2.0).floor();
        (x >= 0.0 && y >= 0.0 && x < self.width as f32 && y < self.height as f32).then_some((x as u32, y as u32))
    }

    /// The canvas point at the middle of cell (x, y).
    pub fn centre_of(&self, x: u32, y: u32) -> Vec2 {
        Vec2::new(
            (x as f32 + 0.5 - self.width as f32 / 2.0) * self.cell_size,
            (y as f32 + 0.5 - self.height as f32 / 2.0) * self.cell_size,
        )
    }

    /// Stamps mark `m` (canvas pixels) as fresh paint painted at `tick`, by `by`, with the ink
    /// of its shape. Stroke/Smear: a capsule of radius (radius + radius_to)/2, with the ribbon
    /// cuts from `variant` bits 1 and 2 and `angle`. Drip: a capsule of radius
    /// max((radius + radius_to)/2, cell_size/2), cut by DRIP_JOINED. Splat, Ring, Star, Crack,
    /// Burst, Dots: a disc of `radius`. Picture: a disc of 0.7 × radius. Lift and Bleed: nothing
    /// (they are not paint).
    pub fn stamp(&mut self, m: &Mark, tick: u32, by: u8) {
        let ink = ink_of(m.shape);
        let (to, radius) = match m.shape {
            MarkShape::Stroke | MarkShape::Smear => (m.to, (m.radius + m.radius_to) * 0.5),
            MarkShape::Drip => (m.to, ((m.radius + m.radius_to) * 0.5).max(self.cell_size * 0.5)),
            MarkShape::Splat
            | MarkShape::Ring
            | MarkShape::Star
            | MarkShape::Crack
            | MarkShape::Burst
            | MarkShape::Dots => (m.from, m.radius),
            MarkShape::Picture => (m.from, m.radius * 0.7),
            MarkShape::Lift | MarkShape::Bleed | MarkShape::Scrape | MarkShape::Chip => return,
        };
        let colour = Color::from_linear(m.colour);
        let alpha = colour.a as i32;
        let a = self.to_fixed(m.from);
        let b = self.to_fixed(to);
        let r = (radius.clamp(0.0, WORLD_LIMIT) / self.cell_size * ONE as f32) as i64;
        let r2 = r as i128 * r as i128;
        // The ribbon cuts, in fixed point as the shader does them.
        let ribbon = matches!(m.shape, MarkShape::Stroke | MarkShape::Smear);
        let joined = m.shape == MarkShape::Drip && m.variant & DRIP_JOINED != 0;
        let before =
            ((m.angle.cos() as f64 * ONE as f64).round() as i128, (m.angle.sin() as f64 * ONE as f64).round() as i128);
        let cut_before = (ribbon && m.variant & 2 != 0) || joined;
        let cut_after = ribbon && m.variant & 4 != 0;
        let start_r = (m.radius.clamp(0.0, WORLD_LIMIT) / self.cell_size * ONE as f32) as i64;
        let start_r2 = start_r as i128 * start_r as i128;

        // Bounding box of cells, clipped to the grid.
        let lo = |v: i64| (v - r).div_euclid(ONE);
        let hi = |v: i64| (v + r).div_euclid(ONE);
        let x0 = lo(a.0.min(b.0)).max(0);
        let x1 = hi(a.0.max(b.0)).min(self.width as i64 - 1);
        let y0 = lo(a.1.min(b.1)).max(0);
        let y1 = hi(a.1.max(b.1)).min(self.height as i64 - 1);
        if x0 > x1 || y0 > y1 {
            return;
        }

        let d = (b.0 as i128 - a.0 as i128, b.1 as i128 - a.1 as i128);
        let dd = d.0 * d.0 + d.1 * d.1;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let p = ((x * ONE + ONE / 2) as i128 - a.0 as i128, (y * ONE + ONE / 2) as i128 - a.1 as i128);
                let dot = p.0 * d.0 + p.1 * d.1;
                // Offset from the nearest point of the segment to the centre.
                let (ox, oy) = if dd == 0 || dot <= 0 {
                    p
                } else if dot >= dd {
                    (p.0 - d.0, p.1 - d.1)
                } else {
                    (p.0 - d.0 * dot / dd, p.1 - d.1 * dot / dd)
                };
                if ox * ox + oy * oy > r2 {
                    continue;
                }
                if cut_before && p.0 * before.0 + p.1 * before.1 < 0 {
                    continue;
                }
                if joined && p.0 * p.0 + p.1 * p.1 < start_r2 {
                    continue;
                }
                if cut_after && dd > 0 && dot >= dd {
                    continue;
                }
                self.paint(x as u32, y as u32, colour, alpha, ink, tick, by, m.shape == MarkShape::Drip);
            }
        }
    }

    /// Replaces a cell, writing (and so un-sharing the tile) only when it changes. `dated`: the
    /// full tick the new paint was painted at, so its tile wakes up; None keeps the tile's. The
    /// tile's tick becomes `dated` when the tile was NEVER painted or `dated` is newer.
    pub fn set(&mut self, x: u32, y: u32, cell: Cell, dated: Option<u32>) {
        let (tile, slot) = self.locate(x, y);
        if self.tiles[tile][slot] != cell {
            Arc::make_mut(&mut self.tiles[tile])[slot] = cell;
        }
        if let Some(tick) = dated {
            self.wake(tile, tick);
        }
    }

    /// Tiles across and down.
    pub fn tile_dims(&self) -> (u32, u32) {
        (self.tiles_x, self.height.div_ceil(TILE))
    }

    /// The cells of tile `t`: x0..x1, y0..y1 (exclusive ends, clipped to the grid).
    pub fn tile_cells(&self, t: usize) -> (u32, u32, u32, u32) {
        let (tx, ty) = (t as u32 % self.tiles_x, t as u32 / self.tiles_x);
        let (x0, y0) = (tx * TILE, ty * TILE);
        (x0, (x0 + TILE).min(self.width), y0, (y0 + TILE).min(self.height))
    }

    /// Tiles with paint still wet at full tick `now`: `tile_tick != NEVER &&
    /// now.wrapping_sub(tile_tick) < dry_ticks`, in index order.
    pub fn wet_tiles(&self, now: u32, dry_ticks: u32) -> Vec<usize> {
        let dry = dry_ticks.min(MAX_DRY_TICKS);
        self.tile_tick
            .iter()
            .enumerate()
            .filter(|(_, t)| **t != NEVER && now.wrapping_sub(**t) < dry)
            .map(|(i, _)| i)
            .collect()
    }

    /// The share of cells that have been painted (`amount > 0`), 0 to 1.
    pub fn covered(&self) -> f32 {
        let painted: usize = self.tiles.iter().map(|t| t.iter().filter(|c| c.amount > 0).count()).sum();
        // Padding cells past the edge are never painted, so count real cells.
        painted as f32 / (self.width as f32 * self.height as f32)
    }

    /// Memory the cells take, counting every tile as if unshared.
    pub fn bytes(&self) -> usize {
        self.tiles.len() * TILE_CELLS * std::mem::size_of::<Cell>() + self.tile_tick.len() * 4
    }

    /// A 64-bit FNV-1a hash of the grid size and every cell.
    pub fn hash(&self) -> u64 {
        const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
        const PRIME: u64 = 0x0000_0100_0000_01b3;
        let mut h = OFFSET;
        let mut feed = |byte: u8| {
            h ^= byte as u64;
            h = h.wrapping_mul(PRIME);
        };
        for b in self.width.to_le_bytes().into_iter().chain(self.height.to_le_bytes()) {
            feed(b);
        }
        for tile in &self.tiles {
            for c in tile.iter() {
                for b in c.rgb {
                    feed(b);
                }
                feed(c.amount);
                for b in c.painted.to_le_bytes() {
                    feed(b);
                }
                feed(c.team);
                feed(c.by);
            }
        }
        h
    }

    /// Tile index and slot within the tile for a cell.
    fn locate(&self, x: u32, y: u32) -> (usize, usize) {
        let tile = (y / TILE) * self.tiles_x + x / TILE;
        let slot = (y % TILE) * TILE + x % TILE;
        (tile as usize, slot as usize)
    }

    /// World position to 16.16 cell units, with the grid centred on the origin.
    fn to_fixed(&self, p: Vec2) -> (i64, i64) {
        let p = p.clamp(Vec2::splat(-WORLD_LIMIT), Vec2::splat(WORLD_LIMIT));
        let cs = self.cell_size as f64;
        let conv = |v: f32, n: u32| ((v as f64 / cs + n as f64 / 2.0) * ONE as f64).round() as i64;
        (conv(p.x, self.width), conv(p.y, self.height))
    }

    /// Marks `tile` as painted at full tick `tick` when that is news.
    fn wake(&mut self, tile: usize, tick: u32) {
        let old = self.tile_tick[tile];
        if old == NEVER || tick.wrapping_sub(old) < 1 << 31 {
            self.tile_tick[tile] = tick;
        }
    }

    /// Lays fresh paint on a cell: new paint covers old by its alpha, ink adds up. A drip running
    /// over paint that is already there (`keep_date`) leaves its date alone, so a drip never
    /// makes the paint it runs from wet again: paint dries on schedule and dripping stops.
    #[allow(clippy::too_many_arguments)]
    fn paint(&mut self, x: u32, y: u32, colour: Color, alpha: i32, ink: u8, tick: u32, by: u8, keep_date: bool) {
        let (tile, slot) = self.locate(x, y);
        let old = self.tiles[tile][slot];
        let new = [colour.r, colour.g, colour.b];
        let rgb = if old.amount == 0 {
            new
        } else {
            std::array::from_fn(|k| {
                let (o, n) = (old.rgb[k] as i32, new[k] as i32);
                (o + ((n - o) * alpha + 127).div_euclid(255)) as u8
            })
        };
        let keep = keep_date && old.amount > 0;
        let painted = if keep { old.painted } else { tick as u16 };
        let next = Cell { rgb, amount: old.amount.saturating_add(ink), painted, team: old.team, by };
        if next != old {
            Arc::make_mut(&mut self.tiles[tile])[slot] = next;
        }
        if !keep {
            self.wake(tile, tick);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_runtime::MarkTarget;
    use std::f32::consts::FRAC_PI_2;

    /// An opaque-red mark of `shape`, the fields a grid reads.
    fn mark(shape: MarkShape, from: Vec2, to: Vec2, radius: f32) -> Mark {
        Mark {
            step: 0,
            target: MarkTarget::Canvas(ThingId(1)),
            shape,
            from,
            to,
            radius,
            radius_to: radius,
            angle: 0.0,
            within: [1.0, 1.0],
            variant: 0,
            colour: [1.0, 0.1, 0.1, 1.0],
            colour_to: [1.0, 0.1, 0.1, 1.0],
        }
    }

    fn colour(m: Mark, c: [f32; 4]) -> Mark {
        Mark { colour: c, colour_to: c, ..m }
    }

    /// The cell containing a world point.
    fn at(g: &CanvasGrid, x: f32, y: f32) -> Cell {
        let (cx, cy) = g.cell_at(Vec2::new(x, y)).unwrap();
        g.get(cx, cy)
    }

    #[test]
    fn cell_is_eight_bytes() {
        assert_eq!(std::mem::size_of::<Cell>(), 8);
    }

    #[test]
    fn cell_size_doubles_past_the_cap() {
        let g = CanvasGrid::new(Vec2::new(1080.0, 1920.0));
        assert_eq!(g.cell_size(), 4.0);
        assert_eq!(g.dims(), (270, 480));
        // 1080x10000 at 4 px is 675,000 cells, at 8 px 168,750.
        assert_eq!(CanvasGrid::new(Vec2::new(1080.0, 10000.0)).cell_size(), 8.0);
        // 1080x20000 is still 337,500 cells at 8 px, so it needs 16.
        assert_eq!(CanvasGrid::new(Vec2::new(1080.0, 20000.0)).cell_size(), 16.0);
    }

    #[test]
    fn capsule_covers_its_cells() {
        let mut g = CanvasGrid::new(Vec2::new(400.0, 400.0));
        g.stamp(&mark(MarkShape::Stroke, Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0), 10.0), 0, 1);
        assert!(at(&g, 0.0, 0.0).amount > 0);
        assert!(at(&g, 90.0, 0.0).amount > 0);
        assert!(at(&g, -90.0, 0.0).amount > 0);
        assert_eq!(at(&g, 0.0, 20.0).amount, 0);
        assert_eq!(at(&g, 120.0, 0.0).amount, 0);
        let c = Color::from_linear([1.0, 0.1, 0.1, 1.0]);
        assert_eq!(at(&g, 0.0, 0.0).rgb, [c.r, c.g, c.b]);
    }

    #[test]
    fn fast_stroke_is_continuous() {
        let mut g = CanvasGrid::new(Vec2::new(2000.0, 200.0));
        g.stamp(&mark(MarkShape::Stroke, Vec2::new(-500.0, 0.0), Vec2::new(500.0, 0.0), 4.0), 0, 1);
        let (w, h) = g.dims();
        let row = (h as f32 / 2.0).floor() as u32;
        for x in 0..w {
            let centre = (x as f32 + 0.5 - w as f32 / 2.0) * g.cell_size();
            if (-500.0..=500.0).contains(&centre) {
                assert!(g.get(x, row).amount > 0, "gap at cell {x}");
            }
        }
    }

    #[test]
    fn stamps_outside_are_clipped() {
        let mut g = CanvasGrid::new(Vec2::new(400.0, 400.0));
        let before = g.hash();
        g.stamp(&mark(MarkShape::Splat, Vec2::new(5000.0, 5000.0), Vec2::new(5000.0, 5000.0), 10.0), 0, 1);
        g.stamp(&mark(MarkShape::Stroke, Vec2::new(-9e9, 0.0), Vec2::new(9e9, 1e12), 10.0), 0, 1);
        assert_eq!(g.hash(), before);
        assert_eq!(g.covered(), 0.0);
    }

    #[test]
    fn clone_shares_unchanged_tiles() {
        let mut a = CanvasGrid::new(Vec2::new(400.0, 400.0));
        let splat = |x: f32, y: f32, r: f32| mark(MarkShape::Splat, Vec2::new(x, y), Vec2::new(x, y), r);
        a.stamp(&splat(-150.0, -150.0, 10.0), 0, 1);
        let mut b = a.clone();
        b.stamp(&splat(100.0, 100.0, 4.0), 0, 1);
        let differing = a.tiles.iter().zip(&b.tiles).filter(|(x, y)| !Arc::ptr_eq(x, y)).count();
        assert_eq!(differing, 1);
    }

    #[test]
    fn same_stamps_same_hash() {
        let run = || {
            let mut g = CanvasGrid::new(Vec2::new(400.0, 400.0));
            g.stamp(&mark(MarkShape::Stroke, Vec2::new(-50.0, 20.0), Vec2::new(70.0, -30.0), 9.0), 3, 1);
            g.stamp(&mark(MarkShape::Splat, Vec2::new(10.0, 10.0), Vec2::new(10.0, 10.0), 15.0), 4, 2);
            g
        };
        let (a, b) = (run(), run());
        assert_eq!(a.hash(), b.hash());
        assert_ne!(a.hash(), CanvasGrid::new(Vec2::new(400.0, 400.0)).hash());
        assert!(a.covered() > 0.0 && a.bytes() > 0);
    }

    #[test]
    fn ink_adds_up_and_saturates() {
        let splat = mark(MarkShape::Splat, Vec2::ZERO, Vec2::ZERO, 20.0);
        let mut g = CanvasGrid::new(Vec2::new(400.0, 400.0));
        g.stamp(&splat, 0, 1);
        g.stamp(&splat, 0, 1);
        assert_eq!(at(&g, 0.0, 0.0).amount, 255);
        let mut g = CanvasGrid::new(Vec2::new(400.0, 400.0));
        g.stamp(&mark(MarkShape::Stroke, Vec2::new(-60.0, 0.0), Vec2::new(60.0, 0.0), 8.0), 0, 1);
        assert_eq!(at(&g, 0.0, 0.0).amount, 96);
        g.stamp(&mark(MarkShape::Stroke, Vec2::new(0.0, -60.0), Vec2::new(0.0, 60.0), 8.0), 0, 1);
        assert_eq!(at(&g, 0.0, 0.0).amount, 192);
        assert_eq!(at(&g, 40.0, 0.0).amount, 96);
    }

    #[test]
    fn ribbon_joints_are_not_counted_twice() {
        let mut g = CanvasGrid::new(Vec2::new(400.0, 400.0));
        let one = Mark { variant: 4, ..mark(MarkShape::Stroke, Vec2::new(-40.0, 0.0), Vec2::ZERO, 8.0) };
        let two = Mark { variant: 2, angle: 0.0, ..mark(MarkShape::Stroke, Vec2::ZERO, Vec2::new(40.0, 0.0), 8.0) };
        g.stamp(&one, 0, 1);
        g.stamp(&two, 0, 1);
        let mut x = -36.0;
        while x <= 36.0 {
            assert_eq!(at(&g, x, 0.0).amount, 96, "x = {x}");
            x += 4.0;
        }
        assert_eq!(at(&g, 0.0, 0.0).amount, 96);
    }

    #[test]
    fn drip_join_skips_the_start_disc() {
        let mut g = CanvasGrid::new(Vec2::new(400.0, 400.0));
        let one = mark(MarkShape::Drip, Vec2::new(0.0, -20.0), Vec2::ZERO, 3.0);
        let two = Mark {
            variant: DRIP_JOINED,
            angle: FRAC_PI_2,
            ..mark(MarkShape::Drip, Vec2::ZERO, Vec2::new(0.0, 20.0), 3.0)
        };
        g.stamp(&one, 0, 0);
        g.stamp(&two, 0, 0);
        assert_eq!(at(&g, 0.0, 0.0).amount, 40);
        assert_eq!(at(&g, 0.0, 12.0).amount, 40);
    }

    #[test]
    fn new_paint_covers_by_alpha() {
        let mut g = CanvasGrid::new(Vec2::new(400.0, 400.0));
        let red = colour(mark(MarkShape::Splat, Vec2::ZERO, Vec2::ZERO, 20.0), [1.0, 0.0, 0.0, 1.0]);
        let blue = colour(mark(MarkShape::Splat, Vec2::ZERO, Vec2::ZERO, 20.0), [0.0, 0.0, 1.0, 0.5]);
        g.stamp(&red, 0, 1);
        g.stamp(&blue, 0, 1);
        let rgb = at(&g, 0.0, 0.0).rgb;
        for (got, want) in rgb.iter().zip([128i32, 0, 127]) {
            assert!((*got as i32 - want).abs() <= 1, "{rgb:?}");
        }
    }

    #[test]
    fn cells_remember_when_and_who() {
        let mut g = CanvasGrid::new(Vec2::new(400.0, 400.0));
        g.stamp(&mark(MarkShape::Splat, Vec2::ZERO, Vec2::ZERO, 20.0), 70_000, 9);
        let c = at(&g, 0.0, 0.0);
        assert_eq!((c.painted, c.by), ((70_000 % 65_536) as u16, 9));
    }

    #[test]
    fn wet_follows_the_dry_ticks() {
        let c = Cell { amount: 96, painted: 10, ..Cell::default() };
        assert!(wet(&c, 39, 30));
        assert!(!wet(&c, 40, 30));
        let wrapped = Cell { amount: 96, painted: 65_530, ..Cell::default() };
        assert!(wet(&wrapped, 65_536 + 5, 30));
        assert!(!wet(&c, 10, 0));
        assert!(!wet(&Cell { amount: 0, painted: 10, ..Cell::default() }, 11, 30));
    }

    #[test]
    fn tiles_know_when_they_were_painted() {
        let mut g = CanvasGrid::new(Vec2::new(400.0, 400.0));
        assert!(g.wet_tiles(0, 30).is_empty());
        g.stamp(&mark(MarkShape::Splat, Vec2::new(-150.0, -150.0), Vec2::new(-150.0, -150.0), 10.0), 100, 1);
        assert_eq!(g.wet_tiles(110, 30).len(), 1);
        assert!(g.wet_tiles(130, 30).is_empty());
    }

    #[test]
    fn picture_marks_stamp_a_smaller_disc() {
        let mut g = CanvasGrid::new(Vec2::new(400.0, 400.0));
        g.stamp(&mark(MarkShape::Picture, Vec2::ZERO, Vec2::ZERO, 40.0), 0, 1);
        assert!(at(&g, 24.0, 0.0).amount > 0);
        assert_eq!(at(&g, 32.0, 0.0).amount, 0);
    }

    #[test]
    fn lift_and_bleed_paint_nothing() {
        let mut g = CanvasGrid::new(Vec2::new(400.0, 400.0));
        let before = g.hash();
        g.stamp(&mark(MarkShape::Lift, Vec2::ZERO, Vec2::ZERO, 30.0), 0, 1);
        g.stamp(&mark(MarkShape::Bleed, Vec2::new(-50.0, -50.0), Vec2::new(50.0, 50.0), 2.0), 0, 1);
        assert_eq!(g.hash(), before);
        assert!(g.wet_tiles(0, 30).is_empty());
    }

    #[test]
    fn a_drip_never_makes_old_paint_wet_again() {
        let mut g = CanvasGrid::new(Vec2::new(400.0, 400.0));
        g.stamp(&mark(MarkShape::Splat, Vec2::ZERO, Vec2::ZERO, 20.0), 5, 1);
        let drip = mark(MarkShape::Drip, Vec2::new(0.0, -10.0), Vec2::new(0.0, 60.0), 3.0);
        g.stamp(&drip, 50, 0);
        // Inside the old paint: dated as it was. Beyond it: fresh.
        assert_eq!(at(&g, 0.0, 0.0).painted, 5);
        assert_eq!(at(&g, 0.0, 40.0).painted, 50);
    }
}
