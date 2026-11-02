//! The canvas composite: draws a canvas's rectangle of the world into the picture with the Flat
//! style or a paint style, sampling its three textures (see canvas.wgsl).

use std::sync::Arc;

use bpe_core::Blend;
use bpe_runtime::MarkTarget;
use bytemuck::{Pod, Zeroable};

use super::CanvasTextures;
use super::pictures::PictureAtlas;
use crate::flow::util;
use crate::{CanvasLayer, HDR_FORMAT, MAX_MASK_SHAPES, MaskShape, PaintStyle, Surface, Under};

/// One canvas's uniform, laid out as `Canvas` in canvas.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct CanvasUniform {
    pose: [f32; 4],
    size: [f32; 4],
    start: [f32; 4],
    clock: [f32; 4],
    /// Style (0 flat, 1 wet paint, 2 watercolour, 3 neon ink), dry time, light angle in radians,
    /// gloss.
    look: [f32; 4],
    /// Grain, edge darkening, glow size, glow strength.
    look2: [f32; 4],
    /// x: surface (0 paint, 1 cover, 2 powder, 3 fog); y: its look (0 matte/sand/frost, 1 foil/snow/steam);
    /// z: ridges (powder) or blur in world px (fog); w: density (fog) or the light angle in radians (powder).
    surface: [f32; 4],
    /// x: underneath (0 scene, 1 colour, 2 gradient, 3 picture, 4 text); y: picture slot; z: the part (0 all,
    /// 1 underneath only, 2 cover only); w: 1 when there is a grid image.
    under: [f32; 4],
    /// Linear light: the underneath colour.
    under_colour: [f32; 4],
    /// x, y: the grid image's size in texels; z: world px per texel; w: border.
    grid: [f32; 4],
    stops: [[f32; 4]; 16],
}

impl CanvasUniform {
    fn new(c: &CanvasLayer, part: u32) -> Self {
        let skin = matches!(c.key, MarkTarget::Skin(_));
        let surface = match c.surface {
            Surface::Paint => [0.0; 4],
            Surface::Cover { foil, .. } => [1.0, foil as u32 as f32, 0.0, 0.0],
            Surface::Powder { snow, ridges, light_angle } => {
                [2.0, snow as u32 as f32, ridges, light_angle.to_radians()]
            }
            Surface::Fog { steam, blur, density } => [3.0, steam as u32 as f32, blur, density],
        };
        let (mut under, mut under_colour, mut stops) = ([0.0; 4], [0.0; 4], [[0.0; 4]; 16]);
        if let Surface::Cover { under: u, .. } = c.surface {
            match u {
                Under::Scene => {}
                Under::Colour(col) => (under[0], under_colour) = (1.0, col),
                Under::Gradient(s) => (under[0], stops) = (2.0, s),
                Under::Picture(slot) => (under[0], under[1]) = (3.0, slot as f32),
                Under::Text(col) => (under[0], under_colour) = (4.0, col),
            }
        }
        under[2] = part as f32;
        under[3] = c.grid.is_some() as u32 as f32;
        let grid = c.grid.as_ref().map_or([0.0; 4], |g| [g.size[0] as f32, g.size[1] as f32, g.cell, c.border]);
        Self {
            surface,
            under,
            under_colour,
            grid,
            stops,
            pose: [c.center[0], c.center[1], c.angle, 0.0],
            size: [c.size[0], c.size[1], c.fade, c.opacity],
            start: c.start.unwrap_or([0.0; 4]),
            clock: [c.time, c.mask.len().min(MAX_MASK_SHAPES) as f32, if skin { 1.0 } else { 0.0 }, 0.0],
            look: [
                match c.look.style {
                    PaintStyle::Flat => 0.0,
                    PaintStyle::WetPaint => 1.0,
                    PaintStyle::Watercolour => 2.0,
                    PaintStyle::NeonInk => 3.0,
                },
                c.look.dry_time,
                c.look.light_angle.to_radians(),
                c.look.gloss,
            ],
            look2: [c.look.grain, c.look.edge_dark, c.look.glow_size, c.look.glow_strength],
        }
    }
}

