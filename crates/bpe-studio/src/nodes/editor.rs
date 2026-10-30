//! A canvas of blocks and wires, shared by every module's Nodes depth.
//!
//! The editor knows nothing about what the blocks do: it is drawn from a [`Catalog`] and edits a
//! [`Graph`] in place, so every change goes through the studio's normal undo. A [`LiveView`]
//! lets the caller light up blocks while the scene plays.
//!
//! Mouse: drag a block to move it, drag from a pin to wire it, drag empty space to select, middle
//! or right drag (or the scroll wheel with Shift) to pan, scroll to zoom. Keys while the pointer
//! is over the canvas: Space or a double-click to add a block, Delete, Ctrl+C / Ctrl+V / Ctrl+D,
//! Ctrl+A, Ctrl+G to frame the selection with a note, F to fit the view.

use super::draw;
use super::palette::{self, Palette};
use super::params::param_control;
use crate::theme;
use bpe_core::Scene;
use bpe_graph::{Catalog, Graph, Link, Node, NodeSpec, Note, ParamSpec, PinType, validate};
use eframe::egui::{
    self, Color32, CornerRadius, Event, Id, Key, LayerId, Modifiers, PointerButton, Pos2, Rect, Sense, Stroke,
    StrokeKind, UiBuilder, Vec2, emath::TSTransform, pos2, vec2,
};
use std::collections::BTreeSet;
use std::sync::Arc;

pub const NODE_WIDTH: f32 = 200.0;
const HEADER: f32 = 28.0;
const ROW: f32 = 22.0;
const PARAM_ROW: f32 = 24.0;
const PIN_RADIUS: f32 = 5.5;
/// How close, in canvas units, the pointer must be to a pin to grab or drop on it.
const PIN_REACH: f32 = 12.0;
const NOTE_TITLE: f32 = 24.0;
const ZOOM: (f32, f32) = (0.2, 2.0);

/// What the caller knows about a block while the scene runs.
pub trait LiveView {
    /// 0 to 1: how brightly the block glows because it just fired.
    fn flash(&self, _node: u32) -> f32 {
        0.0
    }
    /// A short value shown above the block, such as a counter.
    fn badge(&self, _node: u32) -> Option<String> {
        None
    }
    /// A problem found while running, shown like a wiring problem.
    fn error(&self, _node: u32) -> Option<String> {
        None
    }
}

/// What happened in the editor this frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outcome {
    /// The graph was edited.
    pub changed: bool,
    /// The one selected block, if exactly one is selected.
    pub selected: Option<u32>,
    /// A setting whose "Show at Simple" was toggled: (block, setting name).
    pub simple_toggled: Option<(u32, String)>,
}

/// One end of a wire: an input or output of a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PinRef {
    pub node: u32,
    pub pin: u8,
    pub output: bool,
}

impl PinRef {
    fn key(self) -> (u32, u8) {
        (self.node, self.pin)
    }
}

#[derive(Clone, Debug, Default)]
enum Drag {
    #[default]
    None,
    /// Moving the selected blocks.
    Nodes,
    /// Selecting with a rectangle, from this canvas point.
    Box {
        start: Pos2,
        add: bool,
    },
    /// Pulling a new wire out of a pin.
    Wire {
        from: PinRef,
    },
    /// Moving a note and the blocks that sit on it.
    Note {
        index: usize,
        carried: Vec<u32>,
    },
    ResizeNote {
        index: usize,
    },
}

/// What Ctrl+C keeps, shared by every editor so blocks can move between graphs.
#[derive(Clone, Debug, Default)]
struct Clipboard {
    nodes: Vec<Node>,
    links: Vec<Link>,
}

/// Where each part of a block is, in canvas units.
#[derive(Clone, Debug)]
struct Layout {
    rect: Rect,
    inputs: Vec<Pos2>,
    outputs: Vec<Pos2>,
    params_top: f32,
}

pub struct Editor {
    id: Id,
    /// Canvas offset in screen points, and scale.
    pan: Vec2,
    pub(super) zoom: f32,
    fitted: bool,
    pub(super) selection: BTreeSet<u32>,
    selected_note: Option<usize>,
    selected_link: Option<Link>,
    editing_note: Option<usize>,
    drag: Drag,
    pub(super) palette: Option<Palette>,
    /// Where the canvas was right-clicked, in canvas units: the menu's adds and pastes go there.
    menu_at: Pos2,
    /// Where the pointer last was over the canvas, in canvas units, for the "Add block…" button.
    last_pointer: Option<Pos2>,
    pub show_minimap: bool,
    /// A short explanation, such as why a wire was refused, and when it was shown.
    message: Option<(String, f64)>,
    /// The canvas transform and screen rect from the last frame, for tests and the palette.
    to_screen: TSTransform,
    pub(super) screen: Rect,
}

fn keyboard_id() -> Id {
    Id::unique("node editor wants keys")
}

/// Whether a node editor under the pointer took Space, Delete and arrow keys last frame, so
/// the studio's own shortcuts (play, step) should leave them alone. Clears the answer, so a
/// canvas that is no longer drawn doesn't keep the keys.
pub fn take_wants_keyboard(ctx: &egui::Context) -> bool {
    ctx.data_mut(|d| d.remove_temp::<bool>(keyboard_id())).unwrap_or(false)
}

impl Editor {
    pub fn new(id: impl std::hash::Hash + std::fmt::Debug) -> Self {
        Self {
            id: Id::unique("node editor").with(id),
            pan: Vec2::ZERO,
            zoom: 1.0,
            fitted: false,
            selection: BTreeSet::new(),
            selected_note: None,
            selected_link: None,
            editing_note: None,
            drag: Drag::None,
            palette: None,
            menu_at: Pos2::ZERO,
            last_pointer: None,
            show_minimap: true,
            message: None,
            to_screen: TSTransform::IDENTITY,
            screen: Rect::NOTHING,
        }
    }

    /// Fit the whole graph in view on the next frame.
    pub fn refit(&mut self) {
        self.fitted = false;
    }

    /// Opens the add-a-block search where the pointer last was over the canvas, or at the
    /// middle when that spot is out of view.
    pub fn open_palette(&mut self) {
        let to_canvas = self.to_screen.inverse();
        let at = self
            .last_pointer
            .filter(|p| self.screen.contains(self.to_screen * *p))
            .unwrap_or_else(|| to_canvas * self.screen.center());
        self.palette = Some(Palette::new(at, None));
    }

