//! Putting a layout's letters along a path at a moment: straight as laid out, round a circle, on a
//! wave, or along a shape's edge. Each letter is placed by its baseline middle (its origin plus
//! half its advance along the line), the straight layout's x being how far along the path it is,
//! the text's middle (x = 0) at the path's start. Not cached: paths move.

use std::f32::consts::{PI, TAU};

use glam::Vec2;

use super::layout::{Letter, TextLayout};

/// A letter's origin in the text's frame and its turn (radians clockwise).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub at: Vec2,
    pub turn: f32,
}

/// The path the letters follow, in the text's frame.
#[derive(Clone, Debug, PartialEq)]
pub enum PathAt {
    Straight,
    /// Round a circle about the text's middle: `start` the angle (radians, 0 at the top,
    /// clockwise) the text's middle sits at; `inside` reads from inside, the letters' tops toward
    /// the middle.
    Circle {
        radius: f32,
        start: f32,
        inside: bool,
    },
    /// On a wave y = height · sin(2π (x / wavelength + shift)), `shift` in cycles; `follow` turns
    /// the letters with its slope.
    Wave {
        height: f32,
        wavelength: f32,
        shift: f32,
        follow: bool,
    },
    /// Along a closed outline (in the text's frame, going clockwise from its first corner):
    /// `start` 0 … 1 round it where the text's middle sits, `gap` px off it (outward positive).
    Edge {
        points: Vec<Vec2>,
        start: f32,
        gap: f32,
    },
}

/// Every letter of `layout` placed along `path`.
pub fn arrange(layout: &TextLayout, path: &PathAt) -> Vec<Placed> {
    let straight = || layout.letters.iter().map(|l| Placed { at: l.origin, turn: 0.0 }).collect();
    // Lines after the first move a line step toward the path's middle.
    let first_line = layout.letters.first().map_or(0.0, |l| l.origin.y);
    let line_of = |l: &Letter| if layout.line_step > 0.0 { (l.origin.y - first_line) / layout.line_step } else { 0.0 };
    let mid = |l: &Letter| l.origin.x + l.advance * 0.5;
    match *path {
        PathAt::Straight => straight(),
        PathAt::Circle { radius, start, inside } => {
            if !(radius.is_finite() && radius > 1e-3) {
                return straight();
            }
            layout
                .letters
                .iter()
                .map(|l| {
                    let r = (radius - line_of(l) * layout.line_step).max(1e-3);
                    let sign = if inside { -1.0 } else { 1.0 };
                    let theta = start + sign * mid(l) / r;
                    let base = Vec2::new(theta.sin(), -theta.cos()) * r;
                    let turn = if inside { theta + PI } else { theta };
                    Placed { at: base - Vec2::from_angle(turn) * (l.advance * 0.5), turn }
                })
                .collect()
        }
        PathAt::Wave { height, wavelength, shift, follow } => {
            if !(wavelength.is_finite() && wavelength.abs() > 1e-3 && height.is_finite() && shift.is_finite()) {
                return straight();
            }
            layout
                .letters
                .iter()
                .map(|l| {
                    let x = mid(l);
                    let phase = TAU * (x / wavelength + shift);
                    let base = Vec2::new(x, l.origin.y + height * phase.sin());
                    let turn = if follow { (height * TAU / wavelength * phase.cos()).atan() } else { 0.0 };
                    Placed { at: base - Vec2::from_angle(turn) * (l.advance * 0.5), turn }
                })
                .collect()
        }
        PathAt::Edge { ref points, start, gap } => {
            let Some(edge) = Edge::new(points) else { return straight() };
            let start = if start.is_finite() { start } else { 0.0 };
            let gap = if gap.is_finite() { gap } else { 0.0 };
            layout
                .letters
                .iter()
                .map(|l| {
                    let (p, dir) = edge.at(start * edge.length + mid(l));
                    let out = Vec2::new(dir.y, -dir.x);
                    let base = p + out * (gap - line_of(l) * layout.line_step);
                    let turn = dir.y.atan2(dir.x);
                    Placed { at: base - dir * (l.advance * 0.5), turn }
                })
                .collect()
        }
    }
}

/// `letter`'s contours turned by `p.turn` about its origin, then put at `p.at`.
pub fn placed(letter: &Letter, p: Placed) -> Vec<Vec<Vec2>> {
    let turn = Vec2::from_angle(p.turn);
    letter.contours.iter().map(|c| c.iter().map(|q| p.at + turn.rotate(*q)).collect()).collect()
}

