//! A painting ball's trail as one ribbon: a smooth curve through the points the ball is sampled
//! at, cut into short pieces that share their joints exactly.
//!
//! Each tick the ribbon takes the ball's new point and returns the pieces from the last point to
//! it: a Hermite curve whose end directions come from the path so far (so the ribbon is smooth
//! across points) split into pieces of at most [`MAX_PIECE`] px. A hit is a corner and is never
//! smoothed through. The ribbon narrows over the first and last [`Tune::taper`] px, and a speed
//! width is averaged over the ticks.
//!
//! A piece that continues the one before it leaves out its start cap ([`CONTINUES`]), and one
//! that is continued leaves out its end cap ([`CONTINUED`]), so see-through paint is never
//! drawn twice at a joint. Corners and the two ends of a ribbon keep round caps.

use glam::Vec2;

/// The longest piece in px.
pub const MAX_PIECE: f32 = 6.0;
/// A piece's `variant` bit: no start cap, the previous piece covers it.
pub const CONTINUES: u8 = 2;
/// A piece's `variant` bit: no end cap, the next piece covers it.
pub const CONTINUED: u8 = 4;
/// How many pieces one move is cut into at most.
const MOST_PIECES: usize = 512;
/// How far the ball may be from the last point when the ribbon ends without a piece to it, in px.
const END_GAP: f32 = 0.01;

/// One piece of a ribbon, in canvas pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub from: Vec2,
    pub to: Vec2,
    pub radius: f32,
    pub radius_to: f32,
    /// [`CONTINUES`] and [`CONTINUED`].
    pub joints: u8,
    /// For a piece that continues one: the direction of that piece, as an angle in radians. The
    /// two pieces are cut apart by the plane across it at their joint.
    pub joins_at: f32,
    /// The part of the tick's move (0 to 1) that the ball travelled along this piece.
    pub within: [f32; 2],
}

/// One leg of a move: a curve between two points.
struct Leg {
    a: Vec2,
    b: Vec2,
    h0: Vec2,
    h1: Vec2,
    /// Whether a corner follows its end.
    corner_after: bool,
}

/// What shapes a ribbon.
#[derive(Clone, Copy, Debug)]
pub struct Tune {
    /// The length in px over which each end narrows; 0 for no taper.
    pub taper: f32,
    /// How much of the way a changing width moves each tick, 0..1 (1 follows it at once).
    pub follow: f32,
}

/// What one ball's ribbon remembers between ticks.
#[derive(Clone, Debug, Default)]
pub struct Ribbon {
    /// The last point; `None` while the ball isn't painting.
    pub point: Option<Vec2>,
    /// The paint left in the ball's load.
    pub load: f32,
    /// The ball's hits when the load was last noted.
    pub hits: u32,
    /// Whether a hit happened since the last piece.
    pub corner: bool,
    /// The brush colour at the last point (linear), which the next pieces blend on from.
    pub colour: Option<[f32; 4]>,
    /// The direction the ribbon was heading at the last point (unit; zero before the first move).
    heading: Vec2,
    /// The direction of the last move, for guessing the turn.
    chord: Vec2,
    /// The ball's velocity at the last point, for finding where it bounced.
    velocity: Vec2,
    /// The width there, before the taper.
    radius: f32,
    /// How far the ribbon has run.
    length: f32,
    /// The direction of the last piece (unit; zero before the first).
    last_dir: Vec2,
    /// Whether the last piece may be continued (it wasn't a corner or an end).
    open: bool,
}

/// 0 to 1 as `t` goes 0 to 1, flat at both ends.
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn unit(v: Vec2) -> Vec2 {
    v.try_normalize().unwrap_or(Vec2::ZERO)
}

impl Ribbon {
    pub fn new(load: f32, hits: u32) -> Self {
        Self { load, hits, ..Self::default() }
    }

    /// Starts afresh at `at`: no piece yet.
    pub fn begin(&mut self, at: Vec2, velocity: Vec2, radius: f32) {
        self.point = Some(at);
        self.heading = Vec2::ZERO;
        self.chord = Vec2::ZERO;
        self.velocity = velocity;
        self.radius = radius;
        self.length = 0.0;
        self.last_dir = Vec2::ZERO;
        self.open = false;
        self.corner = false;
    }

    /// The width `length` px along a ribbon whose width there is `radius`, with `left` px of
    /// paint still in the load (`None` when it never runs out).
    fn width(&self, tune: &Tune, radius: f32, length: f32, left: Option<f32>) -> f32 {
        if tune.taper <= 0.0 {
            return radius;
        }
        radius * ease(length / tune.taper) * left.map_or(1.0, |l| ease(l / tune.taper))
    }