    fn say(&mut self, ctx: &egui::Context, message: impl Into<String>) {
        self.message = Some((message.into(), ctx.input(|i| i.time)));
    }

    /// Where a pin is on screen, for tests.
    #[cfg(test)]
    pub(super) fn pin_screen_pos(&self, graph: &Graph, catalog: &dyn Catalog, pin: PinRef) -> Pos2 {
        let node = graph.node(pin.node).expect("node");
        let layout = layout(node, catalog.spec(&node.kind));
        let local = if pin.output { layout.outputs[pin.pin as usize] } else { layout.inputs[pin.pin as usize] };
        self.to_screen * local
    }

    /// Where a block's title bar is on screen, for tests.
    #[cfg(test)]
    pub(super) fn node_screen_pos(&self, graph: &Graph, catalog: &dyn Catalog, node: u32) -> Pos2 {
        let node = graph.node(node).expect("node");
        let layout = layout(node, catalog.spec(&node.kind));
        self.to_screen * pos2(layout.rect.center().x, layout.rect.top() + HEADER / 2.0)
    }

    #[cfg(test)]
    pub(super) fn canvas_to_screen(&self, pos: Pos2) -> Pos2 {
        self.to_screen * pos
    }

    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        graph: &mut Graph,
        catalog: &dyn Catalog,
        live: Option<&dyn LiveView>,
        scene: &Scene,
    ) -> Outcome {
        let ctx = ui.ctx().clone();
        let before = graph.clone();
        let mut outcome = Outcome::default();
        let (screen, background) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.screen = screen;
        self.forget_missing(graph);

        let layouts: Vec<Layout> = graph.nodes.iter().map(|n| layout(n, catalog.spec(&n.kind))).collect();
        if !self.fitted && screen.width() > 10.0 {
            self.fit(graph, &layouts, screen);
            self.fitted = true;
        }

        // ---------- pan and zoom ----------
        let pointer = ctx.input(|i| i.pointer.latest_pos());
        let canvas_layer = LayerId::new(ui.layer_id().order, self.id.with("canvas"));
        let overlay_layer = LayerId::new(ui.layer_id().order, self.id.with("overlay"));
        let over_canvas = pointer.is_some_and(|p| {
            screen.contains(p)
                && ctx.layer_id_at(p).is_none_or(|l| l == ui.layer_id() || l == canvas_layer || l == overlay_layer)
        });
        if background.dragged_by(PointerButton::Middle) || background.dragged_by(PointerButton::Secondary) {
            self.pan += background.drag_delta();
        }
        if over_canvas && let Some(p) = pointer {
            let (scroll, zoom_delta, shift) = ctx.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.modifiers.shift));
            let factor = if shift { 1.0 } else { zoom_delta * (scroll.y / 400.0).exp() };
            if shift {
                self.pan += vec2(scroll.x + scroll.y, 0.0);
            }
            if factor != 1.0 {
                let new_zoom = (self.zoom * factor).clamp(ZOOM.0, ZOOM.1);
                let anchor = self.to_screen.inverse() * p;
                self.zoom = new_zoom;
                self.pan = p - screen.min - anchor.to_vec2() * new_zoom;
            }
            if scroll != Vec2::ZERO {
                ctx.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
            }
        }
        self.to_screen = TSTransform::new(screen.min.to_vec2() + self.pan, self.zoom);
        let to_canvas = self.to_screen.inverse();
        let pointer_canvas = pointer.map(|p| to_canvas * p);
        if over_canvas {
            self.last_pointer = pointer_canvas;
        }

        // ---------- background ----------
        let painter = ui.painter_at(screen);
        painter.rect_filled(screen, 0.0, theme::BACKGROUND);
        draw::grid(&painter, screen, self.to_screen);

        ctx.set_sublayer(ui.layer_id(), canvas_layer);
        ctx.set_transform_layer(canvas_layer, self.to_screen);
        let mut canvas =
            ui.new_child(UiBuilder::new().layer_id(canvas_layer).max_rect(to_canvas * screen).sense(Sense::hover()));
        canvas.set_clip_rect(to_canvas * screen.intersect(ui.clip_rect()));
        let cp = canvas.painter().clone();

        // ---------- notes, behind everything ----------
        self.notes(&mut canvas, graph, &layouts, &mut outcome);

        // ---------- wires ----------
        let diagnostics = validate(graph, catalog);
        let mut hovered_link = None;
        let mut wires = Vec::with_capacity(graph.links.len());
        for (index, link) in graph.links.iter().enumerate() {
            let (Some(a), Some(b)) =
                (pin_pos(graph, &layouts, link.from, true), pin_pos(graph, &layouts, link.to, false))
            else {
                continue;
            };
            let ty = graph.output_type(link.from, catalog).unwrap_or(PinType::Number);
            wires.push((index, a, b, ty));
            if hovered_link.is_none()
                && matches!(self.drag, Drag::None)
                && let Some(p) = pointer_canvas
                && over_canvas
                && draw::near_wire(a, b, p, 6.0 / self.zoom.min(1.0))
            {
                hovered_link = Some(index);
            }
        }
        for &(index, a, b, ty) in &wires {
            let broken = diagnostics.iter().any(|d| d.link == Some(index));
            let mut color = if broken { theme::ERROR } else { pin_color(ty) };
            let mut width = 2.5;
            if self.selected_link == graph.links.get(index).copied() {
                width = 4.5;
                color = Color32::WHITE;
            } else if hovered_link == Some(index) {
                width = 3.5;
            }
            draw::wire(&cp, a, b, Stroke::new(width, color));
        }

        // ---------- blocks ----------
        let mut start_wire: Option<PinRef> = None;
        let mut node_clicked = false;
        let dragging_wire = match &self.drag {
            Drag::Wire { from } => Some(*from),
            _ => None,
        };
        let wire_type = dragging_wire.and_then(|from| pin_type(graph, catalog, from));
        for (index, layout) in layouts.iter().enumerate() {
            let node_id = graph.nodes[index].id;
            let spec = catalog.spec(&graph.nodes[index].kind);
            let selected = self.selection.contains(&node_id);
            let id = self.id.with(("node", node_id));
            let body = canvas.interact(layout.rect, id, Sense::click_and_drag());
            let header = Rect::from_min_size(layout.rect.min, vec2(layout.rect.width(), HEADER));

            // Frame and title.
            let problems: Vec<String> = diagnostics
                .iter()
                .filter(|d| d.node == Some(node_id))
                .map(|d| d.message.clone())
                .chain(live.and_then(|l| l.error(node_id)))
                .collect();
            let node = &graph.nodes[index];
            let accent = spec.map_or(theme::ERROR, |s| category_color(&s.category));
            let flash = live.map_or(0.0, |l| l.flash(node_id)).clamp(0.0, 1.0);
            if flash > 0.0 {
                cp.rect_filled(
                    layout.rect.expand(6.0 * flash),
                    CornerRadius::same(10),
                    accent.gamma_multiply(0.5 * flash),
                );
            }
            let fill = if node.enabled { theme::CARD } else { theme::PANEL };
            cp.rect_filled(layout.rect, CornerRadius::same(8), fill);
            cp.rect_filled(header, CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }, accent.gamma_multiply(0.35));
            let border = if selected {
                Stroke::new(2.0, theme::ACCENT_TEXT)
            } else if !problems.is_empty() {
                Stroke::new(1.5, theme::ERROR)
            } else if body.hovered() {
                Stroke::new(1.0, theme::HOVER)
            } else {
                Stroke::new(1.0, theme::LINE)
            };
            cp.rect_stroke(layout.rect, CornerRadius::same(8), border, StrokeKind::Outside);
            let title = spec.map_or_else(|| format!("{} (unknown)", node.kind), |s| s.title.clone());
            let title_color = if node.enabled { theme::TEXT } else { theme::MUTED };
            cp.text(
                header.left_center() + vec2(10.0, 0.0),
                egui::Align2::LEFT_CENTER,
                &title,
                egui::FontId::proportional(14.0),
                title_color,
            );
            if !problems.is_empty() {
                let badge = Rect::from_center_size(header.right_center() - vec2(14.0, 0.0), vec2(18.0, 18.0));
                cp.circle_filled(badge.center(), 9.0, theme::ERROR);
                cp.text(
                    badge.center(),
                    egui::Align2::CENTER_CENTER,
                    "!",
                    egui::FontId::proportional(13.0),
                    Color32::BLACK,
                );
                let response = canvas.interact(badge, id.with("problem"), Sense::hover());
                response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, "Problem"));
                response.on_hover_text(problems.join("\n"));
            }
            if let Some(text) = live.and_then(|l| l.badge(node_id)) {
                let galley = cp.layout_no_wrap(text, egui::FontId::proportional(12.0), Color32::BLACK);
                let rect = Rect::from_min_size(
                    layout.rect.right_top() + vec2(-galley.size().x - 14.0, -galley.size().y - 8.0),
                    galley.size() + vec2(12.0, 4.0),
                );
                cp.rect_filled(rect, CornerRadius::same(6), accent);
                cp.galley(rect.min + vec2(6.0, 2.0), galley, Color32::BLACK);
            }

            // Pins.
            if let Some(spec) = spec {
                for (side, pins, positions) in
                    [(false, &spec.inputs, &layout.inputs), (true, &spec.outputs, &layout.outputs)]
                {
                    for (pin, (spec_pin, &pos)) in pins.iter().zip(positions).enumerate() {
                        let pin_ref = PinRef { node: node_id, pin: pin as u8, output: side };
                        let wired = if side {
                            graph.links_from(pin_ref.key()).next().is_some()
                        } else {
                            graph.links_into(pin_ref.key()).next().is_some()
                        };
                        let compatible = match (dragging_wire, wire_type) {
                            (Some(from), Some(ty)) if from.output != side && from.node != node_id => {
                                if side {
                                    catalog.accepts(ty, spec_pin.ty)
                                } else {
                                    catalog.accepts(spec_pin.ty, ty)
                                }
                            }
                            (Some(_), _) => false,
                            _ => true,
                        };
                        let color = pin_color(spec_pin.ty);
                        let color = if compatible { color } else { color.gamma_multiply(0.25) };
                        let hit = Rect::from_center_size(pos, Vec2::splat(PIN_REACH * 2.0));
                        let response = canvas.interact(hit, id.with(("pin", side, pin)), Sense::click_and_drag());
                        let hot = response.hovered()
                            || (dragging_wire.is_some()
                                && compatible
                                && pointer_canvas.is_some_and(|p| p.distance(pos) < PIN_REACH));
                        let radius = if hot { PIN_RADIUS + 2.0 } else { PIN_RADIUS };
                        if spec_pin.ty == PinType::Trigger {
                            draw::trigger_pin(&cp, pos, radius, color, wired);
                        } else if wired {
                            cp.circle_filled(pos, radius, color);
                        } else {
                            cp.circle(pos, radius, theme::CARD, Stroke::new(2.0, color));
                        }
                        let (align, offset) =
                            if side { (egui::Align2::RIGHT_CENTER, -12.0) } else { (egui::Align2::LEFT_CENTER, 12.0) };
                        cp.text(
                            pos + vec2(offset, 0.0),
                            align,
                            &spec_pin.name,
                            egui::FontId::proportional(12.5),
                            theme::MUTED,
                        );
                        let tip = if spec_pin.help.is_empty() {
                            format!("{} ({})", spec_pin.name, spec_pin.ty.name())
                        } else {
                            format!("{} ({})\n{}", spec_pin.name, spec_pin.ty.name(), spec_pin.help)
                        };
                        let response = response.on_hover_text(tip);
                        if response.drag_started_by(PointerButton::Primary) {
                            start_wire = Some(pin_ref);
                        }
                        if response.clicked() && ctx.input(|i| i.modifiers.alt) {
                            // Alt-click unplugs every wire on the pin.
                            self.selected_link = None;
                            graph.links.retain(|l| if side { l.from != pin_ref.key() } else { l.to != pin_ref.key() });
                        }
                    }
                }
            }

            // Settings.
            if let Some(spec) = spec {
                let wired_names: Vec<&str> = spec
                    .inputs
                    .iter()
                    .enumerate()
                    .filter(|(pin, _)| graph.links_into((node_id, *pin as u8)).next().is_some())
                    .map(|(_, p)| p.name.as_str())
                    .collect();
                let mut y = layout.params_top;
                let node = &mut graph.nodes[index];
                for param in &spec.params {
                    let row =
                        Rect::from_min_size(pos2(layout.rect.left() + 10.0, y), vec2(NODE_WIDTH - 20.0, PARAM_ROW));
                    y += PARAM_ROW;
                    let wired = wired_names.contains(&param.name.as_str());
                    let mut row_ui = canvas.new_child(
                        UiBuilder::new()
                            .max_rect(row)
                            .id_salt((node_id, &param.name))
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                    );
                    row_ui.spacing_mut().interact_size.y = 20.0;
                    row_ui.spacing_mut().button_padding = vec2(6.0, 2.0);
                    if let Some(toggled) = param_row(&mut row_ui, node, param, wired, scene) {
                        outcome.simple_toggled = Some(toggled);
                    }
                }
            } else {
                cp.text(
                    pos2(layout.rect.left() + 10.0, layout.params_top + 10.0),
                    egui::Align2::LEFT_CENTER,
                    "This block can't be used here.",
                    egui::FontId::proportional(12.0),
                    theme::MUTED,
                );
            }

            // Selecting and moving.
            if body.drag_started_by(PointerButton::Primary) {
                if !self.selection.contains(&node_id) {
                    if !ctx.input(|i| i.modifiers.shift || i.modifiers.command) {
                        self.selection.clear();
                    }
                    self.selection.insert(node_id);
                }
                self.drag = Drag::Nodes;
            }
            if body.clicked() {
                node_clicked = true;
                let (shift, command) = ctx.input(|i| (i.modifiers.shift, i.modifiers.command));
                if shift || command {
                    if !self.selection.remove(&node_id) {
                        self.selection.insert(node_id);
                    }
                } else {
                    self.selection = BTreeSet::from([node_id]);
                }
                self.selected_note = None;
                self.selected_link = None;
            }
            body.context_menu(|ui| {
                if !self.selection.contains(&node_id) {
                    self.selection = BTreeSet::from([node_id]);
                }
                self.node_menu(ui, graph, node_id);
            });
        }

        // ---------- dragging ----------
        let delta = ctx.input(|i| i.pointer.delta()) / self.zoom;
        let released = ctx.input(|i| i.pointer.any_released());
        if let Some(from) = start_wire {
            // Dragging out of an input that already has one wire picks that wire up.
            if !from.output {
                let into: Vec<(usize, Link)> = graph.links_into(from.key()).map(|(i, l)| (i, *l)).collect();
                if let [(index, link)] = into[..] {
                    graph.links.remove(index);
                    self.selected_link = None;
                    self.drag = Drag::Wire { from: PinRef { node: link.from.0, pin: link.from.1, output: true } };
                } else {
                    self.drag = Drag::Wire { from };
                }
            } else {
                self.drag = Drag::Wire { from };
            }
        }
        match self.drag.clone() {
            Drag::Nodes => {
                if delta != Vec2::ZERO {
                    for node in graph.nodes.iter_mut().filter(|n| self.selection.contains(&n.id)) {
                        node.pos[0] += delta.x;
                        node.pos[1] += delta.y;
                    }
                }
                if released {
                    self.drag = Drag::None;
                }
            }
            Drag::Wire { from } => {
                let start = pin_pos(graph, &layouts, from.key(), from.output);
                if let (Some(start), Some(end)) = (start, pointer_canvas) {
                    let ty = pin_type(graph, catalog, from).unwrap_or(PinType::Number);
                    let (a, b) = if from.output { (start, end) } else { (end, start) };
                    draw::wire(&cp, a, b, Stroke::new(2.5, pin_color(ty)));
                }
                if released {
                    self.drag = Drag::None;
                    if let Some(end) = pointer_canvas {
                        self.drop_wire(&ctx, graph, catalog, &layouts, from, end, over_canvas);
                    }
                }
            }
            Drag::Note { index, carried } => {
                if let Some(note) = graph.notes.get_mut(index) {
                    note.pos[0] += delta.x;
                    note.pos[1] += delta.y;
                }
                for node in graph.nodes.iter_mut().filter(|n| carried.contains(&n.id)) {
                    node.pos[0] += delta.x;
                    node.pos[1] += delta.y;
                }
                if released {
                    self.drag = Drag::None;
                }
            }
            Drag::ResizeNote { index } => {
                if let Some(note) = graph.notes.get_mut(index) {
                    note.size[0] = (note.size[0] + delta.x).max(120.0);
                    note.size[1] = (note.size[1] + delta.y).max(60.0);
                }
                if released {
                    self.drag = Drag::None;
                }
            }
            Drag::Box { start, add } => {
                if let Some(end) = pointer_canvas {
                    let rect = Rect::from_two_pos(start, end);
                    cp.rect(
                        rect,
                        CornerRadius::same(2),
                        theme::ACCENT.gamma_multiply(0.12),
                        Stroke::new(1.0 / self.zoom, theme::ACCENT_TEXT),
                        StrokeKind::Inside,
                    );
                    if released {
                        if !add {
                            self.selection.clear();
                        }
                        for (node, layout) in graph.nodes.iter().zip(&layouts) {
                            if rect.intersects(layout.rect) {
                                self.selection.insert(node.id);
                            }
                        }
                    }
                }
                if released {
                    self.drag = Drag::None;
                }
            }
            Drag::None => {}
        }

        // ---------- background clicks ----------
        if background.drag_started_by(PointerButton::Primary)
            && matches!(self.drag, Drag::None)
            && let Some(p) = pointer_canvas
        {
            self.drag = Drag::Box { start: p, add: ctx.input(|i| i.modifiers.shift || i.modifiers.command) };
        }
        if background.clicked() && !node_clicked {
            if !ctx.input(|i| i.modifiers.shift) {
                self.selection.clear();
            }
            self.selected_note = None;
            self.selected_link = hovered_link.and_then(|i| graph.links.get(i).copied());
            self.editing_note = None;
        }
        if background.double_clicked()
            && hovered_link.is_none()
            && let Some(p) = pointer_canvas
        {
            self.palette = Some(Palette::new(p, None));
        }
        if background.secondary_clicked() {
            if let Some(p) = pointer_canvas {
                self.menu_at = p;
            }
            self.selected_link = hovered_link.and_then(|i| graph.links.get(i).copied());
        }
        background.context_menu(|ui| {
            if let Some(link) = self.selected_link {
                if ui.button("Remove this wire").clicked() {
                    graph.links.retain(|l| *l != link);
                    self.selected_link = None;
                    ui.close();
                }
                ui.separator();
            }
            let at = self.menu_at;
            if ui.button("Add a block…").on_hover_text("Space").clicked() {
                self.palette = Some(Palette::new(at, None));
                ui.close();
            }
            if ui.button("Add a note").clicked() {
                graph.notes.push(Note { pos: at.into(), size: [240.0, 120.0], text: "Note".into(), ..Note::default() });
                self.selected_note = Some(graph.notes.len() - 1);
                ui.close();
            }
            let clipboard = ui.ctx().data(|d| d.get_temp::<Arc<Clipboard>>(clipboard_id()));
            if ui.add_enabled(clipboard.is_some(), egui::Button::new("Paste")).on_hover_text("Ctrl+V").clicked() {
                self.paste(ui.ctx(), graph, at);
                ui.close();
            }
            ui.separator();
            if ui.button("Select everything").on_hover_text("Ctrl+A").clicked() {
                self.selection = graph.nodes.iter().map(|n| n.id).collect();
                ui.close();
            }
            if ui.button("Fit everything in view").on_hover_text("F").clicked() {
                self.fitted = false;
                ui.close();
            }
            ui.checkbox(&mut self.show_minimap, "Show the map");
        });

        // ---------- keys ----------
        let has_focus = ctx.memory(|m| m.focused().is_none()) && !ctx.text_edit_focused();
        let keys_here = over_canvas && has_focus && self.palette.is_none() && self.editing_note.is_none();
        ctx.data_mut(|d| d.insert_temp(keyboard_id(), keys_here));
        if keys_here {
            self.keys(&ctx, graph, pointer_canvas.unwrap_or_default());
        }

        // ---------- overlay: minimap, message, palette ----------
        ctx.set_sublayer(ui.layer_id(), overlay_layer);
        let mut overlay = ui.new_child(UiBuilder::new().layer_id(overlay_layer).max_rect(screen));
        overlay.set_clip_rect(screen.intersect(ui.clip_rect()));
        if self.show_minimap && !graph.nodes.is_empty() {
            self.minimap(&mut overlay, graph, &layouts);
        }
        if graph.nodes.is_empty() && self.palette.is_none() {
            overlay.painter().text(
                screen.center(),
                egui::Align2::CENTER_CENTER,
                "Press Space or double-click to add a block",
                egui::FontId::proportional(16.0),
                theme::MUTED,
            );
        }
        if let Some((text, shown)) = &self.message {
            let age = ctx.input(|i| i.time) - shown;
            if age < 3.0 {
                let galley =
                    overlay.painter().layout(text.clone(), egui::FontId::proportional(14.0), theme::TEXT, 420.0);
                let rect = Rect::from_center_size(
                    pos2(screen.center().x, screen.bottom() - 36.0),
                    galley.size() + vec2(24.0, 12.0),
                );
                overlay.painter().rect_filled(rect, CornerRadius::same(8), theme::RAISED);
                overlay.painter().galley(rect.min + vec2(12.0, 6.0), galley, theme::TEXT);
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(3.0 - age));
            } else {
                self.message = None;
            }
        }
        if let Some(palette) = &mut self.palette {
            let screen_pos = self.to_screen * palette.at;
            match palette.show(&ctx, self.id.with("palette"), screen_pos, graph, catalog) {
                palette::Result::Open => {}
                palette::Result::Close => self.palette = None,
                palette::Result::Pick(kind) => {
                    let palette = self.palette.take().expect("open palette");
                    if let Some(spec) = catalog.spec(&kind) {
                        self.add_block(&ctx, graph, catalog, spec, palette.at, palette.from);
                    }
                }
            }
        }

        outcome.changed = *graph != before;
        outcome.selected = (self.selection.len() == 1).then(|| *self.selection.first().unwrap());
        outcome
    }

    /// Drops selection entries for blocks, notes and wires that no longer exist (after undo).
    fn forget_missing(&mut self, graph: &Graph) {
        self.selection.retain(|id| graph.node(*id).is_some());
        if self.selected_note.is_some_and(|i| i >= graph.notes.len()) {
            self.selected_note = None;
        }
        if self.editing_note.is_some_and(|i| i >= graph.notes.len()) {
            self.editing_note = None;
        }
        if self.selected_link.is_some_and(|link| !graph.links.contains(&link)) {
            self.selected_link = None;
        }
        match &self.drag {
            Drag::Note { index, .. } | Drag::ResizeNote { index } if *index >= graph.notes.len() => {
                self.drag = Drag::None
            }
            Drag::Wire { from } if graph.node(from.node).is_none() => self.drag = Drag::None,
            _ => {}
        }
    }

    fn fit(&mut self, graph: &Graph, layouts: &[Layout], screen: Rect) {
        let mut bounds = Rect::NOTHING;
        for l in layouts {
            bounds = bounds.union(l.rect);
        }
        for note in &graph.notes {
            bounds = bounds.union(note_rect(note));
        }
        if !bounds.is_positive() {
            self.zoom = 1.0;
            self.pan = screen.size() / 2.0 - vec2(NODE_WIDTH / 2.0, 40.0);
            return;
        }
        let bounds = bounds.expand(40.0);
        let zoom = (screen.size() / bounds.size()).min_elem().clamp(ZOOM.0, 1.0);
        self.zoom = zoom;
        self.pan = screen.size() / 2.0 - bounds.center().to_vec2() * zoom;
    }

    fn notes(&mut self, canvas: &mut egui::Ui, graph: &mut Graph, layouts: &[Layout], outcome: &mut Outcome) {
        let cp = canvas.painter().clone();
        let mut remove = None;
        for index in 0..graph.notes.len() {
            let note = &graph.notes[index];
            let rect = note_rect(note);
            let [r, g, b, _] = note.color;
            let color = Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8);
            cp.rect_filled(rect, CornerRadius::same(10), color.gamma_multiply(0.08));
            let selected = self.selected_note == Some(index);
            let stroke =
                Stroke::new(if selected { 2.0 } else { 1.0 }, color.gamma_multiply(if selected { 0.9 } else { 0.4 }));
            cp.rect_stroke(rect, CornerRadius::same(10), stroke, StrokeKind::Inside);
            let title = Rect::from_min_size(rect.min, vec2(rect.width(), NOTE_TITLE));
            let id = self.id.with(("note", index));
            if self.editing_note == Some(index) {
                let mut text = graph.notes[index].text.clone();
                let edit = canvas.put(
                    rect.shrink(6.0),
                    egui::TextEdit::multiline(&mut text).frame(egui::Frame::NONE).desired_width(rect.width() - 12.0),
                );
                if !edit.has_focus() && !edit.gained_focus() {
                    edit.request_focus();
                }
                if text != graph.notes[index].text {
                    graph.notes[index].text = text;
                }
                if edit.lost_focus() || canvas.input(|i| i.key_pressed(Key::Escape)) {
                    self.editing_note = None;
                }
                continue;
            }
            let galley = cp.layout(
                note.text.clone(),
                egui::FontId::proportional(13.5),
                color.gamma_multiply(0.95),
                (rect.width() - 16.0).max(10.0),
            );
            cp.galley_with_override_text_color(rect.min + vec2(8.0, 5.0), galley, color);
            let head = canvas
                .interact(title, id, Sense::click_and_drag())
                .on_hover_text("Drag to move the note and the blocks on it. Double-click to edit.");
            let corner = Rect::from_min_size(rect.max - vec2(16.0, 16.0), vec2(16.0, 16.0));
            cp.line_segment(
                [corner.right_top() + vec2(-3.0, 4.0), corner.left_bottom() + vec2(4.0, -3.0)],
                Stroke::new(1.0, color),
            );
            let resize = canvas.interact(corner, id.with("size"), Sense::drag());
            if resize.hovered() {
                canvas.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
            }
            if head.drag_started_by(PointerButton::Primary) {
                let carried = graph
                    .nodes
                    .iter()
                    .zip(layouts)
                    .filter(|(_, l)| rect.contains(l.rect.center()))
                    .map(|(n, _)| n.id)
                    .collect();
                self.drag = Drag::Note { index, carried };
                self.selected_note = Some(index);
            }
            if resize.drag_started_by(PointerButton::Primary) {
                self.drag = Drag::ResizeNote { index };
            }
            if head.clicked() {
                self.selected_note = Some(index);
                self.selection.clear();
            }
            if head.double_clicked() {
                self.editing_note = Some(index);
            }
            head.context_menu(|ui| {
                if ui.button("Edit text").clicked() {
                    self.editing_note = Some(index);
                    ui.close();
                }
                ui.horizontal(|ui| {
                    ui.label("Colour");
                    let note = &mut graph.notes[index];
                    let mut rgba = egui::Rgba::from_rgba_unmultiplied(note.color[0], note.color[1], note.color[2], 1.0);
                    if egui::color_picker::color_edit_button_rgba(ui, &mut rgba, egui::color_picker::Alpha::Opaque)
                        .changed()
                    {
                        note.color = [rgba.r(), rgba.g(), rgba.b(), 1.0];
                    }
                });
                if ui.button("Remove note").clicked() {
                    remove = Some(index);
                    ui.close();
                }
            });
        }
        if let Some(index) = remove {
            graph.notes.remove(index);
            self.selected_note = None;
            outcome.changed = true;
        }
    }

    fn node_menu(&mut self, ui: &mut egui::Ui, graph: &mut Graph, node_id: u32) {
        let count = self.selection.len();
        let plural = if count > 1 { format!(" ({count} blocks)") } else { String::new() };
        if ui.button(format!("Duplicate{plural}")).on_hover_text("Ctrl+D").clicked() {
            self.duplicate(graph);
            ui.close();
        }
        if ui.button(format!("Copy{plural}")).on_hover_text("Ctrl+C").clicked() {
            self.copy(ui.ctx(), graph);
            ui.close();
        }
        let enabled = graph.node(node_id).is_some_and(|n| n.enabled);
        if ui
            .button(if enabled { "Turn off" } else { "Turn on" })
            .on_hover_text("A block that is off does nothing")
            .clicked()
        {
            for node in graph.nodes.iter_mut().filter(|n| self.selection.contains(&n.id)) {
                node.enabled = !enabled;
            }
            ui.close();
        }
        if ui.button("Frame with a note").on_hover_text("Ctrl+G").clicked() {
            self.group(graph);
            ui.close();
        }
        ui.separator();
        if ui.button(format!("Delete{plural}")).on_hover_text("Delete").clicked() {
            graph.remove(&self.selection);
            self.selection.clear();
            ui.close();
        }
    }

    fn keys(&mut self, ctx: &egui::Context, graph: &mut Graph, pointer: Pos2) {
        let (copy, paste, cut) = ctx.input(|i| {
            let mut copy = false;
            let mut paste = false;
            let mut cut = false;
            for event in &i.events {
                match event {
                    Event::Copy => copy = true,
                    Event::Cut => cut = true,
                    Event::Paste(_) => paste = true,
                    _ => {}
                }
            }
            (copy, paste, cut)
        });
        let pressed = |modifiers, key| ctx.input_mut(|i| i.consume_key(modifiers, key));
        if copy || cut || pressed(Modifiers::COMMAND, Key::C) {
            self.copy(ctx, graph);
        }
        if cut || pressed(Modifiers::COMMAND, Key::X) {
            self.copy(ctx, graph);
            graph.remove(&self.selection);
            self.selection.clear();
        }
        if paste || pressed(Modifiers::COMMAND, Key::V) {
            self.paste(ctx, graph, pointer);
        }
        if pressed(Modifiers::COMMAND, Key::D) {
            self.duplicate(graph);
        }
        if pressed(Modifiers::COMMAND, Key::A) {
            self.selection = graph.nodes.iter().map(|n| n.id).collect();
        }
        if pressed(Modifiers::COMMAND, Key::G) {
            self.group(graph);
        }
        if pressed(Modifiers::NONE, Key::F) {
            self.fitted = false;
        }
        if pressed(Modifiers::NONE, Key::Space) {
            self.palette = Some(Palette::new(pointer, None));
        }
        if pressed(Modifiers::NONE, Key::Delete) || pressed(Modifiers::NONE, Key::Backspace) {
            if !self.selection.is_empty() {
                graph.remove(&self.selection);
                self.selection.clear();
            } else if let Some(link) = self.selected_link.take() {
                graph.links.retain(|l| *l != link);
            } else if let Some(index) = self.selected_note.take()
                && index < graph.notes.len()
            {
                graph.notes.remove(index);
            }
        }
        if pressed(Modifiers::NONE, Key::Escape) {
            self.selection.clear();
            self.selected_link = None;
            self.selected_note = None;
        }
    }

    fn copy(&self, ctx: &egui::Context, graph: &Graph) {
        if self.selection.is_empty() {
            return;
        }
        let (nodes, links) = graph.duplicate(&self.selection, 0, [0.0, 0.0]);
        ctx.data_mut(|d| d.insert_temp(clipboard_id(), Arc::new(Clipboard { nodes, links })));
    }

    fn paste(&mut self, ctx: &egui::Context, graph: &mut Graph, at: Pos2) {
        let Some(clip) = ctx.data(|d| d.get_temp::<Arc<Clipboard>>(clipboard_id())) else { return };
        let Some(min) = clip.nodes.iter().map(|n| Pos2::from(n.pos)).reduce(|a, b| a.min(b)) else { return };
        let first = graph.next_id();
        let ids: BTreeSet<u32> = clip.nodes.iter().map(|n| n.id).collect();
        let source = Graph { nodes: clip.nodes.clone(), links: clip.links.clone(), ..Graph::default() };
        let (nodes, links) = source.duplicate(&ids, first, (at - min).into());
        self.selection = nodes.iter().map(|n| n.id).collect();
        graph.nodes.extend(nodes);
        graph.links.extend(links);
    }

    fn duplicate(&mut self, graph: &mut Graph) {
        if self.selection.is_empty() {
            return;
        }
        let (nodes, links) = graph.duplicate(&self.selection, graph.next_id(), [30.0, 30.0]);
        self.selection = nodes.iter().map(|n| n.id).collect();
        graph.nodes.extend(nodes);
        graph.links.extend(links);
    }

    fn group(&mut self, graph: &mut Graph) {
        let mut bounds = Rect::NOTHING;
        for node in graph.nodes.iter().filter(|n| self.selection.contains(&n.id)) {
            bounds = bounds.union(Rect::from_min_size(node.pos.into(), vec2(NODE_WIDTH, HEADER + ROW)));
        }
        if !bounds.is_positive() {
            return;
        }
        // Rough block heights are fine here; the frame is resizable.
        let rect = bounds
            .expand2(vec2(24.0, 24.0))
            .with_min_y(bounds.min.y - NOTE_TITLE - 16.0)
            .with_max_y(bounds.max.y + 120.0);
        graph.notes.push(Note {
            pos: rect.min.into(),
            size: rect.size().into(),
            text: "Group".into(),
            ..Note::default()
        });
        self.selected_note = Some(graph.notes.len() - 1);
    }

    #[allow(clippy::too_many_arguments)]
    fn drop_wire(
        &mut self,
        ctx: &egui::Context,
        graph: &mut Graph,
        catalog: &dyn Catalog,
        layouts: &[Layout],
        from: PinRef,
        end: Pos2,
        over_canvas: bool,
    ) {
        let target = graph.nodes.iter().zip(layouts).find_map(|(node, layout)| {
            let positions = if from.output { &layout.inputs } else { &layout.outputs };
            positions.iter().position(|p| p.distance(end) < PIN_REACH).map(|pin| PinRef {
                node: node.id,
                pin: pin as u8,
                output: !from.output,
            })
        });
        match target {
            Some(to) => {
                self.connect(ctx, graph, catalog, from, to);
            }
            None if over_canvas => self.palette = Some(Palette::new(end, Some(from))),
            None => {}
        }
    }

    /// Wires two pins if their types fit and no loop forms, explaining a refusal.
    fn connect(&mut self, ctx: &egui::Context, graph: &mut Graph, catalog: &dyn Catalog, a: PinRef, b: PinRef) -> bool {
        let (out, input) = if a.output { (a, b) } else { (b, a) };
        if out.node == input.node {
            self.say(ctx, "A block can't be wired to itself.");
            return false;
        }
        let (Some(from_ty), Some(to_ty)) =
            (graph.output_type(out.key(), catalog), graph.input_type(input.key(), catalog))
        else {
            return false;
        };
        if !catalog.accepts(to_ty, from_ty) {
            self.say(ctx, format!("A {} can't go into a {} input.", from_ty.name(), to_ty.name()));
            return false;
        }
        let before = graph.links.clone();
        graph.connect(Link { from: out.key(), to: input.key() }, catalog);
        if !catalog.allows_cycles() && bpe_graph::order(graph).is_err() {
            graph.links = before;
            self.say(ctx, "That wire would make a loop, so it was left out.");
            return false;
        }
        true
    }

    fn add_block(
        &mut self,
        ctx: &egui::Context,
        graph: &mut Graph,
        catalog: &dyn Catalog,
        spec: &NodeSpec,
        at: Pos2,
        from: Option<PinRef>,
    ) {
        // A block dropped at the end of a wire sits with its matching pin under the pointer.
        let pin = from.and_then(|from| {
            let ty = pin_type(graph, catalog, from)?;
            if from.output {
                spec.inputs.iter().position(|p| catalog.accepts(p.ty, ty)).map(|i| (i, false))
            } else {
                spec.outputs.iter().position(|p| catalog.accepts(ty, p.ty)).map(|i| (i, true))
            }
        });
        let mut pos = at;
        if let Some((index, output)) = pin {
            let probe = spec.instantiate(0, [0.0, 0.0]);
            let layout = layout(&probe, Some(spec));
            let offset = if output { layout.outputs[index] } else { layout.inputs[index] };
            pos -= offset.to_vec2();
        }
        let id = graph.add(spec, pos.into());
        if let (Some(from), Some((index, output))) = (from, pin) {
            self.connect(ctx, graph, catalog, from, PinRef { node: id, pin: index as u8, output });
        }
        self.selection = BTreeSet::from([id]);
    }

    fn minimap(&mut self, overlay: &mut egui::Ui, graph: &Graph, layouts: &[Layout]) {
        let screen = self.screen;
        let size = vec2(170.0, 110.0);
        let map = Rect::from_min_size(screen.max - size - vec2(12.0, 12.0), size);
        if map.width() * 2.5 > screen.width() || map.height() * 2.5 > screen.height() {
            return;
        }
        let view = self.to_screen.inverse() * screen;
        let mut bounds = view;
        for l in layouts {
            bounds = bounds.union(l.rect);
        }
        for note in &graph.notes {
            bounds = bounds.union(note_rect(note));
        }
        let bounds = bounds.expand(40.0);
        let scale = (map.size() / bounds.size()).min_elem();
        let offset = map.center() - bounds.center().to_vec2() * scale;
        let to_map = |p: Pos2| offset + p.to_vec2() * scale;
        let painter = overlay.painter();
        painter.rect_filled(map, CornerRadius::same(6), theme::PANEL.gamma_multiply(0.92));
        painter.rect_stroke(map, CornerRadius::same(6), Stroke::new(1.0, theme::LINE), StrokeKind::Inside);
        for note in &graph.notes {
            let r = note_rect(note);
            painter.rect_stroke(
                Rect::from_min_max(to_map(r.min), to_map(r.max)),
                1.0,
                Stroke::new(1.0, theme::MUTED),
                StrokeKind::Inside,
            );
        }
        for (node, layout) in graph.nodes.iter().zip(layouts) {
            let rect = Rect::from_min_max(to_map(layout.rect.min), to_map(layout.rect.max));
            let color = if self.selection.contains(&node.id) { theme::ACCENT_TEXT } else { theme::HOVER };
            painter.rect_filled(rect, 1.0, color);
        }
        let view_rect = Rect::from_min_max(to_map(view.min), to_map(view.max)).intersect(map);
        painter.rect_stroke(view_rect, 2.0, Stroke::new(1.5, theme::TEXT), StrokeKind::Inside);
        let response = overlay
            .interact(map, self.id.with("minimap"), Sense::click_and_drag())
            .on_hover_text("Click or drag to look somewhere else");
        if (response.clicked() || response.dragged())
            && let Some(p) = response.interact_pointer_pos()
        {
            let target = ((p - offset) / scale).to_pos2();
            self.pan = screen.size() / 2.0 - target.to_vec2() * self.zoom;
        }
    }
}

