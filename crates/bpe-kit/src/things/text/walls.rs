//! A Text's letters as a wall (Text part 3): each laid-out letter's contours, placed along its
//! path in the Text's own frame, become one [`LetterWall`] named by its part number (its place
//! among the drawn letters). Broken letters are left out.

use bpe_runtime::{LetterWall, LetterWalls};
use glam::Vec2;

use crate::text::{Placed, TextLayout, placed};

/// The letters of `layout` placed at `places` as a wall, leaving out the letters `alive` says are
/// broken.
pub fn letter_walls(layout: &TextLayout, places: &[Placed], alive: &dyn Fn(usize) -> bool) -> LetterWalls {
    let list = layout
        .letters
        .iter()
        .zip(places)
        .enumerate()
        .filter(|(k, _)| alive(*k))
        .filter_map(|(k, (l, p))| LetterWall::new(k as u32, &placed(l, *p)))
        .collect();
    LetterWalls::new(list)
}

/// The middle of each letter's box as placed, in the Text's own frame.
pub fn middles(layout: &TextLayout, places: &[Placed]) -> Vec<Vec2> {
    layout
        .letters
        .iter()
        .zip(places)
        .map(|(l, p)| p.at + Vec2::from_angle(p.turn).rotate(0.5 * (l.min + l.max) - l.origin))
        .collect()
}

/// For each letter, the point inside its stroke furthest from its outline (found on a grid over its
/// box) and how far that is: where a ball fits best inside it. A letter too thin for any grid point
/// gives its box's middle and 0.
pub fn inner_points(layout: &TextLayout, places: &[Placed]) -> Vec<(Vec2, f32)> {
    const GRID: usize = 24;
    layout
        .letters
        .iter()
        .zip(places)
        .enumerate()
        .map(|(k, (l, p))| {
            let Some(wall) = LetterWall::new(k as u32, &placed(l, *p)) else { return (p.at, 0.0) };
            let one = LetterWalls::new(vec![wall.clone()]);
            let mut best = (0.5 * (wall.lo + wall.hi), 0.0f32);
            for i in 0..GRID {
                for j in 0..GRID {
                    let t = Vec2::new((i as f32 + 0.5) / GRID as f32, (j as f32 + 0.5) / GRID as f32);
                    let q = wall.lo + (wall.hi - wall.lo) * t;
                    if !wall.inside(q) {
                        continue;
                    }
                    let d = one.nearest(q).map_or(0.0, |(n, _, _)| n.distance(q));
                    if d > best.1 {
                        best = (q, d);
                    }
                }
            }
            best
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{LayoutSpec, PathAt, arrange, fonts, layout};

    fn laid(words: &str) -> (std::sync::Arc<TextLayout>, Vec<Placed>) {
        let found = fonts::find(&Default::default(), "Anton", 400.0);
        let s = crate::things::text::TextSettings { size: 200.0, ..Default::default() };
        let spec = LayoutSpec { words, ..s.layout_spec() };
        let l = layout(found.font(), &spec);
        let p = arrange(&l, &PathAt::Straight);
        (l, p)
    }

    #[test]
    fn every_letter_is_a_part_and_broken_ones_are_left_out() {
        let (l, p) = laid("OIO");
        let all = letter_walls(&l, &p, &|_| true);
        assert_eq!(all.list.iter().map(|w| w.part).collect::<Vec<_>>(), [0, 1, 2]);
        assert_eq!(all.list[0].starts.len(), 2, "an O has its hole");
        let some = letter_walls(&l, &p, &|k| k != 1);
        assert_eq!(some.list.iter().map(|w| w.part).collect::<Vec<_>>(), [0, 2]);
        let mids = middles(&l, &p);
        assert_eq!(mids.len(), 3);
        assert!(mids[0].x < mids[1].x && mids[1].x < mids[2].x);
    }

    #[test]
    fn an_inner_point_is_inside_its_letter_away_from_the_edge() {
        let (l, p) = laid("OH");
        let walls = letter_walls(&l, &p, &|_| true);
        for (k, (q, d)) in inner_points(&l, &p).into_iter().enumerate() {
            assert!(walls.list[k].inside(q), "letter {k}");
            assert!(d > 5.0, "letter {k}: {d}");
        }
    }
}
