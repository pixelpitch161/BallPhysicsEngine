//! Where a moving part is at a moment: its place, its turn and its size.

use bpe_core::maths::Maths;
use glam::Vec2;

/// A moving part's place (world pixels: where its own frame's origin is), turn (radians, clockwise
/// on the screen, as a thing's rotation) and scale (its size times this, about its place).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub at: Vec2,
    pub turn: f32,
    pub scale: f32,
}

impl Default for Pose {
    fn default() -> Self {
        Pose { at: Vec2::ZERO, turn: 0.0, scale: 1.0 }
    }
}

/// The least and most a pose's scale is taken as (the Moves card's Scale range).
pub const SCALE_RANGE: (f32, f32) = (0.01, 100.0);

impl Pose {
    pub fn new(at: Vec2, turn: f32, scale: f32) -> Pose {
        Pose { at, turn, scale }
    }

    /// A point of the part's own frame in the world.
    pub fn to_world(&self, own: Vec2) -> Vec2 {
        let (s, c) = self.turn.det_sin_cos();
        let p = own * self.scale;
        self.at + Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c)
    }

    /// A world point in the part's own frame (its place, turn and scale undone).
    pub fn to_own(&self, world: Vec2) -> Vec2 {
        let (s, c) = self.turn.det_sin_cos();
        let d = world - self.at;
        Vec2::new(d.x * c + d.y * s, -d.x * s + d.y * c) / self.scale
    }

    /// The pose a share `t` of the way from `a` to `b` (place, turn and scale each evenly).
    pub fn lerp(a: Pose, b: Pose, t: f32) -> Pose {
        Pose {
            at: a.at + (b.at - a.at) * t,
            turn: a.turn + (b.turn - a.turn) * t,
            scale: a.scale + (b.scale - a.scale) * t,
        }
    }

    /// The same pose, with anything that isn't a number taken as its default and the scale kept
    /// to [`SCALE_RANGE`].
    pub fn sane(self) -> Pose {
        Pose {
            at: if self.at.is_finite() { self.at } else { Vec2::ZERO },
            turn: if self.turn.is_finite() { self.turn } else { 0.0 },
            scale: if self.scale.is_finite() { self.scale.clamp(SCALE_RANGE.0, SCALE_RANGE.1) } else { 1.0 },
        }
    }

    /// How fast a part moves from `self` to `to` in `seconds`: place a second, turn a second, and
    /// scale a second (as a pose, for keeping beside the poses).
    pub fn speed_to(&self, to: Pose, seconds: f32) -> Pose {
        let s = if seconds > 0.0 { 1.0 / seconds } else { 0.0 };
        Pose { at: (to.at - self.at) * s, turn: (to.turn - self.turn) * s, scale: (to.scale - self.scale) * s }
    }

    /// This pose moved on for `seconds` at `speed` (from [`Pose::speed_to`]).
    pub fn moved_on(&self, speed: Pose, seconds: f32) -> Pose {
        Pose {
            at: self.at + speed.at * seconds,
            turn: self.turn + speed.turn * seconds,
            scale: self.scale + speed.scale * seconds,
        }
        .sane()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    fn near(a: Vec2, b: Vec2) {
        assert!((a - b).length() < 1e-3, "{a} is not {b}");
    }

    #[test]
    fn a_pose_turns_clockwise_and_back() {
        let p = Pose::new(Vec2::new(10.0, 20.0), FRAC_PI_2, 2.0);
        // (Clockwise on the screen, y down: x turns to y.)
        near(p.to_world(Vec2::new(1.0, 0.0)), Vec2::new(10.0, 22.0));
        for q in [Vec2::new(3.0, -4.0), Vec2::new(-7.5, 0.25), Vec2::ZERO] {
            near(p.to_own(p.to_world(q)), q);
        }
        let half = Pose::lerp(Pose::default(), p, 0.5);
        near(half.at, Vec2::new(5.0, 10.0));
        assert!((half.turn - FRAC_PI_2 / 2.0).abs() < 1e-6 && (half.scale - 1.5).abs() < 1e-6);
    }

    #[test]
    fn a_pose_that_is_not_a_number_is_made_sane() {
        let p = Pose::new(Vec2::new(f32::NAN, 1.0), f32::INFINITY, 0.0).sane();
        assert_eq!(p, Pose::new(Vec2::ZERO, 0.0, SCALE_RANGE.0));
        let speed = Pose::default().speed_to(Pose::new(Vec2::new(60.0, 0.0), 1.0, 2.0), 0.5);
        assert_eq!(speed, Pose::new(Vec2::new(120.0, 0.0), 2.0, 2.0));
        assert_eq!(Pose::default().moved_on(speed, 0.5), Pose::new(Vec2::new(60.0, 0.0), 1.0, 2.0));
    }
}
