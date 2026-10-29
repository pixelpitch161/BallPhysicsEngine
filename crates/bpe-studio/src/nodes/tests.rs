//! Drives the block canvas with simulated mouse and keys, on the real Rules catalog.

use super::editor::{Editor, Outcome, PinRef};
use super::params::{param_control, target_words};
use bpe_core::{Scene, Target, ThingId};
use bpe_graph::{Catalog, Graph, Link, Param, ParamSpec, SpecList};
use bpe_vm::{Role, catalog};
use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::{Harness, kittest::Queryable};

struct State {
    editor: Editor,
    graph: Graph,
    catalog: &'static SpecList,
    scene: Scene,
    outcome: Outcome,
}

/// "When hit" (block 1) and "Grow" (block 2), unwired.
fn graph() -> Graph {
    let c = catalog(Role::Rules);
    let mut graph = Graph::new("Test");
    graph.add(c.spec("when.hit").unwrap(), [0.0, 0.0]);
    graph.add(c.spec("do.grow").unwrap(), [400.0, 0.0]);
    graph
}

fn harness(graph: Graph) -> Harness<'static, State> {
    let scene = bpe_kit::new_scene(&bpe_kit::registry());
    let state =
        State { editor: Editor::new("test"), graph, catalog: catalog(Role::Rules), scene, outcome: Outcome::default() };
    let mut harness = Harness::builder()
        .with_size(vec2(1000.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(120)
        .allow_missing_glyphs()
        .with_context_setup(crate::theme::apply)
        .build_ui_state(
            |ui, state: &mut State| {
                let outcome = state.editor.show(ui, &mut state.graph, state.catalog, None, &state.scene);
                if outcome.changed || outcome.simple_toggled.is_some() {
                    state.outcome = outcome;
                }
            },
            state,
        );
    harness.run();
    harness
}

fn pin(harness: &Harness<'_, State>, node: u32, pin: u8, output: bool) -> Pos2 {
    let state = harness.state();
    state.editor.pin_screen_pos(&state.graph, state.catalog, PinRef { node, pin, output })
}

fn drag(harness: &mut Harness<'_, State>, from: Pos2, to: Pos2) {
    harness.hover_at(from);
    harness.step();
    harness.event(Event::PointerButton {
        pos: from,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    harness.step();
    for i in 1..=8 {
        let p = from + (to - from) * (i as f32 / 8.0);
        harness.hover_at(p);
        harness.step();
    }
    harness.event(Event::PointerButton {
        pos: to,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    harness.step();
    harness.run();
}

fn key(harness: &mut Harness<'_, State>, key: Key, modifiers: Modifiers) {
    harness.event(Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers });
    harness.step();
    harness.event(Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers });
    harness.run();
}

fn click_with(harness: &mut Harness<'_, State>, pos: Pos2, button: PointerButton) {
    harness.hover_at(pos);
    harness.step();
    for pressed in [true, false] {
        harness.event(Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE });
    }
    harness.run();
}

fn click(harness: &mut Harness<'_, State>, pos: Pos2) {
    click_with(harness, pos, PointerButton::Primary);
}

fn empty_spot(harness: &Harness<'_, State>) -> Pos2 {
    harness.state().editor.canvas_to_screen(egui::pos2(200.0, 300.0))
}

fn title(harness: &Harness<'_, State>, node: u32) -> Pos2 {
    let state = harness.state();
    state.editor.node_screen_pos(&state.graph, state.catalog, node)
}

#[test]
fn dragging_from_an_output_to_an_input_wires_them() {
    let mut harness = harness(graph());
    let (a, b) = (pin(&harness, 1, 0, true), pin(&harness, 2, 0, false));
    drag(&mut harness, a, b);
    assert_eq!(harness.state().graph.links, vec![Link { from: (1, 0), to: (2, 0) }]);
    assert!(harness.state().outcome.changed);
}

#[test]
fn dragging_from_an_input_to_an_output_wires_them_too() {
    let mut harness = harness(graph());
    let (a, b) = (pin(&harness, 2, 0, false), pin(&harness, 1, 0, true));
    drag(&mut harness, a, b);
    assert_eq!(harness.state().graph.links, vec![Link { from: (1, 0), to: (2, 0) }]);
}

#[test]
fn a_number_cannot_go_into_a_trigger() {
    let mut harness = harness(graph());
    let (a, b) = (pin(&harness, 1, 3, true), pin(&harness, 2, 0, false));
    drag(&mut harness, a, b);
    assert!(harness.state().graph.links.is_empty());
}

#[test]
fn dragging_a_wire_off_an_input_unplugs_it() {
    let mut g = graph();
    g.links.push(Link { from: (1, 3), to: (2, 2) });
    let mut harness = harness(g);
    let empty = empty_spot(&harness);
    let a = pin(&harness, 2, 2, false);
    drag(&mut harness, a, empty);
    assert!(harness.state().graph.links.is_empty());
}

#[test]
fn dragging_a_block_moves_it() {
    let mut harness = harness(graph());
    let title = title(&harness, 1);
    let zoom = harness.state().editor.zoom;
    drag(&mut harness, title, title + vec2(80.0, 40.0));
    let pos = harness.state().graph.node(1).unwrap().pos;
    assert!((pos[0] - 80.0 / zoom).abs() < 2.0 && (pos[1] - 40.0 / zoom).abs() < 2.0, "{pos:?}");
}

#[test]
fn space_opens_the_search_and_enter_adds_the_block() {
    let mut harness = harness(graph());
    let spot = empty_spot(&harness);
    harness.hover_at(spot);
    harness.run();
    key(&mut harness, Key::Space, Modifiers::NONE);
    assert!(harness.state().editor.palette.is_some());
    harness.event(Event::Text("kick".into()));
    harness.run();
    key(&mut harness, Key::Enter, Modifiers::NONE);
    let state = harness.state();
    assert!(state.editor.palette.is_none());
    assert_eq!(state.graph.nodes.len(), 3);
    assert_eq!(state.graph.nodes[2].kind, "do.kick");
}

#[test]
fn a_right_click_on_the_canvas_offers_to_add_a_block() {
    let mut harness = harness(graph());
    let spot = empty_spot(&harness);
    click_with(&mut harness, spot, PointerButton::Secondary);
    harness.get_by_label("Add a block…").click();
    harness.run();
    assert!(harness.state().editor.palette.is_some());
    harness.event(Event::Text("number".into()));
    harness.run();
    key(&mut harness, Key::Enter, Modifiers::NONE);
    assert_eq!(harness.state().graph.nodes[2].kind, "number");
}

#[test]
fn escape_closes_the_search() {
    let mut harness = harness(graph());
    let spot = empty_spot(&harness);
    harness.hover_at(spot);
    harness.run();
    key(&mut harness, Key::Space, Modifiers::NONE);
    key(&mut harness, Key::Escape, Modifiers::NONE);
    assert!(harness.state().editor.palette.is_none());
    assert_eq!(harness.state().graph.nodes.len(), 2);
}

#[test]
fn dropping_a_wire_on_empty_space_offers_only_blocks_that_fit() {
    let mut harness = harness(graph());
    let empty = empty_spot(&harness);
    let a = pin(&harness, 1, 0, true);
    drag(&mut harness, a, empty);
    assert!(harness.state().editor.palette.is_some());
    // "When hit" has no trigger input, so it isn't offered; the actions are.
    assert!(harness.query_by_label_contains("When hit").is_none());
    assert!(harness.query_by_label_contains("Grow").is_some());
    // Narrow the list so the row is on screen, then pick it with the mouse.
    harness.event(Event::Text("kick".into()));
    harness.run();
    harness.get_by_label_contains("Kick").click();
    harness.run();
    let state = harness.state();
    let id = state.graph.nodes.iter().find(|n| n.kind == "do.kick").expect("kick added").id;
    assert!(state.graph.links.contains(&Link { from: (1, 0), to: (id, 0) }));
}

#[test]
fn delete_removes_the_selected_block_and_its_wires() {
    let mut g = graph();
    g.links.push(Link { from: (1, 0), to: (2, 0) });
    let mut harness = harness(g);
    let title = title(&harness, 2);
    click(&mut harness, title);
    let spot = empty_spot(&harness);
    harness.hover_at(spot);
    harness.run();
    key(&mut harness, Key::Delete, Modifiers::NONE);
    let state = harness.state();
    assert_eq!(state.graph.nodes.len(), 1);
    assert!(state.graph.links.is_empty());
}

#[test]
fn copy_and_paste_makes_new_blocks_with_their_wires() {
    let mut g = graph();
    g.links.push(Link { from: (1, 0), to: (2, 0) });
    let mut harness = harness(g);
    let spot = empty_spot(&harness);
    harness.hover_at(spot);
    harness.run();
    key(&mut harness, Key::A, Modifiers::COMMAND);
    harness.event(Event::Copy);
    harness.run();
    harness.event(Event::Paste(String::new()));
    harness.run();
    let state = harness.state();
    assert_eq!(state.graph.nodes.len(), 4);
    assert_eq!(state.graph.links.len(), 2);
    let ids: Vec<u32> = state.graph.nodes.iter().map(|n| n.id).collect();
    assert_eq!(ids, vec![1, 2, 3, 4]);
    assert_eq!(state.editor.selection.len(), 2);
}

#[test]
fn ctrl_d_duplicates() {
    let mut harness = harness(graph());
    let title = title(&harness, 2);
    click(&mut harness, title);
    let spot = empty_spot(&harness);
    harness.hover_at(spot);
    harness.run();
    key(&mut harness, Key::D, Modifiers::COMMAND);
    assert_eq!(harness.state().graph.nodes.len(), 3);
}

#[test]
fn box_select_picks_the_blocks_inside() {
    let mut harness = harness(graph());
    let from = harness.state().editor.canvas_to_screen(egui::pos2(-50.0, -50.0));
    let to = harness.state().editor.canvas_to_screen(egui::pos2(250.0, 150.0));
    drag(&mut harness, from, to);
    assert_eq!(harness.state().editor.selection.iter().copied().collect::<Vec<_>>(), vec![1]);
    assert_eq!(harness.state().outcome.selected, None, "a selection alone is not a change");
}

#[test]
fn a_wire_that_makes_a_loop_is_refused() {
    let c = catalog(Role::Rules);
    let mut g = Graph::new("Loop");
    let a = g.add(c.spec("math").unwrap(), [0.0, 0.0]);
    let b = g.add(c.spec("math").unwrap(), [400.0, 0.0]);
    g.links.push(Link { from: (a, 0), to: (b, 0) });
    let mut harness = harness(g);
    let (from, to) = (pin(&harness, b, 0, true), pin(&harness, a, 0, false));
    drag(&mut harness, from, to);
    assert_eq!(harness.state().graph.links.len(), 1);
}

#[test]
fn an_unknown_block_shows_a_problem_badge() {
    let mut g = graph();
    g.nodes[1].kind = "mystery".into();
    let harness = harness(g);
    assert_eq!(harness.get_all_by_label("Problem").count(), 1);
}

#[test]
fn fit_shows_every_block() {
    let mut g = graph();
    g.nodes[1].pos = [3000.0, 2000.0];
    let harness = harness(g);
    let far = title(&harness, 2);
    assert!(harness.state().editor.screen.contains(far), "{far:?} not in {:?}", harness.state().editor.screen);
}

#[test]
fn a_setting_can_be_shown_on_the_card_and_at_simple() {
    let mut harness = harness(graph());
    // Widgets on the canvas report their rects in canvas units.
    let label = harness.get_by_role_and_label(egui::accesskit::Role::Label, "By (px)").rect().center();
    let label = harness.state().editor.canvas_to_screen(label);
    click_with(&mut harness, label, PointerButton::Secondary);
    harness.get_by_label("Show on the card").click();
    harness.run();
    assert!(harness.state().graph.node(2).unwrap().exposed.contains("by"));
    click_with(&mut harness, label, PointerButton::Secondary);
    harness.get_by_label("Show at Simple").click();
    harness.run();
    assert_eq!(harness.state().outcome.simple_toggled, Some((2, "by".to_owned())));
}

#[test]
fn the_target_picker_names_a_missing_thing() {
    let scene = bpe_kit::new_scene(&bpe_kit::registry());
    assert_eq!(target_words(&scene, &Target::Me), "Me");
    assert_eq!(target_words(&scene, &Target::Anything), "Anything");
    assert_eq!(target_words(&scene, &Target::Thing(ThingId(2))), "Ball");
    assert_eq!(target_words(&scene, &Target::Thing(ThingId(99))), "Missing thing");
    assert_eq!(target_words(&scene, &Target::Tag("red".into())), "#red");
    assert_eq!(target_words(&scene, &Target::Kind("balls".into())), "Any balls");
}

#[test]
fn picking_a_target_writes_its_id() {
    struct Picker {
        spec: ParamSpec,
        param: Param,
        scene: Scene,
        changed: bool,
    }
    let spec = catalog(Role::Rules).spec("when.hit").unwrap().param_spec("who").unwrap().clone();
    let scene = bpe_kit::new_scene(&bpe_kit::registry());
    let state = Picker { spec, param: Param::Text("me".into()), scene, changed: false };
    let mut harness = Harness::builder()
        .with_size(vec2(400.0, 300.0))
        .with_max_steps(60)
        .allow_missing_glyphs()
        .with_context_setup(crate::theme::apply)
        .build_ui_state(
            |ui, s: &mut Picker| {
                let label = ui.label("Who");
                if param_control(ui, &s.spec, &mut s.param, &s.scene, label.id) {
                    s.changed = true;
                }
            },
            state,
        );
    harness.run();
    harness.get_by_label("Me").click();
    harness.run();
    harness.get_by_label("Ball").click();
    harness.run();
    assert_eq!(harness.state().param, Param::Text("thing:2".into()));
    assert!(harness.state().changed);
}

#[test]
fn drawing_a_block_leaves_a_value_beyond_its_range_alone() {
    // A template may hold a value past a slider's end; showing it must not quietly clamp it.
    let c = catalog(Role::Rules);
    let mut graph = Graph::new("Test");
    let counter = graph.add(c.spec("do.counter").unwrap(), [0.0, 0.0]);
    graph.node_mut(counter).unwrap().params.insert("id".into(), Param::Number(99.0));
    let setting = graph.add(c.spec("setting").unwrap(), [0.0, 300.0]);
    graph.node_mut(setting).unwrap().params.insert("max".into(), Param::Number(20000.0));
    let harness = harness(graph);
    let state = harness.state();
    assert_eq!(state.outcome, Outcome::default(), "nothing was edited");
    assert_eq!(state.graph.node(counter).unwrap().params.get("id"), Some(&Param::Number(99.0)));
    assert_eq!(state.graph.node(setting).unwrap().params.get("max"), Some(&Param::Number(20000.0)));
}
