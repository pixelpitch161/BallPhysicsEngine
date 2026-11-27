//! The Water stage's tool rail: the nine tools down the stage's left edge. The one whose part has
//! not landed (Camera path) is greyed with a line saying so.

use crate::style;
use bpe_kit::things::water;
use eframe::egui::{self, Color32, Painter, Pos2, Rect, Sense, Stroke, Ui, vec2};

/// The rail's width.
pub const RAIL_W: f32 = 44.0;
/// One tool button's side.
const BUTTON: f32 = 36.0;
/// What a tool that is not built yet says when pointed at.
pub const LATER: &str = "Arrives in a later part of Water mode";

/// A tool of the Water stage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Select,
    Carve,
    Piece,
    Pen,
    Pour,
    Drain,
    Lamp,
    Force,
    CameraPath,
}

impl Tool {
    pub const ALL: [Tool; 9] = [
        Tool::Select,
        Tool::Carve,
        Tool::Piece,
        Tool::Pen,
        Tool::Pour,
        Tool::Drain,
        Tool::Lamp,
        Tool::Force,
        Tool::CameraPath,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Carve => "Carve",
            Tool::Piece => "Piece",
            Tool::Pen => "Pen",
            Tool::Pour => "Pour",
            Tool::Drain => "Drain",
            Tool::Lamp => "Lamp",
            Tool::Force => "Force",
            Tool::CameraPath => "Camera path",
        }
    }

    /// One line for the strip above the stage.
    pub fn hint(self) -> &'static str {
        match self {
            Tool::Select => {
                "Click a thing to pick it. Drag it to move it, or a picked piece's dots to turn and size it."
            }
            Tool::Carve => {
                "Paint to dig the course out of the rock. Alt fills it back in. Shift draws a straight line."
            }
            Tool::Piece => "Pick a piece, then click to place it. Alt places it off the grid.",
            Tool::Pen => "Click to add points. Double-click or Enter finishes the wall, Escape drops it.",
            Tool::Pour => "Click where liquid should pour in.",
            Tool::Drain => "Click where liquid should leave.",
            Tool::Lamp => "Click where a lamp should go.",
            Tool::Force => "Click where a force should push or pull.",
            Tool::CameraPath => "Lay the camera's path along the course.",
        }
    }

    /// Whether the tool works yet.
    pub fn ready(self) -> bool {
        !matches!(self, Tool::CameraPath)
    }

    /// The thing kind a click with the tool adds.
    pub fn places(self) -> Option<&'static str> {
        match self {
            Tool::Pour => Some(water::POUR),
            Tool::Drain => Some(water::DRAIN),
            Tool::Lamp => Some(water::LAMP),
            Tool::Force => Some(water::FORCE),
            _ => None,
        }
    }
}

/// Draws the rail in `rect` and switches `tool` to a clicked one.
pub fn show(ui: &mut Ui, rect: Rect, tool: &mut Tool) {
    ui.painter().rect_filled(rect, 0.0, style::PANEL);
    ui.painter().vline(rect.right(), rect.y_range(), Stroke::new(1.0, style::LINE_SOFT));
    let pad = (RAIL_W - BUTTON) / 2.0;
    for (i, t) in Tool::ALL.into_iter().enumerate() {
        let at = Rect::from_min_size(rect.min + vec2(pad, pad + i as f32 * (BUTTON + 4.0)), vec2(BUTTON, BUTTON));
        let on = *tool == t;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(at));
        if !t.ready() {
            child.disable();
        }
        let (at, response) = child.allocate_exact_size(at.size(), Sense::click());
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, t.ready(), on, t.name()));
        let ink = if !t.ready() {
            style::LINE
        } else if on {
            style::ON_LIGHT
        } else {
            style::TEXT
        };
        if on {
            child.painter().rect_filled(at, 3.0, style::TEXT);
        } else if response.hovered() && t.ready() {
            child.painter().rect_filled(at, 3.0, style::HOVER);
        }
        paint(child.painter(), at, t, ink);
        let response = response
            .on_hover_text(format!("{}: {}", t.name(), t.hint()))
            .on_disabled_hover_text(format!("{}. {LATER}", t.name()));
        if response.clicked() {
            *tool = t;
        }
    }
}

/// A tool's picture: the thing's own icon for a tool that places one, a drawn one otherwise.
fn paint(painter: &Painter, rect: Rect, tool: Tool, ink: Color32) {
    let c = rect.center();
    let r = rect.width() * 0.26;
    let stroke = Stroke::new(1.5, ink);
    match tool {
        // An arrow pointing up and to the left.
        Tool::Select => {
            let tip = c + vec2(-r * 0.8, -r);
            let points =
                vec![tip, tip + vec2(0.0, r * 1.9), tip + vec2(r * 0.55, r * 1.35), tip + vec2(r * 1.45, r * 1.35)];
            painter.add(egui::Shape::convex_polygon(points, ink, Stroke::NONE));
        }
        // A block with a round bite out of its corner.
        Tool::Carve => {
            let block = Rect::from_center_size(c, vec2(r * 2.0, r * 2.0));
            painter.rect_stroke(block, 2.0, stroke, egui::StrokeKind::Inside);
            painter.circle_filled(block.right_top() + vec2(-r * 0.35, r * 0.35), r * 0.6, ink);
        }
        // A slanted line with its two points.
        Tool::Pen => {
            let (a, b) = (c + vec2(-r, r * 0.8), c + vec2(r, -r * 0.8));
            painter.line_segment([a, b], stroke);
            painter.circle_filled(a, 2.5, ink);
            painter.circle_filled(b, 2.5, ink);
        }
        // The camera's frame with a path running through it.
        Tool::CameraPath => {
            let frame = Rect::from_center_size(c + vec2(r * 0.35, 0.0), vec2(r * 1.0, r * 1.8));
            painter.rect_stroke(frame, 1.5, stroke, egui::StrokeKind::Inside);
            let path: Vec<Pos2> = vec![c + vec2(-r * 1.2, r * 0.9), c + vec2(-r * 0.6, -r * 0.2), frame.center()];
            painter.add(egui::Shape::line(path, Stroke::new(1.2, ink)));
        }
        Tool::Piece => crate::tree::paint_icon(painter, rect, water::PIECE, ink),
        Tool::Pour | Tool::Drain | Tool::Lamp | Tool::Force => {
            crate::tree::paint_icon(painter, rect, tool.places().expect("a placing tool"), ink)
        }
    }
}
