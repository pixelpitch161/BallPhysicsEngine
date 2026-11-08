//! Text part 3 off the graphics card: a Text set in the world is seen by the lights like a shape.
//! A laser stops at its letters or bends through glass ones, a light can light up only the Text,
//! glass letters bend lamp light, and the light probe sees a letter's shadow.

use bpe_core::{Choice, Scene, SlotId, Target, ThingId, Value};
use bpe_gfx::{Frame, build_frame};
use bpe_kit::modules::look::surface::{SurfaceKind, preset_values};
use bpe_kit::{kit, scenes};
use bpe_runtime::{Log, Outline, Playback, World};
use glam::Vec2;

fn frame_of(scene: &Scene, t: f64) -> Frame {
    let kit = kit();
    let resolved = bpe_core::resolve(scene, &kit.registry);
    let mut playback = Playback::new(scene, &resolved, kit.sims.clone());
    playback.seek(t);
    let mut frame = Frame::default();
    build_frame(scene, &resolved, playback.world(), playback.log(), t, &kit.draws, &mut frame);
    frame
}

fn thing_of(scene: &Scene, kind: &str) -> ThingId {
    scene.things.values().find(|t| t.kind == kind).unwrap().id
}

fn set_light(scene: &mut Scene, key: &str, v: Value) {
    let light = thing_of(scene, "light");
    scene.things.get_mut(&light).unwrap().params.set(key, v);
}

fn set_surface(scene: &mut Scene, thing: ThingId, kind: SurfaceKind) {
    let look = scene.card_mut(thing, SlotId::Look).unwrap();
    look.params.set("surface", Value::Choice(kind.name().into()));
    for (k, v) in preset_values(kind) {
        look.params.set(k, v);
    }
}

/// The lit room (its ring made huge, its ball moved out of the way to `ball`) with "I" set in
/// the world at the middle, 400 px high, that balls bounce off.
fn room_with_text(ball: Vec2) -> (Scene, ThingId) {
    let reg = kit().registry;
    let mut scene = scenes::lit_room(&reg);
    let shape = thing_of(&scene, "shape");
    scene.things.get_mut(&shape).unwrap().params.set("size", Value::Number(2000.0));
    let balls = thing_of(&scene, "balls");
    let b = scene.things.get_mut(&balls).unwrap();
    b.transform.position = ball;
    b.params.set("velocity", Value::Vec2([0.0, 0.0]));
    b.params.set("radius", Value::Number(20.0));
    let id = bpe_kit::add_thing(&mut scene, &reg, bpe_kit::things::text::KIND, ThingId::ROOT).unwrap();
    let t = scene.things.get_mut(&id).unwrap();
    t.screen = Some(false);
    t.transform.position = Vec2::ZERO;
    t.params.set("words", Value::Text("I".into()));
    t.params.set("size", Value::Number(400.0));
    t.params.set("balls", Value::Choice("bounce_off".into()));
    (scene, id)
}

/// The letter's box in the world.
fn letter_box(scene: &Scene, id: ThingId) -> (Vec2, Vec2) {
    let kit = kit();
    let resolved = bpe_core::resolve(scene, &kit.registry);
    let world = World::build(scene, &resolved, &kit.sims);
    let Outline::Letters { letters, center, .. } = &world.walls[&id].outline else { panic!("no letters") };
    (letters.lo + *center, letters.hi + *center)
}

