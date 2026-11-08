//! The block engine.

use crate::buses::{Buses, ShapeRoute};
use crate::hits::HitCursor;
use crate::lag::Lag;
use crate::{BLOCK, BlockCx, BlockHit, Master, MeterReading, Meters, SAMPLE_RATE, SoundPatch, TempoMap, VoiceSource};
use bpe_core::{Resolved, Scene};
use bpe_dsp::{FlushGuard, LoudnessMeter, gain_to_db};
use bpe_kit::Kit;
use bpe_kit::sound::BUSES;
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
    /// Where each voice's sound goes, by the voice's place.
    routes: Vec<ShapeRoute>,
    /// Voices whose thing went away, fading out; dropped once silent. With their routes.
    leaving: Vec<Box<dyn VoiceSource>>,
    leaving_routes: Vec<ShapeRoute>,
    buses: Buses,
    /// One voice's block before it goes to its buses.
    voice_l: [f32; BLOCK],
    voice_r: [f32; BLOCK],
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
    /// Each bus's sound beside the queue, while stems are recorded.
    record_stems: bool,
    /// Noting the notes each shape starts (the export's MIDI file).
    record_notes: bool,
    note_log: crate::NoteLog,
    noted: Vec<crate::NoteOn>,
    stem_queue: Box<[[[f32; 2]; BUSES]; QUEUE]>,
    /// Line the stems up with the master's output (the limiter's lookahead).
    stem_lags: [Lag; BUSES],
    stems_out: [[f32; BLOCK * 2]; BUSES],
    /// The physics is where the voices are. A scene with no voices runs no physics (it would
    /// only hold the playhead back); the first voice brings it up to date.
    physics_synced: bool,
    /// Where hand-played notes are in sound time while stopped (from the playhead on).
    paused_pos: u64,
}

impl Engine {
    /// The engine for `scene`, its wavetables baked if need be.
    pub fn new(scene: &Scene, resolved: &Resolved, kit: &Kit) -> Engine {
        Self::with_patch(scene, resolved, kit, SoundPatch::build(scene, resolved, kit))
    }

