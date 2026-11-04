//! A shape's voice: hits on the shape play notes through two oscillators, their envelopes and
//! the shape's output.
//!
//! Each oscillator: wave → Gain (smoothed, scaled by its routes) → its chain → Level → pan. The
//! two add up, then the Output's Volume and Pan (pulled towards where the hit was by Follow
//! hit). Every knob can be keyed on the timeline (read each block) and every gain is smoothed,
//! so nothing clicks.

// The sample loops walk several buffers in step by index, which reads clearest as it is.
#![allow(clippy::needless_range_loop)]

use super::alloc::{CAPACITY, MAX_VOICES, STEAL_MS, Voices};
use super::hit_filter::HitFilter;
use super::knobs::{self as k, KNOBS, MAX_ENVS, Values};
use super::notes::{NotePicker, PickCx, mix};
use super::pick::AnyPick;
use super::slots::{NotesSpec, SlotClock};
use crate::chain::{Buf, ChainSpec, VoiceChain};
use crate::modmatrix::{Combine, ModMatrix};
use crate::{BLOCK, BlockCx, BlockHit, VoiceSource};
use bpe_core::{LayerId, NoteSlot, ShapePath, ThingId, Track};
use bpe_dsp::{EnvParams, Envelope, Noise, Smoother, SubOsc, SubWaveId, TableSet, UnisonOsc, UnisonParams, db_to_gain};
use bpe_kit::sound::{GateMode, GateUnit, NoiseColour, PhaseMode, PlayMode, SILENT_DB, SubOctave, SubWave, Wave};
use bpe_runtime::Party;
use glam::Vec2;
use std::any::Any;
use std::sync::Arc;

/// How hard an audition (a key clicked in the editor) plays.
pub const AUDITION_STRENGTH: f32 = 0.8;
/// How long a shape that went away (deleted, or its sound turned off) takes to fade out.
pub const LEAVE_MS: f32 = 10.0;
/// The most hits one note takes in one block (more are let go).
const MAX_EVENTS: usize = 8;
/// A note with no envelope on its levels lasts this long.
const PLAIN_NOTE_S: f32 = 0.1;
/// The ball an audition counts as.
const AUDITION_BALL: Party = Party { thing: ThingId::ROOT, item: u32::MAX };
/// Auditions waiting for the next block.
const MAX_AUDITIONS: usize = 8;
/// Strummed notes waiting for their start.
const MAX_STRUMS: usize = 64;

/// A strummed note still to start.
#[derive(Clone, Copy, Debug)]
struct Strum {
    sample: u64,
    note: u8,
    strength: f32,
    ball: Party,
    point: Vec2,
    seed: u64,
}

/// One oscillator's settings that are not knobs.
#[derive(Clone, Debug)]
pub struct OscSpec {
    pub on: bool,
    pub wave: Wave,
    pub noise: NoiseColour,
    pub phase_mode: PhaseMode,
    pub sub: SubWave,
    pub sub_octave: SubOctave,
    /// The wave's tables (none for Noise or an oscillator that is off).
    pub table: Option<Arc<TableSet>>,
}

/// One envelope layer's settings that are not knobs.
#[derive(Clone, Copy, Debug)]
pub struct EnvSpec {
    pub id: LayerId,
    pub gate_mode: GateMode,
    pub gate_unit: GateUnit,
}

/// Everything a shape's voice is built from.
pub struct ShapeSpec {
    pub thing: ThingId,
    /// Its place among the scene's sounding shapes (for its meter).
    pub index: usize,
    pub osc: [OscSpec; 2],
    pub envs: Vec<EnvSpec>,
    pub play_mode: PlayMode,
    pub heard_from: bpe_core::Target,
    pub slots: Vec<NoteSlot>,
    /// How hits pick notes and how the slots take turns.
    pub notes: NotesSpec,
    /// The shape's outline in shape units, for By side (`None` otherwise).
    pub side_path: Option<ShapePath>,
    pub bus: u8,
    /// The knobs as set.
    pub base: Values,
    /// The knobs with keys, by place.
    pub keyed: Vec<(usize, Track)>,
    pub matrix: ModMatrix,
    /// The band-limited square the subs read (`None` with no sub, or while it bakes).
    pub square: Option<Arc<TableSet>>,
    /// The picture's width in scene pixels (Follow hit pans across it).
    pub width: f32,
    /// Both oscillators' layer chains.
    pub chain: Arc<ChainSpec>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Ev {
    Trigger(f32),
    Release,
}

/// One sounding note.
#[derive(Clone, Debug)]
struct Note {
    note: u8,
    strength: f32,
    ball: Party,
    osc: [UnisonOsc; 2],
    sub: [SubOsc; 2],
    noise: [Noise; 2],
    chain: VoiceChain,
    env: [Envelope; MAX_ENVS],
    env_ids: [LayerId; MAX_ENVS],
    /// Samples until each envelope's gate closes (`u64::MAX`: not by time).
    gate: [u64; MAX_ENVS],
    /// The fade every note ends with (1 while it plays).
    fade: f32,
    fade_step: f32,
    /// Where the note starts in the block it starts in (0 after that).
    start_at: u32,
    events: [(u32, usize, Ev); MAX_EVENTS],
    n_events: usize,
    /// The note's pitch now (MIDI numbers, fractional while it glides), where it glides from
    /// and to, and samples of glide left.
    pitch: f32,
    glide_from: f32,
    glide_to: f32,
    glide_left: u32,
    glide_len: u32,
    /// Where across the picture its hit was, -1..1.
    across: f32,
    warp_prev: [f32; 2],
    /// Samples the note has played.
    age: u64,
}

impl Note {
    fn new(chain: &ChainSpec) -> Self {
        Note {
            note: 60,
            strength: 1.0,
            ball: AUDITION_BALL,
            osc: [UnisonOsc::new(), UnisonOsc::new()],
            sub: [SubOsc::default(), SubOsc::default()],
            noise: [Noise::new(0), Noise::new(1)],
            chain: VoiceChain::new(chain),
            env: std::array::from_fn(|_| Envelope::new()),
            env_ids: [LayerId(u32::MAX); MAX_ENVS],
            gate: [u64::MAX; MAX_ENVS],
            fade: 1.0,
            fade_step: 0.0,
            start_at: 0,
            events: [(0, 0, Ev::Release); MAX_EVENTS],
            n_events: 0,
            pitch: 60.0,
            glide_from: 60.0,
            glide_to: 60.0,
            glide_left: 0,
            glide_len: 0,
            across: 0.0,
            warp_prev: [0.0; 2],
            age: 0,
        }
    }

