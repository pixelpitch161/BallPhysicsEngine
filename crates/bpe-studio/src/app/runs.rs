//! Edits and the runs they restart: undo steps, fingerprints, and the physics, aim, particle
//! and sound runs built from the scene.

use super::*;

/// The background aiming `scene` wants, started.
pub(super) fn aim_job_for(scene: &Scene, resolved: &Resolved, kit: &Kit) -> Option<AimJob> {
    let duration = resolved.things.get(&ThingId::ROOT).map_or(0.0, |r| SceneSettings::from_params(&r.params).duration);
    // Particles never change the aim: the aiming runs the physics alone.
    let main = bpe_runtime::main_part(scene, resolved, &kit.sims);
    AimJob::start_if_wanted(scene, &main, kit.sims.clone(), duration.max(0.0) as f64)
}

/// The editor's playback of `scene`: the physics alone, without the things stepped beside it
/// (their own playback is [`ParticleJob`]'s).
pub(super) fn main_playback(scene: &Scene, resolved: &Resolved, kit: &Kit) -> Playback {
    Playback::new(scene, &bpe_runtime::main_part(scene, resolved, &kit.sims), kit.sims.clone())
}

/// The scene's sound for the picture, when any setting follows it.
pub(super) fn follow_of(
    scene: &Scene,
    resolved: &bpe_core::Resolved,
) -> Option<std::sync::Arc<bpe_sound::ControlSampler>> {
    bpe_sound::follows_sound(scene).then(|| std::sync::Arc::new(bpe_sound::ControlSampler::new(scene, resolved)))
}

/// The light probe the picture reads while something in `scene` senses light (lights that follow
/// sound heard through `follow`).
pub(super) fn light_probe_of(
    scene: &Scene,
    resolved: &bpe_core::Resolved,
    follow: &Option<std::sync::Arc<bpe_sound::ControlSampler>>,
) -> Option<std::sync::Arc<dyn bpe_runtime::LightAt>> {
    let sound = follow.clone().map(|f| f as std::sync::Arc<dyn bpe_runtime::SoundAt>);
    bpe_kit::sensing::uses_light(scene).then(|| bpe_kit::sensing::probe(scene, resolved, sound))
}

/// How many particles the main world's (two-way) Particles things have and how long their last
/// step took, or `None` when it has none.
pub(super) fn main_particles(world: &bpe_runtime::World) -> Option<(usize, f32)> {
    use bpe_kit::things::particles::state::ParticlesState;
    let mut found = None;
    for state in world.states.values() {
        if let Some(p) = (**state).as_any().downcast_ref::<ParticlesState>() {
            let (count, ms) = found.get_or_insert((0, 0.0));
            *count += p.specks.len();
            *ms += p.last_ms;
        }
    }
    found
}

impl Studio {
    /// Puts a new thing in the middle of whatever the preview is looking at. A thing stuck to
    /// the screen goes in the middle of the picture, which is where the camera isn't.
    pub fn place_added(&mut self, id: ThingId) {
        let stuck = self
            .scene
            .things
            .get(&id)
            .is_some_and(|t| self.kit.registry.thing(&t.kind).is_some_and(|k| t.sticks_to_screen(k)));
        if let Some(thing) = self.scene.things.get_mut(&id)
            && !thing.rides()
        {
            thing.transform.position = if stuck { glam::Vec2::ZERO } else { self.view_centre.round() };
        }
    }

    /// Records the edit that turned `before` into the scene as it is now, unless nothing
    /// changed, and brings the simulation along.
    #[cfg(test)]
    pub fn after_edit(&mut self, before: Scene) {
        self.edited(before, false);
    }

    /// [`after_edit`](Self::after_edit) for an edit that is always its own undo step.
    pub fn after_separate_edit(&mut self, before: Scene) {
        self.edited(before, true);
    }

    pub(super) fn edited(&mut self, before: Scene, separate: bool) {
        if self.scene == before {
            return;
        }
        self.record_change(before, separate);
        // Made outside the frame's own recording: the next frame starts from the scene as it is.
        self.snapshot = None;
    }

    /// Records the change from `before` (known to differ) and brings the simulation along.
    pub(super) fn record_change(&mut self, before: Scene, separate: bool) {
        if std::mem::take(&mut self.panels.inspector.own_step) {
            self.history.end_merging();
        }
        self.history.record(before, separate);
        self.scene_changed();
    }

    /// Re-resolves after any change. The playback starts over only when the physics changed:
    /// a new colour keeps the balls where they are.
    pub(crate) fn scene_changed(&mut self) {
        self.resolved = resolve_scene(&self.scene, &self.kit.registry);
        self.follow = follow_of(&self.scene, &self.resolved);
        self.light_probe = light_probe_of(&self.scene, &self.resolved, &self.follow);
        self.time = self.time.min(self.duration());
        self.revision += 1;
        self.restart_runs();
    }

