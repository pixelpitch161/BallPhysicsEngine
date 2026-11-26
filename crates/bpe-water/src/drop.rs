//! A drop, and where drops are born: a fill (a lattice of four drops a cell) and a pour.

use bpe_core::maths::Maths;
use bytemuck::{Pod, Zeroable};
use glam::Vec2;

use crate::solid::{Layout, SolidField};

/// The bit of [`Drop::kind`] that makes a drop stop at the course's edge.
pub const STOPS: u32 = 256;
/// The bit of [`Drop::kind`] that makes a drop air (part 2c): trapped air under the water, which
/// takes up room as the liquid does but falls upward and bursts at the top. It is not drawn.
pub const AIR: u32 = 128;

/// One drop of liquid, in grid units. Matches `Drop` in common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Drop {
    pub x: [f32; 2],
    pub v: [f32; 2],
    /// The APIC matrix: xx, xy, yx, yy.
    pub c: [f32; 4],
    /// How stretched the liquid at it is (J - 1).
    pub e: f32,
    /// Bits 0 to 3 its liquid, bit 7 ([`AIR`]) it is air, bit 8 ([`STOPS`]) it stops at the
    /// course's edge.
    pub kind: u32,
    pub serial: u32,
    /// rgba8, linear.
    pub colour: u32,
}

impl Drop {
    pub fn new(x: Vec2, v: Vec2, kind: u32, serial: u32, colour: u32) -> Drop {
        Drop { x: x.to_array(), v: v.to_array(), c: [0.0; 4], e: 0.0, kind, serial, colour }
    }

    pub fn liquid(&self) -> usize {
        (self.kind & 15) as usize
    }

    /// Whether it is trapped air, not liquid.
    pub fn is_air(&self) -> bool {
        self.kind & AIR != 0
    }
}

/// The colour drops are born with when nothing says otherwise (a water blue, linear rgba8).
pub const WATER: u32 = u32::from_le_bytes([8, 51, 184, 255]);

/// A fill: four drops a cell on a lattice over the world rectangle `lo` to `hi`, wherever a drop
/// may be (half a cell clear of every solid), bottom row first.
pub fn fill(layout: &Layout, field: &SolidField, lo: Vec2, hi: Vec2, kind: u32, first_serial: u32) -> Vec<Drop> {
    let (g_lo, g_hi) = (layout.to_grid(lo), layout.to_grid(hi));
    let (c0, c1) = ((g_lo.x * 2.0).ceil() as i64, (g_hi.x * 2.0).floor() as i64);
    let (r0, r1) = ((g_lo.y * 2.0).ceil() as i64, (g_hi.y * 2.0).floor() as i64);
    let mut out = Vec::new();
    for r in (r0..r1).rev() {
        for c in c0..c1 {
            let g = Vec2::new(c as f32 * 0.5 + 0.25, r as f32 * 0.5 + 0.25);
            if field.at(layout.to_world(g)) < 0.5 * layout.cell {
                continue;
            }
            out.push(Drop::new(g, Vec2::ZERO, kind, first_serial.wrapping_add(out.len() as u32), WATER));
        }
    }
    out
}

/// How far below a fill's foot a face may be for the fill to be set down on it, pixels (or two
/// cells, when more): a fill drawn a little above a floor starts on it, one in mid-air falls.
pub const FILL_SNAP: f32 = 16.0;

