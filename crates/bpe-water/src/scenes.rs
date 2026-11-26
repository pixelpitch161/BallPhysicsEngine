//! The physics test scenes (feature `test-scenes`): small courses built by hand for the behaviour
//! tests, the measuring runs and the side-by-side with today's core. They are not scenes anyone
//! opens.
//!
//! World pixels, y down, a screen 1080 x 1920 round the middle.

use std::sync::Arc;

use glam::Vec2;

use crate::drop::{AIR, Pour, STOPS, fill_stacked};
use crate::drops::numbers::{FRAME, Feel, Numbers};
use crate::drops::{LEAST_WALL, Water, WaterGpu};
use crate::solid::{DEFAULT_WETTING, Layout, Solid, SolidField};

/// The course's gravity in the test scenes: the Balls scenes' 500 at today's Bucket size (x 8).
pub const GRAVITY: Vec2 = Vec2::new(0.0, 4000.0);
/// A default pour's speed at today's Bucket size (300 x sqrt 8).
pub const POUR_SPEED: f32 = 848.528;

/// A test scene.
#[derive(Clone, Debug)]
pub struct Scene {
    pub name: &'static str,
    pub layout: Layout,
    pub solids: Vec<Solid>,
    pub gravity: Vec2,
    pub feels: Vec<Feel>,
    /// Rectangles filled with liquid at the start.
    pub fills: Vec<(Vec2, Vec2)>,
    /// Round holes in the fills: their drops left out, or made air.
    pub holes: Vec<Hole>,
    pub pours: Vec<Pour>,
    /// How wet every solid is (part 2c).
    pub wetting: f32,
}

/// A round hole in a scene's fills (part 2c): the fills' drops within `radius` pixels of `centre`
/// are left out, or are made air drops when `air`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hole {
    pub centre: Vec2,
    pub radius: f32,
    pub air: bool,
}

/// A scene being played by the all-drops engine (its pours are the body's own).
pub struct Running {
    pub water: Water,
    pub field: SolidField,
}

impl Scene {
    /// Starts the scene on `gpu` with the all-drops engine's `numbers`.
    pub fn start(&self, gpu: &Arc<WaterGpu>, numbers: Numbers) -> Running {
        let field = SolidField::build_least(self.layout, &self.solids, LEAST_WALL * self.layout.cell)
            .with_wetting(self.wetting);
        let mut water = Water::new(gpu.clone(), &field, self.gravity, numbers, &self.feels);
        for (lo, hi) in &self.fills {
            let mut drops = fill_stacked(&self.layout, &field, *lo, *hi, STOPS, numbers.wall_off);
            for hole in &self.holes {
                let within =
                    |d: &crate::drop::Drop| self.layout.to_world(Vec2::from(d.x)).distance(hole.centre) < hole.radius;
                if hole.air {
                    drops.iter_mut().filter(|d| within(d)).for_each(|d| d.kind |= AIR);
                } else {
                    drops.retain(|d| !within(d));
                }
            }
            water.add(&drops);
        }
        water.set_pours(&self.pours);
        Running { water, field }
    }
}

impl Running {
    /// One frame (the body pours its own).
    pub fn frame(&mut self) {
        self.water.frame();
    }

    /// Plays on to `seconds` from the start, in whole frames.
    pub fn run_to(&mut self, seconds: f32) {
        let until = (seconds / FRAME).round() as u32;
        while self.water.frames() < until {
            self.frame();
        }
    }

    /// A copy that plays on exactly as this one does.
    pub fn copy(&self) -> Running {
        Running { water: self.water.copy(), field: self.field.clone() }
    }
}

/// The test scenes' water: Surface pull 0.2, as Aedan's Water Weird file has it and as the grid
/// core's tests were tuned at (the reference shots set their own).
fn water() -> Vec<Feel> {
    vec![Feel { surface_pull: 0.2, ..Feel::default() }]
}

/// A hollow box (the Balls test scenes' `liquid_box`: 600 x 900, its wall 3 thick) filled to
/// `level` of its height.
pub fn still_box(cell: f32, level: f32) -> Scene {
    boxed(cell, 600.0, 3.0, level)
}

