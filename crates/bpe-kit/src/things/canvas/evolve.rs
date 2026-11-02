//! Wet paint over time. A canvas or skin with Dry time, Bleed or Drips (or a brush with Pickup)
//! runs a tick 30 times a second that works on the tiles with wet paint only: drips first, then
//! pickup, then bleed. The tick reads and writes only the canvas's own state, the balls' picked-up
//! paint and the log's marks. It never touches the physics.

use std::collections::BTreeMap;

use bpe_core::{Color, SlotId, ThingId};
use bpe_runtime::{Mark, MarkShape, MarkTarget, Picked, World};
use glam::Vec2;

use super::grid::{Cell, DRIP_JOINED, painter, wet};
use super::sim::{CanvasState, mix, reaches, to_local};
use super::skin::{line_reach, nearest, skin_settings, thin_reach};
use crate::modules::marks::MarksBrushes;

/// How a canvas or skin changes over time, fixed at build.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Evolve {
    /// 0..1, clamped.
    pub bleed: f32,
    /// 0..1, clamped.
    pub drips: f32,
    /// Ticks paint stays wet: round(dry_time × steps_per_second / tick_every), clamped to 0..=60_000.
    pub dry_ticks: u32,
    /// Whether some brush that marks this canvas has Pickup > 0 (canvases only; skins pick up at hits).
    pub pickup: bool,
}

impl Evolve {
    /// The settings of a canvas or skin for a world stepping `steps_per_second`.
    pub fn new(dry_time: f32, bleed: f32, drips: f32, steps_per_second: u32) -> Self {
        let tick = super::grid::tick_every(steps_per_second) as f32;
        let ticks = (dry_time.max(0.0) * steps_per_second as f32 / tick).round();
        Evolve {
            bleed: bleed.clamp(0.0, 1.0),
            drips: drips.clamp(0.0, 1.0),
            dry_ticks: if ticks.is_finite() { ticks.min(60_000.0) as u32 } else { 0 },
            pickup: false,
        }
    }

    /// Whether a tick has anything to do: dry_ticks > 0 and (bleed > 0 or drips > 0 or pickup).
    pub fn on(&self) -> bool {
        self.dry_ticks > 0 && (self.bleed > 0.0 || self.drips > 0.0 || self.pickup)
    }
}

/// A drip: a small blob of paint the tick moves.
#[derive(Clone, Debug, PartialEq)]
pub struct Runner {
    /// Where it is: canvas pixels, or the skin's thing's own frame.
    pub at: Vec2,
    /// Paint left, 0..1.
    pub load: f32,
    /// sRGB.
    pub rgb: [u8; 3],
    /// The direction of its last piece in radians; None before its first piece.
    pub heading: Option<f32>,
    /// Fixed at its start; its wobble comes from this and the tick.
    pub seed: u64,
    /// On a skin: where on the outline it runs.
    pub on: Option<OnLine>,
}

/// Where on a skin's outline a runner is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OnLine {
    pub line: u32,
    pub seg: u32,
    pub t: f32,
    pub offset: f32,
    pub dir: i8,
}

/// What one tick works with.
pub(crate) struct Tick<'a> {
    pub target: MarkTarget,
    pub world: &'a World,
    /// The full tick number: world.step / tick_every.
    pub now: u32,
    /// Ticks a second: steps_per_second / tick_every.
    pub per_second: f32,
    /// Gravity in the canvas's or thing's own frame.
    pub gravity: Vec2,
}

/// Runs one tick on `st`: drips, pickup (canvases only), then bleed. Returns the marks it made,
/// in order. `picked` is World.picked, taken out of the world for the call. `outline` is the
/// skin's outline in its own frame (None for a canvas).
pub(crate) fn tick(
    cx: &Tick,
    st: &mut CanvasState,
    picked: &mut BTreeMap<ThingId, Vec<Picked>>,
    outline: Option<&[Vec<Vec2>]>,
) -> Vec<Mark> {
    let mut out = Vec::new();
    st.ticks_run += 1;
    if st.evolve.drips > 0.0 {
        run_drips(cx, st, outline, &mut out);
        if cx.gravity.length() > 1e-3 {
            start_drips(cx, st, outline);
        }
    }
    if st.evolve.pickup {
        pickup(cx, st, picked, &mut out);
    }
    if st.evolve.bleed > 0.0 {
        bleed(cx, st, &mut out);
    }
    out
}

/// The Marks brush of `thing`, when it has one.
pub(crate) fn brush_of(world: &World, thing: ThingId) -> Option<&MarksBrushes> {
    world.module_state(thing, SlotId::Marks)?.1.as_any().downcast_ref::<MarksBrushes>()
}

/// Canvas pickup: every living ball of every brush that marks this canvas with Pickup on, in the
/// world's order and then row order, picks up from the paint under it.
fn pickup(cx: &Tick, st: &mut CanvasState, picked: &mut BTreeMap<ThingId, Vec<Picked>>, out: &mut Vec<Mark>) {
    let MarkTarget::Canvas(canvas) = cx.target else { return };
    let world = cx.world;
    let pose = world.poses.get(&canvas).copied().unwrap_or_default();
    for &thing in world.order() {
        let Some(b) = brush_of(world, thing) else { continue };
        if b.paint_pickup <= 0.0 || !world.matches(canvas, thing, &st.settings.brushes_from) {
            continue;
        }
        let Some(bodies) = world.bodies.get(&thing) else { continue };
        for i in 0..bodies.len() {
            if bodies.alive[i] {
                pick_at(cx, st, to_local(pose, bodies.position[i]), bodies.radius[i], (thing, i), b, picked, out);
            }
        }
    }
}

/// One ball (`ball`: its thing and row) of radius `radius` at canvas point `c` picks up wet paint
/// from the cells under half its radius that someone else laid: it mixes their colour into its
/// own picked-up paint, takes some of their paint away and logs a Lift. Shared by canvases (each
/// tick) and skins (at hits).
#[allow(clippy::too_many_arguments)]
pub(crate) fn pick_at(
    cx: &Tick,
    st: &mut CanvasState,
    c: Vec2,
    radius: f32,
    ball: (ThingId, usize),
    b: &MarksBrushes,
    picked: &mut BTreeMap<ThingId, Vec<Picked>>,
    out: &mut Vec<Mark>,
) {
    let (thing, i) = ball;
    let rr = 0.5 * radius;
    let (width, height) = st.grid.dims();
    let cs = st.grid.cell_size();
    // The cells whose middle lies within rr of c, and the one c is in.
    let mut read: Vec<(u32, u32)> = Vec::new();
    let reach = (rr / cs).ceil() as i64 + 1;
    let (fx, fy) = ((c.x / cs + width as f32 / 2.0).floor() as i64, (c.y / cs + height as f32 / 2.0).floor() as i64);
    for y in (fy - reach).max(0)..=(fy + reach).min(height as i64 - 1) {
        for x in (fx - reach).max(0)..=(fx + reach).min(width as i64 - 1) {
            if (st.grid.centre_of(x as u32, y as u32) - c).length() <= rr {
                read.push((x as u32, y as u32));
            }
        }
    }
    if let Some(home) = st.grid.cell_at(c)
        && !read.contains(&home)
    {
        read.push(home);
    }
    if read.is_empty() {
        return;
    }
    let me = painter(thing, i as u32);
    let dry = st.evolve.dry_ticks;
    let taking: Vec<((u32, u32), Cell)> = read
        .iter()
        .map(|&(x, y)| ((x, y), st.grid.get(x, y)))
        .filter(|(_, cell)| wet(cell, cx.now, dry) && cell.by != me)
        .collect();
    if taking.is_empty() {
        return;
    }
    let share = (b.paint_pickup * (taking.len() as f32 / read.len() as f32) * 0.35).min(1.0);
    // The paint they hold, by amount, as a linear colour.
    let total: f32 = taking.iter().map(|(_, cell)| cell.amount as f32).sum();
    let mut rgb = [0.0f32; 3];
    for (_, cell) in &taking {
        for (k, channel) in rgb.iter_mut().enumerate() {
            *channel += cell.rgb[k] as f32 * cell.amount as f32 / total;
        }
    }
    let linear = Color::rgb(rgb[0].round() as u8, rgb[1].round() as u8, rgb[2].round() as u8).to_linear();
    let list = picked.entry(thing).or_default();
    if list.len() <= i {
        list.resize(i + 1, Picked::default());
    }
    let world = cx.world;
    list[i].mix([linear[0], linear[1], linear[2]], share, world.step, world.steps_per_second, b.paint_pickup_fade);
    for ((x, y), cell) in taking {
        let loss = ((cell.amount as f32 * share * 0.5).floor() as u8).max(1);
        st.grid.set(x, y, Cell { amount: cell.amount.saturating_sub(loss), ..cell }, None);
    }
    out.push(Mark {
        step: world.step,
        target: cx.target,
        shape: MarkShape::Lift,
        from: c,
        to: c,
        radius: rr.max(cs * 0.5),
        radius_to: share * 0.5,
        angle: 0.0,
        within: [1.0, 1.0],
        variant: 0,
        colour: [0.0; 4],
        colour_to: [0.0; 4],
    });
}

