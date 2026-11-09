//! Never losing work: the autosave, the crash copy and offering work back.

use super::*;

impl Studio {
    /// Starts autosaving into `dir`, offers back the newest work an editor left there when it
    /// closed unexpectedly, and makes a crash save the unsaved scene first.
    pub fn start_recovery(&mut self, dir: &Path, crash_report: Option<PathBuf>) {
        recovery::tidy(dir);
        if let Some(orphan) = recovery::orphans(dir).into_iter().next() {
            self.offer_back(orphan);
        }
        match recovery::Session::start(dir) {
            Ok(session) => {
                if crash_report.is_some() {
                    recovery::install_crash_hook(self.crash_slot.clone(), session.files().clone(), crash_report);
                }
                self.recovery = Some(session);
            }
            Err(error) => self.report(ProblemKind::Recovery, format!("Autosave is off: {error}")),
        }
    }

    /// Reads autosaved work to offer back; a copy that can't be read is set aside and reported.
    pub(super) fn offer_back(&mut self, orphan: Orphan) {
        let loaded = orphan
            .read()
            .map_err(|e| e.to_string())
            .and_then(|text| bpe_core::load(&text, &self.kit.registry).map_err(|e| e.to_string()));
        match loaded {
            Ok(scene) => {
                let when = recovery::how_long_ago(orphan.saved_at, std::time::SystemTime::now());
                let file_newer = orphan
                    .original
                    .as_ref()
                    .and_then(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
                    .is_some_and(|saved| saved > orphan.saved_at);
                self.offer = Some(Offer { name: scene.name.clone(), scene, orphan, when, file_newer });
            }
            Err(error) => {
                orphan.set_aside();
                self.report(ProblemKind::Recovery, format!("Couldn't restore the autosaved work: {error}"));
            }
        }
    }

    /// Takes the offered work back: the recovered scene, under its own file name, still unsaved.
    pub fn restore(&mut self) {
        let Some(offer) = self.offer.take() else { return };
        let original = offer.orphan.original.clone();
        // What the scene's own file holds now, so the unsaved mark is honest.
        let on_disk = original
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|text| bpe_core::load(&text, &self.kit.registry).ok());
        self.replace_scene(offer.scene, original);
        self.saved = self.held(on_disk.unwrap_or_else(|| bpe_kit::new_studio_scene(&self.kit.registry)));
        self.unsaved_at.set(None);
        // Into this editor's own autosave first; the old copy goes only once ours is written.
        if let Some(session) = &self.recovery {
            session.write(self.scene.clone(), self.path.clone());
            session.flush();
            self.autosaved = Some(self.scene.clone());
            self.autosaved_at = Some(Instant::now());
            self.was_unsaved = true;
            if !matches!(session.outcome(), Some(Err(_))) {
                offer.orphan.discard();
            }
        }
        self.status = format!("Restored your work (saved automatically {}).", offer.when);
    }

    /// Throws the offered work away.
    pub fn discard_offer(&mut self) {
        if let Some(offer) = self.offer.take() {
            offer.orphan.discard();
            self.status = "Discarded the autosaved work.".into();
        }
    }

    /// `scene` as the editor holds every scene: with its camera and its mixer card.
    pub(super) fn held(&self, mut scene: Scene) -> Scene {
        bpe_kit::ensure_camera(&mut scene, &self.kit.registry);
        bpe_kit::ensure_mixer(&mut scene, &self.kit.registry);
        scene
    }

    /// Copies unsaved work to the recovery folder every [`recovery::AUTOSAVE_EVERY`], keeps the
    /// copy a crash would save fresh, and deletes the copy once the work is saved. Gives how long
    /// until it wants to look again.
    pub(super) fn autosave(&mut self, now: Instant) -> Option<Duration> {
        let session = self.recovery.as_ref()?;
        match session.outcome() {
            Some(Ok(())) => self.clear_problem(ProblemKind::Autosave),
            Some(Err(error)) => self.report(ProblemKind::Autosave, format!("Couldn't autosave your work: {error}")),
            None => {}
        }
        let session = self.recovery.as_ref()?;
        if !self.unsaved() {
            // A copy may exist even if the autosave didn't write it (a crash copy): always clear.
            if std::mem::take(&mut self.was_unsaved) {
                session.clear();
                self.autosaved = None;
                *self.crash_slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
                self.crash_copied_at = None;
            }
            return None;
        }
        self.was_unsaved = true;
        let mut wait: Option<Duration> = None;
        let mut soon = |d: Duration| wait = Some(wait.map_or(d, |w| w.min(d)));
        let since_copy = self.crash_copied_at.map_or(recovery::CRASH_COPY_EVERY, |t| now.duration_since(t));
        {
            let mut slot = self.crash_slot.lock().unwrap_or_else(|e| e.into_inner());
            let stale = slot.as_ref().is_none_or(|c| c.scene != self.scene || c.original != self.path);
            if stale && since_copy >= recovery::CRASH_COPY_EVERY {
                *slot = Some(CrashCopy { scene: self.scene.clone(), original: self.path.clone() });
                self.crash_copied_at = Some(now);
            } else if stale {
                soon(recovery::CRASH_COPY_EVERY - since_copy);
            }
        }
        let since_save = self.autosaved_at.map_or(recovery::AUTOSAVE_EVERY, |t| now.duration_since(t));
        if self.autosaved.as_ref() != Some(&self.scene) {
            if since_save >= recovery::AUTOSAVE_EVERY {
                session.write(self.scene.clone(), self.path.clone());
                self.autosaved = Some(self.scene.clone());
                self.autosaved_at = Some(now);
            } else {
                soon(recovery::AUTOSAVE_EVERY - since_save);
            }
        }
        // Nothing waiting to be copied: no need to wake the window.
        wait
    }
}
