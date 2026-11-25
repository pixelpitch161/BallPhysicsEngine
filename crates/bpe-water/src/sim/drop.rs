//! A drop, and where drops are born: a fill (a lattice of four drops a cell) and a pour.

use bytemuck::{Pod, Zeroable};
use glam::Vec2;

use crate::solid::{Layout, SolidField};

/// The bit of [`Drop::kind`] that makes a drop stop at the course's edge.
pub const STOPS: u32 = 256;

/// One drop of liquid, in grid units. Matches `Drop` in common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct Drop {
    pub x: [f32; 2],
    pub v: [f32; 2],
    /// The APIC matrix: xx, xy, yx, yy.
    pub c: [f32; 4],
    /// How stretched the liquid at it is (J - 1).
    pub e: f32,
    /// Bits 0 to 3 its liquid, bit 8 ([`STOPS`]) it stops at the course's edge.
    pub kind: u32,
    pub serial: u32,
    /// rgba8, linear.
    pub colour: u32,
}

impl Drop {
    pub fn new(x: Vec2, v: Vec2, kind: u32, serial: u32, colour: u32) -> Drop {
        Drop { x: x.to_array(), v: v.to_array(), c: [0.0; 4], e: 0.0, kind, serial, colour }
    }

    pub fn liquid(&self) -> usize {
        (self.kind & 15) as usize
    }
}

/// The colour drops are born with when nothing says otherwise (a water blue, linear rgba8).
pub const WATER: u32 = u32::from_le_bytes([8, 51, 184, 255]);

/// A small repeatable dice: the same `seed` and `step` give the same numbers on every machine.
pub struct Dice(u32);

impl Dice {
    pub fn new(seed: u32, step: u32) -> Dice {
        Dice(seed ^ step.wrapping_mul(0x9e37_79b9) ^ 0x85eb_ca6b)
    }

    fn next(&mut self) -> u32 {
        // (xorshift32, never 0)
        let mut x = self.0 | 1;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// From 0 up to 1.
    pub fn unit(&mut self) -> f32 {
        (self.next() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// From -1 to 1.
    pub fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }
}

/// A fill: four drops a cell on a lattice over the world rectangle `lo` to `hi`, wherever a drop
/// may be (half a cell clear of every solid), bottom row first.
pub fn fill(layout: &Layout, field: &SolidField, lo: Vec2, hi: Vec2, kind: u32, first_serial: u32) -> Vec<Drop> {
    let (g_lo, g_hi) = (layout.to_grid(lo), layout.to_grid(hi));
    let (c0, c1) = ((g_lo.x * 2.0).ceil() as i64, (g_hi.x * 2.0).floor() as i64);
    let (r0, r1) = ((g_lo.y * 2.0).ceil() as i64, (g_hi.y * 2.0).floor() as i64);
    let mut out = Vec::new();
    for r in (r0..r1).rev() {
        for c in c0..c1 {
            let g = Vec2::new(c as f32 * 0.5 + 0.25, r as f32 * 0.5 + 0.25);
            if field.at(layout.to_world(g)) < 0.5 * layout.cell {
                continue;
            }
            out.push(Drop::new(g, Vec2::ZERO, kind, first_serial.wrapping_add(out.len() as u32), WATER));
        }
    }
    out
}

/// A pour: a spout that makes as many drops a second as its width times its speed fills.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pour {
    /// The spout's middle, world pixels.
    pub place: Vec2,
    /// Which way it pours, degrees (90 is straight down the screen).
    pub aim: f32,
    /// The stream's width, pixels.
    pub width: f32,
    /// Pixels a second.
    pub speed: f32,
    /// How far the drops' ways fan out, degrees.
    pub spread: f32,
    /// When it starts and stops, seconds.
    pub from: f32,
    pub until: f32,
    pub kind: u32,
    pub colour: u32,
}

impl Default for Pour {
    fn default() -> Self {
        Pour {
            place: Vec2::ZERO,
            aim: 90.0,
            width: 40.0,
            speed: 300.0,
            spread: 4.0,
            from: 0.0,
            until: f32::MAX,
            kind: STOPS,
            colour: WATER,
        }
    }
}

/// What a pour still owes and the serial of its next drop.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Poured {
    pub owed: f32,
    pub born: u32,
}

impl Pour {
    /// The most drops a pour makes in a liquid step.
    pub const MOST_A_STEP: u32 = 20_000;

    /// This pour's drops for liquid step `step` of `dt` seconds on `layout`.
    pub fn drops(&self, state: &mut Poured, layout: &Layout, dt: f32, step: u32, seed: u32) -> Vec<Drop> {
        let time = step as f32 * dt;
        if time < self.from || time >= self.until {
            state.owed = 0.0;
            return Vec::new();
        }
        let per_drop = layout.cell * layout.cell * 0.25;
        let owed = state.owed + self.width * self.speed * dt / per_drop;
        let owed = if owed.is_finite() { owed.min(Pour::MOST_A_STEP as f32 + 1.0) } else { 0.0 };
        let n = owed.floor() as u32;
        state.owed = owed - n as f32;
        let way = Vec2::from_angle(self.aim.to_radians());
        let across = way.perp();
        let mut dice = Dice::new(seed, step);
        let mut out = Vec::with_capacity(n as usize);
        for i in 0..n {
            let u = (i as f32 + dice.unit()) / n as f32 - 0.5;
            // Spread along the way it travels this step too, so drops don't come out in rows.
            let along = dice.unit() * self.speed * dt;
            let at = self.place + across * (u * self.width) + way * along;
            let turn = (self.spread * dice.signed()).to_radians();
            let v = Vec2::from_angle(self.aim.to_radians() + turn) * self.speed;
            out.push(Drop::new(layout.to_grid(at), v / layout.cell, self.kind, state.born, self.colour));
            state.born = state.born.wrapping_add(1);
        }
        out
    }
}
