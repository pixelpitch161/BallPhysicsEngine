//! Tests of the Piece and Pen tools and of Select on pieces and walls (Water mode part 3).

use super::rail::Tool;
use super::tests::{button, click_at, drag, harness, of_kind, view, water_studio};
use crate::app::Studio;
use bpe_core::{Settings, ShapePath, SlotId, ThingId};
use bpe_kit::modules::water::pieces::default_size;
use bpe_kit::modules::water::{LineEnds, PieceKind, PieceShape, WallLine};
use bpe_kit::things::water;
use eframe::egui::{self, Pos2, accesskit::Role};
use egui_kittest::{Harness, kittest::Queryable};
use glam::Vec2;

/// Adds a Piece of `shape` at `at` (turned `turn` radians), as the scene's own edit.
fn add_piece(studio: &mut Studio, shape: &PieceShape, at: Vec2, turn: f32) -> ThingId {
    add_solid(studio, water::PIECE, shape.to_params(), at, turn)
}

/// Adds a Wall through `points` (relative to `at`).
fn add_wall(studio: &mut Studio, points: &[[f32; 2]], thickness: f32, at: Vec2) -> ThingId {
    let line = WallLine { points: points.to_vec(), thickness, curved: false, ends: LineEnds::Round, cuts: false };
    add_solid(studio, water::WALL, line.to_params(), at, 0.0)
}

fn add_solid(studio: &mut Studio, kind: &str, params: bpe_core::Params, at: Vec2, turn: f32) -> ThingId {
    let kit = studio.kit.clone();
    let id = bpe_kit::add_thing(&mut studio.scene, &kit.registry, kind, ThingId::ROOT).unwrap();
    let thing = studio.scene.things.get_mut(&id).unwrap();
    thing.transform.position = at;
    thing.transform.rotation = turn;
    thing.slots.get_mut(&SlotId::Form).unwrap().params = params;
    studio.scene_changed();
    id
}

fn pillar() -> PieceShape {
    PieceShape { piece: PieceKind::Pillar, size: default_size(PieceKind::Pillar), ..PieceShape::default() }
}

fn shape(h: &Harness<Studio>, id: ThingId) -> PieceShape {
    PieceShape::from_params(&h.state().scene.things[&id].slots[&SlotId::Form].params)
}

fn line(h: &Harness<Studio>, id: ThingId) -> WallLine {
    WallLine::from_params(&h.state().scene.things[&id].slots[&SlotId::Form].params)
}

fn place(h: &Harness<Studio>, id: ThingId) -> Vec2 {
    h.state().scene.things[&id].transform.position
}

/// The tool picked on the rail.
fn with_tool<'a>(studio: Studio, tool: Tool) -> Harness<'a, Studio> {
    let mut h = harness(studio);
    h.get_by_role_and_label(Role::Button, tool.name()).click();
    h.run();
    assert_eq!(h.state().water_stage.tool, tool);
    h
}

/// A click with Alt held.
fn alt_click(h: &mut Harness<Studio>, at: Pos2) {
    h.hover_at(at);
    h.step();
    let (alt, primary) = (egui::Modifiers::ALT, egui::PointerButton::Primary);
    h.event(egui::Event::ModifiersChanged(alt));
    h.event(egui::Event::PointerButton { pos: at, button: primary, pressed: true, modifiers: alt });
    h.step();
    h.event(egui::Event::PointerButton { pos: at, button: primary, pressed: false, modifiers: alt });
    h.step();
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    h.run();
}

/// Lets half a second go by, so the next click is not taken with the last one.
fn wait(h: &mut Harness<Studio>) {
    for _ in 0..30 {
        h.step();
    }
}

/// Two quick clicks at one spot.
fn double_click(h: &mut Harness<Studio>, at: Pos2) {
    wait(h);
    h.hover_at(at);
    h.step();
    for _ in 0..2 {
        button(h, at, true);
        button(h, at, false);
    }
    h.run();
}

fn snapping_studio(spacing: f32) -> Studio {
    let mut studio = Studio::headless_water();
    studio.grid.snap = true;
    studio.grid.spacing = spacing;
    studio
}

