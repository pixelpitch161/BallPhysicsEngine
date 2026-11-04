//! The Notes panel driven as the user drives it: the grid, the header, the keyboard.

use super::SLOTS;
use crate::app::Studio;
use crate::inspector::InspectorTab;
use crate::sound_tab::tests::{SHAPE, sounding};
use bpe_core::{Address, NoteSlot, SlotId, ThingId, Value};
use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};

type H = Harness<'static, (Studio, f32)>;

/// `thing`'s slots as the scene holds them.
fn slots_of(h: &H, thing: ThingId) -> Vec<NoteSlot> {
    let reg = bpe_kit::registry();
    match h.state().0.scene.value_at_time(&reg, &Address::on(thing, SlotId::Sound, SLOTS), 0.0) {
        Some(Value::Slots(s)) => s,
        other => panic!("{other:?}"),
    }
}

fn notes(h: &H) -> Vec<Vec<u8>> {
    slots_of(h, SHAPE).into_iter().map(|s| s.notes).collect()
}

fn set_slots(h: &mut H, rows: &[&[u8]]) {
    let reg = bpe_kit::registry();
    let slots = rows.iter().map(|n| NoteSlot { notes: n.to_vec(), order_clicked: n.to_vec() }).collect();
    h.state_mut().0.scene.set_at(&reg, &Address::on(SHAPE, SlotId::Sound, SLOTS), Value::Slots(slots)).unwrap();
    h.run();
}

fn click(h: &mut H, role: Role, name: &str) {
    h.get_by_role_and_label(role, name).click();
    h.run();
}

fn menu(h: &mut H, row: &str, item: &str) {
    h.get_by_role_and_label(Role::Button, row).click_secondary();
    h.run();
    h.get_by_label(item).click();
    h.run();
}

#[test]
fn clicking_a_cell_toggles_that_slots_note() {
    let mut h = sounding();
    assert_eq!(notes(&h), [vec![60]], "the starter's middle C");
    click(&mut h, Role::Button, "Slot 1 E5");
    assert_eq!(notes(&h), [vec![60, 64]]);
    click(&mut h, Role::Button, "Slot 1 C5");
    assert_eq!(notes(&h), [vec![64]]);
    click(&mut h, Role::Button, "Slot 1 C#5");
    assert_eq!(notes(&h), [vec![61, 64]], "the black key's cell, not the white one under it");
    assert!(h.state().0.panels.inspector.auditions.is_empty(), "a cell is silent; the keys play");
}

#[test]
fn plus_slot_adds_up_to_16() {
    let mut h = sounding();
    for _ in 0..15 {
        click(&mut h, Role::Button, "+ Slot");
    }
    assert_eq!(notes(&h).len(), 16);
    assert!(h.query_by_role_and_label(Role::Button, "Slot 16").is_some());
    let add = h.get_by_role_and_label(Role::Button, "+ Slot");
    assert!(add.accesskit_node().is_disabled(), "16 is the most");
    add.click();
    h.run();
    assert_eq!(notes(&h).len(), 16);
}

#[test]
fn row_menu_duplicate_and_delete() {
    let mut h = sounding();
    set_slots(&mut h, &[&[60, 64], &[67]]);
    menu(&mut h, "Slot 1", "Duplicate");
    assert_eq!(notes(&h), [vec![60, 64], vec![60, 64], vec![67]]);
    menu(&mut h, "Slot 2", "Delete");
    assert_eq!(notes(&h), [vec![60, 64], vec![67]]);
    menu(&mut h, "Slot 2", "Clear");
    assert_eq!(notes(&h), [vec![60, 64], vec![]]);
    // The cells take the menu too.
    menu(&mut h, "Slot 1 E5", "Delete");
    assert_eq!(notes(&h), [Vec::<u8>::new()]);
}

#[test]
fn copy_notes_paste_into_another_shape() {
    let mut h = sounding();
    set_slots(&mut h, &[&[60, 63, 67]]);
    let reg = bpe_kit::registry();
    let other = {
        let studio = &mut h.state_mut().0;
        bpe_kit::add_thing(&mut studio.scene, &reg, bpe_kit::things::shape::KIND, ThingId::ROOT).unwrap()
    };
    h.run();
    menu(&mut h, "Slot 1", "Copy notes");
    h.state_mut().0.selection.set(other);
    h.state_mut().0.panels.inspector.tab.insert(other, InspectorTab::Sound);
    h.run();
    h.get_by_role_and_label(Role::CheckBox, "Enable sound").click();
    h.run();
    menu(&mut h, "Slot 1", "Paste notes");
    assert_eq!(slots_of(&h, other)[0].notes, [60, 63, 67]);
}

