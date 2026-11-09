//! Things masks' silhouettes (colour grading part 5, `docs/notes/grading.md`): for each choice of
//! things a frame's Things parts read (`grade::Program::selections`), the frame's things drawn
//! again as their bodies only, in drawing order, into a coverage picture (red the chosen things as
//! seen, alpha every thing), then how far each pixel is from them by jump flooding, into one layer
//! of the **things field** the grade reads at group 2 binding 4. A frame without Things parts
//! draws none of it.

use super::*;
use crate::grade::Selection;

/// Bytes between two selections' (and two flood steps') uniforms: the dynamic offset alignment.
const STRIDE: u64 = 256;
/// The most flood passes one selection takes (a seed pass, steps, the last pass).
const MOST_PASSES: u64 = 20;
const SILHOUETTE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const SEED_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;

/// The pipelines, buffers and pictures of the silhouettes, made the first time a frame has a
/// Things part.
pub(crate) struct Silhouettes {
    select_layout: wgpu::BindGroupLayout,
    /// One uniform a selection: `[first code, codes, every thing, 0]`.
    select: wgpu::Buffer,
    codes: wgpu::Buffer,
    select_group: wgpu::BindGroup,
    path: wgpu::RenderPipeline,
    layered: wgpu::RenderPipeline,
    sprite: wgpu::RenderPipeline,
    flood_layout: wgpu::BindGroupLayout,
    seed: wgpu::RenderPipeline,
    step: wgpu::RenderPipeline,
    last: wgpu::RenderPipeline,
    /// One uniform a flood pass: `[step, read through the Stage camera, 0, 0]`.
    steps: wgpu::Buffer,
    no_uv: wgpu::TextureView,
    pictures: Option<Pictures>,
}

/// The pictures for one size and number of layers.
struct Pictures {
    size: (u32, u32),
    layers: u32,
    /// The world's things, then the things stuck to the screen (drawn over them, never seen
    /// through the Stage camera).
    silhouette: wgpu::TextureView,
    top: wgpu::TextureView,
    seeds: [wgpu::TextureView; 2],
    /// Reading each seeds picture.
    from_seeds: [wgpu::BindGroup; 2],
    field: wgpu::TextureView,
    field_layers: Vec<wgpu::TextureView>,
    /// Reading the silhouettes, and the uv it was made with (`None` the stand-in).
    from_silhouette: Option<(Option<wgpu::TextureView>, wgpu::BindGroup)>,
}

/// The selection snippet with its group set.
fn select_source(group: u32) -> String {
    include_str!("../shaders/silhouette_select.wgsl").replace("SIL_GROUP", &group.to_string())
}

fn texture(
    device: &wgpu::Device,
    label: &str,
    (w, h): (u32, u32),
    layers: u32,
    format: wgpu::TextureFormat,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: w.max(1), height: h.max(1), depth_or_array_layers: layers.max(1) },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

/// The flood steps for a reach of `reach` output pixels: halving from the power of two past it,
/// then one more of 1 (JFA+1), none for no reach.
pub(crate) fn flood_steps(reach: f32) -> Vec<u32> {
    if !(reach.is_finite() && reach > 0.0) {
        return Vec::new();
    }
    let far = (reach.min(8192.0).ceil() as u32 + 2).next_power_of_two();
    let mut k = (far / 2).max(1);
    let mut out = Vec::new();
    while k >= 1 {
        out.push(k);
        k /= 2;
    }
    out.push(1);
    out
}

impl Silhouettes {
    pub(crate) fn new(
        device: &wgpu::Device,
        globals_layout: &wgpu::BindGroupLayout,
        paint_layout: &wgpu::BindGroupLayout,
        layers_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let select_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("silhouette choice"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(16),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let select = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("silhouette choices"),
            size: STRIDE * crate::grade::MAX_SELECTIONS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let codes = Self::make_codes(device, 64);
        let select_group = Self::select_group(device, &select_layout, &select, &codes);

        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let target = [Some(wgpu::ColorTargetState {
            format: SILHOUETTE_FORMAT,
            blend: Some(blend),
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let pipeline = |label: &str,
                        layout: &wgpu::PipelineLayout,
                        module: &wgpu::ShaderModule,
                        (vs, fs): (&str, &str),
                        buffers: &[Option<wgpu::VertexBufferLayout>]| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers,
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &target,
                }),
                multiview_mask: None,
                cache: None,
            })
        };

