//! Aura fields (`layer.aura`): one flowing field round a thing's balls, like the lines of a
//! magnetic field, drawn as glowing contour lines with a soft fill. The field is
//! `f(p) = Σ (r / |p − c|)²` over the live balls (r a ball's radius, c its centre), 1 at a lone
//! ball's edge and falling away outward; where balls come near their fields merge. The lines are
//! where `log2 f × Lines − time × Flow` crosses a half step, found by marching squares on a grid
//! round the balls, so they flow outward as time runs. Nothing is kept between frames.

use std::collections::HashMap;

use bpe_core::{Blend, Color, Gradient, Settings};
use bpe_gfx::{Frame, Particle};
use glam::Vec2;

use super::around::AroundLayer;
use super::around_subs::{HitInfo, Pieces};
use super::trail::draw::{StripLook, strip_budget, strip_with_subs};
use super::trail::kinds::Path;
use super::trail::{AroundPlace, TrailFade, TrailLayer, TrailSource};

/// The layer kind.
pub const AURA: &str = "layer.aura";

/// The particle shape the fill is drawn with (smoke, as in sprites.wgsl).
const SMOKE: u32 = 1;

/// How far past a ball's reach the field's box grows.
const GROW: f32 = 1.6;

/// The most grid cells across the field's box: more would stall a frame.
pub const MOST_DETAIL: f32 = 1024.0;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct AuraLayer {
    /// Under the balls or over them.
    #[setting(label = "Where", group = "Field", simple)]
    pub place: AroundPlace,
    /// How far the field reaches past each ball, in ball sizes.
    #[setting(label = "Reach", group = "Field", range(1.01, 1e9), span(1.5, 8), unit = "×", simple, keyframe, pulse)]
    pub reach: f32,
    /// How many lines each halving of the field has.
    #[setting(label = "Lines", group = "Field", range(0, 1e9), span(0, 8), simple, keyframe)]
    pub lines: f32,
    /// How fast the lines run outward (below 0 inward), in lines a second.
    #[setting(label = "Flow", group = "Field", span(-4, 4), unit = "/s", keyframe, pulse)]
    pub flow: f32,
    /// How fine the grid the lines are found on is, in cells across the field.
    #[setting(label = "Detail", group = "Field", range(4, 1024), span(16, 256))]
    pub detail: f32,
    /// How wide each line is.
    #[setting(label = "Width", group = "Look", range(0, 1e9), span(0.25, 10), unit = "px", simple, keyframe, pulse)]
    pub width: f32,
    /// How wide the soft glow round each line is, in line widths (0 for none).
    #[setting(label = "Glow", group = "Look", span(0, 20), unit = "×", keyframe)]
    pub glow: f32,
    /// How much soft light fills the field.
    #[setting(label = "Fill", group = "Look", span(0, 1), keyframe, pulse)]
    pub fill: f32,
    /// The balls' own colours, mixed where their fields meet, or its own across the field.
    #[setting(label = "Colour from", group = "Look")]
    pub source: TrailSource,
    /// Its own colours, from the balls' edges out to the field's reach.
    #[setting(label = "Colours", group = "Look", when = "source=own")]
    pub colour: Gradient,
    #[setting(label = "Brightness", group = "Look", span(0, 4), keyframe, pulse)]
    pub brightness: f32,
    /// How it mixes with what is under it.
    #[setting(label = "Blend", group = "Blending")]
    pub blend: Blend,
    #[setting(label = "Blend amount", span(0, 1), group = "Blending", keyframe)]
    pub blend_amount: f32,
}

impl Default for AuraLayer {
    fn default() -> Self {
        Self {
            place: AroundPlace::Behind,
            reach: 3.5,
            lines: 3.0,
            flow: 0.6,
            detail: 120.0,
            // About 2 px: thinner lines fade out once a phone video is compressed.
            width: 2.0,
            glow: 4.0,
            fill: 0.2,
            source: TrailSource::Ball,
            colour: Gradient::even(&[Color::rgb(0xff, 0x6a, 0xd5), Color::rgb(0x4a, 0x9c, 0xff)]),
            brightness: 1.0,
            blend: Blend::Add,
            blend_amount: 1.0,
        }
    }
}

