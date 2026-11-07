//! Where each ball of a group starts, relative to the thing, for every group shape. All of them
//! are worked out the same way every time, on every machine.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, TAU};
use std::sync::{Mutex, OnceLock};

use glam::Vec2;

use super::{BallsSettings, GroupShape};
use crate::rng::Rng;

/// The most balls a shape places, so a huge word or area can't stall the editor.
pub const MAX_BALLS: usize = 5000;
/// How many places Scatter tries for each ball before it gives up on it.
const SCATTER_TRIES: u32 = 30;
/// Spiral: how far out each ball sits, in steps, times the square root of its number (plus
/// [`SPIRAL_START`]). The Vogel spiral's closest neighbours then sit just under a step apart.
const SPIRAL_SPREAD: f32 = 0.6;
/// Spiral: added to each ball's number, so the crowded middle opens up.
const SPIRAL_START: f32 = 1.5;
/// The font words are spelt in (the editor's own).
pub(crate) const FONT: &[u8] = include_bytes!("../../../../bpe-studio/assets/fonts/IBMPlexSans-SemiBold.ttf");

/// Where each ball starts, relative to the thing, in the order the balls are numbered: one at
/// the middle when the group is off, else the group's shape, turned by its Turn.
pub fn offsets(s: &BallsSettings) -> Vec<Vec2> {
    if !s.group {
        return vec![Vec2::ZERO];
    }
    let step = s.step();
    let count = (s.count.max(1) as usize).min(MAX_BALLS);
    let mut out = match s.shape {
        GroupShape::Grid => grid(count, s.columns, step),
        GroupShape::Line => line(count, step),
        GroupShape::Ring => ring(count, s.ring_radius),
        GroupShape::FilledRing => filled_ring(s.ring_radius, step),
        GroupShape::FilledCircle => filled_circle(count, step),
        GroupShape::Triangle => triangle(count, step),
        GroupShape::Honeycomb => honeycomb(count, s.columns, step),
        GroupShape::Spiral => spiral(count, step),
        GroupShape::Heart => heart(count, step),
        GroupShape::Scatter => scatter(count, step, s.area, s.seed),
        GroupShape::Word => word(&s.word, s.letter_height, step),
        GroupShape::Custom => s.custom.iter().take(MAX_BALLS).map(|p| Vec2::from_array(*p)).collect(),
    };
    // A word with no letters or a custom shape with no places still has its one ball.
    if out.is_empty() {
        out.push(Vec2::ZERO);
    }
    if s.turn != 0.0 {
        let turn = Vec2::from_angle(s.turn.to_radians());
        out.iter_mut().for_each(|p| *p = turn.rotate(*p));
    }
    out
}

/// Rows of `columns`, row by row from the top, centred on the middle; a part-filled last row is
/// left-aligned. With `count` a whole number of rows this is exactly the old Block.
pub fn grid(count: usize, columns: u32, step: f32) -> Vec<Vec2> {
    let columns = (columns.max(1) as usize).min(count.max(1));
    let rows = count.div_ceil(columns);
    let size = Vec2::new((columns - 1) as f32, (rows - 1) as f32) * step;
    let origin = -size * 0.5;
    (0..count).map(|i| origin + Vec2::new((i % columns) as f32, (i / columns) as f32) * step).collect()
}

/// One row along x, `step` apart, centred.
pub fn line(count: usize, step: f32) -> Vec<Vec2> {
    let half = (count - 1) as f32 * 0.5;
    (0..count).map(|i| Vec2::new((i as f32 - half) * step, 0.0)).collect()
}

/// Clockwise from the top, `radius` from the middle: exactly the old Ring.
pub fn ring(count: usize, radius: f32) -> Vec<Vec2> {
    (0..count)
        .map(|i| {
            let angle = -FRAC_PI_2 + TAU * i as f32 / count as f32;
            Vec2::new(angle.cos(), angle.sin()) * radius
        })
        .collect()
}

