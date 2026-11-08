//! Dichroic glass (glass and light part 4b, plan `docs/superpowers/plans/glass-light/part4.md`,
//! decision 1): light meeting a dichroic surface splits into a hue that goes through and the
//! opposite hue that bounces off, both turning with the angle it meets the surface at. The same
//! rule as lighting.wgsl's `dichroic_hue`, `dichroic_through` and `dichroic_back` (the laser
//! beams and the sphere caustics use this one).

/// A surface's dichroic numbers: how dichroic (0 not), how far the hue turns from straight on to
/// grazing and the hue straight on (both in turns, 1 the whole wheel).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Dichroic {
    pub amount: f32,
    pub shift: f32,
    pub hue: f32,
}

/// The pure colour of hue `h` (in turns) on the wheel: red at 0, green at 1/3, blue at 2/3.
pub fn hue_rgb(h: f32) -> [f32; 3] {
    [0.0f32, 2.0 / 3.0, 1.0 / 3.0].map(|k| (((h + k).rem_euclid(1.0) * 6.0 - 3.0).abs() - 1.0).clamp(0.0, 1.0))
}

/// The mean |cosine| light falling on a sphere meets it at (the caustics fold a dichroic ball's
/// colour in at this angle).
pub const SPHERE_COS: f32 = 2.0 / 3.0;

impl Dichroic {
    /// A surface table entry's dichroic numbers (`SurfaceGpu.g`: amount, shift and hue in turns).
    pub fn of(g: [f32; 4]) -> Self {
        let ok = |v: f32| if v.is_finite() { v } else { 0.0 };
        Self { amount: ok(g[0]).clamp(0.0, 1.0), shift: ok(g[1]), hue: ok(g[2]) }
    }

    /// Whether it changes anything.
    pub fn on(&self) -> bool {
        self.amount > 0.0
    }

    /// The hue light meeting it at `cos` (|cosine| to its normal, 1 straight on) takes.
    pub fn hue_at(&self, cos: f32) -> [f32; 3] {
        hue_rgb(self.hue + self.shift * (1.0 - cos.abs().clamp(0.0, 1.0)))
    }

    /// What goes through, per channel, for light meeting it at `cos`.
    pub fn through(&self, cos: f32) -> [f32; 3] {
        let d = self.amount.clamp(0.0, 1.0);
        self.hue_at(cos).map(|c| 1.0 + (c - 1.0) * d)
    }

    /// The colour a mirror's light thrown off it takes: the opposite hue at full strength, mixed
    /// in by how dichroic it is (lighting.wgsl's `dichroic_back` with share 1).
    pub fn back(&self, cos: f32) -> [f32; 3] {
        let d = self.amount.clamp(0.0, 1.0);
        self.hue_at(cos).map(|c| 1.0 - c * d)
    }

    /// What bounces off, per channel: the rest, so through + bounced is 1.
    pub fn bounced(&self, cos: f32) -> [f32; 3] {
        let d = self.amount.clamp(0.0, 1.0);
        self.hue_at(cos).map(|c| (1.0 - c) * d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wheel_and_the_shader_agree() {
        assert_eq!(hue_rgb(0.0), [1.0, 0.0, 0.0]);
        assert_eq!(hue_rgb(1.0 / 3.0), [0.0, 1.0, 0.0]);
        assert_eq!(hue_rgb(2.0 / 3.0), [0.0, 0.0, 1.0]);
        let y = hue_rgb(1.0 / 6.0);
        assert!((y[0] - 1.0).abs() < 1e-5 && (y[1] - 1.0).abs() < 1e-5 && y[2] == 0.0);
        let src = include_str!("shaders/lighting.wgsl");
        assert!(src.contains("abs(fract(vec3<f32>(h) + vec3<f32>(0.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0) - 1.0"));
    }

    #[test]
    fn what_goes_through_and_what_bounces_add_to_white() {
        for amount in [0.0, 0.3, 1.0] {
            let d = Dichroic { amount, shift: 0.33, hue: 0.53 };
            for k in 0..=10 {
                let cos = k as f32 / 10.0;
                let (t, r) = (d.through(cos), d.bounced(cos));
                for c in 0..3 {
                    assert!((t[c] + r[c] - 1.0).abs() < 1e-6);
                }
            }
        }
        let off = Dichroic { amount: 0.0, shift: 0.5, hue: 0.2 };
        assert_eq!(off.through(0.3), [1.0; 3]);
        assert_eq!(off.bounced(0.3), [0.0; 3]);
    }

    #[test]
    fn the_hue_turns_with_the_angle() {
        let d = Dichroic { amount: 1.0, shift: 1.0 / 3.0, hue: 0.0 };
        assert_eq!(d.through(1.0), [1.0, 0.0, 0.0]);
        assert_eq!(d.through(0.0), [0.0, 1.0, 0.0]);
        assert_eq!(d.bounced(1.0), [0.0, 1.0, 1.0]);
    }

    #[test]
    fn makes_rainbows_packs_beside_catches_beams() {
        use crate::frame::{pack_tags, tag_parts};
        assert_eq!(pack_tags(0.7, 0.0), 0.7, "Makes rainbows 0 leaves Catches beams exactly");
        assert_eq!(pack_tags(1.9, 0.0), 1.9);
        for (catches, makes) in [(0.0, 1.0), (0.5, 0.25), (2.0, 0.6), (1.0 / 64.0, 1.0 / 255.0)] {
            let z = pack_tags(catches, makes);
            assert!(z < 0.0);
            let (c, m) = tag_parts(z);
            assert!((c - catches).abs() <= 0.5 / 64.0, "{catches} came back as {c}");
            assert!((m - makes).abs() <= 0.5 / 255.0, "{makes} came back as {m}");
        }
        assert_eq!(tag_parts(0.3), (0.3, 0.0));
        let src = include_str!("shaders/scene.wgsl");
        assert!(src.contains("floor(-z / 256.0) / 64.0") && src.contains("(v - 256.0 * floor(v / 256.0)) / 255.0"));
    }
}
