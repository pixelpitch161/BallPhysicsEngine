//! A course split into what stands still and what moves (Water mode part 5): the still field (the
//! carved rock and every part that doesn't move) and each moving part's shape in its own frame. The
//! card lays the moving parts over the still field each step (`drops/shaders/movers.wgsl`); the
//! drawing does the same on the processor at the moment it draws ([`Movers::field_at`]).

use std::sync::Arc;

use bpe_core::maths::Maths;
use glam::{UVec2, Vec2};

use super::{MoverShape, Pose};
use crate::solid::{DEFAULT_WETTING, SolidField};

/// One moving part: its shape, how wet its faces are, the owner number its faces are marked with
/// for the drawing's colours (its place among the course's parts plus 1, as a still part's), and
/// where it starts (its texture lies on it as it does there, and goes with it as it moves).
#[derive(Clone, Debug)]
pub struct Mover {
    pub shape: Arc<MoverShape>,
    pub wetting: f32,
    pub owner: u16,
    pub start: Pose,
}

impl Mover {
    /// The Wetting its faces wear (0 to 1).
    pub fn wet(&self) -> f32 {
        if self.wetting.is_finite() { self.wetting.clamp(0.0, 1.0) } else { DEFAULT_WETTING }
    }
}

/// The course as the water sees it when parts move: the still field, and the moving parts laid over
/// it after everything still, in this order (among moving parts the nearer face wins, so the order
/// only decides a tie).
#[derive(Clone, Debug)]
pub struct Movers {
    pub still: Arc<SolidField>,
    pub movers: Vec<Mover>,
}

impl Movers {
    pub fn is_empty(&self) -> bool {
        self.movers.is_empty()
    }

    pub fn len(&self) -> usize {
        self.movers.len()
    }

    /// Mover `k`'s pose from `poses` (its default when there is none), made sane.
    fn pose(poses: &[Pose], k: usize) -> Pose {
        poses.get(k).copied().unwrap_or_default().sane()
    }

    /// The field with every moving part at its pose in `poses`: the still field, and each part's
    /// distance (its shape's, times its scale, at the node in its own frame) taken where it is nearer
    /// (the node is then the part's: its owner and Wetting) or where the node is inside it, as
    /// [`SolidField::add_part`] lays a part. Only the nodes round each part change.
    pub fn field_at(&self, poses: &[Pose]) -> SolidField {
        let mut field = (*self.still).clone();
        let l = field.layout;
        for (k, m) in self.movers.iter().enumerate() {
            let pose = Movers::pose(poses, k);
            let Some((a, b)) = node_box(&l, pose.at, pose.at, m.shape.radius * pose.scale + l.cell) else { continue };
            let wet = m.wet();
            for y in a.y..=b.y {
                for x in a.x..=b.x {
                    let i = (y * l.cols + x) as usize;
                    let p = l.to_world(Vec2::new(x as f32, y as f32));
                    let own = m.shape.at(pose.to_own(p));
                    if own >= SolidField::FAR {
                        continue;
                    }
                    let here = own * pose.scale;
                    let d = field.distance[i];
                    if here < d || here < 0.0 {
                        field.owner[i] = m.owner;
                        field.wetting[i] = wet;
                    }
                    let d = d.min(here);
                    field.distance[i] =
                        if d.is_finite() { d.clamp(-SolidField::FAR, SolidField::FAR) } else { SolidField::FAR };
                }
            }
        }
        field
    }

    /// Where the drawing reads each moving part's texture when the parts are at `poses`: for each
    /// part, its owner number and the map from a world point to the point of the texture there,
    /// `(a, b, c, d)` taking `p` to `(a p.x - b p.y + c, b p.x + a p.y + d)` (the point the part had
    /// at its start, so its texture goes with it).
    pub fn textures(&self, poses: &[Pose]) -> Vec<(u16, [f32; 4])> {
        self.movers
            .iter()
            .enumerate()
            .map(|(k, m)| {
                let (from, now) = (m.start.sane(), Movers::pose(poses, k));
                let (s, c) = (from.turn - now.turn).det_sin_cos();
                let k = from.scale / now.scale;
                let (a, b) = (k * c, k * s);
                let t = from.at - Vec2::new(a * now.at.x - b * now.at.y, b * now.at.x + a * now.at.y);
                let map = [a, b, t.x, t.y];
                (m.owner, if map.iter().all(|v| v.is_finite()) { map } else { [1.0, 0.0, 0.0, 0.0] })
            })
            .collect()
    }

