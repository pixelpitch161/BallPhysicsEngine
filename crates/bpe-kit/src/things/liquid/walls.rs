//! The walls the liquid meets, as the grid has them: every node near a wall gets the wall's
//! normal there, how far the node is from the wall's face along it (in cells, below 0 inside the
//! wall) and how fast the wall moves there. A filled wall keeps the liquid out; a hollow one is a
//! band either side of its line, open at its Gaps.

use std::collections::BTreeMap;

use bpe_core::ThingId;
use bpe_gfx::liquid_sim::{MOST_PAINTING_WALLS, WallNode};
use bpe_runtime::{Wall, World};
use glam::Vec2;

use super::state::Layout;

/// How far from a wall's face, in cells, nodes carry it.
pub const REACH: f32 = 3.0;

/// The walls liquids meet now (those balls meet: not zones).
pub fn walls_now(world: &World) -> BTreeMap<ThingId, Wall> {
    world.walls.iter().filter(|(_, w)| !w.passes).map(|(id, w)| (*id, w.clone())).collect()
}

/// Whether the walls move by themselves (spin, grow or ride something).
pub fn moving(walls: &BTreeMap<ThingId, Wall>) -> bool {
    walls.values().any(|w| w.spin != 0.0 || w.growth != 0.0 || w.velocity != Vec2::ZERO)
}

/// Half a wall's band, in pixels: 0 for a filled one.
fn half_of(w: &Wall) -> f32 {
    if w.solid {
        0.0
    } else {
        let t = w.outline.thickness();
        if t.is_finite() { t.max(0.0) * 0.5 } else { 0.0 }
    }
}

/// Where node `p` (world) is against wall `w`: the wall's normal there (off a filled wall, out of
/// a hollow one's outline), how far from its line along it (pixels, below 0 inside the outline)
/// and the wall's velocity there. `None` at an opening.
fn against(w: &Wall, p: Vec2) -> Option<(Vec2, f32, Vec2)> {
    let (q, n) = w.outline.nearest(p);
    if !q.is_finite() || !n.is_finite() {
        return None;
    }
    if !w.solid && w.gaps.as_ref().is_some_and(|g| g.open_at(&w.outline, q).is_some()) {
        return None;
    }
    let d = (p - q).length();
    let (n, dist) = if w.outline.inside(p) { (-n, -d) } else { (n, d) };
    let v = w.velocity + bpe_runtime::wall_velocity(q, w.outline.center(), w.spin, w.growth);
    Some((n, dist, if v.is_finite() { v } else { Vec2::ZERO }))
}

/// How far a node `dist` from a wall's line is from its face, in cells (below 0 inside it).
fn face(dist: f32, half: f32) -> f32 {
    if half < 0.0 { dist } else { dist.abs() - half }
}

/// The band of nodes near `walls` on the grid `layout`, in node order. Where two walls are near
/// one node, the nearer face wins.
pub fn band(walls: &BTreeMap<ThingId, Wall>, layout: &Layout) -> Vec<WallNode> {
    let mut nodes: BTreeMap<u32, WallNode> = BTreeMap::new();
    let (cols, rows) = (layout.cols as i64, layout.rows as i64);
    for (number, w) in walls.values().enumerate() {
        // Its number, for Shapes paint it (the walls after the first 64 paint nothing).
        let wall = if number < MOST_PAINTING_WALLS { number as u32 } else { 255 };
        let reach = REACH * layout.cell + half_of(w);
        // Half its band in cells; a filled wall says so with -1.
        let half = if w.solid { -1.0 } else { half_of(w) / layout.cell };
        let mut seen = std::collections::HashSet::new();
        for contour in crate::outline::contours(&w.outline) {
            let n = contour.len();
            if n < 2 {
                continue;
            }
            for k in 0..n {
                let (a, b) = (contour[k], contour[(k + 1) % n]);
                if !a.is_finite() || !b.is_finite() {
                    continue;
                }
                let lo = layout.to_grid(a.min(b) - Vec2::splat(reach));
                let hi = layout.to_grid(a.max(b) + Vec2::splat(reach));
                let (x0, y0) = ((lo.x.floor() as i64).max(0), (lo.y.floor() as i64).max(0));
                let (x1, y1) = ((hi.x.ceil() as i64).min(cols - 1), (hi.y.ceil() as i64).min(rows - 1));
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        let index = (y * cols + x) as u32;
                        if !seen.insert(index) {
                            continue;
                        }
                        let p = layout.to_world(Vec2::new(x as f32, y as f32));
                        // Near this segment's box, but is it near the wall at all?
                        let Some((n, dist, v)) = against(w, p) else { continue };
                        let dist = dist / layout.cell;
                        if face(dist, half).abs() >= REACH {
                            continue;
                        }
                        let node =
                            WallNode { index, n: n.to_array(), dist, half, v: (v / layout.cell).to_array(), wall };
                        nodes
                            .entry(index)
                            .and_modify(|e| {
                                if face(dist, half) < face(e.dist, e.half) {
                                    *e = node
                                }
                            })
                            .or_insert(node);
                    }
                }
            }
        }
    }
    nodes.into_values().collect()
}
