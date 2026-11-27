//! Measurements of a body's drops and a debug picture of them, for tests and for the builder.
//! Everything here reads drops already brought back from the card; nothing here is the liquid's
//! look (that is the renderer's, a later part). Every measure of the liquid leaves trapped air out
//! (part 2c); [`print_of`] takes every drop.

use std::borrow::Cow;
use std::collections::HashMap;

use glam::Vec2;

use crate::drop::Drop;
use crate::solid::{Layout, SolidField};

/// What a body's drops are doing as a whole.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub drops: usize,
    /// Root mean square speed, pixels a second.
    pub rms: f32,
    /// The fastest drop, pixels a second.
    pub fastest: f32,
    /// The liquid's top (world y is down): the y above which 2 % of the drops are.
    pub top: f32,
    /// The highest drop's y.
    pub highest: f32,
    /// Drops within a tenth of a cell of another (clumps a picture would show).
    pub crowded: usize,
    /// Drops that are no number.
    pub broken: usize,
}

/// The liquid's drops among `drops`: all but the air.
pub fn liquid(drops: &[Drop]) -> Cow<'_, [Drop]> {
    if drops.iter().any(Drop::is_air) {
        Cow::Owned(drops.iter().filter(|d| !d.is_air()).copied().collect())
    } else {
        Cow::Borrowed(drops)
    }
}

/// The air drops among `drops`.
pub fn air(drops: &[Drop]) -> Vec<Drop> {
    drops.iter().filter(|d| d.is_air()).copied().collect()
}

impl Stats {
    pub fn of(drops: &[Drop], layout: &Layout) -> Stats {
        let drops = &*liquid(drops);
        let mut ys: Vec<f32> = Vec::with_capacity(drops.len());
        let (mut sum, mut fastest, mut broken) = (0.0f64, 0.0f32, 0usize);
        let mut at: HashMap<(i32, i32), usize> = HashMap::new();
        for d in drops {
            let (x, v) = (Vec2::from(d.x), Vec2::from(d.v) * layout.cell);
            if !x.is_finite() || !v.is_finite() {
                broken += 1;
                continue;
            }
            ys.push(layout.to_world(x).y);
            sum += f64::from(v.length_squared());
            fastest = fastest.max(v.length());
            *at.entry(((x.x * 10.0) as i32, (x.y * 10.0) as i32)).or_default() += 1;
        }
        ys.sort_by(f32::total_cmp);
        Stats {
            drops: drops.len(),
            rms: (sum / ys.len().max(1) as f64).sqrt() as f32,
            fastest,
            top: ys.get(ys.len() / 50).copied().unwrap_or(0.0),
            highest: ys.first().copied().unwrap_or(0.0),
            crowded: at.values().filter(|n| **n > 1).sum(),
            broken,
        }
    }
}

/// The drops whose world place is inside the rectangle `lo` to `hi`.
pub fn inside(drops: &[Drop], layout: &Layout, lo: Vec2, hi: Vec2) -> Vec<Drop> {
    drops
        .iter()
        .filter(|d| !d.is_air())
        .filter(|d| {
            let p = layout.to_world(Vec2::from(d.x));
            p.cmpge(lo).all() && p.cmple(hi).all()
        })
        .copied()
        .collect()
}

/// How many drops lie within `reach` pixels of the world segment `from` to `to`, by `bins` equal
/// stretches along it: how even a film or a thread is along its length.
pub fn along(drops: &[Drop], layout: &Layout, from: Vec2, to: Vec2, reach: f32, bins: usize) -> Vec<u32> {
    let drops = &*liquid(drops);
    let mut counts = vec![0u32; bins];
    let length = (to - from).length();
    if bins == 0 || length.is_nan() || length <= 0.0 {
        return counts;
    }
    let way = (to - from) / length;
    for d in drops {
        let p = layout.to_world(Vec2::from(d.x)) - from;
        let (s, off) = (p.dot(way), p.perp_dot(way));
        if p.is_finite() && (0.0..length).contains(&s) && off.abs() <= reach {
            counts[((s / length * bins as f32) as usize).min(bins - 1)] += 1;
        }
    }
    counts
}

