//! Edits to a drawn LFO shape (see [`CurvePoint`]): each takes the points and gives the new ones
//! back, always a valid shape (first point at x 0, last at x 1, in order, at most
//! [`MAX_CURVE_POINTS`]).

use bpe_core::{CurvePoint, MAX_CURVE_POINTS};

/// A flat line in the middle: what Clear leaves.
pub fn flat() -> Vec<CurvePoint> {
    vec![CurvePoint::new(0.0, 0.5, 0.0), CurvePoint::new(1.0, 0.5, 0.0)]
}

/// Makes `points` a valid shape: numbers clamped, in order, the ends at x 0 and x 1, no more
/// than two points at one x (a jump), and no more than the most points.
pub fn tidy(points: &[CurvePoint]) -> Vec<CurvePoint> {
    let mut out: Vec<CurvePoint> = points
        .iter()
        .filter(|p| p.x.is_finite() && p.y.is_finite())
        .map(|p| CurvePoint {
            x: p.x.clamp(0.0, 1.0),
            y: p.y.clamp(0.0, 1.0),
            bend: if p.bend.is_finite() { p.bend.clamp(-1.0, 1.0) } else { 0.0 },
        })
        .collect();
    out.sort_by(|a, b| a.x.total_cmp(&b.x));
    if out.len() < 2 {
        let y = out.first().map_or(0.5, |p| p.y);
        return vec![CurvePoint::new(0.0, y, 0.0), CurvePoint::new(1.0, y, 0.0)];
    }
    out[0].x = 0.0;
    let last = out.len() - 1;
    out[last].x = 1.0;
    // A run of points at one x keeps its first and last (the jump); a jump to the same height
    // is no jump.
    let mut kept: Vec<CurvePoint> = Vec::with_capacity(out.len());
    for (i, p) in out.iter().enumerate() {
        let same_before = i > 0 && out[i - 1].x == p.x;
        let same_after = i < last && out[i + 1].x == p.x;
        if same_before && same_after {
            continue;
        }
        if same_before && kept.last().is_some_and(|k| k.y == p.y) {
            let k = kept.last_mut().expect("just checked");
            k.bend = p.bend;
            continue;
        }
        kept.push(*p);
    }
    if kept.len() < 2 {
        let y = kept.first().map_or(0.5, |p| p.y);
        return vec![CurvePoint::new(0.0, y, 0.0), CurvePoint::new(1.0, y, 0.0)];
    }
    if kept.len() > MAX_CURVE_POINTS {
        let end = kept[kept.len() - 1];
        kept.truncate(MAX_CURVE_POINTS - 1);
        kept.push(end);
    }
    let n = kept.len();
    kept[n - 1].bend = 0.0;
    kept
}

/// Adds a point at (`x`, `y`); the part it lands in keeps its bend on both sides. Returns the
/// shape and the new point's place, or the shape as it was when it is full.
pub fn add_point(points: &[CurvePoint], x: f32, y: f32) -> (Vec<CurvePoint>, Option<usize>) {
    if points.len() >= MAX_CURVE_POINTS {
        return (points.to_vec(), None);
    }
    let x = x.clamp(0.0, 1.0);
    let at = points.iter().position(|p| p.x > x).unwrap_or(points.len()).max(1).min(points.len() - 1);
    let bend = points[at - 1].bend;
    let mut out = points.to_vec();
    out.insert(at, CurvePoint::new(x, y.clamp(0.0, 1.0), bend));
    (out, Some(at))
}

/// Moves point `i` to (`x`, `y`): the ends stay at x 0 and 1, the others between their
/// neighbours.
pub fn move_point(points: &[CurvePoint], i: usize, x: f32, y: f32) -> Vec<CurvePoint> {
    let mut out = points.to_vec();
    let last = out.len().saturating_sub(1);
    let Some(p) = out.get(i).copied() else { return out };
    let x = if i == 0 {
        0.0
    } else if i == last {
        1.0
    } else {
        x.clamp(out[i - 1].x, out[i + 1].x)
    };
    out[i] = CurvePoint { x, y: y.clamp(0.0, 1.0), ..p };
    out
}

/// Takes point `i` away (never an end).
pub fn delete_point(points: &[CurvePoint], i: usize) -> Vec<CurvePoint> {
    let mut out = points.to_vec();
    if i > 0 && i + 1 < out.len() {
        out.remove(i);
    }
    out
}

/// Sets how the part from point `i` to the next bows.
pub fn set_bend(points: &[CurvePoint], i: usize, bend: f32) -> Vec<CurvePoint> {
    let mut out = points.to_vec();
    if i + 1 < out.len() {
        out[i].bend = bend.clamp(-1.0, 1.0);
    }
    out
}

/// How a painted grid cell is filled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    /// Flat at the height painted.
    Step,
    /// From the bottom up to the height painted.
    RampUp,
    /// From the height painted down to the bottom.
    RampDown,
}

