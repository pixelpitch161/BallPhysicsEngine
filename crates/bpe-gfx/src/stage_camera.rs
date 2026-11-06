//! Stage camera moves (lighting part 5, plan `docs/superpowers/plans/lighting/part5.md`): the
//! maths of the camera that swings round a Stage scene, shared by the renderer's camera pass
//! (`stage_camera_pass.rs`) and the editor (which maps the pointer through it).
//!
//! World x runs right, y down, z toward the viewer; the picture plane is z = 0. The front picture
//! (every light pass works in it) is the shot of [`CameraView`]. The **rest camera** is the
//! off-axis perspective camera that frames the picture plane exactly as that shot and sits k·dist
//! above its middle (k = tan Looking down, dist from the Lens), so it tends to Stage's oblique view
//! as the lens gets long. Every point a moved camera's ray hits is coloured from the front picture
//! where the rest camera saw it ([`StageView::uv_rest`]), so with no move the picture is the front
//! picture exactly.

use bpe_runtime::CameraView;
use bytemuck::{Pod, Zeroable};
use glam::{Vec2, Vec3};

/// How a moved camera's view keeps inside what the front picture saw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FillEdges {
    /// Zoom in as little as needed so the picture never runs past what was drawn.
    #[default]
    ZoomIn,
    /// Keep the framing; the front picture's edge stretches where the camera looks past it.
    Stretch,
}

/// The Camera thing's Stage camera settings at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StageCamera {
    /// Degrees round the stage's vertical: positive swings the camera right.
    pub orbit: f32,
    /// Degrees over the stage: positive raises the camera to look down.
    pub tilt: f32,
    /// World px the camera rises (keeping the picture plane's middle in the middle).
    pub height: f32,
    /// The lens, in mm on a full-frame camera's long side (35 mm film: 36 mm).
    pub lens: f32,
    /// The depth the camera turns round (world px; 0 the picture plane, negative toward the wall).
    pub pivot: f32,
    pub fill: FillEdges,
}

impl Default for StageCamera {
    fn default() -> Self {
        Self { orbit: 0.0, tilt: 0.0, height: 0.0, lens: 35.0, pivot: 0.0, fill: FillEdges::ZoomIn }
    }
}

/// The stage as the front picture laid it out (from the Lighting card, see `LightingGpu`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StageLayout {
    /// How far behind the picture plane the back wall is.
    pub back: f32,
    /// tan(Looking down), 0 with no floor.
    pub k: f32,
    /// The floor's world y, when there is one.
    pub floor: Option<f32>,
}

/// The lens's longest the maths takes (longer is flat already, and f32 runs out).
const MOST_LENS: f32 = 10_000.0;
/// How far the fill zoom goes.
const MOST_FILL: f32 = 8.0;
/// The shade of a shape's side, of its front's colour.
pub const SIDE_SHADE: f32 = 0.62;

/// A solved stage camera: where it is, which way it looks, and the rest camera it is measured
/// from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StageView {
    /// The front picture's shot.
    pub view: CameraView,
    /// The picture in screen units (1080 across a 1080-wide video).
    pub size: Vec2,
    pub origin: Vec3,
    /// Unit vectors: the way it looks, its right and its down.
    pub forward: Vec3,
    pub right: Vec3,
    pub down: Vec3,
    /// What every ray is shifted by (world units at `dist`): the rest camera's lift and Height.
    pub shift: Vec3,
    /// From the camera to the picture plane at rest (world units).
    pub dist: f32,
    /// How far the view is zoomed in to keep its edges filled (1 none).
    pub fill: f32,
    /// The rest camera's place.
    pub rest: Vec3,
    /// How far from rest the camera is, 0 to 1 (see the plan, decision 9).
    pub motion: f32,
    pub layout: StageLayout,
}

fn rotate_x(v: Vec3, a: f32) -> Vec3 {
    let (s, c) = a.sin_cos();
    Vec3::new(v.x, v.y * c - v.z * s, v.y * s + v.z * c)
}

