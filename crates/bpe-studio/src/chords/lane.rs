//! The lane: a bar ruler over one block per chord, as wide as the chord's beats. Click a block
//! to pick and hear it, drag it to move it, drag its right edge to change its length (Shift for
//! whole bars), and press Delete to remove the picked one.

use super::{Audition, ChordOutcome, ChordUi, function_colour, length_words};
use crate::theme;
use bpe_core::{MAX_BEATS, Progression};
use bpe_mix::{chord_name, numeral};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Id, Key, Modifiers, Pos2, Rect, RichText, Sense, Stroke, StrokeKind,
    Ui, WidgetInfo, pos2, vec2,
};

/// A block being dragged.
#[derive(Clone, Copy, Debug)]
pub(super) enum Drag {
    /// Its right edge, from `beats` long, moved `px` so far.
    Resize { index: usize, beats: u16, px: f32 },
    /// The whole block, to drop between two others.
    Move { index: usize },
}

const RULER: f32 = 16.0;
const BLOCK: f32 = 46.0;
/// The narrowest a beat is drawn; a longer progression scrolls sideways.
const MIN_BEAT: f32 = 14.0;
const EDGE: f32 = 6.0;

fn focus_key() -> Id {
    Id::unique("chord lane focused")
}

pub(super) fn focused(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<Id>(focus_key())).is_some_and(|id| ctx.memory(|m| m.has_focus(id)))
}

