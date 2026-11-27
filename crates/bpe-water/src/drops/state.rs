//! A body's whole state as bytes (Water mode part 8): the final's restart points. A body made from
//! its state ([`Water::from_state`]) plays on exactly as the body did, as its copy
//! ([`Water::copy`]) does: the same buffers, the same reports read back, the same counters.

use anyhow::{Context, bail};

use super::Water;
use super::clock::ClockState;
use crate::drop::{Pour, Poured};
use crate::drops::numbers::{Feel, Numbers};
use crate::parts::{MOST_MOVERS, Pose};
use crate::solid::SolidField;

const MAGIC: &[u8; 8] = b"BPESTAT2";

/// A body's state at a frame boundary (before frame `frames` starts).
#[derive(Clone, Debug, PartialEq)]
pub struct WaterState {
    pub cols: u32,
    pub rows: u32,
    pub count: u32,
    pub room: u32,
    pub serial: u32,
    pub poured: u32,
    pub steps: u64,
    pub frames: u32,
    pub now: u32,
    pub edges_shut: bool,
    pub deep_last: bool,
    pub deep_dt: f32,
    pub prime_next: bool,
    pub deep_hold: u32,
    pub deep_frames: u32,
    pub pours: Vec<(Pour, Poured)>,
    /// Where each moving part is (part 5) and how fast it moved over the frame before: a body made
    /// from the state is given its parts there ([`Water::resume_movers`]).
    pub poses: Vec<Pose>,
    pub speeds: Vec<Pose>,
    pub(super) clock: ClockState,
    /// The kept drops' buffers (`count` x 16 bytes each) and the deep water's kept cells (whole).
    pub xv: Vec<u8>,
    pub nf: Vec<u8>,
    pub id: Vec<u8>,
    pub cellp: Vec<u8>,
    pub wallc: Vec<u8>,
    pub dfa: Vec<u8>,
}

/// Little-endian bytes out.
#[derive(Default)]
pub(super) struct Out(pub(super) Vec<u8>);

impl Out {
    pub(super) fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub(super) fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub(super) fn f32(&mut self, v: f32) {
        self.u32(v.to_bits());
    }
    pub(super) fn bytes(&mut self, b: &[u8]) {
        self.u64(b.len() as u64);
        self.0.extend_from_slice(b);
    }
}

/// Little-endian bytes in; every read checked.
pub(super) struct In<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> In<'a> {
    pub(super) fn take(&mut self, n: usize) -> anyhow::Result<&'a [u8]> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.b.len()).context("the state is cut short")?;
        let out = &self.b[self.at..end];
        self.at = end;
        Ok(out)
    }
    pub(super) fn u32(&mut self) -> anyhow::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into()?))
    }
    pub(super) fn u64(&mut self) -> anyhow::Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into()?))
    }
    pub(super) fn f32(&mut self) -> anyhow::Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }
    pub(super) fn bytes(&mut self) -> anyhow::Result<Vec<u8>> {
        let n = usize::try_from(self.u64()?)?;
        Ok(self.take(n)?.to_vec())
    }
}

impl WaterState {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut o = Out::default();
        o.0.extend_from_slice(MAGIC);
        for v in [self.cols, self.rows, self.count, self.room, self.serial, self.poured] {
            o.u32(v);
        }
        o.u64(self.steps);
        for v in [self.frames, self.now, u32::from(self.edges_shut), u32::from(self.deep_last)] {
            o.u32(v);
        }
        o.f32(self.deep_dt);
        for v in [u32::from(self.prime_next), self.deep_hold, self.deep_frames] {
            o.u32(v);
        }
        o.u32(self.pours.len() as u32);
        for (p, s) in &self.pours {
            for v in [p.place.x, p.place.y, p.aim, p.width, p.speed, p.spread, p.from, p.until] {
                o.f32(v);
            }
            o.u32(p.kind);
            o.u32(p.colour);
            o.f32(s.owed);
            o.u32(s.born);
            o.f32(s.extra);
        }
        o.u32(self.poses.len() as u32);
        let speeds = self.speeds.iter().chain(std::iter::repeat(&super::movers::STILL));
        for (p, s) in self.poses.iter().zip(speeds) {
            for v in [p.at.x, p.at.y, p.turn, p.scale, s.at.x, s.at.y, s.turn, s.scale] {
                o.f32(v);
            }
        }
        self.clock.write(&mut o);
        for b in [&self.xv, &self.nf, &self.id, &self.cellp, &self.wallc, &self.dfa] {
            o.bytes(b);
        }
        o.0
    }

    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<WaterState> {
        let mut i = In { b: bytes, at: 0 };
        if i.take(8)? != MAGIC {
            bail!("this isn't a water state");
        }
        let (cols, rows, count, room, serial, poured) = (i.u32()?, i.u32()?, i.u32()?, i.u32()?, i.u32()?, i.u32()?);
        let steps = i.u64()?;
        let (frames, now, edges_shut, deep_last) = (i.u32()?, i.u32()?, i.u32()? != 0, i.u32()? != 0);
        let deep_dt = i.f32()?;
        let (prime_next, deep_hold, deep_frames) = (i.u32()? != 0, i.u32()?, i.u32()?);
        let n = i.u32()? as usize;
        if n > super::MOST_POURS {
            bail!("the state has too many pours");
        }
        let mut pours = Vec::with_capacity(n);
        for _ in 0..n {
            let mut f = [0.0f32; 8];
            for v in &mut f {
                *v = i.f32()?;
            }
            let pour = Pour {
                place: glam::Vec2::new(f[0], f[1]),
                aim: f[2],
                width: f[3],
                speed: f[4],
                spread: f[5],
                from: f[6],
                until: f[7],
                kind: i.u32()?,
                colour: i.u32()?,
            };
            let state = Poured { owed: i.f32()?, born: i.u32()?, extra: i.f32()? };
            pours.push((pour, state));
        }
        let n = i.u32()? as usize;
        if n > MOST_MOVERS {
            bail!("the state has too many moving parts");
        }
        let mut poses = Vec::with_capacity(n);
        let mut speeds = Vec::with_capacity(n);
        for _ in 0..n {
            for list in [&mut poses, &mut speeds] {
                let (x, y, turn, scale) = (i.f32()?, i.f32()?, i.f32()?, i.f32()?);
                list.push(Pose::new(glam::Vec2::new(x, y), turn, scale));
            }
        }
        let clock = ClockState::read(&mut i)?;
        let (xv, nf, id) = (i.bytes()?, i.bytes()?, i.bytes()?);
        let (cellp, wallc, dfa) = (i.bytes()?, i.bytes()?, i.bytes()?);
        let drops = u64::from(count) * 16;
        if [&xv, &nf, &id].iter().any(|b| b.len() as u64 != drops) || room < count || now > 1 {
            bail!("the state's drops don't add up");
        }
        Ok(WaterState {
            cols,
            rows,
            count,
            room,
            serial,
            poured,
            steps,
            frames,
            now,
            edges_shut,
            deep_last,
            deep_dt,
            prime_next,
            deep_hold,
            deep_frames,
            pours,
            poses,
            speeds,
            clock,
            xv,
            nf,
            id,
            cellp,
            wallc,
            dfa,
        })
    }
}

