//! The Rules lane: when each rule acted, one colour per rule set. Hidden until a rule set
//! exists.

use super::lanes::{Lane, LaneContext, lane_message};
use crate::theme;
use bpe_sim::EventKind;
use eframe::egui::{self, Pos2, Rect, Stroke, Ui};

#[derive(Default)]
pub struct RulesLane {}

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
        let actions =
            ctx.hits.map_or(0, |h| h.events.iter().filter(|e| matches!(e.kind, EventKind::Action { .. })).count());
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
        let mut any = false;
        // One tick per screen point at most, so long busy clips stay quick to draw.
        let mut last_x = f32::NEG_INFINITY;
        for event in hits.events.iter() {
            let EventKind::Action { rule, .. } = event.kind else { continue };
            any = true;
            let x = ctx.scale.x(event.time as f32);
            if x > rect.right() {
                break;
            }
            if x - last_x < 1.0 {
                continue;
            }
            last_x = x;
            let lanes = ctx.scene.logic.len().max(1) as f32;
            let band = rect.height() / lanes;
            let top = rect.top() + band * (rule as f32).min(lanes - 1.0);
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