    /// Starts over the runs the scene as it is now changes: the physics (and its aiming), the
    /// particle playback, the sound. While [`Studio::hold_runs`] is on they are left as they are
    /// and noted due ([`Studio::runs_due`]): the preview shows the old run drawn with the new
    /// looks until the drag is let go or rests ([`Studio::settle_runs`]).
    pub(super) fn restart_runs(&mut self) {
        let fingerprint = fingerprint(&self.scene, &self.resolved, &self.kit.sims);
        let particle_print = bpe_runtime::beside_fingerprint(&self.scene, &self.resolved, &self.kit.sims);
        let heard = sound_fingerprint(&self.scene, &self.resolved, &self.kit.sims);
        let rebuilt = fingerprint != self.fingerprint;
        let particles = particle_print != self.particle_print;
        let sound_rebuilt = heard != self.sound_fingerprint;
        let same_things = self.scene.things.keys().eq(self.runs_things.iter());
        if self.hold_runs && same_things && (rebuilt || particles || sound_rebuilt) {
            self.runs_due = Some(Instant::now());
            return;
        }
        self.runs_due = None;
        if rebuilt || particles || sound_rebuilt {
            self.runs_things = self.scene.things.keys().copied().collect();
        }
        if rebuilt {
            self.fingerprint = fingerprint;
            self.aim_job = aim_job_for(&self.scene, &self.resolved, &self.kit);
            self.playback = main_playback(&self.scene, &self.resolved, &self.kit);
        }
        // A particle edit starts the particle playback over, and nothing else.
        if particles {
            self.particle_print = particle_print;
            self.particle_job =
                ParticleJob::start_if_wanted(&self.scene, &self.resolved, self.kit.sims.clone(), self.time);
        }
        // The sound follows: a physics change starts its world over (no hits from the old
        // layout), anything else swaps the settings in where it is. How much of the balls' past
        // trails keep is the editor's alone: the sound plays on through it.
        self.sound_fingerprint = heard;
        let patch_print = bpe_sound::patch_fingerprint(&self.scene, &self.resolved);
        let patch_changed = patch_print != self.patch_print;
        self.patch_print = patch_print;
        if let Some(job) = &mut self.sound {
            if sound_rebuilt {
                job.rebuild(&self.scene, &self.resolved);
                if self.playing {
                    job.seek(self.time);
                }
            } else if patch_changed {
                job.patch(SoundPatch::build_ready(&self.scene, &self.resolved, &self.kit));
            }
        }
    }

    /// Starts the held runs over once the drag that changed them is let go (`pointer_down`
    /// false) or has rested [`RUNS_REST`]. Returns how long to look again in, while they wait.
    pub(crate) fn settle_runs(&mut self, pointer_down: bool) -> Option<Duration> {
        let since = self.runs_due?;
        let rested = since.elapsed();
        if pointer_down && self.hold_runs && rested < RUNS_REST {
            return Some(RUNS_REST - rested);
        }
        let hold = std::mem::replace(&mut self.hold_runs, false);
        self.restart_runs();
        self.revision += 1;
        self.hold_runs = hold && pointer_down;
        None
    }

    /// Gives the inspector the selected emitter's rounds as the playback has them so far, and
    /// what its aim should warn about.
    pub(super) fn fill_rounds(&mut self) {
        let rounds = &mut self.panels.inspector.rounds;
        rounds.clear();
        self.panels.inspector.aim_checks.clear();
        let Some(id) = self
            .selection
            .primary
            .filter(|id| self.scene.things.get(id).is_some_and(|t| t.kind == bpe_kit::things::emitter::KIND))
        else {
            return;
        };
        let duration =
            bpe_kit::SceneSettings::from_params(&self.scene.things[&bpe_core::ThingId::ROOT].params).duration;
        rounds.insert(id, bpe_kit::things::emitter::sim::round_marks(self.playback.world(), id, duration as f64));
        if let Some(check) = bpe_kit::things::emitter::aim::check(self.playback.world(), id) {
            self.panels.inspector.aim_checks.insert(id, check);
        }
    }

    // ---------- playback ----------

    /// Records this frame's change to the scene (`before` is how it started), unless it is in the
    /// history already: as its own undo step when it asked to be. A thing selected takes the
    /// inspector back from a rule block.
    pub(crate) fn record_frame(&mut self, before: Scene) {
        let recorded = std::mem::take(&mut self.recorded);
        let separate = std::mem::take(&mut self.separate_edit);
        if self.scene == before {
            self.snapshot = Some(before);
        } else {
            if !recorded {
                self.record_change(before, separate);
            }
            // Changed in place this frame: whether it is unsaved is worked out afresh.
            self.unsaved_at.set(None);
            self.snapshot = Some(self.scene.clone());
        }
        self.after_frame();
    }

    /// What every frame does after its edit is recorded.
    pub(super) fn after_frame(&mut self) {
        if self.selection.primary.is_some() {
            self.panels.rules.picked = None;
        }
    }
}
