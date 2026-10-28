//! The Notes lane: which melody note each bounce played (music workstream). Hidden until it has something to show.

use super::lanes::{Lane, LaneContext};
use eframe::egui::{Rect, Ui};

#[derive(Default)]
pub struct NotesLane {}

impl Lane for NotesLane {
    fn id(&self) -> &'static str {
        "notes"
    }

    fn title(&self, _ctx: &LaneContext) -> String {
        "Notes".into()
    }

    fn visible(&self, _ctx: &LaneContext) -> bool {
        false
    }

    fn draw(&mut self, _ui: &mut Ui, _rect: Rect, _ctx: &LaneContext) {}
}
