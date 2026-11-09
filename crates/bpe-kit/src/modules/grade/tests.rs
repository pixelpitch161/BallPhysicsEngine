use super::*;
use bpe_core::{Value, resolve};
use bpe_gfx::GradePanel;

fn graded() -> (crate::Kit, Scene) {
    let kit = crate::kit();
    let mut scene = Scene::empty(&kit.registry);
    assert!(ensure_grade(&mut scene, &kit.registry));
    (kit, scene)
}

fn card(scene: &Scene) -> &Slot {
    scene.card(ThingId::ROOT, SlotId::Grade).unwrap()
}

fn kinds(scene: &Scene) -> Vec<(String, String)> {
    let c = card(scene);
    panels_of(c, whole_of(c).unwrap()).iter().map(|l| (l.kind.to_string(), l.name.clone())).collect()
}

fn stack_of(kit: &crate::Kit, scene: &Scene) -> bpe_gfx::GradeStack {
    let resolved = resolve(scene, &kit.registry);
    stack(resolved.things[&ThingId::ROOT].slots.get(&SlotId::Grade))
}

#[test]
fn the_kinds_register_and_pass_the_registry_s_checks() {
    let kit = crate::kit();
    let r = &kit.registry;
    assert_eq!(r.module(GRADE).unwrap().slot(), SlotId::Grade);
    assert!(r.module(GRADE).unwrap().fits("scene") && !r.module(GRADE).unwrap().fits("balls"));
    assert_eq!(r.layer(WHOLE).unwrap().section(), LayerSection::Mask);
    for kind in PANEL_ORDER {
        let k = r.layer(kind).unwrap();
        assert_eq!(k.section(), LayerSection::Panel, "{kind}");
        assert!(k.repeats(), "{kind}: Add panel makes copies");
    }
    let problems = bpe_core::check_registry(r);
    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn a_new_grade_is_the_whole_picture_with_basic_then_tone_curve_at_their_defaults() {
    let (kit, scene) = graded();
    let c = card(&scene);
    assert_eq!(c.layers[0].kind, WHOLE);
    assert_eq!(c.layers[0].name, "Whole picture");
    assert_eq!(
        kinds(&scene),
        [(BASIC.to_string(), "Basic".to_string()), (TONE_CURVE.to_string(), "Tone Curve".to_string())]
    );
    assert_eq!(*c, starting_card(&kit.registry), "the editor's template is the card the first edit makes");
    let s = stack_of(&kit, &scene);
    assert_eq!(s.masks.len(), 1);
    assert!(s.is_identity(), "{s:?}");
    assert!(c.params.0.keys().all(|k| !k.contains('/')), "no panel values stored");
}

#[test]
fn panels_reach_the_stack_in_their_order_with_their_values_and_hidden_ones_are_skipped() {
    let (kit, mut scene) = graded();
    let whole = whole_of(card(&scene)).unwrap();
    let ids: Vec<LayerId> = panels_of(card(&scene), whole).iter().map(|l| l.id).collect();
    let (basic, curve) = (ids[0], ids[1]);
    let params = &mut scene.card_mut(ThingId::ROOT, SlotId::Grade).unwrap().params;
    params.set(&format!("{basic}/exposure"), Value::Number(1.5));
    params.set(&format!("{curve}/lights"), Value::Number(-30.0));
    let s = stack_of(&kit, &scene);
    let panels = &s.masks[0].panels;
    assert!(matches!(&panels[0], GradePanel::Basic(b) if b.exposure == 1.5));
    assert!(matches!(&panels[1], GradePanel::ToneCurve(t) if t.lights == -30.0));
    // Dragged: the curve works first.
    scene.move_sub_layer_in(&kit.registry, ThingId::ROOT, SlotId::Grade, curve, 0).unwrap();
    let s = stack_of(&kit, &scene);
    assert!(matches!(s.masks[0].panels[0], GradePanel::ToneCurve(_)));
    // A panel switched off works no more.
    scene.card_mut(ThingId::ROOT, SlotId::Grade).unwrap().layers.iter_mut().find(|l| l.id == curve).unwrap().enabled =
        false;
    let s = stack_of(&kit, &scene);
    assert_eq!(s.masks[0].panels.len(), 1);
    assert!(matches!(s.masks[0].panels[0], GradePanel::Basic(_)));
}

#[test]
fn add_panel_makes_numbered_copies_and_reset_order_puts_them_after_their_first() {
    let (kit, mut scene) = graded();
    let reg = &kit.registry;
    let whole = whole_of(card(&scene)).unwrap();
    let second = add_panel(&mut scene, reg, whole, TONE_CURVE).unwrap();
    let third_basic = add_panel(&mut scene, reg, whole, BASIC).unwrap();
    let names: Vec<String> = kinds(&scene).into_iter().map(|(_, n)| n).collect();
    assert_eq!(names, ["Basic", "Tone Curve", "Tone Curve 2", "Basic 2"]);
    // Shuffle, then reset.
    scene.move_sub_layer_in(reg, ThingId::ROOT, SlotId::Grade, second, 0).unwrap();
    scene.move_sub_layer_in(reg, ThingId::ROOT, SlotId::Grade, third_basic, 1).unwrap();
    let names: Vec<String> = kinds(&scene).into_iter().map(|(_, n)| n).collect();
    assert_eq!(names, ["Tone Curve 2", "Basic 2", "Basic", "Tone Curve"]);
    assert!(reset_order(&mut scene, reg, whole));
    let names: Vec<String> = kinds(&scene).into_iter().map(|(_, n)| n).collect();
    assert_eq!(names, ["Basic 2", "Basic", "Tone Curve 2", "Tone Curve"], "copies keep their own order");
    assert!(!reset_order(&mut scene, reg, whole), "already in order");
    // The Whole picture stays first.
    assert_eq!(card(&scene).layers[0].kind, WHOLE);
}

#[test]
fn a_mask_keeps_at_least_one_panel() {
    let (kit, mut scene) = graded();
    let whole = whole_of(card(&scene)).unwrap();
    let ids: Vec<LayerId> = panels_of(card(&scene), whole).iter().map(|l| l.id).collect();
    assert!(can_remove_panel(card(&scene), ids[0]));
    scene.remove_layer(&kit.registry, ThingId::ROOT, SlotId::Grade, ids[0]).unwrap();
    assert!(!can_remove_panel(card(&scene), ids[1]), "the last panel");
    assert!(!can_remove_panel(card(&scene), whole), "not a panel");
}

#[test]
fn the_grade_off_gives_an_empty_stack_and_keeps_its_settings() {
    let (kit, mut scene) = graded();
    let whole = whole_of(card(&scene)).unwrap();
    let basic = panels_of(card(&scene), whole)[0].id;
    let c = scene.card_mut(ThingId::ROOT, SlotId::Grade).unwrap();
    c.params.set(&format!("{basic}/saturation"), Value::Number(-100.0));
    assert!(!stack_of(&kit, &scene).is_identity());
    scene.card_mut(ThingId::ROOT, SlotId::Grade).unwrap().params.set("on", Value::Bool(false));
    let s = stack_of(&kit, &scene);
    assert!(s.masks.is_empty() && s.is_identity());
    assert_eq!(card(&scene).params.get(&format!("{basic}/saturation")), Some(&Value::Number(-100.0)));
}

#[test]
fn panels_take_no_sub_layers_and_the_whole_picture_takes_only_panels() {
    let (kit, mut scene) = graded();
    let reg = &kit.registry;
    let whole = whole_of(card(&scene)).unwrap();
    let basic = panels_of(card(&scene), whole)[0].id;
    assert!(scene.add_sub_layer(reg, ThingId::ROOT, SlotId::Grade, basic, TONE_CURVE).is_err());
    assert!(scene.add_layer(reg, ThingId::ROOT, SlotId::Grade, BASIC).is_err(), "a panel is never a mask");
    assert!(scene.add_layer(reg, ThingId::ROOT, SlotId::Grade, WHOLE).is_err(), "one whole picture");
    // A non-panel kind of another card is refused under the whole picture.
    assert!(scene.add_sub_layer(reg, ThingId::ROOT, SlotId::Grade, whole, "glow").is_err());
}

#[test]
fn a_graded_scene_saves_and_opens_the_same() {
    let (kit, mut scene) = graded();
    let whole = whole_of(card(&scene)).unwrap();
    let curve = panels_of(card(&scene), whole)[1].id;
    let points = Value::Curve(vec![
        bpe_core::CurvePoint::new(0.0, 0.1, 0.0),
        bpe_core::CurvePoint::new(0.5, 0.6, 0.0),
        bpe_core::CurvePoint::new(1.0, 0.9, 0.0),
    ]);
    scene.card_mut(ThingId::ROOT, SlotId::Grade).unwrap().params.set(&format!("{curve}/rgb"), points.clone());
    let text = bpe_core::save::save(&scene);
    let back = bpe_core::save::load(&text, &kit.registry).unwrap();
    assert_eq!(back.card(ThingId::ROOT, SlotId::Grade), scene.card(ThingId::ROOT, SlotId::Grade));
    assert_eq!(stack_of(&kit, &back), stack_of(&kit, &scene));
}
