use super::*;
use crate::modules::water::{self as cards, SCREEN};
use crate::things::water::{COURSE, DRAIN, FORCE, KINDS, LAMP, PIECE, WALL, new_water_scene};
use bpe_core::{Address, Color, Registry, SettingDef, Stroke, Value, ValueType};
use bpe_water::draw::{PlainLiquid, PlainLook};

fn water_scene(reg: &Registry) -> Scene {
    new_water_scene(reg, "")
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
        fed: false,
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
fn rain_drips_and_bursts_pour_at_their_moments() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    carve(&mut scene, vec![hole([0.0, 0.0], 300.0)]);
    add(&mut scene, &reg, LIQUID, Vec2::ZERO);
    let rain = add(&mut scene, &reg, POUR, Vec2::new(0.0, -200.0));
    set(&mut scene, rain, SlotId::Main, "kind", Value::Choice("rain".into()));
    set(&mut scene, rain, SlotId::Main, "width", Value::Number(400.0));
    set(&mut scene, rain, SlotId::Main, "drops_a_second", Value::Number(30.0));
    set(&mut scene, rain, SlotId::Main, "starts", Value::Number(1.0));
    let s = setup(&scene, &reg);
    let field = s.field();
    assert_eq!(s.note_for(rain), None, "rain pours");
    assert!(s.inputs_on(59, &field).timed.is_empty(), "nothing before its Starts");
    let frame = s.inputs_on(60, &field).timed;
    assert!(!frame.is_empty(), "its first lump comes at its Starts");
    assert!(frame.iter().all(|t| (1.0..1.0 + 1.0 / 60.0).contains(&t.at)));
    let layout = s.layout();
    // (Half the rain's width is over the rock: none is born there.)
    let (mut open, mut lumps) = (0, 0);
    for f in 60..180 {
        for t in s.inputs_on(f, &field).timed {
            let p = layout.to_world(Vec2::from(t.drop.x));
            assert!(field.at(p) > 0.0, "a drop born in the rock at {p}");
            open += 1;
        }
        lumps += 1;
    }
    assert!(open > 100 && lumps == 120, "{open} drops born in 2 s");
    assert!(s.drops_about(&field, 10.0) > 0, "the rain counts in the drops the final holds");
    assert!(s.inputs_at(60).pours.is_empty(), "rain is no tap");
    // A burst: once at its Starts, or again every so often.
    let burst = add(&mut scene, &reg, POUR, Vec2::new(0.0, 0.0));
    set(&mut scene, rain, SlotId::Main, "kind", Value::Choice("drip".into()));
    set(&mut scene, rain, SlotId::Main, "drops_a_second", Value::Number(0.0));
    set(&mut scene, burst, SlotId::Main, "kind", Value::Choice("burst".into()));
    set(&mut scene, burst, SlotId::Main, "starts", Value::Number(0.5));
    let born = |scene: &Scene| -> Vec<u32> {
        let s = setup(scene, &reg);
        let field = s.field();
        (0..180).filter(|f| !s.inputs_on(*f, &field).timed.is_empty()).collect()
    };
    assert_eq!(born(&scene), [30], "one burst at 0.5 s (and a drip at 0 a second drips nothing)");
    set(&mut scene, burst, SlotId::Main, "every", Value::Number(1.0));
    assert_eq!(born(&scene), [30, 90, 150]);
}

#[test]
fn drains_are_read_each_frame() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    let drain = add(&mut scene, &reg, DRAIN, Vec2::new(30.0, 40.0));
    let leak = add(&mut scene, &reg, DRAIN, Vec2::new(-30.0, 40.0));
    let edge = add(&mut scene, &reg, DRAIN, Vec2::ZERO);
    set(&mut scene, leak, SlotId::Main, "kind", Value::Choice("leak".into()));
    set(&mut scene, leak, SlotId::Main, "feeds_pours", Value::Bool(false));
    set(&mut scene, edge, SlotId::Main, "kind", Value::Choice("edge_of_the_course".into()));
    set(&mut scene, edge, SlotId::Main, "edge", Value::Choice("left".into()));
    let rate = Address { thing: drain, slot: Some(SlotId::Main), key: "rate".into() };
    scene.set_key(&reg, &rate, 0.0, Value::Number(1.0)).unwrap();
    scene.set_key(&reg, &rate, 1.0, Value::Number(2.0)).unwrap();
    let s = setup(&scene, &reg);
    let inputs = s.inputs_at(60);
    assert_eq!(
        inputs.drains,
        [
            Drain { place: Vec2::new(30.0, 40.0), reach: 40.0, pull: 2.0 * DRAIN_PULL, edges: 0, feeds: true },
            Drain { place: Vec2::new(-30.0, 40.0), reach: 40.0, pull: LEAK_PULL, edges: 0, feeds: false },
            Drain { place: Vec2::ZERO, reach: 0.0, pull: DRAIN_PULL, edges: Drain::LEFT, feeds: true },
        ]
    );
    assert_eq!(s.inputs_at(0).drains[0].pull, DRAIN_PULL, "its Rate as keyed then");
    // (Past the most drains, the rest take nothing and say so.)
    let mut many = scene.clone();
    let extra: Vec<ThingId> = (0..MOST_DRAINS).map(|_| add(&mut many, &reg, DRAIN, Vec2::ZERO)).collect();
    let s = setup(&many, &reg);
    assert_eq!(s.inputs_at(0).drains.len(), MOST_DRAINS);
    assert_eq!(s.note_for(*extra.last().unwrap()), Some(TOO_MANY_DRAINS));
    assert_eq!(s.note_for(extra[0]), None);
}

#[test]
fn a_fed_tap_says_when_no_drain_feeds_it() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    carve(&mut scene, vec![hole([0.0, 0.0], 300.0)]);
    add(&mut scene, &reg, LIQUID, Vec2::ZERO);
    let tap = add(&mut scene, &reg, POUR, Vec2::new(0.0, -100.0));
    set(&mut scene, tap, SlotId::Main, "fed_by_drains", Value::Bool(true));
    let s = setup(&scene, &reg);
    assert_eq!(s.note_for(tap), Some(NO_FEEDING_DRAIN));
    assert!(s.inputs_at(0).pours[0].fed, "it is fed");
    let drain = add(&mut scene, &reg, DRAIN, Vec2::new(0.0, 200.0));
    assert_eq!(setup(&scene, &reg).note_for(tap), None, "a drain feeds it");
    set(&mut scene, drain, SlotId::Main, "feeds_pours", Value::Bool(false));
    assert_eq!(setup(&scene, &reg).note_for(tap), Some(NO_FEEDING_DRAIN));
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
    assert_eq!(s.inputs_at(0).air_lift, 1.0);
    let rise = Address { thing: ThingId::ROOT, slot: Some(SlotId::Water), key: "bubble_rise".into() };
    scene.set_key(&reg, &rise, 0.0, Value::Number(0.0)).unwrap();
    scene.set_key(&reg, &rise, 1.0, Value::Number(2.0)).unwrap();
    let s = setup(&scene, &reg);
    assert!((s.inputs_at(30).air_lift - 1.0).abs() < 0.01, "{}", s.inputs_at(30).air_lift);
}