/// Rings of balls inside one another, numbered from the middle out: the outer ring `radius` from
/// the middle and one every `step` inward, each holding as many balls as fit with no two closer
/// than `step`, clockwise from the top. A ring smaller than half a step is one ball in the middle.
/// At most [`MAX_BALLS`], the outer rings dropped first.
pub fn filled_ring(radius: f32, step: f32) -> Vec<Vec2> {
    let step = step.max(1e-3);
    let radius = if radius.is_finite() { radius.max(0.0) } else { 0.0 };
    let mut out = Vec::new();
    let mut r = radius % step;
    // From the middle out, so the numbering and the cap start at the middle.
    if r < step * 0.5 {
        out.push(Vec2::ZERO);
        r += step;
    }
    while r <= radius + step * 1e-3 && out.len() < MAX_BALLS {
        // The most balls whose neighbours (a chord apart) are at least a step apart.
        let n = ((std::f32::consts::PI / (step / (2.0 * r)).min(1.0).asin()).floor() as usize).max(1);
        if n > MAX_BALLS - out.len() {
            break;
        }
        out.extend(ring(n, r));
        r += step;
    }
    out
}

/// Packed like a honeycomb outward from the middle, nearest first, so the group is as round as
/// the count allows.
pub fn filled_circle(count: usize, step: f32) -> Vec<Vec2> {
    let row = step * 3f32.sqrt() * 0.5;
    // Rings of the lattice out far enough to hold `count`, with one spare.
    let reach = ((count as f32 / 3.0).sqrt() + 2.0).ceil() as i32;
    let mut points = Vec::new();
    for r in -reach..=reach {
        for q in -reach..=reach {
            let x = (q as f32 + 0.5 * r as f32) * step;
            points.push(Vec2::new(x, r as f32 * row));
        }
    }
    let angle = |p: &Vec2| (p.x.atan2(-p.y) + TAU) % TAU;
    points.sort_by(|a, b| a.length_squared().total_cmp(&b.length_squared()).then(angle(a).total_cmp(&angle(b))));
    points.truncate(count);
    points
}

/// Rows of 1, 2, 3… balls pointing up, nested like a honeycomb, the whole triangle centred; a
/// part-filled last row starts at the left.
pub fn triangle(count: usize, step: f32) -> Vec<Vec2> {
    let row_height = step * 3f32.sqrt() * 0.5;
    let mut out = Vec::with_capacity(count);
    let mut row = 0usize;
    while out.len() < count {
        let n = (row + 1).min(count - out.len());
        let half = row as f32 * 0.5;
        out.extend((0..n).map(|i| Vec2::new((i as f32 - half) * step, row as f32 * row_height)));
        row += 1;
    }
    centred(out)
}

/// A grid whose odd rows sit half a step over, rows close enough that the balls nest.
pub fn honeycomb(count: usize, columns: u32, step: f32) -> Vec<Vec2> {
    let columns = (columns.max(1) as usize).min(count.max(1));
    let row_height = step * 3f32.sqrt() * 0.5;
    let out = (0..count)
        .map(|i| {
            let (c, r) = (i % columns, i / columns);
            let shift = if r % 2 == 1 { 0.5 } else { 0.0 };
            Vec2::new((c as f32 + shift) * step, r as f32 * row_height)
        })
        .collect();
    centred(out)
}

/// A sunflower spiral out from the middle, each ball the golden angle round from the last.
pub fn spiral(count: usize, step: f32) -> Vec<Vec2> {
    let golden = TAU * (1.0 - 1.0 / ((1.0 + 5f32.sqrt()) * 0.5));
    (0..count)
        .map(|i| {
            let r = step * SPIRAL_SPREAD * (i as f32 + SPIRAL_START).sqrt();
            Vec2::from_angle(i as f32 * golden - FRAC_PI_2) * r
        })
        .collect()
}

/// The heart curve's point at `t` (0 to 2π), y down, about 32 units across.
fn heart_at(t: f32) -> Vec2 {
    let x = 16.0 * t.sin().powi(3);
    let y = 13.0 * t.cos() - 5.0 * (2.0 * t).cos() - 2.0 * (3.0 * t).cos() - (4.0 * t).cos();
    Vec2::new(x, -y)
}

