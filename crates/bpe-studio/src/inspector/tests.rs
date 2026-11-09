use super::*;
use bpe_core::{Color, Resolved, Settings, SlotId, Value, resolve};
use bpe_kit::resolve_scene;
use eframe::egui::{self, Modifiers, PointerButton, accesskit::Role};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use glam::Vec2;

struct State {
    inspector: Inspector,
    scene: Scene,
    kit: Kit,
    resolved: Resolved,
    selection: Selection,
    time: f64,
}

fn harness(selected: Option<ThingId>) -> Harness<'static, State> {
    harness_of(bpe_kit::new_scene(&bpe_kit::registry()), selected)
}

fn harness_of(scene: Scene, selected: Option<ThingId>) -> Harness<'static, State> {
    harness_sized(scene, selected, 4000.0)
}

fn harness_sized(scene: Scene, selected: Option<ThingId>, height: f32) -> Harness<'static, State> {
    let kit = bpe_kit::kit();
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut selection = Selection::default();
    if let Some(id) = selected {
        selection.set(id);
    }
    let state = State { inspector: Inspector::default(), scene, kit, resolved, selection, time: 0.0 };
    let mut h = Harness::builder()
        .with_size(egui::vec2(420.0, height))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(120)
        .allow_missing_glyphs()
        .with_context_setup(crate::style::apply)
        .build_ui_state(
            |ui, s: &mut State| {
                if show(ui, &mut s.inspector, &mut s.scene, &s.kit, &s.resolved, &s.selection, s.time) {
                    s.resolved = resolve_scene(&s.scene, &s.kit.registry);
                }
            },
            state,
        );
    h.run();
    h
}

/// The first slider a setting's name labels (a ball's Size shows on its Balls card and again
/// on its Look's Body).
fn slider_rect(h: &Harness<'_, State>, label: &str) -> egui::Rect {
    h.get_all_by_role_and_label(Role::Slider, label).next().unwrap().rect()
}

/// Where the test ball's Glow layer keeps its Strength.
const BALL_GLOW: &str = "2/strength";

/// Picks the layer named `name` in the Look editor's lists.
fn pick_layer(h: &mut Harness<'_, State>, name: &str) {
    h.get_by_role_and_label(Role::Button, name).click();
    h.run();
}