    fn push(&mut self, at: u32, env: usize, ev: Ev) {
        if self.n_events < MAX_EVENTS {
            self.events[self.n_events] = (at, env, ev);
            self.n_events += 1;
        }
    }

    /// Starts fading out over `ms`, from `at` in this block (an earlier fade keeps going).
    fn fade_out(&mut self, ms: f32) {
        let step = 1.0 / (ms / 1000.0 * crate::SAMPLE_RATE).max(1.0);
        if self.fade_step < step {
            self.fade_step = step;
        }
    }
}

/// Per-sample buffers the voice reuses.
struct Scratch {
    env: [[f32; BLOCK]; MAX_ENVS],
    gain: [f32; BLOCK],
    sub_gain: [f32; BLOCK],
    warp: [f32; BLOCK],
    fade: [f32; BLOCK],
    note_l: [f32; BLOCK],
    note_r: [f32; BLOCK],
    shape_l: [f32; BLOCK],
    shape_r: [f32; BLOCK],
    /// The shape's smoothed knobs, sample by sample: Gain, Level, Sub level of each oscillator,
    /// then Volume (linear gains).
    smooth: [[f32; BLOCK]; 7],
    /// The oscillators' sound reaching their chains, then each chain layer's sound.
    lanes: Vec<Buf>,
}

impl Scratch {
    fn new(buffers: usize) -> Box<Self> {
        Box::new(Scratch {
            env: [[0.0; BLOCK]; MAX_ENVS],
            gain: [0.0; BLOCK],
            sub_gain: [0.0; BLOCK],
            warp: [0.0; BLOCK],
            fade: [0.0; BLOCK],
            note_l: [0.0; BLOCK],
            note_r: [0.0; BLOCK],
            shape_l: [0.0; BLOCK],
            shape_r: [0.0; BLOCK],
            smooth: [[0.0; BLOCK]; 7],
            lanes: vec![[[0.0; BLOCK]; 2]; buffers.max(crate::chain::RAW_BUFFERS)],
        })
    }
}

/// A dB knob's gain, with the bottom of its range silent.
fn knob_gain(db: f32) -> f32 {
    if db <= SILENT_DB + 1e-3 { 0.0 } else { db_to_gain(db) }
}

/// Equal-power pan gains (each 1 in the middle).
fn pan_gains(pan: f32) -> (f32, f32) {
    let theta = (pan.clamp(-1.0, 1.0) + 1.0) * std::f32::consts::FRAC_PI_4;
    (theta.cos() * std::f32::consts::SQRT_2, theta.sin() * std::f32::consts::SQRT_2)
}

/// The frequency of MIDI note `pitch` (69 is A at 440 Hz; FL calls 60 C5).
pub fn note_hz(pitch: f32) -> f32 {
    440.0 * ((pitch - 69.0) / 12.0).exp2()
}

/// A shape's voice.
pub struct ShapeVoice {
    spec: ShapeSpec,
    filter: HitFilter,
    picker: AnyPick,
    clock: SlotClock,
    /// Accepted hits since the slot began (the play order's step).
    slot_hits: u64,
    /// The slot's turn those hits count from.
    slot_turn: u64,
    strums: [Strum; MAX_STRUMS],
    n_strums: usize,
    notes: Voices<Note>,
    /// The knobs this block (keys read).
    now: Values,
    /// Smoothed: Gain, Level, Sub level of each oscillator, then Volume; and the pans.
    smooth: [Smoother; 7],
    pans: [Smoother; 3],
    /// Which envelope each of the matrix's sources is (`usize::MAX` for one this voice lacks).
    source_env: [usize; MAX_ENVS],
    /// The envelopes that scale a level: a note ends when they are done.
    level_envs: [bool; MAX_ENVS],
    /// Whether each of Gain, Level, Sub level (per oscillator) and Volume has routes.
    routed: [bool; 7],
    /// Whether each oscillator runs: it is on, or the other's layers tap it.
    plays: [bool; 2],
    scratch: Box<Scratch>,
    /// Fading out because the shape went away (1 while it plays).
    leaving: f32,
    leave_step: f32,
    auditions: [u8; MAX_AUDITIONS],
    n_auditions: usize,
    audition_count: u64,
    /// The loudest sample of the last block.
    peak: f32,
}

/// The smoothed knobs' places: per oscillator Gain, Level, Sub level, then Volume.
const SMOOTHED: [usize; 7] = [
    k::osc(0, k::GAIN),
    k::osc(0, k::LEVEL),
    k::osc(0, k::SUB_LEVEL),
    k::osc(1, k::GAIN),
    k::osc(1, k::LEVEL),
    k::osc(1, k::SUB_LEVEL),
    k::VOLUME,
];

impl ShapeVoice {
    /// Builds the voice (allocates: off the audio path).
    pub fn new(spec: ShapeSpec) -> ShapeVoice {
        let now = spec.base;
        let filter = HitFilter::new(
            spec.thing,
            now[k::MIN_STRENGTH] / 100.0,
            now[k::RETRIGGER] / 1000.0,
            spec.heard_from.clone(),
        );
        let mut source_env = [usize::MAX; MAX_ENVS];
        for (s, id) in spec.matrix.sources().iter().enumerate().take(MAX_ENVS) {
            if let Some(e) = spec.envs.iter().take(MAX_ENVS).position(|x| x.id == *id) {
                source_env[s] = e;
            }
        }
        let mut level_envs = [false; MAX_ENVS];
        let mut routed = [false; 7];
        for (r, &target) in SMOOTHED.iter().enumerate() {
            routed[r] = spec.matrix.reaches(target as u16);
        }
        // An envelope that scales any level decides when the note is over.
        for &target in &SMOOTHED {
            for (s, &e) in source_env.iter().enumerate() {
                if e != usize::MAX && spec.matrix.route_from(s as u16, target as u16, Combine::Scale) {
                    level_envs[e] = true;
                }
            }
        }
        let mut v = ShapeVoice {
            filter,
            picker: AnyPick::new(spec.notes.plays),
            clock: SlotClock::new(spec.slots.len(), super::slots::clock_seed(spec.thing)),
            slot_hits: 0,
            slot_turn: 0,
            strums: [Strum { sample: 0, note: 0, strength: 0.0, ball: AUDITION_BALL, point: Vec2::ZERO, seed: 0 };
                MAX_STRUMS],
            n_strums: 0,
            notes: Voices::new(|| Note::new(&spec.chain)),
            now,
            smooth: std::array::from_fn(|_| Smoother::new(Smoother::KNOB_MS)),
            pans: std::array::from_fn(|_| Smoother::new(Smoother::KNOB_MS)),
            source_env,
            level_envs,
            routed,
            plays: std::array::from_fn(|o| spec.osc[o].on || spec.chain.raw_needed[o]),
            scratch: Scratch::new(spec.chain.buffers()),
            leaving: 1.0,
            leave_step: 0.0,
            auditions: [0; MAX_AUDITIONS],
            n_auditions: 0,
            audition_count: 0,
            peak: 0.0,
            spec,
        };
        v.read_keys(0.0);
        v.snap_smoothers();
        v
    }

