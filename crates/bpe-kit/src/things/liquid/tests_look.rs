//! Tests of liquid part 3: the Liquid's look (its Surface, layers, real lighting and wet balls).

use std::sync::{Arc, OnceLock};

use bpe_core::{Scene, SlotId, ThingId, resolve};
use bpe_gfx::swarm::{SwarmGpu, use_on_this_thread};
use bpe_gfx::{Frame, SurfaceGpu, build_frame};
use bpe_runtime::{Event, EventKind, Log, Party, Playback};
use glam::Vec2;

use super::KIND;
use super::cards::{LOOK, LookSettings};
use super::draw::{reach_of, style_of, surface_index};
use super::look::{
    BUBBLES, CAUSTICS, FOAM, INSIDE, LookLayers, MIST, WET_BALLS, WET_WALLS, is_surface_number, kinds, look_defs,
    surface_of,
};
use super::wet::wetness;
use crate::modules::look::surface::{self, SurfaceKind};
use crate::scenes::{liquid_look_scenes, liquid_scenes};
use crate::things::particles::liquid::NOT_ON_A_LIQUID;
use crate::{kit, registry};

fn gpu() {
    static GPU: OnceLock<Arc<SwarmGpu>> = OnceLock::new();
    let gpu = GPU.get_or_init(|| {
        let (d, q) = bpe_gfx::test_device().expect("a graphics device (install mesa-vulkan-drivers)");
        SwarmGpu::for_device(&d, &q)
    });
    use_on_this_thread(Some(gpu.clone()));
}

fn look_scene(name: &str) -> Scene {
    liquid_look_scenes(&registry()).into_iter().find(|(n, _)| n == name).map(|(_, s)| s).expect("a look scene")
}

fn liquid_id(scene: &Scene) -> ThingId {
    scene.things.values().find(|t| t.kind == KIND).map(|t| t.id).expect("a liquid")
}

fn frame_of(scene: &Scene, time: f64) -> Frame {
    gpu();
    let kit = kit();
    let resolved = resolve(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(time);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), time, &kit.draws, &mut frame);
    frame
}

#[test]
fn the_look_holds_the_surface_numbers_a_liquid_wears_right_after_its_surface() {
    let defs = look_defs();
    let at = defs.iter().position(|d| d.key == "surface").expect("a Surface");
    let numbers: Vec<&str> =
        surface::sub_defs().iter().map(|d| d.key).filter(|k| *k != "surface" && !NOT_ON_A_LIQUID.contains(k)).collect();
    let after: Vec<&str> = defs[at + 1..at + 1 + numbers.len()].iter().map(|d| d.key).collect();
    assert_eq!(after, numbers);
    for k in NOT_ON_A_LIQUID {
        assert!(!defs.iter().any(|d| d.key == k), "{k} is not on a liquid");
        assert!(!is_surface_number(k));
    }
    // It starts as Glass, with Glass's numbers.
    assert_eq!(LookSettings::default().surface, SurfaceKind::Glass);
    for (k, v) in surface::preset_values(SurfaceKind::Glass).into_iter().filter(|(k, _)| is_surface_number(k)) {
        assert_eq!(defs.iter().find(|d| d.key == k).expect(k).default, v, "{k}");
    }
    // No key is there twice.
    let mut keys: Vec<&str> = defs.iter().map(|d| d.key).collect();
    keys.sort();
    let n = keys.len();
    keys.dedup();
    assert_eq!(keys.len(), n, "a key twice");
}

#[test]
fn every_look_layer_kind_is_on_the_liquid_look_once() {
    let reg = registry();
    let on: Vec<&str> = reg.layers_for(LOOK).map(|k| k.id()).collect();
    for k in [FOAM, BUBBLES, INSIDE, MIST, CAUSTICS, WET_WALLS, WET_BALLS] {
        assert!(on.contains(&k), "{k}");
    }
    assert!(kinds().iter().all(|k| !k.repeats && k.card == LOOK));
    let mut scene = liquid_scenes(&reg).remove(0).1;
    let id = liquid_id(&scene);
    scene.add_layer(&reg, id, SlotId::Look, FOAM).expect("foam");
    let resolved = resolve(&scene, &reg);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let layers = LookLayers::read(resolved.things[&id].slots.get(&SlotId::Look));
    assert!(layers.foam.is_some() && layers.bubbles.is_none());
}