/// Presses on the slider and lets go at `fraction` of its width.
fn drag_slider(h: &mut Harness<'_, State>, rect: egui::Rect, fraction: f32) {
    let start = rect.center();
    let end = egui::pos2(rect.left() + rect.width() * fraction, rect.center().y);
    h.hover_at(start);
    h.event(egui::Event::PointerButton {
        pos: start,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    h.event(egui::Event::PointerMoved(end));
    h.step();
    h.event(egui::Event::PointerButton {
        pos: end,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
}

/// The settings of the ball's Body layer (the first of its Look).
fn ball_look(s: &State) -> bpe_core::Params {
    let card = &s.scene.things[&ThingId(2)].slots[&SlotId::Look];
    bpe_core::Params(
        card.params.iter().filter_map(|(k, v)| Some((k.strip_prefix("0/")?.to_owned(), v.clone()))).collect(),
    )
}

/// Right-clicks the first setting `label` names, opening its menu.
fn menu_of(h: &mut Harness<'_, State>, label: &str) {
    h.get_all_by_role_and_label(Role::Label, label).next().unwrap().click_secondary();
    h.run();
}

#[test]
fn a_surface_chip_sets_the_surface_and_its_numbers_in_one_edit() {
    let mut h = harness(Some(ThingId(2)));
    h.get_by_role_and_label(Role::Button, "Chrome surface").click();
    h.run();
    let card = &h.state().scene.things[&ThingId(2)].slots[&SlotId::Look];
    assert_eq!(card.params.get("surface"), Some(&Value::Choice("chrome".into())));
    assert_eq!(card.params.get("metal"), Some(&Value::Number(1.0)));
    assert_eq!(card.params.get("roughness"), Some(&Value::Number(0.05)));
    // The numbers stay folded until opened; then they are there to edit, Bulge too on a ball.
    assert!(h.query_all_by_label("Roughness").next().is_none());
    h.get_by_label("Show Surface").click();
    h.run();
    for label in ["Roughness", "Clear coat", "Bends light", "See-through", "Bulge"] {
        assert!(h.query_all_by_label(label).next().is_some(), "{label}");
    }
}

#[test]
fn the_header_colour_button_sets_the_track_colour() {
    let mut h = harness(Some(ThingId(2)));
    h.get_by_label("Track colour").click();
    h.run();
    h.get_by_label("Colour hex").click();
    h.run();
    h.get_by_label("Colour hex").type_text("#12abef");
    h.run();
    assert_eq!(h.state().scene.things[&ThingId(2)].colour, Some(TrackColour([0x12, 0xab, 0xef])));
}

#[test]
fn a_balls_colours_are_one_list() {
    let mut h = harness(Some(ThingId(2)));
    assert!(h.query_by_label("Colour").is_none(), "no separate Colour row");
    h.get_by_label("Add a colour").click();
    h.run();
    let p = ball_look(h.state());
    assert!(matches!(p.get("colours"), Some(Value::Gradient(g)) if g.stops.len() == 2), "{p:?}");
}

#[test]
fn a_palette_from_the_menu_sets_the_colours() {
    let mut h = harness(Some(ThingId(2)));
    h.get_by_label("Colours palette").click();
    h.run();
    h.get_by_label("Ocean").click();
    h.run();
    let ocean = bpe_kit::paint::palettes::built_in().into_iter().find(|p| p.name == "Ocean").unwrap().colours;
    let p = ball_look(h.state());
    assert_eq!(p.get("colours"), Some(&Value::Gradient(ocean)), "{p:?}");
}

#[test]
fn keyed_gradient_edit_writes_a_key() {
    let mut h = harness(Some(ThingId(2)));
    let colours = Address::on(ThingId(2), SlotId::Look, "0/colours");
    let red_blue = bpe_core::Gradient::even(&[Color::rgb(255, 0, 0), Color::rgb(0, 0, 255)]);
    h.state_mut().scene.set_key(&bpe_kit::kit().registry, &colours, 0.0, Value::Gradient(red_blue.clone())).unwrap();
    h.state_mut().time = 1.0;
    h.run();
    h.get_by_label("Colours bar").click();
    h.run();
    let s = h.state();
    let keys = &s.scene.tracks[&colours].keys;
    assert_eq!(keys.len(), 2, "{keys:?}");
    assert_eq!(keys[0].value, Value::Gradient(red_blue), "the first key is left alone");
    assert_eq!(keys[1].time, 1.0);
    let Value::Gradient(g) = &keys[1].value else { panic!("{:?}", keys[1].value) };
    assert_eq!(g.stops.len(), 3, "the key holds the whole new gradient: {g:?}");
    assert!((g.stops[1].at - 0.5).abs() < 0.01, "{g:?}");
    assert!(s.inspector.keyed_edit);
}

#[test]
fn a_paint_folds_under_its_heading() {
    let mut h = harness(Some(ThingId(2)));
    assert!(h.query_by_label("Add a colour").is_some());
    h.get_by_label("Hide Colours").click();
    h.run();
    assert!(h.query_by_label("Add a colour").is_none(), "the paint's rows fold away");
    assert!(h.query_by_role_and_label(Role::Label, "Shape").is_some(), "rows outside the group stay");
    h.get_by_label("Show Colours").click();
    h.run();
    assert!(h.query_by_label("Add a colour").is_some());
}

#[test]
fn no_dice_padlocks_diamonds_or_module_buttons_are_left() {
    let mut h = harness(Some(ThingId(2)));
    for gone in ["Randomize these settings", "Keyframe Size", "Keyframe Glow", "Make own", "Change here"] {
        assert!(h.query_by_label(gone).is_none(), "{gone}");
    }
    menu_of(&mut h, "Size");
    let items: Vec<String> = h
        .get_all_by_role(Role::Button)
        .filter_map(|n| n.accesskit_node().label())
        .filter(|l| ["Copy Value", "Paste Value", "Create Keyframe", "Lock when randomizing"].contains(&l.as_str()))
        .collect();
    assert_eq!(items, ["Copy Value", "Paste Value", "Create Keyframe"]);
}

#[test]
fn every_setting_that_applies_shows_and_a_card_folds_away() {
    let mut h = harness(Some(ThingId(2)));
    // A single ball: its own Launch speed shows, the block's rows and columns don't.
    assert!(h.query_by_role_and_label(Role::Label, "Launch speed").is_some());
    assert!(h.query_by_role_and_label(Role::Label, "Columns").is_none(), "a single ball has no columns");
    assert!(h.query_by_role_and_label(Role::Label, "Friction").is_some(), "settings once under More show too");
    h.get_by_label("Hide Placement").click();
    h.run();
    assert!(h.query_by_role_and_label(Role::Label, "Launch speed").is_none());
    assert!(h.query_by_role_and_label(Role::Label, "Rotation").is_none());
    assert!(h.state().inspector.collapsed.contains(&(ThingId(2), None, "placement")));
    h.get_by_label("Show Placement").click();
    h.run();
    assert!(h.query_by_role_and_label(Role::Label, "Launch speed").is_some());
}

#[test]
fn a_shape_shows_only_what_does_something_for_it() {
    let h = harness(Some(ThingId(1)));
    // The shape's bounce only matters when it takes over; ball-only settings never show.
    assert!(h.query_by_role_and_label(Role::Label, "Takes over").is_some());
    for ball_only in ["Bounciness", "Grow per hit", "Hits other balls", "Pops after", "Flash"] {
        assert!(h.query_by_role_and_label(Role::Label, ball_only).is_none(), "{ball_only} shows on the shape");
    }
    // A circle has a size but no sides or corners; its outline shows once, under the Body.
    assert_eq!(h.query_all_by_role_and_label(Role::Label, "Size").count(), 1);
    assert!(h.query_by_role_and_label(Role::Label, "Sides").is_none());
    assert!(h.query_by_role_and_label(Role::Label, "Shape settings").is_some());
}

#[test]
fn editing_a_slider_writes_through_set_at() {
    let mut h = harness(Some(ThingId(2)));
    let rect = slider_rect(&h, "Size");
    drag_slider(&mut h, rect, 0.9);
    let s = h.state();
    let v = s.scene.things[&ThingId(2)].params.get("radius").cloned();
    assert!(matches!(v, Some(Value::Number(r)) if r > 100.0), "{v:?}");
}

#[test]
fn right_clicking_a_slider_opens_the_menu_and_changes_nothing() {
    let mut h = harness(Some(ThingId(2)));
    let before = h.state().scene.clone();
    let rect = slider_rect(&h, "Size");
    let at = egui::pos2(rect.right() - 4.0, rect.center().y);
    h.hover_at(at);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos: at,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.step();
    }
    h.run();
    assert_eq!(h.state().scene, before, "a right-click never changes a value");
    assert!(h.query_by_label("Create Keyframe").is_some(), "the menu is open");
}

/// The test scene with the starter sound on its shape (thing 1).
fn sounding_scene() -> Scene {
    let reg = bpe_kit::registry();
    let mut scene = bpe_kit::new_scene(&reg);
    bpe_kit::ensure_mixer(&mut scene, &reg);
    let card = bpe_kit::sound::starter_voice(&reg);
    scene.things.get_mut(&ThingId(1)).unwrap().slots.insert(SlotId::Sound, card);
    scene
}

#[test]
fn follow_sound_menu_adds_a_band() {
    use bpe_core::{LayerId, PulseSource, SoundSource};
    let scene = sounding_scene();
    let shape = scene.things[&ThingId(1)].name.clone();
    let mut h = harness_of(scene, Some(ThingId(2)));
    pick_layer(&mut h, "Glow");
    menu_of(&mut h, "Strength");
    h.get_by_label("Follow sound").click();
    h.run();
    h.get_by_label(&shape).click();
    h.run();
    h.get_by_label("ADSR 1").click();
    h.run();
    let glow = Address::on(ThingId(2), SlotId::Look, BALL_GLOW);
    let pulse = &h.state().scene.pulses[&glow];
    assert_eq!(pulse.source, PulseSource::Sound { shape: ThingId(1), source: SoundSource::Modulator(LayerId(0)) });
    assert_eq!(pulse.amount, Value::Number(0.5), "a quarter of Strength's 0..2");
    assert!(h.query_by_label(&format!("Follows {shape} · ADSR 1")).is_some(), "the band shows");
    // Following a bus, then stopping.
    menu_of(&mut h, "Strength");
    h.get_by_label("Follow sound").click();
    h.run();
    h.get_by_label("Bus 1").click();
    h.run();
    let source = h.state().scene.pulses[&glow].source;
    assert!(matches!(source, PulseSource::Sound { source: SoundSource::Bus(1), .. }), "{source:?}");
    menu_of(&mut h, "Strength");
    h.get_by_label("Stop following sound").click();
    h.run();
    assert!(!h.state().scene.pulses.contains_key(&glow));
    assert!(h.query_by_label(&format!("Follows {shape} · ADSR 1")).is_none());
}

#[test]
fn without_a_sounding_shape_follow_sound_is_greyed_out() {
    let mut h = harness(Some(ThingId(2)));
    menu_of(&mut h, "Size");
    assert!(h.get_by_label("Follow sound").accesskit_node().is_disabled());
}

#[test]
fn create_keyframe_puts_a_key_at_the_playhead() {
    let mut h = harness(Some(ThingId(2)));
    h.state_mut().time = 2.0;
    h.run();
    menu_of(&mut h, "Size");
    h.get_by_label("Create Keyframe").click();
    h.run();
    let size = Address::own(ThingId(2), "radius");
    let keys = &h.state().scene.tracks[&size].keys;
    assert_eq!(keys.len(), 1);
    assert_eq!((keys[0].time, &keys[0].value), (2.0, &Value::Number(24.0)));
    // Again at the same moment, the key there is replaced, not doubled.
    menu_of(&mut h, "Size");
    h.get_by_label("Create Keyframe").click();
    h.run();
    assert_eq!(h.state().scene.tracks[&size].keys.len(), 1);
    // A layer's setting keys the same way.
    pick_layer(&mut h, "Glow");
    menu_of(&mut h, "Strength");
    h.get_by_label("Create Keyframe").click();
    h.run();
    assert!(h.state().scene.tracks.contains_key(&Address::on(ThingId(2), SlotId::Look, BALL_GLOW)));
    // Launch speed can't have keys, so its Create Keyframe is greyed out.
    menu_of(&mut h, "Launch speed");
    assert!(h.get_by_label("Create Keyframe").accesskit_node().is_disabled());
}

/// The setting menu's button whose label starts with `start`.
fn menu_button_starting<'a>(h: &'a Harness<'_, State>, start: &str) -> egui_kittest::Node<'a> {
    h.get_all_by_role(Role::Button)
        .find(|n| n.accesskit_node().label().is_some_and(|l| l.starts_with(start)))
        .unwrap_or_else(|| panic!("no {start}"))
}

fn find(h: &mut Harness<'_, State>, text: &str) {
    h.get_by_label(find::FIND).click();
    h.run();
    h.get_by_label(find::FIND).type_text(text);
    h.run();
}

#[test]
fn find_a_setting_shows_every_setting_it_names_on_any_card() {
    let mut h = harness(Some(ThingId(2)));
    find(&mut h, "streng");
    assert!(h.query_all_by_label_contains("Glow").next().is_some(), "the Glow layer's heading");
    assert!(h.query_all_by_label("Strength").next().is_some(), "its Strength row");
    assert!(h.query_by_role_and_label(Role::Label, "Placement").is_none(), "the cards are gone");
    assert!(h.query_by_role_and_label(Role::Button, "Thing").is_none(), "and the jump bar");
    // A found row edits as it does in its card.
    menu_of(&mut h, "Strength");
    h.get_by_label("Create Keyframe").click();
    h.run();
    let keyed: Vec<&Address> = h.state().scene.tracks.keys().collect();
    assert!(
        keyed.iter().any(|a| a.thing == ThingId(2) && a.slot == Some(SlotId::Look) && a.key.ends_with("/strength")),
        "a layer's Strength got a key: {keyed:?}"
    );
    // Escape empties the box.
    find(&mut h, "x");
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().inspector.find.is_empty(), "{:?}", h.state().inspector.find);
    // Nothing found says so; Clear brings the cards back.
    h.state_mut().inspector.find = "zzzz".into();
    h.run();
    assert!(h.query_by_label("No settings match \u{201c}zzzz\u{201d}.").is_some());
    h.get_by_label("Clear").click();
    h.run();
    assert!(h.state().inspector.find.is_empty());
    assert!(h.query_by_role_and_label(Role::Button, "Thing").is_some(), "the jump bar is back");
}

#[test]
fn reset_to_default_and_remove_all_keyframes_are_in_the_setting_menu() {
    let mut h = harness(Some(ThingId(2)));
    let size = Address::own(ThingId(2), "radius");
    let start = h.state().scene.things[&ThingId(2)].params.get("radius").cloned();
    menu_of(&mut h, "Size");
    assert!(menu_button_starting(&h, "Reset to default").accesskit_node().is_disabled(), "at its default");
    assert!(h.get_by_label("Remove all keyframes").accesskit_node().is_disabled(), "no keys yet");
    h.key_press(egui::Key::Escape);
    h.run();
    h.state_mut().scene.things.get_mut(&ThingId(2)).unwrap().params.set("radius", Value::Number(40.0));
    h.run();
    menu_of(&mut h, "Size");
    menu_button_starting(&h, "Reset to default").click();
    h.run();
    assert_eq!(h.state().scene.things[&ThingId(2)].params.get("radius").cloned(), start);
    // Two keys, then all of them go; the size stays as it is at the playhead.
    {
        let s = h.state_mut();
        let reg = &s.kit.registry;
        s.scene.set_key(reg, &size, 0.0, Value::Number(10.0)).unwrap();
        s.scene.set_key(reg, &size, 2.0, Value::Number(30.0)).unwrap();
        s.time = 1.0;
    }
    h.run();
    menu_of(&mut h, "Size");
    h.get_by_label("Remove all keyframes").click();
    h.run();
    assert!(!h.state().scene.tracks.contains_key(&size));
    assert_eq!(h.state().scene.things[&ThingId(2)].params.get("radius"), Some(&Value::Number(20.0)));
}

#[test]
fn a_picture_chosen_for_particles_is_kept_in_the_scene() {
    let reg = bpe_kit::registry();
    let mut scene = bpe_kit::new_scene(&reg);
    let id = bpe_kit::add_thing(&mut scene, &reg, "particles", ThingId::ROOT).unwrap();
    let mut h = harness_of(scene, Some(id));
    let png: std::sync::Arc<[u8]> = b"\x89PNG\r\n\x1a\nrest".to_vec().into();
    let ctx = h.ctx.clone();
    let picked = crate::picture_picker::PictureRequest { name: "Logo".into(), bytes: png.clone() };
    crate::picture_picker::request(&ctx, picked);
    h.run();
    let scene = &h.state().scene;
    assert_eq!(scene.pictures["Logo"].bytes, png);
    assert_eq!(scene.things[&id].params.get("picture"), Some(&Value::Text("Logo".into())));
    // A file that isn't a PNG is refused, and says so.
    let before = h.state().scene.clone();
    let ctx = h.ctx.clone();
    let wrong = crate::picture_picker::PictureRequest { name: "x".into(), bytes: b"GIF89a".to_vec().into() };
    crate::picture_picker::request(&ctx, wrong);
    h.run();
    assert_eq!(h.state().scene, before);
}

#[test]
fn a_copied_value_pastes_into_another_things_setting() {
    let mut h = harness(Some(ThingId(2)));
    {
        let s = h.state_mut();
        let radius = Address::own(ThingId(2), "radius");
        s.scene.set_at(&s.kit.registry, &radius, Value::Number(75.0)).unwrap();
    }
    h.run();
    menu_of(&mut h, "Size");
    h.get_by_label("Copy Value").click();
    h.run();
    // The shape's Size sits under its Look's Body.
    h.state_mut().selection.set(ThingId(1));
    h.run();
    menu_of(&mut h, "Size");
    h.get_by_label("Paste Value (75 px)").click();
    h.run();
    let size = Address::own(ThingId(1), "size");
    assert_eq!(h.state().scene.value_at(&h.state().kit.registry, &size), Some(Value::Number(75.0)));
}

#[test]
fn a_keyed_or_pulsing_setting_is_marked_for_its_label() {
    let reg = bpe_kit::registry();
    let mut scene = bpe_kit::new_scene(&reg);
    let size = Address::own(ThingId(2), "radius");
    let def = bpe_kit::BallsSettings::defs().iter().find(|d| d.key == "radius").unwrap();
    assert_eq!(mark_for(&scene, def, &size), KeyMark { can_key: true, keyed: false, pulsed: false });
    scene.set_key(&reg, &size, 0.0, Value::Number(40.0)).unwrap();
    assert!(mark_for(&scene, def, &size).keyed);
    let speed = bpe_kit::BallsSettings::defs().iter().find(|d| d.key == "velocity").unwrap();
    assert!(!mark_for(&scene, speed, &Address::own(ThingId(2), "velocity")).can_key);
}

#[test]
fn editing_a_keyed_slider_writes_a_key_not_the_base_value() {
    let mut h = harness(Some(ThingId(2)));
    let size = Address::own(ThingId(2), "radius");
    h.state_mut().scene.set_key(&bpe_kit::kit().registry, &size, 0.0, Value::Number(40.0)).unwrap();
    h.state_mut().time = 3.0;
    h.run();
    let rect = slider_rect(&h, "Size");
    drag_slider(&mut h, rect, 0.9);
    let s = h.state();
    assert_eq!(s.scene.things[&ThingId(2)].params.get("radius"), None, "the plain value is left alone");
    let keys = &s.scene.tracks[&size].keys;
    assert_eq!(keys.len(), 2, "{keys:?}");
    assert_eq!(keys[1].time, 3.0);
    assert!(matches!(keys[1].value, Value::Number(r) if r > 100.0), "{:?}", keys[1].value);
    assert!(s.inspector.keyed_edit, "the studio pauses on this, so the drag keys one moment");
}

#[test]
fn a_card_can_be_removed_and_added_back() {
    let mut h = harness(Some(ThingId(2)));
    let glow = Address::on(ThingId(2), SlotId::Look, BALL_GLOW);
    h.state_mut().scene.set_key(&bpe_kit::kit().registry, &glow, 1.0, Value::Number(0.5)).unwrap();
    h.run();
    h.get_by_label("Remove Look").click();
    h.run();
    assert!(!h.state().scene.things[&ThingId(2)].slots.contains_key(&SlotId::Look));
    assert!(!h.state().scene.tracks.contains_key(&glow), "the card's keys go with it");
    h.get_by_label("+ Add card").click();
    h.run();
    h.get_by_label("Look").click();
    h.run();
    let card = &h.state().scene.things[&ThingId(2)].slots[&SlotId::Look];
    assert_eq!((card.kind.as_str(), card.layers.len()), ("look.layers", 1), "a fresh look: its Body alone");
}

#[test]
fn remove_and_add_reach_every_selected_thing() {
    let mut h = harness(Some(ThingId(2)));
    let other = {
        let s = h.state_mut();
        let other = bpe_kit::add_thing(&mut s.scene, &s.kit.registry, "balls", ThingId::ROOT).unwrap();
        s.selection.extra.insert(other);
        s.resolved = resolve(&s.scene, &s.kit.registry);
        other
    };
    h.run();
    h.get_by_label("Remove Look").click();
    h.run();
    let has_look = |h: &Harness<'_, State>, t: ThingId| h.state().scene.things[&t].slots.contains_key(&SlotId::Look);
    assert!(!has_look(&h, ThingId(2)) && !has_look(&h, other));
    h.get_by_label("+ Add card").click();
    h.run();
    h.get_by_label("Look").click();
    h.run();
    assert!(has_look(&h, ThingId(2)) && has_look(&h, other));
}

/// Drags the first `role` control `label` names by `dx` with the left button.
fn drag_by(h: &mut Harness<'_, State>, role: Role, label: &str, dx: f32) {
    let from = h.get_all_by_role_and_label(role, label).next().unwrap().rect().center();
    let press = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
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
fn with_the_grid_snapping_position_and_rotation_land_on_it() {
    let kit = bpe_kit::kit();
    let scene = bpe_kit::new_scene(&kit.registry);
    let resolved = resolve_scene(&scene, &kit.registry);
    let mut selection = Selection::default();
    selection.set(ThingId(2));
    let state = State { inspector: Inspector::default(), scene, kit, resolved, selection, time: 0.0 };
    let mut h = Harness::builder()
        .with_size(egui::vec2(420.0, 4000.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(120)
        .allow_missing_glyphs()
        .with_context_setup(crate::style::apply)
        .build_ui_state(
            |ui, s: &mut State| {
                let snap = crate::setting_snap::SettingSnap { length: 60.0, angle: 15.0 };
                crate::setting_snap::put(ui.ctx(), Some(snap));
                if show(ui, &mut s.inspector, &mut s.scene, &s.kit, &s.resolved, &s.selection, s.time) {
                    s.resolved = resolve_scene(&s.scene, &s.kit.registry);
                }
            },
            state,
        );
    h.run();
    let before = h.state().scene.things[&ThingId(2)].transform;
    drag_by(&mut h, Role::SpinButton, "Position", 80.0);
    let t = h.state().scene.things[&ThingId(2)].transform;
    assert_ne!(t.position.x, before.position.x, "it moved");
    assert_eq!(t.position.x % 60.0, 0.0, "on a grid point: {}", t.position.x);
    drag_by(&mut h, Role::SpinButton, "Rotation", 50.0);
    let degrees = h.state().scene.things[&ThingId(2)].transform.rotation.to_degrees();
    assert!(degrees.abs() > 1.0, "it turned");
    assert!((degrees / 15.0 - (degrees / 15.0).round()).abs() < 1e-3, "on a 15° step: {degrees}");
}

#[test]
fn right_dragging_a_position_box_moves_nothing() {
    let mut h = harness(Some(ThingId(2)));
    let before = h.state().scene.clone();
    let rect = h.get_all_by_role_and_label(Role::SpinButton, "Position").next().unwrap().rect();
    let (from, to) = (rect.center(), rect.center() + egui::vec2(80.0, 0.0));
    h.hover_at(from);
    h.event(egui::Event::PointerButton {
        pos: from,
        button: PointerButton::Secondary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    h.event(egui::Event::PointerMoved(to));
    h.step();
    h.event(egui::Event::PointerButton {
        pos: to,
        button: PointerButton::Secondary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
    assert_eq!(h.state().scene, before);
}

#[test]
fn add_slot_lists_only_slots_the_kind_lacks_and_adds_a_starter() {
    let mut h = harness(Some(ThingId(2)));
    // A new ball has every card a ball starts with: trails and particles are layers of its Look.
    // Only Health (rules part 4) is left to add.
    h.get_by_label("+ Add card").click();
    h.run();
    h.get_by_label("Health").click();
    h.run();
    assert_eq!(h.state().scene.things[&ThingId(2)].slots[&SlotId::Health].kind, "health");
    assert!(h.get_by_label("+ Add card").accesskit_node().is_disabled());
    h.state_mut().scene.things.get_mut(&ThingId(2)).unwrap().slots.remove(&SlotId::Motion);
    h.run();
    h.get_by_label("+ Add card").click();
    h.run();
    assert!(h.query_all_by_label("Trail").next().is_none(), "trails are layers of the Look");
    assert!(h.query_all_by_label("Particles").next().is_none(), "particles are layers of the Look");
    assert!(h.query_all_by_label("Shader").next().is_none(), "a ball's shaders are layers of its Look");
    assert!(h.query_all_by_label("Motion").next().is_none(), "Motion is added as part of Physics");
    // The menu's Physics, not the card's header or the bar's button.
    h.get_all_by_label("Physics").last().expect("the Physics entry").click();
    h.run();
    assert_eq!(h.state().scene.things[&ThingId(2)].slots[&SlotId::Motion].kind, "motion.basic");
}

#[test]
fn a_new_balls_card_edits_straight_away() {
    let mut h = harness(Some(ThingId(2)));
    let before = h.state().scene.things[&ThingId(2)].slots[&SlotId::Motion].params.clone();
    let rect = slider_rect(&h, "Air drag");
    drag_slider(&mut h, rect, 0.9);
    let s = h.state();
    assert_ne!(s.scene.things[&ThingId(2)].slots[&SlotId::Motion].params, before, "the ball's own motion changed");
}

#[test]
fn typing_a_new_name_renames_the_thing() {
    let mut h = harness(Some(ThingId(2)));
    let field = h.get_by_label("Name");
    field.click();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.event(egui::Event::Text("Hero".into()));
    h.run();
    assert_eq!(h.state().scene.things[&ThingId(2)].name, "Hero");
}

#[test]
fn a_mixed_selection_only_edits_things_that_have_the_setting() {
    let mut h = harness(Some(ThingId(2)));
    {
        let s = h.state_mut();
        let c = bpe_kit::add_thing(&mut s.scene, &s.kit.registry, "balls", ThingId::ROOT).unwrap();
        s.selection.extra.extend([ThingId(1), c]);
        s.resolved = resolve(&s.scene, &s.kit.registry);
    }
    h.run();
    assert!(h.query_by_label("Editing 3 things").is_some());
    let rect = slider_rect(&h, "Size");
    drag_slider(&mut h, rect, 0.9);
    let s = h.state();
    let big = |id| matches!(s.scene.things[&id].params.get("radius"), Some(Value::Number(r)) if *r > 100.0);
    assert!(big(ThingId(2)) && big(ThingId(3)));
    assert!(s.scene.things[&ThingId(1)].params.get("radius").is_none(), "the shape's Size is another setting");
}

#[test]
fn a_card_edit_reaches_every_selected_thing_with_that_card() {
    let mut h = harness(Some(ThingId(1)));
    let takes_over = |t| Address::on(t, SlotId::Bounce, "takes_over");
    let (picked, other) = {
        let s = h.state_mut();
        let picked = bpe_kit::add_thing(&mut s.scene, &s.kit.registry, "shape", ThingId::ROOT).unwrap();
        let other = bpe_kit::add_thing(&mut s.scene, &s.kit.registry, "shape", ThingId::ROOT).unwrap();
        for t in [ThingId(1), picked, other] {
            s.scene.set_at(&s.kit.registry, &takes_over(t), Value::Bool(true)).unwrap();
        }
        s.selection.extra.insert(picked);
        s.resolved = resolve(&s.scene, &s.kit.registry);
        (picked, other)
    };
    h.run();
    let rect = slider_rect(&h, "Friction");
    drag_slider(&mut h, rect, 0.9);
    let s = h.state();
    let friction = |t| s.scene.value_at(&s.kit.registry, &Address::on(t, SlotId::Bounce, "friction"));
    assert!(matches!(friction(ThingId(1)), Some(Value::Number(f)) if f > 0.8), "{:?}", friction(ThingId(1)));
    assert_eq!(friction(picked), friction(ThingId(1)), "the other selected shape's bounce changed too");
    assert_ne!(friction(other), friction(ThingId(1)), "a shape that isn't selected is left alone");
}

#[test]
fn a_layer_edit_reaches_only_selected_balls_with_the_same_layer() {
    let mut h = harness(Some(ThingId(2)));
    let (twin, plain) = {
        let s = h.state_mut();
        // A copy has the same layers; a new ball has its Body alone.
        let twin = crate::tree::duplicate(&mut s.scene, &s.kit.registry, ThingId(2)).unwrap();
        let plain = bpe_kit::add_thing(&mut s.scene, &s.kit.registry, "balls", ThingId::ROOT).unwrap();
        s.selection.extra.extend([twin, plain]);
        s.resolved = resolve(&s.scene, &s.kit.registry);
        (twin, plain)
    };
    let plain_look = h.state().scene.card(plain, SlotId::Look).unwrap().clone();
    h.run();
    pick_layer(&mut h, "Glow");
    let rect = slider_rect(&h, "Strength");
    drag_slider(&mut h, rect, 0.9);
    let s = h.state();
    let glow = |t| s.scene.value_at(&s.kit.registry, &Address::on(t, SlotId::Look, BALL_GLOW));
    assert!(matches!(glow(ThingId(2)), Some(Value::Number(g)) if g > 1.0));
    assert_eq!(glow(twin), glow(ThingId(2)), "the copy's Glow changed too");
    assert_eq!(s.scene.card(plain, SlotId::Look), Some(&plain_look), "a ball with no Glow is left alone");
}

#[test]
fn the_attach_rows_ride_and_regroup() {
    // The tree moves things (Move into, or a drag); the Placement card hangs them off it.
    let mut scene = bpe_kit::new_scene(&bpe_kit::registry());
    scene.set_parent(ThingId(2), ThingId(1), Attach::Group).unwrap();
    let mut h = harness_of(scene, Some(ThingId(2)));
    assert_eq!(h.state().scene.things[&ThingId(2)].parent, Some(ThingId(1)));
    h.get_by_label("Rides on it").click();
    h.run();
    let s = h.state();
    assert!(
        matches!(s.scene.things[&ThingId(2)].attach, Attach::Ride { offset, turn_with_parent: true, .. } if offset == Vec2::new(0.0, -200.0)),
        "{:?}",
        s.scene.things[&ThingId(2)].attach
    );
    assert!(h.query_all_by_label("Offset").next().is_some() && h.query_all_by_label("Position").next().is_none());
    assert!(h.query_all_by_label("One ball").next().is_none(), "a shape has no balls to ride");
    h.get_by_label("Grouped").click();
    h.run();
    let s = h.state();
    assert_eq!(s.scene.things[&ThingId(2)].attach, Attach::Group);
    assert_eq!(s.scene.things[&ThingId(2)].transform.position, Vec2::new(0.0, -200.0));
}

#[test]
fn a_rider_on_balls_rides_the_group_or_one_ball_counted_from_one() {
    // The shape rides the balls (as the Placement card's Attach rows set it).
    let mut scene = bpe_kit::new_scene(&bpe_kit::registry());
    let ride = Attach::Ride { offset: Vec2::ZERO, turn_with_parent: true, item: None };
    scene.set_parent(ThingId(1), ThingId(2), ride).unwrap();
    let mut h = harness_of(scene, Some(ThingId(1)));
    assert!(h.query_all_by_label("Ball it follows").next().is_none(), "the whole group: no number");
    h.get_by_label("One ball").click();
    h.run();
    let item = |h: &Harness<'static, State>| match h.state().scene.things[&ThingId(1)].attach {
        Attach::Ride { item, .. } => item,
        Attach::Group => panic!("still riding"),
    };
    assert_eq!(item(&h), Some(0), "ball number 1 is the first ball");
    assert!(h.query_all_by_label("Ball it follows").next().is_some());
    h.get_by_label("The whole group").click();
    h.run();
    assert_eq!(item(&h), None);
}

#[test]
fn a_target_setting_has_a_picker() {
    let mut h = harness(Some(ThingId(2)));
    // The bounce card's "Which balls" target shows once the balls hit each other.
    {
        let s = h.state_mut();
        let hits = Address::on(ThingId(2), SlotId::Bounce, "hit_balls");
        s.scene.set_at(&s.kit.registry, &hits, Value::Bool(true)).unwrap();
        s.resolved = resolve(&s.scene, &s.kit.registry);
    }
    h.run();
    // Other balls starts folded, marked as holding the change.
    h.get_by_label("Other balls (changed)").click();
    h.run();
    // The picker is a button named by its row, reading "Anything" until something is picked.
    h.get_by_role_and_label(Role::Button, "Which balls").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Any").click();
    h.run();
    // The kind, not the ball's own Shape setting.
    h.get_all_by_role_and_label(Role::Button, "Shape").last().unwrap().click();
    h.run();
    let s = h.state();
    let bounce = &s.scene.things[&ThingId(2)].slots[&SlotId::Bounce];
    assert_eq!(bounce.params.get("balls_to_hit"), Some(&Value::Target(bpe_core::Target::Kind("shape".into()))));
}

#[test]
fn scene_settings_fits_the_panel() {
    // Long labels and units once pushed the number boxes past the edge, so the panel grew.
    let h = harness(Some(ThingId::ROOT));
    for n in h.get_all_by_role(Role::SpinButton) {
        assert!(n.rect().right() <= 420.0, "{:?} reaches {}", n.accesskit_node().label(), n.rect().right());
    }
}

#[test]
fn target_settings_offer_the_scene_things_and_tags() {
    let reg = bpe_kit::registry();
    let mut scene = bpe_kit::new_scene(&reg);
    scene.things.get_mut(&ThingId(2)).unwrap().tags.insert("bouncers".into());
    let special = Address::on(ThingId(2), SlotId::Bounce, "special");
    scene.set_at(&reg, &special, Value::Bool(true)).unwrap();
    let mut h = harness_of(scene, Some(ThingId(2)));
    h.get_by_label("Show Against some things").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Those things").click();
    h.run();
    // The Target menu's Thing, not the jump bar's.
    h.get_all_by_label("Thing").last().expect("the Thing submenu").hover();
    h.run();
    // The header's Ball / Group switch is named Ball too.
    assert_eq!(h.query_all_by_label("Ball").count(), 2, "the ball is offered by name");
    h.get_by_label("Tagged").hover();
    h.run();
    h.get_by_label("bouncers").click();
    h.run();
    let target = h.state().scene.things[&ThingId(2)].slots[&SlotId::Bounce].params.get("special_target").cloned();
    assert_eq!(target, Some(Value::Target(bpe_core::Target::Tag("bouncers".into()))));
}

#[test]
fn variation_and_blending_start_folded_and_open_when_clicked() {
    let mut h = harness(Some(ThingId(2)));
    assert!(h.query_all_by_label("Variation").next().is_some(), "the Variation heading");
    assert!(h.query_by_label("Vary hue").is_none(), "Variation starts folded");
    h.get_all_by_label("Variation").next().unwrap().click();
    h.run();
    assert!(h.query_all_by_label("Vary hue").next().is_some(), "a click opens it");
    // It stays open after looking at another thing and coming back.
    h.state_mut().selection.set(ThingId(1));
    h.run();
    h.state_mut().selection.set(ThingId(2));
    h.run();
    assert!(h.query_all_by_label("Vary hue").next().is_some());
    // And a click on the heading folds it again.
    h.get_all_by_label("Variation").next().unwrap().click();
    h.run();
    assert!(h.query_by_label("Vary hue").is_none());
}

#[test]
fn a_folded_section_with_changes_shows_the_dot() {
    let mut h = harness(Some(ThingId(2)));
    assert!(h.query_all_by_label("Variation").next().is_some());
    assert!(h.query_by_label("Variation (changed)").is_none(), "nothing changed yet");
    assert!(h.query_by_label("Changes inside").is_none(), "no dot with defaults");
    h.state_mut().scene.card_mut(ThingId(2), SlotId::Look).unwrap().params.set("0/vary_hue", Value::Number(20.0));
    h.run();
    assert!(h.query_by_label("Variation (changed)").is_some(), "the heading says it holds changes");
    assert!(h.query_by_label("Changes inside").is_some(), "the dot is drawn");
    // Open, the rows show for themselves, so the dot goes.
    h.get_by_label("Variation (changed)").click();
    h.run();
    assert!(h.query_by_label("Changes inside").is_none());
}

#[test]
fn a_folded_section_with_a_keyframe_shows_the_dot() {
    let mut h = harness(Some(ThingId(2)));
    let wobble = Address::on(ThingId(2), SlotId::Look, "0/wobble");
    let reg = bpe_kit::registry();
    let default = h.state().scene.value_at(&reg, &wobble).unwrap();
    h.state_mut().scene.set_key(&reg, &wobble, 0.0, default).unwrap();
    h.run();
    assert!(h.query_by_label("Variation (changed)").is_some(), "a key at the default still moves the video");
    assert!(h.query_by_label("Changes inside").is_some());
}

#[test]
fn a_folded_section_with_only_a_pulse_shows_the_dot() {
    // The ball's Bounce card has a Bounce section; start it at its defaults so it starts clean.
    // The other cards go.
    let mut h = harness(Some(ThingId(2)));
    let s = h.state_mut();
    s.scene.card_mut(ThingId(2), SlotId::Bounce).unwrap().params = bpe_core::Params::default();
    s.scene.remove_slot(ThingId(2), SlotId::Motion);
    s.resolved = resolve(&s.scene, &s.kit.registry);
    h.run();
    h.get_by_label("Hide Bounce").click();
    h.run();
    assert!(h.query_by_label("Bounce (changed)").is_none(), "Bounce holds no changes yet");
    let def = bpe_kit::modules::bounce::BasicBounce::defs().iter().find(|d| d.key == "bounciness").unwrap();
    assert!(def.flags.pulse && def.group == "Bounce");
    let address = Address::on(ThingId(2), SlotId::Bounce, def.key);
    let reg = bpe_kit::registry();
    h.state_mut().scene.set_pulse(&reg, &address, bpe_core::PulseSource::Hits, Value::Number(0.25), 0.25).unwrap();
    h.run();
    assert!(h.query_by_label("Bounce (changed)").is_some(), "a pulse moves the video");
    assert!(h.query_by_label("Changes inside").is_some());
}

#[test]
fn the_header_eye_hides_and_shows_the_thing() {
    let mut h = harness(Some(ThingId(2)));
    assert!(h.state().scene.things[&ThingId(2)].enabled);
    h.get_by_label("Shown").click();
    h.run();
    assert!(!h.state().scene.things[&ThingId(2)].enabled, "the eye hides it");
    h.get_by_label("Shown").click();
    h.run();
    assert!(h.state().scene.things[&ThingId(2)].enabled, "and shows it again");
}

#[test]
fn scene_settings_sum_up_the_grade_and_open_grade_mode() {
    let reg = bpe_kit::registry();
    // No grade yet: the card says so and offers Grade mode.
    let mut h = harness_of(bpe_kit::new_scene(&reg), Some(ThingId::ROOT));
    h.run();
    assert!(h.query_by_role_and_label(Role::Label, "Grade").is_some(), "a Grade card");
    assert!(h.query_all_by_label("No grade yet: the video shows the picture as it is drawn.").next().is_some());
    assert!(h.query_all_by_role_and_label(Role::Slider, "Brightness").next().is_none(), "the old grade is gone");
    h.get_by_label("Open in Grade mode").click();
    h.run();
    assert!(h.state().inspector.open_grade);
    // The graded sunset: on, with its Basic panel changed; the switch turns it off.
    let mut h = harness_of(bpe_kit::scenes::graded_sunset(&reg), Some(ThingId::ROOT));
    h.run();
    assert!(h.query_all_by_label("Changed: Basic.").next().is_some());
    h.query_all_by_role_and_label(Role::CheckBox, "Grade on").next().expect("the switch").click();
    h.run();
    let card = h.state().scene.card(ThingId::ROOT, SlotId::Grade).unwrap();
    assert_eq!(card.params.get("on"), Some(&Value::Bool(false)));
}

/// The kinds of the layers of the scene's Effects card, bottom first.
fn effect_kinds(s: &State) -> Vec<String> {
    s.scene.things[&ThingId::ROOT].slots[&SlotId::Shader].layers.iter().map(|l| l.kind.clone()).collect()
}

#[test]
fn the_effects_card_lists_its_effects_and_adds_them_from_its_menu() {
    let reg = bpe_kit::registry();
    let mut scene = bpe_kit::new_scene(&reg);
    bpe_kit::fill_slot(&mut scene, &reg, ThingId::ROOT, SlotId::Shader).unwrap();
    assert_eq!(scene.things[&ThingId::ROOT].slots[&SlotId::Shader].kind, bpe_kit::modules::effects::EFFECTS);
    let mut h = harness_of(scene, Some(ThingId::ROOT));
    assert!(h.query_by_role_and_label(Role::Button, "Type").is_none(), "one kind of Effects card");
    assert!(h.query_all_by_label_contains("None yet: add an effect").next().is_some());
    h.get_by_role_and_label(Role::Button, "+ Add effect").click();
    h.run();
    h.get_all_by_label("Kaleidoscope").last().unwrap().click();
    h.run();
    assert_eq!(effect_kinds(h.state()), ["picture.kaleidoscope"]);
    // A picture effect takes the whole picture, its light or its glow.
    assert!(h.query_all_by_label("Takes").next().is_some(), "the Takes row");
    assert!(h.query_all_by_label("Mix").next().is_some(), "the Mix row");
    h.get_by_role_and_label(Role::Button, "+ Add effect").click();
    h.run();
    h.get_all_by_label("Lava").last().unwrap().click();
    h.run();
    assert_eq!(effect_kinds(h.state()), ["picture.kaleidoscope", "picture.lava"]);
    // The new one is picked: a material paints the things, and takes nothing.
    let s = h.state();
    let lava = s.scene.things[&ThingId::ROOT].slots[&SlotId::Shader].layers[1].id;
    assert_eq!(s.inspector.selected_layer.get(&ThingId::ROOT), Some(&lava));
    assert!(h.query_all_by_label("Takes").next().is_none(), "no Takes on a material");
}

#[test]
fn the_header_is_name_kind_and_shown_then_tags() {
    let h = harness(Some(ThingId(2)));
    for label in ["Name", "Shown", "Tags"] {
        assert!(h.query_all_by_label(label).next().is_some(), "{label} is in the header");
    }
    for gone in ["Parent", "Kind"] {
        assert!(h.query_by_role_and_label(Role::Label, gone).is_none(), "{gone} is no longer a row");
    }
    // The Placement card's heading sits above its Position row.
    let heading = h.get_by_role_and_label(Role::Label, "Placement").rect();
    let position = h.get_by_role_and_label(Role::Label, "Position").rect();
    assert!(heading.top() < position.top(), "Position is under Placement");
    for row in ["Rotation", "Attach", "Stick to screen"] {
        let r = h.get_by_role_and_label(Role::Label, row).rect();
        assert!(heading.top() < r.top(), "{row} is under Placement");
    }
    // A ball's own settings sit by its Look's Ball / Group switch and under Placement: no
    // Balls card.
    assert!(h.query_by_label("Hide Balls").is_none());
    assert!(h.query_by_label_contains("Balls settings").is_none());
    // Launch speed and Z-order are under Placement.
    for row in ["Launch speed", "Z-order"] {
        let r = h.get_by_role_and_label(Role::Label, row).rect();
        assert!(heading.top() < r.top(), "{row} is under Placement");
    }
}

#[test]
fn the_camera_and_scene_settings_have_no_placement_card() {
    let root = harness(Some(ThingId::ROOT));
    assert!(root.query_by_role_and_label(Role::Label, "Placement").is_none());
    assert!(root.query_by_role_and_label(Role::Label, "Shown").is_none(), "the root has no eye");
    assert!(root.query_by_label("Hide Video").is_some(), "the root's first card keeps its title");
    let reg = bpe_kit::registry();
    let mut scene = bpe_kit::new_scene(&reg);
    let camera = bpe_kit::add_thing(&mut scene, &reg, "camera", ThingId::ROOT).unwrap();
    let h = harness_of(scene, Some(camera));
    assert!(h.query_by_role_and_label(Role::Label, "Placement").is_none());
    assert!(h.query_by_role_and_label(Role::Label, "Stick to screen").is_none());
    assert!(h.query_all_by_label("Shown").next().is_some(), "the camera can still be hidden");
}

#[test]
fn the_placement_card_folds_away() {
    let mut h = harness(Some(ThingId(2)));
    h.get_by_label("Hide Placement").click();
    h.run();
    assert!(h.query_by_role_and_label(Role::Label, "Rotation").is_none());
    assert!(h.state().inspector.collapsed.contains(&(ThingId(2), None, "placement")));
    h.get_by_label("Show Placement").click();
    h.run();
    assert!(h.query_by_role_and_label(Role::Label, "Rotation").is_some());
}

const BAR_LABELS: [&str; 13] = [
    "Thing",
    "Look",
    "Physics",
    "Motion",
    "Bounce",
    "Shader",
    "Effects",
    "Video",
    "World",
    "Background",
    "Picture",
    "Sound",
    "Mixer",
];

/// The jump bar's buttons, in order, each with whether it is selected.
fn jump_bar(h: &Harness<'_, State>) -> Vec<(String, bool)> {
    h.get_all_by_role(Role::Button)
        .filter_map(|n| {
            let node = n.accesskit_node();
            let label = node.label()?;
            let toggled = node.toggled()?;
            BAR_LABELS.contains(&label.as_str()).then(|| (label, toggled == egui::accesskit::Toggled::True))
        })
        .collect()
}

fn the_bar_button<'h>(h: &'h Harness<'_, State>, label: &'h str) -> egui_kittest::Node<'h> {
    h.get_all_by_role_and_label(Role::Button, label)
        .find(|n| n.accesskit_node().toggled().is_some())
        .expect("the bar's button")
}

#[test]
fn slider_columns_line_up_across_cards() {
    let h = harness(Some(ThingId(2)));
    // The Look's Size, under its layer lists, and the Physics card's Gravity.
    let size = h.get_all_by_role_and_label(Role::Slider, "Size").next().unwrap().rect();
    let gravity = slider_rect(&h, "Gravity ×");
    assert!(
        (size.left() - gravity.left()).abs() <= 1.0,
        "Size starts at {}, Gravity at {}",
        size.left(),
        gravity.left()
    );
}

#[test]
fn the_header_and_placement_line_up_with_the_settings() {
    let h = harness(Some(ThingId(2)));
    let label = |l: &str| h.get_all_by_role_and_label(Role::Label, l).next().unwrap().rect().left();
    let size = label("Size");
    for row in ["Name", "Tags", "Position", "Rotation"] {
        assert!((label(row) - size).abs() <= 1.0, "{row}'s label starts at {}, Size's at {size}", label(row));
    }
    let slider = slider_rect(&h, "Size").left();
    let name = h.get_by_role_and_label(Role::TextInput, "Name").rect().left();
    let position = h.get_all_by_role_and_label(Role::SpinButton, "Position").next().unwrap().rect().left();
    let rotation = h.get_by_role_and_label(Role::SpinButton, "Rotation").rect().left();
    for (row, x) in [("Name", name), ("Position", position), ("Rotation", rotation)] {
        assert!((x - slider).abs() <= 1.0, "{row}'s field starts at {x}, Size's slider at {slider}");
    }
}

#[test]
fn the_jump_bar_lists_thing_then_the_slots_in_order() {
    let h = harness_of(bpe_kit::new_scene(&bpe_kit::registry()), Some(ThingId(2)));
    let labels: Vec<String> = jump_bar(&h).into_iter().map(|(l, _)| l).collect();
    assert_eq!(labels, ["Thing", "Look", "Physics"]);
    assert_eq!(jump_bar(&h)[0], ("Thing".to_owned(), true), "at the top, Thing is selected");
}

#[test]
fn tags_can_be_typed_a_letter_at_a_time() {
    let mut h = harness(Some(ThingId(2)));
    h.get_by_role_and_label(Role::TextInput, "Tags").click();
    h.run();
    for letter in "red blue".chars() {
        h.get_by_role_and_label(Role::TextInput, "Tags").type_text(&letter.to_string());
        h.run();
    }
    h.key_press(egui::Key::Enter);
    h.run();
    let tags: Vec<&str> = h.state().scene.things[&ThingId(2)].tags.iter().map(String::as_str).collect();
    assert_eq!(tags, ["blue", "red"]);
}

#[test]
fn a_balls_thing_button_jumps_to_its_placement() {
    let mut h = harness_sized(bpe_kit::new_scene(&bpe_kit::registry()), Some(ThingId(2)), 600.0);
    the_bar_button(&h, "Physics").click();
    h.run();
    the_bar_button(&h, "Thing").click();
    h.run();
    let s = h.state();
    assert!(s.inspector.jump_to.is_none(), "the jump landed on a card that is drawn");
    let tops = &s.inspector.card_tops;
    let top = tops.iter().find(|(c, _)| *c == Card::Placement).expect("Placement's top").1;
    assert!(top.abs() <= 2.0, "Placement's header is at the top of the scroll area, at {top}");
    let selected: Vec<String> = jump_bar(&h).into_iter().filter(|(_, on)| *on).map(|(l, _)| l).collect();
    assert_eq!(selected, ["Thing"]);
}

#[test]
fn the_jump_bar_unfolds_and_scrolls_to_physics() {
    let mut h = harness_sized(bpe_kit::new_scene(&bpe_kit::registry()), Some(ThingId(2)), 600.0);
    h.state_mut().inspector.collapsed.insert(Card::Physics.fold_key(ThingId(2)));
    h.run();
    assert!(h.query_by_label("Show Physics").is_some(), "the Physics card starts folded");
    the_bar_button(&h, "Physics").click();
    h.run();
    let s = h.state();
    let open = !s.inspector.collapsed.contains(&Card::Physics.fold_key(ThingId(2)));
    assert!(open, "the Physics card is open");
    assert!(s.inspector.jump_to.is_none());
    let tops = &s.inspector.card_tops;
    let top = tops.iter().find(|(c, _)| *c == Card::Physics).expect("Physics' top").1;
    assert!(top.abs() <= 2.0, "Physics' header is at the top of the scroll area, at {top}");
    // The header's own rect sits just under the bar, where the scroll area starts.
    let header = h.get_by_label("Hide Physics").rect();
    let bar = the_bar_button(&h, "Thing").rect();
    assert!(header.top() > bar.bottom() && header.top() < bar.bottom() + 60.0, "{header:?} under {bar:?}");
    let selected: Vec<String> = jump_bar(&h).into_iter().filter(|(_, on)| *on).map(|(l, _)| l).collect();
    assert_eq!(selected, ["Physics"], "tops {:?} bar {:?}", h.state().inspector.card_tops, jump_bar(&h));
}

/// Scene settings with every slot it can have, so its cards run well past one screen.
#[test]
fn the_lighting_card_saves_and_deletes_lighting_scenes() {
    let reg = bpe_kit::registry();
    let mut scene = bpe_kit::new_scene(&reg);
    bpe_kit::things::rig::add_rig(&mut scene, &reg, ThingId::ROOT).unwrap();
    let mut h = harness_of(scene, Some(ThingId::ROOT));
    let names =
        |s: &State| bpe_kit::modules::light_scenes::scenes_of(&s.scene).into_iter().map(|s| s.name).collect::<Vec<_>>();
    let save = |h: &mut Harness<'_, State>| {
        let add = h.get_by_label("+ Save the lights as a scene");
        add.scroll_to_me();
        add.click();
        h.run();
    };
    save(&mut h);
    save(&mut h);
    assert_eq!(names(h.state()), ["Scene 1", "Scene 2"]);
    assert!(h.query_by_label("2 scenes").is_some(), "the grid row counts them");
    let menu = h.get_by_label("Scene 1 menu");
    menu.scroll_to_me();
    menu.click();
    h.run();
    h.get_by_label("Delete").click();
    h.run();
    assert_eq!(names(h.state()), ["Scene 2"]);
}

#[test]
fn the_lighting_card_asks_for_a_final_frame() {
    let reg = bpe_kit::registry();
    let mut scene = bpe_kit::new_scene(&reg);
    bpe_kit::things::rig::add_rig(&mut scene, &reg, ThingId::ROOT).unwrap();
    let mut h = harness_of(scene, Some(ThingId::ROOT));
    assert!(!h.state().inspector.final_frame_wanted);
    let button = h.get_by_label("Preview final frame");
    button.scroll_to_me();
    button.click();
    h.run();
    assert!(h.state().inspector.final_frame_wanted, "the app opens the final frame");
}

fn a_root_with_every_slot() -> Scene {
    let reg = bpe_kit::registry();
    let mut scene = bpe_kit::new_scene(&reg);
    bpe_kit::fill_slot(&mut scene, &reg, ThingId::ROOT, SlotId::Shader).unwrap();
    // The mixer is there too, but has a panel of its own, not a card here.
    bpe_kit::ensure_mixer(&mut scene, &reg);
    scene
}

/// The top of a root card's strong title.
fn heading_top(h: &Harness<'_, State>, name: &str) -> f32 {
    h.get_by_role_and_label(Role::Label, name).rect().top()
}

#[test]
fn scene_settings_shows_a_card_per_group() {
    let h = harness(Some(ThingId::ROOT));
    let mut last = f32::NEG_INFINITY;
    for name in bpe_kit::ROOT_CARDS {
        assert!(h.query_by_label(&format!("Hide {name}")).is_some(), "{name} has a fold arrow");
        let top = heading_top(&h, name);
        assert!(top > last, "{name} comes after the card before it");
        last = top;
    }
    let tops: Vec<Card> = h.state().inspector.card_tops.iter().map(|(c, _)| *c).collect();
    assert_eq!(tops, bpe_kit::ROOT_CARDS.map(Card::Section), "the cards in order, and nothing else");
    assert!(h.query_by_label("Hide Scene settings").is_none(), "no single Scene settings card");
}

#[test]
fn the_root_jump_bar_lists_the_cards_then_the_slots() {
    let h = harness(Some(ThingId::ROOT));
    let labels: Vec<String> = jump_bar(&h).into_iter().map(|(l, _)| l).collect();
    assert_eq!(labels, bpe_kit::ROOT_CARDS);
    assert_eq!(jump_bar(&h)[0], ("Video".to_owned(), true), "at the top, Video is selected");
    assert!(h.query_by_role_and_label(Role::Button, "Thing").is_none(), "no Thing button on the root");
    assert!(h.query_by_role_and_label(Role::Button, "Scene settings").is_none());
    // Its slots follow, in the order their cards are drawn.
    let h = harness_of(a_root_with_every_slot(), Some(ThingId::ROOT));
    let labels: Vec<String> = jump_bar(&h).into_iter().map(|(l, _)| l).collect();
    let mut want: Vec<&str> = bpe_kit::ROOT_CARDS.to_vec();
    want.push("Effects");
    assert_eq!(labels, want);
}

#[test]
fn jumping_to_background_scrolls_it_to_the_top() {
    let mut h = harness_sized(a_root_with_every_slot(), Some(ThingId::ROOT), 600.0);
    the_bar_button(&h, "Background").click();
    h.run();
    let s = h.state();
    let tops = &s.inspector.card_tops;
    let top = tops.iter().find(|(c, _)| *c == Card::Section("Background")).expect("Background's top").1;
    assert!(top.abs() <= 1.0, "Background's header is at the top of the scroll area, at {top}");
    assert_eq!(selected_in_bar(&h), ["Background"]);
}

#[test]
fn a_root_card_with_no_shown_rows_still_has_its_title() {
    let kit = bpe_kit::kit();
    let scene = bpe_kit::new_scene(&kit.registry);
    let resolved = resolve_scene(&scene, &kit.registry);
    let state =
        State { inspector: Inspector::default(), scene, kit, resolved, selection: Selection::default(), time: 0.0 };
    let mut h = Harness::builder()
        .with_size(egui::vec2(420.0, 800.0))
        .allow_missing_glyphs()
        .with_context_setup(crate::style::apply)
        .build_ui_state(
            |ui, s: &mut State| {
                let choices = TargetChoices::from_scene(&s.scene, &s.kit.registry);
                let thing = s.scene.things[&ThingId::ROOT].clone();
                let cx = OwnCx {
                    scene: &s.scene,
                    kit: &s.kit,
                    resolved: &s.resolved,
                    choices: &choices,
                    id: ThingId::ROOT,
                    thing: &thing,
                    time: s.time,
                    label_width: label_width(420.0),
                    applies: &|_| false,
                };
                let mut edits = Vec::new();
                root_card(ui, &mut s.inspector, &cx, "World", &mut edits);
            },
            state,
        );
    h.run();
    assert!(h.query_by_label("Hide World").is_some(), "the card keeps its fold arrow");
    assert!(h.query_by_role_and_label(Role::Label, "World").is_some(), "and its title");
    assert!(h.query_by_role_and_label(Role::Label, "Gravity").is_none(), "no rows");
    assert!(h.query_by_role_and_label(Role::Label, "Seed").is_none(), "no rows");
    assert!(h.query_all_by_role(Role::Slider).next().is_none(), "no grid row is drawn");
}

fn selected_in_bar(h: &Harness<'_, State>) -> Vec<String> {
    jump_bar(h).into_iter().filter(|(_, on)| *on).map(|(l, _)| l).collect()
}

fn wheel(h: &mut Harness<'_, State>, dy: f32) {
    h.hover_at(egui::pos2(200.0, 400.0));
    h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, dy),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
        modifiers: Modifiers::NONE,
    });
    h.run();
}

#[test]
fn a_short_last_card_stays_marked_after_jumping_to_it() {
    let mut h = harness_sized(bpe_kit::new_scene(&bpe_kit::registry()), Some(ThingId(2)), 600.0);
    // Physics with its Motion, Bounce and Growing folded is short enough not to reach the top.
    for (slot, group) in [(SlotId::Motion, "Motion"), (SlotId::Bounce, "Bounce"), (SlotId::Bounce, "Growing")] {
        h.state_mut().inspector.collapsed.insert((ThingId(2), Some(slot), group));
    }
    h.run();
    the_bar_button(&h, "Physics").click();
    h.run();
    let tops = &h.state().inspector.card_tops;
    let last = tops.iter().find(|(c, _)| *c == Card::Physics).expect("Physics' top").1;
    assert!(last > 1.0, "Physics is too short to reach the top, at {last}");
    assert_eq!(selected_in_bar(&h), ["Physics"], "the card jumped to stays marked");
    // A scroll hands the mark back to the card at the top.
    wheel(&mut h, 40.0);
    assert_ne!(selected_in_bar(&h), ["Physics"]);
    assert!(h.state().inspector.jumped.is_none());
}

#[test]
fn the_jump_bar_marks_the_card_at_the_top_while_scrolling() {
    let mut h = harness_sized(bpe_kit::new_scene(&bpe_kit::registry()), Some(ThingId(2)), 600.0);
    // Scroll down until Look's header has passed the top but Physics' hasn't.
    for _ in 0..200 {
        let tops = &h.state().inspector.card_tops;
        let top_of = |card| tops.iter().find(|(c, _)| *c == card).map(|(_, t)| *t);
        if top_of(Card::Slot(SlotId::Look)).is_some_and(|t| t < -1.0) {
            break;
        }
        h.hover_at(egui::pos2(200.0, 400.0));
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, -40.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: Modifiers::NONE,
        });
        h.run();
    }
    let tops = &h.state().inspector.card_tops;
    let look = tops.iter().find(|(c, _)| *c == Card::Slot(SlotId::Look)).unwrap().1;
    let physics = tops.iter().find(|(c, _)| *c == Card::Physics).unwrap().1;
    assert!(look < -1.0 && physics > 1.0, "scrolled past Look's header but not Physics': {look}, {physics}");
    let selected: Vec<String> = jump_bar(&h).into_iter().filter(|(_, on)| *on).map(|(l, _)| l).collect();
    assert_eq!(selected, ["Look"]);
}

