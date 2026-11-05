//! The slot grid: a row per slot, its cells lined up with the keyboard's keys. Each row has its
//! number and chord name at the left (drag the number to move the row) and a marker at the right
//! for notes outside the keys shown. Right-click a row for Duplicate, Clear, Delete, Copy notes
//! and Paste notes.

use super::{NotesState, RowOp};
use crate::sound_tab::TabCx;
use crate::sound_tab::keys::{KeyGeom, note_name};
use crate::style;
use bpe_core::NoteSlot;
use bpe_kit::sound::MAX_SLOTS;
use bpe_sound::music::chord_name;
use eframe::egui::{self, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};

/// A row's height.
pub const ROW_H: f32 = 18.0;

/// The row menu's items.
fn row_menu(resp: &egui::Response, i: usize, rows: usize, has_clip: bool, ops: &mut Vec<RowOp>) {
    resp.context_menu(|ui| {
        if ui.add_enabled(rows < MAX_SLOTS, egui::Button::new("Duplicate")).clicked() {
            ops.push(RowOp::Duplicate(i));
            ui.close();
        }
        if ui.button("Clear").clicked() {
            ops.push(RowOp::Clear(i));
            ui.close();
        }
        if ui.button("Delete").clicked() {
            ops.push(RowOp::Delete(i));
            ui.close();
        }
        ui.separator();
        if ui.button("Copy notes").clicked() {
            ops.push(RowOp::Copy(i));
            ui.close();
        }
        let paste = ui
            .add_enabled(has_clip, egui::Button::new("Paste notes"))
            .on_disabled_hover_text("Copy a slot's notes first (from any shape)");
        if paste.clicked() {
            ops.push(RowOp::Paste(i));
            ui.close();
        }
    });
}

/// A label for screen readers and tests over `rect`, which shows `text`.
fn named(ui: &Ui, rect: Rect, id: impl std::hash::Hash + std::fmt::Debug, text: &str) {
    let r = ui.interact(rect, ui.scope_id().with(id), Sense::hover());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
}