/// A fill for the all-drops engine: the drops' resting lattice (half a cell apart) over the world
/// rectangle `lo` to `hi`, bottom row first. A column whose foot is just above a face (within
/// [`FILL_SNAP`] pixels or two cells) is moved down onto it, as many drops as the rectangle holds,
/// its lowest drop `rest_off` cells off the face; a column over nothing near (in mid-air, over an
/// opening) stays in the rectangle; a column whose foot is inside a solid starts on that solid's
/// face and goes up to the rectangle's top. A drop is never placed nearer a solid than `rest_off`.
///
/// Across, a side of the rectangle that ends inside a solid (along its middle row) is snug: its
/// column sits `rest_off` off the solid's face, and when both sides are snug the columns are spread
/// evenly between them, a little more than half a cell apart. (On the lattice, a gap of up to half a
/// cell was left at a side wall, and the first drop beside it was thrown up the wall.) A free side
/// keeps the lattice's spacing from the other.
pub fn fill_stacked(layout: &Layout, field: &SolidField, lo: Vec2, hi: Vec2, kind: u32, rest_off: f32) -> Vec<Drop> {
    let (g_lo, g_hi) = (layout.to_grid(lo), layout.to_grid(hi));
    // The rows the rectangle holds, half a cell each.
    let held = ((g_hi.y - g_lo.y).max(0.0) * 2.0).floor() as usize;
    // How far down a column's foot looks for a face, in cells.
    let snap = (FILL_SNAP / layout.cell).max(2.0);
    // How far a grid point is from the solids, in cells, less how far a drop rests off them.
    let clear = |g: Vec2| field.at(layout.to_world(g)) / layout.cell - rest_off;
    let step = 0.05;
    let mut columns: Vec<(f32, Vec<f32>)> = Vec::new();
    for x in fill_columns(&clear, g_lo, g_hi, step) {
        // The column's lowest drop and how many it holds.
        let (bottom, rows) = if clear(Vec2::new(x, g_hi.y)) >= 0.0 {
            // The foot is clear: onto the face just below, else a quarter cell in from the foot.
            let mut y = g_hi.y;
            while y < g_hi.y + snap && clear(Vec2::new(x, y + step)) >= 0.0 {
                y += step;
            }
            let bottom = if y < g_hi.y + snap { settle_on(&clear, x, y, y + step) } else { g_hi.y - 0.25 };
            (bottom, held)
        } else {
            // The foot is in a solid: up onto its face, and up to the rectangle's top from there.
            let mut y = g_hi.y;
            while y > g_lo.y && clear(Vec2::new(x, y - step)) < 0.0 {
                y -= step;
            }
            if y <= g_lo.y {
                continue;
            }
            let bottom = settle_on(&clear, x, y - step, y);
            (bottom, ((bottom - g_lo.y) * 2.0).floor() as usize + 1)
        };
        let ys = (0..rows).map(|k| bottom - 0.5 * k as f32).filter(|&y| clear(Vec2::new(x, y)) >= -1e-4).collect();
        columns.push((x, ys));
    }
    // Bottom row first: by how far each drop is above its column's lowest.
    let rows = columns.iter().map(|(_, ys)| ys.len()).max().unwrap_or(0);
    let mut out = Vec::new();
    for r in 0..rows {
        for (x, ys) in &columns {
            if let Some(y) = ys.get(r) {
                out.push(Drop::new(Vec2::new(*x, *y), Vec2::ZERO, kind, out.len() as u32, WATER));
            }
        }
    }
    out
}

