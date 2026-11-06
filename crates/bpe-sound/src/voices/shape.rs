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
use crate::mods::{
    DistanceMod, DistanceParams, HitMod, HitValues, LfoMod, LfoParams, LightMod, LightParams, ModCx, Modulator, NoteCx,
};
use crate::{BLOCK, BlockCx, BlockHit, VoiceSource};
use bpe_core::{LayerId, NoteSlot, ShapePath, Target, ThingId, Track};
use bpe_dsp::{EnvParams, Envelope, Noise, Smoother, SubOsc, SubWaveId, TableSet, UnisonOsc, UnisonParams, db_to_gain};
use bpe_kit::sound::{
    DistanceSettings, Follows, GateMode, GateUnit, HitPick, LfoSettings, LfoStart, LightFollows, LightModSettings,
    NoiseColour, PhaseMode, PlayMode, PlayWhen, SILENT_DB, SubOctave, SubWave, Wave,
};
use bpe_runtime::rules::AS_IF_HIT;
use bpe_runtime::{EventKind, Party};
use glam::Vec2;
use std::any::Any;
use std::sync::Arc;

/// How hard an audition (a key clicked in the editor) plays.
pub const AUDITION_STRENGTH: f32 = 0.8;
/// How long a shape that went away (deleted, or its sound turned off) takes to fade out.
pub const LEAVE_MS: f32 = 10.0;
/// The most hits one note takes in one block (more are let go).
const MAX_EVENTS: usize = 3 * MAX_ENVS;
/// A note with no envelope on its levels lasts this long.
pub(crate) const PLAIN_NOTE_S: f32 = 0.1;
/// A key clicked by hand plays at most this long before its gate closes (otherwise a held gate
/// would wait for a hit that never comes while stopped, or while the shape holds a drone).
const AUDITION_GATE_S: f32 = 0.4;
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
    /// The numbers the hit gave the Hit modulators.
    hv: HitValues,
    /// The hit's sample (its notes share it in the MIDI log).
    group: u64,
}
/// The most notes a drone holds at once.
const MAX_HELD: usize = 16;
/// The most modulators of one kind besides the envelopes.
const MAX_EACH: usize = k::MAX_EACH;
/// The most modulators routes can come from: every envelope and every other modulator.
pub const MAX_SOURCES: usize = MAX_ENVS + 4 * MAX_EACH;

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

/// One LFO layer that runs.
#[derive(Clone, Debug)]
pub struct LfoSpec {
    pub id: LayerId,
    pub settings: LfoSettings,
    pub seed: u64,
    /// The drawn shape, as the engine reads it.
    pub curve: crate::mods::lfo::Curve,
}

/// One Distance layer that runs.
#[derive(Clone, Debug)]
pub struct DistSpec {
    pub id: LayerId,
    pub settings: DistanceSettings,
    /// The balls it hears.
    pub heard: Arc<Target>,
}

/// One Hit layer that runs.
#[derive(Clone, Copy, Debug)]
pub struct HitSpec {
    pub id: LayerId,
    pub pick: HitPick,
}

/// One Light layer that runs.
#[derive(Clone, Debug)]
pub struct LightSpec {
    pub id: LayerId,
    pub settings: LightModSettings,
    /// The balls it hears.
    pub heard: Arc<Target>,
}

/// The scene's light probe, which the Light modulators read.
#[derive(Clone)]
pub struct Probe(pub Arc<dyn bpe_runtime::LightAt>);

impl std::fmt::Debug for Probe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Probe")
    }
}

/// The LFO, Distance, Hit and Light layers that run (up to eight of each), and the order every
/// modulator but the envelopes runs in each block.
#[derive(Clone, Debug, Default)]
pub struct ModsSpec {
    pub lfos: Vec<LfoSpec>,
    pub dists: Vec<DistSpec>,
    pub hits: Vec<HitSpec>,
    pub lights: Vec<LightSpec>,
    pub order: Vec<LayerId>,
    /// The light probe, when there are Light layers and the scene has a Lighting card.
    pub probe: Option<Probe>,
}

impl ModsSpec {
    pub(crate) fn lfo_per_note(&self, i: usize) -> bool {
        self.lfos[i].settings.start == LfoStart::PerNote
    }

    pub(crate) fn dist_per_note(&self, i: usize) -> bool {
        self.dists[i].settings.follows == Follows::EachNotesBall
    }

    pub(crate) fn light_per_note(&self, i: usize) -> bool {
        self.lights[i].settings.follows == LightFollows::EachNotesBall
    }

    /// The probe, as the modulators read it.
    pub(crate) fn probe(&self) -> Option<&dyn bpe_runtime::LightAt> {
        self.probe.as_ref().map(|p| &*p.0)
    }
}

/// What a route's source is, by its place in the matrix's sources.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Src {
    /// A layer this voice doesn't run: reads 0.
    None,
    Env(usize),
    Lfo(usize),
    Dist(usize),
    Hit(usize),
    Light(usize),
}

/// An LFO's settings for a block from the knobs `kv` (keys and routes applied).
pub(crate) fn lfo_params(spec: &LfoSpec, i: usize, kv: &[f32], tempo: &crate::TempoMap) -> LfoParams {
    let at = |key: usize| kv[k::lfo(i, key)];
    let rate = crate::mods::lfo_rate_hz(&spec.settings, at(k::LFO_RATE) as f64, tempo);
    LfoParams {
        wave: crate::mods::lfo::wave_of(spec.settings.shape),
        curve: spec.curve.clone(),
        once: spec.settings.once,
        rate_hz: rate,
        phase: at(k::LFO_PHASE) as f64 / 360.0,
        fade_in: at(k::LFO_FADE_IN).max(0.0) as f64,
        delay: at(k::LFO_DELAY).max(0.0) as f64,
        smooth: (at(k::LFO_SMOOTH) / 100.0).clamp(0.0, 1.0),
        start: spec.settings.start,
    }
}

/// A Distance's settings for a block from the knobs `kv`.
pub(crate) fn dist_params(spec: &DistSpec, i: usize, kv: &[f32]) -> DistanceParams {
    let at = |key: usize| kv[k::dist(i, key)];
    DistanceParams {
        near: at(k::NEAR),
        far: at(k::FAR),
        curve: at(k::DIST_CURVE) / 100.0,
        smooth_ms: at(k::DIST_SMOOTH),
        full_at: at(k::FULL_AT).round().max(1.0),
        ..DistanceParams::of(&spec.settings)
    }
}

/// A Light's settings for a block from the knobs `kv`.
pub(crate) fn light_params(spec: &LightSpec, i: usize, kv: &[f32]) -> LightParams {
    let at = |key: usize| kv[k::light(i, key)];
    LightParams {
        dark: at(k::DARK),
        bright: at(k::BRIGHT),
        curve: at(k::LIGHT_CURVE) / 100.0,
        smooth_ms: at(k::LIGHT_SMOOTH),
        lit_at: at(k::LIT_AT),
        full_at: at(k::LIGHT_FULL_AT).round().max(1.0),
        ..LightParams::of(&spec.settings)
    }
}

/// Sets Hit modulator `m` (layer `i` of the card's hits) from the knobs `kv`.
fn set_hit(m: &mut HitMod, spec: &HitSpec, i: usize, kv: &[f32]) {
    let at = |key: usize| kv[k::hit(i, key)];
    m.set_pick(spec.pick);
    m.set_knobs(at(k::LOW), at(k::HIGH), at(k::HIT_CURVE), at(k::HIT_SMOOTH));
}

