//! The scopes the editor shows under the picture in Grade mode, worked out on the GPU from the
//! finished (graded, 8-bit) picture: the Histogram (part 1), the Waveform, the RGB Parade and the
//! Vectorscope (part 6). And the editor's clipping views (J and Alt), drawn into a picture of
//! their own over the preview. Never part of an export.

use std::sync::{Arc, Mutex};

/// Pixels counted by their red, green, blue and brightness (each 0..255), in that order.
pub type HistogramBins = [[u32; 256]; 4];

/// Columns of a waveform (the picture's width counted into this many) and levels of every scope.
pub const SCOPE_COLUMNS: usize = 256;
pub const SCOPE_LEVELS: usize = 256;

/// The most bins a scope has (the Parade's three waveforms).
const MOST_BINS: u64 = 3 * 65536;

/// Which scope.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ScopeKind {
    /// How many pixels sit at each level of red, green, blue and brightness.
    #[default]
    Histogram,
    /// Each column's brightness levels, darks at the bottom.
    Waveform,
    /// The waveforms of red, green and blue side by side.
    Parade,
    /// Where each pixel's colour lies round the middle (BT.709 Cb across, Cr up).
    Vectorscope,
}

impl ScopeKind {
    pub const ALL: [ScopeKind; 4] =
        [ScopeKind::Histogram, ScopeKind::Waveform, ScopeKind::Parade, ScopeKind::Vectorscope];

    /// How many bins it counts into.
    pub fn bins(self) -> usize {
        match self {
            ScopeKind::Histogram => 4 * 256,
            ScopeKind::Waveform | ScopeKind::Vectorscope => SCOPE_LEVELS * SCOPE_COLUMNS,
            ScopeKind::Parade => 3 * SCOPE_LEVELS * SCOPE_COLUMNS,
        }
    }

    fn entry(self) -> &'static str {
        match self {
            ScopeKind::Histogram => "cs_histogram",
            ScopeKind::Waveform => "cs_waveform",
            ScopeKind::Parade => "cs_parade",
            ScopeKind::Vectorscope => "cs_vectorscope",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// One reading of a scope: its bins (as [`ScopeKind`] lays them out, `scopes.wgsl`) and the size
/// of the picture counted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeCounts {
    pub kind: ScopeKind,
    pub counts: Vec<u32>,
    pub size: (u32, u32),
}

impl ScopeCounts {
    /// The Histogram's bins, for a Histogram reading.
    pub fn histogram(&self) -> Option<HistogramBins> {
        if self.kind != ScopeKind::Histogram || self.counts.len() < 1024 {
            return None;
        }
        let mut bins = [[0u32; 256]; 4];
        for (i, c) in self.counts.iter().take(1024).enumerate() {
            bins[i / 256][i % 256] = *c;
        }
        Some(bins)
    }

    /// A waveform's (or, with `channel` 0..2, a parade channel's) count at `level` in `column`.
    pub fn wave(&self, channel: usize, level: usize, column: usize) -> u32 {
        let i = channel * SCOPE_LEVELS * SCOPE_COLUMNS + level * SCOPE_COLUMNS + column;
        self.counts.get(i).copied().unwrap_or(0)
    }
}

/// Where a colour (sRGB-encoded, 0..1) lands on the Vectorscope, as its bins: `(u, v)`, 0..255
/// each (Cb across, Cr up from the middle at 127.5).
pub fn vectorscope_point(c: [f64; 3]) -> (f64, f64) {
    let y = 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    let cb = (c[2] - y) / 1.8556;
    let cr = (c[0] - y) / 1.5748;
    ((cb + 0.5) * 255.0, (cr + 0.5) * 255.0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reading {
    /// Nothing asked for, or the last reading taken.
    Free,
    /// Copied out and waiting for the buffer to map.
    Mapping,
    Mapped,
}

/// The scopes' pipelines, their counts and their non-blocking read back: [`Scopes::encode`]
/// counts a picture into one scope; once the GPU has done so, [`Scopes::take`] gives the counts.
/// While one reading is on its way, asking for another does nothing.
pub struct Scopes {
    pipelines: Vec<wgpu::ComputePipeline>,
    layout: wgpu::BindGroupLayout,
    bins: wgpu::Buffer,
    read: wgpu::Buffer,
    reading: Arc<Mutex<Reading>>,
    /// What the reading on its way counts.
    asked: Option<(ScopeKind, (u32, u32))>,
    /// The texture view the bind group reads, and the group.
    group: Option<(wgpu::TextureView, wgpu::BindGroup)>,
}

impl Scopes {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scopes"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/scopes.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scopes layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("scopes"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipelines = ScopeKind::ALL
            .iter()
            .map(|kind| {
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(kind.entry()),
                    layout: Some(&pipeline_layout),
                    module: &module,
                    entry_point: Some(kind.entry()),
                    compilation_options: Default::default(),
                    cache: None,
                })
            })
            .collect();
        let buffer = |label, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: MOST_BINS * 4,
                usage,
                mapped_at_creation: false,
            })
        };
        let bins = buffer(
            "scope bins",
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        );
        let read = buffer("scope read", wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);
        Self { pipelines, layout, bins, read, reading: Arc::new(Mutex::new(Reading::Free)), asked: None, group: None }
    }

    /// Whether a reading is on its way (asking again now does nothing).
    pub fn busy(&self) -> bool {
        *self.reading.lock().expect("scopes lock") != Reading::Free
    }

    /// Counts the picture `view` (an `Rgba8Unorm` texture `size` big) into scope `kind` and copies
    /// the counts out, unless a reading is on its way. Returns whether it did. Submit the encoder,
    /// then call [`Scopes::after_submit`].
    pub fn encode(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        size: (u32, u32),
        kind: ScopeKind,
    ) -> bool {
        if self.busy() {
            return false;
        }
        if self.group.as_ref().is_none_or(|(v, _)| v != view) {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("scopes"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                    wgpu::BindGroupEntry { binding: 1, resource: self.bins.as_entire_binding() },
                ],
            });
            self.group = Some((view.clone(), group));
        }
        let bytes = kind.bins() as u64 * 4;
        encoder.clear_buffer(&self.bins, 0, Some(bytes));
        {
            let mut pass = encoder
                .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some(kind.entry()), timestamp_writes: None });
            pass.set_pipeline(&self.pipelines[kind.index()]);
            pass.set_bind_group(0, &self.group.as_ref().expect("made above").1, &[]);
            pass.dispatch_workgroups(size.0.div_ceil(16), size.1.div_ceil(16), 1);
        }
        encoder.copy_buffer_to_buffer(&self.bins, 0, &self.read, 0, bytes);
        *self.reading.lock().expect("scopes lock") = Reading::Mapping;
        self.asked = Some((kind, size));
        true
    }

    /// After the encoder [`Scopes::encode`] wrote into is submitted: starts reading the counts.
    pub fn after_submit(&self) {
        let reading = self.reading.clone();
        let bytes = self.asked.map_or(4, |(k, _)| k.bins() as u64 * 4);
        self.read.slice(..bytes).map_async(wgpu::MapMode::Read, move |r| {
            *reading.lock().expect("scopes lock") = if r.is_ok() { Reading::Mapped } else { Reading::Free };
        });
    }

    /// The counts, once the GPU has them (the device must have been polled since).
    pub fn take(&mut self) -> Option<ScopeCounts> {
        if *self.reading.lock().expect("scopes lock") != Reading::Mapped {
            return None;
        }
        let (kind, size) = self.asked.take()?;
        let n = kind.bins();
        let mut counts = vec![0u32; n];
        if let Ok(data) = self.read.slice(..n as u64 * 4).get_mapped_range() {
            counts.copy_from_slice(&bytemuck::cast_slice::<u8, u32>(&data)[..n]);
        }
        self.read.unmap();
        *self.reading.lock().expect("scopes lock") = Reading::Free;
        Some(ScopeCounts { kind, counts, size })
    }
}

