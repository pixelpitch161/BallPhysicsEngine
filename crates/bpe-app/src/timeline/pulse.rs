//! The Pulse lane: one wavy row for each setting that pulses with the music, drawn from the
//! finished soundtrack once it is ready. Clicking a row opens the pulse's settings.

use super::lanes::{Lane, LaneContext};
use crate::setting_menu::pulse;
use crate::theme;
use eframe::egui::{self, Align2, FontId, Pos2, Rect, Response, Sense, Stroke, Ui};

const ROW: f32 = 22.0;
/// Width of the clickable name at the start of each row.
const LABEL: f32 = 170.0;

#[derive(Default)]
pub struct PulseLane {}

impl PulseLane {
    fn row(rect: Rect, index: usize) -> Rect {
        let top = rect.top() + 2.0 + index as f32 * ROW;
        Rect::from_x_y_ranges(rect.x_range(), top..=top + ROW - 2.0)
    }
}

impl Lane for PulseLane {
    fn id(&self) -> &'static str {
        "pulse"
    }

    fn title(&self, ctx: &LaneContext) -> String {
        format!("Pulse with music ({})", ctx.scene.modulators.len())
    }

    fn height(&self, ctx: &LaneContext) -> f32 {
        ctx.scene.modulators.len() as f32 * ROW + 4.0
    }

    fn visible(&self, ctx: &LaneContext) -> bool {
        !ctx.scene.modulators.is_empty()
    }

    fn draw(&mut self, ui: &mut Ui, rect: Rect, ctx: &LaneContext) {
        let painter = ui.painter_at(rect);
        let music = pulse::published_music(ui.ctx());
        let scale = ctx.scale;
        let end = scale.x(ctx.scene.video.duration).min(rect.right());
        for (index, m) in ctx.scene.modulators.iter().enumerate() {
            let row = Self::row(rect, index);
            painter.rect_filled(row, 3.0, theme::CARD);
            let color = if m.enabled { theme::ACCENT_TEXT } else { theme::MUTED };
            let wave = Rect::from_x_y_ranges(row.left()..=end, row.top() + 3.0..=row.bottom() - 3.0);
            let mut points = Vec::new();
            let mut x = wave.left();
            while x <= wave.right() {
                let time = scale.time(x) as f64;
                let s = match pulse::strength(m, &music, time) {
                    Some(s) => s.clamp(0.0, 1.2) / 1.2,
                    // Without the music it follows, a gentle wave just says "this pulses".
                    None => 0.5 + 0.35 * (time as f32 * 6.0).sin(),
                };
                points.push(Pos2::new(x, wave.bottom() - s * wave.height()));
                x += 2.0;
            }
            if points.len() > 1 {
                painter.add(egui::Shape::line(points, Stroke::new(1.3, color.gamma_multiply(0.85))));
            }
            let name = crate::keys::name(&m.property, ctx.scene);
            let label = format!("{name} · {}", m.source.name());
            painter.text(
                row.left_center() + egui::vec2(6.0, 0.0),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(11.5),
                theme::TEXT,
            );
        }
    }

    fn interact(&mut self, ui: &mut Ui, rect: Rect, _response: &Response, ctx: &LaneContext) -> bool {
        if rect.height() < ROW {
            return false;
        }
        // The name at the start of each row opens the pulse's settings; the rest scrubs.
        let mut used = false;
        for (index, m) in ctx.scene.modulators.iter().enumerate() {
            let row = Self::row(rect, index);
            let name = crate::keys::name(&m.property, ctx.scene);
            let label = Rect::from_min_size(row.min, egui::vec2(LABEL.min(row.width()), row.height()));
            let response = ui.interact(label, ui.scope_id().with(("pulse row", index)), Sense::click());
            let help = format!("{name} pulses with the {}. Click to change how.", m.source.name().to_lowercase());
            if response.on_hover_text(help).clicked() {
                pulse::request(ui.ctx(), pulse::Request::Edit(m.property.clone()));
                used = true;
            }
        }
        used
    }
}