    pub fn spec(&self) -> &ShapeSpec {
        &self.spec
    }

    /// Notes sounding now (not fading).
    pub fn sounding(&self) -> usize {
        self.notes.sounding()
    }

    /// The slot playing now, 0 first.
    pub fn slot(&self) -> usize {
        self.clock.current()
    }

    /// Whether the slot changed during the last block.
    pub fn slot_changed(&self) -> bool {
        self.clock.changed()
    }

    /// The notes sounding now (not fading), lowest first. For tests and the editor; allocates.
    pub fn playing_notes(&self) -> Vec<u8> {
        let mut v: Vec<u8> = (0..CAPACITY)
            .filter(|&i| self.notes.meta[i].active && !self.notes.meta[i].fading)
            .map(|i| self.notes.slots[i].note)
            .collect();
        v.sort_unstable();
        v
    }

    /// The loudest sample of the last block (for the shape's meter).
    pub fn peak(&self) -> f32 {
        self.peak
    }

    /// The knobs at `time`: as set, with keyed ones read from their tracks.
    fn read_keys(&mut self, time: f64) {
        self.now = self.spec.base;
        for (i, track) in &self.spec.keyed {
            if let Some(v) = k::track_value(track, time) {
                self.now[*i] = v;
            }
        }
    }

    fn targets(&self) -> [f32; 10] {
        let n = &self.now;
        let mut t = [0.0; 10];
        for (i, &place) in SMOOTHED.iter().enumerate() {
            t[i] = knob_gain(n[place]);
        }
        t[7] = n[k::osc(0, k::PAN)] / 100.0;
        t[8] = n[k::osc(1, k::PAN)] / 100.0;
        t[9] = n[k::OUT_PAN] / 100.0;
        t
    }

    /// Every smoother where its knob is, with nothing to glide.
    fn snap_smoothers(&mut self) {
        let t = self.targets();
        for (i, s) in self.smooth.iter_mut().enumerate() {
            s.reset(t[i]);
        }
        for (i, s) in self.pans.iter_mut().enumerate() {
            s.reset(t[7 + i]);
        }
    }

    /// Clicks a key in the editor: the note plays on the next block.
    pub fn audition(&mut self, note: u8) {
        if self.n_auditions < MAX_AUDITIONS {
            self.auditions[self.n_auditions] = note;
            self.n_auditions += 1;
        }
    }

    /// The envelope settings of envelope `e` now.
    fn env_params(&self, e: usize) -> EnvParams {
        let n = &self.now;
        let at = |key: usize| n[k::env(e, key)];
        EnvParams {
            delay: at(k::DELAY) / 1000.0,
            attack: at(k::ATTACK) / 1000.0,
            hold: at(k::HOLD) / 1000.0,
            decay: at(k::DECAY) / 1000.0,
            sustain: at(k::SUSTAIN) / 100.0,
            release: at(k::RELEASE) / 1000.0,
            curves: [at(k::ATTACK_CURVE) / 100.0, at(k::DECAY_CURVE) / 100.0, at(k::RELEASE_CURVE) / 100.0],
        }
    }

