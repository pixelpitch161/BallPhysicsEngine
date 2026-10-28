//! The lane framework: each lane shows one kind of thing over time under the ruler.

use super::HitSummary;
use crate::theme;
use bpe_scene::Scene;
use eframe::egui::{self, Align2, FontId, Pos2, Rect, Response, Sense, Stroke, Ui};

/// Converts between seconds and screen x across the lanes.
#[derive(Clone, Copy, Debug)]
pub struct Scale {
    pub left: f32,
    pub width: f32,
    /// Seconds across the full width.
    pub span: f32,
}

impl Scale {
    pub fn x(&self, time: f32) -> f32 {
        self.left + time / self.span * self.width
    }

    pub fn time(&self, x: f32) -> f32 {
        ((x - self.left) / self.width * self.span).max(0.0)
    }
}

/// What every lane can look at while it draws.
pub struct LaneContext<'a> {
    pub scene: &'a Scene,
    pub scale: Scale,
    /// The playhead, in seconds.
    #[allow(dead_code)] // for lanes that mark the playhead's moment
    pub time: f32,
    /// Bounces and the whole video's event log, once worked out after the latest edit.
    pub hits: Option<&'a HitSummary>,
}

/// One lane of the timeline.
pub trait Lane {
    /// A fixed name, used to remember whether the lane is collapsed.
    fn id(&self) -> &'static str;
    /// The name in the lane's header, which may include a count.
    fn title(&self, ctx: &LaneContext) -> String;
    /// Height when open, in points.
    fn height(&self, _ctx: &LaneContext) -> f32 {
        30.0
    }
    /// Whether the lane has anything to show for this scene. Hidden lanes take no room.
    fn visible(&self, _ctx: &LaneContext) -> bool {
        true
    }
    /// Draws the lane's contents in `rect`, which starts at the ruler's zero.
    fn draw(&mut self, ui: &mut Ui, rect: Rect, ctx: &LaneContext);
    /// Handles the pointer in the lane. Returns true when the lane used it (dragging a clip, for
    /// example); otherwise pressing in the lane scrubs, like the ruler.
    fn interact(&mut self, _ui: &mut Ui, _rect: Rect, _response: &Response, _ctx: &LaneContext) -> bool {
        false
    }
}

/// Draws a lane's header with a fold arrow. Returns true when it was clicked.
pub fn lane_header(ui: &mut Ui, rect: Rect, id: &str, title: &str, collapsed: bool) -> bool {
    let response = ui.interact(rect, ui.scope_id().with(("lane header", id)), Sense::click());
    let color = if response.hovered() { theme::ACCENT_TEXT } else { theme::TEXT };
    let painter = ui.painter_at(rect);
    let c = Pos2::new(rect.left() + 8.0, rect.center().y);
    let s = 3.5;
    let arrow = if collapsed {
        vec![c + egui::vec2(-s * 0.6, -s), c + egui::vec2(s * 0.8, 0.0), c + egui::vec2(-s * 0.6, s)]
    } else {
        vec![c + egui::vec2(-s, -s * 0.6), c + egui::vec2(s, -s * 0.6), c + egui::vec2(0.0, s * 0.8)]
    };
    painter.add(egui::Shape::convex_polygon(arrow, theme::MUTED, Stroke::NONE));
    let size = if collapsed { 11.5 } else { 13.0 };
    painter.text(
        rect.left_center() + egui::vec2(18.0, 0.0),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(size),
        color,
    );
    let help = if collapsed { "Show this lane" } else { "Fold this lane away" };
    response.on_hover_text(help).clicked()
}

/// A lane background with a short message, for lanes waiting on something.
pub fn lane_message(ui: &Ui, rect: Rect, text: &str) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, theme::CARD);
    painter.text(
        rect.left_center() + egui::vec2(8.0, 0.0),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(12.0),
        theme::MUTED,
    );
}
