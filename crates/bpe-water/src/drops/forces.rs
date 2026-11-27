//! Forces on the water (Water mode part 4b): a **Magnet** that pulls its liquid in and stands it up in
//! spikes, a **Whirlpool** that spins the liquid round it, and **Wind** that blows what meets the air.
//!
//! A force is an input, as gravity is: the host hands the body its forces once a frame
//! ([`super::Water::set_forces`]) and nothing of them is kept from frame to frame. Every liquid drop the
//! force names (not trapped air) takes its push as it falls (`shaders/forces.wgsl`, from `fall`), after
//! gravity and before the speed cap. All lengths are world pixels and all speeds pixels a second, so a
//! force pushes alike at every detail.
//!
//! - **Magnet**: the slope of a potential `Φ = −A·R·Ψ(u / s)`, `u = r / R`, `Ψ(v) = (1 − v²)³`, so liquid
//!   comes to rest on a level of gravity and magnet together. Its reach's share `s` is whole in the
//!   middle of each of `spikes` teeth spread evenly over `spread` round `aim`, and `1 − depth` of it
//!   between them: `s = 1 − depth·c(u)·(1 − T)`, `T = (1 − |x|)³`, `x` the turn from a tooth's middle over
//!   its half width `w = SPIKE_WIDTH · spread / spikes` (neighbouring teeth just meet), its point rounded
//!   off within [`SPIKE_ROUND`] of the middle, `c(u) = smoothstep(SPIKE_FROM, SPIKE_WHOLE, u)`. Every
//!   level of the potential past `SPIKE_WHOLE` is the same star, so the liquid's skin stands up in cones
//!   with sharp tips and round valleys; inside `SPIKE_FROM` the pull is round, so the mound stays calm;
//!   along every line out of the middle the potential only rises, so the middle is always the deepest
//!   place. The spikes are laid out rather than left to the skin, so they stand in the same place at
//!   every detail.
//! - **Whirlpool**: the speed round its middle is drawn toward `speed · 2x / (1 + x²)`, `x = r / (0.3 R)`,
//!   at `rate` a second, fading out over the last 30 % of its reach.
//! - **Wind**: a drop's speed is drawn toward `velocity` at `rate` a second times how much of it meets the
//!   air (1 for a lone drop, about 0.1 at a flat open top, nothing inside the liquid).

use bpe_core::maths::Maths;
use bytemuck::{Pod, Zeroable};
use glam::Vec2;

use crate::solid::Layout;

/// The most forces a body feels at once.
pub const MOST_FORCES: usize = 16;
/// The pixels a second squared of the course's usual gravity (the Water card's default): the unit a
/// force's strength is told in.
pub const GRAVITY_UNIT: f32 = 4000.0;
/// The most one force pushes a drop, in [`GRAVITY_UNIT`]s (a guard against a hand-edited file).
pub const MOST_PUSH: f32 = 256.0;
/// The largest size of `Ψ'` (at `u = 1/√5`): a magnet of pull `A` and no spikes pulls at most `A` times
/// this.
pub const PSI_SLOPE_MOST: f32 = 1.717_337_2;
/// A tooth's half width as a share of the angle between two spikes (`SPIKE_WIDTH` in forces.wgsl).
pub const SPIKE_WIDTH: f32 = 0.5;
/// How far from a tooth's middle (a share of its half width) its point is rounded off (`SPIKE_ROUND`).
pub const SPIKE_ROUND: f32 = 0.1;
/// The deepest valleys between spikes: a magnet's `depth` is held to this.
pub const MOST_DEPTH: f32 = 0.9;
/// Where the teeth start and are whole, as shares of the reach (`c(u)`; the same in forces.wgsl).
pub const SPIKE_FROM: f32 = 0.2;
pub const SPIKE_WHOLE: f32 = 0.45;
/// A whirlpool's core (turning as one) as a share of its reach, and where it starts fading out.
pub const SPIN_CORE: f32 = 0.3;
pub const SPIN_FADE: f32 = 0.7;
/// The fullness at and above which a drop meets no air, and at and below which it is all in the air.
pub const OPEN_FULL: f32 = 0.75;
pub const OPEN_LONE: f32 = 0.35;