    /// Samples envelope `e`'s gate stays open for.
    fn gate_samples(&self, e: usize, tempo: &crate::TempoMap) -> u64 {
        let spec = &self.spec.envs[e];
        match (spec.gate_mode, spec.gate_unit) {
            (GateMode::UntilNextHit, _) => u64::MAX,
            (GateMode::Fixed, GateUnit::Ms) => {
                (self.now[k::env(e, k::GATE_MS)].max(0.0) / 1000.0 * crate::SAMPLE_RATE) as u64
            }
            (GateMode::Fixed, GateUnit::Beats) => {
                let s = tempo.seconds_of_beats(self.now[k::env(e, k::GATE_BEATS)].max(0.0) as f64);
                (s * crate::SAMPLE_RATE as f64) as u64
            }
        }
    }

    fn envs(&self) -> usize {
        self.spec.envs.len().min(MAX_ENVS)
    }

    /// A note starts at `at` in this block: on a fresh slot (Poly) or the one voice (Mono).
    #[allow(clippy::too_many_arguments)]
    fn start_note(&mut self, cx: &BlockCx, at: u32, note: u8, strength: f32, ball: Party, point: Vec2, seed: u64) {
        let sample = cx.start_sample + at as u64;
        let across = if self.spec.width > 0.0 { (point.x / (self.spec.width * 0.5)).clamp(-1.0, 1.0) } else { 0.0 };
        let mono = self.spec.play_mode == PlayMode::Mono;
        if mono && self.notes.meta[0].active && !self.notes.meta[0].fading {
            // Mono with a note still sounding: the envelopes start again from where they are
            // and the pitch glides there.
            let glide = (self.now[k::GLIDE].max(0.0) / 1000.0 * crate::SAMPLE_RATE) as u32;
            let envs = self.envs();
            let gates: [u64; MAX_ENVS] =
                std::array::from_fn(|e| if e < envs { self.gate_samples(e, cx.tempo) } else { u64::MAX });
            let n = &mut self.notes.slots[0];
            n.note = note;
            n.strength = strength;
            n.ball = ball;
            n.across = across;
            if glide > 0 {
                n.glide_from = n.pitch;
                n.glide_to = note as f32;
                n.glide_len = glide;
                n.glide_left = glide;
            } else {
                n.pitch = note as f32;
                n.glide_left = 0;
            }
            for e in 0..envs {
                n.push(at, e, Ev::Trigger(strength));
                n.gate[e] = gates[e];
            }
            self.notes.meta[0].started = sample;
            return;
        }
        let (slot, steal) = if mono {
            if self.notes.meta[0].active {
                self.notes.slots[0].fade = 0.0;
            }
            self.notes.meta[0] = super::alloc::Meta { active: true, fading: false, started: sample };
            (0, None)
        } else {
            let limit = (self.now[k::VOICES].round() as usize).clamp(1, MAX_VOICES);
            let Some(s) = self.notes.start(limit, sample, cx.start_sample) else { return };
            (s.slot, s.steal)
        };
        if let Some(s) = steal {
            self.notes.slots[s].fade_out(STEAL_MS);
        }
        let envs = self.envs();
        let gates: [u64; MAX_ENVS] =
            std::array::from_fn(|e| if e < envs { self.gate_samples(e, cx.tempo) } else { u64::MAX });
        let params: [EnvParams; MAX_ENVS] =
            std::array::from_fn(|e| if e < envs { self.env_params(e) } else { EnvParams::default() });
        let unison = self.unison_params(note as f32);
        let osc_on = self.plays;
        let subs: [(SubWaveId, u8); 2] = std::array::from_fn(|o| {
            let spec = &self.spec.osc[o];
            let wave = match spec.sub {
                SubWave::Off => SubWaveId::Off,
                SubWave::Sine => SubWaveId::Sine,
                SubWave::Square => SubWaveId::Square,
            };
            (wave, if spec.sub_octave == SubOctave::Two { 2 } else { 1 })
        });
        let restart =
            [self.spec.osc[0].phase_mode == PhaseMode::Restart, self.spec.osc[1].phase_mode == PhaseMode::Restart];
        let ids: [LayerId; MAX_ENVS] =
            std::array::from_fn(|e| self.spec.envs.get(e).map(|s| s.id).unwrap_or(LayerId(u32::MAX)));
        let n = &mut self.notes.slots[slot];
        n.note = note;
        n.strength = strength;
        n.ball = ball;
        n.across = across;
        n.pitch = note as f32;
        n.glide_left = 0;
        n.fade = 1.0;
        n.fade_step = 0.0;
        n.start_at = at;
        n.n_events = 0;
        n.age = 0;
        n.env_ids = ids;
        n.chain.reset();
        for o in 0..2 {
            n.noise[o].seed(mix(seed ^ (o as u64 + 1)));
            if !osc_on[o] {
                continue;
            }
            n.osc[o].prepare(mix(seed.wrapping_add(17 * (o as u64 + 1))));
            n.osc[o].set(&unison[o]);
            n.osc[o].note_on();
            if restart[o] {
                // Back by `at` samples, so the wave is at Phase on the note's first sample.
                n.osc[o].rewind(at);
                n.sub[o].set(unison[o].freq_hz, subs[o].0, subs[o].1, 1.0);
                n.sub[o].note_on();
                n.sub[o].rewind(at);
            }
            n.warp_prev[o] = self.now[k::osc(o, k::WARP)] / 100.0;
        }
        for e in 0..MAX_ENVS {
            n.env[e].reset();
            n.env[e].set(params[e]);
            n.gate[e] = gates[e];
            if e < envs {
                n.push(at, e, Ev::Trigger(strength));
            }
        }
    }