fn rotate_y(v: Vec3, a: f32) -> Vec3 {
    let (s, c) = a.sin_cos();
    Vec3::new(v.x * c + v.z * s, v.y, -v.x * s + v.z * c)
}

fn finite(v: f32, or: f32) -> f32 {
    if v.is_finite() { v } else { or }
}

impl StageView {
    /// The camera `cam` seen round the front shot `view` of a `size` picture laid out as `layout`.
    pub fn solve(cam: &StageCamera, view: &CameraView, size: Vec2, layout: StageLayout) -> StageView {
        let lens = finite(cam.lens, 35.0).clamp(1.0, MOST_LENS);
        let zoom = if view.zoom.is_finite() && view.zoom > 0.0 { view.zoom } else { 1.0 };
        // fov = 2 atan(18 / lens) on the picture's height: dist = (H / 2) / tan(fov / 2).
        let dist = size.y * 0.5 / zoom * lens / 18.0;
        let k = finite(layout.k, 0.0).max(0.0);
        let height = finite(cam.height, 0.0);
        let (orbit, tilt) = (finite(cam.orbit, 0.0).clamp(-89.0, 89.0), finite(cam.tilt, 0.0).clamp(-89.0, 89.0));
        let turn = |v: Vec3| rotate_y(rotate_x(v, tilt.to_radians()), orbit.to_radians());
        let (s, c) = view.turn.sin_cos();
        let (r0, d0) = (Vec3::new(c, s, 0.0), Vec3::new(-s, c, 0.0));
        let centre = view.center.extend(0.0);
        let pivot = centre + Vec3::new(0.0, 0.0, finite(cam.pivot, 0.0));
        let lift = k * dist + height;
        let at_rest = centre + Vec3::new(0.0, -lift, dist);
        let motion = (orbit.abs().max(tilt.abs()) / 2.0 + height.abs() / 20.0).clamp(0.0, 1.0);
        let mut out = StageView {
            view: *view,
            size,
            origin: pivot + turn(at_rest - pivot),
            forward: turn(Vec3::NEG_Z),
            right: turn(r0),
            down: turn(d0),
            shift: turn(Vec3::new(0.0, lift, 0.0)),
            dist,
            fill: 1.0,
            rest: centre + Vec3::new(0.0, -k * dist, dist),
            motion,
            layout,
        };
        if cam.fill == FillEdges::ZoomIn && !out.edges_inside(1.0) {
            let (mut lo, mut hi) = (1.0f32, MOST_FILL);
            if out.edges_inside(hi) {
                for _ in 0..24 {
                    let mid = 0.5 * (lo + hi);
                    if out.edges_inside(mid) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
            }
            out.fill = hi;
        }
        out
    }

    fn zoom(&self) -> f32 {
        if self.view.zoom.is_finite() && self.view.zoom > 0.0 { self.view.zoom } else { 1.0 }
    }

    /// The ray through screen point `s` (screen units, middle 0, y down) at fill zoom `fill`.
    fn ray_at(&self, s: Vec2, fill: f32) -> (Vec3, Vec3) {
        let k = self.zoom() * fill;
        (self.origin, self.forward * self.dist + self.right * (s.x / k) + self.down * (s.y / k) + self.shift)
    }

    /// Where the ray through screen point `s` starts and which way it goes (not unit length: one
    /// along it is the picture plane's distance at rest).
    pub fn ray(&self, s: Vec2) -> (Vec3, Vec3) {
        self.ray_at(s, self.fill)
    }

    /// The picture-plane point the ray through `s` meets, if it does.
    pub fn plane_at(&self, s: Vec2) -> Option<Vec2> {
        let (o, d) = self.ray(s);
        if d.z >= -1e-6 {
            return None;
        }
        let t = -o.z / d.z;
        (t > 0.0).then(|| (o + d * t).truncate())
    }

    /// Where world point `p` on the picture plane shows on screen, if in front of the camera.
    pub fn screen_of(&self, p: Vec2) -> Option<Vec2> {
        let v = p.extend(0.0) - self.origin;
        let t = v.dot(self.forward) / self.dist;
        if !(t > 1e-6) {
            return None;
        }
        let k = self.zoom() * self.fill;
        Some(Vec2::new(
            k * (v.dot(self.right) / t - self.shift.dot(self.right)),
            k * (v.dot(self.down) / t - self.shift.dot(self.down)),
        ))
    }

    /// The front picture's uv (0..1, y down) where the rest camera saw world point `x`.
    pub fn uv_rest(&self, x: Vec3) -> Vec2 {
        let along = self.dist / (self.dist - x.z).max(1e-3);
        let p = self.rest.truncate() + (x.truncate() - self.rest.truncate()) * along;
        self.uv_of(p)
    }

    /// The front picture's uv of picture-plane point `p`.
    pub fn uv_of(&self, p: Vec2) -> Vec2 {
        self.view.to_screen(p) / self.size + Vec2::splat(0.5)
    }

    /// Whether every ray round the edge of the view, at fill zoom `fill`, sees only what the
    /// front picture drew: its picture-plane point and the wall or floor behind it.
    fn edges_inside(&self, fill: f32) -> bool {
        let inside = |uv: Vec2| (-1e-4..=1.0 + 1e-4).contains(&uv.x) && (-1e-4..=1.0 + 1e-4).contains(&uv.y);
        let h = self.size * 0.5;
        let mut points = Vec::with_capacity(16);
        for i in 0..4 {
            let f = -1.0 + 2.0 * i as f32 / 4.0;
            points.extend([Vec2::new(f, -1.0), Vec2::new(1.0, f), Vec2::new(-f, 1.0), Vec2::new(-1.0, -f)]);
        }
        points.iter().all(|&e| {
            let (o, d) = self.ray_at(e * h, fill);
            if d.z >= -1e-6 {
                return false;
            }
            let t0 = -o.z / d.z;
            if !inside(self.uv_of((o + d * t0).truncate())) {
                return false;
            }
            match self.far_hit(o, d) {
                Some(x) => inside(self.uv_rest(x)),
                None => false,
            }
        })
    }

    /// Where a ray meets the back wall or the floor in front of it.
    pub fn far_hit(&self, o: Vec3, d: Vec3) -> Option<Vec3> {
        if d.z >= -1e-6 {
            return None;
        }
        let back = self.layout.back.max(0.0);
        let tw = (-back - o.z) / d.z;
        if let Some(f) = self.layout.floor
            && d.y > 1e-6
        {
            let tf = (f - o.y) / d.y;
            let x = o + d * tf;
            if tf > 0.0 && tf < tw && x.z >= -back {
                return Some(x);
            }
        }
        (tw > 0.0).then(|| o + d * tw)
    }

    /// The camera as the camera pass reads it (`CamGpu` in stage_camera.wgsl). `samples` is 1 or 5
    /// rays a pixel at edges.
    pub fn gpu(&self, samples: u32, pixels: (u32, u32)) -> StageCameraGpu {
        StageCameraGpu {
            origin: self.origin.extend(self.dist).to_array(),
            forward: self.forward.extend(self.zoom() * self.fill).to_array(),
            right: self.right.extend(self.motion).to_array(),
            down: self.down.extend(samples as f32).to_array(),
            shift: self.shift.extend(SIDE_SHADE).to_array(),
            rest: self.rest.extend(0.0).to_array(),
            size: [self.size.x, self.size.y, pixels.0 as f32, pixels.1 as f32],
        }
    }
}

/// `CamGpu` in stage_camera.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct StageCameraGpu {
    /// Its place, and dist.
    pub origin: [f32; 4],
    /// The way it looks, and zoom × fill.
    pub forward: [f32; 4],
    /// Its right, and the move amount.
    pub right: [f32; 4],
    /// Its down, and how many rays a pixel takes at edges.
    pub down: [f32; 4],
    /// Every ray's shift, and the shade of shapes' sides.
    pub shift: [f32; 4],
    /// The rest camera's place; w spare.
    pub rest: [f32; 4],
    /// The picture in screen units, then in pixels.
    pub size: [f32; 4],
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: Vec2 = Vec2::new(1080.0, 1920.0);