pub(super) fn show(ui: &mut Ui, p: &mut Progression, state: &mut ChordUi, out: &mut ChordOutcome) {
    state.blocks.clear();
    if p.chords.is_empty() {
        state.selected = None;
        ui.label(RichText::new("No chords yet. Add one from the palette below.").color(theme::MUTED));
        return;
    }
    let bar = p.meter.0.max(1) as u32;
    let total = p.total_beats();
    let span = (total.div_ceil(bar) * bar + bar).max(16);
    let per_beat = (ui.available_width() / span as f32).max(MIN_BEAT);
    egui::ScrollArea::horizontal().id_salt("chord lane scroll").show(ui, |ui| {
        let (rect, lane) = ui.allocate_exact_size(vec2(per_beat * span as f32, RULER + BLOCK), Sense::click());
        lane.widget_info(|| WidgetInfo::labeled(egui::Role::Group, true, "Chord lane"));
        if lane.has_focus() {
            ui.ctx().data_mut(|d| d.insert_temp(focus_key(), lane.id));
        }
        if lane.clicked() {
            lane.request_focus();
            state.selected = None;
        }
        let painter = ui.painter_at(rect);
        let blocks_top = rect.top() + RULER;
        painter.rect_filled(
            Rect::from_min_max(pos2(rect.left(), blocks_top), rect.right_bottom()),
            CornerRadius::same(4),
            theme::BACKGROUND,
        );
        for beat in 0..=span {
            let x = rect.left() + beat as f32 * per_beat;
            let on_bar = beat % bar == 0;
            let colour = if on_bar { theme::LINE } else { theme::LINE.gamma_multiply(0.4) };
            painter.vline(x, blocks_top..=rect.bottom(), Stroke::new(1.0, colour));
            if on_bar && beat < span {
                painter.text(
                    pos2(x + 3.0, rect.top() + 1.0),
                    Align2::LEFT_TOP,
                    (beat / bar + 1).to_string(),
                    FontId::proportional(11.0),
                    theme::MUTED,
                );
            }
        }

        let mut start = 0u32;
        let mut clicked_block = false;
        let mut remove = None;
        for i in 0..p.chords.len() {
            let c = p.chords[i];
            let x0 = rect.left() + start as f32 * per_beat;
            let r = Rect::from_min_max(
                pos2(x0 + 1.0, blocks_top + 2.0),
                pos2(x0 + c.beats as f32 * per_beat - 1.0, rect.bottom() - 2.0),
            );
            start += c.beats as u32;
            state.blocks.push(r);
            let body = ui.interact(r, lane.id.with(("block", i)), Sense::click_and_drag());
            let name = chord_name(&c, p.key);
            body.widget_info(|| WidgetInfo::labeled(egui::Role::Button, true, &name));
            let edge_rect = Rect::from_min_max(pos2(r.right() - EDGE, r.top()), pos2(r.right() + 2.0, r.bottom()));
            let edge = ui.interact(edge_rect, lane.id.with(("edge", i)), Sense::drag());
            if edge.hovered() || edge.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }

            if body.clicked() {
                state.selected = Some(i);
                clicked_block = true;
                out.audition = Some(Audition::Chord(c));
            }
            if edge.drag_started() {
                state.drag = Some(Drag::Resize { index: i, beats: c.beats, px: 0.0 });
                state.selected = Some(i);
            } else if body.drag_started() {
                state.drag = Some(Drag::Move { index: i });
                state.selected = Some(i);
            }
            if let Some(Drag::Resize { index, beats, px }) = &mut state.drag
                && *index == i
                && edge.dragged()
            {
                *px += edge.drag_delta().x;
                let shift = ui.input(|inp| inp.modifiers.shift);
                let exact = *beats as f32 + *px / per_beat;
                let new = if shift {
                    ((exact / bar as f32).round().max(1.0) * bar as f32) as u16
                } else {
                    exact.round().max(1.0) as u16
                }
                .min(MAX_BEATS);
                if new != p.chords[i].beats {
                    p.chords[i].beats = new;
                    out.changed = true;
                }
            }
            let fill = function_colour(&c, p.key);
            let picked = state.selected == Some(i);
            painter.rect_filled(r, CornerRadius::same(5), fill.gamma_multiply(if picked { 0.95 } else { 0.7 }));
            if picked {
                painter.rect_stroke(r, CornerRadius::same(5), Stroke::new(2.0, Color32::WHITE), StrokeKind::Inside);
            }
            let text = ui.painter_at(r.shrink(2.0));
            text.text(
                pos2(r.left() + 6.0, r.top() + 5.0),
                Align2::LEFT_TOP,
                &name,
                FontId::proportional(14.0),
                theme::BACKGROUND,
            );
            text.text(
                pos2(r.left() + 6.0, r.bottom() - 5.0),
                Align2::LEFT_BOTTOM,
                format!("{} · {}", numeral(&c, p.key), length_words(c.beats, bar as u16)),
                FontId::proportional(11.0),
                theme::BACKGROUND.gamma_multiply(0.8),
            );
        }

        if let Some(Drag::Move { index }) = state.drag {
            let pointer = ui.input(|inp| inp.pointer.interact_pos());
            let to = pointer.map(|at| drop_index(&state.blocks, at));
            if let Some(to) = to {
                let x = if to < state.blocks.len() { state.blocks[to].left() } else { state.blocks[to - 1].right() };
                painter.vline(x - 1.0, blocks_top..=rect.bottom(), Stroke::new(3.0, Color32::WHITE));
            }
            if ui.input(|inp| inp.pointer.any_released()) {
                if let Some(to) = to {
                    let at = if to > index { to - 1 } else { to };
                    if at != index {
                        let c = p.chords.remove(index);
                        p.chords.insert(at, c);
                        state.selected = Some(at);
                        out.changed = true;
                    }
                }
                state.drag = None;
            }
        } else if state.drag.is_some() && ui.input(|inp| !inp.pointer.any_down()) {
            state.drag = None;
        }

        if clicked_block || state.drag.is_some() {
            lane.request_focus();
        } else if lane.clicked_elsewhere() {
            lane.surrender_focus();
        }
        if lane.has_focus()
            && let Some(i) = state.selected.filter(|&i| i < p.chords.len())
            && ui.input_mut(|inp| inp.consume_key(Modifiers::NONE, Key::Delete))
        {
            remove = Some(i);
        }
        if let Some(i) = remove {
            p.chords.remove(i);
            state.selected = None;
            out.changed = true;
        }
    });
}

/// Where a dragged block lands: before the first block whose middle is right of `at`.
fn drop_index(blocks: &[Rect], at: Pos2) -> usize {
    blocks.iter().position(|r| at.x < r.center().x).unwrap_or(blocks.len())
}
