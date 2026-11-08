use bpe_core::{Address, Color, Gradient, LayerSection, Scene, SlotId, ThingId, Value, layered_key, resolve};

use super::*;
use crate::modules::look::layers as l;
use crate::modules::look::{BallShape, shape as s};
use crate::registry;

fn added(kind: &str) -> (Registry, Scene, ThingId) {
    let reg = registry();
    let mut scene = Scene::empty(&reg);
    let id = crate::starters::add_thing(&mut scene, &reg, kind, ThingId::ROOT).unwrap();
    (reg, scene, id)
}

/// The kinds of the thing's top-level layers but the group's, in order.
fn top_kinds(reg: &Registry, scene: &Scene, id: ThingId) -> Vec<String> {
    let card = scene.card(id, SlotId::Look).unwrap();
    let group = |k: &str| reg.layer(k).is_some_and(|k| k.section() == LayerSection::Group);
    card.layers.iter().filter(|x| x.parent.is_none() && !group(&x.kind)).map(|x| x.kind.clone()).collect()
}

fn find_look(id: &str) -> &'static ReadyLook {
    find(id).unwrap_or_else(|| panic!("no look {id}"))
}

#[test]
fn catalogue_is_whole() {
    let balls: Vec<_> = all().iter().filter(|x| x.wears == Wears::Balls).collect();
    let shapes: Vec<_> = all().iter().filter(|x| x.wears == Wears::Shape).collect();
    assert_eq!((balls.len(), shapes.len()), (30, 18));
    for (w, set) in [(Wears::Balls, &balls), (Wears::Shape, &shapes)] {
        let mut ids: Vec<_> = set.iter().map(|x| x.id).collect();
        let mut names: Vec<_> = set.iter().map(|x| x.name).collect();
        ids.sort();
        ids.dedup();
        names.sort();
        names.dedup();
        assert_eq!((ids.len(), names.len()), (set.len(), set.len()), "ids and names unique");
        for fam in families(w) {
            assert_eq!(set.iter().filter(|x| x.family == *fam).count(), 6, "{fam}");
        }
        assert!(set.iter().all(|x| families(w).contains(&x.family)));
    }
    for id in BALL_FAVOURITES {
        assert_eq!(find_look(id).wears, Wears::Balls);
    }
    for id in SHAPE_FAVOURITES {
        assert_eq!(find_look(id).wears, Wears::Shape);
    }
    assert_eq!(favourites("emitter").len(), 7);
    assert!(favourites("text").is_empty());
}

#[test]
fn every_look_pastes_cleanly() {
    for (kind, w) in [("balls", Wears::Balls), (crate::things::emitter::KIND, Wears::Balls), ("shape", Wears::Shape)] {
        for look in all().iter().filter(|x| x.wears == w) {
            for colours in [Colours::Look, Colours::Mine] {
                let (reg, mut scene, id) = added(kind);
                let n = apply(&mut scene, &reg, id, look, colours, How::Replace).unwrap();
                let recipe = look.recipe(None);
                assert_eq!(n, recipe.layers.len());
                let want: Vec<String> = recipe.layers.iter().map(|x| x.kind.to_owned()).collect();
                assert_eq!(top_kinds(&reg, &scene, id), want, "{} on {kind}", look.id);
                let subs = |sc: &Scene| {
                    sc.card(id, SlotId::Look).unwrap().layers.iter().filter(|x| x.parent.is_some()).count()
                };
                let want_subs: usize = recipe.layers.iter().map(|x| x.subs.len()).sum();
                assert_eq!(subs(&scene), want_subs, "{} sub-layers on {kind}", look.id);
                let problems: Vec<_> = resolve(&scene, &reg).problems.into_iter().filter(|p| p.thing == id).collect();
                assert!(problems.is_empty(), "{} on {kind}: {problems:?}", look.id);
            }
        }
    }
}

