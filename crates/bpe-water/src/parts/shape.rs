//! A moving part's solid, sampled once in its own frame: the water and the drawing read it at the
//! part's pose instead of laying the part over the course again.

use glam::Vec2;

use crate::solid::{Solid, SolidField};

/// The most nodes a part's own grid may have (a part far bigger than any course doesn't move).
const MOST_NODES: f32 = 16.0e6;

/// A part's distance field in its own frame (its place at the origin, not turned, at scale 1):
/// node `(x, y)` is at `origin + (x, y) * cell`, its distance from the part's nearest face in
/// pixels (below 0 inside). It covers the part's box and [`SolidField::BAND`] round it; past that it
/// is [`SolidField::FAR`].
#[derive(Clone, Debug, PartialEq)]
pub struct MoverShape {
    pub origin: Vec2,
    pub cell: f32,
    pub cols: u32,
    pub rows: u32,
    pub distance: Vec<f32>,
    /// The furthest node of its grid from its origin (pixels): how far round its place it can
    /// change the field, at any turn.
    pub radius: f32,
    /// The furthest corner of its solids' box from its origin (pixels): how far its fastest point
    /// is from its place as it turns.
    pub reach: f32,
}

impl MoverShape {
    /// The field of `solids` (in the part's own frame) on a grid of `cell` pixels with a node at
    /// `align` (and every whole number of cells from it), thin walls taken as at least `least` pixels
    /// thick; `None` when no solid has anything to it or its grid is too big for the card. With
    /// `align` a node of the course's grid as seen from the part's place, the part at its place, not
    /// turned and at Scale 1, reads exactly the distances its own solids have at the course's nodes,
    /// as a part that stands still does.
    pub fn of(solids: &[Solid], cell: f32, least: f32, align: Vec2) -> Option<MoverShape> {
        let sound: Vec<&Solid> = solids.iter().filter(|s| s.is_sound()).collect();
        let (lo, cols, rows, reach) = MoverShape::grid(&sound, cell, least, align)?;
        let mut distance = Vec::with_capacity((cols * rows) as usize);
        for y in 0..rows {
            for x in 0..cols {
                let p = lo + Vec2::new(x as f32, y as f32) * cell;
                let d = sound.iter().map(|s| s.distance(p, least)).fold(SolidField::FAR, f32::min);
                distance.push(if d.is_finite() { d.clamp(-SolidField::FAR, SolidField::FAR) } else { SolidField::FAR });
            }
        }
        let far = lo + Vec2::new(cols as f32 - 1.0, rows as f32 - 1.0) * cell;
        let radius =
            [lo, far, Vec2::new(lo.x, far.y), Vec2::new(far.x, lo.y)].into_iter().map(Vec2::length).fold(0.0, f32::max);
        Some(MoverShape { origin: lo, cell, cols, rows, distance, radius, reach })
    }

    /// Whether `solids` make a shape at all on `cell`-pixel cells ([`MoverShape::of`] is `Some`):
    /// something to them, and a grid the card can hold.
    pub fn fits(solids: &[Solid], cell: f32, least: f32) -> bool {
        let sound: Vec<&Solid> = solids.iter().filter(|s| s.is_sound()).collect();
        MoverShape::grid(&sound, cell, least, Vec2::ZERO).is_some()
    }

    /// The grid of `sound`'s shape: its first node, its columns and rows, and how far its solids'
    /// box reaches from its origin.
    fn grid(sound: &[&Solid], cell: f32, least: f32, align: Vec2) -> Option<(Vec2, u32, u32, f32)> {
        let (lo, hi) = sound.iter().filter_map(|s| s.bounds(least)).reduce(|(a, b), (c, d)| (a.min(c), b.max(d)))?;
        if !(cell > 0.0 && cell.is_finite()) {
            return None;
        }
        let reach =
            [lo, hi, Vec2::new(lo.x, hi.y), Vec2::new(hi.x, lo.y)].into_iter().map(Vec2::length).fold(0.0, f32::max);
        let pad = Vec2::splat(SolidField::BAND + cell);
        let align = if align.is_finite() { align } else { Vec2::ZERO };
        let lo = align + ((lo - pad - align) / cell).floor() * cell;
        let hi = hi + pad;
        let size = ((hi - lo) / cell).ceil();
        // (A part far bigger than any course is kept to a grid the card can hold.)
        if !(size.x.is_finite() && size.y.is_finite()) || size.x * size.y > MOST_NODES {
            return None;
        }
        Some((lo, size.x as u32 + 1, size.y as u32 + 1, reach))
    }