/// Round the outline of a heart, evenly spaced along it, sized so no two balls are closer than
/// `step` (across the dip at the top they would be, sized by the spacing along the outline).
pub fn heart(count: usize, step: f32) -> Vec<Vec2> {
    const SAMPLES: usize = 4096;
    let points: Vec<Vec2> = (0..=SAMPLES).map(|i| heart_at(TAU * i as f32 / SAMPLES as f32)).collect();
    let mut along = Vec::with_capacity(points.len());
    let mut length = 0.0;
    along.push(0.0);
    for w in points.windows(2) {
        length += w[0].distance(w[1]);
        along.push(length);
    }
    let mut out = Vec::with_capacity(count);
    let mut j = 0;
    for k in 0..count {
        let want = length * k as f32 / count as f32;
        while j + 1 < along.len() - 1 && along[j + 1] < want {
            j += 1;
        }
        let span = (along[j + 1] - along[j]).max(1e-6);
        out.push(points[j].lerp(points[j + 1], ((want - along[j]) / span).clamp(0.0, 1.0)));
    }
    // The closest pair, sweeping the places from left to right: only pairs nearer across than
    // the closest so far are measured.
    let mut closest = length / count as f32;
    let mut by_x = out.clone();
    by_x.sort_by(|a, b| a.x.total_cmp(&b.x));
    for (i, a) in by_x.iter().enumerate() {
        for b in &by_x[i + 1..] {
            if b.x - a.x >= closest {
                break;
            }
            closest = closest.min(a.distance(*b));
        }
    }
    let scale = step / closest.max(1e-6);
    centred(out.into_iter().map(|p| p * scale).collect())
}

/// Random places in an `area`-sized box round the middle, never closer than `step` to another;
/// a ball that finds no room in [`SCATTER_TRIES`] tries is left out.
pub fn scatter(count: usize, step: f32, area: Vec2, seed: i32) -> Vec<Vec2> {
    let mut rng = Rng::new(seed as u32, 0x5ca7);
    let half = area.abs() * 0.5;
    let mut out: Vec<Vec2> = Vec::with_capacity(count);
    let min = step.max(0.0);
    // The places so far by square cells `min` across, so a try only looks at its neighbours.
    let cell = |p: Vec2| ((p.x / min).floor() as i64, (p.y / min).floor() as i64);
    let mut cells: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for _ in 0..count {
        for _ in 0..SCATTER_TRIES {
            let p = Vec2::new(rng.range(-half.x, half.x), rng.range(-half.y, half.y));
            let free = min <= 0.0 || {
                let (cx, cy) = cell(p);
                (-1..=1).all(|dx| {
                    (-1..=1).all(|dy| {
                        cells
                            .get(&(cx + dx, cy + dy))
                            .is_none_or(|near| near.iter().all(|&i| out[i].distance_squared(p) >= min * min))
                    })
                })
            };
            if free {
                if min > 0.0 {
                    cells.entry(cell(p)).or_default().push(out.len());
                }
                out.push(p);
                break;
            }
        }
    }
    out
}

/// The word in capitals `letter_height` tall, filled with balls on a square lattice `step`
/// apart, centred. No letters (only spaces), no balls.
pub fn word(text: &str, letter_height: f32, step: f32) -> Vec<Vec2> {
    type Key = (String, u32, u32);
    static CACHE: OnceLock<Mutex<HashMap<Key, Vec<Vec2>>>> = OnceLock::new();
    let key = (text.to_owned(), letter_height.to_bits(), step.to_bits());
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return hit;
    }
    let out = spell(text, letter_height, step);
    if let Ok(mut c) = cache.lock() {
        if c.len() > 64 {
            c.clear();
        }
        c.insert(key, out.clone());
    }
    out
}

