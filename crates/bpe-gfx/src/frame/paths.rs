//! Shapes and text as paths: outlines, ellipses, letters and glyph masks.

use super::*;

impl Frame {
    /// Adds a shape: a circle of radius `scale` when `points` is None, else the closed outline
    /// through `points` (shape units, turned by `angle`, scaled by `scale`). `half_thickness` 0
    /// fills it. It draws plain in `color` until its `layers` name records. Gives its index in
    /// `paths`.
    #[allow(clippy::too_many_arguments)]
    pub fn push_path(
        &mut self,
        center: Vec2,
        angle: f32,
        scale: f32,
        half_thickness: f32,
        points: Option<&[Vec2]>,
        color: [f32; 4],
        code: f32,
    ) -> usize {
        let first = self.path_points.len();
        let (kind, reach) = match points {
            None => (0.0, 1.0),
            Some(ps) => {
                self.path_points.extend(ps.iter().map(|p| [p.x, p.y, 0.0, 0.0]));
                (1.0, ps.iter().map(|p| p.length()).fold(0.0, f32::max))
            }
        };
        let count = self.path_points.len() - first;
        self.paths.push(PathInstance {
            a: [center.x, center.y, angle, scale],
            b: [half_thickness.max(0.0), kind, first as f32, count as f32],
            c: [code, reach, 0.0, reach],
            color,
            layers: [0, 0, PathInstance::NO_PROGRAM, 0],
            hit: NO_HIT,
            clip: [0.0; 4],
        });
        self.paths.len() - 1
    }

    /// Adds an exact ellipse with half-axes `axes` (shape units, the larger one at most 1) to the
    /// shapes, as [`Frame::push_path`] adds a circle or a list of points (kind 2: see
    /// [`PathInstance`]), giving its index in `paths`.
    #[allow(clippy::too_many_arguments)]
    pub fn push_ellipse(
        &mut self,
        center: Vec2,
        angle: f32,
        scale: f32,
        half_thickness: f32,
        axes: Vec2,
        color: [f32; 4],
        code: f32,
    ) -> usize {
        let reach = axes.x.max(axes.y);
        self.paths.push(PathInstance {
            a: [center.x, center.y, angle, scale],
            b: [half_thickness.max(0.0), 2.0, axes.x, axes.y],
            c: [code, reach, 0.0, reach],
            color,
            layers: [0, 0, PathInstance::NO_PROGRAM, 0],
            hit: NO_HIT,
            clip: [0.0; 4],
        });
        self.paths.len() - 1
    }

    /// Adds one letter of a Text (Text part 1): a filled shape at scale 1 (shape units are pixels)
    /// whose outline is several closed `contours` (holes included, even-odd), turned by `angle`
    /// about `center`. Each contour's first point is marked [`CONTOUR_START`] and its last
    /// [`CONTOUR_END`] in `path_points[k].z`, with `w` its point count less one, so the shaders walk
    /// each contour's own edges (a shape's points keep 0 and close round the whole list as ever).
    /// The quad covers only the letter's box grown by `margin` px (its band, what its layers reach
    /// past it): written as the instance's 4-corner `clip`. A letter with no contour of 3 or more
    /// points draws nothing (`None`).
    pub fn push_letter(
        &mut self,
        center: Vec2,
        angle: f32,
        contours: &[Vec<Vec2>],
        margin: f32,
        color: [f32; 4],
        code: f32,
    ) -> Option<usize> {
        let first = self.path_points.len();
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        let mut reach = 0.0f32;
        for c in contours.iter().filter(|c| c.len() >= 3 && c.iter().all(|p| p.is_finite())) {
            let last = (c.len() - 1) as f32;
            for (k, p) in c.iter().enumerate() {
                let z = if k == 0 {
                    CONTOUR_START
                } else if k + 1 == c.len() {
                    CONTOUR_END
                } else {
                    0.0
                };
                self.path_points.push([p.x, p.y, z, if z > 0.0 { last } else { 0.0 }]);
                lo = lo.min(*p);
                hi = hi.max(*p);
                reach = reach.max(p.length());
            }
        }
        let count = self.path_points.len() - first;
        if count == 0 {
            return None;
        }
        let margin = if margin.is_finite() { margin.max(0.0) } else { 0.0 };
        let (lo, hi) = (lo - Vec2::splat(margin), hi + Vec2::splat(margin));
        let clip_first = self.path_points.len();
        // Round the positive way (clockwise on screen), as a convex clip's corners go.
        for p in [Vec2::new(lo.x, lo.y), Vec2::new(hi.x, lo.y), Vec2::new(hi.x, hi.y), Vec2::new(lo.x, hi.y)] {
            self.path_points.push([p.x, p.y, 0.0, 0.0]);
        }
        self.paths.push(PathInstance {
            a: [center.x, center.y, angle, 1.0],
            b: [0.0, 1.0, first as f32, count as f32],
            c: [code, reach, 0.0, reach],
            color,
            layers: [0, 0, PathInstance::NO_PROGRAM, 0],
            hit: NO_HIT,
            clip: [clip_first as f32, 4.0, 0.0, 0.0],
        });
        Some(self.paths.len() - 1)
    }

