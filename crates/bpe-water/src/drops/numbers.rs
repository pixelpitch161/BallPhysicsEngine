//! The all-drops engine's numbers ([`DropNumbers`]).

use bytemuck::{Pod, Zeroable};

use crate::sim::numbers::{Feel, MOST_LIQUIDS};

/// The all-drops engine's numbers. The measuring tests may replace any by a `BPE_DROPS_*`
/// environment variable ([`DropNumbers::from_env_for`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropNumbers {
    /// Liquid steps a second.
    pub rate: u32,
    /// Sweeps of the pressure a step.
    pub iters: u32,
    /// How far a drop feels its neighbours, in cells (drops at rest are half a cell apart).
    pub reach: f32,
    /// The share of a sweep's correction taken.
    pub relax: f32,
    /// The softening of a correction, as a share of a drop's own stiffness at rest.
    pub eps: f32,
    /// How much of a step's pressure is kept for the next (0: none, plain position based fluids).
    pub warm: f32,
    /// The furthest the kept pressure moves a drop in a step, in cells.
    pub warm_most: f32,
    /// How far off a solid's face a drop stays, in cells.
    pub wall_off: f32,
    /// How much of the solids counts as liquid for the density (1: all of it).
    pub wall_share: f32,
    /// A sub-move's length, in cells.
    pub sub_move: f32,
    /// The most cells a drop moves in a liquid step.
    pub most_move: f32,
    /// The furthest a sweep moves a drop, in cells.
    pub most_fix: f32,
    /// How fast neighbouring drops even out their speeds, pixels squared a second.
    pub visc: f32,
    /// The skin of a liquid at Surface pull 1, in pixels cubed a second squared (its capillary
    /// length squared times the gravity: 4.0e5 is a 10 pixel length under the water's 4000).
    pub pull: f32,
    /// The skin's pull between drops for a unit of `pull` (2 is the plane's own figure for this
    /// kernel and reach).
    pub coh: f32,
    /// The skin's pull along the surface's bend, as a share of the pull between drops.
    pub curv: f32,
    /// The furthest the skin's pull may move a drop in a step from rest, in cells: the pull is
    /// capped so it stays stable.
    pub pull_cfl: f32,
    /// The pull toward a near face at Stickiness 1, in gravities.
    pub adh: f32,
    /// The drag along a face every liquid has, and what Stickiness 1 adds, a second.
    pub drag: f32,
    pub drag_stick: f32,
    /// How far from a face drag reaches, in cells.
    pub drag_reach: f32,
    /// How strongly liquid stretched thin from inside is drawn back together, as a share of how
    /// strongly over-full liquid is pushed apart (0: never, the pressure only pushes).
    pub tens: f32,
    /// How full a drop's neighbourhood must be before it is drawn back at all.
    pub tens_low: f32,
    /// The share of the skin's pull between drops that acts inside the liquid (1: all of it).
    pub coh_in: f32,
}

impl Default for DropNumbers {
    fn default() -> Self {
        DropNumbers {
            rate: 960,
            iters: 4,
            reach: 1.5,
            relax: 1.0,
            eps: 4.0,
            warm: 0.0,
            warm_most: 0.1,
            wall_off: 0.25,
            wall_share: 1.0,
            sub_move: 0.4,
            most_move: 4.0,
            most_fix: 0.25,
            visc: 150.0,
            pull: 4.0e5,
            coh: 2.0,
            curv: 0.25,
            pull_cfl: 0.005,
            adh: 2.0,
            drag: 2.0,
            drag_stick: 30.0,
            drag_reach: 0.75,
            tens: 0.0,
            tens_low: 0.7,
            coh_in: 1.0,
        }
    }
}

impl DropNumbers {
    /// The numbers for a layout of `cell`-pixel cells: twice the grid core's steps a second (the
    /// drops are half a cell apart, and what moves them reaches three drops, not a cell).
    pub fn for_cell(cell: f32) -> DropNumbers {
        let rate = 2 * crate::Numbers::for_cell(cell).rate;
        DropNumbers { rate: rate.min(1920), ..DropNumbers::default() }
    }

    /// [`DropNumbers::for_cell`], each replaced by its `BPE_DROPS_*` environment variable when
    /// that is set. For the measuring tests only.
    pub fn from_env_for(cell: f32) -> DropNumbers {
        fn num<T: std::str::FromStr>(name: &str, or: T) -> T {
            std::env::var(format!("BPE_DROPS_{name}")).ok().and_then(|s| s.parse().ok()).unwrap_or(or)
        }
        let d = DropNumbers::for_cell(cell);
        DropNumbers {
            rate: num("RATE", d.rate),
            iters: num("ITERS", d.iters),
            reach: num("REACH", d.reach),
            relax: num("RELAX", d.relax),
            eps: num("EPS", d.eps),
            warm: num("WARM", d.warm),
            warm_most: num("WARM_MOST", d.warm_most),
            wall_off: num("WALL_OFF", d.wall_off),
            wall_share: num("WALL_SHARE", d.wall_share),
            sub_move: num("SUB_MOVE", d.sub_move),
            most_move: num("MOST_MOVE", d.most_move),
            most_fix: num("MOST_FIX", d.most_fix),
            visc: num("VISC", d.visc),
            pull: num("PULL", d.pull),
            coh: num("COH", d.coh),
            curv: num("CURV", d.curv),
            pull_cfl: num("PULL_CFL", d.pull_cfl),
            adh: num("ADH", d.adh),
            drag: num("DRAG", d.drag),
            drag_stick: num("DRAG_STICK", d.drag_stick),
            drag_reach: num("DRAG_REACH", d.drag_reach),
            tens: num("TENS", d.tens),
            tens_low: num("TENS_LOW", d.tens_low),
            coh_in: num("COH_IN", d.coh_in),
        }
    }