fn clipboard_id() -> Id {
    Id::unique("node editor clipboard")
}

fn note_rect(note: &Note) -> Rect {
    let size = if note.size[0] <= 0.0 || note.size[1] <= 0.0 { [240.0, 120.0] } else { note.size };
    Rect::from_min_size(note.pos.into(), size.into())
}

fn pin_pos(graph: &Graph, layouts: &[Layout], pin: (u32, u8), output: bool) -> Option<Pos2> {
    let i = graph.nodes.iter().position(|n| n.id == pin.0)?;
    let l = layouts.get(i)?;
    if output { l.outputs.get(pin.1 as usize).copied() } else { l.inputs.get(pin.1 as usize).copied() }
}

pub(super) fn pin_type(graph: &Graph, catalog: &dyn Catalog, pin: PinRef) -> Option<PinType> {
    if pin.output { graph.output_type(pin.key(), catalog) } else { graph.input_type(pin.key(), catalog) }
}

fn layout(node: &Node, spec: Option<&NodeSpec>) -> Layout {
    let min = Pos2::from(node.pos);
    let (inputs, outputs, params) = spec.map_or((0, 0, 0), |s| (s.inputs.len(), s.outputs.len(), s.params.len()));
    let rows = inputs.max(outputs);
    let pins_top = min.y + HEADER + 4.0;
    let pin_y = |i: usize| pins_top + ROW * (i as f32 + 0.5);
    let params_top = pins_top + ROW * rows as f32 + if rows > 0 { 4.0 } else { 0.0 };
    let mut height = params_top - min.y + PARAM_ROW * params as f32 + 8.0;
    if spec.is_none() {
        height += ROW;
    }
    Layout {
        rect: Rect::from_min_size(min, vec2(NODE_WIDTH, height.max(HEADER + 12.0))),
        inputs: (0..inputs).map(|i| pos2(min.x, pin_y(i))).collect(),
        outputs: (0..outputs).map(|i| pos2(min.x + NODE_WIDTH, pin_y(i))).collect(),
        params_top,
    }
}