/// Most drips running on one canvas or skin at once.
pub const MAX_RUNNERS: usize = 96;
/// Most drips one tile starts in one tick.
const STARTS_PER_TILE: usize = 2;

/// A drip's radius in pixels: (1 + 2.5·drips)·√load.
fn drip_radius(drips: f32, load: f32) -> f32 {
    (1.0 + 2.5 * drips) * load.max(0.0).sqrt()
}

/// A number from the target, for hashes.
fn salt(target: MarkTarget) -> u64 {
    match target {
        MarkTarget::Canvas(id) => id.0 as u64,
        MarkTarget::Skin(id) => 1 << 32 | id.0 as u64,
    }
}

/// A hash as a number from 0 to 1.
fn unit(h: u64) -> f32 {
    (h >> 40) as f32 / (1u64 << 24) as f32
}

/// Moves every runner one tick (canvas: `outline` None), logging and stamping its pieces.
fn run_drips(cx: &Tick, st: &mut CanvasState, outline: Option<&[Vec<Vec2>]>, out: &mut Vec<Mark>) {
    if cx.gravity.length() <= 1e-3 || st.runners.is_empty() {
        return;
    }
    let drips = st.evolve.drips;
    let g = cx.gravity.normalize();
    if let MarkTarget::Skin(_) = cx.target {
        // A skin's drips run along its outline; with none to run on they wait.
        if let Some(outline) = outline {
            run_skin_drips(cx, st, outline, g, out);
        }
        return;
    }
    let per_px = 1.0 / (60.0 + 200.0 * drips);
    let mut alive = Vec::with_capacity(st.runners.len());
    for mut r in std::mem::take(&mut st.runners) {
        let wobble = (unit(mix(r.seed, cx.now)) - 0.5) * 0.3;
        let dir = Vec2::from_angle(wobble).rotate(g);
        let mut dist = (30.0 + 150.0 * r.load) / cx.per_second;
        let mut load_after = r.load - dist * per_px;
        let done = load_after <= 0.0;
        if done {
            dist = r.load / per_px;
            load_after = 0.0;
        }
        let r0 = drip_radius(drips, r.load);
        let r1 = if done { 0.0 } else { drip_radius(drips, load_after) };
        let to = r.at + dir * dist;
        let colour = Color::rgb(r.rgb[0], r.rgb[1], r.rgb[2]).to_linear();
        let piece = Mark {
            step: cx.world.step,
            target: cx.target,
            shape: MarkShape::Drip,
            from: r.at,
            to,
            radius: r0,
            radius_to: r1,
            angle: r.heading.unwrap_or(0.0),
            within: [1.0, 1.0],
            variant: if r.heading.is_some() { DRIP_JOINED } else { 0 },
            colour,
            colour_to: colour,
        };
        if reaches(st.size, piece.from, piece.to, r0.max(r1)) {
            st.grid.stamp(&piece, cx.now, 0);
            out.push(piece);
        }
        r.heading = Some((to - r.at).to_angle());
        r.at = to;
        r.load = load_after;
        let outside = to.x.abs() > st.size.x * 0.5 || to.y.abs() > st.size.y * 0.5;
        if !done && !outside {
            alive.push(r);
        }
    }
    st.runners = alive;
}

/// Starts new runners in wet tiles.
fn start_drips(cx: &Tick, st: &mut CanvasState, outline: Option<&[Vec<Vec2>]>) {
    // A skin's drips start on its outline; with none, nothing starts.
    if matches!(cx.target, MarkTarget::Skin(_)) && outline.is_none() {
        return;
    }
    let reach = outline.map_or(0.0, |_| skin_reach(cx.world, cx.target));
    let drips = st.evolve.drips;
    let thr = 255.0 - 150.0 * drips;
    let chance = 0.08 + 0.25 * drips;
    let g = cx.gravity;
    let (dx, dy): (i32, i32) =
        if g.x.abs() > g.y.abs() { (if g.x > 0.0 { 1 } else { -1 }, 0) } else { (0, if g.y > 0.0 { 1 } else { -1 }) };
    let salt = salt(cx.target);
    let (width, height) = st.grid.dims();
    for t in st.grid.wet_tiles(cx.now, st.evolve.dry_ticks) {
        let (x0, x1, y0, y1) = st.grid.tile_cells(t);
        let mut starts = 0;
        'tile: for y in y0..y1 {
            for x in x0..x1 {
                if starts >= STARTS_PER_TILE {
                    break 'tile;
                }
                if st.runners.len() >= MAX_RUNNERS {
                    return;
                }
                let c = st.grid.get(x, y);
                if !wet(&c, cx.now, st.evolve.dry_ticks) || (c.amount as f32) < thr {
                    continue;
                }
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                let off = nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32;
                if !off && st.grid.get(nx as u32, ny as u32).amount as f32 >= thr {
                    continue;
                }
                let index = y * width + x;
                if unit(mix(salt ^ ((cx.now as u64) << 20), index)) >= chance {
                    continue;
                }
                let centre = st.grid.centre_of(x, y);
                // On a skin it runs on the line nearest the cell, at the cell's side of it.
                let (at, on) = match outline {
                    None => (centre, None),
                    Some(lines) => {
                        let Some((l, k, p)) = nearest(lines, centre) else { continue };
                        let seg = lines[l][k + 1] - lines[l][k];
                        let (u, len2) = (seg.normalize_or_zero(), seg.length_squared());
                        let n = u.perp();
                        let t = if len2 > 0.0 { ((p - lines[l][k]).dot(seg) / len2).clamp(0.0, 1.0) } else { 0.0 };
                        let offset = (centre - p).dot(n).clamp(-reach, reach);
                        (p + n * offset, Some(OnLine { line: l as u32, seg: k as u32, t, offset, dir: 0 }))
                    }
                };
                st.grid.set(x, y, Cell { amount: c.amount.saturating_sub(48), ..c }, None);
                st.runners.push(Runner {
                    at,
                    load: ((c.amount as f32 - thr / 2.0) / 255.0).clamp(0.2, 1.0),
                    rgb: c.rgb,
                    heading: None,
                    seed: mix(salt, cx.now ^ index),
                    on,
                });
                starts += 1;
            }
        }
    }
}

/// How far a skin's paint reaches either side of its line, for the drips' offsets: the wall's, or
/// the widest thin piece's; 0 for solid shapes (drips run on their edge).
fn skin_reach(world: &World, target: MarkTarget) -> f32 {
    let MarkTarget::Skin(thing) = target else { return 0.0 };
    let Some(skin) = skin_settings(world, thing) else { return 0.0 };
    if let Some(wall) = world.walls.get(&thing) {
        return line_reach(wall.outline.thickness(), skin);
    }
    let parts = world.solids.get(&thing).map_or(0, |s| s.parts.len() as u32);
    (0..parts).filter_map(|i| thin_reach(world, thing, i, skin)).fold(0.0, f32::max)
}

