//! The scopes the editor shows under the picture in Grade mode, worked out on the GPU from the
//! finished (graded, 8-bit) picture. Part 1 has the Histogram. Never part of an export.

use std::sync::{Arc, Mutex};

/// Pixels counted by their red, green, blue and brightness (each 0..255), in that order.
pub type HistogramBins = [[u32; 256]; 4];

const BINS: u64 = 4 * 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reading {
    /// Nothing asked for, or the last reading taken.
    Free,
    /// Copied out and waiting for the buffer to map.
    Mapping,
    Mapped,
}

/// The histogram's pipeline, its counts and their non-blocking read back: [`Histogram::encode`]
/// counts a picture; once the GPU has done so, [`Histogram::take`] gives the counts. While one
/// reading is on its way, asking for another does nothing.
pub struct Histogram {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    bins: wgpu::Buffer,
    read: wgpu::Buffer,
    reading: Arc<Mutex<Reading>>,
    /// The texture view the bind group reads, and the group.
    group: Option<(wgpu::TextureView, wgpu::BindGroup)>,
}

impl Histogram {
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("scopes"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/scopes.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("histogram layout"),
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
                        min_binding_size: wgpu::BufferSize::new(BINS * 4),
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("histogram"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("histogram"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("cs_histogram"),
            compilation_options: Default::default(),
            cache: None,
        });
        let buffer = |label, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: BINS * 4,
                usage,
                mapped_at_creation: false,
            })
        };
        let bins = buffer(
            "histogram bins",
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        );
        let read = buffer("histogram read", wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);
        Self { pipeline, layout, bins, read, reading: Arc::new(Mutex::new(Reading::Free)), group: None }
    }

    /// Whether a reading is on its way (asking again now does nothing).
    pub fn busy(&self) -> bool {
        *self.reading.lock().expect("histogram lock") != Reading::Free
    }

    /// Counts the picture `view` (an `Rgba8Unorm` texture `size` big) into the bins and copies
    /// them out, unless a reading is on its way. Returns whether it did. Submit the encoder, then
    /// call [`Histogram::after_submit`].
    pub fn encode(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        size: (u32, u32),
    ) -> bool {
        if self.busy() {
            return false;
        }
        if self.group.as_ref().is_none_or(|(v, _)| v != view) {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("histogram"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                    wgpu::BindGroupEntry { binding: 1, resource: self.bins.as_entire_binding() },
                ],
            });
            self.group = Some((view.clone(), group));
        }
        encoder.clear_buffer(&self.bins, 0, None);
        {
            let mut pass = encoder
                .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("histogram"), timestamp_writes: None });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.group.as_ref().expect("made above").1, &[]);
            pass.dispatch_workgroups(size.0.div_ceil(16), size.1.div_ceil(16), 1);
        }
        encoder.copy_buffer_to_buffer(&self.bins, 0, &self.read, 0, BINS * 4);
        *self.reading.lock().expect("histogram lock") = Reading::Mapping;
        true
    }

    /// After the encoder [`Histogram::encode`] wrote into is submitted: starts reading the counts.
    pub fn after_submit(&self) {
        let reading = self.reading.clone();
        self.read.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            *reading.lock().expect("histogram lock") = if r.is_ok() { Reading::Mapped } else { Reading::Free };
        });
    }

    /// The counts, once the GPU has them (the device must have been polled since).
    pub fn take(&mut self) -> Option<Box<HistogramBins>> {
        if *self.reading.lock().expect("histogram lock") != Reading::Mapped {
            return None;
        }
        let mut bins = Box::new([[0u32; 256]; 4]);
        if let Ok(data) = self.read.slice(..).get_mapped_range() {
            let counts: &[u32] = bytemuck::cast_slice(&data);
            for (i, c) in counts.iter().enumerate().take(BINS as usize) {
                bins[i / 256][i % 256] = *c;
            }
        }
        self.read.unmap();
        *self.reading.lock().expect("histogram lock") = Reading::Free;
        Some(bins)
    }
}
