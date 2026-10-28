//! The swarm: up to a million points moved by compute shaders and drawn as glowing dots.

use super::sdf::SdfField;
use super::util::{self, groups};
use bpe_scene::{Scene, SwarmColor, SwarmStart};
use bytemuck::{Pod, Zeroable};

/// Size of a flocking grid cell in world pixels.
const CELL: f32 = 24.0;
const MAX_STOPS: usize = 6;

/// Matches `struct Params` in swarm.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
struct SwarmParams {
    counts: [u32; 4],
    motion: [f32; 4],
    steer: [f32; 4],
    more: [f32; 4],
    look: [u32; 4],
    arena: [f32; 4],
    draw: [f32; 4],
    colors: [[f32; 4]; MAX_STOPS],
}

impl SwarmParams {
    fn new(scene: &Scene, points: u32) -> Self {
        let s = &scene.gpu.swarm;
        let (cells_x, cells_y) = cells(scene);
        let mut colors = [[1.0; 4]; MAX_STOPS];
        let stops = s.colors.len().clamp(1, MAX_STOPS);
        for (slot, c) in colors.iter_mut().zip(&s.colors) {
            *slot = c.to_linear();
        }
        let mode = match s.color {
            SwarmColor::Speed => 0,
            SwarmColor::NearestBall => 1,
            SwarmColor::Gradient => 2,
        };
        let start = match s.start {
            SwarmStart::Fill => 0,
            SwarmStart::Center => 1,
            SwarmStart::Ring => 2,
        };
        let center = scene.container.center();
        Self {
            counts: [points, cells_x, cells_y, (s.seed ^ (s.seed >> 32)) as u32],
            motion: [s.max_speed.max(1.0), s.drag.max(0.0), s.flow.max(0.0), s.flow_scale.max(1.0)],
            steer: [s.flow_speed.max(0.0), s.flock.max(0.0), s.separation.max(0.0), s.attraction],
            more: [s.reach.max(0.0), s.avoid_walls.max(0.0), s.gravity, CELL],
            look: [mode, stops as u32, start, 0],
            arena: [center.x, center.y, scene.container.size().max(1.0), s.size.max(0.0)],
            draw: [s.brightness.max(0.0), 0.0, 0.0, 0.0],
            colors,
        }
    }
}

fn cells(scene: &Scene) -> (u32, u32) {
    let (w, h) = (scene.video.width.max(1) as f32, scene.video.height.max(1) as f32);
    ((w / CELL).ceil() as u32, (h / CELL).ceil() as u32)
}

/// A copy of the swarm's points at one step.
pub struct Snapshot {
    points: wgpu::Buffer,
    tints: wgpu::Buffer,
}

pub struct SwarmSim {
    layout: wgpu::BindGroupLayout,
    draw_layout: wgpu::BindGroupLayout,
    init: wgpu::ComputePipeline,
    clear: wgpu::ComputePipeline,
    fill: wgpu::ComputePipeline,
    advance: wgpu::ComputePipeline,
    draw: wgpu::RenderPipeline,
    params: wgpu::Buffer,
    draw_params: wgpu::Buffer,
    points: Option<wgpu::Buffer>,
    tints: Option<wgpu::Buffer>,
    field: Option<wgpu::Buffer>,
    count: u32,
    field_cells: u32,
    group: Option<wgpu::BindGroup>,
    draw_group: Option<wgpu::BindGroup>,
    last_params: Option<SwarmParams>,
}

