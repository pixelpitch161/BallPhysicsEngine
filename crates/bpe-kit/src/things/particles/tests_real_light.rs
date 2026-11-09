//! Tests of W15: a Liquid wears a ball's Surface and, with a Lighting card, the real lights shade
//! it and Soft smoke (its pictures are in `tests/lit_liquid_pictures.rs`).

use bpe_core::{LayerId, Params, Scene, Settings, SlotId, ThingId, Value, layered_key, resolve};
use bpe_gfx::{Frame, SurfaceGpu, build_frame};
use bpe_runtime::Playback;

use super::liquid::{LIQUID, LiquidLayer, RealLight, params_of, pretend_code, surface_index};
use super::look::{SMOKE, SmokeLayer};
use super::looks::{Look, add_particles};
use super::{BornFrom, Emit, ParticlesSettings};
use crate::modules::look::surface::{self, SurfaceKind, SurfaceSettings, preset_values};
use crate::things::light::{LightType, add_light};
use crate::{kit, registry};

/// The scene `scene` with a Particles thing whose only Look layer is `kind` with `params`,
/// giving 40 particles round the middle at the start.
fn with_particles(scene: &mut Scene, kind: &str, params: Params) -> (ThingId, LayerId) {
    let reg = registry();
    let id = add_particles(scene, &reg, Look::Empty, ThingId::ROOT).unwrap();
    let source = ParticlesSettings {
        from: BornFrom::APoint,
        emit: Emit::OnceAtTheStart,
        count: 40,
        life: 10.0,
        speed: 60.0,
        spread: 360.0,
        ..ParticlesSettings::default()
    };
    scene.things.get_mut(&id).unwrap().params = source.to_params();
    let old: Vec<LayerId> = scene.things[&id].slots[&SlotId::Look].layers.iter().map(|l| l.id).collect();
    let layer = scene.add_layer(&reg, id, SlotId::Look, kind).unwrap();
    for l in old {
        scene.remove_layer(&reg, id, SlotId::Look, l).unwrap();
    }
    let card = scene.things.get_mut(&id).unwrap().slots.get_mut(&SlotId::Look).unwrap();
    for (k, v) in params.0 {
        card.params.0.insert(layered_key(layer, &k), v);
    }
    (id, layer)
}

fn frame_of(scene: &Scene, time: f64) -> Frame {
    let kit = kit();
    let resolved = resolve(scene, &kit.registry);
    assert!(resolved.problems.is_empty(), "{:?}", resolved.problems);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(time);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), time, &kit.draws, &mut frame);
    frame
}

fn light_up(scene: &mut Scene) {
    add_light(scene, &registry(), LightType::Point, ThingId::ROOT).unwrap();
}

#[test]
fn a_liquid_wears_a_balls_surface_starting_as_plastic() {
    let reg = registry();
    let kind = reg.layer(LIQUID).unwrap();
    let get = |_: &str| None;
    let all: Vec<&str> =
        kind.settings().iter().chain(kind.dynamic_defs(&get)).chain(kind.light_defs()).map(|d| d.key).collect();
    // The Surface's numbers are named by no other setting of the layer.
    for d in surface::sub_defs() {
        assert_eq!(all.iter().filter(|k| **k == d.key).count(), 1, "{} once", d.key);
    }
    let defs = kind.settings();
    let surface = defs.iter().position(|d| d.key == "surface").expect("a Surface");
    assert_eq!(defs[surface + 1].key, "roughness", "its numbers right under it");
    for (key, value) in preset_values(SurfaceKind::Plastic) {
        let d = defs.iter().find(|d| d.key == key).unwrap_or_else(|| panic!("{key}"));
        assert_eq!(d.default, value, "{key} starts as Plastic's");
    }
    for key in surface::WHOLE_THING_ONLY {
        assert!(!defs.iter().any(|d| d.key == key), "{key} is a whole thing's only");
    }
    assert!(defs.iter().any(|d| d.key == "own_rim" && d.group == "Without lighting"));
}

#[test]
fn the_pretend_light_takes_the_nearest_of_its_own_looks() {
    use SurfaceKind as K;
    let codes: Vec<(K, f32)> = vec![
        (K::Matte, 0.0),
        (K::Plastic, 1.0),
        (K::Chrome, 2.0),
        (K::Metal, 3.0),
        (K::Glass, 4.0),
        (K::Toon, 5.0),
        (K::Velvet, 0.0),
        (K::Jelly, 1.0),
        (K::MirrorBall, 2.0),
        (K::Brushed, 3.0),
        (K::Frosted, 4.0),
        (K::Prism, 4.0),
    ];
    for (kind, code) in codes {
        assert_eq!(pretend_code(kind), code, "{kind:?}");
    }
}

#[test]
fn liquid_surfaces_are_numbered_after_every_balls_and_shapes() {
    let reg = registry();
    let mut scene = crate::new_scene(&reg);
    // The ball's first layer it can give a surface of its own takes one more index.
    let ball = scene.things.values().find(|t| t.kind == "balls").unwrap().id;
    let tops: Vec<LayerId> = scene.things[&ball].slots[&SlotId::Look].layers.iter().map(|l| l.id).collect();
    let own = tops.into_iter().any(|l| scene.add_sub_layer(&reg, ball, SlotId::Look, l, surface::SUB_SURFACE).is_ok());
    assert!(own, "a ball layer takes a Surface sub-layer");
    let (first, a) = with_particles(&mut scene, LIQUID, LiquidLayer::default().to_params());
    let (second, b) = with_particles(&mut scene, LIQUID, LiquidLayer::default().to_params());
    let resolved = resolve(&scene, &reg);
    let taken = surface::taken(&resolved);
    let surfaced =
        resolved.things.values().filter(|t| t.slots.get(&SlotId::Look).is_some_and(|l| surface::has_surface(&l.kind)));
    assert_eq!(taken, surfaced.count() as u32 + 1, "the things' and the one sub-layer's");
    assert_eq!(surface_index(&resolved, first, a), Some(taken + 1));
    assert_eq!(surface_index(&resolved, second, b), Some(taken + 2));
    assert_eq!(surface_index(&resolved, second, LayerId(999)), None, "not a layer of that thing");
}

