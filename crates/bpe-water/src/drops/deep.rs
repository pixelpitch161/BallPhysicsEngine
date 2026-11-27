//! Deep water (part C2, spec section 6): the grid's one-sided pressure underneath, so a pool keeps
//! its height. Each step of a frame that runs it, the sorted drops are gathered onto the layout's
//! cells, the pressure is solved from the one the step before kept (the grid core's multigrid, now
//! in `shaders/deep.wgsl`), and every drop deeper than `deep_d` takes the pressure's change of speed
//! on the place it would reach. The cells' buffers and the pressure's ladder live here; the passes
//! are encoded into the frame by [`super::Water`].

use super::buffers::storage;
use crate::solid::Layout;

/// The most levels the pressure's ladder has.
pub const MOST_LEVELS: usize = 12;
/// The pressure passes' numbers (two a level: twice the level, plus the colour).
pub(super) const TICKS: u32 = 2 * MOST_LEVELS as u32;
/// Words a cell in the sums (`SUMS` in deep.wgsl).
const SUMS: u64 = 7;

/// The pressure's levels for a grid of `cols` x `rows` nodes: (where it starts, columns, rows, 0),
/// each half the one before each way, down to a handful of cells.
pub fn levels(cols: u32, rows: u32) -> Vec<[u32; 4]> {
    let mut out = vec![[0, cols, rows, 0]];
    while out.len() < MOST_LEVELS {
        let [at, c, r, _] = out[out.len() - 1];
        if c.min(r) <= 4 {
            break;
        }
        out.push([at + c * r, c.div_ceil(2), r.div_ceil(2), 0]);
    }
    out
}

/// A body's cells for its deep water.
pub(super) struct Deep {
    pub(super) levels: Vec<[u32; 4]>,
    /// The drops' sums (whole numbers), cleared by the across pass for the next step.
    pub(super) sums: wgpu::Buffer,
    /// The solids by cell, made once when the body is.
    pub(super) wallc: wgpu::Buffer,
    /// The pressure and its ladder: part of the body's state (the sweeps start from it).
    pub(super) cellp: wgpu::Buffer,
    pub(super) coef: wgpu::Buffer,
    /// How full each cell is and then its depth share; how far down its column it is to a cell that
    /// is not full.
    pub(super) dfa: wgpu::Buffer,
    pub(super) dfb: wgpu::Buffer,
    /// When the liquids' weights differ, each cell's mass and volume at its middle (written each
    /// step before the push reads them).
    pub(super) mass: wgpu::Buffer,
    /// The cells deep at a frame's last step.
    pub(super) count: wgpu::Buffer,
    /// The liquid, and the liquid times its drops' fullness, of the cells where the drops-only
    /// water meets the deep water this step (fixed point; emptied by each step's cells pass).
    pub(super) rest: wgpu::Buffer,
}

impl Deep {
    pub(super) fn new(device: &wgpu::Device, layout: &Layout) -> Deep {
        let cells = layout.nodes() as u64;
        let levels = levels(layout.cols, layout.rows);
        let every = levels.iter().map(|l| u64::from(l[1] * l[2])).sum::<u64>();
        Deep {
            levels,
            sums: storage(device, "water deep sums", cells * SUMS * 4),
            wallc: storage(device, "water deep solids", cells * 16),
            cellp: storage(device, "water deep pressure", every * 16),
            coef: storage(device, "water deep faces", every * 16),
            dfa: storage(device, "water deep fill", cells * 4),
            dfb: storage(device, "water deep column", cells * 4),
            mass: storage(device, "water deep mass", cells * 8),
            count: storage(device, "water deep count", 4),
            rest: storage(device, "water deep rest", 16),
        }
    }

    /// The levels as the frame's numbers hold them.
    pub(super) fn ladder(&self) -> [[u32; 4]; MOST_LEVELS] {
        let mut lv = [[0u32; 4]; MOST_LEVELS];
        lv[..self.levels.len()].copy_from_slice(&self.levels);
        lv
    }

    /// The workgroups (16 x 16) a pressure pass of `level` takes.
    pub(super) fn tiles(&self, level: usize) -> (u32, u32) {
        let [_, c, r, _] = self.levels[level];
        (c.div_ceil(16), r.div_ceil(16))
    }

    /// Copies what this body keeps from step to step (the pressure) into `to`, made on the same
    /// layout.
    pub(super) fn copy_kept(&self, encoder: &mut wgpu::CommandEncoder, to: &Deep) {
        encoder.copy_buffer_to_buffer(&self.cellp, 0, &to.cellp, 0, self.cellp.size());
        encoder.copy_buffer_to_buffer(&self.wallc, 0, &to.wallc, 0, self.wallc.size());
        // (The depth share too: a frame that only measures the deep part reads the last frame's on
        // every step before its last, for where trapped air bursts.)
        encoder.copy_buffer_to_buffer(&self.dfa, 0, &to.dfa, 0, self.dfa.size());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_halve_down_to_a_handful() {
        let l = levels(100, 60);
        assert_eq!(l[0], [0, 100, 60, 0]);
        assert_eq!(l[1], [6000, 50, 30, 0]);
        assert!(l.last().is_some_and(|[_, c, r, _]| (*c).min(*r) <= 4));
        assert_eq!(levels(4, 100).len(), 1, "a grid a handful wide has one level");
        assert!(levels(4000, 4000).len() <= MOST_LEVELS);
    }
}