/// One ball as the field sees it.
#[derive(Clone, Copy, Debug)]
pub struct AuraBall {
    pub at: Vec2,
    pub radius: f32,
    /// Its colour as seen, linear light.
    pub colour: [f32; 4],
}

/// One contour line: its points, and at each how far out it is (0 at the balls' edges, 1 at the
/// reach) and the colour the balls give it there.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldLine {
    pub points: Vec<Vec2>,
    pub closed: bool,
    pub out: Vec<f32>,
    pub colour: Vec<[f32; 4]>,
}

/// The field and its colour at `p`: `f`, the balls' colours weighted by how much each adds, and
/// how far `p` is from the nearest ball as a share of that ball's reach.
fn field_at(balls: &[AuraBall], near: &[usize], p: Vec2, reach: f32) -> (f32, [f32; 3], f32) {
    let (mut f, mut c, mut out) = (0.0f32, [0.0f32; 3], f32::INFINITY);
    for &i in near {
        let b = &balls[i];
        let d2 = (p - b.at).length_squared().max(1e-6);
        out = out.min(d2.sqrt() / (b.radius * reach));
        let w = b.radius * b.radius / d2;
        f += w;
        for (c, b) in c.iter_mut().zip(&b.colour[..3]) {
            *c += b * w;
        }
    }
    if f > 0.0 {
        for v in &mut c {
            *v /= f;
        }
    }
    (f, c, out)
}

/// 0 at `a`, 1 at `b`, smooth between.
fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The contour lines of the field round `balls` at `time`.
pub fn field_lines(a: &AuraLayer, balls: &[AuraBall], time: f32) -> Vec<FieldLine> {
    let balls: Vec<AuraBall> =
        balls.iter().copied().filter(|b| b.at.is_finite() && b.radius > 0.0 && b.radius.is_finite()).collect();
    let reach = a.reach.max(1.01);
    if balls.is_empty() || a.lines <= 0.0 || !reach.is_finite() {
        return Vec::new();
    }
    // The box round the balls, grown past each one's reach: where fields add up their outer
    // lines reach further out than a lone ball's.
    let (mut lo, mut hi) = (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY));
    for b in &balls {
        lo = lo.min(b.at - Vec2::splat(b.radius * reach * GROW));
        hi = hi.max(b.at + Vec2::splat(b.radius * reach * GROW));
    }
    let size = hi - lo;
    let cell = size.max_element() / a.detail.clamp(4.0, MOST_DETAIL);
    if cell.is_nan() || cell <= 0.0 {
        return Vec::new();
    }
    let (nx, ny) = ((size.x / cell).ceil() as usize + 1, (size.y / cell).ceil() as usize + 1);
    // Balls by bins as wide as the furthest reach, so each grid point sums only those near it.
    // Each point sums the balls in its bin and the eight round it: every ball within a bin's
    // width, which is past the box's growth.
    let bin = balls.iter().map(|b| b.radius * reach * GROW * 1.25).fold(0.0, f32::max).max(cell);
    let mut bins: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    let key = |p: Vec2| (((p.x - lo.x) / bin).floor() as i32, ((p.y - lo.y) / bin).floor() as i32);
    for (i, b) in balls.iter().enumerate() {
        bins.entry(key(b.at)).or_default().push(i);
    }
    // The field's log and colour at every grid point.
    let edge_low = -2.0 * reach.log2();
    let mut level = vec![f32::NEG_INFINITY; nx * ny];
    let mut colour = vec![[0.0f32; 3]; nx * ny];
    // Lines fade out past every ball's reach, so where many balls' fields add up the outer lines
    // thin away instead of being cut off at the edge of the box.
    let mut fade = vec![0.0f32; nx * ny];
    let mut near = Vec::new();
    for y in 0..ny {
        for x in 0..nx {
            let p = lo + Vec2::new(x as f32, y as f32) * cell;
            let (bx, by) = key(p);
            near.clear();
            for dx in -1..=1 {
                for dy in -1..=1 {
                    if let Some(list) = bins.get(&(bx + dx, by + dy)) {
                        near.extend_from_slice(list);
                    }
                }
            }
            let (f, c, out) = field_at(&balls, &near, p, reach);
            if f > 0.0 {
                level[y * nx + x] = f.log2();
                colour[y * nx + x] = c;
                fade[y * nx + x] = 1.0 - smoothstep(0.75, 1.15, out);
            }
        }
    }
    // The values whose half steps are the lines: inside the balls (log above 0) and past the
    // reach there are none.
    let g = |k: usize| level[k] * a.lines - time * a.flow;
    let (top, bottom) = (-time * a.flow, edge_low * a.lines - time * a.flow);
    let first = (bottom - 0.5).ceil() as i64;
    let last = (top - 0.5).floor() as i64;
    let mut out = Vec::new();
    for n in first..=last.min(first + 4096) {
        let at = n as f32 + 0.5;
        for line in contours(nx, ny, &g, at) {
            let points: Vec<Vec2> = line.points.iter().map(|q| lo + *q * cell).collect();
            // How far out this line is: its log of the field against the reach's.
            let outward = vec![((at + time * a.flow) / a.lines / edge_low).clamp(0.0, 1.0); points.len()];
            let colours: Vec<[f32; 4]> = line
                .points
                .iter()
                .map(|q| {
                    let k = (q.y.round() as usize).min(ny - 1) * nx + (q.x.round() as usize).min(nx - 1);
                    let c = colour[k];
                    [c[0], c[1], c[2], fade[k]]
                })
                .collect();
            out.push(FieldLine { points, closed: line.closed, out: outward, colour: colours });
        }
    }
    out
}