    /// The pieces from the last point to `here`, the ball moving at `velocity` with a width
    /// that would be `raw` at once. `endless` says the load never runs out.
    pub fn advance(&mut self, here: Vec2, velocity: Vec2, raw: f32, tune: &Tune, endless: bool) -> Vec<Piece> {
        let Some(from) = self.point else { return Vec::new() };
        let radius = self.radius + (raw - self.radius) * tune.follow.clamp(0.0, 1.0);
        let corner = std::mem::take(&mut self.corner);
        let legs = match corner.then(|| self.bounce(from, here, velocity)).flatten() {
            Some(b) => vec![
                Leg { a: from, b, h0: self.start_heading(b - from), h1: unit(b - from), corner_after: true },
                Leg { a: b, b: here, h0: unit(here - b), h1: unit(here - b), corner_after: false },
            ],
            None => {
                let chord = unit(here - from);
                // Guess the end direction by turning on as much as the path just turned.
                let end = if self.chord != Vec2::ZERO {
                    Vec2::from_angle(self.chord.angle_to(chord).clamp(-1.0, 1.0) * 0.5).rotate(chord)
                } else {
                    chord
                };
                vec![Leg { a: from, b: here, h0: self.start_heading(here - from), h1: end, corner_after: false }]
            }
        };
        let total: f32 = legs.iter().map(|l| (l.b - l.a).length()).sum::<f32>().max(1e-6);
        let mut out = Vec::new();
        let mut done = 0.0;
        let mut ended = false;
        'legs: for leg in &legs {
            let points = hermite(leg.a, leg.b, leg.h0, leg.h1);
            let count = points.len() - 1;
            for (k, pair) in points.windows(2).enumerate() {
                let (p, mut q) = (pair[0], pair[1]);
                let mut length = (q - p).length();
                let mut last = false;
                if !endless && length >= self.load {
                    // The paint runs out inside this piece.
                    q = p + (q - p) * if length > 0.0 { self.load / length } else { 0.0 };
                    length = self.load;
                    last = true;
                }
                let mix = |at: f32| self.radius + (radius - self.radius) * (at / total).min(1.0);
                let r0 = self.width(tune, mix(done), self.length, (!endless).then_some(self.load));
                self.length += length;
                self.load = (self.load - length).max(0.0);
                done += length;
                let r1 = self.width(tune, mix(done), self.length, (!endless).then_some(self.load));
                // A run ends at the paint's end and at a corner.
                let ends = last || (leg.corner_after && k + 1 == count);
                let joints = if self.open { CONTINUES } else { 0 } | if ends { 0 } else { CONTINUED };
                let joins_at = self.last_dir.to_angle();
                self.last_dir = unit(q - p);
                out.push(Piece { from: p, to: q, radius: r0, radius_to: r1, joints, joins_at, within: [0.0, 1.0] });
                self.open = !ends;
                if last {
                    ended = true;
                    break 'legs;
                }
            }
        }
        // Each piece covers its share of the move.
        let ran: f32 = out.iter().map(|p| (p.to - p.from).length()).sum();
        let mut so_far = 0.0;
        for p in &mut out {
            let start = so_far / ran.max(1e-9);
            so_far += (p.to - p.from).length();
            p.within = [start, (so_far / ran.max(1e-9)).min(1.0)];
        }
        let tail = legs.last().expect("a leg");
        if ended {
            self.point = None;
            self.open = false;
        } else {
            self.point = Some(here);
            self.heading = tail.h1;
            self.chord = unit(here - tail.a);
            self.velocity = velocity;
            self.radius = radius;
        }
        out
    }

    /// The direction a leg starting at the last point sets off in: the one the ribbon was
    /// heading in, or along the leg itself for a fresh ribbon.
    fn start_heading(&self, leg: Vec2) -> Vec2 {
        if self.heading != Vec2::ZERO && self.open { self.heading } else { unit(leg) }
    }

    /// Where the ball turned, when it was at `from` going at the old velocity and is at `here`
    /// going at `velocity`: the two paths' crossing, or the middle when they don't cross on the
    /// way; `None` when it hardly turned.
    fn bounce(&self, from: Vec2, here: Vec2, velocity: Vec2) -> Option<Vec2> {
        let (a, b) = (self.velocity, velocity);
        let det = a.perp_dot(b);
        let middle = (from + here) * 0.5;
        if det.abs() < 1e-3 * a.length() * b.length() + 1e-9 {
            return (a.dot(b) < 0.0).then_some(middle);
        }
        let d = here - from;
        // from + s a = here - t b
        let s = d.perp_dot(b) / det;
        let t = a.perp_dot(d) / det;
        let reach = d.length() * 1.5;
        Some(if s >= 0.0 && t >= 0.0 && s * a.length() <= reach && t * b.length() <= reach {
            from + a * s
        } else {
            middle
        })
    }

    /// Ends the ribbon where the ball ended up, at `end`: the pieces on from the last point to
    /// there, then a tail narrowing it away past it, at most `most` px long (the ball's radius,
    /// so the tail stays under the ball). None when it never moved. A ribbon without a taper
    /// ends in a round cap.
    pub fn finish(&mut self, end: Vec2, most: f32, tune: &Tune, endless: bool) -> Vec<Piece> {
        let mut out = Vec::new();
        if let Some(from) = self.point
            && (end - from).length() > END_GAP
        {
            let (velocity, radius) = (self.velocity, self.radius);
            out = self.advance(end, velocity, radius, tune, endless);
        }
        let Some(at) = self.point.take() else { return out };
        let dir = if self.heading != Vec2::ZERO { self.heading } else { self.chord };
        if self.length <= 0.0 || dir == Vec2::ZERO {
            return out;
        }
        let tail = tune.taper.max(1.0).min(most.max(END_GAP));
        let left = (!endless).then_some(self.load);
        let n = (tail / MAX_PIECE).ceil().max(1.0) as usize;
        let mut open = self.open;
        let mut p = at;
        for k in 0..n {
            let q = if k + 1 == n { at + dir * tail } else { at + dir * tail * (k + 1) as f32 / n as f32 };
            let (s0, s1) = (k as f32 / n as f32, (k + 1) as f32 / n as f32);
            let w = |s: f32, length: f32| {
                if tune.taper > 0.0 { self.width(tune, self.radius, length, left) * ease(1.0 - s) } else { self.radius }
            };
            let r0 = w(s0, self.length + tail * s0);
            let r1 = w(s1, self.length + tail * s1);
            let last = k + 1 == n;
            let joints = if open { CONTINUES } else { 0 } | if last { 0 } else { CONTINUED };
            out.push(Piece {
                from: p,
                to: q,
                radius: r0,
                radius_to: r1,
                joints,
                joins_at: self.last_dir.to_angle(),
                within: [1.0, 1.0],
            });
            self.last_dir = unit(q - p);
            open = true;
            p = q;
        }
        self.open = false;
        out
    }
}