/// The fill's columns across, grid x, left to right (see [`fill_stacked`]): `clear` is a grid point's
/// clearance, the rectangle `g_lo` to `g_hi` in grid cells, `step` how finely an end is looked for.
fn fill_columns(clear: &impl Fn(Vec2) -> f32, g_lo: Vec2, g_hi: Vec2, step: f32) -> Vec<f32> {
    let mid = 0.5 * (g_lo.y + g_hi.y);
    let row = |x: f32| clear(Vec2::new(x, mid));
    // A side's snug column: from its end, in a solid, inwards to the first clear point, and back to
    // where the clearance is nothing.
    let snug = |from: f32, toward: f32| -> Option<f32> {
        if row(from) >= 0.0 {
            return None;
        }
        let way = if toward > from { step } else { -step };
        let mut x = from;
        while (toward - x) * way > 0.0 && row(x) < 0.0 {
            x += way;
        }
        if row(x) < 0.0 {
            return None;
        }
        Some(edge_between(&row, x, x - way))
    };
    // (The lattice's own: a column a half cell's band, the band within the rectangle.)
    let lattice = || {
        let (c0, c1) = ((g_lo.x * 2.0).ceil() as i64, (g_hi.x * 2.0).floor() as i64);
        (c0..c1).map(|c| c as f32 * 0.5 + 0.25).collect::<Vec<f32>>()
    };
    match (snug(g_lo.x, g_hi.x), snug(g_hi.x, g_lo.x)) {
        (Some(a), Some(b)) if b >= a => {
            let gaps = ((b - a) / 0.5).floor() as usize;
            if gaps == 0 {
                return vec![0.5 * (a + b)];
            }
            (0..=gaps).map(|k| a + (b - a) * k as f32 / gaps as f32).collect()
        }
        (Some(a), None) => (0..).map(|k| a + 0.5 * k as f32).take_while(|x| x + 0.25 <= g_hi.x).collect(),
        (None, Some(b)) => {
            let mut xs: Vec<f32> = (0..).map(|k| b - 0.5 * k as f32).take_while(|x| x - 0.25 >= g_lo.x).collect();
            xs.reverse();
            xs
        }
        _ => lattice(),
    }
}

/// Where between `clear_at` (clear of the solids) and `solid_at` (not) the clearance `clear` is
/// nothing, to a ten-thousandth of a cell (the field is straight between its nodes).
fn edge_between(clear: &impl Fn(f32) -> f32, clear_at: f32, solid_at: f32) -> f32 {
    let (mut a, mut b) = (clear_at, solid_at);
    for _ in 0..24 {
        let m = 0.5 * (a + b);
        if clear(m) >= 0.0 {
            a = m;
        } else {
            b = m;
        }
    }
    a
}

/// Where between `clear_at` (clear of the solids) and `solid_at` (not) the clearance in column
/// `x` is nothing.
fn settle_on(clear: &impl Fn(Vec2) -> f32, x: f32, clear_at: f32, solid_at: f32) -> f32 {
    edge_between(&|y| clear(Vec2::new(x, y)), clear_at, solid_at)
}

/// A pour: a spout that makes as many drops a second as its width times its speed fills.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pour {
    /// The spout's middle, world pixels.
    pub place: Vec2,
    /// Which way it pours, degrees (90 is straight down the screen).
    pub aim: f32,
    /// The stream's width, pixels.
    pub width: f32,
    /// Pixels a second.
    pub speed: f32,
    /// How far the drops' ways fan out, degrees.
    pub spread: f32,
    /// When it starts and stops, seconds.
    pub from: f32,
    pub until: f32,
    pub kind: u32,
    pub colour: u32,
}

impl Default for Pour {
    fn default() -> Self {
        Pour {
            place: Vec2::ZERO,
            aim: 90.0,
            width: 40.0,
            speed: 300.0,
            spread: 0.0,
            from: 0.0,
            until: f32::MAX,
            kind: STOPS,
            colour: WATER,
        }
    }
}

/// What a pour still owes and the serial of its next drop. For the all-drops engine's rows
/// ([`Pour::rows`]): `owed` is how far (pixels) the stream has run since its last row, `born` how
/// many drops it has made, and `extra` the share of a drop its rows still owe across.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Poured {
    pub owed: f32,
    pub born: u32,
    pub extra: f32,
}

impl Pour {
    /// The most drops a pour makes in a liquid step.
    pub const MOST_A_STEP: u32 = 20_000;

    /// The spacing of drops at rest on `layout` (pixels): half a cell.
    fn spacing(layout: &Layout) -> f64 {
        f64::from(layout.cell) * 0.5
    }

    /// The stream's width in drops (columns; under 1 when it is narrower than a drop).
    fn columns(&self, layout: &Layout) -> f64 {
        f64::from(self.width.max(0.0)) / Pour::spacing(layout)
    }