#[test]
fn a_ball_shows_one_physics_card_with_motion_and_bounce_inside() {
    let h = harness(Some(ThingId(2)));
    let tops = &h.state().inspector.card_tops;
    assert_eq!(tops.iter().filter(|(c, _)| *c == Card::Physics).count(), 1, "one Physics card: {tops:?}");
    assert!(
        !tops.iter().any(|(c, _)| matches!(c, Card::Slot(SlotId::Motion) | Card::Slot(SlotId::Bounce))),
        "no Motion or Bounce card of their own: {tops:?}"
    );
    assert!(h.query_by_label("Hide Physics").is_some(), "the Physics card's fold arrow");
    for row in ["Gravity ×", "Bounciness"] {
        assert!(h.query_all_by_role_and_label(Role::Slider, row).next().is_some(), "{row} shows");
    }
    for heading in ["Hide Motion", "Hide Bounce", "Hide Growing", "Show Other balls", "Show Against some things"] {
        assert!(h.query_by_label(heading).is_some(), "the {heading} heading");
    }
    // Motion's settings read first.
    let gravity = slider_rect(&h, "Gravity ×").top();
    let bounciness = slider_rect(&h, "Bounciness").top();
    assert!(gravity < bounciness, "Gravity × at {gravity}, Bounciness at {bounciness}");
}

