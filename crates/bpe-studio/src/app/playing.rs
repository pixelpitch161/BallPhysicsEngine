//! The playhead and the sound: play, seek, step, the speakers and auditions.

use super::*;

/// How often missing speakers are looked for again.
pub(super) const SPEAKERS_RETRY: Duration = Duration::from_secs(2);

/// The longest wait between looks for speakers (each look waits twice the last).
pub(super) const SPEAKERS_RETRY_MOST: Duration = Duration::from_secs(30);

impl Studio {
    /// Steps the playhead `n` video frames (with `beat`, `n` beats) from the frame (or beat) it
    /// is on, and pauses.
    pub fn step(&mut self, n: f64, beat: bool) {
        let settings = self.settings();
        let unit = if beat {
            bpe_sound::TempoMap::from_settings(&settings).beat_seconds()
        } else {
            1.0 / f64::from(settings.fps.max(1))
        };
        if !(unit.is_finite() && unit > 0.0) {
            return;
        }
        let at = (self.time / unit).round();
        self.set_playing(false);
        self.seek((at + n) * unit);
    }

    /// Moves the playhead. The preview catches the simulation up, one budget a frame: seeking
    /// itself steps nothing, so a scrub spends the frame's catch-up time once, not twice.
    pub fn seek(&mut self, time: f64) {
        self.time = time.clamp(0.0, self.duration());
        if self.playing
            && let Some(job) = &mut self.sound
        {
            job.seek(self.time);
        }
    }

    /// Steps the playback all the way to the playhead (tests; the editor catches up a budget a
    /// frame in the preview).
    #[cfg(test)]
    pub fn catch_up(&mut self) {
        let target = self.sim_target();
        self.playback.seek(target);
    }

    pub fn set_playing(&mut self, playing: bool) {
        if playing && self.time >= self.duration() {
            self.time = 0.0;
        }
        if let Some(job) = &mut self.sound
            && playing != self.playing
        {
            if playing { job.play(self.time) } else { job.pause(self.time) }
        }
        self.playing = playing;
        self.last_tick = None;
    }

    /// Sets the speaker button and listening volume, and remembers them.
    /// Shows the timeline or the rules in the bottom panel, and remembers it for next time.
    pub fn set_bottom(&mut self, view: settings::BottomView) {
        if self.bottom != view {
            self.bottom = view;
            if self.persist_settings {
                settings::save_bottom_view(view);
            }
        }
    }

    pub fn set_listen(&mut self, listen: ListenSettings) {
        if listen == self.listen {
            return;
        }
        self.listen = listen;
        self.apply_listen();
        if self.persist_settings {
            self.listen_unsaved = Some(Instant::now());
        }
    }

    /// Saves the listening settings once they have rested half a second, and on the way out.
    pub(super) fn save_listen_if_rested(&mut self, now: Option<Instant>) {
        if let Some(changed) = self.listen_unsaved
            && now.is_none_or(|t| t.duration_since(changed) >= Duration::from_millis(500))
        {
            settings::save_listen(self.listen);
            self.listen_unsaved = None;
        }
    }

    /// The speakers: after they fail (unplugged, or the default output changed) or when there
    /// were none at the start, they are looked for again every two seconds, and the sound picks
    /// up where the playhead is.
    pub(super) fn keep_speakers(&mut self, now: Instant) {
        if !self.persist_settings {
            // Tests (and anything without a window) never open a device.
            return;
        }
        if let Some(job) = &self.sound
            && job.failed()
        {
            if job.device_failed() {
                self.report(ProblemKind::Speakers, "The speakers stopped working. Looking for them again…");
            } else {
                // A sound thread that stopped by itself would stop again: no looping restarts.
                self.report(
                    ProblemKind::Speakers,
                    "The sound stopped working. Save, then close and reopen Ball Studio.",
                );
                self.sound_broken = true;
            }
            self.sound = None;
            self.speakers_wait = SPEAKERS_RETRY;
            self.speakers_retry = Some(now + SPEAKERS_RETRY);
        }
        if self.sound.is_some() || self.sound_broken {
            return;
        }
        let due = self.speakers_retry.get_or_insert(now + self.speakers_wait);
        if now < *due {
            return;
        }
        // Opening a device can take a moment: look less and less often while there is none.
        self.speakers_wait = (self.speakers_wait * 2).min(SPEAKERS_RETRY_MOST);
        *due = now + self.speakers_wait;
        let Some(mut job) =
            AudioOut::open().and_then(|out| SoundJob::start(&self.scene, &self.resolved, self.kit.clone(), out))
        else {
            return;
        };
        if self.playing {
            job.play(self.time);
        } else {
            // Keys clicked while stopped read the settings at the playhead.
            job.seek(self.time);
        }
        self.sound = Some(job);
        self.apply_listen();
        self.speakers_retry = None;
        self.speakers_wait = SPEAKERS_RETRY;
        self.status = "The speakers are back.".into();
        self.clear_problem(ProblemKind::Speakers);
    }

