//! The canvas grid: the simulation's coarse, exact copy of a paint canvas.
//!
//! The grid is a plain array of 8-byte cells, split into 32x32 tiles that are
//! shared between clones until written. Ball strokes and splats are stamped in
//! with 16.16 fixed-point integer maths, so the result is identical on every
//! machine. Splat shapes are stamped as a disc of the mark's radius; only the
//! picture draws the real shape.

use std::sync::Arc;

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

/// One cell of the canvas.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Cell {
    /// Paint colour.
    pub rgb: [u8; 3],
    /// How much paint is here; zero means untouched.
    pub amount: u8,
    /// How wet the paint is.
    pub wet: u8,
    /// Which team laid the paint.
    pub team: u8,
    /// Paint thickness.
    pub depth: u8,
    /// Spare bits for later features.
    pub flags: u8,
}

/// A grid of paint cells with copy-on-write tiles.
#[derive(Clone)]
pub struct CanvasGrid {
    cell_size: f32,
    width: u32,
    height: u32,
    tiles_x: u32,
    tiles: Vec<Arc<[Cell; TILE_CELLS]>>,
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
        CanvasGrid { cell_size, width, height, tiles_x, tiles: vec![blank; (tiles_x * tiles_y) as usize] }
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

    /// Paint a capsule: every cell whose centre lies within `radius` of the
    /// segment `from`..`to` takes `cell`'s colour and fields, and its amount
    /// becomes the larger of the old and new amounts.
    pub fn stamp_capsule(&mut self, from: Vec2, to: Vec2, radius: f32, cell: Cell) {
        let a = self.to_fixed(from);
        let b = self.to_fixed(to);
        let r = (radius.clamp(0.0, WORLD_LIMIT) / self.cell_size * ONE as f32) as i64;
        let r2 = r as i128 * r as i128;

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
                if ox * ox + oy * oy <= r2 {
                    self.paint(x as u32, y as u32, cell);
                }
            }
        }
    }

    /// Paint a disc of `radius` around `at`, as a zero-length capsule.
    pub fn stamp_disc(&mut self, at: Vec2, radius: f32, cell: Cell) {
        self.stamp_capsule(at, at, radius, cell);
    }

    /// The share of cells that have been painted (`amount > 0`), 0 to 1.
    pub fn covered(&self) -> f32 {
        let painted: usize = self.tiles.iter().map(|t| t.iter().filter(|c| c.amount > 0).count()).sum();
        // Padding cells past the edge are never painted, so count real cells.
        painted as f32 / (self.width as f32 * self.height as f32)
    }

    /// Memory the cells take, counting every tile as if unshared.
    pub fn bytes(&self) -> usize {
        self.tiles.len() * TILE_CELLS * std::mem::size_of::<Cell>()
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
                for b in [c.amount, c.wet, c.team, c.depth, c.flags] {
                    feed(b);
                }
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

    /// Write one stamp into a cell, un-sharing its tile if needed.
    fn paint(&mut self, x: u32, y: u32, new: Cell) {
        let (tile, slot) = self.locate(x, y);
        let old = self.tiles[tile][slot];
        let mut next = new;
        next.amount = old.amount.max(new.amount);
        if next != old {
            Arc::make_mut(&mut self.tiles[tile])[slot] = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paint() -> Cell {
        Cell { rgb: [200, 30, 40], amount: 255, ..Cell::default() }
    }

    /// The cell containing a world point.
    fn at(g: &CanvasGrid, x: f32, y: f32) -> Cell {
        let (w, h) = g.dims();
        let cx = (x / g.cell_size() + w as f32 / 2.0).floor() as u32;
        let cy = (y / g.cell_size() + h as f32 / 2.0).floor() as u32;
        g.get(cx, cy)
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
        g.stamp_capsule(Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0), 10.0, paint());
        assert!(at(&g, 0.0, 0.0).amount > 0);
        assert!(at(&g, 90.0, 0.0).amount > 0);
        assert!(at(&g, -90.0, 0.0).amount > 0);
        assert_eq!(at(&g, 0.0, 20.0).amount, 0);
        assert_eq!(at(&g, 120.0, 0.0).amount, 0);
        assert_eq!(at(&g, 0.0, 0.0).rgb, [200, 30, 40]);
    }

    #[test]
    fn fast_stroke_is_continuous() {
        let mut g = CanvasGrid::new(Vec2::new(2000.0, 200.0));
        g.stamp_capsule(Vec2::new(-500.0, 0.0), Vec2::new(500.0, 0.0), 4.0, paint());
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
        g.stamp_disc(Vec2::new(5000.0, 5000.0), 10.0, paint());
        g.stamp_capsule(Vec2::new(-9e9, 0.0), Vec2::new(9e9, 1e12), 10.0, Cell::default());
        assert_eq!(g.hash(), before);
        assert_eq!(g.covered(), 0.0);
    }

    #[test]
    fn clone_shares_unchanged_tiles() {
        let mut a = CanvasGrid::new(Vec2::new(400.0, 400.0));
        a.stamp_disc(Vec2::new(-150.0, -150.0), 10.0, paint());
        let mut b = a.clone();
        b.stamp_disc(Vec2::new(100.0, 100.0), 4.0, paint());
        let differing = a.tiles.iter().zip(&b.tiles).filter(|(x, y)| !Arc::ptr_eq(x, y)).count();
        assert_eq!(differing, 1);
    }

    #[test]
    fn same_stamps_same_hash() {
        let run = || {
            let mut g = CanvasGrid::new(Vec2::new(400.0, 400.0));
            g.stamp_capsule(Vec2::new(-50.0, 20.0), Vec2::new(70.0, -30.0), 9.0, paint());
            g.stamp_disc(Vec2::new(10.0, 10.0), 15.0, Cell { amount: 40, ..paint() });
            g
        };
        let (a, b) = (run(), run());
        assert_eq!(a.hash(), b.hash());
        assert_ne!(a.hash(), CanvasGrid::new(Vec2::new(400.0, 400.0)).hash());
        assert!(a.covered() > 0.0 && a.bytes() > 0);
    }
}
