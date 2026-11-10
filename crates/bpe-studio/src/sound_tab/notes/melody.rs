//! The melody (Plays Melody): a row of steps, each a note or a chord, played one a hit in turn.
//! While Plays is Melody the keyboard shows and toggles the picked step, and this sits under it:
//! the step chips, Add step / Remove step / Clear, At the end, a strip showing the steps as bars,
//! and Import MIDI as a melody.

use super::super::keys::note_name;
use super::super::{TabCx, TabEdit, name_it};
use super::NotesState;
use crate::style;
use bpe_core::{NoteSlot, Value};
use bpe_kit::sound::{MAX_MELODY, MelodyEnd};
use eframe::egui::{self, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};

/// The card's melody setting.
pub const MELODY: &str = "notes_melody";

/// The strip's height.
const STRIP_H: f32 = 40.0;

/// The melody's steps (none when it has none).
pub fn steps(cx: &TabCx) -> Vec<NoteSlot> {
    match cx.get(MELODY) {
        Value::Slots(s) => s,
        _ => Vec::new(),
    }
}

/// A change to the melody.
#[derive(Clone, Debug, PartialEq)]
pub enum StepOp {
    /// Turns a note on or off in a step (the first step is made when there is none).
    Toggle(usize, u8),
    /// A new empty step after the picked one.
    Add,
    /// Removes the picked step.
    Remove,
    /// Removes every step.
    Clear,
}

/// `steps` after `op`, and the step to pick after it. Pure, so the edits are tested here.
pub fn apply(mut steps: Vec<NoteSlot>, op: &StepOp, picked: usize) -> (Vec<NoteSlot>, usize) {
    let picked = picked.min(steps.len().saturating_sub(1));
    match *op {
        StepOp::Toggle(i, note) => {
            if steps.is_empty() {
                steps.push(NoteSlot::default());
            }
            let i = i.min(steps.len() - 1);
            let s = &mut steps[i];
            match s.notes.binary_search(&note) {
                Ok(k) => {
                    s.notes.remove(k);
                }
                Err(k) if s.notes.len() < 16 => s.notes.insert(k, note),
                Err(_) => {}
            }
            s.order_clicked = s.notes.clone();
            (steps, i)
        }
        StepOp::Add if steps.len() < MAX_MELODY => {
            let at = if steps.is_empty() { 0 } else { picked + 1 };
            steps.insert(at, NoteSlot::default());
            (steps, at)
        }
        StepOp::Add => (steps, picked),
        StepOp::Remove if !steps.is_empty() => {
            steps.remove(picked);
            let p = picked.min(steps.len().saturating_sub(1));
            (steps, p)
        }
        StepOp::Remove => (steps, 0),
        StepOp::Clear => (Vec::new(), 0),
    }
}

/// A step's name on its chip: its notes ("C5", a chord "C5+E5"), or "rest".
pub fn step_name(step: &NoteSlot) -> String {
    if step.notes.is_empty() {
        return "rest".into();
    }
    step.notes.iter().map(|&n| note_name(n)).collect::<Vec<_>>().join("+")
}

/// The picked step of this sound's melody.
pub fn picked(cx: &TabCx, state: &NotesState) -> usize {
    state.melody_picked.get(&cx.id).copied().unwrap_or(0)
}

/// Applies `ops` to the melody, keeping the picked step.
pub fn apply_ops(cx: &mut TabCx, state: &mut NotesState, ops: &[StepOp]) {
    if ops.is_empty() {
        return;
    }
    let mut now = steps(cx);
    let mut p = picked(cx, state);
    for op in ops {
        (now, p) = apply(now, op, p);
    }
    state.melody_picked.insert(cx.id, p);
    cx.edits.push(TabEdit::OwnStep);
    cx.set(MELODY, Value::Slots(now));
}

/// Draws the melody's part of the Notes panel.
pub fn show(ui: &mut Ui, cx: &mut TabCx, state: &mut NotesState) {
    let all = steps(cx);
    let picked = picked(cx, state).min(all.len().saturating_sub(1));
    let mut ops = Vec::new();
    ui.horizontal(|ui| {
        let count = all.len();
        ui.label(RichText::new("Melody").font(style::semibold(12.0)));
        ui.label(
            RichText::new(format!("{count} step{}: each hit plays the next", if count == 1 { "" } else { "s" }))
                .small()
                .color(style::MUTED),
        );
    });
    // The steps, wrapping; a long melody scrolls.
    egui::ScrollArea::vertical().id_salt(("melody steps", cx.id.0)).max_height(96.0).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(2.0, 2.0);
            for (i, step) in all.iter().enumerate() {
                let text = format!("{} {}", i + 1, step_name(step));
                let r = ui.add(style::toggle_button(&text, i == picked).min_size(egui::vec2(0.0, 20.0)));
                name_it(ui, &r, &format!("Step {}", i + 1));
                if r.clicked() {
                    state.melody_picked.insert(cx.id, i);
                }
            }
            if all.is_empty() {
                ui.label(RichText::new("No steps yet: click a key, or Add step").small().color(style::FAINT));
            }
        });
    });
    ui.horizontal(|ui| {
        let add = ui.add_enabled(all.len() < MAX_MELODY, egui::Button::new("Add step"));
        name_it(ui, &add, "Add step");
        if add.clicked() {
            ops.push(StepOp::Add);
        }
        let remove = ui.add_enabled(!all.is_empty(), egui::Button::new("Remove step"));
        name_it(ui, &remove, "Remove step");
        if remove.clicked() {
            ops.push(StepOp::Remove);
        }
        let clear = ui.add_enabled(!all.is_empty(), egui::Button::new("Clear melody"));
        name_it(ui, &clear, "Clear melody");
        if clear.clicked() {
            ops.push(StepOp::Clear);
        }
        ui.label(RichText::new("At the end").small().color(style::MUTED));
        cx.chips(ui, "notes_melody_end", &[(MelodyEnd::Wrap, "Starts again"), (MelodyEnd::Hold, "Holds the last")]);
    });
    strip(ui, &all, picked, cx.colour);
    import_button(ui, cx);
    apply_ops(cx, state, &ops);
}

