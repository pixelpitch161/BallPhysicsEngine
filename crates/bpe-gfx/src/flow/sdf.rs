//! The wall distance field: for every spot near the wall, how far it is to the wall's inner
//! face, positive in open space and negative inside the wall.
//!
//! The field is baked once on the GPU in the wall's own frame: centre at the origin, unturned,
//! at the size the wall had then. Every step then tells the shaders where the wall is now and
//! how much it has grown ([`WallFrame`]), so one bake serves a wall that spins and shrinks.

use bpe_runtime::Outline;
use bytemuck::{Pod, Zeroable};
use glam::Vec2;

use super::util;

/// Texels along the longer side of the field.
pub const FIELD_LONG_SIDE: u32 = 768;
/// Texels per tile side. Matches `@workgroup_size` in sdf.wgsl.
const TILE: u32 = 8;
/// Farthest distance the field measures, in world pixels. Further away counts as this far.
pub const MAX_DISTANCE: f32 = 64.0;

/// Where the field sits in the wall's frame. Matches `struct SdfInfo` in common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct SdfInfo {
    /// Corner of texel (0, 0), texel size, largest distance.
    pub origin: [f32; 4],
    /// Size in texels, 1 when the field holds a wall.
    pub size: [f32; 4],
}

/// Matches `struct Bake` in sdf.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct BakeUniform {
    origin: [f32; 4],
    size: [u32; 4],
    closed_a: [f32; 4],
    closed_b: [f32; 4],
}

/// The live wall, as the step header carries it to the shaders.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallFrame {
    /// Centre, cos and sin of the wall's angle.
    pub wall: [f32; 4],
    /// The live wall's size over the baked one, then 1 when there is a wall.
    pub more: [f32; 4],
}

impl Default for WallFrame {
    fn default() -> Self {
        Self { wall: [0.0, 0.0, 1.0, 0.0], more: [1.0, 0.0, 0.0, 0.0] }
    }
}

/// The field's size and placement in the wall's frame.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placement {
    width: u32,
    height: u32,
    /// Position of the top-left corner.
    origin: Vec2,
    /// World pixels per texel.
    texel: f32,
}

impl Placement {
    /// Covers the (normalized) outline plus [`MAX_DISTANCE`] all round.
    fn for_outline(outline: &Outline) -> Self {
        let half = match *outline {
            Outline::Ring { radius, thickness, .. } => Vec2::splat(radius + thickness * 0.5 + MAX_DISTANCE),
            Outline::Box { half, thickness, .. } | Outline::Rounded { half, thickness, .. } => {
                half + thickness * 0.5 + MAX_DISTANCE
            }
        };
        let half = half.abs().max(Vec2::splat(1.0));
        let texel = (2.0 * half.max_element() / FIELD_LONG_SIDE as f32).max(1e-3);
        Self {
            width: ((2.0 * half.x / texel).ceil() as u32).max(1),
            height: ((2.0 * half.y / texel).ceil() as u32).max(1),
            origin: -half,
            texel,
        }
    }

    fn tiles(&self) -> (u32, u32) {
        (self.width.div_ceil(TILE), self.height.div_ceil(TILE))
    }

    fn info(&self, has_wall: bool) -> SdfInfo {
        SdfInfo {
            origin: [self.origin.x, self.origin.y, self.texel, MAX_DISTANCE],
            size: [self.width as f32, self.height as f32, if has_wall { 1.0 } else { 0.0 }, 0.0],
        }
    }
}

/// The outline moved to the origin and unturned: what the field is baked from.
pub fn normalized(outline: &Outline) -> Outline {
    match *outline {
        Outline::Ring { radius, sides, thickness, .. } => {
            Outline::Ring { center: Vec2::ZERO, radius, sides, angle: 0.0, thickness }
        }
        Outline::Box { half, thickness, .. } => Outline::Box { center: Vec2::ZERO, half, angle: 0.0, thickness },
        Outline::Rounded { half, corner, thickness, .. } => {
            Outline::Rounded { center: Vec2::ZERO, half, corner, angle: 0.0, thickness }
        }
    }
}

