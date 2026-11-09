use super::*;

use bpe_core::Settings;

#[test]
fn the_label_column_is_108_wide_and_96_when_narrow() {
    assert_eq!(label_width(200.0), 96.0);
    assert_eq!(label_width(329.0), 96.0);
    assert_eq!(label_width(330.0), 108.0);
    assert_eq!(label_width(500.0), 108.0);
}

#[test]
fn a_keyed_label_takes_the_track_colour() {
    let track = Color32::from_rgb(0x3f, 0xd6, 0xb5);
    let keyed = KeyMark { can_key: true, keyed: true, pulsed: false };
    let pulsed = KeyMark { can_key: true, keyed: false, pulsed: true };
    assert_eq!(label_colour(Some(keyed), track), track);
    assert_eq!(label_colour(Some(pulsed), track), track);
    assert_eq!(label_colour(Some(KeyMark::default()), track), style::MUTED);
    assert_eq!(label_colour(None, track), style::MUTED);
}

#[test]
fn a_rule_names_one_letter_of_a_text_in_the_world() {
    let kit = bpe_kit::kit();
    let mut scene = bpe_kit::new_scene(&kit.registry);
    let text = bpe_kit::add_thing(&mut scene, &kit.registry, bpe_kit::things::text::KIND, ThingId::ROOT).unwrap();
    scene.rename_thing(text, "Word");
    let on_screen = TargetChoices::from_scene(&scene, &kit.registry);
    assert!(on_screen.letter_things.is_empty(), "a screen Text has no letters to name");
    scene.things.get_mut(&text).unwrap().screen = Some(false);
    let c = TargetChoices::from_scene(&scene, &kit.registry);
    assert_eq!(c.letter_things, [text]);
    let who = bpe_core::Who::Thing { thing: text, ball: Some(2) };
    assert_eq!(who_words(&who, &c), "Word, letter 2");
    let ball = bpe_core::Who::Thing { thing: ThingId(2), ball: Some(3) };
    assert_eq!(who_words(&ball, &c), "Ball, ball 3");
}

#[test]
fn target_words_name_things_tags_and_kinds() {
    let kit = bpe_kit::kit();
    let mut scene = bpe_kit::new_scene(&kit.registry);
    scene.things.get_mut(&ThingId(1)).unwrap().tags.insert("walls".into());
    let c = TargetChoices::from_scene(&scene, &kit.registry);
    assert_eq!(target_words(&Target::Anything, &c), "Anything");
    assert_eq!(target_words(&Target::Me, &c), "Itself");
    assert_eq!(target_words(&Target::Thing(ThingId(2)), &c), "Ball");
    assert_eq!(target_words(&Target::Thing(ThingId(9)), &c), "A missing thing (id 9)");
    assert_eq!(target_words(&Target::Tag("walls".into()), &c), "Tagged walls");
    assert_eq!(target_words(&Target::Kind("shape".into()), &c), "Any Shape");
    assert_eq!(target_words(&Target::Kind("blob".into()), &c), "Any blob");
    assert_eq!(c.tags, vec!["walls".to_owned()]);
    assert!(c.kinds.iter().all(|(id, _)| id != "scene"));
    assert_eq!(c.things[0], (ThingId::ROOT, "Scene settings".to_owned()));
}

#[test]
fn names_are_made_readable() {
    assert_eq!(pretty("electric_piano"), "Electric piano");
    assert_eq!(pretty("box"), "Box");
    assert_eq!(pretty(""), "");
    assert_eq!(pretty("flat2_d"), "Flat 2D");
    assert_eq!(pretty("x2_delay"), "X2 delay");
}

mod bar {
    use crate::gradient_bar::gradient_bar;
    use bpe_core::{Color, Gradient, Stop};
    use eframe::egui::{self, Modifiers, PointerButton};
    use egui_kittest::{Harness, kittest::Queryable};

    const RED: Color = Color { r: 255, g: 0, b: 0, a: 255 };
    const GREEN: Color = Color { r: 0, g: 255, b: 0, a: 255 };
    const BLUE: Color = Color { r: 0, g: 0, b: 255, a: 255 };

