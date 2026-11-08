//! Piles (Meets things: At shapes Pile up): particles that land on a still shape's up-facing
//! surface, or on the floor, join a heap there instead of staying particles. A pile is a run of
//! [`COLUMN`]-wide columns in world x on one surface, each with the surface's height under it and
//! how much sand stands on it. A grain added slides into lower neighbours while its column stands
//! steeper than the Angle of repose, and pours off a ledge as a particle again. Balls cut through
//! piles and throw the sand back up (and, two-way, rest on them: `two_way.rs`).

use std::collections::BTreeMap;

use bpe_runtime::World;
use glam::Vec2;

/// How wide a column is, in pixels.
pub const COLUMN: f32 = 2.0;
/// The key of the floor's piles (the picture's bottom edge).
pub const FLOOR: u32 = u32::MAX;
/// How far apart (in y) two surfaces at the same place may be and still be one pile's.
pub const SAME_SURFACE: f32 = 6.0;
/// How far up or down from a column its neighbour's surface may be and still be the same surface.
pub const NEXT_SURFACE: f32 = 12.0;
/// The most sand one column holds, in pixels.
pub const MOST_HEIGHT: f32 = 4000.0;
/// The most slides one grain makes.
/// (Each moves half a column's excess; ones smaller than [`SLIDE_LEAST`] are left.)
pub const MOST_SLIDES: usize = 4096;
/// The most grains thrown back up in one go (one ball's cut, one crumble, one step's pours off
/// ledges).
pub const MOST_THROWN: usize = 200;
/// How far past a ledge's last column a grain that pours off starts, in pixels.
pub const SPILL_CLEAR: f32 = 1.0;
/// How fast a grain that pours off a ledge goes out from it, in pixels a second.
pub const SPILL_SPEED: f32 = 30.0;
/// The least sand worth sliding (and how far past its angle a slope may stand), in pixels.
pub const SLIDE_LEAST: f32 = 0.01;

/// One pile: a run of columns on one surface.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pile {
    /// The shape it sits on (its id), or [`FLOOR`].
    pub key: u32,
    /// The first column's index (its left edge at `first × COLUMN`).
    pub first: i32,
    /// Each column's surface y (sand stands from there upwards: smaller y).
    pub base: Vec<f32>,
    /// How much sand stands on each column, in pixels.
    pub height: Vec<f32>,
    /// How far the surface sits from the shape's outline line (its band and the grains' size):
    /// a new column's surface is the outline there plus this.
    pub lift: f32,
    /// Sand poured off its ends not yet a whole grain.
    pub spill: f32,
}

impl Pile {
    /// The middle x of column `i`.
    pub fn x_of(i: i32) -> f32 {
        (i as f32 + 0.5) * COLUMN
    }

    /// The column index `x` falls in.
    pub fn column_at(x: f32) -> i32 {
        (x / COLUMN).floor() as i32
    }

    /// Column `i`'s place in the lists, if the pile has it.
    pub fn index(&self, i: i32) -> Option<usize> {
        let k = i - self.first;
        (k >= 0 && (k as usize) < self.base.len()).then_some(k as usize)
    }

    /// The top of the sand in column place `k` (its y).
    pub fn top(&self, k: usize) -> f32 {
        self.base[k] - self.height[k]
    }

    /// The last column's index.
    pub fn last(&self) -> i32 {
        self.first + self.base.len() as i32 - 1
    }

    /// Whether it holds no sand at all.
    pub fn is_empty(&self) -> bool {
        self.height.iter().all(|h| *h <= 0.0)
    }
}

/// A thing's piles.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Piles {
    pub list: Vec<Pile>,
}

impl Piles {
    pub fn memory(&self) -> usize {
        self.list.iter().map(|p| (p.base.capacity() + p.height.capacity()) * 4).sum::<usize>()
            + self.list.capacity() * std::mem::size_of::<Pile>()
    }

    /// How much sand there is in all (pixels of column height × column width).
    pub fn sand(&self) -> f32 {
        self.list.iter().map(|p| p.height.iter().sum::<f32>()).sum::<f32>() * COLUMN
    }
}

/// The surfaces piles may sit on this step: each still wall's outlines (one for a shape, a
/// Text's every letter contour), and the floor.
#[derive(Clone, Debug, Default)]
pub struct Surfaces {
    pub shapes: BTreeMap<u32, Vec<Vec<Vec2>>>,
    /// The floor's y and its x span, when there is one.
    pub floor: Option<(f32, f32, f32)>,
}