#[test]
fn changing_key_offers_to_move_notes() {
    let mut h = sounding();
    set_slots(&mut h, &[&[60, 64, 67]]);
    click(&mut h, Role::Button, "Root");
    h.get_by_label("G").click();
    h.run();
    assert!(h.query_by_label("Move the notes into the new key?").is_some());
    click(&mut h, Role::Button, "Move notes");
    assert!(h.query_by_label("Move the notes into the new key?").is_none());
    assert_eq!(notes(&h), [vec![55, 59, 62]], "C major moved into G major");
    let reg = bpe_kit::registry();
    let root = h.state().0.scene.value_at_time(&reg, &Address::on(SHAPE, SlotId::Sound, "notes_root"), 0.0);
    assert_eq!(root, Some(Value::Choice("g".into())));
    // Keep notes changes only the key.
    click(&mut h, Role::Button, "Scale");
    h.get_by_label("Minor").click();
    h.run();
    click(&mut h, Role::Button, "Keep notes");
    assert_eq!(notes(&h), [vec![55, 59, 62]]);
    let scale = h.state().0.scene.value_at_time(&reg, &Address::on(SHAPE, SlotId::Sound, "notes_scale"), 0.0);
    assert_eq!(scale, Some(Value::Choice("minor".into())));
}

#[test]
fn an_empty_shape_changes_key_without_asking() {
    let mut h = sounding();
    set_slots(&mut h, &[&[]]);
    click(&mut h, Role::Button, "Root");
    h.get_by_label("D").click();
    h.run();
    assert!(h.query_by_label("Move the notes into the new key?").is_none());
    let reg = bpe_kit::registry();
    let root = h.state().0.scene.value_at_time(&reg, &Address::on(SHAPE, SlotId::Sound, "notes_root"), 0.0);
    assert_eq!(root, Some(Value::Choice("d".into())));
}

#[test]
fn chord_name_shows_on_the_row() {
    let mut h = sounding();
    let c5_labels = |h: &H| h.query_all_by_role_and_label(Role::Label, "C5").count();
    assert_eq!(c5_labels(&h), 2, "the keyboard's C5, and one note reads as itself");
    let c_labels = |h: &H| h.query_all_by_role_and_label(Role::Label, "C").count();
    let before = c_labels(&h); // the key's root shows "C" too
    click(&mut h, Role::Button, "Slot 1 E5");
    click(&mut h, Role::Button, "Slot 1 G5");
    assert_eq!(c_labels(&h), before + 1, "C E G is C");
    assert_eq!(c5_labels(&h), 1);
    click(&mut h, Role::Button, "Slot 1 A#5");
    assert!(h.query_by_role_and_label(Role::Label, "C7").is_some());
}

#[test]
fn out_of_range_notes_show_a_marker() {
    let mut h = sounding();
    set_slots(&mut h, &[&[30, 60, 100, 101]]);
    assert!(h.query_by_label("+1 below").is_some());
    assert!(h.query_by_label("+2 above").is_some());
    h.get_by_label("One octave fewer").click();
    h.run();
    assert!(h.query_by_label("+2 above").is_some(), "still past B5");
}

#[test]
fn drag_reorders_rows() {
    let mut h = sounding();
    set_slots(&mut h, &[&[60], &[64], &[67]]);
    let from = h.get_by_role_and_label(Role::Button, "Slot 1").rect().center();
    let to = h.get_by_role_and_label(Role::Button, "Slot 3").rect().center();
    h.hover_at(from);
    h.run();
    h.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    h.step();
    for k in 1..=4 {
        h.event(egui::Event::PointerMoved(from + (to - from) * (k as f32 / 4.0)));
        h.step();
    }
    h.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    assert_eq!(notes(&h), [vec![64], vec![67], vec![60]]);
}

#[test]
fn clicking_a_key_toggles_it_in_the_picked_row_and_auditions() {
    let mut h = sounding();
    set_slots(&mut h, &[&[60], &[64]]);
    click(&mut h, Role::Button, "Slot 2");
    click(&mut h, Role::Button, "D5");
    assert_eq!(notes(&h), [vec![60], vec![62, 64]]);
    assert!(h.state().0.panels.inspector.auditions.contains(&(SHAPE, 62)), "it plays");
    click(&mut h, Role::Button, "D5");
    assert_eq!(notes(&h), [vec![60], vec![64]]);
    click(&mut h, Role::Button, "C#5");
    assert_eq!(notes(&h), [vec![60], vec![61, 64]], "the black key, not the white under it");
}

