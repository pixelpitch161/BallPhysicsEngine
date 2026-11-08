//! A wall made of letters (Text part 3): every contour of every letter of a Text, in the Text's own
//! frame, each letter with its box so a ball far from a letter skips it at once. A letter is a
//! *part* of its thing: what a contact names, so a rule, a health or a sound can tell the letters
//! apart.

use bpe_core::shape_path::nearest_on_segment;
use glam::Vec2;

/// One letter's outline: its contours (each closed) and its box, in the Text's own frame.
#[derive(Clone, Debug, PartialEq)]
pub struct LetterWall {
    /// Its part number: which drawn letter of the Text it is, from 0.
    pub part: u32,
    /// Every contour's points one after another.
    pub points: Vec<Vec2>,
    /// Where each contour starts in `points` (the last ends at its end).
    pub starts: Vec<u32>,
    pub lo: Vec2,
    pub hi: Vec2,
}

impl LetterWall {
    /// Letter `part` from its contours; contours of fewer than 3 points or with a point that isn't
    /// finite are left out, and a letter with none left is `None`.
    pub fn new(part: u32, contours: &[Vec<Vec2>]) -> Option<LetterWall> {
        let mut points = Vec::new();
        let mut starts = Vec::new();
        for c in contours.iter().filter(|c| c.len() >= 3 && c.iter().all(|p| p.is_finite())) {
            starts.push(points.len() as u32);
            points.extend_from_slice(c);
        }
        if points.is_empty() {
            return None;
        }
        let lo = points.iter().copied().fold(Vec2::splat(f32::MAX), Vec2::min);
        let hi = points.iter().copied().fold(Vec2::splat(f32::MIN), Vec2::max);
        Some(LetterWall { part, points, starts, lo, hi })
    }

    /// Each contour's points.
    pub fn contours(&self) -> impl Iterator<Item = &[Vec2]> + '_ {
        (0..self.starts.len()).map(move |k| {
            let a = self.starts[k] as usize;
            let b = self.starts.get(k + 1).map_or(self.points.len(), |&s| s as usize);
            &self.points[a..b]
        })
    }

    /// How far `p` is from the letter's box (0 inside it): never more than from its outline.
    pub fn box_distance(&self, p: Vec2) -> f32 {
        let d = (self.lo - p).max(p - self.hi).max(Vec2::ZERO);
        d.length()
    }

    /// Whether `p` is inside the letter: even-odd over every contour (a letter's contours are merged
    /// already, so its holes are holes).
    pub fn inside(&self, p: Vec2) -> bool {
        if p.x < self.lo.x || p.y < self.lo.y || p.x > self.hi.x || p.y > self.hi.y {
            return false;
        }
        let mut inside = false;
        for c in self.contours() {
            let n = c.len();
            for i in 0..n {
                let (a, b) = (c[i], c[(i + 1) % n]);
                if (a.y > p.y) != (b.y > p.y) {
                    let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
                    if p.x < x {
                        inside = !inside;
                    }
                }
            }
        }
        inside
    }

    /// The nearest point to `p` on the letter's contours, how far it is, and that piece's unit
    /// normal (either way round). Only pieces nearer than `within` are looked at; `None` when none
    /// is.
    fn nearest_within(&self, p: Vec2, within: f32) -> Option<(f32, Vec2, Vec2)> {
        let mut best: Option<(f32, Vec2, Vec2)> = None;
        let mut limit = within;
        for c in self.contours() {
            let n = c.len();
            for i in 0..n {
                let (a, b) = (c[i], c[(i + 1) % n]);
                let (_, q, d) = nearest_on_segment(a, b, p);
                if d < limit {
                    limit = d;
                    best = Some((d, q, (b - a).perp().normalize_or(Vec2::X)));
                }
            }
        }
        best
    }
}

/// Every letter of a wall made of letters, with the box round them all.
#[derive(Clone, Debug, PartialEq)]
pub struct LetterWalls {
    pub list: Vec<LetterWall>,
    pub lo: Vec2,
    pub hi: Vec2,
    /// How far the furthest point is from the middle (0, 0).
    pub reach: f32,
}

impl LetterWalls {
    pub fn new(list: Vec<LetterWall>) -> LetterWalls {
        let lo = list.iter().map(|l| l.lo).fold(Vec2::splat(f32::MAX), Vec2::min);
        let hi = list.iter().map(|l| l.hi).fold(Vec2::splat(f32::MIN), Vec2::max);
        let reach = list.iter().flat_map(|l| &l.points).map(|p| p.length_squared()).fold(0.0, f32::max).sqrt();
        LetterWalls { list, lo, hi, reach }
    }

    /// Whether `p` (the Text's own frame) is inside any letter.
    pub fn inside(&self, p: Vec2) -> bool {
        self.list.iter().any(|l| l.inside(p))
    }

