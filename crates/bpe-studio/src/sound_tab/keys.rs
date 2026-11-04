//! The notes (part 2: one slot): a keyboard from the lowest octave up, − and + for fewer or
//! more octaves. A key clicked turns its note on or off in the slot and plays it, stopped or
//! playing. Notes are named as FL Studio names them: C5 is middle C (MIDI 60).

use super::{TabCx, TabEdit, panel};
use crate::style;
use bpe_core::{NoteSlot, Value};
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};

/// The slot keys of the card.
const SLOTS: &str = "notes_slots";
const LOW: &str = "notes_low_octave";
const OCTAVES: &str = "notes_octaves";

/// The keyboard's height.
const KEYS_H: f32 = 80.0;
const BLACK_H: f32 = 48.0;

/// Note names in an octave.
const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
/// The white keys' notes in an octave.
const WHITE: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];

/// The name of MIDI note `note`, FL Studio style: 60 is "C5".
pub fn note_name(note: u8) -> String {
    format!("{}{}", NAMES[note as usize % 12], note / 12)
}

/// A keyboard of `octaves` octaves from C of `low`, the `lit` notes in `colour`. The key
/// clicked, if any.
pub fn keyboard(ui: &mut Ui, low: u8, octaves: u8, lit: &[u8], colour: Color32) -> Option<u8> {
    let whites = 7 * octaves as usize;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), KEYS_H), Sense::hover());
    let kw = rect.width() / whites.max(1) as f32;
    let first = 12 * low;
    let mut clicked = None;
    let mut keys: Vec<(u8, Rect, bool)> = Vec::new();
    for w in 0..whites {
        let note = first + 12 * (w / 7) as u8 + WHITE[w % 7];
        let r = Rect::from_min_size(Pos2::new(rect.left() + w as f32 * kw, rect.top()), Vec2::new(kw - 1.0, KEYS_H));
        keys.push((note, r, false));
    }
    for w in 0..whites {
        let n = WHITE[w % 7];
        if matches!(n, 0 | 2 | 5 | 7 | 9) {
            let note = first + 12 * (w / 7) as u8 + n + 1;
            let x = rect.left() + (w + 1) as f32 * kw - kw * 0.3;
            keys.push((note, Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(kw * 0.6, BLACK_H)), true));
        }
    }
    // White keys first, so the black ones on top of them take the clicks there.
    for (note, r, black) in keys {
        let id = ui.scope_id().with(("key", note));
        let response = ui.interact(r, id, Sense::click());
        let name = note_name(note);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
        if response.clicked() {
            clicked = Some(note);
        }
        let on = lit.contains(&note);
        if ui.is_rect_visible(r) {
            let painter = ui.painter();
            let fill = match (on, black) {
                (true, _) => colour,
                (false, true) => style::STAGE,
                (false, false) => style::SLIDER_FILL,
            };
            painter.rect_filled(r, 2.0, fill);
            if black {
                painter.rect_stroke(r, 2.0, Stroke::new(1.0, style::GROUND), egui::StrokeKind::Inside);
            } else if note % 12 == 0 {
                painter.text(
                    Pos2::new(r.center().x, r.bottom() - 4.0),
                    egui::Align2::CENTER_BOTTOM,
                    &name,
                    style::mono(10.0),
                    style::ON_LIGHT,
                );
            }
            if response.hovered() {
                painter.rect_stroke(r, 2.0, Stroke::new(1.0, style::TEXT_STRONG), egui::StrokeKind::Inside);
            }
        }
    }
    // The C labels, for screen readers and tests too.
    for o in 0..octaves {
        let note = first + 12 * o;
        let at = Rect::from_min_size(Pos2::new(rect.left() + 7.0 * o as f32 * kw, rect.bottom() - 16.0), Vec2::new(kw, 14.0));
        let label = ui.interact(at, ui.scope_id().with(("c label", note)), Sense::hover());
        let name = note_name(note);
        label.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &name));
    }
    clicked
}

