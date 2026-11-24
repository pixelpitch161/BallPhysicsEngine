//! The video in and out (part 2 plan, decisions 10 to 12): New video, Open (a scene or a video),
//! Save and Save as in Video mode, and the Recent list.

use super::{VideoSession, arrange};
use crate::app::{Pending, ProblemKind, Studio};
use crate::grade_mode::EditorMode;
use bpe_core::video::Video;
use bpe_core::video_save::VIDEO_EXTENSION;
use eframe::egui;
use std::path::{Path, PathBuf};

/// Whether `path` names a video file.
pub fn is_video(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(VIDEO_EXTENSION))
}

fn file_label(path: &Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

impl Studio {
    /// Whether the video differs from the one last saved or opened.
    pub fn video_unsaved(&self) -> bool {
        // A clip's scene being edited counts: it goes into the video.
        self.video.unsaved() || (self.editing_clip() && self.scene != self.saved)
    }

    /// New video, asking to save the video first if it has unsaved changes.
    pub fn ask_new_video(&mut self, ctx: &egui::Context) {
        self.guard(Pending::NewVideo, ctx);
    }

    /// Open…: a scene or a video, then the question about unsaved work the one picked would lose.
    pub fn ask_open(&mut self, ctx: &egui::Context) {
        let mut dialog = rfd::FileDialog::new()
            .add_filter("Ball Studio scene or video", &["ron", VIDEO_EXTENSION])
            .add_filter("Ball Studio scene", &["ron"])
            .add_filter("Ball Studio video", &[VIDEO_EXTENSION]);
        if let Some(dir) = self.video_dir().or_else(|| self.base_dir()) {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.ask_open_path(path, ctx);
        }
    }

    /// Opens `path` (a scene or a video), asking about the unsaved work it would replace.
    pub fn ask_open_path(&mut self, path: PathBuf, ctx: &egui::Context) {
        if is_video(&path) {
            self.guard(Pending::OpenVideoPath(path), ctx);
        } else {
            self.guard(Pending::OpenPath(path), ctx);
        }
    }

    /// A new video with the scene open in Build mode as its first clip, in Video mode.
    pub fn new_video(&mut self) {
        // A Water scene can't go in a video yet: the new video starts empty.
        let water = self.water();
        let video = if water { Video::new() } else { arrange::new_video_from(&self.scene, &self.kit.registry) };
        self.replace_video(video, None);
        self.video.saved = Video::new();
        self.mode = EditorMode::Video;
        self.status = if water {
            format!("Started a new video. {}", crate::app::NO_WATER_VIDEO)
        } else {
            "Started a new video with the open scene as its first clip.".into()
        };
    }

    /// Puts `video` (as saved at `path`) in the session in place of the one there.
    pub fn replace_video(&mut self, video: Video, path: Option<PathBuf>) {
        self.video_stop();
        let mut session = VideoSession::with(video, path);
        // The still frame's thread and whether it draws stay with the editor, and so do the
        // hits' thread and Loop.
        std::mem::swap(&mut session.still, &mut self.video.still);
        std::mem::swap(&mut session.hits, &mut self.video.hits);
        // The last video's clips' hits would land on the new one's clips (ids start again).
        session.hits.clips.clear();
        session.playing.carry_from(&self.video.playing);
        session.revision = self.video.revision + 1;
        self.video = session;
        // The file chosen to export the video to was the last video's.
        self.panels.export.video_path = None;
    }

    /// Opens the video file at `path` into Video mode. A file that can't be read leaves the
    /// video as it is and says why.
    pub fn open_video_path(&mut self, path: &Path) -> bool {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                self.report(ProblemKind::File, format!("Couldn't open {}: {error}", path.display()));
                return false;
            }
        };
        match bpe_kit::video::open_video(&text, &self.kit.registry) {
            Ok(video) => {
                crate::settings::remember_scene(path);
                self.replace_video(video, Some(path.to_path_buf()));
                self.mode = EditorMode::Video;
                self.status = format!("Opened the video {}.", file_label(path));
                self.clear_problem(ProblemKind::File);
                true
            }
            Err(error) => {
                self.report(ProblemKind::File, format!("Couldn't open {}: {error}", file_label(path)));
                false
            }
        }
    }

    /// The folder the video was saved in.
    pub(crate) fn video_dir(&self) -> Option<PathBuf> {
        self.video.path.as_ref().and_then(|p| p.parent()).map(PathBuf::from)
    }

    /// Saves the video, asking for a file name if it has none (or `choose` is set).
    pub fn save_video(&mut self, choose: bool) -> bool {
        match (&self.video.path, choose) {
            (Some(path), false) => self.save_video_to(path.clone()),
            _ => {
                let mut dialog = rfd::FileDialog::new()
                    .add_filter("Ball Studio video", &[VIDEO_EXTENSION])
                    .set_file_name(format!("{}.{VIDEO_EXTENSION}", self.video.name()));
                if let Some(dir) = self.video_dir().or_else(|| self.base_dir()) {
                    dialog = dialog.set_directory(dir);
                }
                dialog.save_file().is_some_and(|path| self.save_video_to(path))
            }
        }
    }

    /// Saves the video to `path`, keeping the version before as "<name> (backup).ballvideo".
    pub fn save_video_to(&mut self, mut path: PathBuf) -> bool {
        if path.extension().is_none() {
            path.set_extension(VIDEO_EXTENSION);
        }
        match crate::files::write_safely(&path, &bpe_core::video_save::save(&self.video.video), true) {
            Ok(()) => {
                self.status = format!("Saved the video {}.", file_label(&path));
                self.clear_problem(ProblemKind::File);
                crate::settings::remember_scene(&path);
                self.video.path = Some(path);
                self.video.saved = self.video.video.clone();
                self.video.touched();
                true
            }
            Err(error) => {
                self.report(ProblemKind::File, format!("Couldn't save {}: {error}", path.display()));
                false
            }
        }
    }

    /// Undo in Video mode.
    pub fn video_undo(&mut self) -> bool {
        let done = self.video.undo();
        self.status = if done { "Undone.".into() } else { "Nothing to undo.".into() };
        done
    }

    /// Redo in Video mode.
    pub fn video_redo(&mut self) -> bool {
        let done = self.video.redo();
        self.status = if done { "Redone.".into() } else { "Nothing to redo.".into() };
        done
    }
}
