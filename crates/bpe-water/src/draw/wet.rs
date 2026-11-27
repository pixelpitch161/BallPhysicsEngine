//! Wet marks (Water mode part 6b; the plan `docs/superpowers/plans/water-mode/part6-light-and-looks.md`,
//! decision 37): where the liquid has been. The only part of the look with a memory, so the water
//! keeps it: a [`WetField`] on the layout's nodes holds the water frame liquid first touched each
//! node ([`DRY`]: never), marked on the card after every frame (`shaders/wet.wgsl`). A solid near
//! a node wet by the moment drawn is drawn darker and glossier (its Material's Wet marks).
//!
//! The draft marks its own field as it plays (going back keeps it: a node first wet after the
//! playhead reads dry there); a final keeps the field next to each chunk of its take
//! (`take::chunk::write_wet`).

use crate::drops::DropBuffers;
use crate::solid::Layout;

/// A node liquid has never touched.
pub const DRY: u32 = u32::MAX;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct WetU {
    size: [u32; 4],
}

/// A layout's wet marks on the card.
pub struct WetField {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: Layout,
    buffer: wgpu::Buffer,
    numbers: wgpu::Buffer,
    pipeline: wgpu::ComputePipeline,
}

impl std::fmt::Debug for WetField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WetField({} x {})", self.layout.cols, self.layout.rows)
    }
}

impl WetField {
    /// `layout`'s nodes, all dry.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, layout: Layout) -> WetField {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("water wet marks"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/wet.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("water wet marks"),
            layout: None,
            module: &module,
            entry_point: Some("cs_wet"),
            compilation_options: Default::default(),
            cache: None,
        });
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("water wet marks"),
            size: (layout.nodes().max(1) * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let numbers = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("water wet numbers"),
            size: std::mem::size_of::<WetU>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let field = WetField { device: device.clone(), queue: queue.clone(), layout, buffer, numbers, pipeline };
        field.clear();
        field
    }

    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// The marks on the card, one `u32` a node (for the look drawing).
    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    /// Every node dry.
    pub fn clear(&self) {
        self.upload(&vec![DRY; self.layout.nodes()]);
    }

    /// Sets the marks (one a node; a list of another length leaves every node dry).
    pub fn upload(&self, marks: &[u32]) {
        if marks.len() != self.layout.nodes() {
            self.queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&vec![DRY; self.layout.nodes().max(1)]));
            return;
        }
        if !marks.is_empty() {
            self.queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(marks));
        }
    }

    /// Marks the four nodes round each liquid drop of `drops` (in this field's layout's grid units)
    /// with `frame`, where they hold a later one (sent to the card now, after what was sent before).
    pub fn mark(&self, drops: &DropBuffers, frame: u32) {
        if drops.count == 0 || self.layout.nodes() == 0 {
            return;
        }
        let u = WetU { size: [self.layout.cols, self.layout.rows, drops.count, frame] };
        self.queue.write_buffer(&self.numbers, 0, bytemuck::bytes_of(&u));
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("water wet marks"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.numbers.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: drops.xv.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: drops.id.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: self.buffer.as_entire_binding() },
            ],
        });
        let groups = drops.count.div_ceil(64);
        let (x, y) = (groups.min(65535), groups.div_ceil(65535));
        let mut encoder =
            self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("water wet marks") });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("water wet marks"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(x, y, 1);
        }
        self.queue.submit([encoder.finish()]);
    }

    /// The marks, read back (waits for the card).
    pub fn read(&self) -> Vec<u32> {
        let size = (self.layout.nodes() * 4) as u64;
        if size == 0 {
            return Vec::new();
        }
        let out = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("water wet marks read"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&self.buffer, 0, &out, 0, size);
        self.queue.submit([encoder.finish()]);
        let slice = out.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("the card");
        let data = slice.get_mapped_range().expect("mapped");
        bytemuck::cast_slice(&data).to_vec()
    }
}

/// `marks` as a take keeps them: one `u16` a node (little-endian), frames since the take's start
/// (at most 0xFFFE), 0xFFFF dry.
pub fn to_u16(marks: &[u32]) -> Vec<u8> {
    marks.iter().flat_map(|&m| if m == DRY { 0xFFFFu16 } else { m.min(0xFFFE) as u16 }.to_le_bytes()).collect()
}

/// The marks [`to_u16`] keeps.
pub fn from_u16(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(2)
        .map(|b| match u16::from_le_bytes([b[0], b[1]]) {
            0xFFFF => DRY,
            m => u32::from(m),
        })
        .collect()
}

/// `marks` with every node first wet at `frame` or later dry again (a remake from `frame` marks
/// them again).
pub fn dry_from(marks: &mut [u32], frame: u32) {
    for m in marks.iter_mut().filter(|m| **m >= frame) {
        *m = DRY;
    }
}

/// Wet marks read from a take, put on the card for a drawing (sent again only when they change).
#[derive(Debug, Default)]
pub struct HeldWet {
    field: Option<WetField>,
    held: Option<std::sync::Arc<Vec<u32>>>,
}

impl HeldWet {
    /// Gives `look` the marks `wet` (one a node of `layout`, read against its water frame), or
    /// none.
    pub fn give(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: Layout,
        wet: Option<&(std::sync::Arc<Vec<u32>>, u32)>,
        look: &mut crate::draw::LookDraw,
    ) {
        let Some((marks, frame)) = wet else {
            look.set_wet(None, 0);
            return;
        };
        if self.field.as_ref().is_none_or(|f| f.layout() != layout) {
            self.field = Some(WetField::new(device, queue, layout));
            self.held = None;
        }
        let field = self.field.as_ref().expect("just made");
        if self.held.as_ref().is_none_or(|h| !std::sync::Arc::ptr_eq(h, marks)) {
            field.upload(marks);
            self.held = Some(marks.clone());
        }
        look.set_wet(Some(field), *frame);
    }
}