/// The curve from `a` to `b` setting off along `h0` and arriving along `h1` (unit, or zero for
/// a straight line), as points at most [`MAX_PIECE`] apart that begin and end exactly at `a`
/// and `b`.
fn hermite(a: Vec2, b: Vec2, h0: Vec2, h1: Vec2) -> Vec<Vec2> {
    let span = (b - a).length();
    let at = |u: f32| {
        if h0 == Vec2::ZERO || h1 == Vec2::ZERO {
            return a + (b - a) * u;
        }
        let (u2, u3) = (u * u, u * u * u);
        a * (2.0 * u3 - 3.0 * u2 + 1.0)
            + h0 * span * (u3 - 2.0 * u2 + u)
            + b * (-2.0 * u3 + 3.0 * u2)
            + h1 * span * (u3 - u2)
    };
    let walk = |n: usize| -> Vec<Vec2> {
        (0..=n)
            .map(|k| {
                if k == 0 {
                    a
                } else if k == n {
                    b
                } else {
                    at(k as f32 / n as f32)
                }
            })
            .collect()
    };
    let longest = |pts: &[Vec2]| pts.windows(2).map(|w| (w[1] - w[0]).length()).fold(0.0, f32::max);
    let curve: f32 = walk(16).windows(2).map(|w| (w[1] - w[0]).length()).sum();
    let mut n = ((curve / MAX_PIECE).ceil() as usize).clamp(1, MOST_PIECES);
    loop {
        let pts = walk(n);
        let most = longest(&pts);
        if n >= MOST_PIECES || most <= MAX_PIECE {
            return pts;
        }
        // Enough more pieces to bring the longest down to size, in one or two goes.
        n = ((n as f32 * most / MAX_PIECE).ceil() as usize).clamp(n + 1, MOST_PIECES);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hermite_pieces_are_short_even_round_a_hairpin() {
        // Setting off and arriving backwards: a loop far longer than the chord.
        let pts = hermite(Vec2::ZERO, Vec2::new(30.0, 0.0), Vec2::new(-1.0, 0.0), Vec2::new(-1.0, 0.0));
        assert_eq!((pts[0], *pts.last().unwrap()), (Vec2::ZERO, Vec2::new(30.0, 0.0)));
        let lengths: Vec<f32> = pts.windows(2).map(|w| (w[1] - w[0]).length()).collect();
        assert!(lengths.iter().all(|&l| l <= MAX_PIECE), "{lengths:?}");
        let curve: f32 = lengths.iter().sum();
        assert!(lengths.len() as f32 <= 2.0 * curve / MAX_PIECE + 1.0, "{} pieces over {curve} px", lengths.len());
    }
}