    fn harness(g: Gradient) -> Harness<'static, Gradient> {
        let mut h = Harness::builder()
            .with_size(egui::vec2(420.0, 2600.0))
            .allow_missing_glyphs()
            .with_context_setup(crate::style::apply)
            .build_ui_state(
                |ui, g: &mut Gradient| {
                    let id = ui.make_persistent_id("bar");
                    gradient_bar(ui, id, g);
                },
                g,
            );
        h.run();
        h
    }

    fn press(h: &mut Harness<'_, Gradient>, pos: egui::Pos2, button: PointerButton, pressed: bool) {
        h.event(egui::Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE });
    }

    /// Presses at `from`, moves to `to` and lets go there.
    fn drag(h: &mut Harness<'_, Gradient>, from: egui::Pos2, to: egui::Pos2) {
        h.hover_at(from);
        press(h, from, PointerButton::Primary, true);
        h.step();
        h.event(egui::Event::PointerMoved(from + (to - from) * 0.5));
        h.step();
        h.event(egui::Event::PointerMoved(to));
        h.step();
        press(h, to, PointerButton::Primary, false);
        h.run();
    }

    fn bar_rect(h: &Harness<'_, Gradient>) -> egui::Rect {
        h.get_by_label("Colours bar").rect()
    }

    fn ats(g: &Gradient) -> Vec<f32> {
        g.stops.iter().map(|s| s.at).collect()
    }

    #[test]
    fn clicking_the_bar_adds_a_stop() {
        let two = Gradient::even(&[RED, BLUE]);
        let mut h = harness(two.clone());
        let bar = bar_rect(&h);
        assert!(bar.height() >= 18.0 && bar.width() >= 200.0, "{bar:?}");
        let at = egui::pos2(bar.left() + bar.width() * 0.25, bar.center().y);
        h.hover_at(at);
        press(&mut h, at, PointerButton::Primary, true);
        h.step();
        press(&mut h, at, PointerButton::Primary, false);
        h.run();
        let g = h.state();
        assert_eq!(g.stops.len(), 3, "{g:?}");
        assert!((g.stops[1].at - 0.25).abs() < 0.01, "{g:?}");
        assert_eq!(g.stops[1].color, Color::from_linear(two.sample(g.stops[1].at)), "the sampled colour");
        assert_eq!((g.mix, g.looped), (two.mix, two.looped));
    }

    #[test]
    fn dragging_a_handle_moves_its_stop() {
        let mut h = harness(Gradient::even(&[RED, GREEN, BLUE]));
        let bar = bar_rect(&h);
        // Red, the first stop, dragged past green: the list re-sorts and red's handle,
        // now the second, keeps the focus.
        let from = h.get_by_label("Colours stop 1").rect().center();
        drag(&mut h, from, egui::pos2(bar.left() + bar.width() * 0.75, from.y));
        let g = h.state().clone();
        assert_eq!(g.stops.iter().map(|s| s.color).collect::<Vec<_>>(), [GREEN, RED, BLUE], "{g:?}");
        assert!((g.stops[1].at - 0.75).abs() < 0.02, "{g:?}");
        assert_eq!(g.stops[0].at, 0.5);
        assert!(h.get_by_label("Colours stop 2").is_focused(), "the dragged handle keeps the focus");
        // Past the end it stops at the end.
        let from = h.get_by_label("Colours stop 1").rect().center();
        drag(&mut h, from, egui::pos2(bar.left() - 60.0, from.y));
        assert_eq!(ats(h.state()), [0.0, 0.75, 1.0]);
        // Dragged well below the bar, it comes off.
        let from = h.get_by_label("Colours stop 2").rect().center();
        drag(&mut h, from, egui::pos2(from.x, bar.bottom() + 60.0));
        assert_eq!(h.state().stops.iter().map(|s| s.color).collect::<Vec<_>>(), [GREEN, BLUE]);
        // A little below, it stays.
        let from = h.get_by_label("Colours stop 2").rect().center();
        drag(&mut h, from, egui::pos2(from.x - 20.0, bar.bottom() + 20.0));
        assert_eq!(h.state().stops.len(), 2);
    }

    #[test]
    fn right_click_remove_keeps_at_least_one() {
        let mut h = harness(Gradient::even(&[RED, BLUE]));
        h.get_by_label("Colours stop 2").click_secondary();
        h.run();
        assert!(h.query_by_label_contains("Alpha").is_none(), "a right-click opens no picker");
        h.get_by_label("Remove").click();
        h.run();
        assert_eq!(h.state().stops, [Stop::new(0.0, RED)]);
        // The last colour has no handle to take off, only its swatch.
        assert!(h.query_by_label("Colours stop 1").is_none());
        assert!(h.query_by_label("Remove").is_none());
        // Dragging the last stop of two off the bar leaves one.
        h.state_mut().stops.push(Stop::new(1.0, BLUE));
        h.run();
        let bar = bar_rect(&h);
        let from = h.get_by_label("Colours stop 1").rect().center();
        drag(&mut h, from, egui::pos2(from.x, bar.bottom() + 60.0));
        assert_eq!(h.state().stops, [Stop::new(1.0, BLUE)]);
    }

    #[test]
    fn one_stop_shows_a_swatch_and_plus() {
        let mut h = harness(Gradient::solid(RED));
        assert!(h.query_by_label("Colours bar").is_none(), "no bar for one colour");
        h.get_by_label("Colours colour").click();
        h.run();
        assert!(h.query_by_label_contains("Alpha").is_some(), "the swatch opens the picker");
        h.key_press(egui::Key::Escape);
        h.run();
        h.get_by_label("Add a colour").click();
        h.run();
        assert_eq!(h.state().stops, [Stop::new(0.0, RED), Stop::new(1.0, Color::WHITE)]);
        assert_eq!(h.state().mix, bpe_core::Mix::Even, "a new gradient mixes Even");
        assert!(h.query_by_label("Colours bar").is_some());
    }

    #[test]
    fn colour_handle_opens_on_left_click() {
        let mut h = harness(Gradient::even(&[RED, BLUE]));
        assert!(h.query_by_label_contains("Alpha").is_none());
        h.get_by_label("Colours stop 2").click();
        h.run();
        assert!(h.query_by_label_contains("Alpha").is_some(), "a click opens the picker");
        assert!(h.query_by_label_contains("Additive").is_none(), "Blend Add replaced the Additive checkbox");
        assert!(h.query_by_label("Remove").is_none());
        assert_eq!(h.state().stops.len(), 2, "a click on a handle adds nothing");
    }

    #[test]
    fn there_is_no_transition_menu() {
        let h = harness(Gradient::even(&[RED, BLUE]));
        assert!(h.query_by_label("Colours transition").is_none(), "gap blends are in the tabs now");
    }
}

