//! Drains, leaks and the course's edge (Water mode part 4.1): places the liquid leaves the course
//! by. Each step, a liquid drop in a drain's reach leaves with the drain's chance for a step
//! (`finish.wgsl`'s `drain_at`), and the frame's report counts what each drain took
//! (`report.wgsl`'s `cs_drained`). A drop taken is sent to a place that says which drain took it
//! and in which frame ([`code`]), so the count needs no binding of its own in the step passes.

use bpe_core::maths::Maths;
use glam::Vec2;

use crate::solid::Layout;

/// The most drains a body has (the rest are left out).
pub const MOST_DRAINS: usize = 32;
/// The frames a drained drop's place tells apart (their number wraps at this): the place stays a
/// whole number a float holds exactly.
const FRAME_CODES: u32 = 1 << 23;

/// A place the liquid leaves the course by.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drain {
    /// A disc's middle, world pixels (an edge drain has none).
    pub place: Vec2,
    /// How far round its middle a disc takes liquid, pixels.
    pub reach: f32,
    /// How fast it takes the liquid in its reach: in `t` seconds a drop there is gone with chance
    /// `1 - exp(-pull t)`. Infinite takes every drop the step it comes; 0 takes none.
    pub pull: f32,
    /// 0 for a disc; else the course's edges it runs along ([`Drain::LEFT`] and so on), a strip
    /// where the drops stop at each.
    pub edges: u8,
    /// Whether what it takes goes into the tank that fed pours pour from.
    pub feeds: bool,
}

impl Drain {
    pub const LEFT: u8 = 1;
    pub const RIGHT: u8 = 2;
    pub const TOP: u8 = 4;
    pub const BOTTOM: u8 = 8;
    pub const ALL_EDGES: u8 = 15;

    /// A disc of `reach` pixels round `place`.
    pub fn disc(place: Vec2, reach: f32, pull: f32) -> Drain {
        Drain { place, reach, pull, edges: 0, feeds: true }
    }

    /// A strip along `edges` of the course.
    pub fn edge(edges: u8, pull: f32) -> Drain {
        Drain { place: Vec2::ZERO, reach: 0.0, pull, edges: edges & Drain::ALL_EDGES, feeds: true }
    }

    /// The chance a liquid drop in its reach leaves in a step of `dt` seconds.
    pub fn chance(&self, dt: f32) -> f32 {
        let pull = self.pull;
        if pull.is_infinite() && pull > 0.0 {
            return 1.0;
        }
        if !(pull.is_finite() && pull > 0.0 && dt > 0.0) {
            return 0.0;
        }
        (-(-pull * dt).det_exp_m1()).clamp(0.0, 1.0)
    }
}

/// The y of a place a drain takes a drop to in frame `frame` (its x is `-2e6 - drain`): the
/// report counts the dead drops whose y is this frame's, so a drop taken earlier is not counted
/// again. Matches `cs_drained` and `finish_at`.
pub(super) fn code(frame: u32) -> u32 {
    frame % FRAME_CODES
}

/// The drains' numbers on the card for a frame of steps of `dt` seconds: per drain its middle (grid
/// units), reach (cells) and chance a step; its edges (four to an entry); and how many there are and
/// the frame's code. Matches `drains`, `dedges` and `dmeta` in `Fr`.
pub(super) fn on_card(
    drains: &[Drain],
    layout: &Layout,
    dt: f32,
    frame: u32,
) -> ([[f32; 4]; MOST_DRAINS], [[u32; 4]; MOST_DRAINS / 4], [u32; 4]) {
    let mut at = [[0.0f32; 4]; MOST_DRAINS];
    let mut edges = [[0u32; 4]; MOST_DRAINS / 4];
    let n = drains.len().min(MOST_DRAINS);
    for (k, d) in drains.iter().take(MOST_DRAINS).enumerate() {
        let g = layout.to_grid(d.place);
        let reach = d.reach / layout.cell;
        let (g, reach) = if g.is_finite() && reach.is_finite() { (g, reach.max(0.0)) } else { (Vec2::ZERO, 0.0) };
        at[k] = [g.x, g.y, reach, d.chance(dt)];
        edges[k / 4][k % 4] = u32::from(d.edges & Drain::ALL_EDGES);
    }
    (at, edges, [n as u32, code(frame), 0, 0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drains_chance_is_the_same_share_a_second_at_any_step() {
        let d = Drain::disc(Vec2::ZERO, 10.0, 3.0);
        // (Kept over a second's steps, whatever their size.)
        for n in [60u32, 240, 1200] {
            let dt = 1.0 / n as f32;
            let kept = (1.0 - f64::from(d.chance(dt))).powi(n as i32);
            assert!((kept - 0.049_787_068).abs() < 1e-3, "{n} steps a second keep {kept}");
        }
        assert_eq!(Drain::disc(Vec2::ZERO, 1.0, 0.0).chance(0.01), 0.0);
        assert_eq!(Drain::disc(Vec2::ZERO, 1.0, f32::INFINITY).chance(0.01), 1.0);
        assert_eq!(Drain::disc(Vec2::ZERO, 1.0, f32::NAN).chance(0.01), 0.0);
        assert_eq!(Drain::disc(Vec2::ZERO, 1.0, -2.0).chance(0.01), 0.0);
    }

    #[test]
    fn the_card_gets_each_drain_in_grid_units() {
        let layout = Layout { origin: Vec2::new(-100.0, -50.0), cell: 10.0, cols: 40, rows: 30 };
        let drains = [Drain::disc(Vec2::new(0.0, 0.0), 25.0, 1.0), Drain::edge(Drain::BOTTOM | Drain::LEFT, 30.0)];
        let (at, edges, meta) = on_card(&drains, &layout, 0.01, 7);
        assert_eq!(&at[0][..3], &[10.0, 5.0, 2.5]);
        assert_eq!(edges[0][..2], [0, 9]);
        assert_eq!(meta, [2, 7, 0, 0]);
        assert_eq!(code(FRAME_CODES + 3), 3);
        // (A drain's code stays a whole number a float holds exactly.)
        assert_eq!((1e6f32 + (FRAME_CODES - 1) as f32) as u32, 1_000_000 + FRAME_CODES - 1);
    }
}
