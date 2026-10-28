//! The wall distance field: for every spot in the frame, how far it is to the nearest wall.
//!
//! The swarm, the liquid and bouncing particles all read it to stay out of walls. It is baked
//! on the GPU from the same wall pieces the CPU simulation uses: the CPU sorts pieces into
//! small tiles, and each tile only measures the pieces near it.

use super::util::{self, StorageBuffer};
use bpe_scene::{ObstaclePart, Vec2};
use bpe_sim::{Outline, Piece, Simulation, outline_pieces};
use bytemuck::{Pod, Zeroable};

/// Texels along the longer side of the frame.
pub const FIELD_LONG_SIDE: u32 = 768;
/// Texels per tile side. Matches `@workgroup_size` in sdf.wgsl.
const TILE: u32 = 8;
/// Farthest distance the field measures, in world pixels. Further away counts as this far.
pub const MAX_DISTANCE: f32 = 64.0;

/// One wall piece as the GPU sees it. Matches `struct Piece` in sdf.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct PieceGpu {
    pub a: [f32; 4],
    pub b: [f32; 4],
}

impl PieceGpu {
    pub fn segment(from: Vec2, to: Vec2, half_thickness: f32) -> Self {
        Self { a: [from.x, from.y, to.x, to.y], b: [0.0, half_thickness, 0.0, 0.0] }
    }

    pub fn arc(center: Vec2, radius: f32, start: f32, sweep: f32, half_thickness: f32) -> Self {
        Self { a: [center.x, center.y, radius, 0.0], b: [1.0, half_thickness, start, sweep] }
    }

    pub fn disc(center: Vec2, radius: f32) -> Self {
        Self { a: [center.x, center.y, radius, 0.0], b: [2.0, 0.0, 0.0, 0.0] }
    }

    /// Distance from `p` to the piece's surface, the same sum the GPU does.
    pub fn distance(&self, p: Vec2) -> f32 {
        let [x0, y0, x1, y1] = self.a;
        let [kind, half, start, sweep] = self.b;
        if kind < 0.5 {
            let (a, b) = (Vec2::new(x0, y0), Vec2::new(x1, y1));
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.length_squared().max(1e-8)).clamp(0.0, 1.0);
            (p - a - ab * t).length() - half
        } else if kind < 1.5 {
            let (c, r) = (Vec2::new(x0, y0), x1);
            let o = p - c;
            let along = (o.y.atan2(o.x) - start).rem_euclid(std::f32::consts::TAU);
            let d = if along <= sweep {
                (o.length() - r).abs()
            } else {
                let e0 = c + Vec2::from_angle(start) * r;
                let e1 = c + Vec2::from_angle(start + sweep) * r;
                p.distance(e0).min(p.distance(e1))
            };
            d - half
        } else {
            p.distance(Vec2::new(x0, y0)) - x1
        }
    }

    /// Centre and radius of a circle around the whole piece.
    fn bounds(&self) -> (Vec2, f32) {
        let [x0, y0, x1, y1] = self.a;
        let [kind, half, ..] = self.b;
        if kind < 0.5 {
            let (a, b) = (Vec2::new(x0, y0), Vec2::new(x1, y1));
            ((a + b) * 0.5, a.distance(b) * 0.5 + half)
        } else {
            (Vec2::new(x0, y0), x1 + half)
        }
    }
}

/// Every wall in the arena at one moment.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Walls {
    /// The closed arena that keeps everything inside, if there is one.
    pub closed: Option<Outline>,
    /// Open walls: rings with gaps, pegs and lines.
    pub pieces: Vec<PieceGpu>,
}

