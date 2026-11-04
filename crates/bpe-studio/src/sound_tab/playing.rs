//! How hits play notes: Poly or Mono, how many notes at once, Glide (Mono), the softest hit
//! that plays, the retrigger guard and which balls are heard.

use super::{TabCx, panel};
use crate::style;
use bpe_core::Value;
use bpe_kit::sound::PlayMode;
use eframe::egui::{RichText, Ui};

fn plain(v: f64) -> String {
    format!("{v:.0}")
}

fn ms(v: f64) -> String {
    format!("{v:.0} ms")
}

fn percent(v: f64) -> String {
    format!("{v:.0} %")
}

/// Draws the Playing panel.
pub fn show(ui: &mut Ui, cx: &mut TabCx) {
    panel(ui, |ui| {
        ui.label(RichText::new("Playing").font(style::semibold(13.0)));
        cx.chips(ui, "play_mode", &[(PlayMode::Poly, "Poly"), (PlayMode::Mono, "Mono")]);
        let mono = cx.choice::<PlayMode>("play_mode") == PlayMode::Mono;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if !mono {
                cx.knob(ui, "play_voices", "Voices", &plain).on_hover_text(
                    "Notes that sound at once. When more come, the oldest fades out (in 5 ms) to make room.",
                );
            } else {
                cx.knob(ui, "play_glide", "Glide", &ms).on_hover_text("How long a new note slides from the last one");
            }
            cx.knob(ui, "play_min_strength", "Min strength", &percent)
                .on_hover_text("Softer hits than this play nothing. 100 % is a hit at 1500 px/s.");
            cx.knob(ui, "play_retrigger", "Retrigger", &ms)
                .on_hover_text("A ball hitting again sooner than this plays no new note");
        });
        ui.horizontal(|ui| {
            let label = ui.label(RichText::new("Heard from").small().color(style::MUTED));
            let mut target = match cx.get("play_heard_from") {
                Value::Target(t) => t,
                _ => bpe_core::Target::Anything,
            };
            let r = crate::widgets::target_picker(ui, &mut target, cx.choices).labelled_by(label.id);
            if r.changed() {
                cx.set("play_heard_from", Value::Target(target));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::super::tests::sounding;
    use eframe::egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn mono_shows_glide() {
        let mut h = sounding();
        assert!(h.query_by_role_and_label(Role::Slider, "Glide").is_none());
        assert!(h.query_by_role_and_label(Role::Slider, "Voices").is_some());
        h.get_by_label("Mono").click();
        h.run();
        assert!(h.query_by_role_and_label(Role::Slider, "Glide").is_some());
        assert!(h.query_by_role_and_label(Role::Slider, "Voices").is_none(), "Mono plays one note");
    }
}
