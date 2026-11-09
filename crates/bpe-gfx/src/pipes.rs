//! Light pipes (glass and light part 4a, plan `docs/superpowers/plans/glass-light/part4.md`,
//! decision 9): a shape whose Surface has Light pipe on catches the light of the lamps near it and
//! of lasers ending on it, carries it both ways round its outline, fading over Carries and leaking
//! out at corners sharper than its critical angle, and glows with what comes out: a few strip
//! lights along the outline. Worked out on the CPU after every thing has made its light; pipes
//! never cast shadows or send photons, and the light probe never sees them.

use glam::Vec2;

use crate::frame::{LightGpu, LightScene, Pipe};

/// How much brighter a pipe's glow is than the light it gives out (so a lamp inside a ring pipe
/// lights the ring's outside about as a quarter of the lamp would).
pub const PIPE_GAIN: f32 = 4.0;
/// The most strip lights one pipe glows with.
pub const MOST_PIPE_PIECES: usize = 32;
/// The farthest a pipe's glow reaches (world): 4 × Carries, at most this.
pub const PIPE_REACH: f32 = 1200.0;
/// How high above the picture a pipe's glow hangs.
const PIPE_HEIGHT: f32 = 20.0;
/// How near a laser's end must be to the outline (past half the pipe's width) to go in.
const LASER_NEAR: f32 = 6.0;
/// How many times round a pipe light is followed before what is left comes out evenly.
const LAPS: usize = 4;

/// The light a lamp gives at distance `d` against its reach (lighting.wgsl's `falloff`).
pub fn falloff(d: f32, reach: f32, kind: f32) -> f32 {
    if reach <= 0.0 {
        return 0.0;
    }
    let x = d / reach;
    if x >= 1.0 {
        return 0.0;
    }
    if kind < 0.5 {
        let w = 1.0 - x * x * x * x;
        return w * w / (1.0 + 16.0 * x * x);
    }
    if kind < 1.5 {
        let w = 1.0 - x * x;
        return w * w;
    }
    if kind < 2.5 {
        return 1.0 - x;
    }
    1.0
}

/// A pipe's outline cut into equal pieces: each piece's middle, its ends, its length, and how much
/// of the travelling light leaks out at the corner after it.
struct Pieces {
    middles: Vec<Vec2>,
    ends: Vec<Vec2>,
    length: f32,
    leaks: Vec<f32>,
}

/// The closed outline `points` cut into `n` pieces (enough for about 8 world units each, 16 to
/// 256), with the corners sharper than the critical angle of index `bends` leaking light.
fn pieces_of(points: &[Vec2], bends: f32) -> Option<Pieces> {
    let points: Vec<Vec2> = points.iter().copied().filter(|p| p.is_finite()).collect();
    if points.len() < 2 {
        return None;
    }
    let m = points.len();
    let sides: Vec<f32> = (0..m).map(|k| points[k].distance(points[(k + 1) % m])).collect();
    let perimeter: f32 = sides.iter().sum();
    if perimeter.is_nan() || perimeter <= 1e-3 {
        return None;
    }
    // A whole number of pieces a glow strip, so the strips are alike.
    let n = ((perimeter / 8.0).round() as usize).clamp(16, 256);
    let n = if n > MOST_PIPE_PIECES { n.div_ceil(MOST_PIPE_PIECES) * MOST_PIPE_PIECES } else { n };
    let length = perimeter / n as f32;
    let critical = std::f32::consts::FRAC_PI_2 - (1.0 / bends.max(1.0001)).asin();
    // Where each corner is along the outline, and how much it leaks.
    let mut corners = Vec::new();
    let mut at = 0.0;
    for k in 0..m {
        at += sides[k];
        let a = points[k];
        let b = points[(k + 1) % m];
        let c = points[(k + 2) % m];
        let (u, v) = (b - a, c - b);
        if u.length_squared() > 1e-12 && v.length_squared() > 1e-12 {
            let turn = u.angle_to(v).abs();
            if turn > critical {
                corners.push((at % perimeter, ((turn - critical) / std::f32::consts::FRAC_PI_2).min(1.0)));
            }
        }
    }
    // A point `s` along the outline.
    let point_at = |s: f32| {
        let mut s = s.rem_euclid(perimeter);
        for k in 0..m {
            if s <= sides[k] || k == m - 1 {
                let t = if sides[k] > 1e-6 { (s / sides[k]).clamp(0.0, 1.0) } else { 0.0 };
                return points[k].lerp(points[(k + 1) % m], t);
            }
            s -= sides[k];
        }
        points[0]
    };
    let middles = (0..n).map(|i| point_at((i as f32 + 0.5) * length)).collect();
    let ends = (0..=n).map(|i| point_at(i as f32 * length)).collect();
    let mut leaks = vec![0.0f32; n];
    for (s, leak) in corners {
        // The corner after piece i is at the piece's end (i + 1) × length.
        let i = ((s / length).round() as usize + n - 1) % n;
        leaks[i] = 1.0 - (1.0 - leaks[i]) * (1.0 - leak);
    }
    Some(Pieces { middles, ends, length, leaks })
}