    /// Both oscillators' unison settings for a note at `pitch`.
    fn unison_params(&self, pitch: f32) -> [UnisonParams; 2] {
        let n = &self.now;
        std::array::from_fn(|o| {
            let at = |key: usize| n[k::osc(o, key)];
            let semis = 12.0 * at(k::OCTAVE).round() + at(k::SEMI).round() + at(k::CENTS) / 100.0;
            UnisonParams {
                freq_hz: note_hz(pitch + semis),
                unison: at(k::UNISON).round().clamp(1.0, 16.0) as u8,
                detune_cents: at(k::DETUNE),
                spread: at(k::SPREAD) / 100.0,
                blend: at(k::BLEND) / 100.0,
                random_phase: at(k::RANDOM_PHASE) / 100.0,
                phase_deg: at(k::PHASE),
                restart: self.spec.osc[o].phase_mode == PhaseMode::Restart,
            }
        })
    }

    /// The slot playing and its notes (`None` past the last slot, which plays nothing).
    fn slot_now(&mut self) -> usize {
        let turn = self.clock.turn();
        if turn != self.slot_turn {
            // A new slot: the play order starts again.
            self.slot_turn = turn;
            self.slot_hits = 0;
            self.picker.reset();
        }
        self.clock.current()
    }

    /// Hit `hit` (accepted) at `sample` picks its notes. Returns them, the strum and the seed.
    fn pick_for(&mut self, cx: &BlockCx, hit: &BlockHit, time: f64) -> super::notes::NotePick {
        let slot = self.slot_now();
        let Some(notes) = self.spec.slots.get(slot) else { return Default::default() };
        let ball = hit.event.a;
        let n = &self.spec.notes;
        let ball_number = if n.plays == bpe_kit::sound::Plays::ByBall {
            bpe_kit::things::balls::sim::ball_number(cx.world, ball.thing, ball.item)
        } else {
            0
        };
        let side = match &self.spec.side_path {
            Some(path) => bpe_kit::things::shape::side_on(cx.world, self.spec.thing, path, hit.event.point),
            None => 0,
        };
        let pcx = PickCx {
            slot: notes,
            hit_index: self.slot_hits,
            ball_number,
            side,
            seed: mix(((self.spec.thing.0 as u64) << 32) ^ mix(self.slot_turn ^ 0x9A7E)),
            repeat: n.repeat,
            octave_range: n.octave_range,
            strum_s: n.strum_s(time),
            strum_dir: n.strum_dir,
        };
        let pick = self.picker.pick(&pcx);
        self.slot_hits += 1;
        pick
    }

    /// Every hit on this shape that plays, and the auditions, start their notes. The slot
    /// changes landing in the block come before the hits on their samples.
    fn take_hits(&mut self, cx: &BlockCx, hits: &[BlockHit]) {
        self.filter.set(self.now[k::MIN_STRENGTH] / 100.0, self.now[k::RETRIGGER] / 1000.0);
        self.clock.begin_block(cx.start_sample, BLOCK as u32, cx.time, &self.spec.notes, cx.tempo);
        self.start_strums(cx);
        for hit in hits {
            let at = hit.offset.min(BLOCK as u32 - 1);
            let sample = cx.start_sample + at as u64;
            self.clock.until(at, &self.spec.notes);
            let Some(strength) = self.filter.accept(hit, sample, cx.world) else { continue };
            let ball = hit.event.a;
            let seed = mix(sample
                ^ mix(((ball.thing.0 as u64) << 32) | ball.item as u64)
                ^ ((self.spec.thing.0 as u64) << 48));
            self.hit_arrives(at);
            let pick = self.pick_for(cx, hit, cx.time);
            self.clock.hit(&self.spec.notes);
            let gap = (pick.strum as f64 * crate::SAMPLE_RATE as f64).max(0.0);
            for (i, &note) in pick.notes().iter().enumerate() {
                let note_seed = mix(seed ^ note as u64);
                let delay = (i as f64 * gap).round() as u64;
                if at as u64 + delay < BLOCK as u64 {
                    self.start_note(cx, at + delay as u32, note, strength, ball, hit.event.point, note_seed);
                } else if self.n_strums < MAX_STRUMS {
                    self.strums[self.n_strums] =
                        Strum { sample: sample + delay, note, strength, ball, point: hit.event.point, seed: note_seed };
                    self.n_strums += 1;
                }
            }
        }
        self.clock.end_block(&self.spec.notes);
        for i in 0..std::mem::take(&mut self.n_auditions) {
            let note = self.auditions[i];
            self.audition_count += 1;
            let seed = mix(self.audition_count ^ ((self.spec.thing.0 as u64) << 40));
            self.start_note(cx, 0, note, AUDITION_STRENGTH, AUDITION_BALL, Vec2::ZERO, seed);
        }
    }

    /// Starts the strummed notes whose time comes in this block.
    fn start_strums(&mut self, cx: &BlockCx) {
        let end = cx.start_sample + BLOCK as u64;
        let mut i = 0;
        while i < self.n_strums {
            let s = self.strums[i];
            if s.sample < end {
                let at = s.sample.saturating_sub(cx.start_sample) as u32;
                self.start_note(cx, at, s.note, s.strength, s.ball, s.point, s.seed);
                self.n_strums -= 1;
                self.strums[i] = self.strums[self.n_strums];
            } else {
                i += 1;
            }
        }
    }