impl Surfaces {
    /// The still shapes of `world` (no spin, no growth, not moving) and the floor.
    pub fn of(world: &World, floor: Option<(f32, f32, f32)>) -> Surfaces {
        let shapes = world
            .walls
            .iter()
            .filter(|(_, w)| still(w))
            .map(|(id, w)| (id.0, crate::things::light::draw::outline_contours(&w.outline)))
            .collect();
        Surfaces { shapes, floor }
    }

    /// The outline line's y at `x` on surface `key` nearest `near` (within [`NEXT_SURFACE`] plus
    /// `slack`), if there is one.
    pub fn line_at(&self, key: u32, x: f32, near: f32, slack: f32) -> Option<f32> {
        if key == FLOOR {
            let (y, lo, hi) = self.floor?;
            return (x >= lo && x <= hi).then_some(y);
        }
        let mut best: Option<f32> = None;
        for (points, i) in self.shapes.get(&key)?.iter().flat_map(|c| (0..c.len()).map(move |i| (c, i))) {
            let n = points.len();
            let (a, b) = (points[i], points[(i + 1) % n]);
            if (a.x - x) * (b.x - x) > 0.0 || a.x == b.x {
                continue;
            }
            let y = a.y + (x - a.x) / (b.x - a.x) * (b.y - a.y);
            if (y - near).abs() <= NEXT_SURFACE + slack && best.is_none_or(|b| (y - near).abs() < (b - near).abs()) {
                best = Some(y);
            }
        }
        best
    }
}

/// Whether a wall stands still (sand on it stays put).
pub fn still(w: &bpe_runtime::Wall) -> bool {
    w.spin == 0.0 && w.growth == 0.0 && w.velocity == Vec2::ZERO && !w.passes
}

/// Where a falling particle from `previous` to `place` meets a pile's top: the pile's place in
/// the list and the column, and the top's y there.
pub fn meet_piles(piles: &Piles, previous: Vec2, place: Vec2) -> Option<(usize, i32, f32)> {
    let i = Pile::column_at(place.x);
    for (n, p) in piles.list.iter().enumerate() {
        let Some(k) = p.index(i) else { continue };
        if p.height[k] <= 0.0 {
            continue;
        }
        let top = p.top(k);
        if previous.y <= top + 0.5 && place.y >= top && place.y <= p.base[k] + 2.0 {
            return Some((n, i, top));
        }
    }
    None
}

/// What adding sand gave back: grains poured off a ledge, as places.
pub type Spilled = Vec<(Vec2, f32)>;

/// The pile of surface `key` at column `i` with its surface near `base`, made when there is none.
pub fn pile_for(piles: &mut Piles, key: u32, i: i32, base: f32, lift: f32) -> usize {
    for (n, p) in piles.list.iter_mut().enumerate() {
        if p.key != key {
            continue;
        }
        if let Some(k) = p.index(i)
            && (p.base[k] - base).abs() <= SAME_SURFACE
        {
            return n;
        }
        // Next to its ends: it grows to take the column.
        if (i == p.first - 1 && (p.base[0] - base).abs() <= SAME_SURFACE)
            || (i == p.last() + 1 && (p.base[p.base.len() - 1] - base).abs() <= SAME_SURFACE)
        {
            grow(p, i, base);
            return n;
        }
    }
    piles.list.push(Pile { key, first: i, base: vec![base], height: vec![0.0], lift, spill: 0.0 });
    piles.list.len() - 1
}

/// Joins the piles on one surface that have grown into or up to each other, so sand on one
/// stretch of surface is one heap (their heights add where both have a column).
pub fn merge_touching(piles: &mut Piles) {
    let mut n = 0;
    while n < piles.list.len() {
        let mut joined = false;
        let mut m = n + 1;
        while m < piles.list.len() {
            if touching(&piles.list[n], &piles.list[m]) {
                let b = piles.list.remove(m);
                join(&mut piles.list[n], b);
                joined = true;
            } else {
                m += 1;
            }
        }
        // One that grew may now touch one it was checked against before.
        if !joined {
            n += 1;
        }
    }
}