/// How ragged a film or a thread is: for each drop, how far the drops within `reach` pixels of
/// it lie off the line through them (pixels: the lesser spread of that little cloud). A clean
/// thread reads about a third of its own thickness; barbs, ribs and teeth read more. Returns the
/// mean and the value nine tenths of the drops are under (0 with too few drops to tell).
pub fn ragged(drops: &[Drop], layout: &Layout, reach: f32) -> (f32, f32) {
    let drops = &*liquid(drops);
    let at: Vec<Vec2> = drops.iter().map(|d| layout.to_world(Vec2::from(d.x))).filter(|x| x.is_finite()).collect();
    let key = |x: Vec2| ((x.x / reach).floor() as i32, (x.y / reach).floor() as i32);
    let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
    for (i, x) in at.iter().enumerate() {
        cells.entry(key(*x)).or_default().push(i as u32);
    }
    let mut off: Vec<f32> = Vec::with_capacity(at.len());
    for x in &at {
        let (cx, cy) = key(*x);
        let (mut n, mut sum, mut sq, mut xy) = (0.0f64, glam::DVec2::ZERO, glam::DVec2::ZERO, 0.0f64);
        for dy in -1..=1 {
            for dx in -1..=1 {
                for &j in cells.get(&(cx + dx, cy + dy)).into_iter().flatten() {
                    let q = at[j as usize] - *x;
                    if q.length_squared() <= reach * reach {
                        let q = q.as_dvec2();
                        n += 1.0;
                        sum += q;
                        sq += q * q;
                        xy += q.x * q.y;
                    }
                }
            }
        }
        if n < 4.0 {
            continue;
        }
        // The lesser eigenvalue of the cloud's 2 x 2 spread.
        let mean = sum / n;
        let (a, b, c) = (sq.x / n - mean.x * mean.x, xy / n - mean.x * mean.y, sq.y / n - mean.y * mean.y);
        let less = 0.5 * (a + c) - (0.25 * (a - c) * (a - c) + b * b).sqrt();
        off.push(less.max(0.0).sqrt() as f32);
    }
    if off.is_empty() {
        return (0.0, 0.0);
    }
    off.sort_by(f32::total_cmp);
    (off.iter().sum::<f32>() / off.len() as f32, off[off.len() * 9 / 10])
}

/// How whole a stream is: the drops of one region of air, grouped into pieces.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stream {
    pub drops: usize,
    /// Groups of drops in which each is within the joining distance of another.
    pub pieces: usize,
    /// The share of the drops in the biggest piece.
    pub whole: f32,
    /// The share of the drops in pieces of fewer than four drops (a rain of droplets).
    pub strays: f32,
    /// The biggest piece's width (pixels) in each of the bands the region was cut into, top first
    /// (0 where it has no drop).
    pub widths: Vec<f32>,
    /// The longest stretch down the region (pixels) in which the biggest piece has no drop.
    pub gap: f32,
}

impl Stream {
    /// The stream measures of `drops` (already cut to a region, [`inside`]) from world height
    /// `top` to `bottom`, joined at `join` cells, its width taken in `bands` bands.
    pub fn of(drops: &[Drop], layout: &Layout, (top, bottom): (f32, f32), join: f32, bands: usize) -> Stream {
        let drops = &*liquid(drops);
        let n = drops.len();
        if n == 0 {
            return Stream { widths: vec![0.0; bands], gap: bottom - top, ..Stream::default() };
        }
        // Union-find over a hash grid of `join`-sized cells.
        let mut parent: Vec<u32> = (0..n as u32).collect();
        fn find(parent: &mut [u32], mut a: u32) -> u32 {
            while parent[a as usize] != a {
                parent[a as usize] = parent[parent[a as usize] as usize];
                a = parent[a as usize];
            }
            a
        }
        let key = |d: &Drop| ((d.x[0] / join).floor() as i32, (d.x[1] / join).floor() as i32);
        let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, d) in drops.iter().enumerate() {
            cells.entry(key(d)).or_default().push(i as u32);
        }
        for (i, d) in drops.iter().enumerate() {
            let (cx, cy) = key(d);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let Some(list) = cells.get(&(cx + dx, cy + dy)) else { continue };
                    for &j in list {
                        if j as usize > i
                            && (Vec2::from(drops[j as usize].x) - Vec2::from(d.x)).length_squared() <= join * join
                        {
                            let (a, b) = (find(&mut parent, i as u32), find(&mut parent, j));
                            if a != b {
                                parent[a.max(b) as usize] = a.min(b);
                            }
                        }
                    }
                }
            }
        }
        let mut sizes: HashMap<u32, usize> = HashMap::new();
        for i in 0..n as u32 {
            *sizes.entry(find(&mut parent, i)).or_default() += 1;
        }
        let (&biggest, &most) = sizes.iter().max_by_key(|(root, size)| (**size, u32::MAX - **root)).expect("a piece");
        let strays = sizes.values().filter(|s| **s < 4).sum::<usize>();
        // The biggest piece: its width by band and its longest gap down the region.
        let mut lo = vec![f32::MAX; bands];
        let mut hi = vec![f32::MIN; bands];
        let mut ys = vec![top, bottom];
        for (i, d) in drops.iter().enumerate() {
            if find(&mut parent, i as u32) != biggest {
                continue;
            }
            let p = layout.to_world(Vec2::from(d.x));
            ys.push(p.y);
            let band = (((p.y - top) / (bottom - top)) * bands as f32).floor();
            if band >= 0.0 && (band as usize) < bands {
                lo[band as usize] = lo[band as usize].min(p.x);
                hi[band as usize] = hi[band as usize].max(p.x);
            }
        }
        ys.sort_by(f32::total_cmp);
        let gap = ys.windows(2).map(|w| w[1] - w[0]).fold(0.0, f32::max);
        Stream {
            drops: n,
            pieces: sizes.len(),
            whole: most as f32 / n as f32,
            strays: strays as f32 / n as f32,
            widths: lo.iter().zip(&hi).map(|(l, h)| if h >= l { h - l } else { 0.0 }).collect(),
            gap,
        }
    }
}

