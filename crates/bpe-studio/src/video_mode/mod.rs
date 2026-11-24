//! Video mode (video arrangement part 2, `docs/notes/video.md`): the editor's third face, beside
//! Build and Grade, where scenes are laid out as clips on a song's bar grid and exported as one
//! video. The editor keeps one [`VideoSession`] beside its scene; Video mode never touches the
//! scene Build mode edits.
//!
//! - `arrange.rs`: every edit to the video, as plain functions that keep it inside its limits;
//! - `files.rs`: new, open and save of the video; `scenes.rs`: the kept scenes on the left;
//! - `inspect.rs`: the Video, Clip, Transition and Scene inspectors on the right;
//! - `lanes.rs`: the arrangement at the bottom (ruler, Scenes lane, Song lane, drags);
//! - `still.rs`: the still frame at the playhead in the middle, drawn on a thread of its own.

use crate::app::Studio;
use bpe_core::video::{Clip, ClipId, KeptId, Video};
use eframe::egui::{self, Ui};
use std::path::PathBuf;

pub mod arrange;
pub mod edit;
pub mod files;
pub mod hits;
pub mod inspect;
pub mod lanes;
pub mod play;
pub mod scenes;
pub mod still;
pub mod take;
#[cfg(test)]
mod tests;

/// How many steps the video's undo keeps: a video holds whole scenes, so they are few.
pub const VIDEO_UNDO_LIMIT: usize = 64;

/// What the inspector shows in Video mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum VideoPick {
    /// The video's own settings.
    #[default]
    Video,
    Clip(ClipId),
    /// The transition into this clip from the one before.
    Transition(ClipId),
    Scene(KeptId),
}

/// The video's own undo and redo: whole videos as they were.
#[derive(Default)]
pub struct VideoHistory {
    undo: Vec<Video>,
    redo: Vec<Video>,
    /// The edits since the pointer went down are one step.
    merging: bool,
}

impl VideoHistory {
    /// Records `before` as an undo step (or, while `merge` and already merging, keeps the step
    /// begun earlier), and forgets the redo steps.
    pub fn record(&mut self, before: Video, merge: bool) {
        if !(merge && self.merging) {
            self.undo.push(before);
            if self.undo.len() > VIDEO_UNDO_LIMIT {
                self.undo.remove(0);
            }
        }
        self.merging = merge;
        self.redo.clear();
    }

    /// Ends the step being merged (the pointer was let go).
    pub fn end_merging(&mut self) {
        self.merging = false;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Puts `video` back as it was a step ago. False with nothing to undo.
    pub fn undo(&mut self, video: &mut Video) -> bool {
        let Some(before) = self.undo.pop() else { return false };
        self.redo.push(std::mem::replace(video, before));
        self.merging = false;
        true
    }

    pub fn redo(&mut self, video: &mut Video) -> bool {
        let Some(after) = self.redo.pop() else { return false };
        self.undo.push(std::mem::replace(video, after));
        self.merging = false;
        true
    }
}

/// Where the arrangement looks: its zoom and scroll, and where it drew its lanes last frame (so
/// tests and drops can find a time's place).
#[derive(Clone, Copy, Debug, Default)]
pub struct ArrangeView {
    /// Points a second; 0 fits the whole video on the next frame.
    pub px_per_second: f32,
    /// The video time at the lanes' left edge.
    pub left: f64,
    /// The Scenes lane's strip (right of the lane names) as last drawn.
    pub lane: Option<egui::Rect>,
    /// The ruler's strip as last drawn.
    pub ruler: Option<egui::Rect>,
}

impl ArrangeView {
    /// The x of video time `t` in the lanes' strip starting at `left_x`.
    pub fn x_of(&self, left_x: f32, t: f64) -> f32 {
        left_x + ((t - self.left) * f64::from(self.px_per_second)) as f32
    }