impl Walls {
    /// The walls of the simulation as it is now.
    pub fn of(sim: &Simulation) -> Self {
        let mut walls = Walls::default();
        let mut pieces = Vec::new();
        for (ring, outline) in sim.outlines() {
            if ring.is_none() {
                walls.closed = Some(outline);
                continue;
            }
            pieces.clear();
            outline_pieces(&outline, &mut pieces);
            let half = outline.thickness() * 0.5;
            walls.pieces.extend(pieces.iter().map(|piece| match *piece {
                Piece::Arc { center, radius, start, sweep } => PieceGpu::arc(center, radius, start, sweep, half),
                Piece::Segment { from, to } => PieceGpu::segment(from, to, half),
            }));
        }
        walls.pieces.extend(sim.obstacles().iter().filter_map(|part| match *part {
            ObstaclePart::Disc { center, radius, .. } => Some(PieceGpu::disc(center, radius)),
            ObstaclePart::Segment { from, to, thickness, .. } => Some(PieceGpu::segment(from, to, thickness * 0.5)),
            ObstaclePart::Shape { .. } => None,
        }));
        // A custom-shape arena and shape obstacles, as their outlines' straight pieces.
        sim.shape_pieces(|from, to, half| walls.pieces.push(PieceGpu::segment(from, to, half)));
        walls
    }

    pub fn is_empty(&self) -> bool {
        self.closed.is_none() && self.pieces.is_empty()
    }

    /// Distance from `p` to the nearest wall, positive in open space, as the field stores it.
    pub fn distance(&self, p: Vec2) -> f32 {
        let mut d = MAX_DISTANCE;
        for piece in &self.pieces {
            d = d.min(piece.distance(p));
        }
        if let Some(outline) = &self.closed {
            d = d.min(closed_distance(outline, p));
        }
        d.clamp(-MAX_DISTANCE, MAX_DISTANCE)
    }

    /// The closed arena as the bake shader's `closed_a` and `closed_b`.
    fn closed_uniform(&self) -> ([f32; 4], [f32; 4]) {
        match self.closed {
            None => ([0.0; 4], [0.0; 4]),
            Some(Outline::Ring { center, radius, sides, angle, thickness, .. }) => {
                let kind = if sides >= 3 { 2.0 } else { 1.0 };
                ([kind, thickness * 0.5, center.x, center.y], [radius, sides as f32, angle, 0.0])
            }
            Some(Outline::Box { center, half, angle, thickness }) => {
                ([3.0, thickness * 0.5, center.x, center.y], [half.x, half.y, angle, 0.0])
            }
        }
    }
}

/// Distance from `p` to a closed arena's wall, positive inside. Matches `closed_distance` in sdf.wgsl.
pub fn closed_distance(outline: &Outline, p: Vec2) -> f32 {
    use std::f32::consts::{PI, TAU};
    match *outline {
        Outline::Ring { center, radius, sides, angle, thickness, .. } => {
            let o = p - center;
            if sides < 3 {
                return radius - o.length() - thickness * 0.5;
            }
            let step = TAU / sides as f32;
            let apothem = radius * (PI / sides as f32).cos();
            let k = ((o.y.atan2(o.x) - angle) / step).round();
            let best =
                (-1..=1).map(|j| o.dot(Vec2::from_angle(angle + (k + j as f32) * step))).fold(f32::MIN, f32::max);
            apothem - best - thickness * 0.5
        }
        Outline::Box { center, half, angle, thickness } => {
            let x = Vec2::from_angle(angle);
            let o = p - center;
            let q = Vec2::new(o.dot(x), o.dot(x.perp())).abs() - half;
            let outside = q.max(Vec2::ZERO).length() + q.x.max(q.y).min(0.0);
            -outside - thickness * 0.5
        }
    }
}

/// Where the field sits in the world. Matches `struct SdfInfo` in common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct SdfInfo {
    pub origin: [f32; 4],
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

/// The field's size and placement for a video of the given size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub width: u32,
    pub height: u32,
    /// World position of the top-left corner.
    pub origin: Vec2,
    /// World pixels per texel.
    pub texel: f32,
}

impl Layout {
    pub fn for_video(video_width: u32, video_height: u32) -> Self {
        let (w, h) = (video_width.max(1) as f32, video_height.max(1) as f32);
        let texel = w.max(h) / FIELD_LONG_SIDE as f32;
        let width = (w / texel).ceil() as u32;
        let height = (h / texel).ceil() as u32;
        Self { width, height, origin: Vec2::new(-w * 0.5, -h * 0.5), texel }
    }

    fn tiles(&self) -> (u32, u32) {
        (self.width.div_ceil(TILE), self.height.div_ceil(TILE))
    }

