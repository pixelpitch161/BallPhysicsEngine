//! A laser's beam, traced through the world on the CPU (lighting part 4): from the lamp along
//! its direction it meets shapes (each side of an outline a band of the wall's thickness) and
//! balls (circles), and at each one bounces like off a mirror, stops, passes through, or, for a
//! surface that answers [`Meets::Bend`], is bent going in and coming out (Snell's law, total
//! internal reflection). Part 4 never answers `Bend`: it is the hook for glass (part 2b), which
//! only has to say which things are glass. The path's corners are what the renderer draws and
//! lights from, as a strip.

use bpe_core::ThingId;
use bpe_runtime::World;
use glam::Vec2;

use super::draw::outline_points;

/// What a beam does where it meets a thing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Meets {
    /// It bounces off like off a mirror.
    Bounce,
    /// It ends there.
    Stop,
    /// It goes straight on.
    Pass,
    /// It goes in, bent as glass of this index of refraction bends light, and out again.
    Bend { index: f32 },
}

/// The beam's path: its corners from the lamp on, and whether it ended on something (rather
/// than running out of length).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path {
    pub points: Vec<Vec2>,
    pub ended_on_hit: bool,
}

/// The things a beam can meet, as the tracer sees them at one moment.
#[derive(Clone, Debug, Default)]
pub struct Scenery {
    /// Each wall: its thing, the sides of its outline, half its band's thickness.
    walls: Vec<(ThingId, Vec<(Vec2, Vec2)>, f32)>,
    /// Each live ball: its thing, its row, its middle and radius.
    balls: Vec<(ThingId, u32, Vec2, f32)>,
}

/// The world's walls and live balls.
pub fn scenery(world: &World) -> Scenery {
    let mut walls = Vec::new();
    for (&id, wall) in &world.walls {
        let points = outline_points(&wall.outline);
        let n = points.len();
        if n < 2 {
            continue;
        }
        let sides = (0..n).map(|k| (points[k], points[(k + 1) % n])).collect();
        walls.push((id, sides, 0.5 * wall.outline.thickness().max(0.0)));
    }
    let mut balls = Vec::new();
    for (&id, bodies) in &world.bodies {
        for row in 0..bodies.len() {
            if bodies.alive[row] {
                balls.push((id, row as u32, bodies.position[row], bodies.radius[row]));
            }
        }
    }
    Scenery { walls, balls }
}

/// Where a ray meets something: how far along, the surface's normal facing the ray, what it is.
#[derive(Clone, Copy, Debug)]
struct Hit {
    t: f32,
    normal: Vec2,
    thing: ThingId,
    /// The ball's row, for a ball.
    row: Option<u32>,
}

/// Rays shorter than this from where they start see nothing (so a bounce doesn't meet the
/// surface it leaves).
const EPS: f32 = 1e-3;

/// The ray `from` + t `dir` (t > `EPS`) against a circle: where it goes in (`leaving` false) or
/// comes out (true), and the outward normal there.
fn circle(from: Vec2, dir: Vec2, centre: Vec2, r: f32, leaving: bool) -> Option<(f32, Vec2)> {
    let m = from - centre;
    let b = m.dot(dir);
    let c = m.length_squared() - r * r;
    let disc = b * b - c;
    if disc < 0.0 || r <= 0.0 {
        return None;
    }
    let root = disc.sqrt();
    let t = if leaving { -b + root } else { -b - root };
    (t > EPS).then(|| (t, (from + dir * t - centre) / r))
}

/// The ray against a band of half thickness `r` round side `a`-`b` (a capsule): where it goes in
/// (`leaving` false) or comes out, and the outward normal there.
fn band(from: Vec2, dir: Vec2, a: Vec2, b: Vec2, r: f32, leaving: bool) -> Option<(f32, Vec2)> {
    let along = b - a;
    let len = along.length();
    if len <= 1e-6 {
        return circle(from, dir, a, r.max(1e-3), leaving);
    }
    let u = along / len;
    let n = Vec2::new(-u.y, u.x);
    let mut best: Option<(f32, Vec2)> = None;
    let mut keep = |cand: Option<(f32, Vec2)>| {
        if let Some((t, normal)) = cand
            && best.is_none_or(|(bt, _)| t < bt)
        {
            best = Some((t, normal));
        }
    };
    // The two long sides, offset by the band's half thickness.
    for side in [n, -n] {
        let o = a + side * r;
        let facing = dir.dot(side);
        // Going in, the ray runs against the side's normal; coming out, with it.
        if (leaving && facing <= 1e-9) || (!leaving && facing >= -1e-9) {
            continue;
        }
        let t = (o - from).dot(side) / facing;
        if t <= EPS {
            continue;
        }
        let s = (from + dir * t - a).dot(u);
        if (0.0..=len).contains(&s) {
            keep(Some((t, side)));
        }
    }
    // Round ends (a thin line has none: its one line faces whichever way the ray comes from).
    if r > 0.0 {
        keep(circle(from, dir, a, r, leaving));
        keep(circle(from, dir, b, r, leaving));
    }
    best
}