    /// The video time at `x` in the lanes' strip starting at `left_x`.
    pub fn time_of(&self, left_x: f32, x: f32) -> f64 {
        self.left + f64::from(x - left_x) / f64::from(self.px_per_second.max(1e-3))
    }
}

/// The video Video mode edits, and everything Video mode remembers about it.
pub struct VideoSession {
    pub video: Video,
    /// The video as last saved or opened.
    pub saved: Video,
    pub path: Option<PathBuf>,
    pub history: VideoHistory,
    pub pick: VideoPick,
    /// The playhead, in video seconds.
    pub time: f64,
    pub view: ArrangeView,
    /// A clip being dragged in the arrangement.
    pub drag: Option<lanes::ClipDrag>,
    /// The clip Ctrl+C copied (the editor's own).
    pub clip_board: Option<Clip>,
    /// The preview's still frame.
    pub still: still::Still,
    /// Goes up whenever the video changes, so the still frame knows it is out of date.
    pub revision: u64,
    /// Playing the video (part 3).
    pub playing: play::Playing,
    /// Dragging the ruler paused the playing; it plays again when let go.
    pub resume_after_scrub: bool,
    /// The Scene sound lane's hits (part 3).
    pub hits: hits::Hits,
    /// Whether the video differs from the saved one, as worked out at a revision.
    unsaved_at: std::cell::Cell<Option<(u64, bool)>>,
}

impl Default for VideoSession {
    fn default() -> Self {
        Self::with(Video::new(), None)
    }
}

impl VideoSession {
    /// A session on `video` (as opened from `path`, which it equals).
    pub fn with(video: Video, path: Option<PathBuf>) -> Self {
        VideoSession {
            saved: video.clone(),
            video,
            path,
            history: VideoHistory::default(),
            pick: VideoPick::Video,
            time: 0.0,
            view: ArrangeView::default(),
            drag: None,
            clip_board: None,
            still: still::Still::default(),
            revision: 0,
            playing: play::Playing::default(),
            resume_after_scrub: false,
            hits: hits::Hits::default(),
            unsaved_at: std::cell::Cell::new(None),
        }
    }

    /// Whether the video differs from the saved one (worked out once a revision).
    pub fn unsaved(&self) -> bool {
        if let Some((revision, unsaved)) = self.unsaved_at.get()
            && revision == self.revision
        {
            return unsaved;
        }
        let unsaved = self.video != self.saved;
        self.unsaved_at.set(Some((self.revision, unsaved)));
        unsaved
    }

    /// The video's name: its file's, or "Untitled video".
    pub fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled video".into())
    }

    /// Something changed the video outside an edit (open, undo, a drag's frame).
    pub fn touched(&mut self) {
        self.revision += 1;
    }

    /// Changes the video with `f` as one undo step (merged with the edits before it while `merge`:
    /// the pointer is held on the same control). Gives whether anything changed.
    pub fn change(&mut self, merge: bool, f: impl FnOnce(&mut Video)) -> bool {
        let before = self.video.clone();
        f(&mut self.video);
        if self.video == before {
            return false;
        }
        self.history.record(before, merge);
        self.touched();
        true
    }

    /// Records `before` (the video as a gesture began) as one undo step, if the video changed.
    pub fn commit(&mut self, before: Video) {
        if self.video != before {
            self.history.record(before, false);
            self.touched();
        }
    }

    pub fn undo(&mut self) -> bool {
        let done = self.history.undo(&mut self.video);
        if done {
            self.drag = None;
            self.touched();
        }
        done
    }

    pub fn redo(&mut self) -> bool {
        let done = self.history.redo(&mut self.video);
        if done {
            self.drag = None;
            self.touched();
        }
        done
    }

    /// What is picked, or the Video when the picked clip, transition or scene is gone.
    pub fn pick(&self) -> VideoPick {
        match self.pick {
            VideoPick::Clip(id) if arrange::clip(&self.video, id).is_some() => self.pick,
            VideoPick::Transition(id)
                if arrange::index_of(&self.video, id).is_some_and(|i| self.video.overlap(i) > 0.0) =>
            {
                self.pick
            }
            VideoPick::Scene(id) if self.video.scenes.contains_key(&id) => self.pick,
            _ => VideoPick::Video,
        }
    }

    /// The playhead as a whole video frame.
    pub fn frame(&self) -> u32 {
        self.video.frames(self.time).max(0) as u32
    }

    /// Moves the playhead (never before 0).
    pub fn seek(&mut self, time: f64) {
        self.time = time.max(0.0);
    }
}

/// The left column in Video mode: the scenes kept in the video.
pub fn left(ui: &mut Ui, studio: &mut Studio) {
    egui::Panel::left("video scenes")
        .default_size(crate::style::TREE_W)
        .min_size(180.0)
        .max_size(480.0)
        .resizable(true)
        .frame(crate::app::side_frame().inner_margin(egui::Margin::symmetric(8, 10)))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            scenes::show(ui, studio);
        });
}

