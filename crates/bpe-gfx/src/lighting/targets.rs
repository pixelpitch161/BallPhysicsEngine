//! The pictures lighting draws into and reads, the 1b passes' uniform and inputs, and how
//! Quality sets their sizes.

use bytemuck::{Pod, Zeroable};

use super::*;

/// What the part 1b passes read (see `Glow` in the lighting shader).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(super) struct GlowUniform {
    /// The field's size in texels, then its size rounded up for the cascades.
    pub(super) size: [f32; 4],
    /// This cascade, how many there are, steps a ray, which run (0 the first, 1 the bounce's).
    pub(super) cascade: [f32; 4],
    /// Bounce light, glow reach (world units), Light blooms, the bloom level being made.
    pub(super) glow: [f32; 4],
    /// The haze's colour times its amount, how much it dims.
    pub(super) air: [f32; 4],
    /// 1 when there is a light field, 1 when there is a bloom, 1 when there is haze, spare.
    pub(super) on: [f32; 4],
    /// Part 4: the haze moving, the water, the memory (see `Glow` in the lighting shader).
    pub(super) haze: [f32; 4],
    pub(super) haze_more: [f32; 4],
    pub(super) water: [f32; 4],
    pub(super) water_colour: [f32; 4],
    pub(super) water_depth: [f32; 4],
    pub(super) memory: [f32; 4],
    pub(super) memory_colour: [f32; 4],
    pub(super) memory_step: [f32; 4],
    pub(super) last_camera: [f32; 4],
    pub(super) more_on: [f32; 4],
    /// Glass and light part 4b: Rainbow, size, Second bow, Dark band.
    pub(super) rainbow: [f32; 4],
    /// The bow's middle (world xy), 1 when it shows in rain (the tags picture's B), spare.
    pub(super) rainbow_at: [f32; 4],
}

/// Which pictures each pass's group 1 binds (besides its uniform), so no pass binds more than
/// the 16 textures a stage may see: the 1b passes, the direct light (which reads the tags for
/// Lights up), the mix and the extras pass.
pub(super) const GLOW_BINDINGS: &[u32] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
pub(super) const DIRECT_BINDINGS: &[u32] = &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
pub(super) const MIX_BINDINGS: &[u32] = &[2, 3, 6, 7, 8, 9, 10, 11, 12, 13];
pub(super) const EXTRAS_BINDINGS: &[u32] = &[2, 6, 7, 8, 9, 14];

/// The longest gap between two frames over which the remembered light carries on; a longer jump
/// (or one back) starts it again from nothing.
pub(super) const MOST_MEMORY_STEP: f32 = 0.5;
/// Depth of field's small copy is this many times smaller each way (`DOF_SMALL` in the lighting shader).
pub(super) const DOF_SMALL: u32 = 4;

/// Glow in the dark's memory between frames.
#[derive(Default)]
pub(super) struct Memory {
    /// When the last frame that remembered was drawn, and through which camera.
    pub(super) last: Option<(f32, [f32; 4])>,
    /// Which of the two extras pictures holds the memory now.
    pub(super) current: usize,
    /// The memory started again from nothing at a moment after the start (a seek): the editor
    /// warms it up.
    pub(super) cold: bool,
}

/// How many cascades a light field needs to reach `reach` field texels, the first reaching one.
pub fn cascade_count(reach: f32) -> u32 {
    let mut n = 1;
    // Cascade i reaches (4^(i+1) - 1) / 3 texels.
    while n < MOST_CASCADES && ((4f32.powi(n as i32) - 1.0) / 3.0) < reach {
        n += 1;
    }
    n
}

/// How many steps a light field ray takes in one cascade, for a Quality.
pub fn gi_steps(quality: f32) -> u32 {
    match quality.round() as i32 {
        i32::MIN..=0 => 8,
        1 => 12,
        2 => 16,
        3 => 24,
        // Bake (lighting part 3).
        _ => 32,
    }
}
/// How big the field is against the picture, and how many steps a shadow takes, for a Quality.
pub fn quality_tier(quality: f32) -> (u32, u32) {
    match quality.round() as i32 {
        i32::MIN..=0 => (4, 16),
        1 => (2, 32),
        2 => (2, 64),
        3 => (1, 96),
        // Bake (lighting part 3): the field at full size, longer shadow marches.
        _ => (1, 128),
    }
}

/// How many output pixels a lighting field texel covers each way, for a Quality whose field is `tier` times
/// smaller than the video (`quality_tier`), drawn `width` pixels wide of a video `video_width` wide: a smaller
/// picture (the editor's preview) gets a finer field, so a field texel covers as much of the scene as in the video
/// and the preview's shadows look like the video's (or finer, rounding down), never coarser than the Quality says.
pub fn field_divisor(tier: u32, width: u32, video_width: u32) -> u32 {
    let scaled = (tier as f32 * width as f32 / video_width.max(1) as f32 + 1e-4).floor() as u32;
    scaled.clamp(1, tier.max(1))
}

