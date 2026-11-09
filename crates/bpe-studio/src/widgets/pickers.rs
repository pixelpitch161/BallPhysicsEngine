//! Pickers: who and target pickers, and colour buttons.

use super::*;

/// A rule block's who: a menu naming it, with the firing's own parties (when `choices.relative`),
/// Anything, and the scene's things, tags and kinds. A thing with balls gets a "ball" number box
/// beside it (0 = the whole thing). Marked changed when something is picked.
pub(crate) fn who_picker(ui: &mut Ui, w: &mut bpe_core::Who, choices: &TargetChoices) -> Response {
    use bpe_core::Who;
    let mut picked: Option<Who> = None;
    let menu = ui.menu_button(who_words(w, choices), |ui| {
        ui.set_min_width(170.0);
        let mut item = |ui: &mut Ui, who: Who, text: &str, help: &str| {
            if ui.selectable_label(*w == who, text).on_hover_text(help).clicked() {
                picked = Some(who);
                ui.close();
            }
        };
        if choices.relative {
            item(ui, Who::That, "That ball", "The ball (or thing) this firing is about");
            item(ui, Who::Other, "The thing it hit", "The other side of the firing");
            item(ui, Who::Group, "That ball's group", "Every ball of that ball's thing");
        }
        item(ui, Who::Anything, "Anything", "Every thing");
        ui.menu_button("Thing", |ui| {
            for (id, name) in &choices.things {
                let on = matches!(w, Who::Thing { thing, .. } if thing == id);
                if ui.selectable_label(on, name).clicked() {
                    picked = Some(Who::Thing { thing: *id, ball: None });
                    ui.close();
                }
            }
        });
        if choices.tags.is_empty() {
            ui.add_enabled(false, egui::Button::new("Tagged"))
                .on_disabled_hover_text("No tags yet. Give things tags in their header.");
        } else {
            ui.menu_button("Tagged", |ui| {
                for tag in &choices.tags {
                    if ui.selectable_label(matches!(w, Who::Tag(x) if x == tag), tag).clicked() {
                        picked = Some(Who::Tag(tag.clone()));
                        ui.close();
                    }
                }
            });
        }
        ui.menu_button("Any", |ui| {
            for (id, name) in &choices.kinds {
                if ui.selectable_label(matches!(w, Who::Kind(x) if x == id), name).clicked() {
                    picked = Some(Who::Kind(id.clone()));
                    ui.close();
                }
            }
        });
    });
    let mut response = menu.response.on_hover_text("Who this block is about");
    if let Some(p) = picked {
        *w = p;
        response.mark_changed();
    }
    // One ball of a thing with balls, or one letter of a Text in the world, by its number (0:
    // the whole thing).
    if let Who::Thing { thing, ball } = w
        && (choices.ball_things.contains(thing) || choices.letter_things.contains(thing))
    {
        let one = if choices.letter_things.contains(thing) { "letter" } else { "ball" };
        let mut n = ball.unwrap_or(0);
        let number = ui
            .add(
                DragValue::new(&mut n)
                    .range(0..=u32::MAX)
                    .speed(0.1)
                    .custom_formatter(|v, _| if v < 0.5 { format!("every {one}") } else { format!("{one} {v:.0}") })
                    .custom_parser(|t| {
                        let t = t.trim().to_lowercase();
                        if t.starts_with("every") {
                            return Some(0.0);
                        }
                        t.trim_start_matches(one).trim().parse::<f64>().ok()
                    }),
            )
            .on_hover_text(format!("One {one} of it, by its number; 0 for every {one}"));
        if number.changed() {
            *ball = (n > 0).then_some(n);
            response.mark_changed();
        }
        response |= number;
    }
    response
}

/// A menu naming the target, with Anything, Itself and the scene's things, tags and kinds to
/// pick from. Marked changed when something is picked.
pub(crate) fn target_picker(ui: &mut Ui, t: &mut Target, choices: &TargetChoices) -> Response {
    let mut picked: Option<Target> = None;
    let menu = ui.menu_button(target_words(t, choices), |ui| {
        ui.set_min_width(160.0);
        if ui.selectable_label(*t == Target::Anything, "Anything").clicked() {
            picked = Some(Target::Anything);
            ui.close();
        }
        if ui.selectable_label(*t == Target::Me, "Itself").on_hover_text("The thing this setting belongs to").clicked()
        {
            picked = Some(Target::Me);
            ui.close();
        }
        ui.menu_button("Thing", |ui| {
            for (id, name) in &choices.things {
                if ui.selectable_label(*t == Target::Thing(*id), name).clicked() {
                    picked = Some(Target::Thing(*id));
                    ui.close();
                }
            }
        });
        if choices.tags.is_empty() {
            ui.add_enabled(false, egui::Button::new("Tagged"))
                .on_disabled_hover_text("No tags yet. Give things tags in their header.");
        } else {
            ui.menu_button("Tagged", |ui| {
                for tag in &choices.tags {
                    if ui.selectable_label(matches!(t, Target::Tag(x) if x == tag), tag).clicked() {
                        picked = Some(Target::Tag(tag.clone()));
                        ui.close();
                    }
                }
            });
        }
        ui.menu_button("Any", |ui| {
            for (id, name) in &choices.kinds {
                if ui.selectable_label(matches!(t, Target::Kind(x) if x == id), name).clicked() {
                    picked = Some(Target::Kind(id.clone()));
                    ui.close();
                }
            }
        });
    });
    let mut response = menu.response.on_hover_text("What this points at");
    if let Some(p) = picked {
        *t = p;
        response.mark_changed();
    }
    response
}

/// egui keeps an additive colour as premultiplied with no alpha. Built unmultiplied, no alpha
/// turns it into plain transparent black, so "Additive" would undo itself on the next frame.
pub(crate) fn picker_color(c: Color) -> Color32 {
    if c.is_additive() {
        Color32::from_rgba_premultiplied(c.r, c.g, c.b, 0) // scene colour
    } else {
        Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a) // scene colour
    }
}

/// The picker's colour as a scene colour. An additive one comes back as its colour with no
/// alpha, which is how `Color` keeps it.
pub(crate) fn scene_color(c: Color32) -> Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Color { r, g, b, a }
}

pub(crate) fn color_button(ui: &mut Ui, value: &mut Color) -> Response {
    let mut c = picker_color(*value);
    let response = egui::color_picker::color_edit_button_srgba(ui, &mut c, Alpha::OnlyBlend);
    if response.changed() {
        *value = scene_color(c);
    }
    response
}