#[test]
fn removing_physics_takes_both_cards_off() {
    let mut h = harness(Some(ThingId(2)));
    h.get_by_label("Remove Physics").click();
    h.run();
    let slots = &h.state().scene.things[&ThingId(2)].slots;
    assert!(!slots.contains_key(&SlotId::Motion) && !slots.contains_key(&SlotId::Bounce));
    assert!(h.query_by_label("Hide Physics").is_none(), "no Physics card left");
}

#[test]
fn adding_physics_brings_both_back() {
    let mut h = harness(Some(ThingId(2)));
    h.get_by_label("Remove Physics").click();
    h.run();
    h.get_by_label("+ Add card").click();
    h.run();
    assert_eq!(h.query_all_by_label("Physics").count(), 1, "one Physics entry in the menu");
    h.get_by_label("Physics").click();
    h.run();
    let slots = &h.state().scene.things[&ThingId(2)].slots;
    assert_eq!(slots[&SlotId::Motion].kind, "motion.basic");
    assert_eq!(slots[&SlotId::Bounce].kind, "bounce.basic");
}

#[test]
fn a_shape_shows_bounce_settings_only() {
    let h = harness(Some(ThingId(1)));
    assert_eq!(h.state().scene.things[&ThingId(1)].kind, "shape", "the new scene's thing 1 is a shape");
    assert!(h.state().inspector.card_tops.iter().any(|(c, _)| *c == Card::Physics), "a Physics card");
    assert!(h.query_all_by_label("Takes over").next().is_some());
    assert!(h.query_all_by_role_and_label(Role::Slider, "Bounciness").next().is_none(), "only with Takes over");
    assert!(h.query_all_by_label("Gravity ×").next().is_none(), "a shape has no Motion card");
    assert!(h.query_by_label("Hide Motion").is_none());
    assert!(h.query_by_label("Hide Bounce").is_some(), "the shape's Bounce settings show");
}