    /// Whether every letter's box is at least `limit` from `p`, so `p` is outside every letter and
    /// at least that far from all of them.
    pub fn clear_of(&self, p: Vec2, limit: f32) -> bool {
        let whole = (self.lo - p).max(p - self.hi).max(Vec2::ZERO).length();
        whole >= limit || self.list.iter().all(|l| l.box_distance(p) >= limit)
    }

    /// The nearest point to `p` on any letter's contours, that piece's unit normal (either way
    /// round) and the letter's part. Letters whose box is further than the best so far are
    /// skipped. `None` with no letters.
    pub fn nearest(&self, p: Vec2) -> Option<(Vec2, Vec2, u32)> {
        let mut best: Option<(f32, Vec2, Vec2, u32)> = None;
        for l in &self.list {
            let within = best.map_or(f32::INFINITY, |b| b.0);
            if l.box_distance(p) >= within {
                continue;
            }
            if let Some((d, q, n)) = l.nearest_within(p, within) {
                best = Some((d, q, n, l.part));
            }
        }
        best.map(|(_, q, n, part)| (q, n, part))
    }

    /// How far along `d` (a unit direction from the middle) the furthest crossing of any contour
    /// is (0 with none).
    pub fn ray_far(&self, d: Vec2) -> f32 {
        let mut far: f32 = 0.0;
        for l in &self.list {
            for c in l.contours() {
                let n = c.len();
                for i in 0..n {
                    let (a, b) = (c[i], c[(i + 1) % n]);
                    let e = b - a;
                    let den = d.perp_dot(e);
                    if den.abs() < 1e-12 {
                        continue;
                    }
                    let t = a.perp_dot(e) / den;
                    let s = a.perp_dot(d) / den;
                    if t >= 0.0 && (0.0..=1.0).contains(&s) {
                        far = far.max(t);
                    }
                }
            }
        }
        far
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square letter 0 at (0..100, 0..100) with a square hole (30..70), and a square letter 1 at
    /// (200..300, 0..100).
    pub fn two_letters() -> LetterWalls {
        let sq = |x0: f32, y0: f32, s: f32| {
            vec![Vec2::new(x0, y0), Vec2::new(x0 + s, y0), Vec2::new(x0 + s, y0 + s), Vec2::new(x0, y0 + s)]
        };
        let o = LetterWall::new(0, &[sq(0.0, 0.0, 100.0), sq(30.0, 30.0, 40.0)]).unwrap();
        let i = LetterWall::new(1, &[sq(200.0, 0.0, 100.0)]).unwrap();
        LetterWalls::new(vec![o, i])
    }

    #[test]
    fn inside_honours_holes_and_every_letter() {
        let w = two_letters();
        assert!(w.inside(Vec2::new(10.0, 50.0)));
        assert!(!w.inside(Vec2::new(50.0, 50.0)), "the hole is outside");
        assert!(w.inside(Vec2::new(250.0, 50.0)));
        assert!(!w.inside(Vec2::new(150.0, 50.0)));
        assert_eq!(w.lo, Vec2::ZERO);
        assert_eq!(w.hi, Vec2::new(300.0, 100.0));
    }

    #[test]
    fn nearest_names_the_letter_and_matches_a_full_walk() {
        let w = two_letters();
        let (q, n, part) = w.nearest(Vec2::new(195.0, 50.0)).unwrap();
        assert_eq!((q, part), (Vec2::new(200.0, 50.0), 1));
        assert!((n.dot(Vec2::X)).abs() > 0.99);
        let (q, _, part) = w.nearest(Vec2::new(50.0, 45.0)).unwrap();
        assert_eq!((q, part), (Vec2::new(50.0, 30.0), 0), "in the hole: its own edge");
        // The box skip changes nothing: every point's nearest is the brute force one.
        for k in 0..400 {
            let p = Vec2::new((k % 20) as f32 * 17.0 - 20.0, (k / 20) as f32 * 7.0 - 10.0);
            let (q, _, _) = w.nearest(p).unwrap();
            let brute = w
                .list
                .iter()
                .flat_map(|l| l.contours().flat_map(|c| (0..c.len()).map(move |i| (c[i], c[(i + 1) % c.len()]))))
                .map(|(a, b)| nearest_on_segment(a, b, p).2)
                .fold(f32::INFINITY, f32::min);
            assert!((q.distance(p) - brute).abs() < 1e-3, "{p}");
        }
        assert!(LetterWalls::new(Vec::new()).nearest(Vec2::ZERO).is_none());
    }

    #[test]
    fn clear_of_and_the_ray() {
        let w = two_letters();
        assert!(w.clear_of(Vec2::new(150.0, 50.0), 40.0));
        assert!(!w.clear_of(Vec2::new(150.0, 50.0), 60.0));
        assert!(w.clear_of(Vec2::new(150.0, 500.0), 300.0));
        let d = Vec2::new(1.0, 0.1).normalize();
        assert!(w.ray_far(d) > 290.0);
        assert!((w.reach - Vec2::new(300.0, 100.0).length()).abs() < 1e-3);
        assert!(LetterWall::new(0, &[vec![Vec2::ZERO, Vec2::X]]).is_none(), "too few points");
    }
}