    /// The nodes (inclusive, clipped to the layout) that the parts moving between `from` and `to`
    /// can change: for each part whose pose differs, the box its reach sweeps between the two places
    /// (whatever it turns), and a cell more. `None` when no part moves (or none reaches the layout).
    pub fn moved_box(&self, from: &[Pose], to: &[Pose]) -> Option<(UVec2, UVec2)> {
        let l = &self.still.layout;
        let mut out: Option<(UVec2, UVec2)> = None;
        for (k, m) in self.movers.iter().enumerate() {
            let (a, b) = (Movers::pose(from, k), Movers::pose(to, k));
            if a == b {
                continue;
            }
            let r = m.shape.radius * a.scale.max(b.scale) + l.cell;
            if let Some((lo, hi)) = node_box(l, a.at.min(b.at), a.at.max(b.at), r) {
                out = Some(out.map_or((lo, hi), |(p, q)| (p.min(lo), q.max(hi))));
            }
        }
        out
    }

    /// How fast the fastest point of a part moves between `from` and `to` over `seconds` (pixels a
    /// second): its place's speed, its turn's at its furthest solid point, and its growth there.
    pub fn fastest(&self, from: &[Pose], to: &[Pose], seconds: f32) -> f32 {
        if seconds.is_nan() || seconds <= 0.0 {
            return 0.0;
        }
        let mut fastest = 0.0f32;
        for (k, m) in self.movers.iter().enumerate() {
            let (a, b) = (Movers::pose(from, k), Movers::pose(to, k));
            if a == b {
                continue;
            }
            let reach = m.shape.reach;
            let speed = (b.at - a.at).length()
                + (b.turn - a.turn).abs() * reach * a.scale.max(b.scale)
                + (b.scale - a.scale).abs() * reach;
            fastest = fastest.max(speed / seconds);
        }
        if fastest.is_finite() { fastest } else { f32::MAX }
    }
}

