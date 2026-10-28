//! The Notes lane: every note the soundtrack plays, as a small piano roll. Each voice of the
//! Sound tab's voices table has its own colour; the main instrument is the accent colour.

use super::lanes::{Lane, LaneContext, lane_message};
use crate::theme;
use bpe_scene::{Instrument, Scene};
use eframe::egui::{Color32, Pos2, Rect, Ui, vec2};

/// Colours for voices table rows, in order.
const VOICE_COLORS: [Color32; 6] = [
    Color32::from_rgb(0xf2, 0xc4, 0x6d),
    Color32::from_rgb(0x6d, 0xd3, 0xf2),
    Color32::from_rgb(0xf2, 0x7d, 0xb4),
    Color32::from_rgb(0x8d, 0xe0, 0x8a),
    Color32::from_rgb(0xff, 0x9a, 0x6a),
    Color32::from_rgb(0xc7, 0xa6, 0xff),
];

#[derive(Default)]
pub struct NotesLane {}

/// Whether the scene plays anything but the classic one-instrument chords, so the notes are
/// worth a lane of their own.
fn plays_notes(scene: &Scene) -> bool {
    let sound = &scene.sound;
    let classic = matches!(sound.instrument, Instrument::Pluck { .. } | Instrument::Sample { .. });
    sound.enabled && (!classic || !sound.voices.is_empty() || scene.music.melody.is_some())
}

impl Lane for NotesLane {
    fn id(&self) -> &'static str {
        "notes"
    }

    fn title(&self, ctx: &LaneContext) -> String {
        match ctx.hits.and_then(|h| h.music.as_deref()) {
            Some(music) => format!("Notes  {}", music.notes.len()),
            None => "Notes".into(),
        }
    }

    fn height(&self, _ctx: &LaneContext) -> f32 {
        56.0
    }

    fn visible(&self, ctx: &LaneContext) -> bool {
        plays_notes(ctx.scene)
    }

    fn draw(&mut self, ui: &mut Ui, rect: Rect, ctx: &LaneContext) {
        let Some(music) = ctx.hits.and_then(|h| h.music.as_deref()) else {
            lane_message(ui, rect, "Working out the notes…");
            return;
        };
        let (scale, lane) = (ctx.scale, rect);
        let painter = ui.painter_at(lane);
        painter.rect_filled(lane, 3.0, theme::CARD);
        if music.notes.is_empty() {
            lane_message(ui, lane, "No notes play yet.");
            return;
        }
        let low = music.notes.iter().map(|n| n.key).min().unwrap_or(60).saturating_sub(1);
        let high = music.notes.iter().map(|n| n.key).max().unwrap_or(72).saturating_add(1);
        let row = (lane.height() - 4.0) / (high - low).max(1) as f32;
        let main = ctx.scene.sound.voices.len() as u16;
        // Very short marks, at least two points wide, so dense passages still read as notes.
        let width = (0.08 * scale.width / scale.span).clamp(2.0, 10.0);
        let (from, to) = (scale.time(lane.left()) as f64, scale.time(lane.right()) as f64);
        let first = music.notes.partition_point(|n| n.time < from - 0.1);
        for note in music.notes[first..].iter().take_while(|n| n.time <= to) {
            let x = scale.x(note.time as f32);
            let y = lane.bottom() - 2.0 - (note.key - low) as f32 * row;
            let color =
                if note.voice == main { theme::ACCENT } else { VOICE_COLORS[note.voice as usize % VOICE_COLORS.len()] };
            let alpha = 0.45 + 0.55 * note.velocity.clamp(0.0, 1.0);
            let mark = Rect::from_min_size(Pos2::new(x, y - row.max(2.0)), vec2(width, row.max(2.0)));
            painter.rect_filled(mark, 1.0, color.gamma_multiply(alpha));
        }
    }
}
