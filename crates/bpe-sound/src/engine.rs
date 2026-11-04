//! The block engine.

use crate::hits::HitCursor;
use crate::{BLOCK, BlockCx, BlockHit, Master, MeterReading, Meters, SAMPLE_RATE, SoundPatch, TempoMap, VoiceSource};
use bpe_core::{Resolved, Scene};
use bpe_dsp::{FlushGuard, LoudnessMeter, gain_to_db};
use bpe_kit::Kit;
use bpe_runtime::Playback;
use std::sync::Arc;

/// Seconds of one block.
pub const SAMPLES_PER_BLOCK_TIME: f64 = BLOCK as f64 / SAMPLE_RATE as f64;
/// Seconds a seek renders silently before the point it lands on, so tails and drones are
/// already ringing there.
pub const PRE_ROLL: f64 = 4.0;
/// Frames the output queue holds: a block, the master's delay and a block to spare.
const QUEUE: usize = 512;
/// Most hits one block takes; any more wait for the next block.
const MAX_BLOCK_HITS: usize = 1024;

/// Renders a scene's sound, block after block.
pub struct Engine {
    physics: Playback,
    cursor: HitCursor,
    hits: Vec<BlockHit>,
    voices: Vec<Box<dyn VoiceSource>>,
    /// Voices whose thing went away, fading out; dropped once silent.
    leaving: Vec<Box<dyn VoiceSource>>,
    master: Master,
    tempo: TempoMap,
    loudness: LoudnessMeter,
    meters: Arc<Meters>,
    bus_l: [f32; BLOCK],
    bus_r: [f32; BLOCK],
    /// The next sample of sound time the voices render. It runs ahead of the output by the
    /// master's delay, which `discard` takes back at the start.
    voice_pos: u64,
    out_pos: u64,
    discard: usize,
    queue: [[f32; 2]; QUEUE],
    queue_head: usize,
    queue_len: usize,
    /// The physics is where the voices are. A scene with no voices runs no physics (it would
    /// only hold the playhead back); the first voice brings it up to date.
    physics_synced: bool,
    /// Where hand-played notes are in sound time while stopped (from the playhead on).
    paused_pos: u64,
}

impl Engine {
    pub fn new(scene: &Scene, resolved: &Resolved, kit: &Kit) -> Engine {
        let physics = Playback::new(scene, resolved, kit.sims.clone());
        let cursor = HitCursor::new(physics.world().steps_per_second);
        let patch = SoundPatch::build(scene, resolved, kit);
        let master = Master::new(&patch.master);
        let discard = master.latency();
        Engine {
            physics,
            cursor,
            hits: Vec::with_capacity(MAX_BLOCK_HITS),
            voices: patch.voices,
            leaving: Vec::new(),
            master,
            tempo: patch.tempo,
            loudness: LoudnessMeter::new(),
            meters: Arc::new(Meters::default()),
            bus_l: [0.0; BLOCK],
            bus_r: [0.0; BLOCK],
            voice_pos: 0,
            out_pos: 0,
            discard,
            queue: [[0.0; 2]; QUEUE],
            queue_head: 0,
            queue_len: 0,
            physics_synced: true,
            paused_pos: 0,
        }
    }

    /// Output samples rendered so far.
    pub fn sample_position(&self) -> u64 {
        self.out_pos
    }

    /// Where the next block starts, in seconds.
    pub fn time(&self) -> f64 {
        self.out_pos as f64 / SAMPLE_RATE as f64
    }

    /// The step the engine's own physics is at.
    pub fn physics_step(&self) -> u64 {
        self.physics.world().step
    }

    pub fn meters(&self) -> Arc<Meters> {
        self.meters.clone()
    }

    /// Writes the meters into `meters` from now on (the editor keeps one set across rebuilds).
    pub fn share_meters(&mut self, meters: Arc<Meters>) {
        self.meters = meters;
    }

