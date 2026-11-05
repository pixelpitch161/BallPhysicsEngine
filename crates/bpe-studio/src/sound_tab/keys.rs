//! The keyboard under the slot grid (see [`super::notes`]), and where its keys sit so the grid
//! lines up with them. Notes are named as FL Studio names them: C5 is middle C (MIDI 60).

use crate::style;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Ui, Vec2};

/// The keyboard's height.
pub const KEYS_H: f32 = 80.0;
const BLACK_H: f32 = 48.0;

/// Note names in an octave.
const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
/// The white keys' notes in an octave.
const WHITE: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];

/// The name of MIDI note `note`, FL Studio style: 60 is "C5".
pub fn note_name(note: u8) -> String {
    format!("{}{}", NAMES[note as usize % 12], note / 12)
}

/// Where the keys sit: `whites` white keys of `kw` from `left`, from C of octave `low`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyGeom {
    pub left: f32,
    pub kw: f32,
    pub low: u8,
    pub octaves: u8,
}

impl KeyGeom {
    /// The keys across `width` from `left`.
    pub fn new(left: f32, width: f32, low: u8, octaves: u8) -> Self {
        let whites = 7 * octaves.max(1) as usize;
        KeyGeom { left, kw: width / whites as f32, low, octaves: octaves.max(1) }
    }

    /// The lowest and highest notes shown.
    pub fn range(&self) -> (u8, u8) {
        (12 * self.low, (12 * (self.low as u16 + self.octaves as u16) - 1).min(127) as u8)
    }

    /// The middle of `note`'s key across (a black key sits on the line between two whites).
    /// `None` when the note is not on the keyboard.
    pub fn x_of(&self, note: u8) -> Option<f32> {
        let (lo, hi) = self.range();
        if note < lo || note > hi {
            return None;
        }
        let rel = (note - lo) as usize;
        let (octave, pc) = (rel / 12, (rel % 12) as u8);
        let base = self.left + (7 * octave) as f32 * self.kw;
        Some(match WHITE.iter().position(|&w| w == pc) {
            Some(w) => base + (w as f32 + 0.5) * self.kw,
            None => {
                let below = WHITE.iter().position(|&w| w == pc - 1).unwrap_or(0);
                base + (below + 1) as f32 * self.kw
            }
        })
    }

    /// Whether `note` is a black key.
    pub fn is_black(note: u8) -> bool {
        !WHITE.contains(&(note % 12))
    }
}

/// Whether note `note` is in `mask` (bit n for MIDI note n).
pub fn in_mask(mask: u128, note: u8) -> bool {
    note < 128 && mask >> note & 1 == 1
}

/// The keyboard drawn in `rect` with its keys at `geom`; keys `in_key` says are in the
/// shape's key get a faint dot. `lit` (the picked row's notes) are in the shape's colour, and
/// the notes in `sounding` (bit n for MIDI note n) light up white while they play.
pub fn keyboard_at(
    ui: &mut Ui,
    rect: Rect,
    geom: KeyGeom,
    lit: &[u8],
    sounding: u128,
    colour: Color32,
    in_key: &dyn Fn(u8) -> bool,
) -> Option<u8> {
    let (low, octaves, kw) = (geom.low, geom.octaves, geom.kw);
    let whites = 7 * octaves as usize;
    let first = 12 * low;
    let mut clicked = None;
    let mut keys: Vec<(u8, Rect, bool)> = Vec::new();
    for w in 0..whites {
        let note = first + 12 * (w / 7) as u8 + WHITE[w % 7];
        let r =
            Rect::from_min_size(Pos2::new(geom.left + w as f32 * kw, rect.top()), Vec2::new(kw - 1.0, rect.height()));
        keys.push((note, r, false));
    }
    let black_h = rect.height() * BLACK_H / KEYS_H;
    for w in 0..whites {
        let n = WHITE[w % 7];
        if matches!(n, 0 | 2 | 5 | 7 | 9) {
            let note = first + 12 * (w / 7) as u8 + n + 1;
            let x = geom.left + (w + 1) as f32 * kw - kw * 0.3;
            keys.push((note, Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(kw * 0.6, black_h)), true));
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
        let playing = in_mask(sounding, note);
        if ui.is_rect_visible(r) {
            let painter = ui.painter();
            let fill = match (playing, on, black) {
                (true, _, _) => style::TEXT_STRONG,
                (false, true, _) => colour,
                (false, false, true) => style::STAGE,
                (false, false, false) => style::SLIDER_FILL,
            };
            painter.rect_filled(r, 2.0, fill);
            if playing {
                painter.rect_stroke(r, 2.0, Stroke::new(2.0, colour), egui::StrokeKind::Inside);
            }
            if black {
                painter.rect_stroke(r, 2.0, Stroke::new(1.0, style::GROUND), egui::StrokeKind::Inside);
            } else if note % 12 == 0 && !playing {
                painter.text(
                    Pos2::new(r.center().x, r.bottom() - 4.0),
                    egui::Align2::CENTER_BOTTOM,
                    &name,
                    style::mono(10.0),
                    style::ON_LIGHT,
                );
            }
            if in_key(note) && !on && !playing {
                let y = if black { r.bottom() - 6.0 } else { r.bottom() - 20.0 };
                let dot = if black { style::LINE } else { style::FAINT };
                painter.circle_filled(Pos2::new(r.center().x, y), 2.0, dot);
            }
            if response.hovered() {
                painter.rect_stroke(r, 2.0, Stroke::new(1.0, style::TEXT_STRONG), egui::StrokeKind::Inside);
            }
        }
    }
    // The C labels, for screen readers and tests too.
    for o in 0..octaves {
        let note = first + 12 * o;
        let at =
            Rect::from_min_size(Pos2::new(geom.left + 7.0 * o as f32 * kw, rect.bottom() - 16.0), Vec2::new(kw, 14.0));
        let label = ui.interact(at, ui.scope_id().with(("c label", note)), Sense::hover());
        let name = note_name(note);
        label.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &name));
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_names_follow_fl_studio() {
        assert_eq!(note_name(60), "C5");
        assert_eq!(note_name(61), "C#5");
        assert_eq!(note_name(71), "B5");
    }

    #[test]
    fn key_columns_line_up() {
        let g = KeyGeom::new(0.0, 210.0, 4, 3);
        assert_eq!(g.range(), (48, 83));
        assert_eq!(g.x_of(48), Some(5.0), "C4 in the middle of the first white key");
        assert_eq!(g.x_of(49), Some(10.0), "C#4 on the line after it");
        assert_eq!(g.x_of(47), None);
    }
}
