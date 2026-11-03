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

/// What a cover, powder or fog canvas's cells hold when nothing has touched them.
pub const REST_COVER: u8 = 255;
pub const REST_POWDER: u8 = 160;
pub const REST_FOG: u8 = 255;

/// The size of a grid, copied out so cells can be visited while the grid is written.
#[derive(Clone, Copy)]
struct Geom {
    cell_size: f32,
    width: u32,
    height: u32,
}

impl Geom {
    /// World position to 16.16 cell units, with the grid centred on the origin.
    fn to_fixed(self, p: Vec2) -> (i64, i64) {
        let p = p.clamp(Vec2::splat(-WORLD_LIMIT), Vec2::splat(WORLD_LIMIT));
        let cs = self.cell_size as f64;
        let conv = |v: f32, n: u32| ((v as f64 / cs + n as f64 / 2.0) * ONE as f64).round() as i64;
        (conv(p.x, self.width), conv(p.y, self.height))
    }
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
    /// What an untouched cell's amount is: 0 for paint, more for the surfaces balls remove.
    rest: u8,
    /// Per tile, how many real cells have an amount other than `rest`. Not hashed.
    tile_off: Vec<u16>,
    /// Per tile, the sum over real cells of how far the amount is below `rest`. Not hashed.
    tile_deficit: Vec<u32>,
    off_total: u64,
    deficit_total: u64,
}

impl CanvasGrid {
    /// An empty grid covering `size` world pixels, centred on the origin.
    /// Cells start at 4 px and double until there are at most 300,000.
    pub fn new(size: Vec2) -> Self {
        Self::surface(size, 0, START_CELL_SIZE, MAX_CELLS)
    }

    /// A grid covering `size` world pixels whose untouched cells hold `rest`. Cells start at
    /// `start_cell` px and double until there are at most `max_cells`.
    pub fn surface(size: Vec2, rest: u8, start_cell: f32, max_cells: u64) -> Self {
        let size = size.max(Vec2::ONE);
        let mut cell_size = start_cell;
        let dims = |cs: f32| ((size.x / cs).ceil().max(1.0) as u64, (size.y / cs).ceil().max(1.0) as u64);
        loop {
            let (w, h) = dims(cell_size);
            if w * h <= max_cells {
                break;
            }
            cell_size *= 2.0;
        }
        let (w, h) = dims(cell_size);
        let (width, height) = (w as u32, h as u32);
        let tiles_x = width.div_ceil(TILE);
        let tiles_y = height.div_ceil(TILE);
        let blank = Arc::new([Cell { amount: rest, ..Cell::default() }; TILE_CELLS]);
        let count = (tiles_x * tiles_y) as usize;
        CanvasGrid {
            cell_size,
            width,
            height,
            tiles_x,
            tiles: vec![blank; count],
            tile_tick: vec![NEVER; count],
            rest,
            tile_off: vec![0; count],
            tile_deficit: vec![0; count],
            off_total: 0,
            deficit_total: 0,
        }
    }

    /// What an untouched cell holds.
    pub fn rest(&self) -> u8 {
        self.rest
    }

    /// Cells that are really on the canvas (not the padding of the last tiles).
    pub fn real_cells(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    /// Cells whose amount is not `rest`.
    pub fn off_cells(&self) -> u64 {
        self.off_total
    }

    /// The sum over cells of how far the amount is below `rest`.
    pub fn deficit(&self) -> u64 {
        self.deficit_total
    }

    /// How much of a cover, powder or fog has been taken away, 0 to 1; 0 for paint.
    pub fn revealed(&self) -> f32 {
        if self.rest == 0 {
            return 0.0;
        }
        self.deficit_total as f32 / (self.rest as f32 * self.real_cells() as f32)
    }

    /// Paint: the share of cells painted. Other surfaces: the share still there.
    pub fn covered(&self) -> f32 {
        if self.rest == 0 { self.off_total as f32 / self.real_cells() as f32 } else { 1.0 - self.revealed() }
    }

    /// The amounts as an image: RGBA8, one pixel per real cell, row by row, `[amount, 0, 0, 255]`.
    pub fn image(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.real_cells() as usize * 4);
        for y in 0..self.height {
            for x in 0..self.width {
                out.extend_from_slice(&[self.get(x, y).amount, 0, 0, 255]);
            }
        }
        out
    }

