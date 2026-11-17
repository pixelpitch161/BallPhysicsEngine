//! Where drops are born: the Fill (a lattice of four drops a cell, up to Level, in the thing's
//! shape or the whole area) and the Pours (drops from each spout, as many a second as its width
//! times its speed fills).

use bpe_core::dice::Stream;
use bpe_gfx::liquid_sim::{Drop, MOST_DROPS, STOPS};
use bpe_runtime::{Wall, World};
use glam::Vec2;

use crate::paint::{Inputs, Paint, colour_at};

use super::cards::PourLayer;
use super::state::{Layout, LiquidState};

/// The most drops one pour makes in a liquid step.
pub const MOST_A_STEP: u32 = 20_000;

/// The region a liquid fills: its shape (the hollow wall of the thing it is in) or the area less
/// the walls.
pub enum Region<'a> {
    Shape(&'a Wall),
    Free,
}

/// The region thing `st` fills in `world`.
pub fn region<'a>(st: &LiquidState, world: &'a World) -> Region<'a> {
    match st.parent.and_then(|p| world.walls.get(&p)) {
        Some(w) if !w.solid && !w.passes => Region::Shape(w),
        _ => Region::Free,
    }
}

/// Half a wall's band, in pixels (0 for a filled one).
fn half(w: &Wall) -> f32 {
    if w.solid { 0.0 } else { w.outline.thickness().max(0.0) * 0.5 }
}

/// Whether a drop may be born at world point `p`: clear of every wall's band (and out of filled
/// walls), and in `region`.
fn room_at(world: &World, region: &Region, layout: &Layout, p: Vec2) -> bool {
    let gap = layout.cell * 0.5;
    if let Region::Shape(w) = region
        && !w.outline.inside(p)
    {
        return false;
    }
    world.walls.values().filter(|w| !w.passes).all(|w| {
        if w.solid {
            return !w.outline.inside(p);
        }
        let (q, _) = w.outline.nearest(p);
        (p - q).length() > half(w) + gap
    })
}

/// The top and bottom (world y) of what `region` fills, and its left and right.
fn extent(region: &Region, layout: &Layout) -> (Vec2, Vec2) {
    let (lo, hi) = layout.inner();
    match region {
        Region::Shape(w) => {
            let contours = crate::outline::contours(&w.outline);
            let (a, b) = crate::outline::bounds(contours.iter().flatten()).unwrap_or((lo, hi));
            (a.max(lo), b.min(hi))
        }
        Region::Free => (lo, hi),
    }
}

/// The world y at which Level `level` stands in `region`.
pub fn level_y(region: &Region, layout: &Layout, level: f32) -> f32 {
    let (lo, hi) = extent(region, layout);
    hi.y - (hi.y - lo.y) * level.clamp(0.0, 1.0)
}

/// The colour of a drop of `paint` at `p`, its `index` of `count`, at `time`, with `strength`.
fn colour(paint: &Paint, world: &World, p: Vec2, index: usize, count: usize, time: f32, strength: f32) -> [f32; 4] {
    let inputs = Inputs {
        index,
        count,
        place: p,
        place_screen: world.camera.to_screen(p),
        start: p,
        time,
        seed: index as u32,
        ..Inputs::default()
    };
    let c = colour_at(paint, &inputs);
    let f = |x: f32| if x.is_finite() { x.max(0.0) } else { 0.0 };
    [f(c[0]), f(c[1]), f(c[2]), strength]
}

/// The drop `kind` of liquid `st`: its number, and [`STOPS`] when it isn't in a shape.
pub fn kind_of(st: &LiquidState, region: &Region) -> u32 {
    let index = st.index.unwrap_or(0) as u32;
    if matches!(region, Region::Free) { index | STOPS } else { index }
}