/// The length that grows when the wall grows: a ring's radius or a box's half width.
fn size_of(outline: &Outline) -> f32 {
    match *outline {
        Outline::Ring { radius, .. } => radius,
        Outline::Box { half, .. } | Outline::Rounded { half, .. } => half.x,
    }
}

fn angle_of(outline: &Outline) -> f32 {
    match *outline {
        Outline::Ring { angle, .. } | Outline::Box { angle, .. } | Outline::Rounded { angle, .. } => angle,
    }
}

/// The closed wall as the bake shader's `closed_a` and `closed_b`.
fn closed_uniform(outline: &Outline) -> ([f32; 4], [f32; 4]) {
    match *outline {
        Outline::Ring { center, radius, sides, angle, thickness } => {
            let kind = if sides >= 3 { 2.0 } else { 1.0 };
            ([kind, thickness * 0.5, center.x, center.y], [radius, sides as f32, angle, 0.0])
        }
        Outline::Box { center, half, angle, thickness } => {
            ([3.0, thickness * 0.5, center.x, center.y], [half.x, half.y, angle, 0.0])
        }
        Outline::Rounded { center, half, corner, angle, thickness } => {
            ([4.0, thickness * 0.5, center.x, center.y], [half.x, half.y, angle, corner])
        }
    }
}

/// The baked field, its placement and the bind group the layer shaders read it through
/// (binding 0 the [`SdfInfo`] uniform, binding 1 the `r32float` texture).
pub struct Sdf {
    pipeline: wgpu::ComputePipeline,
    bake_layout: wgpu::BindGroupLayout,
    bake_uniform: wgpu::Buffer,
    pub layout: wgpu::BindGroupLayout,
    info: wgpu::Buffer,
    texture: wgpu::Texture,
    group: wgpu::BindGroup,
    placement: Placement,
    /// The normalized outline the texture holds.
    baked: Option<Outline>,
}