    /// Runs the hit counting (the hit filter's memory, the slot clock and the play order) over
    /// the clip's hits before sound sample `upto`, without making sound: after a seek the voice
    /// then plays as it would have from the start. `hits` are every hit before `upto` in sample
    /// order, with their samples. Allocates nothing but may take a while on a long clip, so it
    /// runs in [`crate::Engine::seek`], off the steady sound path.
    pub fn catch_up(
        &mut self,
        hits: &[(u64, BlockHit)],
        upto: u64,
        tempo: &crate::TempoMap,
        world: &bpe_runtime::World,
    ) {
        let block = BLOCK as u64;
        let mut next = 0;
        let mut start = 0;
        while start < upto {
            let time = start as f64 / crate::SAMPLE_RATE as f64;
            self.clock.begin_block(start, BLOCK as u32, time, &self.spec.notes, tempo);
            let end = start + block;
            if next < hits.len() && hits[next].0 < end {
                self.read_keys(time);
                self.filter.set(self.now[k::MIN_STRENGTH] / 100.0, self.now[k::RETRIGGER] / 1000.0);
                let cx = BlockCx { start_sample: start, time, tempo, world };
                while next < hits.len() && hits[next].0 < end {
                    let (sample, hit) = &hits[next];
                    next += 1;
                    let at = (sample - start) as u32;
                    self.clock.until(at, &self.spec.notes);
                    if self.filter.accept(hit, *sample, world).is_none() {
                        continue;
                    }
                    let _ = self.pick_for(&cx, hit, time);
                    self.clock.hit(&self.spec.notes);
                }
            }
            self.clock.end_block(&self.spec.notes);
            start = end;
        }
    }

    /// A new hit closes the Until-next-hit gates of the notes sounding.
    fn hit_arrives(&mut self, at: u32) {
        let envs = self.envs();
        for (i, n) in self.notes.slots.iter_mut().enumerate() {
            if !self.notes.meta[i].active {
                continue;
            }
            for e in 0..envs {
                if self.spec.envs[e].gate_mode == GateMode::UntilNextHit && n.gate[e] == u64::MAX {
                    n.push(at, e, Ev::Release);
                    n.gate[e] = u64::MAX - 1;
                }
            }
        }
    }

