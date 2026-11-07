//! Light bent by glass shapes, liquid and bubbles (glass and light part 3, plan
//! `docs/superpowers/plans/glass-light/part3.md`): photons leave each light toward each bender
//! (a glass shape's box, or the box round every bit of free glass), are bent in and out of the
//! glass on the graphics card (`cs_bent` in `shaders/bent_light.wgsl`, pasted after
//! lighting.wgsl), and each stretch of a photon's way after it first leaves glass (a *leg*) is
//! splatted as a soft line into the direct light and the light in the air. The direct light
//! counts glass that bends as solid, so the photons carry exactly the light it lets through.

use bytemuck::{Pod, Zeroable};

use crate::frame::{Bender, LightGpu, LightScene, MOST_THROWERS, Thrower};

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
    /// Its bender id (a surface index, or [`crate::FREE_BENDER`]; a thrower's surface index), the
    /// light's index, 1 for a thrower's pair (glass and light part 4a; 0 a bender's), spare.
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
    /// Its light (linear rgb, before the falloff), its path key.
    pub c: [f32; 4],
    /// Part 4a: how much a rough mirror has blurred it, at each end (world, added to its half
    /// width), two spare.
    pub d: [f32; 4],
}

/// A thing that throws light as the photons read it (`ThrowerGpu` in bent_light.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ThrowerGpu {
    /// Its surface index, 1 for balls, Facets, Facet gaps.
    pub a: [f32; 4],
    /// What it gives back (linear rgb), how fast it blurs (Roughness squared).
    pub b: [f32; 4],
}

/// What the photon pass reads (`BentU` in bent_light.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct BentUniform {
    /// Photons a pair, pairs, the most steps a photon takes, spare.
    pub counts: [f32; 4],
    /// World units per output pixel, this sample's turn of the photons (0 to 1), half the width of
    /// a photon's line (world), Glass caustics.
    pub more: [f32; 4],
    /// Part 4a: Mirror light, how many things throw light, two spare.
    pub extra: [f32; 4],
    /// Which surface indices throw light ([`throw_bits`]).
    pub throws: [[u32; 4]; 16],
    /// The things that throw light ([`throw_table`]).
    pub table: [ThrowerGpu; MOST_THROWERS],
}

impl Default for BentUniform {
    fn default() -> Self {
        Zeroable::zeroed()
    }
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
    /// Every thing that throws light (part 4a; none on a Stage are balls).
    pub throwers: Vec<Thrower>,
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
        let shadows = s.shadows[0] < 2.5;
        let mut benders = Vec::new();
        if s.bake_more[2] != 0.0 && shadows {
            benders.extend(scene.benders.iter().copied());
            benders.extend(frame.free_glass_box());
        }
        // Part 4a: mirrors throw light with Mirror light on; on a Stage balls are spheres, which the
        // sphere caustics throw from.
        let stage = s.stage[0] > 0.5;
        let throwers: Vec<Thrower> = if s.bent[2] != 0.0 && s.bent[2].is_finite() && shadows {
            scene.throwers.iter().filter(|t| !(stage && t.ball)).take(MOST_THROWERS).copied().collect()
        } else {
            Vec::new()
        };
        let mut pairs = bent_pairs(scene, &benders);
        pairs.extend(thrower_pairs(scene, &throwers));
        let quality = if frame.bake.is_some() { 4.0 } else { s.more[2] };
        let (per, run) = bent_photons(quality, pairs.len(), s.bent[0]);
        BentPlan { benders, throwers, pairs, per, run }
    }

    /// Whether any glass that bends sends photons (the direct light then counts it as solid:
    /// lighting.wgsl's `u.bent.x`).
    pub fn bending(&self) -> bool {
        self.pairs[..self.run].iter().any(|p| p.b[2] < 0.5)
    }

    /// Whether any photons are sent.
    pub fn on(&self) -> bool {
        self.run > 0
    }

    /// Whether photons are sent through free glass: then the glass picture (light pass 9) marks it
    /// in the surface picture.
    pub fn free_glass(&self) -> bool {
        self.pairs[..self.run].iter().any(|p| p.b[2] < 0.5 && p.b[0] as u32 == crate::FREE_BENDER)
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
    let boxes: Vec<([f32; 2], [f32; 2], u32)> = benders.iter().map(|b| (b.lo, b.hi, b.index)).collect();
    pairs_of(scene, &boxes, 0.0)
}