/// One setting of a block: its name and a small control. Returns the (block, setting) whose
/// "Show at Simple" was chosen from the name's menu.
fn param_row(
    ui: &mut egui::Ui,
    node: &mut Node,
    spec: &ParamSpec,
    wired: bool,
    scene: &Scene,
) -> Option<(u32, String)> {
    let label =
        ui.add(egui::Label::new(egui::RichText::new(&spec.label).size(12.5).color(theme::MUTED)).sense(Sense::click()));
    let label = if spec.help.is_empty() { label } else { label.on_hover_text(&spec.help) };
    let exposed = node.exposed.contains(&spec.name);
    let simple = None;
    label.context_menu(|ui| {
        let text = if exposed { "Take off the card" } else { "Show on the card" };
        if ui.button(text).on_hover_text("A setting on the card shows in the inspector, under the module").clicked() {
            if exposed {
                node.exposed.remove(&spec.name);
            } else {
                node.exposed.insert(spec.name.clone());
            }
            ui.close();
        }
        if ui.button("Reset").clicked() {
            node.params.remove(&spec.name);
            ui.close();
        }
    });
    if label.double_clicked() {
        node.params.remove(&spec.name);
    }
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if wired {
            ui.label(egui::RichText::new("from wire").size(12.0).italics().color(theme::MUTED));
            return;
        }
        let mut value = node.params.get(&spec.name).cloned().unwrap_or_else(|| spec.default.clone());
        if param_control(ui, spec, &mut value, scene, label.id) {
            node.params.insert(spec.name.clone(), value);
        }
    });
    simple
}