    /// Renders note `i` into the shape's buffers. Returns false once it is over.
    fn render_note(&mut self, i: usize) -> bool {
        let envs = self.envs();
        let params: [EnvParams; MAX_ENVS] =
            std::array::from_fn(|e| if e < envs { self.env_params(e) } else { EnvParams::default() });
        let n = &mut self.notes.slots[i];
        let sc = &mut *self.scratch;
        // The envelopes, sample by sample, with this block's hits and gates.
        for e in 0..envs {
            n.env[e].set(params[e]);
            for s in 0..BLOCK {
                for k in 0..n.n_events {
                    let (at, env, ev) = n.events[k];
                    if at as usize == s && env == e {
                        match ev {
                            Ev::Trigger(strength) => {
                                let vel = params_velocity(&self.now, e);
                                n.env[e].trigger(1.0 - vel + vel * strength);
                            }
                            Ev::Release => n.env[e].release(),
                        }
                    }
                }
                if n.gate[e] < u64::MAX - 1 && s >= n.start_at as usize {
                    if n.gate[e] == 0 {
                        n.env[e].release();
                        n.gate[e] = u64::MAX - 1;
                    } else {
                        n.gate[e] -= 1;
                    }
                }
                sc.env[e][s] = n.env[e].next();
            }
        }
        n.n_events = 0;
        // The fade every note ends with, silent before its start.
        for s in 0..BLOCK {
            if s < n.start_at as usize {
                sc.fade[s] = 0.0;
                continue;
            }
            n.fade = (n.fade - n.fade_step).max(0.0);
            sc.fade[s] = n.fade;
        }
        let first = n.start_at as usize;
        n.start_at = 0;

        // The knobs as this note's modulators move them, at the block's end.
        let mut sources = [0.0f32; MAX_ENVS];
        for (s, &e) in self.source_env.iter().enumerate() {
            if e != usize::MAX {
                sources[s] = sc.env[e][BLOCK - 1];
            }
        }
        let mut moved = [0.0f32; KNOBS];
        if self.spec.matrix.is_empty() {
            moved = self.now;
        } else {
            self.spec.matrix.apply(&sources, &self.now, &mut moved);
        }

        // Glide.
        if n.glide_left > 0 {
            n.glide_left = n.glide_left.saturating_sub(BLOCK as u32);
            let t = 1.0 - n.glide_left as f32 / n.glide_len.max(1) as f32;
            n.pitch = n.glide_from + (n.glide_to - n.glide_from) * t;
        }

        sc.note_l = [0.0; BLOCK];
        sc.note_r = [0.0; BLOCK];
        // Each oscillator's sound (after its Gain) into its lane's buffer.
        let mut pitch = [n.pitch; 2];
        for o in 0..2 {
            let spec = &self.spec.osc[o];
            if !self.plays[o] {
                continue;
            }
            let at = |key: usize| moved[k::osc(o, key)];
            // Gain: the shape's smoothed knob, scaled by its routes sample by sample.
            let (g_place, s_place) = (k::osc(o, k::GAIN), k::osc(o, k::SUB_LEVEL));
            for s in 0..BLOCK {
                sc.gain[s] = sc.smooth[3 * o][s] * sc.fade[s];
            }
            if self.routed[3 * o] {
                scale_by_routes(&self.spec.matrix, &self.source_env, &sc.env, g_place, &mut sc.gain);
            }
            let semis = 12.0 * at(k::OCTAVE).round() + at(k::SEMI).round() + at(k::CENTS) / 100.0;
            pitch[o] = n.pitch + semis;
            let [lane_l, lane_r] = &mut sc.lanes[o];
            *lane_l = [0.0; BLOCK];
            *lane_r = [0.0; BLOCK];
            if spec.wave == Wave::Noise {
                let pink = spec.noise == NoiseColour::Pink;
                for s in 0..BLOCK {
                    let v = if pink { n.noise[o].pink() } else { n.noise[o].white() } * sc.gain[s];
                    lane_l[s] = v;
                    lane_r[s] = v;
                }
            } else if let Some(table) = &spec.table {
                let freq = note_hz(pitch[o]);
                n.osc[o].set(&UnisonParams {
                    freq_hz: freq,
                    unison: at(k::UNISON).round().clamp(1.0, 16.0) as u8,
                    detune_cents: at(k::DETUNE),
                    spread: at(k::SPREAD) / 100.0,
                    blend: at(k::BLEND) / 100.0,
                    random_phase: at(k::RANDOM_PHASE) / 100.0,
                    phase_deg: at(k::PHASE),
                    restart: spec.phase_mode == PhaseMode::Restart,
                });
                let warp = (at(k::WARP) / 100.0).clamp(0.0, 1.0);
                let from = n.warp_prev[o];
                for s in 0..BLOCK {
                    sc.warp[s] = from + (warp - from) * (s + 1) as f32 / BLOCK as f32;
                }
                n.warp_prev[o] = warp;
                n.osc[o].render(table, &sc.warp, &sc.gain, lane_l, lane_r);
                if spec.sub != SubWave::Off
                    && let Some(square) = &self.spec.square
                {
                    let wave = if spec.sub == SubWave::Sine { SubWaveId::Sine } else { SubWaveId::Square };
                    let octave = if spec.sub_octave == SubOctave::Two { 2 } else { 1 };
                    n.sub[o].set(freq, wave, octave, 1.0);
                    for s in 0..BLOCK {
                        sc.sub_gain[s] = sc.gain[s] * sc.smooth[3 * o + 2][s];
                    }
                    if self.routed[3 * o + 2] {
                        scale_by_routes(&self.spec.matrix, &self.source_env, &sc.env, s_place, &mut sc.sub_gain);
                    }
                    n.sub[o].render(square, &sc.sub_gain, lane_l, lane_r);
                }
            }
        }
        // The layers, top to bottom (and across, where one taps the other oscillator).
        n.chain.render(&self.spec.chain, &moved, pitch, &mut sc.lanes);
        for o in 0..2 {
            if !self.spec.osc[o].on {
                continue;
            }
            // Level and the oscillator's pan.
            let (pl, pr) = pan_gains(self.pans[o].value());
            let mut lv = sc.smooth[3 * o + 1];
            if self.routed[3 * o + 1] {
                scale_by_routes(&self.spec.matrix, &self.source_env, &sc.env, k::osc(o, k::LEVEL), &mut lv);
            }
            let [out_l, out_r] = &sc.lanes[self.spec.chain.out[o]];
            for s in 0..BLOCK {
                sc.note_l[s] += out_l[s] * lv[s] * pl;
                sc.note_r[s] += out_r[s] * lv[s] * pr;
            }
        }
        // The output: Volume and Pan, pulled across by Follow hit.
        let follow = (moved[k::FOLLOW_HIT] / 100.0).clamp(0.0, 1.0);
        let (ol, or) = pan_gains(self.pans[2].value() + follow * n.across);
        let mut vol = sc.smooth[6];
        if self.routed[6] {
            scale_by_routes(&self.spec.matrix, &self.source_env, &sc.env, k::VOLUME, &mut vol);
        }
        for s in 0..BLOCK {
            sc.shape_l[s] += sc.note_l[s] * vol[s] * ol;
            sc.shape_r[s] += sc.note_r[s] * vol[s] * or;
        }
        n.age += (BLOCK - first) as u64;

        // Is it over?
        if n.fade <= 0.0 {
            return false;
        }
        if n.fade_step == 0.0 {
            let mut any_level = false;
            let mut all_done = true;
            let mut every_done = true;
            for e in 0..envs {
                let done = n.env[e].is_done();
                every_done &= done;
                if self.level_envs[e] {
                    any_level = true;
                    all_done &= done;
                }
            }
            let over = if any_level {
                all_done
            } else if envs > 0 {
                every_done
            } else {
                n.age as f32 >= PLAIN_NOTE_S * crate::SAMPLE_RATE
            };
            if over {
                n.fade_out(STEAL_MS);
            }
        }
        true
    }
}

fn params_velocity(now: &Values, e: usize) -> f32 {
    (now[k::env(e, k::VELOCITY)] / 100.0).clamp(0.0, 1.0)
}

/// Scales `out` by every scaling route on the knob at `target`, sample by sample (a route
/// whose source runs no envelope reads 0, as [`ModMatrix::factor`] does).
fn scale_by_routes(
    matrix: &ModMatrix,
    source_env: &[usize; MAX_ENVS],
    env: &[[f32; BLOCK]; MAX_ENVS],
    target: usize,
    out: &mut [f32; BLOCK],
) {
    for (source, amount, invert) in matrix.scaling(target as u16) {
        let a = amount.abs();
        match source_env.get(source as usize).copied().filter(|e| *e != usize::MAX) {
            Some(e) => {
                for (o, m) in out.iter_mut().zip(&env[e]) {
                    let m = if invert { 1.0 - m } else { *m };
                    *o *= (1.0 - a * (1.0 - m.clamp(0.0, 1.0))).max(0.0);
                }
            }
            None => {
                let m = if invert { 1.0 } else { 0.0 };
                let f = (1.0 - a * (1.0 - m)).max(0.0);
                out.iter_mut().for_each(|o| *o *= f);
            }
        }
    }
}

impl VoiceSource for ShapeVoice {
    fn thing(&self) -> ThingId {
        self.spec.thing
    }