#[test]
fn layers_come_in_section_order() {
    let reg = registry();
    let rank = |s: LayerSection| match s {
        LayerSection::Drawn => 0,
        LayerSection::Around => 1,
        LayerSection::Reaction => 2,
        _ => 3,
    };
    for look in all() {
        let recipe = look.recipe(None);
        let ranks: Vec<_> = recipe.layers.iter().map(|x| rank(reg.layer(x.kind).unwrap().section())).collect();
        assert!(ranks.windows(2).all(|w| w[0] <= w[1]), "{}: {ranks:?}", look.id);
        assert!(reg.layer(recipe.layers[0].kind).unwrap().base(), "{} starts with its Body", look.id);
        for x in &recipe.layers {
            for sub in &x.subs {
                assert!(reg.layer(x.kind).unwrap().takes_sub(sub.kind), "{}: {} takes {}", look.id, x.kind, sub.kind);
            }
        }
    }
}

#[test]
fn every_stored_key_is_a_setting() {
    let reg = registry();
    let check = |look: &str, x: &RLayer| {
        let kind = reg.layer(x.kind).unwrap_or_else(|| panic!("{look}: no kind {}", x.kind));
        let get = |k: &str| x.params.get(k).cloned();
        for (key, _) in x.params.iter() {
            assert!(bpe_core::layer_def(kind, &get, key).is_some(), "{look}: {} has no setting {key}", x.kind);
        }
    };
    for look in all() {
        for x in look.recipe(None).layers {
            check(look.id, &x);
            for sub in &x.subs {
                check(look.id, sub);
            }
        }
        for key in look.recipe(None).card.iter().map(|(k, _)| k) {
            let card = match look.wears {
                Wears::Balls => l::CARD,
                Wears::Shape => s::CARD,
            };
            let defs = reg.module(card).unwrap().settings();
            assert!(defs.iter().any(|d| d.key == key), "{}: the card has no setting {key}", look.id);
        }
    }
}

#[test]
fn no_two_looks_are_the_same() {
    for (i, a) in all().iter().enumerate() {
        for b in &all()[i + 1..] {
            if a.wears == b.wears {
                assert_ne!(a.recipe(None), b.recipe(None), "{} and {}", a.id, b.id);
            }
        }
    }
}

/// The colour value of layer `kind`'s `key` on the thing.
fn colour_of(scene: &Scene, id: ThingId, kind: &str, key: &str) -> Option<Value> {
    let card = scene.card(id, SlotId::Look)?;
    let layer = card.layers.iter().find(|x| x.kind == kind)?;
    card.params.get(&layered_key(layer.id, key)).cloned()
}

#[test]
fn keep_mine_puts_the_colours_in() {
    let (reg, mut scene, id) = added("balls");
    let pink = Gradient::solid(Color::rgb(0xff, 0x00, 0x88));
    let body = scene.card(id, SlotId::Look).unwrap().layers[0].id;
    scene.card_mut(id, SlotId::Look).unwrap().params.set(&layered_key(body, "colours"), Value::Gradient(pink.clone()));
    assert_eq!(own_colours(&scene, id), Some(pink.clone()));
    apply(&mut scene, &reg, id, find_look("neon_ring"), Colours::Mine, How::Replace).unwrap();
    assert_eq!(colour_of(&scene, id, l::OUTLINE, "colour"), Some(Value::Gradient(pink.clone())));
    // Chrome has colours of its own: Keep mine changes nothing.
    let (reg, mut a, ida) = added("balls");
    let (_, mut b, idb) = added("balls");
    apply(&mut a, &reg, ida, find_look("chrome"), Colours::Mine, How::Replace).unwrap();
    apply(&mut b, &reg, idb, find_look("chrome"), Colours::Look, How::Replace).unwrap();
    assert_eq!(a.card(ida, SlotId::Look), b.card(idb, SlotId::Look));
    // A shape with no colours of its own has none to keep.
    let (_, shape, sid) = added("shape");
    assert_eq!(own_colours(&shape, sid), None);
}

#[test]
fn a_ball_keeps_its_shape() {
    let (reg, mut scene, id) = added("balls");
    let body = scene.card(id, SlotId::Look).unwrap().layers[0].id;
    let card = scene.card_mut(id, SlotId::Look).unwrap();
    card.params.set(&layered_key(body, "ball_shape"), Value::Choice("square".into()));
    card.params.set(&layered_key(body, "roundness"), Value::Number(0.4));
    apply(&mut scene, &reg, id, find_look("chrome"), Colours::Look, How::Replace).unwrap();
    let b = apply::body_values(&scene, id).unwrap();
    assert_eq!((b.ball_shape, b.roundness), (BallShape::Square, 0.4));
    assert_ne!(b.fill.colours, Gradient::solid(Color::WHITE), "the look's colours came in");
}

