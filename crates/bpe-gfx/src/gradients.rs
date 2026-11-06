//! Gradients on the GPU: every gradient a frame paints with is baked into one row of a texture,
//! and an instance names its row in a [`PaintGpu`] to paint the gradient inside itself.

use std::collections::BTreeMap;

use bpe_core::Gradient;
use bytemuck::{Pod, Zeroable};

/// Samples in one baked row, the first at 0 and the last at 1.
pub const ATLAS_WIDTH: usize = 1024;
/// The atlas texture is at least this many rows tall.
const MIN_HEIGHT: usize = 8;

/// The gradients of one frame, each baked once into its own row.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GradientAtlas {
    rows: Vec<[[f32; 4]; ATLAS_WIDTH]>,
    /// Each row's Hit wave ([`NO_WAVE`] for none), uploaded beside the rows: see [`wave_texels`].
    waves: Vec<[[f32; 4]; 2]>,
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
        self.waves.push(wave_texels(g.moves.wave.as_ref()));
        self.index.insert(key, row);
        row
    }

    /// The row holding exactly `baked`, copying it into a new row unless one already matches:
    /// for moving instances painted from another frame's atlas into this one.
    pub fn adopt(&mut self, baked: &[[f32; 4]; ATLAS_WIDTH]) -> u32 {
        if let Some(row) = self.rows.iter().position(|r| r == baked) {
            return row as u32;
        }
        self.rows.push(*baked);
        self.waves.push(NO_WAVE);
        (self.rows.len() - 1) as u32
    }

    /// The row holding Free points `points` (each a point and its stop's linear colour, at most
    /// [`bpe_core::gradient_form::MOST_POINTS`]): texel `2i` the point, `2i + 1` the colour. The
    /// same points get the same row.
    pub fn points_row(&mut self, points: &[([f32; 2], [f32; 4])]) -> u32 {
        let points = &points[..points.len().min(ATLAS_WIDTH / 2)];
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ 0xf7;
        for (p, c) in points {
            for x in p.iter().chain(c.iter()) {
                for b in x.to_bits().to_le_bytes() {
                    h ^= b as u64;
                    h = h.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
        }
        // Kept apart from the gradients' hashes.
        let key = h.rotate_left(17) ^ 0x005e_ed0f_f7ee_0001;
        if let Some(&row) = self.index.get(&key) {
            return row;
        }
        let mut baked = [[0.0; 4]; ATLAS_WIDTH];
        for (i, (p, c)) in points.iter().enumerate() {
            baked[2 * i] = [p[0], p[1], 0.0, 0.0];
            baked[2 * i + 1] = *c;
        }
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
        self.waves.clear();
        self.index.clear();
    }

    /// The baked rows in order, linear light.
    pub fn rows(&self) -> &[[[f32; 4]; ATLAS_WIDTH]] {
        &self.rows
    }

    /// Each row's Hit wave as two texels ([`wave_texels`]), in the rows' order.
    pub fn waves(&self) -> &[[[f32; 4]; 2]] {
        &self.waves
    }
}

/// A row's wave texels when its gradient has no Hit wave.
pub const NO_WAVE: [[f32; 4]; 2] = [[0.0; 4]; 2];

/// A gradient's Hit wave as paint.wgsl reads it beside its row: strength, width, speed and 1,
/// then the wave's colour in linear light. [`NO_WAVE`] for none.
pub fn wave_texels(wave: Option<&bpe_core::HitWave>) -> [[f32; 4]; 2] {
    let finite = |x: f32| if x.is_finite() { x } else { 0.0 };
    match wave {
        Some(w) if w.strength > 0.0 && w.strength.is_finite() => {
            [[w.strength, finite(w.width).max(1e-4), finite(w.speed), 1.0], w.colour.to_linear()]
        }
        _ => NO_WAVE,
    }
}

/// FNV-1a over the stops' places, colours, gap blends and middles, the mix and Loop, and its
/// moves and Hit wave when it has them (so a still gradient hashes as it always did; not the
/// grain, which doesn't change the baked row).
fn hash(g: &Gradient) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    eat_gradient(&mut h, g);
    h
}

fn eat(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= b as u64;
        *h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
}

fn eat_gradient(h: &mut u64, g: &Gradient) {
    for s in &g.stops {
        eat(h, &s.at.to_bits().to_le_bytes());
        eat(h, &[s.color.r, s.color.g, s.color.b, s.color.a, s.blend as u8]);
        eat(h, &s.middle.to_bits().to_le_bytes());
    }
    eat(h, &[g.mix as u8, g.looped as u8]);
    let m = &g.moves;
    if !m.is_still() {
        eat(h, b"moves");
        for x in [m.shift, m.breathe, m.hue] {
            eat(h, &x.to_bits().to_le_bytes());
        }
        if let Some(morph) = &m.morph {
            eat(h, &morph.amount.to_bits().to_le_bytes());
            eat_gradient(h, &morph.to);
        }
    }
    if let Some(w) = &m.wave {
        eat(h, b"wave");
        for x in wave_texels(Some(w)).iter().flatten() {
            eat(h, &x.to_bits().to_le_bytes());
        }
    }
}