/// The outlines of `text`'s letters as closed polygons (y down), capitals `letter_height` tall.
fn letter_outlines(text: &str, letter_height: f32) -> Vec<Vec<Vec2>> {
    letter_outlines_in(FONT, text, letter_height)
}

/// [`letter_outlines`] in the font `font` (a TrueType file's bytes).
pub(crate) fn letter_outlines_in(font: &[u8], text: &str, letter_height: f32) -> Vec<Vec<Vec2>> {
    use skrifa::MetadataProvider;
    use skrifa::instance::{LocationRef, Size};
    use skrifa::outline::DrawSettings;
    let Ok(font) = skrifa::FontRef::new(font) else { return Vec::new() };
    let unscaled = font.metrics(Size::unscaled(), LocationRef::default());
    let em = unscaled.units_per_em as f32;
    let cap = unscaled.cap_height.filter(|c| *c > 0.0).unwrap_or(0.7 * em);
    let ppem = letter_height.max(1.0) * em / cap;
    let size = Size::new(ppem);
    let advances = font.glyph_metrics(size, LocationRef::default());
    let charmap = font.charmap();
    let glyphs = font.outline_glyphs();
    let mut pen = Pen::default();
    let mut x = 0.0;
    for ch in text.chars() {
        let Some(gid) = charmap.map(ch) else { continue };
        pen.offset = x;
        if let Some(glyph) = glyphs.get(gid) {
            let _ = glyph.draw(DrawSettings::unhinted(size, LocationRef::default()), &mut pen);
        }
        x += advances.advance_width(gid).unwrap_or(0.0);
    }
    skrifa::outline::OutlinePen::close(&mut pen);
    pen.shapes
}

/// [`word`] without the cache.
fn spell(text: &str, letter_height: f32, step: f32) -> Vec<Vec2> {
    let shapes = letter_outlines(text, letter_height);
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for p in shapes.iter().flatten() {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    if shapes.is_empty() || lo.x > hi.x {
        return Vec::new();
    }
    let step = step.max(1.0);
    // The lattice is centred on the letters, so a symmetric word gets a symmetric filling.
    let cells = ((hi - lo) / step).floor();
    let start = (lo + hi) * 0.5 - cells * step * 0.5;
    let mut out = Vec::new();
    for j in 0..=cells.y as i32 {
        for i in 0..=cells.x as i32 {
            let p = start + Vec2::new(i as f32, j as f32) * step;
            if winding(&shapes, p) != 0 {
                out.push(p);
                if out.len() >= MAX_BALLS {
                    return centred_on(out, (lo + hi) * 0.5);
                }
            }
        }
    }
    centred_on(out, (lo + hi) * 0.5)
}

/// How many times the outlines wind round `p` (non-zero: inside a letter).
pub(crate) fn winding(shapes: &[Vec<Vec2>], p: Vec2) -> i32 {
    let mut w = 0;
    for shape in shapes {
        for (i, a) in shape.iter().enumerate() {
            let b = shape[(i + 1) % shape.len()];
            if a.y <= p.y {
                if b.y > p.y && (b - *a).perp_dot(p - *a) > 0.0 {
                    w += 1;
                }
            } else if b.y <= p.y && (b - *a).perp_dot(p - *a) < 0.0 {
                w -= 1;
            }
        }
    }
    w
}

/// Flattens glyph outlines into polygons, y flipped to point down.
#[derive(Default)]
struct Pen {
    offset: f32,
    shapes: Vec<Vec<Vec2>>,
    current: Vec<Vec2>,
}

impl Pen {
    fn at(&self, x: f32, y: f32) -> Vec2 {
        Vec2::new(x + self.offset, -y)
    }
    fn last(&self) -> Vec2 {
        self.current.last().copied().unwrap_or(Vec2::ZERO)
    }
}

impl skrifa::outline::OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        let p = self.at(x, y);
        self.current.push(p);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.at(x, y);
        self.current.push(p);
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (a, c, b) = (self.last(), self.at(cx0, cy0), self.at(x, y));
        for k in 1..=8 {
            let t = k as f32 / 8.0;
            self.current.push(a.lerp(c, t).lerp(c.lerp(b, t), t));
        }
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (a, c0, c1, b) = (self.last(), self.at(cx0, cy0), self.at(cx1, cy1), self.at(x, y));
        for k in 1..=12 {
            let t = k as f32 / 12.0;
            let (p, q, r) = (a.lerp(c0, t), c0.lerp(c1, t), c1.lerp(b, t));
            self.current.push(p.lerp(q, t).lerp(q.lerp(r, t), t));
        }
    }
    fn close(&mut self) {
        if self.current.len() > 2 {
            self.shapes.push(std::mem::take(&mut self.current));
        } else {
            self.current.clear();
        }
    }
}