/// The Course's Rock gives the field its Wetting, and Trapped air turns the engine's air on and off.
#[test]
fn the_rock_wets_and_trapped_air_switches() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    let s = setup(&scene, &reg);
    assert_eq!(s.numbers().air, 1, "trapped air is on in a new scene");
    assert_eq!(s.field().wetting_at(Vec2::ZERO), bpe_water::DEFAULT_WETTING);
    let course = crate::things::water::course_of(&scene).unwrap();
    set(&mut scene, course, SlotId::Material, "wetting", Value::Number(0.9));
    set(&mut scene, ThingId::ROOT, SlotId::Water, "trapped_air", Value::Bool(false));
    let s = setup(&scene, &reg);
    assert!((s.field().wetting_at(Vec2::ZERO) - 0.9).abs() < 1e-6);
    assert_eq!(s.numbers().air, 0);
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
    // (A solid's Material is felt only by its Wetting; a Fixed one's Moves only by how it moves.)
    let physics = |_thing: &str, card: &str, key: &str| match card {
        cards::WORLD => {
            ["gravity", "screens_wide", "screens_tall", "draft_detail", "trapped_air", "bubble_rise"].contains(&key)
        }
        cards::MATERIAL => key == "wetting",
        cards::MOVES => key == "moves",
        cards::CARVE | cards::FEEL | cards::POUR | cards::DRAIN | cards::SHAPE | cards::LINE => true,
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
                if restarts != physics(&thing.kind, &card.kind, def.key) {
                    wrong.push(format!("{} {}: restarts {restarts}", card.kind, def.key));
                }
                checked += 1;
            }
        }
        // Moving it, turning it, and turning it off.
        if crate::things::water::is_placed(&thing.kind) {
            let mut moved = scene.clone();
            moved.things.get_mut(&id).unwrap().transform.position += Vec2::new(5.0, 0.0);
            let restarts = setup(&moved, &reg).print != base;
            if restarts != [POUR, PIECE, WALL, DRAIN].contains(&thing.kind.as_str()) {
                wrong.push(format!("moving a {}: restarts {restarts}", thing.kind));
            }
            let mut turned = scene.clone();
            turned.things.get_mut(&id).unwrap().transform.rotation += 0.3;
            let restarts = setup(&turned, &reg).print != base;
            if restarts != [PIECE, WALL].contains(&thing.kind.as_str()) {
                wrong.push(format!("turning a {}: restarts {restarts}", thing.kind));
            }
            checked += 2;
        }
        if [LIQUID, POUR, PIECE, WALL, DRAIN, FORCE, LAMP].contains(&thing.kind.as_str()) {
            let mut off = scene.clone();
            off.things.get_mut(&id).unwrap().enabled = false;
            let restarts = setup(&off, &reg).print != base;
            if restarts != [LIQUID, POUR, PIECE, WALL, DRAIN].contains(&thing.kind.as_str()) {
                wrong.push(format!("turning off a {}: restarts {restarts}", thing.kind));
            }
            checked += 1;
        }
    }
    // Keyframes count where the setting does.
    let pour = ids[KINDS.iter().position(|k| *k == POUR).unwrap() - 1];
    let piece = ids[KINDS.iter().position(|k| *k == PIECE).unwrap() - 1];
    for (thing, slot, key, v, restarts) in [
        (piece, SlotId::Material, "colour", Value::Color(Color::rgb(1, 2, 3)), false),
        (liquid, SlotId::Feel, "thickness", Value::Number(0.5), true),
        (liquid, SlotId::Look, "colour", Value::Color(Color::rgb(1, 2, 3)), false),
        (ThingId::ROOT, SlotId::Water, "gravity", Value::Vec2([0.0, 10.0]), true),
        (ThingId::ROOT, SlotId::Water, "bubble_rise", Value::Number(2.0), true),
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

/// A square Piece of `side` px (a Pillar sized so) at `at`, as wet as `wetting`.
fn block(scene: &mut Scene, reg: &Registry, at: Vec2, side: f32, wetting: f64) -> ThingId {
    let id = add(scene, reg, PIECE, at);
    set(scene, id, SlotId::Form, "piece", Value::Choice("pillar".into()));
    set(scene, id, SlotId::Form, "size", Value::Vec2([side, side]));
    set(scene, id, SlotId::Material, "wetting", Value::Number(wetting));
    id
}

#[test]
fn a_piece_adds_solid_and_its_own_wetting() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    carve(&mut scene, vec![hole([0.0, 0.0], 400.0)]);
    let id = block(&mut scene, &reg, Vec2::new(0.0, 100.0), 120.0, 0.0);
    let s = setup(&scene, &reg);
    assert_eq!(s.solids.iter().map(|s| s.id).collect::<Vec<_>>(), [id]);
    let field = s.field();
    assert!(field.at(Vec2::new(0.0, 100.0)) < -50.0, "solid in its middle");
    assert!(
        (field.at(Vec2::new(0.0, 20.0)) - 20.0).abs() < 1.0,
        "20 px over its top: {}",
        field.at(Vec2::new(0.0, 20.0))
    );
    assert_eq!(field.wetting_at(Vec2::new(0.0, 36.0)), 0.0, "its face is waxy");
    assert_eq!(field.wetting_at(Vec2::new(0.0, -390.0)), 1.0, "the rock's is soaked");
    // Turned a quarter, a tall piece lies along.
    set(&mut scene, id, SlotId::Form, "size", Value::Vec2([40.0, 300.0]));
    scene.things.get_mut(&id).unwrap().transform.rotation = std::f32::consts::FRAC_PI_2;
    let field = setup(&scene, &reg).field();
    assert!(field.at(Vec2::new(120.0, 100.0)) < 0.0 && field.at(Vec2::new(0.0, 160.0)) > 0.0);
}