impl Water {
    /// The body's whole state where it is (waits for the card, as [`Water::copy`] does).
    pub fn state(&self) -> WaterState {
        let device = &self.gpu.device;
        let clock = self.clock.lock().expect("the clock").state(device);
        let drops = u64::from(self.count) * 16;
        let read = |b: &wgpu::Buffer, n: u64| if n == 0 { Vec::new() } else { self.read_buffer(b, n) };
        WaterState {
            cols: self.layout.cols,
            rows: self.layout.rows,
            count: self.count,
            room: self.room,
            serial: self.serial,
            poured: self.poured,
            steps: self.steps,
            frames: self.frames,
            now: self.now as u32,
            edges_shut: self.edges_shut,
            deep_last: self.deep_last,
            deep_dt: self.deep_dt,
            prime_next: self.prime_next,
            deep_hold: self.deep_hold,
            deep_frames: self.deep_frames,
            pours: self.pours.clone(),
            poses: self.poses().to_vec(),
            speeds: self.move_speeds().to_vec(),
            clock,
            xv: read(&self.by.xv, drops),
            nf: read(&self.by.nf, drops),
            id: read(&self.by.id[self.now], drops),
            cellp: read(&self.deep.cellp, self.deep.cellp.size()),
            wallc: read(&self.deep.wallc, self.deep.wallc.size()),
            dfa: read(&self.deep.dfa, self.deep.dfa.size()),
        }
    }

    /// A body on `field` (the one the state's body was made on; when parts move, give it them with
    /// [`Water::resume_movers`]) that plays on exactly as the body whose state `state` is did, given the same inputs each frame. `gravity`, `numbers` and
    /// `feels` are as [`Water::new`] takes them (the frame's inputs set them again before it plays).
    pub fn from_state(
        gpu: std::sync::Arc<super::WaterGpu>,
        field: &SolidField,
        gravity: glam::Vec2,
        numbers: Numbers,
        feels: &[Feel],
        state: &WaterState,
    ) -> anyhow::Result<Water> {
        if (field.layout.cols, field.layout.rows) != (state.cols, state.rows) {
            bail!("the state was made on another course");
        }
        let mut new = Water::new(gpu, field, gravity, numbers, feels);
        let deep = [(&new.deep.cellp, &state.cellp), (&new.deep.wallc, &state.wallc), (&new.deep.dfa, &state.dfa)];
        if deep.iter().any(|(b, s)| b.size() != s.len() as u64) {
            bail!("the state's deep water doesn't fit this course");
        }
        if state.room > super::MOST_DROPS {
            bail!("the state holds too many drops");
        }
        if state.room > new.room {
            new.by = super::buffers::ByDrop::new(&new.gpu.device, state.room);
            new.room = state.room;
        }
        new.now = state.now as usize;
        new.clock.get_mut().expect("the clock").set_state(&state.clock);
        new.bind();
        let queue = &new.gpu.queue;
        if state.count > 0 {
            queue.write_buffer(&new.by.xv, 0, &state.xv);
            queue.write_buffer(&new.by.nf, 0, &state.nf);
            queue.write_buffer(&new.by.id[new.now], 0, &state.id);
        }
        queue.write_buffer(&new.deep.cellp, 0, &state.cellp);
        queue.write_buffer(&new.deep.wallc, 0, &state.wallc);
        queue.write_buffer(&new.deep.dfa, 0, &state.dfa);
        queue.submit([]);
        new.count = state.count;
        new.serial = state.serial;
        new.poured = state.poured;
        new.steps = state.steps;
        new.frames = state.frames;
        new.edges_shut = state.edges_shut;
        new.pours = state.pours.clone();
        new.deep_last = state.deep_last;
        new.deep_dt = state.deep_dt;
        new.prime_next = state.prime_next;
        new.deep_hold = state.deep_hold;
        new.deep_frames = state.deep_frames;
        Ok(new)
    }
}
