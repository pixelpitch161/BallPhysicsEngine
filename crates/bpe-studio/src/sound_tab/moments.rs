//! A Particles thing's Moments panel (particles part 7): which of its particles' moments play
//! notes, the most notes a second (the strongest are kept) and what makes a note louder. A Balls
//! or Emitter thing's (W10): which of its balls' moments play.

use super::{TabCx, panel};
use crate::style;
use bpe_kit::sound::MomentStrength;
use eframe::egui::{RichText, Ui};

/// Whether the tab's thing is a Particles thing.
pub fn is_particles(cx: &TabCx) -> bool {
    cx.scene.things.get(&cx.id).is_some_and(|t| t.kind == bpe_kit::things::particles::KIND)
}

/// Whether the tab's thing is a Balls or Emitter thing, whose sound plays its own balls (W10).
pub fn is_balls(cx: &TabCx) -> bool {
    cx.scene.things.get(&cx.id).is_some_and(|t| bpe_kit::sound::hears_own_balls(&t.kind))
}

/// Whether the tab's thing is an Emitter.
fn is_emitter(cx: &TabCx) -> bool {
    cx.scene.things.get(&cx.id).is_some_and(|t| t.kind == bpe_kit::things::emitter::KIND)
}

/// Draws a Balls or Emitter thing's Moments panel (W10): which of its balls' moments play.
pub fn show_balls(ui: &mut Ui, cx: &mut TabCx) {
    let emitter = is_emitter(cx);
    let mut moments: Vec<(&str, &str, &str)> = vec![
        ("moments_hit_shape", "Hit a shape", "One of its balls hitting a shape, a word or a wall plays a note"),
        ("moments_hit_ball", "Hit a ball", "One of its balls hitting a ball, its own or another's, plays a note"),
    ];
    if emitter {
        moments.push(("moments_fire", "Are fired", "Each ball it fires plays a note as it sets off"));
        moments.push((
            "moments_catch",
            "Catch",
            "A ball it catches, or one of its balls caught by another emitter, plays a note",
        ));
    } else {
        moments.push(("moments_catch", "Are caught", "One of its balls caught by an emitter plays a note"));
    }
    moments.push(("moments_break", "Break", "One of its balls broken by a rule plays a note"));
    panel(ui, |ui| {
        ui.label(RichText::new("Plays when its balls").font(style::semibold(13.0)));
        ui.horizontal_wrapped(|ui| {
            for (key, name, help) in moments {
                ui.horizontal(|ui| {
                    cx.switch(ui, key, name).on_hover_text(help);
                    ui.label(RichText::new(name).small().color(style::TEXT));
                });
                ui.add_space(6.0);
            }
        });
    });
}

fn per_second(v: f64) -> String {
    format!("{v:.0}/s")
}

/// Draws the Moments panel.
pub fn show(ui: &mut Ui, cx: &mut TabCx) {
    panel(ui, |ui| {
        ui.label(RichText::new("Plays when its particles").font(style::semibold(13.0)));
        ui.horizontal_wrapped(|ui| {
            for (key, name, help) in [
                ("moments_hit_ball", "Hit a ball", "A particle bouncing off a ball, or vanishing at one, plays a note"),
                (
                    "moments_hit_shape",
                    "Hit a shape",
                    "A particle bouncing off a shape, or vanishing at one, plays a note",
                ),
                ("moments_land", "Land", "A particle stopping on a shape plays a note, once"),
                ("moments_born", "Are born", "Each particle born in a burst plays a note"),
                ("moments_burst", "Burst", "Each burst plays a note"),
            ] {
                ui.horizontal(|ui| {
                    cx.switch(ui, key, name).on_hover_text(help);
                    ui.label(RichText::new(name).small().color(style::TEXT));
                });
                ui.add_space(6.0);
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            cx.knob(ui, "moments_most", "Most notes", &per_second)
                .on_hover_text("The most notes its particles play a second. When more happen, the strongest play.");
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Strength from").small().color(style::MUTED));
            cx.chips(
                ui,
                "moments_strength",
                &[
                    (MomentStrength::Speed, "Speed"),
                    (MomentStrength::Size, "Size"),
                    (MomentStrength::SpeedAndSize, "Speed and size"),
                    (MomentStrength::TheSame, "All the same"),
                ],
            );
        });
    });
}