        // Shapes, text and balls: the scene shader with the silhouette entries.
        let scene_source =
            [crate::scene_source(None), select_source(3), include_str!("../shaders/silhouette_scene.wgsl").to_string()]
                .concat();
        let scene = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("silhouettes"),
            source: wgpu::ShaderSource::Wgsl(scene_source.into()),
        });
        let scene_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("silhouettes"),
            bind_group_layouts: &[Some(globals_layout), Some(paint_layout), Some(layers_layout), Some(&select_layout)],
            immediate_size: 0,
        });
        let instances = |stride: usize, attributes: &'static [wgpu::VertexAttribute]| wgpu::VertexBufferLayout {
            array_stride: stride as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes,
        };
        let path = pipeline(
            "path silhouettes",
            &scene_layout,
            &scene,
            ("vs_path", "fs_path_silhouette"),
            &[Some(instances(std::mem::size_of::<PathInstance>(), &PATH_ATTRIBUTES))],
        );
        let layered = pipeline(
            "ball silhouettes",
            &scene_layout,
            &scene,
            ("vs_layers", "fs_layers_silhouette"),
            &[Some(instances(std::mem::size_of::<LayeredInstance>(), &LAYERED_ATTRIBUTES))],
        );
        // A Particles thing's sprites.
        let sprite_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("sprite silhouettes"),
            source: wgpu::ShaderSource::Wgsl(sprite_silhouette_source("").into()),
        });
        let sprite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sprite silhouettes"),
            bind_group_layouts: &[Some(globals_layout), Some(&select_layout)],
            immediate_size: 0,
        });
        let sprite = pipeline(
            "sprite silhouettes",
            &sprite_layout,
            &sprite_module,
            ("vs_sprite", "fs_sprite_silhouette"),
            &[Some(instances(std::mem::size_of::<Particle>(), &crate::sprites::ATTRIBUTES))],
        );

        // The flood.
        let read = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let flood_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("silhouette flood"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: wgpu::BufferSize::new(16),
                    },
                    count: None,
                },
                read(1),
                read(2),
                read(3),
            ],
        });
        let flood_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("silhouette flood"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/silhouette_jfa.wgsl").into()),
        });
        let flood_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("silhouette flood"),
            bind_group_layouts: &[Some(&flood_layout)],
            immediate_size: 0,
        });
        let flood = |fs: &str, format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("silhouette flood"),
                layout: Some(&flood_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &flood_module,
                    entry_point: Some("vs_jfa"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &flood_module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let seed = flood("fs_jfa_seed", SEED_FORMAT);
        let step = flood("fs_jfa_step", SEED_FORMAT);
        let last = flood("fs_jfa_last", HDR_FORMAT);
        let steps = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("silhouette flood steps"),
            size: STRIDE * MOST_PASSES * crate::grade::MAX_SELECTIONS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let no_uv = texture(device, "no stage camera uv", (1, 1), 1, wgpu::TextureFormat::Rg32Float)
            .create_view(&Default::default());
        Self {
            select_layout,
            select,
            codes,
            select_group,
            path,
            layered,
            sprite,
            flood_layout,
            seed,
            step,
            last,
            steps,
            no_uv,
            pictures: None,
        }
    }

    fn make_codes(device: &wgpu::Device, count: u64) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("silhouette codes"),
            size: count.max(4) * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    fn select_group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        select: &wgpu::Buffer,
        codes: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("silhouette choice"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: select,
                        offset: 0,
                        size: wgpu::BufferSize::new(16),
                    }),
                },
                wgpu::BindGroupEntry { binding: 1, resource: codes.as_entire_binding() },
            ],
        })
    }

    /// The layout of the choice group (for the swarms' silhouette pipeline).
    pub(crate) fn select_layout(&self) -> &wgpu::BindGroupLayout {
        &self.select_layout
    }

    /// Writes `selections`' codes and their flood steps for a `size` picture whose reach is
    /// `scale` output pixels a video pixel, making the pictures when they don't fit. Returns
    /// each selection's flood steps.
    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        selections: &[Selection],
        size: (u32, u32),
        scale: f32,
        through_stage: bool,
    ) -> Vec<Vec<u32>> {
        let mut codes: Vec<u32> = Vec::new();
        let mut heads = vec![0u8; STRIDE as usize * selections.len()];
        for (i, s) in selections.iter().enumerate() {
            let list = s.things.as_deref().unwrap_or(&[]);
            let head = [codes.len() as u32, list.len() as u32, u32::from(s.things.is_none()), 0];
            heads[i * STRIDE as usize..i * STRIDE as usize + 16].copy_from_slice(bytemuck::cast_slice(&head));
            codes.extend(list);
        }
        if codes.len() as u64 * 4 > self.codes.size() {
            self.codes = Self::make_codes(device, codes.len() as u64 * 2);
            self.select_group = Self::select_group(device, &self.select_layout, &self.select, &self.codes);
        }
        if !codes.is_empty() {
            queue.write_buffer(&self.codes, 0, bytemuck::cast_slice(&codes));
        }
        queue.write_buffer(&self.select, 0, &heads);

        let plans: Vec<Vec<u32>> = selections.iter().map(|s| flood_steps(s.reach * scale)).collect();
        let mut uniforms = vec![0u8; (STRIDE * MOST_PASSES) as usize * selections.len()];
        let stage = if through_stage { 1.0f32 } else { 0.0 };
        for (i, plan) in plans.iter().enumerate() {
            // Pass 0 seeds, then the steps, then the last.
            let mut write = |pass: usize, k: f32| {
                let at = (i * MOST_PASSES as usize + pass) * STRIDE as usize;
                uniforms[at..at + 16].copy_from_slice(bytemuck::cast_slice(&[k, stage, 0.0f32, 0.0]));
            };
            write(0, 0.0);
            for (n, k) in plan.iter().enumerate() {
                write(1 + n, *k as f32);
            }
        }
        queue.write_buffer(&self.steps, 0, &uniforms);

        let layers = selections.len() as u32;
        if self.pictures.as_ref().is_none_or(|p| p.size != size || p.layers < layers) {
            self.pictures = Some(self.make_pictures(device, size, layers));
        }
        plans
    }

    fn make_pictures(&self, device: &wgpu::Device, size: (u32, u32), layers: u32) -> Pictures {
        let silhouette = texture(device, "silhouette", size, 1, SILHOUETTE_FORMAT).create_view(&Default::default());
        let top =
            texture(device, "silhouette on the screen", size, 1, SILHOUETTE_FORMAT).create_view(&Default::default());
        let seeds =
            [0, 1].map(|_| texture(device, "silhouette seeds", size, 1, SEED_FORMAT).create_view(&Default::default()));
        let field_texture = texture(device, "things field", size, layers, HDR_FORMAT);
        let field = field_texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let field_layers = (0..layers.max(1))
            .map(|l| {
                field_texture.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: l,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let from_seeds = [0, 1].map(|i| self.flood_group(device, &seeds[i], &self.no_uv, &self.no_uv));
        Pictures { size, layers, silhouette, top, seeds, from_seeds, field, field_layers, from_silhouette: None }
    }

    fn flood_group(
        &self,
        device: &wgpu::Device,
        from: &wgpu::TextureView,
        uv: &wgpu::TextureView,
        top: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("silhouette flood"),
            layout: &self.flood_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.steps,
                        offset: 0,
                        size: wgpu::BufferSize::new(16),
                    }),
                },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(from) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(uv) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(top) },
            ],
        })
    }

    /// Floods selection `index`'s silhouette (`plan` its steps) into its layer of the field;
    /// `uv` is the moved Stage camera's (the stand-in otherwise).
    fn flood(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        index: usize,
        plan: &[u32],
        uv: Option<&wgpu::TextureView>,
    ) {
        let p = self.pictures.as_ref().expect("prepared");
        let fresh = p.from_silhouette.as_ref().is_none_or(|(made, _)| made.as_ref() != uv);
        if fresh {
            let group = self.flood_group(device, &p.silhouette, uv.unwrap_or(&self.no_uv), &p.top);
            self.pictures.as_mut().expect("prepared").from_silhouette = Some((uv.cloned(), group));
        }
        let p = self.pictures.as_ref().expect("prepared");
        let from_silhouette = &p.from_silhouette.as_ref().expect("made above").1;
        let offset = |pass: usize| ((index * MOST_PASSES as usize + pass) as u64 * STRIDE) as u32;
        let run = |encoder: &mut wgpu::CommandEncoder,
                   pipeline: &wgpu::RenderPipeline,
                   group: &wgpu::BindGroup,
                   pass: usize,
                   to: &wgpu::TextureView| {
            let mut rp = begin_pass(encoder, to, "silhouette flood", wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
            rp.set_pipeline(pipeline);
            rp.set_bind_group(0, group, &[offset(pass)]);
            rp.draw(0..3, 0..1);
        };
        run(encoder, &self.seed, from_silhouette, 0, &p.seeds[0]);
        let mut at = 0;
        for n in 0..plan.len() {
            run(encoder, &self.step, &p.from_seeds[at], 1 + n, &p.seeds[1 - at]);
            at = 1 - at;
        }
        run(encoder, &self.last, &p.from_seeds[at], 0, &p.field_layers[index]);
    }
}