/// A contour of a grid, in grid units.
struct Contour {
    points: Vec<Vec2>,
    closed: bool,
}

/// The lines where `value` (on an `nx` × `ny` grid) crosses `at`, by marching squares, linked
/// into polylines.
fn contours(nx: usize, ny: usize, value: &dyn Fn(usize) -> f32, at: f32) -> Vec<Contour> {
    if nx < 2 || ny < 2 {
        return Vec::new();
    }
    // An edge of the grid by its lower-left corner and direction (0 across, 1 down).
    type Edge = (usize, usize, u8);
    let v = |x: usize, y: usize| value(y * nx + x);
    let crossing = |e: Edge| -> Vec2 {
        let (x, y, d) = e;
        let (a, b) = if d == 0 { (v(x, y), v(x + 1, y)) } else { (v(x, y), v(x, y + 1)) };
        let t = if (b - a).abs() > 1e-12 { ((at - a) / (b - a)).clamp(0.0, 1.0) } else { 0.5 };
        if d == 0 { Vec2::new(x as f32 + t, y as f32) } else { Vec2::new(x as f32, y as f32 + t) }
    };
    let mut segments: Vec<(Edge, Edge)> = Vec::new();
    for y in 0..ny - 1 {
        for x in 0..nx - 1 {
            let c = [v(x, y), v(x + 1, y), v(x + 1, y + 1), v(x, y + 1)];
            if c.iter().any(|q| !q.is_finite()) {
                continue;
            }
            let bits = c.iter().enumerate().fold(0u8, |m, (k, q)| if *q > at { m | (1 << k) } else { m });
            let top = (x, y, 0);
            let right = (x + 1, y, 1);
            let bottom = (x, y + 1, 0);
            let left = (x, y, 1);
            let mean = (c[0] + c[1] + c[2] + c[3]) * 0.25;
            match bits {
                0 | 15 => {}
                1 | 14 => segments.push((left, top)),
                2 | 13 => segments.push((top, right)),
                3 | 12 => segments.push((left, right)),
                4 | 11 => segments.push((right, bottom)),
                6 | 9 => segments.push((top, bottom)),
                7 | 8 => segments.push((left, bottom)),
                5 => {
                    if mean > at {
                        segments.push((left, bottom));
                        segments.push((top, right));
                    } else {
                        segments.push((left, top));
                        segments.push((right, bottom));
                    }
                }
                10 => {
                    if mean > at {
                        segments.push((left, top));
                        segments.push((right, bottom));
                    } else {
                        segments.push((left, bottom));
                        segments.push((top, right));
                    }
                }
                _ => unreachable!(),
            }
        }
    }
    // Link the segments through the edges they share.
    let mut at_edge: HashMap<Edge, Vec<usize>> = HashMap::new();
    for (k, (a, b)) in segments.iter().enumerate() {
        at_edge.entry(*a).or_default().push(k);
        at_edge.entry(*b).or_default().push(k);
    }
    let mut used = vec![false; segments.len()];
    let mut out = Vec::new();
    let next_from = |edge: Edge, used: &[bool]| at_edge.get(&edge).and_then(|l| l.iter().copied().find(|&k| !used[k]));
    for start in 0..segments.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let (a, b) = segments[start];
        // Walk forward from b, then backward from a.
        let mut forward = vec![b];
        let mut edge = b;
        while let Some(k) = next_from(edge, &used) {
            used[k] = true;
            let (p, q) = segments[k];
            edge = if p == edge { q } else { p };
            forward.push(edge);
        }
        let closed = edge == a;
        let mut backward = Vec::new();
        if !closed {
            let mut edge = a;
            while let Some(k) = next_from(edge, &used) {
                used[k] = true;
                let (p, q) = segments[k];
                edge = if p == edge { q } else { p };
                backward.push(edge);
            }
        }
        let mut edges: Vec<Edge> = backward.into_iter().rev().collect();
        edges.push(a);
        if closed {
            forward.pop();
        }
        edges.extend(forward);
        out.push(Contour { points: edges.into_iter().map(crossing).collect(), closed });
    }
    out
}