/// What a force does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Push {
    /// `pull`: `A`, pixels a second squared (below 0 it pushes away); `spikes` teeth over `spread`
    /// (radians) round `aim` (radians, 0 along +x, the course's y down), its reach between them `1 −
    /// depth` of its reach at their middles (`depth` 0 to [`MOST_DEPTH`]).
    Magnet { pull: f32, spikes: u32, depth: f32, aim: f32, spread: f32 },
    /// `speed`: pixels a second at the core's edge (above 0 it turns from +x toward +y: clockwise on the
    /// screen); `rate`: a second.
    Whirlpool { speed: f32, rate: f32 },
    /// `velocity`: pixels a second; `rate`: a second.
    Wind { velocity: Vec2, rate: f32 },
}

/// One force, in world pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Force {
    pub push: Push,
    /// Its middle (a Wind's is not used).
    pub place: Vec2,
    /// How far from its middle it reaches (a Wind reaches everywhere).
    pub reach: f32,
    /// Which liquids it moves: bit k is liquid k.
    pub liquids: u32,
}

/// A force on the card. Matches `Force` in common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ForceGpu {
    /// Its middle (grid units), its reach (cells), the most it pushes a drop (cells a second squared).
    pub a: [f32; 4],
    /// A magnet: its pull (cells a second squared), its spikes' depth, their aim and spread (radians). A
    /// whirlpool: its speed (cells a second), its rate. Wind: its velocity (cells a second), its rate.
    pub b: [f32; 4],
    /// Its kind (1 magnet, 2 whirlpool, 3 wind), the liquids it moves, a magnet's spikes, 0.
    pub c: [u32; 4],
}

fn finite(xs: &[f32]) -> bool {
    xs.iter().all(|x| x.is_finite())
}