/// The steps as bars, low notes at the bottom, the picked step marked.
fn strip(ui: &mut Ui, steps: &[NoteSlot], picked: usize, colour: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), STRIP_H), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, style::RADIUS, style::GROUND);
    painter.rect_stroke(rect, style::RADIUS, Stroke::new(1.0, style::LINE_SOFT), egui::StrokeKind::Inside);
    let notes = steps.iter().flat_map(|s| s.notes.iter().copied());
    let Some((lo, hi)) =
        notes.fold(None, |r: Option<(u8, u8)>, n| Some(r.map_or((n, n), |(a, b)| (a.min(n), b.max(n)))))
    else {
        return;
    };
    let span = (hi - lo).max(11) as f32 + 1.0;
    let w = rect.width() / steps.len() as f32;
    let inner = rect.shrink(3.0);
    let row = inner.height() / span;
    for (i, step) in steps.iter().enumerate() {
        let x = rect.left() + i as f32 * w;
        if i == picked {
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(w, rect.height())),
                0.0,
                style::SELECTED,
            );
        }
        for &n in &step.notes {
            let y = inner.bottom() - (n - lo) as f32 * row - row;
            let bar = Rect::from_min_size(Pos2::new(x + 0.5, y), Vec2::new((w - 1.0).max(1.0), row.max(1.5)));
            painter.rect_filled(bar, 0.0, colour);
        }
    }
}

/// Import MIDI as a melody…: every chord of a MIDI file, in the order it plays, becomes a step.
fn import_button(ui: &mut Ui, cx: &mut TabCx) {
    let r = ui.button("Import MIDI as a melody…").on_hover_text(
        "Each chord in a MIDI file becomes a step of the melody, in the order it plays (up to 1024), and its tempo \
         and time signature come across",
    );
    name_it(ui, &r, "Import MIDI as a melody");
    if !r.clicked() {
        return;
    }
    let Some(path) = rfd::FileDialog::new()
        .set_title("Import MIDI as a melody")
        .add_filter("MIDI files", &["mid", "midi"])
        .pick_file()
    else {
        return;
    };
    match super::header::read_midi(&path) {
        Ok((file, import)) => {
            cx.edits.push(TabEdit::OwnStep);
            cx.edits.push(TabEdit::ImportMidiMelody { file, import: Box::new(import) });
        }
        Err(text) => cx.edits.push(TabEdit::Note { text, problem: true }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(notes: &[u8]) -> NoteSlot {
        NoteSlot { notes: notes.to_vec(), order_clicked: notes.to_vec() }
    }

    #[test]
    fn step_ops() {
        let (s, p) = apply(Vec::new(), &StepOp::Toggle(3, 60), 0);
        assert_eq!((s, p), (vec![step(&[60])], 0), "a key on an empty melody makes its first step");
        let (s, p) = apply(vec![step(&[60]), step(&[62])], &StepOp::Add, 0);
        assert_eq!((s.len(), s[1].clone(), p), (3, NoteSlot::default(), 1), "after the picked step");
        let (s, p) = apply(s, &StepOp::Toggle(1, 64), p);
        assert_eq!((s[1].clone(), p), (step(&[64]), 1));
        let (s, p) = apply(s, &StepOp::Toggle(1, 60), p);
        assert_eq!(s[1], step(&[60, 64]), "a chord");
        let (s, p) = apply(s, &StepOp::Remove, p);
        assert_eq!((s, p), (vec![step(&[60]), step(&[62])], 1));
        let (s, p) = apply(vec![step(&[60])], &StepOp::Remove, 0);
        assert_eq!((s, p), (vec![], 0));
        assert_eq!(apply(vec![step(&[60])], &StepOp::Clear, 0), (vec![], 0));
        let full = vec![NoteSlot::default(); MAX_MELODY];
        assert_eq!(apply(full, &StepOp::Add, 0).0.len(), MAX_MELODY, "never past 1024 steps");
        assert_eq!(step_name(&step(&[60, 64])), "C5+E5");
        assert_eq!(step_name(&NoteSlot::default()), "rest");
    }
}
