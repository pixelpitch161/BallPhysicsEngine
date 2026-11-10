//! Grade mode's **Snapshots** and grade files, in the left column under the masks: Save snapshot;
//! a row per snapshot (a click restores it, as one undo step; hovering shows it in the preview;
//! right-click Rename, Update, Delete); then **Save grade…** and **Load grade…** (`.grade` files,
//! one grade to use in another project) and a line saying what happened.

use super::GradeMode;
use super::cx::GradeEdit;
use crate::app::Studio;
use crate::style;
use bpe_core::{GRADE_EXTENSION, GradeSnapshot};
use bpe_kit::modules::grade::snapshots;
use eframe::egui::{self, RichText, Sense, Ui};

/// A grade file asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileAsk {
    Save,
    Load,
}

/// The section, its edits applied (and the file pickers opened when asked).
pub fn section(ui: &mut Ui, studio: &mut Studio) {
    ui.label(RichText::new("Snapshots").font(style::semibold(13.0)));
    ui.add_space(4.0);
    let list = snapshots::snapshots_of(&studio.scene);
    let mut edits = Vec::new();
    let mut peek = None;
    let mode = &mut studio.grade;
    if ui
        .button("Save snapshot")
        .on_hover_text("Keep the grade as it is now, to come back to it (saved with the scene)")
        .clicked()
    {
        edits.push(GradeEdit::SaveSnapshot);
    }
    ui.add_space(2.0);
    if list.is_empty() {
        ui.label(RichText::new("Save the grade as it is to come back to it.").small().color(style::FAINT));
    }
    for (index, s) in list.iter().enumerate() {
        if row(ui, mode, index, s, &mut edits) {
            peek = Some(index);
        }
    }
    mode.peek = peek;
    ui.add_space(10.0);
    ui.horizontal_wrapped(|ui| {
        if ui
            .button("Save grade\u{2026}")
            .on_hover_text("Save the grade to a file, to use in another project")
            .clicked()
        {
            mode.ask_file = Some(FileAsk::Save);
        }
        if ui
            .button("Load grade\u{2026}")
            .on_hover_text("Put a saved grade in place of this one (its snapshots stay)")
            .clicked()
        {
            mode.ask_file = Some(FileAsk::Load);
        }
    });
    if let Some((text, problem)) = &mode.file_note {
        let ink = if *problem { style::PROBLEM } else { style::MUTED };
        ui.label(RichText::new(text).small().color(ink));
    }
    super::apply(studio, edits);
    if let Some(ask) = studio.grade.ask_file.take() {
        files(studio, ask);
    }
}

/// One snapshot's row. True while it is hovered (the preview shows it).
fn row(ui: &mut Ui, mode: &mut GradeMode, index: usize, s: &GradeSnapshot, edits: &mut Vec<GradeEdit>) -> bool {
    if let Some((_, typed)) = mode.snapshot_renaming.as_mut().filter(|(i, _)| *i == index) {
        let mut done = false;
        ui.horizontal(|ui| {
            let label = ui.label(RichText::new("Name").small().color(style::MUTED));
            let field = ui.add(egui::TextEdit::singleline(typed).desired_width(140.0)).labelled_by(label.id);
            done = field.lost_focus();
            if !done && !field.has_focus() {
                field.request_focus();
            }
            if done && !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                edits.push(GradeEdit::RenameSnapshot(index, typed.clone()));
            }
        });
        if done {
            mode.snapshot_renaming = None;
        }
        return false;
    }
    let button = egui::Button::new(RichText::new(&s.name).color(style::TEXT)).frame(false).sense(Sense::click());
    let response = ui
        .add(button)
        .on_hover_text("Click to put this grade back (Ctrl+Z undoes it). The picture shows it while the pointer is here. Right-click for more.");
    if response.clicked() {
        edits.push(GradeEdit::RestoreSnapshot(index));
    }
    response.context_menu(|ui| {
        if ui.button("Rename").clicked() {
            mode.snapshot_renaming = Some((index, s.name.clone()));
            ui.close();
        }
        if ui.button("Update").on_hover_text("Put the grade as it is now into this snapshot").clicked() {
            edits.push(GradeEdit::UpdateSnapshot(index));
            ui.close();
        }
        ui.separator();
        if ui.button("Delete").clicked() {
            edits.push(GradeEdit::RemoveSnapshot(index));
            ui.close();
        }
    });
    response.hovered()
}

/// Opens the system's file picker for `ask`, then saves or loads.
fn files(studio: &mut Studio, ask: FileAsk) {
    let filter = ("Ball Studio grade", [GRADE_EXTENSION]);
    match ask {
        FileAsk::Save => {
            let name = format!("{}.{GRADE_EXTENSION}", crate::export_window::safe_file_name(&studio.scene.name));
            let Some(path) = rfd::FileDialog::new()
                .set_title("Save grade")
                .add_filter(filter.0, &filter.1)
                .set_file_name(name)
                .save_file()
            else {
                return;
            };
            studio.grade.file_note = Some(save_to(studio, &path));
        }
        FileAsk::Load => {
            let Some(path) = rfd::FileDialog::new().set_title("Load grade").add_filter(filter.0, &filter.1).pick_file()
            else {
                return;
            };
            studio.grade.file_note = Some(load_from(studio, &path));
        }
    }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Saves the grade to `path`: what to say, and whether it is a problem.
pub fn save_to(studio: &Studio, path: &std::path::Path) -> (String, bool) {
    let text = snapshots::grade_text(&studio.scene, &studio.kit.registry);
    match crate::files::write_safely(path, &text, false) {
        Ok(()) => (format!("Saved {}.", file_name(path)), false),
        Err(e) => (format!("Couldn't save {}: {e}", file_name(path)), true),
    }
}

/// Loads the grade at `path` in place of the scene's (one undo step): what to say, and whether it
/// is a problem.
pub fn load_from(studio: &mut Studio, path: &std::path::Path) -> (String, bool) {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => return (format!("Couldn't read {}: {e}", file_name(path)), true),
    };
    match snapshots::read_grade(&text, &studio.kit.registry) {
        Ok(grade) => {
            super::apply(studio, vec![GradeEdit::LoadGrade(Box::new(grade))]);
            (format!("Loaded {}.", file_name(path)), false)
        }
        Err(words) => (words, true),
    }
}
