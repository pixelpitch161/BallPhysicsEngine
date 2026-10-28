//! The Bounces lane: how busy each moment is, ring escapes, and chord changes behind.

use super::lanes::{Lane, LaneContext, lane_message};
use crate::theme;
use eframe::egui::{self, Align2, FontId, Pos2, Rect, Stroke, Ui};

pub struct BouncesLane;

impl Lane for BouncesLane {
    fn id(&self) -> &'static str {
        "bounces"
    }

    fn title(&self, ctx: &LaneContext) -> String {
        match ctx.hits {
            Some(hits) => format!("Bounces  {}", hits.total),
            None => "Bounces".into(),
        }
    }

    fn draw(&mut self, ui: &mut Ui, rect: Rect, ctx: &LaneContext) {
        let (scale, scene, lane) = (ctx.scale, ctx.scene, rect);
        let Some(hits) = ctx.hits else {
            lane_message(ui, lane, "Working out the bounces…");
            return;
        };
        let painter = ui.painter_at(lane);
        painter.rect_filled(lane, 3.0, theme::CARD);

        // Chord changes, faintly behind the bars.
        let sound = &scene.sound;
        let cycle: f32 = sound.progression.iter().map(|c| c.seconds.max(0.0)).sum();
        if sound.enabled && cycle > 0.05 {
            let mut t = 0.0;
            'chords: loop {
                for step in &sound.progression {
                    if t > scale.span.min(scene.video.duration) {
                        break 'chords;
                    }
                    let x = scale.x(t);
                    painter.line_segment(
                        [Pos2::new(x, lane.top()), Pos2::new(x, lane.bottom())],
                        Stroke::new(1.0, theme::LINE),
                    );
                    if step.seconds * scale.width / scale.span > 24.0 {
                        painter.text(
                            Pos2::new(x + 3.0, lane.top() + 1.0),
                            Align2::LEFT_TOP,
                            &step.chord,
                            FontId::proportional(10.5),
                            theme::MUTED.gamma_multiply(0.8),
                        );
                    }
                    t += step.seconds.max(0.0);
                }
            }
        }

        // One bar per two screen points, as tall as the busiest moment under it.
        let busiest = hits.busiest(0.0, scene.video.duration).max(1) as f32;
        let column = 2.0;
        let mut x = lane.left();
        while x < lane.right() {
            let (from, to) = (scale.time(x), scale.time(x + column));
            let count = hits.busiest(from, to);
            if count > 0 {
                let height = (count as f32 / busiest).sqrt() * (lane.height() - 4.0);
                let bar =
                    Rect::from_min_max(Pos2::new(x, lane.bottom() - height), Pos2::new(x + column, lane.bottom()));
                painter.rect_filled(bar, 0.0, theme::ACCENT.gamma_multiply(0.75));
            }
            x += column;
        }
        for &escape in &hits.escapes {
            let x = scale.x(escape);
            let mark =
                vec![Pos2::new(x - 4.0, lane.top()), Pos2::new(x + 4.0, lane.top()), Pos2::new(x, lane.top() + 7.0)];
            painter.add(egui::Shape::convex_polygon(mark, theme::WARNING, Stroke::NONE));
        }
    }
}
