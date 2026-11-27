//! The clock: what the card reports at the end of each frame ([`Report`]), read back two frames
//! later without waiting, so frame `k` takes its steps from the water as it was at the end of
//! frame `k - 2`, and drops that left the course are let go at a frame boundary.

use std::collections::VecDeque;

use bytemuck::{Pod, Zeroable};

use super::drains::MOST_DRAINS;

/// The most pours a body has (each has its spout's fullness in the report).
pub const MOST_POURS: usize = 64;
/// Reports on their way back at once.
pub(super) const SLOTS: usize = 3;

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
    /// The liquid's drops each drain took in the frame.
    pub drained: [u32; MOST_DRAINS],
}

impl Report {
    /// The fastest drop's speed, pixels a second.
    pub fn fastest_px(&self) -> f32 {
        let f = f32::from_bits(self.fastest);
        if f.is_finite() { f.max(0.0) } else { 0.0 }
    }

    fn of(live: u32) -> Report {
        Report { fastest: 0, live, deep: 0, air: 0, liquid: 0, spout: [0.0; MOST_POURS], drained: [0; MOST_DRAINS] }
    }
}

#[derive(Clone, Debug)]
enum Back {
    /// Known on the host (a frame with no drops, or read back already).
    Known(Box<Report>),
    /// In report slot `slot`, once submission `done` is.
    OnCard { slot: usize, done: wgpu::SubmissionIndex },
}

