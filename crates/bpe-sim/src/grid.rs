//! Spatial lookups that let a ball skip pegs and walls that are nowhere near it.
//!
//! They only ever leave out things a ball cannot touch, and they hand back what's left in the
//! same order as a full search would, so the simulation gives exactly the same result.

use bpe_scene::{ObstaclePart, Vec2};

/// Below this many pegs and wall pieces, checking all of them is quicker than looking them up.
const MIN_OBSTACLES: usize = 12;
/// Extra room around every box, in pixels, so rounding can never leave out a real touch.
const MARGIN: f32 = 1.0;

/// Pegs and wall pieces sorted into square cells by the area each one covers.
#[derive(Clone, Default)]
pub struct ObstacleGrid {
    cell: f32,
    origin: Vec2,
    columns: i32,
    rows: i32,
    /// Obstacle indices in each cell, lowest first. Cell (x, y) is at `y * columns + x`.
    cells: Vec<Vec<u32>>,
    /// Each obstacle's box, grown by its own radius or half thickness.
    boxes: Vec<(Vec2, Vec2)>,
    /// Off when there are too few obstacles for a lookup to pay off.
    enabled: bool,
}

impl ObstacleGrid {
    /// `shape_reach` gives how far each scene shape reaches from its middle, in its own units.
    pub fn new(obstacles: &[ObstaclePart], shape_reach: impl Fn(u32) -> f32) -> Self {
        if obstacles.len() < MIN_OBSTACLES {
            return Self::default();
        }
        let boxes: Vec<(Vec2, Vec2)> = obstacles
            .iter()
            .map(|part| match *part {
                ObstaclePart::Disc { center, radius, .. } => (center - radius, center + radius),
                ObstaclePart::Segment { from, to, thickness, .. } => {
                    let half = thickness.max(0.0) * 0.5;
                    (from.min(to) - half, from.max(to) + half)
                }
                ObstaclePart::Shape { shape, place, thickness, .. } => {
                    let reach = place.size.abs() * shape_reach(shape) + thickness.max(0.0) * 0.5;
                    (place.center - reach, place.center + reach)
                }
            })
            .collect();
        if boxes.iter().any(|(low, high)| !low.is_finite() || !high.is_finite()) {
            return Self::default();
        }
        let low = boxes.iter().fold(Vec2::splat(f32::MAX), |m, b| m.min(b.0)) - MARGIN;
        let high = boxes.iter().fold(Vec2::splat(f32::MIN), |m, b| m.max(b.1)) + MARGIN;
        // Cells about the size of a typical obstacle's spacing, but never more than 256 x 256.
        let area = (high - low).max(Vec2::ONE);
        let cell = (area.x * area.y / obstacles.len() as f32).sqrt().max(area.max_element() / 256.0).max(4.0);
        let columns = (area.x / cell).ceil() as i32 + 1;
        let rows = (area.y / cell).ceil() as i32 + 1;
        let mut grid = Self {
            cell,
            origin: low,
            columns,
            rows,
            cells: vec![Vec::new(); (columns * rows) as usize],
            boxes,
            enabled: true,
        };
        for (index, &(low, high)) in grid.boxes.iter().enumerate() {
            let (x0, y0, x1, y1) = grid.span(low - MARGIN, high + MARGIN);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    grid.cells[(y * columns + x) as usize].push(index as u32);
                }
            }
        }
        grid
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// The cells a box covers, clamped to the grid.
    fn span(&self, low: Vec2, high: Vec2) -> (i32, i32, i32, i32) {
        let cell = |v: f32, last: i32| ((v / self.cell).floor().clamp(-1.0, last as f32 + 1.0) as i32).clamp(0, last);
        let (a, b) = (low - self.origin, high - self.origin);
        (cell(a.x, self.columns - 1), cell(a.y, self.rows - 1), cell(b.x, self.columns - 1), cell(b.y, self.rows - 1))
    }

    /// Fills `out` with every obstacle after `after` (or from the first, if None) that a ball of
    /// `radius` moving from `from` to `to` could touch, lowest index first.
    pub fn query(&self, from: Vec2, to: Vec2, radius: f32, after: Option<u32>, out: &mut Vec<u32>) {
        out.clear();
        let reach = radius.max(0.0) + MARGIN;
        let (low, high) = (from.min(to) - reach, from.max(to) + reach);
        if !low.is_finite() || !high.is_finite() {
            // Something has gone badly wrong with this ball: check everything, as a full search would.
            out.extend(after.map_or(0, |a| a + 1)..self.boxes.len() as u32);
            return;
        }
        let grid_high = self.origin + Vec2::new(self.columns as f32, self.rows as f32) * self.cell;
        if high.x < self.origin.x || high.y < self.origin.y || low.x > grid_high.x || low.y > grid_high.y {
            return;
        }
        let (x0, y0, x1, y1) = self.span(low, high);
        for y in y0..=y1 {
            for x in x0..=x1 {
                for &index in &self.cells[(y * self.columns + x) as usize] {
                    if after.is_some_and(|a| index <= a) {
                        continue;
                    }
                    let (b_low, b_high) = self.boxes[index as usize];
                    if b_high.x >= low.x && b_low.x <= high.x && b_high.y >= low.y && b_low.y <= high.y {
                        out.push(index);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
    }
}

/// Finds pairs of balls close enough to touch, visiting them in the same order as a grid of
/// cells sorted by position (x, then y), each cell's balls lowest index first.
#[derive(Clone, Default)]
pub struct BallGrid {
    /// (cell, ball) for every ball, sorted.
    keyed: Vec<((i32, i32), u32)>,
    /// Each occupied cell and where its balls start and end in `keyed`, sorted by cell.
    runs: Vec<((i32, i32), u32, u32)>,
    /// Open-addressing hash table of run numbers plus one (0 is empty), for finding a cell's run.
    table: Vec<u32>,
    /// When the balls are packed closely, a table with every cell's start in `keyed` instead,
    /// covering the cells from `low` spanning `size`, numbered x-major.
    starts: Vec<u32>,
    /// The other list `keyed` is sorted from and into, kept to reuse.
    spare: Vec<((i32, i32), u32)>,
    low: (i32, i32),
    size: (i32, i32),
}

/// Most cells in the packed-balls table, per ball.
const CELLS_PER_BALL: i64 = 32;
/// Fewer balls than this always use the hash table.
const MIN_TABLE_BALLS: usize = 256;

fn cell_hash((x, y): (i32, i32)) -> u64 {
    ((x as u32 as u64) << 32 | y as u32 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

impl BallGrid {
    /// Sorts balls at `positions` into cells of `cell` pixels.
    pub fn build(&mut self, positions: impl Iterator<Item = Vec2>, cell: f32) {
        let key = |p: Vec2| ((p.x / cell).floor() as i32, (p.y / cell).floor() as i32);
        self.keyed.clear();
        self.keyed.extend(positions.enumerate().map(|(i, p)| (key(p), i as u32)));
        self.size = (0, 0);
        let n = self.keyed.len();
        let (mut low, mut high) = ((i32::MAX, i32::MAX), (i32::MIN, i32::MIN));
        for &((x, y), _) in &self.keyed {
            low = (low.0.min(x), low.1.min(y));
            high = (high.0.max(x), high.1.max(y));
        }
        // A ring of empty cells on every side, so neighbours never fall off the table.
        let (width, height) = (high.0 as i64 - low.0 as i64 + 3, high.1 as i64 - low.1 as i64 + 3);
        if n >= MIN_TABLE_BALLS && width * height <= CELLS_PER_BALL * n as i64 + 4096 {
            // Packed closely: a counting sort into a table of every cell.
            self.low = (low.0 - 1, low.1 - 1);
            self.size = (width as i32, height as i32);
            self.starts.clear();
            self.starts.resize((width * height) as usize + 1, 0);
            for &(cell, _) in &self.keyed {
                let slot = self.slot(cell);
                self.starts[slot + 1] += 1;
            }
            for i in 1..self.starts.len() {
                self.starts[i] += self.starts[i - 1];
            }
            let mut fill = std::mem::take(&mut self.table);
            fill.clear();
            fill.extend_from_slice(&self.starts);
            // Sort into last step's spare list, so no step allocates once the lists have grown.
            let mut sorted = std::mem::take(&mut self.spare);
            sorted.clear();
            sorted.resize(n, ((0, 0), 0));
            for &(cell, i) in &self.keyed {
                let slot = self.slot(cell);
                sorted[fill[slot] as usize] = (cell, i);
                fill[slot] += 1;
            }
            self.spare = std::mem::replace(&mut self.keyed, sorted);
            self.table = fill;
        } else {
            self.keyed.sort_unstable();
        }
        self.runs.clear();
        let mut start = 0;
        while start < self.keyed.len() {
            let cell = self.keyed[start].0;
            let mut end = start + 1;
            while end < self.keyed.len() && self.keyed[end].0 == cell {
                end += 1;
            }
            self.runs.push((cell, start as u32, end as u32));
            start = end;
        }
        if self.size != (0, 0) {
            return;
        }
        let size = (self.runs.len() * 2).next_power_of_two().max(16);
        self.table.clear();
        self.table.resize(size, 0);
        let mask = size - 1;
        for (run, &(cell, _, _)) in self.runs.iter().enumerate() {
            let mut slot = (cell_hash(cell) >> 32) as usize & mask;
            while self.table[slot] != 0 {
                slot = (slot + 1) & mask;
            }
            self.table[slot] = run as u32 + 1;
        }
    }

    fn slot(&self, (x, y): (i32, i32)) -> usize {
        (x - self.low.0) as usize * self.size.1 as usize + (y - self.low.1) as usize
    }

    fn members(&self, cell: (i32, i32)) -> &[((i32, i32), u32)] {
        if self.size != (0, 0) {
            let slot = self.slot(cell);
            return &self.keyed[self.starts[slot] as usize..self.starts[slot + 1] as usize];
        }
        let mask = self.table.len() - 1;
        let mut slot = (cell_hash(cell) >> 32) as usize & mask;
        loop {
            match self.table[slot] {
                0 => return &[],
                run => {
                    let (found, start, end) = self.runs[run as usize - 1];
                    if found == cell {
                        return &self.keyed[start as usize..end as usize];
                    }
                }
            }
            slot = (slot + 1) & mask;
        }
    }

    /// Calls `visit(i, j)` for every pair of balls in the same or neighbouring cells, with `j > i`.
    pub fn for_each_pair(&self, mut visit: impl FnMut(usize, usize)) {
        let mut around: [&[((i32, i32), u32)]; 9] = [&[]; 9];
        for &((cx, cy), start, end) in &self.runs {
            let mut slot = 0;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    around[slot] = self.members((cx.wrapping_add(dx), cy.wrapping_add(dy)));
                    slot += 1;
                }
            }
            for &(_, i) in &self.keyed[start as usize..end as usize] {
                for others in around {
                    for &(_, j) in others {
                        if j > i {
                            visit(i as usize, j as usize);
                        }
                    }
                }
            }
        }
    }
}
