//! A body's moving parts (Water mode part 5): the still field and the parts' own grids on the card,
//! where each part is now and where it will be at the coming frame's end, and the frame's numbers
//! for them (`movers.wgsl`). The body lays them over the still field at each step's end in a frame
//! where they move ([`super::Water::play_frame`]).

use bytemuck::{Pod, Zeroable};
use glam::{UVec2, Vec2};

use super::buffers::STORE;
use super::{FRAME, WORKGROUP, Water};
use crate::parts::{MOST_MOVERS, Movers, Pose};
use wgpu::util::DeviceExt;

/// A moving part as a frame sees it. Matches `Mover` in common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub(super) struct MoverGpu {
    a: [f32; 4],
    b: [f32; 4],
    e: [f32; 4],
    g: [f32; 4],
    h: [u32; 4],
}

/// The body's moving parts.
#[derive(Clone)]
pub(super) struct Moving {
    pub(super) movers: Movers,
    /// Where each is now (at the boundary the body is at), and at the coming frame's end.
    pub(super) poses: Vec<Pose>,
    pub(super) ends: Vec<Pose>,
    /// Each part's shape as the card takes it (`g` and `h`).
    shapes: Vec<([f32; 4], [u32; 4])>,
}

/// The furthest a field says on the card (cells).
const FAR_CELLS: f32 = 64.0;

impl Moving {
    /// The frame's numbers: 1 when the parts move between `poses` and `ends`, how many there are,
    /// the nodes their moves can change, and each part.
    pub(super) fn numbers(&self, layout: &crate::solid::Layout) -> ([u32; 4], [u32; 4], [MoverGpu; MOST_MOVERS]) {
        let mut out = [MoverGpu::default(); MOST_MOVERS];
        let cell = layout.cell;
        for (k, (g, h)) in self.shapes.iter().enumerate().take(MOST_MOVERS) {
            let (a, b) = (self.poses[k], self.ends[k]);
            let grid = |p: Pose| [layout.to_grid(p.at).x, layout.to_grid(p.at).y, p.turn, p.scale];
            let speed = a.speed_to(b, FRAME);
            let v = speed.at / cell;
            out[k] = MoverGpu { a: grid(a), b: grid(b), e: [v.x, v.y, speed.turn, speed.scale], g: *g, h: *h };
        }
        let moved = self.moved_box();
        let (on, (lo, hi)) = match moved {
            Some(b) => (1, b),
            None => (0, (UVec2::ZERO, UVec2::ZERO)),
        };
        ([on, self.shapes.len().min(MOST_MOVERS) as u32, 0, 0], [lo.x, lo.y, hi.x, hi.y], out)
    }

    /// The nodes the coming frame's moves can change, if any part moves.
    pub(super) fn moved_box(&self) -> Option<(UVec2, UVec2)> {
        self.movers.moved_box(&self.poses, &self.ends)
    }

    /// How fast the fastest point of a part moves in the coming frame (pixels a second).
    pub(super) fn fastest(&self) -> f32 {
        self.movers.fastest(&self.poses, &self.ends, FRAME)
    }
    /// Each part's pose a share `t` of the way through the coming frame (where it was keyed at 1) and
    /// its speed over the frame.
    pub(super) fn at_share(&self, t: f32) -> Vec<(Pose, Pose)> {
        let at = |a: Pose, b: Pose| if t >= 1.0 { b } else { Pose::lerp(a, b, t) };
        self.poses.iter().zip(&self.ends).map(|(a, b)| (at(*a, *b), a.speed_to(*b, FRAME))).collect()
    }
}

