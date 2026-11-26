use super::*;
use crate::modules::water::{self as cards, SCREEN};
use crate::things::water::{COURSE, DRAIN, FORCE, KINDS, LAMP, PIECE, StartingLook, WALL, new_water_scene};
use bpe_core::{Address, Registry, SettingDef, Stroke, Value, ValueType};

fn water_scene(reg: &Registry) -> Scene {
    new_water_scene(reg, StartingLook::Plain, "")
}

fn add(scene: &mut Scene, reg: &Registry, kind: &str, at: Vec2) -> ThingId {
    let id = crate::add_thing(scene, reg, kind, ThingId::ROOT).unwrap();
    scene.things.get_mut(&id).unwrap().transform.position = at;
    id
}

fn set(scene: &mut Scene, id: ThingId, slot: SlotId, key: &str, v: Value) {
    scene.things.get_mut(&id).unwrap().slots.get_mut(&slot).unwrap().params.set(key, v);
}

fn carve(scene: &mut Scene, list: Vec<Stroke>) {
    let course = course_of(scene).unwrap();
    set(scene, course, SlotId::Form, "carved", Value::Strokes(Strokes { list }));
}

/// A square hole of half-width `half` round `at`.
fn hole(at: [f32; 2], half: f32) -> Stroke {
    Stroke { points: vec![at], size: half, strength: 1.0, square: true, ..Default::default() }
}

fn setup(scene: &Scene, reg: &Registry) -> WaterSetup {
    WaterSetup::of(scene, &bpe_core::resolve(scene, reg))
}

fn fill_pour(scene: &mut Scene, reg: &Registry, at: Vec2, size: Vec2) -> ThingId {
    let pour = add(scene, reg, POUR, at);
    set(scene, pour, SlotId::Main, "kind", Value::Choice("fill_at_the_start".into()));
    set(scene, pour, SlotId::Main, "fill_size", Value::Vec2(size.to_array()));
    pour
}

#[test]
fn a_new_water_scene_sets_up_an_all_rock_course_with_nothing_poured() {
    let reg = crate::registry();
    let s = setup(&water_scene(&reg), &reg);
    assert_eq!((s.size, s.cell), (SCREEN, 8.0));
    assert!(s.liquids.is_empty() && s.pours.is_empty() && s.notes.is_empty());
    assert_eq!(s.layout(), layout_for(SCREEN, 8.0));
    let field = s.field();
    assert_eq!(field.layout, s.layout());
    assert!(field.distance.iter().all(|d| *d < 0.0), "all rock");
    let inputs = s.inputs_at(0);
    assert!(inputs.feels.is_empty() && inputs.pours.is_empty());
    assert_eq!(inputs.gravity, Vec2::new(0.0, 4000.0));
}

#[test]
fn a_tap_pours_its_liquid_at_its_place() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    add(&mut scene, &reg, LIQUID, Vec2::ZERO);
    let pour = add(&mut scene, &reg, POUR, Vec2::new(100.0, -500.0));
    for (key, v) in [("rate", 0.5), ("width", 60.0), ("speed", 400.0), ("aim", 80.0), ("spread", 5.0)] {
        set(&mut scene, pour, SlotId::Main, key, Value::Number(v));
    }
    set(&mut scene, pour, SlotId::Main, "starts", Value::Number(1.0));
    set(&mut scene, pour, SlotId::Main, "stops", Value::Number(3.0));
    let s = setup(&scene, &reg);
    let inputs = s.inputs_at(0);
    let want = Pour {
        place: Vec2::new(100.0, -500.0),
        aim: 80.0,
        width: 30.0,
        speed: 400.0,
        spread: 5.0,
        from: 1.0,
        until: 3.0,
        kind: STOPS,
        colour: WATER,
    };
    assert_eq!(inputs.pours, [want]);
    assert_eq!(inputs.feels, [Feel::default()], "the Feel card's defaults are the engine's");
    assert!(s.fills(&s.layout(), &s.field()).is_empty());
    assert_eq!(s.note_for(pour), None);
}