impl Scenery {
    /// The nearest thing the ray meets within `most` (`inside`: the thing it is inside, whose
    /// surface it can only leave), leaving out `skip` (a ball the laser rides).
    fn nearest(
        &self,
        from: Vec2,
        dir: Vec2,
        most: f32,
        skip: Option<(ThingId, u32)>,
        inside: Option<(ThingId, Option<u32>)>,
    ) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        let mut keep = |t: f32, normal: Vec2, thing: ThingId, row: Option<u32>| {
            if t <= most && best.is_none_or(|b| t < b.t) {
                best = Some(Hit { t, normal, thing, row });
            }
        };
        for (thing, sides, r) in &self.walls {
            let leaving = inside.is_some_and(|(id, row)| id == *thing && row.is_none());
            if inside.is_some() && !leaving {
                continue;
            }
            for &(a, b) in sides {
                if let Some((t, n)) = band(from, dir, a, b, *r, leaving) {
                    keep(t, if leaving { -n } else { n }, *thing, None);
                }
            }
        }
        for &(thing, row, centre, r) in &self.balls {
            if skip == Some((thing, row)) {
                continue;
            }
            let leaving = inside == Some((thing, Some(row)));
            if inside.is_some() && !leaving {
                continue;
            }
            if let Some((t, n)) = circle(from, dir, centre, r, leaving) {
                keep(t, if leaving { -n } else { n }, thing, Some(row));
            }
        }
        best
    }
}

/// `dir` reflected off a surface of normal `n`.
fn reflect(dir: Vec2, n: Vec2) -> Vec2 {
    dir - 2.0 * dir.dot(n) * n
}

/// `dir` bent through a surface of normal `n` (facing it) from a medium of index `from` into one
/// of index `into`, or `None` when it is all reflected (total internal reflection).
pub fn refract(dir: Vec2, n: Vec2, from: f32, into: f32) -> Option<Vec2> {
    let eta = from / into.max(1e-6);
    let cosi = -dir.dot(n);
    let k = 1.0 - eta * eta * (1.0 - cosi * cosi);
    (k >= 0.0).then(|| (dir * eta + n * (eta * cosi - k.sqrt())).normalize_or(dir))
}

