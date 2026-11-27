//! Drops laid out by hand (feature `test-scenes`) for the plain drawing's tests, its pinned picture,
//! the full-size look check and its speed: never a physics run, so a driver update that moves the
//! physics' last bits moves none of these pictures. World pixels, y down, a 1080 x 1920 screen round
//! the middle. Every drop sits on the resting lattice (half a cell apart) with a fixed small wobble.

use bpe_core::maths::Maths;
use glam::Vec2;

use crate::draw::plain::{PlainLiquid, PlainLook};
use crate::drop::{Drop, WATER};
use crate::drops::LEAST_WALL;
use crate::solid::{Layout, Part, Solid, SolidField};

/// How far off a face the drops rest, in cells (the engine's `Numbers::wall_off`).
pub const REST: f32 = 0.25;

/// A set: the course's solids and the drops on its layout.
#[derive(Clone, Debug)]
pub struct Bench {
    pub field: SolidField,
    pub drops: Vec<Drop>,
}

/// The screen's layout at `cell`.
fn screen_layout(cell: f32) -> Layout {
    Layout::cover(Vec2::new(-540.0, -960.0), Vec2::new(540.0, 960.0), cell)
}

/// A fixed wobble for lattice point `(i, j)`, each way under a fifth of a spacing (grid units).
fn wobble(i: i32, j: i32) -> Vec2 {
    let mut h = (i as u32).wrapping_mul(0x9e37_79b9) ^ (j as u32).wrapping_mul(0x85eb_ca6b) ^ 0x27d4_eb2f;
    let mut next = || {
        h ^= h >> 15;
        h = h.wrapping_mul(0x2c1b_3c6d);
        h ^= h >> 12;
        h = h.wrapping_mul(0x297a_2d39);
        h ^= h >> 15;
        (h >> 8) as f32 / (1u32 << 24) as f32 - 0.5
    };
    Vec2::new(next(), next()) * 0.1
}

/// Builds a set's drops.
struct Laying<'a> {
    field: &'a SolidField,
    drops: Vec<Drop>,
}

impl Laying<'_> {
    fn layout(&self) -> Layout {
        self.field.layout
    }

    /// A drop at world point `p` (kept only `REST` cells or more off every face).
    fn drop_at(&mut self, p: Vec2, kind: u32) {
        let l = self.layout();
        if self.field.at(p) < REST * l.cell {
            return;
        }
        let serial = self.drops.len() as u32;
        self.drops.push(Drop::new(l.to_grid(p), Vec2::ZERO, kind, serial, WATER));
    }

    /// The resting lattice over `lo` to `hi` (world pixels), each point `inside` says, of liquid `kind(p)`.
    fn lattice(&mut self, lo: Vec2, hi: Vec2, inside: impl Fn(Vec2) -> bool, kind: impl Fn(Vec2) -> u32) {
        let l = self.layout();
        let (a, b) = (l.to_grid(lo), l.to_grid(hi));
        let (i0, i1) = ((a.x * 2.0).floor() as i32, (b.x * 2.0).ceil() as i32);
        let (j0, j1) = ((a.y * 2.0).floor() as i32, (b.y * 2.0).ceil() as i32);
        for j in j0..=j1 {
            for i in i0..=i1 {
                let g = Vec2::new(0.25 + 0.5 * i as f32, 0.25 + 0.5 * j as f32) + wobble(i, j);
                let p = l.to_world(g);
                if p.cmpge(lo).all() && p.cmple(hi).all() && inside(p) {
                    self.drop_at(p, kind(p));
                }
            }
        }
    }
}