/// What the clipping picture shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ClipView {
    /// Nothing.
    #[default]
    Off,
    /// J: red where every channel is at white, blue where every channel is at black.
    Marks,
    /// Alt on Exposure, Highlights or Whites: black, with each channel that is at white lit.
    Whites,
    /// Alt on Shadows or Blacks: white, with each channel that is at black gone.
    Blacks,
}

impl ClipView {
    fn word(self) -> u32 {
        match self {
            ClipView::Off => 0,
            ClipView::Marks => 1,
            ClipView::Whites => 2,
            ClipView::Blacks => 3,
        }
    }
}

/// The clipping views: a compute pass that reads the finished picture and writes a picture of
/// its own (`Rgba8Unorm`, the same size, see-through where J marks nothing), for the editor to
/// draw over the preview. The export never makes it.
pub struct Clipping {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    mode: wgpu::Buffer,
    /// The picture it writes, its view and size.
    target: Option<(wgpu::Texture, wgpu::TextureView, (u32, u32))>,
    /// The picture read and the target written, and their group.
    group: Option<(wgpu::TextureView, wgpu::TextureView, wgpu::BindGroup)>,
}

impl Clipping {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("clipping"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/clipping.wgsl").into()),
        });
        let compute = wgpu::ShaderStages::COMPUTE;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("clipping layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: compute,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: compute,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: compute,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(16),
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("clipping"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("clipping"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("cs_clipping"),
            compilation_options: Default::default(),
            cache: None,
        });
        let mode = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("clipping mode"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { pipeline, layout, mode, target: None, group: None }
    }

    /// The picture it writes (made on the first [`Clipping::run`]), for the editor to show.
    pub fn view(&self) -> Option<&wgpu::TextureView> {
        self.target.as_ref().map(|(_, v, _)| v)
    }

    /// Works out `view` of the finished picture `picture` (an `Rgba8Unorm` texture `size` big)
    /// and submits it. Returns whether its own picture was made anew (a first run, or a new
    /// size), so the editor points its texture at the new one.
    pub fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        picture: &wgpu::TextureView,
        size: (u32, u32),
        view: ClipView,
    ) -> bool {
        let size = (size.0.max(1), size.1.max(1));
        let made = self.target.as_ref().is_none_or(|(_, _, s)| *s != size);
        if made {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("clipping"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let target = texture.create_view(&Default::default());
            self.target = Some((texture, target, size));
            self.group = None;
        }
        let target = &self.target.as_ref().expect("made above").1;
        if self.group.as_ref().is_none_or(|(p, t, _)| p != picture || t != target) {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("clipping"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(picture) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(target) },
                    wgpu::BindGroupEntry { binding: 2, resource: self.mode.as_entire_binding() },
                ],
            });
            self.group = Some((picture.clone(), target.clone(), group));
        }
        queue.write_buffer(&self.mode, 0, bytemuck::cast_slice(&[view.word(), 0, 0, 0]));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("clipping") });
        {
            let mut pass = encoder
                .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("clipping"), timestamp_writes: None });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.group.as_ref().expect("made above").2, &[]);
            pass.dispatch_workgroups(size.0.div_ceil(16), size.1.div_ceil(16), 1);
        }
        queue.submit(Some(encoder.finish()));
        made
    }

    /// Reads its picture back (tests): RGBA bytes, row by row.
    pub fn read(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Vec<u8>> {
        let (texture, _, (w, h)) = self.target.as_ref()?;
        crate::renderer::read_rgba8(device, queue, texture, *w, *h).ok()
    }
}
