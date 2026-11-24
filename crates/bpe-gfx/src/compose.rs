//! Putting a video together (`docs/superpowers/specs/2026-11-24-video-arrangement-design.md`,
//! "Pictures and transitions"): one pass that draws a clip's finished picture cropped to the
//! video (cover fit), or two clips' pictures mixed by a transition. The maths is
//! [`reference`] here and `shaders/compose.wgsl` on the card, line for line.

use bytemuck::{Pod, Zeroable};

use crate::OUTPUT_FORMAT;

/// A transition's look (bpe-cli maps the video's `TransitionKind` to it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Look {
    Crossfade,
    /// Through this colour (linear 0 to 1 values of the output's bytes).
    FadeThrough([f32; 3]),
    Flash([f32; 3]),
    /// `direction`: 0 up, 1 down, 2 left, 3 right. `softness`: 0 to 0.3 of the screen.
    Wipe {
        direction: u32,
        softness: f32,
    },
    Push {
        direction: u32,
    },
    ZoomThrough,
}

/// The pass's uniform.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ComposeParams {
    /// 0 one picture (the new one), 1 crossfade, 2 fade through, 3 flash, 4 wipe, 5 push, 6 zoom.
    pub look: u32,
    pub direction: u32,
    pub progress: f32,
    pub softness: f32,
    pub colour: [f32; 4],
    /// x, y, width, height of each picture shown (0 to 1 of it).
    pub old_crop: [f32; 4],
    pub new_crop: [f32; 4],
    /// The output's width and height (set by [`Compose::draw`]).
    pub size: [f32; 4],
}

/// The uniform for `look` (`None`: the new picture alone) at `progress` (0 to 1).
pub fn params(look: Option<Look>, progress: f32, old_crop: [f32; 4], new_crop: [f32; 4]) -> ComposeParams {
    let mut p = ComposeParams { progress: progress.clamp(0.0, 1.0), old_crop, new_crop, ..Default::default() };
    let rgb = |c: [f32; 3]| [c[0], c[1], c[2], 1.0];
    match look {
        None => p.look = 0,
        Some(Look::Crossfade) => p.look = 1,
        Some(Look::FadeThrough(c)) => (p.look, p.colour) = (2, rgb(c)),
        Some(Look::Flash(c)) => (p.look, p.colour) = (3, rgb(c)),
        Some(Look::Wipe { direction, softness }) => {
            (p.look, p.direction, p.softness) = (4, direction.min(3), softness.clamp(0.0, 0.3))
        }
        Some(Look::Push { direction }) => (p.look, p.direction) = (5, direction.min(3)),
        Some(Look::ZoomThrough) => p.look = 6,
    }
    p
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

fn scale(a: [f32; 4], k: f32) -> [f32; 4] {
    a.map(|v| v * k)
}

fn add(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    std::array::from_fn(|i| a[i] + b[i])
}

const BLACK: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// The side a wipe or push comes from, as a step across the screen (y down): up comes from the
/// bottom, down from the top, left from the right, right from the left.
fn side(direction: u32) -> [f32; 2] {
    match direction {
        0 => [0.0, 1.0],
        1 => [0.0, -1.0],
        2 => [1.0, 0.0],
        _ => [-1.0, 0.0],
    }
}

/// The colour at screen point `uv` (0 to 1, y down), from `old` and `new` (each sampled at a
/// point in its own picture, 0 to 1): the CPU twin of `compose.wgsl`.
pub fn reference(
    params: &ComposeParams,
    uv: [f32; 2],
    old: impl Fn([f32; 2]) -> [f32; 4],
    new: impl Fn([f32; 2]) -> [f32; 4],
) -> [f32; 4] {
    // A point in a picture's shown part; black off it.
    let at = |f: &dyn Fn([f32; 2]) -> [f32; 4], crop: [f32; 4], q: [f32; 2]| {
        if q[0] < 0.0 || q[0] > 1.0 || q[1] < 0.0 || q[1] > 1.0 {
            return BLACK;
        }
        f([crop[0] + q[0] * crop[2], crop[1] + q[1] * crop[3]])
    };
    let o = |q: [f32; 2]| at(&old, params.old_crop, q);
    let n = |q: [f32; 2]| at(&new, params.new_crop, q);
    let p = params.progress;
    let e = smoothstep(0.0, 1.0, p);
    let c = params.colour;
    let out = match params.look {
        1 => mix(o(uv), n(uv), e),
        2 => {
            if e < 0.5 {
                mix(o(uv), c, 2.0 * e)
            } else {
                mix(c, n(uv), 2.0 * e - 1.0)
            }
        }
        3 => {
            // A quick rise to the middle, a slower fall.
            let f = if p < 0.5 { smoothstep(0.2, 0.5, p) } else { 1.0 - smoothstep(0.5, 1.0, p) };
            add(mix(o(uv), n(uv), e), scale(c, f))
        }
        4 => {
            let s = side(params.direction);
            // How far the point is from the side the new picture comes in on.
            let along = uv[0] * s[0].abs() + uv[1] * s[1].abs();
            let d = if s[0] + s[1] > 0.0 { 1.0 - along } else { along };
            let soft = params.softness;
            let w = if soft <= 0.0 {
                if d < p { 1.0 } else { 0.0 }
            } else {
                ((p * (1.0 + soft) - d) / soft).clamp(0.0, 1.0)
            };
            mix(o(uv), n(uv), w)
        }
        5 => {
            let s = side(params.direction);
            let nq = [uv[0] - s[0] * (1.0 - e), uv[1] - s[1] * (1.0 - e)];
            let oq = [uv[0] + s[0] * e, uv[1] + s[1] * e];
            let inside = |q: [f32; 2]| (0.0..=1.0).contains(&q[0]) && (0.0..=1.0).contains(&q[1]);
            if inside(nq) { n(nq) } else { o(oq) }
        }
        6 => {
            let k = 1.0 + 2.0 * e;
            let m = 0.6 + 0.4 * e;
            let oq = [0.5 + (uv[0] - 0.5) / k, 0.5 + (uv[1] - 0.5) / k];
            let nq = [0.5 + (uv[0] - 0.5) / m, 0.5 + (uv[1] - 0.5) / m];
            add(scale(o(oq), 1.0 - e), scale(n(nq), e))
        }
        _ => n(uv),
    };
    [out[0].clamp(0.0, 1.0), out[1].clamp(0.0, 1.0), out[2].clamp(0.0, 1.0), 1.0]
}

/// The overlay pass's uniform (the On top lane): a frame in output pixels, the part of the picture
/// it shows, and its corner, border and alpha.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct OverlayParams {
    /// x, y, width, height in output pixels.
    pub rect: [f32; 4],
    /// x, y, width, height of the picture shown (0 to 1 of it).
    pub crop: [f32; 4],
    /// Linear 0 to 1 values of the output's bytes.
    pub border_colour: [f32; 4],
    /// Corner radius (px), border width (px), alpha (0 to 1), unused.
    pub shape: [f32; 4],
}

