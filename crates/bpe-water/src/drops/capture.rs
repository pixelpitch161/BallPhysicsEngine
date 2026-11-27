//! Moments kept for the final's take (Water mode part 8): each frame's end, and in slow motion
//! more moments inside a frame, each drop packed into 8 bytes on the card (`shaders/pack.wgsl`)
//! and read back a frame later, so the card steps the next frame while the host packs.
//!
//! A moment inside a frame is the body at the end of a whole step: that step ends with the plain
//! order's own pass (`cs_finish`, which keeps the places) and the next starts with `cs_predict`, as
//! a frame does when the steps are not joined, which gives the same water to the bit.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::Water;
use super::buffers::STORE;
use super::passes::{ENTRIES, P, STEP_ROOM};

/// The most moments a frame keeps (its end and seven inside it: 8 times the frames).
pub const MOST_KEPT: u32 = 8;

/// When a kept moment is: water frame `frame`'s start plus `steps` of its `of` steps (a frame's
/// end is the next frame's start, `steps` 0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Moment {
    pub frame: u32,
    pub steps: u32,
    pub of: u32,
}

impl Moment {
    /// Water seconds.
    pub fn time(&self) -> f64 {
        (f64::from(self.frame) + f64::from(self.steps) / f64::from(self.of.max(1))) / 60.0
    }
}

/// One moment's drops, each as `[cell | kind << 24, place and speed]` (all ones: not in the
/// course), in the order the card holds them.
#[derive(Clone, Debug)]
pub struct Captured {
    pub moment: Moment,
    pub drops: Vec<[u32; 2]>,
}

/// A moment's buffers: where the pass packs, and its copy for reading.
struct Slot {
    pack: wgpu::Buffer,
    back: wgpu::Buffer,
    room: u32,
}

/// One frame's moments on their way back.
struct Pending {
    moments: Vec<(Moment, u32)>,
    done: Option<wgpu::SubmissionIndex>,
    /// Its buffers not mapped yet.
    left: Arc<AtomicU32>,
}

impl Pending {
    /// Maps the first `n` moments' bytes of each of `slots` (as `moments` say).
    fn map(slots: &[Slot], moments: Vec<(Moment, u32)>, done: wgpu::SubmissionIndex) -> Pending {
        let left = Arc::new(AtomicU32::new(moments.iter().filter(|m| m.1 > 0).count() as u32));
        for (k, (_, n)) in moments.iter().enumerate() {
            if *n > 0 {
                let left = left.clone();
                slots[k].back.slice(..u64::from(*n) * 8).map_async(wgpu::MapMode::Read, move |_| {
                    left.fetch_sub(1, Ordering::AcqRel);
                });
            }
        }
        Pending { moments, done: Some(done), left }
    }
}

/// The moments' buffers, two frames' worth (a frame's are read while the next frame's are filled),
/// and the moments read back.
#[derive(Default)]
pub struct Capture {
    sets: [Vec<Slot>; 2],
    busy: [Option<Pending>; 2],
    /// Sets on their way, oldest first.
    order: VecDeque<usize>,
    ready: VecDeque<Captured>,
}

impl Capture {
    pub fn new() -> Capture {
        Capture::default()
    }

    /// The moments read back (or that had no drops), oldest first; with `wait`, every one on its way
    /// is read first (waiting for the card).
    pub fn take(&mut self, device: &wgpu::Device, wait: bool) -> Vec<Captured> {
        let _ = device.poll(wgpu::PollType::Poll);
        while let Some(&set) = self.order.front() {
            if !wait && !self.mapped(set) {
                break;
            }
            self.read(device, set);
        }
        self.ready.drain(..).collect()
    }

    /// How many frames' moments are on their way.
    pub fn on_the_way(&self) -> usize {
        self.order.len()
    }

    /// Whether every buffer of `set` is mapped (its frame is done).
    fn mapped(&self, set: usize) -> bool {
        self.busy[set].as_ref().is_none_or(|p| p.left.load(Ordering::Acquire) == 0)
    }

