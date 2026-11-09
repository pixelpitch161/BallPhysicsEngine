//! The inspector's header: the thing's name, kind, track colour and eye, then its tags.

use super::*;

/// Row 1: the name, the kind and the eye; row 2: the tags. Indented like a card's insides, so its
/// columns line up with the cards' settings.
pub(super) fn show(
    ui: &mut Ui,
    inspector: &mut Inspector,
    (id, thing): (ThingId, &bpe_core::Thing),
    kind: &dyn bpe_core::ThingKind,
    lw: f32,
    edits: &mut Vec<Edit>,
) {
    let indent = egui::Frame::new().inner_margin(egui::Margin::symmetric(CARD_MARGIN, 0));
    indent.show(ui, |ui| {
        settings_like_grid(("header", id)).show(ui, |ui| {
            let name_label = grid_label(ui, lw, "Name");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Hiding the root would hide the whole scene; its card already says what it is.
                if id != ThingId::ROOT {
                    if eye_toggle(ui, thing.enabled) {
                        edits.push(Edit::Enabled(!thing.enabled));
                    }
                    ui.label(RichText::new(kind.name()).small().color(style::MUTED)).on_hover_text(kind.help());
                    // The track colour, which opens a colour picker.
                    let swatch = style::swatch_button(ui, thing.colour, "Track colour")
                        .on_hover_text("This thing's colour in the tree, the preview and the timeline");
                    egui::Popup::menu(&swatch).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(
                        |ui| {
                            if let Some(colour) = style::track_picker(ui, thing.colour) {
                                edits.push(Edit::Colour(colour));
                            }
                        },
                    );
                }
                let mut name = thing.name.clone();
                let edit = egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY);
                if ui.add(edit).labelled_by(name_label.id).changed() {
                    edits.push(Edit::Name(name));
                }
            });
            ui.end_row();
            let tags_label = grid_label(ui, lw, "Tags")
                .on_hover_text("Words that bounce targets and the camera can pick this out by, separated by spaces");
            // What is typed is kept while the field has the keyboard, and set when it lets go.
            let mut tags = match &inspector.tags_draft {
                Some((t, draft)) if *t == id => draft.clone(),
                _ => thing.tags.iter().cloned().collect::<Vec<_>>().join(" "),
            };
            let tag_edit = ui
                .add(egui::TextEdit::singleline(&mut tags).desired_width(f32::INFINITY).hint_text("none"))
                .labelled_by(tags_label.id);
            if tag_edit.lost_focus() {
                inspector.tags_draft = None;
                let set: BTreeSet<String> = tags.split_whitespace().map(str::to_owned).collect();
                if set != thing.tags {
                    edits.push(Edit::Tags(set));
                }
            } else if tag_edit.has_focus() {
                inspector.tags_draft = Some((id, tags));
            }
            ui.end_row();
        })
    });
}

/// The header's eye: open while the thing is shown. True when it was clicked.
pub(super) fn eye_toggle(ui: &mut Ui, shown: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(18.0, 16.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, shown, "Shown"));
    let ink = if response.hovered() { style::TEXT } else { style::MUTED };
    crate::tree::paint_eye(ui.painter(), rect, shown, ink);
    let help = if shown {
        "Shown. A hidden thing is left out of the video and the physics."
    } else {
        "Hidden: left out of the video and the physics. Click to show it."
    };
    response.on_hover_text(help).clicked()
}
