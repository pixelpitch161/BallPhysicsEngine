//! Where drops are born: the Fill (a lattice of four drops a cell, up to Level, in the thing's
//! shape or the whole area) and the Pours (drops from each spout, as many a second as its width
//! times its speed fills).

use bpe_core::dice::Stream;
use bpe_gfx::liquid_sim::{Drop, STOPS};
use bpe_runtime::{Wall, World};
use glam::Vec2;

use crate::paint::{Inputs, Paint, colour_at};

use super::cards::{FillWith, PourLayer};
use super::state::{Layout, LiquidState, MOST_BORN_A_STEP};

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

/// The colour a reaction that makes liquid `st` gives its drops: its Fill colour at the start.
pub fn made_colour(st: &LiquidState, world: &World) -> [f32; 4] {
    colour(&st.fill.colour, world, Vec2::ZERO, 0, 1, 0.0, st.fill.strength)
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

/// The Fill's first drops, up to Level (none when Fill at the start is off), or in the shape of
/// a picture or a word.
pub fn first_fill(st: &mut LiquidState, world: &World, layout: &Layout) -> Vec<Drop> {
    if !st.fill.fill || st.index.is_none() {
        return Vec::new();
    }
    if st.fill.fill_with != FillWith::Level {
        let drops = picture_or_word(st, world, layout);
        return heated(st, drops);
    }
    let region = region(st, world);
    let (bottom, top) = (extent(&region, layout).1.y, level_y(&region, layout, st.fill.level));
    st.filled = st.fill.level;
    let most = st.kept;
    let drops = fill_between(st, world, layout, bottom, top, most);
    heated(st, drops)
}

/// `drops` at the liquid's Temperature.
fn heated(st: &LiquidState, drops: Vec<Drop>) -> Vec<Drop> {
    let t = st.settings.temperature;
    drops.into_iter().map(|d| d.at_heat(t)).collect()
}

/// The places (from the thing's place, pixels) and colours of a Fill made of a picture's solid
/// parts or a word's letters, on a lattice `spacing` apart.
fn shaped(st: &LiquidState, world: &World, spacing: f32) -> Vec<(Vec2, Option<[f32; 3]>)> {
    match st.fill.fill_with {
        FillWith::Level => Vec::new(),
        FillWith::AWord => crate::things::particles::spots::word_spots(
            &st.fill.text,
            st.fill.letter_height,
            crate::things::particles::Weight::SemiBold,
            spacing,
        )
        .into_iter()
        .map(|p| (p, None))
        .collect(),
        FillWith::APicture => {
            use crate::things::particles::spots::{read_png, read_png_from};
            let path = &st.fill.picture;
            if path.trim().is_empty() {
                return Vec::new();
            }
            let kept = world.tree.as_deref().and_then(|t| t.pictures.get(path)).map(|p| p.bytes.clone());
            let picture = match kept {
                Some(bytes) => read_png_from(std::io::Cursor::new(&bytes[..])),
                None => read_png(path),
            };
            let Some((rgba, w, h)) = picture else { return Vec::new() };
            let width = st.fill.picture_width.max(1.0);
            let scale = width / w as f32;
            let size = Vec2::new(width, h as f32 * scale);
            let (cols, rows) = ((size.x / spacing).floor() as i64, (size.y / spacing).floor() as i64);
            // At most 4000 × 4000 places looked at, and 2 million kept: far more than any region holds.
            let mut out = Vec::new();
            'rows: for j in 0..rows.min(4_000) {
                for i in 0..cols.min(4_000) {
                    if out.len() >= 2_000_000 {
                        break 'rows;
                    }
                    let p = Vec2::new((i as f32 + 0.5) * spacing, (j as f32 + 0.5) * spacing);
                    let (px, py) = ((p.x / scale) as usize, (p.y / scale) as usize);
                    if px >= w || py >= h {
                        continue;
                    }
                    let c = &rgba[(py * w + px) * 4..][..4];
                    if c[3] < 128 {
                        continue;
                    }
                    let linear = bpe_core::Color { r: c[0], g: c[1], b: c[2], a: 255 }.to_linear();
                    out.push((p - size * 0.5, Some([linear[0], linear[1], linear[2]])));
                }
            }
            out
        }
    }
}