/// The bench: a course carved in rock (its inside from (-492, -892) to (492, 892)), a slope block and a disc; a pool on the floor
/// with a gently waved top, its left part a second liquid; a stream 20 px wide falling into it; a
/// film one drop thick resting on the slope; a blob of the second liquid in the air; spray (lone drops
/// at sub-pixel offsets, and pairs one, two and three spacings apart); one drop past the layout and
/// one that is not a number.
pub fn bench(cell: f32) -> Bench {
    let layout = screen_layout(cell);
    let slope =
        [Vec2::new(-470.0, -330.0), Vec2::new(-80.0, -150.0), Vec2::new(-80.0, -90.0), Vec2::new(-470.0, -270.0)];
    let solids = vec![
        Solid::rect(Vec2::new(-700.0, -1100.0), Vec2::new(-492.0, 1100.0), true, 0.0),
        Solid::rect(Vec2::new(492.0, -1100.0), Vec2::new(700.0, 1100.0), true, 0.0),
        Solid::rect(Vec2::new(-700.0, -1100.0), Vec2::new(700.0, -892.0), true, 0.0),
        Solid::rect(Vec2::new(-700.0, 892.0), Vec2::new(700.0, 1100.0), true, 0.0),
        Solid::Polygon { points: slope.to_vec(), filled: true, thickness: 0.0 },
        Solid::Circle { centre: Vec2::new(250.0, -280.0), radius: 100.0, filled: true, thickness: 0.0 },
    ];
    let field = SolidField::build_least(layout, &solids, LEAST_WALL * cell);
    let mut lay = Laying { field: &field, drops: Vec::new() };
    let top = |x: f32| 430.0 + 16.0 * (x / 70.0).det_sin() + 8.0 * (x / 23.0).det_sin();
    // The pool, the second liquid left of x = -240.
    lay.lattice(Vec2::new(-500.0, 380.0), Vec2::new(500.0, 900.0), |p| p.y >= top(p.x), |p| u32::from(p.x < -240.0));
    // The stream, down into the pool.
    lay.lattice(Vec2::new(50.0, -860.0), Vec2::new(70.0, 460.0), |p| p.y < top(p.x), |_| 0);
    // The film on the slope's top face, one drop thick.
    let (a, b) = (slope[0], slope[1]);
    let along = (b - a).normalize();
    let up = Vec2::new(along.y, -along.x);
    let s = 0.5 * cell;
    let mut t = 20.0;
    while t < (b - a).length() - 20.0 {
        lay.drop_at(a + along * t + up * (REST * cell + 0.01), 0);
        t += s;
    }
    // A blob of the second liquid in the air.
    let blob = Vec2::new(-300.0, 200.0);
    lay.lattice(blob - Vec2::splat(46.0), blob + Vec2::splat(46.0), |p| (p - blob).length() <= 45.0, |_| 1);
    // Spray: lone drops a little off whole pixels, and pairs.
    for k in 0..8 {
        lay.drop_at(Vec2::new(120.0 + 41.37 * k as f32, -560.0 + 0.29 * k as f32), 0);
    }
    for (k, apart) in [1.0f32, 2.0, 3.0].into_iter().enumerate() {
        let at = Vec2::new(150.0 + 100.0 * k as f32, -640.0);
        lay.drop_at(at, 0);
        lay.drop_at(at + Vec2::new(apart * s, 0.0), 0);
    }
    let mut drops = lay.drops;
    let n = drops.len() as u32;
    drops.push(Drop::new(Vec2::new(-4.0, -4.0), Vec2::ZERO, 0, n, WATER));
    drops.push(Drop::new(Vec2::new(f32::NAN, 10.0), Vec2::ZERO, 0, n + 1, WATER));
    Bench { field, drops }
}

/// Part 3's pieces on the bench's carved box, each its own colour ([`pieces_look`]): a ramp with a
/// pillar laid over its tip, a basin holding water, a wheel (a rim and four paddles), a pipe cut
/// through the right-hand rock and lined with its walls, and a square-ended pen wall standing in
/// the pool on the floor. For the look check: the edges where colours meet, and where a cut meets
/// rock.
pub fn pieces(cell: f32) -> Bench {
    let layout = screen_layout(cell);
    let least = LEAST_WALL * cell;
    let rock = carved(&[Solid::rect(Vec2::new(-500.0, -900.0), Vec2::new(500.0, 900.0), false, 16.0)], least);
    let mut field = SolidField::build_least(layout, &rock, least);
    let v = Vec2::new;
    let line =
        |points: &[Vec2], thickness: f32, square: bool| Solid::Polyline { points: points.to_vec(), thickness, square };
    let part = |solids: Vec<Solid>, cut: Vec<Solid>| Part { solids, cut, wetting: 1.0 };
    let wheel = v(250.0, -450.0);
    let mut spokes = vec![Solid::Circle { centre: wheel, radius: 90.0, filled: false, thickness: 12.0 }];
    for k in 0..4 {
        let (s, c) = (0.3 + std::f32::consts::FRAC_PI_4 * k as f32).det_sin_cos();
        spokes.push(Solid::Line { a: wheel - v(c, s) * 84.0, b: wheel + v(c, s) * 84.0, thickness: 10.0 });
    }
    let parts = [
        part(
            vec![Solid::Polygon {
                points: vec![v(-492.0, -120.0), v(-110.0, -120.0), v(-492.0, -320.0)],
                filled: true,
                thickness: 0.0,
            }],
            vec![],
        ),
        part(vec![Solid::rect(v(-150.0, -300.0), v(-100.0, 20.0), true, 0.0)], vec![]),
        part(vec![line(&[v(-40.0, -220.0), v(-40.0, -60.0), v(240.0, -60.0), v(240.0, -220.0)], 16.0, false)], vec![]),
        part(spokes, vec![]),
        part(
            vec![
                line(&[v(300.0, 212.0), v(720.0, 212.0)], 12.0, true),
                line(&[v(300.0, 288.0), v(720.0, 288.0)], 12.0, true),
            ],
            // Its bore only, as the kit's cutting pipe (`PieceShape::cut`): the walls add after.
            vec![line(&[v(300.0, 250.0), v(720.0, 250.0)], 64.0, true)],
        ),
        part(vec![line(&[v(-400.0, 640.0), v(-150.0, 700.0), v(100.0, 650.0)], 24.0, true)], vec![]),
    ];
    for (k, p) in parts.iter().enumerate() {
        field.add_part(p, k as u16 + 1, least);
    }
    let mut lay = Laying { field: &field, drops: Vec::new() };
    let top = |x: f32| 560.0 + 10.0 * (x / 60.0).det_sin();
    lay.lattice(v(-492.0, 500.0), v(492.0, 892.0), |p| p.y >= top(p.x), |_| 0);
    lay.lattice(v(-32.0, -150.0), v(232.0, -68.0), |_| true, |_| 1);
    let drops = lay.drops;
    Bench { field, drops }
}

