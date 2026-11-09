//! The output picture as video colours on the graphics card (review S23): BT.709, TV range, 4:2:0,
//! planar (`yuv420p`, for x264 and NVENC) or with the chroma interleaved (`nv12`, for AMF and Quick
//! Sync). The export reads these planes back and hands them to ffmpeg as they are, a third of
//! the bytes of RGBA, with no colour conversion left for ffmpeg's CPU.

/// How a [`crate::Readback`] gives frames back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelLayout {
    /// RGBA, 4 bytes a pixel (sRGB-encoded): pictures, tests, frame exports.
    Rgba,
    /// Y, then Cb, then Cr, the chroma at half size each way (ffmpeg's `yuv420p`).
    Yuv420p,
    /// Y, then Cb and Cr in pairs at half size (ffmpeg's `nv12`).
    Nv12,
}

impl PixelLayout {
    /// The `-pix_fmt` ffmpeg reads the frames as.
    pub fn ffmpeg_name(self) -> &'static str {
        match self {
            PixelLayout::Rgba => "rgba",
            PixelLayout::Yuv420p => "yuv420p",
            PixelLayout::Nv12 => "nv12",
        }
    }

    /// Whether a `width` x `height` picture can be given back this way (4:2:0 needs even sizes).
    pub fn fits(self, width: u32, height: u32) -> bool {
        self == PixelLayout::Rgba || (width.is_multiple_of(2) && height.is_multiple_of(2) && width > 0 && height > 0)
    }

    /// The planes of a `width` x `height` frame: each one's size in texels and bytes a texel.
    pub(crate) fn planes(self, width: u32, height: u32) -> Vec<(u32, u32, u32)> {
        let (cw, ch) = (width / 2, height / 2);
        match self {
            PixelLayout::Rgba => vec![(width, height, 4)],
            PixelLayout::Yuv420p => vec![(width, height, 1), (cw, ch, 1), (cw, ch, 1)],
            PixelLayout::Nv12 => vec![(width, height, 1), (cw, ch, 2)],
        }
    }
}

/// The conversion's pipelines and the plane pictures it draws into.
pub(crate) struct VideoPlanes {
    layout: PixelLayout,
    group_layout: wgpu::BindGroupLayout,
    luma: wgpu::RenderPipeline,
    chroma: wgpu::RenderPipeline,
    /// The planes' pictures, in the frame's order (see [`PixelLayout::planes`]).
    pub(crate) textures: Vec<wgpu::Texture>,
    views: Vec<wgpu::TextureView>,
    /// The group reading the output picture it was made for.
    group: Option<(wgpu::TextureView, wgpu::BindGroup)>,
}

impl VideoPlanes {
    pub(crate) fn new(device: &wgpu::Device, layout: PixelLayout, width: u32, height: u32) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("video colours"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/yuv.wgsl").into()),
        });
        let group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("video colours"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("video colours"),
            bind_group_layouts: &[Some(&group_layout)],
            immediate_size: 0,
        });
        let formats: Vec<wgpu::TextureFormat> = layout
            .planes(width, height)
            .iter()
            .map(|&(_, _, bytes)| if bytes == 2 { wgpu::TextureFormat::Rg8Unorm } else { wgpu::TextureFormat::R8Unorm })
            .collect();
        let make = |entry: &str, targets: &[wgpu::TextureFormat]| {
            let targets: Vec<Option<wgpu::ColorTargetState>> =
                targets.iter().map(|&format| Some(format.into())).collect();
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_full"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &targets,
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let luma = make("fs_luma", &formats[..1]);
        let chroma = match layout {
            PixelLayout::Nv12 => make("fs_chroma_pairs", &formats[1..]),
            _ => make("fs_chroma_planes", &formats[1..]),
        };
        let textures: Vec<wgpu::Texture> = layout
            .planes(width, height)
            .iter()
            .zip(&formats)
            .map(|(&(w, h, _), &format)| {
                device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("video plane"),
                    size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                })
            })
            .collect();
        let views = textures.iter().map(|t| t.create_view(&Default::default())).collect();
        VideoPlanes { layout, group_layout, luma, chroma, textures, views, group: None }
    }

    /// Draws `picture` (the renderer's output) into the planes.
    pub(crate) fn convert(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        picture: &wgpu::TextureView,
    ) {
        if self.group.as_ref().is_none_or(|(v, _)| v != picture) {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("video colours"),
                layout: &self.group_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(picture) }],
            });
            self.group = Some((picture.clone(), group));
        }
        let group = &self.group.as_ref().expect("just made").1;
        let pass =
            |encoder: &mut wgpu::CommandEncoder, views: &[wgpu::TextureView], pipeline: &wgpu::RenderPipeline| {
                let attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = views
                    .iter()
                    .map(|view| {
                        Some(wgpu::RenderPassColorAttachment {
                            view,
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                store: wgpu::StoreOp::Store,
                            },
                        })
                    })
                    .collect();
                let mut p = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("video colours"),
                    color_attachments: &attachments,
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                p.set_pipeline(pipeline);
                p.set_bind_group(0, group, &[]);
                p.draw(0..3, 0..1);
            };
        pass(encoder, &self.views[..1], &self.luma);
        pass(encoder, &self.views[1..], &self.chroma);
        debug_assert_ne!(self.layout, PixelLayout::Rgba);
    }
}

/// The BT.709 TV-range planes of a `width` x `height` RGBA picture worked out on the CPU, as the
/// shader does (the chroma from each 2 x 2 block's mean colour): what the tests compare with.
pub fn yuv_of_rgba(rgba: &[u8], width: u32, height: u32, layout: PixelLayout) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let at = |x: usize, y: usize| {
        let i = (y * w + x) * 4;
        [rgba[i] as f32 / 255.0, rgba[i + 1] as f32 / 255.0, rgba[i + 2] as f32 / 255.0]
    };
    let (kr, kb) = (0.2126f32, 0.0722f32);
    let luma = |c: [f32; 3]| kr * c[0] + (1.0 - kr - kb) * c[1] + kb * c[2];
    let byte = |v: f32| v.round().clamp(0.0, 255.0) as u8;
    let mut out = Vec::with_capacity(w * h * 3 / 2);
    for y in 0..h {
        for x in 0..w {
            out.push(byte(16.0 + 219.0 * luma(at(x, y))));
        }
    }
    let (cw, ch) = (w / 2, h / 2);
    let mut cb = Vec::with_capacity(cw * ch);
    let mut cr = Vec::with_capacity(cw * ch);
    for y in 0..ch {
        for x in 0..cw {
            let mut c = [0.0f32; 3];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let p = at(2 * x + dx, 2 * y + dy);
                for k in 0..3 {
                    c[k] += 0.25 * p[k];
                }
            }
            let l = luma(c);
            cb.push(byte(128.0 + 224.0 * (c[2] - l) / (2.0 * (1.0 - kb))));
            cr.push(byte(128.0 + 224.0 * (c[0] - l) / (2.0 * (1.0 - kr))));
        }
    }
    match layout {
        PixelLayout::Rgba => return rgba.to_vec(),
        PixelLayout::Yuv420p => {
            out.extend(cb);
            out.extend(cr);
        }
        PixelLayout::Nv12 => out.extend(cb.into_iter().zip(cr).flat_map(|(b, r)| [b, r])),
    }
    out
}
