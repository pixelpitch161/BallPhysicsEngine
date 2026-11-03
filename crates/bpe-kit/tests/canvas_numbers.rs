//! The numbers canvases report, and that every kind of canvas seeks to exactly what a straight
//! run gives.

use bpe_core::{Params, Resolved, Scene, SlotId, ThingId, Value, resolve};
use bpe_kit::{CanvasState, Kit, add_thing, fill_slot, kit, new_scene};
use bpe_runtime::{Log, Mark, MarkShape, MarkTarget, Playback, World, state, state_mut};
use glam::Vec2;

const BALLS: ThingId = ThingId(2);
const CANVAS: ThingId = ThingId(3);

/// The new scene with a canvas (with `canvas`'s changes) and `brush`'s changes to the ball's brushes.
fn scene_of(canvas: impl FnOnce(&mut Params), brush: Option<&dyn Fn(&mut Params)>) -> (Scene, Resolved, Kit) {
    let kit = kit();
    let mut scene = new_scene(&kit.registry);
    assert_eq!(add_thing(&mut scene, &kit.registry, "canvas", ThingId::ROOT).unwrap(), CANVAS);
    canvas(&mut scene.things.get_mut(&CANVAS).unwrap().params);
    scene.things.get_mut(&BALLS).unwrap().slots.remove(&SlotId::Marks);
    if let Some(brush) = brush {
        fill_slot(&mut scene, &kit.registry, BALLS, SlotId::Marks).unwrap();
        let m = bpe_core::resolve_slot(&scene, &kit.registry, BALLS, SlotId::Marks, &mut Vec::new()).unwrap().module;
        brush(&mut scene.library.modules.get_mut(&m).unwrap().params);
    }
    let resolved = resolve(&scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    (scene, resolved, kit)
}

fn choice(p: &mut Params, key: &str, v: &str) {
    p.set(key, Value::Choice(v.into()));
}

fn flag(p: &mut Params, key: &str, on: bool) {
    p.set(key, Value::Bool(on));
}

fn run(scene: &Scene, resolved: &Resolved, kit: &Kit, seconds: f64) -> (World, Log) {
    let mut world = World::build(scene, resolved, &kit.sims);
    let mut log = Log::default();
    world.advance_to(seconds, &kit.sims, &mut log);
    (world, log)
}

fn numbers_at(scene: &Scene, resolved: &Resolved, kit: &Kit, seconds: f64) -> bpe_runtime::CanvasNumbers {
    let (world, _) = run(scene, resolved, kit, seconds);
    world.canvas_numbers_of(CANVAS).cloned().expect("the canvas reported")
}

fn pour() -> (Scene, Resolved, Kit) {
    scene_of(|_| {}, Some(&|p| flag(p, "paint_on", true)))
}

fn scratch() -> (Scene, Resolved, Kit) {
    scene_of(
        |c| choice(c, "kind", "cover"),
        Some(&|p| {
            flag(p, "paint_on", false);
            flag(p, "scrape_on", true);
            flag(p, "chip_on", true);
        }),
    )
}

fn war() -> (Scene, Resolved, Kit) {
    scene_of(
        |c| {
            c.set("tile_size", Value::Number(40.0));
            c.set("grow", Value::Number(0.5));
        },
        Some(&|p| {
            flag(p, "paint_on", false);
            flag(p, "claim_on", true);
            flag(p, "claim_hits", true);
        }),
    )
}

fn sand() -> (Scene, Resolved, Kit) {
    scene_of(
        |c| {
            choice(c, "kind", "powder");
            c.set("grows_back", Value::Number(3.0));
        },
        Some(&|p| {
            flag(p, "paint_on", false);
            flag(p, "scrape_on", true);
        }),
    )
}

fn fog() -> (Scene, Resolved, Kit) {
    scene_of(
        |c| {
            choice(c, "kind", "fog");
            choice(c, "fog_look", "steam");
            c.set("grows_back", Value::Number(4.0));
            c.set("drops", Value::Number(1.0));
        },
        Some(&|p| {
            flag(p, "paint_on", false);
            flag(p, "scrape_on", true);
        }),
    )
}

#[test]
fn numbers_arrive_60_times_a_second() {
    let (scene, resolved, kit) = pour();
    let (_, log) = run(&scene, &resolved, &kit, 2.0);
    let samples: Vec<_> = log.canvas_samples_of(CANVAS).collect();
    assert!((120..=121).contains(&samples.len()), "{}", samples.len());
    assert!(samples.windows(2).all(|w| w[0].step < w[1].step));
    assert!(samples.windows(2).all(|w| w[1].numbers.covered >= w[0].numbers.covered));
    assert!(samples.last().unwrap().numbers.covered > 0.0);
}

#[test]
fn revealed_climbs_as_balls_scrape() {
    let (scene, resolved, kit) = scratch();
    let (one, five) = (numbers_at(&scene, &resolved, &kit, 1.0), numbers_at(&scene, &resolved, &kit, 5.0));
    assert!(five.revealed > one.revealed && one.revealed > 0.0, "{} {}", one.revealed, five.revealed);
    assert!((five.covered + five.revealed - 1.0).abs() < 1e-6);
}

#[test]
fn shares_add_up() {
    let (scene, resolved, kit) = war();
    let (world, _) = run(&scene, &resolved, &kit, 6.0);
    let n = world.canvas_numbers_of(CANVAS).unwrap();
    assert!(n.shares.iter().sum::<f32>() <= 1.0 + 1e-6);
    assert!(n.leader().is_some());
    let claims = state::<CanvasState>(world.states[&CANVAS].as_ref()).claims.as_ref().unwrap();
    assert_eq!(n.shares, claims.map.shares(claims.teams.len()));
    assert_eq!(world.teams_of(CANVAS).len(), n.shares.len());
}

#[test]
fn wet_falls_as_paint_dries() {
    // No brushes: paint is put in by hand at tick 0, so only drying changes `wet`.
    let (scene, resolved, kit) = scene_of(|_| {}, None);
    let mut world = World::build(&scene, &resolved, &kit.sims);
    let mut log = Log::default();
    {
        let st = state_mut::<CanvasState>(world.states.get_mut(&CANVAS).unwrap().as_mut());
        let m = Mark {
            step: 0,
            target: MarkTarget::Canvas(CANVAS),
            shape: MarkShape::Splat,
            from: Vec2::ZERO,
            to: Vec2::ZERO,
            radius: 40.0,
            radius_to: 40.0,
            angle: 0.0,
            within: [1.0, 1.0],
            variant: 0,
            colour: [1.0, 0.0, 0.0, 1.0],
            colour_to: [1.0, 0.0, 0.0, 1.0],
        };
        st.grid.stamp(&m, 0, 1);
    }
    world.advance_to(0.5, &kit.sims, &mut log);
    assert!(world.canvas_numbers_of(CANVAS).unwrap().wet > 0.9);
    world.advance_to(2.5, &kit.sims, &mut log);
    assert_eq!(world.canvas_numbers_of(CANVAS).unwrap().wet, 0.0);
}

/// Everything a canvas keeps that a seek must reproduce.
fn fingerprint(pb: &Playback) -> (u64, u64, Vec<bpe_kit::things::canvas::drops::FogDrop>, String, String) {
    let canvas = *pb.world().states.iter().find(|(_, s)| s.as_any().is::<CanvasState>()).expect("a canvas").0;
    let st = state::<CanvasState>(pb.world().states[&canvas].as_ref());
    let claim_hash = st.claims.as_ref().map_or(0, |c| c.map.hash());
    (
        st.grid.hash(),
        claim_hash,
        st.drops.clone(),
        format!("{:?}", pb.log().canvas_samples()),
        format!("{:?} {:?}", pb.log().teams_of(canvas), pb.world().teams_of(canvas)),
    )
}

/// A seek straight to 12 s gives the same as seeking 5 s, back to 2 s, then on to 12 s.
pub fn seeks_to_the_straight_run(scene: &Scene, resolved: &Resolved, kit: &Kit) {
    let mut straight = Playback::new(scene, resolved, kit.sims.clone());
    straight.seek(12.0);
    let mut jumpy = Playback::new(scene, resolved, kit.sims.clone());
    jumpy.seek(5.0);
    jumpy.seek(2.0);
    jumpy.seek(12.0);
    assert!(fingerprint(&straight) == fingerprint(&jumpy), "{}", scene.name);
}

/// A shipped Marks demo, built and resolved, and its canvas.
fn demo(name: &str) -> (Scene, Resolved, Kit, ThingId) {
    let kit = kit();
    let scene = bpe_kit::scenes::marks(&kit.registry)
        .into_iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("no demo called {name}"))
        .1;
    let resolved = bpe_kit::resolve_scene(&scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let canvas = *scene.things.iter().find(|(_, t)| t.kind == "canvas").expect("a canvas").0;
    (scene, resolved, kit, canvas)
}

#[test]
fn scratch_to_reveal_reveals() {
    let (scene, resolved, kit, card) = demo("Scratch to reveal");
    let (world, _) = run(&scene, &resolved, &kit, 15.0);
    let revealed = world.canvas_numbers_of(card).unwrap().revealed;
    assert!((0.2..0.98).contains(&revealed), "{revealed} of the card is scratched off");
}

#[test]
fn colour_war_has_four_teams() {
    let (scene, resolved, kit, war) = demo("Colour war");
    let (world, _) = run(&scene, &resolved, &kit, 10.0);
    let mut names: Vec<_> = world.teams_of(war).iter().map(|t| t.name.clone()).collect();
    names.sort();
    assert_eq!(names, ["Blue", "Green", "Red", "Yellow"]);
    let shares = &world.canvas_numbers_of(war).unwrap().shares;
    assert!(shares.iter().all(|s| *s > 0.02), "every team holds ground: {shares:?}");
}

#[test]
fn territory_grows() {
    let (scene, resolved, kit, land) = demo("Territory");
    let claimed = |seconds| {
        let (world, _) = run(&scene, &resolved, &kit, seconds);
        world.canvas_numbers_of(land).unwrap().shares.iter().sum::<f32>()
    };
    let (early, late) = (claimed(5.0), claimed(15.0));
    assert!(late > early * 1.5 || late > 0.9, "{early} claimed at 5 s, {late} at 15 s");
}

#[test]
fn sand_garden_ridges() {
    let (scene, resolved, kit, sand) = demo("Sand garden");
    let (world, _) = run(&scene, &resolved, &kit, 10.0);
    let grid = &state::<CanvasState>(world.states[&sand].as_ref()).grid;
    let (w, h) = grid.dims();
    let ridged = (0..h).any(|y| (0..w).any(|x| grid.get(x, y).amount > 160));
    assert!(ridged, "ploughed sand piles up above its rest height");
}

#[test]
fn fog_glass_runs_drops() {
    let (scene, resolved, kit, fog) = demo("Fog glass");
    let mut world = World::build(&scene, &resolved, &kit.sims);
    let mut log = Log::default();
    let mut seen = false;
    for k in 1..=40 {
        world.advance_to(k as f64 * 0.25, &kit.sims, &mut log);
        seen |= !state::<CanvasState>(world.states[&fog].as_ref()).drops.is_empty();
    }
    assert!(seen, "steam drops ran down the glass");
}

#[test]
fn every_new_demo_seeks_to_the_straight_run() {
    for (scene, resolved, kit) in [scratch(), war(), sand(), fog()] {
        seeks_to_the_straight_run(&scene, &resolved, &kit);
    }
    for name in ["Scratch to reveal", "Colour war", "Territory", "Sand garden", "Fog glass"] {
        let (scene, resolved, kit, _) = demo(name);
        seeks_to_the_straight_run(&scene, &resolved, &kit);
    }
}
