//! Lighting in the flat picture (parts 1a and 1b of the lighting spec): with a Lighting card on
//! the scene the renderer draws the things again (their lit colour, their surface and, when
//! something gives off light, that light; see [`crate::Renderer::render`]), then this works out
//! a signed distance field of everything that blocks light (a jump flood), the light from every
//! Light thing at each pixel with its shadow marched through the field (and the light in the
//! haze), the light field (radiance cascades: every glowing thing lights the rest, and lit things
//! bounce light), the Light blooms, and mixes it all in. Nothing here is made or run for a scene
//! without the card, and no 1b pass runs when its settings are off.
//!
//! Part 4 adds, each only when used: the balls stirring moving haze, the extras picture (the
//! moving haze's density and the remembered light of glow in the dark, which is kept from frame
//! to frame), and in the mix smoke, underwater light, UV glow, the memory's glow and laser beams.

use bytemuck::{Pod, Zeroable};

use crate::bent_light::{self, BENT_LEGS, BentUniform, Leg};
use crate::frame::{LightGpu, LightScene, MOST_MIRRORS, MirrorGpu, SphereGpu, SurfaceGpu};
use crate::{Frame, GrowableBuffer, HDR_FORMAT, light_pass};

mod binds;
mod glass;
mod kit;
mod passes;
mod run;
mod targets;
#[cfg(test)]
mod tests;
mod tiles;

use binds::BindCache;
pub use glass::*;
pub(crate) use kit::LightKit;
use targets::*;
pub use targets::{cascade_count, field_divisor, gi_steps, quality_tier};
pub use tiles::*;

/// The light field's pictures: what it is made from and what it gives.
const GI_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// The most cascades the light field uses (the last reaches 87381 field texels).
const MOST_CASCADES: u32 = 9;
/// The bloom of the lights: its first picture and this many smaller ones.
const GLOW_LEVELS: u32 = 4;

const SEED_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;
const FIELD_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;
// The blockers' field: the signed distance and how much the nearest blocker blocks of each
// channel (see-through and coloured blockers, the lighting shader's `fs_field`).
const BLOCKER_FIELD_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// The most jump flood passes: enough for a field 2^15 texels across.
const MOST_JUMPS: usize = 16;

/// What the lighting passes read (see `Lighting` in the lighting shader).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct Uniform {
    view: [f32; 4],
    camera: [f32; 4],
    ambient: [f32; 4],
    shadows: [f32; 4],
    more: [f32; 4],
    field: [f32; 4],
    /// Lighting part 3: this sample's index, how many, 1 when baking, light bounces.
    sample: [f32; 4],
    /// The bake's traced bounce: Bounce light, 1 on a Stage (see `fs_seed`), 1 when light through
    /// glass takes its colour, 1 when the tint picture was drawn (glass and light part 1).
    trace: [f32; 4],
    /// Glass and light part 3: 1 when glass bends lamp light (the photon pass runs); part 4b: how
    /// many entries `dichroic` has; two spare.
    bent: [f32; 4],
    /// Review S19: the light tiles ([`LightTiles::uniform`]): tiles per unit of uv across and
    /// down, how many across and down (0: no lists).
    tiles: [f32; 4],
    /// Where the tiles' heads start in the links buffer; three spare.
    tile_list: [f32; 4],
    /// Which surface indices are glass that bends lamp light ([`bent_light::bend_bits`]).
    bends: [[u32; 4]; 16],
    /// Glass and light part 4b: the surfaces that are dichroic, [index, Dichroic, shift, hue]
    /// ([`dichroic_list`]), for the shadows' march (whose passes don't see the surface table).
    dichroic: [[f32; 4]; MOST_DICHROIC],
}

/// The most dichroic surfaces the shadows tint by (glass and light part 4b).
pub const MOST_DICHROIC: usize = 16;