// The Piece tool.

#[test]
fn the_palette_places_each_kind_with_its_size() {
    let mut h = with_tool(water_studio(), Tool::Piece);
    h.get_by_role_and_label(Role::Button, "Funnel").click();
    h.run();
    assert_eq!(h.state().water_stage.piece.kind, PieceKind::Funnel);
    let v = view(&h);
    let at = Vec2::new(100.0, -300.0);
    click_at(&mut h, v.to_screen(at));
    let pieces = of_kind(h.state(), water::PIECE);
    assert_eq!(pieces.len(), 1, "one Piece");
    let piece = pieces[0];
    let s = shape(&h, piece);
    assert_eq!((s.piece, s.size, s.flip, s.cuts), (PieceKind::Funnel, default_size(PieceKind::Funnel), false, false));
    assert!(place(&h, piece).distance(at) < 0.6, "at the click: {}", place(&h, piece));
    assert_eq!(h.state().scene.things[&piece].name, "Funnel");
    assert_eq!(h.state().selection.primary, Some(piece), "the new piece is picked");
    assert_eq!(h.state().water_stage.tool, Tool::Piece, "the tool stays in hand");
    // A second click places a second one; each is one undo step.
    click_at(&mut h, v.to_screen(Vec2::new(-200.0, 300.0)));
    assert_eq!(of_kind(h.state(), water::PIECE).len(), 2);
    assert!(h.state_mut().undo());
    assert_eq!(of_kind(h.state(), water::PIECE), [piece]);
    assert!(h.state_mut().undo());
    assert!(of_kind(h.state(), water::PIECE).is_empty());
}

#[test]
fn a_piece_snaps_to_another_ones_corner() {
    let mut studio = snapping_studio(25.0);
    add_piece(&mut studio, &pillar(), Vec2::ZERO, 0.0);
    let mut h = with_tool(studio, Tool::Piece);
    let v = view(&h);
    // A ramp (300 x 150) has a corner at (-150, 75) of its place: asked for 4 px and 3 px off the
    // pillar's corner at (30, 150), it lands on it.
    let asked = Vec2::new(30.0 + 150.0 + 4.0, 150.0 - 75.0 + 3.0);
    click_at(&mut h, v.to_screen(asked));
    let ramp = *of_kind(h.state(), water::PIECE).last().unwrap();
    assert_eq!(shape(&h, ramp).piece, PieceKind::Ramp);
    assert!(place(&h, ramp).distance(Vec2::new(180.0, 75.0)) < 0.01, "on the corner: {}", place(&h, ramp));
    // With Alt, it lands where it is asked for (no grid either). (The strip has Make custom now,
    // so the course sits a little lower.)
    let v = view(&h);
    alt_click(&mut h, v.to_screen(asked));
    let free = *of_kind(h.state(), water::PIECE).last().unwrap();
    assert_ne!(free, ramp);
    assert!(place(&h, free).distance(asked) < 0.6, "where asked: {}", place(&h, free));
    // Far from any corner, it lands on the grid.
    let v = view(&h);
    click_at(&mut h, v.to_screen(Vec2::new(-260.0, -611.0)));
    let gridded = *of_kind(h.state(), water::PIECE).last().unwrap();
    assert_eq!(place(&h, gridded), Vec2::new(-250.0, -600.0));
}

#[test]
fn corners_snap_with_the_grid_off_too() {
    let mut studio = water_studio();
    assert!(!studio.grid.snap);
    add_piece(&mut studio, &pillar(), Vec2::ZERO, 0.0);
    let mut h = with_tool(studio, Tool::Piece);
    let v = view(&h);
    click_at(&mut h, v.to_screen(Vec2::new(184.0, 78.0)));
    let ramp = *of_kind(h.state(), water::PIECE).last().unwrap();
    assert!(place(&h, ramp).distance(Vec2::new(180.0, 75.0)) < 0.01, "on the corner: {}", place(&h, ramp));
    // A pen point too.
    h.get_by_role_and_label(Role::Button, "Pen").click();
    h.run();
    let v = view(&h);
    click_at(&mut h, v.to_screen(Vec2::new(-33.0, -147.0)));
    let first = h.state().water_stage.line.as_ref().and_then(|l| l.first().copied());
    assert_eq!(first, Some(Vec2::new(-30.0, -150.0)));
}

