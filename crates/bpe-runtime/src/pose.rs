//! Where a thing is right now: its place, turn and speed, so other things can ride on it.

use bpe_core::Transform;
use glam::Vec2;

/// A thing's place in the world this step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pose {
    pub position: Vec2,
    /// Radians, clockwise on screen.
    pub angle: f32,
    /// Pixels per second.
    pub velocity: Vec2,
    /// Radians per second, clockwise on screen.
    pub spin: f32,
}

impl Pose {
    /// A still pose at the transform's place and turn.
    pub fn from_transform(t: &Transform) -> Pose {
        Pose { position: t.position, angle: t.rotation, velocity: Vec2::ZERO, spin: 0.0 }
    }

    /// The pose of something sitting `offset` away from this one. With `turn`, the offset turns
    /// with this pose and the result turns and sweeps round with it; without, it just moves along.
    pub fn local(&self, offset: Vec2, turn: bool) -> Pose {
        if turn {
            let position = self.position + Vec2::from_angle(self.angle).rotate(offset);
            let velocity = self.velocity + (position - self.position).perp() * self.spin;
            Pose { position, angle: self.angle, velocity, spin: self.spin }
        } else {
            Pose { position: self.position + offset, angle: 0.0, velocity: self.velocity, spin: 0.0 }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    #[test]
    fn local_turns_the_offset_and_adds_the_rim_speed() {
        let p = Pose { position: Vec2::new(10.0, 0.0), angle: FRAC_PI_2, velocity: Vec2::new(1.0, 0.0), spin: 2.0 };
        let r = p.local(Vec2::new(5.0, 0.0), true);
        assert!((r.position - Vec2::new(10.0, 5.0)).length() < 1e-5, "{r:?}");
        // The rim at (0, 5) from the centre, turning at 2 rad/s, sweeps at perp((0, 5)) * 2 = (-10, 0).
        assert!((r.velocity - Vec2::new(1.0 - 10.0, 0.0)).length() < 1e-4, "{r:?}");
        assert_eq!((r.angle, r.spin), (FRAC_PI_2, 2.0));
        let g = p.local(Vec2::new(5.0, 0.0), false);
        assert_eq!(g, Pose { position: Vec2::new(15.0, 0.0), velocity: Vec2::new(1.0, 0.0), ..Pose::default() });
        let t = Pose::from_transform(&Transform { position: Vec2::new(3.0, 4.0), rotation: 0.5 });
        assert_eq!(t, Pose { position: Vec2::new(3.0, 4.0), angle: 0.5, ..Pose::default() });
    }
}