#[test]
fn a_fill_at_the_start_fills_its_box_where_it_is_open() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    carve(&mut scene, vec![hole([0.0, 0.0], 200.0)]);
    add(&mut scene, &reg, LIQUID, Vec2::ZERO);
    // Half of it in the rock: it runs past the hole's sides and its floor.
    fill_pour(&mut scene, &reg, Vec2::new(0.0, 150.0), Vec2::new(600.0, 200.0));
    let s = setup(&scene, &reg);
    let (layout, field) = (s.layout(), s.field());
    let drops = s.fills(&layout, &field);
    for d in &drops {
        let at = layout.to_world(Vec2::from(d.x));
        assert!(field.at(at) > 0.0, "a drop in the rock at {at}");
        assert_eq!(d.kind, STOPS);
    }
    // The open part: 400 across by 150 down, four drops a cell.
    let want = 400.0 * 150.0 / (4.0 * 4.0);
    let got = drops.len() as f32;
    assert!((got - want).abs() <= want * 0.06, "{got} drops, about {want} wanted");
    assert!(s.inputs_at(0).pours.is_empty(), "a fill pours nothing after the start");
}

#[test]
fn a_tap_in_the_rock_pours_nothing_and_says_so() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    add(&mut scene, &reg, LIQUID, Vec2::ZERO);
    let tap = add(&mut scene, &reg, POUR, Vec2::new(0.0, -500.0));
    let s = setup(&scene, &reg);
    let field = s.field();
    assert_eq!(s.taps_in_rock(&field), [tap], "a course of solid rock");
    let inputs = s.inputs_on(0, &field);
    assert_eq!(inputs.pours.len(), 1, "it keeps its place among the taps");
    assert_eq!(inputs.pours[0].width, 0.0);
    // Carved out round its spout, it pours as it should.
    carve(&mut scene, vec![hole([0.0, -500.0], 200.0)]);
    let s = setup(&scene, &reg);
    let field = s.field();
    assert!(s.taps_in_rock(&field).is_empty());
    assert_eq!(s.inputs_on(0, &field), s.inputs_at(0));
}

#[test]
fn a_tap_wider_than_its_hole_pours_from_the_open_part() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    carve(&mut scene, vec![hole([0.0, 0.0], 200.0)]);
    add(&mut scene, &reg, LIQUID, Vec2::ZERO);
    // Pouring straight down 150 from the hole's left side: 50 of room on that side.
    let tap = add(&mut scene, &reg, POUR, Vec2::new(-150.0, -100.0));
    set(&mut scene, tap, SlotId::Main, "rate", Value::Number(1.0));
    set(&mut scene, tap, SlotId::Main, "width", Value::Number(300.0));
    let s = setup(&scene, &reg);
    let field = s.field();
    let width = s.inputs_on(0, &field).pours[0].width;
    assert!((80.0..=100.0).contains(&width), "{width}");
    let pour = &s.inputs_on(0, &field).pours[0];
    let layout = s.layout();
    let born = pour.rows(&mut bpe_water::Poured::default(), &layout, 0.0, 1.0 / 60.0);
    assert!(!born.is_empty());
    for d in &born {
        let at = layout.to_world(Vec2::from(d.x));
        assert!(field.at(at) > 0.0, "a drop born in the rock at {at}");
    }
}

#[test]
fn huge_pours_are_kept_to_the_course() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    carve(&mut scene, vec![hole([0.0, 0.0], 300.0)]);
    add(&mut scene, &reg, LIQUID, Vec2::ZERO);
    fill_pour(&mut scene, &reg, Vec2::ZERO, Vec2::new(1e7, 1e7));
    let tap = add(&mut scene, &reg, POUR, Vec2::new(0.0, -200.0));
    set(&mut scene, tap, SlotId::Main, "rate", Value::Number(1e6));
    set(&mut scene, tap, SlotId::Main, "width", Value::Number(1e9));
    let s = setup(&scene, &reg);
    let (layout, field) = (s.layout(), s.field());
    let started = std::time::Instant::now();
    let drops = s.fills(&layout, &field);
    let width = s.inputs_on(0, &field).pours[0].width;
    assert!(started.elapsed().as_secs_f32() < 2.0, "{:?}", started.elapsed());
    // The hole's 600 by 600, four drops a cell.
    assert!(drops.len() <= 600 * 600 / 16 + 600, "{}", drops.len());
    assert!(width <= 600.0, "{width}");
}