#[test]
fn without_lighting_a_liquid_keeps_its_own_light_and_with_it_takes_the_real_lights() {
    let reg = registry();
    let mut scene = crate::new_studio_scene(&reg);
    let glass = LiquidLayer { surface: SurfaceKind::Glass, ..LiquidLayer::default() };
    let (id, layer) = with_particles(&mut scene, LIQUID, params_of(&glass));
    let unlit = frame_of(&scene, 0.2);
    let style = unlit.liquids[0].style;
    assert!(!style.real(), "no Lighting card: its own light");
    assert_eq!(style.d, [0.0; 4]);
    assert!(style.glass(), "its own Glass reads the picture behind it");
    light_up(&mut scene);
    let lit = frame_of(&scene, 0.2);
    let style = lit.liquids[0].style;
    assert!(style.real(), "a Lighting card: the real lights");
    assert!(!style.glass(), "the lighting shows what is behind it");
    let resolved = resolve(&scene, &reg);
    let index = surface_index(&resolved, id, layer).unwrap();
    assert_eq!(style.d[1], index as f32 + SurfaceGpu::GLASS_FLAG, "its index, see-through");
    assert_eq!(style.d[2], 0.0, "it blocks no light until asked");
    assert_eq!(style.c[2], 0.0, "it bends lamp light through its surface, not as free glass");
    let scene_lights = lit.light.as_ref().expect("a lit frame");
    let s = SurfaceSettings::from_params(&params_of(&glass));
    assert_eq!(scene_lights.surfaces[index as usize], s.gpu(SurfaceGpu::SHEET), "its Surface in the table");
    assert!(scene_lights.refracts, "see-through: the frame makes a picture behind glass");
    // Lit 0 keeps its own look in a lit scene; Blocks light blocks (less where it lets light through).
    let card = scene.things.get_mut(&id).unwrap().slots.get_mut(&SlotId::Look).unwrap();
    card.params.set(&layered_key(layer, "lit"), Value::Number(0.0));
    assert!(!frame_of(&scene, 0.2).liquids[0].style.real());
    let card = scene.things.get_mut(&id).unwrap().slots.get_mut(&SlotId::Look).unwrap();
    card.params.set(&layered_key(layer, "lit"), Value::Number(1.0));
    card.params.set(&layered_key(layer, "blocks_light"), Value::Bool(true));
    let blocking = frame_of(&scene, 0.2);
    let blocks = blocking.liquids[0].style.d[2];
    assert!((blocks - surface::keeps_of(0.85)).abs() < 1e-6, "{blocks}");
    // Blocking light, its glass bends lamp light as a glass shape's does: a bender round its blobs.
    let benders = &blocking.light.as_ref().unwrap().benders;
    let bender = benders.iter().find(|b| b.index == index).expect("a bender with its index");
    for b in &blocking.liquids[0].blobs {
        assert!(b.place[0] >= bender.lo[0] && b.place[0] <= bender.hi[0], "round every blob");
        assert!(b.place[1] >= bender.lo[1] && b.place[1] <= bender.hi[1], "round every blob");
    }
}

#[test]
fn a_liquid_style_says_what_the_shader_reads() {
    let l = LiquidLayer { surface: SurfaceKind::Pearl, own_rim: 0.7, ..LiquidLayer::default() };
    let s = SurfaceSettings::from_params(&params_of(&l));
    let pretend = l.style(0.8, &s, None);
    assert_eq!(pretend.a[3], 1.0, "Pearl's own light is Plastic's");
    assert_eq!(pretend.c, [0.7, 0.8, 0.0, 0.0]);
    let real = l.style(0.8, &s, Some(RealLight { lit: 1.5, stamp: 7.0, blocks: 2.0 }));
    assert_eq!(real.d, [1.5, 7.0, 1.0, 0.0]);
    assert_eq!(real.c[2], 0.0, "Pearl lets no light through, so bends none");
}

#[test]
fn smoke_stands_up_for_the_real_lights_only_in_a_lit_scene() {
    let reg = registry();
    let mut scene = crate::new_studio_scene(&reg);
    let smoke = SmokeLayer { shading: 0.5, ..SmokeLayer::default() };
    let (id, layer) = with_particles(&mut scene, SMOKE, smoke.to_params());
    let unlit = frame_of(&scene, 0.2);
    assert_eq!(unlit.fields.len(), 1);
    assert_eq!(unlit.fields[0].look.shade[3], 0.0, "no Lighting card: its own light");
    light_up(&mut scene);
    let lit = frame_of(&scene, 0.2);
    assert_eq!(lit.fields[0].look.shade[3], super::draw::SMOKE_RELIEF * 0.5);
    let card = scene.things.get_mut(&id).unwrap().slots.get_mut(&SlotId::Look).unwrap();
    card.params.set(&layered_key(layer, "lit"), Value::Number(0.0));
    assert_eq!(frame_of(&scene, 0.2).fields[0].look.shade[3], 0.0, "Lit 0: its own light");
}
