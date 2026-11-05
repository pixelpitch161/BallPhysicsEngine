//! The Notes panel (board NotesGrid): the header (key, chord presets, the keyboard's octaves),
//! the slot grid lined up with the keyboard, the keyboard, then how hits play the notes (Plays)
//! and how the slots take turns (Slots change).
//!
//! The grid's rows are the shape's slots. Clicking in a row turns that note on or off there and
//! picks the row; the keyboard shows the picked row and a key clicked toggles it there and
//! plays it. Changing the key with notes in place asks whether to move them into the new key.

pub mod grid;
pub mod header;
pub mod presets_menu;

use super::keys::{KEYS_H, KeyGeom, keyboard_at};
use super::{TabCx, TabEdit, name_it, number, panel};
use crate::style;
use bpe_core::{NoteSlot, ThingId, Value};
use bpe_kit::sound::{BeatsOrBars, Change, MAX_SLOTS, Plays, Root, Scale, SlotOrder, StrumDir};
use bpe_sound::music::{Voicing, in_scale, move_into_key};
use eframe::egui::{self, RichText, Sense, Ui, Vec2};
use std::collections::BTreeMap;

/// The card's slot setting.
pub const SLOTS: &str = "notes_slots";
pub const LOW: &str = "notes_low_octave";
pub const OCTAVES: &str = "notes_octaves";

/// Room left of the keys for a row's number and chord name, and right of them for the
/// out-of-range markers.
pub const GUTTER: f32 = 72.0;
pub const MARKER_W: f32 = 64.0;

/// The Notes panel's own state, per shape where it matters.
#[derive(Clone, Debug, Default)]
pub struct NotesState {
    /// The row each shape's keyboard shows and edits.
    pub picked: BTreeMap<ThingId, usize>,
    /// A key change waiting on "Move notes" or "Keep notes".
    pub key_change: Option<(ThingId, Root, Scale)>,
    /// The row being dragged by its number, and the row it would land on.
    pub dragging: Option<(ThingId, usize, usize)>,
    /// The chord presets' options.
    pub voicing: Voicing,
    pub add_bass: bool,
    /// Until when (the editor's clock, seconds) each note of each shape stays lit after it
    /// sounded, so even a short note is seen.
    pub lit_until: BTreeMap<ThingId, Box<[f64; 128]>>,
    /// The shape whose notes were last read from the meters, and on which frame.
    pub last_read: Option<(ThingId, u64)>,
}

/// How long a note stays lit after it was last heard sounding.
pub const LIT_FOR: f64 = 0.15;

impl NotesState {
    /// Notes `sounding` (bit n for MIDI note n) on `shape` stay lit until `now + LIT_FOR`;
    /// returns the notes lit at `now`.
    pub fn light(&mut self, shape: ThingId, sounding: u128, now: f64) -> u128 {
        if sounding == 0 && !self.lit_until.contains_key(&shape) {
            return 0;
        }
        let until = self.lit_until.entry(shape).or_insert_with(|| Box::new([f64::NEG_INFINITY; 128]));
        let mut lit = 0u128;
        for (n, t) in until.iter_mut().enumerate() {
            if sounding >> n & 1 == 1 {
                *t = now + LIT_FOR;
            }
            if *t > now {
                lit |= 1 << n;
            }
        }
        lit
    }
}

/// The slots of the card (one empty slot when it has none).
pub fn slots(cx: &TabCx) -> Vec<NoteSlot> {
    match cx.get(SLOTS) {
        Value::Slots(s) if !s.is_empty() => s,
        _ => vec![NoteSlot::default()],
    }
}

/// The shape's key.
pub fn key(cx: &TabCx) -> (Root, Scale) {
    (cx.choice("notes_root"), cx.choice("notes_scale"))
}

/// The keyboard's lowest octave and how many it shows (never past B9).
pub fn octaves(cx: &TabCx) -> (u8, u8) {
    let low = cx.number(LOW).round().clamp(0.0, 8.0) as u8;
    let octaves = cx.number(OCTAVES).round().clamp(1.0, 8.0) as u8;
    let top = (low + octaves).min(10);
    (low, top - low)
}

