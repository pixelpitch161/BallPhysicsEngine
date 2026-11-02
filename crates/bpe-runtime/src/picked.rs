//! Paint a ball has picked up from wet paint: a colour laid over its own, fading back over time.

use crate::world::World;
use bpe_core::ThingId;

/// Paint a ball has picked up: a colour laid over its own as strongly as `amount` says, fading
/// back over `fade` seconds from `step`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Picked {
    /// Linear light.
    pub rgb: [f32; 3],
    /// How strongly it shows at `step`, 0..1.
    pub amount: f32,
    /// The world step it was last mixed at.
    pub step: u64,
    /// Seconds it takes to fade from 1 to 0; 0 never fades.
    pub fade: f32,
}

impl Picked {
    /// How strongly it shows at world step `now`: amount − (seconds since step) / fade, floored at 0.
    /// `fade <= 0` (or not finite) never fades. `now < step` counts as no time passed.
    pub fn shown(&self, now: u64, steps_per_second: u32) -> f32 {
        if !(self.fade > 0.0) || !self.fade.is_finite() {
            return self.amount;
        }
        let seconds = now.saturating_sub(self.step) as f32 / steps_per_second.max(1) as f32;
        (self.amount - seconds / self.fade).max(0.0)
    }

    /// Mixes `colour` (linear) in by `share` (0..1) at step `now`, fading over `fade`.
    /// With A = self.shown(now, sps) and s = share.clamp(0, 1):
    ///   A' = 1 − (1 − A)(1 − s)
    ///   rgb' = (rgb·A·(1 − s) + colour·s) / A'   (when A' > 0; else rgb' = colour)
    ///   amount = A', step = now, fade = fade.
    pub fn mix(&mut self, colour: [f32; 3], share: f32, now: u64, steps_per_second: u32, fade: f32) {
        let a = self.shown(now, steps_per_second);
        let s = share.clamp(0.0, 1.0);
        let a2 = 1.0 - (1.0 - a) * (1.0 - s);
        if a2 > 0.0 {
            for k in 0..3 {
                self.rgb[k] = (self.rgb[k] * a * (1.0 - s) + colour[k] * s) / a2;
            }
        } else {
            self.rgb = colour;
        }
        self.amount = a2;
        self.step = now;
        self.fade = fade;
    }
}

/// `c` (linear, straight alpha) with ball `i` of `thing`'s picked-up paint over it:
/// rgb = lerp(c.rgb, picked.rgb, shown). Alpha unchanged. No entry, or shown 0: `c` unchanged.
pub fn with_picked(world: &World, thing: ThingId, i: usize, c: [f32; 4]) -> [f32; 4] {
    let Some(p) = world.picked.get(&thing).and_then(|v| v.get(i)) else {
        return c;
    };
    let a = p.shown(world.step, world.steps_per_second);
    if a <= 0.0 {
        return c;
    }
    [c[0] + (p.rgb[0] - c[0]) * a, c[1] + (p.rgb[1] - c[1]) * a, c[2] + (p.rgb[2] - c[2]) * a, c[3]]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing;

    #[test]
    fn picked_fades_by_its_time() {
        let p = Picked { rgb: [1.0, 0.0, 0.0], amount: 0.8, step: 240, fade: 2.0 };
        assert_eq!(p.shown(240, 240), 0.8);
        assert!((p.shown(480, 240) - 0.3).abs() < 1e-6);
        assert_eq!(p.shown(720, 240), 0.0);
        assert_eq!(p.shown(100, 240), 0.8);
        let never = Picked { fade: 0.0, ..p };
        assert_eq!(never.shown(10_000, 240), 0.8);
    }

    #[test]
    fn picked_mix_moves_the_shown_colour() {
        let mut p = Picked::default();
        p.mix([0.0, 0.0, 1.0], 0.5, 0, 240, 3.0);
        assert_eq!(p.amount, 0.5);
        assert_eq!(p.rgb, [0.0, 0.0, 1.0]);
        p.mix([1.0, 0.0, 0.0], 0.5, 0, 240, 3.0);
        assert_eq!(p.amount, 0.75);
        let want = [2.0 / 3.0, 0.0, 1.0 / 3.0];
        for k in 0..3 {
            assert!((p.rgb[k] - want[k]).abs() < 1e-6, "{:?}", p.rgb);
        }
    }

    #[test]
    fn with_picked_leaves_other_balls_alone() {
        let (scene, resolved, sims) = testing::dots_in_ring(1);
        let mut w = World::build(&scene, &resolved, &sims);
        w.picked.insert(ThingId(5), vec![Picked { rgb: [0.0, 0.0, 1.0], amount: 1.0, step: 0, fade: 0.0 }]);
        let c = [1.0, 0.0, 0.0, 1.0];
        assert_eq!(with_picked(&w, ThingId(5), 0, c), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(with_picked(&w, ThingId(5), 3, c), c);
        assert_eq!(with_picked(&w, ThingId(6), 0, c), c);
    }
}