/// A Fill made of a picture or a word: drops two a cell over it, round the thing's place, in
/// the picture's colours (a word in the Fill colour), only where a drop may be.
fn picture_or_word(st: &mut LiquidState, world: &World, layout: &Layout) -> Vec<Drop> {
    let region = region(st, world);
    let kind = kind_of(st, &region);
    let anchor = world.anchor(st.id);
    let turn = Vec2::from_angle(anchor.angle);
    let (lo, hi) = layout.inner();
    let spots: Vec<(Vec2, Option<[f32; 3]>)> = shaped(st, world, layout.cell * 0.5)
        .into_iter()
        .map(|(p, c)| (anchor.position + turn.rotate(p), c))
        .filter(|(p, _)| p.cmpge(lo).all() && p.cmple(hi).all() && room_at(world, &region, layout, *p))
        .take(st.kept as usize)
        .collect();
    let time = world.step as f32 / world.steps_per_second.max(1) as f32;
    let count = spots.len();
    let mut out = Vec::with_capacity(count);
    for (i, (p, c)) in spots.into_iter().enumerate() {
        let colour = match c {
            Some(rgb) => [rgb[0], rgb[1], rgb[2], st.fill.strength],
            None => colour(&st.fill.colour, world, p, i, count, time, st.fill.strength),
        };
        out.push(Drop::new(layout.to_grid(p).to_array(), [0.0, 0.0], colour, kind, st.born));
        st.born = st.born.wrapping_add(1);
    }
    out
}

/// How many cells down from the top a raindrop looks for room.
const MOST_RAIN_TRIES: u32 = 400;
/// The most drops a raindrop or a ball's burst is made of.
const MOST_A_CLUMP: u32 = 5_000;

/// `n` drops filling a disc of `radius` (pixels) round `centre`, on a sunflower's spiral.
fn disc(centre: Vec2, radius: f32, n: u32) -> impl Iterator<Item = Vec2> {
    let golden = std::f32::consts::PI * (3.0 - 5f32.sqrt());
    (0..n).map(move |k| {
        let r = radius * ((k as f32 + 0.5) / n as f32).sqrt();
        centre + Vec2::from_angle(k as f32 * golden) * r
    })
}

/// How many drops fill a disc of `radius` pixels (at least one).
fn drops_in(radius: f32, layout: &Layout) -> u32 {
    let per_drop = layout.cell * layout.cell * 0.25;
    let n = std::f32::consts::PI * radius * radius / per_drop;
    if n.is_finite() { (n.round() as u32).clamp(1, MOST_A_CLUMP) } else { 1 }
}

/// Which way is down: along gravity, or down the screen without it.
fn down(world: &World) -> Vec2 {
    let g = world.gravity.normalize_or_zero();
    if g == Vec2::ZERO { Vec2::Y } else { g }
}

/// The rain's drops for one liquid step of `dt` seconds: raindrops at random along the top of
/// what the liquid fills, falling at Rain speed.
pub fn rain(st: &mut LiquidState, world: &World, layout: &Layout, dt: f32, step: u32) -> Vec<Drop> {
    if st.fill.rain <= 0.0 || st.index.is_none() {
        st.rain_owed = 0.0;
        return Vec::new();
    }
    let owed = st.rain_owed + st.fill.rain * dt;
    let owed = if owed.is_finite() { owed.min(1000.0) } else { 0.0 };
    let n = owed.floor() as u32;
    st.rain_owed = owed - n as f32;
    if n == 0 {
        return Vec::new();
    }
    let region = region(st, world);
    let kind = kind_of(st, &region);
    let (lo, hi) = extent(&region, layout);
    let radius = st.fill.rain_size * 0.5;
    let each = drops_in(radius, layout);
    let v = down(world) * st.fill.rain_speed;
    let time = step as f32 * dt;
    let mut dice = Stream::new(st.id.0.wrapping_mul(0x9e37_79b9) ^ 0x5241_494e, step);
    let mut out = Vec::new();
    for _ in 0..n {
        // No more than one liquid step can take.
        if out.len() >= MOST_BORN_A_STEP {
            break;
        }
        // From the top down to the first place a drop may be (a round shape's top is narrow).
        let x = lo.x + (hi.x - lo.x) * dice.unit();
        let top = lo.y + radius + layout.cell;
        let Some(centre) = (0..MOST_RAIN_TRIES)
            .map(|k| Vec2::new(x, top + k as f32 * layout.cell))
            .take_while(|p| p.y < hi.y)
            .find(|p| room_at(world, &region, layout, *p))
        else {
            continue;
        };
        for (i, p) in disc(centre, radius, each).enumerate() {
            let c = colour(&st.fill.colour, world, p, i, each as usize, time, st.fill.strength);
            out.push(Drop::new(layout.to_grid(p).to_array(), (v / layout.cell).to_array(), c, kind, st.born));
            st.born = st.born.wrapping_add(1);
        }
    }
    heated(st, out)
}

