//! The Song lane: the song's waveform and beats (music workstream). Hidden until it has something to show.

use super::lanes::{Lane, LaneContext};
use eframe::egui::{Rect, Ui};

#[derive(Default)]
pub struct SongLane {}

impl Lane for SongLane {
    fn id(&self) -> &'static str {
        "song"
    }

    fn title(&self, _ctx: &LaneContext) -> String {
        "Song".into()
    }

    fn visible(&self, _ctx: &LaneContext) -> bool {
        false
    }

    fn draw(&mut self, _ui: &mut Ui, _rect: Rect, _ctx: &LaneContext) {}
}
