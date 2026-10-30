//! A one-octave keyboard strip to hear an instrument note by note.

use crate::theme;
use eframe::egui::{self, Color32, Rect, Sense, Stroke, Ui, vec2};

/// The octaves the strip can show; C4 is middle C.
const OCTAVES: std::ops::RangeInclusive<i8> = 1..=7;
const WHITE_W: f32 = 26.0;
const WHITE_H: f32 = 56.0;
const BLACK_W: f32 = 16.0;
const BLACK_H: f32 = 34.0;

/// Semitones above C of the white keys, and of the black keys with the white key they sit after.
const WHITES: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
const BLACKS: [(u8, usize); 5] = [(1, 0), (3, 1), (6, 3), (8, 4), (10, 5)];

/// Note names by semitone above C.
const NAMES: [&str; 12] = ["C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B"];

/// The octave buttons and the keys from C to B of `octave`. A clicked key comes back as its
/// MIDI note (C4 is 60).
pub fn show(ui: &mut Ui, octave: &mut i8) -> Option<u8> {
    let mut played = None;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(*octave > *OCTAVES.start(), egui::Button::new("Oct −"))
            .on_hover_text("An octave down")
            .clicked()
        {
            *octave -= 1;
        }
        if ui.add_enabled(*octave < *OCTAVES.end(), egui::Button::new("Oct +")).on_hover_text("An octave up").clicked()
        {
            *octave += 1;
        }
    });
    *octave = (*octave).clamp(*OCTAVES.start(), *OCTAVES.end());
    let (strip, _) = ui.allocate_exact_size(vec2(WHITE_W * 7.0, WHITE_H), Sense::hover());
    let base = 12 * (*octave as i32 + 1);
    let mut key = |ui: &mut Ui, rect: Rect, semitone: u8, black: bool| {
        let note = (base + semitone as i32).clamp(0, 127) as u8;
        let name = format!("{}{}", NAMES[semitone as usize], octave);
        let response = ui.interact(rect, ui.scope_id().with(("key", semitone)), Sense::click());
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name.clone()));
        let fill = match (black, response.is_pointer_button_down_on(), response.hovered()) {
            (_, true, _) => theme::ACCENT,
            (true, _, true) => theme::HOVER,
            (true, _, false) => theme::BACKGROUND,
            (false, _, true) => Color32::from_gray(220),
            (false, _, false) => theme::TEXT,
        };
        ui.painter().rect(rect, 2.0, fill, Stroke::new(1.0, theme::LINE), egui::StrokeKind::Inside);
        if !black && semitone == 0 {
            let at = egui::pos2(rect.center().x, rect.bottom() - 9.0);
            ui.painter().text(at, egui::Align2::CENTER_CENTER, &name, egui::FontId::proportional(10.0), theme::MUTED);
        }
        if response.on_hover_text(name).clicked() {
            played = Some(note);
        }
    };
    // White keys first, so the black ones drawn over them also take the clicks there.
    for (i, &semitone) in WHITES.iter().enumerate() {
        let rect = Rect::from_min_size(strip.min + vec2(i as f32 * WHITE_W, 0.0), vec2(WHITE_W, WHITE_H));
        key(ui, rect, semitone, false);
    }
    for &(semitone, after) in &BLACKS {
        let x = (after + 1) as f32 * WHITE_W - BLACK_W / 2.0;
        key(ui, Rect::from_min_size(strip.min + vec2(x, 0.0), vec2(BLACK_W, BLACK_H)), semitone, true);
    }
    played
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::accesskit::Role;
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };

    struct State {
        octave: i8,
        played: Vec<u8>,
    }

    fn harness() -> Harness<'static, State> {
        let mut h = Harness::builder()
            .with_size(vec2(320.0, 160.0))
            .with_max_steps(60)
            .allow_missing_glyphs()
            .with_context_setup(theme::apply)
            .build_ui_state(
                |ui, s: &mut State| {
                    s.played.extend(show(ui, &mut s.octave));
                },
                State { octave: 4, played: Vec::new() },
            );
        h.run();
        h
    }

    #[test]
    fn clicking_a_key_plays_that_note() {
        let mut h = harness();
        h.get_by_role_and_label(Role::Button, "C4").click();
        h.run();
        // The black key sits over two white ones and still gets its own click.
        h.get_by_role_and_label(Role::Button, "C♯4").click();
        h.run();
        h.get_by_label("Oct +").click();
        h.run();
        h.get_by_role_and_label(Role::Button, "C5").click();
        h.run();
        h.get_by_role_and_label(Role::Button, "B5").click();
        h.run();
        assert_eq!(h.state().played, [60, 61, 72, 83]);
    }

    #[test]
    fn octave_stops_at_the_ends() {
        let mut h = harness();
        for _ in 0..10 {
            h.get_by_label("Oct −").click();
            h.run();
        }
        assert_eq!(h.state().octave, 1);
        assert!(h.get_by_label("Oct −").accesskit_node().is_disabled());
        for _ in 0..10 {
            h.get_by_label("Oct +").click();
            h.run();
        }
        assert_eq!(h.state().octave, 7);
        assert!(h.get_by_label("Oct +").accesskit_node().is_disabled());
        h.get_by_role_and_label(Role::Button, "B7").click();
        h.run();
        assert_eq!(h.state().played, [107]);
    }
}