/// Draws aura `a` (the around layer `layer`) round `balls` at `time` into `frame`.
pub fn draw(layer: &AroundLayer, a: &AuraLayer, balls: &[AuraBall], time: f32, frame: &mut Frame) {
    let front = a.place == AroundPlace::InFront;
    let adds = a.blend == Blend::Add;
    let k = a.brightness.max(0.0);
    if a.fill > 0.0 {
        let fills = if front { &mut frame.particles_front } else { &mut frame.particles };
        for b in balls.iter().filter(|b| b.at.is_finite() && b.radius > 0.0) {
            let c = match a.source {
                TrailSource::Ball => b.colour,
                TrailSource::Own => a.colour.sample(0.0),
            };
            let m = a.fill * k * 0.5;
            let mut p = Particle::new(
                SMOKE,
                b.at,
                0.0,
                b.radius * a.reach.max(1.0) * 0.6,
                [c[0] * m, c[1] * m, c[2] * m, -1.0],
            );
            p.style[2] = 1.0;
            fills.push(p);
        }
    }
    if a.width <= 0.0 && a.glow <= 0.0 {
        return;
    }
    let trail = TrailLayer {
        brightness: k,
        fade: TrailFade::None,
        blend: a.blend,
        blend_amount: a.blend_amount,
        ..TrailLayer::default()
    };
    let radius = balls.iter().map(|b| b.radius).fold(0.0, f32::max).max(1.0);
    let line_look = StripLook { trail: &trail, subs: &layer.subs, hit: HitInfo::default(), radius };
    let halo = StripLook { trail: &trail, subs: &[], hit: HitInfo::default(), radius };
    let mut pieces = Pieces::default();
    let mut points_left = strip_budget(&layer.subs) / 2;
    for line in field_lines(a, balls, time) {
        let mut points = line.points;
        let mut out = line.out;
        let mut colours = line.colour;
        if line.closed && !points.is_empty() {
            points.push(points[0]);
            out.push(out[0]);
            colours.push(colours[0]);
        }
        let n = points.len();
        if n < 2 {
            continue;
        }
        if n > points_left {
            break;
        }
        points_left -= n;
        let mut along = 0.0;
        let arc: Vec<f32> = (0..n)
            .map(|i| {
                if i > 0 {
                    along += points[i].distance(points[i - 1]);
                }
                along
            })
            .collect();
        let bases: Vec<[f32; 4]> = (0..n)
            .map(|i| {
                // Out by level, and thinning past the balls' reach (the colours' alpha).
                let fade = (1.0 - out[i]) * colours[i][3];
                let c = match a.source {
                    TrailSource::Ball => colours[i],
                    TrailSource::Own => a.colour.sample(out[i]),
                };
                if adds { [c[0] * fade, c[1] * fade, c[2] * fade, fade] } else { [c[0], c[1], c[2], fade] }
            })
            .collect();
        let path = Path { points, ago: vec![0.0; n], arc, corner: vec![false; n] };
        if a.glow > 0.0 {
            let soft: Vec<[f32; 4]> =
                bases.iter().map(|c| [c[0] * 0.25, c[1] * 0.25, c[2] * 0.25, c[3] * 0.25]).collect();
            strip_with_subs(&halo, &path, &vec![a.width.max(0.0) * a.glow * 0.5; n], &soft, 1.0, &mut pieces);
        }
        if a.width > 0.0 {
            strip_with_subs(&line_look, &path, &vec![a.width * 0.5; n], &bases, 0.5, &mut pieces);
        }
    }
    pieces.into_frame(frame, front);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ball(x: f32) -> AuraBall {
        AuraBall { at: Vec2::new(x, 0.0), radius: 20.0, colour: [1.0, 0.5, 0.0, 1.0] }
    }

    #[test]
    fn one_ball_gives_closed_rings_round_it() {
        let a = AuraLayer { flow: 0.0, ..AuraLayer::default() };
        let lines = field_lines(&a, &[ball(0.0)], 0.0);
        assert!(!lines.is_empty());
        for l in &lines {
            assert!(l.closed, "a lone ball's lines go all the way round");
            let r: Vec<f32> = l.points.iter().map(|p| p.length()).collect();
            let (lo, hi) = r.iter().fold((f32::MAX, 0.0f32), |(a, b), x| (a.min(*x), b.max(*x)));
            assert!(lo >= 19.0 && hi <= 20.0 * a.reach + 2.0, "{lo}..{hi}");
            assert!(hi - lo < 3.0, "round: {lo}..{hi}");
        }
        // The rings run outward as time goes on.
        let later = field_lines(&AuraLayer { flow: 0.6, ..a.clone() }, &[ball(0.0)], 0.4);
        assert_ne!(later.first().map(|l| l.points[0]), lines.first().map(|l| l.points[0]));
    }

    #[test]
    fn two_near_balls_share_their_outer_lines() {
        let a = AuraLayer { flow: 0.0, ..AuraLayer::default() };
        let lines = field_lines(&a, &[ball(-30.0), ball(30.0)], 0.0);
        // The outermost line goes round both.
        let widest = lines
            .iter()
            .max_by(|x, y| {
                let w = |l: &FieldLine| l.points.iter().map(|p| p.x).fold(f32::MIN, f32::max);
                w(x).total_cmp(&w(y))
            })
            .unwrap();
        assert!(widest.closed);
        assert!(widest.points.iter().any(|p| p.x < -30.0) && widest.points.iter().any(|p| p.x > 30.0));
        let mut frame = Frame::default();
        let layer = AroundLayer {
            id: bpe_core::LayerId(1),
            kind: super::super::around::AroundKind::Aura(Box::new(a.clone())),
            subs: Vec::new(),
            only: Vec::new(),
            light: [0.0; 4],
            emit: None,
            more: [0.0; 4],
        };
        draw(&layer, &a, &[ball(-30.0), ball(30.0)], 0.0, &mut frame);
        assert!(!frame.trails.is_empty() && frame.particles.len() == 2, "lines and a fill behind the balls");
    }
}