/// The Balls settings' grid, its values in `State::values`, with Size keyed when `keyed`.
#[derive(Default)]
struct State {
    values: std::collections::BTreeMap<&'static str, Value>,
    new_keys: Vec<&'static str>,
    changes: usize,
}

fn balls_grid(keyed: bool) -> egui_kittest::Harness<'static, State> {
    let defs = bpe_kit::BallsSettings::defs();
    let values = defs.iter().map(|d| (d.key, d.default.clone())).collect();
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(420.0, 1600.0))
        .allow_missing_glyphs()
        .with_context_setup(crate::style::apply)
        .build_ui_state(
            move |ui, state: &mut State| {
                let marks = |key: &str| {
                    let def = def_in(defs, key)?;
                    def.flags.keyframe.then_some(KeyMark {
                        can_key: true,
                        keyed: keyed && key == "radius",
                        pulsed: false,
                    })
                };
                let choices = TargetChoices::default();
                let cx = GridCx {
                    choices: &choices,
                    marks: &marks,
                    applies: &|_| true,
                    folded: None,
                    label_width: label_width(420.0),
                    track: style::TEXT,
                    follow: None,
                    vary: None,
                    offers: None,
                    rails: None,
                };
                let got = settings_grid(ui, "g", defs, |k| state.values[k].clone(), &cx);
                for (key, value) in got.changes {
                    state.values.insert(key, value);
                    state.changes += 1;
                }
                state.new_keys.extend(got.new_keys);
            },
            State { values, ..State::default() },
        );
    h.run();
    h
}