#[test]
fn the_jump_bar_has_one_physics_chip() {
    let h = harness(Some(ThingId(2)));
    let labels: Vec<String> = jump_bar(&h).into_iter().map(|(l, _)| l).collect();
    assert_eq!(labels.iter().filter(|l| *l == "Physics").count(), 1, "{labels:?}");
    assert!(!labels.iter().any(|l| l == "Motion" || l == "Bounce"), "{labels:?}");
}

#[test]
fn editing_inside_physics_edits_the_right_card() {
    let mut h = harness(Some(ThingId(2)));
    let card = |h: &Harness<'_, State>, slot| h.state().scene.things[&ThingId(2)].slots[&slot].params.clone();
    let (motion, bounce) = (card(&h, SlotId::Motion), card(&h, SlotId::Bounce));
    let rect = slider_rect(&h, "Air drag");
    drag_slider(&mut h, rect, 0.9);
    assert_ne!(card(&h, SlotId::Motion), motion, "Air drag is Motion's");
    assert_eq!(card(&h, SlotId::Bounce), bounce, "Bounce is untouched");
    let motion = card(&h, SlotId::Motion);
    let rect = slider_rect(&h, "Bounciness");
    drag_slider(&mut h, rect, 0.2);
    assert_ne!(card(&h, SlotId::Bounce), bounce, "Bounciness is Bounce's");
    assert_eq!(card(&h, SlotId::Motion), motion, "Motion is untouched");
}

