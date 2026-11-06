//! The lens pre-pass (lighting part 5): what an effect with a `// lens:` header line reads. The
//! chain draws the effect's `bright` at half size into level 0 of [`LensPass`]'s soft picture;
//! [`LensPass::run`] then softens it over [`SOFT_LEVELS`] levels (`lens_soft` in the prelude) and
//! draws streaks out of it (`lens_rays`): for each of the call's points, a one-sided exponential
//! streak by Kawase's streak filter, all added into one picture. Nothing is kept between frames.

use bytemuck::{Pod, Zeroable};

use crate::HDR_FORMAT;

/// How many levels the soft picture has (level 0 the bright parts at half size).
pub const SOFT_LEVELS: u32 = 5;
/// The most streak points drawn for one call (the setting keeps what is typed).
pub const MOST_LENS_RAYS: usize = 32;
/// The most passes one streak takes: 4^8 texels, longer than any picture.
const MOST_RAY_PASSES: u32 = 8;

/// Matches `LensUniform` in lens.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
struct LensUniform {
    way: [f32; 2],
    step: f32,
    weight: f32,
    scale: f32,
    _pad: [f32; 3],
}

/// The streaks a call asks for: how many points, turned how far (degrees), and how long (of the
/// picture's height).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RaySettings {
    pub count: f32,
    pub turn: f32,
    pub length: f32,
}

impl RaySettings {
    /// The points drawn: the count rounded, at least 0 and at most [`MOST_LENS_RAYS`].
    pub fn points(&self) -> usize {
        if self.count.is_finite() { self.count.round().clamp(0.0, MOST_LENS_RAYS as f32) as usize } else { 0 }
    }
}

/// A streak `length` texels long: the weight a tap one texel further along takes, and how many
/// passes reach about four lengths (where it has faded under 2%). A length under a texel is one
/// pass with no tail.
pub fn ray_passes(length: f32) -> (f32, u32) {
    if !(length > 1e-3) || !length.is_finite() {
        return (0.0, 1);
    }
    let a = (-1.0 / length).exp();
    let reach = (4.0 * length).max(1.0);
    let n = (reach.ln() / 4f32.ln()).ceil().clamp(1.0, MOST_RAY_PASSES as f32) as u32;
    (a, n)
}

struct Pictures {
    size: (u32, u32),
    _soft: wgpu::Texture,
    /// One view per level, to draw into and to read the level below from.
    soft_levels: Vec<wgpu::TextureView>,
    /// Every level, for `lens_soft`.
    soft_all: wgpu::TextureView,
    rays: (wgpu::Texture, wgpu::TextureView),
    ping: [(wgpu::Texture, wgpu::TextureView); 2],
}

pub(crate) struct LensPass {
    layout: wgpu::BindGroupLayout,
    soft_down: wgpu::RenderPipeline,
    ray: wgpu::RenderPipeline,
    /// The last pass of each streak: added onto the streaks picture.
    ray_add: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    uniforms: Vec<wgpu::Buffer>,
    pictures: Option<Pictures>,
}

