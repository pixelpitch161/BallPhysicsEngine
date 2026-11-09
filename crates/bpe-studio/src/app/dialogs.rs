//! The editor's dialogs: stop an export and close, restore autosaved work, confirm losing changes.

use super::*;

impl Studio {
    /// "A video is still exporting (40%). Stop it and close?"
    pub(super) fn stop_export_dialog(&mut self, ctx: &egui::Context) {
        if !self.ask_stop_export {
            return;
        }
        if !self.panels.export.busy() {
            // It finished while asking: carry on closing.
            self.ask_stop_export = false;
            self.close_after_export(ctx);
            return;
        }
        let percent = self.panels.export.progress_percent().unwrap_or(0);
        let mut stop = None;
        egui::Modal::new(egui::Id::unique("stop-export")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading(format!("A video is still exporting ({percent}%). Stop it and close?"));
            ui.label(RichText::new("The unfinished video will be deleted.").color(style::MUTED));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.add(style::primary_button("Keep exporting")).clicked() {
                    stop = Some(false);
                }
                if ui.button("Stop and close").clicked() {
                    stop = Some(true);
                }
            });
        });
        ctx.request_repaint_after(Duration::from_millis(250));
        match stop {
            Some(true) => {
                self.ask_stop_export = false;
                self.panels.export.shutdown();
                // The news of the stopped export is not wanted on the way out.
                let _ = self.panels.export.take_news();
                self.close_after_export(ctx);
            }
            Some(false) => self.ask_stop_export = false,
            None => {}
        }
    }

    /// Closes, asking about unsaved changes first.
    pub(super) fn close_after_export(&mut self, ctx: &egui::Context) {
        self.guard(Pending::Close, ctx);
    }

    /// "Ball Studio closed unexpectedly. Restore your last work?"
    pub(super) fn restore_dialog(&mut self, ctx: &egui::Context) {
        let Some(offer) = &self.offer else { return };
        let (name, when, file_newer) = (offer.name.clone(), offer.when.clone(), offer.file_newer);
        let mut choice = None;
        egui::Modal::new(egui::Id::unique("restore-work")).show(ctx, |ui| {
            ui.set_width(420.0);
            ui.heading("Ball Studio closed unexpectedly");
            ui.label(format!("Restore your last work on \"{name}\"? It was saved automatically {when}."));
            if file_newer {
                ui.label(
                    RichText::new(
                        "Its file was saved again after this copy was made, so restoring would bring back older work.",
                    )
                    .color(style::PROBLEM),
                );
            }
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                // The safer choice leads: Discard when the file is newer.
                let (restore, discard) = if file_newer {
                    (ui.button("Restore"), ui.add(style::primary_button("Discard")))
                } else {
                    (ui.add(style::primary_button("Restore")), ui.button("Discard"))
                };
                if restore.clicked() {
                    choice = Some(true);
                }
                if discard.clicked() {
                    choice = Some(false);
                }
            });
        });
        match choice {
            Some(true) => self.restore(),
            Some(false) => self.discard_offer(),
            None => {}
        }
    }

    pub(super) fn confirm_dialog(&mut self, ctx: &egui::Context) {
        if self.confirm.is_none() {
            return;
        }
        let name = self.scene.name.clone();
        let mut choice = None;
        egui::Modal::new(egui::Id::unique("save-changes")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading(format!("Save changes to \"{name}\"?"));
            ui.label(RichText::new("Your changes will be lost if you don't save them.").color(style::MUTED));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let save = style::primary_button("Save");
                if ui.add(save).clicked() {
                    choice = Some(true);
                }
                if ui.button("Don't save").clicked() {
                    choice = Some(false);
                }
                if ui.button("Cancel").clicked() {
                    self.confirm = None;
                }
            });
        });
        if let Some(save) = choice
            && let Some(action) = self.confirm.take()
            && (!save || self.save(false))
        {
            self.perform(action, ctx);
        }
    }
}