/// A change to the slots.
#[derive(Clone, Debug, PartialEq)]
pub enum RowOp {
    Toggle(usize, u8),
    Add,
    Duplicate(usize),
    Clear(usize),
    Delete(usize),
    Copy(usize),
    Paste(usize),
    Move(usize, usize),
}

/// `slots` after `op`, and the row to pick after it. Pure, so the grid's edits are tested here.
pub fn apply_op(mut slots: Vec<NoteSlot>, op: &RowOp, clip: Option<&[u8]>, picked: usize) -> (Vec<NoteSlot>, usize) {
    if slots.is_empty() {
        slots.push(NoteSlot::default());
    }
    let last = slots.len() - 1;
    let at = |i: usize| i.min(last);
    let picked = match *op {
        RowOp::Toggle(i, note) => {
            let s = &mut slots[at(i)];
            match s.notes.binary_search(&note) {
                Ok(k) => {
                    s.notes.remove(k);
                    s.order_clicked.retain(|n| *n != note);
                }
                Err(k) => {
                    s.notes.insert(k, note);
                    s.order_clicked.retain(|n| *n != note);
                    s.order_clicked.push(note);
                }
            }
            at(i)
        }
        RowOp::Add if slots.len() < MAX_SLOTS => {
            slots.push(NoteSlot::default());
            slots.len() - 1
        }
        RowOp::Add => picked,
        RowOp::Duplicate(i) if slots.len() < MAX_SLOTS => {
            let copy = slots[at(i)].clone();
            slots.insert(at(i) + 1, copy);
            at(i) + 1
        }
        RowOp::Duplicate(_) => picked,
        RowOp::Clear(i) => {
            slots[at(i)] = NoteSlot::default();
            at(i)
        }
        RowOp::Delete(i) => {
            if slots.len() == 1 {
                slots[0] = NoteSlot::default();
                0
            } else {
                slots.remove(at(i));
                picked.min(slots.len() - 1)
            }
        }
        RowOp::Copy(_) => picked,
        RowOp::Paste(i) => {
            if let Some(notes) = clip {
                let mut n: Vec<u8> = notes.iter().copied().filter(|&n| n <= 127).collect();
                n.sort_unstable();
                n.dedup();
                slots[at(i)] = NoteSlot { order_clicked: n.clone(), notes: n };
            }
            at(i)
        }
        RowOp::Move(from, to) => {
            let (from, to) = (at(from), at(to));
            let row = slots.remove(from);
            slots.insert(to, row);
            to
        }
    };
    (slots, picked)
}

/// A number field for setting `key`, labelled `label`, with the right-click value menu.
pub fn field(ui: &mut Ui, cx: &mut TabCx, key: &str, label: &str, suffix: &str) -> Option<egui::Response> {
    let def = cx.def(key)?;
    let now = cx.get(key);
    let (lo, hi) = def.range.unwrap_or((0.0, 100.0));
    let mut v = number(&now).unwrap_or(lo);
    let int = def.ty == bpe_core::ValueType::Int;
    let l = ui.label(RichText::new(label).small().color(style::MUTED));
    let drag = egui::DragValue::new(&mut v)
        .range(lo..=hi)
        .speed(if int { 0.1 } else { (hi - lo) / 400.0 })
        .suffix(suffix)
        .max_decimals(if int { 0 } else { 1 });
    let r = ui.add(drag).labelled_by(l.id);
    if r.changed() {
        cx.set(key, if int { Value::Int(v.round() as i64) } else { Value::Number(v) });
    }
    cx.menu(&r, key, def, &now);
    Some(r)
}