/// A closed outline measured along its length, going clockwise on screen.
struct Edge {
    points: Vec<Vec2>,
    /// How far along each point is.
    along: Vec<f32>,
    length: f32,
}

impl Edge {
    fn new(points: &[Vec2]) -> Option<Edge> {
        if points.len() < 3 || points.iter().any(|p| !p.is_finite()) {
            return None;
        }
        let mut points = points.to_vec();
        // Clockwise on screen (y down) has a positive shoelace sum; turn it round about its first
        // corner when it goes the other way.
        let area: f32 = (0..points.len()).map(|k| points[k].perp_dot(points[(k + 1) % points.len()])).sum();
        if area < 0.0 {
            points[1..].reverse();
        }
        let mut along = Vec::with_capacity(points.len());
        let mut length = 0.0;
        for k in 0..points.len() {
            along.push(length);
            length += (points[(k + 1) % points.len()] - points[k]).length();
        }
        (length > 1e-3).then_some(Edge { points, along, length })
    }

    /// The point `s` along it (wrapping round) and the way it goes there.
    fn at(&self, s: f32) -> (Vec2, Vec2) {
        let s = s.rem_euclid(self.length);
        let k = self.along.partition_point(|a| *a <= s).saturating_sub(1);
        let (a, b) = (self.points[k], self.points[(k + 1) % self.points.len()]);
        let len = (b - a).length().max(1e-9);
        let dir = (b - a) / len;
        (a + dir * (s - self.along[k]).min(len), dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::fonts::{find_in, repo_fonts_dir};
    use crate::text::layout::{Align, Case, LayoutSpec, layout};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn laid(words: &str) -> Arc<TextLayout> {
        let font = find_in(Some(&repo_fonts_dir()), &BTreeMap::new(), "Anton", 400.0).font().clone();
        layout(
            &font,
            &LayoutSpec {
                words,
                size: 60.0,
                weight: 400.0,
                slant: 0.0,
                letter_spacing: 0.0,
                line_spacing: 1.0,
                align: Align::Centre,
                wrap: 0.0,
                case: Case::AsTyped,
            },
        )
    }

    /// A placed letter's baseline middle.
    fn middle(l: &Letter, p: Placed) -> Vec2 {
        p.at + Vec2::from_angle(p.turn) * (l.advance * 0.5)
    }

    #[test]
    fn straight_keeps_the_layout() {
        let l = laid("HELLO");
        for (letter, p) in l.letters.iter().zip(arrange(&l, &PathAt::Straight)) {
            assert_eq!((p.at, p.turn), (letter.origin, 0.0));
            assert_eq!(placed(letter, p)[0][0], letter.origin + letter.contours[0][0]);
        }
    }

    #[test]
    fn a_circle_puts_letters_on_the_radius_facing_out() {
        let l = laid("ROUNDABOUT");
        let start = 0.4;
        let ps = arrange(&l, &PathAt::Circle { radius: 300.0, start, inside: false });
        for (letter, p) in l.letters.iter().zip(&ps) {
            assert!((middle(letter, *p).length() - 300.0).abs() < 0.5);
        }
        assert!(ps.windows(2).all(|w| w[1].turn > w[0].turn), "clockwise");
        // The middle of the text sits at Starts at: halfway between the two middle letters.
        let (a, b) = (middle(&l.letters[4], ps[4]), middle(&l.letters[5], ps[5]));
        let at = ((a + b) * 0.5).normalize();
        assert!((at - Vec2::new(start.sin(), -start.cos())).length() < 0.02, "{at}");
        // A letter at the top faces out: its up (−y) points away from the middle.
        let p = ps[4];
        let up = Vec2::from_angle(p.turn).rotate(Vec2::NEG_Y);
        assert!(up.dot(middle(&l.letters[4], p).normalize()) > 0.99);
        // From inside: the other way round, tops toward the middle.
        let inside = arrange(&l, &PathAt::Circle { radius: 300.0, start, inside: true });
        assert!(inside.windows(2).all(|w| w[1].turn < w[0].turn), "counter-clockwise");
        let p = inside[4];
        let up = Vec2::from_angle(p.turn).rotate(Vec2::NEG_Y);
        assert!(up.dot(middle(&l.letters[4], p).normalize()) < -0.99);
        for (letter, p) in l.letters.iter().zip(&inside) {
            assert!((middle(letter, *p).length() - 300.0).abs() < 0.5);
        }
    }

    #[test]
    fn later_lines_go_in_by_a_line_step() {
        let l = laid("AB\nCD");
        let ps = arrange(&l, &PathAt::Circle { radius: 300.0, start: 0.0, inside: false });
        let r = |k: usize| middle(&l.letters[k], ps[k]).length();
        assert!((r(0) - 300.0).abs() < 0.5);
        assert!((r(2) - (300.0 - l.line_step)).abs() < 0.5, "{} {}", r(2), l.line_step);
    }

    #[test]
    fn a_wave_follows_the_curve() {
        let l = laid("WAVYWAVY");
        let (height, wavelength, shift) = (40.0, 300.0, 0.25);
        let ps = arrange(&l, &PathAt::Wave { height, wavelength, shift, follow: true });
        for (letter, p) in l.letters.iter().zip(&ps) {
            let m = middle(letter, *p);
            let x = letter.origin.x + letter.advance * 0.5;
            let phase = TAU * (x / wavelength + shift);
            assert!((m.x - x).abs() < 1e-3);
            assert!((m.y - (letter.origin.y + height * phase.sin())).abs() < 1e-3);
            let slope = height * TAU / wavelength * phase.cos();
            assert!((p.turn - slope.atan()).abs() < 1e-5);
        }
        let flat = arrange(&l, &PathAt::Wave { height, wavelength, shift, follow: false });
        assert!(flat.iter().all(|p| p.turn == 0.0));
    }

    #[test]
    fn an_edge_runs_along_the_outline() {
        let l = laid("EDGE");
        let square: Vec<Vec2> =
            [(-200.0, -200.0), (200.0, -200.0), (200.0, 200.0), (-200.0, 200.0)].map(|(x, y)| Vec2::new(x, y)).into();
        let on_square = |p: Vec2| {
            let d = p.abs();
            (d.x.max(d.y) - 200.0).abs()
        };
        // Start 1/8 round: the middle of the top side.
        let ps = arrange(&l, &PathAt::Edge { points: square.clone(), start: 0.125, gap: 0.0 });
        for (letter, p) in l.letters.iter().zip(&ps) {
            let m = middle(letter, *p);
            assert!(on_square(m) < 0.5, "{m}");
            assert!((m.y + 200.0).abs() < 0.5, "on the top side: {m}");
            assert!(p.turn.abs() < 1e-5);
        }
        // Half way round: the middle of the bottom side, upside down.
        let ps = arrange(&l, &PathAt::Edge { points: square.clone(), start: 0.625, gap: 0.0 });
        for (letter, p) in l.letters.iter().zip(&ps) {
            let m = middle(letter, *p);
            assert!((m.y - 200.0).abs() < 0.5, "on the bottom side: {m}");
        }
        // A gap moves the letters out.
        let ps = arrange(&l, &PathAt::Edge { points: square.clone(), start: 0.125, gap: 30.0 });
        assert!((middle(&l.letters[0], ps[0]).y + 230.0).abs() < 0.5);
        // Given the other way round, it is turned round.
        let mut back = square.clone();
        back[1..].reverse();
        assert_eq!(arrange(&l, &PathAt::Edge { points: back, start: 0.125, gap: 0.0 }), {
            arrange(&l, &PathAt::Edge { points: square, start: 0.125, gap: 0.0 })
        });
    }

    #[test]
    fn degenerate_paths_do_not_panic() {
        let l = laid("ODD");
        let straight = arrange(&l, &PathAt::Straight);
        for path in [
            PathAt::Circle { radius: 0.0, start: 0.0, inside: false },
            PathAt::Circle { radius: f32::NAN, start: 0.0, inside: false },
            PathAt::Wave { height: 10.0, wavelength: 0.0, shift: 0.0, follow: true },
            PathAt::Edge { points: vec![Vec2::ZERO, Vec2::X], start: 0.0, gap: 0.0 },
            PathAt::Edge { points: vec![Vec2::ZERO; 5], start: 0.0, gap: 0.0 },
        ] {
            assert_eq!(arrange(&l, &path), straight, "{path:?}");
        }
        assert!(arrange(&laid(""), &PathAt::Circle { radius: 10.0, start: 0.0, inside: false }).is_empty());
    }
}
