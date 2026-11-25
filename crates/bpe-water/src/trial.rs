//! The all-drops trial's switch and shots (feature `test-scenes`): either engine plays a test
//! scene behind one face ([`Body`]), so every shot, measure and film is the same code for both.
//! `docs/superpowers/plans/water-mode/all-drops-trial.md`.

use std::sync::Arc;

use glam::Vec2;

use crate::drops::numbers::DropNumbers;
use crate::drops::{Drops, DropsGpu};
use crate::scenes::{self, GRAVITY, POUR_SPEED, Scene};
use crate::sim::drop::{Drop, Pour, Poured, STOPS, fill};
use crate::sim::numbers::Numbers;
use crate::sim::{Water, WaterGpu};
use crate::solid::{Layout, Solid, SolidField};

/// Which engine plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    /// The merged core: drops on a grid, the pressure solved there.
    Grid,
    /// The trial: all drops.
    Drops,
}

impl Engine {
    pub fn name(self) -> &'static str {
        match self {
            Engine::Grid => "grid",
            Engine::Drops => "drops",
        }
    }

    pub fn named(name: &str) -> Option<Engine> {
        match name.trim() {
            "grid" => Some(Engine::Grid),
            "drops" => Some(Engine::Drops),
            _ => None,
        }
    }
}

/// Both engines' pipelines on one device.
#[derive(Clone, Debug)]
pub struct Cards {
    pub grid: Arc<WaterGpu>,
    pub drops: Arc<DropsGpu>,
}

impl Cards {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Cards {
        Cards { grid: WaterGpu::new(device, queue), drops: DropsGpu::new(device, queue) }
    }
}

/// A body of liquid on either engine.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Body {
    Grid(Water),
    Drops(Drops),
}

impl Body {
    pub fn engine(&self) -> Engine {
        match self {
            Body::Grid(_) => Engine::Grid,
            Body::Drops(_) => Engine::Drops,
        }
    }

    /// What the bridge to today's drawing reads of this body.
    pub fn feed(&self) -> crate::bridge::Feed<'_> {
        match self {
            Body::Grid(w) => w.feed(),
            Body::Drops(d) => d.feed(),
        }
    }

    pub fn add(&mut self, born: &[Drop]) -> u32 {
        match self {
            Body::Grid(w) => w.add(born),
            Body::Drops(d) => d.add(born),
        }
    }

    pub fn step(&mut self) {
        match self {
            Body::Grid(w) => w.step(),
            Body::Drops(d) => d.step(),
        }
    }

    pub fn read(&self) -> Vec<Drop> {
        match self {
            Body::Grid(w) => w.read(),
            Body::Drops(d) => d.read(),
        }
    }

    pub fn count(&self) -> u32 {
        match self {
            Body::Grid(w) => w.count(),
            Body::Drops(d) => d.count(),
        }
    }

    pub fn steps(&self) -> u32 {
        match self {
            Body::Grid(w) => w.steps(),
            Body::Drops(d) => d.steps(),
        }
    }

    pub fn layout(&self) -> Layout {
        match self {
            Body::Grid(w) => w.layout(),
            Body::Drops(d) => d.layout(),
        }
    }

    /// Liquid steps a second.
    pub fn rate(&self) -> u32 {
        match self {
            Body::Grid(w) => w.numbers().rate,
            Body::Drops(d) => d.numbers().rate,
        }
    }

    pub fn set_gravity(&mut self, gravity: Vec2) {
        match self {
            Body::Grid(w) => w.set_gravity(gravity),
            Body::Drops(d) => d.set_gravity(gravity),
        }
    }

    /// Waits until the card has done every step sent so far.
    pub fn wait(&mut self) {
        match self {
            Body::Grid(w) => w.wait(),
            Body::Drops(d) => d.wait(),
        }
    }

    pub fn copy(&self) -> Body {
        match self {
            Body::Grid(w) => Body::Grid(w.copy()),
            Body::Drops(d) => Body::Drops(d.copy()),
        }
    }
}

/// A scene being played on either engine.
pub struct Play {
    pub body: Body,
    pub field: SolidField,
    pours: Vec<(Pour, Poured)>,
}