/// The nodes of `l` within `r` pixels of the box from `lo` to `hi`, clipped to the layout.
fn node_box(l: &crate::solid::Layout, lo: Vec2, hi: Vec2, r: f32) -> Option<(UVec2, UVec2)> {
    let a = l.to_grid(lo - Vec2::splat(r)).floor().max(Vec2::ZERO);
    let b = l.to_grid(hi + Vec2::splat(r)).ceil().min(Vec2::new(l.cols as f32 - 1.0, l.rows as f32 - 1.0));
    (a.x <= b.x && a.y <= b.y).then(|| (a.as_uvec2(), b.as_uvec2()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solid::{Layout, Part, Solid};

    fn course() -> SolidField {
        let layout = Layout::cover(Vec2::new(-200.0, -200.0), Vec2::new(200.0, 200.0), 4.0);
        let floor = Solid::rect(Vec2::new(-300.0, 150.0), Vec2::new(300.0, 300.0), true, 0.0);
        SolidField::build_least(layout, &[floor], 6.0).with_wetting(1.0)
    }

    fn ramp() -> Vec<Solid> {
        vec![Solid::Polygon {
            points: vec![Vec2::new(-60.0, 30.0), Vec2::new(60.0, 30.0), Vec2::new(60.0, -30.0)],
            filled: true,
            thickness: 0.0,
        }]
    }

    fn movers_at(still: SolidField, solids: &[Solid], place: Vec2) -> Movers {
        let shape = Arc::new(MoverShape::of(solids, still.layout.cell, 6.0, still.layout.origin - place).unwrap());
        Movers {
            still: Arc::new(still),
            movers: vec![Mover { shape, wetting: 0.25, owner: 2, start: Pose::new(place, 0.0, 1.0) }],
        }
    }

    fn movers(still: SolidField, solids: &[Solid]) -> Movers {
        movers_at(still, solids, Vec2::ZERO)
    }

    #[test]
    fn a_mover_at_its_place_lays_as_a_part() {
        let at = Vec2::new(20.0, 40.0);
        let mut laid = course();
        let placed: Vec<Solid> = ramp()
            .into_iter()
            .map(|s| match s {
                Solid::Polygon { points, filled, thickness } => {
                    Solid::Polygon { points: points.iter().map(|p| *p + at).collect(), filled, thickness }
                }
                s => s,
            })
            .collect();
        laid.add_part(&Part { solids: placed, cut: Vec::new(), wetting: 0.25 }, 2, 6.0);
        let moved = movers_at(course(), &ramp(), at).field_at(&[Pose::new(at, 0.0, 1.0)]);
        let cell = laid.layout.cell;
        for i in 0..laid.distance.len() {
            let (a, b) = (laid.distance[i], moved.distance[i]);
            // (At its place its grid lies on the course's: the same distances, but for rounding, within
            // the band a part changes; past it the part's own grid may say a node is nearer.)
            if a.abs() < SolidField::BAND {
                assert!((a - b).abs() < 1e-3, "node {i}: {b} is not {a}");
            }
            assert_eq!(a < 0.0, b < 0.0, "node {i}: {b} and {a}");
            // (Colours and Wetting are read only in a solid and near its faces: far out in open water a
            // node may be marked as the part's a cell further off than `add_part`'s band reaches.)
            if a.abs() > cell && a < 3.0 * cell {
                assert_eq!((laid.owner[i], laid.wetting[i]), (moved.owner[i], moved.wetting[i]), "node {i}");
            }
        }
    }

    #[test]
    fn a_moved_mover_leaves_nothing_behind() {
        let still = course();
        let m = movers(still.clone(), &ramp());
        let r = m.movers[0].shape.radius + 48.0;
        let (here, there) = (Vec2::new(-80.0, 0.0), Vec2::new(80.0, 0.0));
        let _ = m.field_at(&[Pose::new(here, 0.0, 1.0)]);
        let moved = m.field_at(&[Pose::new(there, 0.4, 1.0)]);
        let l = still.layout;
        for y in 0..l.rows {
            for x in 0..l.cols {
                let i = (y * l.cols + x) as usize;
                let p = l.to_world(Vec2::new(x as f32, y as f32));
                if p.distance(there) > r {
                    assert_eq!(moved.distance[i].to_bits(), still.distance[i].to_bits());
                    assert_eq!((moved.owner[i], moved.wetting[i]), (still.owner[i], still.wetting[i]));
                }
            }
        }
        let g = l.to_grid(here).round();
        let i = (g.y as u32 * l.cols + g.x as u32) as usize;
        assert_eq!(moved.distance[i], still.distance[i]);
    }

    #[test]
    fn the_moved_box_covers_where_it_was_and_is() {
        let m = movers(course(), &ramp());
        let (a, b) = (Pose::new(Vec2::new(-100.0, 0.0), 0.0, 1.0), Pose::new(Vec2::new(60.0, 10.0), 1.0, 1.5));
        let (lo, hi) = m.moved_box(&[a], &[b]).unwrap();
        let l = m.still.layout;
        let r = m.movers[0].shape.radius;
        let (glo, ghi) = (l.to_grid(Vec2::new(-100.0 - r, -r)), l.to_grid(Vec2::new(60.0 + 1.5 * r, 10.0 + 1.5 * r)));
        assert!(lo.x as f32 <= glo.x.max(0.0) && lo.y as f32 <= glo.y.max(0.0));
        assert!(hi.x as f32 >= ghi.x.min(l.cols as f32 - 1.0) && hi.y as f32 >= ghi.y.min(l.rows as f32 - 1.0));
        assert!(m.moved_box(&[a], &[a]).is_none());
    }

    #[test]
    fn the_fastest_point_counts_the_turn() {
        let bar = [Solid::rect(Vec2::new(-100.0, -5.0), Vec2::new(100.0, 5.0), true, 0.0)];
        let m = movers(course(), &bar);
        let a = Pose::default();
        let b = Pose::new(Vec2::ZERO, std::f32::consts::FRAC_PI_2, 1.0);
        let want = (100.0f32 * 100.0 + 5.0 * 5.0).sqrt() * std::f32::consts::FRAC_PI_2 * 60.0;
        let got = m.fastest(&[a], &[b], 1.0 / 60.0);
        assert!((got - want).abs() < 0.02 * want, "{got} is not {want}");
        assert_eq!(m.fastest(&[a], &[a], 1.0 / 60.0), 0.0);
    }

    #[test]
    fn a_parts_texture_goes_with_it() {
        let bar = [Solid::rect(Vec2::new(-100.0, -5.0), Vec2::new(100.0, 5.0), true, 0.0)];
        let mut m = movers(course(), &bar);
        let start = Pose::new(Vec2::new(10.0, -4.0), 0.3, 1.5);
        m.movers[0].start = start;
        let now = Pose::new(Vec2::new(50.0, 20.0), 1.1, 0.75);
        let [(owner, [a, b, c, d])] = m.textures(&[now])[..] else { panic!("one part") };
        assert_eq!(owner, 2);
        for own in [Vec2::ZERO, Vec2::new(30.0, -7.0), Vec2::new(-80.0, 4.0)] {
            let p = now.to_world(own);
            let got = Vec2::new(a * p.x - b * p.y + c, b * p.x + a * p.y + d);
            let want = start.to_world(own);
            assert!((got - want).length() < 1e-3, "{got} is not {want}");
        }
        // (Where it starts, its texture is the world's.)
        let [(_, map)] = m.textures(&[start])[..] else { panic!("one part") };
        let want = [1.0, 0.0, 0.0, 0.0];
        assert!(map.iter().zip(want).all(|(g, w)| (g - w).abs() < 1e-4), "{map:?}");
    }
}
