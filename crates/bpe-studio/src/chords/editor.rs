//! The palette of the key's own and borrowed chords, and the picked chord's settings.

use super::{Audition, ChordOutcome, ChordUi, function_colour, length_words};
use crate::theme;
use bpe_core::{Chord, Extension, MAX_BEATS, Progression, Quality};
use bpe_mix::{borrowed, chord_name, diatonic, intervals, note_name, numeral, parse_chord};
use eframe::egui::{self, RichText, Stroke, Ui, vec2};

pub(super) fn palette(ui: &mut Ui, p: &mut Progression, state: &mut ChordUi, out: &mut ChordOutcome) {
    let bar = p.meter.0.max(1) as u16;
    let own: Vec<Chord> = (0..7).map(|d| diatonic(p.key, d, bar)).collect();
    let mut add = None;
    ui.label(RichText::new("In key").small().color(theme::MUTED));
    ui.horizontal_wrapped(|ui| {
        for c in &own {
            if chord_button(ui, c, p).clicked() {
                add = Some(*c);
            }
        }
    });
    let lent = borrowed(p.key);
    if !lent.is_empty() {
        ui.label(RichText::new("Borrowed").small().color(theme::MUTED))
            .on_hover_text("Chords from the parallel key, for colour");
        ui.horizontal_wrapped(|ui| {
            for c in &lent {
                if chord_button(ui, c, p).clicked() {
                    add = Some(Chord { beats: bar, ..*c });
                }
            }
        });
    }
    if let Some(c) = add {
        let at = state.selected.filter(|&i| i < p.chords.len()).map_or(p.chords.len(), |i| i + 1);
        p.chords.insert(at, c);
        state.selected = Some(at);
        out.changed = true;
        out.audition = Some(Audition::Chord(c));
    }
}

/// A palette button: the numeral over the name, topped with the chord's function colour.
fn chord_button(ui: &mut Ui, c: &Chord, p: &Progression) -> egui::Response {
    let text = format!("{}\n{}", numeral(c, p.key), chord_name(c, p.key));
    let response = ui.add(egui::Button::new(text).min_size(vec2(46.0, 40.0)));
    let r = response.rect;
    ui.painter().hline(r.left() + 4.0..=r.right() - 4.0, r.top() + 1.5, Stroke::new(3.0, function_colour(c, p.key)));
    response
}

fn quality_label(q: Quality) -> &'static str {
    match q {
        Quality::Major => "Major",
        Quality::Minor => "Minor",
        Quality::Diminished => "Diminished",
        Quality::Augmented => "Augmented",
    }
}

fn extension_label(e: Extension) -> &'static str {
    match e {
        Extension::Triad => "Plain",
        Extension::Seven => "7",
        Extension::Maj7 => "maj7",
        Extension::Sus2 => "sus2",
        Extension::Sus4 => "sus4",
        Extension::Add9 => "add9",
        Extension::Nine => "9",
        Extension::Eleven => "11",
        Extension::Thirteen => "13",
    }
}

pub(super) fn chord(ui: &mut Ui, p: &mut Progression, state: &mut ChordUi, out: &mut ChordOutcome) {
    let Some(i) = state.selected.filter(|&i| i < p.chords.len()) else { return };
    let key = p.key;
    let bar = p.meter.0.max(1) as u16;
    let mut c = p.chords[i];
    egui::Grid::new("chord editor").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        let label = ui.label("Quality");
        egui::ComboBox::from_id_salt("chord quality")
            .selected_text(quality_label(c.quality))
            .show_ui(ui, |ui| {
                for q in Quality::ALL {
                    ui.selectable_value(&mut c.quality, q, quality_label(q));
                }
            })
            .response
            .labelled_by(label.id);
        ui.end_row();

        let label = ui.label("Colour").on_hover_text("What is added to the chord, or swapped into it");
        egui::ComboBox::from_id_salt("chord colour")
            .selected_text(extension_label(c.ext))
            .show_ui(ui, |ui| {
                for e in Extension::ALL {
                    ui.selectable_value(&mut c.ext, e, extension_label(e));
                }
            })
            .response
            .labelled_by(label.id);
        ui.end_row();

        let bass_label =
            ui.label("Bass note").on_hover_text("The lowest note: the root, another of the chord's notes, or any note");
        ui.horizontal_wrapped(|ui| {
            if ui.selectable_label(c.bass.is_none(), "Root").clicked() {
                c.bass = None;
            }
            let tones: Vec<u8> = intervals(&c).iter().skip(1).map(|s| (c.root + s) % 12).collect();
            for pc in tones.iter().copied().filter(|&pc| pc != c.root % 12) {
                if ui.selectable_label(c.bass == Some(pc), note_name(pc, key)).clicked() {
                    c.bass = Some(pc);
                }
            }
            let other = c.bass.filter(|b| !tones.contains(b));
            let mut pick = other;
            egui::ComboBox::from_id_salt("chord other bass")
                .width(64.0)
                .selected_text(other.map_or("Other…", |b| note_name(b, key)))
                .show_ui(ui, |ui| {
                    for pc in 0..12 {
                        ui.selectable_value(&mut pick, Some(pc), note_name(pc, key));
                    }
                })
                .response
                .labelled_by(bass_label.id);
            if pick != other {
                c.bass = pick.filter(|&b| b != c.root % 12);
            }
        });
        ui.end_row();

        ui.label("Length");
        ui.horizontal_wrapped(|ui| {
            for beats in [1, 2, bar, bar * 2] {
                if ui.selectable_label(c.beats == beats, length_words(beats, bar)).clicked() {
                    c.beats = beats;
                }
            }
            ui.add(egui::DragValue::new(&mut c.beats).range(1..=MAX_BEATS).suffix(" beats").speed(0.1));
        });
        ui.end_row();

        let label = ui.label("Name").on_hover_text("Type a chord such as F#m7, Bb, Dsus4 or C/E, then press Enter");
        if state.named != Some((i, c)) && !ui.memory(|m| m.has_focus(ui.scope_id().with("chord name"))) {
            state.name = chord_name(&c, key);
            state.named = Some((i, c));
        }
        let field = ui
            .add(egui::TextEdit::singleline(&mut state.name).id(ui.scope_id().with("chord name")).desired_width(120.0))
            .labelled_by(label.id);
        if field.changed() {
            state.name_error = false;
        }
        if field.lost_focus() && ui.input(|inp| inp.key_pressed(egui::Key::Enter)) {
            match parse_chord(&state.name) {
                Some(typed) => {
                    c = Chord { beats: c.beats, ..typed };
                    state.name_error = false;
                }
                None => state.name_error = true,
            }
        }
        ui.end_row();
        if state.name_error {
            ui.label("");
            ui.label(RichText::new("Not a chord name I know").color(theme::ERROR));
            ui.end_row();
        }
    });
    let c = c.clamp();
    if c != p.chords[i] {
        let was = p.chords[i];
        p.chords[i] = c;
        state.named = None;
        out.changed = true;
        if (c.root, c.quality, c.ext, c.bass) != (was.root, was.quality, was.ext, was.bass) {
            out.audition = Some(Audition::Chord(c));
        }
    }
}