impl Water {
    /// Gives the body the course's moving parts (at most [`MOST_MOVERS`]; the field it was made on
    /// must be `movers.still`), each at its pose in `poses` (a missing one at its default), and lays
    /// them over the still field on the card there, with the deep water's solids worked out again: a
    /// fresh body and one made from a restart point both start from the card's own field. With no
    /// parts, nothing changes.
    pub fn set_movers(&mut self, movers: Movers, poses: &[Pose]) {
        if movers.is_empty() {
            return;
        }
        let mut movers = movers;
        movers.movers.truncate(MOST_MOVERS);
        let layout = self.layout;
        let cell = layout.cell;
        let n = layout.nodes();
        let mut still = movers.still.for_card();
        still.truncate(2 * n);
        let mut grids: Vec<f32> = Vec::new();
        let mut shapes = Vec::with_capacity(movers.len());
        for m in &movers.movers {
            let s = &m.shape;
            let at = grids.len() as u32;
            grids.extend(s.distance.iter().map(|d| (d / cell).clamp(-FAR_CELLS, FAR_CELLS)));
            let origin: Vec2 = s.origin / cell;
            shapes.push(([origin.x, origin.y, s.radius / cell, m.wet()], [s.cols, s.rows, at, 0]));
        }
        let device = &self.gpu.device;
        self.still_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("water still field"),
            contents: bytemuck::cast_slice(&still),
            usage: STORE,
        });
        if grids.is_empty() {
            grids.push(FAR_CELLS);
        }
        self.shapes_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("water moving parts"),
            contents: bytemuck::cast_slice(&grids),
            usage: STORE,
        });
        let poses: Vec<Pose> = (0..movers.len()).map(|k| poses.get(k).copied().unwrap_or_default().sane()).collect();
        self.moving = Some(Moving { movers, ends: poses.clone(), poses, shapes });
        self.bind();
        self.lay_movers_everywhere();
    }

    /// Lays the moving parts at their poses now over the whole still field on the card, and works out
    /// the deep water's solids everywhere.
    pub(super) fn lay_movers_everywhere(&mut self) {
        let Some(moving) = &self.moving else { return };
        let gpu = self.gpu.clone();
        let (mut mv, _, movers) = moving.numbers(&self.layout);
        mv[0] = 1;
        let mut frame = self.frame_gpu(FRAME / self.numbers.floor.max(1) as f32, false, 0.0);
        frame.mv = mv;
        frame.mbox = [0, 0, self.layout.cols - 1, self.layout.rows - 1];
        frame.movers = movers;
        gpu.queue.write_buffer(&self.frame_uniform, 0, bytemuck::bytes_of(&frame));
        let mut step = [0u8; super::passes::STEP_BYTES as usize];
        step[16..20].copy_from_slice(&1.0f32.to_le_bytes());
        gpu.queue.write_buffer(&self.step_uniform, 0, &step);
        let mut encoder =
            gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("water movers") });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("water movers"),
                timestamp_writes: None,
            });
            let group = |p: super::P| &self.groups[(p as usize * 2 + self.now) * 2];
            pass.set_pipeline(&gpu.pipes.list[super::P::Movers as usize].0);
            pass.set_bind_group(0, group(super::P::Movers), &[0]);
            pass.dispatch_workgroups(self.layout.cols.div_ceil(16), self.layout.rows.div_ceil(16), 1);
            pass.set_pipeline(&gpu.pipes.list[super::P::DeepWalls as usize].0);
            pass.set_bind_group(0, group(super::P::DeepWalls), &[0]);
            pass.dispatch_workgroups((self.layout.nodes() as u32).div_ceil(WORKGROUP).max(1), 1, 1);
        }
        gpu.queue.submit([encoder.finish()]);
    }

    /// Where each moving part is at the coming frame's end (from the next frame; it starts the frame
    /// where it is now). A missing one stays where it is.
    pub fn move_to(&mut self, ends: &[Pose]) {
        if let Some(m) = &mut self.moving {
            for (k, end) in m.ends.iter_mut().enumerate() {
                *end = ends.get(k).copied().map_or(m.poses[k], Pose::sane);
            }
        }
    }

    /// Each moving part's pose a share `t` through the coming frame and its speed over it (none
    /// without parts).
    pub(super) fn poses_at_share(&self, t: f32) -> Vec<(Pose, Pose)> {
        self.moving.as_ref().map_or_else(Vec::new, |m| m.at_share(t))
    }

    /// Where each moving part is now (at the boundary the body is at).
    pub fn poses(&self) -> &[Pose] {
        self.moving.as_ref().map_or(&[], |m| &m.poses)
    }

    /// The course's moving parts, if it has any.
    pub fn movers(&self) -> Option<&Movers> {
        self.moving.as_ref().map(|m| &m.movers)
    }

    /// The field on the card now: the distances (cells), the Wettings, and each node's moving part
    /// (its number plus 1). Waits for the card: tests only.
    #[doc(hidden)]
    pub fn solid_field_now(&self) -> Vec<f32> {
        let bytes = (3 * self.layout.nodes() * 4) as u64;
        bytemuck::pod_collect_to_vec(&self.read_buffer(&self.solid, bytes))
    }
}

/// A placeholder for a body with no moving parts (a binding must have a buffer).
pub(super) fn empty(device: &wgpu::Device, label: &'static str) -> wgpu::Buffer {
    super::buffers::storage(device, label, 256)
}
