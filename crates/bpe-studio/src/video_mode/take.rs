//! A take of the video's sound (part 3 plan, decisions 1 to 3): the whole mix
//! ([`bpe_sound::VideoMix`], exactly as the export renders it) rendered from 0 on a thread of its
//! own into memory, as fast as it goes. The sound thread plays it ([`crate::sound_job`]'s
//! `Command::Video`), so a jump to anywhere already rendered is instant.

use bpe_core::video::Video;
use bpe_kit::Kit;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Frames (stereo samples) rendered at a time.
const CHUNK: usize = 4_800;

/// The sound's rate.
pub const RATE: usize = bpe_sound::SAMPLE_RATE as usize;

/// A video's sound, filling up as it renders.
pub struct Take {
    /// Interleaved stereo, `ready` frames of it so far.
    samples: Mutex<Vec<f32>>,
    ready: AtomicUsize,
    done: AtomicBool,
}

impl Take {
    /// Starts rendering `video`'s sound. The thread stops once nothing else holds the take.
    pub fn start(video: Video, kit: Arc<Kit>) -> Arc<Take> {
        let take = Arc::new(Take {
            samples: Mutex::new(Vec::new()),
            ready: AtomicUsize::new(0),
            done: AtomicBool::new(false),
        });
        let theirs = take.clone();
        // A liquid that pushes balls steps on the editor's card in the sound's world too.
        let card = bpe_gfx::swarm::on_this_thread();
        let spawned = std::thread::Builder::new().name("ball-studio video take".into()).spawn(move || {
            crate::thread_priority::set_this_thread(crate::thread_priority::Priority::Low);
            bpe_gfx::swarm::use_on_this_thread(card);
            theirs.render(&video, &kit);
        });
        if spawned.is_err() {
            // No thread: an empty take that is done plays silence.
            take.done.store(true, Ordering::Release);
        }
        take
    }

    /// A take of a video that makes no sound: done, and silent all through.
    pub fn silent() -> Arc<Take> {
        Arc::new(Take { samples: Mutex::new(Vec::new()), ready: AtomicUsize::new(0), done: AtomicBool::new(true) })
    }

    /// A finished take of these samples (tests).
    #[cfg(test)]
    pub fn from_samples(samples: Vec<f32>, done: bool) -> Arc<Take> {
        let frames = samples.len() / 2;
        Arc::new(Take { samples: Mutex::new(samples), ready: AtomicUsize::new(frames), done: AtomicBool::new(done) })
    }

    /// Adds samples to a take made by [`Take::from_samples`] (tests).
    #[cfg(test)]
    pub fn push(&self, more: &[f32], done: bool) {
        let mut samples = self.samples.lock().unwrap_or_else(|e| e.into_inner());
        samples.extend_from_slice(more);
        self.ready.store(samples.len() / 2, Ordering::Release);
        self.done.store(done, Ordering::Release);
    }

    fn render(self: &Arc<Self>, video: &Video, kit: &Kit) {
        let mut mix = bpe_sound::VideoMix::new(video, kit);
        let total = mix.total();
        let mut chunk = Vec::with_capacity(CHUNK * 2);
        let mut written = 0;
        while written < total {
            // Nobody wants it any more (a newer take took over, or the editor closed).
            if Arc::strong_count(self) == 1 {
                return;
            }
            chunk.clear();
            let frames = CHUNK.min(total - written);
            mix.fill(&mut chunk, frames);
            written += frames;
            let mut samples = self.samples.lock().unwrap_or_else(|e| e.into_inner());
            samples.extend_from_slice(&chunk);
            self.ready.store(samples.len() / 2, Ordering::Release);
        }
        self.done.store(true, Ordering::Release);
    }

    /// Frames rendered so far.
    pub fn ready_frames(&self) -> usize {
        self.ready.load(Ordering::Acquire)
    }

    /// The whole take is rendered.
    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }

    /// Whether it can play `frames` from `from` (or is all there is).
    pub fn holds(&self, from: usize, frames: usize) -> bool {
        self.is_done() || self.ready_frames() >= from + frames
    }

    /// Whether it has rendered past `seconds` (or is done).
    pub fn ready_to(&self, seconds: f64) -> bool {
        self.holds((seconds.max(0.0) * RATE as f64) as usize, 0)
    }

    /// Copies frames from frame `from` into `out` (interleaved stereo); gives how many frames came.
    pub fn read(&self, from: usize, out: &mut [f32]) -> usize {
        let samples = self.samples.lock().unwrap_or_else(|e| e.into_inner());
        let have = (samples.len() / 2).saturating_sub(from).min(out.len() / 2);
        if have == 0 {
            return 0;
        }
        out[..have * 2].copy_from_slice(&samples[from * 2..(from + have) * 2]);
        have
    }
}