/// The overlay uniform for a frame `rect` (output pixels) showing `crop` of its picture.
pub fn overlay_params(
    rect: [f32; 4],
    crop: [f32; 4],
    corner: f32,
    border: f32,
    border_colour: [f32; 3],
    alpha: f32,
) -> OverlayParams {
    let [r, g, b] = border_colour;
    OverlayParams {
        rect,
        crop,
        border_colour: [r, g, b, 1.0],
        shape: [corner.max(0.0), border.max(0.0), alpha.clamp(0.0, 1.0), 0.0],
    }
}

/// The colour at output pixel centre `pos` (pixels, y down) after the overlay `params` is drawn
/// over `under`, with its picture sampled by `picture` (a point in it, 0 to 1): the CPU twin of
/// `compose_overlay.wgsl`.
pub fn overlay_reference(
    params: &OverlayParams,
    pos: [f32; 2],
    under: [f32; 4],
    picture: impl Fn([f32; 2]) -> [f32; 4],
) -> [f32; 4] {
    let [x, y, w, h] = params.rect;
    let half = [w * 0.5, h * 0.5];
    let centre = [x + half[0], y + half[1]];
    let r = params.shape[0].clamp(0.0, half[0].min(half[1]));
    let q = [(pos[0] - centre[0]).abs() - half[0] + r, (pos[1] - centre[1]).abs() - half[1] + r];
    let outside = (q[0].max(0.0).powi(2) + q[1].max(0.0).powi(2)).sqrt();
    let d = outside + q[0].max(q[1]).min(0.0) - r;
    let cover = (0.5 - d).clamp(0.0, 1.0) * params.shape[2];
    if cover <= 0.0 {
        return under;
    }
    let uv = [((pos[0] - x) / w).clamp(0.0, 1.0), ((pos[1] - y) / h).clamp(0.0, 1.0)];
    let c = params.crop;
    let mut colour = picture([c[0] + uv[0] * c[2], c[1] + uv[1] * c[3]]);
    let border = params.shape[1];
    if border > 0.0 {
        let b = (border + d + 0.5).clamp(0.0, 1.0);
        colour = mix(colour, params.border_colour, b);
    }
    let out = mix(under, colour.map(|v| v.clamp(0.0, 1.0)), cover);
    [out[0], out[1], out[2], under[3]]
}

/// The overlay pass (the On top lane): its pipeline, layout and uniform.
struct OverlayPass {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniform: wgpu::Buffer,
}