    /// Reads set `set` back (its frame is done: waits for the card when it is not) and frees it.
    fn read(&mut self, device: &wgpu::Device, set: usize) {
        let Some(pending) = self.busy[set].take() else { return };
        if let Some(done) = &pending.done {
            let _ = device.poll(wgpu::PollType::Wait { submission_index: Some(done.clone()), timeout: None });
        }
        for (k, (moment, n)) in pending.moments.into_iter().enumerate() {
            let drops = if n == 0 {
                Vec::new()
            } else {
                let back = &self.sets[set][k].back;
                let bytes = u64::from(n) * 8;
                let out = {
                    let mapped = back.slice(..bytes).get_mapped_range().expect("the moment is mapped");
                    bytemuck::pod_collect_to_vec::<u8, [u32; 2]>(&mapped)
                };
                back.unmap();
                out
            };
            self.ready.push_back(Captured { moment, drops });
        }
        self.order.retain(|s| *s != set);
    }

    /// A free set (waiting for the oldest when both are on their way), with room for `counts`.
    fn free_set(&mut self, device: &wgpu::Device, counts: &[u32]) -> usize {
        let set = match (0..2).find(|s| self.busy[*s].is_none()) {
            Some(s) => s,
            None => {
                let oldest = self.order[0];
                self.read(device, oldest);
                oldest
            }
        };
        let slots = &mut self.sets[set];
        for (k, &n) in counts.iter().enumerate() {
            if k >= slots.len() || slots[k].room < n {
                let room = n.max(4096).next_power_of_two();
                let bytes = u64::from(room) * 8;
                let pack = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("water kept moment"),
                    size: bytes,
                    usage: STORE,
                    mapped_at_creation: false,
                });
                let back = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("water kept moment back"),
                    size: bytes,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let slot = Slot { pack, back, room };
                if k < slots.len() {
                    slots[k] = slot;
                } else {
                    slots.push(slot);
                }
            }
        }
        set
    }

    /// Frame moments that had no drops (nothing sent to the card).
    pub(super) fn none(&mut self, moments: &[Moment]) {
        self.ready.extend(moments.iter().map(|m| Captured { moment: *m, drops: Vec::new() }));
    }
}

/// The steps a frame of `n` steps keeping `keep` moments is cut after (each the step index whose
/// end is kept, the frame's last left out), and how many steps in each cut is.
pub(super) fn cuts(n: u32, keep: u32) -> Vec<u32> {
    let keep = keep.clamp(1, MOST_KEPT);
    let mut out: Vec<u32> = (1..keep)
        .map(|i| ((f64::from(i) * f64::from(n) / f64::from(keep)).round() as u32).clamp(1, n))
        .filter(|c| *c < n)
        .map(|c| c - 1)
        .collect();
    out.dedup();
    out
}

impl Water {
    /// The pass that packs the first `count` drops into `slot`, its kinds as `now`'s binding 5
    /// holds them, with its numbers at pack slot `at`.
    pub(super) fn pack_group(&self, now: usize, slot_buffer: &wgpu::Buffer) -> wgpu::BindGroup {
        let (name, bindings) = ENTRIES[P::Pack as usize];
        let entries: Vec<wgpu::BindGroupEntry> = bindings
            .iter()
            .map(|&b| {
                let resource = match b {
                    0 => self.frame_uniform.as_entire_binding(),
                    1 => self.step_binding(),
                    2 => self.by.xv.as_entire_binding(),
                    5 => self.by.id[1 - now].as_entire_binding(),
                    _ => slot_buffer.as_entire_binding(),
                };
                wgpu::BindGroupEntry { binding: b, resource }
            })
            .collect();
        self.gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(name),
            layout: &self.gpu.pipes.list[P::Pack as usize].1,
            entries: &entries,
        })
    }

    /// One frame, as [`Water::frame`] plays it (the same water to the bit), keeping `keep` moments
    /// of it (at most [`MOST_KEPT`]): its end, and `keep - 1` more evenly inside it at whole steps.
    /// They come back through `capture` ([`Capture::take`]).
    pub fn frame_keeping(&mut self, keep: u32, capture: &mut Capture) {
        self.settle();
        let n = self.steps_now();
        self.play_frame(n, Some((capture, keep)));
    }

    /// Keeps the body as it is now (frame `frames()`'s start: the take's first moment, or a restart
    /// point's).
    pub fn capture_now(&mut self, capture: &mut Capture) {
        let moment = Moment { frame: self.frames, steps: 0, of: 1 };
        if self.count == 0 {
            capture.none(&[moment]);
            return;
        }
        let device = self.gpu.device.clone();
        let set = capture.free_set(&device, &[self.count]);
        let slot = &capture.sets[set][0];
        let at = PACK_NOW;
        self.gpu.queue.write_buffer(
            &self.step_uniform,
            u64::from(at) * STEP_ROOM,
            bytemuck::cast_slice(&[self.count, 0u32, 0, 0]),
        );
        let group = self.pack_group(1 - self.now, &slot.pack);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("water keep") });
        {
            let mut pass = encoder
                .begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("water keep"), timestamp_writes: None });
            pass.set_pipeline(&self.gpu.pipes.list[P::Pack as usize].0);
            pass.set_bind_group(0, &group, &[at * STEP_ROOM as u32]);
            pass.dispatch_workgroups(self.count.div_ceil(super::WORKGROUP), 1, 1);
        }
        let bytes = u64::from(self.count) * 8;
        encoder.copy_buffer_to_buffer(&slot.pack, 0, &slot.back, 0, bytes);
        let done = self.gpu.queue.submit([encoder.finish()]);
        capture.busy[set] = Some(Pending::map(&capture.sets[set], vec![(moment, self.count)], done));
        capture.order.push_back(set);
    }
}