pub(crate) struct Lighting {
    layout: wgpu::BindGroupLayout,
    glow_layout: wgpu::BindGroupLayout,
    direct_layout: wgpu::BindGroupLayout,
    mix_layout: wgpu::BindGroupLayout,
    extras_layout: wgpu::BindGroupLayout,
    stir_layout: wgpu::BindGroupLayout,
    seed: wgpu::RenderPipeline,
    jump: wgpu::RenderPipeline,
    field: wgpu::RenderPipeline,
    direct: wgpu::RenderPipeline,
    gi_scene: wgpu::RenderPipeline,
    seed_gi: wgpu::RenderPipeline,
    gi_dist: wgpu::RenderPipeline,
    cascade: wgpu::RenderPipeline,
    resolve: wgpu::RenderPipeline,
    glow_source: wgpu::RenderPipeline,
    glow_down: wgpu::RenderPipeline,
    mix: wgpu::RenderPipeline,
    extras: wgpu::RenderPipeline,
    stir: wgpu::RenderPipeline,
    /// The glossy floor's pass (Stage).
    floor: wgpu::RenderPipeline,
    /// Depth of field (Stage, part 2b).
    dof: wgpu::RenderPipeline,
    /// The quarter-size box-filtered copy depth of field reads its wide blurs from.
    dof_small: wgpu::RenderPipeline,
    /// Mirror walls (part 2b).
    mirror: wgpu::RenderPipeline,
    /// The bake's bounce light in Stage, its clean-up and the pass adding it (part 3).
    trace: wgpu::RenderPipeline,
    denoise: wgpu::RenderPipeline,
    trace_add: wgpu::RenderPipeline,
    /// Caustics (part 3): the photons' pipeline, their group 3 and its parts, an empty group 1.
    caustic: wgpu::RenderPipeline,
    caustic_layout: wgpu::BindGroupLayout,
    /// Glass and light part 2: light beams' bands, added after the mix.
    ribbon: wgpu::RenderPipeline,
    ribbon_layout: wgpu::BindGroupLayout,
    ribbons: GrowableBuffer,
    empty_group: wgpu::BindGroup,
    caustic_uniform: wgpu::Buffer,
    glass: GrowableBuffer,
    /// Light bent by glass (glass and light part 3): the photons' compute pipeline and the pipeline
    /// splatting their legs, the layouts of their groups 3, their uniform, the pairs, the legs and
    /// how many photons the last run sent.
    bent: wgpu::ComputePipeline,
    bent_splat: wgpu::RenderPipeline,
    bent_layout: wgpu::BindGroupLayout,
    bent_draw_layout: wgpu::BindGroupLayout,
    bent_uniform: wgpu::Buffer,
    bent_pairs: GrowableBuffer,
    legs: GrowableBuffer,
    bent_count: u32,
    /// Marks free glass in the surface picture (`shaders/free_glass.wgsl`) and its layout.
    free_glass: wgpu::RenderPipeline,
    free_glass_layout: wgpu::BindGroupLayout,
    /// The clean-up passes' uniforms: steps 1, 2, 4, and 0 (a plain copy).
    trace_uniforms: Vec<wgpu::Buffer>,
    sampler: wgpu::Sampler,
    /// Surfaces and the stage (group 2 of the mix and floor passes).
    stage_layout: wgpu::BindGroupLayout,
    stage_uniform: wgpu::Buffer,
    surfaces: GrowableBuffer,
    spheres: GrowableBuffer,
    cells: GrowableBuffer,
    items: GrowableBuffer,
    mirrors: GrowableBuffer,
    /// The base uniform, then one per jump flood pass (each with its own step).
    uniforms: Vec<wgpu::Buffer>,
    /// The 1b passes' uniforms: the base one, then one per cascade of each run, then one per
    /// bloom level.
    glow_uniforms: Vec<wgpu::Buffer>,
    lights: GrowableBuffer,
    segments: GrowableBuffer,
    links: GrowableBuffer,
    stirs: GrowableBuffer,
    targets: Option<Targets>,
    memory: Memory,
    /// The bind groups [`Lighting::run`] makes, kept between frames (review S24).
    binds: BindCache,
    /// Whether pixels take only their tile's lights (review S19; off only to check the lists).
    pub(crate) tiles_on: bool,
    /// The last run's bind groups 0 and 2, for the Stage camera pass (lighting part 5).
    bound: Option<[wgpu::BindGroup; 2]>,
    /// The Stage camera's pipelines, from the kit this pass was made with.
    stage: crate::stage_camera_pass::StageKit,
}

