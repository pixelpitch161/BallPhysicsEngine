//! The Rules lane: when each rule acted, one colour per rule set. Hidden until a rule set
//! exists.

use super::lanes::{Lane, LaneContext, lane_message};
use crate::theme;
use bpe_sim::EventKind;
use eframe::egui::{self, Pos2, Rect, Stroke, Ui};

#[derive(Default)]
pub struct RulesLane {
    /// The action count for the title, kept with the log it was counted in (its address and
    /// length) so it is only counted again after the log changes.
    count: std::cell::Cell<(usize, usize, usize)>,
}

/// A colour per rule set, spread around the colour wheel.
pub fn rule_color(rule: u16) -> egui::Color32 {
    let hue = (rule as f32 * 0.618_034).fract();
    egui::ecolor::Hsva::new(hue, 0.6, 0.95, 1.0).into()
}

impl Lane for RulesLane {
    fn id(&self) -> &'static str {
        "rules"
    }

    fn title(&self, ctx: &LaneContext) -> String {
        let actions = ctx.hits.map_or(0, |h| {
            let key = (h.events.as_ptr() as usize, h.events.len());
            let (address, len, count) = self.count.get();
            if (address, len) == key {
                return count;
            }
            let count = h.events.iter().filter(|e| matches!(e.kind, EventKind::Action { .. })).count();
            self.count.set((key.0, key.1, count));
            count
        });
        if actions > 0 { format!("Rules  {actions}") } else { "Rules".into() }
    }

    fn visible(&self, ctx: &LaneContext) -> bool {
        !ctx.scene.logic.is_empty()
    }

    fn draw(&mut self, ui: &mut Ui, rect: Rect, ctx: &LaneContext) {
        let Some(hits) = ctx.hits else {
            lane_message(ui, rect, "Working out the rules…");
            return;
        };
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 3.0, theme::CARD);
        let any = hits.events.iter().any(|e| matches!(e.kind, EventKind::Action { .. }));
        let lanes = ctx.scene.logic.len().max(1);
        let band = rect.height() / lanes as f32;
        // One tick per screen point per rule set at most, so long busy clips stay quick to draw,
        // starting from the first event on screen.
        let mut last_x = vec![f32::NEG_INFINITY; lanes];
        let start = ctx.scale.time(rect.left()) as f64;
        let first = hits.events.partition_point(|e| e.time < start);
        for event in &hits.events[first..] {
            let EventKind::Action { rule, .. } = event.kind else { continue };
            let x = ctx.scale.x(event.time as f32);
            if x > rect.right() {
                break;
            }
            let row = (rule as usize).min(lanes - 1);
            if x - last_x[row] < 1.0 {
                continue;
            }
            last_x[row] = x;
            let top = rect.top() + band * row as f32;
            painter.line_segment(
                [Pos2::new(x, top + 2.0), Pos2::new(x, top + band - 2.0)],
                Stroke::new(1.5, rule_color(rule)),
            );
        }
        if !any {
            lane_message(ui, rect, "No rule has acted yet. Rules that only change balls show in the picture.");
        }
    }
}