/// Walks along `line` from where `on` says, `dist` pixels downhill (`g` is the unit gravity in the
/// line's frame). Returns the points of the line it passed through and ended at (before the
/// offset is applied), and whether it stopped (at a lowest point, or at an open end).
pub(crate) fn walk(line: &[Vec2], on: &mut OnLine, g: Vec2, mut dist: f32) -> (Vec<Vec2>, bool) {
    let mut points = Vec::new();
    if line.len() < 2 {
        return (points, true);
    }
    let segs = line.len() as i32 - 1;
    let looped = line.len() > 2 && line[0] == line[line.len() - 1];
    let mut seg = (on.seg as i32).clamp(0, segs - 1);
    for _ in 0..(4 * segs).max(8) {
        let d = line[seg as usize + 1] - line[seg as usize];
        let len = d.length();
        if len < 1e-6 {
            if on.dir == 0 {
                return (points, true);
            }
            // Past a zero-length piece in the way it was going.
            let next = seg + on.dir as i32;
            if next < 0 || next >= segs {
                if !looped {
                    return (points, true);
                }
                seg = if on.dir > 0 { 0 } else { segs - 1 };
            } else {
                seg = next;
            }
            on.seg = seg as u32;
            on.t = if on.dir > 0 { 0.0 } else { 1.0 };
            continue;
        }
        let pull = g.dot(d / len);
        if on.dir == 0 {
            if pull.abs() < 0.05 {
                return (points, true);
            }
            on.dir = if pull > 0.0 { 1 } else { -1 };
        } else if pull * (on.dir as f32) < 0.0 || pull.abs() < 0.05 {
            return (points, true);
        }
        let dir = on.dir as f32;
        let room = if on.dir > 0 { (1.0 - on.t) * len } else { on.t * len };
        if dist <= room {
            on.t += dir * dist / len;
            on.seg = seg as u32;
            points.push(line[seg as usize] + d * on.t);
            return (points, false);
        }
        dist -= room;
        let corner = if on.dir > 0 { line[seg as usize + 1] } else { line[seg as usize] };
        points.push(corner);
        let next = seg + on.dir as i32;
        if next < 0 || next >= segs {
            if !looped {
                on.seg = seg as u32;
                on.t = if on.dir > 0 { 1.0 } else { 0.0 };
                return (points, true);
            }
            seg = if on.dir > 0 { 0 } else { segs - 1 };
        } else {
            seg = next;
        }
        on.seg = seg as u32;
        on.t = if on.dir > 0 { 0.0 } else { 1.0 };
    }
    (points, true)
}

/// The unit normal of `line` at `p`: the average of the normals of the pieces `p` lies on.
fn normal_at(line: &[Vec2], p: Vec2) -> Vec2 {
    let mut sum = Vec2::ZERO;
    for w in line.windows(2) {
        let d = w[1] - w[0];
        let len2 = d.length_squared();
        if len2 < 1e-12 {
            continue;
        }
        let t = ((p - w[0]).dot(d) / len2).clamp(0.0, 1.0);
        if (w[0] + d * t - p).length_squared() < 1e-6 {
            sum += d.normalize().perp();
        }
    }
    sum.normalize_or_zero()
}

/// Moves every skin runner one tick along its line.
fn run_skin_drips(cx: &Tick, st: &mut CanvasState, outline: &[Vec<Vec2>], g: Vec2, out: &mut Vec<Mark>) {
    let drips = st.evolve.drips;
    let per_px = 1.0 / (60.0 + 200.0 * drips);
    let mut alive = Vec::with_capacity(st.runners.len());
    for mut r in std::mem::take(&mut st.runners) {
        let Some(mut on) = r.on else { continue };
        let Some(line) = outline.get(on.line as usize) else { continue };
        if line.len() < 2 {
            continue;
        }
        let seg = (on.seg as usize).min(line.len() - 2);
        let u = (line[seg + 1] - line[seg]).normalize_or_zero();
        let mut dist = (30.0 + 150.0 * r.load) * g.dot(u).abs().max(0.25) / cx.per_second;
        let mut done = r.load - dist * per_px <= 0.0;
        if done {
            dist = r.load / per_px;
        }
        let (points, stopped) = walk(line, &mut on, g, dist);
        // The points, pushed off the line to the runner's side.
        let mut at = r.at;
        let mut used = 0.0;
        let mut pieces: Vec<(Vec2, Vec2, f32)> = Vec::new();
        for p in &points {
            let to = *p + normal_at(line, *p) * on.offset;
            let len = (to - at).length();
            pieces.push((at, to, used));
            used += len;
            at = to;
        }
        let total = used;
        // If the walk ran short of its distance, the load used up is what it really ran.
        let load_after = (r.load - if done && !stopped { dist } else { total.min(dist) } * per_px).max(0.0);
        if stopped {
            done = false;
        }
        let load_at = |d: f32| (r.load - d * per_px).max(0.0);
        let mut heading = r.heading;
        let count = pieces.len();
        for (i, (from, to, d0)) in pieces.into_iter().enumerate() {
            // Standing at a corner leaves a piece of no length; it draws nothing.
            if (to - from).length() < 1e-4 {
                continue;
            }
            let (a, b) = (d0, d0 + (to - from).length());
            let r0 = drip_radius(drips, load_at(a));
            let r1 = if done && i + 1 == count { 0.0 } else { drip_radius(drips, load_at(b)) };
            let colour = Color::rgb(r.rgb[0], r.rgb[1], r.rgb[2]).to_linear();
            let piece = Mark {
                step: cx.world.step,
                target: cx.target,
                shape: MarkShape::Drip,
                from,
                to,
                radius: r0,
                radius_to: r1,
                angle: heading.unwrap_or(0.0),
                within: [1.0, 1.0],
                variant: if heading.is_some() { DRIP_JOINED } else { 0 },
                colour,
                colour_to: colour,
            };
            if reaches(st.size, from, to, r0.max(r1)) {
                st.grid.stamp(&piece, cx.now, 0);
                out.push(piece);
            }
            heading = Some((to - from).to_angle());
        }
        r.at = at;
        r.heading = heading;
        r.load = load_after;
        r.on = Some(on);
        if stopped {
            // A drop where it stopped, round, with no join.
            if load_after > 0.0 {
                let radius = 1.6 * drip_radius(drips, load_after);
                let colour = Color::rgb(r.rgb[0], r.rgb[1], r.rgb[2]).to_linear();
                let drop = Mark {
                    step: cx.world.step,
                    target: cx.target,
                    shape: MarkShape::Drip,
                    from: at,
                    to: at,
                    radius,
                    radius_to: radius,
                    angle: 0.0,
                    within: [1.0, 1.0],
                    variant: 0,
                    colour,
                    colour_to: colour,
                };
                if reaches(st.size, at, at, radius) {
                    st.grid.stamp(&drop, cx.now, 0);
                    out.push(drop);
                }
            }
        } else if !done {
            alive.push(r);
        }
    }
    st.runners = alive;
}

/// The four neighbours of (x, y) inside a grid of `width` x `height`, in the order left, right,
/// up, down.
fn neighbours(x: u32, y: u32, width: u32, height: u32) -> impl Iterator<Item = (u32, u32)> {
    [
        (x > 0).then(|| (x.wrapping_sub(1), y)),
        (x + 1 < width).then_some((x + 1, y)),
        (y > 0).then(|| (x, y.wrapping_sub(1))),
        (y + 1 < height).then_some((x, y + 1)),
    ]
    .into_iter()
    .flatten()
}

/// Wet paint takes thicker than this to spread into empty cells.
const SOURCE_AMOUNT: u8 = 48;