impl OverlayPass {
    fn new(device: &wgpu::Device) -> OverlayPass {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("compose overlay"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/compose_overlay.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("compose overlay"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("compose overlay"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        // Straight alpha over what is there; the output keeps its own alpha.
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("compose overlay"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_full"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_overlay"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: OUTPUT_FORMAT,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("compose overlay"),
            size: std::mem::size_of::<OverlayParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        OverlayPass { pipeline, layout, uniform }
    }
}

/// The compose pass and the picture it draws into (the video's size, the output format).
pub struct Compose {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    /// A black picture for a missing side.
    black: wgpu::TextureView,
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    /// Made the first time an On top clip is drawn.
    overlay: Option<OverlayPass>,
}

impl Compose {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, width: u32, height: u32) -> Compose {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("compose"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/compose.wgsl").into()),
        });
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("compose"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("compose"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("compose"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_full"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_compose"),
                compilation_options: Default::default(),
                targets: &[Some(OUTPUT_FORMAT.into())],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("compose"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("compose"),
            size: std::mem::size_of::<ComposeParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let texture = |label, w, h, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: OUTPUT_FORMAT,
                usage,
                view_formats: &[],
            })
        };
        let black =
            texture("compose black", 1, 1, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
        queue.write_texture(
            black.as_image_copy(),
            &[0, 0, 0, 255],
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4), rows_per_image: Some(1) },
            wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
        );
        let output = texture(
            "compose output",
            width,
            height,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
        );
        Compose {
            pipeline,
            layout,
            sampler,
            uniform,
            black: black.create_view(&Default::default()),
            output_view: output.create_view(&Default::default()),
            output,
            overlay: None,
        }
    }

    /// Draws `picture` (a finished picture) over the output inside the frame `params` describes.
    pub fn overlay(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        picture: &wgpu::TextureView,
        params: &OverlayParams,
    ) {
        let pass = self.overlay.get_or_insert_with(|| OverlayPass::new(device));
        queue.write_buffer(&pass.uniform, 0, bytemuck::bytes_of(params));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("compose overlay"),
            layout: &pass.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(picture) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: pass.uniform.as_entire_binding() },
            ],
        });
        let mut encoder =
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("compose overlay") });
        {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("compose overlay"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.output_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rp.set_pipeline(&pass.pipeline);
            rp.set_bind_group(0, &group, &[]);
            rp.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
    }

    /// Draws `old` and `new` (finished pictures, `None` for black) put together by `params` into
    /// the output.
    pub fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        old: Option<&wgpu::TextureView>,
        new: Option<&wgpu::TextureView>,
        params: &ComposeParams,
    ) {
        let size = [self.output.width() as f32, self.output.height() as f32, 0.0, 0.0];
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&ComposeParams { size, ..*params }));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("compose"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(old.unwrap_or(&self.black)),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(new.unwrap_or(&self.black)),
                },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: self.uniform.as_entire_binding() },
            ],
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("compose") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("compose"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.output_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        queue.submit([encoder.finish()]);
    }

    pub fn output_texture(&self) -> &wgpu::Texture {
        &self.output
    }

    pub fn output_view(&self) -> &wgpu::TextureView {
        &self.output_view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
    const WHOLE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

    fn at(look: Look, p: f32, uv: [f32; 2]) -> [f32; 4] {
        reference(&params(Some(look), p, WHOLE, WHOLE), uv, |_| RED, |_| BLUE)
    }

    fn near(a: [f32; 4], b: [f32; 4]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5)
    }

    const MID: [f32; 2] = [0.5, 0.5];

    #[test]
    fn crossfade_is_old_then_half_then_new() {
        assert!(near(at(Look::Crossfade, 0.0, MID), RED));
        assert!(near(at(Look::Crossfade, 0.5, MID), [0.5, 0.0, 0.5, 1.0]));
        assert!(near(at(Look::Crossfade, 1.0, MID), BLUE));
    }

    #[test]
    fn fade_through_is_old_then_the_colour_then_new() {
        let look = Look::FadeThrough([0.0; 3]);
        assert!(near(at(look, 0.0, MID), RED));
        assert!(near(at(look, 0.5, MID), [0.0, 0.0, 0.0, 1.0]));
        assert!(near(at(look, 1.0, MID), BLUE));
    }

    #[test]
    fn flash_is_old_then_white_then_new() {
        let look = Look::Flash([1.0; 3]);
        assert!(near(at(look, 0.0, MID), RED));
        let mid = at(look, 0.5, MID);
        assert!(mid[..3].iter().all(|&c| c >= 0.9), "{mid:?}");
        assert!(near(at(look, 1.0, MID), BLUE));
        // It rises faster than it falls: just after the middle it's brighter than just before.
        assert!(at(look, 0.4, MID)[1] < at(look, 0.6, MID)[1]);
    }

    #[test]
    fn wipe_left_moves_right_to_left() {
        let look = Look::Wipe { direction: 2, softness: 0.05 };
        assert!(near(at(look, 0.0, [0.99, 0.5]), RED));
        assert!(near(at(look, 0.5, [0.125, 0.5]), RED), "the left quarter is still old");
        assert!(near(at(look, 0.5, [0.875, 0.5]), BLUE), "the right quarter is new");
        assert!(near(at(look, 1.0, [0.01, 0.5]), BLUE));
        let up = Look::Wipe { direction: 0, softness: 0.0 };
        assert!(near(at(up, 0.5, [0.5, 0.9]), BLUE), "up comes in from the bottom");
        assert!(near(at(up, 0.5, [0.5, 0.1]), RED));
    }

    #[test]
    fn push_slides_the_new_picture_in() {
        let look = Look::Push { direction: 2 };
        assert!(near(at(look, 0.0, MID), RED));
        assert!(near(at(look, 0.5, [0.25, 0.5]), RED));
        assert!(near(at(look, 0.5, [0.75, 0.5]), BLUE));
        assert!(near(at(look, 1.0, MID), BLUE));
        // The old picture moves left with it: at ½ the screen's left shows the old picture's right half.
        let p = params(Some(look), 0.5, WHOLE, WHOLE);
        let x = reference(&p, [0.25, 0.5], |q| [q[0], 0.0, 0.0, 1.0], |_| BLUE)[0];
        assert!((x - 0.75).abs() < 1e-5, "{x}");
    }

    #[test]
    fn zoom_through_is_old_then_new() {
        assert!(near(at(Look::ZoomThrough, 0.0, MID), RED));
        assert!(near(at(Look::ZoomThrough, 1.0, MID), BLUE));
        // The new picture starts small: early on, its edge is black around it.
        let early = at(Look::ZoomThrough, 0.3, [0.02, 0.5]);
        assert!(early[2] == 0.0, "{early:?}");
    }

    #[test]
    fn one_picture_is_cropped() {
        let p = params(None, 0.0, WHOLE, [0.25, 0.0, 0.5, 1.0]);
        let x = |uv| reference(&p, uv, |_| RED, |q| [q[0], q[1], 0.0, 1.0]);
        assert!(near(x([0.0, 0.0]), [0.25, 0.0, 0.0, 1.0]));
        assert!(near(x([1.0, 1.0]), [0.75, 1.0, 0.0, 1.0]));
    }

    #[test]
    fn the_uniform_is_a_whole_number_of_vec4s() {
        assert_eq!(std::mem::size_of::<ComposeParams>() % 16, 0);
        assert_eq!(std::mem::size_of::<OverlayParams>() % 16, 0);
    }

    /// A 100 × 200 frame at (20, 40), corner 10, border 4, white.
    fn framed(alpha: f32) -> OverlayParams {
        overlay_params([20.0, 40.0, 100.0, 200.0], WHOLE, 10.0, 4.0, [1.0; 3], alpha)
    }

    fn over(p: &OverlayParams, pos: [f32; 2]) -> [f32; 4] {
        overlay_reference(p, pos, RED, |_| BLUE)
    }

    #[test]
    fn an_overlay_draws_its_picture_inside_its_frame() {
        let p = framed(1.0);
        assert!(near(over(&p, [70.5, 140.5]), BLUE), "the middle is the picture");
        assert!(near(over(&p, [10.5, 140.5]), RED), "left of the frame is what was there");
        assert!(near(over(&p, [70.5, 245.5]), RED), "below it too");
        assert!(near(over(&p, [21.5, 140.5]), [1.0; 4]), "the border is its colour");
        assert!(near(over(&p, [20.5, 40.5]), RED), "the rounded corner leaves the corner pixel");
        assert!(near(over(&p, [23.5, 140.5]), [1.0; 4]), "the border is 4 px wide");
        assert!(near(over(&p, [24.5, 140.5]), BLUE), "and the picture is inside it");
        let half = over(&framed(0.5), [70.5, 140.5]);
        assert!(near(half, [0.5, 0.0, 0.5, 1.0]), "alpha ½ is half way: {half:?}");
        assert!(near(over(&framed(0.0), [70.5, 140.5]), RED), "alpha 0 draws nothing");
    }

    #[test]
    fn an_overlay_shows_its_crop() {
        let p = overlay_params([0.0, 0.0, 10.0, 10.0], [0.25, 0.0, 0.5, 1.0], 0.0, 0.0, [0.0; 3], 1.0);
        let x = |pos| overlay_reference(&p, pos, RED, |q| [q[0], q[1], 0.0, 1.0]);
        assert!(near(x([0.5, 0.5]), [0.275, 0.05, 0.0, 1.0]));
        assert!(near(x([9.5, 9.5]), [0.725, 0.95, 0.0, 1.0]));
        assert!(near(x([5.0, 5.0]), [0.5, 0.5, 0.0, 1.0]));
    }
}