/// The shape with the cell from `x0` to `x1` painted at height `y`: everything inside the cell
/// goes, the shape around it stays (jumping to the cell at its edges).
pub fn paint_cell(points: &[CurvePoint], x0: f32, x1: f32, y: f32, paint: Paint) -> Vec<CurvePoint> {
    let (x0, x1) = (x0.clamp(0.0, 1.0), x1.clamp(0.0, 1.0));
    if x1 <= x0 {
        return points.to_vec();
    }
    let numbers = super::curve_numbers(points);
    let before = left_of(&numbers, x0);
    let after = bpe_dsp::lfo::curve_at(&numbers, x1 as f64 % 1.0);
    let y = y.clamp(0.0, 1.0);
    let (a, b) = match paint {
        Paint::Step => (y, y),
        Paint::RampUp => (0.0, y),
        Paint::RampDown => (y, 0.0),
    };
    let mut out: Vec<CurvePoint> = points.iter().filter(|p| p.x < x0).copied().collect();
    if x0 > 0.0 {
        // The part leading up to the cell ends where the shape was at x0.
        out.push(CurvePoint::new(x0, before, 0.0));
    }
    out.push(CurvePoint::new(x0, a, 0.0));
    out.push(CurvePoint::new(x1, b, 0.0));
    if x1 < 1.0 {
        out.push(CurvePoint::new(x1, after, points.iter().rev().find(|p| p.x <= x1).map_or(0.0, |p| p.bend)));
        out.extend(points.iter().filter(|p| p.x > x1).copied());
    }
    tidy(&out)
}

/// The height just before `x` (at a jump, the one before it).
fn left_of(numbers: &[[f32; 3]], x: f32) -> f32 {
    if x <= 0.0 {
        return numbers.first().map_or(0.5, |p| p[1]);
    }
    // Just short of x, from the left.
    let i = numbers.iter().rposition(|q| q[0] < x).unwrap_or(0);
    let a = numbers[i];
    let Some(b) = numbers.get(i + 1) else { return a[1] };
    let w = b[0] - a[0];
    if w <= 0.0 {
        return b[1];
    }
    a[1] + (b[1] - a[1]) * bpe_dsp::lfo::bend_at(((x - a[0]) / w).min(1.0), a[2])
}

/// Upside down.
pub fn flip_y(points: &[CurvePoint]) -> Vec<CurvePoint> {
    points.iter().map(|p| CurvePoint { y: 1.0 - p.y, ..*p }).collect()
}

/// Back to front.
pub fn flip_x(points: &[CurvePoint]) -> Vec<CurvePoint> {
    let n = points.len();
    let out: Vec<CurvePoint> = (0..n)
        .map(|i| {
            let p = points[n - 1 - i];
            // The part after this point was the part before it, bowing the other way.
            let bend = if i + 1 < n { -points[n - 2 - i].bend } else { 0.0 };
            CurvePoint::new(1.0 - p.x, p.y, bend)
        })
        .collect();
    tidy(&out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::curve_is_valid;

    fn p(x: f32, y: f32) -> CurvePoint {
        CurvePoint::new(x, y, 0.0)
    }

    #[test]
    fn adding_moving_and_deleting_keep_the_shape_valid() {
        let s = flat();
        let (s, at) = add_point(&s, 0.3, 0.9);
        assert_eq!(at, Some(1));
        assert_eq!(s[1], p(0.3, 0.9));
        let s = move_point(&s, 1, 2.0, -1.0);
        assert_eq!(s[1], p(1.0, 0.0), "kept before the last point and inside");
        let s = move_point(&s, 0, 0.4, 0.2);
        assert_eq!(s[0], p(0.0, 0.2), "the first point stays at the start");
        assert_eq!(delete_point(&s, 0).len(), 3, "ends stay");
        assert_eq!(delete_point(&s, 1).len(), 2);
        assert!(curve_is_valid(&s));
    }

    #[test]
    fn painting_a_cell_puts_a_step_in_the_shape() {
        let s = paint_cell(&flat(), 0.25, 0.5, 1.0, Paint::Step);
        assert!(curve_is_valid(&s), "{s:?}");
        let n = super::super::curve_numbers(&s);
        assert_eq!(bpe_dsp::lfo::curve_at(&n, 0.1), 0.5);
        assert_eq!(bpe_dsp::lfo::curve_at(&n, 0.3), 1.0);
        assert_eq!(bpe_dsp::lfo::curve_at(&n, 0.6), 0.5);
        // Painting the same cell again replaces it.
        let s = paint_cell(&s, 0.25, 0.5, 1.0, Paint::RampUp);
        let n = super::super::curve_numbers(&s);
        assert!((bpe_dsp::lfo::curve_at(&n, 0.375) - 0.5).abs() < 1e-6, "halfway up the ramp");
        assert_eq!(s.len(), 6, "{s:?}");
        // At the ends there is nothing to jump from.
        let s = paint_cell(&flat(), 0.0, 1.0, 0.25, Paint::Step);
        assert_eq!(s, vec![p(0.0, 0.25), p(1.0, 0.25)]);
    }

    #[test]
    fn flips_and_tidy() {
        let saw = vec![p(0.0, 0.0), p(1.0, 1.0)];
        assert_eq!(flip_y(&saw), vec![p(0.0, 1.0), p(1.0, 0.0)]);
        assert_eq!(flip_x(&saw), vec![p(0.0, 1.0), p(1.0, 0.0)]);
        let bent = vec![CurvePoint::new(0.0, 0.0, 0.5), p(1.0, 1.0)];
        assert_eq!(flip_x(&bent)[0].bend, -0.5);
        let messy = vec![p(0.2, 0.1), p(0.5, 0.2), p(0.5, 0.3), p(0.5, 0.4), p(0.9, 2.0)];
        assert_eq!(tidy(&messy), vec![p(0.0, 0.1), p(0.5, 0.2), p(0.5, 0.4), p(1.0, 1.0)]);
        assert!(curve_is_valid(&tidy(&[])));
    }
}