#[test]
fn a_dragged_wall_keeps_its_points_on_the_grid() {
    let mut studio = snapping_studio(60.0);
    // Points at (0, 0) and (60, 0): its place, their middle, is half a step off the grid.
    let wall = add_wall(&mut studio, &[[-30.0, 0.0], [30.0, 0.0]], 24.0, Vec2::new(30.0, 0.0));
    let mut h = harness(studio);
    let v = view(&h);
    let from = v.to_screen(Vec2::new(30.0, 0.0));
    drag(&mut h, from, from + egui::vec2(47.0, 13.0) * v.scale());
    let at = place(&h, wall);
    let points: Vec<Vec2> = line(&h, wall).points.iter().map(|p| at + Vec2::from(*p)).collect();
    assert_eq!(points, [Vec2::new(60.0, 0.0), Vec2::new(120.0, 0.0)], "on the grid");
}

#[test]
fn turn_flip_and_cuts_go_on_the_piece() {
    let mut h = with_tool(water_studio(), Tool::Piece);
    h.get_by_role_and_label(Role::Button, "Basin").click();
    h.get_by_role_and_label(Role::Button, "Flip").click();
    h.get_by_role_and_label(Role::Button, "Cuts").click();
    h.run();
    h.state_mut().water_stage.piece.turn_deg = 30.0;
    h.run();
    let v = view(&h);
    click_at(&mut h, v.to_screen(Vec2::new(0.0, 200.0)));
    let piece = of_kind(h.state(), water::PIECE)[0];
    let s = shape(&h, piece);
    assert_eq!((s.piece, s.flip, s.cuts), (PieceKind::Basin, true, true));
    let turn = h.state().scene.things[&piece].transform.rotation;
    assert!((turn - 30f32.to_radians()).abs() < 1e-6, "{turn}");
}

#[test]
fn make_custom_turns_a_ramp_into_its_outline() {
    let mut studio = water_studio();
    let ramp_shape = PieceShape { piece: PieceKind::Ramp, size: Vec2::new(300.0, 150.0), ..PieceShape::default() };
    let ramp = add_piece(&mut studio, &ramp_shape, Vec2::ZERO, 0.0);
    let funnel_shape = PieceShape { piece: PieceKind::Funnel, ..ramp_shape.clone() };
    let funnel = add_piece(&mut studio, &funnel_shape, Vec2::new(0.0, 500.0), 0.0);
    studio.selection.set(funnel);
    let mut h = harness(studio);
    assert!(h.query_by_label("Make custom").is_none(), "a funnel is several parts: no Make custom");
    h.state_mut().selection.set(ramp);
    h.run();
    h.get_by_role_and_label(Role::Button, "Make custom").click();
    h.run();
    let s = shape(&h, ramp);
    assert_eq!(s.piece, PieceKind::Custom);
    assert_eq!(s.path, ramp_shape.outline().unwrap(), "its own outline");
    assert_eq!(s.size, ramp_shape.size);
    assert!(h.query_by_label("Make custom").is_none(), "a Custom piece is custom already");
    assert!(h.state_mut().undo());
    assert_eq!(shape(&h, ramp).piece, PieceKind::Ramp, "one undo step");
}

// The Pen tool.