    /// How far apart (pixels, along the way) its rows are born: the resting spacing, or more for a
    /// stream narrower than a drop, so it still pours its amount as single drops with gaps.
    pub fn pitch(&self, layout: &Layout) -> f32 {
        let columns = self.columns(layout);
        let s = Pour::spacing(layout);
        (if columns >= 1.0 { s } else { s / columns.max(1e-6) }) as f32
    }

    /// How far the stream runs (pixels) in the part of the step from `from` to `from + dt`
    /// seconds that falls between its Starts and Stops.
    fn travel(&self, from: f64, dt: f64) -> f64 {
        let t0 = from.max(f64::from(self.from));
        let t1 = (from + dt).min(f64::from(self.until));
        if t1.is_nan() || t0.is_nan() || t1 <= t0 || !self.speed.is_finite() {
            return 0.0;
        }
        f64::from(self.speed.max(0.0)) * (t1 - t0)
    }

    /// Whether the stream runs at all in the step from `from` to `from + dt` seconds.
    pub fn runs(&self, from: f64, dt: f64) -> bool {
        from.max(f64::from(self.from)) < (from + dt).min(f64::from(self.until))
    }

    /// This pour's rows for the step from `from` to `from + dt` seconds, for the all-drops engine:
    /// rows across the spout exactly the resting spacing apart, one each time the last has run that
    /// far (its first at the moment it starts), each where a row born at that moment is at the
    /// step's end. A stream whose width is not a whole number of drops has one more drop in some
    /// rows, spread evenly through time, so it pours its width times its speed without squeezing.
    pub fn rows(&self, state: &mut Poured, layout: &Layout, from: f64, dt: f64) -> Vec<Drop> {
        if !self.runs(from, dt) {
            return Vec::new();
        }
        let columns = self.columns(layout);
        if columns <= 0.0 || !columns.is_finite() {
            return Vec::new();
        }
        let s = Pour::spacing(layout);
        let pitch = f64::from(self.pitch(layout));
        let (whole, part) = if columns >= 1.0 { (columns.floor(), columns - columns.floor()) } else { (1.0, 0.0) };
        // (Its first row comes the moment it starts.)
        let first = state.born == 0 && state.owed == 0.0;
        let mut owed = f64::from(state.owed) + self.travel(from, dt) + if first { pitch } else { 0.0 };
        let mut extra = f64::from(state.extra);
        let way = way_at(self.aim);
        let across = way.perp();
        let mut out = Vec::new();
        let most = Pour::MOST_A_STEP as usize;
        while owed >= pitch && out.len() < most {
            owed -= pitch;
            extra += part;
            let m = if extra >= 1.0 {
                extra -= 1.0;
                whole + 1.0
            } else {
                whole
            };
            let along = way * owed as f32;
            for i in 0..m as u32 {
                let u = ((f64::from(i) + 0.5) / m - 0.5) as f32;
                let at = self.place + across * (u * (m * s) as f32) + along;
                let v = self.fanned(u) * self.speed;
                out.push(Drop::new(layout.to_grid(at), v / layout.cell, self.kind, state.born, self.colour));
                state.born = state.born.wrapping_add(1);
            }
        }
        // (A pour that owes more than a step can make lets the rest go.)
        state.owed = owed.min(pitch * most as f64) as f32;
        state.extra = extra as f32;
        out
    }

    /// Which way the stream sets off from the middle of its spout.
    pub fn way(&self) -> Vec2 {
        way_at(self.aim)
    }

    /// The same step while the spout is full: no rows, and what the stream owes stops at `rows`
    /// rows, so a freed spout never bursts.
    pub fn hold(&self, state: &mut Poured, layout: &Layout, from: f64, dt: f64, rows: u32) {
        let pitch = f64::from(self.pitch(layout));
        let first = state.born == 0 && state.owed == 0.0 && self.runs(from, dt);
        let owed = f64::from(state.owed) + self.travel(from, dt) + if first { pitch } else { 0.0 };
        state.owed = owed.min(pitch * f64::from(rows.max(1))) as f32;
    }