    /// Where tile `hash` (`bytes`, as [`Frame::mask_tiles`] holds them) is in this frame's tiles,
    /// adding it if it isn't there yet; `None` past the most a frame holds.
    pub fn mask_tile(&mut self, hash: u64, bytes: &std::sync::Arc<[u8]>) -> Option<u32> {
        if let Some(i) = self.mask_tiles.iter().position(|(h, _)| *h == hash) {
            return Some(i as u32);
        }
        if self.mask_tiles.len() >= crate::MAX_MASK_TILES || bytes.len() != crate::MASK_TILE_BYTES {
            return None;
        }
        self.mask_tiles.push((hash, bytes.clone()));
        Some(self.mask_tiles.len() as u32 - 1)
    }

    /// A fast letter (Text part 2): a shape at `center` turned `angle` (scale 1) whose distance is
    /// read from distance tile `tile` of [`Frame::mask_tiles`] instead of an outline ([`PATH_GLYPH`]).
    /// The tile's middle sits at `tile_at` in the shape's own frame, turned `turn`, `half` px from its
    /// middle to its sides; the quad covers the tile (its 4-corner `clip`). See paths.wgsl.
    #[allow(clippy::too_many_arguments)]
    pub fn push_glyph(
        &mut self,
        center: Vec2,
        angle: f32,
        tile_at: Vec2,
        turn: f32,
        half: f32,
        tile: u32,
        color: [f32; 4],
        code: f32,
    ) -> Option<usize> {
        if !(tile_at.is_finite() && turn.is_finite() && half.is_finite() && half > 0.0) {
            return None;
        }
        let record = self.path_points.len();
        self.path_points.push([tile_at.x, tile_at.y, turn, half]);
        let clip_first = self.path_points.len();
        let r = Vec2::from_angle(turn);
        let mut reach = 0.0f32;
        // Round the positive way (clockwise on screen), as a convex clip's corners go.
        for c in [Vec2::new(-1.0, -1.0), Vec2::new(1.0, -1.0), Vec2::new(1.0, 1.0), Vec2::new(-1.0, 1.0)] {
            let p = tile_at + r.rotate(c * half);
            reach = reach.max(p.length());
            self.path_points.push([p.x, p.y, 0.0, 0.0]);
        }
        self.paths.push(PathInstance {
            a: [center.x, center.y, angle, 1.0],
            b: [0.0, PATH_GLYPH, record as f32, tile as f32],
            c: [code, reach, 0.0, reach],
            color,
            layers: [0, 0, PathInstance::NO_PROGRAM, 0],
            hit: NO_HIT,
            clip: [clip_first as f32, 4.0, 0.0, 0.0],
        });
        Some(self.paths.len() - 1)
    }
}