#[test]
fn three_clicks_and_enter_make_a_wall() {
    let mut h = with_tool(water_studio(), Tool::Pen);
    h.state_mut().water_stage.pen.thickness = 30.0;
    h.run();
    let v = view(&h);
    for p in [Vec2::new(-100.0, 0.0), Vec2::new(0.0, -50.0), Vec2::new(100.0, 0.0)] {
        wait(&mut h);
        click_at(&mut h, v.to_screen(p));
    }
    assert!(of_kind(h.state(), water::WALL).is_empty(), "not yet");
    assert_eq!(h.state().water_stage.line.as_ref().map(Vec::len), Some(3));
    h.key_press(egui::Key::Enter);
    h.run();
    let walls = of_kind(h.state(), water::WALL);
    assert_eq!(walls.len(), 1, "one Wall");
    let wall = walls[0];
    assert!(place(&h, wall).distance(Vec2::new(0.0, -25.0)) < 0.6, "at its points' middle: {}", place(&h, wall));
    let l = line(&h, wall);
    let want = [[-100.0, 25.0], [0.0, -25.0], [100.0, 25.0]];
    for (got, want) in l.points.iter().zip(want) {
        assert!(Vec2::from(*got).distance(Vec2::from(want)) < 1.0, "{:?}", l.points);
    }
    assert_eq!((l.points.len(), l.thickness, l.ends, l.cuts), (3, 30.0, LineEnds::Round, false));
    assert_eq!(h.state().selection.primary, Some(wall));
    assert_eq!(h.state().water_stage.tool, Tool::Pen, "the tool stays in hand");
    assert!(h.state().water_stage.line.is_none());
    assert!(h.state_mut().undo());
    assert!(of_kind(h.state(), water::WALL).is_empty(), "one undo step");
}

#[test]
fn a_double_click_finishes_a_wall() {
    let mut h = with_tool(water_studio(), Tool::Pen);
    h.get_by_role_and_label(Role::Button, "Square").click();
    h.get_by_role_and_label(Role::Button, "Cuts").click();
    h.run();
    let v = view(&h);
    click_at(&mut h, v.to_screen(Vec2::new(-200.0, 100.0)));
    double_click(&mut h, v.to_screen(Vec2::new(200.0, 100.0)));
    let walls = of_kind(h.state(), water::WALL);
    assert_eq!(walls.len(), 1);
    let l = line(&h, walls[0]);
    assert_eq!((l.points.len(), l.ends, l.cuts), (2, LineEnds::Square, true), "{:?}", l.points);
}

#[test]
fn escape_drops_the_line_and_leaves_no_undo_step() {
    let mut h = with_tool(water_studio(), Tool::Pen);
    let v = view(&h);
    click_at(&mut h, v.to_screen(Vec2::new(-200.0, 100.0)));
    wait(&mut h);
    click_at(&mut h, v.to_screen(Vec2::new(200.0, 100.0)));
    h.key_press(egui::Key::Escape);
    h.run();
    assert!(h.state().water_stage.line.is_none());
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(of_kind(h.state(), water::WALL).is_empty());
    assert!(!h.state_mut().undo(), "nothing to undo");
    // Another tool drops a line too.
    click_at(&mut h, v.to_screen(Vec2::new(-200.0, 100.0)));
    assert!(h.state().water_stage.line.is_some());
    h.get_by_role_and_label(Role::Button, "Select").click();
    h.run();
    assert!(h.state().water_stage.line.is_none());
}

#[test]
fn one_point_makes_no_wall() {
    let mut h = with_tool(water_studio(), Tool::Pen);
    let v = view(&h);
    click_at(&mut h, v.to_screen(Vec2::new(0.0, 100.0)));
    wait(&mut h);
    click_at(&mut h, v.to_screen(Vec2::new(0.0, 100.0)));
    assert_eq!(h.state().water_stage.line.as_ref().map(Vec::len), Some(1), "two clicks on one spot are one point");
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(of_kind(h.state(), water::WALL).is_empty());
    assert!(h.state().water_stage.line.is_none());
    assert!(!h.state_mut().undo());
}

#[test]
fn a_pen_point_snaps_to_a_pieces_corner() {
    let mut studio = snapping_studio(25.0);
    add_piece(&mut studio, &pillar(), Vec2::ZERO, 0.0);
    let mut h = with_tool(studio, Tool::Pen);
    let v = view(&h);
    click_at(&mut h, v.to_screen(Vec2::new(33.0, -147.0)));
    wait(&mut h);
    click_at(&mut h, v.to_screen(Vec2::new(310.0, -161.0)));
    h.key_press(egui::Key::Enter);
    h.run();
    let wall = of_kind(h.state(), water::WALL)[0];
    let at = place(&h, wall);
    let points: Vec<Vec2> = line(&h, wall).points.iter().map(|p| at + Vec2::from(*p)).collect();
    assert!(points[0].distance(Vec2::new(30.0, -150.0)) < 0.01, "on the pillar's corner: {points:?}");
    assert!(points[1].distance(Vec2::new(300.0, -150.0)) < 0.01, "on the grid: {points:?}");
}