/// The Fill's drops between world heights `from` (lower on screen, larger y) up to `to`, bottom
/// row first, at most `most`: a lattice of two drops a cell each way.
pub fn fill_between(st: &mut LiquidState, world: &World, layout: &Layout, from: f32, to: f32, most: u32) -> Vec<Drop> {
    let region = region(st, world);
    let kind = kind_of(st, &region);
    let (lo, hi) = extent(&region, layout);
    let g_lo = layout.to_grid(Vec2::new(lo.x, to.max(lo.y)));
    let g_hi = layout.to_grid(Vec2::new(hi.x, from.min(hi.y)));
    let mut spots = Vec::new();
    let (c0, c1) = ((g_lo.x * 2.0).floor() as i64, (g_hi.x * 2.0).ceil() as i64);
    let (r0, r1) = ((g_lo.y * 2.0).floor() as i64, (g_hi.y * 2.0).ceil() as i64);
    'rows: for r in (r0..r1).rev() {
        for c in c0..c1 {
            let g = Vec2::new(c as f32 * 0.5 + 0.25, r as f32 * 0.5 + 0.25);
            let p = layout.to_world(g);
            if p.y < to || p.y > from || p.x < lo.x || p.x > hi.x {
                continue;
            }
            if !room_at(world, &region, layout, p) {
                continue;
            }
            if spots.len() as u32 >= most {
                break 'rows;
            }
            spots.push((g, p));
        }
    }
    let time = world.step as f32 / world.steps_per_second.max(1) as f32;
    let count = spots.len();
    let mut out = Vec::with_capacity(count);
    for (i, (g, p)) in spots.into_iter().enumerate() {
        let c = colour(&st.fill.colour, world, p, i, count, time, st.fill.strength);
        out.push(Drop::new(g.to_array(), [0.0, 0.0], c, kind, st.born));
        st.born = st.born.wrapping_add(1);
    }
    out
}

/// The Fill's first drops, up to Level (none when Fill at the start is off).
pub fn first_fill(st: &mut LiquidState, world: &World, layout: &Layout) -> Vec<Drop> {
    if !st.fill.fill || st.index.is_none() {
        return Vec::new();
    }
    let region = region(st, world);
    let (bottom, top) = (extent(&region, layout).1.y, level_y(&region, layout, st.fill.level));
    st.filled = st.fill.level;
    let most = st.fill.most.min(MOST_DROPS);
    fill_between(st, world, layout, bottom, top, most)
}

/// What a Level that moved since the last fill does: a rise adds drops up to it, a fall cuts the
/// liquid at it this step (`st.cut`, in grid y).
pub fn level_moved(st: &mut LiquidState, world: &World, layout: &Layout) -> Vec<Drop> {
    st.cut = None;
    if !st.fill.fill || st.index.is_none() {
        return Vec::new();
    }
    let level = st.fill.level;
    let region = region(st, world);
    // A row of drops is half a cell: smaller moves wait until they add up to one.
    let (lo, hi) = extent(&region, layout);
    let row = (layout.cell * 0.5) / (hi.y - lo.y).max(1e-3);
    if level > st.filled + row {
        let (from, to) = (level_y(&region, layout, st.filled), level_y(&region, layout, level));
        st.filled = level;
        let most = st.fill.most.min(MOST_DROPS);
        return fill_between(st, world, layout, from, to, most);
    }
    if level < st.filled - row {
        st.filled = level;
        st.cut = Some(layout.to_grid(Vec2::new(0.0, level_y(&region, layout, level))).y);
    }
    Vec::new()
}

/// Pour `k`'s drops for one liquid step of `dt` seconds (its share owed carried in `st.owed`).
pub fn pour(st: &mut LiquidState, k: usize, world: &World, layout: &Layout, dt: f32, step: u32) -> Vec<Drop> {
    let p: PourLayer = st.pours[k].clone();
    if !p.on || st.index.is_none() {
        st.owed[k] = 0.0;
        return Vec::new();
    }
    let region = region(st, world);
    let kind = kind_of(st, &region);
    let per_drop = layout.cell * layout.cell * 0.25;
    let owed = st.owed[k] + p.width * p.speed * dt / per_drop;
    let owed = if owed.is_finite() { owed.min(MOST_A_STEP as f32 + 1.0) } else { 0.0 };
    let n = owed.floor() as u32;
    st.owed[k] = owed - n as f32;
    if n == 0 {
        return Vec::new();
    }
    let anchor = world.anchor(st.id);
    let turn = Vec2::from_angle(anchor.angle);
    let spout = anchor.position + turn.rotate(p.place);
    let aim = p.aim.to_radians() + anchor.angle;
    let way = Vec2::from_angle(aim);
    let across = way.perp();
    let time = step as f32 * dt;
    let mut dice = Stream::new(st.id.0.wrapping_mul(0x9e37_79b9) ^ k as u32, step);
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n {
        let u = (i as f32 + dice.unit()) / n as f32 - 0.5;
        // Spread along the way it travels this step too, so drops don't come out in rows.
        let along = dice.unit() * p.speed * dt;
        let at = spout + across * (u * p.width) + way * along;
        let turn = (p.spread * dice.signed()).to_radians();
        let v = Vec2::from_angle(aim + turn) * p.speed;
        let c = colour(&p.colour, world, at, i as usize, n as usize, time, p.strength);
        let g = layout.to_grid(at);
        out.push(Drop::new(g.to_array(), (v / layout.cell).to_array(), c, kind, st.born));
        st.born = st.born.wrapping_add(1);
    }
    out
}
