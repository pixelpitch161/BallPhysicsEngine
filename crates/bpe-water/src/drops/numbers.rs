//! The all-drops engine's numbers ([`Numbers`]) and what a liquid is like ([`Feel`]).

use bytemuck::{Pod, Zeroable};

/// The most liquids a body holds.
pub const MOST_LIQUIDS: usize = 16;
/// A frame's length, seconds: a body moves on a frame at a time, cut into steps.
pub const FRAME: f32 = 1.0 / 60.0;

/// What a liquid is like: the Feel card's settings, as the engine takes them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Feel {
    /// How many times the course's gravity it falls at.
    pub gravity: f32,
    /// How strongly it pulls itself together: 0 is spray (no skin), 1 ropes.
    pub surface_pull: f32,
    /// How thick it is: 0 runs like water, higher is syrup, then honey, then paste.
    pub thickness: f32,
    /// How much it clings to the course.
    pub stickiness: f32,
    /// How heavy it is (water is 1).
    pub weight: f32,
    /// How fast its motion dies down (0: never).
    pub settles: f32,
}

impl Default for Feel {
    fn default() -> Self {
        Feel { gravity: 1.0, surface_pull: 0.5, thickness: 0.02, stickiness: 0.0, weight: 1.0, settles: 0.0 }
    }
}

impl From<&Feel> for crate::grid::Feel {
    /// The same liquid for the grid core (which has no Thickness; its spray stays at its own default).
    fn from(f: &Feel) -> Self {
        crate::grid::Feel {
            gravity: f.gravity,
            surface_pull: f.surface_pull,
            stickiness: f.stickiness,
            weight: f.weight,
            settles: f.settles,
            ..crate::grid::Feel::default()
        }
    }
}

/// The all-drops engine's numbers. The defaults are the ones the liquid ships with
/// ([`Numbers::for_cell`] picks the fewest steps a frame for a cell size); the measuring tests may
/// replace any by a `BPE_WATER_*` environment variable ([`Numbers::from_env_for`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Numbers {
    /// The fewest steps a frame.
    pub floor: u32,
    /// The most steps a frame: the trial's 32 (1920 a second). The skin's bend is only calm up to
    /// about 40 steps a frame (above that it throws a pour's drops back out of the spout), so a
    /// frame never takes more, and a skin that wants more steps is capped instead.
    pub most_steps: u32,
    /// The furthest the fastest drop moves in a step, in drop spacings (half a cell).
    pub move_most: f32,
    /// Sweeps of the pressure a step.
    pub iters: u32,
    /// How far a drop feels its neighbours, in cells (drops at rest are half a cell apart).
    pub reach: f32,
    /// The share of a sweep's correction taken.
    pub relax: f32,
    /// The softening of a correction, as a share of a drop's own stiffness at rest.
    pub eps: f32,
    /// How far off a solid's face a drop stays, in cells.
    pub wall_off: f32,
    /// How much of the solids counts as liquid for the density (1: all of it).
    pub wall_share: f32,
    /// A sub-move's length, in cells.
    pub sub_move: f32,
    /// The most cells a drop moves in a step (the speed cap is this over the step).
    pub most_move: f32,
    /// The furthest a sweep moves a drop, in cells.
    pub most_fix: f32,
    /// How fast neighbouring drops even out their speeds, pixels squared a second: `visc_base`
    /// for every liquid, and `visc_thick` more for each unit of Thickness.
    pub visc_base: f32,
    pub visc_thick: f32,
    /// The skin of a liquid at Surface pull 1, in pixels cubed a second squared (its capillary
    /// length squared times the gravity: 4.0e5 is a 10 pixel length under the water's 4000).
    pub pull: f32,
    /// The skin's pull between drops for a unit of `pull` (2 is the plane's own figure for this
    /// kernel and reach).
    pub coh: f32,
    /// The skin's pull along the surface's bend, as a share of the pull between drops.
    pub curv: f32,
    /// The furthest the skin's pull may move a drop in a step from rest, in cells: the frame takes
    /// the steps for this, and only at `most_steps` is the pull capped.
    pub pull_cfl: f32,
    /// The pull toward a near face at Stickiness 1, in gravities.
    pub adh: f32,
    /// The drag along a face every liquid has, and what Stickiness 1 adds, a second.
    pub drag: f32,
    pub drag_stick: f32,
    /// How far from a face drag reaches, in cells.
    pub drag_reach: f32,
    /// How full a spout's neighbourhood must be for its pour to hold back.
    pub full_spout: f32,
    /// The most rows a held pour may owe.
    pub held_rows: u32,
    /// 1: a step's end is joined with the next step's fall, one pass fewer a step (the same water
    /// to the bit either way; kept while the card run times both).
    pub join: u32,
}

