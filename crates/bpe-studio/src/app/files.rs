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
        self.fingerprint = fingerprint(&self.scene, &self.resolved, &self.kit.sims);
        self.sound_fingerprint = sound_fingerprint(&self.scene, &self.resolved, &self.kit.sims);
        self.patch_print = bpe_sound::patch_fingerprint(&self.scene, &self.resolved);
        self.aim_job = aim_job_for(&self.scene, &self.resolved, &self.kit);
        self.playback = main_playback(&self.scene, &self.resolved, &self.kit);
        self.particle_print = bpe_runtime::beside_fingerprint(&self.scene, &self.resolved, &self.kit.sims);
        self.particle_job = ParticleJob::start_if_wanted(&self.scene, &self.resolved, self.kit.sims.clone(), 0.0);
        self.follow = follow_of(&self.scene, &self.resolved);
        self.light_probe = light_probe_of(&self.scene, &self.resolved, &self.follow);
        self.revision += 1;
        self.snapshot = None;
        self.runs_due = None;
        self.runs_things = self.scene.things.keys().copied().collect();
        self.unsaved_at.set(None);
        // The file chosen to export to was the last scene's.
        self.panels.export.path = None;
        if let Some(job) = &mut self.sound {
            job.rebuild(&self.scene, &self.resolved);
        }
    }

    pub fn new_scene(&mut self) {
        self.replace_scene(bpe_kit::new_studio_scene(&self.kit.registry), None);
        self.status = "Started a new scene.".into();
    }

    // ---------- copying things (W5) ----------

    /// The window's title: the scene's name, a dot while it has unsaved changes, then the app's.
    pub fn window_title(&self) -> String {
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
        if self.unsaved() {
            self.confirm = Some(action);
        } else {
            self.perform(action, ctx);
        }
    }

    pub(super) fn perform(&mut self, action: Pending, ctx: &egui::Context) {
        match action {
            Pending::Close => {
                self.closing = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Pending::Open => self.open(),
            Pending::New => self.new_scene(),
            Pending::OpenPath(path) => _ = self.open_path(&path),
        }
    }

    pub fn open(&mut self) {
        let mut dialog = rfd::FileDialog::new().add_filter("Ball Studio scene", &["ron"]);
        if let Some(dir) = self.base_dir() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(path) = dialog.pick_file() {
            self.open_path(&path);
        }
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
        let path = match (&self.path, choose) {
            (Some(path), false) => Some(path.clone()),
            _ => {
                let mut dialog = rfd::FileDialog::new()
                    .add_filter("Ball Studio scene", &["ron"])
                    .set_file_name(format!("{}.ron", self.scene.name));
                if let Some(dir) = self.base_dir() {
                    dialog = dialog.set_directory(dir);
                }
                dialog.save_file()
            }
        };
        let Some(path) = path else { return false };
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
                self.report(ProblemKind::File, format!("Couldn't save {}: {error}", path.display()));
                false
            }
        }
    }

    // ---------- the frame ----------
}