fn right_click(h: &mut egui_kittest::Harness<'_, State>, at: egui::Pos2) {
    h.hover_at(at);
    h.event(egui::Event::PointerButton {
        pos: at,
        button: PointerButton::Secondary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
    h.event(egui::Event::PointerButton {
        pos: at,
        button: PointerButton::Secondary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
}

#[test]
fn right_clicking_a_slider_opens_the_menu_and_leaves_the_value() {
    use eframe::egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;
    let mut h = balls_grid(false);
    let before = h.state().values["radius"].clone();
    let slider = h.get_by_role_and_label(Role::Slider, "Size").rect();
    // Near the slider's right end, far from where the value sits.
    right_click(&mut h, egui::pos2(slider.right() - 4.0, slider.center().y));
    assert_eq!(h.state().values["radius"], before, "a right-click never changes a value");
    assert_eq!(h.state().changes, 0);
    assert!(h.query_by_label("Copy Value").is_some(), "the menu is open");
    assert!(h.query_by_label("Paste Value").is_some());
    assert!(h.query_by_label("Create Keyframe").is_some());
    assert!(h.query_by_label("Pulse with hits").is_none(), "no pulses in the menu");
}

#[test]
fn copy_then_paste_moves_a_value_between_settings() {
    use eframe::egui::accesskit::Role;
    use egui_kittest::kittest::{NodeT, Queryable};
    let mut h = balls_grid(false);
    h.state_mut().values.insert("space", Value::Number(42.0));
    h.run();
    // Nothing copied yet: Paste is there but greyed out.
    let size = h.get_by_role_and_label(Role::Label, "Size").rect().center();
    right_click(&mut h, size);
    assert!(h.get_by_label("Paste Value").accesskit_node().is_disabled());
    h.key_press(egui::Key::Escape);
    h.run();
    let spacing = h.get_by_role_and_label(Role::Label, "Space between").rect().center();
    right_click(&mut h, spacing);
    h.get_by_label("Copy Value").click();
    h.run();
    assert_eq!(copied(&h.ctx).map(|c| c.words), Some("42 px".to_owned()));
    right_click(&mut h, size);
    h.get_by_label("Paste Value (42 px)").click();
    h.run();
    assert_eq!(h.state().values["radius"], Value::Number(42.0));
    // A number doesn't fit a choice.
    let arrangement = h.get_by_role_and_label(Role::Label, "Shape").rect().center();
    right_click(&mut h, arrangement);
    assert!(h.get_by_label("Paste Value (42 px)").accesskit_node().is_disabled());
}

#[test]
fn create_keyframe_is_offered_only_where_keys_are_allowed() {
    use eframe::egui::accesskit::Role;
    use egui_kittest::kittest::{NodeT, Queryable};
    let mut h = balls_grid(true);
    let speed = h.get_by_role_and_label(Role::Label, "Launch speed").rect().center();
    right_click(&mut h, speed);
    assert!(h.get_by_label("Create Keyframe").accesskit_node().is_disabled(), "launch speed can't have keys");
    h.key_press(egui::Key::Escape);
    h.run();
    let size = h.get_by_role_and_label(Role::Label, "Size").rect().center();
    right_click(&mut h, size);
    h.get_by_label("Create Keyframe").click();
    h.run();
    assert_eq!(h.state().new_keys, ["radius"]);
    assert_eq!(h.state().changes, 0);
    assert!(h.query_by_label("Keyframe Size").is_none(), "no diamonds");
}

#[test]
fn pasting_fits_the_value_to_the_setting() {
    let defs = bpe_kit::BallsSettings::defs();
    let def = |k: &str| defs.iter().find(|d| d.key == k).unwrap();
    assert_eq!(pasted(def("radius"), &Value::Number(9999.0)), Some(Value::Number(200.0)), "kept in range");
    assert_eq!(pasted(def("columns"), &Value::Number(3.6)), Some(Value::Int(4)));
    assert_eq!(pasted(def("radius"), &Value::Bool(true)), None);
    assert_eq!(pasted(def("shape"), &Value::Choice("nowhere".into())), None);
}

#[test]
fn copied_values_read_in_a_few_words() {
    let c = TargetChoices::default();
    assert_eq!(value_words(&Value::Number(12.5), "px", &c), "12.5 px");
    assert_eq!(value_words(&Value::Number(0.25), "", &c), "0.25");
    assert_eq!(value_words(&Value::Int(3), "", &c), "3");
    assert_eq!(value_words(&Value::Bool(false), "", &c), "Off");
    assert_eq!(value_words(&Value::Vec2([0.0, 980.0]), "px/s²", &c), "0, 980 px/s²");
    assert_eq!(value_words(&Value::Vec2([300.0, -400.0]), "px/s", &c), "500 px/s at -53°");
    assert_eq!(value_words(&Value::Color(Color::rgb(0xff, 0x80, 0x00)), "", &c), "#FF8000");
    let g = bpe_core::Gradient::even(&[Color::WHITE, Color::rgb(0, 0, 0), Color::WHITE]);
    assert_eq!(value_words(&Value::Gradient(g), "", &c), "3 colours");
    assert_eq!(value_words(&Value::Choice("electric_piano".into()), "", &c), "Electric piano");
    assert_eq!(value_words(&Value::Target(Target::Me), "", &c), "Itself");
}

#[test]
fn every_setting_that_applies_shows_with_groups_under_headings() {
    let defs = bpe_kit::SceneSettings::defs();
    let all = rows_for(defs, &|_| true);
    assert_eq!(all.iter().filter(|r| matches!(r, Row::Setting(_))).count(), defs.len());
    let headings: Vec<&str> = all.iter().filter_map(|r| if let Row::Group(g) = r { Some(*g) } else { None }).collect();
    // Each card's name heads its rows once (in the order the fields first name them).
    let mut sorted = headings.clone();
    sorted.sort_unstable();
    let mut cards = bpe_kit::ROOT_CARDS.to_vec();
    cards.sort_unstable();
    assert_eq!(sorted, cards);
    // A group whose settings all drop out loses its heading too.
    let no_world = rows_for(defs, &|d| d.group != "World");
    assert!(!no_world.iter().any(|r| matches!(r, Row::Group("World"))));
}

#[test]
fn a_looks_colours_are_one_gradient_row() {
    let defs = bpe_kit::modules::look::BasicLook::defs();
    let rows = rows_for(defs, &|_| true);
    let colours: Vec<&SettingDef> =
        rows.iter().filter_map(|r| if let Row::Setting(d) = r { Some(*d) } else { None }).collect();
    let colours: Vec<&str> = colours.iter().filter(|d| d.ty == ValueType::Gradient).map(|d| d.key).collect();
    // One row for the colours, one for the outline's, one for those inside each one.
    assert_eq!(colours, ["outline_colours", "colours", "in_colours"]);
}

fn headings<'a>(rows: &[Row<'a>]) -> Vec<&'static str> {
    rows.iter().filter_map(|r| if let Row::Group(g) = r { Some(*g) } else { None }).collect()
}

#[test]
fn rows_follow_the_section_order() {
    let base = &bpe_kit::modules::look::BasicLook::defs()[0];
    let def = |key: &'static str, group: &'static str| SettingDef { key, group, ..base.clone() };
    let defs = [def("a", "Blending"), def("b", "Light"), def("c", "Shape"), def("d", "")];
    let rows = rows_for(&defs, &|_| true);
    assert!(matches!(rows[0], Row::Setting(d) if d.key == "d"), "ungrouped rows come first");
    assert_eq!(headings(&rows), ["Shape", "Light", "Blending"]);
    // Groups that are not sections (a thing kind's own) keep their order, after the sections.
    let defs = [def("a", "Video"), def("b", "Light"), def("c", "Grade")];
    assert_eq!(headings(&rows_for(&defs, &|_| true)), ["Light", "Video", "Grade"]);
}

#[test]
fn the_look_cards_sections_come_in_order() {
    let defs = bpe_kit::modules::look::BasicLook::defs();
    let headings = headings(&rows_for(defs, &|_| true));
    assert_eq!(
        headings,
        [
            "Shape",
            "Style",
            "Across the group",
            "Inside each one",
            "Light",
            "Motion",
            "Outline colours variation",
            "Variation",
            "Blending"
        ]
    );
}

#[test]
fn the_bounce_card_s_sections_come_in_order() {
    let bounce = bpe_kit::BasicBounce::defs();
    assert_eq!(headings(&rows_for(bounce, &|_| true)), ["Bounce", "Growing", "Other balls", "Against some things"]);
}

/// A grid of `defs` at their defaults, every section open, in a panel 420 wide.
fn open_grid(defs: &'static [SettingDef]) -> egui_kittest::Harness<'static> {
    let value = |k: &str| defs.iter().find(|d| d.key == k).map(|d| d.default.clone());
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(420.0, 2400.0))
        .allow_missing_glyphs()
        .with_context_setup(crate::style::apply)
        .build_ui(move |ui| {
            let choices = TargetChoices::default();
            let cx = GridCx {
                choices: &choices,
                marks: &|_| None,
                applies: &|_| true,
                folded: Some(&|_| false),
                label_width: label_width(420.0),
                track: style::TEXT,
                follow: None,
                vary: None,
                offers: None,
                rails: None,
            };
            settings_grid(ui, "g", defs, |k| value(k).unwrap(), &cx);
        });
    h.run();
    h
}

#[test]
fn a_heading_has_a_row_of_its_own() {
    use eframe::egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;
    let h = open_grid(bpe_kit::modules::look::BasicLook::defs());
    let heading = h.get_by_role_and_label(Role::Label, "Light").rect();
    let label = h.get_by_role_and_label(Role::Label, "Glow").rect();
    let slider = h.get_by_role_and_label(Role::Slider, "Glow").rect();
    assert!(heading.bottom() <= label.top(), "the heading {heading:?} is above the label {label:?}");
    let (l, s) = (label.center().y, slider.center().y);
    assert!((l - s).abs() <= 1.5, "the slider is level with its label: {l} and {s}");
}

#[test]
fn a_wrapped_label_makes_its_row_taller() {
    use eframe::egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;
    let h = open_grid(bpe_kit::SceneSettings::defs());
    let wrapped = h.get_by_role_and_label(Role::Label, "Background stays on screen").rect();
    assert!(wrapped.height() > 24.0, "the label wraps: {wrapped:?}");
    // The Background group's last row; the Picture heading comes next.
    let next = h.get_by_role_and_label(Role::Label, "Picture").rect();
    assert!(wrapped.bottom() <= next.top(), "{wrapped:?} runs into the next row {next:?}");
    let check = h.get_by_role_and_label(Role::CheckBox, "Background stays on screen").rect();
    let (l, c) = (wrapped.center().y, check.center().y);
    assert!((l - c).abs() <= 1.5, "the checkbox is level with its label: {l} and {c}");
}

#[test]
fn a_section_with_no_shown_rows_has_no_heading() {
    use egui_kittest::{Harness, kittest::Queryable};
    let defs = bpe_kit::modules::look::BasicLook::defs();
    let value = |k: &str| defs.iter().find(|d| d.key == k).map(|d| d.default.clone());
    // On an arena, every Shape row is hidden by its condition.
    let applies = |d: &SettingDef| d.applies(Some("arena"), &value);
    let rows = rows_for(defs, &applies);
    assert!(!headings(&rows).contains(&"Shape"), "{:?}", headings(&rows));
    let mut h = Harness::builder().with_size(egui::vec2(420.0, 1600.0)).allow_missing_glyphs().build_ui(|ui| {
        let choices = TargetChoices::default();
        let applies = |d: &SettingDef| d.applies(Some("arena"), &value);
        let cx = GridCx {
            choices: &choices,
            marks: &|_| None,
            applies: &applies,
            folded: Some(&|_| false),
            label_width: label_width(420.0),
            track: style::TEXT,
            follow: None,
            vary: None,
            offers: None,
            rails: None,
        };
        settings_grid(ui, "g", defs, |k| value(k).unwrap(), &cx);
    });
    h.run();
    assert!(h.query_by_label("Hide Shape").is_none(), "no fold arrow for an empty section");
    assert!(h.query_by_label("Shape").is_none(), "no heading for an empty section");
    assert!(h.query_by_label("Hide Blending").is_some(), "other sections keep theirs");
}

#[test]
fn span_reads_px_when_following_place() {
    let defs = bpe_kit::modules::look::BasicLook::defs();
    let span = defs.iter().find(|d| d.key == "span").expect("the fill's span");
    let outline_span = defs.iter().find(|d| d.key == "outline_span").expect("the outline's span");
    let following = |follow: &'static str| move |k: &str| (k == "follow").then(|| Value::Choice(follow.into()));
    assert_eq!(unit_for(span, &following("place")), "px");
    assert_eq!(unit_for(span, &following("start")), "px");
    assert_eq!(unit_for(span, &following("speed")), span.unit);
    // The outline's span follows the outline's own follow, not the fill's.
    assert_eq!(unit_for(outline_span, &following("place")), outline_span.unit);
    let outline = |k: &str| (k == "outline_follow").then(|| Value::Choice("place".into()));
    assert_eq!(unit_for(outline_span, &outline), "px");
    let glow = defs.iter().find(|d| d.key == "glow_size").unwrap();
    assert_eq!(unit_for(glow, &following("place")), glow.unit);
}

#[test]
fn numbers_move_in_even_steps_for_their_span() {
    let feel = |span: f64| number_feel(span, false);
    assert_eq!(feel(1.0).step, 0.001);
    assert_eq!(feel(1.0).decimals, 3);
    assert_eq!(feel(1.5).step, 0.001);
    assert_eq!(feel(40.0).step, 0.1);
    assert_eq!(feel(40.0).decimals, 1);
    assert_eq!(feel(720.0).step, 1.0);
    assert_eq!(feel(1980.0).step, 1.0);
    assert_eq!(feel(1980.0).decimals, 0);
    assert_eq!(number_feel(126.0, true).step, 1.0);
    assert!((feel(40.0).drag_speed - 0.1).abs() < 1e-9, "a drag crosses a small span in 400 pixels");
    // Wide spans move by ones, on the slider and the number box alike.
    for span in [2000.0, 4999.0, 40000.0] {
        assert_eq!(feel(span).step, 1.0, "{span}");
        assert_eq!(number_feel(span, true).step, 1.0, "{span}");
        assert_eq!(feel(span).drag_speed, 1.0, "{span}");
    }
    let speed = bpe_kit::BallsSettings::defs().iter().find(|d| d.key == "velocity").unwrap();
    assert_eq!(setting_feel(speed).step, 1.0, "a speed without a range moves by its usual span");
}

/// A span slider over 0..4 (hard range `range`) holding `value`, named "Amount".
fn span_harness(value: f64, range: Option<(f64, f64)>) -> egui_kittest::Harness<'static, f64> {
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(400.0, 80.0))
        .with_context_setup(style::apply)
        .build_ui_state(
            move |ui, v: &mut f64| {
                let feel = number_feel(4.0, false);
                span_slider(ui, v, (0.0, 4.0), range, feel, false, String::new(), Named::Words("Amount"));
            },
            value,
        );
    h.run();
    h
}

