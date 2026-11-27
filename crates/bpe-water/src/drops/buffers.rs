//! The drops' buffers, stored by field: each holds at most 16 bytes a drop, so a body of
//! [`MOST_DROPS`] stays within the 128 MiB a storage buffer may be bound with on every card.

use super::MOST_DROPS;

pub(super) const STORE: wgpu::BufferUsages =
    wgpu::BufferUsages::STORAGE.union(wgpu::BufferUsages::COPY_DST).union(wgpu::BufferUsages::COPY_SRC);

/// The fewest drops a body has room for.
const LEAST_ROOM: u32 = 4096;

pub(super) fn storage(device: &wgpu::Device, label: &'static str, bytes: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: bytes.max(256),
        usage: STORE,
        mapped_at_creation: false,
    })
}

/// Room for `n` drops: a power of two, at least [`LEAST_ROOM`], at most [`MOST_DROPS`].
pub(super) fn room_for(n: u32) -> u32 {
    n.max(LEAST_ROOM).next_power_of_two().min(MOST_DROPS)
}

/// The buffers sized by how many drops there is room for.
pub(super) struct ByDrop {
    /// Kept from step to step: place and speed; which way the liquid lies and how full.
    pub(super) xv: wgpu::Buffer,
    pub(super) nf: wgpu::Buffer,
    /// Kind, serial, colour: twice, as each step sorts them from one into the other.
    pub(super) id: [wgpu::Buffer; 2],
    /// A step's own: where each drop would be (unsorted), the sorted places in and out of a sweep,
    /// the multipliers, where each was and then its speed, and the sorted order.
    pub(super) xp: wgpu::Buffer,
    pub(super) y: [wgpu::Buffer; 2],
    pub(super) lamb: wgpu::Buffer,
    pub(super) velb: wgpu::Buffer,
    pub(super) idx: wgpu::Buffer,
    /// A colour mix pass's new mixes (mix.wgsl).
    pub(super) mixb: wgpu::Buffer,
}

impl ByDrop {
    pub(super) fn new(device: &wgpu::Device, room: u32) -> ByDrop {
        let n = u64::from(room);
        ByDrop {
            xv: storage(device, "water places", n * 16),
            nf: storage(device, "water ways", n * 16),
            id: [storage(device, "water kinds a", n * 16), storage(device, "water kinds b", n * 16)],
            xp: storage(device, "water predicted", n * 16),
            y: [storage(device, "water sweep a", n * 16), storage(device, "water sweep b", n * 16)],
            lamb: storage(device, "water multipliers", n * 16),
            velb: storage(device, "water speeds", n * 16),
            idx: storage(device, "water order", n * 4),
            mixb: storage(device, "water mixes", n * 4),
        }
    }

    /// Copies the `count` drops kept from step to step (their kinds from `id[now]`) into `to`.
    pub(super) fn copy_kept(&self, encoder: &mut wgpu::CommandEncoder, to: &ByDrop, count: u32, now: usize) {
        if count == 0 {
            return;
        }
        let bytes = u64::from(count) * 16;
        encoder.copy_buffer_to_buffer(&self.xv, 0, &to.xv, 0, bytes);
        encoder.copy_buffer_to_buffer(&self.nf, 0, &to.nf, 0, bytes);
        encoder.copy_buffer_to_buffer(&self.id[now], 0, &to.id[now], 0, bytes);
    }
}

/// A body's drops on the card, for the drawing to read where they are (part C3): each drop's
/// place and speed (`xv`, grid units), which way the liquid lies from it and how full its
/// neighbourhood is (`nf`: n.x, n.y, fullness, 0), and its kind, serial and colour (`id`, as
/// `vec4<u32>`). The first `count` are the drops.
pub struct DropBuffers<'a> {
    pub xv: &'a wgpu::Buffer,
    pub nf: &'a wgpu::Buffer,
    pub id: &'a wgpu::Buffer,
    pub count: u32,
}
