//! The all-drops engine's numbers ([`Numbers`]) and what a liquid is like ([`Feel`]).

use bpe_core::maths::Maths;
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
    /// 1: deep water is held up by the grid's pressure underneath (`deep.wgsl`); 0: never.
    pub deep: u32,
    /// Pixels: no push within `deep_d` of a cell that is not full, all of it deeper than
    /// `deep_d + deep_b` (each at least two cells).
    pub deep_d: f32,
    pub deep_b: f32,
    /// The fill (the drops' volume and the solids' share) from which a cell is full.
    pub deep_full: f32,
    /// The most cells the push moves a drop in a step.
    pub deep_most: f32,
    /// V-cycles of the pressure a step: each sweeps the cells, hands what they still lack to a grid
    /// half as fine (and on down to a handful of cells), then adds the coarser answers back on the
    /// way up. `prime` on the first step the deep part runs after it did not (a pool there at the
    /// start, or water just grown deep), so the pool's weight is held before anything moves.
    pub cycles: u32,
    pub prime: u32,
    /// Sweeps on a level on the way down, on the way up, and on the coarsest.
    pub pre: u32,
    pub post: u32,
    pub coarse: u32,
    /// The sweeps' over-relaxation, and how much of a coarser level's answer is added.
    pub omega: f32,
    pub scale: f32,
    /// The share of a cell's excess fill pushed out a step, and the most a step (cells of volume).
    /// (0.05 left the deep pool moving at 2 px/s at Fine and 4 at Extreme on the owner's card; 0.02
    /// at 1.5 and 3.)
    pub push_out: f32,
    pub most_out: f32,
    /// The share of a cell's room that may fill a step before it pushes back.
    pub fill_rate: f32,
    /// The least mass on a face for it to have a speed (a drop's is 0.25).
    pub face_floor: f32,
    /// The share of the speed at which drops close on each other (along the line between them,
    /// and only while they close, each of a pair alike so momentum is kept) taken away in the
    /// time a drop takes to cross the same share of a spacing as at Fine's 16 steps a frame: drops
    /// that meet do not bounce apart, at every detail and step count alike ([`Self::meet_share`]).
    /// Parting and sliding past each other are left alone.
    pub meet_even: f32,
    /// The deep water rests as full as the drops-only water just above it (the drops there are
    /// squeezed a little by the water over them), at most this much fuller than at rest (0: always
    /// as at rest). Resting any less full, the deep water is lighter than the water on it, and the
    /// pool's top turns over and over.
    pub rest_most: f32,
    /// 1: the card finds trapped air under the water each frame and fills it with air drops
    /// (part 2c, `shaders/air.wgsl`; it needs the deep part's cells, `deep` 1); 0: never. Air drops
    /// already there rise and burst either way.
    pub air: u32,
    /// How many times the course's gravity an air drop falls upward at (the Water card's Bubbles
    /// rise). At 1 a pocket starts up at one gravity, as real air under water does in the plane: its
    /// lift and the water's push on it against its own and the water's inertia.
    pub air_lift: f32,
    /// An air drop whose neighbourhood is less full than this has open air beside it and bursts (a
    /// drop at a flat open surface is about 0.67 full, one inside a pocket about 1).
    pub air_pop: f32,
    /// How far from a cell that is not full the card looks for the water over it and round it,
    /// pixels (at least three cells): a hole wider or taller than twice this is left as a hole.
    pub air_reach: f32,
    /// The share of the evening of speeds and of the closing loss that air and liquid have between
    /// them (1 as liquid has with liquid): air slips through the water this much more freely, so
    /// small bubbles rise rather than ride the water's flow.
    pub air_drag: f32,
    /// The most air drops born in a frame.
    pub air_most: u32,
    /// The share of the drag along a face every liquid has that a waxy face (Wetting 0) keeps; a
    /// soaked one (Wetting 1) keeps it all.
    pub wet_drag: f32,
    /// How much a wetted face counts in the skin's bend (part 3, hand-over item 0): its resting
    /// lattice's drops as neighbours with no surface of their own, times this. At 0 (part 2c) a
    /// wetted face was liquid only to the skin's pull and to the way the liquid lies, so the bend
    /// held a soaked floor's edge up at about 72° at every detail; at 1 a cap's edge follows Young's
    /// law (cos θ = 2 Wetting - 1) from Wetting 0.5 on, within 8° at 6, 3 and 2 px under a tenth of
    /// gravity, Wetting 1 spreads into a film at every detail, and Wetting 0 is as it was (its
    /// table is not used) (part 3's plan, "As built and measured").
    pub wet_bend: f32,
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
            deep: 1,
            deep_d: 24.0,
            deep_b: 16.0,
            deep_full: 0.9,
            deep_most: 0.25,
            cycles: 1,
            prime: 12,
            pre: 2,
            post: 2,
            coarse: 8,
            omega: 1.0,
            scale: 1.0,
            push_out: 0.02,
            most_out: 0.1,
            fill_rate: 0.5,
            face_floor: 0.005,
            meet_even: 0.3,
            rest_most: 0.02,
            air: 1,
            air_lift: 1.0,
            air_pop: 0.75,
            air_reach: 48.0,
            air_drag: 0.2,
            air_most: 65_536,
            wet_drag: 1.0,
            wet_bend: 1.0,
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
        // (At 2 px, Final, the deep pool's pressure needs a second V-cycle a step: with one it
        // moved at 15 to 20 px/s on the owner's card, with two at 5 to 6. At 3 px and coarser the
        // second bought little for a fifth more time a step.)
        let cycles = if cell < 2.5 { 2 } else { 1 };
        Numbers { floor, cycles, ..Numbers::default() }
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
            deep: num("DEEP", d.deep),
            deep_d: num("DEEP_D", d.deep_d),
            deep_b: num("DEEP_B", d.deep_b),
            deep_full: num("DEEP_FULL", d.deep_full),
            deep_most: num("DEEP_MOST", d.deep_most),
            cycles: num("CYCLES", d.cycles),
            prime: num("PRIME", d.prime),
            pre: num("PRE", d.pre),
            post: num("POST", d.post),
            coarse: num("COARSE", d.coarse),
            omega: num("OMEGA", d.omega),
            scale: num("SCALE", d.scale),
            push_out: num("PUSH_OUT", d.push_out),
            most_out: num("MOST_OUT", d.most_out),
            fill_rate: num("FILL_RATE", d.fill_rate),
            face_floor: num("FACE_FLOOR", d.face_floor),
            meet_even: num("MEET_EVEN", d.meet_even),
            rest_most: num("REST_MOST", d.rest_most),
            air: num("AIR", d.air),
            air_lift: num("AIR_LIFT", d.air_lift),
            air_pop: num("AIR_POP", d.air_pop),
            air_reach: num("AIR_REACH", d.air_reach),
            air_drag: num("AIR_DRAG", d.air_drag),
            air_most: num("AIR_MOST", d.air_most),
            wet_drag: num("WET_DRAG", d.wet_drag),
            wet_bend: num("WET_BEND", d.wet_bend),
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

    /// A wetted solid as resting liquid to a drop's skin (Wetting, part 2c): for 33 distances from a
    /// flat face (0 to a reach), what the drops of a resting lattice filling the solid behind the
    /// face (rows half a cell apart from a quarter cell in) would add to the drop's skin, each as a
    /// number times the face's outward way: (x) the skin's pull (`skin`, summed as `finish_at` sums
    /// it, so a pull toward the face is above 0), and (y) the smooth kernel's slope (as `finish_at`'s
    /// `lies`: the liquid lies toward the face by this much). Averaged over where along the lattice
    /// the drop is; for this reach, two to an entry.
    pub fn wet_table(&self) -> [[f32; 4]; 17] {
        let h = f64::from(self.reach.max(0.5));
        let skin = |q: f64| {
            let t = (1.0 - q) * q;
            let c = 64.0 * t * t * t;
            if q > 0.5 { c } else { 2.0 * c - 1.0 }
        };
        let smooth = 24.0 / (std::f64::consts::PI * h * h * h * h);
        let across = (h / 0.5).ceil() as i64 + 1;
        let phases = 16;
        let mut flat = [0.0f32; 68];
        for k in 0..=32usize {
            let d = k as f64 / 32.0 * h;
            let (mut pull, mut lies) = (0.0f64, 0.0f64);
            for phase in 0..phases {
                let x0 = (phase as f64 + 0.5) / phases as f64 * 0.5;
                for row in 0.. {
                    let dy = d + 0.25 + 0.5 * f64::from(row);
                    if dy >= h {
                        break;
                    }
                    for c in -across..=across {
                        let dx = x0 + 0.5 * c as f64;
                        let r2 = dx * dx + dy * dy;
                        if r2 >= h * h {
                            continue;
                        }
                        let far = r2.sqrt();
                        let along = dy / far;
                        let t = 1.0 - r2 / (h * h);
                        pull += skin(far / h) * along;
                        lies += far * smooth * t * t * along;
                    }
                }
            }
            flat[2 * k] = (pull / f64::from(phases)) as f32;
            flat[2 * k + 1] = (lies / f64::from(phases)) as f32;
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

    /// The deep water's reach on `cell`-pixel cells, in cells: (no push within this of a cell that
    /// is not full, all of it this much further in, how far the depth is looked for). Each of the
    /// first two is at least two cells, and the third covers both and one more.
    pub fn deep_window(&self, cell: f32) -> (f32, f32, u32) {
        let cell = cell.max(1e-3);
        let d = (self.deep_d / cell).max(2.0);
        let b = (self.deep_b / cell).max(2.0);
        let reach = ((d + b).ceil() as u32 + 1).min(256);
        (d, b, reach)
    }

    /// How far the card looks from an empty cell for trapped air on `cell`-pixel cells, in cells
    /// (`air_reach`, at least three cells and at most 64).
    pub fn air_window(&self, cell: f32) -> u32 {
        let r = (self.air_reach / cell.max(1e-3)).round();
        if r.is_finite() { (r as u32).clamp(3, 64) } else { 3 }
    }

    /// The share of drops' closing speed taken away in one step of `dt` seconds on `cell`-pixel
    /// cells: `meet_even` for the step that crosses the same share of a spacing as Fine's (6 px
    /// cells, 16 steps a frame: `FRAME * cell / 96` seconds), and for any other step what keeps
    /// the share a second the same (so a frame of 32 steps at 2 px, or a frame whose fast water
    /// took more steps, loses as much as the reference's steps would).
    pub fn meet_share(&self, dt: f32, cell: f32) -> f32 {
        let s = self.meet_even.clamp(0.0, 1.0);
        let reference = FRAME * cell.max(1e-3) / 96.0;
        let steps = dt.max(0.0) / reference;
        if s <= 0.0 || !steps.is_finite() {
            return 0.0;
        }
        1.0 - (1.0 - s).det_powf(steps)
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
        for (cell, cycles) in [(12.0, 1), (6.0, 1), (3.0, 1), (2.0, 2), (1.5, 2)] {
            assert_eq!(Numbers::for_cell(cell).cycles, cycles, "{cell} px");
        }
    }

    #[test]
    fn drops_that_meet_lose_the_same_share_a_second_at_any_step() {
        let n = Numbers::default();
        let s = n.meet_even;
        let near = |a: f32, b: f32| (a - b).abs() < 1e-5;
        // (The reference steps: 16 a frame at 6 px, 32 at 3 px, 8 at 12 px.)
        assert!(near(n.meet_share(FRAME / 16.0, 6.0), s));
        assert!(near(n.meet_share(FRAME / 32.0, 3.0), s));
        assert!(near(n.meet_share(FRAME / 8.0, 12.0), s));
        // (A frame of 32 steps at 6 px, or at 2 px where the reference is 48, keeps what is left
        // after a frame the same.)
        let left = |dt: f32, cell: f32, steps: i32| (1.0 - n.meet_share(dt, cell)).powi(steps);
        let fine = (1.0 - s).powi(16);
        assert!(near(left(FRAME / 32.0, 6.0, 32), fine), "{}", left(FRAME / 32.0, 6.0, 32));
        assert!((left(FRAME / 32.0, 2.0, 32) - (1.0 - s).powi(48)).abs() < 1e-6);
        assert!(n.meet_share(FRAME / 32.0, 2.0) > s, "a 2 px step crosses more of a spacing");
        assert_eq!(Numbers { meet_even: 0.0, ..n }.meet_share(FRAME / 16.0, 6.0), 0.0);
        assert!(near(Numbers { meet_even: 1.0, ..n }.meet_share(FRAME / 64.0, 6.0), 1.0));
        assert_eq!(n.meet_share(f32::NAN, 6.0), 0.0);
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
    fn the_window_covers_the_blend() {
        let n = Numbers::default();
        for cell in [1.5f32, 2.0, 3.0, 6.0, 8.0, 12.0] {
            let (d, b, reach) = n.deep_window(cell);
            assert!(d >= 2.0 && b >= 2.0, "{cell} px: {d} {b}");
            assert!(reach as f32 >= d + b + 1.0, "{cell} px: the depth is looked for far enough ({reach})");
            assert!((d - (24.0 / cell).max(2.0)).abs() < 1e-5, "{cell} px");
        }
        assert_eq!(n.deep_window(12.0), (2.0, 2.0, 5));
    }

    #[test]
    fn a_wetted_face_is_resting_liquid_to_the_skin() {
        let n = Numbers::default();
        let table = n.wet_table();
        let at = |k: usize| {
            let (e, i) = (k / 2, (k % 2) * 2);
            (table[e][i], table[e][i + 1])
        };
        // (A drop resting on the face, a quarter cell off it: 0.25 / 1.5 of a reach.)
        let rest = at(((0.25 / n.reach) * 32.0).round() as usize);
        assert!(rest.0 > 0.0, "the liquid in the face pulls a drop resting on it: {rest:?}");
        assert!(rest.1 > 0.0, "and the liquid lies toward the face: {rest:?}");
        assert_eq!(at(32), (0.0, 0.0), "a reach off the face, nothing");
        for k in 0..32 {
            assert!(at(k).1 >= at(k + 1).1 - 1e-6, "the slope falls off the face: {k} {:?} {:?}", at(k), at(k + 1));
        }
        // A drop resting on a wetted face feels, from the lattice in it, what a drop in a resting
        // pool's top row feels from the rows under it: the same lattice, half a cell on.
        let h = n.reach;
        let (mut below, mut lies) = (0.0f32, 0.0f32);
        for row in 1..4 {
            for c in -4..=4 {
                for phase in 0..16 {
                    let dx = (phase as f32 + 0.5) / 16.0 * 0.5 + 0.5 * c as f32;
                    let dy = 0.5 * row as f32;
                    let far = (dx * dx + dy * dy).sqrt();
                    if far < h {
                        let q = far / h;
                        let t = (1.0 - q) * q;
                        let c3 = 64.0 * t * t * t;
                        below += if q > 0.5 { c3 } else { 2.0 * c3 - 1.0 } * dy / far / 16.0;
                        let s = 1.0 - far * far / (h * h);
                        lies += far * 24.0 / (std::f32::consts::PI * h * h * h * h) * s * s * dy / far / 16.0;
                    }
                }
            }
        }
        let k = ((0.25 / h) * 32.0) as usize;
        let f = (0.25 / h) * 32.0 - k as f32;
        let mid = (at(k).0 + (at(k + 1).0 - at(k).0) * f, at(k).1 + (at(k + 1).1 - at(k).1) * f);
        assert!((mid.0 - below).abs() < 0.05 * below.abs(), "{mid:?} against {below}");
        assert!((mid.1 - lies).abs() < 0.05 * lies.abs(), "{mid:?} against {lies}");
    }

    #[test]
    fn trapped_air_is_looked_for_a_few_cells_round() {
        let n = Numbers::default();
        assert_eq!(n.air_window(6.0), 8);
        assert_eq!(n.air_window(3.0), 16);
        assert_eq!(n.air_window(12.0), 4);
        assert_eq!(n.air_window(24.0), 3, "at least three cells");
        assert_eq!(n.air_window(0.5), 64, "at most 64");
        assert_eq!(Numbers { air_reach: f32::NAN, ..n }.air_window(6.0), 3);
    }

    #[test]
    fn thickness_sets_the_evening() {
        assert!((Numbers::default().evening(&Feel::default()) - 150.0).abs() < 1e-3);
    }
}