fn drag_rail(h: &mut egui_kittest::Harness<'static, f64>, dx: f32) {
    use egui_kittest::kittest::{NodeT, Queryable};
    let rail = h
        .query_all_by_label("Amount")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Slider)
        .unwrap()
        .rect()
        .center();
    let press = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.hover_at(rail);
    h.run();
    h.event(press(rail, true));
    h.step();
    for k in 1..=10 {
        h.event(egui::Event::PointerMoved(rail + egui::vec2(dx * k as f32 / 10.0, 0.0)));
        h.step();
    }
    h.event(press(rail + egui::vec2(dx, 0.0), false));
    h.run();
}

#[test]
fn a_value_past_its_span_shows_as_it_is_and_drags_keep_going() {
    assert_eq!(past_span(14.0, (0.0, 4.0)), 1);
    assert_eq!(past_span(-1.0, (0.0, 4.0)), -1);
    assert_eq!(past_span(4.0, (0.0, 4.0)), 0);
    let mut h = span_harness(14.0, None);
    assert_eq!(*h.state(), 14.0, "drawing never pulls it into the span");
    // From the middle (2) a long drag right goes past the end at the same rate.
    *h.state_mut() = 2.0;
    h.run();
    drag_rail(&mut h, 600.0);
    assert!(*h.state() > 8.0, "kept going past 4: {}", h.state());
    drag_rail(&mut h, -2000.0);
    assert!(*h.state() < 0.0, "and below 0 the other way: {}", h.state());
}

