//! The canvas composite: draws a canvas's rectangle of the world into the picture with the Flat
//! style, sampling its two textures (see canvas.wgsl).

use bpe_core::Blend;
use bytemuck::{Pod, Zeroable};

use super::CanvasTextures;
use crate::flow::util;
use crate::{CanvasLayer, HDR_FORMAT};

/// One canvas's uniform, laid out as `Canvas` in canvas.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct CanvasUniform {
    pose: [f32; 4],
    size: [f32; 4],
    start: [f32; 4],
    clock: [f32; 4],
}

impl CanvasUniform {
    fn new(c: &CanvasLayer) -> Self {
        Self {
            pose: [c.center[0], c.center[1], c.angle, 0.0],
            size: [c.size[0], c.size[1], c.fade, c.opacity],
            start: c.start.unwrap_or([0.0; 4]),
            clock: [c.time, 0.0, 0.0, 0.0],
        }
    }
}

pub(crate) struct Composite {
    layout: wgpu::BindGroupLayout,
    /// Over what is there, by the canvas's coverage: Normal, and every blend drawn into the blend
    /// layer picture.
    over: wgpu::RenderPipeline,
    /// Light added on top: Add.
    add: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    /// Cleared 1×1 textures for a canvas whose own are not made yet: only its start colour shows.
    blank: CanvasTextures,
    /// One uniform per canvas in a frame: every canvas is recorded before the frame is submitted,
    /// so each needs its own.
    uniforms: Vec<wgpu::Buffer>,
    /// Each slot's bind group, kept while the slot draws the same paint texture.
    groups: Vec<Option<(wgpu::Texture, wgpu::BindGroup)>>,
}