    /// The engine for `scene` playing `patch` (built from the same scene).
    pub fn with_patch(scene: &Scene, resolved: &Resolved, kit: &Kit, patch: SoundPatch) -> Engine {
        // Things stepped beside the physics make no sound, but those whose events it hears (a
        // Particles thing whose sound is on): the engine runs the physics and those.
        let physics = Playback::new(scene, &bpe_runtime::heard_part(scene, resolved, &kit.sims), kit.sims.clone());
        let cursor = HitCursor::new(physics.world().steps_per_second);
        let mut master = Master::new(&patch.master);
        master.set_keys(patch.master_keys);
        master.at(0.0, None);
        master.reset();
        let buses = Buses::new(patch.buses);
        let discard = master.latency() + buses.latency();
        Engine {
            physics,
            cursor,
            hits: Vec::with_capacity(MAX_BLOCK_HITS),
            voices: patch.voices,
            routes: patch.routes,
            leaving: Vec::new(),
            leaving_routes: Vec::new(),
            buses,
            voice_l: [0.0; BLOCK],
            voice_r: [0.0; BLOCK],
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
            record_stems: false,
            record_notes: false,
            note_log: crate::NoteLog::default(),
            noted: Vec::new(),
            stem_queue: Box::new([[[0.0; 2]; BUSES]; QUEUE]),
            stem_lags: std::array::from_fn(|_| Lag::new()),
            stems_out: [[0.0; BLOCK * 2]; BUSES],
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

    /// Where the voices have got to (ahead of [`Engine::time`] by the master's delay): the
    /// moment the meters' modulator readings are from.
    pub fn voice_time(&self) -> f64 {
        self.voice_pos as f64 / SAMPLE_RATE as f64
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
        self.meters.keep_fx(&self.buses.fx_layers());
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
        self.leaving_routes.clear();
        // The master's keyed gains start where they are at the first block rendered.
        let first = if self.voices.is_empty() { target } else { start };
        self.master.at(first as f64 / rate, None);
        self.master.reset();
        self.buses.reset(first as f64 / rate);
        for r in &mut self.routes {
            r.at(first as f64 / rate, None);
            r.snap();
        }
        self.paused_pos = 0;
        if self.voices.is_empty() {
            // Silence needs no pre-roll and no physics.
            self.voice_pos = target;
            self.out_pos = target;
            self.discard = self.latency();
            self.queue_head = 0;
            self.queue_len = 0;
            self.physics_synced = false;
            self.loudness.reset();
            return;
        }
        for lag in &mut self.stem_lags {
            lag.reset();
        }
        self.voice_pos = start;
        self.sync_physics();
        self.catch_up(start);
        self.out_pos = start;
        self.discard = self.latency();
        self.queue_head = 0;
        self.queue_len = 0;
        let mut scratch = [0.0f32; BLOCK * 2];
        while self.out_pos < target {
            self.render(&mut scratch);
        }
        self.loudness.reset();
    }

    /// Starts or stops noting every note the shapes start, for a MIDI file (the export only:
    /// noting allocates).
    pub fn record_notes(&mut self, on: bool) {
        self.record_notes = on;
        for v in &mut self.voices {
            v.record_notes(on);
        }
    }

    /// The notes noted so far.
    pub fn note_log(&self) -> &crate::NoteLog {
        &self.note_log
    }

    /// Keeps each bus's sound from now on, lined up with the mix ([`Engine::stems`]). For the
    /// export; the editor never records.
    pub fn record_stems(&mut self, on: bool) {
        self.record_stems = on;
        for lag in &mut self.stem_lags {
            lag.set(self.master.latency());
            lag.reset();
        }
    }

    /// Each bus's sound (interleaved stereo) in the block [`Engine::render`] last handed out,
    /// after its fader, mute and solo: the three add up to the mix before the master. Silence
    /// unless [`Engine::record_stems`] is on.
    pub fn stems(&self) -> &[[f32; BLOCK * 2]; BUSES] {
        &self.stems_out
    }

    /// Frames from a voice to the speakers: the buses' and the master's delays.
    fn latency(&self) -> usize {
        self.master.latency() + self.buses.latency()
    }

    /// Every voice counts the clip's hits before sound sample `upto` (the physics has run that
    /// far), so slots and play orders are where playing from the start would have put them.
    fn catch_up(&mut self, upto: u64) {
        let all = vec![true; self.voices.len()];
        self.catch_up_these(upto, &all);
    }

    /// [`Engine::catch_up`] for the voices whose place in `these` is true.
    fn catch_up_these(&mut self, upto: u64, these: &[bool]) {
        if upto == 0 || !these.contains(&true) {
            return;
        }
        let sps = self.cursor.samples_per_step();
        let mut hits: Vec<(u64, BlockHit)> = self
            .physics
            .log()
            .events()
            .iter()
            .filter(|e| crate::hits::plays(&e.kind))
            .map(|e| (crate::hits::hit_sample(e, sps), e))
            .filter(|(at, _)| *at < upto)
            .map(|(at, e)| (at, BlockHit { offset: 0, event: e.clone() }))
            .collect();
        hits.sort_by_key(|(at, _)| *at);
        let world = self.physics.world();
        for (v, _) in self.voices.iter_mut().zip(these).filter(|(_, on)| **on) {
            v.catch_up(&hits, upto, &self.tempo, world);
        }
    }

    pub fn tempo(&self) -> &TempoMap {
        &self.tempo
    }

    /// Takes new settings between blocks. A voice of the same thing takes over what the old one
    /// was still playing.
    pub fn swap_patch(&mut self, patch: SoundPatch) {
        self.master.set(&patch.master);
        self.master.set_keys(patch.master_keys);
        self.tempo = patch.tempo;
        let before = self.latency();
        let mut buses = Buses::new(patch.buses);
        buses.adopt(&mut self.buses);
        self.buses = buses;
        // Effects that went free their meters (a new one may reuse a layer id).
        self.meters.keep_fx(&self.buses.fx_layers());
        // A limiter added or taken away changes how late the sound comes out: skip ahead or
        // wait so the sound stays on time (the editor only; an export never swaps).
        let after = self.latency();
        if after > before {
            self.discard += after - before;
        } else {
            for _ in 0..(before - after).min(QUEUE - self.queue_len) {
                let at = (self.queue_head + self.queue_len) % QUEUE;
                self.queue[at] = [0.0; 2];
                self.queue_len += 1;
            }
        }
        let mut old = std::mem::take(&mut self.voices);
        let mut old_routes = std::mem::take(&mut self.routes);
        let mut voices = patch.voices;
        let mut routes = patch.routes;
        let mut kept = vec![false; old.len()];
        let mut fresh = vec![true; voices.len()];
        for ((v, r), fresh) in voices.iter_mut().zip(routes.iter_mut()).zip(&mut fresh) {
            if let Some(i) = old.iter().position(|o| o.thing() == v.thing()) {
                v.adopt(old[i].as_mut());
                r.adopt(&old_routes[i]);
                kept[i] = true;
                *fresh = false;
            }
        }
        // A voice whose thing has gone (or whose sound was turned off) fades out.
        for ((o, r), kept) in old.drain(..).zip(old_routes.drain(..)).zip(kept) {
            if !kept {
                let mut o = o;
                o.fade_out(crate::voices::LEAVE_MS);
                if !o.is_silent() {
                    self.leaving.push(o);
                    self.leaving_routes.push(r);
                }
            }
        }
        self.voices = voices;
        self.routes = routes;
        // A voice new in the middle of the clip (sound just turned on, or pasted) counts the hits
        // so far, as a seek would, so its slot and play order are where playing from the start
        // puts them (and where the timeline's slot marks say).
        if self.voice_pos > 0 && fresh.contains(&true) {
            if !self.physics_synced {
                self.sync_physics();
            }
            self.catch_up_these(self.voice_pos, &fresh);
        }
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
        self.leaving_routes.clear();
        self.master.reset();
        self.buses.reset(self.out_pos as f64 / SAMPLE_RATE as f64);
    }

    /// While stopped: the next block of the notes played by hand ([`Engine::audition`]) through
    /// the master, straight out. No hits, no physics, and the playhead stays where it is: the
    /// keys read at that moment. Never allocates.
    pub fn render_paused(&mut self, out: &mut [f32; BLOCK * 2]) {
        let _flush = FlushGuard::new();
        self.paused_pos = self.paused_pos.max(self.voice_pos);
        let time = self.out_pos as f64 / SAMPLE_RATE as f64;
        self.run_voices(self.paused_pos, time, false);
        self.master.at(time, self.physics.world().rules());
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
        let mut i = 0;
        while i < self.leaving.len() {
            if self.leaving[i].is_silent() {
                self.leaving.remove(i);
                self.leaving_routes.remove(i);
            } else {
                i += 1;
            }
        }
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
            if self.record_stems {
                for (b, stem) in self.stem_queue[self.queue_head].iter().enumerate() {
                    self.stems_out[b][2 * i] = stem[0];
                    self.stems_out[b][2 * i + 1] = stem[1];
                }
            }
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
        let time = self.voice_pos as f64 / SAMPLE_RATE as f64;
        self.run_voices(self.voice_pos, time, true);
        self.master.at(time, self.physics.world().rules());
        self.master.process(&mut self.bus_l, &mut self.bus_r);
        self.voice_pos += BLOCK as u64;
        let mut stems = [[[0.0f32; BLOCK]; 2]; BUSES];
        if self.record_stems {
            stems = *self.buses.stems();
            for (stem, lag) in stems.iter_mut().zip(&mut self.stem_lags) {
                let [l, r] = stem;
                lag.process(l, r);
            }
        }
        #[allow(clippy::needless_range_loop)]
        for i in 0..BLOCK {
            if self.discard > 0 {
                self.discard -= 1;
                continue;
            }
            let at = (self.queue_head + self.queue_len) % QUEUE;
            self.queue[at] = [self.bus_l[i], self.bus_r[i]];
            if self.record_stems {
                for (slot, stem) in self.stem_queue[at].iter_mut().zip(&stems) {
                    *slot = [stem[0][i], stem[1][i]];
                }
            }
            self.queue_len += 1;
        }
    }

    /// Every voice's block into its buses, and the buses into the master's input (`bus_l`,
    /// `bus_r`). With `hits` false the voices hear no hits (stopped). Never allocates.
    fn run_voices(&mut self, start_sample: u64, time: f64, hits: bool) {
        let playing = hits;
        let hits = if hits { &self.hits[..] } else { &[] };
        // Stopped (no hits): only the keys clicked by hand sound.
        let cx = BlockCx {
            start_sample,
            time,
            tempo: &self.tempo,
            world: self.physics.world(),
            log: self.physics.log(),
            playing,
        };
        self.buses.begin(time, cx.world.rules());
        let voices = self.voices.iter_mut().zip(self.routes.iter_mut());
        let leaving = self.leaving.iter_mut().zip(self.leaving_routes.iter_mut());
        for (v, route) in voices.chain(leaving) {
            self.voice_l = [0.0; BLOCK];
            self.voice_r = [0.0; BLOCK];
            v.on_block(&cx, hits, &mut self.voice_l, &mut self.voice_r);
            if self.record_notes {
                v.take_notes(&mut self.noted);
                let thing = v.thing();
                for n in self.noted.drain(..) {
                    self.note_log.push(thing, n);
                }
            }
            route.at(time, cx.world.rules());
            route.send(&mut self.buses, &self.voice_l, &self.voice_r);
        }
        self.buses.process(&mut self.bus_l, &mut self.bus_r, time, &self.tempo, hits, &self.meters, cx.world.rules());
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
        for (i, v) in self.voices.iter().enumerate() {
            self.meters.write_shape(i, v.thing().0, gain_to_db(v.level()));
            self.meters.write_slot(i, v.playing_slot());
            self.meters.write_notes(i, v.sounding_notes());
            self.meters.write_mods(i, &mut |f| v.each_mod(f));
            let (recent, heard) = v.recent_hits();
            self.meters.write_hits(i, recent, heard);
        }
        self.meters.clear_shapes_from(self.voices.len());
    }
}