/// Whether `a` and `b` lie on one stretch of the same surface: overlapping or side by side,
/// their surfaces together where they meet.
fn touching(a: &Pile, b: &Pile) -> bool {
    if a.key != b.key || a.first > b.last() + 1 || b.first > a.last() + 1 {
        return false;
    }
    let (from, to) = (a.first.max(b.first), a.last().min(b.last()));
    if from <= to {
        // Overlapping: the same surface in every shared column.
        (from..=to).all(|i| match (a.index(i), b.index(i)) {
            (Some(ka), Some(kb)) => (a.base[ka] - b.base[kb]).abs() <= SAME_SURFACE,
            _ => false,
        })
    } else {
        // Side by side: the facing ends' surfaces.
        let (left, right) = if a.last() < b.first { (a, b) } else { (b, a) };
        (left.base[left.base.len() - 1] - right.base[0]).abs() <= SAME_SURFACE
    }
}

/// Puts `b`'s sand into `a` (which [`touching`] said lies with it).
fn join(a: &mut Pile, b: Pile) {
    let (first, last) = (a.first.min(b.first), a.last().max(b.last()));
    let mut base = Vec::with_capacity((last - first + 1) as usize);
    let mut height = Vec::with_capacity(base.capacity());
    for i in first..=last {
        let (ka, kb) = (a.index(i), b.index(i));
        base.push(ka.map(|k| a.base[k]).or(kb.map(|k| b.base[k])).unwrap_or(0.0));
        let h = ka.map_or(0.0, |k| a.height[k]) + kb.map_or(0.0, |k| b.height[k]);
        height.push(h.min(MOST_HEIGHT));
    }
    a.first = first;
    a.base = base;
    a.height = height;
    a.spill += b.spill;
}

/// Grows `p` by column `i` (just past one of its ends) with surface `base`.
fn grow(p: &mut Pile, i: i32, base: f32) {
    if i < p.first {
        p.first = i;
        p.base.insert(0, base);
        p.height.insert(0, 0.0);
    } else {
        p.base.push(base);
        p.height.push(0.0);
    }
}

/// Whether column `j` (next to column `i`) is in `p`, grown when the surface goes on there;
/// false past a ledge.
fn reach_column(p: &mut Pile, j: i32, i: i32, surfaces: &Surfaces) -> bool {
    if p.index(j).is_some() {
        return true;
    }
    let Some(k) = p.index(i) else { return false };
    let Some(line) = surfaces.line_at(p.key, Pile::x_of(j), p.base[k] - p.lift, 0.0) else { return false };
    grow(p, j, line + p.lift);
    true
}

/// Adds `amount` pixels of sand to column `i` of pile `n`, letting it slide down to the angle
/// `repose` (radians); what pours off a ledge comes back as grains of `grain` pixels each.
pub fn add_sand(
    piles: &mut Piles,
    n: usize,
    i: i32,
    amount: f32,
    repose: f32,
    grain: f32,
    surfaces: &Surfaces,
) -> Spilled {
    let mut spilled = Vec::new();
    let Some(p) = piles.list.get_mut(n) else { return spilled };
    let Some(k) = p.index(i) else { return spilled };
    p.height[k] = (p.height[k] + amount.max(0.0)).min(MOST_HEIGHT);
    let allowed = repose.clamp(0.0, 1.55).tan() * COLUMN;
    let grain = grain.max(1e-3);
    // Columns to look at again: one that gave sand away may now stand too far below its other
    // neighbour, and one that took sand too far above its own.
    let mut todo = vec![i];
    let mut moves = 0;
    while let Some(i) = todo.pop() {
        if moves >= MOST_SLIDES {
            break;
        }
        let Some(_) = p.index(i) else { continue };
        // The lower neighbour the column stands most above, the left one first on a tie; past a
        // ledge the sand falls away from the column's own surface.
        let mut best: Option<(i32, f32, bool)> = None;
        for side in [-1, 1] {
            let there = reach_column(p, i + side, i, surfaces);
            let k = p.index(i).expect("the column is in the pile");
            let drop = match p.index(i + side).filter(|_| there) {
                Some(nk) => p.top(nk) - p.top(k),
                None => p.height[k],
            };
            if drop > allowed + SLIDE_LEAST && best.is_none_or(|(_, d, _)| drop > d) {
                best = Some((side, drop, there));
            }
        }
        let Some((side, drop, there)) = best else { continue };
        let k = p.index(i).expect("the column is in the pile");
        let moved = ((drop - allowed) * 0.5).min(p.height[k]).max(0.0);
        if moved <= SLIDE_LEAST * 0.5 {
            continue;
        }
        moves += 1;
        p.height[k] -= moved;
        if there {
            let nk = p.index(i + side).expect("reached");
            p.height[nk] = (p.height[nk] + moved).min(MOST_HEIGHT);
            todo.extend([i - side, i, i + side]);
        } else {
            // Off the ledge: whole grains fall from the column's top edge.
            p.spill += moved;
            // Clear of the column, so it falls past the ledge and not back onto it.
            let edge = Vec2::new(Pile::x_of(i) + side as f32 * (COLUMN * 0.5 + SPILL_CLEAR), p.top(k));
            while p.spill >= grain && spilled.len() < MOST_THROWN {
                p.spill -= grain;
                spilled.push((edge, side as f32));
            }
            todo.extend([i - side, i]);
        }
    }
    spilled
}