/// A menu button for the choice setting `key`, named `name`, listing `all`.
pub fn choice_menu<T: bpe_core::Choice + Default + PartialEq + Copy>(
    ui: &mut Ui,
    cx: &mut TabCx,
    key: &str,
    name: &str,
    all: &[T],
    label: fn(T) -> &'static str,
) -> Option<T> {
    let now: T = cx.choice(key);
    let mut picked = None;
    let menu = ui.menu_button(label(now), |ui| {
        ui.set_min_width(130.0);
        for &c in all {
            if ui.selectable_label(c == now, label(c)).clicked() {
                picked = Some(c);
                ui.close();
            }
        }
    });
    name_it(ui, &menu.response, name);
    picked.filter(|p| *p != now)
}

/// Draws the Notes panel.
/// `playing` is the slot playing now and `sounding` the notes lit as they play (bit n for MIDI
/// note n).
pub fn show(ui: &mut Ui, cx: &mut TabCx, state: &mut NotesState, playing: Option<usize>, sounding: u128) {
    let clip = cx.notes_clip.clone();
    panel(ui, |ui| {
        let (root, scale) = key(cx);
        if let Some((r, s)) = header::show(ui, cx, state) {
            // A new key: with notes in place, ask before moving them.
            if slots(cx).iter().any(|s| !s.notes.is_empty()) {
                state.key_change = Some((cx.id, r, s));
            } else {
                set_key(cx, r, s);
            }
        }
        let (low, n_oct) = octaves(cx);
        let width = ui.available_width();
        let left = ui.cursor().left();
        let geom = KeyGeom::new(left + GUTTER, (width - GUTTER - MARKER_W).max(60.0), low, n_oct);
        let mut rows = slots(cx);
        let picked = state.picked.get(&cx.id).copied().unwrap_or(0).min(rows.len() - 1);
        let ops = grid::show(ui, cx, state, &rows, picked, geom, playing, sounding);
        // A row number clicked picks that row for the keyboard now.
        let picked = state.picked.get(&cx.id).copied().unwrap_or(0).min(rows.len() - 1);
        // The keyboard, lined up with the grid's columns.
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, KEYS_H), Sense::hover());
        let keys_rect = egui::Rect::from_min_size(
            egui::pos2(geom.left, rect.top()),
            Vec2::new(geom.kw * 7.0 * n_oct as f32, KEYS_H),
        );
        let lit = rows[picked].notes.clone();
        let in_key = |n: u8| scale != Scale::Chromatic && in_scale(root, scale, n);
        let mut ops = ops;
        if let Some(note) = keyboard_at(ui, keys_rect, geom, &lit, sounding, cx.colour, &in_key) {
            ops.push(RowOp::Toggle(picked, note));
            cx.edits.push(TabEdit::Audition(note));
        }
        ui.label(
            RichText::new(
                "Each row is a slot, lined up with the keys: click in a row to turn its notes on or off. The keys \
                 show the picked row; click one to hear it.",
            )
            .small()
            .color(style::MUTED),
        );
        // The rows' edits, in the order made.
        let mut changed = false;
        let mut now_picked = picked;
        for op in &ops {
            if let RowOp::Copy(i) = op {
                cx.edits.push(TabEdit::CopyNotes(rows[(*i).min(rows.len() - 1)].notes.clone()));
                continue;
            }
            let (next, p) = apply_op(rows, op, clip.as_deref(), now_picked);
            rows = next;
            now_picked = p;
            changed = true;
        }
        state.picked.insert(cx.id, now_picked);
        if changed {
            cx.edits.push(TabEdit::OwnStep);
            cx.set(SLOTS, Value::Slots(rows));
        }
        ui.separator();
        ui.columns(2, |cols| {
            plays(&mut cols[0], cx);
            changes(&mut cols[1], cx);
        });
    });
    key_change_dialog(ui, cx, state);
}

/// Sets the shape's key without touching its notes.
fn set_key(cx: &mut TabCx, root: Root, scale: Scale) {
    cx.edits.push(TabEdit::OwnStep);
    cx.set("notes_root", Value::Choice(bpe_core::Choice::name(root).into()));
    cx.set("notes_scale", Value::Choice(bpe_core::Choice::name(scale).into()));
}