impl Sdf {
    pub fn new(device: &wgpu::Device) -> Self {
        use wgpu::ShaderStages as S;
        let bake_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sdf bake layout"),
            entries: &[
                util::uniform_entry(0, S::COMPUTE, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: S::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::R32Float,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sdf bake"),
            bind_group_layouts: &[Some(&bake_layout)],
            immediate_size: 0,
        });
        let module = util::shader(device, "sdf bake", &[include_str!("../shaders/flow/sdf.wgsl")]);
        let pipeline = util::compute_pipeline(device, "sdf bake", &pipeline_layout, &module, "bake_field");
        let bake_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sdf bake uniform"),
            size: std::mem::size_of::<BakeUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sdf layout"),
            entries: &[
                util::uniform_entry(0, S::COMPUTE | S::FRAGMENT, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: S::COMPUTE | S::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let info = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sdf info"),
            size: std::mem::size_of::<SdfInfo>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let placement = Placement { width: 1, height: 1, origin: Vec2::ZERO, texel: 1.0 };
        let texture = Self::texture(device, &placement);
        let group = Self::group(device, &layout, &info, &texture);
        Self { pipeline, bake_layout, bake_uniform, layout, info, texture, group, placement, baked: None }
    }

    fn texture(device: &wgpu::Device, placement: &Placement) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("wall distance field"),
            size: wgpu::Extent3d { width: placement.width, height: placement.height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    }

    fn group(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        info: &wgpu::Buffer,
        texture: &wgpu::Texture,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sdf"),
            layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: info.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&texture.create_view(&Default::default())),
                },
            ],
        })
    }

    /// The bind group the layer shaders read the field through.
    pub fn bind_group(&self) -> &wgpu::BindGroup {
        &self.group
    }

    /// Records a bake of `outline` (moved to the origin and unturned) into `encoder`. The
    /// texture is remade when the outline needs a different size.
    pub fn bake(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        outline: &Outline,
    ) {
        let outline = normalized(outline);
        let placement = Placement::for_outline(&outline);
        if placement != self.placement {
            self.placement = placement;
            self.texture = Self::texture(device, &placement);
            self.group = Self::group(device, &self.layout, &self.info, &self.texture);
        }
        queue.write_buffer(&self.info, 0, bytemuck::bytes_of(&placement.info(true)));
        let (closed_a, closed_b) = closed_uniform(&outline);
        let (tiles_x, tiles_y) = placement.tiles();
        let uniform = BakeUniform {
            origin: [placement.origin.x, placement.origin.y, placement.texel, MAX_DISTANCE],
            size: [placement.width, placement.height, tiles_x, 0],
            closed_a,
            closed_b,
        };
        queue.write_buffer(&self.bake_uniform, 0, bytemuck::bytes_of(&uniform));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sdf bake"),
            layout: &self.bake_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.bake_uniform.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&self.texture.create_view(&Default::default())),
                },
            ],
        });
        let mut pass = encoder
            .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("sdf bake"), timestamp_writes: None });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(tiles_x, tiles_y, 1);
        drop(pass);
        self.baked = Some(outline);
    }

    /// Forgets the wall: the shaders then read open space everywhere.
    pub fn clear(&mut self, queue: &wgpu::Queue) {
        queue.write_buffer(&self.info, 0, bytemuck::bytes_of(&self.placement.info(false)));
        self.baked = None;
    }

    /// Where the live wall is now, relative to the bake.
    pub fn frame(&self, live: Option<&Outline>) -> WallFrame {
        match (live, &self.baked) {
            (Some(live), Some(baked)) => {
                let center = live.center();
                let angle = angle_of(live);
                let scale = size_of(live) / size_of(baked);
                let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
                WallFrame { wall: [center.x, center.y, angle.cos(), angle.sin()], more: [scale, 1.0, 0.0, 0.0] }
            }
            _ => WallFrame::default(),
        }
    }

    /// Copies the field back to the CPU, for tests.
    #[cfg(test)]
    pub fn read_back(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> anyhow::Result<Field> {
        let p = self.placement;
        let row = p.width as usize * 4;
        let padded_row = row.next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sdf readback"),
            size: (padded_row * p.height as usize) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row as u32),
                    rows_per_image: Some(p.height),
                },
            },
            wgpu::Extent3d { width: p.width, height: p.height, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        let (sender, receiver) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        device.poll(wgpu::PollType::wait_indefinitely())?;
        receiver.recv()??;
        let mapped = slice.get_mapped_range()?;
        let mut values = Vec::with_capacity((p.width * p.height) as usize);
        for chunk in mapped.chunks(padded_row) {
            values.extend(bytemuck::cast_slice::<u8, f32>(&chunk[..row]));
        }
        drop(mapped);
        buffer.unmap();
        Ok(Field { placement: p, values })
    }
}

/// The field on the CPU, read like `sdf_at` in common.wgsl does (in the wall's frame).
#[cfg(test)]
pub struct Field {
    placement: Placement,
    values: Vec<f32>,
}

#[cfg(test)]
impl Field {
    fn texel(&self, x: i32, y: i32) -> f32 {
        let p = &self.placement;
        let x = x.clamp(0, p.width as i32 - 1) as usize;
        let y = y.clamp(0, p.height as i32 - 1) as usize;
        self.values[y * p.width as usize + x]
    }

    pub fn sample(&self, local: Vec2) -> f32 {
        let p = &self.placement;
        let q = (local - p.origin) / p.texel - 0.5;
        let i = q.floor();
        let f = q - i;
        let (x, y) = (i.x as i32, i.y as i32);
        let top = self.texel(x, y) * (1.0 - f.x) + self.texel(x + 1, y) * f.x;
        let bottom = self.texel(x, y + 1) * (1.0 - f.x) + self.texel(x + 1, y + 1) * f.x;
        top * (1.0 - f.y) + bottom * f.y
    }
}