/// What each piece of pipe `pipe` (cut as `pieces`) takes in from `lights` (rgb).
fn taken_in(pipe: &Pipe, pieces: &Pieces, lights: &[LightGpu], segments: &[[f32; 4]]) -> Vec<[f32; 3]> {
    let mut input = vec![[0.0f32; 3]; pieces.middles.len()];
    let add = |to: &mut [f32; 3], c: [f32; 4], k: f32| {
        for (o, c) in to.iter_mut().zip(c) {
            *o += c.max(0.0) * k;
        }
    };
    for l in lights {
        let kind = l.p0[3];
        if !l.p1[..3].iter().all(|c| c.is_finite()) {
            continue;
        }
        if kind > 4.5 {
            // A laser or a beam: what ends on the pipe goes in there, all of it.
            let first = l.p4[2].max(0.0) as usize;
            let count = l.p4[3].max(0.0) as usize;
            let near = 0.5 * pipe.width.max(0.0) + LASER_NEAR;
            // Only where it first ends on the pipe: a beam bouncing along inside the pipe, or
            // passing through it, puts its light in once.
            for s in segments.iter().skip(first).take(count) {
                let end = Vec2::new(s[2], s[3]);
                let Some((i, d)) = nearest_piece(pieces, end) else { continue };
                if d <= near {
                    add(&mut input[i], l.p1, 1.0);
                    break;
                }
            }
            continue;
        }
        if kind > 1.5 && kind < 2.5 {
            // Suns aren't caught (a known limit).
            continue;
        }
        let size = l.p2[2].max(0.0);
        for (i, mid) in pieces.middles.iter().enumerate() {
            let from = source_point(l, *mid, segments);
            let d = from.distance(*mid);
            let mut k = falloff(d, l.p2[0], l.p2[1]);
            if k == 0.0 {
                continue;
            }
            if kind > 0.5 && kind < 1.5 && d > 1e-3 {
                let away = (*mid - from) / d;
                let cosine = away.dot(Vec2::new(l.p3[0], l.p3[1]));
                let e = ((cosine - l.p3[3]) / (l.p3[2] - l.p3[3]).max(1e-6)).clamp(0.0, 1.0);
                k *= e * e * (3.0 - 2.0 * e);
            }
            k *= pieces.length / (std::f32::consts::TAU * d.max(size + 4.0));
            add(&mut input[i], l.p1, k);
        }
    }
    input
}

