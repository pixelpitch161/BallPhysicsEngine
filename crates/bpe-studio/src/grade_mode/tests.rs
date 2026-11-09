use super::*;
use crate::app::Studio;
use bpe_core::{Scene, SlotId, ThingId, Value};
use bpe_kit::modules::grade;
use eframe::egui::{Modifiers, Pos2, accesskit::Role};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

fn harness_with(studio: Studio) -> Harness<'static, Studio> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(420.0, 2400.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(120)
        .allow_missing_glyphs()
        .with_context_setup(style::apply)
        .build_ui_state(|ui, s: &mut Studio| panels::column(ui, s), studio);
    h.run();
    h
}

fn graded_studio() -> Studio {
    let mut studio = Studio::headless();
    studio.scene = bpe_kit::scenes::graded_sunset(&studio.kit.registry);
    studio
}

/// The panels' names, in order.
fn names(scene: &Scene) -> Vec<String> {
    let card = scene.card(ThingId::ROOT, SlotId::Grade).unwrap();
    grade::panels_of(card, grade::whole_of(card).unwrap()).iter().map(|l| l.name.clone()).collect()
}

fn panel_id(scene: &Scene, name: &str) -> bpe_core::LayerId {
    let card = scene.card(ThingId::ROOT, SlotId::Grade).unwrap();
    card.layers.iter().find(|l| l.name == name).unwrap().id
}

fn press(h: &mut Harness<'_, Studio>, pos: Pos2, pressed: bool, button: egui::PointerButton) {
    h.event(egui::Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE });
}

/// The header label of panel `name`.
fn header(h: &Harness<'_, Studio>, name: &str) -> Pos2 {
    h.query_all_by_label(name).find(|n| n.accesskit_node().role() == Role::Button).expect("the header").rect().center()
}

fn right_click(h: &mut Harness<'_, Studio>, at: Pos2) {
    h.hover_at(at);
    h.run();
    press(h, at, true, egui::PointerButton::Secondary);
    h.step();
    press(h, at, false, egui::PointerButton::Secondary);
    h.run();
}

fn pick(h: &mut Harness<'_, Studio>, button: &str) {
    h.get_by_role_and_label(Role::Button, button).click();
    h.run();
}

#[test]
fn the_column_shows_the_starting_panels_and_the_first_edit_makes_the_card() {
    let mut h = harness_with(Studio::headless());
    assert!(h.query_all_by_label("Whole picture").next().is_some());
    for panel in ["Basic", "Tone Curve"] {
        assert!(h.query_all_by_label(panel).next().is_some(), "{panel}");
    }
    for setting in ["Temp", "Exposure", "Vibrance", "Lights"] {
        assert!(h.query_all_by_role_and_label(Role::Slider, setting).next().is_some(), "{setting}");
    }
    assert!(h.query_all_by_label("Tone curve").next().is_some(), "the curve's graph");
    assert!(h.state().scene.card(ThingId::ROOT, SlotId::Grade).is_none(), "looking changes nothing");
    // A drag on Exposure gives the scene its card, at the template's ids.
    let rect = h.get_all_by_role_and_label(Role::Slider, "Exposure").next().unwrap().rect();
    let (start, end) = (rect.center(), egui::pos2(rect.right() - 4.0, rect.center().y));
    h.hover_at(start);
    press(&mut h, start, true, egui::PointerButton::Primary);
    h.step();
    h.event(egui::Event::PointerMoved(end));
    h.step();
    press(&mut h, end, false, egui::PointerButton::Primary);
    h.run();
    let scene = &h.state().scene;
    let basic = panel_id(scene, "Basic");
    let card = scene.card(ThingId::ROOT, SlotId::Grade).expect("made by the edit");
    assert_eq!(
        *card.layers.iter().map(|l| l.id).collect::<Vec<_>>(),
        *grade::starting_card(&h.state().kit.registry).layers.iter().map(|l| l.id).collect::<Vec<_>>()
    );
    assert!(
        matches!(card.params.get(&format!("{basic}/exposure")), Some(Value::Number(x)) if *x > 0.5),
        "{:?}",
        card.params
    );
}

#[test]
fn add_panel_adds_a_numbered_copy_at_the_end() {
    let mut h = harness_with(Studio::headless());
    h.get_by_label("Add panel").click();
    h.run();
    // The menu's item is the "Tone Curve" button nearest the top (the panel's header is lower).
    let item = h
        .query_all_by_role_and_label(Role::Button, "Tone Curve")
        .min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
        .unwrap();
    item.click();
    h.run();
    assert_eq!(names(&h.state().scene), ["Basic", "Tone Curve", "Tone Curve 2"]);
    assert!(h.query_all_by_label("Tone Curve 2").next().is_some(), "shown");
}