#[test]
fn a_laser_stops_at_a_letter_and_bends_through_a_glass_one() {
    let (mut scene, id) = room_with_text(Vec2::new(0.0, 800.0));
    set_light(&mut scene, "kind", Value::Choice("laser".into()));
    set_light(&mut scene, "position", Value::Vec2([-450.0, -60.0]));
    set_light(&mut scene, "direction", Value::Number(15.0));
    set_light(&mut scene, "bounces", Value::Int(2));
    set_light(&mut scene, "at_shapes", Value::Choice("stop".into()));
    set_light(&mut scene, "colours", Value::Int(1));
    let (lo, hi) = letter_box(&scene, id);
    let beam = |scene: &Scene| {
        let lit = frame_of(scene, 0.0).light.unwrap();
        let l = lit.lights.iter().find(|l| l.p0[3] == 5.0).expect("a laser");
        let first = l.p4[2] as usize;
        lit.segments[first..first + l.p4[3] as usize].to_vec()
    };
    let stopped = beam(&scene);
    assert_eq!(stopped.len(), 1);
    let end = Vec2::new(stopped[0][2], stopped[0][3]);
    assert!((end.x - lo.x).abs() < 2.0 && end.y > lo.y && end.y < hi.y, "it stops on the letter's left side: {end}");
    // Glass bends a beam that would bounce off (one that stops there stops).
    set_surface(&mut scene, id, SurfaceKind::Glass);
    set_light(&mut scene, "at_shapes", Value::Choice("bounce".into()));
    let bent = beam(&scene);
    assert!(bent.len() >= 3, "in, through and out: {bent:?}");
    let inside = bent[1];
    let dir_in = Vec2::new(inside[2] - inside[0], inside[3] - inside[1]).normalize();
    let dir_out = Vec2::new(bent[0][2] - bent[0][0], bent[0][3] - bent[0][1]).normalize();
    assert!(dir_in.angle_to(dir_out).abs() > 0.05, "bent going in: {dir_in} vs {dir_out}");
    assert!(bent[2][2] > hi.x + 10.0 || bent[2][3] > hi.y, "and out the other side: {:?}", bent[2]);
}

#[test]
fn a_light_can_light_up_only_the_text() {
    let (mut scene, id) = room_with_text(Vec2::new(0.0, 800.0));
    set_light(&mut scene, "lights_up", Value::Choice("only_these".into()));
    set_light(&mut scene, "link_things", Value::Target(Target::Thing(id)));
    let kit = kit();
    let resolved = bpe_core::resolve(&scene, &kit.registry);
    let number = bpe_kit::things::light::links::number_of(&resolved, id);
    let frame = frame_of(&scene, 0.0);
    let lit = frame.light.as_ref().unwrap();
    assert!(lit.links.contains(&number), "{:?} names {number}", lit.links);
    let code = bpe_gfx::thing_code(id);
    let letters: Vec<_> = frame.paths.iter().filter(|p| p.c[0] == code).collect();
    assert_eq!(letters.len(), 1);
    let first = letters[0].layers[0] as usize;
    assert_eq!(frame.look_layers[first].more[3], number, "the letter's records carry its link");
}

#[test]
fn glass_letters_bend_lamp_light() {
    let (mut scene, id) = room_with_text(Vec2::new(0.0, 800.0));
    assert!(frame_of(&scene, 0.0).light.unwrap().benders.is_empty());
    set_surface(&mut scene, id, SurfaceKind::Glass);
    let lit = frame_of(&scene, 0.0).light.unwrap();
    let (lo, hi) = letter_box(&scene, id);
    assert_eq!(lit.benders.len(), 1);
    let b = lit.benders[0];
    assert!(b.lo[0] < lo.x && b.lo[1] < lo.y && b.hi[0] > hi.x && b.hi[1] > hi.y, "over the letter: {b:?}");
}

#[test]
fn the_light_probe_sees_a_letters_shadow() {
    // The light at the top, the ball under the letter: in its shadow; the ball off to the side
    // isn't.
    let shade = |ball: Vec2| {
        let (mut scene, _) = room_with_text(ball);
        set_light(&mut scene, "position", Value::Vec2([0.0, -600.0]));
        set_light(&mut scene, "reach", Value::Number(3000.0));
        let kit = kit();
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let world = World::build(&scene, &resolved, &kit.sims);
        let probe = bpe_kit::sensing::probe(&scene, &resolved, None);
        probe.ball(&world, &Log::default(), thing_of(&scene, "balls"), 0, 0.0)
    };
    let under = shade(Vec2::new(0.0, 400.0));
    let open = shade(Vec2::new(400.0, 400.0));
    assert!(under.shadow > 0.5 && open.shadow < 0.2, "{under:?} {open:?}");
    assert!(under.light < open.light);
}