impl SwarmSim {
    pub fn new(device: &wgpu::Device, inputs_layout: &wgpu::BindGroupLayout, hdr_format: wgpu::TextureFormat) -> Self {
        use wgpu::ShaderStages as S;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("swarm layout"),
            entries: &[
                util::uniform_entry(0, S::COMPUTE, false),
                util::storage_entry(1, S::COMPUTE, false),
                util::storage_entry(2, S::COMPUTE, false),
                util::storage_entry(3, S::COMPUTE, false),
                util::float_texture_entry(4, S::COMPUTE, false),
                util::uniform_entry(5, S::COMPUTE, false),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("swarm"),
            bind_group_layouts: &[Some(inputs_layout), Some(&layout)],
            immediate_size: 0,
        });
        let module = util::shader(
            device,
            "swarm",
            &[util::COMMON_WGSL, super::STEP_WGSL, include_str!("../shaders/gpu/swarm.wgsl")],
        );
        let compute = |entry| util::compute_pipeline(device, entry, &pipeline_layout, &module, entry);

        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("swarm draw layout"),
            entries: &[
                util::uniform_entry(0, S::VERTEX, false),
                util::storage_entry(1, S::VERTEX, true),
                util::storage_entry(2, S::VERTEX, true),
            ],
        });
        let globals = util::globals_layout(device);
        let draw_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("swarm draw"),
            bind_group_layouts: &[Some(&globals), Some(&draw_layout)],
            immediate_size: 0,
        });
        let draw_module = util::shader(device, "swarm draw", &[include_str!("../shaders/gpu/swarm_draw.wgsl")]);
        let draw = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("swarm draw"),
            layout: Some(&draw_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &draw_module,
                entry_point: Some("vs_point"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &draw_module,
                entry_point: Some("fs_point"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: hdr_format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::One,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent::OVER,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniform = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: std::mem::size_of::<SwarmParams>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        Self {
            init: compute("init_points"),
            clear: compute("clear_field"),
            fill: compute("fill_field"),
            advance: compute("move_points"),
            layout,
            draw_layout,
            draw,
            params: uniform("swarm params"),
            draw_params: uniform("swarm draw params"),
            points: None,
            tints: None,
            field: None,
            count: 0,
            field_cells: 0,
            group: None,
            draw_group: None,
            last_params: None,
        }
    }

    /// Bytes of GPU memory one checkpoint takes.
    pub fn state_bytes(&self) -> u64 {
        self.count as u64 * 24
    }

    pub fn forget_bindings(&mut self) {
        self.group = None;
    }

    /// Makes fresh points for the scene's swarm at its starting places.
    pub fn reset(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        inputs: &wgpu::BindGroup,
        scene: &Scene,
        sdf: &SdfField,
    ) {
        let count = scene.gpu.swarm.points();
        let (cx, cy) = cells(scene);
        if count != self.count || self.points.is_none() {
            let storage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST;
            let make = |label, size: u64| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some(label),
                    size: size.max(16),
                    usage: storage,
                    mapped_at_creation: false,
                })
            };
            self.points = Some(make("swarm points", count as u64 * 16));
            self.tints = Some(make("swarm colours", count as u64 * 8));
            self.count = count;
            self.group = None;
            self.draw_group = None;
        }
        if cx * cy != self.field_cells || self.field.is_none() {
            self.field = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("swarm grid"),
                size: (cx * cy * 3 * 4) as u64,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }));
            self.field_cells = cx * cy;
            self.group = None;
        }
        let params = SwarmParams::new(scene, count);
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
        self.ensure_group(device, sdf);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("swarm start"), timestamp_writes: None });
        pass.set_pipeline(&self.init);
        pass.set_bind_group(0, inputs, &[0]);
        pass.set_bind_group(1, self.group.as_ref().unwrap(), &[]);
        pass.dispatch_workgroups(groups(count, 256), 1, 1);
    }

    fn ensure_group(&mut self, device: &wgpu::Device, sdf: &SdfField) {
        if self.group.is_some() {
            return;
        }
        self.group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("swarm"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: self.points.as_ref().unwrap().as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: self.tints.as_ref().unwrap().as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: self.field.as_ref().unwrap().as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&sdf.view) },
                wgpu::BindGroupEntry { binding: 5, resource: sdf.info.as_entire_binding() },
            ],
        }));
    }

    /// Records one step, reading the step inputs at `offset`.
    pub fn step(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        inputs: &wgpu::BindGroup,
        offset: u32,
        sdf: &SdfField,
    ) {
        self.ensure_group(device, sdf);
        let flocking = self.last_params.is_none_or(|p| p.steer[1] > 0.0 || p.steer[2] > 0.0);
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("swarm step"), timestamp_writes: None });
        pass.set_bind_group(0, inputs, &[offset]);
        pass.set_bind_group(1, self.group.as_ref().unwrap(), &[]);
        if flocking {
            pass.set_pipeline(&self.clear);
            pass.dispatch_workgroups(groups(self.field_cells * 3, 256), 1, 1);
            pass.set_pipeline(&self.fill);
            pass.dispatch_workgroups(groups(self.count, 256), 1, 1);
        }
        pass.set_pipeline(&self.advance);
        pass.dispatch_workgroups(groups(self.count, 256), 1, 1);
    }

    /// Settings can change without restarting (only looks), so the params are written each frame.
    pub fn prepare_draw(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, scene: &Scene, _video: (u32, u32)) {
        let params = SwarmParams::new(scene, self.count);
        if self.last_params != Some(params) {
            queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&params));
            queue.write_buffer(&self.draw_params, 0, bytemuck::bytes_of(&params));
            self.last_params = Some(params);
        }
        if self.draw_group.is_none() {
            self.draw_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("swarm draw"),
                layout: &self.draw_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: self.draw_params.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: self.points.as_ref().unwrap().as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: self.tints.as_ref().unwrap().as_entire_binding() },
                ],
            }));
        }
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass<'_>, globals: &wgpu::BindGroup) {
        let Some(group) = &self.draw_group else { return };
        if self.count == 0 || self.last_params.is_none_or(|p| p.draw[0] <= 0.0) {
            return;
        }
        pass.set_pipeline(&self.draw);
        pass.set_bind_group(0, globals, &[]);
        pass.set_bind_group(1, group, &[]);
        pass.draw(0..4, 0..self.count);
    }

    pub fn snapshot(&self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder) -> Snapshot {
        let mut copy = |source: &wgpu::Buffer, label| {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: source.size(),
                usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            encoder.copy_buffer_to_buffer(source, 0, &buffer, 0, source.size());
            buffer
        };
        Snapshot {
            points: copy(self.points.as_ref().unwrap(), "swarm checkpoint"),
            tints: copy(self.tints.as_ref().unwrap(), "swarm checkpoint colours"),
        }
    }

    pub fn restore(&mut self, encoder: &mut wgpu::CommandEncoder, snapshot: &Snapshot) {
        let (points, tints) = (self.points.as_ref().unwrap(), self.tints.as_ref().unwrap());
        if snapshot.points.size() == points.size() {
            encoder.copy_buffer_to_buffer(&snapshot.points, 0, points, 0, points.size());
            encoder.copy_buffer_to_buffer(&snapshot.tints, 0, tints, 0, tints.size());
        }
    }

    /// Reads the points back to the CPU, for tests.
    #[cfg(test)]
    pub fn read_points(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<bpe_scene::Vec2> {
        let source = self.points.as_ref().unwrap();
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("swarm readback"),
            size: source.size(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(source, 0, &buffer, 0, source.size());
        queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let data = buffer.slice(..).get_mapped_range().unwrap();
        let values: &[[f32; 4]] = bytemuck::cast_slice(&data);
        values.iter().take(self.count as usize).map(|v| bpe_scene::Vec2::new(v[0], v[1])).collect()
    }
}