#[test]
fn the_group_layers_stay() {
    let (reg, mut scene, id) = added("balls");
    scene.add_layer(&reg, id, SlotId::Look, l::GROUP_GRADIENT).unwrap();
    apply(&mut scene, &reg, id, find_look("lava"), Colours::Look, How::Replace).unwrap();
    let card = scene.card(id, SlotId::Look).unwrap();
    assert!(card.layers.iter().any(|x| x.kind == l::GROUP_GRADIENT));
}

#[test]
fn on_top_keeps_the_stack() {
    let (reg, mut scene, id) = added("balls");
    let outline = scene.add_layer(&reg, id, SlotId::Look, l::OUTLINE).unwrap();
    let before_body = apply::body_values(&scene, id);
    let n = apply(&mut scene, &reg, id, find_look("neon_ring"), Colours::Look, How::OnTop).unwrap();
    assert_eq!(n, 2);
    assert_eq!(apply::body_values(&scene, id), before_body, "the Body is the thing's own");
    let kinds = top_kinds(&reg, &scene, id);
    assert_eq!(kinds, vec![l::BODY, l::OUTLINE, l::OUTLINE, l::GLOW]);
    assert!(scene.card(id, SlotId::Look).unwrap().layer(outline).is_some());
}

#[test]
fn a_pulse_follows_its_layer() {
    let (reg, mut scene, id) = added("balls");
    apply(&mut scene, &reg, id, find_look("pulse"), Colours::Look, How::Replace).unwrap();
    let card = scene.card(id, SlotId::Look).unwrap();
    let glow = card.layers.iter().find(|x| x.kind == l::GLOW).unwrap().id;
    let at = Address { thing: id, slot: Some(SlotId::Look), key: layered_key(glow, "strength") };
    assert!(scene.pulses.contains_key(&at));
    apply(&mut scene, &reg, id, find_look("chrome"), Colours::Look, How::Replace).unwrap();
    assert!(scene.pulses.keys().all(|a| a.thing != id), "the pulse went with its layer");
    // On top, it lands on the layer pasted.
    apply(&mut scene, &reg, id, find_look("pulse"), Colours::Look, How::OnTop).unwrap();
    assert_eq!(scene.pulses.keys().filter(|a| a.thing == id).count(), 1);
}

#[test]
fn a_look_only_goes_where_it_fits() {
    let (reg, mut scene, id) = added("shape");
    let before = scene.clone();
    assert!(apply(&mut scene, &reg, id, find_look("chrome"), Colours::Look, How::Replace).is_err());
    assert_eq!(scene, before);
    let (reg, mut scene, id) = added("balls");
    assert!(apply(&mut scene, &reg, id, find_look("shape.solid"), Colours::Look, How::Replace).is_err());
}

#[test]
fn add_with_look_adds_a_dressed_thing() {
    let reg = registry();
    let mut scene = Scene::empty(&reg);
    let id = add_with_look(&mut scene, &reg, "balls", ThingId::ROOT, find_look("comet")).unwrap();
    assert_eq!(top_kinds(&reg, &scene, id), vec![l::BODY, l::GLOW, l::TRAIL]);
    let shape = add_with_look(&mut scene, &reg, "shape", ThingId::ROOT, find_look("shape.neon_tube")).unwrap();
    assert_eq!(top_kinds(&reg, &scene, shape), vec![s::BODY, s::PAINT, s::OUTLINE, s::GLOW]);
}

#[test]
fn words_find_looks() {
    assert!(find_look("neon_ring").matches("neon"));
    assert!(find_look("glow_orb").matches("Neon"), "by family");
    assert!(find_look("chrome").matches("mirror metal"));
    assert!(!find_look("chrome").matches("neon"));
    let reg = registry();
    assert_eq!(find_look("chrome").comes_as(&reg), "Body, Shine, Outline, Flash on hit");
}
