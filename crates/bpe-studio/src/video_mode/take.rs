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
    /// Fixed chunks of [`CHUNK`] frames (the last may be short), so growing never copies what is
    /// there while the sound thread waits to read.
    samples: Mutex<Vec<Vec<f32>>>,
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
            // Liquids at their Export detail, as the export's sound renders them.
            let _export = bpe_kit::things::liquid::exporting_for_now();
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
        let take = Take { samples: Mutex::new(Vec::new()), ready: AtomicUsize::new(0), done: AtomicBool::new(done) };
        append(&mut take.samples.lock().unwrap(), &samples);
        take.ready.store(frames, Ordering::Release);
        Arc::new(take)
    }

    /// Adds samples to a take made by [`Take::from_samples`] (tests).
    #[cfg(test)]
    pub fn push(&self, more: &[f32], done: bool) {
        let mut samples = self.samples.lock().unwrap_or_else(|e| e.into_inner());
        append(&mut samples, more);
        self.ready.fetch_add(more.len() / 2, Ordering::AcqRel);
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
            append(&mut samples, &chunk);
            self.ready.fetch_add(frames, Ordering::AcqRel);
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
        let wanted = out.len() / 2;
        let mut got = 0;
        while got < wanted {
            let at = from + got;
            let Some(chunk) = samples.get(at / CHUNK) else { break };
            let inside = at % CHUNK;
            let n = (chunk.len() / 2).saturating_sub(inside).min(wanted - got);
            if n == 0 {
                break;
            }
            out[got * 2..(got + n) * 2].copy_from_slice(&chunk[inside * 2..(inside + n) * 2]);
            got += n;
        }
        got
    }
}

/// Appends interleaved `more` to `chunks`, filling the last chunk to [`CHUNK`] frames first.
fn append(chunks: &mut Vec<Vec<f32>>, mut more: &[f32]) {
    while !more.is_empty() {
        if chunks.last().is_none_or(|c| c.len() >= CHUNK * 2) {
            chunks.push(Vec::with_capacity(CHUNK * 2));
        }
        let last = chunks.last_mut().expect("just made");
        let n = (CHUNK * 2 - last.len()).min(more.len());
        last.extend_from_slice(&more[..n]);
        more = &more[n..];
    }
}