#[test]
fn adding_physics_fills_every_selected_ball() {
    let mut h = harness(Some(ThingId(2)));
    {
        let s = h.state_mut();
        let other = bpe_kit::add_thing(&mut s.scene, &s.kit.registry, "balls", ThingId::ROOT).unwrap();
        s.scene.remove_slot(ThingId(2), SlotId::Bounce);
        s.scene.remove_slot(other, SlotId::Motion);
        s.scene.remove_slot(other, SlotId::Bounce);
        s.selection.extra.insert(other);
        s.resolved = resolve(&s.scene, &s.kit.registry);
    }
    h.run();
    h.get_by_label("+ Add card").click();
    h.run();
    h.get_all_by_label("Physics").last().expect("the Physics entry").click();
    h.run();
    let s = h.state();
    for t in [ThingId(2), *s.selection.extra.last().expect("the other ball")] {
        let slots = &s.scene.things[&t].slots;
        assert!(slots.contains_key(&SlotId::Motion) && slots.contains_key(&SlotId::Bounce), "{t:?}");
    }
}

/// A blank scene with one Text, selected.
fn text_harness() -> (Harness<'static, State>, ThingId) {
    let reg = bpe_kit::registry();
    let mut scene = bpe_kit::new_scene(&reg);
    let id = bpe_kit::add_thing(&mut scene, &reg, bpe_kit::things::text::KIND, ThingId::ROOT).unwrap();
    (harness_of(scene, Some(id)), id)
}