/// The same with the box `width` wide and its wall `wall` thick.
pub fn boxed(cell: f32, width: f32, wall: f32, level: f32) -> Scene {
    let (hw, hh) = (width * 0.5, 450.0);
    let half = Vec2::new(hw, hh);
    // What the liquid's own space is: inside the wall's inner face (a thin wall is a cell thick
    // to the physics).
    let inner = hw - 0.5 * wall.max(cell);
    let floor = hh - 0.5 * wall.max(cell);
    Scene {
        name: "still box",
        layout: Layout::cover(-half - Vec2::splat(40.0), half + Vec2::splat(40.0), cell),
        solids: vec![Solid::rect(-half, half, false, wall)],
        gravity: GRAVITY,
        feels: water(),
        fills: vec![(Vec2::new(-inner, floor - 2.0 * hh * level), Vec2::new(inner, floor))],
        holes: Vec::new(),
        wetting: DEFAULT_WETTING,
        pours: Vec::new(),
    }
}

/// The trial's course (`docs/superpowers/plans/water-mode/physics-trial.md`): a pool at the top
/// that drains through a 48-pixel opening in its floor onto a slope, off the slope's end into
/// pegs, down a funnel into a corridor 36 pixels wide, into a bottom pool whose floor has a
/// 12-pixel and a 24-pixel opening, each over its own catch basin.
pub fn course(cell: f32) -> Scene {
    const TOP: f32 = -640.0;
    const BOTTOM: f32 = 640.0;
    const BASINS: f32 = -100.0;
    let slab = |x0: f32, y0: f32, x1: f32, y1: f32| Solid::rect(Vec2::new(x0, y0), Vec2::new(x1, y1), true, 0.0);
    let block = |points: &[(f32, f32)]| Solid::Polygon {
        points: points.iter().map(|&(x, y)| Vec2::new(x, y)).collect(),
        filled: true,
        thickness: 0.0,
    };
    let half = Vec2::new(450.0, 900.0);
    let mut solids = vec![
        Solid::rect(-half, half, false, 6.0),
        // The top pool's floor, its opening 48 wide round x = -300.
        slab(-450.0, TOP, -324.0, TOP + 30.0),
        slab(-276.0, TOP, 450.0, TOP + 30.0),
        // The slope under the opening, down to the right.
        block(&[(-450.0, -500.0), (200.0, -380.0), (200.0, -356.0), (-450.0, -476.0)]),
        // The funnel and its corridor, 36 wide round x = 170.
        block(&[(-450.0, -60.0), (152.0, 150.0), (152.0, 330.0), (-450.0, 330.0)]),
        block(&[(188.0, 150.0), (450.0, 60.0), (450.0, 330.0), (188.0, 330.0)]),
        // The bottom pool's floor: a 12 wide opening round x = -220 and a 24 wide one round x = 220.
        slab(-450.0, BOTTOM, -226.0, BOTTOM + 30.0),
        slab(-214.0, BOTTOM, 208.0, BOTTOM + 30.0),
        slab(232.0, BOTTOM, 450.0, BOTTOM + 30.0),
        // The wall between the catch basins.
        slab(BASINS - 8.0, BOTTOM + 30.0, BASINS + 8.0, 900.0),
    ];
    // Pegs under the slope's end.
    for (row, y) in [-260.0f32, -190.0, -120.0].into_iter().enumerate() {
        let xs: &[f32] = if row % 2 == 0 { &[110.0, 210.0, 310.0, 410.0] } else { &[160.0, 260.0, 360.0] };
        for &x in xs {
            solids.push(Solid::Circle { centre: Vec2::new(x, y), radius: 16.0, filled: true, thickness: 0.0 });
        }
    }
    Scene {
        name: "course",
        layout: Layout::cover(-half - Vec2::splat(40.0), half + Vec2::splat(40.0), cell),
        solids,
        gravity: GRAVITY,
        feels: water(),
        // The top pool, 870 x 220 of water.
        fills: vec![(Vec2::new(-435.0, -872.0), Vec2::new(435.0, -652.0))],
        holes: Vec::new(),
        wetting: DEFAULT_WETTING,
        pours: Vec::new(),
    }
}

/// Where the course's parts are, for counting drops: the heights of the top pool's floor and the
/// bottom pool's floor, and the x of the wall between the catch basins.
pub const COURSE_LINES: (f32, f32, f32) = (-640.0, 640.0, -100.0);