#[test]
fn a_cutting_pipe_opens_a_tunnel_lined_with_its_material() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    let pipe = add(&mut scene, &reg, PIECE, Vec2::new(0.0, 0.0));
    set(&mut scene, pipe, SlotId::Form, "piece", Value::Choice("pipe".into()));
    set(&mut scene, pipe, SlotId::Form, "size", Value::Vec2([400.0, 80.0]));
    set(&mut scene, pipe, SlotId::Form, "cuts", Value::Bool(true));
    set(&mut scene, pipe, SlotId::Material, "wetting", Value::Number(0.25));
    let s = setup(&scene, &reg);
    let field = s.field();
    assert!(field.at(Vec2::ZERO) > 10.0, "open down its bore: {}", field.at(Vec2::ZERO));
    assert!(field.at(Vec2::new(0.0, 36.0)) < 0.0, "its walls stand in the hole");
    assert!(field.at(Vec2::new(0.0, 200.0)) < 0.0, "the rock round it stays");
    assert!(field.at(Vec2::new(300.0, 0.0)) < 0.0, "past its ends too");
    assert!((field.wetting_at(Vec2::ZERO) - 0.25).abs() < 1e-6, "lined with its own Material");
    // A wall that cuts is a channel.
    let wall = add(&mut scene, &reg, WALL, Vec2::new(0.0, 300.0));
    set(&mut scene, wall, SlotId::Form, "cuts", Value::Bool(true));
    let field = setup(&scene, &reg).field();
    assert!(field.at(Vec2::new(0.0, 300.0)) > 0.0 && field.at(Vec2::new(0.0, 360.0)) < 0.0);
}

#[test]
fn order_decides_what_a_cut_takes() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    carve(&mut scene, vec![hole([0.0, 0.0], 400.0)]);
    let plug = block(&mut scene, &reg, Vec2::ZERO, 200.0, 1.0);
    let cutter = block(&mut scene, &reg, Vec2::ZERO, 80.0, 1.0);
    set(&mut scene, cutter, SlotId::Form, "cuts", Value::Bool(true));
    // The cut comes after the plug: a hole in it.
    let s = setup(&scene, &reg);
    assert!(s.field().at(Vec2::ZERO) > 0.0);
    // Moved above the plug in the tree, it cuts only the open water, and the plug fills it back.
    let at = |scene: &Scene, id| scene.order.iter().position(|i| *i == id).unwrap();
    let (a, b) = (at(&scene, plug), at(&scene, cutter));
    scene.order.swap(a, b);
    let s2 = setup(&scene, &reg);
    assert!(s2.field().at(Vec2::ZERO) < 0.0);
    assert_ne!(s.print, s2.print, "the order restarts the draft");
}

#[test]
fn a_tap_inside_a_piece_pours_nothing() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    carve(&mut scene, vec![hole([0.0, 0.0], 400.0)]);
    add(&mut scene, &reg, LIQUID, Vec2::ZERO);
    let tap = add(&mut scene, &reg, POUR, Vec2::new(0.0, -200.0));
    let s = setup(&scene, &reg);
    assert!(s.taps_in_rock(&s.field()).is_empty());
    block(&mut scene, &reg, Vec2::new(0.0, -200.0), 100.0, 1.0);
    let s = setup(&scene, &reg);
    let field = s.field();
    assert_eq!(s.taps_in_rock(&field), [tap]);
    assert_eq!(s.inputs_on(0, &field).pours[0].width, 0.0);
}

#[test]
fn a_huge_piece_is_kept_to_the_layout() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    carve(&mut scene, vec![hole([0.0, 0.0], 400.0)]);
    let id = block(&mut scene, &reg, Vec2::ZERO, 1e6, 1.0);
    scene.things.get_mut(&id).unwrap().transform.rotation = 0.4;
    let far = block(&mut scene, &reg, Vec2::new(1e7, -1e7), 50.0, 1.0);
    let started = std::time::Instant::now();
    let s = setup(&scene, &reg);
    let field = s.field();
    assert!(started.elapsed().as_secs_f32() < 2.0, "{:?}", started.elapsed());
    assert!(field.distance.iter().all(|d| *d < 0.0), "solid all through");
    assert!(s.solid(far).is_some());
}

#[test]
fn thin_and_moving_pieces_say_so() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    let wall = add(&mut scene, &reg, WALL, Vec2::ZERO);
    let thick = add(&mut scene, &reg, WALL, Vec2::ZERO);
    set(&mut scene, wall, SlotId::Form, "thickness", Value::Number(14.0));
    let wheel = add(&mut scene, &reg, PIECE, Vec2::ZERO);
    set(&mut scene, wheel, SlotId::Moves, "moves", Value::Choice("driven".into()));
    let s = setup(&scene, &reg);
    assert_eq!(s.note_for(wall), Some(THIN), "14 px is under two 8 px cells");
    assert_eq!(s.note_for(thick), None, "24 px is not");
    assert_eq!(s.note_for(wheel), Some(DRIVEN_LATER));
    assert!(s.movers.is_empty(), "a Driven part stays where it is for now");
    // A 24 px wall keyed to half its size is 12 px at its smallest.
    set(&mut scene, thick, SlotId::Moves, "moves", Value::Choice("keyframed".into()));
    let scale = Address { thing: thick, slot: Some(SlotId::Moves), key: "scale".into() };
    scene.set_key(&reg, &scale, 1.0, Value::Number(0.5)).unwrap();
    assert_eq!(setup(&scene, &reg).note_for(thick), Some(THIN));
    // At a finer draft (6 px), 14 px is thick enough.
    set(&mut scene, ThingId::ROOT, SlotId::Water, "draft_detail", Value::Choice("fine".into()));
    assert_eq!(setup(&scene, &reg).note_for(wall), None);
}

#[test]
fn a_piece_is_picked_by_its_body() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    let under = block(&mut scene, &reg, Vec2::ZERO, 200.0, 1.0);
    let over = block(&mut scene, &reg, Vec2::new(80.0, 0.0), 100.0, 1.0);
    let wall = add(&mut scene, &reg, WALL, Vec2::new(0.0, 300.0));
    let s = setup(&scene, &reg);
    assert_eq!(s.solid_at(Vec2::new(-80.0, 0.0), 0.0), Some(under));
    assert_eq!(s.solid_at(Vec2::new(90.0, 0.0), 0.0), Some(over), "the later one is on top");
    assert_eq!(s.solid_at(Vec2::new(140.0, 300.0), 0.0), Some(wall), "a wall by its line");
    assert_eq!(s.solid_at(Vec2::new(0.0, 200.0), 0.0), None);
    assert_eq!(s.solid_at(Vec2::new(0.0, 103.0), 5.0), Some(under), "within the reach");
}