/// How wide and how tall the liquid lying on a level floor at world height `floor` between world
/// `x.0` and `x.1` is, pixels: its columns a spacing wide that hold a drop (gaps left out), and its
/// tallest column's top above the floor (half a spacing added, a drop's own room). (Keep `x` two
/// cells in from any wall: liquid climbing a wall is not the puddle's.)
pub fn puddle(drops: &[Drop], layout: &Layout, floor: f32, x: (f32, f32)) -> (f32, f32) {
    let spacing = 0.5 * layout.cell;
    let mut tops = std::collections::BTreeMap::<i64, f32>::new();
    for d in liquid(drops).iter() {
        let p = layout.to_world(Vec2::from(d.x));
        if !p.is_finite() || p.y > floor || p.x < x.0 || p.x > x.1 {
            continue;
        }
        let top = tops.entry((p.x / spacing).floor() as i64).or_insert(f32::MAX);
        *top = top.min(p.y);
    }
    let width = tops.len() as f32 * spacing;
    let height = tops.values().map(|y| floor - y).fold(0.0f32, f32::max) + 0.5 * spacing;
    (width, height)
}

/// A debug picture of `drops` on `field`'s solids: `scale` picture pixels a world pixel, rgba8.
/// A drop is a dot: blue, towards red as it carries another liquid's colour (part 4.2); an air drop
/// a smaller pale yellow dot.
pub fn picture(drops: &[Drop], field: &SolidField, scale: f32) -> (Vec<u8>, u32, u32) {
    let l = &field.layout;
    let size = Vec2::new((l.cols - 1) as f32, (l.rows - 1) as f32) * l.cell * scale;
    let (w, h) = (size.x.ceil().max(1.0) as u32, size.y.ceil().max(1.0) as u32);
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let p = l.origin + Vec2::new(x as f32 + 0.5, y as f32 + 0.5) / scale;
            let solid = field.at(p) < 0.0;
            let c: [u8; 4] = if solid { [235, 235, 235, 255] } else { [40, 40, 44, 255] };
            out[((y * w + x) * 4) as usize..][..4].copy_from_slice(&c);
        }
    }
    let radius = (0.3 * l.cell * scale).max(0.8);
    // (Liquid first, then air on top, a pale yellow, a little smaller.)
    let air = air(drops);
    for d in liquid(drops).iter().chain(air.iter()) {
        let p = (l.to_world(Vec2::from(d.x)) - l.origin) * scale;
        if !p.is_finite() {
            continue;
        }
        let radius = if d.is_air() { (0.22 * l.cell * scale).max(0.6) } else { radius };
        let colour: [u8; 4] = if d.is_air() {
            [250, 215, 90, 255]
        } else if d.mix_share() > 0.0 {
            // (A drop carrying another liquid's colour, by how much: blue to red.)
            let t = d.mix_share();
            [(90.0 + 150.0 * t) as u8, (150.0 - 80.0 * t) as u8, (255.0 - 195.0 * t) as u8, 255]
        } else {
            [90, 150, 255, 255]
        };
        let r = radius.ceil() as i32;
        for dy in -r..=r {
            for dx in -r..=r {
                let (x, y) = (p.x.floor() as i32 + dx, p.y.floor() as i32 + dy);
                let off = Vec2::new(x as f32 + 0.5, y as f32 + 0.5) - p;
                if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h && off.length() <= radius {
                    out[((y as u32 * w + x as u32) * 4) as usize..][..4].copy_from_slice(&colour);
                }
            }
        }
    }
    (out, w, h)
}