/// The sprites' shader with the silhouette entry (`extra` pasted after it: the swarm's own code).
pub(crate) fn sprite_silhouette_source(extra: &str) -> String {
    let group = if extra.is_empty() { 1 } else { 2 };
    [
        crate::light_pass::WGSL,
        include_str!("../shaders/tint.wgsl"),
        include_str!("../shaders/sprites.wgsl"),
        include_str!("../shaders/each.wgsl"),
        extra,
        &select_source(group),
        include_str!("../shaders/silhouette_sprite.wgsl"),
    ]
    .concat()
}

impl Renderer {
    /// Draws the silhouettes `frame`'s grade reads (none without Things parts) and gives back the
    /// things field. `moved`: the picture is seen through the moved Stage camera.
    pub(crate) fn draw_silhouettes(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        frame: &Frame,
        moved: bool,
    ) -> Option<wgpu::TextureView> {
        let selections = self.grade.program(&frame.grade).map(|p| p.selections.clone()).unwrap_or_default();
        if selections.is_empty() {
            return None;
        }
        let mut sil = self
            .silhouettes
            .take()
            .unwrap_or_else(|| Silhouettes::new(device, &self.globals_layout, &self.paint_layout, &self.layers_layout));
        let swarms = std::iter::once(frame)
            .chain(frame.overlay.as_deref())
            .chain(frame.layers.iter().map(|l| &l.frame))
            .chain(frame.overlay.iter().flat_map(|o| o.layers.iter().map(|l| &l.frame)))
            .any(|f| !f.swarms.is_empty() || !f.swarms_front.is_empty());
        if swarms {
            self.swarms.ensure_silhouette(device, &self.globals_layout, sil.select_layout());
        }
        let size = (self.targets.width, self.targets.height);
        // Output pixels a scene unit, as the grade reads it (`post.wgsl`: size.y · view.y / 2).
        let scale = self.targets.height as f32 / frame.height.max(1) as f32;
        let uv = if moved { self.stage_camera.as_ref().and_then(|s| s.uv_view()).cloned() } else { None };
        let plans = sil.prepare(device, queue, &selections, size, scale, uv.is_some());
        let layer_buffers = std::mem::take(&mut self.layer_buffers);
        let overlay_buffers = std::mem::take(&mut self.overlay);
        for (index, plan) in plans.iter().enumerate() {
            let p = sil.pictures.as_ref().expect("prepared");
            let (world_view, top_view) = (p.silhouette.clone(), p.top.clone());
            let offset = (index as u64 * STRIDE) as u32;
            {
                let mut pass =
                    begin_pass(encoder, &world_view, "silhouette", wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
                let world = (&self.globals_group, &self.paint.group);
                self.silhouette_frame(&mut pass, &sil, offset, frame, &self.buffers, &layer_buffers, world);
            }
            {
                let mut pass = begin_pass(
                    encoder,
                    &top_view,
                    "silhouette on the screen",
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                );
                if let Some(o) = frame.overlay.as_deref() {
                    let own = &layer_buffers[frame.layers.len().min(layer_buffers.len())..];
                    let screen = (&self.screen_globals_group, &self.overlay_paint.group);
                    self.silhouette_frame(&mut pass, &sil, offset, o, &overlay_buffers, own, screen);
                }
            }
            sil.flood(device, encoder, index, plan, uv.as_ref());
        }
        self.layer_buffers = layer_buffers;
        self.overlay = overlay_buffers;
        let field = sil.pictures.as_ref().map(|p| p.field.clone());
        self.silhouettes = Some(sil);
        field
    }

    /// `f`'s things into a silhouette, in drawing order: its layers below 0, its own sprites
    /// behind and shapes, its layers behind the balls, its balls and sprites in front, then the
    /// rest of its layers. `globals` are the world's (or the screen's) globals and paint.
    #[allow(clippy::too_many_arguments)]
    fn silhouette_frame(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        sil: &Silhouettes,
        offset: u32,
        f: &Frame,
        b: &FrameBuffers,
        layers: &[FrameBuffers],
        globals: (&wgpu::BindGroup, &wgpu::BindGroup),
    ) {
        let each = |pass: &mut wgpu::RenderPass<'_>, which: &dyn Fn(&BlendLayer) -> bool| {
            for (l, lb) in f.layers.iter().zip(layers).filter(|(l, _)| which(l)) {
                self.silhouette_part(pass, sil, offset, &l.frame, lb, globals, false);
                self.silhouette_part(pass, sil, offset, &l.frame, lb, globals, true);
            }
        };
        each(pass, &|l| l.z < 0);
        self.silhouette_part(pass, sil, offset, f, b, globals, false);
        each(pass, &|l| l.z == 0 && l.behind());
        self.silhouette_part(pass, sil, offset, f, b, globals, true);
        each(pass, &|l| l.z > 0 || (l.z == 0 && !l.behind()));
    }