    fn layout() -> StageLayout {
        StageLayout { back: 240.0, k: 14f32.to_radians().tan(), floor: Some(800.0) }
    }

    fn front() -> CameraView {
        CameraView { center: Vec2::new(30.0, -40.0), zoom: 1.3, turn: 0.2 }
    }

    #[test]
    fn at_rest_every_ray_meets_the_plane_where_the_front_picture_shows_it() {
        let v = StageView::solve(&StageCamera::default(), &front(), SIZE, layout());
        assert_eq!(v.fill, 1.0);
        assert_eq!(v.motion, 0.0);
        for s in [Vec2::ZERO, Vec2::new(-500.0, 900.0), Vec2::new(320.0, -700.0)] {
            let p = v.plane_at(s).unwrap();
            assert!((p - front().to_world(s)).length() < 1e-2, "{s}: {p}");
            // Anything further along the same ray is seen at that very pixel.
            let (o, d) = v.ray(s);
            let uv = s / SIZE + 0.5;
            for t in [0.5, 1.0, 1.05, 1.2] {
                assert!((v.uv_rest(o + d * t) - uv).length() < 1e-4, "{s} at {t}");
            }
        }
    }

    #[test]
    fn the_editor_s_mapping_round_trips_under_a_move() {
        let cam = StageCamera { orbit: 25.0, tilt: -10.0, height: 80.0, ..StageCamera::default() };
        let v = StageView::solve(&cam, &front(), SIZE, layout());
        for s in [Vec2::ZERO, Vec2::new(-300.0, 500.0), Vec2::new(200.0, -600.0)] {
            let p = v.plane_at(s).unwrap();
            assert!((v.screen_of(p).unwrap() - s).length() < 1e-2, "{s}");
        }
    }