/// [`pieces`]' look: the sets' liquids, and each piece in its own colour.
pub fn pieces_look() -> PlainLook {
    let solids = vec![
        [0.78, 0.42, 0.30],
        [0.45, 0.50, 0.60],
        [0.20, 0.62, 0.60],
        [0.55, 0.35, 0.80],
        [0.75, 0.85, 0.90],
        [0.92, 0.78, 0.25],
    ];
    PlainLook { solids, ..look() }
}

/// The screen's box (its faces on the screen's edges) filled with liquid to `share` of its height:
/// two thirds at 2 px cells is about 1.4 million drops, at 8 px about 86,000 (the drawing's speed
/// targets).
pub fn screen_pool(cell: f32, share: f32) -> Bench {
    let layout = screen_layout(cell);
    let solids = vec![Solid::rect(Vec2::new(-548.0, -968.0), Vec2::new(548.0, 968.0), false, 16.0)];
    let field = SolidField::build_least(layout, &solids, LEAST_WALL * cell);
    let mut lay = Laying { field: &field, drops: Vec::new() };
    let top = 960.0 - 1920.0 * share.clamp(0.0, 1.0);
    lay.lattice(Vec2::new(-540.0, top), Vec2::new(540.0, 960.0), |_| true, |_| 0);
    Bench { drops: lay.drops, field }
}

/// The screen's box open at the top: rock under y 900 and past x -500 and 500, the sky above.
pub fn open_top(cell: f32) -> SolidField {
    const FAR: f32 = 1.0e4;
    let solids = [
        Solid::rect(Vec2::new(-FAR, -FAR), Vec2::new(-500.0, FAR), true, 0.0),
        Solid::rect(Vec2::new(500.0, -FAR), Vec2::new(FAR, FAR), true, 0.0),
        Solid::rect(Vec2::new(-FAR, 900.0), Vec2::new(FAR, FAR), true, 0.0),
    ];
    SolidField::build_least(screen_layout(cell), &solids, LEAST_WALL * cell)
}

/// A pool on the floor of the box [`open_top`] with a top waved `amplitude` px over `wavelength` px
/// (its floor at y 900, its top round y 900 - `depth`), for the look's caustics.
pub fn wavy_pool(cell: f32, depth: f32, amplitude: f32, wavelength: f32) -> Bench {
    let field = open_top(cell);
    let mut lay = Laying { field: &field, drops: Vec::new() };
    let k = std::f32::consts::TAU / wavelength.max(1.0);
    let top = |x: f32| 900.0 - depth + amplitude * (k * x).det_sin();
    lay.lattice(
        Vec2::new(-500.0, 900.0 - depth - amplitude - 8.0),
        Vec2::new(500.0, 900.0),
        |p| p.y >= top(p.x),
        |_| 0,
    );
    Bench { drops: lay.drops, field }
}