#[test]
fn the_header_menu_moves_resets_and_removes_panels() {
    let mut h = harness_with(graded_studio());
    let at = header(&h, "Basic");
    right_click(&mut h, at);
    pick(&mut h, "Move down");
    assert_eq!(names(&h.state().scene), ["Tone Curve", "Basic"]);
    let at = header(&h, "Basic");
    right_click(&mut h, at);
    pick(&mut h, "Reset order");
    assert_eq!(names(&h.state().scene), ["Basic", "Tone Curve"]);
    // Reset panel takes its values and keys away.
    let basic = panel_id(&h.state().scene, "Basic");
    let at = header(&h, "Basic");
    right_click(&mut h, at);
    pick(&mut h, "Reset panel");
    let scene = &h.state().scene;
    let card = scene.card(ThingId::ROOT, SlotId::Grade).unwrap();
    assert!(card.params.0.keys().all(|k| !k.starts_with(&format!("{basic}/"))), "{:?}", card.params);
    assert!(scene.tracks.keys().all(|a| a.slot != Some(SlotId::Grade)), "the vibrance keys went");
    // The last panel stays.
    let at = header(&h, "Tone Curve");
    right_click(&mut h, at);
    pick(&mut h, "Remove panel");
    assert_eq!(names(&h.state().scene), ["Basic"]);
    let at = header(&h, "Basic");
    right_click(&mut h, at);
    let remove = h.get_by_role_and_label(Role::Button, "Remove panel");
    assert!(remove.accesskit_node().is_disabled(), "the last panel can't go");
}

#[test]
fn dragging_a_header_changes_when_its_panel_works() {
    let mut h = harness_with(graded_studio());
    let from = header(&h, "Tone Curve");
    let to = header(&h, "Basic") - egui::vec2(0.0, 20.0);
    h.hover_at(from);
    h.run();
    press(&mut h, from, true, egui::PointerButton::Primary);
    h.step();
    for k in 1..=8 {
        h.event(egui::Event::PointerMoved(from + (to - from) * (k as f32 / 8.0)));
        h.step();
    }
    press(&mut h, to, false, egui::PointerButton::Primary);
    h.run();
    assert_eq!(names(&h.state().scene), ["Tone Curve", "Basic"]);
    let card = h.state().scene.card(ThingId::ROOT, SlotId::Grade).unwrap();
    let resolved = bpe_core::resolve(&h.state().scene, &h.state().kit.registry);
    let stack = grade::stack(resolved.things[&ThingId::ROOT].slots.get(&SlotId::Grade));
    assert!(matches!(stack.masks[0].panels[0], bpe_gfx::GradePanel::ToneCurve(_)), "{card:?}");
}

#[test]
fn a_double_click_puts_a_setting_back_and_the_switch_turns_a_panel_off() {
    let studio = graded_studio();
    let basic = panel_id(&studio.scene, "Basic");
    let mut h = harness_with(studio);
    let at = h.get_all_by_role_and_label(Role::Slider, "Temp").next().unwrap().rect().center();
    h.hover_at(at);
    h.run();
    for _ in 0..2 {
        press(&mut h, at, true, egui::PointerButton::Primary);
        h.step();
        press(&mut h, at, false, egui::PointerButton::Primary);
        h.step();
    }
    h.run();
    let card = h.state().scene.card(ThingId::ROOT, SlotId::Grade).unwrap();
    assert_eq!(card.params.get(&format!("{basic}/temp")).cloned().unwrap_or(Value::Number(0.0)), Value::Number(0.0));
    h.get_by_role_and_label(Role::CheckBox, "Basic on").click();
    h.run();
    let card = h.state().scene.card(ThingId::ROOT, SlotId::Grade).unwrap();
    assert!(!card.layer(basic).unwrap().enabled);
}

#[test]
fn a_click_on_the_point_curve_adds_a_point() {
    let mut studio = Studio::headless();
    grade::ensure_grade(&mut studio.scene, &studio.kit.registry);
    let curve = panel_id(&studio.scene, "Tone Curve");
    studio
        .scene
        .card_mut(ThingId::ROOT, SlotId::Grade)
        .unwrap()
        .params
        .set(&format!("{curve}/mode"), Value::Choice("point".into()));
    let mut h = harness_with(studio);
    let rect = h.get_by_label("Tone curve").rect();
    let at = egui::pos2(rect.left() + rect.width() * 0.5, rect.top() + rect.height() * 0.3);
    h.hover_at(at);
    h.run();
    press(&mut h, at, true, egui::PointerButton::Primary);
    h.step();
    press(&mut h, at, false, egui::PointerButton::Primary);
    h.run();
    let card = h.state().scene.card(ThingId::ROOT, SlotId::Grade).unwrap();
    let Some(Value::Curve(points)) = card.params.get(&format!("{curve}/rgb")) else { panic!("{:?}", card.params) };
    assert_eq!(points.len(), 3);
    assert!((points[1].x - 0.5).abs() < 0.02 && (points[1].y - 0.7).abs() < 0.03, "{points:?}");
    // The channel chips pick the curve shown; points go on that one.
    pick(&mut h, "Red");
    assert_eq!(h.state().grade.channel.get(&curve), Some(&Channel::Red));
}

