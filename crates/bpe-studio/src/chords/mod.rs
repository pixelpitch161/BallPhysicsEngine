//! The chord editor shown under a card with a Chords setting: a key bar, a lane of chord blocks
//! sized by their beats, a palette of the key's chords, the selected chord's settings and a list
//! of presets to start from.

mod editor;
mod lane;
mod presets;

use crate::theme;
use bpe_core::{Chord, Mode, MusicKey, Progression};
use bpe_mix::{Function, function, note_name};
use eframe::egui::{self, Color32, Rect, RichText, Ui};

/// What one progression's editor remembers between frames.
#[derive(Default)]
pub struct ChordUi {
    /// The chord block picked in the lane.
    pub selected: Option<usize>,
    pub presets_open: bool,
    /// The presets' mood filter; `None` shows them all.
    pub mood: Option<&'static str>,
    /// The chord name field's text, and whether the last name typed wasn't a chord.
    name: String,
    name_error: bool,
    /// The chord the name field was last filled from, so it refills when that changes.
    named: Option<(usize, Chord)>,
    drag: Option<lane::Drag>,
    /// Where each block was drawn last frame, for tests.
    pub(crate) blocks: Vec<Rect>,
}

/// What a progression's editor did this frame.
#[derive(Default)]
pub struct ChordOutcome {
    pub changed: bool,
    pub audition: Option<Audition>,
}

/// Something to hear now.
#[derive(Clone, Debug, PartialEq)]
pub enum Audition {
    /// One chord, after it was clicked or added.
    Chord(Chord),
    /// The whole progression from its start.
    All,
}

/// Chords to hear through a sound module's instrument.
#[derive(Clone, Debug)]
pub struct ChordClip {
    pub chords: Vec<Chord>,
    pub instrument: bpe_mix::InstrumentChoice,
    pub seconds_per_beat: f64,
}

impl ChordClip {
    /// The clip's samples: interleaved stereo at the mixer's rate.
    pub fn render(&self) -> Vec<f32> {
        bpe_mix::audition(&self.chords, &self.instrument, self.seconds_per_beat)
    }
}

/// Draws the editor for `p`, changing it in place.
pub fn show(ui: &mut Ui, p: &mut Progression, state: &mut ChordUi) -> ChordOutcome {
    let mut out = ChordOutcome::default();
    key_bar(ui, p, state, &mut out);
    if state.presets_open {
        presets::show(ui, p, state, &mut out);
    }
    ui.add_space(4.0);
    lane::show(ui, p, state, &mut out);
    ui.add_space(4.0);
    editor::palette(ui, p, state, &mut out);
    if state.selected.is_some_and(|i| i < p.chords.len()) {
        ui.add_space(4.0);
        editor::chord(ui, p, state, &mut out);
    }
    out
}

/// Whether a chord lane has the keyboard, so Delete removes its chord and not a thing.
pub fn lane_focused(ctx: &egui::Context) -> bool {
    lane::focused(ctx)
}

/// The colour of what a chord does in the key.
pub fn function_colour(c: &Chord, key: MusicKey) -> Color32 {
    match function(c, key) {
        Function::Home => theme::HOME,
        Function::Away => theme::AWAY,
        Function::Pull => theme::PULL,
        Function::Borrowed => theme::BORROWED,
    }
}

/// How long `beats` is, in bars when it is whole bars.
pub fn length_words(beats: u16, bar: u16) -> String {
    let bar = bar.max(1);
    match beats {
        b if b % bar == 0 && b / bar == 1 => "1 bar".into(),
        b if b % bar == 0 => format!("{} bars", b / bar),
        1 => "1 beat".into(),
        b => format!("{b} beats"),
    }
}

const METERS: [(u8, u8); 3] = [(3, 4), (4, 4), (6, 8)];

