//! The clock: what the card reports at the end of each frame ([`Report`]), read back two frames
//! later without waiting, so frame `k` takes its steps from the water as it was at the end of
//! frame `k - 2`, and drops that left the course are let go at a frame boundary.

use std::collections::VecDeque;

use bytemuck::{Pod, Zeroable};

/// The most pours a body has (each has its spout's fullness in the report).
pub const MOST_POURS: usize = 64;
/// Reports on their way back at once.
const SLOTS: usize = 3;

/// What the card reports at the end of a frame. Matches `Report` in report.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Report {
    /// The bits of the fastest drop's speed in the course, pixels a second.
    pub fastest: u32,
    /// The drops in the course after the frame's last sort (the rest sit last), and the air born
    /// after it.
    pub live: u32,
    /// The cells deep at the frame's last step (when it measured them: see `Water::frame`).
    pub deep: u32,
    /// The spots of trapped air the frame's last step found (born or not).
    pub air: u32,
    /// The liquid's drops in the course at the frame's end (air left out).
    pub liquid: u32,
    /// Each pour's fullness at its spout (1 is full).
    pub spout: [f32; MOST_POURS],
}

impl Report {
    /// The fastest drop's speed, pixels a second.
    pub fn fastest_px(&self) -> f32 {
        let f = f32::from_bits(self.fastest);
        if f.is_finite() { f.max(0.0) } else { 0.0 }
    }

    fn of(live: u32) -> Report {
        Report { fastest: 0, live, deep: 0, air: 0, liquid: 0, spout: [0.0; MOST_POURS] }
    }
}

#[derive(Clone, Debug)]
enum Back {
    /// Known on the host (a frame with no drops, or read back already).
    Known(Box<Report>),
    /// In report slot `slot`, once submission `done` is.
    OnCard { slot: usize, done: wgpu::SubmissionIndex },
}

/// A frame's report, how many drops the frame ended with, and how many liquid drops had been added
/// by its end (counting on from the body's start, wrapping).
#[derive(Clone, Debug)]
struct Entry {
    frame: u32,
    count: u32,
    poured: u32,
    back: Back,
}

/// The report slots and what the host keeps of the last frames.
pub(super) struct Clock {
    /// Each slot: the buffer the report pass writes, and its copy for reading.
    slots: Vec<(wgpu::Buffer, wgpu::Buffer)>,
    recent: VecDeque<Entry>,
    /// The drops let go at the last boundary.
    let_go: u32,
    /// The fastest speed (pixels a second) among the drops added at each of the last boundaries,
    /// by boundary % 3: the report two frames back has not seen them.
    added: [f32; SLOTS],
    /// The boundary (the frame about to start) the clock has settled, and the fastest speed (pixels
    /// a second) in the report it read then.
    settled: Option<u32>,
    reported: f32,
    /// The pours' fullness at their spouts in that report, its deep cells and its trapped air.
    pub(super) spouts: [f32; MOST_POURS],
    pub(super) deep: u32,
    pub(super) air: u32,
    /// The liquid's drops in the course in the last report read at a boundary, and how many liquid
    /// drops had been added by that frame's end (as `Entry::poured`).
    pub(super) liquid: (u32, u32),
    pub(super) last_steps: u32,
}

impl Clock {
    pub(super) fn new(device: &wgpu::Device) -> Clock {
        let bytes = std::mem::size_of::<Report>() as u64;
        let slots = (0..SLOTS)
            .map(|_| {
                let on_card = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("water report"),
                    size: bytes,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let back = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("water report back"),
                    size: bytes,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                (on_card, back)
            })
            .collect();
        Clock {
            slots,
            recent: VecDeque::new(),
            let_go: 0,
            added: [0.0; SLOTS],
            settled: None,
            reported: 0.0,
            spouts: [0.0; MOST_POURS],
            deep: 0,
            air: 0,
            liquid: (0, 0),
            last_steps: 0,
        }
    }

    /// The buffer frame `frame`'s report pass writes.
    pub(super) fn slot_of(&self, frame: u32) -> usize {
        frame as usize % SLOTS
    }

    pub(super) fn on_card(&self, slot: usize) -> &wgpu::Buffer {
        &self.slots[slot].0
    }

    /// Adds the copy of frame `frame`'s report to `encoder` (after its report pass).
    pub(super) fn copy_back(&self, encoder: &mut wgpu::CommandEncoder, frame: u32) {
        let (on_card, back) = &self.slots[self.slot_of(frame)];
        encoder.copy_buffer_to_buffer(on_card, 0, back, 0, on_card.size());
    }