    /// The distance at `own` (a point of its own frame, pixels), between the nodes round it;
    /// [`SolidField::FAR`] off its grid.
    pub fn at(&self, own: Vec2) -> f32 {
        let g = (own - self.origin) / self.cell;
        let (w, h) = (self.cols as f32 - 1.0, self.rows as f32 - 1.0);
        if !(g.x >= 0.0 && g.y >= 0.0 && g.x <= w && g.y <= h) {
            return SolidField::FAR;
        }
        let x = (g.x.floor() as u32).min(self.cols.saturating_sub(2));
        let y = (g.y.floor() as u32).min(self.rows.saturating_sub(2));
        let f = g - Vec2::new(x as f32, y as f32);
        let at = |x: u32, y: u32| self.distance[(y * self.cols + x) as usize];
        let top = at(x, y) + (at(x + 1, y) - at(x, y)) * f.x;
        let bottom = at(x, y + 1) + (at(x + 1, y + 1) - at(x, y + 1)) * f.x;
        top + (bottom - top) * f.y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shape_reads_its_solids_distance() {
        let rect = Solid::rect(Vec2::new(-50.0, -20.0), Vec2::new(50.0, 20.0), true, 0.0);
        let shape = MoverShape::of(std::slice::from_ref(&rect), 4.0, 6.0, Vec2::new(1.5, -0.7)).unwrap();
        for p in [
            Vec2::new(0.0, -15.0),
            Vec2::new(45.0, 0.0),
            Vec2::new(60.0, 0.0),
            Vec2::new(0.0, -30.0),
            Vec2::new(70.0, 40.0),
            Vec2::new(-33.3, 17.1),
        ] {
            let (got, want) = (shape.at(p), rect.distance(p, 6.0));
            assert!((got - want).abs() < 0.6, "at {p}: {got} is not {want}");
        }
        assert_eq!(shape.at(Vec2::new(400.0, 0.0)), SolidField::FAR);
        // (Its nodes fall on the point it was aligned to, a whole number of cells off.)
        let off = (shape.origin - Vec2::new(1.5, -0.7)) / 4.0;
        assert!((off - off.round()).abs().max_element() < 1e-4);
        assert!(shape.radius >= 50.0 + SolidField::BAND);
        assert!((shape.reach - (50.0f32 * 50.0 + 20.0 * 20.0).sqrt()).abs() < 1e-3);
    }

    #[test]
    fn a_shape_takes_thin_walls_as_least_thick() {
        // (A 2 px wall at 8 px cells, taken as 1.5 cells thick, as a still part's is.)
        let wall = Solid::Polyline {
            points: vec![Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0)],
            thickness: 2.0,
            square: false,
        };
        let shape = MoverShape::of(std::slice::from_ref(&wall), 8.0, 1.5 * 8.0, Vec2::ZERO).unwrap();
        assert!((shape.at(Vec2::new(10.0, 0.0)) + 6.0).abs() < 0.5, "{}", shape.at(Vec2::new(10.0, 0.0)));
        assert!(MoverShape::fits(std::slice::from_ref(&wall), 8.0, 12.0));
        // (One far bigger than any course has no shape.)
        let huge = Solid::rect(Vec2::splat(-30_000.0), Vec2::splat(30_000.0), true, 0.0);
        assert!(!MoverShape::fits(std::slice::from_ref(&huge), 8.0, 12.0));
        assert!(MoverShape::of(&[huge], 8.0, 12.0, Vec2::ZERO).is_none());
    }

    #[test]
    fn nothing_sound_makes_no_shape() {
        assert!(MoverShape::of(&[], 4.0, 6.0, Vec2::ZERO).is_none());
        let flat = Solid::Polygon { points: vec![Vec2::ZERO, Vec2::X], filled: true, thickness: 0.0 };
        assert!(MoverShape::of(&[flat], 4.0, 6.0, Vec2::ZERO).is_none());
    }
}