/// "Move the notes into the new key?"
fn key_change_dialog(ui: &mut Ui, cx: &mut TabCx, state: &mut NotesState) {
    let Some((id, root, scale)) = state.key_change else { return };
    if id != cx.id {
        return;
    }
    let from = key(cx);
    let modal = egui::Modal::new(egui::Id::unique(("notes key change", id.0))).show(ui.ctx(), |ui| {
        ui.set_width(300.0);
        ui.label(RichText::new("Move the notes into the new key?").font(style::semibold(13.0)));
        ui.label(
            RichText::new(format!(
                "{} {} to {} {}. Moving shifts each note by the root's step and puts it on the new scale; no \
                 note is lost.",
                from.0.label(),
                from.1.label(),
                root.label(),
                scale.label()
            ))
            .small()
            .color(style::MUTED),
        );
        ui.add_space(6.0);
        let mut done = false;
        ui.horizontal(|ui| {
            if ui.button("Move notes").clicked() {
                let moved: Vec<NoteSlot> = slots(cx)
                    .into_iter()
                    .map(|s| {
                        let notes = move_into_key(&s.notes, from, (root, scale));
                        // The click order follows the notes it named.
                        let order = s
                            .order_clicked
                            .iter()
                            .filter_map(|n| s.notes.iter().position(|x| x == n))
                            .filter_map(|i| move_into_key(&s.notes[i..=i], from, (root, scale)).first().copied())
                            .filter(|n| notes.contains(n))
                            .fold(Vec::new(), |mut v, n| {
                                if !v.contains(&n) {
                                    v.push(n);
                                }
                                v
                            });
                        NoteSlot { notes, order_clicked: order }
                    })
                    .collect();
                set_key(cx, root, scale);
                cx.set(SLOTS, Value::Slots(moved));
                done = true;
            }
            if ui.button("Keep notes").clicked() {
                set_key(cx, root, scale);
                done = true;
            }
        });
        done
    });
    if modal.inner || modal.should_close() {
        state.key_change = None;
    }
}

/// Plays, Repeat each, Octave range, and Strum for All at once.
fn plays(ui: &mut Ui, cx: &mut TabCx) {
    ui.horizontal(|ui| {
        let l = ui.label(RichText::new("Plays").small().color(style::MUTED));
        if let Some(p) = choice_menu(ui, cx, "notes_plays", "Plays", &Plays::ALL, Plays::label) {
            cx.set("notes_plays", Value::Choice(bpe_core::Choice::name(p).into()));
        }
        let _ = l;
        field(ui, cx, "notes_repeat", "Repeat each", " hits");
    });
    ui.horizontal(|ui| {
        field(ui, cx, "notes_octave_range", "Octave range", " oct");
    });
    if cx.choice::<Plays>("notes_plays") == Plays::AllAtOnce {
        ui.horizontal(|ui| {
            field(ui, cx, "notes_strum", "Strum", " ms");
            cx.chips(
                ui,
                "notes_strum_dir",
                &[(StrumDir::Up, "Up"), (StrumDir::Down, "Down"), (StrumDir::Random, "Any")],
            );
        });
    }
}