fn text_settings(h: &Harness<'_, State>, id: ThingId) -> bpe_kit::things::text::TextSettings {
    bpe_kit::things::text::TextSettings::from_params(&h.state().scene.things[&id].params)
}

fn font_file(name: &str) -> Vec<u8> {
    std::fs::read(bpe_kit::text::fonts::repo_fonts_dir().join(name)).unwrap()
}

#[test]
fn typing_in_the_words_box_changes_the_words() {
    let (mut h, id) = text_harness();
    let words = h.get_by_role_and_label(Role::MultilineTextInput, "Words");
    words.click();
    h.run();
    h.get_by_role_and_label(Role::MultilineTextInput, "Words").type_text("HELLO\nWORLD");
    h.run();
    assert!(text_settings(&h, id).words.ends_with("HELLO\nWORLD"), "{:?}", text_settings(&h, id).words);
}

#[test]
fn insert_value_puts_a_value_in_the_words() {
    let (mut h, id) = text_harness();
    h.get_by_role_and_label(Role::Button, "Insert value").click();
    h.run();
    h.get_by_label("Time").click();
    h.run();
    assert!(text_settings(&h, id).words.ends_with("{time}"), "{:?}", text_settings(&h, id).words);
}

#[test]
fn a_text_shows_its_show_and_riding_rows() {
    let (h, _) = text_harness();
    for label in ["Show", "Appear", "Disappear", "Timed by", "Fast letters"] {
        assert!(h.query_all_by_label(label).next().is_some(), "{label}");
    }
}