    /// Goes to `time` (rounded down to a block): every voice and the master start over, the
    /// physics goes back to [`PRE_ROLL`] seconds earlier and the sound from there to `time` is
    /// rendered and thrown away, so tails and drones are already ringing at `time`.
    pub fn seek(&mut self, time: f64) {
        let block = BLOCK as u64;
        let rate = SAMPLE_RATE as f64;
        let target = (time.max(0.0) * rate) as u64 / block * block;
        let start = ((target as f64 / rate - PRE_ROLL).max(0.0) * rate) as u64 / block * block;
        for v in &mut self.voices {
            v.reset();
        }
        self.leaving.clear();
        self.master.reset();
        self.paused_pos = 0;
        if self.voices.is_empty() {
            // Silence needs no pre-roll and no physics.
            self.voice_pos = target;
            self.out_pos = target;
            self.discard = self.master.latency();
            self.queue_head = 0;
            self.queue_len = 0;
            self.physics_synced = false;
            self.loudness.reset();
            return;
        }
        self.voice_pos = start;
        self.sync_physics();
        self.out_pos = start;
        self.discard = self.master.latency();
        self.queue_head = 0;
        self.queue_len = 0;
        let mut scratch = [0.0f32; BLOCK * 2];
        while self.out_pos < target {
            self.render(&mut scratch);
        }
        self.loudness.reset();
    }

    pub fn tempo(&self) -> &TempoMap {
        &self.tempo
    }

    /// Takes new settings between blocks. A voice of the same thing takes over what the old one
    /// was still playing.
    pub fn swap_patch(&mut self, patch: SoundPatch) {
        self.master.set(&patch.master);
        self.tempo = patch.tempo;
        let mut old = std::mem::take(&mut self.voices);
        let mut voices = patch.voices;
        let mut kept = vec![false; old.len()];
        for v in &mut voices {
            if let Some(i) = old.iter().position(|o| o.thing() == v.thing()) {
                v.adopt(old[i].as_mut());
                kept[i] = true;
            }
        }
        // A voice whose thing has gone (or whose sound was turned off) fades out.
        for (o, kept) in old.into_iter().zip(kept) {
            if !kept {
                let mut o = o;
                o.fade_out(crate::voices::LEAVE_MS);
                if !o.is_silent() {
                    self.leaving.push(o);
                }
            }
        }
        self.voices = voices;
    }

    /// Plays `note` on `shape`'s voice from the editor (a key clicked), outside the hits.
    pub fn audition(&mut self, shape: bpe_core::ThingId, note: u8) {
        if let Some(v) = self.voices.iter_mut().find(|v| v.thing() == shape) {
            v.audition(note);
        }
    }

    /// Whether anything is sounding or still to come from voices that went away.
    pub fn has_leaving(&self) -> bool {
        !self.leaving.is_empty()
    }

    /// Whether every voice is silent (the master may still have a few milliseconds to give).
    pub fn is_quiet(&self) -> bool {
        self.leaving.is_empty() && self.voices.iter().all(|v| v.is_silent())
    }

    /// Every note stops at once (playback stopped); the next [`Engine::seek`] starts over.
    pub fn stop_notes(&mut self) {
        for v in &mut self.voices {
            v.reset();
        }
        self.leaving.clear();
        self.master.reset();
    }

    /// While stopped: the next block of the notes played by hand ([`Engine::audition`]) through
    /// the master, straight out. No hits, no physics, and the playhead stays where it is: the
    /// keys read at that moment. Never allocates.
    pub fn render_paused(&mut self, out: &mut [f32; BLOCK * 2]) {
        let _flush = FlushGuard::new();
        self.paused_pos = self.paused_pos.max(self.voice_pos);
        self.bus_l = [0.0; BLOCK];
        self.bus_r = [0.0; BLOCK];
        let cx = BlockCx {
            start_sample: self.paused_pos,
            time: self.out_pos as f64 / SAMPLE_RATE as f64,
            tempo: &self.tempo,
            world: self.physics.world(),
        };
        for v in &mut self.voices {
            v.on_block(&cx, &[], &mut self.bus_l, &mut self.bus_r);
        }
        for v in &mut self.leaving {
            v.on_block(&cx, &[], &mut self.bus_l, &mut self.bus_r);
        }
        self.master.process(&mut self.bus_l, &mut self.bus_r);
        self.paused_pos += BLOCK as u64;
        for i in 0..BLOCK {
            out[2 * i] = self.bus_l[i];
            out[2 * i + 1] = self.bus_r[i];
        }
        self.meter();
    }

    /// Renders the next block, interleaved stereo.
    pub fn render(&mut self, out: &mut [f32; BLOCK * 2]) {
        self.step_physics();
        self.render_prepared(out);
    }