#[test]
fn snap_points_leave_the_piece_itself_out() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    let a = block(&mut scene, &reg, Vec2::ZERO, 100.0, 1.0);
    let wall = add(&mut scene, &reg, WALL, Vec2::new(0.0, 300.0));
    let s = setup(&scene, &reg);
    let all = s.snap_points(None);
    assert!(all.iter().any(|p| p.distance(Vec2::new(50.0, 50.0)) < 1e-3), "{all:?}");
    assert!(all.iter().any(|p| p.distance(Vec2::new(150.0, 300.0)) < 1e-3), "the wall's end");
    let without = s.snap_points(Some(a));
    assert!(!without.iter().any(|p| p.distance(Vec2::new(50.0, 50.0)) < 1e-3));
    assert_eq!(s.snap_points(Some(wall)).len(), 4);
}

#[test]
fn the_plain_look_colours_each_solid() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    let a = block(&mut scene, &reg, Vec2::ZERO, 100.0, 1.0);
    let b = add(&mut scene, &reg, WALL, Vec2::ZERO);
    set(&mut scene, a, SlotId::Material, "colour", Value::Color(Color::rgb(255, 0, 0)));
    set(&mut scene, b, SlotId::Material, "colour", Value::Color(Color::rgb(0, 0, 255)));
    let look = plain_look(&scene, &bpe_core::resolve(&scene, &reg), 0.0);
    assert_eq!(look.solids, [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]);
    // The field marks each one's solid by its place in that list.
    let field = setup(&scene, &reg).field();
    let node = |p: Vec2| {
        let g = field.layout.to_grid(p).round();
        field.owner[g.y as usize * field.layout.cols as usize + g.x as usize]
    };
    assert_eq!(node(Vec2::new(0.0, 30.0)), 1);
    assert_eq!(node(Vec2::new(-150.0, 0.0)), 2, "the wall, laid after");
    // Off, it is in neither.
    scene.things.get_mut(&a).unwrap().enabled = false;
    let look = plain_look(&scene, &bpe_core::resolve(&scene, &reg), 0.0);
    assert_eq!(look.solids, [[0.0, 0.0, 1.0]]);
}

/// One screen at Fine (6 px), carved, with 40 pieces of every kind: the field builds quickly (by
/// hand in release it prints its time; 150 ms is the aim).
#[test]
fn a_field_with_forty_pieces_builds_quickly() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    set(&mut scene, ThingId::ROOT, SlotId::Water, "draft_detail", Value::Choice("fine".into()));
    carve(&mut scene, vec![hole([0.0, 0.0], 500.0)]);
    let kinds =
        ["ramp", "step", "pillar", "basin", "funnel", "pipe", "sieve", "bucket", "wheel", "gate", "float", "custom"];
    for k in 0..40 {
        let at = Vec2::new(-400.0 + (k % 5) as f32 * 200.0, -800.0 + (k / 5) as f32 * 220.0);
        let id = add(&mut scene, &reg, PIECE, at);
        set(&mut scene, id, SlotId::Form, "piece", Value::Choice(kinds[k % kinds.len()].into()));
        scene.things.get_mut(&id).unwrap().transform.rotation = k as f32 * 0.1;
    }
    let s = setup(&scene, &reg);
    let started = std::time::Instant::now();
    let mask = s.mask();
    let masked = started.elapsed();
    let started = std::time::Instant::now();
    let field = s.field_with(&mask);
    let took = started.elapsed();
    println!("40 pieces over a carved screen at 6 px: the carving {masked:?}, the pieces over it {took:?}");
    assert!(took.as_secs_f32() < 2.0, "{took:?}");
    assert_eq!(field.layout.cell, 6.0);
}

/// The final's setup is at Final detail and its print follows Final detail, the draft's Draft
/// detail; Speed is neither's physics (part 8).
#[test]
fn the_final_is_at_final_detail_and_speed_is_no_physics() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    let resolved = bpe_core::resolve(&scene, &reg);
    let (draft, fin) = (WaterSetup::of(&scene, &resolved), WaterSetup::final_of(&scene, &resolved));
    assert_eq!((draft.cell, fin.cell), (8.0, 2.0));
    assert_eq!(fin.detail, Detail::Final);
    let prints = |scene: &Scene| {
        let r = bpe_core::resolve(scene, &reg);
        (WaterSetup::of(scene, &r).print, WaterSetup::final_of(scene, &r).print)
    };
    let base = prints(&scene);
    let mut edited = scene.clone();
    set(&mut edited, ThingId::ROOT, SlotId::Water, "final_detail", Value::Choice("fine".into()));
    let p = prints(&edited);
    assert!(p.0 == base.0 && p.1 != base.1, "Final detail is the final's alone");
    let mut edited = scene.clone();
    set(&mut edited, ThingId::ROOT, SlotId::Water, "draft_detail", Value::Choice("fine".into()));
    let p = prints(&edited);
    assert!(p.0 != base.0 && p.1 == base.1, "Draft detail is the draft's alone");
    set(&mut scene, ThingId::ROOT, SlotId::Water, "speed", Value::Number(0.25));
    let speed = Address { thing: ThingId::ROOT, slot: Some(SlotId::Water), key: "speed".into() };
    scene.set_key(&reg, &speed, 1.0, Value::Number(0.5)).unwrap();
    assert_eq!(prints(&scene), base, "Speed changes no water");
    assert_eq!(fin.clone().with_cell(8.0).cell, 8.0);
    assert_ne!(fin.clone().with_cell(8.0).print, fin.print);
}

