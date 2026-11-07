//! Light bent by glass shapes, liquid and bubbles (glass and light part 3, plan
//! `docs/superpowers/plans/glass-light/part3.md`): photons leave each light toward each bender
//! (a glass shape's box, or the box round every bit of free glass), are bent in and out of the
//! glass on the graphics card (`cs_bent` in `shaders/bent_light.wgsl`, pasted after
//! lighting.wgsl), and each stretch of a photon's way after it first leaves glass (a *leg*) is
//! splatted as a soft line into the direct light and the light in the air. The direct light
//! counts glass that bends as solid, so the photons carry exactly the light it lets through.

use bytemuck::{Pod, Zeroable};

use crate::frame::{Bender, LightGpu, LightScene};

/// The most photons a frame sends (all pairs together).
pub const MOST_BENT_PHOTONS: u32 = 1 << 17;
/// The legs a photon keeps at most (each a stretch between two bends); lighting.wgsl's
/// `BENT_LEGS` is the same.
pub const BENT_LEGS: u32 = 8;
/// The fewest photons a (light, bender) pair sends.
pub const LEAST_PER_PAIR: u32 = 256;

/// A (light, bender) pair photons go from and to (`BentPair` in bent_light.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct BentPair {
    /// The bender's box (world): lo xy, hi xy.
    pub a: [f32; 4],
    /// Its bender id (a surface index, or [`crate::FREE_BENDER`]), the light's index, two spare.
    pub b: [f32; 4],
}

/// One stretch of a photon's way (`Leg` in bent_light.wgsl): an empty one (light 0) draws
/// nothing.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Leg {
    /// From xy, to xy (world).
    pub a: [f32; 4],
    /// The path length from the light at each end, its share in the haze, the light's index + 1.
    pub b: [f32; 4],
    /// Its light (linear rgb, before the falloff), spare.
    pub c: [f32; 4],
}

/// What the photon pass reads (`BentU` in bent_light.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct BentUniform {
    /// Photons a pair, pairs, the most steps a photon takes, spare.
    pub counts: [f32; 4],
    /// World units per output pixel, this sample's turn of the photons (0 to 1), half the width of
    /// a photon's line (world), Glass caustics.
    pub more: [f32; 4],
}

/// How many photons a frame sends at a Quality (0 draft .. 3 ultra, 4 a baked sample) times
/// Bent light rays, before they are shared out among the pairs.
pub fn bent_budget(quality: f32, rays: f32) -> u32 {
    let base: f32 = match quality.round() as i32 {
        i32::MIN..=0 => 4096.0,
        1 => 8192.0,
        2 => 16384.0,
        _ => 32768.0,
    };
    let rays = if rays.is_finite() { rays.max(0.0) } else { 0.0 };
    (base * rays).min(MOST_BENT_PHOTONS as f32) as u32
}

/// How many photons each pair sends and how many pairs send them: the budget shared out, at least
/// [`LEAST_PER_PAIR`] a pair while [`MOST_BENT_PHOTONS`] allows (fewer, down to 1, when there are
/// that many pairs, so every pair's light is carried: the direct light counts its glass as solid);
/// past that many pairs the last are left out. (0, 0) when nothing is sent.
pub fn bent_photons(quality: f32, pairs: usize, rays: f32) -> (u32, usize) {
    let budget = bent_budget(quality, rays);
    if pairs == 0 || budget == 0 {
        return (0, 0);
    }
    let n = pairs.min(u32::MAX as usize) as u32;
    let per = (budget / n).max(LEAST_PER_PAIR).min(MOST_BENT_PHOTONS / n).max(1);
    let run = pairs.min((MOST_BENT_PHOTONS / per) as usize);
    (per, run)
}

/// What a frame's photon pass does: its (light, bender) pairs, the photons each sends and how many
/// pairs send them ([`bent_photons`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BentPlan {
    /// Every bender (each glass shape's, then free glass's box).
    pub benders: Vec<Bender>,
    /// The pairs; only the first `run` send photons.
    pub pairs: Vec<BentPair>,
    /// Photons a pair.
    pub per: u32,
    /// How many pairs send them.
    pub run: usize,
}

impl BentPlan {
    /// The plan for `frame` lit by `scene`: benders only with Glass caustics on and shadows not
    /// Off, at the frame's Quality (a baked sample's at 4) times Bent light rays.
    pub fn of(frame: &crate::Frame, scene: &LightScene) -> BentPlan {
        let s = &scene.settings;
        let mut benders = Vec::new();
        if s.bake_more[2] != 0.0 && s.shadows[0] < 2.5 {
            benders.extend(scene.benders.iter().copied());
            benders.extend(frame.free_glass_box());
        }
        let pairs = bent_pairs(scene, &benders);
        let quality = if frame.bake.is_some() { 4.0 } else { s.more[2] };
        let (per, run) = bent_photons(quality, pairs.len(), s.bent[0]);
        BentPlan { benders, pairs, per, run }
    }

    /// Whether any photons are sent.
    pub fn on(&self) -> bool {
        self.run > 0
    }

    /// Whether photons are sent through free glass: then the glass picture (light pass 9) marks it
    /// in the surface picture.
    pub fn free_glass(&self) -> bool {
        self.pairs[..self.run].iter().any(|p| p.b[0] as u32 == crate::FREE_BENDER)
    }
}

/// Whether light `l` sends photons through glass: it casts shadows, and isn't a laser (whose beam
/// is bent on the CPU).
fn sends_photons(l: &LightGpu) -> bool {
    l.p5[3] > 0.5 && l.p0[3] < 4.5
}