    #[test]
    fn orbit_swings_right_and_tilt_raises() {
        let rest = StageView::solve(&StageCamera::default(), &CameraView::default(), SIZE, layout());
        let right = StageView::solve(
            &StageCamera { orbit: 20.0, ..StageCamera::default() },
            &CameraView::default(),
            SIZE,
            layout(),
        );
        let up = StageView::solve(
            &StageCamera { tilt: 20.0, ..StageCamera::default() },
            &CameraView::default(),
            SIZE,
            layout(),
        );
        assert!(right.origin.x > rest.origin.x + 100.0);
        assert!(up.origin.y < rest.origin.y - 100.0);
        assert_eq!(right.motion, 1.0);
    }

    #[test]
    fn a_move_with_a_back_wall_zooms_in_to_keep_the_edges_filled() {
        let cam = StageCamera { orbit: 20.0, ..StageCamera::default() };
        let filled = StageView::solve(&cam, &CameraView::default(), SIZE, layout());
        assert!(filled.fill > 1.01 && filled.fill < MOST_FILL, "{}", filled.fill);
        assert!(filled.edges_inside(filled.fill));
        let stretched =
            StageView::solve(&StageCamera { fill: FillEdges::Stretch, ..cam }, &CameraView::default(), SIZE, layout());
        assert_eq!(stretched.fill, 1.0);
    }

    #[test]
    fn wild_settings_stay_finite() {
        let cam =
            StageCamera { orbit: 1e9, tilt: f32::NAN, height: 1e6, lens: 1e9, pivot: -1e7, ..StageCamera::default() };
        let v = StageView::solve(&cam, &CameraView::default(), SIZE, layout());
        let g = v.gpu(5, (270, 480));
        for f in g.origin.iter().chain(&g.forward).chain(&g.right).chain(&g.down).chain(&g.shift).chain(&g.rest) {
            assert!(f.is_finite(), "{g:?}");
        }
        let tiny = StageView::solve(
            &StageCamera { lens: 0.0, ..StageCamera::default() },
            &CameraView::default(),
            SIZE,
            layout(),
        );
        assert!(tiny.dist > 0.0 && tiny.dist.is_finite());
    }
}
