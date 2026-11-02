//! Wet paint over time. A canvas or skin with Dry time, Bleed or Drips (or a brush with Pickup)
//! runs a tick 30 times a second that works on the tiles with wet paint only: drips first, then
//! pickup, then bleed. The tick reads and writes only the canvas's own state, the balls' picked-up
//! paint and the log's marks. It never touches the physics.

use std::collections::BTreeMap;

use bpe_core::ThingId;
use bpe_runtime::{Mark, MarkShape, MarkTarget, Picked, World};
use glam::Vec2;

use super::grid::{Cell, wet};
use super::sim::CanvasState;

/// How a canvas or skin changes over time, fixed at build.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Evolve {
    /// 0..1, clamped.
    pub bleed: f32,
    /// 0..1, clamped.
    pub drips: f32,
    /// Ticks paint stays wet: round(dry_time × steps_per_second / tick_every), clamped to 0..=60_000.
    pub dry_ticks: u32,
    /// Whether some brush that marks this canvas has Pickup > 0 (canvases only; skins pick up at hits).
    pub pickup: bool,
}

impl Evolve {
    /// The settings of a canvas or skin for a world stepping `steps_per_second`.
    pub fn new(dry_time: f32, bleed: f32, drips: f32, steps_per_second: u32) -> Self {
        let tick = super::grid::tick_every(steps_per_second) as f32;
        let ticks = (dry_time.max(0.0) * steps_per_second as f32 / tick).round();
        Evolve {
            bleed: bleed.clamp(0.0, 1.0),
            drips: drips.clamp(0.0, 1.0),
            dry_ticks: if ticks.is_finite() { ticks.min(60_000.0) as u32 } else { 0 },
            pickup: false,
        }
    }

    /// Whether a tick has anything to do: dry_ticks > 0 and (bleed > 0 or drips > 0 or pickup).
    pub fn on(&self) -> bool {
        self.dry_ticks > 0 && (self.bleed > 0.0 || self.drips > 0.0 || self.pickup)
    }
}

/// A drip: a small blob of paint the tick moves.
#[derive(Clone, Debug, PartialEq)]
pub struct Runner {
    /// Where it is: canvas pixels, or the skin's thing's own frame.
    pub at: Vec2,
    /// Paint left, 0..1.
    pub load: f32,
    /// sRGB.
    pub rgb: [u8; 3],
    /// The direction of its last piece in radians; None before its first piece.
    pub heading: Option<f32>,
    /// Fixed at its start; its wobble comes from this and the tick.
    pub seed: u64,
    /// On a skin: where on the outline it runs.
    pub on: Option<OnLine>,
}

/// Where on a skin's outline a runner is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OnLine {
    pub line: u32,
    pub seg: u32,
    pub t: f32,
    pub offset: f32,
    pub dir: i8,
}

/// What one tick works with.
pub(crate) struct Tick<'a> {
    pub target: MarkTarget,
    pub world: &'a World,
    /// The full tick number: world.step / tick_every.
    pub now: u32,
    /// Ticks a second: steps_per_second / tick_every.
    pub per_second: f32,
    /// Gravity in the canvas's or thing's own frame.
    #[allow(dead_code)] // Drips read it (Task 6).
    pub gravity: Vec2,
}

/// Runs one tick on `st`: drips, pickup (canvases only), then bleed. Returns the marks it made,
/// in order. `picked` is World.picked, taken out of the world for the call. `outline` is the
/// skin's outline in its own frame (None for a canvas).
pub(crate) fn tick(
    cx: &Tick,
    st: &mut CanvasState,
    _picked: &mut BTreeMap<ThingId, Vec<Picked>>,
    _outline: Option<&[Vec<Vec2>]>,
) -> Vec<Mark> {
    let mut out = Vec::new();
    st.ticks_run += 1;
    // drips: Task 6
    // pickup: Task 8
    if st.evolve.bleed > 0.0 {
        bleed(cx, st, &mut out);
    }
    out
}

/// The four neighbours of (x, y) inside a grid of `width` x `height`, in the order left, right,
/// up, down.
fn neighbours(x: u32, y: u32, width: u32, height: u32) -> impl Iterator<Item = (u32, u32)> {
    [
        (x > 0).then(|| (x.wrapping_sub(1), y)),
        (x + 1 < width).then_some((x + 1, y)),
        (y > 0).then(|| (x, y.wrapping_sub(1))),
        (y + 1 < height).then_some((x, y + 1)),
    ]
    .into_iter()
    .flatten()
}

/// Wet paint takes thicker than this to spread into empty cells.
const SOURCE_AMOUNT: u8 = 48;

