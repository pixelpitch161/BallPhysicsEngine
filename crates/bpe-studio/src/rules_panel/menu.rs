//! The block menu: When / If / Logic / Do, then the kinds of that section the registry has.

use bpe_core::{BlockSection, Registry};
use eframe::egui::Ui;

/// The sections in the order the menu lists them.
const SECTIONS: [BlockSection; 4] = [BlockSection::When, BlockSection::If, BlockSection::Logic, BlockSection::Do];

/// The block kinds of `section`, in registry order: (id, name, help).
pub fn kinds_in(reg: &Registry, section: BlockSection) -> Vec<(&'static str, &'static str, &'static str)> {
    reg.blocks().filter(|k| k.section() == section).map(|k| (k.id(), k.name(), k.help())).collect()
}

/// The menu's insides: a submenu per section (a section with one kind is a button), or only the
/// When kinds when `whens_only`. The kind picked, if any.
pub fn block_menu(ui: &mut Ui, reg: &Registry, whens_only: bool) -> Option<&'static str> {
    let mut picked = None;
    ui.set_min_width(140.0);
    let mut kinds = |ui: &mut Ui, list: &[(&'static str, &'static str, &'static str)]| {
        for &(id, name, help) in list {
            if ui.button(name).on_hover_text(help).clicked() {
                picked = Some(id);
                ui.close();
            }
        }
    };
    if whens_only {
        kinds(ui, &kinds_in(reg, BlockSection::When));
        return picked;
    }
    for section in SECTIONS {
        let list = kinds_in(reg, section);
        match list.as_slice() {
            [] => {}
            // Logic is one block with modes.
            [(_, name, _)] if *name == section.name() => kinds(ui, &list),
            _ => {
                ui.menu_button(section.name(), |ui| {
                    ui.set_min_width(150.0);
                    kinds(ui, &list);
                });
            }
        }
    }
    picked
}