    fn info(&self, has_walls: bool) -> SdfInfo {
        SdfInfo {
            origin: [self.origin.x, self.origin.y, self.texel, MAX_DISTANCE],
            size: [self.width as f32, self.height as f32, if has_walls { 1.0 } else { 0.0 }, 0.0],
        }
    }
}

/// Sorts wall pieces into the field's tiles: each tile gets the pieces that come within
/// [`MAX_DISTANCE`] of it. Returns where each tile's list starts (one extra entry at the end)
/// and the lists themselves.
pub fn bin_pieces(layout: &Layout, pieces: &[PieceGpu]) -> (Vec<u32>, Vec<u32>) {
    let (tiles_x, tiles_y) = layout.tiles();
    let tile_world = TILE as f32 * layout.texel;
    let half_diagonal = tile_world * std::f32::consts::FRAC_1_SQRT_2;
    let mut counts = vec![0u32; (tiles_x * tiles_y) as usize + 1];
    let mut pairs: Vec<(u32, u32)> = Vec::new();
    for (index, piece) in pieces.iter().enumerate() {
        let (center, radius) = piece.bounds();
        if !center.is_finite() || !radius.is_finite() {
            continue;
        }
        let reach = radius + MAX_DISTANCE + half_diagonal;
        let lo = ((center - Vec2::splat(reach) - layout.origin) / tile_world).floor();
        let hi = ((center + Vec2::splat(reach) - layout.origin) / tile_world).floor();
        let (x0, y0) = (lo.x.max(0.0) as u32, lo.y.max(0.0) as u32);
        let (x1, y1) = (hi.x.min(tiles_x as f32 - 1.0), hi.y.min(tiles_y as f32 - 1.0));
        if x1 < 0.0 || y1 < 0.0 {
            continue;
        }
        for ty in y0..=y1 as u32 {
            for tx in x0..=x1 as u32 {
                let tile_center = layout.origin + (Vec2::new(tx as f32, ty as f32) + 0.5) * tile_world;
                if piece.distance(tile_center) <= MAX_DISTANCE + half_diagonal {
                    let tile = ty * tiles_x + tx;
                    counts[tile as usize] += 1;
                    pairs.push((tile, index as u32));
                }
            }
        }
    }
    // Counting sort by tile, keeping pieces in their original order within each tile.
    let mut starts = Vec::with_capacity(counts.len());
    let mut total = 0;
    for &count in &counts {
        starts.push(total);
        total += count;
    }
    let mut cursor = starts.clone();
    let mut lists = vec![0u32; pairs.len()];
    for (tile, index) in pairs {
        let slot = &mut cursor[tile as usize];
        lists[*slot as usize] = index;
        *slot += 1;
    }
    (starts, lists)
}

/// The baked field and what is needed to read it.
pub struct SdfField {
    pub layout: Layout,
    pub view: wgpu::TextureView,
    pub info: wgpu::Buffer,
    /// The walls last baked into the texture, so unchanged walls are not baked again.
    baked: Option<Walls>,
}

pub struct SdfBaker {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    uniforms: StorageBuffer,
    pieces: StorageBuffer,
    starts: StorageBuffer,
    lists: StorageBuffer,
    /// Bytes already used in each buffer by bakes recorded since the last submit.
    used: [u64; 4],
    field: Option<SdfField>,
}