/// The owner's "Water Weird" scene rebuilt by hand: an 840 x 1680 hollow box, a tilted block and a
/// disc under a thin pour.
pub fn weird(cell: f32) -> Scene {
    let half = Vec2::new(420.0, 840.0);
    let at = Vec2::new(120.0, -240.0);
    let quad = [(-0.666_666_6, 0.0), (0.5, -0.5), (0.666_666_7, 0.166_666_67), (-0.5, 0.666_666_7)];
    Scene {
        name: "weird",
        layout: Layout::cover(-half - Vec2::splat(40.0), half + Vec2::splat(40.0), cell),
        solids: vec![
            Solid::rect(-half, half, false, 3.0),
            Solid::Polygon {
                points: quad.iter().map(|&(x, y)| at + Vec2::new(x, y) * 180.0).collect(),
                filled: true,
                thickness: 0.0,
            },
            Solid::Circle { centre: Vec2::new(-180.0, -120.0), radius: 120.0, filled: true, thickness: 0.0 },
        ],
        gravity: GRAVITY,
        feels: water(),
        fills: Vec::new(),
        // Width 60 at Rate 0.32: a stream 19.2 wide.
        holes: Vec::new(),
        wetting: DEFAULT_WETTING,
        pours: vec![Pour { place: Vec2::new(120.0, -570.0), width: 19.2, speed: POUR_SPEED, ..Pour::default() }],
    }
}

/// Where [`thin_pour`]'s spout and floor are (world y).
pub const THIN_POUR: (f32, f32) = (-900.0, 900.0);

/// A pour `cells` cells wide falling a whole screen onto a floor, with nothing in its way.
pub fn thin_pour(cell: f32, cells: f32) -> Scene {
    let (top, floor) = THIN_POUR;
    Scene {
        name: "thin pour",
        layout: Layout::cover(Vec2::new(-260.0, top - 40.0), Vec2::new(260.0, floor + 60.0), cell),
        solids: vec![Solid::rect(Vec2::new(-400.0, floor), Vec2::new(400.0, floor + 200.0), true, 0.0)],
        gravity: GRAVITY,
        feels: water(),
        fills: Vec::new(),
        holes: Vec::new(),
        wetting: DEFAULT_WETTING,
        pours: vec![Pour { place: Vec2::new(0.0, top), width: cells * cell, speed: POUR_SPEED, ..Pour::default() }],
    }
}

/// Where [`sheet`]'s shelf ends (x), its top (y) and the floor (y).
pub const SHEET: (f32, f32, f32) = (0.0, -300.0, 600.0);

/// A pour onto a level shelf that is walled on its left and ends in the air on its right: the
/// liquid runs off the edge as a sheet and falls to the floor.
pub fn sheet(cell: f32) -> Scene {
    let (edge, shelf, floor) = SHEET;
    Scene {
        name: "sheet",
        layout: Layout::cover(Vec2::new(-460.0, -700.0), Vec2::new(460.0, floor + 60.0), cell),
        solids: vec![
            Solid::rect(Vec2::new(-400.0, shelf), Vec2::new(edge, shelf + 30.0), true, 0.0),
            Solid::rect(Vec2::new(-430.0, shelf - 200.0), Vec2::new(-400.0, shelf + 30.0), true, 0.0),
            Solid::rect(Vec2::new(-600.0, floor), Vec2::new(600.0, floor + 200.0), true, 0.0),
        ],
        gravity: GRAVITY,
        feels: water(),
        fills: Vec::new(),
        holes: Vec::new(),
        wetting: DEFAULT_WETTING,
        pours: vec![Pour { place: Vec2::new(-250.0, -620.0), width: 32.0, speed: POUR_SPEED, ..Pour::default() }],
    }
}

/// Where [`on_a_disc`]'s disc is: its middle and its radius.
pub const DISC: (Vec2, f32) = (Vec2::new(0.0, -300.0), 60.0);

/// A thin pour onto a small disc, the liquid `surface_pull` strong: how it breaks up below the
/// disc is the liquid's Surface pull to decide.
pub fn on_a_disc(cell: f32, surface_pull: f32) -> Scene {
    let mut scene = thin_pour(cell, 3.0);
    scene.name = "on a disc";
    scene.solids.push(Solid::Circle { centre: DISC.0, radius: DISC.1, filled: true, thickness: 0.0 });
    scene.pours[0].place.x = 14.0;
    scene.feels = vec![Feel { surface_pull, ..Feel::default() }];
    scene
}

// ---- the all-drops trial's shots, at the engine's own feel (Surface pull 0.5) ----------------

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
        feels: vec![Feel::default()],
        fills: Vec::new(),
        holes: Vec::new(),
        wetting: DEFAULT_WETTING,
        pours: vec![Pour { place: Vec2::new(-200.0, -720.0), width: 14.0, speed: POUR_SPEED, ..Pour::default() }],
    }
}