// Select on pieces and walls.

#[test]
fn a_click_on_a_pieces_body_picks_it() {
    let mut studio = water_studio();
    let piece = add_piece(&mut studio, &pillar(), Vec2::ZERO, 0.0);
    let mut h = harness(studio);
    let v = view(&h);
    // Far from its marker (at its middle), on its body.
    click_at(&mut h, v.to_screen(Vec2::new(0.0, 120.0)));
    assert_eq!(h.state().selection.primary, Some(piece));
    click_at(&mut h, v.to_screen(Vec2::new(200.0, 120.0)));
    assert_eq!(h.state().selection.primary, None, "beside it lets go");
}

#[test]
fn dragging_a_piece_moves_it_with_snapping() {
    let mut studio = snapping_studio(25.0);
    add_piece(&mut studio, &pillar(), Vec2::ZERO, 0.0);
    let moved = add_piece(&mut studio, &pillar(), Vec2::new(200.0, 0.0), 0.0);
    let mut h = harness(studio);
    let v = view(&h);
    // Moved by (-136, 3) its corners come 5 px from the first pillar's: it lands touching it.
    let from = v.to_screen(Vec2::new(200.0, 100.0));
    drag(&mut h, from, from + egui::vec2(-136.0, 3.0) * v.scale());
    assert_eq!(h.state().selection.primary, Some(moved));
    assert!(place(&h, moved).distance(Vec2::new(60.0, 0.0)) < 0.01, "{}", place(&h, moved));
    assert!(h.state_mut().undo());
    assert_eq!(place(&h, moved), Vec2::new(200.0, 0.0), "the drag is one undo step");
}

#[test]
fn dragging_a_walls_point_moves_only_that_point() {
    let mut studio = water_studio();
    let wall = add_wall(&mut studio, &[[-100.0, 0.0], [0.0, 0.0], [100.0, 50.0]], 24.0, Vec2::new(0.0, 300.0));
    studio.selection.set(wall);
    let mut h = harness(studio);
    let v = view(&h);
    let (points, ..) = super::solid_edit::handle_points(h.state(), &v).unwrap();
    assert_eq!(points.len(), 3);
    assert!(points[1].distance(v.to_screen(Vec2::new(0.0, 300.0))) < 0.01);
    drag(&mut h, points[1], v.to_screen(Vec2::new(20.0, 340.0)));
    let l = line(&h, wall);
    assert_eq!(l.points[0], [-100.0, 0.0]);
    assert_eq!(l.points[2], [100.0, 50.0]);
    assert!(Vec2::from(l.points[1]).distance(Vec2::new(20.0, 40.0)) < 0.6, "{:?}", l.points);
    assert_eq!(place(&h, wall), Vec2::new(0.0, 300.0), "the wall stays put");
    assert!(h.state_mut().undo());
    assert_eq!(line(&h, wall).points[1], [0.0, 0.0], "one undo step");
}

