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
    stack(resolved.things[&ThingId::ROOT].slots.get(&SlotId::Grade), None)
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
        [
            (BASIC.to_string(), "Basic".to_string()),
            (TONE_CURVE.to_string(), "Tone Curve".to_string()),
            (HSL.to_string(), "HSL / Colour".to_string()),
            (POINT_COLOUR.to_string(), "Point Colour".to_string()),
            (COLOUR_GRADING.to_string(), "Colour Grading".to_string()),
            (DETAIL.to_string(), "Detail".to_string()),
            (EFFECTS.to_string(), "Effects".to_string()),
            (CALIBRATION.to_string(), "Calibration".to_string()),
        ]
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
    assert_eq!(s.masks[0].panels.len(), PANEL_ORDER.len() - 1);
    assert!(matches!(s.masks[0].panels[0], GradePanel::Basic(_)));
    assert!(!s.masks[0].panels.iter().any(|p| matches!(p, GradePanel::ToneCurve(_))));
}

#[test]
fn add_panel_makes_numbered_copies_and_reset_order_puts_them_after_their_first() {
    let (kit, mut scene) = graded();
    let reg = &kit.registry;
    let whole = whole_of(card(&scene)).unwrap();
    let second = add_panel(&mut scene, reg, whole, TONE_CURVE).unwrap();
    let third_basic = add_panel(&mut scene, reg, whole, BASIC).unwrap();
    let names: Vec<String> = kinds(&scene).into_iter().map(|(_, n)| n).collect();
    let colour = ["HSL / Colour", "Point Colour", "Colour Grading", "Detail", "Effects", "Calibration"];
    let with = |names: &[&str]| names.iter().chain(&colour).map(|n| n.to_string()).collect::<Vec<_>>();
    let mut want = vec!["Basic", "Tone Curve"];
    want.extend(colour);
    want.extend(["Tone Curve 2", "Basic 2"]);
    assert_eq!(names, want);
    // Shuffle, then reset.
    scene.move_sub_layer_in(reg, ThingId::ROOT, SlotId::Grade, second, 0).unwrap();
    scene.move_sub_layer_in(reg, ThingId::ROOT, SlotId::Grade, third_basic, 1).unwrap();
    let names: Vec<String> = kinds(&scene).into_iter().map(|(_, n)| n).collect();
    assert_eq!(names[..4], ["Tone Curve 2", "Basic 2", "Basic", "Tone Curve"]);
    assert!(reset_order(&mut scene, reg, whole));
    let names: Vec<String> = kinds(&scene).into_iter().map(|(_, n)| n).collect();
    assert_eq!(names, with(&["Basic 2", "Basic", "Tone Curve 2", "Tone Curve"]), "copies keep their own order");
    assert!(!reset_order(&mut scene, reg, whole), "already in order");
    // The Whole picture stays first.
    assert_eq!(card(&scene).layers[0].kind, WHOLE);
}