/// The places of a modulator's knobs.
fn knob_range(src: Src) -> (usize, usize) {
    match src {
        Src::Env(e) => (k::env(e, 0), k::env(e, k::ADSR_LEN)),
        Src::Lfo(i) => (k::lfo(i, 0), k::lfo(i, k::LFO_LEN)),
        Src::Dist(i) => (k::dist(i, 0), k::dist(i, k::DIST_LEN)),
        Src::Hit(i) => (k::hit(i, 0), k::hit(i, k::HIT_LEN)),
        Src::Light(i) => (k::light(i, 0), k::light(i, k::LIGHT_LEN)),
        Src::None => (0, 0),
    }
}

/// Everything a shape's voice is built from.
pub struct ShapeSpec {
    pub thing: ThingId,
    /// Its place among the scene's sounding shapes (for its meter).
    pub index: usize,
    pub osc: [OscSpec; 2],
    pub envs: Vec<EnvSpec>,
    pub play_mode: PlayMode,
    /// When its notes sound: on hits, always (held), or while a ball is in range.
    pub play_when: PlayWhen,
    /// The Distance (its place in `mods.dists`) whose range holds the notes, for In range.
    pub zone: Option<usize>,
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
    /// The modulators besides the envelopes.
    pub mods: ModsSpec,
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
    /// The note's own modulators (those that run per note), by their place in the spec.
    lfo: [LfoMod; MAX_EACH],
    dist: [DistanceMod; MAX_EACH],
    hit: [HitMod; MAX_EACH],
    light: [LightMod; MAX_EACH],
    /// Every route source's value at the end of the last block, as this note heard it.
    src_prev: [f32; MAX_SOURCES],
}

impl Note {
    fn new(chain: &ChainSpec, heard: &Arc<Target>) -> Self {
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
            lfo: std::array::from_fn(|_| LfoMod::new(0)),
            dist: std::array::from_fn(|_| DistanceMod::new(heard.clone())),
            hit: std::array::from_fn(|_| HitMod::default()),
            light: std::array::from_fn(|_| LightMod::new(heard.clone())),
            src_prev: [0.0; MAX_SOURCES],
        }
    }

    /// Queues `ev` for envelope `env` at `at` in this block. False when the block's queue is full
    /// (the event is let go).
    fn push(&mut self, at: u32, env: usize, ev: Ev) -> bool {
        if self.n_events < MAX_EVENTS {
            self.events[self.n_events] = (at, env, ev);
            self.n_events += 1;
            true
        } else {
            false
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
    /// Every route source sample by sample, this note, this block.
    src: [[f32; BLOCK]; MAX_SOURCES],
    /// A level knob moved by its adding routes, sample by sample (a linear gain).
    level: [f32; BLOCK],
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
            src: [[0.0; BLOCK]; MAX_SOURCES],
            level: [0.0; BLOCK],
        })
    }
}

/// A dB knob's gain, with the bottom of its range silent.
pub(crate) fn knob_gain(db: f32) -> f32 {
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
    /// The notes held (Always, In range): their voice slots and notes.
    held: [usize; MAX_HELD],
    held_notes: [u8; MAX_HELD],
    n_held: usize,
    /// The last accepted hits' numbers, a ring written at `heard % RECENT_HITS`.
    recent: [HitValues; crate::meters::RECENT_HITS],
    heard: u32,
    notes: Voices<Note>,
    /// The knobs this block (keys read).
    now: Values,
    /// Smoothed: Gain, Level, Sub level of each oscillator, then Volume; and the pans.
    smooth: [Smoother; 7],
    pans: [Smoother; 3],
    /// The envelopes that scale a level: a note ends when they are done.
    level_envs: [bool; MAX_ENVS],
    /// Whether each of Gain, Level, Sub level (per oscillator) and Volume has routes.
    routed: [bool; 7],
    /// Whether each of them has adding routes (from modulators other than envelopes).
    offsets: [bool; 7],
    /// Whether each oscillator runs: it is on, or the other's layers tap it.
    plays: [bool; 2],
    scratch: Box<Scratch>,
    /// Fading out because the shape went away (1 while it plays).
    leaving: f32,
    leave_step: f32,
    auditions: [u8; MAX_AUDITIONS],
    n_auditions: usize,
    audition_count: u64,
    /// The note being started takes a voice of its own even on a Mono shape (a key clicked
    /// while a Mono drone holds the one voice).
    own_voice: bool,
    /// A rule's note length in samples while it starts (the MIDI log notes it as the gate).
    note_gate: Option<u64>,
    /// The hit (or drone change) the notes being started belong to, for the MIDI log.
    group: u64,
    /// The loudest sample of the last block.
    peak: f32,
    /// The notes started since they were last taken, while noting (the export's MIDI).
    noted: Option<Vec<crate::NoteOn>>,
    /// What each of the matrix's sources is.
    src: [Src; MAX_SOURCES],
    n_src: usize,
    /// Each LFO's, Distance's and Hit's place among the matrix's sources.
    lfo_src: [Option<usize>; MAX_EACH],
    dist_src: [Option<usize>; MAX_EACH],
    hit_src: [Option<usize>; MAX_EACH],
    light_src: [Option<usize>; MAX_EACH],
    /// The modulators besides the envelopes, in the order they run.
    order: Vec<Src>,
    /// The shape's shared modulators (a Free or Restart LFO, a Distance not following each
    /// note's ball).
    lfo: [LfoMod; MAX_EACH],
    dist: [DistanceMod; MAX_EACH],
    light: [LightMod; MAX_EACH],
    /// The shared sources' values at the end of the block before and this one.
    shared_prev: [f32; MAX_SOURCES],
    shared_now: [f32; MAX_SOURCES],
    /// Each per-note source's largest value among the notes, last block (what a shared
    /// modulator's knobs hear from it) and this block so far.
    note_max: [f32; MAX_SOURCES],
    note_max_next: [f32; MAX_SOURCES],
    /// A knob whose value each note shows, for tests (`watch`).
    watch: Option<usize>,
    watched: [f32; CAPACITY],
}

