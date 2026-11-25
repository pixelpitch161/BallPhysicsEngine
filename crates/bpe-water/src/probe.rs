//! Measurements of a body's drops and a debug picture of them, for tests and for the builder.
//! Everything here reads drops already brought back from the card; nothing here is the liquid's
//! look (that is the renderer's, a later part).

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

impl Stats {
    pub fn of(drops: &[Drop], layout: &Layout) -> Stats {
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
        .filter(|d| {
            let p = layout.to_world(Vec2::from(d.x));
            p.cmpge(lo).all() && p.cmple(hi).all()
        })
        .copied()
        .collect()
}

/// How evenly drops are spaced: each drop's distance to its nearest other, in cells. (Liquid at
/// rest has four drops a cell, half a cell apart; drops strewn at random at that density have a
/// median of about a quarter of a cell, and a third of them within a fifth.)
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Spacing {
    /// The median distance to the nearest other drop (0 with fewer than two drops).
    pub median: f32,
    /// The share of the drops with another within a quarter of a cell.
    pub close: f32,
}

impl Spacing {
    pub fn of(drops: &[Drop]) -> Spacing {
        let drops: Vec<Vec2> = drops.iter().map(|d| Vec2::from(d.x)).filter(|x| x.is_finite()).collect();
        if drops.len() < 2 {
            return Spacing::default();
        }
        let key = |x: Vec2| (x.x.floor() as i32, x.y.floor() as i32);
        let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, x) in drops.iter().enumerate() {
            cells.entry(key(*x)).or_default().push(i as u32);
        }
        // (A drop with no other within a cell of it counts as a cell away.)
        let mut nearest: Vec<f32> = drops
            .iter()
            .enumerate()
            .map(|(i, x)| {
                let (cx, cy) = key(*x);
                let mut least = 1.0f32;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        for &j in cells.get(&(cx + dx, cy + dy)).into_iter().flatten() {
                            if j as usize != i {
                                least = least.min((drops[j as usize] - *x).length());
                            }
                        }
                    }
                }
                least
            })
            .collect();
        let close = nearest.iter().filter(|d| **d < 0.25).count() as f32 / nearest.len() as f32;
        nearest.sort_by(f32::total_cmp);
        Spacing { median: nearest[nearest.len() / 2], close }
    }
}

/// How many drops lie within `reach` pixels of the world segment `from` to `to`, by `bins` equal
/// stretches along it: how even a film or a thread is along its length.
pub fn along(drops: &[Drop], layout: &Layout, from: Vec2, to: Vec2, reach: f32, bins: usize) -> Vec<u32> {
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

/// A debug picture of `drops` on `field`'s solids: `scale` picture pixels a world pixel, rgba8.
/// A drop is a dot: blue at rest, towards yellow as it stretches, red once it is at `tear`.
pub fn picture(drops: &[Drop], field: &SolidField, scale: f32, tear: f32) -> (Vec<u8>, u32, u32) {
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
    for d in drops {
        let p = (l.to_world(Vec2::from(d.x)) - l.origin) * scale;
        if !p.is_finite() {
            continue;
        }
        let colour: [u8; 4] = if d.e >= tear {
            [240, 70, 60, 255]
        } else if d.e > 0.02 {
            let t = (d.e / tear).clamp(0.0, 1.0);
            [(90.0 + 160.0 * t) as u8, (150.0 + 80.0 * t) as u8, (255.0 - 200.0 * t) as u8, 255]
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

/// A print of every number of every drop: equal prints mean equal drops.
pub fn print_of(drops: &[Drop]) -> u64 {
    bytemuck::cast_slice::<Drop, u32>(drops)
        .iter()
        .fold(0xcbf2_9ce4_8422_2325u64, |h, w| (h ^ u64::from(*w)).wrapping_mul(0x0000_0100_0000_01b3))
}