    /// Voice blocks the next [`Engine::render_prepared`] will run.
    fn blocks_needed(&self) -> u64 {
        let (mut len, mut discard, mut n) = (self.queue_len, self.discard, 0);
        while len < BLOCK {
            let dropped = discard.min(BLOCK);
            discard -= dropped;
            len += BLOCK - dropped;
            n += 1;
        }
        n
    }

    /// Steps the engine's physics far enough for the next block's hits. [`Engine::render`]
    /// does this itself; it is apart so the physics, which may allocate, can be kept out of a
    /// check on the sound path.
    pub fn step_physics(&mut self) {
        // Voices that finished fading go here, where freeing them is allowed.
        self.leaving.retain(|v| !v.is_silent());
        if self.voices.is_empty() {
            self.physics_synced = false;
            return;
        }
        if !self.physics_synced {
            self.sync_physics();
        }
        let end = self.voice_pos + self.blocks_needed() * BLOCK as u64;
        let sps = self.cursor.samples_per_step();
        let target = (end as f64 / sps).ceil() as u64;
        if self.physics.world().step < target {
            let rate = self.physics.world().steps_per_second as f64;
            self.physics.seek((target as f64 + 0.5) / rate);
            self.cursor.read(self.physics.log());
        }
    }

    /// Puts the physics (and the hits waiting) at the voices' place in sound time.
    fn sync_physics(&mut self) {
        let steps = self.physics.world().steps_per_second as f64;
        let step = (self.voice_pos as f64 / self.cursor.samples_per_step()).floor();
        self.physics.seek((step + 0.5) / steps);
        self.cursor.seek(self.physics.log(), self.voice_pos);
        self.physics_synced = true;
    }

    /// Renders the next block, the physics already stepped by [`Engine::step_physics`]. Never
    /// allocates.
    pub fn render_prepared(&mut self, out: &mut [f32; BLOCK * 2]) {
        let _flush = FlushGuard::new();
        while self.queue_len < BLOCK {
            self.render_voices();
        }
        for i in 0..BLOCK {
            let [l, r] = self.queue[self.queue_head];
            self.queue_head = (self.queue_head + 1) % QUEUE;
            out[2 * i] = l;
            out[2 * i + 1] = r;
            self.bus_l[i] = l;
            self.bus_r[i] = r;
        }
        self.queue_len -= BLOCK;
        self.out_pos += BLOCK as u64;
        self.meter();
    }

    /// One block of sound time through the voices and the master, onto the queue.
    fn render_voices(&mut self) {
        self.cursor.take_block(self.voice_pos, BLOCK as u64, &mut self.hits);
        self.bus_l = [0.0; BLOCK];
        self.bus_r = [0.0; BLOCK];
        let cx = BlockCx {
            start_sample: self.voice_pos,
            time: self.voice_pos as f64 / SAMPLE_RATE as f64,
            tempo: &self.tempo,
            world: self.physics.world(),
        };
        for v in &mut self.voices {
            v.on_block(&cx, &self.hits, &mut self.bus_l, &mut self.bus_r);
        }
        for v in &mut self.leaving {
            v.on_block(&cx, &self.hits, &mut self.bus_l, &mut self.bus_r);
        }
        self.master.process(&mut self.bus_l, &mut self.bus_r);
        self.voice_pos += BLOCK as u64;
        for i in 0..BLOCK {
            if self.discard > 0 {
                self.discard -= 1;
                continue;
            }
            let at = (self.queue_head + self.queue_len) % QUEUE;
            self.queue[at] = [self.bus_l[i], self.bus_r[i]];
            self.queue_len += 1;
        }
    }

    /// Measures the block just handed out (in the buses) and writes the meters.
    fn meter(&mut self) {
        self.loudness.process(&self.bus_l, &self.bus_r);
        let peak = |b: &[f32; BLOCK]| gain_to_db(b.iter().fold(0.0f32, |m, v| m.max(v.abs())));
        self.meters.write(&MeterReading {
            peak_l: peak(&self.bus_l),
            peak_r: peak(&self.bus_r),
            momentary: self.loudness.momentary(),
            short_term: self.loudness.short_term(),
            integrated: self.loudness.integrated(),
            true_peak: self.loudness.true_peak_db(),
            reduction: self.master.gain_reduction_db(),
        });
    }
}