/// The (light, thrower) pairs (part 4a), as [`bent_pairs`]'s, marked as a thrower's.
pub fn thrower_pairs(scene: &LightScene, throwers: &[Thrower]) -> Vec<BentPair> {
    let boxes: Vec<([f32; 2], [f32; 2], u32)> = throwers.iter().map(|t| (t.lo, t.hi, t.index)).collect();
    pairs_of(scene, &boxes, 1.0)
}

/// The pairs of every light that sends photons with every box (lo, hi, id) its reach gets to,
/// in light then box order, `kind` in each pair's `b[2]`.
fn pairs_of(scene: &LightScene, boxes: &[([f32; 2], [f32; 2], u32)], kind: f32) -> Vec<BentPair> {
    let mut pairs = Vec::new();
    for (i, l) in scene.lights.iter().enumerate() {
        if !sends_photons(l) {
            continue;
        }
        let sun = l.p0[3] > 1.5 && l.p0[3] < 2.5;
        for &(lo, hi, index) in boxes {
            if !sun {
                // The light's reach (from anywhere on an area or a strip, its size round its place).
                let spread = match l.p0[3] {
                    k if k > 2.5 && k < 3.5 => 0.5 * l.p4[0].max(0.0).hypot(l.p4[1].max(0.0)),
                    k if k > 3.5 => f32::INFINITY,
                    _ => 0.0,
                };
                let dx = (lo[0] - l.p0[0]).max(l.p0[0] - hi[0]).max(0.0);
                let dy = (lo[1] - l.p0[1]).max(l.p0[1] - hi[1]).max(0.0);
                if dx.hypot(dy) - spread >= l.p2[0].max(0.0) {
                    continue;
                }
            }
            pairs.push(BentPair { a: [lo[0], lo[1], hi[0], hi[1]], b: [index as f32, i as f32, kind, 0.0] });
        }
    }
    pairs
}

/// The bits bent_light.wgsl's `thrower_at` reads (part 4a): bit `i % 32` of word `i / 32` for
/// every thrower's surface index `i` from 1 to 2047.
pub fn throw_bits(throwers: &[Thrower]) -> [[u32; 4]; 16] {
    let mut bits = [[0u32; 4]; 16];
    for t in throwers {
        let i = t.index as usize;
        if i == 0 || i >= 2048 {
            continue;
        }
        let word = i / 32;
        bits[word / 4][word % 4] |= 1 << (i % 32);
    }
    bits
}