impl LensPass {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lens layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("lens pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("lens"),
            source: wgpu::ShaderSource::Wgsl(include_str!("lens.wgsl").into()),
        });
        let pipeline = |entry: &str, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_full"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: HDR_FORMAT,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let add = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("lens sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        Self {
            soft_down: pipeline("fs_soft_down", None),
            ray: pipeline("fs_ray", None),
            ray_add: pipeline("fs_ray", Some(add)),
            layout,
            sampler,
            uniforms: Vec::new(),
            pictures: None,
        }
    }

    /// Makes the pictures for a `width` × `height` picture (half that size), if they aren't.
    pub(crate) fn ensure(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        let size = ((width / 2).max(1), (height / 2).max(1));
        if self.pictures.as_ref().is_some_and(|p| p.size == size) {
            return;
        }
        let levels = SOFT_LEVELS.min(32 - size.0.max(size.1).leading_zeros()).max(1);
        let soft = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("lens soft"),
            size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let soft_levels = (0..levels)
            .map(|level| {
                soft.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let soft_all = soft.create_view(&Default::default());
        let make = |label| super::chain::make_texture(device, size.0, size.1, HDR_FORMAT, label);
        self.pictures = Some(Pictures {
            size,
            _soft: soft,
            soft_levels,
            soft_all,
            rays: make("lens rays"),
            ping: [make("lens ping"), make("lens pong")],
        });
    }

    /// Where a call's `bright` is drawn (level 0 of the soft picture). Made by [`Self::ensure`].
    pub(crate) fn bright_target(&self) -> &wgpu::TextureView {
        &self.pictures.as_ref().expect("ensured").soft_levels[0]
    }

    /// The streaks and the soft picture, as the effect reads them.
    pub(crate) fn views(&self) -> (&wgpu::TextureView, &wgpu::TextureView) {
        let p = self.pictures.as_ref().expect("ensured");
        (&p.rays.1, &p.soft_all)
    }

    fn uniform(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, used: &mut usize, u: LensUniform) -> usize {
        if self.uniforms.len() <= *used {
            self.uniforms.push(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("lens uniform"),
                size: std::mem::size_of::<LensUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        queue.write_buffer(&self.uniforms[*used], 0, bytemuck::bytes_of(&u));
        *used += 1;
        *used - 1
    }

    /// After the call's `bright` is drawn: its soft levels (when `soft`) and its streaks (when
    /// `rays` asks for any points; the streaks picture is cleared either way). Each call's passes
    /// use their own uniforms, so several lens effects in one frame don't share them.
    pub(crate) fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        used: &mut usize,
        rays: Option<RaySettings>,
        soft: bool,
    ) {
        let Some(p) = self.pictures.as_ref() else { return };
        let (w, h) = p.size;
        let levels = p.soft_levels.len();
        // Passes are planned first (their uniforms written), then drawn.
        let mut plan: Vec<(usize, usize, bool)> = Vec::new();
        let mut softs = Vec::new();
        if soft {
            for level in 1..levels {
                let u = self.uniform(device, queue, used, LensUniform::default());
                softs.push((level, u));
            }
        }
        let points = rays.map_or(0, |r| r.points());
        if let Some(r) = rays.filter(|_| points > 0) {
            let length = r.length.max(0.0) * h as f32;
            let (a, n) = ray_passes(if length.is_finite() { length } else { 0.0 });
            let scale = 1.0 - a;
            for i in 0..points {
                let angle = (r.turn + i as f32 * 360.0 / points as f32).to_radians();
                let way = [angle.cos() / w as f32, angle.sin() / h as f32];
                for pass in 0..n {
                    let step = 4f32.powi(pass as i32);
                    let last = pass + 1 == n;
                    let u = LensUniform {
                        way,
                        step,
                        weight: a.powf(step),
                        scale: if last { scale } else { 1.0 },
                        _pad: [0.0; 3],
                    };
                    let u = self.uniform(device, queue, used, u);
                    plan.push((u, pass as usize, last));
                }
            }
        }
        let p = self.pictures.as_ref().expect("checked");
        let group = |u: usize, src: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("lens"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.uniforms[u].as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(src) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            })
        };
        let draw = |encoder: &mut wgpu::CommandEncoder,
                    target: &wgpu::TextureView,
                    load: wgpu::LoadOp<wgpu::Color>,
                    pipeline: &wgpu::RenderPipeline,
                    bind: &wgpu::BindGroup| {
            let mut pass = crate::begin_pass(encoder, target, "lens", load);
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.draw(0..3, 0..1);
        };
        for (level, u) in softs {
            let g = group(u, &p.soft_levels[level - 1]);
            draw(encoder, &p.soft_levels[level], wgpu::LoadOp::Clear(wgpu::Color::BLACK), &self.soft_down, &g);
        }
        drop(crate::begin_pass(encoder, &p.rays.1, "lens rays clear", wgpu::LoadOp::Clear(wgpu::Color::BLACK)));
        // Each streak: from the bright parts, back and forth between the two scratch pictures, its
        // last pass added onto the streaks.
        let mut from_ping: Option<usize> = None;
        for (u, pass, last) in plan {
            let src = match (pass, from_ping) {
                (0, _) | (_, None) => &p.soft_levels[0],
                (_, Some(i)) => &p.ping[i].1,
            };
            let g = group(u, src);
            if last {
                draw(encoder, &p.rays.1, wgpu::LoadOp::Load, &self.ray_add, &g);
                from_ping = None;
            } else {
                let to = match from_ping {
                    Some(i) if pass > 0 => 1 - i,
                    _ => 0,
                };
                draw(encoder, &p.ping[to].1, wgpu::LoadOp::Clear(wgpu::Color::BLACK), &self.ray, &g);
                from_ping = Some(to);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_streak_reaches_four_lengths_in_whole_passes() {
        assert_eq!(ray_passes(0.0), (0.0, 1));
        assert_eq!(ray_passes(f32::NAN), (0.0, 1));
        let (a, n) = ray_passes(100.0);
        assert!((a - (-0.01f32).exp()).abs() < 1e-6);
        // 4^n >= 400: n = 5 (1024).
        assert_eq!(n, 5);
        assert_eq!(ray_passes(1e9).1, MOST_RAY_PASSES);
        assert_eq!(ray_passes(0.2).1, 1);
    }

    #[test]
    fn points_are_whole_and_capped() {
        let r = |count| RaySettings { count, turn: 0.0, length: 0.1 };
        assert_eq!(r(2.4).points(), 2);
        assert_eq!(r(-3.0).points(), 0);
        assert_eq!(r(1e9).points(), MOST_LENS_RAYS);
        assert_eq!(r(f32::INFINITY).points(), 0);
    }
}