#[test]
fn the_font_picker_lists_every_group_and_picks_a_font() {
    let (mut h, id) = text_harness();
    h.get_by_role_and_label(Role::Button, "Font").click();
    h.run();
    for group in bpe_kit::text::fonts::FontGroup::ALL {
        assert!(h.query_all_by_label(group.name()).next().is_some(), "{}", group.name());
    }
    assert!(h.query_all_by_label("Bebas Neue").next().is_some());
    let antons = h.query_all_by_label("Anton").count();
    h.get_by_label("Search fonts").click();
    h.run();
    h.get_by_label("Search fonts").type_text("beb");
    h.run();
    assert_eq!(h.query_all_by_label("Anton").count(), antons - 1, "Anton is hidden");
    h.get_by_role_and_label(Role::Button, "Bebas Neue").click();
    h.run();
    assert_eq!(text_settings(&h, id).font, "Bebas Neue");
}

#[test]
fn load_font_adds_it_to_the_scene_and_uses_it() {
    let (mut h, id) = text_harness();
    let ctx = h.ctx.clone();
    let bytes = font_file("BebasNeue.ttf");
    crate::font_picker::request(&ctx, crate::font_picker::FontRequest::Load(bytes, "file".into()));
    // One frame (one undo step) both keeps the font and sets the text in it.
    h.step();
    assert!(h.state().scene.fonts.contains_key("Bebas Neue"), "named by its family");
    assert_eq!(text_settings(&h, id).font, "Bebas Neue");
    h.run();
    // Loaded fonts head the picker, each with Remove.
    h.get_by_role_and_label(Role::Button, "Font").click();
    h.run();
    assert!(h.query_by_label("Loaded").is_some());
    h.get_by_label("Remove Bebas Neue").click();
    h.run();
    assert!(h.state().scene.fonts.is_empty());
}

#[test]
fn load_font_refuses_a_file_that_is_not_a_font() {
    let (mut h, _) = text_harness();
    let before = h.state().scene.clone();
    let ctx = h.ctx.clone();
    crate::font_picker::request(&ctx, crate::font_picker::FontRequest::Load(b"not a font".to_vec(), "x".into()));
    h.run();
    assert_eq!(h.state().scene, before, "the scene is unchanged");
    assert!(h.query_by_label(crate::font_picker::NOT_A_FONT).is_some());
    let ctx = h.ctx.clone();
    crate::font_picker::request(&ctx, crate::font_picker::FontRequest::Load(Vec::new(), "empty".into()));
    h.run();
    assert_eq!(h.state().scene, before, "nor an empty file");
}

#[test]
fn a_missing_font_says_so_on_the_row() {
    let reg = bpe_kit::registry();
    let mut scene = bpe_kit::new_scene(&reg);
    let id = bpe_kit::add_thing(&mut scene, &reg, bpe_kit::things::text::KIND, ThingId::ROOT).unwrap();
    scene.things.get_mut(&id).unwrap().params.set("font", Value::Text("Nope".into()));
    let h = harness_of(scene, Some(id));
    assert!(h.query_by_label("Font \"Nope\" not found; showing IBM Plex Sans instead").is_some());
}

#[test]
fn the_body_of_a_text_points_at_the_words_card() {
    let (h, _) = text_harness();
    assert!(h.query_by_label("The words, font and path are in the Words card above.").is_some());
    assert!(h.query_all_by_label("Drag corners and sides in the video to mould it.").next().is_none());
}

#[test]
fn a_text_cannot_add_a_particles_layer() {
    let (h, _) = text_harness();
    assert!(h.query_all_by_label("+ Add particles").next().is_none());
    assert!(h.query_all_by_label("+ Add layer").next().is_some(), "but paint layers, yes");
}
