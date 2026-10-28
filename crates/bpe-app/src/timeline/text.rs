//! The Text lane of the timeline: one clip per caption, showing when it is on screen. Drag a
//! clip to move it, drag its ends to change when it appears and disappears, click it to edit it.

use crate::theme;
use bpe_scene::Scene;
use eframe::egui::{self, Align2, Color32, CursorIcon, FontId, Rect, Sense, Stroke, Ui};

/// Height of one caption's row.
const CLIP_ROW: f32 = 18.0;
/// Screen points at each end of a clip that resize it instead of moving it.
const HANDLE: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    /// Moving a whole clip; `grab` is how far into the clip it was picked up, and `length` how
    /// long the clip was then, in seconds, so moving it never changes its length.
    Move {
        index: usize,
        grab: f32,
        length: f32,
    },
    Start {
        index: usize,
    },
    End {
        index: usize,
    },
}

#[derive(Default)]
pub struct TextLane {
    drag: Option<Drag>,
}

/// What happened in the lane this frame.
#[derive(Default)]
pub struct LaneOutcome {
    /// A clip was clicked: show its caption in the Text tab.
    pub clicked: Option<usize>,
}

impl TextLane {
    /// Height the lane needs for the scene's captions, or 0 with none.
    pub fn height(scene: &Scene) -> f32 {
        if scene.captions.is_empty() { 0.0 } else { scene.captions.len().min(8) as f32 * CLIP_ROW + 6.0 }
    }

    /// Draws the lane across the full width of `ui`. Times map to screen x as
    /// `left + time / span * width`, the same as the rest of the timeline.
    pub fn show(&mut self, ui: &mut Ui, scene: &mut Scene, left: f32, width: f32, span: f32) -> LaneOutcome {
        let mut outcome = LaneOutcome::default();
        let height = Self::height(scene);
        if height == 0.0 {
            self.drag = None;
            return outcome;
        }
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::hover());
        let x = |t: f32| left + t / span.max(1e-3) * width;
        let time = |px: f32| ((px - left) / width.max(1.0) * span).max(0.0);
        let fps = scene.video.fps.max(1) as f32;
        let frame = |t: f32| (t * fps).round() / fps;
        let duration = scene.video.duration;
        let painter = ui.painter_at(rect);
        painter.text(
            rect.left_top() + egui::vec2(4.0, 3.0),
            Align2::LEFT_TOP,
            "Text",
            FontId::proportional(13.0),
            theme::TEXT,
        );
        let lane = Rect::from_x_y_ranges(left..=rect.right(), rect.top() + 2.0..=rect.bottom() - 2.0);
        painter.rect_filled(lane, 3.0, theme::CARD);

        let pointer = ui.input(|i| i.pointer.interact_pos());
        let down = ui.input(|i| i.pointer.primary_down());
        for (index, caption) in scene.captions.iter_mut().enumerate().take(8) {
            let (from, to) = caption.span(duration);
            let top = rect.top() + 3.0 + index as f32 * CLIP_ROW;
            let clip = Rect::from_x_y_ranges(x(from)..=x(to).max(x(from) + 4.0), top..=top + CLIP_ROW - 3.0);
            let id = ui.scope_id().with(("caption clip", index));
            let response = ui.interact(clip, id, Sense::click_and_drag());
            let near_start = pointer.is_some_and(|p| (p.x - clip.left()).abs() <= HANDLE);
            let near_end = pointer.is_some_and(|p| (p.x - clip.right()).abs() <= HANDLE);
            if response.hovered()
                || matches!(self.drag, Some(Drag::Start { index: i } | Drag::End { index: i }) if i == index)
            {
                if near_start || near_end {
                    ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
                } else {
                    ui.ctx().set_cursor_icon(CursorIcon::Grab);
                }
            }
            if response.drag_started()
                && let Some(p) = pointer
            {
                // On a clip too short to tell its ends apart, the nearer end wins.
                let end_nearer = (p.x - clip.right()).abs() < (p.x - clip.left()).abs();
                self.drag = Some(if near_end && (!near_start || end_nearer) {
                    Drag::End { index }
                } else if near_start {
                    Drag::Start { index }
                } else {
                    Drag::Move { index, grab: time(p.x) - from, length: to - from }
                });
            }
            if response.clicked() {
                outcome.clicked = Some(index);
            }
            let fill = if caption.enabled { theme::ACCENT.gamma_multiply(0.55) } else { theme::RAISED };
            let hovered = response.hovered() || self.drag.is_some_and(|d| drag_index(d) == index);
            painter.rect_filled(clip, 3.0, fill);
            if hovered {
                painter.rect_stroke(clip, 3.0, Stroke::new(1.0, theme::ACCENT_TEXT), egui::StrokeKind::Inside);
            }
            let label = caption.text.lines().next().unwrap_or_default().to_string();
            let text_painter = ui.painter_at(clip.shrink(1.0));
            text_painter.text(
                clip.left_center() + egui::vec2(5.0, 0.0),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(11.0),
                Color32::WHITE,
            );
            response.on_hover_text(
                "Drag to move. Drag an end to change when the caption appears or disappears. Click to edit.",
            );
        }

        match (self.drag, pointer) {
            (Some(_), _) if !down => self.drag = None,
            (Some(Drag::Move { index, grab, length }), Some(p)) => {
                if let Some(caption) = scene.captions.get_mut(index) {
                    if caption.show_to > 0.0 {
                        // A clip with a fixed end stops at the end of the video.
                        let start = frame((time(p.x) - grab).clamp(0.0, (duration - length).max(0.0)));
                        caption.show_from = start;
                        caption.show_to = start + length;
                    } else {
                        caption.show_from = frame((time(p.x) - grab).clamp(0.0, (duration - 0.1).max(0.0)));
                    }
                }
            }
            (Some(Drag::Start { index }), Some(p)) => {
                if let Some(caption) = scene.captions.get_mut(index) {
                    let (_, to) = caption.span(duration);
                    caption.show_from = frame(time(p.x)).clamp(0.0, (to - 0.1).max(0.0));
                }
            }
            (Some(Drag::End { index }), Some(p)) => {
                if let Some(caption) = scene.captions.get_mut(index) {
                    let end = frame(time(p.x)).min(duration);
                    // Dragging the end to the end of the video keeps the caption to the end.
                    caption.show_to = if end >= duration - 0.5 / fps { 0.0 } else { end.max(caption.show_from + 0.1) };
                }
            }
            _ => {}
        }
        if self.drag.is_some() {
            ui.ctx().request_repaint();
        }
        // Past the end of the video.
        painter.rect_filled(
            Rect::from_x_y_ranges(x(duration)..=lane.right(), lane.y_range()),
            0.0,
            Color32::from_black_alpha(90),
        );
        outcome
    }
}

fn drag_index(drag: Drag) -> usize {
    match drag {
        Drag::Move { index, .. } | Drag::Start { index } | Drag::End { index } => index,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_scene::{Caption, CaptionStyle};

    #[test]
    fn lane_grows_with_captions() {
        let mut scene = Scene::default();
        assert_eq!(TextLane::height(&scene), 0.0);
        scene.captions.push(Caption::new("a", CaptionStyle::Hook));
        let one = TextLane::height(&scene);
        scene.captions.push(Caption::new("b", CaptionStyle::Hook));
        assert!(TextLane::height(&scene) > one);
    }
}