fn texture(
    device: &wgpu::Device,
    label: &'static str,
    (w, h): (u32, u32),
    format: wgpu::TextureFormat,
    copy: bool,
) -> (wgpu::Texture, wgpu::TextureView) {
    let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    if copy {
        usage |= wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST;
    }
    let t = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    let v = t.create_view(&Default::default());
    (t, v)
}

fn uniform_buffer(device: &wgpu::Device, label: &'static str, size: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl Lighting {
    /// A lighting pass drawing with `kit`'s pipelines (built once and shared by every renderer
    /// on the device), with its own pictures and buffers.
    pub fn with_kit(device: &wgpu::Device, kit: &LightKit) -> Self {
        let LightKit {
            layout,
            glow_layout,
            direct_layout,
            mix_layout,
            extras_layout,
            stir_layout,
            seed,
            jump,
            field,
            direct,
            gi_scene,
            seed_gi,
            gi_dist,
            cascade,
            resolve,
            glow_source,
            glow_down,
            mix,
            extras,
            stir,
            floor,
            dof,
            dof_small,
            mirror,
            trace,
            denoise,
            trace_add,
            caustic,
            caustic_layout,
            ribbon,
            ribbon_layout,
            empty_group,
            bent,
            bent_splat,
            bent_layout,
            bent_draw_layout,
            free_glass,
            free_glass_layout,
            sampler,
            stage_layout,
            stage,
        } = kit.clone();
        let uniforms = (0..=MOST_JUMPS)
            .map(|_| uniform_buffer(device, "lighting uniform", std::mem::size_of::<Uniform>()))
            .collect();
        Self {
            layout,
            glow_layout,
            direct_layout,
            mix_layout,
            extras_layout,
            stir_layout,
            seed,
            jump,
            field,
            direct,
            gi_scene,
            seed_gi,
            gi_dist,
            cascade,
            resolve,
            glow_source,
            glow_down,
            mix,
            extras,
            stir,
            floor,
            dof,
            dof_small,
            mirror,
            trace,
            denoise,
            trace_add,
            caustic,
            caustic_layout,
            ribbon,
            ribbon_layout,
            empty_group,
            bent,
            bent_splat,
            bent_layout,
            bent_draw_layout,
            free_glass,
            free_glass_layout,
            sampler,
            stage_layout,
            stage,
            ribbons: GrowableBuffer::storage("beam ribbons"),
            caustic_uniform: uniform_buffer(device, "caustics uniform", std::mem::size_of::<CausticUniform>()),
            glass: GrowableBuffer::storage("caustic glass"),
            bent_uniform: uniform_buffer(device, "bent light uniform", std::mem::size_of::<BentUniform>()),
            bent_pairs: GrowableBuffer::storage("bent light pairs"),
            legs: GrowableBuffer::storage("bent light legs"),
            bent_count: 0,
            trace_uniforms: (0..4)
                .map(|_| uniform_buffer(device, "bounce light uniform", std::mem::size_of::<Uniform>()))
                .collect(),
            stage_uniform: uniform_buffer(device, "lighting stage uniform", std::mem::size_of::<StageUniform>()),
            surfaces: GrowableBuffer::storage("light surfaces"),
            spheres: GrowableBuffer::storage("light spheres"),
            cells: GrowableBuffer::storage("light sphere cells"),
            items: GrowableBuffer::storage("light sphere items"),
            mirrors: GrowableBuffer::storage("light mirrors"),
            uniforms,
            glow_uniforms: Vec::new(),
            lights: GrowableBuffer::storage("lights"),
            segments: GrowableBuffer::storage("light segments"),
            links: GrowableBuffer::storage("light links"),
            stirs: GrowableBuffer::storage("haze stirs"),
            targets: None,
            memory: Memory::default(),
            binds: BindCache::default(),
            tiles_on: true,
            bound: None,
        }
    }

    /// The Stage camera's pipelines, from the kit.
    pub(crate) fn stage_kit(&self) -> &crate::stage_camera_pass::StageKit {
        &self.stage
    }

    /// How many photons the last lit frame sent through glass that bends light (0 when it sent
    /// none).
    pub fn bent_photons(&self) -> u32 {
        self.bent_count
    }

    /// The last run's bind groups 0 and 2.
    pub(crate) fn bound(&self) -> Option<&[wgpu::BindGroup; 2]> {
        self.bound.as_ref()
    }

    /// Makes the lighting pictures for a `width` × `height` output of a scene `video_width` world units wide (the
    /// frame's width) lit as `scene` asks, unless they are there already.
    pub fn ensure(&mut self, device: &wgpu::Device, width: u32, height: u32, video_width: u32, scene: &LightScene) {
        let (tier, _) = quality_tier(scene.settings.more[2]);
        let mut divisor = field_divisor(tier, width, video_width);
        // The cascades are twice the (rounded up) field across: keep them within the device's limit.
        let most = device.limits().max_texture_dimension_2d;
        let padded = |divisor: u32| {
            let (w, h) = ((width / divisor).max(1), (height / divisor).max(1));
            let n = cascade_count(((w * w + h * h) as f32).sqrt());
            let tile = 1u32 << (n - 1);
            (w.div_ceil(tile) * tile, h.div_ceil(tile) * tile)
        };
        while divisor < 64 && {
            let (pw, ph) = padded(divisor);
            2 * pw.max(ph) > most
        } {
            divisor *= 2;
        }
        if self.targets.as_ref().is_some_and(|t| t.width == width && t.height == height && t.divisor == divisor) {
            return;
        }
        let full = (width, height);
        let small = ((width / divisor).max(1), (height / divisor).max(1));
        let glow = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("light bloom"),
            // At least 8 texels a side, so every level has one (a tiny draft render).
            size: wgpu::Extent3d { width: small.0.max(8), height: small.1.max(8), depth_or_array_layers: 1 },
            mip_level_count: GLOW_LEVELS,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: GI_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let levels = (0..GLOW_LEVELS)
            .map(|level| {
                glow.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let all = glow.create_view(&Default::default());
        let seeds = || texture(device, "light seeds", small, SEED_FORMAT, false);
        self.targets = Some(Targets {
            width,
            height,
            divisor,
            lit: texture(device, "lit colour", full, HDR_FORMAT, true),
            surface: texture(device, "surface", full, HDR_FORMAT, true),
            emission: texture(device, "light given off", full, HDR_FORMAT, true),
            direct: texture(device, "direct light", full, HDR_FORMAT, true),
            air: texture(device, "light in the air", full, HDR_FORMAT, false),
            seeds: [seeds(), seeds(), seeds(), seeds()],
            field: texture(device, "light field", small, BLOCKER_FIELD_FORMAT, false),
            gi_scene: texture(device, "light field scene", small, GI_FORMAT, false),
            gi_dist: texture(device, "light field distance", small, FIELD_FORMAT, false),
            gi: [
                texture(device, "light field ways a", small, GI_FORMAT, false),
                texture(device, "light field ways b", small, GI_FORMAT, false),
                texture(device, "light field ways c", small, GI_FORMAT, false),
            ],
            cascades: None,
            padded: padded(divisor),
            glow: (glow, levels, all),
            dummy: texture(device, "light field stand-in", (1, 1), FIELD_FORMAT, false),
            dummy_colour: texture(device, "lighting stand-in", (1, 1), GI_FORMAT, false),
            uv: None,
            tags: None,
            tint: None,
            glass: None,
            extras: None,
            stir: None,
            traced: None,
            stage: None,
            stage_more: None,
            dof_small: None,
            behind: None,
            bg_glow: None,
        });
        // New pictures: nothing is remembered, and no group binds them yet.
        self.memory.last = None;
        self.binds.clear();
    }

    /// The lit colour, surface and emission pictures (texture, view), made by
    /// [`Lighting::ensure`].
    pub fn pictures(&self) -> Option<[(wgpu::Texture, wgpu::TextureView); 3]> {
        self.targets.as_ref().map(|t| [t.lit.clone(), t.surface.clone(), t.emission.clone()])
    }

    /// The picture light pass `mode` draws into (1 lit colour, 2 surface, 3 emission, 5 what
    /// glows under UV, 6 the tags, made by [`Lighting::ensure`]; 8 the tint picture, made by
    /// [`Lighting::make_tint`]).
    pub fn picture(&self, mode: u32) -> Option<(wgpu::Texture, wgpu::TextureView)> {
        let t = self.targets.as_ref()?;
        Some(match mode {
            light_pass::LIT => t.lit.clone(),
            light_pass::SURFACE => t.surface.clone(),
            light_pass::EMISSION => t.emission.clone(),
            light_pass::UV => t.uv.clone()?,
            light_pass::TINT => t.tint.clone()?,
            light_pass::GLASS => t.glass.as_ref()?[0].clone(),
            _ => t.tags.clone()?,
        })
    }

    /// Makes the UV picture (light pass 5) and the tags picture (pass 6) when this frame draws them,
    /// and lets them go when it doesn't (review S24: two full-size pictures most scenes never use).
    pub fn keep_uv_and_tags(&mut self, device: &wgpu::Device, uv: bool, tags: bool) {
        let Some(t) = self.targets.as_mut() else { return };
        let size = (t.width, t.height);
        let changed = uv != t.uv.is_some() || tags != t.tags.is_some();
        if uv != t.uv.is_some() {
            t.uv = uv.then(|| texture(device, "glows under UV", size, HDR_FORMAT, true));
        }
        if tags != t.tags.is_some() {
            t.tags = tags.then(|| texture(device, "light tags", size, HDR_FORMAT, true));
        }
        if changed {
            // Groups binding a picture let go of would keep it.
            self.binds.clear();
        }
    }

    /// Makes the tint picture (light pass 8) the first time it is wanted.
    pub fn make_tint(&mut self, device: &wgpu::Device) {
        if let Some(t) = self.targets.as_mut().filter(|t| t.tint.is_none()) {
            t.tint = Some(texture(device, "light tint", (t.width, t.height), HDR_FORMAT, true));
        }
    }

    /// Makes the glass picture (light pass 9) and the surface picture's copy the first time they
    /// are wanted (glass and light part 3).
    pub fn make_glass(&mut self, device: &wgpu::Device) {
        if let Some(t) = self.targets.as_mut().filter(|t| t.glass.is_none()) {
            let size = (t.width, t.height);
            t.glass = Some([
                texture(device, "free glass", size, HDR_FORMAT, false),
                texture(device, "surface under free glass", size, HDR_FORMAT, true),
            ]);
        }
    }

    /// Whether the remembered light (glow in the dark) started again from nothing at a moment
    /// after the start, so it is less than it would be in the video until warmed up.
    pub fn memory_cold(&self) -> bool {
        self.memory.cold
    }

    /// Forgets the remembered light (the scene changed): the next lit frame starts it again.
    pub fn forget_memory(&mut self) {
        self.memory.last = None;
    }

    /// The picture the background's own light is drawn into, before [`Lighting::run`] adds the
    /// lamps' light to it.
    pub fn background_glow(&self) -> Option<wgpu::TextureView> {
        self.targets.as_ref().map(|t| t.direct.1.clone())
    }

    /// The picture glass shows (bent) behind it, made when `scene` has see-through things: draw
    /// the frame into it with what is see-through left out.
    pub fn behind(&mut self, device: &wgpu::Device, scene: &LightScene) -> Option<(wgpu::Texture, wgpu::TextureView)> {
        let t = self.targets.as_mut()?;
        if !scene.refracts {
            return None;
        }
        if t.behind.is_none() {
            t.behind = Some(texture(device, "behind glass", (t.width, t.height), HDR_FORMAT, true));
        }
        t.behind.clone()
    }

    /// How many bind groups the lighting has made (review S24; a test checks a frame like the last
    /// makes none).
    pub(crate) fn binds_made(&self) -> usize {
        self.binds.made()
    }

    /// The 1b uniform at `index`, made the first time.
    fn glow_uniform(&mut self, device: &wgpu::Device, index: usize) -> wgpu::Buffer {
        while self.glow_uniforms.len() <= index {
            self.glow_uniforms.push(uniform_buffer(device, "lighting 1b uniform", std::mem::size_of::<GlowUniform>()));
        }
        self.glow_uniforms[index].clone()
    }
}