/// The piece whose middle is nearest `p`, and how far `p` is from the outline there.
fn nearest_piece(pieces: &Pieces, p: Vec2) -> Option<(usize, f32)> {
    let n = pieces.middles.len();
    let mut best: Option<(usize, f32)> = None;
    for i in 0..n {
        let (a, b) = (pieces.ends[i], pieces.ends[i + 1]);
        let d = distance_to_segment(p, a, b);
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    best
}

fn distance_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Where light `l` comes from for a point `p` (lighting.wgsl's `source_point`, areas as their
/// middle).
fn source_point(l: &LightGpu, p: Vec2, segments: &[[f32; 4]]) -> Vec2 {
    let at = Vec2::new(l.p0[0], l.p0[1]);
    if l.p0[3] > 3.5 {
        let first = l.p4[2].max(0.0) as usize;
        let count = l.p4[3].max(0.0) as usize;
        let mut best = (at, f32::INFINITY);
        for s in segments.iter().skip(first).take(count) {
            let (a, b) = (Vec2::new(s[0], s[1]), Vec2::new(s[2], s[3]));
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
            let q = a + ab * t;
            let d = q.distance(p);
            if d < best.1 {
                best = (q, d);
            }
        }
        return best.0;
    }
    at
}

/// What comes out of each piece when `input` runs round the pipe, half each way, fading over
/// `carries` and leaking at corners; what is left after [`LAPS`] times round comes out evenly, so
/// everything that went in comes out.
fn given_out(pieces: &Pieces, input: &[[f32; 3]], carries: f32) -> Vec<[f32; 3]> {
    let n = input.len();
    let mut out = vec![[0.0f32; 3]; n];
    let keep = (-pieces.length / carries.max(1e-3)).exp();
    let mut left = [0.0f32; 3];
    for (i, inp) in input.iter().enumerate() {
        let total = inp[0].max(inp[1]).max(inp[2]);
        if total <= 0.0 {
            continue;
        }
        for forward in [true, false] {
            let mut t = inp.map(|c| 0.5 * c);
            let mut k = i;
            // The first piece is half crossed.
            let mut step_keep = keep.sqrt();
            for _ in 0..LAPS * n {
                for c in 0..3 {
                    out[k][c] += t[c] * (1.0 - step_keep);
                    t[c] *= step_keep;
                }
                step_keep = keep;
                // The corner after this piece (going forward) or before it (going back).
                let corner = if forward { k } else { (k + n - 1) % n };
                let leak = pieces.leaks[corner];
                if leak > 0.0 {
                    for c in 0..3 {
                        out[k][c] += t[c] * leak;
                        t[c] *= 1.0 - leak;
                    }
                }
                if t[0].max(t[1]).max(t[2]) < 1e-4 * total {
                    break;
                }
                k = if forward { (k + 1) % n } else { (k + n - 1) % n };
            }
            for c in 0..3 {
                left[c] += t[c];
            }
        }
    }
    for o in out.iter_mut() {
        for c in 0..3 {
            o[c] += left[c] / n as f32;
        }
    }
    out
}

impl LightScene {
    /// Turns every light pipe into strip lights carrying the light it catches (decision 9): call
    /// once, after every thing has added its lights. Nothing comes out of a pipe that caught
    /// nothing.
    pub fn light_pipes(&mut self) {
        if self.pipes.is_empty() {
            return;
        }
        let pipes = std::mem::take(&mut self.pipes);
        let lights_before = self.lights.len();
        for pipe in &pipes {
            let Some(pieces) = pieces_of(&pipe.points, pipe.bends) else { continue };
            let input = taken_in(pipe, &pieces, &self.lights[..lights_before], &self.segments);
            let out = given_out(&pieces, &input, pipe.carries);
            let gain = PIPE_GAIN * if pipe.glow.is_finite() { pipe.glow.max(0.0) } else { 0.0 };
            let n = out.len();
            let run = n.div_ceil(MOST_PIPE_PIECES);
            let mut start = 0;
            while start < n {
                let end = (start + run).min(n);
                let mut sum = [0.0f32; 3];
                for o in &out[start..end] {
                    for c in 0..3 {
                        sum[c] += o[c];
                    }
                }
                let colour = [0, 1, 2].map(|c| sum[c] * pipe.tint[c].clamp(0.0, 1.0) * gain);
                if colour.iter().any(|c| *c > 1e-6) {
                    let reach = (4.0 * pipe.carries).clamp(1.0, PIPE_REACH);
                    self.push_pipe_strip(&pieces.ends[start..=end], colour, pipe.width, reach);
                }
                start = end;
            }
        }
        self.pipes = pipes;
    }

    /// A strip light along `points` of light `colour`, `width` wide, reaching `reach`, casting no
    /// shadows.
    fn push_pipe_strip(&mut self, points: &[Vec2], colour: [f32; 3], width: f32, reach: f32) {
        let first = self.segments.len() as f32;
        for pair in points.windows(2) {
            self.segments.push([pair[0].x, pair[0].y, pair[1].x, pair[1].y]);
        }
        let middle = points.iter().copied().sum::<Vec2>() / points.len().max(1) as f32;
        let width = width.max(4.0);
        self.lights.push(LightGpu {
            p0: [middle.x, middle.y, PIPE_HEIGHT, 4.0],
            p1: [colour[0], colour[1], colour[2], 0.0],
            p2: [reach, 1.0, 0.5 * width, 0.0],
            p3: [1.0, 0.0, 1.0, 1.0],
            p4: [width, 0.0, first, (points.len() - 1) as f32],
            ..LightGpu::ZERO
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring(r: f32, n: usize) -> Vec<Vec2> {
        (0..n)
            .map(|k| {
                let a = k as f32 / n as f32 * std::f32::consts::TAU;
                Vec2::new(r * a.cos(), r * a.sin())
            })
            .collect()
    }

    fn square(half: f32) -> Vec<Vec2> {
        vec![Vec2::new(-half, -half), Vec2::new(half, -half), Vec2::new(half, half), Vec2::new(-half, half)]
    }

    fn pipe(points: Vec<Vec2>, carries: f32) -> Pipe {
        Pipe { points, carries, glow: 1.0, tint: [1.0; 3], bends: 1.5, width: 12.0 }
    }

    fn lamp(at: Vec2, strength: f32) -> LightGpu {
        LightGpu {
            p0: [at.x, at.y, 60.0, 0.0],
            p1: [strength; 4],
            p2: [1500.0, 0.0, 24.0, 1.0],
            p5: [0.0, 0.0, 0.0, 1.0],
            ..LightGpu::ZERO
        }
    }

    fn total(lights: &[LightGpu]) -> f32 {
        lights.iter().map(|l| l.p1[0]).sum()
    }

    #[test]
    fn no_pipes_no_lights() {
        let mut scene = LightScene { lights: vec![lamp(Vec2::ZERO, 2.0)], ..Default::default() };
        scene.light_pipes();
        assert_eq!(scene.lights.len(), 1);
        // A pipe with no light near it glows with nothing.
        let mut scene = LightScene { pipes: vec![pipe(ring(200.0, 64), 600.0)], ..Default::default() };
        scene.light_pipes();
        assert!(scene.lights.is_empty());
    }

    #[test]
    fn a_lamp_inside_a_ring_lights_it_all_evenly_and_nothing_is_made() {
        let mut scene = LightScene {
            lights: vec![lamp(Vec2::ZERO, 2.0)],
            pipes: vec![pipe(ring(200.0, 64), 600.0)],
            ..Default::default()
        };
        scene.light_pipes();
        let glow = &scene.lights[1..];
        assert_eq!(glow.len(), MOST_PIPE_PIECES);
        let (lo, hi) = glow.iter().map(|l| l.p1[0]).fold((f32::MAX, 0.0f32), |(a, b), v| (a.min(v), b.max(v)));
        assert!(hi / lo < 1.1, "even round the ring: {lo} .. {hi}");
        assert!(glow.iter().all(|l| l.p0[3] == 4.0 && l.p5[3] == 0.0), "strips that cast no shadows");
        // All the ring caught (it is all round the lamp: falloff at 200 of 1500) comes out, times the gain.
        let x: f32 = 200.0 / 1500.0;
        let caught = 2.0 * (1.0 - x.powi(4)).powi(2) / (1.0 + 16.0 * x * x);
        let out = total(glow) / PIPE_GAIN;
        assert!((out / caught - 1.0).abs() < 0.02, "out {out} against caught {caught}");
    }

    #[test]
    fn a_laser_goes_in_where_it_ends_and_fades_along_the_pipe() {
        let mut laser = LightGpu { p0: [-400.0, 0.0, 0.0, 5.0], p1: [3.0, 3.0, 3.0, 0.0], ..LightGpu::ZERO };
        laser.p4 = [2.0, 0.0, 0.0, 1.0];
        let glow_of = |carries: f32| {
            let mut scene = LightScene {
                lights: vec![laser],
                segments: vec![[-400.0, 0.0, -200.0, 0.0]],
                pipes: vec![pipe(ring(200.0, 64), carries)],
                ..Default::default()
            };
            scene.light_pipes();
            scene.lights[1..].to_vec()
        };
        for carries in [200.0, 2000.0] {
            let glow = glow_of(carries);
            let brightest = glow.iter().max_by(|a, b| a.p1[0].total_cmp(&b.p1[0])).unwrap();
            assert!(Vec2::new(brightest.p0[0], brightest.p0[1]).distance(Vec2::new(-200.0, 0.0)) < 60.0);
            assert!((total(&glow) / PIPE_GAIN / 3.0 - 1.0).abs() < 0.02, "all of it comes out");
        }
        // A laser bouncing along the pipe (each bounce ending on it) puts its light in once.
        let mut scene = LightScene {
            lights: vec![LightGpu { p4: [2.0, 0.0, 0.0, 3.0], ..laser }],
            segments: vec![[-400.0, 0.0, -200.0, 0.0], [-200.0, 0.0, 0.0, 200.0], [0.0, 200.0, 200.0, 0.0]],
            pipes: vec![pipe(ring(200.0, 64), 600.0)],
            ..Default::default()
        };
        scene.light_pipes();
        assert!((total(&scene.lights[1..]) / PIPE_GAIN / 3.0 - 1.0).abs() < 0.02, "counted once");
        // With a short Carries the far side is much dimmer than with a long one.
        let far = |glow: &[LightGpu]| {
            glow.iter().filter(|l| l.p0[0] > 150.0 && l.p0[1].abs() < 60.0).map(|l| l.p1[0]).sum::<f32>()
        };
        assert!(far(&glow_of(200.0)) * 3.0 < far(&glow_of(2000.0)));
    }

    #[test]
    fn sharp_corners_let_light_out() {
        // A square pipe (90 degree corners, past glass's 48 degrees) lit from inside: its corners
        // give out more than the middles of its sides.
        let mut scene = LightScene {
            lights: vec![lamp(Vec2::ZERO, 2.0)],
            pipes: vec![Pipe { carries: 3000.0, ..pipe(square(200.0), 3000.0) }],
            ..Default::default()
        };
        scene.light_pipes();
        let glow = &scene.lights[1..];
        let at = |p: Vec2| {
            glow.iter().min_by(|a, b| {
                Vec2::new(a.p0[0], a.p0[1]).distance(p).total_cmp(&Vec2::new(b.p0[0], b.p0[1]).distance(p))
            })
        };
        let corner = at(Vec2::new(200.0, 200.0)).unwrap().p1[0];
        let side = at(Vec2::new(200.0, 0.0)).unwrap().p1[0];
        assert!(corner > 1.5 * side, "corner {corner} against side {side}");
        // Glass's corners leak at 90 degrees; an index that hardly bends holds next to nothing, so
        // its corners leak more; 45 degree corners hold glass's light in.
        let glass = pieces_of(&square(200.0), 1.5).unwrap();
        assert_eq!(glass.leaks.iter().filter(|l| **l > 0.0).count(), 4);
        let thin = pieces_of(&square(200.0), 1.0001).unwrap();
        let most = |p: &Pieces| p.leaks.iter().copied().fold(0.0f32, f32::max);
        assert!(most(&thin) > most(&glass));
        let octagon = pieces_of(&ring(200.0, 8), 1.5).unwrap();
        assert!(octagon.leaks.iter().all(|l| *l == 0.0));
    }

    #[test]
    fn falloff_matches_the_shader() {
        let src = crate::LIGHTING_WGSL;
        assert!(src.contains("return w * w / (1.0 + 16.0 * x * x);"));
        assert_eq!(falloff(0.0, 100.0, 0.0), 1.0);
        assert_eq!(falloff(100.0, 100.0, 1.0), 0.0);
        assert!((falloff(50.0, 100.0, 2.0) - 0.5).abs() < 1e-6);
    }
}