/// The smoothed knobs' places: per oscillator Gain, Level, Sub level, then Volume.
pub(crate) const SMOOTHED: [usize; 7] = [
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
        // By source: the envelope it is (usize::MAX for other modulators).
        let mut source_env = [usize::MAX; MAX_SOURCES];
        let mut src = [Src::None; MAX_SOURCES];
        let (mut lfo_src, mut dist_src, mut hit_src) = ([None; MAX_EACH], [None; MAX_EACH], [None; MAX_EACH]);
        let mut light_src = [None; MAX_EACH];
        let n_src = spec.matrix.sources().len().min(MAX_SOURCES);
        for (s, id) in spec.matrix.sources().iter().enumerate().take(MAX_SOURCES) {
            let m = &spec.mods;
            if let Some(e) = spec.envs.iter().take(MAX_ENVS).position(|x| x.id == *id) {
                source_env[s] = e;
                src[s] = Src::Env(e);
            } else if let Some(i) = m.lfos.iter().position(|x| x.id == *id) {
                src[s] = Src::Lfo(i);
                lfo_src[i] = Some(s);
            } else if let Some(i) = m.dists.iter().position(|x| x.id == *id) {
                src[s] = Src::Dist(i);
                dist_src[i] = Some(s);
            } else if let Some(i) = m.hits.iter().position(|x| x.id == *id) {
                src[s] = Src::Hit(i);
                hit_src[i] = Some(s);
            } else if let Some(i) = m.lights.iter().position(|x| x.id == *id) {
                src[s] = Src::Light(i);
                light_src[i] = Some(s);
            }
        }
        let order: Vec<Src> = spec
            .mods
            .order
            .iter()
            .filter_map(|id| {
                let m = &spec.mods;
                m.lfos
                    .iter()
                    .position(|x| x.id == *id)
                    .map(Src::Lfo)
                    .or_else(|| m.dists.iter().position(|x| x.id == *id).map(Src::Dist))
                    .or_else(|| m.hits.iter().position(|x| x.id == *id).map(Src::Hit))
                    .or_else(|| m.lights.iter().position(|x| x.id == *id).map(Src::Light))
            })
            .collect();
        let anything = Arc::new(Target::Anything);
        let mut level_envs = [false; MAX_ENVS];
        let mut routed = [false; 7];
        let mut offsets = [false; 7];
        for (r, &target) in SMOOTHED.iter().enumerate() {
            routed[r] = spec.matrix.reaches(target as u16);
            offsets[r] = spec.matrix.adds(target as u16);
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
            strums: [Strum {
                sample: 0,
                note: 0,
                strength: 0.0,
                ball: AUDITION_BALL,
                point: Vec2::ZERO,
                seed: 0,
                hv: HitValues::default(),
                group: 0,
            }; MAX_STRUMS],
            n_strums: 0,
            notes: Voices::new(|| Note::new(&spec.chain, &anything)),
            now,
            smooth: std::array::from_fn(|_| Smoother::new(Smoother::KNOB_MS)),
            pans: std::array::from_fn(|_| Smoother::new(Smoother::KNOB_MS)),
            level_envs,
            routed,
            offsets,
            plays: std::array::from_fn(|o| spec.osc[o].on || spec.chain.raw_needed[o]),
            scratch: Scratch::new(spec.chain.buffers()),
            leaving: 1.0,
            leave_step: 0.0,
            auditions: [0; MAX_AUDITIONS],
            n_auditions: 0,
            audition_count: 0,
            own_voice: false,
            note_gate: None,
            group: 0,
            held: [0; MAX_HELD],
            held_notes: [0; MAX_HELD],
            n_held: 0,
            recent: [HitValues::default(); crate::meters::RECENT_HITS],
            heard: 0,
            peak: 0.0,
            noted: None,
            src,
            n_src,
            lfo_src,
            dist_src,
            hit_src,
            light_src,
            order,
            lfo: std::array::from_fn(|i| LfoMod::new(spec.mods.lfos.get(i).map_or(0, |l| l.seed))),
            dist: std::array::from_fn(|i| {
                DistanceMod::new(spec.mods.dists.get(i).map_or_else(|| anything.clone(), |d| d.heard.clone()))
            }),
            light: std::array::from_fn(|i| {
                LightMod::new(spec.mods.lights.get(i).map_or_else(|| anything.clone(), |d| d.heard.clone()))
            }),
            shared_prev: [0.0; MAX_SOURCES],
            shared_now: [0.0; MAX_SOURCES],
            note_max: [0.0; MAX_SOURCES],
            note_max_next: [0.0; MAX_SOURCES],
            watch: None,
            watched: [0.0; CAPACITY],
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

    /// Shows the knob at `place` (see [`super::knobs`]) as each note has it after its routes,
    /// read with [`ShapeVoice::watched`]. For tests.
    pub fn watch(&mut self, place: usize) {
        self.watch = Some(place);
    }

    /// The watched knob of each sounding note, with its note. Allocates.
    pub fn watched(&self) -> Vec<(u8, f32)> {
        (0..CAPACITY)
            .filter(|&i| self.notes.meta[i].active && !self.notes.meta[i].fading)
            .map(|i| (self.notes.slots[i].note, self.watched[i]))
            .collect()
    }

    /// LFO `layer`'s place in its shape now (0..1): a shared one's own, a per-note one's in the
    /// newest sounding note. `None` for another layer, or a per-note LFO with no note.
    pub fn lfo_position(&self, layer: LayerId) -> Option<f32> {
        let m = &self.spec.mods;
        let i = m.lfos.iter().take(MAX_EACH).position(|x| x.id == layer)?;
        if !m.lfo_per_note(i) {
            return Some(self.lfo[i].position());
        }
        (0..CAPACITY)
            .filter(|&n| self.notes.meta[n].active && !self.notes.meta[n].fading)
            .max_by_key(|&n| self.notes.meta[n].started)
            .map(|n| self.notes.slots[n].lfo[i].position())
    }

    /// Modulator `layer`'s value now, 0..1: a shared one's own, a per-note one's largest among
    /// the sounding notes (0 with none). `None` for a layer this voice doesn't run.
    pub fn mod_value(&self, layer: LayerId) -> Option<f32> {
        let m = &self.spec.mods;
        let notes = || (0..CAPACITY).filter(|&i| self.notes.meta[i].active && !self.notes.meta[i].fading);
        if let Some(i) = m.lfos.iter().position(|x| x.id == layer) {
            return Some(if m.lfo_per_note(i) {
                notes().map(|n| self.notes.slots[n].lfo[i].value()).fold(0.0, f32::max)
            } else {
                self.lfo[i].value()
            });
        }
        if let Some(i) = m.dists.iter().position(|x| x.id == layer) {
            return Some(if m.dist_per_note(i) {
                notes().map(|n| self.notes.slots[n].dist[i].value()).fold(0.0, f32::max)
            } else {
                self.dist[i].value()
            });
        }
        if let Some(i) = m.lights.iter().position(|x| x.id == layer) {
            return Some(if m.light_per_note(i) {
                notes().map(|n| self.notes.slots[n].light[i].value()).fold(0.0, f32::max)
            } else {
                self.light[i].value()
            });
        }
        if let Some(i) = m.hits.iter().position(|x| x.id == layer) {
            return Some(notes().map(|n| self.notes.slots[n].hit[i].value()).fold(0.0, f32::max));
        }
        let e = self.spec.envs.iter().take(MAX_ENVS).position(|x| x.id == layer)?;
        Some(notes().map(|n| self.notes.slots[n].env[e].level()).fold(0.0, f32::max))
    }

    /// The shape's shared modulators move on a block: in their order, each one's knobs moved by
    /// the modulators run before it this block (and the rest as they were the block before; a
    /// per-note source counts at its largest among the notes).
    fn run_shared(&mut self, cx: &BlockCx) {
        self.shared_prev = self.shared_now;
        if self.order.is_empty() {
            return;
        }
        let mut vals = [0.0f32; MAX_SOURCES];
        for s in 0..self.n_src {
            vals[s] = match self.src[s] {
                Src::Lfo(i) if !self.spec.mods.lfo_per_note(i) => self.shared_now[s],
                Src::Dist(i) if !self.spec.mods.dist_per_note(i) => self.shared_now[s],
                Src::Light(i) if !self.spec.mods.light_per_note(i) => self.shared_now[s],
                Src::None => 0.0,
                _ => self.note_max[s],
            };
        }
        let mcx = ModCx {
            start_sample: cx.start_sample,
            tempo: cx.tempo,
            world: cx.world,
            shape: self.spec.thing,
            log: cx.log,
            light: self.spec.mods.probe(),
        };
        let mut kv = [0.0f32; KNOBS];
        for &m in &self.order {
            let (lo, hi) = knob_range(m);
            match m {
                Src::Lfo(i) if !self.spec.mods.lfo_per_note(i) => {
                    self.spec.matrix.apply_range(&vals, &self.now, &mut kv, lo, hi);
                    self.lfo[i].set_params(lfo_params(&self.spec.mods.lfos[i], i, &kv, cx.tempo));
                    self.lfo[i].next_block(&mcx);
                    if let Some(s) = self.lfo_src[i] {
                        vals[s] = self.lfo[i].value();
                        self.shared_now[s] = vals[s];
                    }
                }
                Src::Dist(i) if !self.spec.mods.dist_per_note(i) => {
                    self.spec.matrix.apply_range(&vals, &self.now, &mut kv, lo, hi);
                    self.dist[i].set_params(dist_params(&self.spec.mods.dists[i], i, &kv));
                    self.dist[i].next_block(&mcx);
                    if let Some(s) = self.dist_src[i] {
                        vals[s] = self.dist[i].value();
                        self.shared_now[s] = vals[s];
                    }
                }
                Src::Light(i) if !self.spec.mods.light_per_note(i) => {
                    self.spec.matrix.apply_range(&vals, &self.now, &mut kv, lo, hi);
                    self.light[i].set_params(light_params(&self.spec.mods.lights[i], i, &kv));
                    self.light[i].next_block(&mcx);
                    if let Some(s) = self.light_src[i] {
                        vals[s] = self.light[i].value();
                        self.shared_now[s] = vals[s];
                    }
                }
                _ => {}
            }
        }
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
        env_params_of(&self.now, e)
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
    fn start_note(
        &mut self,
        cx: &BlockCx,
        at: u32,
        note: u8,
        strength: f32,
        ball: Party,
        point: Vec2,
        seed: u64,
        hv: &HitValues,
    ) -> Option<usize> {
        let sample = cx.start_sample + at as u64;
        let ncx = NoteCx { sample, hit: hv, ball: (ball != AUDITION_BALL).then_some(ball) };
        let across = if self.spec.width > 0.0 { (point.x / (self.spec.width * 0.5)).clamp(-1.0, 1.0) } else { 0.0 };
        let mono = self.spec.play_mode == PlayMode::Mono && !self.own_voice;
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
                // The note started in an earlier block, so its gate counts from the block's
                // start: the new gate is the hit's offset longer, to close after its trigger.
                n.gate[e] = if gates[e] >= u64::MAX - 1 { gates[e] } else { gates[e] + at as u64 };
            }
            // Its own modulators take the new hit: a Hit its number, an LFO a fresh start, a
            // Distance the new ball.
            let m = &self.spec.mods;
            for i in 0..m.hits.len().min(MAX_EACH) {
                set_hit(&mut n.hit[i], &m.hits[i], i, &self.now);
                n.hit[i].note_on(&ncx);
            }
            for i in 0..m.lfos.len().min(MAX_EACH) {
                n.lfo[i].note_on(&ncx);
            }
            for i in 0..m.dists.len().min(MAX_EACH) {
                n.dist[i].set_params(dist_params(&m.dists[i], i, &self.now));
                n.dist[i].note_on(&ncx);
            }
            for i in 0..m.lights.len().min(MAX_EACH) {
                n.light[i].set_params(light_params(&m.lights[i], i, &self.now));
                n.light[i].note_on(&ncx);
            }
            self.notes.meta[0].started = sample;
            self.log_note(cx, sample, note, strength);
            return Some(0);
        }
        let (slot, steal) = if mono {
            if self.notes.meta[0].active {
                self.notes.slots[0].fade = 0.0;
            }
            self.notes.meta[0] = super::alloc::Meta { active: true, fading: false, started: sample };
            (0, None)
        } else {
            let limit = if self.own_voice {
                // Never the drone's own voice.
                MAX_VOICES
            } else {
                (self.now[k::VOICES].round() as usize).clamp(1, MAX_VOICES)
            };
            let s = self.notes.start(limit, sample, cx.start_sample)?;
            (s.slot, s.steal)
        };
        // Logged once it has a voice (a note with none plays nothing).
        self.log_note(cx, sample, note, strength);
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
        // The note's own modulators start with it.
        let m = &self.spec.mods;
        n.src_prev = [0.0; MAX_SOURCES];
        for i in 0..m.lfos.len().min(MAX_EACH) {
            n.lfo[i].reset();
            n.lfo[i].reseed(m.lfos[i].seed ^ seed);
            n.lfo[i].set_params(lfo_params(&m.lfos[i], i, &self.now, cx.tempo));
            n.lfo[i].note_on(&ncx);
        }
        for i in 0..m.dists.len().min(MAX_EACH) {
            n.dist[i].reset();
            n.dist[i].set_params(dist_params(&m.dists[i], i, &self.now));
            n.dist[i].note_on(&ncx);
        }
        for i in 0..m.lights.len().min(MAX_EACH) {
            n.light[i].reset();
            n.light[i].set_params(light_params(&m.lights[i], i, &self.now));
            n.light[i].note_on(&ncx);
        }
        for i in 0..m.hits.len().min(MAX_EACH) {
            n.hit[i].reset();
            set_hit(&mut n.hit[i], &m.hits[i], i, &self.now);
            n.hit[i].note_on(&ncx);
            if let Some(s) = self.hit_src[i] {
                n.src_prev[s] = n.hit[i].value();
            }
        }
        Some(slot)
    }

    /// Notes `note` starting at `sample` in the MIDI log, while recording.
    fn log_note(&mut self, cx: &BlockCx, sample: u64, note: u8, strength: f32) {
        if self.noted.is_none() {
            return;
        }
        let gate = match self.note_gate {
            Some(n) => n,
            None if self.envs() > 0 => self.gate_samples(0, cx.tempo),
            None => u64::MAX,
        };
        let group = self.group;
        if let Some(noted) = self.noted.as_mut() {
            noted.push(crate::NoteOn::new(sample, note, strength, gate, group));
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
            if hit.event.kind == EventKind::Note {
                self.rule_note(cx, at, sample, hit);
                continue;
            }
            let Some(strength) = self.filter.accept(hit, sample, cx.world) else { continue };
            let ball = hit.event.a;
            let seed = mix(sample
                ^ mix(((ball.thing.0 as u64) << 32) | ball.item as u64)
                ^ ((self.spec.thing.0 as u64) << 48));
            let hv = self.hit_values_of(cx, hit, seed, strength);
            self.recent[self.heard as usize % crate::meters::RECENT_HITS] = hv;
            self.heard = self.heard.wrapping_add(1);
            self.restart_shared(&NoteCx { sample, hit: &hv, ball: Some(ball) });
            if self.spec.play_when != PlayWhen::OnHit {
                // Held notes stay held (the hit starts no envelope again); it counts towards the
                // slots and gives the Hit modulators its numbers.
                self.clock.hit(&self.spec.notes);
                self.held_hear_hit(&NoteCx { sample, hit: &hv, ball: Some(ball) });
                continue;
            }
            self.hit_arrives(at, sample);
            self.group = sample;
            let pick = self.pick_for(cx, hit, cx.time);
            self.clock.hit(&self.spec.notes);
            let gap = (pick.strum as f64 * crate::SAMPLE_RATE as f64).max(0.0);
            for (i, &note) in pick.notes().iter().enumerate() {
                let note_seed = mix(seed ^ note as u64);
                let delay = (i as f64 * gap).round() as u64;
                if at as u64 + delay < BLOCK as u64 {
                    self.start_note(cx, at + delay as u32, note, strength, ball, hit.event.point, note_seed, &hv);
                } else if self.n_strums < MAX_STRUMS {
                    self.strums[self.n_strums] = Strum {
                        sample: sample + delay,
                        note,
                        strength,
                        ball,
                        point: hit.event.point,
                        seed: note_seed,
                        hv,
                        group: sample,
                    };
                    self.n_strums += 1;
                }
            }
        }
        self.clock.end_block(&self.spec.notes);
        self.hold(cx);
        for i in 0..std::mem::take(&mut self.n_auditions) {
            let note = self.auditions[i];
            self.audition_count += 1;
            let seed = mix(self.audition_count ^ ((self.spec.thing.0 as u64) << 40));
            let hv = HitValues {
                strength: AUDITION_STRENGTH,
                random: bpe_dsp::lfo::random_at(seed, 0),
                ..HitValues::default()
            };
            self.restart_shared(&NoteCx { sample: cx.start_sample, hit: &hv, ball: None });
            self.group = cx.start_sample;
            // On a drone the key sounds beside the held notes instead of taking their voices.
            self.own_voice = self.n_held > 0;
            let started = self.start_note(cx, 0, note, AUDITION_STRENGTH, AUDITION_BALL, Vec2::ZERO, seed, &hv);
            self.own_voice = false;
            if let Some(slot) = started {
                // A gate that waits for the next hit would never close on a key: it closes after
                // a short press. A Fixed gate plays as set.
                let most = (AUDITION_GATE_S * crate::SAMPLE_RATE) as u64;
                for g in &mut self.notes.slots[slot].gate {
                    if *g == u64::MAX {
                        *g = most;
                    }
                }
            }
        }
    }

    /// A rule's Play a note (rules part 6) on this shape, at `at` in the block: a set pitch, or
    /// the slot's next pick as a hit would make it (a chord's notes together). It passes no hit
    /// filter and moves no slot clock; on a drone it sounds on its own voice beside the held
    /// notes, as a clicked key does.
    fn rule_note(&mut self, cx: &BlockCx, at: u32, sample: u64, hit: &BlockHit) {
        let e = &hit.event;
        let Some(shape) = e.b.filter(|b| b.thing == self.spec.thing) else { return };
        let strength = if e.speed.is_finite() { e.speed.clamp(0.0, 1.0) } else { 0.0 };
        let ball = e.a;
        let seed = mix(sample
            ^ mix(((ball.thing.0 as u64) << 32) | ball.item as u64)
            ^ ((self.spec.thing.0 as u64) << 48)
            ^ 0x4e07e);
        let mut hv = self.hit_values_of(cx, hit, seed, strength);
        hv.strength = strength;
        self.restart_shared(&NoteCx { sample, hit: &hv, ball: Some(ball) });
        let drone = self.spec.play_when != PlayWhen::OnHit;
        if !drone {
            self.hit_arrives(at, sample);
        }
        self.group = sample;
        let pick = (shape.item == AS_IF_HIT).then(|| self.pick_for(cx, hit, cx.time));
        let one = [shape.item.min(127) as u8];
        let notes: &[u8] = match &pick {
            Some(p) => p.notes(),
            None => &one,
        };
        let length = e.normal.x;
        let length = if length.is_finite() && length > 0.0 { Some((length * crate::SAMPLE_RATE) as u64) } else { None };
        self.own_voice = drone && self.n_held > 0;
        self.note_gate = length;
        for &note in notes {
            let mono = self.spec.play_mode == PlayMode::Mono && !self.own_voice;
            let again = mono && self.notes.meta[0].active && !self.notes.meta[0].fading;
            let note_seed = mix(seed ^ note as u64);
            let Some(slot) = self.start_note(cx, at, note, strength, ball, e.point, note_seed, &hv) else { continue };
            // A drone's own voice never waits for a hit; a length closes every gate after it (from
            // the block's start for a Mono note that started before).
            let gate = match length {
                Some(n) => Some(if again { n + at as u64 } else { n }),
                None if drone => Some((AUDITION_GATE_S * crate::SAMPLE_RATE) as u64),
                None => None,
            };
            if let Some(gate) = gate {
                for g in &mut self.notes.slots[slot].gate {
                    if length.is_some() || *g == u64::MAX {
                        *g = gate;
                    }
                }
            }
        }
        self.own_voice = false;
        self.note_gate = None;
    }

    /// Always and In range: holds the playing slot's notes (while a ball is in range), moving
    /// to the slot's notes when they change (Mono glides) and letting go when nothing is held.
    fn hold(&mut self, cx: &BlockCx) {
        if !cx.playing && self.n_held > 0 {
            // Stopped: Pause has already faded what was heard, so these were started unheard by a
            // seek's pre-roll. They go at once, with nothing under the keys clicked by hand.
            for i in 0..std::mem::take(&mut self.n_held) {
                let s = self.held[i];
                if self.notes.meta[s].active && self.notes.slots[s].note == self.held_notes[i] {
                    self.notes.free(s);
                }
            }
            return;
        }
        // While stopped a drone waits: only keys clicked by hand sound.
        let want = match self.spec.play_when {
            PlayWhen::OnHit => return,
            PlayWhen::Always => cx.playing,
            PlayWhen::InRange => cx.playing && self.in_range(cx),
        };
        let mono = self.spec.play_mode == PlayMode::Mono;
        let limit = if mono { 1 } else { (self.now[k::VOICES].round() as usize).clamp(1, MAX_VOICES).min(MAX_HELD) };
        let mut wanted = [0u8; MAX_HELD];
        let mut n = 0;
        if want && let Some(slot) = self.spec.slots.get(self.clock.current()) {
            for &note in slot.notes.iter().take(limit) {
                wanted[n] = note;
                n += 1;
            }
        }
        let alive = (0..self.n_held).all(|i| {
            let s = self.held[i];
            self.notes.meta[s].active && !self.notes.meta[s].fading && self.notes.slots[s].note == self.held_notes[i]
        });
        if alive && n == self.n_held && wanted[..n] == self.held_notes[..n] {
            return;
        }
        let hv = HitValues { strength: 1.0, ..HitValues::default() };
        let thing = self.spec.thing.0 as u64;
        let seed = |note: u8| mix((thing << 40) ^ 0xD20E ^ note as u64);
        if mono && alive && n == 1 && self.n_held == 1 && self.held[0] == 0 {
            // The one voice moves to the new note, legato: it glides there (with Glide) and its
            // envelopes carry on as they were, like a key held down while another is pressed.
            let glide = (self.now[k::GLIDE].max(0.0) / 1000.0 * crate::SAMPLE_RATE) as u32;
            let n = &mut self.notes.slots[0];
            n.note = wanted[0];
            if glide > 0 {
                n.glide_from = n.pitch;
                n.glide_to = wanted[0] as f32;
                n.glide_len = glide;
                n.glide_left = glide;
            } else {
                n.pitch = wanted[0] as f32;
                n.glide_left = 0;
            }
            self.held_notes[0] = wanted[0];
            if let Some(noted) = self.noted.as_mut() {
                noted.push(crate::NoteOn::new(cx.start_sample, wanted[0], 1.0, u64::MAX, cx.start_sample));
            }
            return;
        }
        self.release_held(cx.start_sample);
        self.group = cx.start_sample;
        for &note in &wanted[..n] {
            let s = seed(note);
            if let Some(slot) = self.start_note(cx, 0, note, 1.0, AUDITION_BALL, Vec2::ZERO, s, &hv) {
                // Held until let go (in the MIDI log too).
                if let Some(last) = self.noted.as_mut().and_then(|n| n.last_mut()) {
                    last.gate = u64::MAX;
                }
                self.notes.slots[slot].gate = [u64::MAX; MAX_ENVS];
                self.held[self.n_held] = slot;
                self.held_notes[self.n_held] = note;
                self.n_held += 1;
            }
        }
    }

    /// Whether a ball is in range of the In range Distance (its value above 0, read from the
    /// nearest ball when it follows each note's own).
    fn in_range(&self, cx: &BlockCx) -> bool {
        let Some(i) = self.spec.zone else { return false };
        let Some(d) = self.spec.mods.dists.get(i) else { return false };
        let mut p = dist_params(d, i, &self.now);
        if p.follows == Follows::EachNotesBall {
            p.follows = Follows::Nearest;
        }
        crate::mods::distance::distance_value(&p, &d.heard, cx.world, self.spec.thing, None).is_some_and(|v| v > 0.0)
    }

    /// The held notes close their gates and ring out (at `sample`, in the MIDI log).
    fn release_held(&mut self, sample: u64) {
        for i in 0..std::mem::take(&mut self.n_held) {
            if let Some(noted) = self.noted.as_mut() {
                noted.push(crate::NoteOn::end(sample, self.held_notes[i]));
            }
            let s = self.held[i];
            if self.notes.meta[s].active && self.notes.slots[s].note == self.held_notes[i] {
                for e in 0..self.envs() {
                    if self.notes.slots[s].gate[e] == u64::MAX {
                        self.notes.slots[s].gate[e] = 0;
                    }
                }
            }
        }
    }

    /// A hit on a shape holding its notes: the held notes' Hit modulators take the hit's numbers
    /// (their envelopes carry on: a held note is not played again).
    fn held_hear_hit(&mut self, ncx: &NoteCx) {
        for i in 0..self.n_held {
            let s = self.held[i];
            if !self.notes.meta[s].active {
                continue;
            }
            let n = &mut self.notes.slots[s];
            let m = &self.spec.mods;
            for h in 0..m.hits.len().min(MAX_EACH) {
                set_hit(&mut n.hit[h], &m.hits[h], h, &self.now);
                n.hit[h].note_on(ncx);
            }
        }
    }

    /// Whether voice slot `slot` holds one of the drone's notes.
    fn holds(&self, slot: usize) -> bool {
        (0..self.n_held).any(|i| self.held[i] == slot)
    }

    /// The numbers an accepted hit gives the Hit modulators (worked out only when there are
    /// any).
    fn hit_values_of(&self, cx: &BlockCx, hit: &BlockHit, seed: u64, strength: f32) -> HitValues {
        if self.spec.mods.hits.is_empty() {
            return HitValues { strength, ..HitValues::default() };
        }
        crate::mods::hit_values(cx.world, self.spec.thing, &hit.event, seed)
    }

    /// A hit (or a key clicked) restarts the shape's Restart-on-hit LFOs.
    fn restart_shared(&mut self, ncx: &NoteCx) {
        for i in 0..self.spec.mods.lfos.len().min(MAX_EACH) {
            if !self.spec.mods.lfo_per_note(i) {
                self.lfo[i].note_on(ncx);
            }
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
                self.group = s.group;
                self.start_note(cx, at, s.note, s.strength, s.ball, s.point, s.seed, &s.hv);
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
        // Catching up only picks notes; no modulator reads the light here.
        let no_log = bpe_runtime::Log::default();
        let mut next = 0;
        let mut start = 0;
        while start < upto {
            let time = start as f64 / crate::SAMPLE_RATE as f64;
            self.clock.begin_block(start, BLOCK as u32, time, &self.spec.notes, tempo);
            let end = start + block;
            if next < hits.len() && hits[next].0 < end {
                self.read_keys(time);
                self.filter.set(self.now[k::MIN_STRENGTH] / 100.0, self.now[k::RETRIGGER] / 1000.0);
                let cx = BlockCx { start_sample: start, time, tempo, world, log: &no_log, playing: true };
                while next < hits.len() && hits[next].0 < end {
                    let (sample, hit) = &hits[next];
                    next += 1;
                    let at = (sample - start) as u32;
                    self.clock.until(at, &self.spec.notes);
                    // A rule's note picks as a hit would (the next note) but passes no filter.
                    if hit.event.kind == EventKind::Note {
                        if hit.event.b.is_some_and(|b| b.thing == self.spec.thing && b.item == AS_IF_HIT) {
                            let _ = self.pick_for(&cx, hit, time);
                        }
                        continue;
                    }
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

    /// A new hit at sample `sample` (offset `at` in the block) closes the Until-next-hit gates
    /// of the notes sounding. A strummed note starting on this sample or later in the block has
    /// not sounded yet, so the hit after this one closes it.
    fn hit_arrives(&mut self, at: u32, sample: u64) {
        let envs = self.envs();
        for (i, n) in self.notes.slots.iter_mut().enumerate() {
            let meta = &self.notes.meta[i];
            if !meta.active || meta.started >= sample {
                continue;
            }
            for e in 0..envs {
                // The gate closes only once its release is queued (a full queue leaves it open
                // for the next hit rather than stuck closed without a release).
                if self.spec.envs[e].gate_mode == GateMode::UntilNextHit
                    && n.gate[e] == u64::MAX
                    && n.push(at, e, Ev::Release)
                {
                    n.gate[e] = u64::MAX - 1;
                }
            }
        }
    }

    /// Renders note `i` into the shape's buffers. Returns false once it is over.
    fn render_note(&mut self, cx: &BlockCx, i: usize) -> bool {
        let envs = self.envs();
        let drone = self.holds(i);
        let mut params: [EnvParams; MAX_ENVS] =
            std::array::from_fn(|e| if e < envs { self.env_params(e) } else { EnvParams::default() });
        let mut moved = [0.0f32; KNOBS];
        // Envelope knobs moved by other modulators hear them as they were a block ago.
        for (e, p) in params.iter_mut().enumerate().take(envs) {
            let (lo, hi) = knob_range(Src::Env(e));
            if self.spec.matrix.reaches_range(lo, hi) {
                let prev = self.notes.slots[i].src_prev;
                self.spec.matrix.apply_range(&prev, &self.now, &mut moved, lo, hi);
                *p = env_params_of(&moved, e);
            }
        }
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

        // Every source's value at the block's end: envelopes as they ran, shared modulators as
        // the shape ran them, then this note's own in their order (each hearing those before it
        // now and the rest a block ago).
        let mut sources = [0.0f32; MAX_SOURCES];
        let per_note = |src: Src| match src {
            Src::Lfo(m) => self.spec.mods.lfo_per_note(m),
            Src::Dist(m) => self.spec.mods.dist_per_note(m),
            Src::Light(m) => self.spec.mods.light_per_note(m),
            Src::Hit(_) => true,
            _ => false,
        };
        for s in 0..self.n_src {
            sources[s] = match self.src[s] {
                Src::Env(e) => sc.env[e][BLOCK - 1],
                Src::None => 0.0,
                src if per_note(src) => n.src_prev[s],
                _ => self.shared_now[s],
            };
        }
        let mcx = ModCx {
            start_sample: cx.start_sample,
            tempo: cx.tempo,
            world: cx.world,
            shape: self.spec.thing,
            log: cx.log,
            light: self.spec.mods.probe(),
        };
        for &m in &self.order {
            if !per_note(m) {
                continue;
            }
            let (lo, hi) = knob_range(m);
            self.spec.matrix.apply_range(&sources, &self.now, &mut moved, lo, hi);
            let (value, s) = match m {
                Src::Lfo(j) => {
                    n.lfo[j].set_params(lfo_params(&self.spec.mods.lfos[j], j, &moved, cx.tempo));
                    n.lfo[j].next_block(&mcx);
                    (n.lfo[j].value(), self.lfo_src[j])
                }
                Src::Dist(j) => {
                    n.dist[j].set_params(dist_params(&self.spec.mods.dists[j], j, &moved));
                    n.dist[j].next_block(&mcx);
                    (n.dist[j].value(), self.dist_src[j])
                }
                Src::Light(j) => {
                    n.light[j].set_params(light_params(&self.spec.mods.lights[j], j, &moved));
                    n.light[j].next_block(&mcx);
                    (n.light[j].value(), self.light_src[j])
                }
                Src::Hit(j) => {
                    set_hit(&mut n.hit[j], &self.spec.mods.hits[j], j, &moved);
                    n.hit[j].next_block(&mcx);
                    (n.hit[j].value(), self.hit_src[j])
                }
                _ => continue,
            };
            if let Some(s) = s {
                sources[s] = value;
            }
        }
        // Each source sample by sample: envelopes as they ran, the rest across the block.
        for s in 0..self.n_src {
            let (from, to) = match self.src[s] {
                Src::Env(e) => {
                    sc.src[s] = sc.env[e];
                    continue;
                }
                Src::None => (0.0, 0.0),
                src if per_note(src) => (n.src_prev[s], sources[s]),
                _ => (self.shared_prev[s], self.shared_now[s]),
            };
            for (j, v) in sc.src[s].iter_mut().enumerate() {
                *v = from + (to - from) * (j + 1) as f32 / BLOCK as f32;
            }
            self.note_max_next[s] = self.note_max_next[s].max(sources[s]);
        }
        n.src_prev = sources;
        if self.spec.matrix.is_empty() {
            moved = self.now;
        } else {
            self.spec.matrix.apply(&sources, &self.now, &mut moved);
        }
        if let Some(place) = self.watch {
            self.watched[i] = moved[place];
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
            // Gain: the shape's smoothed knob, moved by its adding routes and scaled by its
            // envelopes sample by sample.
            let (g_place, s_place) = (k::osc(o, k::GAIN), k::osc(o, k::SUB_LEVEL));
            let g = if self.offsets[3 * o] {
                offset_by_routes(&self.spec.matrix, &sc.src, g_place, &sc.smooth[3 * o], &mut sc.level);
                &sc.level
            } else {
                &sc.smooth[3 * o]
            };
            for s in 0..BLOCK {
                sc.gain[s] = g[s] * sc.fade[s];
            }
            if self.routed[3 * o] {
                scale_by_routes(&self.spec.matrix, &sc.src, g_place, &mut sc.gain);
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
                    let sub = if self.offsets[3 * o + 2] {
                        offset_by_routes(&self.spec.matrix, &sc.src, s_place, &sc.smooth[3 * o + 2], &mut sc.level);
                        &sc.level
                    } else {
                        &sc.smooth[3 * o + 2]
                    };
                    for s in 0..BLOCK {
                        sc.sub_gain[s] = sc.gain[s] * sub[s];
                    }
                    if self.routed[3 * o + 2] {
                        scale_by_routes(&self.spec.matrix, &sc.src, s_place, &mut sc.sub_gain);
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
            if self.offsets[3 * o + 1] {
                offset_by_routes(&self.spec.matrix, &sc.src, k::osc(o, k::LEVEL), &sc.smooth[3 * o + 1], &mut lv);
            }
            if self.routed[3 * o + 1] {
                scale_by_routes(&self.spec.matrix, &sc.src, k::osc(o, k::LEVEL), &mut lv);
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
        if self.offsets[6] {
            offset_by_routes(&self.spec.matrix, &sc.src, k::VOLUME, &sc.smooth[6], &mut vol);
        }
        if self.routed[6] {
            scale_by_routes(&self.spec.matrix, &sc.src, k::VOLUME, &mut vol);
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
        // A drone's note never ends by itself, however low its sustain (it is let go when the
        // shape stops holding it); everything else ends when its envelopes are done.
        let held = drone && (envs == 0 || (0..envs).any(|e| n.gate[e] == u64::MAX));
        if n.fade_step == 0.0 && !held {
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

/// Envelope `e`'s settings from the knobs `kv`.
pub(crate) fn env_params_of(kv: &[f32], e: usize) -> EnvParams {
    let at = |key: usize| kv[k::env(e, key)];
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

pub(crate) fn params_velocity(now: &Values, e: usize) -> f32 {
    (now[k::env(e, k::VELOCITY)] / 100.0).clamp(0.0, 1.0)
}

/// The level knob at `target` sample by sample into `out`: `smooth` (its smoothed gain) back in
/// dB, moved by the knob's adding routes (their sources read from `src`, a source past the last
/// reading 0), stopped at the knob's ends and made a gain again, the bottom being silence.
fn offset_by_routes(
    matrix: &ModMatrix,
    src: &[[f32; BLOCK]; MAX_SOURCES],
    target: usize,
    smooth: &[f32; BLOCK],
    out: &mut [f32; BLOCK],
) {
    let Some((lo, hi)) = matrix.range_of(target as u16) else {
        *out = *smooth;
        return;
    };
    let to_db = |g: f32| if g <= 0.0 { SILENT_DB } else { (20.0 * g.log10()).max(SILENT_DB) };
    // A knob at rest (the usual case) needs one logarithm, not one a sample.
    if smooth.iter().all(|g| *g == smooth[0]) {
        out.fill(to_db(smooth[0]));
    } else {
        for (o, g) in out.iter_mut().zip(smooth) {
            *o = to_db(*g);
        }
    }
    for (source, depth, bipolar, invert) in matrix.adding(target as u16) {
        let buf = src.get(source as usize);
        for (s, o) in out.iter_mut().enumerate() {
            let v = buf.map_or(0.0, |b| b[s]);
            let m = if invert { 1.0 - v } else { v };
            let m = if bipolar { 2.0 * m - 1.0 } else { m };
            *o += depth * m;
        }
    }
    for o in out.iter_mut() {
        *o = knob_gain(o.clamp(lo, hi));
    }
}

/// Scales `out` by every scaling route on the knob at `target`, sample by sample, its sources
/// read from `src` (a source past the last reads 0, as [`ModMatrix::factor`] does).
fn scale_by_routes(matrix: &ModMatrix, src: &[[f32; BLOCK]; MAX_SOURCES], target: usize, out: &mut [f32; BLOCK]) {
    for (source, amount, invert) in matrix.scaling(target as u16) {
        let a = amount.abs();
        match src.get(source as usize) {
            Some(buf) => {
                for (o, m) in out.iter_mut().zip(buf) {
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
        self.run_shared(cx);
        self.scratch.shape_l = [0.0; BLOCK];
        self.scratch.shape_r = [0.0; BLOCK];
        self.note_max_next = [0.0; MAX_SOURCES];
        for i in 0..CAPACITY {
            if !self.notes.meta[i].active {
                continue;
            }
            if !self.render_note(cx, i) {
                self.notes.free(i);
            } else if self.notes.slots[i].fade_step > 0.0 {
                self.notes.fade(i);
            }
        }
        self.note_max = self.note_max_next;
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
        self.n_held = 0;
        for m in &mut self.lfo {
            m.reset();
        }
        for m in &mut self.dist {
            m.reset();
        }
        for m in &mut self.light {
            m.reset();
        }
        self.shared_prev = [0.0; MAX_SOURCES];
        self.shared_now = [0.0; MAX_SOURCES];
        self.note_max = [0.0; MAX_SOURCES];
        self.n_auditions = 0;
        self.peak = 0.0;
        self.read_keys(0.0);
        self.snap_smoothers();
    }

    fn adopt(&mut self, old: &mut dyn VoiceSource) {
        let Some(old) = old.as_any_mut().downcast_mut::<ShapeVoice>() else { return };
        self.filter.take_memory(&old.filter);
        // The slots and the play order carry on where they were.
        self.clock.take_over(&old.clock, &self.spec.notes);
        self.slot_turn = old.slot_turn;
        self.slot_hits = old.slot_hits;
        if self.spec.notes.plays == old.spec.notes.plays {
            self.picker = old.picker;
        }
        self.strums = old.strums;
        self.n_strums = old.n_strums;
        self.recent = old.recent;
        self.heard = old.heard;
        let keep_held = self.spec.play_when != PlayWhen::OnHit && self.spec.play_when == old.spec.play_when;
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
        // Shared modulators carry on where they were, by layer.
        for (i, l) in self.spec.mods.lfos.iter().enumerate() {
            if let Some(j) = old.spec.mods.lfos.iter().position(|o| o.id == l.id) {
                self.lfo[i].take_over(&old.lfo[j]);
            }
        }
        for (i, d) in self.spec.mods.dists.iter().enumerate() {
            if let Some(j) = old.spec.mods.dists.iter().position(|o| o.id == d.id) {
                self.dist[i].take_over(&old.dist[j]);
            }
        }
        for (i, d) in self.spec.mods.lights.iter().enumerate() {
            if let Some(j) = old.spec.mods.lights.iter().position(|o| o.id == d.id) {
                self.light[i].take_over(&old.light[j]);
            }
        }
        // Each source's values, by layer.
        let old_src: Vec<Option<usize>> = self
            .spec
            .matrix
            .sources()
            .iter()
            .take(MAX_SOURCES)
            .map(|id| old.spec.matrix.sources().iter().take(MAX_SOURCES).position(|o| o == id))
            .collect();
        for (s, o) in old_src.iter().enumerate() {
            if let Some(o) = *o {
                self.shared_prev[s] = old.shared_prev[o];
                self.shared_now[s] = old.shared_now[o];
                self.note_max[s] = old.note_max[o];
            }
        }
        let lfo_from: [Option<usize>; MAX_EACH] = std::array::from_fn(|i| {
            let id = self.spec.mods.lfos.get(i)?.id;
            old.spec.mods.lfos.iter().position(|o| o.id == id)
        });
        let dist_from: [Option<usize>; MAX_EACH] = std::array::from_fn(|i| {
            let id = self.spec.mods.dists.get(i)?.id;
            old.spec.mods.dists.iter().position(|o| o.id == id)
        });
        let hit_from: [Option<usize>; MAX_EACH] = std::array::from_fn(|i| {
            let id = self.spec.mods.hits.get(i)?.id;
            old.spec.mods.hits.iter().position(|o| o.id == id)
        });
        let light_from: [Option<usize>; MAX_EACH] = std::array::from_fn(|i| {
            let id = self.spec.mods.lights.get(i)?.id;
            old.spec.mods.lights.iter().position(|o| o.id == id)
        });
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
            // The note's own modulators follow their layers too; new ones start afresh.
            for i in 0..MAX_EACH {
                n.lfo[i] = match lfo_from[i] {
                    Some(j) => prev.lfo[j].clone(),
                    None => LfoMod::new(0),
                };
                n.dist[i] = match dist_from[i] {
                    Some(j) => prev.dist[j].clone(),
                    None => new.dist[i].clone(),
                };
                n.hit[i] = match hit_from[i] {
                    Some(j) => prev.hit[j].clone(),
                    None => HitMod::default(),
                };
                n.light[i] = match light_from[i] {
                    Some(j) => prev.light[j].clone(),
                    None => new.light[i].clone(),
                };
            }
            let mut src_prev = [0.0; MAX_SOURCES];
            for (s, o) in old_src.iter().enumerate() {
                if let Some(o) = *o {
                    src_prev[s] = prev.src_prev[o];
                }
            }
            n.src_prev = src_prev;
            *new = n;
        }
        // Held notes stay held while the shape still holds notes, and let go when it plays on
        // hits again (the slots came across one for one).
        if keep_held {
            self.held = old.held;
            self.held_notes = old.held_notes;
            self.n_held = old.n_held;
        } else if old.n_held > 0 {
            self.held = old.held;
            self.held_notes = old.held_notes;
            self.n_held = old.n_held;
            self.release_held(0);
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

    fn record_notes(&mut self, on: bool) {
        self.noted = on.then(Vec::new);
    }

    fn take_notes(&mut self, out: &mut Vec<crate::NoteOn>) {
        if let Some(noted) = self.noted.as_mut() {
            out.append(noted);
        }
    }

    fn each_mod(&self, f: &mut dyn FnMut(LayerId, f32, f32)) {
        let m = &self.spec.mods;
        let envs = self.spec.envs.iter().take(MAX_ENVS).map(|e| e.id);
        let others = m.lfos.iter().map(|x| x.id).chain(m.dists.iter().map(|x| x.id)).chain(m.hits.iter().map(|x| x.id));
        let others = others.chain(m.lights.iter().map(|x| x.id));
        for layer in envs.chain(others) {
            if let Some(v) = self.mod_value(layer) {
                f(layer, v, self.lfo_position(layer).unwrap_or(f32::NAN));
            }
        }
    }

    fn recent_hits(&self) -> (&[HitValues], u32) {
        (&self.recent, self.heard)
    }

    fn playing_slot(&self) -> Option<usize> {
        Some(self.clock.current())
    }

    fn sounding_notes(&self) -> u128 {
        (0..CAPACITY)
            .filter(|&i| self.notes.meta[i].active && !self.notes.meta[i].fading)
            .fold(0, |mask, i| mask | 1u128 << (self.notes.slots[i].note & 127))
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