    /// The way a drop born at `u` across the spout (-0.5 at one edge, 0.5 at the other) sets off:
    /// the stream fans out smoothly from its middle to `spread` degrees at its edges. (A way
    /// picked at random for each drop is not a fan but drops knocking into each other all the way
    /// down: the stream then lands in pulses, and whatever it feeds comes out in lumps.)
    pub fn fanned(&self, u: f32) -> Vec2 {
        let u = if u.is_finite() { u.clamp(-0.5, 0.5) } else { 0.0 };
        way_at(self.aim + self.spread * 2.0 * u)
    }
}

/// The unit vector at `degrees`, the same on every platform.
fn way_at(degrees: f32) -> Vec2 {
    let (sin, cos) = degrees.to_radians().det_sin_cos();
    Vec2::new(cos, sin)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn degrees(way: Vec2) -> f32 {
        way.y.det_atan2(way.x).to_degrees()
    }

    #[test]
    fn air_is_its_own_bit() {
        let air = Drop::new(Vec2::ZERO, Vec2::ZERO, AIR | STOPS | 3, 0, WATER);
        assert!(air.is_air());
        assert_eq!(air.liquid(), 3);
        assert!(!Drop::new(Vec2::ZERO, Vec2::ZERO, STOPS | 15, 0, WATER).is_air());
        assert_eq!(AIR & (STOPS | 15), 0);
    }

    #[test]
    fn a_pour_fans_out_smoothly() {
        assert_eq!(Pour::default().spread, 0.0, "a pour is one stream unless a Spread is asked for");
        let pour = Pour { aim: 90.0, spread: 10.0, ..Pour::default() };
        for (u, at) in [(-0.5, 80.0), (0.0, 90.0), (0.5, 100.0)] {
            assert!((degrees(pour.fanned(u)) - at).abs() < 1e-3, "{u}: {}", degrees(pour.fanned(u)));
        }
        assert_eq!(pour.fanned(3.0), pour.fanned(0.5), "held to the spout's edge");
        assert_eq!(pour.fanned(f32::NAN), pour.fanned(0.0));

        let layout = Layout { cell: 8.0, cols: 64, rows: 64, origin: Vec2::ZERO };
        let born = |pour: &Pour| pour.rows(&mut Poured::default(), &layout, 0.0, 1.0 / 240.0);
        // No Spread: every drop sets off the same way.
        let spout = Pour { place: Vec2::new(256.0, 64.0), width: 80.0, speed: 1200.0, ..Pour::default() };
        let straight = born(&spout);
        assert!(straight.len() > 10);
        assert!(straight.iter().all(|d| d.v == straight[0].v));
        // A wide Spread from a spout with no width: every way finite and inside the Spread.
        let fan = Pour { spread: 45.0, ..spout };
        assert!(born(&fan).len() > 10);
        for d in born(&Pour { width: 0.0, ..fan }).iter().chain(&born(&fan)) {
            let v = Vec2::from(d.v);
            assert!(v.is_finite());
            assert!(v.length() == 0.0 || (degrees(v) - 90.0).abs() <= 45.0 + 1e-3);
        }
        // The fan is in order across the spout: a drop further along it leans further that way.
        let mut drops = born(&fan);
        drops.sort_by(|a, b| a.x[0].total_cmp(&b.x[0]));
        assert!(drops.windows(2).all(|w| w[0].v[0] <= w[1].v[0] + 1e-6), "left of the spout leans left");
    }

    /// A pour straight down from (100, 50) on a layout of `cell`-pixel cells from the origin.
    fn tap(cell: f32, width: f32) -> (Pour, Layout) {
        let pour = Pour { place: Vec2::new(100.0, 50.0), aim: 90.0, width, speed: 848.528, ..Pour::default() };
        (pour, Layout { cell, cols: 256, rows: 256, origin: Vec2::ZERO })
    }

    /// The pour's rows over `steps` steps of `dt` from the start: how many drops, and how far the
    /// stream ran.
    fn poured(pour: &Pour, layout: &Layout, steps: u32, dt: f64) -> (usize, f64) {
        let mut state = Poured::default();
        let mut drops = 0;
        for k in 0..steps {
            drops += pour.rows(&mut state, layout, f64::from(k) * dt, dt).len();
        }
        (drops, f64::from(pour.speed) * f64::from(steps) * dt)
    }

    #[test]
    fn rows_pour_their_amount() {
        for rate in [1.0, 0.5] {
            for cell in [8.0, 6.0, 3.0, 2.0] {
                let (pour, layout) = tap(cell, 19.2 * rate);
                let s = f64::from(cell) * 0.5;
                // (A thousand rows' run, in steps of 1/960 s.)
                let dt = 1.0 / 960.0;
                let steps = (1000.0 * f64::from(pour.pitch(&layout)) / (f64::from(pour.speed) * dt)).ceil() as u32;
                let (drops, ran) = poured(&pour, &layout, steps, dt);
                let amount = drops as f64 * s * s / (f64::from(pour.width) * ran);
                assert!((amount - 1.0).abs() < 0.02, "rate {rate} at {cell} px: {amount}");
            }
        }
    }

    #[test]
    fn a_thin_tap_pours_its_amount_as_single_drops() {
        let (pour, layout) = tap(8.0, 3.0);
        let mut state = Poured::default();
        let dt = 1.0 / 960.0;
        let mut drops = 0;
        for k in 0..2000 {
            let rows = pour.rows(&mut state, &layout, f64::from(k) * dt, dt);
            // (Each row is one drop, on the spout's middle line.)
            for d in &rows {
                assert!((layout.to_world(Vec2::from(d.x)).x - pour.place.x).abs() < 1e-4, "{:?}", d.x);
            }
            drops += rows.len();
        }
        let ran = f64::from(pour.speed) * 2000.0 * dt;
        let amount = drops as f64 * 16.0 / (f64::from(pour.width) * ran);
        assert!((amount - 1.0).abs() < 0.02, "{amount}");
        assert!(pour.pitch(&layout) > 4.0, "single drops with gaps: {}", pour.pitch(&layout));
    }

    #[test]
    fn a_pour_starts_at_its_time() {
        let (mut pour, layout) = tap(8.0, 12.0);
        pour.from = 0.0123;
        let mut state = Poured::default();
        let dt = 1.0 / 960.0;
        let pitch = f64::from(pour.pitch(&layout));
        let mut rows_before = 0u32;
        for k in 0..60 {
            let from = f64::from(k) * dt;
            let end = from + dt;
            let born = pour.rows(&mut state, &layout, from, dt);
            if end <= f64::from(pour.from) {
                assert!(born.is_empty(), "nothing before it starts: step {k}");
                continue;
            }
            // (Rows of 3 drops, the newest last; each where a row born at its moment is at the
            // step's end.)
            for row in born.chunks(3) {
                let along = f64::from(layout.to_world(Vec2::from(row[0].x)).y - pour.place.y);
                let want = f64::from(pour.speed) * (end - f64::from(pour.from)) - pitch * f64::from(rows_before);
                assert!((along - want).abs() < 1e-4, "step {k}, row {rows_before}: {along} px, not {want}");
                rows_before += 1;
            }
        }
        assert!(rows_before >= 10, "{rows_before}");
    }

    #[test]
    fn rows_are_even() {
        let (pour, layout) = tap(8.0, 19.2);
        let mut state = Poured::default();
        // (A long step: several rows at once.)
        let dt = 1.0 / 60.0;
        for k in 0..4 {
            let born = pour.rows(&mut state, &layout, f64::from(k) * dt, dt);
            let at: Vec<Vec2> = born.iter().map(|d| layout.to_world(Vec2::from(d.x))).collect();
            let mut rows: Vec<Vec<Vec2>> = Vec::new();
            for p in at {
                match rows.last_mut() {
                    Some(row) if (row[0].y - p.y).abs() < 1e-3 => row.push(p),
                    _ => rows.push(vec![p]),
                }
            }
            assert!(rows.len() > 2, "{}", rows.len());
            for row in &rows {
                assert!((4..=5).contains(&row.len()), "{} drops in a row", row.len());
                for w in row.windows(2) {
                    assert!(((w[0].x - w[1].x).abs() - 4.0).abs() < 1e-3, "across: {w:?}");
                }
            }
            for w in rows.windows(2) {
                assert!(((w[0][0].y - w[1][0].y).abs() - 4.0).abs() < 1e-3, "along: {} {}", w[0][0].y, w[1][0].y);
            }
        }
    }

    /// A floor at y = 100 with an opening from x = 40 to 60, and a fill box over it.
    fn floor_with_opening(cell: f32) -> (Layout, SolidField) {
        let layout = Layout::cover(Vec2::new(-100.0, -200.0), Vec2::new(200.0, 300.0), cell);
        let solids = [
            crate::solid::Solid::rect(Vec2::new(-100.0, 100.0), Vec2::new(40.0, 130.0), true, 0.0),
            crate::solid::Solid::rect(Vec2::new(60.0, 100.0), Vec2::new(200.0, 130.0), true, 0.0),
        ];
        (layout, SolidField::build_least(layout, &solids, 1.5 * cell))
    }

    #[test]
    fn a_fill_is_set_down_on_a_floor_but_not_through_an_opening() {
        for cell in [6.0, 3.0] {
            let (layout, field) = floor_with_opening(cell);
            // Drawn 12 px above the floor, the opening under its middle.
            let (lo, hi) = (Vec2::new(0.0, 28.0), Vec2::new(100.0, 88.0));
            let drops = fill_stacked(&layout, &field, lo, hi, STOPS, 0.25);
            let world = |d: &Drop| layout.to_world(Vec2::from(d.x));
            // The amount is the box's: as many rows a column as the box holds.
            let columns = ((layout.to_grid(hi).x * 2.0).floor() - (layout.to_grid(lo).x * 2.0).ceil()) as usize;
            let rows = ((hi.y - lo.y) / cell * 2.0).floor() as usize;
            assert_eq!(drops.len(), columns * rows, "{cell} px");
            for d in &drops {
                let p = world(d);
                // (One at the opening's very corner may sit level with the floor's top.)
                assert!(p.y < 100.0 + 0.1 * cell, "{cell} px: no drop below the floor's top, one at {p}");
                if (42.0..58.0).contains(&p.x) {
                    assert!(p.y < hi.y, "{cell} px: over the opening the fill stays in its box, one at {p}");
                }
            }
            // Beside the opening, the lowest drops rest a quarter cell off the floor.
            let lowest = drops.iter().map(world).filter(|p| p.x < 30.0).map(|p| p.y).fold(f32::MIN, f32::max);
            assert!((lowest - (100.0 - 0.25 * cell)).abs() < 0.05 * cell, "{cell} px: {lowest}");
        }
    }

    /// A left wall whose face is 0.3 cells off the lattice (grid x 9.3), a right one (if
    /// `right_wall`) whose face is 0.6 off it (grid x 19.6), and a floor at y = 60.
    fn two_walls(cell: f32, right_wall: bool) -> (Layout, SolidField) {
        let layout = Layout::cover(Vec2::new(-60.0, -100.0), Vec2::new(120.0, 100.0), cell);
        let origin = layout.origin.x;
        let face = |cells: f32| origin + cells * cell;
        let (left, right) = (face(9.3), face(19.6));
        let mut solids = vec![
            crate::solid::Solid::rect(Vec2::new(-60.0, -100.0), Vec2::new(left, 100.0), true, 0.0),
            crate::solid::Solid::rect(Vec2::new(-60.0, 60.0), Vec2::new(120.0, 100.0), true, 0.0),
        ];
        if right_wall {
            solids.push(crate::solid::Solid::rect(Vec2::new(right, -100.0), Vec2::new(120.0, 100.0), true, 0.0));
        }
        (layout, SolidField::build_least(layout, &solids, 1.5 * cell))
    }

    #[test]
    fn a_fill_sits_snug_against_its_walls() {
        let cell = 6.0;
        let (layout, field) = two_walls(cell, true);
        let drops = fill_stacked(&layout, &field, Vec2::new(-40.0, 0.0), Vec2::new(100.0, 60.0), STOPS, 0.25);
        let mut xs: Vec<f32> = drops.iter().map(|d| d.x[0]).collect();
        xs.sort_by(f32::total_cmp);
        xs.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
        let off = |x: f32| field.at(layout.to_world(Vec2::new(x, 15.0))) / cell;
        assert!((off(xs[0]) - 0.25).abs() < 1e-3, "the left column {} cell off its wall", off(xs[0]));
        let last = xs[xs.len() - 1];
        assert!((off(last) - 0.25).abs() < 1e-3, "the right column {} cell off its wall", off(last));
        let gaps: Vec<f32> = xs.windows(2).map(|w| w[1] - w[0]).collect();
        assert!(gaps.iter().all(|g| (g - gaps[0]).abs() < 1e-4), "evenly spread: {gaps:?}");
        assert!((0.5..0.5 + 0.5 / gaps.len() as f32 + 1e-4).contains(&gaps[0]), "about half a cell apart: {}", gaps[0]);
        // Every drop clear of the solids.
        assert!(drops.iter().all(|d| field.at(layout.to_world(Vec2::from(d.x))) / cell >= 0.25 - 1e-3));
    }

    #[test]
    fn a_fill_with_a_free_side_keeps_the_lattice() {
        let cell = 6.0;
        let (layout, field) = two_walls(cell, false);
        let hi = Vec2::new(100.0, 60.0);
        let drops = fill_stacked(&layout, &field, Vec2::new(-40.0, 0.0), hi, STOPS, 0.25);
        let mut xs: Vec<f32> = drops.iter().map(|d| d.x[0]).collect();
        xs.sort_by(f32::total_cmp);
        xs.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
        let off = |x: f32| field.at(layout.to_world(Vec2::new(x, 15.0))) / cell;
        assert!((off(xs[0]) - 0.25).abs() < 1e-3, "snug at the wall: {}", off(xs[0]));
        assert!(xs.windows(2).all(|w| (w[1] - w[0] - 0.5).abs() < 1e-4), "half a cell apart from it");
        let last = xs[xs.len() - 1];
        assert!(last + 0.25 <= layout.to_grid(hi).x && last + 0.75 > layout.to_grid(hi).x, "to the free side");
    }

    #[test]
    fn a_fill_in_mid_air_stays_in_its_box() {
        let (layout, field) = floor_with_opening(6.0);
        let (lo, hi) = (Vec2::new(-80.0, -80.0), Vec2::new(20.0, 20.0));
        let drops = fill_stacked(&layout, &field, lo, hi, STOPS, 0.25);
        let columns = ((layout.to_grid(hi).x * 2.0).floor() - (layout.to_grid(lo).x * 2.0).ceil()) as usize;
        assert_eq!(drops.len(), columns * ((hi.y - lo.y) / 6.0 * 2.0).floor() as usize);
        for d in &drops {
            let p = layout.to_world(Vec2::from(d.x));
            assert!(p.y > lo.y && p.y < hi.y, "{p}");
        }
    }
}