/// paint.wgsl (with what it shares with the Ripples field), reading the atlas and the field from
/// bind group `group`.
pub(crate) fn paint_wgsl(group: u32) -> String {
    [
        include_str!("shaders/ripple_common.wgsl"),
        &include_str!("shaders/paint.wgsl").replace("PAINT_GROUP", &group.to_string()),
    ]
    .concat()
}

/// The bind group layout paint.wgsl reads the atlas through: the texture and its sampler, read
/// in vertex shaders too, so things that colour their vertices can paint, and the rows' Hit waves
/// (two texels a row, read whole).
pub(crate) fn paint_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    use wgpu::ShaderStages as S;
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("paint layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: S::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: S::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: S::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            crate::ripples::field_entry(3, S::VERTEX_FRAGMENT),
            crate::ripples::field_entry(4, S::VERTEX_FRAGMENT),
            crate::ripples::sources_entry(5, S::VERTEX_FRAGMENT),
        ],
    })
}

/// How many rows the texture needs for `len` gradients: a power of two, at least 8, but no
/// taller than `max` (the device's largest texture side).
pub(crate) fn atlas_height(len: usize, max: usize) -> usize {
    len.next_power_of_two().max(MIN_HEIGHT).min(max)
}

/// How many of `len` gradients fit in a texture at most `max` rows tall. paint.wgsl paints any
/// row past the last with the last one.
pub(crate) fn uploaded_rows(len: usize, max: usize) -> usize {
    len.min(max)
}

/// The rows as half floats, ready for an `Rgba16Float` texture.
pub(crate) fn half_texels<const W: usize>(rows: &[[[f32; 4]; W]]) -> Vec<u8> {
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
    /// The gradient's atlas row; -1 paints nothing and the instance keeps its own colours. -2
    /// is shade mode: the pattern blends the instance's own colour, at 0, to that colour shaded
    /// by `t0` (toward black below 0, toward white above, in linear light), at 1, with no
    /// gradient and no variation.
    pub row: f32,
    /// Where along the gradient the pattern starts (stripes and rings are centred on it, a path
    /// starts there), 0 to 1. In shade mode the shade, -1 to 1.
    pub t0: f32,
    /// 0 none (the colour at `t0`), 1 linear, 2 radial, 3 sweep, 4 path, 5 stripes, 6 rings,
    /// 7 dots, 8 checker, 9 halves, 10 slices, 11 swirl, 12 face. Linear runs 0 to 1 across the
    /// shape, radial 0 to 1 from its centre out, sweep 0 to 1 round it; these honour `repeat`.
    /// Stripes, rings and path never run off the gradient's end: Clamp mirrors them. Dots to
    /// face paint their mask (0 or 1) along the gradient, clamped whatever the repeat.
    pub pattern: f32,
    /// The gradient's grain: still per-pixel noise of this strength in 1/255 steps (its size
    /// is taken), 0 none. See [`bpe_core::Gradient::grain`].
    pub grain: f32,
    /// The pattern's turn in degrees, from +x towards +y: where linear and stripes point, where
    /// sweep starts, and how far the masks are turned.
    pub angle: f32,
    /// The pattern's size: gradients across the shape (linear, stripes), from its centre out
    /// (radial, rings) or round it (sweep); repeats across it for the masks.
    pub scale: f32,
    /// Gradients per second the pattern moves along.
    pub scroll: f32,
    /// 0 clamp, 1 repeat, 2 mirror.
    pub repeat: f32,
    /// The instance's own colour variation, turned on what the gradient paints: hue turned by
    /// this many turns, saturation added, value multiplied, and one unused. [`NO_VARY`] leaves
    /// the colour exactly as sampled.
    pub vary: [f32; 4],
    /// The form's own numbers (see `paint_form_along` in paint.wgsl and
    /// [`bpe_core::Layout::numbers`]); for Free points `[1]` is the row [`GradientAtlas::points_row`]
    /// gave, `[2]` how many and `[3]` 1 to blend in OKLab.
    pub form: [f32; 4],
    /// The form's centre xy in the instance's own space (before the turn), then spare.
    pub place: [f32; 4],
}

/// A [`PaintGpu::vary`] that changes nothing.
pub const NO_VARY: [f32; 4] = [0.0, 0.0, 1.0, 0.0];