/// The pack passes' numbers' slots in the steps' buffer: one for each moment a frame keeps, and
/// one for [`Water::capture_now`].
pub(super) const PACK_SLOTS: u32 = MOST_KEPT + 1;
pub(super) const PACK_NOW: u32 = super::PACK_SLOT + MOST_KEPT;

/// What a frame's pack passes need, made before its pass is encoded.
pub(super) struct Packing {
    pub(super) set: usize,
    /// The step each in-frame moment is kept after, and its group; then the frame's end's group.
    pub(super) cuts: Vec<u32>,
    pub(super) groups: Vec<wgpu::BindGroup>,
}

impl Packing {
    /// The frame's moments (each with the drops it packs) and the set's buffers for them.
    pub(super) fn prepare(
        water: &Water,
        capture: &mut Capture,
        n: u32,
        keep: u32,
        counts: &[u32],
        end: u32,
    ) -> Packing {
        let cuts = cuts(n, keep);
        let mut sizes: Vec<u32> = cuts.iter().map(|&j| counts[j as usize]).collect();
        sizes.push(end);
        let set = capture.free_set(&water.gpu.device, &sizes);
        let mut records = vec![0u8; sizes.len() * STEP_ROOM as usize];
        for (k, &c) in sizes.iter().enumerate() {
            let at = k * STEP_ROOM as usize;
            records[at..at + 4].copy_from_slice(&c.to_le_bytes());
        }
        water.gpu.queue.write_buffer(&water.step_uniform, u64::from(super::PACK_SLOT) * STEP_ROOM, &records);
        let groups = cuts
            .iter()
            .enumerate()
            .map(|(k, &j)| water.pack_group(water.now ^ (j as usize & 1), &capture.sets[set][k].pack))
            .chain(std::iter::once(
                water.pack_group(water.now ^ ((n - 1) as usize & 1), &capture.sets[set][cuts.len()].pack),
            ))
            .collect();
        Packing { set, cuts, groups }
    }

    /// Hands the frame's moments to `capture`: copies its buffers back in `encoder` (before the
    /// submission), then, once it is sent as `done`, maps them.
    pub(super) fn copy_back(&self, capture: &Capture, encoder: &mut wgpu::CommandEncoder, sizes: &[u32]) {
        for (k, &n) in sizes.iter().enumerate() {
            if n > 0 {
                let slot = &capture.sets[self.set][k];
                encoder.copy_buffer_to_buffer(&slot.pack, 0, &slot.back, 0, u64::from(n) * 8);
            }
        }
    }

    pub(super) fn sent(self, capture: &mut Capture, moments: Vec<(Moment, u32)>, done: wgpu::SubmissionIndex) {
        capture.busy[self.set] = Some(Pending::map(&capture.sets[self.set], moments, done));
        capture.order.push_back(self.set);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_is_cut_evenly_at_whole_steps() {
        assert!(cuts(32, 1).is_empty());
        assert_eq!(cuts(32, 2), vec![15]);
        assert_eq!(cuts(32, 4), vec![7, 15, 23]);
        assert_eq!(cuts(32, 8), vec![3, 7, 11, 15, 19, 23, 27]);
        assert_eq!(cuts(3, 8), vec![0, 1], "never more cuts than steps");
        assert_eq!(cuts(1, 4), Vec::<u32>::new());
    }
}