impl SdfBaker {
    pub fn new(device: &wgpu::Device) -> Self {
        use wgpu::ShaderStages as S;
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sdf bake layout"),
            entries: &[
                util::uniform_entry(0, S::COMPUTE, false),
                util::storage_entry(1, S::COMPUTE, true),
                util::storage_entry(2, S::COMPUTE, true),
                util::storage_entry(3, S::COMPUTE, true),
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
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
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = util::shader(device, "sdf bake", &[include_str!("../shaders/gpu/sdf.wgsl")]);
        let pipeline = util::compute_pipeline(device, "sdf bake", &pipeline_layout, &module, "bake_field");
        Self {
            pipeline,
            layout,
            uniforms: StorageBuffer::new(device, "sdf bake uniforms", wgpu::BufferUsages::UNIFORM),
            pieces: StorageBuffer::new(device, "sdf pieces", wgpu::BufferUsages::empty()),
            starts: StorageBuffer::new(device, "sdf tile starts", wgpu::BufferUsages::empty()),
            lists: StorageBuffer::new(device, "sdf tile lists", wgpu::BufferUsages::empty()),
            used: [0; 4],
            field: None,
        }
    }

    /// The field, once [`SdfBaker::bake`] has run.
    pub fn field(&self) -> Option<&SdfField> {
        self.field.as_ref()
    }

    /// Call after each submit: buffer space used by earlier bakes can be written again.
    pub fn begin_batch(&mut self) {
        self.used = [0; 4];
    }

    /// Bytes the next bake may need, so the caller can submit first when space runs short.
    pub fn batch_bytes(&self) -> u64 {
        self.used.iter().sum()
    }

    /// Makes sure the field texture exists for this video size.
    pub fn ensure_field(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, video: (u32, u32)) -> &SdfField {
        let layout = Layout::for_video(video.0, video.1);
        if self.field.as_ref().is_none_or(|f| f.layout != layout) {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("wall distance field"),
                size: wgpu::Extent3d { width: layout.width, height: layout.height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R32Float,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let info = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("sdf info"),
                size: std::mem::size_of::<SdfInfo>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            queue.write_buffer(&info, 0, bytemuck::bytes_of(&layout.info(false)));
            let view = texture.create_view(&Default::default());
            self.field = Some(SdfField { layout, view, info, baked: None });
        }
        self.field.as_ref().unwrap()
    }

    /// Records a bake of `walls` into `encoder`, unless the field already holds them.
    pub fn bake(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        walls: &Walls,
        video: (u32, u32),
    ) {
        self.ensure_field(device, queue, video);
        let field = self.field.as_mut().unwrap();
        if field.baked.as_ref() == Some(walls) {
            return;
        }
        // Once baked, the field always counts as holding walls, even empty ones (it then reads
        // far from everything). The flag is a buffer write, which lands before the whole batch
        // of steps runs, so it must not change between steps of one batch.
        if field.baked.is_none() {
            queue.write_buffer(&field.info, 0, bytemuck::bytes_of(&field.layout.info(true)));
        }
        field.baked = Some(walls.clone());
        let layout = field.layout;
        let (starts, lists) = bin_pieces(&layout, &walls.pieces);
        let (closed_a, closed_b) = walls.closed_uniform();
        let (tiles_x, tiles_y) = layout.tiles();
        let uniform = BakeUniform {
            origin: [layout.origin.x, layout.origin.y, layout.texel, MAX_DISTANCE],
            size: [layout.width, layout.height, tiles_x, 0],
            closed_a,
            closed_b,
        };
        // Bindings can't be empty: a closed arena with no other walls still binds one piece.
        let no_pieces = [PieceGpu::zeroed()];
        let pieces = if walls.pieces.is_empty() { &no_pieces[..] } else { &walls.pieces[..] };
        let parts: [&[u8]; 4] = [
            bytemuck::bytes_of(&uniform),
            bytemuck::cast_slice(pieces),
            bytemuck::cast_slice(&starts),
            bytemuck::cast_slice(&lists),
        ];
        let buffers = [&mut self.uniforms, &mut self.pieces, &mut self.starts, &mut self.lists];
        let mut offsets = [0u64; 4];
        for (i, (buffer, bytes)) in buffers.into_iter().zip(parts).enumerate() {
            offsets[i] = self.used[i];
            buffer.write(device, queue, offsets[i], bytes);
            self.used[i] = util::align(offsets[i] + bytes.len().max(4) as u64);
        }
        let sizes = parts.map(|p| p.len().max(4) as u64);
        let field = self.field.as_ref().unwrap();
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sdf bake"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.uniforms.binding(offsets[0], sizes[0]) },
                wgpu::BindGroupEntry { binding: 1, resource: self.pieces.binding(offsets[1], sizes[1]) },
                wgpu::BindGroupEntry { binding: 2, resource: self.starts.binding(offsets[2], sizes[2]) },
                wgpu::BindGroupEntry { binding: 3, resource: self.lists.binding(offsets[3], sizes[3]) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&field.view) },
            ],
        });
        let mut pass = encoder
            .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("sdf bake"), timestamp_writes: None });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(tiles_x, tiles_y, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_scene::Scene;

    #[test]
    fn layout_covers_the_frame() {
        let layout = Layout::for_video(1080, 1920);
        assert_eq!(layout.height, FIELD_LONG_SIDE);
        assert!(layout.width as f32 * layout.texel >= 1080.0);
        assert_eq!(layout.origin, Vec2::new(-540.0, -960.0));
        let square = Layout::for_video(1, 1);
        assert!(square.width >= 1 && square.height >= 1);
    }

    #[test]
    fn closed_arenas_are_positive_inside_and_negative_outside() {
        for scene in [Scene::default(), Scene::box_arena(), Scene::spinning_hexagon()] {
            let walls = Walls::of(&Simulation::new(&scene));
            let outline = walls.closed.expect("closed arena");
            let center = outline.center();
            assert!(walls.distance(center) > 0.0, "{}", scene.name);
            assert!(walls.distance(center + Vec2::new(0.0, 900.0)) < 0.0, "{}", scene.name);
        }
    }

    #[test]
    fn rings_and_pegs_become_pieces() {
        let walls = Walls::of(&Simulation::new(&Scene::escape_rings()));
        assert!(walls.closed.is_none());
        assert!(!walls.pieces.is_empty());
        let walls = Walls::of(&Simulation::new(&Scene::plinko()));
        let discs = walls.pieces.iter().filter(|p| p.b[0] == 2.0).count();
        assert!(discs > 20, "{discs}");
        // Right on a peg's edge the distance is zero; inside it is negative.
        let peg = walls.pieces.iter().find(|p| p.b[0] == 2.0).unwrap();
        let c = Vec2::new(peg.a[0], peg.a[1]);
        assert!(peg.distance(c + Vec2::new(peg.a[2], 0.0)).abs() < 1e-4);
        assert!(walls.distance(c) < 0.0);
    }

    #[test]
    fn binning_finds_every_piece_near_each_tile() {
        let walls = Walls::of(&Simulation::new(&Scene::plinko()));
        let layout = Layout::for_video(1080, 1920);
        let (starts, lists) = bin_pieces(&layout, &walls.pieces);
        let (tx, ty) = layout.tiles();
        assert_eq!(starts.len(), (tx * ty + 1) as usize);
        assert_eq!(*starts.last().unwrap() as usize, lists.len());
        // A texel's distance using only its tile's pieces matches using every piece.
        let tile_world = TILE as f32 * layout.texel;
        for (i, &(x, y)) in [(10u32, 10u32), (200, 300), (431, 767), (216, 384), (100, 700)].iter().enumerate() {
            let p = layout.origin + (Vec2::new(x as f32, y as f32) + 0.5) * layout.texel;
            let tile =
                ((p.y - layout.origin.y) / tile_world) as u32 * tx + ((p.x - layout.origin.x) / tile_world) as u32;
            let near = lists[starts[tile as usize] as usize..starts[tile as usize + 1] as usize]
                .iter()
                .map(|&k| walls.pieces[k as usize].distance(p))
                .fold(MAX_DISTANCE, f32::min);
            let all = walls.pieces.iter().map(|piece| piece.distance(p)).fold(MAX_DISTANCE, f32::min);
            assert_eq!(near, all, "texel {i}");
        }
    }

    #[test]
    fn binning_survives_empty_and_broken_pieces() {
        let layout = Layout::for_video(1080, 1920);
        let (starts, lists) = bin_pieces(&layout, &[]);
        assert!(lists.is_empty() && starts.iter().all(|&s| s == 0));
        let bad = [
            PieceGpu::segment(Vec2::splat(f32::NAN), Vec2::ZERO, 1.0),
            PieceGpu::disc(Vec2::new(1e9, 1e9), 4.0),
            PieceGpu::segment(Vec2::ZERO, Vec2::ZERO, 0.0),
        ];
        let (_, lists) = bin_pieces(&layout, &bad);
        assert!(lists.iter().all(|&k| k == 2), "only the zero-length segment lands in tiles");
    }
}