#[test]
fn double_clicks_add_and_remove_points() {
    let mut studio = water_studio();
    let wall = add_wall(&mut studio, &[[-100.0, 0.0], [100.0, 0.0]], 24.0, Vec2::ZERO);
    studio.selection.set(wall);
    let mut h = harness(studio);
    let v = view(&h);
    // A wall keeps two points.
    double_click(&mut h, v.to_screen(Vec2::new(100.0, 0.0)));
    assert_eq!(line(&h, wall).points.len(), 2);
    // On its line, one more.
    double_click(&mut h, v.to_screen(Vec2::new(10.0, 1.0)));
    let l = line(&h, wall);
    assert_eq!(l.points.len(), 3, "{:?}", l.points);
    assert!(Vec2::from(l.points[1]).distance(Vec2::new(10.0, 0.0)) < 1.0, "{:?}", l.points);
    assert!(h.state_mut().undo());
    assert_eq!(line(&h, wall).points.len(), 2, "one undo step");
    assert!(h.state_mut().redo());
    // On the new point, it goes again.
    double_click(&mut h, v.to_screen(Vec2::new(10.0, 0.0)));
    assert_eq!(line(&h, wall).points.len(), 2);

    // A Custom piece keeps three.
    let custom = PieceShape {
        piece: PieceKind::Custom,
        size: Vec2::new(200.0, 200.0),
        path: ShapePath::even(4),
        ..PieceShape::default()
    };
    let studio = h.state_mut();
    let piece = add_piece(studio, &custom, Vec2::new(0.0, 500.0), 0.0);
    studio.selection.set(piece);
    h.run();
    let corners = |h: &Harness<Studio>| shape(h, piece).path.points.len();
    let start = corners(&h);
    assert_eq!(start, 4, "a square");
    for want in [3, 3] {
        let (points, ..) = super::solid_edit::handle_points(h.state(), &v).unwrap();
        double_click(&mut h, points[0]);
        assert_eq!(corners(&h), want);
    }
}

#[test]
fn the_turn_dot_turns_in_fifteen_degree_steps() {
    let mut studio = water_studio();
    let piece = add_piece(&mut studio, &pillar(), Vec2::ZERO, 0.0);
    studio.selection.set(piece);
    let mut h = harness(studio);
    let v = view(&h);
    let (_, turn, _) = super::solid_edit::handle_points(h.state(), &v).unwrap();
    let dot = turn.expect("a piece has a turn dot");
    assert!(dot.y < v.to_screen(Vec2::new(0.0, -150.0)).y, "above it");
    // To 50° clockwise of straight up: 45°.
    let (s, c) = 50f32.to_radians().sin_cos();
    let to = v.to_screen(Vec2::new(s, -c) * 200.0);
    drag(&mut h, dot, to);
    let turn = h.state().scene.things[&piece].transform.rotation;
    assert!((turn - 45f32.to_radians()).abs() < 1e-4, "{}", turn.to_degrees());
    assert_eq!(place(&h, piece), Vec2::ZERO, "it turns where it is");
    assert!(h.state_mut().undo());
    assert_eq!(h.state().scene.things[&piece].transform.rotation, 0.0, "one undo step");
}

#[test]
fn the_size_dot_resizes_in_the_pieces_frame() {
    let mut studio = water_studio();
    let basin = PieceShape { piece: PieceKind::Basin, size: Vec2::new(300.0, 150.0), ..PieceShape::default() };
    let piece = add_piece(&mut studio, &basin, Vec2::ZERO, 90f32.to_radians());
    studio.selection.set(piece);
    let mut h = harness(studio);
    let v = view(&h);
    let (_, _, size) = super::solid_edit::handle_points(h.state(), &v).unwrap();
    let dot = size.expect("a piece has a size dot");
    // Turned a quarter clockwise, its box's corner (150, 75) is at (-75, 150) in the world.
    assert!(dot.distance(v.to_screen(Vec2::new(-75.0, 150.0))) < 0.5, "{dot:?}");
    drag(&mut h, dot, v.to_screen(Vec2::new(-100.0, 200.0)));
    let s = shape(&h, piece);
    assert!(s.size.distance(Vec2::new(400.0, 200.0)) < 1.5, "{}", s.size);
    assert_eq!(place(&h, piece), Vec2::ZERO);
}

#[test]
fn a_picked_thin_wall_says_so() {
    let mut studio = water_studio();
    let wall = add_wall(&mut studio, &[[-100.0, 0.0], [100.0, 0.0]], 4.0, Vec2::ZERO);
    let thick = add_wall(&mut studio, &[[-100.0, 0.0], [100.0, 0.0]], 40.0, Vec2::new(0.0, 300.0));
    studio.selection.set(thick);
    let mut h = harness(studio);
    assert!(h.query_by_label(water::draft::THIN).is_none());
    h.state_mut().selection.set(wall);
    h.run();
    assert!(h.query_by_label(water::draft::THIN).is_some());
}
