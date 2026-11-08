//! The Looks tab's preview: the hovered (else the picked) look worn by this thing, its name and what it comes as,
//! whose colours to put on, and the two buttons.

use bpe_core::{Scene, ThingId};
use bpe_kit::Kit;
use bpe_kit::ready_looks::{self, Colours, How, ReadyLook, Wears};
use eframe::egui::{self, RichText, Ui};
use eframe::egui_wgpu;

use super::{LooksPage, colours_for, keeps_mine, picked};
use crate::look_editor::ball_view;
use crate::style;

/// Draws the preview of thing `id` (with `selected` the whole selection); returns how a button asked to put the
/// picked look on.
#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    page: &mut LooksPage,
    render: Option<&egui_wgpu::RenderState>,
    scene: &Scene,
    kit: &Kit,
    selected: &[ThingId],
    id: ThingId,
    w: Wears,
    hovered: Option<&'static ReadyLook>,
    time: f64,
) -> Option<How> {
    let reg = &kit.registry;
    let look = hovered.unwrap_or_else(|| picked(page, w));
    let colours = colours_for(page, scene, id, look);
    let dress = |mini: &mut Scene| {
        let _ = ready_looks::apply(mini, reg, id, look, colours, How::Replace);
    };
    match w {
        Wears::Balls => {
            ball_view::show_dressed(ui, &mut page.view, render, scene, kit, id, 0, time, &dress);
        }
        Wears::Shape => super::shape_view::show(ui, &mut page.shape, render, scene, kit, id, time, &dress),
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(look.name).font(style::semibold(15.0)).color(style::TEXT_STRONG));
        ui.label(RichText::new(look.family).small().color(style::MUTED));
    });
    ui.label(RichText::new(look.help).color(style::TEXT));
    ui.label(RichText::new(format!("Comes as: {}", look.comes_as(reg))).small().color(style::MUTED));
    ui.add_space(8.0);

    // Whose colours: the look's, or the thing's own where the look's main colour goes.
    let shown = picked(page, w);
    // The things the buttons reach: this one and every other selected thing that can wear the look.
    let mut reach: Vec<ThingId> = vec![id];
    reach.extend(selected.iter().copied().filter(|t| *t != id));
    reach.retain(|t| ready_looks::fits(scene, *t, shown));
    let can_keep = reach.iter().any(|t| keeps_mine(scene, *t, shown));
    ui.horizontal(|ui| {
        ui.label(RichText::new("Colours").small().color(style::MUTED));
        let theirs = page.colours == Colours::Look || !can_keep;
        let look_button = style::toggle_button("The look's", theirs).frame_when_inactive(theirs);
        if ui.add(look_button).on_hover_text("The colours the look comes in").clicked() {
            page.colours = Colours::Look;
        }
        let mine = !theirs;
        let mine_button = style::toggle_button("Keep mine", mine).frame_when_inactive(mine);
        let why = if !shown.takes_colour {
            format!("{} has colours of its own", shown.name)
        } else if w == Wears::Shape {
            "This shape has no colours of its own yet: give it a Colour layer first".to_owned()
        } else {
            "This look has no colours of its own to keep yet".to_owned()
        };
        let response = ui
            .add_enabled(can_keep, mine_button)
            .on_hover_text("This thing's own colours in place of the look's main colour")
            .on_disabled_hover_text(why);
        if response.clicked() {
            page.colours = Colours::Mine;
        }
    });
    ui.add_space(8.0);

    let count = reach.len();
    let can = count > 0;
    let mut pressed = None;
    let use_it = ui
        .add_enabled(
            can,
            style::primary_button("Use this look").min_size(egui::vec2(ui.available_width(), style::CONTROL_H)),
        )
        .on_hover_text(format!("{} in place of this look, with its layers to change as you like", shown.name));
    if use_it.clicked() {
        pressed = Some(How::Replace);
    }
    let on_top = ui
        .add_enabled(
            can,
            egui::Button::new("Add its layers on top").min_size(egui::vec2(ui.available_width(), style::CONTROL_H)),
        )
        .on_hover_text(format!("Keeps this look and adds {}'s layers to it, all but its Body", shown.name));
    if on_top.clicked() {
        pressed = Some(How::OnTop);
    }
    let name = scene.things.get(&id).map_or(String::new(), |t| t.name.clone());
    let words = match &page.done {
        Some((t, words)) if *t == id => words.clone(),
        _ if !can => "It has no Look card to put a look on: add one from Settings first.".to_owned(),
        _ if count > 1 => format!("Use this look replaces the look of {count} things. Ctrl+Z undoes either button."),
        _ => format!("Use this look replaces {name}'s look; the other keeps it. Ctrl+Z undoes either."),
    };
    ui.label(RichText::new(words).small().color(style::MUTED));
    pressed
}
