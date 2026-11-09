//! The outline toolbox (workstream W14): an outline's middle, its points and contours, a point some
//! share of the way round it, and whether a point is inside. Every thing that reads a wall's outline
//! (lights, particles, texts, the camera, sensing) uses these, so they all agree.

use bpe_runtime::Outline;
use glam::Vec2;

/// How many points a round outline (a circle, an ellipse) is cut into.
pub const ROUND_POINTS: usize = 96;

/// The middle of an outline.
pub fn centre(outline: &Outline) -> Vec2 {
    outline.center()
}

/// An outline's centre line in the world as a closed list of points, starting at its top.
pub fn points(outline: &Outline) -> Vec<Vec2> {
    let round = |center: Vec2, half: Vec2, angle: f32| -> Vec<Vec2> {
        let turn = Vec2::from_angle(angle);
        (0..ROUND_POINTS)
            .map(|k| {
                let t = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * k as f32 / ROUND_POINTS as f32;
                center + turn.rotate(Vec2::new(half.x * t.cos(), half.y * t.sin()))
            })
            .collect()
    };
    match outline {
        Outline::Ring { center, radius, sides, .. } if *sides < 3 => round(*center, Vec2::splat(*radius), 0.0),
        Outline::Ring { .. } | Outline::Box { .. } => outline.corners(),
        Outline::Ellipse { center, half, angle, .. } => round(*center, *half, *angle),
        Outline::Path { center, angle, scale, points, .. } => {
            let turn = Vec2::from_angle(*angle);
            points.iter().map(|p| *center + turn.rotate(*p * *scale)).collect()
        }
        // A Text's letters: its longest contour (what wants one closed list gets one letter's
        // outline; what wants every side reads `contours`).
        Outline::Letters { .. } => contours(outline).into_iter().max_by_key(Vec::len).unwrap_or_default(),
    }
}

/// Every closed contour of an outline's centre line in the world: one for every outline but a
/// Text's letters, which give each letter's every contour.
pub fn contours(outline: &Outline) -> Vec<Vec<Vec2>> {
    match outline {
        Outline::Letters { center, angle, letters, .. } => {
            let turn = Vec2::from_angle(*angle);
            letters
                .list
                .iter()
                .flat_map(|l| l.contours())
                .map(|c| c.iter().map(|p| *center + turn.rotate(*p)).collect())
                .collect()
        }
        _ => vec![points(outline)],
    }
}

/// The point `share` of the way round the closed contours `contours`, by length, walking them in
/// order (0 is the first one's first point; the share wraps, so 1 is there again), and the way out
/// of its contour there (away from that contour's middle, up when there is none). `None` with no
/// points.
pub fn round(contours: &[&[Vec2]], share: f32) -> Option<(Vec2, Vec2)> {
    let side = |c: &[Vec2], i: usize| c[i].distance(c[(i + 1) % c.len()]);
    let total: f32 = contours.iter().map(|c| (0..c.len()).map(|i| side(c, i)).sum::<f32>()).sum();
    // A share already from 0 to 1 is kept as it is (what particles have always been born from).
    let share = if (0.0..1.0).contains(&share) { share } else { share.rem_euclid(1.0) };
    let mut left = share * total;
    let last = contours.iter().rposition(|c| !c.is_empty())?;
    for (k, c) in contours.iter().enumerate() {
        let n = c.len();
        if n == 0 {
            continue;
        }
        let length: f32 = (0..n).map(|i| side(c, i)).sum();
        if left > length && k != last {
            left -= length;
            continue;
        }
        let middle = c.iter().copied().sum::<Vec2>() / n as f32;
        for i in 0..n {
            let l = side(c, i);
            if left <= l || i == n - 1 {
                let (a, b) = (c[i], c[(i + 1) % n]);
                let at = a.lerp(b, if l > 0.0 { (left / l).clamp(0.0, 1.0) } else { 0.0 });
                return Some((at, (at - middle).try_normalize().unwrap_or(Vec2::NEG_Y)));
            }
            left -= l;
        }
    }
    None
}

/// [`round`] on one closed contour.
pub fn round_one(points: &[Vec2], share: f32) -> Option<(Vec2, Vec2)> {
    round(&[points], share)
}

/// Whether `p` is inside the closed polygon `points` (even-odd).
pub fn inside(points: &[Vec2], p: Vec2) -> bool {
    bpe_core::shape_path::inside_polygon(points, p)
}

/// Whether `p` is inside the closed contours taken together (even-odd, so holes are holes).
pub fn inside_any(contours: &[Vec<Vec2>], p: Vec2) -> bool {
    bpe_core::shape_path::inside_contours(contours, p)
}