#[test]
fn octave_buttons_grow_and_shrink() {
    let mut h = sounding();
    assert!(h.query_by_role_and_label(Role::Button, "C7").is_none());
    h.get_by_label("One octave more").click();
    h.run();
    assert!(h.query_by_role_and_label(Role::Button, "C7").is_some());
    assert!(h.query_by_role_and_label(Role::Button, "Slot 1 C7").is_some(), "the grid follows the keys");
    h.get_by_label("One octave fewer").click();
    h.run();
    h.get_by_label("One octave fewer").click();
    h.run();
    assert!(h.query_by_role_and_label(Role::Button, "C6").is_none());
    assert!(h.query_by_role_and_label(Role::Button, "B5").is_some());
}

#[test]
fn lower_and_higher_reach_every_octave() {
    let mut h = sounding();
    assert!(h.query_by_role_and_label(Role::Button, "C3").is_none());
    h.get_by_label("Show one octave lower").click();
    h.run();
    assert!(h.query_by_label("C3 to B5").is_some());
    assert!(h.query_by_role_and_label(Role::Button, "C3").is_some(), "the lower octave's keys show");
    assert!(h.query_by_role_and_label(Role::Button, "Slot 1 C3").is_some(), "the grid follows the keys");
    for _ in 0..3 {
        h.get_by_label("Show one octave lower").click();
        h.run();
    }
    assert!(h.query_by_label("C0 to B2").is_some(), "down to the bottom of MIDI");
    h.get_by_label("Show one octave higher").click();
    h.run();
    assert!(h.query_by_label("C1 to B3").is_some());
}

#[test]
fn key_labels_use_fl_names() {
    let h = sounding();
    assert!(h.query_all_by_label("C5").any(|n| n.accesskit_node().role() == Role::Label), "C5 under middle C");
    assert!(h.query_by_label("C4 to B6").is_some());
}

#[test]
fn grid_cells_line_up_with_the_keys() {
    let h = sounding();
    for note in ["C5", "C#5", "E5", "B6"] {
        let key = h.get_by_role_and_label(Role::Button, note).rect().center().x;
        let cell = h.get_by_role_and_label(Role::Button, &format!("Slot 1 {note}")).rect().center().x;
        assert!((key - cell).abs() < 1.0, "{note}: key at {key}, cell at {cell}");
    }
}

fn choice(h: &H, key: &str) -> Option<Value> {
    let reg = bpe_kit::registry();
    h.state().0.scene.value_at_time(&reg, &Address::on(SHAPE, SlotId::Sound, key), 0.0)
}

#[test]
fn choosing_a_preset_fills_slots_and_sets_by_tempo() {
    let mut h = sounding();
    click(&mut h, Role::Button, "Chord presets");
    h.get_by_label("Axis I–V–vi–IV   C G Am F").click();
    h.run();
    assert_eq!(notes(&h), [vec![60, 64, 67], vec![59, 62, 67], vec![60, 64, 69], vec![60, 65, 69]]);
    assert_eq!(choice(&h, "notes_change"), Some(Value::Choice("by_tempo".into())));
    assert_eq!(choice(&h, "notes_every_unit"), Some(Value::Choice("bars".into())));
    assert_eq!(choice(&h, "notes_every_beats"), Some(Value::Int(1)));
    // One undo takes the whole preset back.
    assert!(h.state_mut().0.undo());
    h.run();
    assert_eq!(notes(&h), [vec![60]]);
}

#[test]
fn preset_options_change_the_voicing() {
    let mut h = sounding();
    click(&mut h, Role::Button, "Chord presets");
    h.get_by_role_and_label(Role::CheckBox, "Add bass").click();
    h.run();
    h.get_by_label("Axis I–V–vi–IV   C G Am F").click();
    h.run();
    assert_eq!(notes(&h)[0], [48, 60, 64, 67], "the root an octave under the chord");
}

#[test]
fn preset_labels_use_the_shapes_key() {
    let mut h = sounding();
    set_slots(&mut h, &[&[]]);
    click(&mut h, Role::Button, "Root");
    h.get_by_label("G").click();
    h.run();
    click(&mut h, Role::Button, "Chord presets");
    assert!(h.query_by_label("Axis I–V–vi–IV   G D Em C").is_some());
    assert!(h.query_by_label("Axis I–V–vi–IV   C G Am F").is_none());
}

#[test]
fn jump_to_slot_says_it_works_by_keys() {
    let h = sounding();
    assert!(h.query_by_label("Jump to slot").is_some());
    assert!(h.query_by_label("keys only: right-click it, Create Keyframe").is_some(), "the plain value does nothing");
}
