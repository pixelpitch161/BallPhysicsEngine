//! The Text lane: caption clips (captions workstream). Hidden until it has something to show.

use super::lanes::{Lane, LaneContext};
use eframe::egui::{Rect, Ui};

#[derive(Default)]
pub struct TextLane {}

impl Lane for TextLane {
    fn id(&self) -> &'static str {
        "text"
    }

    fn title(&self, _ctx: &LaneContext) -> String {
        "Text".into()
    }

    fn visible(&self, _ctx: &LaneContext) -> bool {
        false
    }

    fn draw(&mut self, _ui: &mut Ui, _rect: Rect, _ctx: &LaneContext) {}
}