/// Writes rgba8 `pixels` as a PNG.
pub fn save_png(path: &std::path::Path, pixels: &[u8], w: u32, h: u32) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = std::fs::File::create(path)?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(pixels)?;
    Ok(())
}

/// How a body of liquid is broken up ([`pieces`]).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pieces {
    /// Pieces in the region (drops nearer each other than the reach are one piece).
    pub pieces: usize,
    /// Drops in the region.
    pub drops: usize,
    /// The share of the region's drops in small pieces (under `small_px` across both ways).
    pub small_share: f32,
    /// The biggest piece's drops.
    pub biggest: usize,
}

/// How the drops inside the world rectangle `region` (lo, hi) are broken up: drops nearer than
/// `near_spacings` rest spacings (half a cell each) are one piece, and a piece is small when its
/// bounding box is under `small_px` pixels across both ways. Spray at every detail is small; a sheet
/// or a stream is not.
pub fn pieces(drops: &[Drop], layout: &Layout, near_spacings: f32, small_px: f32, region: (Vec2, Vec2)) -> Pieces {
    let here = inside(drops, layout, region.0, region.1);
    let n = here.len();
    if n == 0 {
        return Pieces::default();
    }
    let reach = near_spacings * 0.5 * layout.cell;
    let at: Vec<Vec2> = here.iter().map(|d| layout.to_world(Vec2::from(d.x))).collect();
    // (Bins as wide as the reach: a drop's neighbours are in its bin or the eight round it.)
    let bin = |p: Vec2| ((p.x / reach).floor() as i64, (p.y / reach).floor() as i64);
    let mut bins: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for (i, p) in at.iter().enumerate() {
        bins.entry(bin(*p)).or_default().push(i);
    }
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for (i, p) in at.iter().enumerate() {
        let (bx, by) = bin(*p);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let Some(list) = bins.get(&(bx + dx, by + dy)) else { continue };
                for &j in list {
                    if j > i && at[j].distance_squared(*p) < reach * reach {
                        let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                        if a != b {
                            parent[a.max(b)] = a.min(b);
                        }
                    }
                }
            }
        }
    }
    // Each piece's drops and bounding box.
    let mut boxes: HashMap<usize, (usize, Vec2, Vec2)> = HashMap::new();
    for (i, p) in at.iter().enumerate() {
        let r = root(&mut parent, i);
        let e = boxes.entry(r).or_insert((0, *p, *p));
        e.0 += 1;
        e.1 = e.1.min(*p);
        e.2 = e.2.max(*p);
    }
    let small: usize = boxes
        .values()
        .filter(|(_, lo, hi)| {
            let size = *hi - *lo;
            size.x < small_px && size.y < small_px
        })
        .map(|(k, _, _)| *k)
        .sum();
    Pieces {
        pieces: boxes.len(),
        drops: n,
        small_share: small as f32 / n as f32,
        biggest: boxes.values().map(|(k, _, _)| *k).max().unwrap_or(0),
    }
}

/// Room shut in by the liquid ([`pockets`]): what a picture shows as holes in the water.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pockets {
    /// The trapped room, square pixels.
    pub area: f32,
    /// The biggest connected pocket's room (square pixels) and its middle (world).
    pub biggest: f32,
    pub middle: Vec2,
}