#[test]
fn overlapping_fills_do_not_double_up() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    carve(&mut scene, vec![hole([0.0, 0.0], 300.0)]);
    add(&mut scene, &reg, LIQUID, Vec2::ZERO);
    fill_pour(&mut scene, &reg, Vec2::new(0.0, 200.0), Vec2::new(400.0, 200.0));
    let one = {
        let s = setup(&scene, &reg);
        s.fills(&s.layout(), &s.field()).len()
    };
    assert!(one > 0);
    fill_pour(&mut scene, &reg, Vec2::new(0.0, 200.0), Vec2::new(400.0, 200.0));
    let s = setup(&scene, &reg);
    assert_eq!(s.fills(&s.layout(), &s.field()).len(), one);
}

#[test]
fn a_pour_finds_its_liquid() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    let first = add(&mut scene, &reg, LIQUID, Vec2::ZERO);
    let hot = add(&mut scene, &reg, LIQUID, Vec2::ZERO);
    scene.things.get_mut(&hot).unwrap().tags.insert("hot".into());
    let targets = [
        Target::Thing(hot),
        Target::Kind(LIQUID.into()),
        Target::Anything,
        Target::Tag("hot".into()),
        Target::Tag("cold".into()),
        Target::Me,
    ];
    let pours: Vec<ThingId> = targets
        .iter()
        .map(|t| {
            let p = add(&mut scene, &reg, POUR, Vec2::ZERO);
            set(&mut scene, p, SlotId::Main, "liquid", Value::Target(t.clone()));
            p
        })
        .collect();
    let liquids = |scene: &Scene| -> Vec<Option<usize>> {
        let s = setup(scene, &reg);
        pours.iter().map(|id| s.pours.iter().find(|p| p.id == *id).unwrap().liquid).collect()
    };
    assert_eq!(liquids(&scene), [Some(1), Some(0), Some(0), Some(1), None, None]);
    let s = setup(&scene, &reg);
    assert_eq!(s.note_for(pours[4]), Some(NO_LIQUID));
    assert_eq!(s.inputs_at(0).pours.len(), 4, "the two with no Liquid pour nothing");
    // A Liquid that is off doesn't count: the hot one is the first now.
    scene.things.get_mut(&first).unwrap().enabled = false;
    assert_eq!(liquids(&scene), [Some(0), Some(0), Some(0), Some(0), None, None]);
    // With no Liquid at all, nothing pours.
    let mut bare = water_scene(&reg);
    let p = add(&mut bare, &reg, POUR, Vec2::ZERO);
    let s = setup(&bare, &reg);
    assert_eq!((s.note_for(p), s.inputs_at(0).pours.len()), (Some(NO_LIQUID), 0));
}

#[test]
fn later_pour_kinds_pour_nothing_yet() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    carve(&mut scene, vec![hole([0.0, 0.0], 300.0)]);
    add(&mut scene, &reg, LIQUID, Vec2::ZERO);
    for kind in ["rain", "drip", "burst"] {
        let p = add(&mut scene, &reg, POUR, Vec2::ZERO);
        set(&mut scene, p, SlotId::Main, "kind", Value::Choice(kind.into()));
        let s = setup(&scene, &reg);
        assert_eq!(s.note_for(p), Some(LATER_KINDS), "{kind}");
        assert!(s.inputs_at(0).pours.is_empty(), "{kind}");
        assert!(s.fills(&s.layout(), &s.field()).is_empty(), "{kind}");
    }
}

#[test]
fn keyframed_physics_is_read_each_frame() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    add(&mut scene, &reg, LIQUID, Vec2::ZERO);
    let pour = add(&mut scene, &reg, POUR, Vec2::ZERO);
    let gravity = Address { thing: ThingId::ROOT, slot: Some(SlotId::Water), key: "gravity".into() };
    scene.set_key(&reg, &gravity, 0.0, Value::Vec2([0.0, 4000.0])).unwrap();
    scene.set_key(&reg, &gravity, 1.0, Value::Vec2([0.0, 1000.0])).unwrap();
    let rate = Address { thing: pour, slot: Some(SlotId::Main), key: "rate".into() };
    scene.set_key(&reg, &rate, 0.0, Value::Number(1.0)).unwrap();
    scene.set_key(&reg, &rate, 1.0, Value::Number(0.0)).unwrap();
    let s = setup(&scene, &reg);
    assert_eq!(s.inputs_at(0).gravity, Vec2::new(0.0, 4000.0));
    assert!((s.inputs_at(30).gravity.y - 2500.0).abs() < 0.01, "{}", s.inputs_at(30).gravity);
    assert_eq!(s.inputs_at(0).pours[0].width, 40.0);
    assert_eq!(s.inputs_at(60).pours[0].width, 0.0);
    assert_eq!(s.inputs_at(90).pours.len(), 1, "the tap stays in the list, so its place in it never changes");
}