/// Shot 2, a sheet off a ledge: the [`sheet`] scene.
pub fn ledge(cell: f32) -> Scene {
    sheet(cell)
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
        feels: vec![Feel::default()],
        fills: vec![(Vec2::new(-inner, floor - POOL.2), Vec2::new(inner, floor))],
        holes: Vec::new(),
        wetting: DEFAULT_WETTING,
        pours: vec![Pour {
            place: Vec2::new(-90.0, -700.0),
            width: 70.0,
            speed: POUR_SPEED,
            until: 3.0,
            ..Pour::default()
        }],
    }
}

/// Where [`pool_with_hole`]'s hole is: its middle and its radius (world pixels).
pub const HOLE: (Vec2, f32) = (Vec2::new(0.0, 290.0), 36.0);

/// The still box filled to 0.6 of its height (about 540 px deep) with a round hole 72 px across in
/// its water, its foot about 120 px above the floor: a pocket of air when `air`, nothing when not.
pub fn pool_with_hole(cell: f32, air: bool) -> Scene {
    let mut scene = still_box(cell, 0.6);
    scene.name = "pool with a hole";
    scene.holes = vec![Hole { centre: HOLE.0, radius: HOLE.1, air }];
    scene
}

/// The deep pool (as [`still_box`] at 0.6) narrower, so that 2 px detail is quick to play: 240 px
/// wide, 540 px deep.
pub fn deep_narrow(cell: f32) -> Scene {
    Scene { name: "deep narrow", ..boxed(cell, 240.0, 3.0, 0.6) }
}

/// A pool made by a pour: a 70 px tap at 300 px/s into an empty 300 x 400 px box for 2 s (about
/// 145 px deep at the end), so its water turns deep part way.
pub fn fill_pool(cell: f32) -> Scene {
    let half = Vec2::new(150.0, 200.0);
    Scene {
        name: "fill pool",
        layout: Layout::cover(-half - Vec2::splat(40.0), half + Vec2::splat(40.0), cell),
        solids: vec![boxed_in(half)],
        gravity: GRAVITY,
        feels: water(),
        fills: Vec::new(),
        holes: Vec::new(),
        wetting: DEFAULT_WETTING,
        pours: vec![Pour { place: Vec2::new(-40.0, -170.0), width: 70.0, speed: 300.0, until: 2.0, ..Pour::default() }],
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
        feels: vec![Feel::default()],
        fills,
        holes: Vec::new(),
        wetting: DEFAULT_WETTING,
        pours: Vec::new(),
    }
}

/// Where [`blob`]'s floor is (world y) and the half width of its box.
pub const BLOB: (f32, f32) = (150.0, 300.0);

/// A square blob of the engine's own water (Surface pull 0.5) `side` pixels across set on a level
/// floor in a box, every solid `wetting` wet: how it settles (a bead or a film) is Wetting's to say.
pub fn blob(cell: f32, side: f32, wetting: f32) -> Scene {
    let (floor, hw) = BLOB;
    let half = Vec2::new(hw, 250.0);
    let lo = Vec2::new(-0.5 * side, floor - side);
    Scene {
        name: "blob",
        layout: Layout::cover(Vec2::new(-hw, -250.0) - Vec2::splat(40.0), half + Vec2::splat(40.0), cell),
        solids: vec![boxed_in(half), Solid::rect(Vec2::new(-hw, floor), Vec2::new(hw, 250.0), true, 0.0)],
        gravity: GRAVITY,
        feels: vec![Feel::default()],
        fills: vec![(lo, Vec2::new(0.5 * side, floor - 0.5))],
        holes: Vec::new(),
        pours: Vec::new(),
        wetting,
    }
}

/// Shot 5's beads ([`beads`]) with every solid `wetting` wet.
pub fn blobs(cell: f32, wetting: f32) -> Scene {
    Scene { name: "blobs", wetting, ..beads(cell) }
}

/// The all-drops trial's shots by name (`docs/superpowers/plans/water-mode/all-drops-trial.md`):
/// `slope`, `ledge`, `pool`, `course`, `beads`, `weird`, `deep` (the deep pool at rest), `thin` (a
/// two-cell pour down a whole screen), and part C2's `narrow` ([`deep_narrow`]), `fill` ([`fill_pool`]).
pub fn shot(name: &str, cell: f32) -> Option<Scene> {
    Some(match name {
        "slope" => slope(cell),
        "ledge" => ledge(cell),
        "pool" => pool_pour(cell),
        "course" => course(cell),
        "beads" => beads(cell),
        "weird" => weird(cell),
        "deep" => still_box(cell, 0.6),
        "narrow" => deep_narrow(cell),
        "fill" => fill_pool(cell),
        "box" => still_box(cell, 0.3),
        "thin" => thin_pour(cell, 2.0),
        _ => return None,
    })
}