/// The resting lattice over the rectangle `lo` to `hi` (world pixels) on `field`'s layout, every
/// drop of liquid `kind`, none nearer a face than [`REST`] cells.
pub fn lattice(field: &SolidField, lo: Vec2, hi: Vec2, kind: u32) -> Vec<Drop> {
    let mut lay = Laying { field, drops: Vec::new() };
    lay.lattice(lo, hi, |_| true, |_| kind);
    lay.drops
}

/// `solids` as a Water course is made, carved in rock: each hollow axis-aligned rectangle (the test
/// scenes' boxes, `Solid::rect(.., false, ..)`) becomes rock everywhere outside its inner face. The
/// physics only ever meets the inner face, which stays where it was (`least`: thin walls' least
/// thickness, px, as the physics' field was built with). For drawing the test scenes: light then
/// comes in only through the water's own air, never through a thin wall from outside the box.
pub fn carved(solids: &[Solid], least: f32) -> Vec<Solid> {
    let mut out = Vec::with_capacity(solids.len() + 3);
    for s in solids {
        match s {
            Solid::Polygon { points, filled: false, thickness } if is_rect(points) => {
                let lo = points.iter().copied().fold(Vec2::splat(f32::MAX), Vec2::min);
                let hi = points.iter().copied().fold(Vec2::splat(f32::MIN), Vec2::max);
                let half = 0.5 * thickness.max(least);
                let (a, b) = (lo + half, hi - half);
                const FAR: f32 = 1.0e4;
                let slab = |lo: Vec2, hi: Vec2| Solid::rect(lo, hi, true, 0.0);
                out.push(slab(Vec2::new(-FAR, -FAR), Vec2::new(a.x, FAR)));
                out.push(slab(Vec2::new(b.x, -FAR), Vec2::new(FAR, FAR)));
                out.push(slab(Vec2::new(-FAR, -FAR), Vec2::new(FAR, a.y)));
                out.push(slab(Vec2::new(-FAR, b.y), Vec2::new(FAR, FAR)));
            }
            other => out.push(other.clone()),
        }
    }
    out
}

/// Whether `points` are an axis-aligned rectangle's four corners.
fn is_rect(points: &[Vec2]) -> bool {
    points.len() == 4
        && (0..4).all(|i| {
            let (a, b) = (points[i], points[(i + 1) % 4]);
            a.x == b.x || a.y == b.y
        })
}

/// The sets' look: the Water Look card's water, and an orange second liquid.
pub fn look() -> PlainLook {
    let orange = PlainLiquid { colour: [1.0, 0.54, 0.24], deep: [0.35, 0.1, 0.02], clarity: 120.0 };
    PlainLook { liquids: vec![PlainLiquid::default(), orange], ..PlainLook::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bench_has_every_part() {
        let b = bench(8.0);
        let l = b.field.layout;
        assert!(b.drops.iter().any(|d| d.liquid() == 0) && b.drops.iter().any(|d| d.liquid() == 1));
        let out = |d: &Drop| {
            let g = Vec2::from(d.x);
            !g.is_finite() || g.x < 0.0 || g.y < 0.0 || g.x > (l.cols - 1) as f32 || g.y > (l.rows - 1) as f32
        };
        assert_eq!(b.drops.iter().filter(|d| out(d)).count(), 2, "one past the layout, one not a number");
        // The stream, half way down: 20 px across.
        let row: Vec<f32> = b
            .drops
            .iter()
            .map(|d| l.to_world(Vec2::from(d.x)))
            .filter(|p| (p.y + 300.0).abs() < 2.0 && p.x > 0.0 && p.x < 120.0)
            .map(|p| p.x)
            .collect();
        let wide = row.iter().copied().fold(f32::MIN, f32::max) - row.iter().copied().fold(f32::MAX, f32::min);
        // (Its drops' middles span the width less a spacing, 4 px.)
        assert!((wide + 4.0 - 20.0).abs() <= 4.0 + 1e-3, "the stream is 20 px across: {wide}");
    }

    #[test]
    fn screen_pool_counts_match_the_targets() {
        let fine = screen_pool(2.0, 2.0 / 3.0).drops.len();
        assert!((1_300_000..=1_500_000).contains(&fine), "{fine}");
        let draft = screen_pool(8.0, 2.0 / 3.0).drops.len();
        assert!((80_000..=92_000).contains(&draft), "{draft}");
    }

    #[test]
    fn sets_are_the_same_every_time() {
        let (a, b) = (bench(3.0), bench(3.0));
        assert_eq!(a.drops.len(), b.drops.len());
        assert!(a.drops.iter().zip(&b.drops).all(|(x, y)| bytemuck::bytes_of(x) == bytemuck::bytes_of(y)));
    }
}