/// The (light, bender) pairs photons are sent for: every light that sends them, with every bender
/// its reach gets to (a sun's always does), in light then bender order.
pub fn bent_pairs(scene: &LightScene, benders: &[Bender]) -> Vec<BentPair> {
    let mut pairs = Vec::new();
    for (i, l) in scene.lights.iter().enumerate() {
        if !sends_photons(l) {
            continue;
        }
        let sun = l.p0[3] > 1.5 && l.p0[3] < 2.5;
        for b in benders {
            if !sun {
                // The light's reach (from anywhere on an area or a strip, its size round its place).
                let spread = match l.p0[3] {
                    k if k > 2.5 && k < 3.5 => 0.5 * l.p4[0].max(0.0).hypot(l.p4[1].max(0.0)),
                    k if k > 3.5 => f32::INFINITY,
                    _ => 0.0,
                };
                let dx = (b.lo[0] - l.p0[0]).max(l.p0[0] - b.hi[0]).max(0.0);
                let dy = (b.lo[1] - l.p0[1]).max(l.p0[1] - b.hi[1]).max(0.0);
                if dx.hypot(dy) - spread >= l.p2[0].max(0.0) {
                    continue;
                }
            }
            pairs.push(BentPair { a: [b.lo[0], b.lo[1], b.hi[0], b.hi[1]], b: [b.index as f32, i as f32, 0.0, 0.0] });
        }
    }
    pairs
}

/// The bits lighting.wgsl's `bender_at` reads: bit `i % 32` of word `i / 32` for every bender's
/// surface index `i` under 2048 (0, the plain sheet, never; free glass needs no bit).
pub fn bend_bits(benders: &[Bender]) -> [[u32; 4]; 16] {
    let mut bits = [[0u32; 4]; 16];
    for b in benders {
        let i = b.index as usize;
        if i == 0 || i >= 2048 {
            continue;
        }
        let word = i / 32;
        bits[word / 4][word % 4] |= 1 << (i % 32);
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FREE_BENDER;

    fn bender(index: u32) -> Bender {
        Bender { lo: [0.0, 0.0], hi: [10.0, 10.0], index }
    }

    #[test]
    fn bend_bits_set_each_bender_but_the_sheet_and_free_glass() {
        let bits = bend_bits(&[bender(0), bender(5), bender(2047), bender(2048), bender(FREE_BENDER)]);
        assert_eq!(bits[0][0], 1 << 5);
        assert_eq!(bits[15][3], 1 << 31);
        let set: u32 = bits.iter().flatten().map(|w| w.count_ones()).sum();
        assert_eq!(set, 2);
    }

    #[test]
    fn budgets_grow_with_quality_and_rays_and_stop_at_the_cap() {
        assert_eq!(bent_budget(0.0, 1.0), 4096);
        assert_eq!(bent_budget(2.0, 1.0), 16384);
        assert_eq!(bent_budget(4.0, 1.0), 32768);
        assert_eq!(bent_budget(2.0, 2.0), 32768);
        assert_eq!(bent_budget(4.0, 100.0), MOST_BENT_PHOTONS);
        assert_eq!(bent_budget(2.0, 0.0), 0);
        assert_eq!(bent_budget(2.0, f32::NAN), 0);
    }

    #[test]
    fn photons_are_shared_out_with_a_floor_and_the_last_pairs_left_out() {
        assert_eq!(bent_photons(2.0, 1, 1.0), (16384, 1));
        assert_eq!(bent_photons(2.0, 4, 1.0), (4096, 4));
        // Many pairs: each sends the least the cap allows, so every pair is carried.
        assert_eq!(bent_photons(2.0, 1000, 1.0), (131, 1000));
        assert_eq!(bent_photons(2.0, 512, 1.0), (LEAST_PER_PAIR, 512));
        // More pairs than photons: one each, the last left out.
        assert_eq!(bent_photons(2.0, 200_000, 1.0), (1, MOST_BENT_PHOTONS as usize));
        assert_eq!(bent_photons(2.0, 0, 1.0), (0, 0));
        assert_eq!(bent_photons(2.0, 3, 0.0), (0, 0));
    }

    #[test]
    fn pairs_skip_lights_without_shadows_lasers_and_benders_out_of_reach() {
        let lamp = |x: f32, reach: f32, kind: f32, shadows: f32| {
            let mut l = LightGpu::ZERO;
            l.p0 = [x, 0.0, 0.0, kind];
            l.p2 = [reach, 0.0, 0.0, 1.0];
            l.p5 = [0.0, 0.0, 0.0, shadows];
            l
        };
        let scene = LightScene {
            lights: vec![
                lamp(-100.0, 200.0, 0.0, 1.0),
                lamp(-100.0, 50.0, 0.0, 1.0),
                lamp(-100.0, 200.0, 0.0, 0.0),
                lamp(-100.0, 200.0, 5.0, 1.0),
                lamp(-5000.0, 1.0, 2.0, 1.0),
            ],
            ..Default::default()
        };
        let pairs = bent_pairs(&scene, &[bender(7)]);
        let lights: Vec<f32> = pairs.iter().map(|p| p.b[1]).collect();
        assert_eq!(lights, vec![0.0, 4.0]);
        assert_eq!(pairs[0].b[0], 7.0);
        assert_eq!(pairs[0].a, [0.0, 0.0, 10.0, 10.0]);
    }

    #[test]
    fn the_shader_agrees_on_the_legs_and_free_glass() {
        let src = include_str!("shaders/bent_light.wgsl");
        assert!(src.contains(&format!("const BENT_LEGS: u32 = {BENT_LEGS}u;")));
        let lighting = include_str!("shaders/lighting.wgsl");
        assert!(lighting.contains(&format!("const FREE_BENDER: i32 = {FREE_BENDER};")));
        assert_eq!(std::mem::size_of::<Leg>(), 48);
    }
}
