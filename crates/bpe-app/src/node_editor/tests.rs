//! Drives the block canvas with simulated mouse and keys.

use super::{NodeEditor, PinRef};
use bpe_graph::{Graph, Link, NodeSpec, ParamSpec, PinSpec, PinType, SpecList};
use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2, vec2};
use egui_kittest::{Harness, kittest::Queryable};

struct State {
    editor: NodeEditor,
    graph: Graph,
    catalog: SpecList,
}

fn catalog() -> SpecList {
    let t = PinType::Trigger;
    SpecList {
        specs: vec![
            NodeSpec::new("on_hit", "When a ball hits", "When", "Fires on every bounce")
                .output(PinSpec::new("then", t))
                .output(PinSpec::new("speed", PinType::Number)),
            NodeSpec::new("flash", "Flash the screen", "Do", "A quick flash")
                .input(PinSpec::new("do", t))
                .param(ParamSpec::number("strength", "Strength", 1.0, 0.0, 4.0)),
            NodeSpec::new("spawn", "Spawn balls", "Do", "Adds balls")
                .input(PinSpec::new("do", t))
                .input(PinSpec::new("count", PinType::Number))
                .output(PinSpec::new("then", t)),
        ],
        cycles: false,
    }
}

fn graph() -> Graph {
    let catalog = catalog();
    let mut graph = Graph::new("Test");
    graph.add(&catalog.specs[0], [0.0, 0.0]);
    graph.add(&catalog.specs[1], [400.0, 0.0]);
    graph
}

fn harness(graph: Graph) -> Harness<'static, State> {
    let state = State { editor: NodeEditor::new("test"), graph, catalog: catalog() };
    let mut harness = Harness::builder()
        .with_size(vec2(1000.0, 700.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(120)
        .with_context_setup(crate::theme::apply)
        .build_ui_state(
            |ui, state: &mut State| {
                state.editor.show(ui, &mut state.graph, &state.catalog, None);
            },
            state,
        );
    harness.run();
    harness
}

fn pin(harness: &Harness<'_, State>, node: u32, pin: u8, output: bool) -> Pos2 {
    let state = harness.state();
    state.editor.pin_screen_pos(&state.graph, &state.catalog, PinRef { node, pin, output })
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

fn click(harness: &mut Harness<'_, State>, pos: Pos2) {
    harness.hover_at(pos);
    harness.step();
    for pressed in [true, false] {
        harness.event(Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
    }
    harness.run();
}

fn empty_spot(harness: &Harness<'_, State>) -> Pos2 {
    harness.state().editor.canvas_to_screen(egui::pos2(200.0, 300.0))
}

#[test]
fn dragging_from_an_output_to_an_input_wires_them() {
    let mut harness = harness(graph());
    let (a, b) = (pin(&harness, 1, 0, true), pin(&harness, 2, 0, false));
    drag(&mut harness, a, b);
    assert_eq!(harness.state().graph.links, vec![Link { from: (1, 0), to: (2, 0) }]);
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
    let (a, b) = (pin(&harness, 1, 1, true), pin(&harness, 2, 0, false));
    drag(&mut harness, a, b);
    assert!(harness.state().graph.links.is_empty());
}

#[test]
fn dragging_a_wire_off_an_input_unplugs_it() {
    let mut g = graph();
    g.links.push(Link { from: (1, 1), to: (2, 0) });
    // Make the input a number so it holds one wire, by using the spawn block's count.
    let spawn = catalog().specs[2].clone();
    let id = g.add(&spawn, [400.0, 250.0]);
    g.links = vec![Link { from: (1, 1), to: (id, 1) }];
    let mut harness = harness(g);
    let empty = empty_spot(&harness);
    let (a, b) = (pin(&harness, id, 1, false), empty);
    drag(&mut harness, a, b);
    assert!(harness.state().graph.links.is_empty());
}

#[test]
fn dragging_a_block_moves_it() {
    let mut harness = harness(graph());
    let state = harness.state();
    let title = state.editor.node_screen_pos(&state.graph, &state.catalog, 1);
    let zoom = state.editor.zoom;
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
    harness.event(Event::Text("spawn".into()));
    harness.run();
    key(&mut harness, Key::Enter, Modifiers::NONE);
    let state = harness.state();
    assert!(state.editor.palette.is_none());
    assert_eq!(state.graph.nodes.len(), 3);
    assert_eq!(state.graph.nodes[2].kind, "spawn");
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
    let (a, b) = (pin(&harness, 1, 0, true), empty);
    drag(&mut harness, a, b);
    assert!(harness.state().editor.palette.is_some());
    // "When a ball hits" has no trigger input, so it isn't offered.
    assert!(harness.query_by_label("When a ball hits").is_none());
    assert!(harness.query_by_label("Flash the screen").is_some());
    harness.get_by_label("Spawn balls").click();
    harness.run();
    let state = harness.state();
    let id = state.graph.nodes.iter().find(|n| n.kind == "spawn").expect("spawn added").id;
    assert!(state.graph.links.contains(&Link { from: (1, 0), to: (id, 0) }));
}

#[test]
fn delete_removes_the_selected_block_and_its_wires() {
    let mut g = graph();
    g.links.push(Link { from: (1, 0), to: (2, 0) });
    let mut harness = harness(g);
    let state = harness.state();
    let title = state.editor.node_screen_pos(&state.graph, &state.catalog, 2);
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
    let state = harness.state();
    let title = state.editor.node_screen_pos(&state.graph, &state.catalog, 2);
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
}

#[test]
fn a_wire_that_makes_a_loop_is_refused() {
    let catalog = catalog();
    let mut g = Graph::new("Loop");
    let a = g.add(&catalog.specs[2], [0.0, 0.0]);
    let b = g.add(&catalog.specs[2], [400.0, 0.0]);
    g.links.push(Link { from: (a, 0), to: (b, 0) });
    let mut harness = harness(g);
    let (a, b) = (pin(&harness, b, 0, true), pin(&harness, a, 0, false));
    drag(&mut harness, a, b);
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
    let state = harness.state();
    let far = state.editor.node_screen_pos(&state.graph, &state.catalog, 2);
    assert!(state.editor.screen.contains(far), "{far:?} not in {:?}", state.editor.screen);
}
