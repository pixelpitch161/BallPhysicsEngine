//! The rounds strip under the mini view: the clip's length as a lane, each round a block from
//! when it fires to when the next does, labelled with its ball count. Rounds fired so far are
//! filled; those still to come (when their time is known) are outlined.

use bpe_kit::things::emitter::sim::RoundMark;
use eframe::egui::{self, Pos2, Rect, Sense, Stroke, Ui};

use crate::style;

/// The strip's height, in points.
pub const HEIGHT: f32 = 26.0;
/// The narrowest a block draws, in points.
const NARROWEST: f32 = 3.0;

/// Each round's block, from left to right: its span along the lane (0 to 1 of the clip) and the
/// round. A block reaches to the next round's start, the last one to the clip's end.
pub fn blocks(rounds: &[RoundMark], duration: f32) -> Vec<(f32, f32, RoundMark)> {
    let length = duration.max(1e-3) as f64;
    let mut out = Vec::new();
    for (i, r) in rounds.iter().enumerate() {
        if r.time > length {
            break;
        }
        let end = rounds.get(i + 1).map_or(length, |n| n.time.min(length));
        out.push(((r.time / length) as f32, (end / length) as f32, *r));
    }
    out
}

/// A count in few letters: 999, 1.2k, 34k.
pub fn short_count(n: u32) -> String {
    match n {
        0..=999 => n.to_string(),
        1000..=9_999 => format!("{:.1}k", n as f32 / 1000.0),
        _ => format!("{}k", n / 1000),
    }
}

/// Draws the strip for `rounds` over a clip `duration` seconds long, the playhead at `time`.
pub fn show(ui: &mut Ui, rounds: &[RoundMark], duration: f32, time: f64) {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, HEIGHT), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, style::RADIUS, style::GROUND);
    let lane = rect.shrink2(egui::vec2(2.0, 3.0));
    let x = |t: f32| lane.left() + t * lane.width();
    let shown = blocks(rounds, duration);
    for &(from, to, r) in &shown {
        let block = Rect::from_min_max(
            Pos2::new(x(from), lane.top()),
            Pos2::new(x(to).max(x(from) + NARROWEST), lane.bottom()),
        );
        let block = block.shrink2(egui::vec2(0.5, 0.0));
        if r.fired {
            painter.rect_filled(block, 2.0, style::RAISED);
            painter.rect_stroke(block, 2.0, Stroke::new(1.0, style::LINE), egui::StrokeKind::Inside);
        } else {
            painter.rect_stroke(block, 2.0, Stroke::new(1.0, style::FAINT), egui::StrokeKind::Inside);
        }
        let label = short_count(r.size);
        let room = block.width() - 4.0;
        if room >= 7.0 * label.len() as f32 {
            let ink = if r.fired { style::TEXT } else { style::MUTED };
            painter.text(block.center(), egui::Align2::CENTER_CENTER, label, style::mono(10.0), ink);
        }
    }
    if shown.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Rounds show here as they fire",
            egui::FontId::proportional(10.0),
            style::MUTED,
        );
    }
    // The playhead.
    let t = (time as f32 / duration.max(1e-3)).clamp(0.0, 1.0);
    painter.line_segment(
        [Pos2::new(x(t), rect.top()), Pos2::new(x(t), rect.bottom())],
        Stroke::new(1.0, style::TEXT_STRONG),
    );
    let fired = rounds.iter().filter(|r| r.fired).count();
    let words = format!("Rounds: {fired} fired so far, {} more to come", rounds.len() - fired);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, words.clone()));
    if let Some(p) = response.hover_pos() {
        let at = ((p.x - lane.left()) / lane.width().max(1.0)).clamp(0.0, 1.0);
        let hovered = shown.iter().rev().find(|(from, ..)| *from <= at);
        let text = match hovered {
            Some((_, _, r)) => {
                let when = if r.fired { "fired" } else { "fires" };
                format!("Round {}: {} balls, {when} at {:.2} s", r.round, r.size, r.time)
            }
            None => words,
        };
        response.on_hover_text(text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mark(time: f64, round: u32, fired: bool) -> RoundMark {
        RoundMark { time, round, size: 10, fired }
    }

    #[test]
    fn each_round_reaches_to_the_next_and_the_last_to_the_end() {
        let rounds = [mark(0.0, 1, true), mark(5.0, 2, true), mark(10.0, 3, false), mark(30.0, 4, false)];
        let b = blocks(&rounds, 20.0);
        assert_eq!(b.len(), 3, "a round after the clip's end isn't shown");
        assert_eq!((b[0].0, b[0].1), (0.0, 0.25));
        assert_eq!((b[2].0, b[2].1), (0.5, 1.0));
        assert!(b[1].2.fired && !b[2].2.fired);
    }

    #[test]
    fn counts_are_short() {
        assert_eq!(short_count(10), "10");
        assert_eq!(short_count(1000), "1.0k");
        assert_eq!(short_count(1250), "1.2k");
        assert_eq!(short_count(34_000), "34k");
    }
}
