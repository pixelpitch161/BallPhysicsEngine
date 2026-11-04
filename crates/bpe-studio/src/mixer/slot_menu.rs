//! A bus's slot menus: the add row at the end of its effects, and the right-click menu on an
//! effect (Rename, Duplicate, Copy, Paste, Bypass, Delete).

use super::cx::{MixCx, MixEdit};
use super::{Mixer, Renaming};
use crate::style;
use bpe_core::Layer;
use bpe_kit::sound::fx::FX_KINDS;
use eframe::egui::{self, RichText, Ui};

/// The last row of a bus's effects: "+ Add" opens the list of effect kinds.
pub fn add_row(ui: &mut Ui, cx: &mut MixCx, lane: u8) -> egui::Response {
    let menu = ui.menu_button(RichText::new("+ Add").size(12.0).color(style::MUTED), |ui| {
        ui.set_min_width(150.0);
        for kind in FX_KINDS {
            let label = cx.kit.registry.layer(kind).map_or(kind, |k| k.name());
            let help = cx.kit.registry.layer(kind).map_or("", |k| k.help());
            if ui.button(label).on_hover_text(help).clicked() {
                cx.edits.push(MixEdit::Add { kind, lane });
                ui.close();
            }
        }
    });
    let menu = menu.response.on_hover_text("Add an effect to this bus");
    crate::sound_tab::name_it(ui, &menu, &format!("Add to Bus {}", lane + 1));
    menu
}

/// The right-click menu on effect `layer` of bus `lane`.
pub fn row_menu(response: &egui::Response, cx: &mut MixCx, mixer: &mut Mixer, lane: u8, layer: &Layer) {
    response.context_menu(|ui| {
        ui.set_min_width(140.0);
        if ui.button("Rename").clicked() {
            mixer.renaming = Some((Renaming::Layer(layer.id), String::new()));
            ui.close();
        }
        if ui.button("Duplicate").clicked() {
            cx.edits.push(MixEdit::Duplicate(layer.id));
            ui.close();
        }
        if ui.button("Copy").clicked() {
            cx.edits.push(MixEdit::Copy(layer.id));
            ui.close();
        }
        let paste = mixer.copied.as_ref().map(|c| c.name.clone());
        let label = match &paste {
            Some(n) if !n.is_empty() => format!("Paste {n}"),
            _ => "Paste".to_owned(),
        };
        if ui.add_enabled(paste.is_some(), egui::Button::new(label)).clicked() {
            cx.edits.push(MixEdit::Paste(lane));
            ui.close();
        }
        let bypass = if layer.enabled { "Bypass" } else { "Turn on" };
        if ui.button(bypass).clicked() {
            cx.edits.push(MixEdit::Enable(layer.id, !layer.enabled));
            ui.close();
        }
        ui.separator();
        if ui.button("Delete").clicked() {
            cx.edits.push(MixEdit::Remove(layer.id));
            ui.close();
        }
    });
}
