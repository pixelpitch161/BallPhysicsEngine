use super::*;
use crate::app::LeftPanel;
use bpe_core::Settings;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

/// A scene with a ball and a shape, plus a spot light (which gives the scene its Lighting card).
fn lit_studio() -> (Studio, ThingId) {
    let mut studio = Studio::headless();
    let reg = &studio.kit.clone().registry;
    let mut scene = studio.scene.clone();
    let id = bpe_kit::things::light::add_light(&mut scene, reg, LightType::Spot, ThingId::ROOT).unwrap();
    scene.rename_thing(id, "Torch");
    // A fresh history: the light is part of the scene opened.
    studio.replace_scene(scene, None);
    (studio, id)
}

/// The left panel on the Lights tab beside an empty middle; each frame's change is recorded as
/// the editor does.
fn harness(mut studio: Studio) -> Harness<'static, Studio> {
    studio.left = LeftPanel::Lights;
    let mut h = Harness::builder()
        .with_size(egui::vec2(1200.0, 900.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(120)
        .allow_missing_glyphs()
        .with_context_setup(style::apply)
        .build_ui_state(
            |ui, studio: &mut Studio| {
                let before = studio.scene.clone();
                studio.recorded = false;
                studio.left_panel(ui, &before);
                egui::CentralPanel::default().show(ui, |_| {});
                if !std::mem::take(&mut studio.recorded) {
                    studio.after_edit(before);
                }
            },
            studio,
        );
    h.run();
    h
}

#[test]
fn every_light_and_ambient_get_a_strip() {
    let (studio, id) = lit_studio();
    let list = sources(&studio.scene, &studio.kit.registry);
    assert_eq!(list.iter().map(|s| s.source).collect::<Vec<_>>(), [LightSource::Light(id), LightSource::Ambient]);
    assert_eq!(list[0].name, "Torch");
    assert_eq!(list[0].what, "spot");
    assert_eq!(list[0].strength, Some(Address::own(id, "strength")));
}

#[test]
fn a_rider_says_what_it_rides_and_a_folder_groups_its_lights() {
    let mut studio = Studio::headless();
    let reg = &studio.kit.clone().registry;
    let ball = studio.scene.order.iter().copied().find(|i| studio.scene.things[i].kind == "balls").unwrap();
    let rider = bpe_kit::things::light::add_light(&mut studio.scene, reg, LightType::Point, ball).unwrap();
    let folder = bpe_kit::add_thing(&mut studio.scene, reg, "folder", ThingId::ROOT).unwrap();
    studio.scene.rename_thing(folder, "Warm side");
    let lamp = bpe_kit::things::light::add_light(&mut studio.scene, reg, LightType::Sun, folder).unwrap();
    let list = sources(&studio.scene, reg);
    let of = |id| list.iter().find(|s| s.source == LightSource::Light(id)).unwrap();
    assert_eq!(of(rider).what, "point · on Ball");
    assert_eq!(of(rider).group, None);
    assert_eq!(of(lamp).group.as_ref().map(|g| g.name.as_str()), Some("Warm side"));
}

#[test]
fn the_lights_chip_opens_the_tab_with_a_strip_per_light() {
    let (studio, _) = lit_studio();
    let mut h = harness(studio);
    h.state_mut().left = LeftPanel::Things;
    h.run();
    h.get_by_label("Lights").click();
    h.run();
    assert_eq!(h.state().left, LeftPanel::Lights);
    assert!(h.query_all_by_label("Torch strength").next().is_some());
    assert!(h.query_all_by_label("Ambient strength").next().is_some());
    assert!(h.query_all_by_label("Depth").next().is_some() && h.query_all_by_label("Quality").next().is_some());
}

#[test]
fn mute_turns_the_light_off_and_undoes() {
    let (studio, id) = lit_studio();
    let mut h = harness(studio);
    h.get_by_label("Torch mute").click();
    h.run();
    assert!(!h.state().scene.things[&id].enabled, "muted is saved: the thing is off");
    assert!(h.state_mut().undo());
    assert!(h.state().scene.things[&id].enabled);
}

#[test]
fn ambient_mute_sets_ambient_on() {
    let (studio, _) = lit_studio();
    let mut h = harness(studio);
    h.get_by_label("Ambient mute").click();
    h.run();
    let card = h.state().scene.card(ThingId::ROOT, SlotId::Lighting).unwrap();
    assert_eq!(card.params.get("ambient_on"), Some(&Value::Bool(false)));
}

#[test]
fn solo_is_the_editors_alone_and_leaves_the_others_out_of_the_picture() {
    let (mut studio, id) = lit_studio();
    let reg = &studio.kit.clone().registry;
    let other = bpe_kit::things::light::add_light(&mut studio.scene, reg, LightType::Point, ThingId::ROOT).unwrap();
    studio.after_edit(studio.saved.clone());
    let scene_before = studio.scene.clone();
    let mut h = harness(studio);
    h.get_by_label("Torch solo").click();
    h.run();
    let st = h.state();
    assert_eq!(st.scene, scene_before, "solo never touches the scene");
    assert!(st.light_solo.contains(&LightSource::Light(id)));
    let copy = soloed(&st.scene, &st.resolved, &st.light_solo).expect("something soloed");
    assert!(copy.things.contains_key(&id));
    assert!(!copy.things.contains_key(&other), "the other light is left out");
    let lighting = &copy.things[&ThingId::ROOT].slots[&SlotId::Lighting];
    assert_eq!(lighting.params.get("ambient_on"), Some(&Value::Bool(false)), "ambient is off unless soloed");
    assert!(st.resolved.things.contains_key(&other), "the editor's own resolved scene is untouched");
}

#[test]
fn nothing_soloed_draws_the_scene_as_it_is() {
    let (studio, _) = lit_studio();
    assert!(soloed(&studio.scene, &studio.resolved, &BTreeSet::new()).is_none());
    let gone = BTreeSet::from([LightSource::Light(ThingId(999))]);
    assert!(soloed(&studio.scene, &studio.resolved, &gone).is_none(), "a light no longer there solos nothing");
}

/// A mouse click at `at`.
fn click_at(h: &mut Harness<'static, Studio>, at: egui::Pos2) {
    let press = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.hover_at(at);
    h.run();
    h.event(press(true));
    h.step();
    h.event(press(false));
    h.run();
}

/// Drags the named slider's knob `dx` points sideways.
fn drag_slider(h: &mut Harness<'static, Studio>, name: &str, dx: f32) {
    let rail = h.query_all_by_label(name).find(|n| n.accesskit_node().role() == Role::Slider).expect("the slider");
    let from = rail.rect().center();
    let press = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.hover_at(from);
    h.run();
    h.event(press(from, true));
    h.step();
    for k in 1..=10 {
        h.event(egui::Event::PointerMoved(from + egui::vec2(dx * k as f32 / 10.0, 0.0)));
        h.step();
    }
    h.event(press(from + egui::vec2(dx, 0.0), false));
    h.run();
}

#[test]
fn the_fader_drags_strength_past_its_span_and_keys_it_when_keyed() {
    let (studio, id) = lit_studio();
    let reg = &studio.kit.clone().registry;
    let a = Address::own(id, "strength");
    let mut h = harness(studio);
    // Far past the rail's end: the value keeps going, past the usual span of 0 to 4.
    drag_slider(&mut h, "Torch strength", 900.0);
    let Some(Value::Number(x)) = h.state().scene.value_at(reg, &a) else { panic!("a number") };
    assert!(x > 4.0, "dragged past its span: {x}");
    assert!(h.state_mut().undo());
    assert_eq!(h.state().scene.value_at(reg, &a), Some(Value::Number(1.5)), "undoes in one step");

    // Keyed: a key lands at the playhead and the first key stays.
    let (mut studio, _) = lit_studio();
    studio.scene.set_key(reg, &a, 0.0, Value::Number(1.0)).unwrap();
    studio.time = 1.0;
    let mut h = harness(studio);
    drag_slider(&mut h, "Torch strength", -40.0);
    let at_one = h.state().scene.value_at_time(reg, &a, 1.0);
    assert!(matches!(at_one, Some(Value::Number(x)) if x < 1.0), "{at_one:?}");
    assert_eq!(h.state().scene.value_at_time(reg, &a, 0.0), Some(Value::Number(1.0)), "the first key stays");
}

#[test]
fn clicking_a_strip_selects_its_light() {
    let (studio, id) = lit_studio();
    let mut h = harness(studio);
    let at = h.get_by_label("Torch").rect().center();
    click_at(&mut h, at);
    assert_eq!(h.state().selection.primary, Some(id));
    let at = h.get_by_label("Ambient").rect().center();
    click_at(&mut h, at);
    assert_eq!(h.state().selection.primary, Some(ThingId::ROOT), "Ambient opens Scene settings");
}

#[test]
fn an_unlit_scene_offers_lighting_and_the_add_buttons_light_it() {
    let studio = Studio::headless();
    let mut h = harness(studio);
    assert!(h.query_by_label("Add Lighting").is_some());
    h.get_by_label("+ Spot").click();
    h.run();
    let st = h.state();
    let id = st.selection.primary.expect("the new light is selected");
    assert_eq!(st.scene.things[&id].kind, LIGHT);
    assert_eq!(LightSettings::from_params(&st.scene.things[&id].params).kind, LightType::Spot);
    assert!(st.scene.card(ThingId::ROOT, SlotId::Lighting).is_some(), "the first light lights the scene");
}

#[test]
fn add_lighting_adds_the_card() {
    let studio = Studio::headless();
    let mut h = harness(studio);
    h.get_by_label("Add Lighting").click();
    h.run();
    assert!(h.state().scene.card(ThingId::ROOT, SlotId::Lighting).is_some());
}

#[test]
fn a_folders_lights_are_listed_together_even_past_a_rider_and_a_rig_names_its_lights() {
    let mut studio = Studio::headless();
    let reg = &studio.kit.clone().registry;
    let scene = &mut studio.scene;
    let folder = bpe_kit::add_thing(scene, reg, "folder", ThingId::ROOT).unwrap();
    scene.rename_thing(folder, "Stage");
    let a = bpe_kit::things::light::add_light(scene, reg, LightType::Point, folder).unwrap();
    let balls = bpe_kit::add_thing(scene, reg, "balls", folder).unwrap();
    let rider = bpe_kit::things::light::add_light(scene, reg, LightType::Point, balls).unwrap();
    let c = bpe_kit::things::light::add_light(scene, reg, LightType::Sun, folder).unwrap();
    let outside = bpe_kit::things::light::add_light(scene, reg, LightType::Point, ThingId::ROOT).unwrap();
    let rig = bpe_kit::things::rig::add_rig(scene, reg, balls).unwrap();
    scene.rename_thing(rig, "Front rig");
    let list = sources(scene, reg);
    let at = |id| list.iter().position(|s| s.source == LightSource::Light(id)).unwrap();
    // The folder's three are together, the light outside comes after them.
    assert_eq!((at(a), at(rider), at(c)), (0, 1, 2), "{list:#?}");
    assert!(at(outside) > at(c));
    assert_eq!(list[at(rider)].group.as_ref().map(|g| g.id), Some(folder));
    // A rig's spot rides the balls the rig sits in and is listed under the rig.
    let spot = scene.descendants(rig).into_iter().find(|i| scene.things[i].kind == LIGHT).unwrap();
    let of = &list[at(spot)];
    assert!(of.what.ends_with(&format!("on {}", scene.things[&balls].name)), "{}", of.what);
    let g = of.group.as_ref().unwrap();
    assert!(g.rig);
    assert_eq!(g.name, "Stage › Front rig");
    assert_eq!(light_parent(scene, Some(rig)), rig, "a selected rig takes the new light");
}

#[test]
fn opening_another_scene_ends_the_solo() {
    let (mut studio, id) = lit_studio();
    studio.light_solo.insert(LightSource::Light(id));
    studio.replace_scene(studio.scene.clone(), None);
    assert!(studio.light_solo.is_empty());
}

#[test]
fn a_particles_layer_that_gives_off_light_gets_a_strip() {
    let (mut studio, _) = lit_studio();
    let reg = &studio.kit.clone().registry;
    let id = bpe_kit::things::particles::looks::add_particles(
        &mut studio.scene,
        reg,
        bpe_kit::things::particles::looks::Look::Fireflies,
        ThingId::ROOT,
    )
    .unwrap();
    let list = sources(&studio.scene, reg);
    let strip = list.iter().find(|s| matches!(s.source, LightSource::Layer(t, _, _) if t == id)).unwrap();
    assert_eq!(strip.what, "gives off");
    assert!(strip.strength.is_some(), "its Strength fader is the sub-layer's own");
}

#[test]
fn many_lights_show_a_warning_on_the_lights_tab() {
    assert_eq!(many_lights_warning(crate::app::MANY_LIGHTS), None);
    let words = many_lights_warning(crate::app::MANY_LIGHTS + 1).expect("a warning");
    assert!(words.starts_with(&format!("{} lights are lit.", crate::app::MANY_LIGHTS + 1)), "{words}");
    let (mut studio, _) = lit_studio();
    studio.lighting = Some((120, None));
    let mut h = harness(studio);
    h.run();
    assert!(h.query_by_label_contains("120 lights are lit.").is_some());
    h.state_mut().lighting = Some((3, None));
    h.run();
    assert!(h.query_by_label_contains("lights are lit.").is_none());
}

#[test]
fn double_clicking_the_fader_puts_the_default_strength_back() {
    let (mut studio, id) = lit_studio();
    let reg = &studio.kit.clone().registry;
    let a = Address::own(id, "strength");
    studio.scene.set_at(reg, &a, Value::Number(3.5)).unwrap();
    let default = studio.scene.def_at(reg, &a).unwrap().default.clone();
    let mut h = harness(studio);
    // Near the rail's left end, so the click alone lands far from the default.
    let rail =
        h.query_all_by_label("Torch strength").find(|n| n.accesskit_node().role() == Role::Slider).unwrap().rect();
    let at = egui::pos2(rail.left() + 4.0, rail.center().y);
    h.hover_at(at);
    h.run();
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            h.step();
        }
    }
    h.run();
    assert_eq!(h.state().scene.value_at(reg, &a), Some(default));
    assert!(h.state_mut().undo());
    assert_eq!(h.state().scene.value_at(reg, &a), Some(Value::Number(3.5)), "one undo step");
}
