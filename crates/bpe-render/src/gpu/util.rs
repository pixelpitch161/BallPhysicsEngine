//! Small helpers for the compute passes: bind group layout entries and buffers that grow.

/// A storage buffer that grows to fit whatever is uploaded, never shrinking.
pub struct StorageBuffer {
    label: &'static str,
    usage: wgpu::BufferUsages,
    buffer: wgpu::Buffer,
}

impl StorageBuffer {
    pub fn new(device: &wgpu::Device, label: &'static str, extra: wgpu::BufferUsages) -> Self {
        let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | extra;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: 256, usage, mapped_at_creation: false });
        Self { label, usage, buffer }
    }

    /// Makes room for `size` bytes. Returns true when the buffer was replaced, so bind groups
    /// that use it must be made again. The old contents are lost.
    pub fn reserve(&mut self, device: &wgpu::Device, size: u64) -> bool {
        if size <= self.buffer.size() {
            return false;
        }
        let size = size.next_power_of_two().max(256);
        self.buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(self.label),
            size,
            usage: self.usage,
            mapped_at_creation: false,
        });
        true
    }

    /// Writes `bytes` at `offset`, growing first if needed. Returns whether it grew.
    pub fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, offset: u64, bytes: &[u8]) -> bool {
        let grew = self.reserve(device, offset + bytes.len().max(4) as u64);
        if !bytes.is_empty() {
            queue.write_buffer(&self.buffer, offset, bytes);
        }
        grew
    }

    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    pub fn binding(&self, offset: u64, size: u64) -> wgpu::BindingResource<'_> {
        wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer: &self.buffer,
            offset,
            size: wgpu::BufferSize::new(size.max(4)),
        })
    }
}

pub fn uniform_entry(binding: u32, visibility: wgpu::ShaderStages, dynamic: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: dynamic,
            min_binding_size: None,
        },
        count: None,
    }
}

pub fn storage_entry(binding: u32, visibility: wgpu::ShaderStages, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// A float texture read with `textureLoad`, such as the wall distance field.
pub fn float_texture_entry(binding: u32, visibility: wgpu::ShaderStages, filterable: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable },
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

pub fn sampler_entry(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

/// The renderer's globals layout: one uniform buffer at binding 0. Layouts with the same
/// entries are interchangeable, so the renderer's own globals bind group fits pipelines made
/// with this one.
pub fn globals_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("globals layout"),
        entries: &[uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT, false)],
    })
}

pub fn compute_pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    entry: &str,
) -> wgpu::ComputePipeline {
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        module,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: None,
    })
}

pub fn shader(device: &wgpu::Device, label: &str, sources: &[&str]) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(sources.concat().into()),
    })
}

/// Workgroups needed to cover `count` items `size` at a time.
pub fn groups(count: u32, size: u32) -> u32 {
    count.div_ceil(size).max(1)
}

/// Rounds up to the alignment wgpu needs for buffer offsets in bind groups.
pub fn align(offset: u64) -> u64 {
    offset.next_multiple_of(256)
}

pub const COMMON_WGSL: &str = include_str!("../shaders/gpu/common.wgsl");