/// The drops balls make for one liquid step of `dt` seconds: leaked off the back of each moving
/// ball the Fill card's Balls names, and (on the world step's first liquid step, `first`) a
/// burst where a ball seen last world step has gone.
pub fn from_balls(st: &mut LiquidState, world: &World, layout: &Layout, dt: f32, step: u32, first: bool) -> Vec<Drop> {
    let fill = &st.fill;
    if (fill.leaks <= 0.0 && fill.bursts <= 0.0) || st.index.is_none() {
        st.seen.clear();
        return Vec::new();
    }
    let region = region(st, world);
    let kind = kind_of(st, &region);
    let time = step as f32 * dt;
    let mut out: Vec<(Vec2, Vec2)> = Vec::new();
    let mut now = std::collections::BTreeMap::new();
    let mut dice = Stream::new(st.id.0.wrapping_mul(0x85eb_ca6b) ^ 0x4241_4c4c, step);
    for (&thing, b) in &world.bodies {
        if !world.names(&fill.from_balls, st.id, thing) {
            continue;
        }
        for r in b.live_rows() {
            let (x, v, radius) = (b.position[r], b.velocity[r], b.radius[r]);
            if !b.alive[r] || !x.is_finite() || !v.is_finite() || !radius.is_finite() {
                continue;
            }
            now.insert((thing, r as u32), (x, v, radius));
            if fill.leaks > 0.0 {
                let speed = v.length();
                let n = fill.leaks * (speed / 500.0).min(4.0) * dt + dice.unit();
                let back = x - v.normalize_or_zero() * (radius + layout.cell * 0.5);
                for _ in 0..(n.floor() as u32).min(MOST_A_CLUMP) {
                    if out.len() >= MOST_BORN_A_STEP {
                        break;
                    }
                    let jitter = Vec2::new(dice.signed(), dice.signed()) * layout.cell * 0.5;
                    out.push((back + jitter, v * 0.5));
                }
            }
        }
    }
    if first && fill.bursts > 0.0 {
        for (key, &(x, v, radius)) in &st.seen {
            if now.contains_key(key) {
                continue;
            }
            let n = drops_in(radius * fill.bursts.sqrt(), layout);
            for p in disc(x, radius * fill.bursts.sqrt().max(0.5), n.min(MOST_BORN_A_STEP as u32)) {
                if out.len() >= MOST_BORN_A_STEP {
                    break;
                }
                let away = (p - x).normalize_or_zero() * 120.0;
                out.push((p, v + away));
            }
        }
    }
    if first {
        st.seen = if fill.bursts > 0.0 { now } else { Default::default() };
    }
    let (lo, hi) = layout.inner();
    let count = out.len();
    let mut drops = Vec::with_capacity(count);
    for (i, (p, v)) in out.into_iter().enumerate() {
        if !(p.cmpge(lo).all() && p.cmple(hi).all() && room_at(world, &region, layout, p)) {
            continue;
        }
        let c = colour(&st.fill.colour, world, p, i, count, time, st.fill.strength);
        drops.push(Drop::new(layout.to_grid(p).to_array(), (v / layout.cell).to_array(), c, kind, st.born));
        st.born = st.born.wrapping_add(1);
    }
    heated(st, drops)
}

/// What a Level that moved since the last fill does: a rise adds drops up to it, a fall cuts the
/// liquid at it this step (`st.cut`, in grid y).
pub fn level_moved(st: &mut LiquidState, world: &World, layout: &Layout) -> Vec<Drop> {
    st.cut = None;
    if !st.fill.fill || st.fill.fill_with != FillWith::Level || st.index.is_none() {
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
        let most = st.kept;
        let drops = fill_between(st, world, layout, from, to, most);
        return heated(st, drops);
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
    let owed = st.owed[k] + p.width * p.speed * p.rate * dt / per_drop;
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
    heated(st, out)
}