/// `points` moved so the middle of their bounds is at the origin.
fn centred(points: Vec<Vec2>) -> Vec<Vec2> {
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for p in &points {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    centred_on(points, (lo + hi) * 0.5)
}

fn centred_on(mut points: Vec<Vec2>, middle: Vec2) -> Vec<Vec2> {
    if middle.is_finite() {
        points.iter_mut().for_each(|p| *p -= middle);
    }
    points
}

/// The middle of `points`' bounds and how far they reach from it, as (low, high) corners.
pub fn bounds(points: &[Vec2]) -> (Vec2, Vec2) {
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for p in points {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }
    if lo.x > hi.x { (Vec2::ZERO, Vec2::ZERO) } else { (lo, hi) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scatter_places_the_same_balls_as_checking_every_one() {
        let slow = |count: usize, step: f32, area: Vec2, seed: i32| {
            let mut rng = Rng::new(seed as u32, 0x5ca7);
            let half = area.abs() * 0.5;
            let mut out: Vec<Vec2> = Vec::new();
            for _ in 0..count {
                for _ in 0..SCATTER_TRIES {
                    let p = Vec2::new(rng.range(-half.x, half.x), rng.range(-half.y, half.y));
                    if out.iter().all(|q| q.distance_squared(p) >= step * step) {
                        out.push(p);
                        break;
                    }
                }
            }
            out
        };
        for (count, step, seed) in [(300, 20.0, 1), (500, 7.5, 9), (40, 0.0, 3)] {
            let area = Vec2::new(300.0, 200.0);
            assert_eq!(scatter(count, step, area, seed), slow(count, step.max(0.0), area, seed));
        }
    }

    fn settings(shape: GroupShape, f: impl FnOnce(&mut BallsSettings)) -> BallsSettings {
        let mut s = BallsSettings { group: true, shape, ..BallsSettings::default() };
        f(&mut s);
        s
    }

    /// The old Block arrangement, as it was written.
    fn old_block(columns: u32, rows: u32, spacing: f32) -> Vec<Vec2> {
        let size = Vec2::new((columns - 1) as f32, (rows - 1) as f32) * spacing;
        let origin = -size * 0.5;
        let mut out = Vec::new();
        for row in 0..rows {
            for column in 0..columns {
                out.push(origin + Vec2::new(column as f32, row as f32) * spacing);
            }
        }
        out
    }

    fn closest(points: &[Vec2]) -> f32 {
        let mut best = f32::MAX;
        for (i, a) in points.iter().enumerate() {
            for b in &points[i + 1..] {
                best = best.min(a.distance(*b));
            }
        }
        best
    }

    #[test]
    fn off_is_one_ball_at_the_middle() {
        assert_eq!(offsets(&BallsSettings::default()), [Vec2::ZERO]);
    }

    #[test]
    fn a_grid_is_the_old_block_bit_for_bit() {
        for (columns, rows, spacing, radius) in
            [(2, 3, 10.0, 3.0), (8, 8, 46.0, 19.0), (28, 28, 19.0, 4.5), (5, 1, 95.0, 70.0)]
        {
            let mut s = BallsSettings { radius, ..BallsSettings::default() };
            s.block(columns, rows, spacing);
            let new = offsets(&s);
            let old = old_block(columns, rows, spacing);
            assert_eq!(new.len(), old.len());
            for (a, b) in new.iter().zip(&old) {
                assert_eq!((a.x.to_bits(), a.y.to_bits()), (b.x.to_bits(), b.y.to_bits()), "{columns}x{rows}");
            }
        }
    }

    #[test]
    fn a_part_filled_last_row_is_left_aligned() {
        let p = grid(5, 3, 10.0);
        assert_eq!(
            p,
            [
                Vec2::new(-10.0, -5.0),
                Vec2::new(0.0, -5.0),
                Vec2::new(10.0, -5.0),
                Vec2::new(-10.0, 5.0),
                Vec2::new(0.0, 5.0)
            ]
        );
    }

    /// Space between well below minus twice the Size still keeps neighbours a little apart: a
    /// filled ring spreads out from the middle instead of stacking thousands in a dot.
    #[test]
    fn a_very_negative_space_still_steps_forward() {
        let s = settings(GroupShape::FilledRing, |s| {
            s.radius = 24.0;
            s.space = -100.0;
        });
        assert!(s.step() > 0.0, "{}", s.step());
        let p = offsets(&s);
        assert!(closest(&p) >= s.step() * 0.999);
        let furthest = p.iter().map(|q| q.length()).fold(0.0, f32::max);
        assert!(furthest > 50.0, "{} balls within {furthest} px", p.len());
        let tri = offsets(&settings(GroupShape::Triangle, |s| {
            s.count = 6;
            s.space = -100.0;
        }));
        // The point of the triangle stays at the top.
        let top = tri.iter().map(|q| q.y).fold(f32::MAX, f32::min);
        assert_eq!(tri[0].y, top, "{tri:?}");
    }

    #[test]
    fn a_ring_is_the_old_ring() {
        let mut s = BallsSettings::default();
        s.ring(4, 10.0);
        let p = offsets(&s);
        assert_eq!(p.len(), 4);
        assert!((p[0] - Vec2::new(0.0, -10.0)).length() < 1e-4 && (p[1] - Vec2::new(10.0, 0.0)).length() < 1e-4);
    }

    #[test]
    fn a_filled_ring_fills_the_ring_to_the_middle_with_room_between() {
        for (radius, ball, space) in [(200.0, 24.0, 8.0), (100.0, 10.0, 0.0), (37.0, 5.0, 3.0), (5.0, 24.0, 8.0)] {
            let s = settings(GroupShape::FilledRing, |s| {
                s.ring_radius = radius;
                s.radius = ball;
                s.space = space;
            });
            let p = offsets(&s);
            let step = s.step();
            assert!(!p.is_empty());
            if p.len() > 1 {
                assert!(closest(&p) >= step * 0.999, "{radius}: neighbours closer than a step");
            }
            let furthest = p.iter().map(|q| q.length()).fold(0.0, f32::max);
            assert!((furthest - radius).abs() < 1e-3 || p.len() == 1, "the outer ring sits at Radius");
            // Every gap inside the outer ring is filled: each point of the disc is within a step
            // of a ball.
            for k in 0..200 {
                let at = Vec2::from_angle(k as f32 * 0.7) * radius * (k as f32 / 200.0);
                let near = p.iter().map(|q| q.distance(at)).fold(f32::MAX, f32::min);
                assert!(near <= step * 1.05, "{radius}: a hole at {at}");
            }
            assert_eq!(p, offsets(&s), "the same every time");
        }
        // Numbered from the middle out: a ball in the middle when the rings reach it, else the
        // innermost ring's top ball.
        let four_steps = settings(GroupShape::FilledRing, |s| s.ring_radius = 4.0 * s.step());
        assert_eq!(offsets(&four_steps)[0], Vec2::ZERO);
        let s = settings(GroupShape::FilledRing, |s| s.ring_radius = 200.0);
        assert!((offsets(&s)[0] - Vec2::new(0.0, -(200.0 % s.step()))).length() < 1e-3);
    }

    #[test]
    fn every_counted_shape_places_how_many_it_is_asked_to_with_room_between() {
        for shape in [
            GroupShape::Grid,
            GroupShape::Line,
            GroupShape::Ring,
            GroupShape::FilledCircle,
            GroupShape::Triangle,
            GroupShape::Honeycomb,
            GroupShape::Spiral,
            GroupShape::Heart,
        ] {
            for count in [1, 2, 7, 30, 101] {
                let s = settings(shape, |s| s.count = count);
                let p = offsets(&s);
                assert_eq!(p.len(), count as usize, "{shape:?} {count}");
                // A ring keeps its radius however many.
                if count > 1 && shape != GroupShape::Ring {
                    let gap = closest(&p);
                    assert!(gap >= 0.9 * s.step(), "{shape:?} {count}: neighbours {gap} apart, step {}", s.step());
                }
            }
        }
    }

    #[test]
    fn shapes_sit_round_the_middle() {
        for shape in [GroupShape::Triangle, GroupShape::Honeycomb, GroupShape::Heart, GroupShape::Line] {
            let (lo, hi) = bounds(&offsets(&settings(shape, |_| {})));
            assert!(((lo + hi) * 0.5).length() < 1e-3, "{shape:?}: {lo} {hi}");
        }
    }

    #[test]
    fn scatter_is_the_same_every_time_and_keeps_its_distance() {
        let s = settings(GroupShape::Scatter, |s| s.count = 60);
        let a = offsets(&s);
        assert_eq!(a, offsets(&s));
        assert!(closest(&a) >= s.step());
        assert!(a.iter().all(|p| p.x.abs() <= 300.0 && p.y.abs() <= 300.0));
        let other = offsets(&settings(GroupShape::Scatter, |s| {
            s.count = 60;
            s.seed = 2;
        }));
        assert_ne!(a, other, "another seed, another pick");
        // Too many for the area: some are left out.
        let crowded = offsets(&settings(GroupShape::Scatter, |s| {
            s.count = 500;
            s.area = Vec2::new(200.0, 200.0);
        }));
        assert!(crowded.len() < 500 && !crowded.is_empty());
    }

    #[test]
    fn a_word_fills_its_letters() {
        let i = offsets(&settings(GroupShape::Word, |s| {
            s.word = "I".into();
            s.radius = 5.0;
            s.space = 0.0;
        }));
        assert!(i.len() > 5, "{}", i.len());
        let (lo, hi) = bounds(&i);
        assert!(hi.y - lo.y > 2.0 * (hi.x - lo.x), "an I is a tall column: {lo} {hi}");
        assert!((hi.y - lo.y - 240.0).abs() <= 20.0, "letter height {}", hi.y - lo.y);
        let balls = offsets(&settings(GroupShape::Word, |s| {
            s.radius = 5.0;
            s.space = 0.0;
        }));
        assert!(balls.len() > 3 * i.len(), "BALLS has more to fill than I");
        assert_eq!(offsets(&settings(GroupShape::Word, |s| s.word = "   ".into())), [Vec2::ZERO], "no letters");
    }

    #[test]
    fn custom_places_are_the_points_and_none_is_one_ball() {
        let s = settings(GroupShape::Custom, |s| s.custom = vec![[1.0, 2.0], [-5.0, 0.0]]);
        assert_eq!(offsets(&s), [Vec2::new(1.0, 2.0), Vec2::new(-5.0, 0.0)]);
        assert_eq!(offsets(&settings(GroupShape::Custom, |_| {})), [Vec2::ZERO]);
    }

    #[test]
    fn turn_turns_the_whole_shape() {
        let s = settings(GroupShape::Line, |s| {
            s.count = 2;
            s.turn = 90.0;
        });
        let p = offsets(&s);
        let half = s.step() * 0.5;
        assert!(
            (p[0] - Vec2::new(0.0, -half)).length() < 1e-3 && (p[1] - Vec2::new(0.0, half)).length() < 1e-3,
            "{p:?}"
        );
    }
}