/// The throwers as the photons read them, at most [`MOST_THROWERS`], and how many.
pub fn throw_table(throwers: &[Thrower]) -> ([ThrowerGpu; MOST_THROWERS], u32) {
    let mut table = [ThrowerGpu::default(); MOST_THROWERS];
    let finite = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
    for (e, t) in table.iter_mut().zip(throwers) {
        e.a = [t.index as f32, if t.ball { 1.0 } else { 0.0 }, finite(t.facets), finite(t.gaps).min(1.0)];
        e.b = [finite(t.colour[0]), finite(t.colour[1]), finite(t.colour[2]), finite(t.widen)];
    }
    (table, throwers.len().min(MOST_THROWERS) as u32)
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
        assert!(src.contains(&format!("const MOST_THROWERS: u32 = {MOST_THROWERS}u;")));
        assert!(src.contains("const BENT_THROW: i32 = -16;"));
        let lighting = include_str!("shaders/lighting.wgsl");
        assert!(lighting.contains(&format!("const FREE_BENDER: i32 = {FREE_BENDER};")));
        assert_eq!(std::mem::size_of::<Leg>(), 64);
        assert_eq!(std::mem::size_of::<BentUniform>(), 48 + 256 + 32 * MOST_THROWERS);
    }

    /// The light pass's source (lighting.wgsl with bent_light.wgsl pasted after it) parses and
    /// validates without a GPU.
    #[test]
    fn the_light_pass_source_is_valid() {
        let source = [include_str!("shaders/lighting.wgsl"), include_str!("shaders/bent_light.wgsl")].concat();
        let module = naga::front::wgsl::parse_str(&source).map_err(|e| e.emit_to_string(&source)).unwrap();
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
            .validate(&module)
            .map_err(|e| format!("{}", e.as_inner()))
            .unwrap();
    }

    fn thrower(index: u32, ball: bool) -> Thrower {
        Thrower {
            lo: [0.0, 0.0],
            hi: [10.0, 10.0],
            index,
            colour: [0.9, 0.8, 0.7],
            widen: 0.01,
            facets: 24.0,
            gaps: 0.1,
            ball,
        }
    }

    #[test]
    fn throwers_have_bits_and_a_table_and_the_65th_is_left_out() {
        let bits = throw_bits(&[thrower(0, false), thrower(9, false), thrower(2047, true), thrower(4000, false)]);
        assert_eq!(bits[0][0], 1 << 9);
        assert_eq!(bits[15][3], 1 << 31);
        let many: Vec<Thrower> = (1..=70).map(|i| thrower(i, false)).collect();
        let (table, n) = throw_table(&many);
        assert_eq!(n, MOST_THROWERS as u32);
        assert_eq!(table[0].a, [1.0, 0.0, 24.0, 0.1]);
        assert_eq!(table[0].b, [0.9, 0.8, 0.7, 0.01]);
        assert_eq!(table[63].a[0], 64.0);
    }

    #[test]
    fn mirror_pairs_follow_the_benders_and_need_mirror_light_and_shadows() {
        let mut l = LightGpu::ZERO;
        l.p0 = [-100.0, 0.0, 0.0, 0.0];
        l.p2 = [500.0, 0.0, 0.0, 1.0];
        l.p5 = [0.0, 0.0, 0.0, 1.0];
        let mut scene = LightScene {
            lights: vec![l],
            benders: vec![bender(3)],
            throwers: vec![thrower(5, false), thrower(6, true)],
            ..Default::default()
        };
        scene.settings.bake_more[2] = 1.0;
        scene.settings.bent = [1.0, 1.0, 1.0, 0.0];
        scene.settings.more[2] = 2.0;
        let frame = crate::Frame::default();
        let plan = BentPlan::of(&frame, &scene);
        let kinds: Vec<(f32, f32)> = plan.pairs.iter().map(|p| (p.b[0], p.b[2])).collect();
        assert_eq!(kinds, vec![(3.0, 0.0), (5.0, 1.0), (6.0, 1.0)]);
        assert!(plan.bending());
        // On a Stage ball throwers throw through the sphere caustics instead.
        scene.settings.stage[0] = 1.0;
        let plan = BentPlan::of(&frame, &scene);
        assert_eq!(plan.pairs.len(), 2);
        scene.settings.stage[0] = 0.0;
        // Mirrors alone don't make the direct light count glass as solid.
        scene.benders.clear();
        let plan = BentPlan::of(&frame, &scene);
        assert!(plan.on() && !plan.bending());
        scene.settings.bent[2] = 0.0;
        assert!(!BentPlan::of(&frame, &scene).on(), "Mirror light 0");
        scene.settings.bent[2] = 1.0;
        scene.settings.shadows[0] = 3.0;
        assert!(!BentPlan::of(&frame, &scene).on(), "shadows Off");
    }
}