/// The earliest time two scenes' water can differ at.
#[test]
fn the_earliest_change_is_where_the_water_can_first_differ() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    carve(&mut scene, vec![hole([0.0, 0.0], 300.0)]);
    let liquid = add(&mut scene, &reg, LIQUID, Vec2::ZERO);
    let tap = add(&mut scene, &reg, POUR, Vec2::new(0.0, -100.0));
    set(&mut scene, tap, SlotId::Main, "starts", Value::Number(5.0));
    let gravity = Address { thing: ThingId::ROOT, slot: Some(SlotId::Water), key: "gravity".into() };
    scene.set_key(&reg, &gravity, 1.0, Value::Vec2([0.0, 4000.0])).unwrap();
    let inputs = |scene: &Scene| WaterSetup::final_of(scene, &bpe_core::resolve(scene, &reg)).inputs();
    let base = inputs(&scene);
    let change = |edit: &dyn Fn(&mut Scene)| {
        let mut s = scene.clone();
        edit(&mut s);
        let got = base.earliest_change(&inputs(&s));
        assert_eq!(got, inputs(&s).earliest_change(&base), "both ways round");
        got
    };
    assert_eq!(base.earliest_change(&inputs(&scene)), None, "the same scene makes the same water");
    assert_eq!(change(&|s| set(s, liquid, SlotId::Look, "colour", Value::Color(Color::rgb(1, 2, 3)))), None);
    assert_eq!(change(&|s| set(s, liquid, SlotId::Feel, "thickness", Value::Number(0.7))), Some(0.0));
    assert_eq!(change(&|s| carve(s, vec![hole([0.0, 0.0], 290.0)])), Some(0.0));
    let rate = change(&|s| set(s, tap, SlotId::Main, "rate", Value::Number(2.0))).unwrap();
    assert!((rate - (5.0 - 3.0 / 60.0)).abs() < 1e-9, "a tap's change shows from just before it starts: {rate}");
    let later = change(&|s| s.set_key(&reg, &gravity, 3.0, Value::Vec2([0.0, 1000.0])).map(|_| ()).unwrap()).unwrap();
    assert_eq!(later, 1.0, "a key added after one at 1 s changes the water from 1 s");
    let first = change(&|s| s.set_key(&reg, &gravity, 0.5, Value::Vec2([0.0, 1000.0])).map(|_| ()).unwrap()).unwrap();
    assert_eq!(first, 0.0, "a key before every other changes it from the start");
    let speed = Address { thing: ThingId::ROOT, slot: Some(SlotId::Water), key: "speed".into() };
    assert_eq!(
        change(&|s| s.set_key(&reg, &speed, 2.0, Value::Number(0.25)).map(|_| ()).unwrap()),
        None,
        "Speed is no physics"
    );
    // A tap put in front of it: the taps are told apart by their place, so from the earlier start.
    let ahead = change(&|s| {
        let t = add(s, &reg, POUR, Vec2::new(50.0, -100.0));
        set(s, t, SlotId::Main, "starts", Value::Number(2.0));
        let at = s.order.iter().position(|id| *id == tap).unwrap();
        s.order.retain(|id| *id != t);
        s.order.insert(at, t);
    });
    assert!((ahead.unwrap() - (2.0 - 3.0 / 60.0)).abs() < 1e-9, "{ahead:?}");
    // Rain from 4 s changes the water from just before then; a drain from the start.
    let rain = change(&|s| {
        let r = add(s, &reg, POUR, Vec2::new(0.0, -200.0));
        set(s, r, SlotId::Main, "kind", Value::Choice("rain".into()));
        set(s, r, SlotId::Main, "starts", Value::Number(4.0));
    });
    assert!((rain.unwrap() - (4.0 - 3.0 / 60.0)).abs() < 1e-9, "{rain:?}");
    assert_eq!(change(&|s| _ = add(s, &reg, DRAIN, Vec2::new(0.0, 200.0))), Some(0.0));
}

/// Slow motion: the video plays a water second at Speed s for 1/s seconds, and keeps more
/// moments a frame where it is slow.
#[test]
fn slow_motion_lengthens_the_video_and_keeps_more_moments() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    let speeds = |scene: &Scene| Speeds::of(scene, &bpe_core::resolve(scene, &reg), 10.0);
    let real = speeds(&scene);
    assert!(!real.slowed());
    assert!((real.video_length(10.0) - 10.0).abs() < 1e-9);
    assert_eq!(real.keep_for(100), 1);
    set(&mut scene, ThingId::ROOT, SlotId::Water, "speed", Value::Number(0.25));
    let slow = speeds(&scene);
    assert!(slow.slowed());
    assert!((slow.video_length(10.0) - 40.0).abs() < 1e-6, "{}", slow.video_length(10.0));
    assert_eq!(slow.keep_for(0), 4);
    // A ramp from real time at 0 s to an eighth at 1 s, and back to real time at 2 s.
    let speed = Address { thing: ThingId::ROOT, slot: Some(SlotId::Water), key: "speed".into() };
    let mut ramp = water_scene(&reg);
    for (t, v) in [(0.0, 1.0), (1.0, 0.125), (2.0, 1.0)] {
        ramp.set_key(&reg, &speed, t, Value::Number(v)).unwrap();
    }
    let r = speeds(&ramp);
    assert!(r.slowed());
    let keeps: Vec<u32> = [2, 30, 45, 58, 61, 118].iter().map(|f| r.keep_for(*f)).collect();
    assert_eq!(keeps[0], 1, "real time at the start: {keeps:?}");
    assert!(keeps.windows(2).take(3).all(|w| w[0] <= w[1]), "more and more toward the slowest: {keeps:?}");
    assert_eq!(keeps[4], 8, "eight at the slowest: {keeps:?}");
    let v = r.video_length(10.0);
    assert!(v > 11.0 && v < 18.0, "the ramp lengthens the video: {v}");
    for t in [0.0, 0.3, 1.0, 1.7, 2.5, 9.0] {
        let back = r.water_time(r.video_time(t));
        assert!((back - t).abs() < 1e-6, "water time {t} came back as {back}");
    }
}

#[test]
fn the_finish_reads_the_scenes_effects_over_the_picture_and_its_grade() {
    use bpe_core::layered_key;
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    scene.add_slot(&reg, ThingId::ROOT, SlotId::Shader, crate::modules::effects::EFFECTS).unwrap();
    let k = scene.add_layer(&reg, ThingId::ROOT, SlotId::Shader, "picture.kaleidoscope").unwrap();
    // (A material, pasted in: it paints things, which a Water scene has none of.)
    scene.add_layer(&reg, ThingId::ROOT, SlotId::Shader, "picture.lava").unwrap();
    let mix = Address { thing: ThingId::ROOT, slot: Some(SlotId::Shader), key: layered_key(k, "mix") };
    scene.set_key(&reg, &mix, 0.0, Value::Number(0.0)).unwrap();
    scene.set_key(&reg, &mix, 2.0, Value::Number(1.0)).unwrap();
    let grade = crate::modules::grade::starting_card(&reg);
    scene.things.get_mut(&ThingId::ROOT).unwrap().slots.insert(SlotId::Grade, grade);
    let resolved = bpe_core::resolve(&scene, &reg);
    let got = crate::things::water::look::look_of(&scene, &resolved, 1.0).finish;
    assert_eq!(got.effects.len(), 1, "only the effect over the picture");
    let e = &got.effects[0];
    assert!(e.wgsl.contains("name: Kaleidoscope") && e.stage == bpe_gfx::Stage::Picture);
    assert!((e.blend - 0.5).abs() < 1e-6, "the Mix at 1 s, half way between its keys: {}", e.blend);
    assert_eq!(
        got.grade,
        crate::modules::grade::stack(resolved.things[&ThingId::ROOT].slots.get(&SlotId::Grade), None)
    );
    assert!(!got.grade.masks.is_empty(), "the grade's Whole picture");
    // The stage's whole course goes without them.
    let course = got.without_shot_finish();
    assert!(course.effects.is_empty() && course.grade.masks.is_empty());
    assert_eq!((course.bloom, course.exposure), (got.bloom, got.exposure));
}