/// Bleed: the grid step and its Bleed marks (one per wet tile).
fn bleed(cx: &Tick, st: &mut CanvasState, out: &mut Vec<Mark>) {
    let dry = st.evolve.dry_ticks;
    let before = st.grid.clone();
    let tiles = before.wet_tiles(cx.now, dry);
    if tiles.is_empty() {
        return;
    }
    let w = st.evolve.bleed.clamp(0.0, 1.0) * 0.25;
    let (width, height) = before.dims();
    let age = |c: &Cell| (cx.now as u16).wrapping_sub(c.painted);
    for &t in &tiles {
        let (x0, x1, y0, y1) = before.tile_cells(t);
        // The tile grown by one cell on every side.
        let (gx0, gx1) = (x0.saturating_sub(1), (x1 + 1).min(width));
        let (gy0, gy1) = (y0.saturating_sub(1), (y1 + 1).min(height));
        for y in gy0..gy1 {
            for x in gx0..gx1 {
                let c = before.get(x, y);
                let near: Vec<Cell> = neighbours(x, y, width, height).map(|(nx, ny)| before.get(nx, ny)).collect();
                if wet(&c, cx.now, dry) {
                    let wets: Vec<&Cell> = near.iter().filter(|n| wet(n, cx.now, dry)).collect();
                    if wets.is_empty() {
                        continue;
                    }
                    let mut rgb = c.rgb;
                    for (k, channel) in rgb.iter_mut().enumerate() {
                        let avg = wets.iter().map(|n| n.rgb[k] as f32).sum::<f32>() / wets.len() as f32;
                        let own = c.rgb[k] as f32;
                        *channel = (own + (avg - own) * w).round().clamp(0.0, 255.0) as u8;
                    }
                    if rgb != c.rgb {
                        st.grid.set(x, y, Cell { rgb, ..c }, None);
                    }
                } else if c.amount == 0 {
                    let sources: Vec<&Cell> =
                        near.iter().filter(|n| wet(n, cx.now, dry) && n.amount >= SOURCE_AMOUNT).collect();
                    let sum: u32 = sources.iter().map(|n| n.amount as u32).sum();
                    let take = (sum as f32 * w).floor() as u32;
                    if sources.is_empty() || take < 1 {
                        continue;
                    }
                    let mut rgb = [0u8; 3];
                    for (k, channel) in rgb.iter_mut().enumerate() {
                        let mixed = sources.iter().map(|n| n.rgb[k] as f32 * n.amount as f32).sum::<f32>() / sum as f32;
                        *channel = mixed.round().clamp(0.0, 255.0) as u8;
                    }
                    // The newest of their paint dates it; on a tie the first neighbour does.
                    let mut newest = sources[0];
                    for n in &sources[1..] {
                        if age(n) < age(newest) {
                            newest = n;
                        }
                    }
                    let full = cx.now.wrapping_sub(age(newest) as u32);
                    let cell = Cell { rgb, amount: take.min(255) as u8, painted: newest.painted, team: 0, by: 0 };
                    st.grid.set(x, y, cell, Some(full));
                }
            }
        }
    }
    // One Bleed mark per wet tile, over the tile grown by one cell.
    let cs = before.cell_size();
    let seconds = dry as f32 / cx.per_second;
    for &t in &tiles {
        let (x0, x1, y0, y1) = before.tile_cells(t);
        out.push(Mark {
            step: cx.world.step,
            target: cx.target,
            shape: MarkShape::Bleed,
            from: before.centre_of(x0, y0) - Vec2::splat(cs * 1.5),
            to: before.centre_of(x1 - 1, y1 - 1) + Vec2::splat(cs * 1.5),
            radius: seconds,
            radius_to: st.evolve.bleed,
            angle: 0.0,
            within: [1.0, 1.0],
            variant: 0,
            colour: [0.0; 4],
            colour_to: [0.0; 4],
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{ARENA, CANVAS, busy, canvas_state, painted};
    use crate::things::canvas::skin::skin_state;
    use crate::{Kit, kit, scenes};
    use bpe_core::{Scene, Value, resolve};
    use bpe_runtime::{Log, Playback};

    /// A canvas scene with the settings, built into a world, with the canvas's state taken out.
    fn rig(bleed: f32, drips: f32, dry: f32) -> (World, CanvasState) {
        let (mut scene, _, kit) = painted(false, |_| {});
        let p = &mut scene.things.get_mut(&CANVAS).unwrap().params;
        p.set("bleed", Value::Number(bleed as f64));
        p.set("drips", Value::Number(drips as f64));
        p.set("dry_time", Value::Number(dry as f64));
        let resolved = resolve(&scene, &kit.registry);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        let st = canvas_state(&world, CANVAS).clone();
        world.states.remove(&CANVAS);
        (world, st)
    }

    fn splat(at: Vec2, radius: f32, colour: [f32; 4]) -> Mark {
        Mark {
            step: 0,
            target: MarkTarget::Canvas(CANVAS),
            shape: MarkShape::Splat,
            from: at,
            to: at,
            radius,
            radius_to: radius,
            angle: 0.0,
            within: [1.0, 1.0],
            variant: 0,
            colour,
            colour_to: colour,
        }
    }

    /// Runs ticks `1..=n` on `st`, with `gravity`, and gives the marks they made.
    fn ticks(world: &World, st: &mut CanvasState, n: u32, gravity: Vec2) -> Vec<Mark> {
        ticks_from(world, st, 1, n, gravity)
    }

    /// Runs ticks `from..=to`.
    fn ticks_from(world: &World, st: &mut CanvasState, from: u32, to: u32, gravity: Vec2) -> Vec<Mark> {
        let mut out = Vec::new();
        let mut picked = BTreeMap::new();
        for now in from..=to {
            let cx = Tick { target: MarkTarget::Canvas(CANVAS), world, now, per_second: 30.0, gravity };
            if st.evolve.on() {
                st.ticks_run += 1;
            }
            out.extend(tick(&cx, st, &mut picked, None));
        }
        out
    }

    fn cell_at(st: &CanvasState, x: f32, y: f32) -> Cell {
        let (cx, cy) = st.grid.cell_at(Vec2::new(x, y)).unwrap();
        st.grid.get(cx, cy)
    }

    #[test]
    fn no_tick_without_bleed_drips_or_pickup() {
        let kit = kit();
        for (name, scene) in scenes::marks(&kit.registry) {
            let resolved = resolve(&scene, &kit.registry);
            let (world, log) = crate::testing::run(&scene, &resolved, &kit, 5.0);
            assert!(
                log.marks().iter().all(|m| !matches!(m.shape, MarkShape::Drip | MarkShape::Lift | MarkShape::Bleed)),
                "{name}"
            );
            for state in world.states.values().chain(world.skins.values()) {
                if let Some(st) = state.as_any().downcast_ref::<CanvasState>() {
                    assert_eq!(st.ticks_run, 0, "{name}");
                }
            }
        }
    }

    #[test]
    fn bleed_blends_wet_neighbours() {
        let (world, mut st) = rig(1.0, 0.0, 5.0);
        st.grid.stamp(&splat(Vec2::new(-20.0, 0.0), 20.0, [1.0, 0.0, 0.0, 1.0]), 0, 1);
        st.grid.stamp(&splat(Vec2::new(20.0, 0.0), 20.0, [0.0, 0.0, 1.0, 1.0]), 0, 2);
        ticks(&world, &mut st, 10, Vec2::ZERO);
        let middle = cell_at(&st, 0.0, 0.0);
        assert!(middle.rgb[0] > 40 && middle.rgb[2] > 40, "{:?}", middle.rgb);
        assert!(cell_at(&st, -30.0, 0.0).rgb[0] > 200);
    }

    #[test]
    fn bleed_spreads_thick_paint_a_cell_or_two() {
        let (world, mut st) = rig(0.5, 0.0, 5.0);
        let mut line = splat(Vec2::new(-60.0, 0.0), 6.0, [1.0, 0.0, 0.0, 1.0]);
        line.shape = MarkShape::Stroke;
        line.to = Vec2::new(60.0, 0.0);
        st.grid.stamp(&line, 0, 1);
        ticks(&world, &mut st, 30, Vec2::ZERO);
        let cs = st.grid.cell_size();
        let (w, h) = st.grid.dims();
        for y in 0..h {
            for x in 0..w {
                if st.grid.get(x, y).amount > 0 {
                    let c = st.grid.centre_of(x, y);
                    assert!(c.y.abs() <= 6.0 + 2.0 * cs, "paint at {c:?}");
                }
            }
        }
    }

    #[test]
    fn dry_paint_never_bleeds() {
        let (world, mut st) = rig(1.0, 0.0, 0.2);
        st.grid.stamp(&splat(Vec2::new(-20.0, 0.0), 20.0, [1.0, 0.0, 0.0, 1.0]), 0, 1);
        st.grid.stamp(&splat(Vec2::new(20.0, 0.0), 20.0, [0.0, 0.0, 1.0, 1.0]), 0, 2);
        ticks(&world, &mut st, 7, Vec2::ZERO);
        let after_seven = st.grid.hash();
        // Run the rest from tick 8 on.
        let mut picked = BTreeMap::new();
        for now in 8..=20 {
            let cx =
                Tick { target: MarkTarget::Canvas(CANVAS), world: &world, now, per_second: 30.0, gravity: Vec2::ZERO };
            tick(&cx, &mut st, &mut picked, None);
        }
        assert_eq!(st.grid.hash(), after_seven);
    }

    #[test]
    fn bleed_logs_one_mark_per_wet_tile() {
        let (world, mut st) = rig(0.5, 0.0, 1.0);
        let at = st.grid.centre_of(10, 10);
        st.grid.stamp(&splat(at, 12.0, [1.0, 0.0, 0.0, 1.0]), 0, 1);
        let marks = ticks(&world, &mut st, 40, Vec2::ZERO);
        // Wet for 30 ticks (ticks 1..=29 see it wet), one mark per tick.
        assert_eq!(marks.len(), 29);
        for m in &marks {
            assert_eq!(m.shape, MarkShape::Bleed);
            assert!(m.from.x < at.x && m.from.y < at.y && m.to.x > at.x && m.to.y > at.y);
            assert!((m.radius - 1.0).abs() < 1e-4 && (m.radius_to - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn bleed_reads_the_grid_from_before_the_tick() {
        let (world, mut st) = rig(1.0, 0.0, 5.0);
        st.grid.stamp(&splat(Vec2::ZERO, 24.0, [1.0, 0.2, 0.0, 1.0]), 0, 1);
        ticks(&world, &mut st, 5, Vec2::ZERO);
        let (w, h) = st.grid.dims();
        for y in 0..h {
            for x in 0..w {
                assert_eq!(st.grid.get(x, y), st.grid.get(w - 1 - x, y), "({x}, {y})");
            }
        }
    }

    #[test]
    fn skin_evolves_without_hits() {
        let kit = kit();
        let (_, mut scene) = scenes::marks(&kit.registry).into_iter().find(|(n, _)| n == "Splat ring").unwrap();
        let arena = &mut scene.things.get_mut(&ARENA).unwrap().params;
        arena.set("skin_bleed", Value::Number(0.5));
        arena.set("skin_dry_time", Value::Number(3.0));
        let resolved = resolve(&scene, &kit.registry);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        let skin_ticks = |w: &World| skin_state(w, ARENA).map_or(0, |s| s.ticks_run);
        // To the first paint on the skin.
        while skin_state(&world, ARENA).is_none_or(|s| s.grid.covered() == 0.0) {
            world.step(&kit.sims, &mut log);
            assert!(world.step < 240 * 20, "no hit on the skin");
        }
        let (start, before) = (world.step, skin_ticks(&world));
        let mut hitless = 0;
        for _ in 0..120 {
            world.step(&kit.sims, &mut log);
            let hit = log
                .marks()
                .iter()
                .any(|m| m.step == world.step && m.target == MarkTarget::Skin(ARENA) && m.shape != MarkShape::Bleed);
            if !hit && world.step.is_multiple_of(skin_state(&world, ARENA).unwrap().tick_every) {
                hitless += 1;
            }
        }
        assert!(world.step > start && hitless > 0);
        assert!(skin_ticks(&world) - before >= hitless, "{} vs {hitless}", skin_ticks(&world) - before);
    }

    fn with_wet(kit: &Kit, mut scene: Scene) -> (Scene, bpe_core::Resolved) {
        let p = &mut scene.things.get_mut(&CANVAS).unwrap().params;
        p.set("bleed", Value::Number(0.5));
        p.set("dry_time", Value::Number(2.0));
        let resolved = resolve(&scene, &kit.registry);
        (scene, resolved)
    }

    #[test]
    fn same_run_same_grid_with_bleed() {
        let (scene, _, kit) = busy();
        let (scene, resolved) = with_wet(&kit, scene);
        let mut a = Playback::new(&scene, &resolved, kit.sims.clone());
        let mut b = Playback::new(&scene, &resolved, kit.sims.clone());
        a.seek(6.0);
        b.seek(6.0);
        let (ga, gb) = (&canvas_state(a.world(), CANVAS).grid, &canvas_state(b.world(), CANVAS).grid);
        assert_eq!(ga.hash(), gb.hash());
        assert!(a.log().marks().iter().any(|m| m.shape == MarkShape::Bleed));
        assert_eq!(a.log().marks(), b.log().marks());
    }

    #[test]
    fn seek_equals_straight_with_bleed() {
        let (scene, _, kit) = busy();
        let (scene, resolved) = with_wet(&kit, scene);
        let mut straight = Playback::new(&scene, &resolved, kit.sims.clone());
        straight.seek(5.0);
        let mut jumpy = Playback::new(&scene, &resolved, kit.sims.clone());
        jumpy.seek(8.0);
        jumpy.seek(5.0);
        assert_eq!(canvas_state(straight.world(), CANVAS).grid.hash(), canvas_state(jumpy.world(), CANVAS).grid.hash());
        assert_eq!(straight.log().marks(), jumpy.log().marks());
        assert_eq!(straight.log().hash(), jumpy.log().hash());
    }

    /// A canvas with a thick wet blob at (0, -100) and `drips`, painted at tick 0.
    fn drip_canvas(drips: f32) -> (World, CanvasState) {
        let (world, mut st) = rig(0.0, drips, 4.0);
        for _ in 0..2 {
            st.grid.stamp(&splat(Vec2::new(0.0, -100.0), 30.0, [0.0, 0.3, 1.0, 1.0]), 0, 1);
        }
        (world, st)
    }

    fn drips_of(marks: &[Mark]) -> Vec<&Mark> {
        marks.iter().filter(|m| m.shape == MarkShape::Drip).collect()
    }

    #[test]
    fn drips_run_along_gravity() {
        let (world, mut st) = drip_canvas(0.8);
        let marks = ticks(&world, &mut st, 30, Vec2::new(0.0, 900.0));
        let drips = drips_of(&marks);
        assert!(!drips.is_empty());
        for m in drips {
            let d = m.to - m.from;
            assert!(d.y > 0.0 && d.x.abs() <= 0.16 * d.length(), "{d:?}");
        }
    }

    #[test]
    fn drips_follow_sideways_gravity() {
        let (world, mut st) = drip_canvas(0.8);
        let marks = ticks(&world, &mut st, 30, Vec2::new(900.0, 0.0));
        let drips = drips_of(&marks);
        assert!(!drips.is_empty());
        assert!(drips.iter().all(|m| m.to.x > m.from.x));
    }

    #[test]
    fn no_gravity_no_drips() {
        let (world, mut st) = drip_canvas(0.8);
        let marks = ticks(&world, &mut st, 30, Vec2::ZERO);
        assert!(drips_of(&marks).is_empty() && st.runners.is_empty());
    }

    #[test]
    fn dry_or_thin_paint_never_drips() {
        let (world, mut st) = drip_canvas(0.8);
        let marks = ticks_from(&world, &mut st, 200, 260, Vec2::new(0.0, 900.0));
        assert!(drips_of(&marks).is_empty());
        let (world, mut st) = rig(0.0, 0.3, 4.0);
        let mut line = splat(Vec2::new(-60.0, 0.0), 6.0, [1.0, 0.0, 0.0, 1.0]);
        line.shape = MarkShape::Stroke;
        line.to = Vec2::new(60.0, 0.0);
        st.grid.stamp(&line, 0, 1);
        let marks = ticks(&world, &mut st, 100, Vec2::new(0.0, 900.0));
        assert!(drips_of(&marks).is_empty());
    }

    #[test]
    fn drips_end_tapered() {
        let (world, mut st) = drip_canvas(0.8);
        let g = Vec2::new(0.0, 900.0);
        let mut marks = Vec::new();
        let mut picked = BTreeMap::new();
        for now in 1..=600 {
            let cx = Tick { target: MarkTarget::Canvas(CANVAS), world: &world, now, per_second: 30.0, gravity: g };
            marks.extend(tick(&cx, &mut st, &mut picked, None));
            if now > 125 && st.runners.is_empty() {
                break;
            }
        }
        assert!(st.runners.is_empty());
        let drips = drips_of(&marks);
        assert!(drips.iter().any(|m| m.radius_to == 0.0));
        // Every chain that ended inside the canvas ended with a point.
        for m in &drips {
            let continued = drips.iter().any(|n| n.from == m.to && n.variant & DRIP_JOINED != 0);
            let inside = m.to.x.abs() <= st.size.x * 0.5 && m.to.y.abs() <= st.size.y * 0.5;
            assert!(continued || !inside || m.radius_to == 0.0, "{m:?}");
        }
    }

    #[test]
    fn drip_pieces_join_without_beads() {
        let (world, mut st) = drip_canvas(0.8);
        let marks = ticks(&world, &mut st, 60, Vec2::new(0.0, 900.0));
        let drips = drips_of(&marks);
        let mut joined = 0;
        for m in &drips {
            if m.variant & DRIP_JOINED == 0 {
                assert_eq!(m.variant, 0);
                continue;
            }
            // Its predecessor is the piece that ended where this one starts.
            let before = drips.iter().find(|n| n.to == m.from).expect("a piece before");
            assert_eq!(m.angle.to_bits(), (before.to - before.from).to_angle().to_bits());
            joined += 1;
        }
        assert!(joined > 0);
    }

    #[test]
    fn runners_are_capped() {
        let (world, mut st) = rig(0.0, 1.0, 20.0);
        let (w, h) = st.grid.dims();
        let size = st.size;
        let mut y = -size.y * 0.5;
        while y < size.y * 0.5 + 20.0 {
            let mut x = -size.x * 0.5;
            while x < size.x * 0.5 + 20.0 {
                st.grid.stamp(&splat(Vec2::new(x, y), 20.0, [1.0, 0.0, 0.0, 1.0]), 0, 1);
                x += 20.0;
            }
            y += 20.0;
        }
        assert!(st.grid.covered() > 0.99, "{w}x{h}");
        let mut picked = BTreeMap::new();
        let (tiles_x, _) = st.grid.tile_dims();
        for now in 1..=60 {
            let cx = Tick {
                target: MarkTarget::Canvas(CANVAS),
                world: &world,
                now,
                per_second: 30.0,
                gravity: Vec2::new(0.0, 900.0),
            };
            tick(&cx, &mut st, &mut picked, None);
            assert!(st.runners.len() <= MAX_RUNNERS);
            let mut per_tile: BTreeMap<usize, usize> = BTreeMap::new();
            for r in st.runners.iter().filter(|r| r.heading.is_none()) {
                let (cx, cy) = st.grid.cell_at(r.at).unwrap();
                *per_tile.entry((cy / 32 * tiles_x + cx / 32) as usize).or_default() += 1;
            }
            assert!(per_tile.values().all(|&n| n <= STARTS_PER_TILE), "{per_tile:?}");
        }
    }

    #[test]
    fn drip_streaks_are_wet_paint() {
        let (world, mut st) = drip_canvas(0.8);
        let marks = ticks(&world, &mut st, 10, Vec2::new(0.0, 900.0));
        let drips = drips_of(&marks);
        assert!(!drips.is_empty());
        let found = drips.iter().any(|m| {
            let c = cell_at(&st, (m.from.x + m.to.x) * 0.5, (m.from.y + m.to.y) * 0.5);
            c.by == 0 && c.amount >= 40 && wet(&c, 10, st.evolve.dry_ticks)
        });
        assert!(found);
    }

    #[test]
    fn drips_leave_the_canvas_and_are_dropped() {
        let (world, mut st) = rig(0.0, 0.8, 2.0);
        let low = st.size.y * 0.5 - 40.0;
        for _ in 0..2 {
            st.grid.stamp(&splat(Vec2::new(0.0, low), 30.0, [0.0, 0.3, 1.0, 1.0]), 0, 1);
        }
        let marks = ticks(&world, &mut st, 200, Vec2::new(0.0, 900.0));
        assert!(!drips_of(&marks).is_empty());
        assert!(st.runners.is_empty(), "{} runners", st.runners.len());
        for m in drips_of(&marks) {
            assert!(reaches(st.size, m.from, m.to, m.radius.max(m.radius_to)));
        }
    }

    #[test]
    fn same_run_same_runners() {
        let (mut scene, _, kit) = busy();
        let p = &mut scene.things.get_mut(&CANVAS).unwrap().params;
        p.set("drips", Value::Number(0.6));
        p.set("dry_time", Value::Number(3.0));
        let resolved = resolve(&scene, &kit.registry);
        let mut a = Playback::new(&scene, &resolved, kit.sims.clone());
        let mut b = Playback::new(&scene, &resolved, kit.sims.clone());
        a.seek(6.0);
        b.seek(6.0);
        let (sa, sb) = (canvas_state(a.world(), CANVAS), canvas_state(b.world(), CANVAS));
        assert_eq!(sa.runners, sb.runners);
        assert_eq!(sa.grid.hash(), sb.grid.hash());
        assert_eq!(a.log().marks(), b.log().marks());
    }

    // --- skins ---

    /// The Splat ring scene with the drips on for the ring and the bar, and its bar's id.
    fn dripping_ring(drips: f32) -> (Scene, bpe_core::Resolved, Kit, ThingId) {
        let kit = kit();
        let (_, mut scene) = scenes::marks(&kit.registry).into_iter().find(|(n, _)| n == "Splat ring").unwrap();
        let bar = *scene.things.iter().find(|(_, t)| t.name == "Spinning bar").unwrap().0;
        for id in [ARENA, bar] {
            let p = &mut scene.things.get_mut(&id).unwrap().params;
            p.set("skin_drips", Value::Number(drips as f64));
            p.set("skin_dry_time", Value::Number(6.0));
        }
        let resolved = resolve(&scene, &kit.registry);
        assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
        (scene, resolved, kit, bar)
    }

    fn skin_drips(log: &Log, thing: ThingId) -> Vec<Mark> {
        log.marks()
            .iter()
            .filter(|m| m.target == MarkTarget::Skin(thing) && m.shape == MarkShape::Drip)
            .copied()
            .collect()
    }

    /// How far `p` is from the nearest of `lines`.
    fn off_line(lines: &[Vec<Vec2>], p: Vec2) -> f32 {
        let (_, _, q) = nearest(lines, p).unwrap();
        (p - q).length()
    }

    #[test]
    fn skin_drips_stay_on_the_ring() {
        let (scene, resolved, kit, _) = dripping_ring(0.8);
        let (world, log) = crate::testing::run(&scene, &resolved, &kit, 8.0);
        let drips = skin_drips(&log, ARENA);
        assert!(!drips.is_empty());
        let lines = crate::things::canvas::outline_local(&world, ARENA);
        let skin = skin_settings(&world, ARENA).unwrap();
        let reach = line_reach(world.walls[&ARENA].outline.thickness(), skin) + 0.5;
        for m in &drips {
            assert!(off_line(&lines, m.from) <= reach && off_line(&lines, m.to) <= reach, "{m:?}");
        }
    }

    #[test]
    fn skin_drips_gather_at_the_bottom_of_a_ring() {
        // The new scene's ring with a skin, and a thick wet blob put on it by hand, 20 degrees
        // round from the bottom, so a drip's paint lasts long enough to get there.
        let kit = kit();
        let mut scene = crate::new_scene(&kit.registry);
        let p = &mut scene.things.get_mut(&ARENA).unwrap().params;
        p.set("skin_drips", Value::Number(0.8));
        p.set("skin_dry_time", Value::Number(6.0));
        crate::testing::brushes(&mut scene, &kit, crate::testing::BALLS, |p| {
            p.set("paint_on", Value::Bool(false));
            p.set("splat_on", Value::Bool(true));
        });
        let resolved = resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let mut st = skin_state(&world, ARENA).expect("a skin").clone();
        let radius = match world.walls[&ARENA].outline {
            bpe_runtime::Outline::Ring { radius, .. } => radius,
            _ => panic!("a ring"),
        };
        let at = Vec2::new(20f32.to_radians().sin(), 20f32.to_radians().cos()) * radius * 0.97;
        let blob = Mark { target: MarkTarget::Skin(ARENA), ..splat(at, 30.0, [0.0, 0.3, 1.0, 1.0]) };
        st.grid.stamp(&blob, 0, 1);
        st.grid.stamp(&blob, 0, 1);
        let lines = crate::things::canvas::outline_local(&world, ARENA);
        let mut picked = BTreeMap::new();
        let mut marks = Vec::new();
        for now in 1..=150 {
            let cx = Tick {
                target: MarkTarget::Skin(ARENA),
                world: &world,
                now,
                per_second: 30.0,
                gravity: Vec2::new(0.0, 900.0),
            };
            marks.extend(tick(&cx, &mut st, &mut picked, Some(&lines)));
        }
        let drops: Vec<&Mark> = marks.iter().filter(|m| m.shape == MarkShape::Drip && m.from == m.to).collect();
        assert!(!drops.is_empty());
        for d in drops {
            assert!(d.from.y > 0.9 * radius, "{d:?}");
        }
    }

    #[test]
    fn skin_drips_stay_on_a_spinning_bar() {
        let (scene, resolved, kit, bar) = dripping_ring(0.8);
        let (world, log) = crate::testing::run(&scene, &resolved, &kit, 10.0);
        let drips = skin_drips(&log, bar);
        assert!(!drips.is_empty());
        let lines = crate::things::canvas::outline_local(&world, bar);
        // The bar is solid, so the drips run on its edge: within a pixel of it.
        for m in &drips {
            assert!(off_line(&lines, m.from) <= 1.0 && off_line(&lines, m.to) <= 1.0, "{m:?}");
        }
    }

    #[test]
    fn skin_drips_run_round_a_peg_to_its_underside() {
        let (mut scene, _, kit) = crate::testing::scene_with_obstacle(|p| {
            p.set("shape", Value::Choice("peg".into()));
            p.set("radius", Value::Number(40.0));
            p.set("skin_drips", Value::Number(1.0));
            p.set("skin_dry_time", Value::Number(6.0));
        });
        crate::testing::brushes(&mut scene, &kit, crate::testing::BALLS, |p| {
            p.set("paint_on", Value::Bool(false));
            p.set("splat_on", Value::Bool(true));
            p.set("splat_size", Value::Number(40.0));
            p.set("splat_min_speed", Value::Number(0.0));
        });
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 900.0]));
        let resolved = resolve(&scene, &kit.registry);
        let (world, log) = crate::testing::run(&scene, &resolved, &kit, 6.0);
        let drips = skin_drips(&log, crate::testing::OBSTACLE);
        assert!(!drips.is_empty());
        let drops: Vec<&Mark> = drips.iter().filter(|m| m.from == m.to).collect();
        assert!(!drops.is_empty());
        for d in drops {
            assert!(d.from.y > 0.8 * 40.0, "{d:?}");
        }
        let lines = crate::things::canvas::outline_local(&world, crate::testing::OBSTACLE);
        for m in &drips {
            assert!(off_line(&lines, m.from) <= 1.0 && off_line(&lines, m.to) <= 1.0, "{m:?}");
        }
    }

    #[test]
    fn no_gravity_no_skin_drips() {
        let (mut scene, _, kit, _) = dripping_ring(0.8);
        scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, 0.0]));
        let resolved = resolve(&scene, &kit.registry);
        let (_, log) = crate::testing::run(&scene, &resolved, &kit, 8.0);
        assert!(log.marks().iter().any(|m| matches!(m.target, MarkTarget::Skin(_))), "the balls still hit");
        assert!(log.marks().iter().all(|m| m.shape != MarkShape::Drip));
    }

    #[test]
    fn seek_equals_straight_for_skin_drips() {
        let (scene, resolved, kit, _) = dripping_ring(0.8);
        let mut straight = Playback::new(&scene, &resolved, kit.sims.clone());
        straight.seek(5.0);
        let mut jumpy = Playback::new(&scene, &resolved, kit.sims.clone());
        jumpy.seek(8.0);
        jumpy.seek(5.0);
        let skin = |p: &Playback| skin_state(p.world(), ARENA).unwrap().clone();
        let (a, b) = (skin(&straight), skin(&jumpy));
        assert_eq!(a.grid.hash(), b.grid.hash());
        assert_eq!(a.runners, b.runners);
        assert_eq!(straight.log().marks(), jumpy.log().marks());
        assert!(straight.log().marks().iter().any(|m| m.shape == MarkShape::Drip));
    }

    // --- pickup ---

    const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

    /// Gives the canvas (or the skin of `target`) a blue wet layer over everything, laid by
    /// someone else, at the world's current tick.
    fn pour_blue(world: &mut World, target: MarkTarget) {
        let now = (world.step / 8) as u32;
        let state = match target {
            MarkTarget::Canvas(c) => world.states.get_mut(&c),
            MarkTarget::Skin(t) => world.skins.get_mut(&t),
        };
        let st = state.unwrap().as_any_mut().downcast_mut::<CanvasState>().unwrap();
        let mut layer = splat(Vec2::ZERO, 5000.0, BLUE);
        layer.target = target;
        st.grid.stamp(&layer, now, 200);
    }

    /// A canvas whose ball paints a trail with `pickup`, built and ready to run.
    fn pickup_world(pickup: f32, dry: f32) -> (World, Log, Kit) {
        let (mut scene, _, kit) = painted(true, |p| {
            p.set("paint_pickup", Value::Number(pickup as f64));
            p.set("paint_pickup_fade", Value::Number(2.0));
        });
        scene.things.get_mut(&CANVAS).unwrap().params.set("dry_time", Value::Number(dry as f64));
        let resolved = resolve(&scene, &kit.registry);
        (World::build(&scene, &resolved, &kit.sims), Log::default(), kit)
    }

    #[test]
    fn pickup_takes_only_wet_paint() {
        let (mut world, mut log, kit) = pickup_world(1.0, 4.0);
        pour_blue(&mut world, MarkTarget::Canvas(CANVAS));
        world.advance_to(0.5, &kit.sims, &mut log);
        let p = world.picked[&crate::testing::BALLS][0];
        assert!(p.amount > 0.0 && p.rgb[2] > p.rgb[0] && p.rgb[2] > p.rgb[1], "{p:?}");
        // The same layer, dry by the time the ball is over it.
        let (mut world, mut log, kit) = pickup_world(1.0, 0.1);
        world.advance_to(1.0, &kit.sims, &mut log);
        let mut dry = splat(Vec2::ZERO, 5000.0, BLUE);
        dry.target = MarkTarget::Canvas(CANVAS);
        let st = world.states.get_mut(&CANVAS).unwrap().as_any_mut().downcast_mut::<CanvasState>().unwrap();
        st.grid.stamp(&dry, 0, 200);
        world.advance_to(2.0, &kit.sims, &mut log);
        assert!(world.picked.is_empty());
    }

    #[test]
    fn pickup_never_takes_the_balls_own_paint() {
        let (mut world, mut log, kit) = pickup_world(1.0, 4.0);
        world.advance_to(3.0, &kit.sims, &mut log);
        assert!(world.picked.is_empty());
        assert!(log.marks().iter().any(|m| m.shape == MarkShape::Stroke));
        assert!(log.marks().iter().all(|m| m.shape != MarkShape::Lift));
    }

    #[test]
    fn pickup_tint_fades_over_pickup_fade() {
        let (mut world, mut log, kit) = pickup_world(1.0, 4.0);
        pour_blue(&mut world, MarkTarget::Canvas(CANVAS));
        world.advance_to(0.2, &kit.sims, &mut log);
        let p = world.picked[&crate::testing::BALLS][0];
        let base = [1.0, 0.0, 0.0, 1.0];
        let at = |w: &mut World, seconds: f32| {
            w.step = p.step + (seconds * 240.0) as u64;
            bpe_runtime::with_picked(w, crate::testing::BALLS, 0, base)
        };
        let full = p.shown(p.step, 240);
        let half = at(&mut world, 1.0);
        let want = p.shown(p.step + 240, 240);
        assert!((want - (full - 0.5)).abs() < 1e-5);
        for k in 0..3 {
            assert!((half[k] - (base[k] + (p.rgb[k] - base[k]) * want)).abs() < 0.05, "{half:?}");
        }
        assert_eq!(at(&mut world, 3.0), base);
    }

    #[test]
    fn later_trail_paints_the_mixed_colour() {
        let (mut scene, _, kit) = painted(true, |p| {
            p.set("paint_pickup", Value::Number(1.0));
        });
        let look = crate::testing::module_of(&scene, &kit.registry, crate::testing::BALLS, bpe_core::SlotId::Look);
        let red = bpe_core::Color::rgb(255, 0, 0);
        let p = &mut scene.library.modules.get_mut(&look).unwrap().params;
        p.set("colours", Value::Gradient(bpe_core::Gradient::even(&[red, red])));
        let resolved = resolve(&scene, &kit.registry);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        world.advance_to(0.1, &kit.sims, &mut log);
        pour_blue(&mut world, MarkTarget::Canvas(CANVAS));
        world.advance_to(1.5, &kit.sims, &mut log);
        let mixed = log
            .marks()
            .iter()
            .filter(|m| m.shape == MarkShape::Stroke)
            .any(|m| m.colour[2] > 0.05 && m.colour[0] > 0.2);
        assert!(mixed, "a stroke after the crossing is neither pure red nor pure blue");
        assert!(log.marks().iter().filter(|m| m.shape == MarkShape::Stroke).take(3).all(|m| m.colour[2] < 0.01));
    }

    #[test]
    fn pickup_lifts_paint() {
        let (world, mut st) = rig(0.0, 0.0, 4.0);
        let mut blob = splat(Vec2::ZERO, 30.0, BLUE);
        blob.target = MarkTarget::Canvas(CANVAS);
        st.grid.stamp(&blob, 0, 200);
        let before = st.grid.clone();
        let b = crate::modules::marks::MarksBrushes { paint_pickup: 1.0, ..Default::default() };
        let cx =
            Tick { target: MarkTarget::Canvas(CANVAS), world: &world, now: 1, per_second: 30.0, gravity: Vec2::ZERO };
        let mut picked = BTreeMap::new();
        let mut marks = Vec::new();
        st.evolve = Evolve::new(4.0, 0.0, 0.0, 240);
        pick_at(&cx, &mut st, Vec2::ZERO, 20.0, (crate::testing::BALLS, 0), &b, &mut picked, &mut marks);
        assert_eq!(marks.len(), 1);
        let lift = marks[0];
        assert!(lift.shape == MarkShape::Lift && lift.radius_to > 0.0 && lift.radius_to <= 0.5, "{lift:?}");
        let (w, h) = st.grid.dims();
        let mut thinner = 0;
        for y in 0..h {
            for x in 0..w {
                let (old, new) = (before.get(x, y), st.grid.get(x, y));
                assert_eq!(old.painted, new.painted);
                assert!(new.amount <= old.amount);
                thinner += (new.amount < old.amount) as u32;
            }
        }
        assert!(thinner > 0);
    }

    #[test]
    fn pickup_leaves_the_log_alone() {
        let run = |pickup: f64| {
            let (mut scene, _, kit) = painted(true, |p| p.set("paint_pickup", Value::Number(pickup)));
            let ball = scene.things.get_mut(&crate::testing::BALLS).unwrap();
            ball.params.set("arrangement", Value::Choice("block".into()));
            ball.params.set("columns", Value::Int(3));
            ball.params.set("rows", Value::Int(1));
            ball.params.set("spacing", Value::Number(60.0));
            let resolved = resolve(&scene, &kit.registry);
            let (_, log) = crate::testing::run(&scene, &resolved, &kit, 20.0);
            (log.len(), log.hash())
        };
        assert_eq!(run(0.6), run(0.0));
    }

    #[test]
    fn skin_pickup_happens_at_hits() {
        let kit = kit();
        let build = |gravity: f64, velocity: [f64; 2]| {
            let mut scene = crate::new_scene(&kit.registry);
            let p = &mut scene.things.get_mut(&ARENA).unwrap().params;
            p.set("skin_dry_time", Value::Number(6.0));
            scene.things.get_mut(&ThingId::ROOT).unwrap().params.set("gravity", Value::Vec2([0.0, gravity as f32]));
            let ball = scene.things.get_mut(&crate::testing::BALLS).unwrap();
            ball.transform.position = Vec2::ZERO;
            ball.params.set("velocity", Value::Vec2([velocity[0] as f32, velocity[1] as f32]));
            crate::testing::brushes(&mut scene, &kit, crate::testing::BALLS, |p| {
                p.set("paint_on", Value::Bool(false));
                p.set("splat_on", Value::Bool(true));
                p.set("paint_pickup", Value::Number(1.0));
            });
            let resolved = resolve(&scene, &kit.registry);
            let mut world = World::build(&scene, &resolved, &kit.sims);
            assert!(world.skins.contains_key(&ARENA));
            pour_blue(&mut world, MarkTarget::Skin(ARENA));
            world
        };
        let mut log = Log::default();
        let mut world = build(900.0, [0.0, 0.0]);
        world.advance_to(3.0, &kit.sims, &mut log);
        assert!(!world.picked.is_empty());
        assert!(log.marks().iter().any(|m| m.shape == MarkShape::Lift && m.target == MarkTarget::Skin(ARENA)));
        // No gravity and no speed: no hits, nothing picked up (the known limit).
        let mut log = Log::default();
        let mut world = build(0.0, [0.0, 0.0]);
        world.advance_to(3.0, &kit.sims, &mut log);
        assert!(world.picked.is_empty());
    }

    #[test]
    fn a_scene_without_marks_keeps_pickup_empty() {
        let (scene, resolved, kit) = crate::testing::scene_with_balls("block", |_| {});
        let (world, _) = crate::testing::run(&scene, &resolved, &kit, 5.0);
        assert!(world.picked.is_empty() && world.pictures.is_empty() && world.skins.is_empty());
        assert!(world.looks.is_empty());
    }
}