/// The room shut in by the liquid among `drops` on `field`: squares half a cell wide, out of the
/// solids, with no liquid drop within a cell of their middle (a hole at least two cells across),
/// and liquid above, below, left and right of them along their column and row. (Air drops are not
/// liquid: a pocket filled with air is still a pocket.)
pub fn pockets(drops: &[Drop], field: &SolidField) -> Pockets {
    let layout = &field.layout;
    let drops = &*liquid(drops);
    let at: Vec<Vec2> = drops.iter().map(|d| Vec2::from(d.x)).filter(|x| x.is_finite()).collect();
    if at.is_empty() {
        return Pockets::default();
    }
    let (cols, rows) = ((layout.cols - 1) * 2, (layout.rows - 1) * 2);
    // The liquid's reach along each column and row of squares: its first and last drop there.
    let mut down = vec![(f32::MAX, f32::MIN); cols as usize];
    let mut across = vec![(f32::MAX, f32::MIN); rows as usize];
    let mut bins: HashMap<(i32, i32), Vec<Vec2>> = HashMap::new();
    for x in &at {
        let (c, r) = ((x.x * 2.0).floor() as i64, (x.y * 2.0).floor() as i64);
        if (0..i64::from(cols)).contains(&c) {
            let e = &mut down[c as usize];
            *e = (e.0.min(x.y), e.1.max(x.y));
        }
        if (0..i64::from(rows)).contains(&r) {
            let e = &mut across[r as usize];
            *e = (e.0.min(x.x), e.1.max(x.x));
        }
        bins.entry((x.x.floor() as i32, x.y.floor() as i32)).or_default().push(*x);
    }
    let clear = |q: Vec2| {
        let (bx, by) = (q.x.floor() as i32, q.y.floor() as i32);
        for dy in -1..=1 {
            for dx in -1..=1 {
                if bins.get(&(bx + dx, by + dy)).is_some_and(|l| l.iter().any(|p| p.distance_squared(q) < 1.0)) {
                    return false;
                }
            }
        }
        true
    };
    let mut trapped = vec![false; (cols * rows) as usize];
    for r in 0..rows {
        let (left, right) = across[r as usize];
        for c in 0..cols {
            let q = Vec2::new(c as f32 * 0.5 + 0.25, r as f32 * 0.5 + 0.25);
            let (top, bottom) = down[c as usize];
            if !(top < q.y && bottom > q.y && left < q.x && right > q.x) {
                continue;
            }
            if field.at(layout.to_world(q)) > 0.25 * layout.cell && clear(q) {
                trapped[(r * cols + c) as usize] = true;
            }
        }
    }
    // The pieces, by their squares' sides.
    let mut parent: Vec<usize> = (0..trapped.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for r in 0..rows as usize {
        for c in 0..cols as usize {
            let i = r * cols as usize + c;
            if !trapped[i] {
                continue;
            }
            for j in [i + 1, i + cols as usize] {
                let side = if j == i + 1 { c + 1 < cols as usize } else { r + 1 < rows as usize };
                if side && trapped[j] {
                    let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                    if a != b {
                        parent[a.max(b)] = a.min(b);
                    }
                }
            }
        }
    }
    let square = 0.25 * layout.cell * layout.cell;
    let mut sizes: HashMap<usize, (usize, Vec2)> = HashMap::new();
    let mut all = 0usize;
    for (i, t) in trapped.iter().enumerate() {
        if *t {
            all += 1;
            let q = Vec2::new((i % cols as usize) as f32 * 0.5 + 0.25, (i / cols as usize) as f32 * 0.5 + 0.25);
            let e = sizes.entry(root(&mut parent, i)).or_insert((0, Vec2::ZERO));
            e.0 += 1;
            e.1 += q;
        }
    }
    let (n, sum) = sizes.values().copied().max_by_key(|(n, _)| *n).unwrap_or((0, Vec2::ZERO));
    Pockets {
        area: all as f32 * square,
        biggest: n as f32 * square,
        middle: if n > 0 { layout.to_world(sum / n as f32) } else { Vec2::ZERO },
    }
}

/// A print of every number of every drop: equal prints mean equal drops.
pub fn print_of(drops: &[Drop]) -> u64 {
    bytemuck::cast_slice::<Drop, u32>(drops)
        .iter()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, w| (h ^ u64::from(*w)).wrapping_mul(0x0000_0100_0000_01b3))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drop::{STOPS, WATER};

    #[test]
    fn the_measures_leave_air_out() {
        use crate::drop::AIR;
        let layout = Layout { origin: Vec2::ZERO, cell: 8.0, cols: 200, rows: 200 };
        let at = |x: f32, y: f32, kind: u32| Drop::new(layout.to_grid(Vec2::new(x, y)), Vec2::ZERO, kind, 0, WATER);
        let water: Vec<Drop> = (0..20).map(|k| at(100.0 + 4.0 * k as f32, 400.0, STOPS)).collect();
        let mut both = water.clone();
        both.extend((0..30).map(|k| at(100.0 + 4.0 * k as f32, 100.0, AIR | STOPS)));
        assert_eq!(Stats::of(&both, &layout), Stats::of(&water, &layout));
        assert_eq!(inside(&both, &layout, Vec2::ZERO, Vec2::splat(1600.0)).len(), 20);
        assert_eq!(liquid(&both).len(), 20);
        assert_eq!(air(&both).len(), 30);
        assert!(matches!(liquid(&water), Cow::Borrowed(_)), "no copy when there is no air");
        assert_ne!(print_of(&both), print_of(&water), "a print takes every drop");
    }

    #[test]
    fn pockets_are_holes_shut_in_by_the_liquid() {
        use crate::solid::Solid;
        let layout = Layout::cover(Vec2::new(-200.0, -200.0), Vec2::new(200.0, 200.0), 8.0);
        let field = SolidField::build(layout, &[Solid::rect(Vec2::splat(-190.0), Vec2::splat(190.0), false, 4.0)]);
        // A block of water 200 px square on the lattice (4 px apart), with a round hole 48 px across.
        let mut drops = Vec::new();
        for r in 0..50 {
            for c in 0..50 {
                let p = Vec2::new(-100.0 + 4.0 * c as f32 + 2.0, -100.0 + 4.0 * r as f32 + 2.0);
                if p.length() >= 24.0 {
                    drops.push(Drop::new(layout.to_grid(p), Vec2::ZERO, STOPS, 0, WATER));
                }
            }
        }
        let p = pockets(&drops, &field);
        // (The hole less a cell's rim all round: a disc 16 px across.)
        let disc = std::f32::consts::PI * 16.0 * 16.0;
        assert!(p.area > 0.6 * disc && p.area < 1.6 * disc, "{p:?} against {disc}");
        assert_eq!(p.biggest, p.area);
        assert!(p.middle.length() < 4.0, "{p:?}");
        // The same block whole has none, and one with the hole open to the top has none either.
        let whole: Vec<Drop> = (0..2500)
            .map(|k| {
                let p = Vec2::new(-100.0 + 4.0 * (k % 50) as f32 + 2.0, -100.0 + 4.0 * (k / 50) as f32 + 2.0);
                Drop::new(layout.to_grid(p), Vec2::ZERO, STOPS, 0, WATER)
            })
            .collect();
        assert_eq!(pockets(&whole, &field).area, 0.0);
        let open: Vec<Drop> = drops
            .iter()
            .filter(|d| layout.to_world(Vec2::from(d.x)).x.abs() >= 24.0 || d.x[1] > layout.to_grid(Vec2::ZERO).y)
            .copied()
            .collect();
        assert_eq!(pockets(&open, &field).area, 0.0, "a notch open to the top is no pocket");
    }

    #[test]
    fn pieces_tell_a_sheet_from_spray() {
        let layout = Layout { origin: Vec2::ZERO, cell: 8.0, cols: 200, rows: 200 };
        let at = |x: f32, y: f32| Drop::new(layout.to_grid(Vec2::new(x, y)), Vec2::ZERO, STOPS, 0, WATER);
        // A sheet 100 px long, two drops thick at rest spacing (4 px), and ten lone drops 30 px apart.
        let mut drops: Vec<Drop> =
            (0..25).flat_map(|k| [at(100.0 + 4.0 * k as f32, 100.0), at(100.0 + 4.0 * k as f32, 104.0)]).collect();
        drops.extend((0..10).map(|k| at(100.0 + 30.0 * k as f32, 300.0)));
        let all = (Vec2::ZERO, Vec2::splat(1600.0));
        let p = pieces(&drops, &layout, 1.6, 16.0, all);
        assert_eq!((p.pieces, p.drops, p.biggest), (11, 60, 50), "{p:?}");
        assert!((p.small_share - 10.0 / 60.0).abs() < 1e-6, "{p:?}");
        let only_spray = pieces(&drops, &layout, 1.6, 16.0, (Vec2::new(0.0, 200.0), Vec2::splat(1600.0)));
        assert_eq!(only_spray.small_share, 1.0);
    }
}