impl Composite {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        use wgpu::ShaderStages as S;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("canvas layout"),
            entries: &[
                util::uniform_entry(0, S::VERTEX_FRAGMENT, false),
                util::float_texture_entry(1, S::FRAGMENT),
                util::float_texture_entry(2, S::FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: S::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let globals = util::globals_layout(device);
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("canvas"),
            bind_group_layouts: &[Some(&globals), Some(&layout)],
            immediate_size: 0,
        });
        let module = util::shader(device, "canvas", &[include_str!("../shaders/canvas.wgsl")]);
        let make = |label, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_canvas"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_canvas"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: HDR_FORMAT,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let over = make("canvas over", wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        let add = make("canvas add", util::ADDITIVE);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("canvas"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let blank = CanvasTextures::new(device, [1, 1]);
        Self { layout, over, add, sampler, blank, uniforms: Vec::new(), groups: Vec::new() }
    }

    /// Whether `blend` is drawn straight into the picture; any other goes through the blend
    /// layer picture and the blend composite.
    pub(crate) fn direct(blend: Blend) -> bool {
        matches!(blend, Blend::Normal | Blend::Add)
    }

    /// Keeps the cached bind groups of this frame's canvases only, `current[slot]` being the
    /// textures slot `slot` draws (none: the blank ones): a slot past the end, or one whose
    /// textures changed, lets its old textures go.
    pub(crate) fn keep(&mut self, current: &[Option<&CanvasTextures>]) {
        self.groups.truncate(current.len());
        for (cached, textures) in self.groups.iter_mut().zip(current) {
            let paint = &textures.unwrap_or(&self.blank).paint;
            if cached.as_ref().is_some_and(|(held, _)| held != paint) {
                *cached = None;
            }
        }
    }

    /// Bind groups cached, for tests.
    #[cfg(test)]
    fn cached(&self) -> usize {
        self.groups.iter().flatten().count()
    }

    /// Records `canvas` into `pass`, seen through `globals`, sampling `textures` (none: only its
    /// start colour). Add adds its light; every other blend paints over what is there by its
    /// coverage. `slot` tells the canvases of one frame apart.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn draw(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'_>,
        globals: &wgpu::BindGroup,
        slot: usize,
        canvas: &CanvasLayer,
        textures: Option<&CanvasTextures>,
    ) {
        while self.uniforms.len() <= slot {
            self.uniforms.push(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("canvas"),
                size: std::mem::size_of::<CanvasUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        let uniform = &self.uniforms[slot];
        queue.write_buffer(uniform, 0, bytemuck::bytes_of(&CanvasUniform::new(canvas)));
        let textures = textures.unwrap_or(&self.blank);
        self.groups.resize_with(self.groups.len().max(slot + 1), || None);
        let cached = &mut self.groups[slot];
        if cached.as_ref().is_none_or(|(paint, _)| *paint != textures.paint) {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("canvas"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&textures.paint_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&textures.aux_view),
                    },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                ],
            });
            *cached = Some((textures.paint.clone(), group));
        }
        let (_, group) = cached.as_ref().expect("made above");
        pass.set_pipeline(if canvas.blend == Blend::Add { &self.add } else { &self.over });
        pass.set_bind_group(0, globals, &[]);
        pass.set_bind_group(1, group, &[]);
        pass.draw(0..4, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use bpe_core::ThingId;

    use super::*;
    use crate::{BallInstance, CanvasPlace, Frame, PaintGpu, Renderer, headless_device};

    const W: u32 = 270;
    const H: u32 = 480;

    fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
        match headless_device() {
            Ok(pair) => Some(pair),
            Err(_) => {
                eprintln!("no GPU adapter available, skipping");
                None
            }
        }
    }

    fn canvas(place: CanvasPlace, start: Option<[f32; 4]>) -> CanvasLayer {
        CanvasLayer {
            thing: ThingId(7),
            center: [0.0, 0.0],
            angle: 0.0,
            size: [200.0, 200.0],
            texels: [200, 200],
            place,
            start,
            fade: 0.0,
            blend: Blend::Normal,
            opacity: 1.0,
            time: 0.0,
        }
    }

    /// The output pixel at world point (x, y) of a 1080×1920 frame drawn at a quarter size.
    fn pixel(pixels: &[u8], x: f32, y: f32) -> [u8; 4] {
        let (px, py) = (((x + 540.0) / 4.0) as usize, ((y + 960.0) / 4.0) as usize);
        let i = (py * W as usize + px) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2], pixels[i + 3]]
    }

    fn render(device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame) -> Vec<u8> {
        let mut renderer = Renderer::new(device, W, H);
        renderer.render(device, queue, frame);
        renderer.read_output(device, queue).unwrap()
    }

    #[test]
    fn start_colour_fills_the_canvas_rect() {
        let Some((device, queue)) = gpu() else { return };
        let mut frame = Frame::default();
        let background = pixel(&render(&device, &queue, &frame), 150.0, 0.0);
        frame.canvases.push(canvas(CanvasPlace::BehindBalls, Some([1.0, 0.0, 0.0, 1.0])));
        let pixels = render(&device, &queue, &frame);
        let centre = pixel(&pixels, 0.0, 0.0);
        assert!(centre[0] > 200 && centre[1] < 40 && centre[2] < 40, "{centre:?}: the centre is red");
        assert_eq!(pixel(&pixels, 150.0, 0.0), background, "outside the rectangle is the background");
    }

    #[test]
    fn behind_balls_is_under_a_ball() {
        let Some((device, queue)) = gpu() else { return };
        let mut frame = Frame::default();
        frame.canvases.push(canvas(CanvasPlace::BehindBalls, Some([1.0, 0.0, 0.0, 1.0])));
        frame.balls.push(BallInstance {
            center: [0.0, 0.0],
            radius: 40.0,
            glow: 0.0,
            color: [0.0, 0.0, 1.0, 1.0],
            code: [0.0; 4],
            paint: PaintGpu::default(),
        });
        let pixels = render(&device, &queue, &frame);
        let ball = pixel(&pixels, 0.0, 0.0);
        assert!(ball[2] > 200 && ball[2] > ball[0] + 100, "{ball:?}: the ball keeps its blue over the canvas");
        let canvas = pixel(&pixels, 80.0, 0.0);
        assert!(canvas[0] > 200 && canvas[2] < 40, "{canvas:?}: the canvas is red beside the ball");
    }

    #[test]
    fn on_top_covers_a_ball() {
        let Some((device, queue)) = gpu() else { return };
        let mut frame = Frame::default();
        frame.canvases.push(canvas(CanvasPlace::OnTop, Some([1.0, 0.0, 0.0, 1.0])));
        frame.balls.push(BallInstance {
            center: [0.0, 0.0],
            radius: 40.0,
            glow: 0.0,
            color: [0.0, 0.0, 1.0, 1.0],
            code: [0.0; 4],
            paint: PaintGpu::default(),
        });
        let ball = pixel(&render(&device, &queue, &frame), 0.0, 0.0);
        assert!(ball[0] > 200 && ball[2] < 40, "{ball:?}: the canvas is red over the ball");
    }

    #[test]
    fn a_multiply_canvas_goes_through_the_blend_layer() {
        let Some((device, queue)) = gpu() else { return };
        let mut frame = Frame::default();
        frame.background.row = 0;
        frame.background.ends = Some([[0.0, 1.0, 1.0, 1.0], [0.0, 1.0, 1.0, 1.0]]);
        let mut c = canvas(CanvasPlace::BehindEverything, Some([1.0, 1.0, 0.0, 1.0]));
        c.blend = Blend::Multiply;
        frame.canvases.push(c);
        let pixels = render(&device, &queue, &frame);
        let centre = pixel(&pixels, 0.0, 0.0);
        // Cyan times yellow is green.
        assert!(centre[1] > 200 && centre[0] < 40 && centre[2] < 40, "{centre:?}: the centre is green");
    }

    /// f16 bits for `x`, for the few exact values the tests write.
    fn half(x: f32) -> u16 {
        match x {
            0.0 => 0,
            0.5 => 0x3800,
            1.0 => 0x3c00,
            _ => panic!("no half for {x}"),
        }
    }

    /// Writes one row of `texels` (rgba each) into `texture`.
    fn write_row(queue: &wgpu::Queue, texture: &wgpu::Texture, texels: &[[f32; 4]]) {
        let bytes: Vec<u8> = texels.iter().flatten().flat_map(|&c| half(c).to_le_bytes()).collect();
        queue.write_texture(
            texture.as_image_copy(),
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(texels.len() as u32 * 8),
                rows_per_image: None,
            },
            wgpu::Extent3d { width: texels.len() as u32, height: 1, depth_or_array_layers: 1 },
        );
    }

    /// Draws `canvas` alone with `textures` over a black picture the size of the frame and reads
    /// back the red and green of each pixel (premultiplied).
    fn draw_alone(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        canvas: &CanvasLayer,
        textures: &CanvasTextures,
    ) -> impl Fn(u32, u32) -> [f32; 2] + use<> {
        const SIDE: u32 = 64;
        let frame = Frame { width: SIDE, height: SIDE, ..Frame::default() };
        let globals_data = crate::Globals::new(&frame, &frame.camera, SIDE);
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: std::mem::size_of::<crate::Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&globals, 0, bytemuck::bytes_of(&globals_data));
        let globals_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &util::globals_layout(device),
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });
        let picture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: SIDE, height: SIDE, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = picture.create_view(&Default::default());
        let mut composite = Composite::new(device);
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = crate::begin_pass(&mut encoder, &view, "test", wgpu::LoadOp::Clear(wgpu::Color::BLACK));
            composite.draw(device, queue, &mut pass, &globals_group, 0, canvas, Some(textures));
        }
        let row = (SIDE * 8) as usize;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (row * SIDE as usize) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            picture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row as u32),
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d { width: SIDE, height: SIDE, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let bytes = buffer.slice(..).get_mapped_range().unwrap().to_vec();
        move |x, y| {
            let at = |c: usize| {
                let i = y as usize * row + x as usize * 8 + c * 2;
                f16_to_f32(u16::from_le_bytes([bytes[i], bytes[i + 1]]))
            };
            [at(0), at(1)]
        }
    }

    fn f16_to_f32(bits: u16) -> f32 {
        let exponent = ((bits >> 10) & 0x1f) as i32;
        let mantissa = (bits & 0x3ff) as f32;
        let value = if exponent == 0 {
            mantissa * 2f32.powi(-24)
        } else {
            (1.0 + mantissa / 1024.0) * 2f32.powi(exponent - 15)
        };
        if bits & 0x8000 != 0 { -value } else { value }
    }

    #[test]
    fn canvas_textures_are_sampled_and_fade() {
        let Some((device, queue)) = gpu() else { return };
        // A 32×32 canvas over a 64×64 picture of 64×64 world pixels; the left texel of its two
        // is painted red, all of it, at time 0.
        let textures = CanvasTextures::new(&device, [2, 1]);
        write_row(&queue, &textures.paint, &[[1.0, 0.0, 0.0, 1.0], [0.0; 4]]);
        write_row(&queue, &textures.aux, &[[0.0; 4], [0.0; 4]]);
        let mut c = canvas(CanvasPlace::BehindBalls, Some([0.0, 1.0, 0.0, 1.0]));
        c.size = [32.0, 32.0];
        c.texels = [2, 1];
        let read = draw_alone(&device, &queue, &c, &textures);
        let (left, right) = (read(17, 32), read(46, 32));
        assert!(left[0] > 0.9 && left[1] < 0.1, "{left:?}: the left is the red marks");
        assert!(right[0] < 0.1 && right[1] > 0.9, "{right:?}: the right is the green start");
        // Half a second into a one second fade the marks are half gone over the start.
        c.fade = 1.0;
        c.time = 0.5;
        let left = draw_alone(&device, &queue, &c, &textures)(17, 32);
        assert!((left[0] - 0.5).abs() < 0.05 && (left[1] - 0.5).abs() < 0.05, "{left:?}: half faded");
    }

    #[test]
    fn canvas_time_painted_is_read_unblended() {
        let Some((device, queue)) = gpu() else { return };
        // Both texels are red; the left was painted at 0 s and has faded, the right at 1 s.
        let textures = CanvasTextures::new(&device, [2, 1]);
        write_row(&queue, &textures.paint, &[[1.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]]);
        write_row(&queue, &textures.aux, &[[0.0; 4], [1.0, 0.0, 0.0, 0.0]]);
        let mut c = canvas(CanvasPlace::BehindBalls, None);
        c.size = [32.0, 32.0];
        c.texels = [2, 1];
        c.fade = 1.0;
        c.time = 1.0;
        let read = draw_alone(&device, &queue, &c, &textures);
        // Just right of the seam the mark keeps its full colour: its time is not mixed with the
        // older one beside it.
        let (left, right) = (read(30, 32), read(33, 32));
        assert!(left[0] < 0.01, "{left:?}: the old mark has faded");
        assert!(right[0] > 0.99, "{right:?}: the new mark is whole up to its edge");
    }

    #[test]
    fn canvas_cache_lets_go_of_gone_canvases() {
        let Some((device, queue)) = gpu() else { return };
        let frame = Frame { width: 8, height: 8, ..Frame::default() };
        let globals = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: std::mem::size_of::<crate::Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&globals, 0, bytemuck::bytes_of(&crate::Globals::new(&frame, &frame.camera, 8)));
        let globals = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &util::globals_layout(&device),
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: globals.as_entire_binding() }],
        });
        let picture = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 8, height: 8, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = picture.create_view(&Default::default());
        let (a, b) = (CanvasTextures::new(&device, [4, 4]), CanvasTextures::new(&device, [4, 4]));
        let mut composite = Composite::new(&device);
        let c = canvas(CanvasPlace::BehindBalls, None);
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = crate::begin_pass(&mut encoder, &view, "test", wgpu::LoadOp::Load);
            composite.draw(&device, &queue, &mut pass, &globals, 0, &c, Some(&a));
            composite.draw(&device, &queue, &mut pass, &globals, 1, &c, Some(&b));
        }
        queue.submit([encoder.finish()]);
        assert_eq!(composite.cached(), 2);
        composite.keep(&[Some(&a), Some(&b)]);
        assert_eq!(composite.cached(), 2, "the same canvases keep theirs");
        composite.keep(&[Some(&b)]);
        assert_eq!(composite.cached(), 0, "slot 0 draws other textures now, and slot 1 is gone");
        assert!(composite.groups.len() <= 1);
        composite.keep(&[]);
        assert!(composite.groups.is_empty(), "no canvases, nothing held");
    }
}