/// Bleed: the grid step and its Bleed marks (one per wet tile).
fn bleed(cx: &Tick, st: &mut CanvasState, out: &mut Vec<Mark>) {
    let dry = st.evolve.dry_ticks;
    let before = st.grid.clone();
    let tiles = before.wet_tiles(cx.now, dry);
    if tiles.is_empty() {
        return;
    }
    let w = st.evolve.bleed.clamp(0.0, 1.0) * 0.25;
    let (width, height) = before.dims();
    let age = |c: &Cell| (cx.now as u16).wrapping_sub(c.painted);
    for &t in &tiles {
        let (x0, x1, y0, y1) = before.tile_cells(t);
        // The tile grown by one cell on every side.
        let (gx0, gx1) = (x0.saturating_sub(1), (x1 + 1).min(width));
        let (gy0, gy1) = (y0.saturating_sub(1), (y1 + 1).min(height));
        for y in gy0..gy1 {
            for x in gx0..gx1 {
                let c = before.get(x, y);
                let near: Vec<Cell> = neighbours(x, y, width, height).map(|(nx, ny)| before.get(nx, ny)).collect();
                if wet(&c, cx.now, dry) {
                    let wets: Vec<&Cell> = near.iter().filter(|n| wet(n, cx.now, dry)).collect();
                    if wets.is_empty() {
                        continue;
                    }
                    let mut rgb = c.rgb;
                    for (k, channel) in rgb.iter_mut().enumerate() {
                        let avg = wets.iter().map(|n| n.rgb[k] as f32).sum::<f32>() / wets.len() as f32;
                        let own = c.rgb[k] as f32;
                        *channel = (own + (avg - own) * w).round().clamp(0.0, 255.0) as u8;
                    }
                    if rgb != c.rgb {
                        st.grid.set(x, y, Cell { rgb, ..c }, None);
                    }
                } else if c.amount == 0 {
                    let sources: Vec<&Cell> =
                        near.iter().filter(|n| wet(n, cx.now, dry) && n.amount >= SOURCE_AMOUNT).collect();
                    let sum: u32 = sources.iter().map(|n| n.amount as u32).sum();
                    let take = (sum as f32 * w).floor() as u32;
                    if sources.is_empty() || take < 1 {
                        continue;
                    }
                    let mut rgb = [0u8; 3];
                    for (k, channel) in rgb.iter_mut().enumerate() {
                        let mixed = sources.iter().map(|n| n.rgb[k] as f32 * n.amount as f32).sum::<f32>() / sum as f32;
                        *channel = mixed.round().clamp(0.0, 255.0) as u8;
                    }
                    // The newest of their paint dates it; on a tie the first neighbour does.
                    let mut newest = sources[0];
                    for n in &sources[1..] {
                        if age(n) < age(newest) {
                            newest = n;
                        }
                    }
                    let full = cx.now.wrapping_sub(age(newest) as u32);
                    let cell = Cell { rgb, amount: take.min(255) as u8, painted: newest.painted, team: 0, by: 0 };
                    st.grid.set(x, y, cell, Some(full));
                }
            }
        }
    }
    // One Bleed mark per wet tile, over the tile grown by one cell.
    let cs = before.cell_size();
    let seconds = dry as f32 / cx.per_second;
    for &t in &tiles {
        let (x0, x1, y0, y1) = before.tile_cells(t);
        out.push(Mark {
            step: cx.world.step,
            target: cx.target,
            shape: MarkShape::Bleed,
            from: before.centre_of(x0, y0) - Vec2::splat(cs * 1.5),
            to: before.centre_of(x1 - 1, y1 - 1) + Vec2::splat(cs * 1.5),
            radius: seconds,
            radius_to: st.evolve.bleed,
            angle: 0.0,
            within: [1.0, 1.0],
            variant: 0,
            colour: [0.0; 4],
            colour_to: [0.0; 4],
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{ARENA, CANVAS, busy, canvas_state, painted};
    use crate::things::canvas::skin::skin_state;
    use crate::{Kit, kit, scenes};
    use bpe_core::{Scene, Value, resolve};
    use bpe_runtime::{Log, Playback};

    /// A canvas scene with the settings, built into a world, with the canvas's state taken out.
    fn rig(bleed: f32, drips: f32, dry: f32) -> (World, CanvasState) {
        let (mut scene, _, kit) = painted(false, |_| {});
        let p = &mut scene.things.get_mut(&CANVAS).unwrap().params;
        p.set("bleed", Value::Number(bleed as f64));
        p.set("drips", Value::Number(drips as f64));
        p.set("dry_time", Value::Number(dry as f64));
        let resolved = resolve(&scene, &kit.registry);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        let st = canvas_state(&world, CANVAS).clone();
        world.states.remove(&CANVAS);
        (world, st)
    }

    fn splat(at: Vec2, radius: f32, colour: [f32; 4]) -> Mark {
        Mark {
            step: 0,
            target: MarkTarget::Canvas(CANVAS),
            shape: MarkShape::Splat,
            from: at,
            to: at,
            radius,
            radius_to: radius,
            angle: 0.0,
            within: [1.0, 1.0],
            variant: 0,
            colour,
            colour_to: colour,
        }
    }

    /// Runs ticks `1..=n` on `st`, with `gravity`, and gives the marks they made.
    fn ticks(world: &World, st: &mut CanvasState, n: u32, gravity: Vec2) -> Vec<Mark> {
        let mut out = Vec::new();
        let mut picked = BTreeMap::new();
        for now in 1..=n {
            let cx = Tick { target: MarkTarget::Canvas(CANVAS), world, now, per_second: 30.0, gravity };
            if st.evolve.on() {
                st.ticks_run += 1;
            }
            out.extend(tick(&cx, st, &mut picked, None));
        }
        out
    }

    fn cell_at(st: &CanvasState, x: f32, y: f32) -> Cell {
        let (cx, cy) = st.grid.cell_at(Vec2::new(x, y)).unwrap();
        st.grid.get(cx, cy)
    }

    #[test]
    fn no_tick_without_bleed_drips_or_pickup() {
        let kit = kit();
        for (name, scene) in scenes::marks(&kit.registry) {
            let resolved = resolve(&scene, &kit.registry);
            let (world, log) = crate::testing::run(&scene, &resolved, &kit, 5.0);
            assert!(
                log.marks().iter().all(|m| !matches!(m.shape, MarkShape::Drip | MarkShape::Lift | MarkShape::Bleed)),
                "{name}"
            );
            for state in world.states.values().chain(world.skins.values()) {
                if let Some(st) = state.as_any().downcast_ref::<CanvasState>() {
                    assert_eq!(st.ticks_run, 0, "{name}");
                }
            }
        }
    }

    #[test]
    fn bleed_blends_wet_neighbours() {
        let (world, mut st) = rig(1.0, 0.0, 5.0);
        st.grid.stamp(&splat(Vec2::new(-20.0, 0.0), 20.0, [1.0, 0.0, 0.0, 1.0]), 0, 1);
        st.grid.stamp(&splat(Vec2::new(20.0, 0.0), 20.0, [0.0, 0.0, 1.0, 1.0]), 0, 2);
        ticks(&world, &mut st, 10, Vec2::ZERO);
        let middle = cell_at(&st, 0.0, 0.0);
        assert!(middle.rgb[0] > 40 && middle.rgb[2] > 40, "{:?}", middle.rgb);
        assert!(cell_at(&st, -30.0, 0.0).rgb[0] > 200);
    }

    #[test]
    fn bleed_spreads_thick_paint_a_cell_or_two() {
        let (world, mut st) = rig(0.5, 0.0, 5.0);
        let mut line = splat(Vec2::new(-60.0, 0.0), 6.0, [1.0, 0.0, 0.0, 1.0]);
        line.shape = MarkShape::Stroke;
        line.to = Vec2::new(60.0, 0.0);
        st.grid.stamp(&line, 0, 1);
        ticks(&world, &mut st, 30, Vec2::ZERO);
        let cs = st.grid.cell_size();
        let (w, h) = st.grid.dims();
        for y in 0..h {
            for x in 0..w {
                if st.grid.get(x, y).amount > 0 {
                    let c = st.grid.centre_of(x, y);
                    assert!(c.y.abs() <= 6.0 + 2.0 * cs, "paint at {c:?}");
                }
            }
        }
    }

    #[test]
    fn dry_paint_never_bleeds() {
        let (world, mut st) = rig(1.0, 0.0, 0.2);
        st.grid.stamp(&splat(Vec2::new(-20.0, 0.0), 20.0, [1.0, 0.0, 0.0, 1.0]), 0, 1);
        st.grid.stamp(&splat(Vec2::new(20.0, 0.0), 20.0, [0.0, 0.0, 1.0, 1.0]), 0, 2);
        ticks(&world, &mut st, 7, Vec2::ZERO);
        let after_seven = st.grid.hash();
        // Run the rest from tick 8 on.
        let mut picked = BTreeMap::new();
        for now in 8..=20 {
            let cx =
                Tick { target: MarkTarget::Canvas(CANVAS), world: &world, now, per_second: 30.0, gravity: Vec2::ZERO };
            tick(&cx, &mut st, &mut picked, None);
        }
        assert_eq!(st.grid.hash(), after_seven);
    }

    #[test]
    fn bleed_logs_one_mark_per_wet_tile() {
        let (world, mut st) = rig(0.5, 0.0, 1.0);
        let at = st.grid.centre_of(10, 10);
        st.grid.stamp(&splat(at, 12.0, [1.0, 0.0, 0.0, 1.0]), 0, 1);
        let marks = ticks(&world, &mut st, 40, Vec2::ZERO);
        // Wet for 30 ticks (ticks 1..=29 see it wet), one mark per tick.
        assert_eq!(marks.len(), 29);
        for m in &marks {
            assert_eq!(m.shape, MarkShape::Bleed);
            assert!(m.from.x < at.x && m.from.y < at.y && m.to.x > at.x && m.to.y > at.y);
            assert!((m.radius - 1.0).abs() < 1e-4 && (m.radius_to - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn bleed_reads_the_grid_from_before_the_tick() {
        let (world, mut st) = rig(1.0, 0.0, 5.0);
        st.grid.stamp(&splat(Vec2::ZERO, 24.0, [1.0, 0.2, 0.0, 1.0]), 0, 1);
        ticks(&world, &mut st, 5, Vec2::ZERO);
        let (w, h) = st.grid.dims();
        for y in 0..h {
            for x in 0..w {
                assert_eq!(st.grid.get(x, y), st.grid.get(w - 1 - x, y), "({x}, {y})");
            }
        }
    }

    #[test]
    fn skin_evolves_without_hits() {
        let kit = kit();
        let (_, mut scene) = scenes::marks(&kit.registry).into_iter().find(|(n, _)| n == "Splat ring").unwrap();
        let arena = &mut scene.things.get_mut(&ARENA).unwrap().params;
        arena.set("skin_bleed", Value::Number(0.5));
        arena.set("skin_dry_time", Value::Number(3.0));
        let resolved = resolve(&scene, &kit.registry);
        let mut world = World::build(&scene, &resolved, &kit.sims);
        let mut log = Log::default();
        let skin_ticks = |w: &World| skin_state(w, ARENA).map_or(0, |s| s.ticks_run);
        // To the first paint on the skin.
        while skin_state(&world, ARENA).is_none_or(|s| s.grid.covered() == 0.0) {
            world.step(&kit.sims, &mut log);
            assert!(world.step < 240 * 20, "no hit on the skin");
        }
        let (start, before) = (world.step, skin_ticks(&world));
        let mut hitless = 0;
        for _ in 0..120 {
            world.step(&kit.sims, &mut log);
            let hit = log
                .marks()
                .iter()
                .any(|m| m.step == world.step && m.target == MarkTarget::Skin(ARENA) && m.shape != MarkShape::Bleed);
            if !hit && world.step.is_multiple_of(skin_state(&world, ARENA).unwrap().tick_every) {
                hitless += 1;
            }
        }
        assert!(world.step > start && hitless > 0);
        assert!(skin_ticks(&world) - before >= hitless, "{} vs {hitless}", skin_ticks(&world) - before);
    }

    fn with_wet(kit: &Kit, mut scene: Scene) -> (Scene, bpe_core::Resolved) {
        let p = &mut scene.things.get_mut(&CANVAS).unwrap().params;
        p.set("bleed", Value::Number(0.5));
        p.set("dry_time", Value::Number(2.0));
        let resolved = resolve(&scene, &kit.registry);
        (scene, resolved)
    }

    #[test]
    fn same_run_same_grid_with_bleed() {
        let (scene, _, kit) = busy();
        let (scene, resolved) = with_wet(&kit, scene);
        let mut a = Playback::new(&scene, &resolved, kit.sims.clone());
        let mut b = Playback::new(&scene, &resolved, kit.sims.clone());
        a.seek(6.0);
        b.seek(6.0);
        let (ga, gb) = (&canvas_state(a.world(), CANVAS).grid, &canvas_state(b.world(), CANVAS).grid);
        assert_eq!(ga.hash(), gb.hash());
        assert!(a.log().marks().iter().any(|m| m.shape == MarkShape::Bleed));
        assert_eq!(a.log().marks(), b.log().marks());
    }

    #[test]
    fn seek_equals_straight_with_bleed() {
        let (scene, _, kit) = busy();
        let (scene, resolved) = with_wet(&kit, scene);
        let mut straight = Playback::new(&scene, &resolved, kit.sims.clone());
        straight.seek(5.0);
        let mut jumpy = Playback::new(&scene, &resolved, kit.sims.clone());
        jumpy.seek(8.0);
        jumpy.seek(5.0);
        assert_eq!(canvas_state(straight.world(), CANVAS).grid.hash(), canvas_state(jumpy.world(), CANVAS).grid.hash());
        assert_eq!(straight.log().marks(), jumpy.log().marks());
        assert_eq!(straight.log().hash(), jumpy.log().hash());
    }
}
