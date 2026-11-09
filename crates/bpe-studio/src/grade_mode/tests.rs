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
    harness_sized(studio, 2400.0)
}

fn harness_sized(studio: Studio, height: f32) -> Harness<'static, Studio> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(420.0, height))
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

/// The starting card's panel names, in order.
fn start_names() -> Vec<String> {
    ["Basic", "Tone Curve", "HSL / Colour", "Point Colour", "Colour Grading", "Detail", "Effects", "Calibration"]
        .map(String::from)
        .to_vec()
}

/// The panels' names with the first two swapped.
fn first_two(a: &str, b: &str) -> Vec<String> {
    let mut names = start_names();
    names[0] = a.into();
    names[1] = b.into();
    names
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
    let mut want = start_names();
    want.push("Tone Curve 2".into());
    assert_eq!(names(&h.state().scene), want);
    assert!(h.query_all_by_label("Tone Curve 2").next().is_some(), "shown");
}

#[test]
fn the_header_menu_moves_resets_and_removes_panels() {
    let mut h = harness_with(graded_studio());
    let at = header(&h, "Basic");
    right_click(&mut h, at);
    pick(&mut h, "Move down");
    assert_eq!(names(&h.state().scene), first_two("Tone Curve", "Basic"));
    let at = header(&h, "Basic");
    right_click(&mut h, at);
    pick(&mut h, "Reset order");
    assert_eq!(names(&h.state().scene), start_names());
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
    assert_eq!(names(&h.state().scene)[..2], ["Basic", "HSL / Colour"]);
    for name in ["HSL / Colour", "Point Colour", "Colour Grading", "Detail", "Effects", "Calibration"] {
        let id = panel_id(&h.state().scene, name);
        apply(h.state_mut(), vec![cx::GradeEdit::RemovePanel(id)]);
    }
    h.run();
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
    assert_eq!(names(&h.state().scene), first_two("Tone Curve", "Basic"));
    let card = h.state().scene.card(ThingId::ROOT, SlotId::Grade).unwrap();
    let resolved = bpe_core::resolve(&h.state().scene, &h.state().kit.registry);
    let stack = grade::stack(resolved.things[&ThingId::ROOT].slots.get(&SlotId::Grade), None);
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
            ..Default::default()
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
        .build_ui_state(|ui, s: &mut Studio| masks::column(ui, s), graded_studio());
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

fn graded_blank() -> Studio {
    let mut studio = Studio::headless();
    grade::ensure_grade(&mut studio.scene, &studio.kit.registry);
    studio
}

/// The Grade card's value of panel `panel`'s setting `key`.
fn value(h: &Harness<'_, Studio>, panel: &str, key: &str) -> Option<Value> {
    let scene = &h.state().scene;
    let id = panel_id(scene, panel);
    scene.card(ThingId::ROOT, SlotId::Grade)?.params.get(&format!("{id}/{key}")).cloned()
}

fn number(h: &Harness<'_, Studio>, panel: &str, key: &str) -> f64 {
    match value(h, panel, key) {
        Some(Value::Number(x)) => x,
        _ => 0.0,
    }
}

/// The chip (a button) labelled `name` nearest the top of panel... the first one down the column.
fn chip(h: &mut Harness<'_, Studio>, name: &str) {
    let at = h
        .query_all_by_role_and_label(Role::Button, name)
        .min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
        .unwrap_or_else(|| panic!("no {name} chip"))
        .rect()
        .center();
    h.hover_at(at);
    h.run();
    press(h, at, true, egui::PointerButton::Primary);
    h.step();
    press(h, at, false, egui::PointerButton::Primary);
    h.run();
}

fn sliders(h: &Harness<'_, Studio>, name: &str) -> usize {
    h.query_all_by_role_and_label(Role::Slider, name).count()
}

#[test]
fn the_hsl_tabs_show_their_sliders() {
    let mut h = harness_with(graded_blank());
    assert_eq!(sliders(&h, "Magenta"), 1, "Hue shows one row a band");
    assert_eq!(sliders(&h, "Aqua"), 1);
    chip(&mut h, "All");
    assert_eq!(sliders(&h, "Magenta"), 3, "All shows hue, saturation and luminance");
    chip(&mut h, "Luminance");
    assert_eq!(sliders(&h, "Magenta"), 1);
    // A drag on the luminance tab's Blue moves blue_luminance.
    let rect = h.get_all_by_role_and_label(Role::Slider, "Blue").next().unwrap().rect();
    let (start, end) = (rect.center(), egui::pos2(rect.left() + 4.0, rect.center().y));
    h.hover_at(start);
    press(&mut h, start, true, egui::PointerButton::Primary);
    h.step();
    h.event(egui::Event::PointerMoved(end));
    h.step();
    press(&mut h, end, false, egui::PointerButton::Primary);
    h.run();
    assert!(number(&h, "HSL / Colour", "blue_luminance") < -50.0);
    assert_eq!(number(&h, "HSL / Colour", "blue_hue"), 0.0);
}

/// Drags from `from` to `to` in `steps` moves, with `modifiers` held.
fn drag(h: &mut Harness<'_, Studio>, from: Pos2, to: Pos2, modifiers: Modifiers) {
    h.hover_at(from);
    h.event(egui::Event::ModifiersChanged(modifiers));
    h.run();
    h.event(egui::Event::PointerButton { pos: from, button: egui::PointerButton::Primary, pressed: true, modifiers });
    h.step();
    for k in 1..=6 {
        h.event(egui::Event::PointerMoved(from + (to - from) * (k as f32 / 6.0)));
        h.step();
    }
    h.event(egui::Event::PointerButton { pos: to, button: egui::PointerButton::Primary, pressed: false, modifiers });
    h.run();
    h.event(egui::Event::ModifiersChanged(Modifiers::NONE));
    h.run();
}

#[test]
fn a_wheel_drag_tints_its_region_and_a_double_click_puts_it_back() {
    let mut h = harness_with(graded_blank());
    assert!(h.query_all_by_role_and_label(Role::Slider, "Midtones wheel").next().is_some(), "3-way");
    assert!(h.query_all_by_role_and_label(Role::Slider, "Global wheel").next().is_none());
    let rect = h.get_by_role_and_label(Role::Slider, "Shadows wheel").rect();
    // From the middle straight up to the rim: hue 90, saturation 100.
    drag(&mut h, rect.center(), egui::pos2(rect.center().x, rect.top()), Modifiers::NONE);
    let (hue, sat) = (number(&h, "Colour Grading", "shadows_hue"), number(&h, "Colour Grading", "shadows_saturation"));
    assert!((hue - 90.0).abs() < 1.0 && sat > 95.0, "{hue}, {sat}");
    // A double-click on the disc puts it back in the middle.
    let at = rect.center() + egui::vec2(10.0, 10.0);
    h.hover_at(at);
    h.run();
    for _ in 0..2 {
        press(&mut h, at, true, egui::PointerButton::Primary);
        h.step();
        press(&mut h, at, false, egui::PointerButton::Primary);
        h.step();
    }
    h.run();
    assert_eq!(number(&h, "Colour Grading", "shadows_saturation"), 0.0);
    // Shift moves the puck a tenth as far.
    let r = rect.width() / 2.0 - 6.0;
    let from = rect.center();
    drag(&mut h, from, from + egui::vec2(r, 0.0), Modifiers::SHIFT);
    let sat = number(&h, "Colour Grading", "shadows_saturation");
    assert!(sat > 5.0 && sat < 15.0, "{sat}");
}

#[test]
fn one_region_s_view_shows_its_wheel_and_sliders() {
    let mut h = harness_with(graded_blank());
    chip(&mut h, "Global");
    assert!(h.query_all_by_role_and_label(Role::Slider, "Global wheel").next().is_some());
    assert!(h.query_all_by_role_and_label(Role::Slider, "Shadows wheel").next().is_none());
    assert!(sliders(&h, "Balance") == 1 && sliders(&h, "Blending") == 1);
    assert!(h.state().grade.wheel_view.values().any(|v| *v == WheelView::One(3)));
}

#[test]
fn pick_fills_the_first_free_slot_and_remove_gives_it_up() {
    let mut h = harness_with(graded_blank());
    let panel = panel_id(&h.state().scene, "Point Colour");
    assert!(h.query_all_by_role_and_label(Role::Button, "Colour 1").next().is_none());
    h.get_by_role_and_label(Role::Button, "Pick").click();
    h.run();
    assert_eq!(crate::eyedropper::target(&h.ctx), Some(colour::pick_target(panel)));
    crate::eyedropper::deliver(&h.ctx, bpe_core::Color::rgb(30, 90, 200));
    h.run();
    assert_eq!(value(&h, "Point Colour", "pick1_on"), Some(Value::Bool(true)));
    assert_eq!(value(&h, "Point Colour", "pick1_colour"), Some(Value::Color(bpe_core::Color::rgb(30, 90, 200))));
    assert!(h.query_all_by_role_and_label(Role::Button, "Colour 1").next().is_some(), "its swatch");
    assert_eq!(sliders(&h, "Range"), 1, "its sliders");
    let resolved = bpe_core::resolve(&h.state().scene, &h.state().kit.registry);
    let stack = grade::stack(resolved.things[&ThingId::ROOT].slots.get(&SlotId::Grade), None);
    assert!(stack.is_identity(), "a pick that moves nothing changes nothing");
    // Its keys and pulses go with it.
    let hue = cx::at(&format!("{panel}/pick1_hue"));
    let sat = cx::at(&format!("{panel}/pick1_saturation"));
    {
        let Studio { scene, kit, .. } = h.state_mut();
        scene.set_key(&kit.registry, &hue, 0.0, Value::Number(20.0)).unwrap();
        crate::follow::set(scene, &kit.registry, &sat, Some(bpe_core::PulseSource::default()));
    }
    assert!(h.state().scene.pulses.contains_key(&sat));
    h.run();
    h.get_by_role_and_label(Role::Button, "Remove").click();
    h.run();
    assert_eq!(value(&h, "Point Colour", "pick1_on"), None);
    assert_eq!(value(&h, "Point Colour", "pick1_colour"), None);
    assert!(!h.state().scene.tracks.contains_key(&hue) && !h.state().scene.pulses.contains_key(&sat));
    // The next pick takes slot 1 again; a second one goes in slot 2.
    for (i, c) in [bpe_core::Color::rgb(200, 40, 40), bpe_core::Color::rgb(40, 200, 40)].into_iter().enumerate() {
        h.get_by_role_and_label(Role::Button, "Pick").click();
        h.run();
        crate::eyedropper::deliver(&h.ctx, c);
        h.run();
        assert_eq!(value(&h, "Point Colour", &format!("pick{}_colour", i + 1)), Some(Value::Color(c)));
    }
}

#[test]
fn a_lane_whose_label_its_panel_shares_names_its_group() {
    let mut studio = graded_blank();
    let hsl = panel_id(&studio.scene, "HSL / Colour");
    let a = cx::at(&format!("{hsl}/blue_saturation"));
    studio.scene.set_key(&studio.kit.registry, &a, 0.0, Value::Number(10.0)).unwrap();
    let mut groups = crate::timeline::lanes::groups_for(&studio.scene, &studio.kit);
    crate::timeline::grade_lanes_only(&mut groups);
    let lanes: Vec<_> = groups.iter().flat_map(|g| &g.lanes).collect();
    assert_eq!(lanes[0].short, "HSL / Colour · Saturation · Blue");
}

#[test]
fn detail_and_effects_show_their_settings_and_a_drag_sharpens_less_widely() {
    // Tall enough that the Detail panel is on screen.
    let mut h = harness_sized(Studio::headless(), 6000.0);
    for name in ["Texture", "Clarity", "Dehaze", "Radius", "Midpoint", "Roundness", "Feather", "Size"] {
        assert_eq!(sliders(&h, name), 1, "{name}");
    }
    // Only Detail and Effects have an Amount: Detail's, the vignette's, the grain's and the
    // gradient map's.
    assert_eq!(sliders(&h, "Amount"), 4);
    assert!(h.query_all_by_label("Gradient").next().is_some(), "the gradient map's row");
    // A drag on Radius to the left end makes the card with the narrowest sharpening.
    let rect = h.get_all_by_role_and_label(Role::Slider, "Radius").next().unwrap().rect();
    let (start, end) = (rect.center(), egui::pos2(rect.left() - 40.0, rect.center().y));
    h.hover_at(start);
    press(&mut h, start, true, egui::PointerButton::Primary);
    h.step();
    h.event(egui::Event::PointerMoved(end));
    h.step();
    press(&mut h, end, false, egui::PointerButton::Primary);
    h.run();
    assert_eq!(number(&h, "Detail", "radius"), 0.5);
    let resolved = bpe_core::resolve(&h.state().scene, &h.state().kit.registry);
    let stack = grade::stack(resolved.things[&ThingId::ROOT].slots.get(&SlotId::Grade), None);
    assert!(stack.masks[0].panels.iter().any(|p| matches!(p, bpe_gfx::GradePanel::Detail(d) if d.radius == 0.5)));
}

// --- Part 4: masks -------------------------------------------------------------------------

fn masks_harness(studio: Studio) -> Harness<'static, Studio> {
    let mut h = Harness::builder()
        .with_size(egui::vec2(280.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(120)
        .allow_missing_glyphs()
        .with_context_setup(style::apply)
        .build_ui_state(|ui, s: &mut Studio| masks::column(ui, s), studio);
    h.run();
    h
}

fn card(scene: &Scene) -> &bpe_core::Slot {
    scene.card(ThingId::ROOT, SlotId::Grade).unwrap()
}

/// The masks' names, in order.
fn mask_names(scene: &Scene) -> Vec<String> {
    grade::masks_of(card(scene)).iter().map(|l| l.name.clone()).collect()
}

/// A studio with a Radial Gradient mask, selected.
fn masked_studio() -> (Studio, bpe_core::LayerId) {
    let mut studio = Studio::headless();
    apply(&mut studio, vec![cx::GradeEdit::AddMask(grade::RADIAL)]);
    let mask = studio.grade.mask.expect("the new mask is selected");
    (studio, mask)
}

/// The topmost button labelled `name` (a menu's item sits over what is under it).
fn top_button(h: &Harness<'_, Studio>, name: &str) -> Pos2 {
    h.query_all_by_role_and_label(Role::Button, name)
        .min_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
        .unwrap_or_else(|| panic!("no {name} button"))
        .rect()
        .center()
}

fn click_at(h: &mut Harness<'_, Studio>, at: Pos2) {
    h.hover_at(at);
    h.run();
    press(h, at, true, egui::PointerButton::Primary);
    h.step();
    press(h, at, false, egui::PointerButton::Primary);
    h.run();
}

#[test]
fn add_mask_makes_a_mask_with_its_part_and_a_basic_panel_and_selects_it() {
    let mut h = masks_harness(Studio::headless());
    h.get_by_label("Add mask").click();
    h.run();
    pick(&mut h, "Radial Gradient");
    let scene = &h.state().scene;
    let masks = grade::masks_of(card(scene));
    assert_eq!(masks.len(), 1);
    let mask = masks[0].id;
    assert_eq!(h.state().grade.mask, Some(mask), "selected");
    let parts = grade::parts_of(card(scene), mask);
    assert_eq!(parts.iter().map(|p| p.kind.as_str()).collect::<Vec<_>>(), [grade::RADIAL]);
    let panels = grade::panels_of(card(scene), mask);
    assert_eq!(panels.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Basic"]);
    let name = masks[0].name.clone();
    assert!(h.query_all_by_label(&format!("{name} shown")).next().is_some(), "its eye");
    // The eye hides it (the layer is switched off, its settings kept).
    h.get_by_label(&format!("{name} shown")).click();
    h.run();
    assert!(!grade::masks_of(card(&h.state().scene))[0].enabled);
}

#[test]
fn the_mask_menu_renames_duplicates_moves_and_deletes() {
    let (mut studio, first) = masked_studio();
    apply(&mut studio, vec![cx::GradeEdit::AddMask(grade::LINEAR)]);
    let mut h = masks_harness(studio);
    let names = mask_names(&h.state().scene);
    assert_eq!(names.len(), 2);
    // Rename types in place; Enter keeps it.
    let at = top_button(&h, &names[0]);
    right_click(&mut h, at);
    pick(&mut h, "Rename");
    assert_eq!(h.state().grade.renaming.as_ref().map(|r| r.0), Some(first));
    h.state_mut().grade.renaming = Some((first, "Sun".into()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(mask_names(&h.state().scene)[0], "Sun");
    // Move down puts it after the other.
    let at = top_button(&h, "Sun");
    right_click(&mut h, at);
    pick(&mut h, "Move down");
    assert_eq!(mask_names(&h.state().scene), [names[1].clone(), "Sun".into()]);
    // Duplicate copies it just after, with its part and panel, and selects the copy.
    let at = top_button(&h, "Sun");
    right_click(&mut h, at);
    pick(&mut h, "Duplicate");
    let scene = &h.state().scene;
    let masks = grade::masks_of(card(scene));
    assert_eq!(masks.len(), 3);
    let copy = masks[2].id;
    assert_eq!(h.state().grade.mask, Some(copy));
    assert_eq!(grade::parts_of(card(scene), copy).len(), 1);
    assert_eq!(grade::panels_of(card(scene), copy).len(), 1);
    // Delete takes it with its parts and panels.
    let name = masks[2].name.clone();
    let at = h
        .query_all_by_role_and_label(Role::Button, &name)
        .max_by(|a, b| a.rect().top().total_cmp(&b.rect().top()))
        .unwrap()
        .rect()
        .center();
    right_click(&mut h, at);
    pick(&mut h, "Delete");
    let card = card(&h.state().scene);
    assert_eq!(grade::masks_of(card).len(), 2);
    assert!(card.layers.iter().all(|l| l.id != copy && l.parent != Some(copy)), "its parts and panels went");
    assert_eq!(h.state().grade.mask, None, "the Whole picture shows");
}

#[test]
fn a_masks_column_shows_its_parts_and_add_part_joins_another() {
    let (studio, mask) = masked_studio();
    let mut h = harness_with(studio);
    assert!(h.query_all_by_label("Parts").next().is_some());
    assert!(h.query_all_by_role_and_label(Role::Slider, "Amount").next().is_some(), "the mask's Amount");
    assert!(h.query_all_by_label("Radial Gradient").next().is_some(), "the part's box");
    assert!(h.query_all_by_role_and_label(Role::Slider, "Width").next().is_some());
    assert!(h.query_all_by_role_and_label(Role::Button, "Add").next().is_none(), "the first part has no Combine chip");
    assert!(h.query_all_by_label("Basic").next().is_some(), "its panel");
    // Add part, Subtract, Linear Gradient.
    h.get_by_label("Add part").click();
    h.run();
    let at = top_button(&h, "Subtract");
    click_at(&mut h, at);
    let at = top_button(&h, "Linear Gradient");
    click_at(&mut h, at);
    let scene = &h.state().scene;
    let parts = grade::parts_of(card(scene), mask);
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[1].kind, grade::LINEAR);
    let combine = card(scene).params.get(&format!("{}/combine", parts[1].id)).cloned();
    assert_eq!(combine, Some(Value::Choice("subtract".into())));
    // The second part's header shows how it joins.
    assert!(h.query_all_by_role_and_label(Role::Button, "Subtract").next().is_some());
    // A click on its name shows its handles.
    let linear = parts[1].id;
    let at = top_button(&h, "Linear Gradient");
    click_at(&mut h, at);
    assert_eq!(h.state().grade.part.get(&mask), Some(&linear));
}

#[test]
fn the_overlay_shows_only_on_the_selected_mask() {
    let (mut studio, mask) = masked_studio();
    apply(&mut studio, vec![cx::GradeEdit::AddMask(grade::LINEAR)]);
    studio.grade.mask = Some(mask);
    let resolved = bpe_core::resolve(&studio.scene, &studio.kit.registry);
    let stack = grade::stack(resolved.things[&ThingId::ROOT].slots.get(&SlotId::Grade), None);
    assert_eq!(stack.masks.len(), 3, "the Whole picture and two masks");
    let mut shown = stack.clone();
    studio.grade.show_on(&mut shown);
    assert_eq!(shown, stack, "no overlay until O");
    studio.grade.overlay = true;
    let mut shown = stack.clone();
    studio.grade.show_on(&mut shown);
    let overlays: Vec<(u32, bool)> = shown.masks.iter().map(|m| (m.id, m.overlay)).collect();
    assert_eq!(overlays.iter().filter(|(_, o)| *o).collect::<Vec<_>>(), [&(mask.0, true)]);
    assert_eq!(*studio.grade.shown.borrow(), stack, "the handles read the stack as drawn");
}