#[test]
fn the_look_reads_every_card() {
    use bpe_water::draw::look::{self, BackLook, Lamp, LightLook, LiquidLook, MaterialLook, Quality};
    let reg = crate::registry();
    let look_at =
        |scene: &Scene, t: f64| crate::things::water::look::look_of(scene, &bpe_core::resolve(scene, &reg), t);
    // The defaults are the look drawing's own.
    let mut scene = water_scene(&reg);
    let got = look_at(&scene, 0.0);
    assert!(got.liquids.is_empty() && got.solids.is_empty() && got.lamps.is_empty());
    assert_eq!((got.rock, got.back), (MaterialLook::default(), BackLook::default()));
    assert_eq!((got.light.ambient, got.light), (1.0, LightLook::default()));
    // (The finish measures in the video's pixels: the scene's size; its masks name the course.)
    let course = course_of(&scene).map(|id| bpe_gfx::thing_code(id) as u32);
    let things = look::ThingCodes { rock: course, ..look::ThingCodes::default() };
    assert_eq!(got.finish, look::FinishLook { video: (1080, 1920), things, ..look::FinishLook::default() });
    let a = add(&mut scene, &reg, LIQUID, Vec2::ZERO);
    let lamp = add(&mut scene, &reg, LAMP, Vec2::new(-200.0, -300.0));
    let got = look_at(&scene, 0.0);
    assert_eq!(got.liquids, [LiquidLook::default()]);
    assert_eq!(got.lamps, [Lamp { place: Vec2::new(-200.0, -300.0), ..Lamp::default() }]);

    // Every card, and keyframes read at the time asked for.
    let b = add(&mut scene, &reg, LIQUID, Vec2::ZERO);
    set(&mut scene, a, SlotId::Look, "shine", Value::Number(0.9));
    set(&mut scene, b, SlotId::Look, "colour_from", Value::Choice("speed".into()));
    set(&mut scene, b, SlotId::Look, "bands", Value::Int(3));
    let piece = block(&mut scene, &reg, Vec2::ZERO, 100.0, 1.0);
    let wall = add(&mut scene, &reg, WALL, Vec2::ZERO);
    set(&mut scene, piece, SlotId::Material, "material", Value::Choice("glass".into()));
    set(&mut scene, wall, SlotId::Material, "outline", Value::Number(3.0));
    set(&mut scene, lamp, SlotId::Main, "kind", Value::Choice("strip".into()));
    set(&mut scene, ThingId::ROOT, SlotId::WaterLight, "quality", Value::Choice("best".into()));
    set(&mut scene, ThingId::ROOT, SlotId::Back, "shows", Value::Choice("gradient".into()));
    let key = |scene: &mut Scene, a: Address, t: f64, v: f64| scene.set_key(&reg, &a, t, Value::Number(v)).unwrap();
    key(&mut scene, Address::on(lamp, SlotId::Main, "brightness"), 0.0, 1.0);
    key(&mut scene, Address::on(lamp, SlotId::Main, "brightness"), 2.0, 3.0);
    key(&mut scene, Address::on(ThingId::ROOT, SlotId::WaterLight, "ambient"), 2.0, 0.25);
    key(&mut scene, Address::own(ThingId::ROOT, "bloom"), 0.0, 0.0);
    key(&mut scene, Address::own(ThingId::ROOT, "bloom"), 2.0, 0.5);
    let (start, later) = (look_at(&scene, 0.0), look_at(&scene, 2.0));
    assert_eq!(start.liquids.iter().map(|l| l.shine).collect::<Vec<_>>(), [0.9, 0.5]);
    assert_eq!((start.liquids[1].colour_from, start.liquids[1].bands), (look::ColourFrom::Speed, 3));
    // A gradient is sent only when the colour is read from it, as linear samples.
    assert!(start.liquids[0].gradient.is_empty());
    let g = &start.liquids[1].gradient;
    assert_eq!(g.len(), look::GRADIENT_SAMPLES);
    let blue = Color::rgb(0x2f, 0x9b, 0xff).to_linear();
    assert!((0..3).all(|k| (g[0][k] - blue[k]).abs() < 1e-4 && (g[g.len() - 1][k] - 1.0).abs() < 1e-4), "{g:?}");
    // The solids in the order the field numbers them.
    assert_eq!(
        start.solids.iter().map(|s| (s.kind, s.outline)).collect::<Vec<_>>(),
        [(look::MaterialKind::Glass, 0.0), (look::MaterialKind::Rock, 3.0)]
    );
    assert!(matches!(&start.back, BackLook::Gradient { samples, angle } if samples.len() == 64 && *angle == 90.0));
    assert_eq!((start.light.quality, start.lamps[0].kind), (Quality::Best, look::LampKind::Strip));
    assert_eq!((start.lamps[0].brightness, later.lamps[0].brightness), (1.0, 3.0));
    assert_eq!((start.light.ambient, later.light.ambient), (0.25, 0.25), "one key holds all along");
    assert_eq!((start.finish.bloom, later.finish.bloom, later.time), (0.0, 0.5, 2.0));
    // Off, a thing is left out.
    scene.things.get_mut(&piece).unwrap().enabled = false;
    scene.things.get_mut(&a).unwrap().enabled = false;
    let got = look_at(&scene, 0.0);
    assert_eq!((got.solids.len(), got.solids[0].outline, got.liquids.len()), (1, 3.0, 1));
    // A picture behind is its path (the drawing reads it), or the picture the scene keeps under its name.
    set(&mut scene, ThingId::ROOT, SlotId::Back, "shows", Value::Choice("picture".into()));
    set(&mut scene, ThingId::ROOT, SlotId::Back, "picture", Value::Text("C:/pictures/sky.png".into()));
    assert_eq!(look_at(&scene, 0.0).back, BackLook::Picture(look::BackPicture::file("C:/pictures/sky.png")));
    let bytes: std::sync::Arc<[u8]> = vec![1, 2, 3].into();
    let name = scene.add_picture("Sky", bytes.clone());
    set(&mut scene, ThingId::ROOT, SlotId::Back, "picture", Value::Text(name.clone()));
    assert_eq!(look_at(&scene, 0.0).back, BackLook::Picture(look::BackPicture { name, kept: Some(bytes) }));
}