/// A slider in `unit` over `span` (hard range `range`) holding `value`, named "Amount", with
/// the grid snapping at 60 px and 15°.
fn snapping_harness(
    value: f64,
    span: (f64, f64),
    range: Option<(f64, f64)>,
    unit: &'static str,
) -> egui_kittest::Harness<'static, f64> {
    use crate::setting_snap::{SettingSnap, Snapper, put};
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(400.0, 80.0))
        .with_context_setup(style::apply)
        .build_ui_state(
            move |ui, v: &mut f64| {
                put(ui.ctx(), Some(SettingSnap { length: 60.0, angle: 15.0 }));
                let feel = number_feel(span.1 - span.0, false).with_snap(Snapper::for_unit(ui, unit, false));
                span_slider(ui, v, span, range, feel, false, suffix(unit), Named::Words("Amount"));
            },
            value,
        );
    h.run();
    h
}

/// [`drag_rail`] with `modifiers` held all the way.
fn drag_rail_holding(h: &mut egui_kittest::Harness<'static, f64>, dx: f32, modifiers: egui::Modifiers) {
    use egui_kittest::kittest::{NodeT, Queryable};
    let rail = h
        .query_all_by_label("Amount")
        .find(|n| n.accesskit_node().role() == egui::accesskit::Role::Slider)
        .unwrap()
        .rect()
        .center();
    let press = |pos, pressed| egui::Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers };
    h.event(egui::Event::ModifiersChanged(modifiers));
    h.hover_at(rail);
    h.run();
    h.event(press(rail, true));
    h.step();
    for k in 1..=10 {
        h.event(egui::Event::PointerMoved(rail + egui::vec2(dx * k as f32 / 10.0, 0.0)));
        h.step();
    }
    h.event(press(rail + egui::vec2(dx, 0.0), false));
    h.step();
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    h.run();
}