impl Play {
    /// Starts `scene` on `engine`, with that engine's numbers for the scene's cell size (each
    /// replaceable by its `BPE_WATER_*` or `BPE_DROPS_*`).
    pub fn start(scene: &Scene, cards: &Cards, engine: Engine) -> Play {
        let field = SolidField::build(scene.layout, &scene.solids);
        let cell = scene.layout.cell;
        let mut body = match engine {
            Engine::Grid => Body::Grid(Water::new(
                cards.grid.clone(),
                &field,
                scene.gravity,
                Numbers::from_env_for(cell),
                &scene.feels,
            )),
            Engine::Drops => Body::Drops(Drops::new(
                cards.drops.clone(),
                &field,
                scene.gravity,
                DropNumbers::from_env_for(cell),
                &scene.feels,
            )),
        };
        let mut born = 0u32;
        for (lo, hi) in &scene.fills {
            let drops = fill(&scene.layout, &field, *lo, *hi, STOPS, born);
            born = born.wrapping_add(body.add(&drops));
        }
        let pours = scene.pours.iter().map(|p| (*p, Poured { owed: 0.0, born: 1 << 24 })).collect();
        Play { body, field, pours }
    }

    pub fn step(&mut self) {
        let (dt, step, layout) = (1.0 / self.body.rate() as f32, self.body.steps(), self.body.layout());
        let engine = self.body.engine();
        for (k, (pour, state)) in self.pours.iter_mut().enumerate() {
            let drops = match engine {
                Engine::Grid => pour.drops(state, &layout, dt, step, k as u32 + 1),
                Engine::Drops => rows(pour, state, &layout, dt, step),
            };
            self.body.add(&drops);
        }
        self.body.step();
    }

    /// Plays on to `seconds` from the start.
    pub fn run_to(&mut self, seconds: f32) {
        let until = (f64::from(seconds) * f64::from(self.body.rate())).round() as u32;
        while self.body.steps() < until {
            self.step();
        }
    }

    pub fn copy(&self) -> Play {
        Play { body: self.body.copy(), field: self.field.clone(), pours: self.pours.clone() }
    }
}

/// A pour's drops for the all-drops engine: whole rows across the spout, one each time the stream
/// has moved on a drop's spacing, so the stream leaves the spout already evenly spaced. (The grid
/// core's pour strews its drops at random within the stream, which its grid never notices; drops
/// that feel each other directly are thrown apart by it. The amount poured is the same.)
pub fn rows(pour: &Pour, state: &mut Poured, layout: &Layout, dt: f32, step: u32) -> Vec<Drop> {
    let time = step as f32 * dt;
    if time < pour.from || time >= pour.until {
        return Vec::new();
    }
    let spacing = 0.5 * layout.cell;
    let n = (pour.width / spacing).round().max(1.0);
    // The drops are born at their resting spacing both ways (`BPE_DROPS_ROWS=flux` squeezes the
    // rows together or apart instead, for the grid core's amount a second to the drop: born
    // squeezed, the stream throws bits off sideways as it leaves the spout).
    let (apart, wide) = if std::env::var("BPE_DROPS_ROWS").is_ok_and(|v| v == "flux") {
        (n * spacing * spacing / pour.width.max(1e-3), pour.width)
    } else {
        (spacing, n * spacing)
    };
    let way = pour.fanned(0.0);
    let across = way.perp();
    state.owed += pour.speed * dt;
    let mut out = Vec::new();
    while state.owed >= apart && out.len() < Pour::MOST_A_STEP as usize {
        state.owed -= apart;
        for i in 0..n as u32 {
            let u = (i as f32 + 0.5) / n - 0.5;
            let at = pour.place + across * (u * wide) + way * state.owed;
            let v = pour.fanned(u) * pour.speed;
            out.push(Drop::new(layout.to_grid(at), v / layout.cell, pour.kind, state.born, pour.colour));
            state.born = state.born.wrapping_add(1);
        }
    }
    out
}

fn boxed_in(half: Vec2) -> Solid {
    Solid::rect(-half, half, false, 6.0)
}

/// A slab from `a` to `b`, `thick` pixels thick below that line (down the screen).
fn slab(a: Vec2, b: Vec2, thick: f32) -> Solid {
    let n = (b - a).normalize().perp() * thick;
    let n = if n.y < 0.0 { -n } else { n };
    Solid::Polygon { points: vec![a, b, b + n, a + n], filled: true, thickness: 0.0 }
}

/// Where [`slope`]'s plate is: its upper end and its lower end (its top face).
pub const SLOPE: (Vec2, Vec2) = (Vec2::new(-330.0, -260.0), Vec2::new(170.0, -60.0));