#[test]
fn a_detail_change_gives_a_new_layout() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    let before = setup(&scene, &reg);
    set(&mut scene, ThingId::ROOT, SlotId::Water, "draft_detail", Value::Choice("fine".into()));
    let s = setup(&scene, &reg);
    assert_eq!((s.cell, s.layout().cell, s.field().layout.cell), (6.0, 6.0, 6.0));
    assert_ne!(s.print, before.print);
}

/// A different value for a setting (the rule of `tests/restarts.rs`, plus a stroke for strokes
/// and another target for a target).
fn changed(def: &SettingDef, now: &Value, liquid: ThingId) -> Option<Value> {
    let width = def.range.or(def.span).map_or(1.0, |(lo, hi)| (hi - lo).min(100.0));
    let moved = |x: f64| {
        let step = (x.abs() * 0.5).max(width * 0.1).max(1e-3);
        let up = def.clamp(Value::Number(x + step));
        if up != Value::Number(x) { up } else { def.clamp(Value::Number(x - step)) }
    };
    let v = match (def.ty, now) {
        (ValueType::Number, Value::Number(x)) => moved(*x),
        (ValueType::Int, Value::Int(x)) => {
            let up = def.clamp(Value::Int(x + 1 + x.abs() / 2));
            if up != Value::Int(*x) { up } else { def.clamp(Value::Int(x - 1)) }
        }
        (ValueType::Bool, Value::Bool(b)) => Value::Bool(!b),
        (ValueType::Choice(names), Value::Choice(c)) => {
            let at = names.iter().position(|n| n == c)?;
            Value::Choice(names[(at + 1) % names.len()].to_string())
        }
        (ValueType::Vec2, Value::Vec2([x, y])) => Value::Vec2([x + 13.0, y - 7.0]),
        (ValueType::Color, Value::Color(c)) => {
            let other = if c.r > 128 { Color::rgb(20, 200, 90) } else { Color::rgb(240, 60, 30) };
            Value::Color(other)
        }
        (ValueType::Text, Value::Text(t)) => Value::Text(if t == "Hi" { "Yo".into() } else { "Hi".into() }),
        (ValueType::Strokes, Value::Strokes(s)) => {
            let mut s = s.clone();
            s.list.push(hole([10.0, 20.0], 30.0));
            Value::Strokes(s)
        }
        (ValueType::Target, Value::Target(t)) => {
            Value::Target(if *t == Target::Thing(liquid) { Target::Anything } else { Target::Thing(liquid) })
        }
        _ => return None,
    };
    (v != *now).then_some(v)
}

