//! Every thing but the scene settings carries an editor track colour: new things take the next
//! free one, the shape and camera start with their own, and old files get theirs on opening.

use bpe_core::{Scene, ThingId, TrackColour, load, save};
use bpe_kit::{registry, starters::add_thing};

fn colours(scene: &Scene) -> Vec<Option<TrackColour>> {
    scene.order.iter().map(|id| scene.things[id].colour).collect()
}

#[test]
fn colours_go_in_order() {
    let reg = registry();
    let mut scene = Scene::empty(&reg);
    for _ in 0..3 {
        add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
    }
    use TrackColour::*;
    assert_eq!(colours(&scene), [None, Some(Pink), Some(Cyan), Some(Lime)]);
}

#[test]
fn shapes_and_cameras_take_their_own() {
    let reg = registry();
    let mut scene = Scene::empty(&reg);
    let camera = add_thing(&mut scene, &reg, "camera", ThingId::ROOT).unwrap();
    let arena = add_thing(&mut scene, &reg, "shape", ThingId::ROOT).unwrap();
    let second = add_thing(&mut scene, &reg, "shape", ThingId::ROOT).unwrap();
    assert_eq!(scene.things[&camera].colour, Some(TrackColour::Violet));
    assert_eq!(scene.things[&arena].colour, Some(TrackColour::Orange));
    assert_eq!(scene.things[&second].colour, Some(TrackColour::Pink), "orange is taken");
}

#[test]
fn the_ninth_thing_takes_the_least_used() {
    let reg = registry();
    let mut scene = Scene::empty(&reg);
    let ids: Vec<ThingId> = (0..9).map(|_| add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap()).collect();
    assert_eq!(scene.things[&ids[8]].colour, Some(TrackColour::Pink));
    // Cyan is now used once and every other colour but Pink once too; take Cyan away.
    scene.things.get_mut(&ids[1]).unwrap().colour = Some(TrackColour::Lime);
    let next = add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
    assert_eq!(scene.things[&next].colour, Some(TrackColour::Cyan));
}

#[test]
fn old_scenes_get_colours_in_tree_order() {
    let reg = registry();
    let mut scene = Scene::empty(&reg);
    for kind in ["balls", "shape", "balls", "camera"] {
        add_thing(&mut scene, &reg, kind, ThingId::ROOT).unwrap();
    }
    for thing in scene.things.values_mut() {
        thing.colour = None;
    }
    scene.things.get_mut(&ThingId::ROOT).unwrap().colour = Some(TrackColour::Red);
    let text = save(&scene);
    let opened = load(&text, &reg).unwrap();
    use TrackColour::*;
    assert_eq!(colours(&opened), [None, Some(Pink), Some(Orange), Some(Cyan), Some(Violet)]);
}

#[test]
fn a_colour_survives_save_and_load() {
    let reg = registry();
    let mut scene = Scene::empty(&reg);
    let id = add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
    scene.things.get_mut(&id).unwrap().colour = Some(TrackColour::Teal);
    let opened = load(&save(&scene), &reg).unwrap();
    assert_eq!(opened.things[&id].colour, Some(TrackColour::Teal));
}