/// Slots change, its Every, the Order, and Jump to slot.
fn changes(ui: &mut Ui, cx: &mut TabCx) {
    let change: Change = cx.choice("notes_change");
    ui.horizontal(|ui| {
        ui.label(RichText::new("Slots change").small().color(style::MUTED));
        if let Some(c) = choice_menu(ui, cx, "notes_change", "Slots change", &Change::ALL, Change::label) {
            cx.set("notes_change", Value::Choice(bpe_core::Choice::name(c).into()));
        }
    });
    ui.horizontal(|ui| match change {
        Change::ByTempo => {
            field(ui, cx, "notes_every_beats", "Every", "");
            cx.chips(ui, "notes_every_unit", &[(BeatsOrBars::Beats, "Beats"), (BeatsOrBars::Bars, "Bars")]);
        }
        Change::ByTime => {
            field(ui, cx, "notes_every_seconds", "Every", " s");
        }
        Change::ByHits => {
            field(ui, cx, "notes_every_hits", "Every", " hits");
        }
        Change::OnlyKeyframes => {
            ui.label(RichText::new("Only where a Jump to slot key says").small().color(style::MUTED));
        }
    });
    if change != Change::OnlyKeyframes {
        cx.chips(
            ui,
            "notes_order",
            &[
                (SlotOrder::Forward, "Forward"),
                (SlotOrder::Reverse, "Reverse"),
                (SlotOrder::PingPong, "Ping-pong"),
                (SlotOrder::Random, "Random"),
                (SlotOrder::Shuffle, "Shuffle"),
            ],
        );
    }
    ui.horizontal(|ui| {
        if let Some(r) = field(ui, cx, "notes_jump", "Jump to slot", "") {
            r.on_hover_text(
                "Right-click, Create Keyframe: the shape jumps to this slot there, whatever Slots change says",
            );
        }
        // Without keys the number does nothing, so say so.
        ui.label(RichText::new("keys only: right-click it, Create Keyframe").small().color(style::FAINT));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(notes: &[u8]) -> NoteSlot {
        NoteSlot { notes: notes.to_vec(), order_clicked: notes.to_vec() }
    }

    #[test]
    fn row_ops() {
        let s = vec![slot(&[60]), slot(&[64])];
        let (t, p) = apply_op(s.clone(), &RowOp::Toggle(1, 67), None, 0);
        assert_eq!((t[1].notes.clone(), t[1].order_clicked.clone(), p), (vec![64, 67], vec![64, 67], 1));
        let (t, _) = apply_op(t, &RowOp::Toggle(1, 64), None, 1);
        assert_eq!(t[1], slot(&[67]));
        let (t, p) = apply_op(s.clone(), &RowOp::Duplicate(0), None, 0);
        assert_eq!((t.len(), t[1].clone(), p), (3, slot(&[60]), 1));
        let (t, p) = apply_op(s.clone(), &RowOp::Delete(0), None, 1);
        assert_eq!((t, p), (vec![slot(&[64])], 0));
        let (t, _) = apply_op(vec![slot(&[60])], &RowOp::Delete(0), None, 0);
        assert_eq!(t, vec![NoteSlot::default()], "the last slot is cleared, not removed");
        let (t, p) = apply_op(s.clone(), &RowOp::Move(0, 1), None, 0);
        assert_eq!((t, p), (vec![slot(&[64]), slot(&[60])], 1));
        let (t, _) = apply_op(s.clone(), &RowOp::Paste(0), Some(&[67, 60, 67]), 0);
        assert_eq!(t[0].notes, [60, 67]);
        let full = vec![NoteSlot::default(); MAX_SLOTS];
        assert_eq!(apply_op(full.clone(), &RowOp::Add, None, 0).0.len(), MAX_SLOTS);
        assert_eq!(apply_op(full, &RowOp::Duplicate(3), None, 0).0.len(), MAX_SLOTS);
    }
}

#[cfg(test)]
mod ui_tests;

#[cfg(test)]
mod lit_tests {
    use super::*;

    #[test]
    fn a_note_stays_lit_a_moment_after_it_sounded() {
        let mut s = NotesState::default();
        let shape = ThingId(4);
        assert_eq!(s.light(shape, 0, 0.0), 0);
        assert_eq!(s.light(shape, 1 << 60, 1.0), 1 << 60);
        assert_eq!(s.light(shape, 0, 1.0 + LIT_FOR / 2.0), 1 << 60, "still lit");
        assert_eq!(s.light(shape, 1 << 62, 1.0 + LIT_FOR * 1.5), 1 << 62, "60 went out");
        assert_eq!(s.light(ThingId(5), 0, 2.0), 0, "another shape");
    }
}