fn smooth(lo: f32, hi: f32, x: f32) -> f32 {
    let t = ((x - lo) / (hi - lo)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Force {
    /// Whether it can be felt: finite numbers that push (a magnet's pull not 0, a rate above 0), a reach
    /// above 0 where it has one, and some liquid.
    pub fn is_felt(&self) -> bool {
        let numbers = match self.push {
            Push::Magnet { pull, depth, aim, spread, .. } => finite(&[pull, depth, aim, spread]) && pull != 0.0,
            Push::Whirlpool { speed, rate } => finite(&[speed, rate]) && rate > 0.0,
            Push::Wind { velocity, rate } => finite(&[velocity.x, velocity.y, rate]) && rate > 0.0,
        };
        let reaches = matches!(self.push, Push::Wind { .. }) || (self.reach.is_finite() && self.reach > 0.0);
        numbers && reaches && finite(&[self.place.x, self.place.y]) && self.liquids != 0
    }

    /// The push the frame's steps make room for, pixels a second squared, as they do for gravity: a magnet's
    /// pull without its spikes (Strength gravities; a spike's sharper slopes are thin bands the liquid rests
    /// across, and counting them took a 6 px frame to 32 steps, where the skin's bend stirred a Strength 4
    /// mound: its tips moved 3.1 px a second against 0.65 at the 16 steps of the floor), a whirlpool's and a
    /// wind's draw on a drop at rest. At most [`MOST_PUSH`] gravities.
    pub fn headroom(&self) -> f32 {
        let most = MOST_PUSH * GRAVITY_UNIT;
        let a = match self.push {
            Push::Magnet { pull, .. } => pull.abs() * PSI_SLOPE_MOST,
            Push::Whirlpool { speed, rate } => speed.abs() * rate.max(0.0),
            Push::Wind { velocity, rate } => velocity.length() * rate.max(0.0),
        };
        if a.is_finite() { a.min(most) } else { most }
    }

    /// The force on the card, on `layout`.
    pub fn gpu(&self, layout: &Layout) -> ForceGpu {
        let cell = layout.cell;
        let at = layout.to_grid(self.place);
        let most = MOST_PUSH * GRAVITY_UNIT / cell;
        let a = [at.x, at.y, (self.reach / cell).max(0.0), most];
        let (b, c) = match self.push {
            Push::Magnet { pull, spikes, depth, aim, spread } => {
                let spread = spread.clamp(0.0, std::f32::consts::TAU);
                ([pull / cell, depth.clamp(0.0, MOST_DEPTH), aim, spread], [1, self.liquids, spikes.min(64), 0])
            }
            Push::Whirlpool { speed, rate } => ([speed / cell, rate.max(0.0), 0.0, 0.0], [2, self.liquids, 0, 0]),
            Push::Wind { velocity, rate } => {
                ([velocity.x / cell, velocity.y / cell, rate.max(0.0), 0.0], [3, self.liquids, 0, 0])
            }
        };
        ForceGpu { a, b, c }
    }

    /// A magnet's share `s` of its reach and its slopes along `u` and the turn (a radian) at `u` and `x`,
    /// the turn from a tooth's middle over its half width (-1 to 1, and 1 past the spread); `(1, 0, 0)`
    /// for a magnet without spikes or any other force.
    fn shape(&self, u: f32, x: f32) -> (f32, f32, f32) {
        let Push::Magnet { spikes, depth, spread, .. } = self.push else { return (1.0, 0.0, 0.0) };
        let spread = spread.clamp(0.0, std::f32::consts::TAU);
        let depth = depth.clamp(0.0, MOST_DEPTH);
        if spikes == 0 || depth <= 0.0 || spread <= 0.0 {
            return (1.0, 0.0, 0.0);
        }
        let c = smooth(SPIKE_FROM, SPIKE_WHOLE, u);
        let dc = if u > SPIKE_FROM && u < SPIKE_WHOLE {
            let t = (u - SPIKE_FROM) / (SPIKE_WHOLE - SPIKE_FROM);
            6.0 * t * (1.0 - t) / (SPIKE_WHOLE - SPIKE_FROM)
        } else {
            0.0
        };
        let w = SPIKE_WIDTH * spread / spikes.min(64) as f32;
        let soft = (x * x + SPIKE_ROUND * SPIKE_ROUND).sqrt();
        let whole = (1.0 + SPIKE_ROUND * SPIKE_ROUND).sqrt() - SPIKE_ROUND;
        let t = (1.0 - (soft - SPIKE_ROUND) / whole).max(0.0);
        let tooth = t * t * t;
        let dtooth = -3.0 * t * t * x / (soft * whole * w);
        (1.0 - depth * c * (1.0 - tooth), -depth * dc * (1.0 - tooth), depth * c * dtooth)
    }

    /// The host's copy of the card's sums, for tests and measuring: [`Self::shape`] at `d` (pixels) from
    /// a magnet's middle.
    fn teeth(&self, d: Vec2) -> (f32, f32, f32) {
        let Push::Magnet { spikes, aim, spread, .. } = self.push else { return (1.0, 0.0, 0.0) };
        let u = d.length() / self.reach;
        let spread = spread.clamp(0.0, std::f32::consts::TAU);
        let tau = std::f32::consts::TAU;
        let mut th = d.y.det_atan2(d.x) - aim;
        th -= tau * ((th + std::f32::consts::PI) / tau).floor();
        if spikes == 0 || th.abs() > spread * 0.5 {
            // (Past the spread, the valleys' reach.)
            return self.shape(u, 1.0);
        }
        let n = spikes.min(64) as f32;
        let span = spread / n;
        let t = (th + spread * 0.5) / span;
        let k = t.floor().min(n - 1.0);
        self.shape(u, ((t - k - 0.5) / SPIKE_WIDTH).clamp(-1.0, 1.0))
    }

    /// A magnet's potential at world point `at` (pixels squared a second squared; 0 for any other force).
    pub fn magnet_potential(&self, at: Vec2) -> f32 {
        let Push::Magnet { pull, .. } = self.push else { return 0.0 };
        let d = at - self.place;
        if !self.is_felt() {
            return 0.0;
        }
        let v = d.length() / (self.reach * self.teeth(d).0);
        if v >= 1.0 {
            return 0.0;
        }
        let q = 1.0 - v * v;
        -pull * self.reach * q * q * q
    }

    /// A magnet's push at world point `at` (pixels a second squared; nothing for any other force), as the
    /// card works it out: down the slope of [`Self::magnet_potential`], held to [`MOST_PUSH`] gravities.
    pub fn magnet_push(&self, at: Vec2) -> Vec2 {
        let Push::Magnet { pull, .. } = self.push else { return Vec2::ZERO };
        let d = at - self.place;
        let r = d.length();
        if !self.is_felt() || r >= self.reach || r < 1e-6 * self.reach {
            return Vec2::ZERO;
        }
        let (share, along, round) = self.teeth(d);
        let u = r / self.reach;
        let v = u / share;
        if v >= 1.0 {
            return Vec2::ZERO;
        }
        let q = 1.0 - v * v;
        let dpsi = -6.0 * v * q * q;
        let out = d / r;
        let shares = share * share;
        let p = pull * dpsi * ((1.0 / share - u * along / shares) * out - (round / shares) * out.perp());
        let big = p.length();
        let most = MOST_PUSH * GRAVITY_UNIT;
        if !big.is_finite() {
            Vec2::ZERO
        } else if big > most {
            p * (most / big)
        } else {
            p
        }
    }

    /// The speed a drop at `at` moving at `v` (pixels a second) with a neighbourhood `fullness` full has
    /// after a step of `dt` seconds of this force alone, as the card works it out.
    pub fn after(&self, at: Vec2, v: Vec2, fullness: f32, dt: f32) -> Vec2 {
        if !self.is_felt() {
            return v;
        }
        match self.push {
            Push::Magnet { .. } => v + self.magnet_push(at) * dt,
            Push::Whirlpool { speed, rate } => {
                let d = at - self.place;
                let r = d.length();
                if r >= self.reach || r < 1e-6 * self.reach {
                    return v;
                }
                let x = r / (SPIN_CORE * self.reach);
                let want = speed * 2.0 * x / (1.0 + x * x);
                let fade = 1.0 - smooth(SPIN_FADE, 1.0, r / self.reach);
                let round = (d / r).perp();
                let keep = (-rate.max(0.0) * fade * dt).det_exp();
                v + (want - v.dot(round)) * (1.0 - keep) * round
            }
            Push::Wind { velocity, rate } => {
                let open = 1.0 - smooth(OPEN_LONE, OPEN_FULL, fullness);
                let keep = (-rate.max(0.0) * open * dt).det_exp();
                velocity + (v - velocity) * keep
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn magnet(spikes: u32, spread_deg: f32) -> Force {
        Force {
            push: Push::Magnet {
                pull: 2.0 * GRAVITY_UNIT / PSI_SLOPE_MOST,
                spikes,
                depth: 0.45,
                aim: (-90.0f32).to_radians(),
                spread: spread_deg.to_radians(),
            },
            place: Vec2::new(30.0, -20.0),
            reach: 300.0,
            liquids: 1,
        }
    }

    #[test]
    fn the_magnet_pulls_down_its_own_slope() {
        for f in [magnet(0, 180.0), magnet(9, 180.0), magnet(5, 360.0)] {
            let mut checked = 0;
            for i in 0..20 {
                for j in 0..10 {
                    let turn = i as f32 / 20.0 * std::f32::consts::TAU + 0.013;
                    let r = (j as f32 + 0.5) / 10.0 * 290.0;
                    let (s, c) = turn.det_sin_cos();
                    let at = f.place + Vec2::new(c, s) * r;
                    let e = 0.01;
                    let slope = Vec2::new(
                        f.magnet_potential(at + Vec2::X * e) - f.magnet_potential(at - Vec2::X * e),
                        f.magnet_potential(at + Vec2::Y * e) - f.magnet_potential(at - Vec2::Y * e),
                    ) / (2.0 * e);
                    let p = f.magnet_push(at);
                    let most = MOST_PUSH * GRAVITY_UNIT;
                    // (Past the most it pushes, it pushes the most, down the slope.)
                    let want = if slope.length() > most { -slope * (most / slope.length()) } else { -slope };
                    let scale = p.length().max(want.length()).max(50.0);
                    assert!((p - want).length() < 0.02 * scale, "{f:?} at {at}: push {p}, slope {slope}");
                    checked += 1;
                }
            }
            assert_eq!(checked, 200);
        }
    }

    /// The angles (degrees from the aim) at which the pull along a ring at half the reach is largest.
    fn peaks(f: &Force) -> Vec<f32> {
        let n = 3600;
        let size: Vec<f32> = (0..n)
            .map(|k| {
                let turn = (-90.0 + 360.0 * k as f32 / n as f32).to_radians();
                let (s, c) = turn.det_sin_cos();
                -f.magnet_potential(f.place + Vec2::new(c, s) * 150.0)
            })
            .collect();
        let mut out = Vec::new();
        for k in 0..n {
            let (a, b, c) = (size[(k + n - 1) % n], size[k], size[(k + 1) % n]);
            if b > a && b >= c && b > size.iter().cloned().fold(f32::MAX, f32::min) + 1.0 {
                let deg = 360.0 * k as f32 / n as f32;
                out.push(if deg > 180.0 { deg - 360.0 } else { deg });
            }
        }
        out
    }

    #[test]
    fn the_magnet_has_its_teeth() {
        for (n, spread) in [(1, 180.0), (5, 60.0), (5, 180.0), (9, 180.0), (9, 360.0)] {
            let got = peaks(&magnet(n, spread));
            assert_eq!(got.len() as u32, n, "{n} over {spread}°: {got:?}");
            assert!(got.iter().all(|d| d.abs() <= spread / 2.0), "{n} over {spread}°: {got:?}");
        }
        assert!(peaks(&magnet(0, 180.0)).is_empty(), "no spikes, a round pull");
    }

    #[test]
    fn nothing_past_the_reach_or_at_the_middle() {
        let f = magnet(9, 180.0);
        assert_eq!(f.magnet_push(f.place), Vec2::ZERO);
        assert_eq!(f.magnet_push(f.place + Vec2::new(300.0, 0.0)), Vec2::ZERO);
        assert_eq!(f.magnet_push(f.place + Vec2::new(0.0, 450.0)), Vec2::ZERO);
        assert_eq!(f.magnet_potential(f.place + Vec2::new(0.0, 450.0)), 0.0);
        let near = f.magnet_push(f.place + Vec2::new(0.0, -100.0));
        assert!(near.y > 0.0, "it pulls toward its middle: {near}");
    }

    #[test]
    fn odd_numbers_push_nothing() {
        let mut f = magnet(9, 180.0);
        let at = f.place + Vec2::new(10.0, -60.0);
        for pull in [f32::NAN, f32::INFINITY] {
            f.push = Push::Magnet { pull, spikes: 9, depth: 0.45, aim: 0.0, spread: 3.0 };
            assert_eq!(f.magnet_push(at), Vec2::ZERO);
            assert!(!f.is_felt());
        }
        f = magnet(9, 180.0);
        f.reach = 0.0;
        assert_eq!(f.magnet_push(at), Vec2::ZERO);
        assert!(!f.is_felt());
        f = magnet(9, 180.0);
        f.liquids = 0;
        assert!(!f.is_felt());
        let huge = Force { push: Push::Magnet { pull: 1e12, spikes: 0, depth: 0.0, aim: 0.0, spread: 0.0 }, ..f };
        let huge = Force { liquids: 1, ..huge };
        let p = huge.magnet_push(at);
        assert!(p.is_finite() && p.length() <= MOST_PUSH * GRAVITY_UNIT * 1.0001, "{p}");
        assert!(huge.headroom() <= MOST_PUSH * GRAVITY_UNIT);
    }

    #[test]
    fn wind_and_whirlpool_draw_the_speed_toward_their_own() {
        let wind = Force {
            push: Push::Wind { velocity: Vec2::new(1000.0, 0.0), rate: 3.0 },
            place: Vec2::ZERO,
            reach: 0.0,
            liquids: 1,
        };
        let lone = wind.after(Vec2::ZERO, Vec2::ZERO, 0.1, 1.0);
        assert!((lone.x - 1000.0 * (1.0 - (-3.0f32).det_exp())).abs() < 1.0, "{lone}");
        assert_eq!(wind.after(Vec2::ZERO, Vec2::new(5.0, 7.0), 1.0, 1.0), Vec2::new(5.0, 7.0), "inside: no wind");
        // (Two half steps are one step.)
        let half = wind.after(Vec2::ZERO, wind.after(Vec2::ZERO, Vec2::ZERO, 0.2, 0.5), 0.2, 0.5);
        assert!((half - wind.after(Vec2::ZERO, Vec2::ZERO, 0.2, 1.0)).length() < 0.05);
        let spin =
            Force { push: Push::Whirlpool { speed: 600.0, rate: 4.0 }, place: Vec2::ZERO, reach: 300.0, liquids: 1 };
        let v = spin.after(Vec2::new(90.0, 0.0), Vec2::ZERO, 1.0, 10.0);
        assert!((v - Vec2::new(0.0, 600.0)).length() < 1.0, "at the core's edge, the core's speed: {v}");
        let v = spin.after(Vec2::new(90.0, 0.0), Vec2::new(50.0, 0.0), 1.0, 10.0);
        assert!((v.x - 50.0).abs() < 1e-3, "only the speed round it: {v}");
        assert_eq!(spin.after(Vec2::new(400.0, 0.0), Vec2::ZERO, 1.0, 1.0), Vec2::ZERO, "past the reach");
    }

    #[test]
    fn the_card_layout_is_48_bytes() {
        assert_eq!(std::mem::size_of::<ForceGpu>(), 48);
        let layout = Layout { origin: Vec2::new(-100.0, -100.0), cell: 4.0, cols: 60, rows: 60 };
        let g = magnet(9, 180.0).gpu(&layout);
        assert_eq!(g.a[0], 32.5);
        assert_eq!(g.a[2], 75.0);
        assert_eq!(g.c, [1, 1, 9, 0]);
    }
}