    /// Says so in the status line when the sound couldn't keep up with the speakers.
    pub(super) fn note_dropouts(&mut self) {
        let Some(job) = &self.sound else { return };
        let underruns = job.shared().underruns();
        if underruns > self.underruns_seen {
            self.underruns_seen = underruns;
            self.status = "Sound can't keep up: try fewer voices or effects.".into();
        } else if underruns < self.underruns_seen {
            // New speakers count from 0.
            self.underruns_seen = underruns;
        }
    }

    pub(super) fn apply_listen(&self) {
        if let Some(job) = &self.sound {
            job.shared().set_volume(self.listen.volume);
            job.shared().set_muted(!self.listen.on);
            job.shared().set_speaker_delay_ms(self.listen.speaker_delay_ms);
        }
    }

    /// Moves the playhead while playing: with the sound the speakers have played, or with the
    /// wall clock when there are no speakers.
    pub(super) fn tick(&mut self) {
        let now = Instant::now();
        let dt = self.last_tick.replace(now).map_or(0.0, |last| (now - last).as_secs_f64().min(0.25));
        self.save_listen_if_rested(Some(now));
        self.keep_speakers(now);
        self.note_dropouts();
        self.tell_gpu_errors();
        self.tell_export_news();
        if let Some(wait) = self.autosave(now)
            && let Some(ctx) = &self.repaint
        {
            ctx.request_repaint_after(wait);
        }
        if !self.playing {
            return;
        }
        let duration = self.duration();
        let next = match &self.sound {
            Some(job) => job.clock(),
            None => self.time + dt,
        };
        if next < duration {
            self.time = next;
        } else if self.looping {
            self.time = 0.0;
            if let Some(job) = &mut self.sound {
                job.seek(0.0);
            }
        } else {
            self.time = duration;
            self.set_playing(false);
        }
    }

    // ---------- files ----------

    /// Plays the notes clicked on a Sound tab's keyboard, after this frame's edit has reached
    /// the sound (a key that was just turned on plays with its new note list).
    /// When wavetables finish baking in the background, the sound is built again with them
    /// (until then the oscillators that need them are silent). Looks again shortly while a bake
    /// is under way.
    pub(super) fn follow_bakes(&mut self, ctx: &egui::Context) {
        // A wavetable baked or a sound file read: the patch that was waiting on it is built again.
        let baked = WaveBank::baked() + bpe_sound::SampleBank::decoded();
        if baked != self.tables_baked {
            self.tables_baked = baked;
            if let Some(job) = &mut self.sound {
                job.patch(SoundPatch::build_ready(&self.scene, &self.resolved, &self.kit));
            }
        }
        if WaveBank::busy() || bpe_sound::SampleBank::busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    pub(super) fn send_auditions(&mut self) {
        let auditions = std::mem::take(&mut self.panels.inspector.auditions);
        if let Some(job) = &mut self.sound
            && !auditions.is_empty()
        {
            // While stopped, a key sounds as the sound is at the playhead.
            job.park(self.time);
            for (thing, note) in auditions {
                job.audition(crate::sound_job::Audition { thing, note, velocity: 0.8, on: true });
            }
        }
    }
}
