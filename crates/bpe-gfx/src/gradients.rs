//! Gradients on the GPU: every gradient a frame paints with is baked into one row of a texture,
//! and an instance names its row in a [`PaintGpu`] to paint the gradient inside itself.

use std::collections::BTreeMap;

use bpe_core::Gradient;
use bytemuck::{Pod, Zeroable};

/// Samples in one baked row, the first at 0 and the last at 1.
pub const ATLAS_WIDTH: usize = 256;
/// The atlas texture is at least this many rows tall.
const MIN_HEIGHT: usize = 8;

/// The gradients of one frame, each baked once into its own row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GradientAtlas {
    rows: Vec<[[f32; 4]; ATLAS_WIDTH]>,
    /// Each gradient's hash to its row.
    index: BTreeMap<u64, u32>,
}

impl GradientAtlas {
    /// The row `g` is baked into, baking it into a new row the first time it is seen.
    pub fn row(&mut self, g: &Gradient) -> u32 {
        let key = hash(g);
        if let Some(&row) = self.index.get(&key) {
            return row;
        }
        let mut baked = [[0.0; 4]; ATLAS_WIDTH];
        baked.copy_from_slice(&g.bake(ATLAS_WIDTH));
        let row = self.rows.len() as u32;
        self.rows.push(baked);
        self.index.insert(key, row);
        row
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn clear(&mut self) {
        self.rows.clear();
        self.index.clear();
    }

    /// The baked rows in order, linear light.
    pub fn rows(&self) -> &[[[f32; 4]; ATLAS_WIDTH]] {
        &self.rows
    }
}

/// FNV-1a over the stops' places and colours and the transition.
fn hash(g: &Gradient) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for s in &g.stops {
        eat(&s.at.to_bits().to_le_bytes());
        eat(&[s.color.r, s.color.g, s.color.b, s.color.a]);
    }
    eat(&[g.transition as u8]);
    h
}

/// paint.wgsl, reading the atlas from bind group `group`.
pub(crate) fn paint_wgsl(group: u32) -> String {
    include_str!("shaders/paint.wgsl").replace("PAINT_GROUP", &group.to_string())
}

/// How many rows the texture needs for `len` gradients: a power of two, at least 8.
pub(crate) fn atlas_height(len: usize) -> usize {
    len.next_power_of_two().max(MIN_HEIGHT)
}

/// The rows as half floats, ready for an `Rgba16Float` texture.
pub(crate) fn half_texels(rows: &[[[f32; 4]; ATLAS_WIDTH]]) -> Vec<u8> {
    rows.iter().flatten().flatten().flat_map(|&c| f16_bits(c).to_le_bytes()).collect()
}

/// `x` as an IEEE half float, rounded to the nearest (ties to even). Too big is infinity.
fn f16_bits(x: f32) -> u16 {
    let bits = x.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32;
    let mantissa = bits & 0x7f_ffff;
    if exponent == 0xff {
        return sign | 0x7c00 | if mantissa != 0 { 0x200 } else { 0 };
    }
    // Drops the low `shift` bits of `m`, rounding to the nearest and ties to even.
    let round = |m: u32, shift: u32| {
        let kept = m >> shift;
        let rest = m & ((1 << shift) - 1);
        let half = 1 << (shift - 1);
        if rest > half || (rest == half && kept & 1 == 1) { kept + 1 } else { kept }
    };
    let e = exponent - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7c00;
    }
    if e <= 0 {
        // Subnormal, or too small for even that.
        if e < -10 {
            return sign;
        }
        return sign | round(mantissa | 0x80_0000, (14 - e) as u32) as u16;
    }
    // A carry out of the mantissa steps the exponent up, which is still the right half.
    sign | round(((e as u32) << 23) | mantissa, 13) as u16
}

