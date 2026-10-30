//! What the video shows of the world: where the camera looks, how close and how turned.

use glam::Vec2;

/// World units across the shot at Zoom 1: the width of the video the scenes were made for.
pub const SHOT_WIDTH: f32 = 1080.0;

/// The camera's shot at one moment. The default is the identity: the world as the video,
/// centred on the origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraView {
    /// The world point at the middle of the shot.
    pub center: Vec2,
    /// Above 1 shows less of the world, bigger.
    pub zoom: f32,
    /// Radians, clockwise on screen: how far the camera is turned.
    pub turn: f32,
}

impl Default for CameraView {
    fn default() -> Self {
        Self { center: Vec2::ZERO, zoom: 1.0, turn: 0.0 }
    }
}

fn rotate(p: Vec2, angle: f32) -> Vec2 {
    let (s, c) = angle.sin_cos();
    Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c)
}

impl CameraView {
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    /// A world point in screen units: 1080 across the video, origin at its centre, y down.
    pub fn to_screen(&self, world: Vec2) -> Vec2 {
        rotate(world - self.center, -self.turn) * self.zoom
    }

    /// The world point under a screen point.
    pub fn to_world(&self, screen: Vec2) -> Vec2 {
        rotate(screen / self.zoom, self.turn) + self.center
    }

    /// Half the shot's size in world units, unturned; `aspect` is the video's height / width.
    pub fn shot_half(&self, aspect: f32) -> Vec2 {
        Vec2::new(SHOT_WIDTH * 0.5, SHOT_WIDTH * 0.5 * aspect) / self.zoom
    }

    /// Half the size of the axis-aligned box around the turned shot.
    pub fn shot_bounds_half(&self, aspect: f32) -> Vec2 {
        let h = self.shot_half(aspect);
        let (s, c) = self.turn.sin_cos();
        Vec2::new(h.x * c.abs() + h.y * s.abs(), h.x * s.abs() + h.y * c.abs())
    }

    /// Whether a round thing at `center` might be in the shot. Never false for something seen;
    /// may be true for something just outside a turned shot's corner.
    pub fn sees(&self, aspect: f32, center: Vec2, radius: f32) -> bool {
        let d = (center - self.center).abs();
        let h = self.shot_bounds_half(aspect);
        d.x <= h.x + radius && d.y <= h.y + radius
    }

    /// How far a world point is outside the turned shot, in world units; 0 inside.
    pub fn distance_outside(&self, aspect: f32, world: Vec2) -> f32 {
        let local = rotate(world - self.center, -self.turn).abs();
        (local - self.shot_half(aspect)).max(Vec2::ZERO).length()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    #[test]
    fn identity_is_exact() {
        let v = CameraView::default();
        for p in [Vec2::new(123.456, -789.01), Vec2::new(0.1, 1e6)] {
            assert_eq!(v.to_screen(p), p);
            assert_eq!(v.to_world(p), p);
        }
    }

    #[test]
    fn round_trips() {
        let v = CameraView { center: Vec2::new(300.0, -2000.0), zoom: 2.5, turn: 0.7 };
        for p in [Vec2::new(1.0, 2.0), Vec2::new(-4000.0, 900.0)] {
            assert!((v.to_world(v.to_screen(p)) - p).length() < 1e-3);
        }
    }

    #[test]
    fn zoom_and_turn() {
        let v = CameraView { center: Vec2::new(100.0, 0.0), zoom: 2.0, turn: FRAC_PI_2 };
        assert!((v.to_screen(Vec2::new(100.0, 50.0)) - Vec2::new(100.0, 0.0)).length() < 1e-4);
    }

    #[test]
    fn sees_culls() {
        let v = CameraView::default();
        let aspect = 1920.0 / 1080.0;
        assert!(v.sees(aspect, Vec2::ZERO, 10.0));
        assert!(!v.sees(aspect, Vec2::new(5000.0, 0.0), 10.0));
    }

    #[test]
    fn distance_outside_is_zero_inside() {
        let v = CameraView { center: Vec2::new(1000.0, 0.0), zoom: 1.0, turn: 0.0 };
        assert_eq!(v.distance_outside(1920.0 / 1080.0, Vec2::new(1000.0, 0.0)), 0.0);
        assert!((v.distance_outside(1920.0 / 1080.0, Vec2::new(1640.0, 0.0)) - 100.0).abs() < 1e-3);
    }
}