fn on_the_grid(v: f64) -> bool {
    (v / 60.0).fract().abs() < 1e-9 || (v.abs() < 60.0 && (v / 7.5).fract().abs() < 1e-9)
}

#[test]
fn a_length_dragged_while_the_grid_snaps_lands_on_it() {
    for dx in [37.0, 81.0, 123.0, -51.0] {
        let mut h = snapping_harness(200.0, (0.0, 400.0), None, "px");
        drag_rail_holding(&mut h, dx, egui::Modifiers::NONE);
        let v = *h.state();
        assert!(v != 200.0 || dx.abs() < 10.0, "it moved: {dx} {v}");
        assert!(on_the_grid(v), "{dx}: {v}");
    }
}

#[test]
fn dragging_the_number_box_lands_on_the_grid_too() {
    let mut h = snapping_harness(200.0, (0.0, 400.0), None, "px");
    drag_named(&mut h, "Amount", egui::accesskit::Role::SpinButton, 90.0);
    let v = *h.state();
    assert!(v != 200.0 && on_the_grid(v), "{v}");
}

#[test]
fn alt_held_drags_a_length_freely() {
    let mut h = snapping_harness(200.0, (0.0, 400.0), None, "px");
    drag_rail_holding(&mut h, 37.0, egui::Modifiers::ALT);
    let v = *h.state();
    assert!(!on_the_grid(v), "free with Alt: {v}");
}

#[test]
fn angles_land_on_the_angle_step_and_speeds_never_snap() {
    let mut h = snapping_harness(0.0, (-180.0, 180.0), None, "°");
    drag_rail_holding(&mut h, 41.0, egui::Modifiers::NONE);
    let v = *h.state();
    assert!(v != 0.0 && (v / 15.0).fract().abs() < 1e-9, "on 15° steps: {v}");
    let mut h = snapping_harness(200.0, (0.0, 400.0), None, "px/s");
    drag_rail_holding(&mut h, 37.0, egui::Modifiers::NONE);
    assert!(!on_the_grid(*h.state()), "a speed isn't a distance: {}", h.state());
}

#[test]
fn a_snapped_length_still_keeps_its_hard_range() {
    let mut h = snapping_harness(2.0, (0.5, 400.0), Some((0.5, 400.0)), "px");
    drag_rail_holding(&mut h, -300.0, egui::Modifiers::NONE);
    assert_eq!(*h.state(), 0.5, "0 is a step, the range lifts it to 0.5");
}

#[test]
fn a_typed_length_stays_exact() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let mut h = snapping_harness(200.0, (0.0, 400.0), None, "px");
    let field = h
        .query_all_by_label("Amount")
        .find(|n| n.accesskit_node().role() != egui::accesskit::Role::Slider)
        .unwrap()
        .rect()
        .center();
    // A double-click opens the number box for typing.
    h.hover_at(field);
    h.run();
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: field,
                button: PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        h.step();
    }
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.event(egui::Event::Text("123".into()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(*h.state(), 123.0);
}

#[test]
fn a_dropdown_offers_only_the_choices_that_fit() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let defs = bpe_kit::things::light::LightSettings::defs();
    let def = defs.iter().find(|d| d.key == "rides_on").unwrap();
    let choices = Box::leak(Box::new(TargetChoices::default()));
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(400.0, 300.0))
        .with_context_setup(style::apply)
        .build_ui_state(
            move |ui, v: &mut Value| {
                let name = ui.label(def.label).id;
                // A light in a shape: its middle or its edge.
                let offers = |c: &str| matches!(c, "the_middle" | "the_edge");
                widget(ui, def, def.unit, v, name, egui::Id::unique("bar"), choices, false, Some(&offers));
            },
            Value::Choice("the_middle".into()),
        );
    h.run();
    let combo = h.query_all_by_label("Rides on").find(|n| n.accesskit_node().role() == egui::accesskit::Role::ComboBox);
    combo.expect("the dropdown").click();
    h.run();
    assert!(h.query_all_by_label("The edge").next().is_some());
    assert!(h.query_all_by_label("Each ball").next().is_none() && h.query_all_by_label("Ball number").next().is_none());
    h.query_all_by_label("The edge").last().unwrap().click();
    h.run();
    assert_eq!(*h.state(), Value::Choice("the_edge".into()));
}