#[test]
fn the_style_carries_only_the_layers_that_are_on() {
    let look = LookSettings::default();
    let none = style_of(&look, &LookLayers::default(), 2, (Vec2::new(-10.0, 5.0), 8.0), None);
    assert!(none.full(), "a Liquid thing's look is the full one");
    assert_eq!((none.m[0], none.o[0], none.p[2], none.q[2], none.s[1], none.u[0]), (0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
    assert!(!none.dots() && !none.wet());
    assert_eq!(none.e, [-10.0, 5.0, 8.0, 8.0 * reach_of(look.smoothing)]);
    assert_eq!(reach_of(0.25), 1.5, "the default Smoothing keeps part 1's reach");
    assert_eq!(none.h[0], look.clarity);
    assert!(none.reads_behind(), "clear water that bends reads what is behind it");
    assert_eq!(none.f[2], 2.0);
    let reg = registry();
    let scene = look_scene("Liquid looks");
    let resolved = resolve(&scene, &reg);
    let id = liquid_id(&scene);
    let card = resolved.things[&id].slots.get(&SlotId::Look);
    let layers = LookLayers::read(card);
    let all = style_of(&LookSettings::read(card), &layers, 0, (Vec2::ZERO, 8.0), None);
    assert!(all.m[0] > 0.0 && all.o[0] > 0.0 && all.p[2] > 0.0 && all.q[2] > 0.0 && all.s[1] > 0.0);
    assert!(all.dots() && all.wet());
    assert!(all.u[1] > 0.0 && all.u[1] < 1.0, "Dries in sets the wet floor: {:?}", all.u);
    // The Particles Liquid layer's style leaves every new number 0.
    let p = crate::things::particles::liquid::LiquidLayer::default();
    let old = p.style(1.0, &surface::SurfaceSettings::default(), None);
    assert!(!old.full() && !old.dots() && !old.wet());
    assert_eq!([old.h, old.m, old.o, old.u, old.v], [[0.0; 4]; 5]);
}

#[test]
fn a_liquid_takes_a_surface_index_after_every_particles_liquid() {
    let reg = registry();
    let scene = look_scene("Liquid lit pool");
    let resolved = resolve(&scene, &reg);
    let id = liquid_id(&scene);
    let first = surface::taken(&resolved) + crate::things::particles::liquid::layers_taken(&resolved) + 1;
    assert_eq!(surface_index(&resolved, id), Some(first));
}

#[test]
fn in_a_lit_scene_the_real_lights_shade_it_through_its_surface() {
    let scene = look_scene("Liquid lit pool");
    let id = liquid_id(&scene);
    let frame = frame_of(&scene, 0.2);
    let style = frame.liquids[0].style;
    assert!(style.real(), "a Lighting card: the real lights");
    assert!(!style.reads_behind(), "the lighting mix shows what is behind it");
    let resolved = resolve(&scene, &registry());
    let index = surface_index(&resolved, id).unwrap();
    assert_eq!(style.d[1], index as f32 + SurfaceGpu::GLASS_FLAG, "its index, see-through");
    let lights = frame.light.as_ref().expect("a lit frame");
    let s = surface_of(resolved.things[&id].slots.get(&SlotId::Look));
    assert_eq!(lights.surfaces[index as usize], s.gpu(SurfaceGpu::SHEET), "its Surface in the table");
    assert!(lights.refracts);
    assert!(style.d[2] > 0.0, "Blocks light is on by default");
    assert!(lights.benders.iter().any(|b| b.index == index), "glass that blocks light bends lamp light");
    // Unlit, it keeps its own light.
    let unlit = frame_of(&look_scene("Liquid looks"), 0.2);
    assert!(!unlit.liquids[0].style.real());
    assert!(unlit.light.is_none());
}

#[test]
fn wet_balls_are_wet_inside_and_dry_after_they_come_out() {
    let (liquid, ball) = (ThingId(7), ThingId(3));
    let mut log = Log::default();
    let event = |step: u64, item: u32| Event {
        step,
        kind: EventKind::Liquid,
        a: Party { thing: ball, item: 2 },
        b: Some(Party { thing: liquid, item }),
        point: Vec2::ZERO,
        normal: Vec2::ZERO,
        speed: 0.0,
        at: 0.0,
    };
    log.push(event(240, 0));
    log.push(event(480, 1));
    let rate = 240;
    assert!(wetness(&log, rate, 0.5, liquid, 3.0).is_empty(), "dry before it goes in");
    assert_eq!(wetness(&log, rate, 1.5, liquid, 3.0)[&(ball, 2)], 1.0, "wet through while in");
    let after = wetness(&log, rate, 5.0, liquid, 3.0)[&(ball, 2)];
    assert!((after - (-1.0f32).exp()).abs() < 1e-4, "{after}");
    assert!(wetness(&log, rate, 60.0, liquid, 3.0).is_empty(), "dry at last");
    assert!(wetness(&log, rate, 1.5, ThingId(8), 3.0).is_empty(), "another liquid's events");
}

#[test]
fn the_looks_scene_draws_wet_balls_once_a_ball_is_in() {
    let scene = look_scene("Liquid looks");
    let frame = frame_of(&scene, 3.0);
    assert_eq!(frame.liquids.len(), 1);
    assert!(frame.liquids[0].style.dots());
    assert!(!frame.particles_front.is_empty(), "the ball dropped in is wet");
}