/// The smallest box round every point of `contours`: `None` with no points.
pub fn bounds<'a>(contours: impl IntoIterator<Item = &'a Vec2>) -> Option<(Vec2, Vec2)> {
    let (lo, hi) = contours
        .into_iter()
        .fold((Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
    (lo.x <= hi.x).then_some((lo, hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUARE: [Vec2; 4] = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)];

    #[test]
    fn a_point_goes_round_by_its_share() {
        let at = |share: f32| round_one(&SQUARE, share).unwrap().0;
        assert_eq!(at(0.0), Vec2::ZERO);
        assert_eq!(at(0.125), Vec2::new(5.0, 0.0));
        assert_eq!(at(0.5), Vec2::new(10.0, 10.0));
        assert_eq!(at(1.25), Vec2::new(10.0, 0.0), "the share wraps");
        assert_eq!(at(1.0), Vec2::ZERO, "all the way round is the start again");
        assert_eq!(round_one(&SQUARE, 0.125).unwrap().1, Vec2::new(0.0, -5.0).normalize(), "out of the middle");
        assert_eq!(round_one(&[], 0.3), None);
    }

    #[test]
    fn a_point_goes_round_several_contours_in_turn() {
        let far: Vec<Vec2> = SQUARE.iter().map(|p| *p + Vec2::new(100.0, 0.0)).collect();
        let both = [&SQUARE[..], &far[..]];
        let at = |share: f32| round(&both, share).unwrap().0;
        assert_eq!(at(0.0), Vec2::ZERO);
        assert_eq!(at(0.25), Vec2::new(10.0, 10.0), "half way round the first");
        assert_eq!(at(0.5625), Vec2::new(105.0, 0.0), "on along the second");
        assert_eq!(at(0.625), Vec2::new(110.0, 0.0));
        assert_eq!(round(&[&[], &SQUARE[..]], 0.125).unwrap().0, Vec2::new(5.0, 0.0), "empty contours are skipped");
    }

    /// The particles' own edge point before W14: the same numbers for every share they are born at.
    #[test]
    fn the_edge_point_is_the_particles_old_one() {
        let old = |points: &[Vec2], t: f32| -> Option<(Vec2, Vec2)> {
            let n = points.len();
            let len = |i: usize| points[i].distance(points[(i + 1) % n]);
            let total: f32 = (0..n).map(len).sum();
            let mut left = t.clamp(0.0, 1.0) * total;
            let middle = points.iter().copied().sum::<Vec2>() / n as f32;
            for i in 0..n {
                let l = len(i);
                if left <= l || i == n - 1 {
                    let (a, b) = (points[i], points[(i + 1) % n]);
                    let at = a.lerp(b, if l > 0.0 { (left / l).clamp(0.0, 1.0) } else { 0.0 });
                    return Some((at, (at - middle).try_normalize().unwrap_or(Vec2::NEG_Y)));
                }
                left -= l;
            }
            None
        };
        let ring = Outline::Ring { center: Vec2::new(3.0, -7.0), radius: 55.0, sides: 0, angle: 0.0, thickness: 4.0 };
        let shapes = [points(&ring), SQUARE.to_vec(), vec![Vec2::ONE; 3]];
        for p in &shapes {
            for k in 0..997 {
                let t = bpe_core::dice::unit24((k as u32).wrapping_mul(2_654_435_761));
                assert_eq!(round_one(p, t), old(p, t), "{t}");
            }
        }
    }

    #[test]
    fn a_round_outline_starts_at_its_top() {
        let ring = Outline::Ring { center: Vec2::new(5.0, 5.0), radius: 10.0, sides: 0, angle: 0.0, thickness: 4.0 };
        let points = points(&ring);
        assert_eq!(points.len(), ROUND_POINTS);
        assert!(points[0].distance(Vec2::new(5.0, -5.0)) < 1e-4, "{:?}", points[0]);
        assert_eq!(centre(&ring), Vec2::new(5.0, 5.0));
    }

    #[test]
    fn inside_counts_holes() {
        let hole: Vec<Vec2> = SQUARE.iter().map(|p| *p * 0.4 + Vec2::splat(3.0)).collect();
        assert!(inside(&SQUARE, Vec2::splat(5.0)));
        assert!(!inside_any(&[SQUARE.to_vec(), hole.clone()], Vec2::splat(5.0)));
        assert!(inside_any(&[SQUARE.to_vec(), hole], Vec2::new(1.0, 1.0)));
        assert_eq!(bounds(SQUARE.iter()), Some((Vec2::ZERO, Vec2::splat(10.0))));
        assert_eq!(bounds([].iter()), None);
    }
}