    fn geom(&self) -> Geom {
        Geom { cell_size: self.cell_size, width: self.width, height: self.height }
    }

    /// Writes `new` into `slot` of `tile`, keeping the tile's and the grid's counters, and only
    /// un-sharing the tile when the cell changes.
    fn write(&mut self, tile: usize, slot: usize, new: Cell) {
        let old = self.tiles[tile][slot];
        if old == new {
            return;
        }
        let rest = self.rest;
        let off = |c: Cell| (c.amount != rest) as i64;
        let short = |c: Cell| (rest as i64 - c.amount as i64).max(0);
        let d_off = off(new) - off(old);
        let d_def = short(new) - short(old);
        self.tile_off[tile] = (self.tile_off[tile] as i64 + d_off) as u16;
        self.tile_deficit[tile] = (self.tile_deficit[tile] as i64 + d_def) as u32;
        self.off_total = (self.off_total as i64 + d_off) as u64;
        self.deficit_total = (self.deficit_total as i64 + d_def) as u64;
        Arc::make_mut(&mut self.tiles[tile])[slot] = new;
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
        let drip = m.shape == MarkShape::Drip;
        let mut cells = Vec::new();
        Self::capsule_cells(self.geom(), m, to, radius, radius, &mut |x, y, _| cells.push((x, y)));
        for (x, y) in cells {
            self.paint(x, y, colour, alpha, ink, tick, by, drip);
        }
    }