    /// Seconds a liquid step.
    pub fn dt(&self) -> f32 {
        1.0 / self.rate.max(1) as f32
    }

    /// The density at rest (drops on a square lattice half a cell apart) and a drop's own
    /// stiffness there (the sum of its neighbours' slopes squared over the density squared), for
    /// this reach.
    pub fn at_rest(&self) -> (f32, f32) {
        let h = f64::from(self.reach);
        let (mut rho, mut sq) = (0.0f64, 0.0f64);
        let n = (h / 0.5).ceil() as i32 + 1;
        for j in -n..=n {
            for i in -n..=n {
                let far = f64::from(i * i + j * j).sqrt() * 0.5;
                if far >= h {
                    continue;
                }
                let t = 1.0 - far / h;
                rho += 10.0 / (std::f64::consts::PI * h * h) * t * t * t;
                if i != 0 || j != 0 {
                    let g = 30.0 / (std::f64::consts::PI * h * h * h) * t * t;
                    sq += g * g;
                }
            }
        }
        (rho as f32, (sq / (rho * rho)) as f32)
    }

    /// The solids' share of a drop's neighbourhood: for 33 distances from a flat face (0 to a
    /// reach), the share of the kernel behind the face, and how fast that share falls a reach.
    pub fn wall_table() -> [[f32; 4]; 17] {
        // The kernel (1 - r)^3 summed along the chord at `s` from the middle.
        let chord = |s: f64| {
            let half = (1.0 - s * s).max(0.0).sqrt();
            let n = 400;
            (0..n)
                .map(|k| {
                    let t = (k as f64 + 0.5) / n as f64 * half;
                    let r = (s * s + t * t).sqrt();
                    2.0 * (1.0 - r).max(0.0).powi(3) * half / n as f64
                })
                .sum::<f64>()
        };
        let all = std::f64::consts::PI / 10.0;
        let mut flat = [0.0f32; 68];
        let mut behind = 0.0f64;
        let fine = 16;
        for k in (0..=32usize).rev() {
            let q = k as f64 / 32.0;
            flat[2 * k] = (behind / all) as f32;
            flat[2 * k + 1] = (chord(q) / all) as f32;
            if k > 0 {
                for m in 0..fine {
                    let s = q - (m as f64 + 0.5) / (32.0 * fine as f64);
                    behind += chord(s) / (32.0 * fine as f64);
                }
            }
        }
        let mut out = [[0.0f32; 4]; 17];
        for (k, v) in flat.iter().enumerate() {
            out[k / 4][k % 4] = *v;
        }
        out
    }
}

/// A liquid's numbers on the card. Matches `Feel` in alldrops.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct DropFeelGpu {
    /// gravity x, the skin's pull between drops, its pull along the bend, the pull to a face
    pub a: [f32; 4],
    /// damping (a second), drag along a face (a second), the skin's share inside the liquid, 0
    pub b: [f32; 4],
}

impl DropNumbers {
    /// `feel` on a layout of `cell`-pixel cells under a gravity of `gravity` cells a second
    /// squared.
    pub fn feel(&self, feel: &Feel, cell: f32, gravity: f32) -> DropFeelGpu {
        let dt = self.dt();
        let skin = self.coh * self.pull * feel.surface_pull * feel.surface_pull / (cell * cell * cell);
        let skin = skin.min(self.pull_cfl / (dt * dt)).max(0.0);
        let stick = feel.stickiness.max(0.0);
        DropFeelGpu {
            a: [feel.gravity, skin, skin * self.curv, self.adh * stick * gravity * feel.gravity.abs()],
            b: [8.0 * feel.settles * feel.settles, self.drag + self.drag_stick * stick, self.coh_in, 0.0],
        }
    }
}

/// One liquid step's numbers. Matches `St` in alldrops.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct DropStepGpu {
    pub grid: [u32; 4],
    pub cells: [u32; 4],
    pub time: [f32; 4],
    pub k: [f32; 4],
    pub w: [f32; 4],
    pub s: [f32; 4],
    pub feels: [DropFeelGpu; MOST_LIQUIDS],
    pub wall: [[f32; 4]; 17],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_density_at_rest_is_four_drops_a_cell() {
        // (The kernel sums to 1 over the plane, and four drops fill a cell.)
        let (rho, stiff) = DropNumbers::default().at_rest();
        assert!((rho - 4.0).abs() < 0.4, "{rho}");
        let table = DropNumbers::wall_table();
        assert!((table[0][0] - 0.5).abs() < 0.01, "half the neighbourhood is behind a face it touches: {table:?}");
        assert!(table[16][0].abs() < 1e-6 && table[8][0] > 0.0);
        assert!(stiff > 0.0 && stiff.is_finite());
        assert_eq!(DropNumbers::for_cell(6.0).rate, 960);
        assert_eq!(DropNumbers::for_cell(2.0).rate, 1920);
    }
}
