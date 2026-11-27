//! A Water scene's final in the editor's frame (Water mode part 8): the toolbar's Draft | Final
//! switch, Make final and the Finals menu; the make followed each frame (and the export after it);
//! the Export window told what the final is; the playhead's speed in slow motion.

use bpe_kit::things::water::make::gb;
use eframe::egui::{self, Button};

use super::final_job::WaterView;
use crate::app::Studio;
use crate::style;

/// What Make final's hover says when the final is made.
pub const ALL_MADE: &str = "The final is made and up to date.";

/// The toolbar's Water final controls.
pub(crate) fn toolbar(ui: &mut egui::Ui, studio: &mut Studio) {
    let view = studio.water_final.view;
    let draft = ui
        .add(style::toggle_button("Draft", view == WaterView::Draft))
        .on_hover_text("Show the draft on the stage: quick, coarse water that plays as you edit.");
    if draft.clicked() {
        studio.water_final.view = WaterView::Draft;
    }
    let shown = ui.add(style::toggle_button("Final", view == WaterView::Final)).on_hover_text(
        "Show the final on the stage: the water at Final detail as the video shows it, where Make final has \
         made it (the draft shows where it hasn't).",
    );
    if shown.clicked() {
        studio.water_final.view = WaterView::Final;
    }
    if studio.water_final.job.is_some() {
        let stop = ui
            .add(Button::new("Stop making").frame_when_inactive(false))
            .on_hover_text("Stop making the final. What is made so far is kept, and Make final goes on from there.");
        if stop.clicked() {
            studio.water_final.stop_make();
        }
    } else {
        let found = studio.water_final.found.as_ref();
        let can = found.is_some_and(|f| !f.status.complete());
        let hover = found.map_or(String::new(), |f| {
            format!("Work out the whole scene's water at Final detail for the video: {}.", f.making_words())
        });
        let why = if found.is_some() { ALL_MADE } else { "There is no Water scene to make." };
        let make = ui
            .add_enabled(can, Button::new("Make final").frame_when_inactive(false))
            .on_hover_text(hover)
            .on_disabled_hover_text(why);
        if make.clicked() {
            studio.water_final.start_make(Some(ui.ctx()));
        }
    }
    finals_menu(ui, studio);
}

/// The Finals menu: clear this scene's final or every one, show the folder, preview a frame.
fn finals_menu(ui: &mut egui::Ui, studio: &mut Studio) {
    let response = ui.menu_button("Finals", |ui| {
        let making = studio.water_final.job.is_some();
        let own = studio.water_final.found.as_ref().is_some_and(|f| f.status.own && f.status.take.is_some());
        let clear = ui
            .add_enabled(own && !making, Button::new("Clear this scene's final"))
            .on_hover_text("Remove this scene's final from the disk. Make final makes it again.")
            .on_disabled_hover_text("This scene has no final to clear (or it is being made).");
        if clear.clicked() {
            studio.status = match studio.water_final.clear_this() {
                Ok(()) => "Cleared this scene's final.".into(),
                Err(e) => format!("Couldn't clear the final: {e:#}"),
            };
            ui.close();
        }
        let size = studio.water_final.size();
        let all = ui
            .add_enabled(size > 0 && !making, Button::new(format!("Clear every final ({})", gb(size))))
            .on_hover_text("Remove every scene's final from the disk, to free the room they take.");
        if all.clicked() {
            studio.status = match studio.water_final.clear_all() {
                Ok(freed) => format!("Cleared every final, freeing {}.", gb(freed)),
                Err(e) => format!("Couldn't clear the finals: {e:#}"),
            };
            ui.close();
        }
        let folder = ui.button("Show the finals folder").on_hover_text(
            "Open the folder the finals are kept in. Finals not used for 30 days are removed when Ball Studio starts.",
        );
        if folder.clicked() {
            let dir = studio.water_final.finals.dir().to_path_buf();
            let _ = std::fs::create_dir_all(&dir);
            crate::export_window::open_file(&dir);
            ui.close();
        }
        let made = studio.water_final.found.as_ref().is_some_and(|f| f.status.take.is_some() && f.status.made > 0);
        let preview = ui
            .add_enabled(made, Button::new("Preview final frame"))
            .on_hover_text("Draw the final's frame at the playhead at the export's size.")
            .on_disabled_hover_text("Nothing of the final is made yet.");
        if preview.clicked() {
            studio.panels.export.test_frame = Some((studio.panels.export.scale, None));
            ui.close();
        }
    });
    response.response.on_hover_text("This scene's final and the finals folder");
}

impl Studio {
    /// Finds the scene's final as it is, follows a make (exporting after it when asked) and
    /// tells the Export window what the final is. Every frame.
    pub(crate) fn follow_water_final(&mut self, ctx: &egui::Context) {
        let settings = self.settings();
        let wf = &mut self.water_final;
        if wf.find(self.revision, &self.scene, &self.resolved, self.water_draft.setup.as_deref()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let export = &mut self.panels.export;
        if std::mem::take(&mut export.make_and_export) {
            wf.export_after = true;
            wf.start_make(Some(ctx));
        }
        if std::mem::take(&mut export.stop_making) {
            wf.stop_make();
        }
        match wf.follow() {
            Some(true) => {
                self.status = "The final is made.".into();
                if std::mem::take(&mut wf.export_after) {
                    export.export_final = true;
                }
                // (Found again now, so the export reads it whole.)
                wf.stale = true;
                wf.find(self.revision, &self.scene, &self.resolved, self.water_draft.setup.as_deref());
            }
            Some(false) => {
                let after = std::mem::take(&mut wf.export_after);
                match &wf.problem {
                    Some(why) if after => export.water_make_failed(why),
                    Some(_) => {}
                    None => self.status = "Stopped making the final. What was made is kept.".into(),
                }
            }
            None => {}
        }
        export.water = wf.found.as_ref().filter(|_| self.scene.sim == bpe_core::SimKind::Water).map(|found| {
            crate::export_window::WaterExport {
                finals: wf.finals.dir().to_path_buf(),
                complete: found.status.complete(),
                words: wf.words().map(|w| w.0).unwrap_or_default(),
                making_words: found.making_words(),
                making: wf
                    .job
                    .as_ref()
                    .filter(|_| wf.export_after)
                    .map(|job| (job.progress().share(), job.seconds_left())),
                water_seconds: found.length,
                video_seconds: found.video_seconds,
                frames: found.video_frames(settings.fps),
            }
        });
        self.panels.final_frame.finals = Some(wf.finals.dir().to_path_buf());
    }

    /// The Speed the water plays at, at water time `time` (1 in a Balls scene).
    pub(crate) fn water_speed(&self, time: f64) -> f64 {
        match &self.water_final.found {
            Some(found) if self.water() => f64::from(found.make.speeds.speed_at(time)),
            _ => 1.0,
        }
    }
}