/// The notes of slot 0.
pub fn slot_notes(cx: &TabCx) -> Vec<u8> {
    match cx.get(SLOTS) {
        Value::Slots(slots) => slots.first().map(|s| s.notes.clone()).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Draws the Notes panel.
pub fn show(ui: &mut Ui, cx: &mut TabCx) {
    panel(ui, |ui| {
        let low = cx.number(LOW).round().clamp(0.0, 8.0) as u8;
        let octaves = cx.number(OCTAVES).round().clamp(1.0, 8.0) as u8;
        let top = (low + octaves).min(10);
        let octaves = top - low;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Notes").font(style::semibold(13.0)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let more = ui.add_enabled(octaves < 8 && top < 10, egui::Button::new("+")).on_hover_text("One octave more");
                super::name_it(ui, &more, "One octave more");
                if more.clicked() {
                    cx.set(OCTAVES, Value::Int(octaves as i64 + 1));
                }
                let fewer = ui.add_enabled(octaves > 1, egui::Button::new("-")).on_hover_text("One octave fewer");
                super::name_it(ui, &fewer, "One octave fewer");
                if fewer.clicked() {
                    cx.set(OCTAVES, Value::Int(octaves as i64 - 1));
                }
                let range = format!("{} to {}", note_name(12 * low), note_name(12 * top - 1));
                ui.label(RichText::new(range).small().color(style::MUTED));
            });
        });
        let lit = slot_notes(cx);
        if let Some(note) = keyboard(ui, low, octaves, &lit, cx.colour) {
            let mut notes = lit.clone();
            match notes.binary_search(&note) {
                Ok(i) => {
                    notes.remove(i);
                }
                Err(i) => notes.insert(i, note),
            }
            let mut slots = match cx.get(SLOTS) {
                Value::Slots(s) => s,
                _ => Vec::new(),
            };
            if slots.is_empty() {
                slots.push(NoteSlot::default());
            }
            slots[0].order_clicked.retain(|n| notes.contains(n));
            if notes.contains(&note) {
                slots[0].order_clicked.push(note);
            }
            slots[0].notes = notes;
            cx.set(SLOTS, Value::Slots(slots));
            cx.edits.push(TabEdit::Audition(note));
        }
        ui.label(
            RichText::new("Each hit plays one of the lit notes, picked at random. Click a key to hear it.")
                .small()
                .color(style::MUTED),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::super::tests::{SHAPE, sounding};
    use super::*;
    use bpe_core::{Address, SlotId};
    use egui_kittest::kittest::Queryable;

    fn notes(h: &egui_kittest::Harness<'_, (crate::app::Studio, f32)>) -> Vec<u8> {
        let reg = bpe_kit::registry();
        match h.state().0.scene.value_at_time(&reg, &Address::on(SHAPE, SlotId::Sound, SLOTS), 0.0) {
            Some(Value::Slots(s)) => s[0].notes.clone(),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn note_names_follow_fl_studio() {
        assert_eq!(note_name(60), "C5");
        assert_eq!(note_name(61), "C#5");
        assert_eq!(note_name(71), "B5");
    }

    #[test]
    fn clicking_a_key_toggles_it_and_auditions() {
        let mut h = sounding();
        assert_eq!(notes(&h), [60]);
        h.get_by_role_and_label(egui::Role::Button, "D5").click();
        h.run();
        assert_eq!(notes(&h), [60, 62]);
        assert!(h.state().0.panels.inspector.auditions.contains(&(SHAPE, 62)), "it plays");
        h.get_by_role_and_label(egui::Role::Button, "D5").click();
        h.run();
        assert_eq!(notes(&h), [60]);
        h.get_by_role_and_label(egui::Role::Button, "C#5").click();
        h.run();
        assert_eq!(notes(&h), [60, 61], "the black key, not the white under it");
    }

    #[test]
    fn octave_buttons_grow_and_shrink() {
        let mut h = sounding();
        assert!(h.query_by_role_and_label(egui::Role::Button, "C7").is_none());
        h.get_by_label("One octave more").click();
        h.run();
        assert!(h.query_by_role_and_label(egui::Role::Button, "C7").is_some());
        h.get_by_label("One octave fewer").click();
        h.run();
        h.get_by_label("One octave fewer").click();
        h.run();
        assert!(h.query_by_role_and_label(egui::Role::Button, "C6").is_none());
        assert!(h.query_by_role_and_label(egui::Role::Button, "B5").is_some());
    }

    #[test]
    fn key_labels_use_fl_names() {
        let h = sounding();
        assert!(h.query_by_role_and_label(egui::Role::Label, "C5").is_some(), "C5 under middle C");
        assert!(h.query_by_label("C4 to B6").is_some());
    }
}