/// Shot 1, a tap stream onto a slope: a thin steady stream lands on a tilted plate whose lower end
/// is in the air, far above the floor.
pub fn slope(cell: f32) -> Scene {
    let half = Vec2::new(450.0, 800.0);
    Scene {
        name: "slope",
        layout: Layout::cover(-half - Vec2::splat(40.0), half + Vec2::splat(40.0), cell),
        solids: vec![boxed_in(half), slab(SLOPE.0, SLOPE.1, 28.0)],
        gravity: GRAVITY,
        feels: vec![Default::default()],
        fills: Vec::new(),
        pours: vec![Pour { place: Vec2::new(-200.0, -720.0), width: 14.0, speed: POUR_SPEED, ..Pour::default() }],
    }
}

/// Shot 2, a sheet off a ledge: the grid core's own `sheet` scene.
pub fn ledge(cell: f32) -> Scene {
    scenes::sheet(cell)
}

/// Where [`pool_pour`]'s pool is: its half width, its floor (y) and its depth at the start.
pub const POOL: (f32, f32, f32) = (330.0, 800.0, 300.0);

/// Shot 3, a thick pour into a pool, for three seconds.
pub fn pool_pour(cell: f32) -> Scene {
    let half = Vec2::new(POOL.0, POOL.1);
    let inner = half.x - 0.5 * cell.max(6.0);
    let floor = half.y - 0.5 * cell.max(6.0);
    Scene {
        name: "pool",
        layout: Layout::cover(-half - Vec2::splat(40.0), half + Vec2::splat(40.0), cell),
        solids: vec![boxed_in(half)],
        gravity: GRAVITY,
        feels: vec![Default::default()],
        fills: vec![(Vec2::new(-inner, floor - POOL.2), Vec2::new(inner, floor))],
        pours: vec![Pour {
            place: Vec2::new(-90.0, -700.0),
            width: 70.0,
            speed: POUR_SPEED,
            until: 3.0,
            ..Pour::default()
        }],
    }
}

/// Shot 5, beads on a slope: separate small drops set down on a tilted plate and on a level one.
pub fn beads(cell: f32) -> Scene {
    let half = Vec2::new(450.0, 800.0);
    let (a, b) = (Vec2::new(-380.0, -200.0), Vec2::new(380.0, -38.0));
    let level = 300.0;
    let mut fills = Vec::new();
    // On the tilted plate: each square's foot just above the plate under its upper corner.
    for (x, side) in [(-340.0f32, 14.0f32), (-270.0, 22.0), (-170.0, 32.0), (-40.0, 48.0), (120.0, 18.0), (220.0, 28.0)]
    {
        let foot = a.y + (x - a.x) * (b.y - a.y) / (b.x - a.x) - 1.0;
        fills.push((Vec2::new(x, foot - side), Vec2::new(x + side, foot)));
    }
    // On the level plate.
    for (x, side) in [(-330.0f32, 12.0f32), (-240.0, 20.0), (-120.0, 30.0), (20.0, 44.0), (180.0, 64.0)] {
        fills.push((Vec2::new(x, level - 1.0 - side), Vec2::new(x + side, level - 1.0)));
    }
    Scene {
        name: "beads",
        layout: Layout::cover(-half - Vec2::splat(40.0), half + Vec2::splat(40.0), cell),
        solids: vec![
            boxed_in(half),
            slab(a, b, 24.0),
            Solid::rect(Vec2::new(-380.0, level), Vec2::new(300.0, level + 24.0), true, 0.0),
        ],
        gravity: GRAVITY,
        feels: vec![Default::default()],
        fills,
        pours: Vec::new(),
    }
}

/// The trial's shots by name: `slope`, `ledge`, `pool`, `course`, `beads`, `weird`, `deep` (the
/// deep pool at rest), `thin` (a two-cell pour down a whole screen).
pub fn shot(name: &str, cell: f32) -> Option<Scene> {
    Some(match name {
        "slope" => slope(cell),
        "ledge" => ledge(cell),
        "pool" => pool_pour(cell),
        "course" => scenes::course(cell),
        "beads" => beads(cell),
        "weird" => scenes::weird(cell),
        "deep" => scenes::still_box(cell, 0.6),
        "box" => scenes::still_box(cell, 0.3),
        "thin" => scenes::thin_pour(cell, 2.0),
        _ => return None,
    })
}