impl Default for PaintGpu {
    fn default() -> Self {
        Self {
            row: -1.0,
            t0: 0.0,
            pattern: 0.0,
            grain: 0.0,
            angle: 0.0,
            scale: 1.0,
            scroll: 0.0,
            repeat: 0.0,
            vary: NO_VARY,
            form: [0.0; 4],
            place: [0.0; 4],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Color, GapBlend, Mix};

    fn two(a: Color, b: Color) -> Gradient {
        Gradient::even(&[a, b])
    }

    #[test]
    fn same_gradient_gets_the_same_row() {
        let mut atlas = GradientAtlas::default();
        let red_blue = two(Color::rgb(255, 0, 0), Color::rgb(0, 0, 255));
        let a = atlas.row(&red_blue);
        let b = atlas.row(&two(Color::rgb(0, 255, 0), Color::rgb(0, 0, 255)));
        assert_eq!((a, b), (0, 1));
        assert_eq!(atlas.row(&red_blue.clone()), a);
        // The gap blends, middles, mix and Loop are part of the gradient; the grain isn't.
        let mut steps = red_blue.clone();
        steps.stops[0].blend = GapBlend::Steps;
        assert_eq!(atlas.row(&steps), 2);
        let mut middle = red_blue.clone();
        middle.stops[0].middle = 0.3;
        assert_eq!(atlas.row(&middle), 3);
        assert_eq!(atlas.row(&Gradient { mix: Mix::Even, ..red_blue.clone() }), 4);
        assert_eq!(atlas.row(&Gradient { looped: true, ..red_blue.clone() }), 5);
        assert_eq!(atlas.row(&Gradient { grain: 4.0, ..red_blue }), a);
        assert_eq!(atlas.len(), 6);
        atlas.clear();
        assert_eq!(atlas.len(), 0);
        assert_eq!(atlas, GradientAtlas::default());
    }

    #[test]
    fn moves_and_waves_get_their_own_rows() {
        let mut atlas = GradientAtlas::default();
        let still = two(Color::rgb(255, 0, 0), Color::rgb(0, 0, 255));
        let a = atlas.row(&still);
        assert_eq!(atlas.waves()[a as usize], NO_WAVE);
        let slid = Gradient { moves: bpe_core::Moves { shift: 0.25, ..Default::default() }, ..still.clone() };
        let b = atlas.row(&slid);
        assert_ne!(a, b);
        assert_eq!(atlas.rows()[b as usize].to_vec(), slid.bake(ATLAS_WIDTH));
        assert_ne!(atlas.rows()[a as usize], atlas.rows()[b as usize]);
        let wave = bpe_core::HitWave { strength: 1.5, width: 0.2, speed: -3.0, colour: Color::WHITE };
        let waved = Gradient { moves: bpe_core::Moves { wave: Some(wave), ..Default::default() }, ..still.clone() };
        let c = atlas.row(&waved);
        assert_ne!(c, a, "a wave needs its own wave texels");
        assert_eq!(atlas.rows()[c as usize], atlas.rows()[a as usize], "but bakes the same colours");
        assert_eq!(atlas.waves()[c as usize], [[1.5, 0.2, -3.0, 1.0], [1.0, 1.0, 1.0, 1.0]]);
        assert_eq!(atlas.row(&waved.clone()), c);
        assert_eq!(atlas.row(&still), a, "a still gradient hashes as it always did");
        assert_eq!(atlas.waves().len(), atlas.len());
    }

    #[test]
    fn adopting_a_row_reuses_a_matching_one() {
        let mut from = GradientAtlas::default();
        let red_blue = two(Color::rgb(255, 0, 0), Color::rgb(0, 0, 255));
        let green = two(Color::rgb(0, 255, 0), Color::rgb(0, 255, 0));
        let (rb, g) = (from.row(&red_blue), from.row(&green));
        let mut into = GradientAtlas::default();
        into.row(&green);
        assert_eq!(into.adopt(&from.rows()[rb as usize]), 1);
        assert_eq!(into.adopt(&from.rows()[g as usize]), 0);
        assert_eq!(into.adopt(&from.rows()[rb as usize]), 1);
        assert_eq!(into.len(), 2);
        // The adopted row holds the same colours.
        assert_eq!(into.rows()[1], from.rows()[rb as usize]);
    }

    #[test]
    fn atlas_grows_past_its_first_height() {
        let mut atlas = GradientAtlas::default();
        let gradients: Vec<Gradient> = (0..20).map(|i| two(Color::rgb(i * 10, 0, 0), Color::rgb(0, 0, 255))).collect();
        let rows: Vec<u32> = gradients.iter().map(|g| atlas.row(g)).collect();
        assert_eq!(rows, (0..20).collect::<Vec<u32>>());
        assert_eq!(atlas.len(), 20);
        assert_eq!(atlas.rows()[0].to_vec(), gradients[0].bake(ATLAS_WIDTH));
        assert_eq!(atlas.rows()[19].to_vec(), gradients[19].bake(ATLAS_WIDTH));
        assert_eq!(atlas_height(atlas.len(), 8192), 32);
        assert_eq!(atlas_height(0, 8192), 8);
        assert_eq!(atlas_height(8, 8192), 8);
        assert_eq!(atlas_height(9, 8192), 16);
    }

    #[test]
    fn the_atlas_stops_growing_at_the_device_s_largest_texture() {
        assert_eq!(atlas_height(5000, 8192), 8192);
        assert_eq!(atlas_height(9000, 8192), 8192);
        // A limit that is not a power of two is still a limit.
        assert_eq!(atlas_height(3000, 2048 + 100), 2148);
        assert_eq!(atlas_height(0, 4), 4);
        // Only the rows that fit are uploaded; the shader paints later rows with the last one.
        assert_eq!(uploaded_rows(9000, 8192), 8192);
        assert_eq!(uploaded_rows(20, 8192), 20);
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
        assert_eq!(std::mem::size_of::<PaintGpu>(), 80);
    }
}
