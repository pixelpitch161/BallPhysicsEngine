//! The patch fingerprint: the editor builds the sound again only when it could have changed.

use bpe_core::{Address, Scene, Settings, SlotId, ThingId, Value, layered_key};
use bpe_kit::sound::{LIGHT_MOD, add_modulator, starter_voice};
use bpe_kit::things::light::{LightSettings, LightType, add_light};
use bpe_kit::{Kit, new_scene, resolve_scene};
use bpe_sound::patch_fingerprint;

fn shape_of(scene: &Scene) -> ThingId {
    scene.order.iter().copied().find(|id| scene.things[id].kind == "shape").unwrap()
}

fn sounding(kit: &Kit) -> Scene {
    let mut scene = new_scene(&kit.registry);
    bpe_kit::ensure_mixer(&mut scene, &kit.registry);
    let shape = shape_of(&scene);
    scene.things.get_mut(&shape).unwrap().slots.insert(SlotId::Sound, starter_voice(&kit.registry));
    scene
}

fn print(kit: &Kit, scene: &Scene) -> u64 {
    patch_fingerprint(scene, &resolve_scene(scene, &kit.registry))
}

/// The print after `edit`, beside the print before.
fn before_after(kit: &Kit, scene: &Scene, edit: impl FnOnce(&mut Scene)) -> (u64, u64) {
    let mut after = scene.clone();
    edit(&mut after);
    (print(kit, scene), print(kit, &after))
}

#[test]
fn picture_edits_keep_the_print() {
    let kit = bpe_kit::kit();
    let scene = sounding(&kit);
    let ball = scene.order.iter().copied().find(|id| scene.things[id].kind == "balls").unwrap();
    let (a, b) = before_after(&kit, &scene, |s| {
        s.set_at(&kit.registry, &Address::own(ThingId::ROOT, "bloom"), Value::Number(0.7)).unwrap();
        s.set_at(&kit.registry, &Address::own(ThingId::ROOT, "exposure"), Value::Number(2.0)).unwrap();
        s.things.get_mut(&ball).unwrap().transform.position.x += 30.0;
    });
    assert_eq!(a, b, "bloom, exposure and a ball's place are not the sound's");
}

#[test]
fn sound_edits_change_the_print() {
    let kit = bpe_kit::kit();
    let reg = &kit.registry;
    let scene = sounding(&kit);
    let shape = shape_of(&scene);
    type Edit<'a> = Box<dyn Fn(&mut Scene) + 'a>;
    let edits: Vec<(&str, Edit)> = vec![
        (
            "a sound knob",
            Box::new(|s| s.set_at(reg, &Address::on(shape, SlotId::Sound, "a_gain"), Value::Number(-8.0)).unwrap()),
        ),
        (
            "a mixer knob",
            Box::new(|s| {
                s.set_at(reg, &Address::on(ThingId::ROOT, SlotId::Mixer, "master_output"), Value::Number(-3.0)).unwrap()
            }),
        ),
        (
            "a key on a sound knob",
            Box::new(|s| {
                s.set_key(reg, &Address::on(shape, SlotId::Sound, "a_gain"), 2.0, Value::Number(-4.0)).unwrap();
            }),
        ),
        ("the tempo", Box::new(|s| s.set_at(reg, &Address::own(ThingId::ROOT, "tempo"), Value::Number(97.0)).unwrap())),
        (
            "the clip length",
            Box::new(|s| s.set_at(reg, &Address::own(ThingId::ROOT, "duration"), Value::Number(9.0)).unwrap()),
        ),
        (
            "sound off",
            Box::new(|s| {
                s.things.get_mut(&shape).unwrap().slots.remove(&SlotId::Sound);
            }),
        ),
    ];
    for (what, edit) in edits {
        let (a, b) = before_after(&kit, &scene, edit);
        assert_ne!(a, b, "{what} changes the sound");
    }
}

#[test]
fn a_lamp_changes_the_print_only_when_a_light_modulator_hears_it() {
    let kit = bpe_kit::kit();
    let mut scene = sounding(&kit);
    let light = add_light(&mut scene, &kit.registry, LightType::Point, ThingId::ROOT).unwrap();
    let brighter = |s: &mut Scene| {
        let mut l = LightSettings::from_params(&s.things[&light].params);
        l.strength += 1.0;
        s.things.get_mut(&light).unwrap().params = l.to_params();
    };
    let (a, b) = before_after(&kit, &scene, brighter);
    assert_eq!(a, b, "nothing in the sound reads the light");
    let shape = shape_of(&scene);
    let layer = add_modulator(&mut scene, &kit.registry, shape, LIGHT_MOD).unwrap();
    let card = scene.things.get_mut(&shape).unwrap().slots.get_mut(&SlotId::Sound).unwrap();
    card.params.set(&layered_key(layer, "smooth"), Value::Number(0.0));
    let (a, b) = before_after(&kit, &scene, brighter);
    assert_ne!(a, b, "the Light modulator hears the lamp");
}