    fn on_block(&mut self, cx: &BlockCx, hits: &[BlockHit], out_l: &mut [f32; BLOCK], out_r: &mut [f32; BLOCK]) {
        self.read_keys(cx.time);
        let t = self.targets();
        for (i, s) in self.smooth.iter_mut().enumerate() {
            s.set_target(t[i]);
            s.fill(&mut self.scratch.smooth[i]);
        }
        for (i, s) in self.pans.iter_mut().enumerate() {
            s.set_target(t[7 + i]);
            for _ in 0..BLOCK {
                s.next();
            }
        }
        if self.leave_step == 0.0 {
            self.take_hits(cx, hits);
        }
        self.scratch.shape_l = [0.0; BLOCK];
        self.scratch.shape_r = [0.0; BLOCK];
        for i in 0..CAPACITY {
            if !self.notes.meta[i].active {
                continue;
            }
            if !self.render_note(i) {
                self.notes.free(i);
            } else if self.notes.slots[i].fade_step > 0.0 {
                self.notes.fade(i);
            }
        }
        let mut peak = 0.0f32;
        for s in 0..BLOCK {
            let g = self.leaving;
            self.leaving = (self.leaving - self.leave_step).max(0.0);
            let (l, r) = (self.scratch.shape_l[s] * g, self.scratch.shape_r[s] * g);
            out_l[s] += l;
            out_r[s] += r;
            peak = peak.max(l.abs()).max(r.abs());
        }
        self.peak = peak;
    }

    fn bus(&self) -> u8 {
        self.spec.bus
    }

    fn reset(&mut self) {
        self.notes.clear();
        for n in &mut self.notes.slots {
            n.n_events = 0;
            for e in &mut n.env {
                e.reset();
            }
        }
        self.filter.reset();
        self.clock.reset();
        self.picker.reset();
        self.slot_hits = 0;
        self.slot_turn = 0;
        self.n_strums = 0;
        self.n_auditions = 0;
        self.peak = 0.0;
        self.read_keys(0.0);
        self.snap_smoothers();
    }

    fn adopt(&mut self, old: &mut dyn VoiceSource) {
        let Some(old) = old.as_any_mut().downcast_mut::<ShapeVoice>() else { return };
        self.filter.take_memory(&old.filter);
        // The slots and the play order carry on where they were.
        self.clock.take_over(&old.clock);
        self.slot_turn = old.slot_turn;
        self.slot_hits = old.slot_hits;
        if self.spec.notes.plays == old.spec.notes.plays {
            self.picker = old.picker;
        }
        self.strums = old.strums;
        self.n_strums = old.n_strums;
        // The smoothers glide on from where the old ones were.
        for (new, prev) in self.smooth.iter_mut().zip(&old.smooth) {
            let target = new.target();
            new.reset(prev.value());
            new.set_target(target);
        }
        for (new, prev) in self.pans.iter_mut().zip(&old.pans) {
            let target = new.target();
            new.reset(prev.value());
            new.set_target(target);
        }
        self.notes.meta = old.notes.meta;
        let ids: [LayerId; MAX_ENVS] =
            std::array::from_fn(|e| self.spec.envs.get(e).map(|s| s.id).unwrap_or(LayerId(u32::MAX)));
        for (new, prev) in self.notes.slots.iter_mut().zip(old.notes.slots.iter_mut()) {
            // Cloned without its chain (which would allocate); the chain's layers take over
            // their memories one by one.
            let old_chain = std::mem::take(&mut prev.chain);
            let mut n = prev.clone();
            prev.chain = old_chain;
            n.chain = std::mem::take(&mut new.chain);
            n.chain.adopt(&prev.chain);
            // Envelopes follow their layers; a new one starts silent.
            for (e, id) in ids.iter().enumerate() {
                match prev.env_ids.iter().position(|x| x == id) {
                    Some(p) => {
                        n.env[e] = prev.env[p].clone();
                        n.gate[e] = prev.gate[p];
                    }
                    None => {
                        n.env[e] = Envelope::new();
                        n.gate[e] = u64::MAX - 1;
                    }
                }
            }
            n.env_ids = ids;
            n.n_events = 0;
            *new = n;
        }
        self.auditions = old.auditions;
        self.n_auditions = old.n_auditions;
        self.audition_count = old.audition_count;
        self.leaving = old.leaving;
        self.leave_step = old.leave_step;
    }

    fn fade_out(&mut self, ms: f32) {
        let step = 1.0 / (ms / 1000.0 * crate::SAMPLE_RATE).max(1.0);
        self.leave_step = self.leave_step.max(step);
    }

    fn is_silent(&self) -> bool {
        self.leaving <= 0.0 || self.notes.active() == 0
    }

    fn audition(&mut self, note: u8) {
        ShapeVoice::audition(self, note);
    }

    fn catch_up(&mut self, hits: &[(u64, BlockHit)], upto: u64, tempo: &crate::TempoMap, world: &bpe_runtime::World) {
        ShapeVoice::catch_up(self, hits, upto, tempo, world);
    }

    fn level(&self) -> f32 {
        self.peak
    }

    fn playing_slot(&self) -> Option<usize> {
        Some(self.clock.current())
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
