//! The colour grade's program on the GPU: the compiled stack (`crate::grade::compile`) in a
//! storage buffer the composite reads at group 2.

use super::*;
use crate::grade::{self, Program};

/// The grade's buffer and bind group. The stack is compiled and written only when it differs
/// from the last frame's; the buffer grows (and its bind group is made again) when a program
/// does not fit.
pub(crate) struct GradeGpu {
    pub(crate) layout: wgpu::BindGroupLayout,
    buffer: wgpu::Buffer,
    pub(crate) group: wgpu::BindGroup,
    /// The stack the buffer holds the program of.
    held: Option<GradeStack>,
}

/// Bytes in one program word.
const WORD: u64 = 16;

impl GradeGpu {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("grade layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(WORD),
                },
                count: None,
            }],
        });
        // Room for a Basic panel and a Tone Curve before it first grows.
        let (buffer, group) = Self::make(device, &layout, 4 * (grade::CURVE_SAMPLES as u64 + 8));
        Self { layout, buffer, group, held: None }
    }

    fn make(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, words: u64) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("grade program"),
            size: words.max(1) * WORD,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("grade program"),
            layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
        });
        (buffer, group)
    }

    /// Makes the buffer hold `stack`'s program, when the composite runs it (the stack does
    /// something: see `Globals::new`).
    pub(crate) fn sync(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, stack: &GradeStack) {
        if stack.is_identity() || self.held.as_ref() == Some(stack) {
            return;
        }
        let program: Program = grade::compile(stack);
        let words = program.words.len() as u64;
        if words * WORD > self.buffer.size() {
            // Half again, so a growing stack doesn't remake it every frame.
            let (buffer, group) = Self::make(device, &self.layout, words + words / 2);
            self.buffer = buffer;
            self.group = group;
        }
        queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&program.words));
        self.held = Some(stack.clone());
    }
}