/// A frame's report, how many drops the frame ended with, how many liquid drops had been added
/// by its end (counting on from the body's start, wrapping), and which of its drains fed the tank
/// (bit `k` drain `k`, as the drains were when it ran).
#[derive(Clone, Debug)]
struct Entry {
    frame: u32,
    count: u32,
    poured: u32,
    feeds: u32,
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
    /// What each drain took in that report's frame, and how much of it went to the tank.
    pub(super) drained: [u32; MOST_DRAINS],
    pub(super) fed: u64,
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
            drained: [0; MOST_DRAINS],
            fed: 0,
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
    /// was sent as `done` (or had no drops), its drains `feeds` feeding the tank.
    pub(super) fn sent(
        &mut self,
        frame: u32,
        count: u32,
        poured: u32,
        feeds: u32,
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
        self.recent.push_back(Entry { frame, count, poured, feeds, back });
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

    /// The report of frame `frame`, the count it ended with, the liquid drops added by then and the
    /// drains that fed the tank, if the clock has it.
    fn report_of(&mut self, device: &wgpu::Device, frame: u32) -> Option<(Report, u32, u32, u32)> {
        let i = self.recent.iter().position(|e| e.frame == frame)?;
        let report = self.resolve(device, i);
        let e = &self.recent[i];
        Some((report, e.count, e.poured, e.feeds))
    }

    /// Settles boundary `k` (the start of frame `k`), once: reads frame `k - 2`'s report and returns
    /// how many drops to let go from the end (dead ones: they sort last and never come back), and
    /// how many liquid drops that frame's feeding drains took (none when `k` was settled before).
    pub(super) fn settle(&mut self, device: &wgpu::Device, k: u32) -> (u32, u64) {
        if self.settled == Some(k) {
            return (0, 0);
        }
        self.settled = Some(k);
        // (The boundary after this one takes the place of the one two before.)
        self.added[(k + 1) as usize % SLOTS] = 0.0;
        self.reported = 0.0;
        self.spouts = [0.0; MOST_POURS];
        self.deep = 0;
        self.air = 0;
        self.drained = [0; MOST_DRAINS];
        self.fed = 0;
        let mut go = 0;
        if k >= 2
            && let Some((report, count, poured, feeds)) = self.report_of(device, k - 2)
        {
            self.drained = report.drained;
            self.fed = (0..MOST_DRAINS).filter(|&d| feeds & (1 << d) != 0).map(|d| u64::from(report.drained[d])).sum();
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
        (go, self.fed)
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

    /// The host's side of this clock (every report read back first), for a restart point.
    pub(super) fn state(&mut self, device: &wgpu::Device) -> ClockState {
        self.read_all(device);
        let recent = self
            .recent
            .iter()
            .map(|e| {
                let report = match &e.back {
                    Back::Known(r) => **r,
                    Back::OnCard { .. } => unreachable!("every report was read back"),
                };
                (e.frame, e.count, e.poured, e.feeds, report)
            })
            .collect();
        ClockState {
            recent,
            let_go: self.let_go,
            added: self.added,
            settled: self.settled,
            reported: self.reported,
            spouts: self.spouts,
            deep: self.deep,
            air: self.air,
            liquid: self.liquid,
            last_steps: self.last_steps,
            drained: self.drained,
            fed: self.fed,
        }
    }

    /// Takes the host's side from `state` (a body made from a restart point).
    pub(super) fn set_state(&mut self, state: &ClockState) {
        self.recent = state
            .recent
            .iter()
            .map(|&(frame, count, poured, feeds, report)| Entry {
                frame,
                count,
                poured,
                feeds,
                back: Back::Known(Box::new(report)),
            })
            .collect();
        self.let_go = state.let_go;
        self.added = state.added;
        self.settled = state.settled;
        self.reported = state.reported;
        self.spouts = state.spouts;
        self.deep = state.deep;
        self.air = state.air;
        self.liquid = state.liquid;
        self.last_steps = state.last_steps;
        self.drained = state.drained;
        self.fed = state.fed;
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
        to.drained = self.drained;
        to.fed = self.fed;
    }
}

/// The host's side of a clock, every report known: what a restart point keeps of it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ClockState {
    /// The last frames' (frame, drops at its end, liquid added by its end, feeding drains, report).
    recent: Vec<(u32, u32, u32, u32, Report)>,
    let_go: u32,
    added: [f32; SLOTS],
    settled: Option<u32>,
    reported: f32,
    spouts: [f32; MOST_POURS],
    deep: u32,
    air: u32,
    liquid: (u32, u32),
    last_steps: u32,
    drained: [u32; MOST_DRAINS],
    fed: u64,
}

impl ClockState {
    pub(super) fn write(&self, o: &mut super::state::Out) {
        o.u32(self.recent.len() as u32);
        for (frame, count, poured, feeds, report) in &self.recent {
            o.u32(*frame);
            o.u32(*count);
            o.u32(*poured);
            o.u32(*feeds);
            o.bytes(bytemuck::bytes_of(report));
        }
        o.u32(self.let_go);
        for a in self.added {
            o.f32(a);
        }
        o.u32(u32::from(self.settled.is_some()));
        o.u32(self.settled.unwrap_or(0));
        o.f32(self.reported);
        for s in self.spouts {
            o.f32(s);
        }
        for v in [self.deep, self.air, self.liquid.0, self.liquid.1, self.last_steps] {
            o.u32(v);
        }
        for d in self.drained {
            o.u32(d);
        }
        o.u32(self.fed as u32);
        o.u32((self.fed >> 32) as u32);
    }

    pub(super) fn read(i: &mut super::state::In) -> anyhow::Result<ClockState> {
        let n = i.u32()? as usize;
        if n > SLOTS {
            anyhow::bail!("the state's clock has too many reports");
        }
        let mut recent = Vec::with_capacity(n);
        for _ in 0..n {
            let (frame, count, poured, feeds) = (i.u32()?, i.u32()?, i.u32()?, i.u32()?);
            let bytes = i.bytes()?;
            let report = bytemuck::try_pod_read_unaligned::<Report>(&bytes)
                .map_err(|_| anyhow::anyhow!("the state's report is the wrong size"))?;
            recent.push((frame, count, poured, feeds, report));
        }
        let let_go = i.u32()?;
        let mut added = [0.0; SLOTS];
        for a in &mut added {
            *a = i.f32()?;
        }
        let settled = (i.u32()? != 0, i.u32()?);
        let reported = i.f32()?;
        let mut spouts = [0.0; MOST_POURS];
        for s in &mut spouts {
            *s = i.f32()?;
        }
        let (deep, air, l0, l1, last_steps) = (i.u32()?, i.u32()?, i.u32()?, i.u32()?, i.u32()?);
        let mut drained = [0; MOST_DRAINS];
        for d in &mut drained {
            *d = i.u32()?;
        }
        let fed = u64::from(i.u32()?) | (u64::from(i.u32()?) << 32);
        Ok(ClockState {
            recent,
            let_go,
            added,
            settled: settled.0.then_some(settled.1),
            reported,
            spouts,
            deep,
            air,
            liquid: (l0, l1),
            last_steps,
            drained,
            fed,
        })
    }
}