/// Draws the rows and "+ Slot". Returns the edits made.
pub fn show(
    ui: &mut Ui,
    cx: &mut TabCx,
    state: &mut NotesState,
    rows: &[NoteSlot],
    picked: usize,
    geom: KeyGeom,
    playing: Option<usize>,
    sounding: u128,
) -> Vec<RowOp> {
    let mut ops = Vec::new();
    let width = ui.available_width();
    let (lo, hi) = geom.range();
    let keys_w = geom.kw * 7.0 * geom.octaves as f32;
    let has_clip = state_clip(cx);
    let mut rects = Vec::with_capacity(rows.len());
    let dragging = state.dragging.filter(|d| d.0 == cx.id);
    ui.spacing_mut().item_spacing.y = 2.0;
    for (i, slot) in rows.iter().enumerate() {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::hover());
        rects.push(rect);
        let gutter = Rect::from_min_max(rect.min, Pos2::new(geom.left - 4.0, rect.bottom()));
        let cells = Rect::from_min_size(Pos2::new(geom.left, rect.top()), Vec2::new(keys_w, ROW_H));
        let painter = ui.painter().clone();
        // The row: picked outlined in the text colour, the one playing in white.
        let is_picked = i == picked;
        let fill = if is_picked { style::SELECTED } else { style::GROUND };
        let line = if playing == Some(i) {
            Stroke::new(1.5, style::TEXT_STRONG)
        } else if is_picked {
            Stroke::new(1.0, style::TEXT)
        } else {
            Stroke::new(1.0, style::LINE_SOFT)
        };
        painter.rect_filled(cells, style::RADIUS, fill);
        painter.rect_stroke(cells, style::RADIUS, line, egui::StrokeKind::Inside);
        // The number (drag it to move the row) and the chord.
        let num = ui.interact(gutter, ui.scope_id().with(("slot row", i)), Sense::click_and_drag());
        let name = format!("Slot {}", i + 1);
        num.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
        if num.clicked() {
            state.picked.insert(cx.id, i);
        }
        if num.drag_started() {
            state.dragging = Some((cx.id, i, i));
        }
        let chord = chord_name(&slot.notes);
        let colour = if is_picked { style::TEXT } else { style::MUTED };
        painter.text(
            Pos2::new(gutter.left() + 2.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            format!("{}", i + 1),
            style::mono(11.0),
            colour,
        );
        if !chord.is_empty() {
            let at = Rect::from_min_max(Pos2::new(gutter.left() + 22.0, rect.top()), gutter.max);
            painter.text(
                Pos2::new(at.left(), rect.center().y),
                egui::Align2::LEFT_CENTER,
                &chord,
                style::mono(11.0),
                colour,
            );
            named(ui, at, ("chord", i), &chord);
        }
        if num.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        row_menu(&num, i, rows.len(), has_clip, &mut ops);
        // The cells: white keys first, so a black key's narrower cell on top takes its clicks.
        let mut notes: Vec<u8> = (lo..=hi).filter(|n| !KeyGeom::is_black(*n)).collect();
        notes.extend((lo..=hi).filter(|n| KeyGeom::is_black(*n)));
        for note in notes {
            let Some(x) = geom.x_of(note) else { continue };
            let w = if KeyGeom::is_black(note) { geom.kw * 0.6 } else { geom.kw };
            let cell = Rect::from_center_size(Pos2::new(x, rect.center().y), Vec2::new(w, ROW_H));
            let r = ui.interact(cell, ui.scope_id().with(("cell", i, note)), Sense::click());
            let cell_name = format!("{name} {}", note_name(note));
            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &cell_name));
            if r.clicked() {
                ops.push(RowOp::Toggle(i, note));
            }
            row_menu(&r, i, rows.len(), has_clip, &mut ops);
            let on = slot.notes.contains(&note);
            let dot = Rect::from_center_size(Pos2::new(x, rect.center().y), Vec2::splat(10.0));
            let lit_row = playing.map_or(is_picked, |p| p == i);
            if on && lit_row && super::super::keys::in_mask(sounding, note) {
                // Sounding now: lit white, ringed in the shape's colour.
                let big = dot.expand(2.0);
                painter.rect_filled(big, style::RADIUS, style::TEXT_STRONG);
                painter.rect_stroke(big, style::RADIUS, Stroke::new(1.5, cx.colour), egui::StrokeKind::Outside);
            } else if on {
                painter.rect_filled(dot, style::RADIUS, if is_picked { cx.colour } else { style::MUTED });
            } else if r.hovered() {
                painter.rect_stroke(dot, style::RADIUS, Stroke::new(1.0, style::FAINT), egui::StrokeKind::Inside);
            }
        }
        // Notes past either end of the keys still play; say how many.
        let below = slot.notes.iter().filter(|&&n| n < lo).count();
        let above = slot.notes.iter().filter(|&&n| n > hi).count();
        let marker = Rect::from_min_max(Pos2::new(cells.right() + 6.0, rect.top()), rect.max);
        let mut words = Vec::new();
        if below > 0 {
            words.push(format!("+{below} below"));
        }
        if above > 0 {
            words.push(format!("+{above} above"));
        }
        if !words.is_empty() {
            let text = words.join(" ");
            painter.text(
                Pos2::new(marker.left(), rect.center().y),
                egui::Align2::LEFT_CENTER,
                &text,
                style::mono(10.0),
                style::MUTED,
            );
            for w in &words {
                named(ui, marker, ("marker", i, w.as_str()), w);
            }
        }
    }
    // A row being dragged: where it would land, and the move when it is let go.
    if let Some((_, from, _)) = dragging {
        let y = ui.input(|i| i.pointer.interact_pos()).map(|p| p.y);
        let to = y.map_or(from, |y| rects.iter().position(|r| y < r.bottom()).unwrap_or(rects.len().saturating_sub(1)));
        state.dragging = Some((cx.id, from, to));
        if let Some(r) = rects.get(to) {
            let y = if to > from { r.bottom() + 1.0 } else { r.top() - 1.0 };
            ui.painter().hline(r.left()..=r.right(), y, Stroke::new(2.0, cx.colour));
        }
        if !ui.input(|i| i.pointer.any_down()) {
            if to != from {
                ops.push(RowOp::Move(from, to));
            }
            state.dragging = None;
        }
    }
    let add = ui.add_enabled(rows.len() < MAX_SLOTS, egui::Button::new(RichText::new("+ Slot").small()));
    let add = add.on_hover_text("Another slot of notes (up to 16)").on_disabled_hover_text("16 slots is the most");
    if add.clicked() {
        ops.push(RowOp::Add);
    }
    ops
}

/// Whether there are copied notes to paste (the tab passes them in through the clipboard).
fn state_clip(cx: &TabCx) -> bool {
    cx.notes_clip.is_some()
}