    /// Frame `frame`, which ended with `count` drops and `poured` liquid drops added since the start,
    /// was sent as `done` (or had no drops).
    pub(super) fn sent(
        &mut self,
        frame: u32,
        count: u32,
        poured: u32,
        done: Option<wgpu::SubmissionIndex>,
        steps: u32,
    ) {
        let back = match done {
            Some(done) => {
                let slot = self.slot_of(frame);
                self.slots[slot].1.slice(..).map_async(wgpu::MapMode::Read, |_| {});
                Back::OnCard { slot, done }
            }
            None => Back::Known(Box::new(Report::of(0))),
        };
        self.recent.push_back(Entry { frame, count, poured, back });
        while self.recent.len() > SLOTS {
            self.recent.pop_front();
        }
        self.last_steps = steps;
    }

    /// Drops were added at the coming boundary, the fastest at `speed` pixels a second.
    pub(super) fn added(&mut self, boundary: u32, speed: f32) {
        let k = boundary as usize % SLOTS;
        if speed.is_finite() {
            self.added[k] = self.added[k].max(speed);
        } else {
            self.added[k] = f32::MAX;
        }
    }

    /// Reads every report still on its way (waiting for the card).
    pub(super) fn read_all(&mut self, device: &wgpu::Device) {
        for i in 0..self.recent.len() {
            self.resolve(device, i);
        }
    }

    fn resolve(&mut self, device: &wgpu::Device, i: usize) -> Report {
        match &self.recent[i].back {
            Back::Known(r) => **r,
            Back::OnCard { slot, done } => {
                let _ = device.poll(wgpu::PollType::Wait { submission_index: Some(done.clone()), timeout: None });
                let back = &self.slots[*slot].1;
                let report = {
                    let mapped = back.slice(..).get_mapped_range().expect("the report is mapped");
                    *bytemuck::from_bytes::<Report>(&mapped)
                };
                back.unmap();
                self.recent[i].back = Back::Known(Box::new(report));
                report
            }
        }
    }

    /// The report of frame `frame`, the count it ended with and the liquid drops added by then, if
    /// the clock has it.
    fn report_of(&mut self, device: &wgpu::Device, frame: u32) -> Option<(Report, u32, u32)> {
        let i = self.recent.iter().position(|e| e.frame == frame)?;
        let report = self.resolve(device, i);
        Some((report, self.recent[i].count, self.recent[i].poured))
    }

    /// Settles boundary `k` (the start of frame `k`), once: reads frame `k - 2`'s report and returns
    /// how many drops to let go from the end (dead ones: they sort last and never come back).
    pub(super) fn settle(&mut self, device: &wgpu::Device, k: u32) -> u32 {
        if self.settled == Some(k) {
            return 0;
        }
        self.settled = Some(k);
        // (The boundary after this one takes the place of the one two before.)
        self.added[(k + 1) as usize % SLOTS] = 0.0;
        self.reported = 0.0;
        self.spouts = [0.0; MOST_POURS];
        self.deep = 0;
        self.air = 0;
        let mut go = 0;
        if k >= 2
            && let Some((report, count, poured)) = self.report_of(device, k - 2)
        {
            self.liquid = (report.liquid, poured);
            self.reported = report.fastest_px();
            self.spouts = report.spout;
            self.deep = report.deep;
            self.air = report.air;
            // (The dead at the end of frame k - 2, less those let go at the last boundary: they were
            // among them.)
            go = count.saturating_sub(report.live).saturating_sub(self.let_go);
        }
        self.let_go = go;
        go
    }

    /// The fastest speed (pixels a second) frame `k` must allow for, before headroom: frame
    /// `k - 2`'s report and the drops added since (once boundary `k` is settled).
    pub(super) fn fastest(&self, k: u32) -> f32 {
        let mut fastest = self.reported.max(self.added[k as usize % SLOTS]);
        if k >= 1 {
            fastest = fastest.max(self.added[(k - 1) as usize % SLOTS]);
        }
        fastest
    }

    /// The host's side of this clock, for a copy (every report read back first).
    pub(super) fn copy_into(&mut self, device: &wgpu::Device, to: &mut Clock) {
        self.read_all(device);
        to.recent = self.recent.clone();
        to.let_go = self.let_go;
        to.added = self.added;
        to.settled = self.settled;
        to.reported = self.reported;
        to.spouts = self.spouts;
        to.deep = self.deep;
        to.air = self.air;
        to.liquid = self.liquid;
        to.last_steps = self.last_steps;
    }
}