#[test]
fn a_mask_keeps_at_least_one_panel() {
    let (kit, mut scene) = graded();
    let whole = whole_of(card(&scene)).unwrap();
    let ids: Vec<LayerId> = panels_of(card(&scene), whole).iter().map(|l| l.id).collect();
    let (last, rest) = ids.split_last().unwrap();
    for id in rest {
        assert!(can_remove_panel(card(&scene), *id));
        scene.remove_layer(&kit.registry, ThingId::ROOT, SlotId::Grade, *id).unwrap();
    }
    assert!(!can_remove_panel(card(&scene), *last), "the last panel");
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

#[test]
fn the_colour_panels_reach_the_stack_with_their_values() {
    let (kit, mut scene) = graded();
    let whole = whole_of(card(&scene)).unwrap();
    let ids: Vec<LayerId> = panels_of(card(&scene), whole).iter().map(|l| l.id).collect();
    let (hsl, point, wheels, calibration) = (ids[2], ids[3], ids[4], ids[7]);
    let params = &mut scene.card_mut(ThingId::ROOT, SlotId::Grade).unwrap().params;
    params.set(&format!("{hsl}/blue_saturation"), Value::Number(-100.0));
    params.set(&format!("{hsl}/red_hue"), Value::Number(20.0));
    params.set(&format!("{point}/pick2_on"), Value::Bool(true));
    params.set(&format!("{point}/pick2_colour"), Value::Color(bpe_core::Color::rgb(0, 0, 255)));
    params.set(&format!("{point}/pick2_luminance"), Value::Number(-40.0));
    params.set(&format!("{point}/pick5_luminance"), Value::Number(70.0));
    params.set(&format!("{wheels}/shadows_hue"), Value::Number(210.0));
    params.set(&format!("{wheels}/shadows_saturation"), Value::Number(30.0));
    params.set(&format!("{calibration}/red_hue"), Value::Number(-15.0));
    let s = stack_of(&kit, &scene);
    let panels = &s.masks[0].panels;
    assert!(matches!(&panels[2], GradePanel::Hsl(h) if h.saturation[5] == -100.0 && h.hue[0] == 20.0));
    match &panels[3] {
        GradePanel::PointColour(p) => {
            assert_eq!(p.picks.len(), 1, "only used picks: {p:?}");
            assert_eq!(p.picks[0].luminance, -40.0);
            assert_eq!(p.picks[0].range, 50.0);
            assert!(p.picks[0].colour[2] > 0.99 && p.picks[0].colour[0] == 0.0, "linear blue");
        }
        other => panic!("{other:?}"),
    }
    assert!(
        matches!(&panels[4], GradePanel::ColourGrading(g) if g.shadows.hue == 210.0 && g.shadows.saturation == 30.0 && g.blending == 50.0)
    );
    assert!(matches!(&panels[7], GradePanel::Calibration(c) if c.red_hue == -15.0));
}

#[test]
fn point_colour_s_free_slot_is_the_first_unused_one() {
    let mut p = PointColourPanel::default();
    assert_eq!(p.free_slot(), Some(0));
    p.pick1.on = true;
    p.pick3.on = true;
    assert_eq!(p.free_slot(), Some(1));
    for pick in [&mut p.pick2, &mut p.pick4, &mut p.pick5, &mut p.pick6, &mut p.pick7, &mut p.pick8] {
        pick.on = true;
    }
    assert_eq!(p.free_slot(), None);
    assert_eq!(pick_prefix(0), "pick1_");
    assert!(PointColourPanel::defs().iter().any(|d| d.key == "pick8_range" && d.default == Value::Number(50.0)));
}

#[test]
fn presence_detail_and_effects_reach_the_stack_with_their_values() {
    let (kit, mut scene) = graded();
    let whole = whole_of(card(&scene)).unwrap();
    let ids: Vec<LayerId> = panels_of(card(&scene), whole).iter().map(|l| l.id).collect();
    let (basic, detail, effects) = (ids[0], ids[5], ids[6]);
    let map = bpe_core::Gradient::even(&[bpe_core::Color::rgb(20, 0, 60), bpe_core::Color::rgb(255, 200, 0)]);
    let params = &mut scene.card_mut(ThingId::ROOT, SlotId::Grade).unwrap().params;
    params.set(&format!("{basic}/clarity"), Value::Number(35.0));
    params.set(&format!("{basic}/dehaze"), Value::Number(-20.0));
    params.set(&format!("{detail}/amount"), Value::Number(80.0));
    params.set(&format!("{detail}/radius"), Value::Number(2.0));
    params.set(&format!("{effects}/grain_amount"), Value::Number(30.0));
    params.set(&format!("{effects}/map"), Value::Gradient(map.clone()));
    params.set(&format!("{effects}/map_amount"), Value::Number(40.0));
    let s = stack_of(&kit, &scene);
    let panels = &s.masks[0].panels;
    assert!(matches!(&panels[0], GradePanel::Basic(b) if b.clarity == 35.0 && b.dehaze == -20.0 && b.texture == 0.0));
    assert!(matches!(&panels[5], GradePanel::Detail(d) if d.amount == 80.0 && d.radius == 2.0 && d.detail == 25.0));
    assert!(
        matches!(&panels[6], GradePanel::Effects(e) if e.grain_amount == 30.0 && e.map_amount == 40.0 && e.map == map
            && e.vignette_midpoint == 50.0 && e.grain_size == 25.0)
    );
    assert!(!s.is_identity());
    // The panels' defaults are the frame's defaults.
    assert_eq!(DetailPanel::default().gpu(), bpe_gfx::GradeDetail::default());
    assert_eq!(EffectsPanel::default().gpu(), bpe_gfx::GradeEffects::default());
}

// ---- Part 4: masks ----

use bpe_core::Target;
use bpe_gfx::{GradeCombine, GradePlace, GradeShape};
use masks::{RANGE_PICKS, follow_note, places};

#[test]
fn mask_and_part_kinds_register_in_their_sections() {
    let kit = crate::kit();
    let r = &kit.registry;
    assert_eq!(r.layer(MASK).unwrap().section(), LayerSection::Mask);
    assert!(r.layer(MASK).unwrap().repeats() && !r.layer(MASK).unwrap().base());
    for kind in PART_KINDS {
        assert_eq!(r.layer(kind).unwrap().section(), LayerSection::MaskPart, "{kind}");
        assert!(r.layer(MASK).unwrap().takes_sub(kind) && !r.layer(WHOLE).unwrap().takes_sub(kind), "{kind}");
    }
    assert_eq!(ColourRangePart::default().slots().len(), RANGE_PICKS);
}

#[test]
fn a_new_mask_comes_last_with_its_part_and_a_basic_panel() {
    let (kit, mut scene) = graded();
    let reg = &kit.registry;
    let first = add_mask(&mut scene, reg, RADIAL).unwrap();
    let second = add_mask(&mut scene, reg, LINEAR).unwrap();
    let c = card(&scene);
    let names: Vec<&str> = masks_of(c).iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Mask", "Mask 2"]);
    assert_eq!(parts_of(c, first).iter().map(|l| l.kind.as_str()).collect::<Vec<_>>(), [RADIAL]);
    assert_eq!(panels_of(c, second).iter().map(|l| l.kind.as_str()).collect::<Vec<_>>(), [BASIC]);
    // A part added later goes after the parts, before the panels, and keeps how it joins.
    let sub = add_part(&mut scene, reg, first, LUMINANCE_RANGE, Combine::Subtract).unwrap();
    let c = card(&scene);
    let at = |id: LayerId| c.layers.iter().position(|l| l.id == id).unwrap();
    assert!(at(sub) < at(panels_of(c, first)[0].id));
    let s = stack_of(&kit, &scene);
    assert_eq!(s.masks.len(), 3, "the Whole picture and two masks");
    let area = s.masks[1].area.as_ref().unwrap();
    assert_eq!(area.parts.len(), 2);
    assert_eq!(area.parts[1].combine, GradeCombine::Subtract);
    assert!(matches!(&area.parts[0].shape, GradeShape::Radial(r) if r.places == [GradePlace::SCREEN]));
    assert_eq!((s.masks[1].amount, s.masks[0].area.is_none()), (1.0, true));
    assert_eq!(s.masks[1].id, first.0);
}

#[test]
fn a_hidden_mask_or_part_is_left_out_and_amount_is_a_share() {
    let (kit, mut scene) = graded();
    let reg = &kit.registry;
    let mask = add_mask(&mut scene, reg, RADIAL).unwrap();
    let part = parts_of(card(&scene), mask)[0].id;
    scene
        .card_mut(ThingId::ROOT, SlotId::Grade)
        .unwrap()
        .params
        .set(&bpe_core::layered_key(mask, "amount"), Value::Number(40.0));
    assert!((stack_of(&kit, &scene).masks[1].amount - 0.4).abs() < 1e-6);
    scene.set_layer_enabled(reg, ThingId::ROOT, SlotId::Grade, part, false).unwrap();
    assert!(stack_of(&kit, &scene).masks[1].area.as_ref().unwrap().parts.is_empty());
    scene.set_layer_enabled(reg, ThingId::ROOT, SlotId::Grade, mask, false).unwrap();
    assert_eq!(stack_of(&kit, &scene).masks.len(), 1);
}

/// The test scene's balls (no arena, no gravity) as a built world.
fn world_with_balls() -> (crate::Kit, Scene, bpe_runtime::World) {
    let (scene, resolved, kit) = crate::testing::scene_with_balls("block", |p| {
        p.set("columns", Value::Int(3));
        p.set("rows", Value::Int(1));
        p.set("spacing", Value::Number(100.0));
    });
    let world = bpe_runtime::World::build(&scene, &resolved, &kit.sims);
    (kit, scene, world)
}

#[test]
fn gradients_sit_on_a_ball_on_every_ball_or_in_the_world() {
    let (_kit, _scene, mut world) = world_with_balls();
    let balls = Target::Thing(crate::testing::BALLS);
    let b = &world.bodies[&crate::testing::BALLS];
    assert_eq!(b.len(), 3, "the block's balls");
    let second = b.position[b.row_of(1).unwrap()];
    let one = places(Some(&world), Placed::OnOneBall, &balls, 2, false, false);
    assert_eq!(one.len(), 1);
    assert!((one[0].at[0] - second.x).abs() < 1e-3 && (one[0].at[1] - second.y).abs() < 1e-3);
    assert_eq!(places(Some(&world), Placed::OnEveryBall, &balls, 1, false, false).len(), 3);
    // Grows with it: a 48 px ball doubles the sizes.
    world.bodies.get_mut(&crate::testing::BALLS).unwrap().radius.iter_mut().for_each(|r| *r = 48.0);
    let grown = places(Some(&world), Placed::OnOneBall, &balls, 1, false, true);
    assert!((grown[0].scale - 2.0).abs() < 1e-6);
    // In the world, the camera moving right moves it left on the screen, and its zoom scales it.
    world.camera = bpe_runtime::CameraView { center: glam::Vec2::new(100.0, 0.0), zoom: 2.0, turn: 0.0 };
    let w = places(Some(&world), Placed::InTheWorld, &balls, 1, false, false);
    assert_eq!(w, [GradePlace { at: [-200.0, 0.0], turn: 0.0, scale: 2.0, world: true }]);
    assert_eq!(places(Some(&world), Placed::OnTheScreen, &balls, 1, false, false), [GradePlace::SCREEN]);
    // Nothing there: no place, and the row says so.
    let gone = Target::Thing(ThingId(999));
    assert!(places(Some(&world), Placed::OnOneBall, &gone, 1, false, false).is_empty());
    assert!(places(Some(&world), Placed::OnOneBall, &balls, 9, false, false).is_empty(), "no ninth ball");
    let mut p = bpe_core::Params::default();
    p.set("place", Value::Choice("on_every_ball".into()));
    p.set("target", Value::Target(gone));
    assert_eq!(follow_note(&world, RADIAL, &p).as_deref(), Some("Thing not found"));
    p.set("target", Value::Target(balls));
    assert_eq!(follow_note(&world, RADIAL, &p), None);
}

#[test]
fn a_ball_s_turn_is_the_way_it_is_going() {
    let (_kit, _scene, mut world) = world_with_balls();
    let balls = Target::Thing(crate::testing::BALLS);
    world.bodies.get_mut(&crate::testing::BALLS).unwrap().velocity[0] = glam::Vec2::new(0.0, 50.0);
    let row = world.bodies[&crate::testing::BALLS].row_of(0).unwrap();
    assert_eq!(row, 0);
    let turned = places(Some(&world), Placed::OnOneBall, &balls, 1, true, false);
    assert!((turned[0].turn - std::f32::consts::FRAC_PI_2).abs() < 1e-6, "going down: a quarter turn clockwise");
    let still = places(Some(&world), Placed::OnOneBall, &balls, 1, false, false);
    assert_eq!(still[0].turn, 0.0);
}

#[test]
fn a_gradient_with_nowhere_to_be_is_left_out_so_it_covers_nothing_even_inverted() {
    let (kit, mut scene) = graded();
    let reg = &kit.registry;
    let mask = add_mask(&mut scene, reg, RADIAL).unwrap();
    let part = parts_of(card(&scene), mask)[0].id;
    let params = &mut scene.card_mut(ThingId::ROOT, SlotId::Grade).unwrap().params;
    params.set(&bpe_core::layered_key(part, "place"), Value::Choice("on_a_thing".into()));
    params.set(&bpe_core::layered_key(part, "target"), Value::Target(Target::Thing(ThingId(999))));
    params.set(&bpe_core::layered_key(part, "invert"), Value::Bool(true));
    params.set(&bpe_core::layered_key(mask, "invert"), Value::Bool(true));
    let s = stack_of(&kit, &scene);
    let area = s.masks[1].area.as_ref().unwrap();
    assert!(area.parts.is_empty(), "{area:?}");
    assert_eq!(bpe_gfx::grade::maths::area_cover(area, [0.5; 3], [0.0, 0.0], 1.0), 0.0);
}

#[test]
fn a_mask_s_panels_count_from_their_own_first_and_never_share_a_name() {
    let (kit, mut scene) = graded();
    let reg = &kit.registry;
    let mask = add_mask(&mut scene, reg, LINEAR).unwrap();
    let names = |scene: &Scene| panels_of(card(scene), mask).iter().map(|l| l.name.clone()).collect::<Vec<_>>();
    assert_eq!(names(&scene), ["Basic"], "not numbered after the Whole picture's Basic");
    add_panel(&mut scene, reg, mask, BASIC).unwrap();
    let third = add_panel(&mut scene, reg, mask, BASIC).unwrap();
    assert_eq!(names(&scene), ["Basic", "Basic 2", "Basic 3"]);
    let second = panels_of(card(&scene), mask)[1].id;
    scene.remove_layer(reg, ThingId::ROOT, SlotId::Grade, second).unwrap();
    add_panel(&mut scene, reg, mask, BASIC).unwrap();
    assert_eq!(names(&scene), ["Basic", "Basic 3", "Basic 4"]);
    assert!(card(&scene).layer(third).is_some());
    assert_eq!(parts_of(card(&scene), mask)[0].name, "Linear Gradient");
}