#[test]
fn a_water_scenes_things_parts_name_what_the_water_draws() {
    use crate::modules::grade::{self, Combine, THINGS, ThingsOnly, ThingsPart};
    use bpe_core::{Target, layered_key};
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    let (a, b) = (add(&mut scene, &reg, LIQUID, Vec2::ZERO), add(&mut scene, &reg, LIQUID, Vec2::ZERO));
    let piece = block(&mut scene, &reg, Vec2::new(0.0, 300.0), 80.0, 1.0);
    let wall = add(&mut scene, &reg, WALL, Vec2::new(0.0, -300.0));
    let course = course_of(&scene).unwrap();
    let code = |id: ThingId| bpe_gfx::thing_code(id) as u32;
    // The drawing names its liquids by kind and its solids by owner, in the draft's order.
    let resolved = bpe_core::resolve(&scene, &reg);
    let things = crate::things::water::look::look_of(&scene, &resolved, 0.0).finish.things;
    assert_eq!(things.liquids, [code(a), code(b)]);
    assert_eq!((things.rock, &things.solids), (Some(code(course)), &vec![code(piece), code(wall)]));
    let ids = setup(&scene, &reg).solids.iter().map(|s| code(s.id)).collect::<Vec<_>>();
    assert_eq!(ids, things.solids, "owner k is the draft's solid k");
    // What each Only covers.
    let part = |only, target| ThingsPart { only, target, ..ThingsPart::default() }.water_things(&scene);
    let covers = |only| part(only, Target::Anything);
    let sorted = |mut v: Vec<u32>| {
        v.sort_unstable();
        Some((Some(v), false))
    };
    assert_eq!(covers(ThingsOnly::Liquids), sorted(vec![code(a), code(b)]));
    assert_eq!(covers(ThingsOnly::Solids), sorted(vec![code(course), code(piece), code(wall)]));
    assert_eq!(covers(ThingsOnly::Back), Some((None, true)));
    assert_eq!(covers(ThingsOnly::Extras), sorted(vec![bpe_water::draw::look::EXTRAS]));
    assert_eq!(covers(ThingsOnly::Balls), sorted(vec![]), "no balls in a Water scene");
    assert_eq!(part(ThingsOnly::OneThing, Target::Thing(piece)), sorted(vec![code(piece)]));
    assert_eq!(part(ThingsOnly::OneThing, Target::Kind(LIQUID.into())), sorted(vec![code(a), code(b)]));
    assert_eq!(part(ThingsOnly::OneThing, Target::Thing(ThingId(999))), None, "left out");
    // Its row's note looks in the scene, not in the blank Balls world a Water scene plays beside.
    let kit = crate::kit();
    let none = crate::new_studio_scene(&kit.registry);
    let blank = bpe_runtime::World::build(&none, &bpe_core::resolve(&none, &kit.registry), &kit.sims);
    let mut p = bpe_core::Params::default();
    p.set("only", Value::Choice("one_thing".into()));
    p.set("target", Value::Target(Target::Thing(piece)));
    assert_eq!(grade::masks::follow_note(&blank, THINGS, &p, Some(&scene)), None);
    assert_eq!(grade::masks::follow_note(&blank, THINGS, &p, None).as_deref(), Some("Thing not found"));
    p.set("target", Value::Target(Target::Thing(ThingId(999))));
    assert_eq!(grade::masks::follow_note(&blank, THINGS, &p, Some(&scene)).as_deref(), Some("Thing not found"));
    // A new Things part starts on the Liquids, and reaches the finish's grade.
    let mask = grade::add_mask(&mut scene, &reg, THINGS).unwrap();
    let card = |scene: &Scene| scene.card(ThingId::ROOT, SlotId::Grade).unwrap().clone();
    let first = grade::parts_of(&card(&scene), mask)[0].id;
    let second = grade::add_part(&mut scene, &reg, mask, THINGS, Combine::Add).unwrap();
    for part in [first, second] {
        assert_eq!(card(&scene).params.get(&layered_key(part, "only")), Some(&Value::Choice("liquids".into())));
    }
    let resolved = bpe_core::resolve(&scene, &reg);
    let finish = crate::things::water::look::look_of(&scene, &resolved, 0.0).finish;
    let area = finish.grade.masks[1].area.as_ref().unwrap();
    match &area.parts[0].shape {
        bpe_gfx::GradeShape::Things(t) => assert_eq!(t.things.as_deref(), Some(&[code(a), code(b)][..])),
        other => panic!("{other:?}"),
    }
}

/// Makes Piece or Wall `id` Keyframed, with `keys` (setting, time, value) on its Moves card.
fn keyframed(scene: &mut Scene, reg: &Registry, id: ThingId, keys: &[(&str, f64, Value)]) {
    set(scene, id, SlotId::Moves, "moves", Value::Choice("keyframed".into()));
    for (key, time, v) in keys {
        let a = Address { thing: id, slot: Some(SlotId::Moves), key: (*key).into() };
        scene.set_key(reg, &a, *time, v.clone()).unwrap();
    }
}

#[test]
fn a_keyframed_piece_follows_its_keys() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    carve(&mut scene, vec![hole([0.0, 0.0], 400.0)]);
    let id = block(&mut scene, &reg, Vec2::new(10.0, 20.0), 80.0, 1.0);
    scene.things.get_mut(&id).unwrap().transform.rotation = 0.25;
    let keys = [
        ("offset", 0.0, Value::Vec2([0.0, 0.0])),
        ("offset", 1.0, Value::Vec2([100.0, 0.0])),
        ("turn", 0.0, Value::Number(0.0)),
        ("turn", 1.0, Value::Number(90.0)),
    ];
    keyframed(&mut scene, &reg, id, &keys);
    let s = setup(&scene, &reg);
    assert_eq!(s.movers, [0]);
    assert_eq!(s.poses_at(0.0), [bpe_water::Pose::new(Vec2::new(10.0, 20.0), 0.25, 1.0)]);
    let half = s.poses_at(0.5)[0];
    assert!((half.at - Vec2::new(60.0, 20.0)).length() < 1e-3, "{half:?}");
    assert!((half.turn - (0.25 + std::f32::consts::FRAC_PI_4)).abs() < 1e-3, "{half:?}");
    assert_eq!(half.scale, 1.0);
}

