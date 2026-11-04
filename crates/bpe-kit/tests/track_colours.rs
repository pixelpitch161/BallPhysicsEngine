//! Every thing but the scene settings carries an editor track colour: any colour, a random
//! readable one for a new thing, spread round the colour wheel; old files get theirs on opening.

use bpe_core::{Scene, ThingId, TrackColour, load, save};
use bpe_kit::{registry, starters::add_thing};

/// sRGB bytes to HSL lightness and saturation.
fn lightness_saturation(c: TrackColour) -> (f32, f32) {
    let [r, g, b] = c.0.map(|v| v as f32 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let s = if max == min { 0.0 } else { (max - min) / (1.0 - (2.0 * l - 1.0).abs()) };
    (l, s)
}

#[test]
fn new_things_take_readable_colours_spread_round_the_wheel() {
    let reg = registry();
    let mut scene = Scene::empty(&reg);
    let ids: Vec<ThingId> = (0..4).map(|_| add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap()).collect();
    assert_eq!(scene.things[&ThingId::ROOT].colour, None, "the scene settings have none");
    let hues: Vec<f32> = ids
        .iter()
        .map(|id| {
            let c = scene.things[id].colour.expect("a new thing has a colour");
            let (l, s) = lightness_saturation(c);
            assert!((0.58..=0.74).contains(&l), "{} is readable: lightness {l}", c.hex());
            assert!((0.5..=0.9).contains(&s), "{} is colourful: saturation {s}", c.hex());
            c.hue().unwrap()
        })
        .collect();
    // The best of 16 random hues is never close to the ones in use: four things spread out.
    for (i, a) in hues.iter().enumerate() {
        for b in &hues[i + 1..] {
            let d = (a - b).rem_euclid(1.0);
            assert!(d.min(1.0 - d) > 0.04, "hues {a} and {b} are too close");
        }
    }
}

#[test]
fn shapes_and_cameras_take_colours_too() {
    let reg = registry();
    let mut scene = Scene::empty(&reg);
    for kind in ["camera", "shape", "folder"] {
        let id = add_thing(&mut scene, &reg, kind, ThingId::ROOT).unwrap();
        assert!(scene.things[&id].colour.is_some(), "{kind}");
    }
}

#[test]
fn old_scenes_get_colours() {
    let reg = registry();
    let mut scene = Scene::empty(&reg);
    for kind in ["balls", "shape", "balls", "camera"] {
        add_thing(&mut scene, &reg, kind, ThingId::ROOT).unwrap();
    }
    for thing in scene.things.values_mut() {
        thing.colour = None;
    }
    scene.things.get_mut(&ThingId::ROOT).unwrap().colour = Some(TrackColour::RED);
    let opened = load(&save(&scene), &reg).unwrap();
    for id in &opened.order {
        assert_eq!(opened.things[id].colour.is_some(), *id != ThingId::ROOT);
    }
}

#[test]
fn a_colour_survives_save_and_load_and_old_names_read() {
    let reg = registry();
    let mut scene = Scene::empty(&reg);
    let id = add_thing(&mut scene, &reg, "balls", ThingId::ROOT).unwrap();
    scene.things.get_mut(&id).unwrap().colour = Some(TrackColour([0x12, 0xab, 0xef]));
    let text = save(&scene);
    assert!(text.contains("\"#12abef\""), "saved as hex");
    let opened = load(&text, &reg).unwrap();
    assert_eq!(opened.things[&id].colour, Some(TrackColour([0x12, 0xab, 0xef])));
    // A file from before the picker names one of the eight colours.
    let old = text.replace("\"#12abef\"", "\"teal\"");
    assert_eq!(load(&old, &reg).unwrap().things[&id].colour, Some(TrackColour::TEAL));
    assert_eq!(TrackColour::parse("nope"), None);
    assert_eq!(TrackColour::parse("#12abef"), Some(TrackColour([0x12, 0xab, 0xef])));
}
