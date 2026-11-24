//! Scenes in and out: new, open, save, and asking before unsaved work is lost.

use super::*;

pub(super) fn file_label(path: &Path) -> String {
    path.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

impl Studio {
    pub fn replace_scene(&mut self, mut scene: Scene, path: Option<PathBuf>) {
        // Every scene the editor holds has a camera; an older file gets one at the identity, so
        // it looks as it always did.
        bpe_kit::ensure_camera(&mut scene, &self.kit.registry);
        // And a mixer card, for its sound.
        bpe_kit::ensure_mixer(&mut scene, &self.kit.registry);
        self.saved = scene.clone();
        self.scene = scene;
        self.path = path;
        self.history = History::default();
        self.selection.clear();
        // Typing into the last scene's text ends with it.
        self.typing = None;
        // Solo is the editor's alone and was the last scene's.
        self.light_solo.clear();
        self.time = 0.0;
        self.set_playing(false);
        self.recorded = true;
        self.resolved = resolve_scene(&self.scene, &self.kit.registry);
        // A Water scene's ball engine runs an empty stand-in ([`ball_side`]).
        let none = self.no_balls.clone();
        let (ball, ball_resolved) = ball_side(&self.scene, &self.resolved, &none);
        self.fingerprint = fingerprint(ball, ball_resolved, &self.kit.sims);
        self.sound_fingerprint = sound_fingerprint(ball, ball_resolved, &self.kit.sims);
        self.patch_print = bpe_sound::patch_fingerprint(ball, ball_resolved);
        self.aim_job = aim_job_for(ball, ball_resolved, &self.kit);
        self.playback = main_playback(ball, ball_resolved, &self.kit);
        self.particle_print = bpe_runtime::beside_fingerprint(ball, ball_resolved, &self.kit.sims);
        self.particle_job = ParticleJob::start_if_wanted(ball, ball_resolved, self.kit.sims.clone(), 0.0);
        self.follow = follow_of(ball, ball_resolved);
        self.light_probe = light_probe_of(ball, ball_resolved, &self.follow);
        self.revision += 1;
        self.snapshot = None;
        self.runs_due = None;
        self.runs_things = self.scene.things.keys().copied().collect();
        self.unsaved_at.set(None);
        // The file chosen to export to was the last scene's.
        self.panels.export.path = None;
        if let Some(job) = &mut self.sound {
            job.rebuild(ball, ball_resolved);
        }
    }

    pub fn new_scene(&mut self) {
        self.replace_scene(bpe_kit::new_studio_scene(&self.kit.registry), None);
        self.status = "Started a new scene.".into();
    }

    // ---------- copying things (W5) ----------

    /// The window's title: the scene's name (the video's in Video mode), a dot while it has unsaved
    /// changes, then the app's.
    pub fn window_title(&self) -> String {
        if self.in_video_mode() {
            let dot = if self.video_unsaved() { " •" } else { "" };
            return format!("{}{dot} – Ball Studio", self.video.name());
        }
        let name = self.scene.name.trim();
        let name = if name.is_empty() { "Untitled" } else { name };
        let dot = if self.unsaved() { " •" } else { "" };
        format!("{name}{dot} – Ball Studio")
    }

    pub(crate) fn base_dir(&self) -> Option<PathBuf> {
        self.path.as_ref().and_then(|p| p.parent()).map(PathBuf::from)
    }

    /// Runs `action` now, or asks to save first if there are unsaved changes.
    pub(crate) fn guard(&mut self, action: Pending, ctx: &egui::Context) {
        if self.editing_clip() && !matches!(action, Pending::Close) {
            // A scene or video opened while a clip's scene is edited: the edit goes into the
            // video first, and the question is about the scene that was open before.
            self.back_to_video();
        }
        if self.must_ask(&action) {
            self.confirm = Some(action);
        } else {
            self.perform(action, ctx);
        }
    }

    /// Whether `action` would lose unsaved work: the video's for a video's action, the scene's for
    /// the scene's, either for closing (the scene's until it has been asked about).
    pub(crate) fn must_ask(&self, action: &Pending) -> bool {
        match action {
            Pending::NewVideo | Pending::OpenVideoPath(_) => self.video_unsaved(),
            Pending::Close => (self.unsaved() && !self.close_scene_answered) || self.video_unsaved(),
            _ => self.unsaved(),
        }
    }

    /// Whether the open question is about the video (else the scene).
    pub(crate) fn asking_about_video(&self) -> bool {
        match &self.confirm {
            Some(Pending::NewVideo | Pending::OpenVideoPath(_)) => true,
            Some(Pending::Close) => !self.unsaved() || self.close_scene_answered,
            _ => false,
        }
    }

    pub(super) fn perform(&mut self, action: Pending, ctx: &egui::Context) {
        if matches!(action, Pending::New | Pending::OpenPath(_) | Pending::OpenBuiltIn(_)) && self.in_video_mode() {
            // A scene opened from Video mode shows where it can be seen: in Build mode.
            self.mode = crate::grade_mode::EditorMode::Build;
        }
        match action {
            Pending::Close => {
                self.closing = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Pending::NewVideo => self.new_video(),
            Pending::OpenVideoPath(path) => _ = self.open_video_path(&path),
            Pending::New => self.new_scene(),
            Pending::OpenPath(path) => _ = self.open_path(&path),
            Pending::OpenBuiltIn(id) => _ = self.open_built_in(id),
        }
    }

    /// Opens the built-in scene `id` as a new project with no file yet: an ordinary scene, made fresh, that owns
    /// every setting (spec 2026-11-10-built-in-scenes). An unknown id leaves the current scene and says so.
    pub fn open_built_in(&mut self, id: &str) -> bool {
        let Some(entry) = bpe_kit::library::find(id) else {
            self.report(ProblemKind::File, format!("There is no built-in scene called {id}."));
            return false;
        };
        let scene = entry.scene(&self.kit.registry);
        self.replace_scene(scene, None);
        self.status = format!("Opened the built-in scene {}. Save it to keep your changes.", entry.info.name);
        self.clear_problem(ProblemKind::File);
        true
    }

    /// Opens the scene file at `path`. A file that can't be read leaves the current scene and
    /// says why in the status line.
    pub fn open_path(&mut self, path: &Path) -> bool {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                self.report(ProblemKind::File, format!("Couldn't open {}: {error}", path.display()));
                return false;
            }
        };
        match bpe_core::load(&text, &self.kit.registry) {
            Ok(scene) => {
                settings::remember_scene(path);
                self.replace_scene(scene, Some(path.to_path_buf()));
                self.status = format!("Opened {}.", file_label(path));
                self.clear_problem(ProblemKind::File);
                true
            }
            Err(error) => {
                self.report(ProblemKind::File, error.to_string());
                false
            }
        }
    }

    /// Saves, asking for a file name if the scene has none (or `choose` is set).
    /// Returns whether the scene was saved.
    pub fn save(&mut self, choose: bool) -> bool {
        match (&self.path, choose) {
            (Some(path), false) => self.save_to(path.clone(), false),
            _ => {
                let mut dialog = rfd::FileDialog::new()
                    .add_filter("Ball Studio scene", &["ron"])
                    .set_file_name(format!("{}.ron", self.scene.name));
                if let Some(dir) = self.base_dir() {
                    dialog = dialog.set_directory(dir);
                }
                dialog.save_file().is_some_and(|path| self.save_to(path, true))
            }
        }
    }

    /// Saves to `path`. A file name just `chosen` in the dialog becomes the scene's name too, so the name box, the
    /// "Save changes to…" question and the video's name don't go on showing the name of the file it was before.
    pub(crate) fn save_to(&mut self, path: PathBuf, chosen: bool) -> bool {
        let before = self.scene.name.clone();
        if chosen
            && let Some(name) = path.file_stem().map(|s| s.to_string_lossy().trim().to_owned())
            && !name.is_empty()
            && name != before
        {
            self.scene.name = name;
            // Its own undo step, never part of the edit made just before.
            self.separate_edit = true;
        }
        // Written beside it and swapped in, keeping the version before as "<name> (backup).ron".
        match crate::files::write_safely(&path, &bpe_core::save(&self.scene), true) {
            Ok(()) => {
                self.status = format!("Saved {}.", file_label(&path));
                self.clear_problem(ProblemKind::File);
                settings::remember_scene(&path);
                self.path = Some(path);
                self.saved = self.scene.clone();
                self.unsaved_at.set(None);
                true
            }
            Err(error) => {
                self.scene.name = before;
                self.report(ProblemKind::File, format!("Couldn't save {}: {error}", path.display()));
                false
            }
        }
    }

    // ---------- the frame ----------
}