/// The colour of a wire carrying this type.
pub fn pin_color(ty: PinType) -> Color32 {
    match ty {
        PinType::Trigger => Color32::from_rgb(0xf2, 0xf2, 0xf5),
        PinType::Number => Color32::from_rgb(0x6c, 0xc4, 0xff),
        PinType::Vector => Color32::from_rgb(0x9b, 0x8c, 0xff),
        PinType::Color => Color32::from_rgb(0xff, 0x8f, 0xc8),
        PinType::Bool => Color32::from_rgb(0xff, 0xc8, 0x5c),
        PinType::Ball => Color32::from_rgb(0x6d, 0xe0, 0x9a),
        PinType::Text => Color32::from_rgb(0xd6, 0xd0, 0xb8),
        PinType::Field => Color32::from_rgb(0xff, 0x9a, 0x5c),
    }
}

/// A title colour for a block category, so "When" blocks look alike across graphs.
pub fn category_color(category: &str) -> Color32 {
    let lower = category.to_lowercase();
    if lower.starts_with("when") || lower.contains("trigger") || lower.contains("event") {
        Color32::from_rgb(0xff, 0x9a, 0x4d)
    } else if lower.starts_with("if") || lower.starts_with("only") || lower.contains("condition") {
        Color32::from_rgb(0xf2, 0xc4, 0x6d)
    } else if lower.starts_with("do") || lower.contains("action") {
        Color32::from_rgb(0x5c, 0xd6, 0x8a)
    } else if lower.contains("value") || lower.contains("math") || lower.contains("number") {
        Color32::from_rgb(0x6c, 0xc4, 0xff)
    } else if lower.contains("setting") {
        Color32::from_rgb(0xc9, 0x9b, 0xff)
    } else if lower.contains("module") || lower.contains("output") || lower.contains("sound") {
        Color32::from_rgb(0xff, 0x7a, 0xc0)
    } else {
        let hash = lower.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
        let hue = (hash % 360) as f32 / 360.0;
        egui::ecolor::Hsva::new(hue, 0.55, 0.95, 1.0).into()
    }
}