#[test]
fn a_moving_piece_is_not_in_the_still_field() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    carve(&mut scene, vec![hole([0.0, 0.0], 400.0)]);
    block(&mut scene, &reg, Vec2::new(-200.0, 100.0), 80.0, 1.0);
    let id = block(&mut scene, &reg, Vec2::new(0.0, 100.0), 80.0, 0.25);
    let keys = [("offset", 0.0, Value::Vec2([0.0, 0.0])), ("offset", 1.0, Value::Vec2([0.0, -50.0]))];
    keyframed(&mut scene, &reg, id, &keys);
    let s = setup(&scene, &reg);
    let open = s.still_with(&s.mask());
    assert!(open.at(Vec2::new(0.0, 100.0)) > 20.0, "open where it moves from");
    assert!(open.at(Vec2::new(-200.0, 100.0)) < -20.0, "the still piece stays");
    let field = s.field();
    assert!(field.at(Vec2::new(0.0, 100.0)) < -20.0, "it starts where it is");
    assert!((field.wetting_at(Vec2::new(0.0, 56.0)) - 0.25).abs() < 1e-6, "as wet as its Material");
    let movers = s.movers().unwrap();
    assert_eq!(movers.movers.iter().map(|m| m.owner).collect::<Vec<_>>(), [2], "marked as the course's second part");
    let later = movers.field_at(&s.poses_at(1.0));
    assert!(later.at(Vec2::new(0.0, 50.0)) < -20.0 && later.at(Vec2::new(0.0, 125.0)) > 0.0, "and moves");
}

#[test]
fn the_moves_card_restarts_the_draft() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    carve(&mut scene, vec![hole([0.0, 0.0], 400.0)]);
    let id = block(&mut scene, &reg, Vec2::ZERO, 80.0, 1.0);
    let print = |scene: &Scene| setup(scene, &reg).print;
    let fixed = print(&scene);
    let edited = |scene: &Scene, key: &str, v: Value| {
        let mut s = scene.clone();
        set(&mut s, id, SlotId::Moves, key, v);
        print(&s)
    };
    assert_eq!(edited(&scene, "offset", Value::Vec2([5.0, 0.0])), fixed, "a Fixed part's Position is no physics");
    let mut moving = scene.clone();
    keyframed(&mut moving, &reg, id, &[("offset", 1.0, Value::Vec2([50.0, 0.0]))]);
    let keyed = print(&moving);
    assert_ne!(keyed, fixed, "Fixed to Keyframed");
    for (key, v) in [("offset", Value::Vec2([5.0, 0.0])), ("turn", Value::Number(20.0)), ("scale", Value::Number(2.0))]
    {
        assert_ne!(edited(&moving, key, v), keyed, "{key} moves a Keyframed part");
    }
    for (key, v) in
        [("weight", Value::Number(3.0)), ("joint", Value::Choice("pin".into())), ("motor", Value::Number(1.0))]
    {
        assert_eq!(edited(&moving, key, v), keyed, "{key} is a Driven part's");
    }
    let mut moved = moving.clone();
    keyframed(&mut moved, &reg, id, &[("offset", 1.0, Value::Vec2([60.0, 0.0]))]);
    assert_ne!(print(&moved), keyed, "a key moved");
    keyframed(&mut moved, &reg, id, &[("offset", 2.0, Value::Vec2([0.0, 0.0]))]);
    assert_ne!(print(&moved), keyed, "a key added");
}

#[test]
fn a_moved_key_changes_the_water_from_the_key_before() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    carve(&mut scene, vec![hole([0.0, 0.0], 300.0)]);
    let id = block(&mut scene, &reg, Vec2::ZERO, 80.0, 1.0);
    let keys: Vec<(&str, f64, Value)> =
        [1.0, 2.0, 3.0].iter().map(|t| ("offset", *t, Value::Vec2([*t as f32 * 10.0, 0.0]))).collect();
    keyframed(&mut scene, &reg, id, &keys);
    let inputs = |scene: &Scene| WaterSetup::final_of(scene, &bpe_core::resolve(scene, &reg)).inputs();
    let base = inputs(&scene);
    assert!(base.parts.iter().any(|p| p.name == "mover:0"));
    let change = |keys: &[(&str, f64, Value)]| {
        let mut s = scene.clone();
        keyframed(&mut s, &reg, id, keys);
        base.earliest_change(&inputs(&s))
    };
    assert_eq!(change(&[("offset", 3.0, Value::Vec2([99.0, 0.0]))]), Some(2.0), "from the key before the one moved");
    assert_eq!(change(&[("offset", 0.5, Value::Vec2([5.0, 0.0]))]), Some(0.0), "a key before every other");
    assert_eq!(change(&[("turn", 2.5, Value::Number(30.0))]), Some(0.0), "the first key of a setting");
    assert_eq!(change(&[]), None);
}

#[test]
fn notes_for_moving_parts() {
    let reg = crate::registry();
    let mut scene = water_scene(&reg);
    carve(&mut scene, vec![hole([0.0, 0.0], 500.0)]);
    let cutter = block(&mut scene, &reg, Vec2::new(0.0, -300.0), 60.0, 1.0);
    set(&mut scene, cutter, SlotId::Form, "cuts", Value::Bool(true));
    keyframed(&mut scene, &reg, cutter, &[]);
    let mut last = cutter;
    for k in 0..bpe_water::MOST_MOVERS {
        last = block(&mut scene, &reg, Vec2::new(-400.0 + (k % 8) as f32 * 100.0, (k / 8) as f32 * 50.0), 20.0, 1.0);
        keyframed(&mut scene, &reg, last, &[]);
    }
    let s = setup(&scene, &reg);
    assert_eq!(s.note_for(cutter), Some(MOVING_CUTS));
    assert_eq!(s.movers.len(), bpe_water::MOST_MOVERS);
    assert_eq!(s.note_for(last), Some(TOO_MANY_MOVERS), "the 65th");
    let place = scene.things[&last].transform.position;
    assert!(s.still_with(&s.mask()).at(place) < 0.0, "and stays where it is");
    // A moving part never cuts: it moves as the solid it is.
    let start = s.field();
    assert!(start.at(Vec2::new(0.0, -300.0)) < 0.0, "{}", start.at(Vec2::new(0.0, -300.0)));
}