impl Default for Numbers {
    fn default() -> Self {
        Numbers {
            floor: 16,
            most_steps: 32,
            move_most: 1.3,
            iters: 4,
            reach: 1.5,
            relax: 1.0,
            eps: 4.0,
            wall_off: 0.25,
            wall_share: 1.0,
            sub_move: 0.4,
            most_move: 4.0,
            most_fix: 0.25,
            visc_base: 100.0,
            visc_thick: 2500.0,
            pull: 4.0e5,
            coh: 2.0,
            curv: 0.25,
            pull_cfl: 0.005,
            adh: 2.0,
            drag: 2.0,
            drag_stick: 30.0,
            drag_reach: 0.75,
            full_spout: 0.9,
            held_rows: 4,
            join: 1,
        }
    }
}

impl Numbers {
    /// The numbers for a layout of `cell`-pixel cells: 8 steps a frame at Draft's coarse 12 pixels,
    /// 16 at 8 and 6, 32 at 3 and finer (the trial's 480, 960 and 1920 a second).
    pub fn for_cell(cell: f32) -> Numbers {
        let floor = if cell >= 10.0 {
            8
        } else if cell >= 4.0 {
            16
        } else {
            32
        };
        Numbers { floor, ..Numbers::default() }
    }

    /// [`Numbers::for_cell`], each replaced by its `BPE_WATER_*` environment variable when that is
    /// set. For the measuring tests only.
    pub fn from_env_for(cell: f32) -> Numbers {
        fn num<T: std::str::FromStr>(name: &str, or: T) -> T {
            std::env::var(format!("BPE_WATER_{name}")).ok().and_then(|s| s.parse().ok()).unwrap_or(or)
        }
        let d = Numbers::for_cell(cell);
        Numbers {
            floor: num("FLOOR", d.floor),
            most_steps: num("MOST_STEPS", d.most_steps),
            move_most: num("MOVE_MOST", d.move_most),
            iters: num("ITERS", d.iters),
            reach: num("REACH", d.reach),
            relax: num("RELAX", d.relax),
            eps: num("EPS", d.eps),
            wall_off: num("WALL_OFF", d.wall_off),
            wall_share: num("WALL_SHARE", d.wall_share),
            sub_move: num("SUB_MOVE", d.sub_move),
            most_move: num("MOST_MOVE", d.most_move),
            most_fix: num("MOST_FIX", d.most_fix),
            visc_base: num("VISC_BASE", d.visc_base),
            visc_thick: num("VISC_THICK", d.visc_thick),
            pull: num("PULL", d.pull),
            coh: num("COH", d.coh),
            curv: num("CURV", d.curv),
            pull_cfl: num("PULL_CFL", d.pull_cfl),
            adh: num("ADH", d.adh),
            drag: num("DRAG", d.drag),
            drag_stick: num("DRAG_STICK", d.drag_stick),
            drag_reach: num("DRAG_REACH", d.drag_reach),
            full_spout: num("FULL_SPOUT", d.full_spout),
            held_rows: num("HELD_ROWS", d.held_rows),
            join: num("JOIN", d.join),
        }
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

    /// The skin of `feel` on `cell`-pixel cells, uncapped: its pull between drops in cells a
    /// second squared (Surface pull squared, so the slider's middle is a quarter of its top).
    pub fn skin(&self, feel: &Feel, cell: f32) -> f32 {
        let sp = feel.surface_pull.max(0.0);
        (self.coh * self.pull * sp * sp / (cell * cell * cell)).max(0.0)
    }

    /// How fast `feel`'s neighbouring drops even out their speeds, pixels squared a second.
    pub fn evening(&self, feel: &Feel) -> f32 {
        (self.visc_base + self.visc_thick * feel.thickness.max(0.0)).max(0.0)
    }

    /// The steps a frame takes when its fastest drop moves at `fastest` pixels a second (with any
    /// headroom already added), for the liquids `feels` on `cell`-pixel cells.
    pub fn steps_for(&self, fastest: f32, feels: &[Feel], cell: f32) -> u32 {
        let per_second = 1.0 / FRAME;
        let spacing = cell / 2.0;
        let fastest = if fastest.is_finite() { fastest.max(0.0) } else { 1e30 };
        let n_speed = (fastest / per_second / (self.move_most.max(0.05) * spacing)).ceil().min(1e6) as u32;
        let skin = feels.iter().map(|f| self.skin(f, cell)).fold(0.0f32, f32::max);
        let n_skin = ((skin / self.pull_cfl.max(1e-9)).sqrt() / per_second).ceil().min(1e6) as u32;
        self.floor.max(n_speed).max(n_skin).clamp(1, self.most_steps.max(1))
    }

    /// `feel` on the card, for steps of `dt` seconds on `cell`-pixel cells under a gravity of
    /// `gravity` cells a second squared. The skin is capped (so it stays stable) only when `capped`:
    /// a frame at `most_steps` that the skin wanted more for.
    pub fn feel_gpu(&self, feel: &Feel, cell: f32, gravity: f32, dt: f32, capped: bool) -> FeelGpu {
        let mut skin = self.skin(feel, cell);
        if capped {
            skin = skin.min(self.pull_cfl / (dt * dt));
        }
        let stick = feel.stickiness.max(0.0);
        let reach_px = self.reach * cell;
        let even = (self.evening(feel) * dt * 20.0 / (reach_px * reach_px)).min(0.5);
        FeelGpu {
            a: [feel.gravity, skin, skin * self.curv, self.adh * stick * gravity * feel.gravity.abs()],
            b: [
                8.0 * feel.settles * feel.settles,
                self.drag + self.drag_stick * stick,
                even,
                feel.weight.clamp(0.05, 16.0),
            ],
        }
    }
}

/// A liquid's numbers on the card. Matches `Feel` in the engine's common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct FeelGpu {
    /// gravity x, the skin's pull between drops, its pull along the bend, the pull to a face
    /// (cells a second squared)
    pub a: [f32; 4],
    /// damping (a second), drag along a face (a second), the share of the neighbours' mean speed
    /// taken a step, weight
    pub b: [f32; 4],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_density_at_rest_is_four_drops_a_cell() {
        // (The kernel sums to 1 over the plane, and four drops fill a cell.)
        let (rho, stiff) = Numbers::default().at_rest();
        assert!((rho - 4.0).abs() < 0.4, "{rho}");
        let table = Numbers::wall_table();
        assert!((table[0][0] - 0.5).abs() < 0.01, "half the neighbourhood is behind a face it touches: {table:?}");
        assert!(table[16][0].abs() < 1e-6 && table[8][0] > 0.0);
        assert!(stiff > 0.0 && stiff.is_finite());
    }