/// The pictures lighting draws into and reads, made the first time a frame is lit and remade
/// when the size or the Quality changes.
pub(super) struct Targets {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) divisor: u32,
    pub(super) lit: (wgpu::Texture, wgpu::TextureView),
    pub(super) surface: (wgpu::Texture, wgpu::TextureView),
    /// The light things give off (pass 3).
    pub(super) emission: (wgpu::Texture, wgpu::TextureView),
    /// The light from Light things on the surfaces (and the background's own light), then the
    /// light in the air.
    pub(super) direct: (wgpu::Texture, wgpu::TextureView),
    pub(super) air: (wgpu::Texture, wgpu::TextureView),
    /// The jump flood's pictures: the blockers' pair, then the light field's pair.
    pub(super) seeds: [(wgpu::Texture, wgpu::TextureView); 4],
    pub(super) field: (wgpu::Texture, wgpu::TextureView),
    /// What the light field is made from (light given off and bounced, how opaque) and how far
    /// each texel is from something opaque.
    pub(super) gi_scene: (wgpu::Texture, wgpu::TextureView),
    pub(super) gi_dist: (wgpu::Texture, wgpu::TextureView),
    /// The light field: the light arriving at each texel from each of four ways (see
    /// `gi_a` in the lighting shader).
    pub(super) gi: [(wgpu::Texture, wgpu::TextureView); 3],
    /// The cascades, made the first time a light field is drawn.
    pub(super) cascades: Option<[(wgpu::Texture, wgpu::TextureView); 2]>,
    /// The field's size rounded up so every cascade's probes tile it.
    pub(super) padded: (u32, u32),
    /// The lights' bloom: a picture with [`GLOW_LEVELS`] levels, a view of each and of them all.
    pub(super) glow: (wgpu::Texture, Vec<wgpu::TextureView>, wgpu::TextureView),
    /// Bound in place of a picture a pass draws into.
    pub(super) dummy: (wgpu::Texture, wgpu::TextureView),
    /// The light pass's picture when a glossy floor mirrors it afterwards (made when first needed).
    pub(super) stage: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// A second such picture when depth of field follows the floor (part 2b).
    pub(super) stage_more: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// Depth of field's quarter-size copy of the picture it blurs (made when first needed).
    pub(super) dof_small: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// The picture without what is see-through, drawn when glass bends what is behind it.
    pub(super) behind: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// The background's own light alone, copied before the lamps' light is added over it.
    pub(super) bg_glow: Option<(wgpu::Texture, wgpu::TextureView)>,
    pub(super) dummy_colour: (wgpu::Texture, wgpu::TextureView),
    /// Part 4: what glows under UV (light pass 5) and the tags (pass 6: what catches beams, link
    /// numbers), full size; made only while a frame draws them ([`Lighting::keep_uv_and_tags`],
    /// review S24).
    pub(super) uv: Option<(wgpu::Texture, wgpu::TextureView)>,
    pub(super) tags: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// Glass and light part 1: the tint picture (light pass 8, how deep each see-through thing's
    /// colour is), full size, made the first time it is drawn.
    pub(super) tint: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// Glass and light part 3: free glass (light pass 9, made by [`Lighting::make_glass`]) and the
    /// copy of the surface picture it is marked over.
    pub(super) glass: Option<[(wgpu::Texture, wgpu::TextureView); 2]>,
    /// The extras pictures (the memory and the haze's density, field size, two to keep the last
    /// frame's memory) and the balls' stirring, made the first time they are drawn.
    pub(super) extras: Option<[(wgpu::Texture, wgpu::TextureView); 2]>,
    pub(super) stir: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// The bake's bounce light and its clean-up's second picture (part 3), made when first traced.
    pub(super) traced: Option<[(wgpu::Texture, wgpu::TextureView); 2]>,
}

impl Targets {
    pub(super) fn field_size(&self) -> (u32, u32) {
        ((self.width / self.divisor).max(1), (self.height / self.divisor).max(1))
    }
}

#[derive(Clone, Copy)]
pub(super) struct GlowInputs<'a> {
    pub(super) emission: &'a wgpu::TextureView,
    pub(super) direct: &'a wgpu::TextureView,
    pub(super) air: &'a wgpu::TextureView,
    pub(super) gi_scene: &'a wgpu::TextureView,
    pub(super) gi_dist: &'a wgpu::TextureView,
    pub(super) above: &'a wgpu::TextureView,
    pub(super) gi: [&'a wgpu::TextureView; 3],
    pub(super) glow: &'a wgpu::TextureView,
    pub(super) tags: &'a wgpu::TextureView,
    pub(super) uv: &'a wgpu::TextureView,
    pub(super) extras: &'a wgpu::TextureView,
    pub(super) stir: &'a wgpu::TextureView,
}

impl<'a> GlowInputs<'a> {
    /// The picture at group 1 `binding`.
    pub(super) fn at(&self, binding: u32) -> &'a wgpu::TextureView {
        match binding {
            1 => self.emission,
            2 => self.direct,
            3 => self.air,
            4 => self.gi_scene,
            5 => self.gi_dist,
            6 => self.above,
            7 => self.gi[0],
            8 => self.gi[1],
            9 => self.gi[2],
            10 => self.glow,
            11 => self.tags,
            12 => self.uv,
            13 => self.extras,
            _ => self.stir,
        }
    }
}