fn key_bar(ui: &mut Ui, p: &mut Progression, state: &mut ChordUi, out: &mut ChordOutcome) {
    ui.horizontal_wrapped(|ui| {
        let key_label = ui.label("Key");
        let mut root = p.key.root % 12;
        let spell = |pc: u8, mode: Mode| note_name(pc, MusicKey { root: pc, mode });
        egui::ComboBox::from_id_salt("chord key root")
            .width(52.0)
            .selected_text(spell(root, p.key.mode))
            .show_ui(ui, |ui| {
                for pc in 0..12 {
                    ui.selectable_value(&mut root, pc, spell(pc, p.key.mode));
                }
            })
            .response
            .labelled_by(key_label.id);
        if root != p.key.root % 12 {
            p.transpose_to(root);
            out.changed = true;
        }
        let mut mode = p.key.mode;
        let mode_label = ui.label("Mode");
        egui::ComboBox::from_id_salt("chord key mode")
            .width(92.0)
            .selected_text(mode.label())
            .show_ui(ui, |ui| {
                for m in Mode::ALL {
                    ui.selectable_value(&mut mode, m, m.label());
                }
            })
            .response
            .labelled_by(mode_label.id);
        if mode != p.key.mode {
            p.key.mode = mode;
            out.changed = true;
        }
        let mut meter = p.meter;
        let meter_label = ui.label("Meter");
        egui::ComboBox::from_id_salt("chord meter")
            .width(52.0)
            .selected_text(format!("{}/{}", meter.0, meter.1))
            .show_ui(ui, |ui| {
                for m in METERS {
                    ui.selectable_value(&mut meter, m, format!("{}/{}", m.0, m.1));
                }
            })
            .response
            .labelled_by(meter_label.id);
        if meter != p.meter {
            p.meter = meter;
            out.changed = true;
        }
        if ui.button("Play").on_hover_text("Hear the chords from the start").clicked() {
            out.audition = Some(Audition::All);
        }
        let presets =
            ui.selectable_label(state.presets_open, "Presets").on_hover_text("Start from a known progression");
        if presets.clicked() {
            state.presets_open = !state.presets_open;
        }
    });
    if p.chords.is_empty() {
        return;
    }
    let total = p.total_beats();
    let bar = p.meter.0.max(1) as u32;
    let words = if total.is_multiple_of(bar) {
        format!("{} bars, then it repeats", total / bar)
    } else {
        format!("{total} beats, then it repeats")
    };
    ui.label(RichText::new(words).small().color(theme::MUTED));
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Extension, Quality};
    use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2, accesskit::Role, vec2};
    use egui_kittest::{Harness, kittest::Queryable};

    struct State {
        p: Progression,
        ui: ChordUi,
        heard: Vec<Audition>,
        /// Draws a button above the editor, to click away from the lane.
        elsewhere: bool,
    }

    fn harness(p: Progression) -> Harness<'static, State> {
        let state = State { p, ui: ChordUi::default(), heard: Vec::new(), elsewhere: false };
        let mut h = Harness::builder()
            .with_size(vec2(520.0, 700.0))
            .with_step_dt(1.0 / 60.0)
            .with_max_steps(120)
            .allow_missing_glyphs()
            .with_context_setup(theme::apply)
            .build_ui_state(
                |ui, s: &mut State| {
                    if ui.button("Elsewhere").clicked() {
                        s.elsewhere = true;
                    }
                    let out = show(ui, &mut s.p, &mut s.ui);
                    s.heard.extend(out.audition);
                },
                state,
            );
        h.run();
        h
    }

    fn click(h: &mut Harness<'_, State>, pos: Pos2, modifiers: Modifiers) {
        h.hover_at(pos);
        h.step();
        for pressed in [true, false] {
            h.event(Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers });
        }
        h.run();
    }

    fn drag(h: &mut Harness<'_, State>, from: Pos2, to: Pos2, modifiers: Modifiers) {
        h.hover_at(from);
        h.step();
        h.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers });
        h.step();
        for i in 1..=8 {
            h.hover_at(from + (to - from) * (i as f32 / 8.0));
            h.step();
        }
        h.event(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers });
        h.step();
        h.run();
    }

    fn block(h: &Harness<'_, State>, i: usize) -> Rect {
        h.state().ui.blocks[i]
    }

    fn roots(p: &Progression) -> Vec<u8> {
        p.chords.iter().map(|c| c.root).collect()
    }

    #[test]
    fn clicking_a_palette_chord_adds_it_after_the_selection() {
        let mut h = harness(Progression::starter());
        let at = block(&h, 1).center();
        click(&mut h, at, Modifiers::NONE);
        assert_eq!(h.state().ui.selected, Some(1));
        h.get_by_label("IV\nF").click();
        h.run();
        let s = h.state();
        assert_eq!(roots(&s.p), [0, 2, 5, 9, 0, 9, 2]);
        assert_eq!(s.p.chords[2].beats, 4, "a new chord lasts a bar");
        assert_eq!(s.ui.selected, Some(2), "the new chord is picked");
        assert_eq!(s.heard.last(), Some(&Audition::Chord(s.p.chords[2])));
    }

    #[test]
    fn dragging_a_block_edge_changes_its_length_in_beats() {
        let mut h = harness(Progression::starter());
        let r = block(&h, 0);
        let per_beat = r.width() / 8.0;
        let edge = Pos2::new(r.right() - 1.0, r.center().y);
        drag(&mut h, edge, edge + vec2(per_beat * 3.0, 0.0), Modifiers::NONE);
        assert_eq!(h.state().p.chords[0].beats, 11);
        // Shift snaps to whole bars.
        let r = block(&h, 0);
        let per_beat = r.width() / 11.0;
        let edge = Pos2::new(r.right() - 1.0, r.center().y);
        drag(&mut h, edge, edge + vec2(per_beat * 0.6, 0.0), Modifiers::SHIFT);
        assert_eq!(h.state().p.chords[0].beats, 12);
        assert_eq!(h.state().p.chords.len(), 6, "resizing adds and removes nothing");
    }

    #[test]
    fn dragging_a_block_moves_it() {
        let mut h = harness(Progression::starter());
        let from = block(&h, 0).center();
        let to = Pos2::new(block(&h, 2).right() - 4.0, from.y);
        drag(&mut h, from, to, Modifiers::NONE);
        assert_eq!(roots(&h.state().p), [2, 9, 0, 0, 9, 2]);
    }

    #[test]
    fn delete_removes_the_selected_chord_only_when_the_lane_has_focus() {
        let mut h = harness(Progression::starter());
        let at = block(&h, 2).center();
        click(&mut h, at, Modifiers::NONE);
        assert!(lane_focused(&h.ctx));
        h.key_press(Key::Delete);
        h.run();
        assert_eq!(roots(&h.state().p), [0, 2, 0, 9, 2]);
        // Clicking away takes the keyboard from the lane.
        let at = block(&h, 1).center();
        click(&mut h, at, Modifiers::NONE);
        h.get_by_label("Elsewhere").click();
        h.run();
        assert!(h.state().elsewhere);
        assert!(!lane_focused(&h.ctx));
        h.key_press(Key::Delete);
        h.run();
        assert_eq!(h.state().p.chords.len(), 5);
    }

    #[test]
    fn changing_the_key_transposes_every_chord() {
        let mut p = Progression::starter();
        p.chords.push(Chord { bass: Some(4), ..Chord::new(0, Quality::Major, Extension::Triad, 4) });
        let mut h = harness(p);
        h.get_by_role_and_label(Role::ComboBox, "Key").click();
        h.run();
        h.get_by_label("D").click();
        h.run();
        let p = &h.state().p;
        assert_eq!(p.key.root, 2);
        assert_eq!(roots(p), [2, 4, 11, 2, 11, 4, 2]);
        assert_eq!(p.chords[6].bass, Some(6));
    }

    #[test]
    fn a_preset_replaces_the_chords_in_the_current_key() {
        let mut p = Progression::starter();
        p.transpose_to(2);
        let mut h = harness(p);
        h.get_by_label("Presets").click();
        h.run();
        h.get_by_label("dramatic").click();
        h.run();
        h.get_by_label("Andalusian").click();
        h.run();
        let s = h.state();
        assert_eq!(s.p.key, MusicKey { root: 2, mode: Mode::Minor });
        // D minor: Dm, C, Bb, A.
        assert_eq!(roots(&s.p), [2, 0, 10, 9]);
        assert!(!s.ui.presets_open, "picking one closes the list");
    }

    #[test]
    fn the_mood_filter_narrows_the_presets() {
        let mut h = harness(Progression::starter());
        h.get_by_label("Presets").click();
        h.run();
        assert!(h.query_by_label("Pop anthem").is_some());
        h.get_by_label("dark").click();
        h.run();
        assert!(h.query_by_label("Pop anthem").is_none());
        assert!(h.query_by_label("Night drive").is_some());
    }

    fn type_name(h: &mut Harness<'_, State>, text: &str) {
        h.get_by_role_and_label(Role::TextInput, "Name").click();
        h.run();
        for _ in 0..12 {
            h.key_press(Key::Backspace);
        }
        h.event(Event::Text(text.into()));
        h.key_press(Key::Enter);
        h.run();
    }

    #[test]
    fn typing_a_chord_name_sets_the_chord() {
        let mut h = harness(Progression::starter());
        let at = block(&h, 0).center();
        click(&mut h, at, Modifiers::NONE);
        type_name(&mut h, "F#m7/E");
        let c = h.state().p.chords[0];
        assert_eq!((c.root, c.quality, c.ext, c.bass, c.beats), (6, Quality::Minor, Extension::Seven, Some(4), 8));
        assert!(h.query_by_label("Not a chord name I know").is_none());
    }

    #[test]
    fn a_bad_name_shows_a_message_and_changes_nothing() {
        let mut h = harness(Progression::starter());
        let at = block(&h, 0).center();
        click(&mut h, at, Modifiers::NONE);
        type_name(&mut h, "Hello");
        assert_eq!(h.state().p, Progression::starter());
        assert!(h.query_by_label("Not a chord name I know").is_some());
    }

    #[test]
    fn an_empty_progression_shows_the_empty_state() {
        let mut h = harness(Progression { chords: Vec::new(), ..Progression::starter() });
        assert!(h.query_by_label("No chords yet. Add one from the palette below.").is_some());
        h.get_by_label("I\nC").click();
        h.run();
        assert_eq!(roots(&h.state().p), [0]);
    }

    #[test]
    fn the_chord_editor_changes_quality_and_bass() {
        let mut h = harness(Progression::starter());
        let at = block(&h, 2).center();
        click(&mut h, at, Modifiers::NONE);
        h.get_by_role_and_label(Role::ComboBox, "Quality").click();
        h.run();
        h.get_by_label("Major").click();
        h.run();
        assert_eq!(h.state().p.chords[2].quality, Quality::Major);
        // A major: A, C#, E. The E button puts E in the bass.
        h.get_by_label("E").click();
        h.run();
        assert_eq!(h.state().p.chords[2].bass, Some(4));
        h.get_by_label("1 bar").click();
        h.run();
        assert_eq!(h.state().p.chords[2].beats, 4);
    }

    #[test]
    fn play_asks_to_hear_everything() {
        let mut h = harness(Progression::starter());
        h.get_by_label("Play").click();
        h.run();
        assert_eq!(h.state().heard, [Audition::All]);
    }
}