#[test]
fn before_and_split_change_only_the_preview_s_grade() {
    let mut mode = GradeMode::default();
    let mut stack = bpe_gfx::GradeStack {
        masks: vec![bpe_gfx::GradeMask {
            amount: 1.0,
            panels: vec![bpe_gfx::GradePanel::Basic(bpe_gfx::GradeBasic { exposure: 1.0, ..Default::default() })],
        }],
        split: None,
    };
    let graded = stack.clone();
    mode.show_on(&mut stack);
    assert_eq!(stack, graded, "After shows the grade");
    mode.toggle_split();
    mode.split = 0.3;
    mode.show_on(&mut stack);
    assert_eq!(stack.split, Some(0.3));
    mode.toggle_before();
    assert_eq!(mode.compare, Compare::Before);
    mode.show_on(&mut stack);
    assert!(stack.is_identity());
    mode.toggle_before();
    assert_eq!(mode.compare, Compare::After);
}

#[test]
fn the_masks_list_has_the_whole_picture_and_its_eye_turns_the_grade_off() {
    let mut h = Harness::builder()
        .with_size(egui::vec2(280.0, 600.0))
        .allow_missing_glyphs()
        .with_context_setup(style::apply)
        .build_ui_state(|ui, s: &mut Studio| masks(ui, s), graded_studio());
    h.run();
    assert!(h.query_all_by_label("Whole picture").next().is_some());
    assert!(h.query_all_by_label("1 panel changed").next().is_some());
    assert!(h.query_all_by_role_and_label(Role::Image, "Histogram").next().is_some());
    h.get_by_label("Grade on").click();
    h.run();
    let card = h.state().scene.card(ThingId::ROOT, SlotId::Grade).unwrap();
    assert_eq!(card.params.get("on"), Some(&Value::Bool(false)));
}

#[test]
fn a_dropped_header_lands_between_the_others() {
    let r = |y: f32| egui::Rect::from_min_size(egui::pos2(0.0, y), egui::vec2(100.0, 20.0));
    let (a, b, c) = (bpe_core::LayerId(1), bpe_core::LayerId(2), bpe_core::LayerId(3));
    let headers = [(a, r(0.0)), (b, r(100.0)), (c, r(200.0))];
    assert_eq!(panels::drop_index(&headers, c, -5.0), 0);
    assert_eq!(panels::drop_index(&headers, c, 50.0), 1);
    assert_eq!(panels::drop_index(&headers, c, 150.0), 2, "where it was");
    assert_eq!(panels::drop_index(&headers, a, 250.0), 2);
    assert_eq!(panels::drop_index(&headers, b, 50.0), 1, "back where it was");
}

#[test]
fn grade_mode_s_timeline_keeps_only_the_grade_s_lanes() {
    let studio = graded_studio();
    let mut scene = studio.scene.clone();
    let exposure = bpe_core::Address::own(ThingId::ROOT, "exposure");
    scene.set_key(&studio.kit.registry, &exposure, 0.0, Value::Number(1.0)).unwrap();
    let mut groups = crate::timeline::lanes::groups_for(&scene, &studio.kit);
    assert!(groups.iter().flat_map(|g| &g.lanes).any(|l| l.address == exposure));
    crate::timeline::grade_lanes_only(&mut groups);
    let lanes: Vec<_> = groups.iter().flat_map(|g| &g.lanes).collect();
    assert_eq!(lanes.len(), 1, "{lanes:?}");
    assert_eq!(lanes[0].address.slot, Some(SlotId::Grade));
    assert_eq!(lanes[0].short, "Basic · Vibrance");
}

#[test]
fn choosing_point_is_no_change_and_reset_panel_keeps_it() {
    let mut studio = Studio::headless();
    grade::ensure_grade(&mut studio.scene, &studio.kit.registry);
    let curve = panel_id(&studio.scene, "Tone Curve");
    let edits = vec![cx::GradeEdit::Set(format!("{curve}/mode"), Value::Choice("point".into()))];
    apply(&mut studio, edits);
    let start = studio.grade.start(&studio.kit.registry);
    let card = cx::card_or_start(&studio.scene, &start);
    let gx = cx::GradeCx { scene: &studio.scene, kit: &studio.kit, card, time: 0.0 };
    assert!(!gx.changed(curve));
    apply(
        &mut studio,
        vec![cx::GradeEdit::Set(format!("{curve}/darks"), Value::Number(20.0)), cx::GradeEdit::ResetPanel(curve)],
    );
    let card = studio.scene.card(ThingId::ROOT, SlotId::Grade).unwrap();
    assert_eq!(card.params.get(&format!("{curve}/mode")), Some(&Value::Choice("point".into())));
    assert_eq!(card.params.get(&format!("{curve}/darks")), None);
}