/// A setting row's widget for `def` holding `value`, the grid snapping at 60 px and 15°.
fn widget_harness(def: SettingDef, value: Value) -> egui_kittest::Harness<'static, Value> {
    use crate::setting_snap::{SettingSnap, put};
    let choices = Box::leak(Box::new(TargetChoices::default()));
    let mut h = egui_kittest::Harness::builder()
        .with_size(egui::vec2(400.0, 80.0))
        .with_context_setup(style::apply)
        .build_ui_state(
            move |ui, v: &mut Value| {
                put(ui.ctx(), Some(SettingSnap { length: 60.0, angle: 15.0 }));
                let name = ui.label(def.label).id;
                let unit = def.unit;
                widget(ui, &def, unit, v, name, egui::Id::unique("bar"), choices, false, None);
            },
            value,
        );
    h.run();
    h
}

/// Drags the first control named `name` of role `role` by `dx`.
fn drag_named<S>(h: &mut egui_kittest::Harness<'static, S>, name: &str, role: egui::accesskit::Role, dx: f32) {
    use egui_kittest::kittest::{NodeT, Queryable};
    let at = h.query_all_by_label(name).find(|n| n.accesskit_node().role() == role).unwrap().rect().center();
    let press = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    h.hover_at(at);
    h.run();
    h.event(press(at, true));
    h.step();
    for k in 1..=10 {
        h.event(egui::Event::PointerMoved(at + egui::vec2(dx * k as f32 / 10.0, 0.0)));
        h.step();
    }
    h.event(press(at + egui::vec2(dx, 0.0), false));
    h.run();
}

#[test]
fn a_ranged_length_and_a_place_land_on_the_grid() {
    let base = bpe_kit::modules::look::BasicLook::defs()[0].clone();
    let thick = SettingDef {
        key: "thickness",
        label: "Thickness",
        ty: ValueType::Number,
        default: Value::Number(100.0),
        range: Some((0.5, 400.0)),
        span: None,
        unit: "px",
        when: "",
        ..base.clone()
    };
    let mut h = widget_harness(thick, Value::Number(100.0));
    drag_named(&mut h, "Thickness", egui::accesskit::Role::Slider, 37.0);
    let Value::Number(v) = *h.state() else { panic!() };
    assert!(v != 100.0 && on_the_grid(v), "an egui slider snapped after: {v}");

    let at = SettingDef {
        key: "at",
        label: "At",
        ty: ValueType::Vec2,
        default: Value::Vec2([0.0, 0.0]),
        range: None,
        span: None,
        unit: "px",
        when: "",
        ..base
    };
    let mut h = widget_harness(at, Value::Vec2([10.0, 0.0]));
    drag_named(&mut h, "At", egui::accesskit::Role::SpinButton, 90.0);
    let Value::Vec2([x, y]) = *h.state() else { panic!() };
    assert!(x != 10.0 && (x / 60.0).fract() == 0.0, "x on a grid point: {x}");
    assert_eq!(y, 0.0, "y wasn't touched");
}

#[test]
fn a_velocity_shows_as_speed_and_angle() {
    let launch = SettingDef {
        key: "velocity",
        label: "Launch",
        ty: ValueType::Vec2,
        default: Value::Vec2([0.0, 0.0]),
        range: None,
        span: None,
        unit: "px/s",
        when: "",
        ..bpe_kit::modules::look::BasicLook::defs()[0].clone()
    };
    let mut h = widget_harness(launch, Value::Vec2([300.0, -200.0]));
    h.run();
    assert_eq!(*h.state(), Value::Vec2([300.0, -200.0]), "untouched, it keeps its exact x and y");
    drag_named(&mut h, "Launch", egui::accesskit::Role::SpinButton, 60.0);
    let Value::Vec2([x, y]) = *h.state() else { panic!() };
    let (speed, angle) = speed_angle([x, y]);
    assert!(speed > 361.0, "faster: {speed}");
    let was = (-200.0f64).atan2(300.0).to_degrees();
    assert!((angle - was).abs() < 1e-3, "the same way: {angle} {was}");
}

#[test]
fn a_hard_range_still_holds() {
    let mut h = span_harness(2.0, Some((0.0, 5.0)));
    drag_rail(&mut h, 2000.0);
    assert_eq!(*h.state(), 5.0);
    drag_rail(&mut h, -4000.0);
    assert_eq!(*h.state(), 0.0);
}