    #[test]
    fn floors_follow_the_detail() {
        for (cell, floor) in [(12.0, 8), (8.0, 16), (6.0, 16), (3.0, 32), (2.0, 32), (1.5, 32)] {
            assert_eq!(Numbers::for_cell(cell).floor, floor, "{cell} px");
        }
    }

    #[test]
    fn the_skin_gets_its_steps() {
        // (The skin's own rule, with no ceiling on the steps...)
        let at = |sp: f32, cell: f32| {
            let feel = Feel { surface_pull: sp, ..Feel::default() };
            Numbers { most_steps: 1024, ..Numbers::for_cell(cell) }.steps_for(0.0, &[feel], cell)
        };
        let near = |got: u32, want: u32| got.abs_diff(want) <= 1;
        assert!(near(at(1.0, 3.0), 41), "{}", at(1.0, 3.0));
        assert!(near(at(0.5, 2.0), 37), "{}", at(0.5, 2.0));
        assert!(near(at(1.0, 1.5), 115), "{}", at(1.0, 1.5));
        assert_eq!(at(0.5, 6.0), 16);
        assert_eq!(at(0.0, 1.5), 32, "no skin, no steps for it");
        // (...and with it: the skin is capped instead.)
        let feel = Feel { surface_pull: 1.0, ..Feel::default() };
        assert_eq!(Numbers::for_cell(1.5).steps_for(0.0, &[feel], 1.5), 32);
    }

    #[test]
    fn fast_water_gets_its_steps() {
        let n = Numbers::for_cell(6.0);
        let feels = [Feel::default()];
        assert_eq!(n.steps_for(3900.0, &feels, 6.0), 17);
        assert_eq!(n.steps_for(1e7, &feels, 6.0), 32);
        assert_eq!(n.steps_for(f32::INFINITY, &feels, 6.0), 32);
        assert_eq!(n.steps_for(f32::NAN, &feels, 6.0), 32);
        assert_eq!(n.steps_for(0.0, &[], 6.0), 16);
    }

    #[test]
    fn thickness_sets_the_evening() {
        assert!((Numbers::default().evening(&Feel::default()) - 150.0).abs() < 1e-3);
    }
}