/// Cuts every pile down to the circle at `centre` of `radius` where it stands in the sand: the
/// sand above its bottom goes. Returns where sand was cut and how much (pixels of height).
pub fn cut_by_ball(piles: &mut Piles, centre: Vec2, radius: f32) -> Vec<(Vec2, f32)> {
    let mut cut = Vec::new();
    if !(centre.is_finite() && radius > 0.0) {
        return cut;
    }
    let (from, to) = (Pile::column_at(centre.x - radius), Pile::column_at(centre.x + radius));
    for p in &mut piles.list {
        if p.last() < from || p.first > to {
            continue;
        }
        for i in from.max(p.first)..=to.min(p.last()) {
            let Some(k) = p.index(i) else { continue };
            let dx = Pile::x_of(i) - centre.x;
            if dx.abs() >= radius || p.height[k] <= 0.0 || centre.y >= p.base[k] {
                continue;
            }
            let bottom = centre.y + (radius * radius - dx * dx).sqrt();
            let top = p.top(k);
            if bottom <= top {
                continue;
            }
            let new_top = bottom.min(p.base[k]);
            let gone = (new_top - top).min(p.height[k]);
            if gone > 0.0 {
                p.height[k] -= gone;
                cut.push((Vec2::new(Pile::x_of(i), top), gone));
            }
        }
    }
    cut
}

/// How deep the circle at `centre` of `radius` sinks into the piles: the most, over the columns
/// under it, its bottom stands below the sand's top (0 when it doesn't touch sand).
pub fn sinks(piles: &Piles, centre: Vec2, radius: f32) -> f32 {
    let mut most: f32 = 0.0;
    if !(centre.is_finite() && radius > 0.0) {
        return 0.0;
    }
    let (from, to) = (Pile::column_at(centre.x - radius), Pile::column_at(centre.x + radius));
    for p in &piles.list {
        if p.last() < from || p.first > to {
            continue;
        }
        for i in from.max(p.first)..=to.min(p.last()) {
            let Some(k) = p.index(i) else { continue };
            let dx = Pile::x_of(i) - centre.x;
            if dx.abs() >= radius || p.height[k] <= 0.5 || centre.y >= p.base[k] {
                continue;
            }
            let bottom = centre.y + (radius * radius - dx * dx).sqrt();
            most = most.max(bottom - p.top(k));
        }
    }
    most
}

/// Takes away the piles whose surface is gone or has started moving, giving back where their
/// sand was as `(top place, height)` per column with sand.
pub fn crumble_moved(piles: &mut Piles, surfaces: &Surfaces) -> Vec<(Vec2, f32)> {
    let mut out = Vec::new();
    piles.list.retain(|p| {
        let there = if p.key == FLOOR { surfaces.floor.is_some() } else { surfaces.shapes.contains_key(&p.key) };
        if !there {
            for k in 0..p.base.len() {
                if p.height[k] > 0.0 {
                    out.push((Vec2::new(Pile::x_of(p.first + k as i32), p.top(k)), p.height[k]));
                }
            }
        }
        there
    });
    out
}

/// The outline line's y under a landing at `at` on `key`, for a new pile's lift.
pub fn lift_of(surfaces: &Surfaces, key: u32, at: Vec2) -> f32 {
    surfaces.line_at(key, at.x, at.y, 8.0).map_or(0.0, |line| at.y - line)
}