    /// One half of a frame's own lists: sprites behind and shapes (`front` false), or balls and
    /// sprites in front.
    #[allow(clippy::too_many_arguments)]
    fn silhouette_part(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        sil: &Silhouettes,
        offset: u32,
        f: &Frame,
        b: &FrameBuffers,
        (globals, paint): (&wgpu::BindGroup, &wgpu::BindGroup),
        front: bool,
    ) {
        // Sprites, and swarms between them where they sit (fields aren't silhouettes).
        let sprites = |pass: &mut wgpu::RenderPass<'_>, front: bool| {
            let (buffer, count, runs) = if front {
                (&b.particles_front, f.particles_front.len() as u32, &f.swarms_front)
            } else {
                (&b.particles, f.particles.len() as u32, &f.swarms)
            };
            let range = |pass: &mut wgpu::RenderPass<'_>, range: std::ops::Range<u32>| {
                if !range.is_empty() {
                    pass.set_pipeline(&sil.sprite);
                    pass.set_bind_group(0, globals, &[]);
                    pass.set_bind_group(1, &sil.select_group, &[offset]);
                    pass.set_vertex_buffer(0, buffer.slice());
                    pass.draw(0..4, range);
                }
            };
            let mut start = 0;
            for (k, run) in runs.iter().enumerate() {
                let at = run.at.clamp(start, count);
                range(pass, start..at);
                start = at;
                pass.set_bind_group(0, globals, &[]);
                self.swarms.draw_silhouette(pass, &b.swarms, front, k, (&sil.select_group, offset));
            }
            range(pass, start..count);
        };
        let scene_groups = |pass: &mut wgpu::RenderPass<'_>, records: &wgpu::BindGroup| {
            pass.set_bind_group(0, globals, &[]);
            pass.set_bind_group(1, paint, &[]);
            pass.set_bind_group(2, records, &[]);
            pass.set_bind_group(3, &sil.select_group, &[offset]);
        };
        if front {
            if let Some(records) = b.layers_group.as_ref().filter(|_| !f.layered.is_empty()) {
                pass.set_pipeline(&sil.layered);
                scene_groups(pass, records);
                pass.set_vertex_buffer(0, b.layered.slice());
                pass.draw(0..4, 0..f.layered.len() as u32);
            }
            sprites(pass, true);
        } else {
            sprites(pass, false);
            if let Some(records) = b.layers_group.as_ref().filter(|_| !f.paths.is_empty()) {
                pass.set_pipeline(&sil.path);
                scene_groups(pass, records);
                pass.set_vertex_buffer(0, b.paths.slice());
                pass.draw(0..4, 0..f.paths.len() as u32);
            }
        }
    }
}