    /// Visits every grid cell whose centre is within `reach` (canvas px) of the segment
    /// m.from → to, with the ribbon and drip cuts of `m` applied. Passes the cell and whether it
    /// is also inside `radius`.
    fn capsule_cells(g: Geom, m: &Mark, to: Vec2, radius: f32, reach: f32, f: &mut dyn FnMut(u32, u32, bool)) {
        let a = g.to_fixed(m.from);
        let b = g.to_fixed(to);
        let to_cells = |v: f32| (v.clamp(0.0, WORLD_LIMIT) / g.cell_size * ONE as f32) as i64;
        let r = to_cells(radius);
        let r2 = r as i128 * r as i128;
        let reach_r = to_cells(reach).max(r);
        let reach2 = reach_r as i128 * reach_r as i128;
        // The ribbon cuts, in fixed point as the shader does them.
        let ribbon = matches!(m.shape, MarkShape::Stroke | MarkShape::Smear | MarkShape::Scrape);
        let joined = m.shape == MarkShape::Drip && m.variant & DRIP_JOINED != 0;
        let before =
            ((m.angle.cos() as f64 * ONE as f64).round() as i128, (m.angle.sin() as f64 * ONE as f64).round() as i128);
        let cut_before = (ribbon && m.variant & 2 != 0) || joined;
        let cut_after = ribbon && m.variant & 4 != 0;
        let start_r = to_cells(m.radius);
        let start_r2 = start_r as i128 * start_r as i128;

        // Bounding box of cells, clipped to the grid.
        let lo = |v: i64| (v - reach_r).div_euclid(ONE);
        let hi = |v: i64| (v + reach_r).div_euclid(ONE);
        let x0 = lo(a.0.min(b.0)).max(0);
        let x1 = hi(a.0.max(b.0)).min(g.width as i64 - 1);
        let y0 = lo(a.1.min(b.1)).max(0);
        let y1 = hi(a.1.max(b.1)).min(g.height as i64 - 1);
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
                let dist2 = ox * ox + oy * oy;
                if dist2 > reach2 {
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
                f(x as u32, y as u32, dist2 <= r2);
            }
        }
    }

    /// Takes away what a Scrape or Chip mark `m` (canvas pixels) covers; other marks do nothing.
    /// `ridges` (powder only) piles a ridge of up to 35 % of the rest height beside the track.
    /// Colour, date, team and painter of a cell are left alone, and no tile wakes.
    pub fn scrape(&mut self, m: &Mark, ridges: f32) {
        let s = (m.colour[3].clamp(0.0, 1.0) * 255.0).round() as u32;
        if s == 0 {
            return;
        }
        let material = self.rest > 0;
        let take = ((s * self.rest as u32 + 127) / 255) as u8;
        let ridge = if self.rest == REST_POWDER {
            (ridges.clamp(0.0, 1.0) * s as f32 / 255.0 * self.rest as f32 * 0.35).round() as u8
        } else {
            0
        };
        let wipe = |c: Cell, inside: bool| -> Cell {
            let amount = if inside {
                if material { c.amount.saturating_sub(take) } else { (c.amount as u32 * (255 - s) / 255) as u8 }
            } else {
                c.amount.saturating_add(ridge)
            };
            Cell { amount, ..c }
        };
        let mut cells: Vec<(u32, u32, bool)> = Vec::new();
        match m.shape {
            MarkShape::Scrape => {
                let radius = (m.radius + m.radius_to) * 0.5;
                let reach = if ridge > 0 { radius * 1.6 } else { radius };
                Self::capsule_cells(self.geom(), m, m.to, radius, reach, &mut |x, y, inside| {
                    cells.push((x, y, inside))
                });
            }
            MarkShape::Chip => {
                let r = m.radius.clamp(0.0, WORLD_LIMIT);
                let (c, sn) = (m.angle.cos(), m.angle.sin());
                let shape = ((m.variant >> 5) & 7) as u32;
                let reach = 2.4 * r;
                let lo = self.cell_at_clamped(m.from - Vec2::splat(reach));
                let hi = self.cell_at_clamped(m.from + Vec2::splat(reach));
                for y in lo.1..=hi.1 {
                    for x in lo.0..=hi.0 {
                        let o = self.centre_of(x, y) - m.from;
                        let q = Vec2::new(c * o.x + sn * o.y, -sn * o.x + c * o.y);
                        let d = bpe_gfx::chip_distance(shape, q, r, (m.variant & 31) as u32);
                        if d <= 0.0 {
                            cells.push((x, y, true));
                        } else if d <= 0.6 * r {
                            cells.push((x, y, false));
                        }
                    }
                }
            }
            _ => return,
        }
        for (x, y, inside) in cells {
            if !inside && ridge == 0 {
                continue;
            }
            let (tile, slot) = self.locate(x, y);
            let old = self.tiles[tile][slot];
            self.write(tile, slot, wipe(old, inside));
        }
    }

    /// The cell holding canvas point `p`, pulled onto the grid.
    fn cell_at_clamped(&self, p: Vec2) -> (u32, u32) {
        let p = p.clamp(Vec2::splat(-WORLD_LIMIT), Vec2::splat(WORLD_LIMIT));
        let x = (p.x / self.cell_size + self.width as f32 / 2.0).floor().clamp(0.0, self.width as f32 - 1.0);
        let y = (p.y / self.cell_size + self.height as f32 / 2.0).floor().clamp(0.0, self.height as f32 - 1.0);
        (x as u32, y as u32)
    }

    /// Moves every changed cell toward `rest` by at most `step_amount`, up or down.
    pub fn grow_back(&mut self, step_amount: u8) {
        if step_amount == 0 {
            return;
        }
        for t in 0..self.tiles.len() {
            if self.tile_off[t] == 0 {
                continue;
            }
            let (x0, x1, y0, y1) = self.tile_cells(t);
            for y in y0..y1 {
                for x in x0..x1 {
                    let (tile, slot) = self.locate(x, y);
                    let old = self.tiles[tile][slot];
                    if old.amount == self.rest {
                        continue;
                    }
                    let amount = if old.amount < self.rest {
                        old.amount.saturating_add(step_amount).min(self.rest)
                    } else {
                        old.amount.saturating_sub(step_amount).max(self.rest)
                    };
                    self.write(tile, slot, Cell { amount, ..old });
                }
            }
        }
    }

    /// Replaces a cell, writing (and so un-sharing the tile) only when it changes. `dated`: the
    /// full tick the new paint was painted at, so its tile wakes up; None keeps the tile's. The
    /// tile's tick becomes `dated` when the tile was NEVER painted or `dated` is newer.
    pub fn set(&mut self, x: u32, y: u32, cell: Cell, dated: Option<u32>) {
        let (tile, slot) = self.locate(x, y);
        self.write(tile, slot, cell);
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

    /// Memory the cells take, counting every tile as if unshared.
    pub fn bytes(&self) -> usize {
        self.tiles.len() * TILE_CELLS * std::mem::size_of::<Cell>()
            + self.tile_tick.len() * 4
            + self.tile_off.len() * 2
            + self.tile_deficit.len() * 4
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
        self.write(tile, slot, next);
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

    /// The stamps of the capsule and splat tests, one grid.
    fn paint_baseline() -> CanvasGrid {
        let mut g = CanvasGrid::new(Vec2::new(400.0, 400.0));
        g.stamp(&mark(MarkShape::Stroke, Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0), 10.0), 1, 1);
        g.stamp(&mark(MarkShape::Splat, Vec2::new(10.0, 10.0), Vec2::new(10.0, 10.0), 15.0), 2, 2);
        let one = Mark { variant: 4, ..mark(MarkShape::Stroke, Vec2::new(-40.0, 60.0), Vec2::new(0.0, 60.0), 8.0) };
        let two = Mark { variant: 2, ..mark(MarkShape::Stroke, Vec2::new(0.0, 60.0), Vec2::new(40.0, 80.0), 8.0) };
        g.stamp(&one, 3, 3);
        g.stamp(&two, 3, 3);
        g.stamp(&mark(MarkShape::Drip, Vec2::new(0.0, -20.0), Vec2::new(0.0, 60.0), 3.0), 4, 0);
        g.stamp(&mark(MarkShape::Picture, Vec2::new(-80.0, -80.0), Vec2::new(-80.0, -80.0), 40.0), 5, 4);
        g
    }

    #[test]
    fn paint_grid_is_unchanged_by_the_rest_value() {
        let g = paint_baseline();
        assert_eq!(g.hash(), 8373683792163834270, "baseline");
    }

    fn scrape_mark(from: Vec2, to: Vec2, radius: f32, strength: f32) -> Mark {
        Mark {
            colour: [0.0, 0.0, 0.0, strength],
            colour_to: [0.0, 0.0, 0.0, strength],
            ..mark(MarkShape::Scrape, from, to, radius)
        }
    }

    fn chip_mark(at: Vec2, radius: f32, variant: u8, strength: f32) -> Mark {
        Mark {
            variant,
            colour: [0.0, 0.0, 0.0, strength],
            colour_to: [0.0, 0.0, 0.0, strength],
            ..mark(MarkShape::Chip, at, at, radius)
        }
    }

    fn cover(size: f32) -> CanvasGrid {
        CanvasGrid::surface(Vec2::splat(size), REST_COVER, 2.0, 600_000)
    }

    #[test]
    fn surface_starts_at_rest() {
        let g = cover(400.0);
        assert_eq!(g.cell_size(), 2.0);
        assert_eq!(at(&g, 0.0, 0.0).amount, 255);
        assert_eq!(at(&g, 150.0, -150.0).amount, 255);
        assert_eq!((g.off_cells(), g.deficit()), (0, 0));
        assert_eq!((g.revealed(), g.covered()), (0.0, 1.0));
    }

    #[test]
    fn surface_cell_size_doubles_past_its_cap() {
        let g = CanvasGrid::surface(Vec2::new(1080.0, 1920.0), 255, 2.0, 600_000);
        assert_eq!((g.cell_size(), g.dims()), (2.0, (540, 960)));
        assert_eq!(CanvasGrid::surface(Vec2::new(1080.0, 4000.0), 255, 2.0, 600_000).cell_size(), 4.0);
    }

    #[test]
    fn scrape_takes_cover_away() {
        let mut g = cover(400.0);
        g.scrape(&scrape_mark(Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0), 10.0, 1.0), 0.0);
        assert_eq!(at(&g, 0.0, 0.0).amount, 0);
        assert_eq!(at(&g, 90.0, 0.0).amount, 0);
        assert_eq!(at(&g, 0.0, 20.0).amount, 255);
        let changed = g.off_cells();
        assert!(changed > 0 && g.revealed() > 0.0);
        let want = (changed * 255) as f32 / (255.0 * g.real_cells() as f32);
        assert!((g.revealed() - want).abs() < 1e-6);
        assert!((g.covered() + g.revealed() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn half_strength_takes_half() {
        let mut g = cover(400.0);
        let m = scrape_mark(Vec2::new(-50.0, 0.0), Vec2::new(50.0, 0.0), 10.0, 0.5);
        g.scrape(&m, 0.0);
        assert_eq!(at(&g, 0.0, 0.0).amount, 127);
        g.scrape(&m, 0.0);
        assert_eq!(at(&g, 0.0, 0.0).amount, 0);
    }

    #[test]
    fn scrape_wipes_paint() {
        let mut g = CanvasGrid::new(Vec2::new(400.0, 400.0));
        let red = mark(MarkShape::Splat, Vec2::ZERO, Vec2::ZERO, 30.0);
        g.stamp(&red, 0, 1);
        g.stamp(&red, 0, 1);
        g.stamp(&mark(MarkShape::Stroke, Vec2::new(-5.0, 0.0), Vec2::new(5.0, 0.0), 8.0), 0, 1);
        assert_eq!(at(&g, 0.0, 0.0).amount, 255);
        let before = at(&g, 0.0, 0.0);
        g.scrape(&scrape_mark(Vec2::new(-50.0, 0.0), Vec2::new(50.0, 0.0), 10.0, 0.5), 0.0);
        let c = at(&g, 0.0, 0.0);
        assert_eq!(c.amount as u32, 255 * 127 / 255);
        assert_eq!((c.rgb, c.painted, c.by), (before.rgb, before.painted, before.by));
        g.scrape(&scrape_mark(Vec2::new(-50.0, 0.0), Vec2::new(50.0, 0.0), 10.0, 1.0), 0.0);
        assert_eq!(at(&g, 0.0, 0.0).amount, 0);
    }

    #[test]
    fn ridges_rise_beside_the_track() {
        let powder = || CanvasGrid::surface(Vec2::splat(400.0), REST_POWDER, 4.0, 300_000);
        let mut g = powder();
        g.scrape(&scrape_mark(Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0), 20.0, 1.0), 1.0);
        assert_eq!(at(&g, 0.0, 0.0).amount, 0);
        assert_eq!(at(&g, 0.0, 26.0).amount, 216);
        assert_eq!(at(&g, 0.0, 40.0).amount, 160);
        let mut flat = powder();
        flat.scrape(&scrape_mark(Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0), 20.0, 1.0), 0.0);
        assert_eq!(at(&flat, 0.0, 26.0).amount, 160);
        // A ridge never passes 255, even when scraped over again.
        for _ in 0..10 {
            g.scrape(&scrape_mark(Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0), 20.0, 1.0), 1.0);
        }
        assert_eq!(at(&g, 0.0, 26.0).amount, 255);
        // A cover has no ridges.
        let mut c = cover(400.0);
        c.scrape(&scrape_mark(Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0), 20.0, 1.0), 1.0);
        assert_eq!(at(&c, 0.0, 26.0).amount, 255);
    }

    #[test]
    fn ridges_respect_the_ribbon_cut() {
        let mut g = CanvasGrid::surface(Vec2::splat(400.0), REST_POWDER, 4.0, 300_000);
        let m = Mark { variant: 2, angle: 0.0, ..scrape_mark(Vec2::ZERO, Vec2::new(100.0, 0.0), 20.0, 1.0) };
        g.scrape(&m, 1.0);
        assert_eq!(at(&g, -26.0, 0.0).amount, 160);
        assert_eq!(at(&g, 50.0, 26.0).amount, 216);
    }

    #[test]
    fn chip_covers_its_shape() {
        let mut g = cover(400.0);
        g.scrape(&chip_mark(Vec2::ZERO, 40.0, 1 << 5, 1.0), 0.0);
        assert_eq!(at(&g, 34.0, 0.0).amount, 0);
        assert_eq!(at(&g, 0.0, 0.0).amount, 255);
    }

    #[test]
    fn grow_back_returns_to_rest() {
        let mut g = cover(400.0);
        g.scrape(&scrape_mark(Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0), 10.0, 1.0), 0.0);
        let step = (255.0f32 / (2.0 * 30.0)).ceil() as u8;
        for _ in 0..60 {
            g.grow_back(step);
        }
        assert_eq!((g.off_cells(), g.deficit()), (0, 0));
        assert_eq!(at(&g, 0.0, 0.0).amount, 255);
        let mut p = CanvasGrid::surface(Vec2::splat(400.0), REST_POWDER, 4.0, 300_000);
        p.scrape(&scrape_mark(Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0), 20.0, 1.0), 1.0);
        assert_eq!(at(&p, 0.0, 26.0).amount, 216);
        for _ in 0..60 {
            p.grow_back(10);
        }
        assert_eq!(at(&p, 0.0, 26.0).amount, 160);
        assert_eq!((p.off_cells(), p.deficit()), (0, 0));
        g.grow_back(0);
    }

    #[test]
    fn counters_match_a_full_scan() {
        let mut g = CanvasGrid::surface(Vec2::splat(300.0), REST_POWDER, 4.0, 300_000);
        let unit = |k: u64, i: u64| (mix(k, i) >> 40) as f32 / (1u64 << 24) as f32;
        let check = |g: &CanvasGrid, what: &str| {
            let (w, h) = g.dims();
            let (mut off, mut def) = (0u64, 0u64);
            for y in 0..h {
                for x in 0..w {
                    let a = g.get(x, y).amount;
                    off += (a != g.rest()) as u64;
                    def += g.rest().saturating_sub(a) as u64;
                }
            }
            assert_eq!((g.off_cells(), g.deficit()), (off, def), "{what}");
        };
        for k in 0..500u64 {
            let at = Vec2::new(unit(k, 0) * 300.0 - 150.0, unit(k, 1) * 300.0 - 150.0);
            let r = 2.0 + unit(k, 2) * 38.0;
            let strength = unit(k, 3);
            if k % 2 == 0 {
                let to = at + Vec2::new(unit(k, 4) * 80.0 - 40.0, unit(k, 5) * 80.0 - 40.0);
                g.scrape(&scrape_mark(at, to, r, strength), 0.7);
            } else {
                g.scrape(&chip_mark(at, r, ((k % 6) as u8) << 5 | (k % 8) as u8, strength), 0.7);
            }
            if k % 25 == 0 {
                g.grow_back(9);
            }
            check(&g, &format!("op {k}"));
        }
    }

    /// A SplitMix-style mix of two numbers, for test positions.
    fn mix(a: u64, b: u64) -> u64 {
        let mut z =
            a.wrapping_mul(0x9e37_79b9_7f4a_7c15).wrapping_add(b.wrapping_mul(0xbf58_476d_1ce4_e5b9)).wrapping_add(1);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    #[test]
    fn image_has_one_pixel_per_cell() {
        let mut g = cover(100.0);
        g.scrape(&scrape_mark(Vec2::new(-5.0, 0.0), Vec2::new(5.0, 0.0), 3.0, 1.0), 0.0);
        let img = g.image();
        let (w, h) = g.dims();
        assert_eq!(img.len(), (w * h * 4) as usize);
        let (cx, cy) = g.cell_at(Vec2::ZERO).unwrap();
        let i = ((cy * w + cx) * 4) as usize;
        assert_eq!(&img[i..i + 4], &[0, 0, 0, 255]);
        assert_eq!(&img[0..4], &[255, 0, 0, 255]);
    }
}