/// The ten bindings of a canvas's bind group.
fn group_entries<'a>(
    uniform: &'a wgpu::Buffer,
    mask: &'a wgpu::Buffer,
    textures: &'a CanvasTextures,
    sampler: &'a wgpu::Sampler,
    [grid_view, atlas_view]: [&'a wgpu::TextureView; 2],
    [grid_sampler, atlas_sampler]: [&'a wgpu::Sampler; 2],
) -> [wgpu::BindGroupEntry<'a>; 10] {
    use wgpu::BindingResource::{Sampler, TextureView};
    [
        wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
        wgpu::BindGroupEntry { binding: 1, resource: TextureView(&textures.paint_view) },
        wgpu::BindGroupEntry { binding: 2, resource: TextureView(&textures.aux_view) },
        wgpu::BindGroupEntry { binding: 3, resource: Sampler(sampler) },
        wgpu::BindGroupEntry { binding: 4, resource: mask.as_entire_binding() },
        wgpu::BindGroupEntry { binding: 5, resource: TextureView(&textures.body_view) },
        wgpu::BindGroupEntry { binding: 6, resource: TextureView(grid_view) },
        wgpu::BindGroupEntry { binding: 7, resource: Sampler(grid_sampler) },
        wgpu::BindGroupEntry { binding: 8, resource: TextureView(atlas_view) },
        wgpu::BindGroupEntry { binding: 9, resource: Sampler(atlas_sampler) },
    ]
}

/// A canvas's grid image on the graphics card.
struct GridTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: [u32; 2],
    srgb: bool,
    /// What was last written, to skip uploading the same picture again.
    last: Arc<Vec<u8>>,
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
    /// One uniform and one mask buffer per canvas in a frame: every canvas is recorded before the
    /// frame is submitted, so each needs its own.
    uniforms: Vec<(wgpu::Buffer, wgpu::Buffer)>,
    /// Each slot's bind group, kept while the slot draws the same paint texture.
    groups: Vec<Option<(wgpu::Texture, wgpu::BindGroup)>>,
    /// Each slot's grid image, when its canvas has one.
    grids: Vec<Option<GridTexture>>,
    /// Samples a grid image smoothly (the other samples it nearest).
    grid_sampler: wgpu::Sampler,
    grid_nearest: wgpu::Sampler,
    /// Stand-ins for a canvas with no grid image and no pictures: clear 1×1 textures.
    blank_grid: wgpu::TextureView,
    blank_atlas: wgpu::TextureView,
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
                util::storage_entry(4, S::FRAGMENT, true),
                util::float_texture_entry(5, S::FRAGMENT),
                util::float_texture_entry(6, S::FRAGMENT),
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: S::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: S::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 9,
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
        let grid_sampler = |filter| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("canvas grid"),
                mag_filter: filter,
                min_filter: filter,
                ..Default::default()
            })
        };
        let (grid_smooth, grid_nearest) =
            (grid_sampler(wgpu::FilterMode::Linear), grid_sampler(wgpu::FilterMode::Nearest));
        let stand_in = |dimension, layers| {
            // A new texture is all zeros: clear, which is what a canvas with nothing there reads.
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("canvas stand-in"),
                    size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: layers },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor { dimension: Some(dimension), ..Default::default() })
        };
        Self {
            layout,
            over,
            add,
            sampler,
            blank,
            uniforms: Vec::new(),
            groups: Vec::new(),
            grids: Vec::new(),
            grid_sampler: grid_smooth,
            grid_nearest,
            blank_grid: stand_in(wgpu::TextureViewDimension::D2, 1),
            blank_atlas: stand_in(wgpu::TextureViewDimension::D2Array, 1),
        }
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
        self.grids.truncate(current.len());
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

    /// Uploads `canvas`'s grid image into slot `slot`'s texture, making the texture when it is
    /// new or a different size or kind.
    fn upload_grid(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, slot: usize, canvas: &CanvasLayer) {
        self.grids.resize_with(self.grids.len().max(slot + 1), || None);
        let Some(image) = &canvas.grid else {
            self.grids[slot] = None;
            return;
        };
        let held = &mut self.grids[slot];
        if held.as_ref().is_none_or(|g| g.size != image.size || g.srgb != image.srgb) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("canvas grid"),
                size: wgpu::Extent3d { width: image.size[0], height: image.size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: if image.srgb { wgpu::TextureFormat::Rgba8UnormSrgb } else { wgpu::TextureFormat::Rgba8Unorm },
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            *held = Some(GridTexture { texture, view, size: image.size, srgb: image.srgb, last: Arc::new(Vec::new()) });
        }
        let g = held.as_mut().expect("made above");
        let same = Arc::ptr_eq(&g.last, &image.pixels) || *g.last == *image.pixels;
        if !same {
            queue.write_texture(
                g.texture.as_image_copy(),
                &image.pixels,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(image.size[0] * 4), rows_per_image: None },
                wgpu::Extent3d { width: image.size[0], height: image.size[1], depth_or_array_layers: 1 },
            );
            g.last = image.pixels.clone();
        }
    }

    /// Records `canvas` into `pass`, seen through `globals`, sampling `textures` (none: only its
    /// start colour) and, for covers with a picture underneath, `atlas`. Add adds its light; every
    /// other blend paints over what is there by its coverage. `slot` tells the canvases of one
    /// frame apart; a cover with text underneath draws twice, parts 1 and 2, in slots `2 i` and
    /// `2 i + 1` of its own, so `part` says which part (0: all of it).
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
        atlas: Option<&PictureAtlas>,
        part: u32,
    ) {
        while self.uniforms.len() <= slot {
            let buffer = |label, size, usage| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size,
                    usage: usage | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            };
            let uniform = buffer("canvas", std::mem::size_of::<CanvasUniform>() as u64, wgpu::BufferUsages::UNIFORM);
            let mask_bytes = (MAX_MASK_SHAPES * std::mem::size_of::<MaskShape>()) as u64;
            self.uniforms.push((uniform, buffer("canvas mask", mask_bytes, wgpu::BufferUsages::STORAGE)));
        }
        let (uniform, mask) = &self.uniforms[slot];
        queue.write_buffer(uniform, 0, bytemuck::bytes_of(&CanvasUniform::new(canvas, part)));
        let shapes = &canvas.mask[..canvas.mask.len().min(MAX_MASK_SHAPES)];
        if !shapes.is_empty() {
            queue.write_buffer(mask, 0, bytemuck::cast_slice(shapes));
        }
        self.upload_grid(device, queue, slot, canvas);
        let textures = textures.unwrap_or(&self.blank);
        self.groups.resize_with(self.groups.len().max(slot + 1), || None);
        let (uniform, mask) = &self.uniforms[slot];
        let grid = self.grids[slot].as_ref();
        let picture = matches!(canvas.surface, Surface::Cover { under: Under::Picture(_), .. });
        let entries = |grid_view, grid_sampler, atlas_view, atlas_sampler| {
            group_entries(
                uniform,
                mask,
                textures,
                &self.sampler,
                [grid_view, atlas_view],
                [grid_sampler, atlas_sampler],
            )
        };
        let fresh;
        let group = if grid.is_none() && !picture {
            // The cached path: nothing but the paint textures changes the group.
            let cached = &mut self.groups[slot];
            if cached.as_ref().is_none_or(|(paint, _)| *paint != textures.paint) {
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("canvas"),
                    layout: &self.layout,
                    entries: &entries(&self.blank_grid, &self.grid_sampler, &self.blank_atlas, &self.sampler),
                });
                *cached = Some((textures.paint.clone(), group));
            }
            &cached.as_ref().expect("made above").1
        } else {
            let (grid_view, grid_sampler) = match (grid, &canvas.grid) {
                (Some(g), Some(image)) => {
                    (&g.view, if image.nearest { &self.grid_nearest } else { &self.grid_sampler })
                }
                _ => (&self.blank_grid, &self.grid_sampler),
            };
            let (atlas_view, atlas_sampler) = match atlas {
                Some(a) if picture => (&a.view, &a.sampler),
                _ => (&self.blank_atlas, &self.sampler),
            };
            fresh = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("canvas surface"),
                layout: &self.layout,
                entries: &entries(grid_view, grid_sampler, atlas_view, atlas_sampler),
            });
            &fresh
        };
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
            key: MarkTarget::Canvas(ThingId(7)),
            center: [0.0, 0.0],
            angle: 0.0,
            size: [200.0, 200.0],
            texels: [200, 200],
            place,
            start,
            fade: 0.0,
            blend: Blend::Normal,
            opacity: 1.0,
            look: Default::default(),
            time: 0.0,
            mask: Vec::new(),
            surface: crate::Surface::Paint,
            grid: None,
            border: 0.0,
            under_glyphs: Vec::new(),
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
    fn a_skin_shows_only_through_its_mask() {
        let Some((device, queue)) = gpu() else { return };
        let mut frame = Frame::default();
        let background = pixel(&render(&device, &queue, &frame), 150.0, 0.0);
        let red = |p: [u8; 4]| p[0] > 200 && p[1] < 40 && p[2] < 40;
        // A skin turned a quarter turn, whose mask is a disc 40 px right of its middle (in its
        // own pixels) and a ring line: the disc lands below the middle on the screen.
        let mut skin = canvas(CanvasPlace::OnThing, Some([1.0, 0.0, 0.0, 1.0]));
        skin.key = MarkTarget::Skin(ThingId(7));
        skin.size = [400.0, 400.0];
        skin.angle = std::f32::consts::FRAC_PI_2;
        let ring = bpe_runtime::Outline::Ring {
            center: glam::Vec2::ZERO,
            radius: 150.0,
            sides: 0,
            angle: 0.0,
            thickness: 4.0,
        };
        skin.mask = vec![MaskShape::disc(glam::Vec2::new(40.0, 0.0), 20.0), MaskShape::line(&ring, 10.0)];
        frame.canvases.push(skin.clone());
        let pixels = render(&device, &queue, &frame);
        assert!(red(pixel(&pixels, 0.0, 40.0)), "{:?}: the disc shows", pixel(&pixels, 0.0, 40.0));
        assert_eq!(pixel(&pixels, 40.0, 0.0), background, "turned with the skin, not beside it");
        assert_eq!(pixel(&pixels, 0.0, 0.0), background, "between the shapes nothing shows");
        assert!(red(pixel(&pixels, 150.0, 0.0)), "{:?}: on the ring's line", pixel(&pixels, 150.0, 0.0));
        assert_eq!(pixel(&pixels, 172.0, 0.0), background, "past the line's reach nothing shows");
        // No shapes: nothing at all.
        skin.mask.clear();
        frame.canvases[0] = skin;
        let pixels = render(&device, &queue, &frame);
        assert_eq!(pixel(&pixels, 0.0, 40.0), background);
        assert_eq!(pixel(&pixels, 150.0, 0.0), background);
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
        draw_part(device, queue, canvas, textures, None, 0)
    }

    /// Like `draw_alone`, with a picture atlas and one `part` of the canvas.
    fn draw_part(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        canvas: &CanvasLayer,
        textures: &CanvasTextures,
        atlas: Option<&PictureAtlas>,
        part: u32,
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
            composite.draw(device, queue, &mut pass, &globals_group, 0, canvas, Some(textures), atlas, part);
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
            composite.draw(&device, &queue, &mut pass, &globals, 0, &c, Some(&a), None, 0);
            composite.draw(&device, &queue, &mut pass, &globals, 1, &c, Some(&b), None, 0);
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

    /// A hash of every pixel of `read`'s 64×64 picture.
    fn picture_hash(read: impl Fn(u32, u32) -> [f32; 2]) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325_u64;
        for y in 0..64 {
            for x in 0..64 {
                for v in read(x, y) {
                    for b in v.to_bits().to_le_bytes() {
                        h ^= b as u64;
                        h = h.wrapping_mul(0x0000_0100_0000_01b3);
                    }
                }
            }
        }
        h
    }

    #[test]
    fn a_paint_canvas_without_claims_draws_as_before() {
        let Some((device, queue)) = gpu() else { return };
        let textures = CanvasTextures::new(&device, [2, 1]);
        write_row(&queue, &textures.paint, &[[1.0, 0.0, 0.0, 1.0], [0.0; 4]]);
        write_row(&queue, &textures.aux, &[[0.0; 4], [0.0; 4]]);
        let mut hashes = Vec::new();
        for style in [PaintStyle::Flat, PaintStyle::WetPaint, PaintStyle::Watercolour, PaintStyle::NeonInk] {
            let mut c = canvas(CanvasPlace::BehindBalls, Some([0.0, 1.0, 0.0, 1.0]));
            c.size = [32.0, 32.0];
            c.texels = [2, 1];
            c.look.style = style;
            c.time = 0.5;
            hashes.push(picture_hash(draw_alone(&device, &queue, &c, &textures)));
        }
        // Recorded before covers, powder and claimed ground existed.
        assert_eq!(hashes, BEFORE, "{hashes:?}");
    }

    /// A 2×1 grid image: `left` and `right` texels.
    fn grid2(left: [u8; 4], right: [u8; 4], srgb: bool) -> Option<crate::GridImage> {
        let pixels = std::sync::Arc::new([left, right].concat());
        Some(crate::GridImage { size: [2, 1], pixels, srgb, nearest: true, cell: 16.0 })
    }

    fn blank(device: &wgpu::Device, queue: &wgpu::Queue) -> CanvasTextures {
        let textures = CanvasTextures::new(device, [2, 1]);
        write_row(queue, &textures.paint, &[[0.0; 4]; 2]);
        write_row(queue, &textures.aux, &[[0.0; 4]; 2]);
        textures
    }

    fn small(start: Option<[f32; 4]>) -> CanvasLayer {
        let mut c = canvas(CanvasPlace::BehindBalls, start);
        c.size = [32.0, 32.0];
        c.texels = [2, 1];
        c
    }

    #[test]
    fn claimed_ground_shows_under_paint() {
        let Some((device, queue)) = gpu() else { return };
        let textures = blank(&device, &queue);
        let mut c = small(None);
        c.grid = grid2([255, 0, 0, 255], [0; 4], true);
        let read = draw_alone(&device, &queue, &c, &textures);
        let (left, right) = (read(20, 32), read(44, 32));
        assert!(left[0] > 0.9 && left[1] < 0.05, "{left:?}: the claimed half is red");
        assert!(right[0] < 0.01 && right[1] < 0.01, "{right:?}: the unclaimed half is clear");
    }

    #[test]
    fn borders_darken_team_edges() {
        let Some((device, queue)) = gpu() else { return };
        let textures = blank(&device, &queue);
        let mut c = small(None);
        c.grid = grid2([255, 0, 0, 255], [0, 255, 0, 255], true);
        let plain = draw_alone(&device, &queue, &c, &textures);
        c.border = 1.0;
        let bordered = draw_alone(&device, &queue, &c, &textures);
        assert!(bordered(31, 32)[0] < plain(31, 32)[0] - 0.1, "the seam is darker with a border");
        assert!((bordered(20, 32)[0] - plain(20, 32)[0]).abs() < 0.01, "away from the seam nothing changes");
    }

    #[test]
    fn a_cover_hides_and_reveals() {
        let Some((device, queue)) = gpu() else { return };
        let textures = blank(&device, &queue);
        let mut c = small(Some([1.0, 0.0, 0.0, 1.0]));
        c.surface = crate::Surface::Cover { foil: false, under: crate::Under::Colour([0.0, 1.0, 0.0, 1.0]) };
        c.grid = grid2([255, 0, 0, 255], [0; 4], false);
        let read = draw_alone(&device, &queue, &c, &textures);
        let (left, right) = (read(20, 32), read(44, 32));
        assert!(left[0] > 0.8 && left[1] < 0.1, "{left:?}: the cover is where the amount is full");
        assert!(right[0] < 0.1 && right[1] > 0.9, "{right:?}: scraped ground shows what is underneath");
    }

    #[test]
    fn foil_shines_where_matte_does_not() {
        let Some((device, queue)) = gpu() else { return };
        let textures = blank(&device, &queue);
        let mut c = small(Some([0.5, 0.5, 0.5, 1.0]));
        c.grid = grid2([255, 0, 0, 255], [255, 0, 0, 255], false);
        c.surface = crate::Surface::Cover { foil: false, under: crate::Under::Scene };
        let matte = picture_hash(draw_alone(&device, &queue, &c, &textures));
        c.surface = crate::Surface::Cover { foil: true, under: crate::Under::Scene };
        let foil = draw_alone(&device, &queue, &c, &textures);
        let spread = (0..64).map(|x| foil(x, 32)[0]).filter(|v| *v > 0.0);
        let (lo, hi) = spread.fold((f32::MAX, 0.0f32), |(l, h), v| (l.min(v), h.max(v)));
        assert!(hi - lo > 0.1, "foil varies across its face ({lo}..{hi})");
        assert_ne!(matte, picture_hash(foil), "foil and matte differ");
    }

    #[test]
    fn cover_parts_add_up() {
        let Some((device, queue)) = gpu() else { return };
        let textures = blank(&device, &queue);
        let mut c = small(Some([1.0, 0.0, 0.0, 1.0]));
        c.surface = crate::Surface::Cover { foil: false, under: crate::Under::Colour([0.0, 1.0, 0.0, 1.0]) };
        c.grid = grid2([255, 0, 0, 255], [0; 4], false);
        let under = draw_part(&device, &queue, &c, &textures, None, 1);
        let over = draw_part(&device, &queue, &c, &textures, None, 2);
        assert!(under(20, 32)[1] > 0.9 && under(44, 32)[1] > 0.9, "part 1 is the underneath everywhere");
        assert!(over(20, 32)[0] > 0.8, "part 2 is the cover where it is whole");
        assert!(over(44, 32)[0] < 0.1 && over(44, 32)[1] < 0.1, "part 2 is clear where scraped");
    }

    #[test]
    fn powder_ridges_catch_light() {
        let Some((device, queue)) = gpu() else { return };
        let textures = blank(&device, &queue);
        let mut c = small(Some([0.8, 0.7, 0.4, 1.0]));
        c.surface = crate::Surface::Powder { snow: false, ridges: 1.0, light_angle: 45.0 };
        // Full on the left, bare on the right.
        c.grid = grid2([255, 0, 0, 255], [0; 4], false);
        let read = draw_alone(&device, &queue, &c, &textures);
        assert!(read(20, 32)[0] > 0.1, "powder is there where the amount is");
        assert!(read(44, 32)[0] < 0.01, "and bare ground is clear");
        // The slope at the heap's edge is lit differently from its flat top.
        assert!((read(30, 32)[0] - read(20, 32)[0]).abs() > 0.005, "the edge shades");
    }

    #[test]
    fn under_picture_shows_the_atlas() {
        let Some((device, queue)) = gpu() else { return };
        let dir = std::env::temp_dir().join(format!("bpe-under-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        image::RgbaImage::from_pixel(8, 8, image::Rgba([0, 255, 0, 255])).save(dir.join("g.png")).unwrap();
        let atlas = PictureAtlas::load(&device, &queue, &["g.png".to_string()], Some(&dir));
        let textures = blank(&device, &queue);
        let mut c = small(Some([1.0, 0.0, 0.0, 1.0]));
        c.surface = crate::Surface::Cover { foil: false, under: crate::Under::Picture(0) };
        c.grid = grid2([255, 0, 0, 255], [0; 4], false);
        let read = draw_part(&device, &queue, &c, &textures, Some(&atlas), 0);
        let right = read(44, 32);
        assert!(right[1] > 0.5 && right[0] < 0.2, "{right:?}: the picture shows where scraped");
        assert!(read(20, 32)[0] > 0.8, "the cover is still whole on the left");
    }

    const BEFORE: [u64; 4] = [16326282278750507301, 12348260933151833957, 7236297003248527420, 17745117802717598613];
}
