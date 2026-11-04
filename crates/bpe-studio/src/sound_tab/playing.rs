//! How hits play notes: Poly or Mono, how many notes at once, Glide (Mono), the softest hit
//! that plays, the retrigger guard and which balls are heard.

use super::{TabCx, panel};
use crate::style;
use bpe_core::Value;
use bpe_kit::sound::{ADSR, DISTANCE, PlayMode, PlayWhen};
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
            ui.label(RichText::new("Plays").small().color(style::MUTED));
            cx.chips(
                ui,
                "play_when",
                &[(PlayWhen::OnHit, "On hit"), (PlayWhen::Always, "Always"), (PlayWhen::InRange, "In range")],
            );
        });
        let when = cx.choice::<PlayWhen>("play_when");
        if when == PlayWhen::InRange {
            zone_menu(ui, cx);
        }
        if when != PlayWhen::OnHit && fades_while_held(cx) {
            let name = first_envelope_name(cx);
            ui.label(
                RichText::new(format!(
                    "{name}'s Sustain is 0, so a held note dies away after its Decay. Raise Sustain to drone."
                ))
                .small()
                .color(style::FAINT),
            );
        }
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

/// Whether the first envelope (the notes' level) is on with its Sustain at 0: a held note then
/// dies away after its Decay.
fn fades_while_held(cx: &TabCx) -> bool {
    let Some(card) = cx.scene.card(cx.id, bpe_core::SlotId::Sound) else { return false };
    let Some(env) = card.layers.iter().find(|l| l.kind == ADSR && l.enabled) else { return false };
    super::number(&cx.get(&bpe_core::layered_key(env.id, "sustain"))).is_some_and(|v| v <= 0.0)
}

/// The first envelope's name ("ADSR 1" on the starter sound).
fn first_envelope_name(cx: &TabCx) -> String {
    cx.scene
        .card(cx.id, bpe_core::SlotId::Sound)
        .and_then(|c| c.layers.iter().find(|l| l.kind == ADSR && l.enabled))
        .map_or_else(|| "The envelope".into(), |l| l.name.clone())
}

/// In range of which Distance (the first that is on when none is named).
fn zone_menu(ui: &mut Ui, cx: &mut TabCx) {
    let Some(card) = cx.scene.card(cx.id, bpe_core::SlotId::Sound) else { return };
    let dists: Vec<(bpe_core::LayerId, String, bool)> =
        card.layers.iter().filter(|l| l.kind == DISTANCE).map(|l| (l.id, l.name.clone(), l.enabled)).collect();
    let named = match cx.get("play_zone") {
        Value::Text(t) => t.trim().parse::<u32>().ok(),
        _ => None,
    };
    // The Distance the sound uses: the named one while it is on, otherwise the first that is on.
    let now = dists
        .iter()
        .find(|(id, _, on)| Some(id.0) == named && *on)
        .or_else(|| dists.iter().find(|(_, _, on)| *on))
        .map(|(_, name, _)| name.clone());
    ui.horizontal(|ui| {
        let label = ui.label(RichText::new("In range of").small().color(style::MUTED));
        let shown = now.unwrap_or_else(|| "No Distance is on".into());
        let r = ui.menu_button(shown, |ui| {
            for (id, name, on) in &dists {
                let text = if *on { name.clone() } else { format!("{name} (off)") };
                if ui.button(text).clicked() {
                    cx.set("play_zone", Value::Text(id.0.to_string()));
                    ui.close();
                }
            }
        });
        let _ = r.response.labelled_by(label.id);
        if !dists.iter().any(|(_, _, on)| *on) {
            ui.label(RichText::new("Turn a Distance on to hold notes in its range").small().color(style::FAINT));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::super::tests::sounding;
    use eframe::egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn plays_always_and_in_range_of_a_distance() {
        let mut h = sounding();
        let get = |h: &egui_kittest::Harness<'_, (crate::app::Studio, f32)>, k: &str| {
            h.state().0.scene.card(super::super::tests::SHAPE, bpe_core::SlotId::Sound).unwrap().params.get(k).cloned()
        };
        h.get_by_label("Always").scroll_to_me();
        h.run();
        h.get_by_label("Always").click();
        h.run();
        assert_eq!(get(&h, "play_when"), Some(bpe_core::Value::Choice("always".into())));
        h.get_by_label("In range").click();
        h.run();
        assert!(h.query_by_label("Turn a Distance on to hold notes in its range").is_some(), "none on yet");
        assert!(h.query_by_label("In range of").is_some());
    }

    #[test]
    fn mono_shows_glide() {
        let mut h = sounding();
        assert!(h.query_by_role_and_label(Role::Slider, "Glide").is_none());
        assert!(h.query_by_role_and_label(Role::Slider, "Voices").is_some());
        h.get_by_label("Mono").scroll_to_me();
        h.run();
        h.get_by_label("Mono").click();
        h.run();
        assert!(h.query_by_role_and_label(Role::Slider, "Glide").is_some());
        assert!(h.query_by_role_and_label(Role::Slider, "Voices").is_none(), "Mono plays one note");
    }
}