/// The right column in Video mode: the inspector of what is picked.
pub fn right(ui: &mut Ui, studio: &mut Studio) {
    egui::Panel::right("video inspector")
        .default_size(crate::settings::INSPECTOR_DEFAULT)
        .min_size(260.0)
        .max_size(560.0)
        .resizable(true)
        .frame(crate::app::side_frame())
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            egui::ScrollArea::vertical().id_salt("video inspector").show(ui, |ui| inspect::show(ui, studio));
        });
}

/// The arrangement at the bottom in Video mode.
pub fn bottom(ui: &mut Ui, studio: &mut Studio) {
    egui::Panel::bottom("arrangement")
        .frame(
            egui::Frame::new()
                .fill(crate::style::PANEL)
                .inner_margin(egui::Margin { left: 10, right: 10, top: 4, bottom: 6 })
                .stroke(egui::Stroke::new(1.0, crate::style::LINE_SOFT)),
        )
        .resizable(true)
        .default_size(230.0)
        .min_size(150.0)
        .max_size(520.0)
        .show(ui, |ui| {
            ui.set_min_height(ui.available_height());
            lanes::show(ui, studio);
        });
}

/// The middle in Video mode: the still frame at the playhead.
pub fn centre(ui: &mut Ui, studio: &mut Studio) {
    still::show(ui, studio);
}

impl Studio {
    /// Video mode's keys, read first in the frame (after Save, Open and New): undo and redo of the
    /// video, the playhead's keys, and Space (play and stop).
    pub(crate) fn video_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let pressed = |modifiers, key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(modifiers, key)));
        if pressed(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z) || pressed(Modifiers::COMMAND, Key::Y) {
            self.video_redo();
        }
        if pressed(Modifiers::COMMAND, Key::Z) {
            self.video_undo();
        }
        if ctx.input(|i| i.key_pressed(Key::Space)) {
            self.video_toggle_play();
        }
        if ctx.memory(|m| m.focused().is_some()) {
            return;
        }
        let session = &mut self.video;
        if ctx.input(|i| i.key_pressed(Key::Home)) {
            session.seek(0.0);
        }
        if ctx.input(|i| i.key_pressed(Key::End)) {
            session.seek(session.video.end());
        }
        let beat = session.video.settings.beat_seconds();
        let frame = session.video.seconds(1);
        let step = |session: &mut VideoSession, by: f64| {
            let t = session.video.seconds(session.video.frames(session.time + by));
            session.seek(t);
        };
        if pressed(Modifiers::SHIFT, Key::ArrowLeft) {
            step(session, -beat);
        } else if pressed(Modifiers::NONE, Key::ArrowLeft) {
            step(session, -frame);
        }
        if pressed(Modifiers::SHIFT, Key::ArrowRight) {
            step(session, beat);
        } else if pressed(Modifiers::NONE, Key::ArrowRight) {
            step(session, frame);
        }
    }

    /// Video mode's keys read after the panels: Delete, copy, paste and duplicate of the picked
    /// clip, each its own undo step.
    pub(crate) fn video_late_shortcuts(&mut self, ctx: &egui::Context, copy: bool, paste: bool) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let pressed = |modifiers, key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(modifiers, key)));
        let kit = self.kit.clone();
        let reg = &kit.registry;
        let picked = match self.video.pick() {
            VideoPick::Clip(id) => Some(id),
            _ => None,
        };
        if let Some(id) = picked
            && pressed(Modifiers::NONE, Key::Delete)
        {
            self.video.change(false, |v| _ = arrange::delete_clip(v, id));
            self.status = "Deleted the clip.".into();
        }
        if let Some(id) = picked
            && pressed(Modifiers::COMMAND, Key::D)
        {
            let mut made = None;
            self.video.change(false, |v| made = arrange::duplicate_clip(v, reg, id));
            if let Some(made) = made {
                self.video.pick = VideoPick::Clip(made);
            }
        }
        if copy && let Some(clip) = picked.and_then(|id| arrange::clip(&self.video.video, id)) {
            self.video.clip_board = Some(clip.clone());
            self.status = "Copied the clip.".into();
            self.copied_note = Some("Ball Studio: Copied a clip".into());
        }
        if paste && let Some(clip) = self.video.clip_board.clone() {
            let start = arrange::snap(self.video.time, &self.video.video.settings, false);
            let mut made = None;
            self.video.change(false, |v| made = arrange::paste_clip(v, reg, &clip, start));
            match made {
                Some(made) => self.video.pick = VideoPick::Clip(made),
                None => self.status = "The copied clip's scene is no longer in the video.".into(),
            }
        }
    }
}