/// The beam from `from` along `dir` for `length` in all, bouncing at most `bounces` times
/// (bends count as bounces), each thing answered by `meets(thing, is a ball)`.
pub fn trace(
    scenery: &Scenery,
    from: Vec2,
    dir: Vec2,
    length: f32,
    bounces: u32,
    skip: Option<(ThingId, u32)>,
    meets: &dyn Fn(ThingId, bool) -> Meets,
) -> Path {
    let mut points = vec![from];
    let Some(mut dir) = dir.try_normalize() else { return Path { points: vec![from, from], ended_on_hit: false } };
    let mut at = from;
    let mut left = length.max(0.0);
    let mut turns = 0u32;
    let mut inside: Option<(ThingId, Option<u32>, f32)> = None;
    // A beam passing through things can meet very many: a limit keeps it from looping forever.
    for _ in 0..(4 * bounces as usize + 4096) {
        let Some(hit) = scenery.nearest(at, dir, left, skip, inside.map(|(t, r, _)| (t, r))) else {
            points.push(at + dir * left);
            return Path { points, ended_on_hit: false };
        };
        let point = at + dir * hit.t;
        left -= hit.t;
        let answer = meets(hit.thing, hit.row.is_some());
        if answer == Meets::Pass && inside.is_none() {
            at = point;
            continue;
        }
        points.push(point);
        if answer == Meets::Stop || turns >= bounces {
            return Path { points, ended_on_hit: true };
        }
        turns += 1;
        at = point;
        dir = match (answer, inside) {
            (_, Some((_, _, index))) => match refract(dir, hit.normal, index, 1.0) {
                Some(out) => {
                    inside = None;
                    out
                }
                None => reflect(dir, hit.normal),
            },
            (Meets::Bend { index }, None) => match refract(dir, hit.normal, 1.0, index) {
                Some(into) => {
                    inside = Some((hit.thing, hit.row, index));
                    into
                }
                None => reflect(dir, hit.normal),
            },
            _ => reflect(dir, hit.normal),
        };
    }
    Path { points, ended_on_hit: false }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall_scenery(a: Vec2, b: Vec2, half: f32) -> Scenery {
        Scenery { walls: vec![(ThingId(7), vec![(a, b)], half)], balls: Vec::new() }
    }

    #[test]
    fn a_beam_bounces_off_a_wall_like_a_mirror() {
        let s = wall_scenery(Vec2::new(-100.0, 100.0), Vec2::new(100.0, 100.0), 5.0);
        let dir = Vec2::new(1.0, 1.0).normalize();
        let path = trace(&s, Vec2::ZERO, dir, 1000.0, 4, None, &|_, _| Meets::Bounce);
        assert_eq!(path.points.len(), 3);
        // It meets the band's near side (y = 95) and comes back up at the same angle.
        assert!((path.points[1] - Vec2::new(95.0, 95.0)).length() < 1e-3, "{:?}", path.points);
        let out = (path.points[2] - path.points[1]).normalize();
        assert!((out - Vec2::new(1.0, -1.0).normalize()).length() < 1e-4, "{out:?}");
        assert!(!path.ended_on_hit);
        let total: f32 = path.points.windows(2).map(|w| w[0].distance(w[1])).sum();
        assert!((total - 1000.0).abs() < 1e-2, "it keeps to its length: {total}");
    }

    #[test]
    fn a_beam_stops_passes_or_runs_out_of_bounces() {
        let s = wall_scenery(Vec2::new(-100.0, 100.0), Vec2::new(100.0, 100.0), 0.0);
        let down = Vec2::new(0.0, 1.0);
        let stop = trace(&s, Vec2::ZERO, down, 1000.0, 4, None, &|_, _| Meets::Stop);
        assert_eq!(stop.points, vec![Vec2::ZERO, Vec2::new(0.0, 100.0)]);
        assert!(stop.ended_on_hit);
        let pass = trace(&s, Vec2::ZERO, down, 1000.0, 4, None, &|_, _| Meets::Pass);
        assert_eq!(pass.points, vec![Vec2::ZERO, Vec2::new(0.0, 1000.0)]);
        let none = trace(&s, Vec2::ZERO, down, 1000.0, 0, None, &|_, _| Meets::Bounce);
        assert!(none.ended_on_hit && none.points.len() == 2, "no bounces left: it ends there");
    }

    #[test]
    fn a_beam_bounces_off_a_ball_and_skips_the_one_it_rides() {
        let s = Scenery { walls: Vec::new(), balls: vec![(ThingId(3), 0, Vec2::new(0.0, 100.0), 20.0)] };
        let path = trace(&s, Vec2::ZERO, Vec2::new(0.0, 1.0), 500.0, 4, None, &|_, _| Meets::Bounce);
        assert!((path.points[1] - Vec2::new(0.0, 80.0)).length() < 1e-3);
        assert!((path.points[2] - Vec2::new(0.0, -340.0)).length() < 1e-2, "straight back: {:?}", path.points);
        let ridden = trace(&s, Vec2::new(0.0, 100.0), Vec2::new(0.0, 1.0), 500.0, 4, Some((ThingId(3), 0)), &|_, _| {
            Meets::Bounce
        });
        assert_eq!(ridden.points.len(), 2, "the ball it rides doesn't stop it");
    }

    #[test]
    fn glass_bends_a_beam_in_and_out() {
        // Through a glass ball off its middle: bent in, bent out, carrying on beyond.
        let s = Scenery { walls: Vec::new(), balls: vec![(ThingId(3), 0, Vec2::new(0.0, 200.0), 50.0)] };
        let path =
            trace(&s, Vec2::new(25.0, 0.0), Vec2::new(0.0, 1.0), 1000.0, 8, None, &|_, _| Meets::Bend { index: 1.5 });
        assert_eq!(path.points.len(), 4, "{:?}", path.points);
        let out = (path.points[3] - path.points[2]).normalize();
        assert!(out.x < -0.1, "a glass ball focuses light toward its axis: {out:?}");
        // Head on, it goes straight through.
        let straight = trace(&s, Vec2::ZERO, Vec2::new(0.0, 1.0), 1000.0, 8, None, &|_, _| Meets::Bend { index: 1.5 });
        assert!((straight.points.last().unwrap().x).abs() < 1e-3);
    }

    #[test]
    fn steep_light_inside_glass_is_all_reflected() {
        let n = Vec2::new(0.0, -1.0);
        let dir = Vec2::new(0.9, 0.1).normalize();
        assert!(refract(dir, n, 1.5, 1.0).is_none(), "past the critical angle");
        assert!(refract(Vec2::new(0.0, 1.0), n, 1.5, 1.0).is_some());
    }
}
