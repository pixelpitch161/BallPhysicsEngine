//! Editing a clip's scene (part 3 plan, decisions 14 to 16): double-click a clip (or Edit scene
//! in its inspector) and its kept scene opens in Build mode, the scene that was open there parked
//! until Back to video puts the edited scene into the video (one undo step) and brings it back.

use super::{VideoPick, arrange};
use crate::app::Studio;
use crate::grade_mode::EditorMode;
use crate::history::History;
use crate::style;
use bpe_core::Scene;
use bpe_core::video::{ClipId, KeptId, Video};
use eframe::egui::{self, RichText, Ui};
use std::path::PathBuf;

/// The Build scene as it was when a clip's scene was opened in its place.
pub struct ParkedScene {
    pub scene: Scene,
    pub saved: Scene,
    pub path: Option<PathBuf>,
    pub history: History,
    /// The Build playhead.
    pub time: f64,
}

/// Which kept scene Build mode is editing for the video.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipEdit {
    pub kept: KeptId,
    /// The kept scene's name, for the bar.
    pub name: String,
}

impl Studio {
    /// Whether Build mode is editing a clip's scene.
    pub fn editing_clip(&self) -> bool {
        self.clip_edit.is_some()
    }

    /// Opens clip `clip`'s kept scene in Build mode, parking the scene open there.
    pub fn edit_clip_scene(&mut self, clip: ClipId) -> bool {
        let Some(kept) = arrange::clip(&self.video.video, clip).map(|c| c.scene) else { return false };
        if self.editing_clip() {
            self.back_to_video();
        }
        let Some(k) = self.video.video.scenes.get(&kept) else { return false };
        let (scene, name) = (k.scene.clone(), k.name.clone());
        self.video_stop();
        let parked = ParkedScene {
            scene: std::mem::take(&mut self.scene),
            saved: std::mem::take(&mut self.saved),
            path: self.path.take(),
            history: std::mem::take(&mut self.history),
            time: self.time,
        };
        self.leave_video_mode();
        self.replace_scene(scene, None);
        self.clip_edit = Some((ClipEdit { kept, name: name.clone() }, parked));
        self.mode = EditorMode::Build;
        self.seen_mode = EditorMode::Build;
        self.status = format!("Editing {name} for the video. Back to video puts your changes in it.");
        true
    }

    /// The video with the scene being edited put in (the video itself when none is).
    pub fn video_with_edit(&self) -> Video {
        let mut video = self.video.video.clone();
        if let Some((edit, _)) = &self.clip_edit
            && let Some(k) = video.scenes.get_mut(&edit.kept)
        {
            k.scene = self.scene.clone();
            arrange::tidy(&mut video, &self.kit.registry);
        }
        video
    }

    /// Puts the scene being edited into the video (one undo step there, if it changed), staying
    /// in Build mode.
    pub fn put_back_clip_scene(&mut self) {
        let Some((edit, _)) = &self.clip_edit else { return };
        let kept = edit.kept;
        // Nothing edited (`saved` is the kept copy as opened): nothing to put back, no undo step.
        if !self.video.video.scenes.contains_key(&kept) || self.scene == self.saved {
            return;
        }
        let scene = self.scene.clone();
        let kit = self.kit.clone();
        self.video.change(false, |v| {
            if let Some(k) = v.scenes.get_mut(&kept) {
                k.scene = scene;
            }
            arrange::tidy(v, &kit.registry);
        });
        // What was put back is now the kept copy: the scene matches it.
        self.saved = self.scene.clone();
    }

    /// Back to video: the edited scene goes into the video, the parked scene comes back to Build
    /// mode, and the editor is in Video mode.
    pub fn back_to_video(&mut self) {
        let Some((edit, _)) = &self.clip_edit else { return };
        let (kept, name) = (edit.kept, edit.name.clone());
        let gone = !self.video.video.scenes.contains_key(&kept);
        self.put_back_clip_scene();
        let Some((_, parked)) = self.clip_edit.take() else { return };
        // Saved as it was, it stays saved (whatever the editor adds to every scene it holds).
        let was_saved = parked.scene == parked.saved;
        self.replace_scene(parked.scene, parked.path);
        if !was_saved {
            self.saved = parked.saved;
        }
        self.time = parked.time.clamp(0.0, self.duration());
        self.history = parked.history;
        self.mode = EditorMode::Video;
        self.seen_mode = EditorMode::Video;
        self.entered_video_mode();
        if let Some(clip) = self.video.video.all_clips().find(|c| c.scene == kept) {
            self.video.pick = VideoPick::Clip(clip.id);
        }
        self.status = if gone {
            format!("{name} is no longer in the video, so the changes to it were left out.")
        } else {
            format!("Back to the video, with your changes to {name}.")
        };
    }
}

/// The bar across the top while a clip's scene is being edited.
pub fn bar(ui: &mut Ui, studio: &mut Studio) {
    let Some((edit, _)) = &studio.clip_edit else { return };
    let words = format!("Editing {} in the video {}.", edit.name, studio.video.name());
    let mut back = false;
    egui::Panel::top("editing a clip's scene")
        .frame(
            egui::Frame::new()
                .fill(style::RAISED)
                .inner_margin(egui::Margin::symmetric(12, 6))
                .stroke(egui::Stroke::new(1.0, style::LINE_SOFT)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(words).color(style::TEXT_STRONG));
                ui.add_space(8.0);
                back = ui
                    .add(style::primary_button("Back to video"))
                    .on_hover_text("Put the changes into the video and go back to it")
                    .clicked();
            });
        });
    if back {
        studio.back_to_video();
    }
}
