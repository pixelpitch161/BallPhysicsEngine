//! The Chords track: the progression the selection plays, laid out on the video's time, in the
//! colours of what each chord does. Read-only; the chords are edited in the inspector.

use super::{LABEL_WIDTH, LANE_HEIGHT, Ruler};
use crate::app::{Studio, clock};
use crate::chords::function_colour;
use crate::theme;
use bpe_core::{Chord, MusicKey, ProgressionRef, SettingValue, SlotId, ThingId};
use bpe_mix::{ChordClock, chord_name};
use eframe::egui::{self, Align2, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui};

/// The chords from the start of the video to `duration`, laid on `beats`: (start, end, chord).
pub fn chord_spans(p: &bpe_core::Progression, beats: &[f64], duration: f64) -> Vec<(f64, f64, Chord)> {
    ChordClock::new(p, beats).spans(duration)
}

/// The progression the selection's sound plays: its Sound module's own pick, else Scene
/// defaults'.
fn shown(studio: &Studio) -> Option<bpe_core::ProgressionId> {
    let chords_of = |id: ThingId, slot: Option<SlotId>| {
        let thing = studio.resolved.things.get(&id)?;
        let params = match slot {
            Some(slot) => &thing.slots.get(&slot)?.params,
            None => &thing.params,
        };
        ProgressionRef::from_value(params.get("chords")?)?.0
    };
    let own = studio.selection.primary.and_then(|id| chords_of(id, Some(SlotId::Sound)));
    own.or_else(|| chords_of(ThingId::ROOT, None)).filter(|p| studio.scene.library.progressions.contains_key(p))
}

pub(super) fn show(ui: &mut Ui, studio: &Studio, ruler: &Ruler) {
    let Some(pid) = shown(studio) else { return };
    let p = &studio.scene.library.progressions[&pid];
    let beats: Vec<f64> = studio.panels.sound.beats.iter().map(|&b| b as f64).collect();
    let spans = chord_spans(p, &beats, ruler.duration);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), LANE_HEIGHT), Sense::hover());
    let label_rect = Rect::from_min_size(rect.min, egui::vec2(LABEL_WIDTH, LANE_HEIGHT));
    let label = format!("Chords ({})", p.name);
    ui.put(label_rect, egui::Label::new(RichText::new(label).small()).truncate().selectable(false));
    let strip = Rect::from_min_max(
        Pos2::new(ruler.track.left(), rect.top() + 1.0),
        Pos2::new(ruler.track.right(), rect.bottom() - 1.0),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(strip, 3.0, theme::CARD);
    for (i, (start, end, c)) in spans.iter().enumerate() {
        let block = Rect::from_min_max(
            Pos2::new(ruler.x_of(*start) + 0.5, strip.top() + 1.0),
            Pos2::new(ruler.x_of(*end) - 0.5, strip.bottom() - 1.0),
        );
        draw_block(ui, block, c, p.key, ui.scope_id().with(("chord span", i)), (*start, *end));
    }
    let x = ruler.x_of(studio.time);
    painter.line_segment([Pos2::new(x, strip.top()), Pos2::new(x, strip.bottom())], Stroke::new(1.0, theme::ACCENT));
}

fn draw_block(ui: &mut Ui, block: Rect, c: &Chord, key: MusicKey, id: egui::Id, (start, end): (f64, f64)) {
    let name = chord_name(c, key);
    let response = ui.interact(block, id, Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, name.clone()));
    let painter = ui.painter_at(block);
    painter.rect_filled(block, 2.0, function_colour(c, key).gamma_multiply(0.75));
    let font = FontId::proportional(11.0);
    let wide = ui.fonts_mut(|f| f.layout_no_wrap(name.clone(), font.clone(), theme::BACKGROUND).size().x) + 6.0;
    if block.width() >= wide {
        painter.text(block.left_center() + egui::vec2(3.0, 0.0), Align2::LEFT_CENTER, &name, font, theme::BACKGROUND);
    }
    response.on_hover_text(format!("{name} · {} to {}", clock(start), clock(end)));
}

#[cfg(test)]
mod tests {
    use super::super::show as timeline;
    use crate::app::Studio;
    use eframe::egui;
    use egui_kittest::{Harness, kittest::Queryable};

    #[test]
    fn the_timeline_shows_a_chords_track_with_names() {
        let studio = Studio::headless();
        let h = Harness::builder()
            .with_size(egui::vec2(1400.0, 300.0))
            .allow_missing_glyphs()
            .with_context_setup(crate::theme::apply)
            .build_ui_state(|ui, s: &mut Studio| timeline(ui, s), studio);
        assert!(h.query_by_label("Chords (Starter)").is_some());
        assert!(h.query_all_by_label("C").next().is_some(), "the first chord is named");
        assert!(h.query_all_by_label("Dsus4").next().is_some());
    }
}
