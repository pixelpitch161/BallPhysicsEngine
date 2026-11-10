//! The Scenes window's preview sound: a sound job of its own for the scene being previewed, on the same
//! speakers, looping over the scene's preview. Its clock says which clip frame shows; with no speakers the wall
//! clock does.

use crate::audio_out::AudioOut;
use crate::sound_job::SoundJob;
use bpe_kit::Kit;
use bpe_kit::library::Entry;
use std::sync::Arc;
use std::time::Instant;

/// `clock` wrapped into `0..seconds`.
pub fn looped(clock: f64, seconds: f64) -> f64 {
    if seconds <= 0.0 || !clock.is_finite() { 0.0 } else { clock.max(0.0) % seconds }
}

#[derive(Default)]
pub struct PreviewSound {
    job: Option<SoundJob>,
    /// The scene it plays.
    id: Option<&'static str>,
    /// How long the preview is before it loops.
    seconds: f64,
    playing: bool,
    /// Sound off mutes it; the clock still runs.
    muted: bool,
    /// With no speakers: when the wall clock was at `from`.
    wall: Option<(Instant, f64)>,
    /// Where it stopped, while paused.
    at: f64,
}

impl PreviewSound {
    /// Starts previewing `entry` from its start, with its sound at `volume` if the speakers open.
    pub fn play(&mut self, entry: &'static Entry, kit: &Arc<Kit>, volume: f32) {
        let scene = entry.scene(&kit.registry);
        let resolved = bpe_core::resolve(&scene, &kit.registry);
        let job = AudioOut::open().and_then(|out| SoundJob::start(&scene, &resolved, kit.clone(), out));
        self.begin(entry, job, volume);
    }

    /// Starts previewing `entry` from its start with `job` (none: no speakers, the wall clock runs it).
    fn begin(&mut self, entry: &'static Entry, job: Option<SoundJob>, volume: f32) {
        self.stop();
        self.job = job;
        self.id = Some(entry.info.id);
        self.seconds = entry.info.preview_seconds;
        if let Some(job) = &mut self.job {
            job.shared().set_volume(volume);
            job.shared().set_muted(self.muted);
        }
        self.at = 0.0;
        self.playing = false;
        self.set_playing(true);
    }

    /// The scene being previewed.
    pub fn previewing(&self) -> Option<&'static str> {
        self.id
    }

    pub fn playing(&self) -> bool {
        self.playing
    }

    pub fn set_playing(&mut self, playing: bool) {
        if self.id.is_none() || playing == self.playing {
            return;
        }
        let at = self.clock().unwrap_or(0.0);
        self.playing = playing;
        self.at = at;
        match &mut self.job {
            Some(job) if playing => job.play(at),
            Some(job) => job.pause(at),
            None => self.wall = playing.then(|| (Instant::now(), at)),
        }
    }

    /// Mutes or unmutes it; the picture plays on either way.
    pub fn set_on(&mut self, on: bool) {
        self.muted = !on;
        if let Some(job) = &self.job {
            job.shared().set_muted(!on);
        }
    }

    /// Where the preview is, looped; `None` when nothing is previewed. Starts the sound again from the top when
    /// it passes the end.
    pub fn clock(&mut self) -> Option<f64> {
        self.id?;
        if !self.playing {
            return Some(self.at);
        }
        let raw = match (&self.job, self.wall) {
            (Some(job), _) => job.clock(),
            (None, Some((since, from))) => from + since.elapsed().as_secs_f64(),
            (None, None) => self.at,
        };
        if raw >= self.seconds {
            match &mut self.job {
                Some(job) => job.play(0.0),
                None => self.wall = Some((Instant::now(), 0.0)),
            }
            return Some(looped(raw, self.seconds));
        }
        Some(raw.max(0.0))
    }

    /// Stops and drops the sound.
    pub fn stop(&mut self) {
        self.job = None;
        self.id = None;
        self.playing = false;
        self.wall = None;
        self.at = 0.0;
    }

    #[cfg(test)]
    pub fn has_job(&self) -> bool {
        self.job.is_some()
    }

    /// Previews `entry` with a job that records what it is sent (tests).
    #[cfg(test)]
    pub fn play_recording(&mut self, entry: &'static Entry) {
        self.begin(entry, Some(SoundJob::recording()), 1.0);
    }

    #[cfg(test)]
    pub fn recorded(&self) -> Vec<&'static str> {
        self.job.as_ref().map(SoundJob::recorded).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_kit::library;

    #[test]
    fn the_clock_loops_over_the_preview() {
        assert_eq!(looped(3.0, 8.0), 3.0);
        assert_eq!(looped(9.5, 8.0), 1.5);
        assert_eq!(looped(-1.0, 8.0), 0.0);
        assert_eq!(looped(5.0, 0.0), 0.0);
        assert_eq!(looped(f64::NAN, 8.0), 0.0);
    }

    #[test]
    fn with_no_speakers_the_wall_clock_runs_it_and_pause_holds_it() {
        let entry = library::find("first-bounce").unwrap();
        let mut sound = PreviewSound::default();
        assert_eq!(sound.clock(), None);
        sound.begin(entry, None, 1.0);
        assert_eq!(sound.previewing(), Some("first-bounce"));
        assert!(sound.playing());
        std::thread::sleep(std::time::Duration::from_millis(30));
        let t = sound.clock().unwrap();
        assert!(t > 0.02 && t < entry.info.preview_seconds, "{t}");
        sound.set_playing(false);
        let held = sound.clock().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(sound.clock(), Some(held), "paused, it holds");
        sound.stop();
        assert_eq!(sound.clock(), None);
    }

    #[test]
    fn stop_drops_the_job() {
        let mut sound = PreviewSound::default();
        sound.play_recording(library::find("first-bounce").unwrap());
        assert!(sound.has_job());
        assert!(sound.recorded().contains(&"play"), "{:?}", sound.recorded());
        sound.set_on(false);
        sound.stop();
        assert!(!sound.has_job());
        assert_eq!(sound.previewing(), None);
    }
}