#[test]
fn what_restarts_the_draft() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    carve(&mut scene, vec![hole([0.0, 0.0], 300.0)]);
    let mut ids = Vec::new();
    for kind in KINDS.into_iter().filter(|k| *k != COURSE) {
        ids.push(add(&mut scene, &reg, kind, Vec2::new(10.0, 20.0)));
    }
    let liquid = scene.order.iter().copied().find(|id| scene.things[id].kind == LIQUID).unwrap();
    crate::fill_slot(&mut scene, &reg, liquid, SlotId::Heat).unwrap();
    let base = setup(&scene, &reg).print;
    let physics = |card: &str, key: &str| match card {
        cards::WORLD => ["gravity", "screens_wide", "screens_tall", "draft_detail"].contains(&key),
        cards::CARVE | cards::FEEL | cards::POUR => true,
        _ => false,
    };
    let mut checked = 0;
    let mut wrong = Vec::new();
    for id in scene.order.clone() {
        let thing = scene.things[&id].clone();
        for (slot, card) in &thing.slots {
            let Some(kind) = reg.module(&card.kind) else { continue };
            for def in kind.settings() {
                let now = card.params.get(def.key).cloned().unwrap_or_else(|| def.default.clone());
                let Some(v) = changed(def, &now, liquid) else { continue };
                let mut edited = scene.clone();
                set(&mut edited, id, *slot, def.key, v);
                let restarts = setup(&edited, &reg).print != base;
                if restarts != physics(&card.kind, def.key) {
                    wrong.push(format!("{} {}: restarts {restarts}", card.kind, def.key));
                }
                checked += 1;
            }
        }
        // Moving it, and turning it off.
        if crate::things::water::is_placed(&thing.kind) {
            let mut moved = scene.clone();
            moved.things.get_mut(&id).unwrap().transform.position += Vec2::new(5.0, 0.0);
            let restarts = setup(&moved, &reg).print != base;
            if restarts != (thing.kind == POUR) {
                wrong.push(format!("moving a {}: restarts {restarts}", thing.kind));
            }
            checked += 1;
        }
        if [LIQUID, POUR, PIECE, WALL, DRAIN, FORCE, LAMP].contains(&thing.kind.as_str()) {
            let mut off = scene.clone();
            off.things.get_mut(&id).unwrap().enabled = false;
            let restarts = setup(&off, &reg).print != base;
            if restarts != [LIQUID, POUR].contains(&thing.kind.as_str()) {
                wrong.push(format!("turning off a {}: restarts {restarts}", thing.kind));
            }
            checked += 1;
        }
    }
    // Keyframes count where the setting does.
    let pour = ids[KINDS.iter().position(|k| *k == POUR).unwrap() - 1];
    for (thing, slot, key, v, restarts) in [
        (liquid, SlotId::Feel, "thickness", Value::Number(0.5), true),
        (liquid, SlotId::Look, "colour", Value::Color(Color::rgb(1, 2, 3)), false),
        (ThingId::ROOT, SlotId::Water, "gravity", Value::Vec2([0.0, 10.0]), true),
        (ThingId::ROOT, SlotId::Back, "darker", Value::Number(0.1), false),
        (pour, SlotId::Main, "speed", Value::Number(10.0), true),
    ] {
        let mut keyed = scene.clone();
        keyed.set_key(&reg, &Address { thing, slot: Some(slot), key: key.into() }, 2.0, v).unwrap();
        if (setup(&keyed, &reg).print != base) != restarts {
            wrong.push(format!("a key on {key}: restarts {}", !restarts));
        }
        checked += 1;
    }
    println!("what restarts the draft: {checked} changes checked");
    assert!(checked > 60, "{checked}");
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn the_plain_look_reads_the_cards() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    let liquid = add(&mut scene, &reg, LIQUID, Vec2::ZERO);
    set(&mut scene, liquid, SlotId::Look, "colour", Value::Color(Color::rgb(255, 0, 0)));
    set(&mut scene, liquid, SlotId::Look, "deep", Value::Color(Color::rgb(0, 0, 0x40)));
    set(&mut scene, liquid, SlotId::Look, "clarity", Value::Number(50.0));
    let course = course_of(&scene).unwrap();
    set(&mut scene, course, SlotId::Material, "colour", Value::Color(Color::rgb(0x10, 0x20, 0x30)));
    set(&mut scene, ThingId::ROOT, SlotId::Back, "shows", Value::Choice("colour".into()));
    set(&mut scene, ThingId::ROOT, SlotId::Back, "colour", Value::Color(Color::rgb(1, 2, 3)));
    let look = |scene: &Scene| plain_look(scene, &bpe_core::resolve(scene, &reg), 0.0);
    let got = look(&scene);
    let c = |r: f32, g: f32, b: f32| [r / 255.0, g / 255.0, b / 255.0];
    assert_eq!(got.back, c(1.0, 2.0, 3.0));
    assert_eq!(got.rock, c(16.0, 32.0, 48.0));
    assert_eq!(got.liquids, [PlainLiquid { colour: [1.0, 0.0, 0.0], deep: c(0.0, 0.0, 64.0), clarity: 50.0 }]);
    set(&mut scene, ThingId::ROOT, SlotId::Back, "shows", Value::Choice("darker_rock".into()));
    set(&mut scene, ThingId::ROOT, SlotId::Back, "darker", Value::Number(0.25));
    let got = look(&scene);
    for (b, r) in got.back.iter().zip(got.rock) {
        assert!((b - r * 0.75).abs() < 1e-6);
    }
    // The defaults are the plain drawing's own.
    let plain = look(&water_scene(&reg));
    assert_eq!(plain, PlainLook::default());
}