/// How an instance paints a gradient inside itself. Matches `struct PaintGpu` in paint.wgsl.
#[repr(C)]
#[derive(Pod, Zeroable, Clone, Copy, Debug, PartialEq)]
pub struct PaintGpu {
    /// The gradient's atlas row; -1 paints nothing and the instance keeps its own colours.
    pub row: f32,
    /// Where along the gradient the pattern starts, 0 to 1.
    pub t0: f32,
    /// 0 none (the colour at `t0`), 1 stripes, 2 rings, 3 sweep.
    pub pattern: f32,
    /// 1 picks the nearest baked colour instead of blending neighbours.
    pub steps: f32,
    /// The stripes' direction in degrees, from +x towards +y.
    pub angle: f32,
    /// Stripes and rings: gradients across the shape; sweep: repeats round it.
    pub scale: f32,
    /// Gradients per second the pattern moves along.
    pub scroll: f32,
    /// 0 clamp, 1 repeat, 2 mirror.
    pub repeat: f32,
}

impl Default for PaintGpu {
    fn default() -> Self {
        Self { row: -1.0, t0: 0.0, pattern: 0.0, steps: 0.0, angle: 0.0, scale: 1.0, scroll: 0.0, repeat: 0.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Color, Stop, Transition};

    fn two(a: Color, b: Color, transition: Transition) -> Gradient {
        Gradient { stops: vec![Stop { at: 0.0, color: a }, Stop { at: 1.0, color: b }], transition }
    }

    #[test]
    fn same_gradient_gets_the_same_row() {
        let mut atlas = GradientAtlas::default();
        let red_blue = two(Color::rgb(255, 0, 0), Color::rgb(0, 0, 255), Transition::Smooth);
        let a = atlas.row(&red_blue);
        let b = atlas.row(&two(Color::rgb(0, 255, 0), Color::rgb(0, 0, 255), Transition::Smooth));
        assert_eq!((a, b), (0, 1));
        assert_eq!(atlas.row(&red_blue.clone()), a);
        // The transition is part of the gradient.
        assert_eq!(atlas.row(&Gradient { transition: Transition::Steps, ..red_blue }), 2);
        assert_eq!(atlas.len(), 3);
        atlas.clear();
        assert_eq!(atlas.len(), 0);
        assert_eq!(atlas, GradientAtlas::default());
    }

    #[test]
    fn atlas_grows_past_its_first_height() {
        let mut atlas = GradientAtlas::default();
        let gradients: Vec<Gradient> =
            (0..20).map(|i| two(Color::rgb(i * 10, 0, 0), Color::rgb(0, 0, 255), Transition::Smooth)).collect();
        let rows: Vec<u32> = gradients.iter().map(|g| atlas.row(g)).collect();
        assert_eq!(rows, (0..20).collect::<Vec<u32>>());
        assert_eq!(atlas.len(), 20);
        assert_eq!(atlas.rows()[0].to_vec(), gradients[0].bake(ATLAS_WIDTH));
        assert_eq!(atlas.rows()[19].to_vec(), gradients[19].bake(ATLAS_WIDTH));
        assert_eq!(atlas_height(atlas.len()), 32);
        assert_eq!(atlas_height(0), 8);
        assert_eq!(atlas_height(8), 8);
        assert_eq!(atlas_height(9), 16);
    }

    #[test]
    fn half_floats_round_to_the_nearest() {
        assert_eq!(f16_bits(0.0), 0);
        assert_eq!(f16_bits(1.0), 0x3c00);
        assert_eq!(f16_bits(0.5), 0x3800);
        assert_eq!(f16_bits(-2.0), 0xc000);
        assert_eq!(f16_bits(65504.0), 0x7bff);
        assert_eq!(f16_bits(1e9), 0x7c00);
        // The smallest subnormal half.
        assert_eq!(f16_bits(2f32.powi(-24)), 1);
        // 1 + 2^-11 is halfway between two halves and rounds to the even one.
        assert_eq!(f16_bits(1.0 + 2f32.powi(-11)), 0x3c00);
        assert_eq!(f16_bits(1.0 + 3.0 * 2f32.powi(-11)), 0x3c02);
    }

    #[test]
    fn a_new_paint_paints_nothing() {
        assert_eq!(PaintGpu::default().row, -1.0);
        assert_eq!(std::mem::size_of::<PaintGpu>(), 32);
    }
}
