//! The Rules lane: when each rule fired, one colour per rule. Hidden until it has something to show.

use super::lanes::{Lane, LaneContext};
use eframe::egui::{Rect, Ui};

#[derive(Default)]
pub struct RulesLane {}

impl Lane for RulesLane {
    fn id(&self) -> &'static str {
        "rules"
    }

    fn title(&self, _ctx: &LaneContext) -> String {
        "Rules".into()
    }

    fn visible(&self, _ctx: &LaneContext) -> bool {
        false
    }

    fn draw(&mut self, _ui: &mut Ui, _rect: Rect, _ctx: &LaneContext) {}
}
