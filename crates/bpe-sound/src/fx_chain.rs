//! One bus's effects, run in order on its stereo sound once a block. Each effect's Mix
//! crossfades its dry and wet sound (equal gain for the tone and level effects, equal power for
//! the time effects), and its on switch fades it in and out so bypassing never clicks. Every
//! number setting may be keyed on the timeline; keys are read once a block.

use crate::chain::{filter_type, shaper_type};
use crate::hits::BlockHit;
use crate::lag::Lag;
use crate::voices::knobs::track_value;
use crate::voices::strength;
use crate::{BLOCK, FxReading, Meters, TempoMap};
use bpe_core::Settings;
use bpe_core::{Address, LayerId, Params, Scene, SettingDef, SlotId, Target, ThingId, Track, Value, layered_key};
use bpe_dsp::fx::analyser::Analyser;
use bpe_dsp::fx::compressor::{CompParams, Compressor, Key};
use bpe_dsp::fx::delay::{self as dly, Delay, DelayParams, Tap};
use bpe_dsp::fx::eq::{BandParams, BandShape, Eq8, Place};
use bpe_dsp::fx::modfx::{Chorus, Flanger, ModParams, Phaser};
use bpe_dsp::fx::reverb_common::ReverbParams;
use bpe_dsp::fx::reverb_hall::Hall;
use bpe_dsp::fx::reverb_plate::Plate;
use bpe_dsp::fx::reverb_spring::Spring;
use bpe_dsp::fx::width::{Correlation, Width};
use bpe_dsp::{FilterParams, Shaper, ShaperParams, Smoother, StereoFilter, TruePeakLimiter};
use bpe_kit::sound::fx::{
    self, AnalyserMode, BandKind, CompressorSettings, DelayCharacter, DelayMode, DelaySettings, EqBand, EqSettings,
    Feel, HallLines, LimiterSettings, ModFxSettings, NoteLength, ReverbKind, ReverbSettings, Sidechain, StereoPlace,
    WidthSettings,
};
use bpe_kit::sound::{BUSES, DistortionSettings, FilterSettings, MIXER, Slope};
use bpe_kit::things::shape::KIND as SHAPE;

/// What an effect can read while it runs one block.
pub struct FxCx<'a> {
    /// The block's start in seconds.
    pub time: f64,
    pub tempo: &'a TempoMap,
    /// Every hit in the block (a Hits sidechain picks out its shape's).
    pub hits: &'a [BlockHit],
    /// Each bus's output from its last block, for a Bus sidechain.
    pub buses: &'a [[[f32; BLOCK]; 2]; BUSES],
    /// The bus running (0..2).
    pub bus: usize,
    pub meters: &'a Meters,
}

/// The number settings of one effect that have keys, read once a block.
#[derive(Clone, Default)]
pub struct Keyed {
    keys: Vec<(&'static str, Track)>,
    now: Vec<f32>,
}

impl Keyed {
    /// The keyed settings of layer `id` (of the mixer card) among `defs`.
    fn of(scene: &Scene, id: LayerId, defs: &'static [SettingDef]) -> Keyed {
        let keys: Vec<(&'static str, Track)> = defs
            .iter()
            .filter(|d| d.flags.keyframe)
            .filter_map(|d| {
                let a = Address { thing: ThingId::ROOT, slot: Some(SlotId::Mixer), key: layered_key(id, d.key) };
                scene.tracks.get(&a).filter(|t| !t.keys.is_empty()).map(|t| (d.key, t.clone()))
            })
            .collect();
        let now = vec![f32::NAN; keys.len()];
        Keyed { keys, now }
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Reads every key at `time`. Never allocates.
    fn at(&mut self, time: f64) {
        for (i, (_, t)) in self.keys.iter().enumerate() {
            self.now[i] = track_value(t, time).unwrap_or(f32::NAN);
        }
    }

    /// `key`'s keyed value now, or `base` when it has no keys.
    pub fn get(&self, key: &str, base: f32) -> f32 {
        match self.keys.iter().position(|(k, _)| *k == key) {
            Some(i) if self.now[i].is_finite() => self.now[i],
            _ => base,
        }
    }
}

/// How an effect's Mix crossfades.
#[derive(Clone, Copy, Debug, PartialEq)]
enum MixLaw {
    /// Dry and wet add to one (tone and level effects, where both are the same sound).
    Gain,
    /// Dry and wet keep their power (time effects, whose wet is a different sound).
    Power,
}

/// The length of a synced note at `tempo`, in seconds: whole-note fractions, or bars.
pub fn note_seconds(note: NoteLength, feel: Feel, tempo: &TempoMap) -> f64 {
    let base = match note {
        NoteLength::Bar => tempo.bar_seconds(),
        NoteLength::TwoBars => 2.0 * tempo.bar_seconds(),
        n => n.wholes() * 240.0 / tempo.bpm(),
    };
    base * feel.factor()
}

/// The keys of each EQ band's numbers, so reading keys never builds a string.
const BAND_KEYS: [[&str; 3]; 8] = [
    ["band1_freq", "band1_gain", "band1_q"],
    ["band2_freq", "band2_gain", "band2_q"],
    ["band3_freq", "band3_gain", "band3_q"],
    ["band4_freq", "band4_gain", "band4_q"],
    ["band5_freq", "band5_gain", "band5_q"],
    ["band6_freq", "band6_gain", "band6_q"],
    ["band7_freq", "band7_gain", "band7_q"],
    ["band8_freq", "band8_gain", "band8_q"],
];

/// An EQ band's settings for the sound-making part.
pub fn band_params(b: &EqBand, freq: f32, gain: f32, q: f32) -> BandParams {
    BandParams {
        on: b.on,
        shape: match b.kind {
            BandKind::LowCut => BandShape::LowCut,
            BandKind::LowShelf => BandShape::LowShelf,
            BandKind::Bell => BandShape::Bell,
            BandKind::Notch => BandShape::Notch,
            BandKind::HighShelf => BandShape::HighShelf,
            BandKind::HighCut => BandShape::HighCut,
        },
        freq,
        gain_db: gain,
        q,
        sections: b.slope.sections() as u8,
        place: match b.place {
            StereoPlace::Both => Place::Both,
            StereoPlace::Mid => Place::Mid,
            StereoPlace::Side => Place::Side,
        },
    }
}

/// Every band of an EQ as set (no keys), for the editor's curve.
pub fn eq_bands(s: &EqSettings) -> [BandParams; 8] {
    bands_of(s.bands())
}

/// Eight bands as set (no keys): a bus EQ's or an oscillator's.
pub fn bands_of(b: [&EqBand; 8]) -> [BandParams; 8] {
    std::array::from_fn(|i| band_params(b[i], b[i].freq, b[i].gain, b[i].q))
}

/// The most hits one block hands a Hits sidechain.
const MAX_KEY_HITS: usize = 64;
/// Blocks between two analyser readings written to the meters.
const ANALYSER_EVERY: u32 = 4;

/// A Chorus, Phaser or Flanger.
enum ModDsp {
    Chorus(Box<Chorus>),
    Phaser(Box<Phaser>),
    Flanger(Box<Flanger>),
}

impl ModDsp {
    fn set(&mut self, p: &ModParams) {
        match self {
            ModDsp::Chorus(d) => d.set(p),
            ModDsp::Phaser(d) => d.set(p),
            ModDsp::Flanger(d) => d.set(p),
        }
    }
    fn reset(&mut self) {
        match self {
            ModDsp::Chorus(d) => d.reset(),
            ModDsp::Phaser(d) => d.reset(),
            ModDsp::Flanger(d) => d.reset(),
        }
    }
    fn set_phase(&mut self, phase: f64) {
        match self {
            ModDsp::Chorus(d) => d.set_phase(phase),
            ModDsp::Phaser(d) => d.set_phase(phase),
            ModDsp::Flanger(d) => d.set_phase(phase),
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        match self {
            ModDsp::Chorus(d) => d.process(l, r),
            ModDsp::Phaser(d) => d.process(l, r),
            ModDsp::Flanger(d) => d.process(l, r),
        }
    }
}

/// A Plate, Hall or Spring.
enum Room {
    Plate(Box<Plate>),
    Hall(Box<Hall>, usize),
    Spring(Box<Spring>),
}

impl Room {
    fn of(set: &ReverbSettings) -> Room {
        match set.kind {
            ReverbKind::Plate => Room::Plate(Box::default()),
            ReverbKind::Hall => {
                let lines = match set.lines {
                    HallLines::Eight => 8,
                    HallLines::Sixteen => 16,
                };
                Room::Hall(Box::new(Hall::new(lines)), lines)
            }
            ReverbKind::Spring => Room::Spring(Box::default()),
        }
    }
    fn set(&mut self, p: &ReverbParams) {
        match self {
            Room::Plate(d) => d.set(p),
            Room::Hall(d, _) => d.set(p),
            Room::Spring(d) => d.set(p),
        }
    }
    fn reset(&mut self) {
        match self {
            Room::Plate(d) => d.reset(),
            Room::Hall(d, _) => d.reset(),
            Room::Spring(d) => d.reset(),
        }
    }
    fn process(&mut self, l: &mut [f32], r: &mut [f32]) {
        match self {
            Room::Plate(d) => d.process(l, r),
            Room::Hall(d, _) => d.process(l, r),
            Room::Spring(d) => d.process(l, r),
        }
    }
    /// Takes over `old`'s tail when it is the same room (another kind, or a hall of other
    /// lines, starts empty).
    fn adopt(&mut self, old: &mut Room) {
        match (self, old) {
            (Room::Plate(d), Room::Plate(o)) => std::mem::swap(d, o),
            (Room::Hall(d, a), Room::Hall(o, b)) if a == b => std::mem::swap(d, o),
            (Room::Spring(d), Room::Spring(o)) => std::mem::swap(d, o),
            _ => {}
        }
    }
}

/// The tap keys of the multi-tap delay.
const TAP_KEYS: [[&str; 3]; 4] = [
    ["tap1_time", "tap1_level", "tap1_pan"],
    ["tap2_time", "tap2_level", "tap2_pan"],
    ["tap3_time", "tap3_level", "tap3_pan"],
    ["tap4_time", "tap4_level", "tap4_pan"],
];

/// The delay's settings, keyed where they have keys, for the sound-making part.
fn delay_params(set: &DelaySettings, k: &Keyed, tempo: &TempoMap) -> DelayParams {
    let seconds = if set.sync {
        note_seconds(set.time_note, set.time_feel, tempo)
    } else {
        k.get("time_ms", set.time_ms) as f64 / 1000.0
    };
    let base = [
        (set.tap1_time, set.tap1_level, set.tap1_pan),
        (set.tap2_time, set.tap2_level, set.tap2_pan),
        (set.tap3_time, set.tap3_level, set.tap3_pan),
        (set.tap4_time, set.tap4_level, set.tap4_pan),
    ];
    let taps = std::array::from_fn(|i| {
        let [t, g, p] = TAP_KEYS[i];
        let (bt, bg, bp) = base[i];
        Tap {
            time: (k.get(t, bt) / 100.0).clamp(0.01, 1.0),
            gain: bpe_dsp::db_to_gain(k.get(g, bg)),
            pan: (k.get(p, bp) / 100.0).clamp(-1.0, 1.0),
        }
    });
    DelayParams {
        mode: match set.mode {
            DelayMode::Standard => dly::DelayMode::Standard,
            DelayMode::PingPong => dly::DelayMode::PingPong,
            DelayMode::MultiTap => dly::DelayMode::MultiTap,
        },
        character: match set.character {
            DelayCharacter::Digital => dly::Character::Digital,
            DelayCharacter::Analog => dly::Character::Analog,
            DelayCharacter::Tape => dly::Character::Tape,
        },
        time_samples: (seconds * crate::SAMPLE_RATE as f64).clamp(48.0, dly::MAX_TIME_SAMPLES as f64) as f32,
        feedback: (k.get("feedback", set.feedback) / 100.0).clamp(0.0, 1.0),
        low_cut_hz: k.get("low_cut", set.low_cut),
        high_cut_hz: k.get("high_cut", set.high_cut),
        reso: (k.get("reso", set.reso) / 100.0).clamp(0.0, 1.0),
        width: (k.get("width", set.width) / 100.0).clamp(0.0, 1.0),
        taps,
    }
}

/// The reverb's settings, keyed where they have keys, for the sound-making part.
fn reverb_params(set: &ReverbSettings, k: &Keyed) -> ReverbParams {
    let pc = |key: &str, base: f32| (k.get(key, base) / 100.0).clamp(0.0, 1.0);
    ReverbParams {
        pre_delay_samples: (k.get("pre_delay", set.pre_delay) * crate::SAMPLE_RATE / 1000.0)
            .clamp(0.0, bpe_dsp::fx::reverb_common::MAX_PRE_DELAY as f32),
        decay_s: k.get("decay", set.decay).clamp(0.1, 20.0),
        size: pc("size", set.size),
        damping: pc("damping", set.damping),
        low_cut_hz: k.get("low_cut", set.low_cut),
        high_cut_hz: k.get("high_cut", set.high_cut),
        width: pc("width", set.width),
        diffusion: pc("diffusion", set.diffusion),
        modulation: pc("modulation", set.modulation),
        early: pc("early", set.early),
        springs: set.springs.clamp(1, 3) as u8,
        tension: pc("tension", set.tension),
        drip: pc("drip", set.drip),
        freeze: set.freeze,
    }
}

/// The bus filter's settings, keyed where they have keys (no key tracking: a bus has no note).
fn filter_params(set: &FilterSettings, k: &Keyed, kind: bpe_dsp::FilterType, slope24: bool) -> FilterParams {
    let pc = |key: &str, base: f32| (k.get(key, base) / 100.0).clamp(0.0, 1.0);
    FilterParams {
        kind,
        slope24,
        cutoff_hz: k.get("cutoff", set.cutoff).max(1.0),
        reso: pc("reso", set.reso),
        drive: pc("drive", set.drive),
        gain_db: k.get("gain", set.gain),
        vowel: k.get("vowel", set.vowel),
        shift: k.get("shift", set.shift),
    }
}

/// The bus distortion's settings, keyed where they have keys.
fn shaper_params(set: &DistortionSettings, k: &Keyed) -> ShaperParams {
    ShaperParams {
        drive_db: k.get("drive", set.drive),
        bits: k.get("bits", set.bits),
        rate_hz: k.get("rate", set.rate),
        bias: (k.get("bias", set.bias) / 100.0).clamp(-1.0, 1.0),
        tone: (k.get("tone", set.tone) / 100.0).clamp(-1.0, 1.0),
        output_db: k.get("output", set.output),
    }
}

/// One effect's sound-making part, with its settings as set (keys read on top).
enum Fx {
    Limiter {
        dsp: Box<TruePeakLimiter>,
        set: LimiterSettings,
    },
    Mod {
        dsp: ModDsp,
        set: ModFxSettings,
        rate_hz: f64,
    },
    Width {
        dsp: Box<Width>,
        corr: Box<Correlation>,
        set: WidthSettings,
    },
    Eq {
        dsp: Box<Eq8>,
        analyser: Box<Analyser>,
        set: Box<EqSettings>,
        count: u32,
    },
    Comp {
        dsp: Box<Compressor>,
        set: CompressorSettings,
        /// The shape a Hits sidechain listens to; `None` (none chosen, or deleted) listens
        /// to itself.
        shape: Option<ThingId>,
        hits: Box<[(u32, f32); MAX_KEY_HITS]>,
    },
    Delay {
        dsp: Box<Delay>,
        set: Box<DelaySettings>,
    },
    Reverb {
        room: Room,
        set: Box<ReverbSettings>,
    },
    Filter {
        dsp: Box<StereoFilter>,
        set: Box<FilterSettings>,
    },
    Distortion {
        dsp: Box<Shaper>,
        set: Box<DistortionSettings>,
    },
}

impl Fx {
    /// The effect for a layer of `kind` with these settings, or `None` for a kind this build
    /// doesn't know (it is passed over).
    fn build(kind: &str, p: &Params, scene: &Scene) -> Option<(Fx, MixLaw)> {
        Some(match kind {
            fx::EQ => {
                let set = Box::new(EqSettings::from_params(p));
                (Fx::Eq { dsp: Box::default(), analyser: Box::default(), set, count: 0 }, MixLaw::Gain)
            }
            fx::COMPRESSOR => {
                let set = CompressorSettings::from_params(p);
                let shape = match set.sc_shape {
                    Target::Thing(id) if scene.things.get(&id).is_some_and(|t| t.kind == SHAPE) => Some(id),
                    _ => None,
                };
                let hits = Box::new([(0, 0.0); MAX_KEY_HITS]);
                (Fx::Comp { dsp: Box::default(), set, shape, hits }, MixLaw::Gain)
            }
            fx::LIMITER => {
                let set = LimiterSettings::from_params(p);
                (Fx::Limiter { dsp: Box::new(TruePeakLimiter::new()), set }, MixLaw::Gain)
            }
            fx::CHORUS | fx::PHASER | fx::FLANGER => {
                let dsp = match kind {
                    fx::CHORUS => ModDsp::Chorus(Box::default()),
                    fx::PHASER => ModDsp::Phaser(Box::default()),
                    _ => ModDsp::Flanger(Box::default()),
                };
                (Fx::Mod { dsp, set: fx::mod_fx_settings(kind, p), rate_hz: 1.0 }, MixLaw::Power)
            }
            fx::DELAY => {
                let set = Box::new(DelaySettings::from_params(p));
                (Fx::Delay { dsp: Box::default(), set }, MixLaw::Power)
            }
            fx::REVERB => {
                let set = Box::new(ReverbSettings::from_params(p));
                (Fx::Reverb { room: Room::of(&set), set }, MixLaw::Power)
            }
            fx::FILTER => {
                let set = Box::new(FilterSettings::from_params(p));
                let dsp = Box::new(StereoFilter::new(filter_type(set.kind), set.slope == Slope::Db24));
                (Fx::Filter { dsp, set }, MixLaw::Gain)
            }
            fx::DISTORTION => {
                let set = Box::new(DistortionSettings::from_params(p));
                let factor = if set.kind.oversampled() { set.quality.factor() } else { 1 };
                let dsp = Box::new(Shaper::new(shaper_type(set.kind), factor));
                (Fx::Distortion { dsp, set }, MixLaw::Gain)
            }
            fx::WIDTH => {
                let set = WidthSettings::from_params(p);
                (Fx::Width { dsp: Box::default(), corr: Box::default(), set }, MixLaw::Gain)
            }
            _ => return None,
        })
    }

    /// Frames it delays the sound by.
    fn latency(&self) -> usize {
        match self {
            Fx::Limiter { dsp, .. } => dsp.latency(),
            Fx::Distortion { dsp, .. } => dsp.latency(),
            _ => 0,
        }
    }

    /// Silence, as new; a sweep starts where song time `time` puts it.
    fn reset(&mut self, time: f64) {
        match self {
            Fx::Limiter { dsp, .. } => dsp.reset(),
            Fx::Mod { dsp, rate_hz, .. } => {
                dsp.reset();
                dsp.set_phase((*rate_hz * time).fract());
            }
            Fx::Width { dsp, corr, .. } => {
                dsp.reset();
                corr.reset();
            }
            Fx::Eq { dsp, analyser, .. } => {
                dsp.reset();
                analyser.reset();
            }
            Fx::Comp { dsp, .. } => dsp.reset(),
            Fx::Delay { dsp, .. } => dsp.reset(),
            Fx::Reverb { room, .. } => room.reset(),
            Fx::Filter { dsp, .. } => dsp.reset(),
            Fx::Distortion { dsp, .. } => dsp.reset(),
        }
    }

    /// Takes over `old`'s memory when it is the same kind of effect (a patch swap); the
    /// caller hands it the new settings after.
    fn adopt(&mut self, old: &mut Fx) {
        match (self, old) {
            (Fx::Limiter { dsp, .. }, Fx::Limiter { dsp: o, .. }) => std::mem::swap(dsp, o),
            (Fx::Mod { dsp: ModDsp::Chorus(d), .. }, Fx::Mod { dsp: ModDsp::Chorus(o), .. }) => std::mem::swap(d, o),
            (Fx::Mod { dsp: ModDsp::Phaser(d), .. }, Fx::Mod { dsp: ModDsp::Phaser(o), .. }) => std::mem::swap(d, o),
            (Fx::Mod { dsp: ModDsp::Flanger(d), .. }, Fx::Mod { dsp: ModDsp::Flanger(o), .. }) => std::mem::swap(d, o),
            (Fx::Width { dsp, corr, .. }, Fx::Width { dsp: o, corr: oc, .. }) => {
                std::mem::swap(dsp, o);
                std::mem::swap(corr, oc);
            }
            (Fx::Eq { dsp, analyser, .. }, Fx::Eq { dsp: o, analyser: oa, .. }) => {
                std::mem::swap(dsp, o);
                std::mem::swap(analyser, oa);
            }
            (Fx::Comp { dsp, .. }, Fx::Comp { dsp: o, .. }) => std::mem::swap(dsp, o),
            (Fx::Delay { dsp, .. }, Fx::Delay { dsp: o, .. }) => std::mem::swap(dsp, o),
            (Fx::Reverb { room, .. }, Fx::Reverb { room: o, .. }) => room.adopt(o),
            // Another type, slope or quality starts empty.
            (Fx::Filter { dsp, .. }, Fx::Filter { dsp: o, .. })
                if dsp.kind() == o.kind() && dsp.slope24() == o.slope24() =>
            {
                std::mem::swap(dsp, o)
            }
            (Fx::Distortion { dsp, .. }, Fx::Distortion { dsp: o, .. })
                if dsp.kind() == o.kind() && dsp.factor() == o.factor() =>
            {
                std::mem::swap(dsp, o)
            }
            _ => {}
        }
    }

    /// Hands its settings, keyed where they have keys, to the sound-making part.
    fn apply(&mut self, k: &Keyed, tempo: &TempoMap) {
        match self {
            Fx::Limiter { dsp, set } => {
                dsp.set(k.get("ceiling", set.ceiling), k.get("release", set.release));
            }
            Fx::Mod { dsp, set, rate_hz } => {
                *rate_hz = if set.sync {
                    1.0 / note_seconds(set.rate_note, set.rate_feel, tempo).max(1e-3)
                } else {
                    k.get("rate", set.rate).clamp(0.01, 10.0) as f64
                };
                dsp.set(&ModParams {
                    rate_hz: *rate_hz as f32,
                    depth: k.get("depth", set.depth) / 100.0,
                    feedback: k.get("feedback", set.feedback) / 100.0,
                    voices: set.voices.clamp(2, 4) as u8,
                    stages: set.stages.clamp(2, 12) as u8,
                    width: k.get("width", set.width) / 100.0,
                });
            }
            Fx::Width { dsp, set, .. } => {
                dsp.set(k.get("width", set.width) / 100.0, k.get("mono_below", set.mono_below));
            }
            Fx::Eq { dsp, set, .. } => {
                let b = set.bands();
                let bands = std::array::from_fn(|i| {
                    let [f, g, q] = BAND_KEYS[i];
                    band_params(b[i], k.get(f, b[i].freq), k.get(g, b[i].gain), k.get(q, b[i].q))
                });
                dsp.set(&bands);
                dsp.set_output_db(k.get("output", set.output));
            }
            Fx::Comp { dsp, set, .. } => {
                dsp.set(&CompParams {
                    threshold_db: k.get("threshold", set.threshold),
                    ratio: k.get("ratio", set.ratio),
                    attack_ms: k.get("attack", set.attack),
                    release_ms: k.get("release", set.release),
                    knee_db: k.get("knee", set.knee),
                    makeup_db: k.get("makeup", set.makeup),
                    sc_low_cut_hz: k.get("sc_low_cut", set.sc_low_cut),
                    sc_high_cut_hz: k.get("sc_high_cut", set.sc_high_cut),
                });
            }
            Fx::Delay { dsp, set } => dsp.set(&delay_params(set, k, tempo)),
            Fx::Reverb { room, set } => room.set(&reverb_params(set, k)),
            Fx::Filter { dsp, set } => dsp.set(&filter_params(set, k, dsp.kind(), dsp.slope24())),
            Fx::Distortion { dsp, set } => dsp.set(&shaper_params(set, k)),
        }
    }

    /// Runs one block in place (wet only) and writes its meter; returns its gain reduction
    /// (dB, 0 or less) for the bus's meter.
    fn process(&mut self, l: &mut [f32; BLOCK], r: &mut [f32; BLOCK], cx: &FxCx, id: LayerId) -> f32 {
        match self {
            Fx::Limiter { dsp, .. } => {
                dsp.process(l, r);
                let gr = dsp.gain_reduction_db();
                cx.meters.write_fx(id.0, &FxReading { reduction: gr, ..FxReading::SILENT });
                gr
            }
            Fx::Mod { dsp, .. } => {
                dsp.process(l, r);
                0.0
            }
            Fx::Width { dsp, corr, .. } => {
                dsp.process(l, r);
                corr.push(l, r);
                cx.meters.write_fx(id.0, &FxReading { value: corr.value(), ..FxReading::SILENT });
                0.0
            }
            Fx::Eq { dsp, analyser, set, count } => {
                if set.analyser == AnalyserMode::Pre {
                    analyser.push(l, r);
                }
                dsp.process(l, r);
                if set.analyser == AnalyserMode::Post {
                    analyser.push(l, r);
                }
                *count += 1;
                if set.analyser != AnalyserMode::Off && *count % ANALYSER_EVERY == 0 {
                    cx.meters.write_fx_bins(id.0, analyser.bins());
                }
                0.0
            }
            Fx::Comp { dsp, set, shape, hits } => {
                let own = cx.bus;
                match (set.sidechain, shape) {
                    (Sidechain::Bus, _)
                        if (1..=BUSES as i32).contains(&set.sc_bus) && set.sc_bus as usize - 1 != own =>
                    {
                        let [kl, kr] = &cx.buses[set.sc_bus as usize - 1];
                        dsp.process(l, r, Key::External(kl, kr));
                    }
                    (Sidechain::Hits, Some(shape)) => {
                        let mut n = 0;
                        for h in cx.hits {
                            let on_shape = h.event.a.thing == *shape || h.event.b.is_some_and(|b| b.thing == *shape);
                            if on_shape && n < MAX_KEY_HITS {
                                hits[n] = (h.offset, strength(h.event.speed));
                                n += 1;
                            }
                        }
                        dsp.process(l, r, Key::Hits(&hits[..n]));
                    }
                    _ => dsp.process(l, r, Key::SelfInput),
                }
                let gr = dsp.gain_reduction_db();
                let reading = FxReading { reduction: gr, input: dsp.input_db(), output: dsp.output_db(), value: 0.0 };
                cx.meters.write_fx(id.0, &reading);
                gr
            }
            Fx::Delay { dsp, .. } => {
                dsp.process(l, r);
                0.0
            }
            Fx::Reverb { room, .. } => {
                room.process(l, r);
                0.0
            }
            Fx::Filter { dsp, .. } => {
                dsp.process(l, r);
                0.0
            }
            Fx::Distortion { dsp, .. } => {
                dsp.process(l, r);
                0.0
            }
        }
    }
}

/// One effect in the chain.
struct Slot {
    id: LayerId,
    kind: String,
    fx: Fx,
    law: MixLaw,
    keyed: Keyed,
    /// Mix, 0..1, as set.
    mix_base: f32,
    mix: Smoother,
    /// 1 while on, 0 while bypassed.
    on: Smoother,
    /// It was bypassed and silent: its memory is cleared before it comes back.
    asleep: bool,
    /// Samples the switch-on fade waits after waking, while a lookahead effect (the Limiter)
    /// fills its delay: fading in before then would fade the dry out into silence.
    wake_wait: usize,
    dry: [[f32; BLOCK]; 2],
    /// Lines the dry sound up with the wet when the effect delays it.
    dry_lag: Lag,
}

/// A bus's effects in order.
#[derive(Default)]
pub struct FxChain {
    slots: Vec<Slot>,
    latency: usize,
    tempo: TempoMap,
    /// The start of the last block run, in song seconds.
    time: f64,
}

/// How long switching an effect on or off fades.
const BYPASS_MS: f32 = 10.0;

impl FxChain {
    /// The chain of bus `lane` (0..2) from the scene's mixer card's effect layers, in their
    /// order, bypassed ones included (they fade in and out when switched).
    pub fn build(scene: &Scene, lane: u8, registry: &bpe_core::Registry, tempo: &TempoMap) -> FxChain {
        let mut slots = Vec::new();
        let Some(card) = scene.card(ThingId::ROOT, SlotId::Mixer).filter(|c| c.kind == MIXER) else {
            return FxChain::default();
        };
        for layer in card.layers.iter().filter(|l| fx::is_fx(&l.kind) && l.lane == Some(lane)) {
            let Some(kind) = registry.layer(&layer.kind).filter(|k| k.card() == MIXER) else { continue };
            let defs = kind.settings();
            let mut p = Params(defs.iter().map(|d| (d.key.to_owned(), d.default.clone())).collect());
            for d in defs {
                if let Some(v) = card.params.get(&layered_key(layer.id, d.key)) {
                    p.set(d.key, v.clone());
                }
            }
            let Some((fx, law)) = Fx::build(&layer.kind, &p, scene) else { continue };
            let mix_base = match p.get("mix") {
                Some(Value::Number(m)) => (*m as f32 / 100.0).clamp(0.0, 1.0),
                _ => 1.0,
            };
            let mut mix = Smoother::new(Smoother::KNOB_MS);
            mix.reset(mix_base);
            let mut on = Smoother::new(BYPASS_MS);
            on.reset(if layer.enabled { 1.0 } else { 0.0 });
            let mut dry_lag = Lag::new();
            dry_lag.set(fx.latency());
            slots.push(Slot {
                id: layer.id,
                kind: layer.kind.clone(),
                fx,
                law,
                keyed: Keyed::of(scene, layer.id, defs),
                mix_base,
                mix,
                on,
                asleep: !layer.enabled,
                wake_wait: 0,
                dry: [[0.0; BLOCK]; 2],
                dry_lag,
            });
        }
        let latency = slots.iter().map(|s| s.fx.latency()).sum();
        let mut chain = FxChain { slots, latency, tempo: *tempo, time: 0.0 };
        chain.apply_all(0.0);
        chain
    }

    /// Frames the chain delays the sound by.
    pub fn latency(&self) -> usize {
        self.latency
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// The layers in it, in order.
    pub fn layers(&self) -> impl Iterator<Item = LayerId> + '_ {
        self.slots.iter().map(|s| s.id)
    }

    fn apply_all(&mut self, time: f64) {
        for s in &mut self.slots {
            s.keyed.at(time);
            s.fx.apply(&s.keyed, &self.tempo);
            s.mix.set_target(s.keyed.get("mix", s.mix_base * 100.0) / 100.0);
        }
    }

    /// Silence, as new, every setting where it is at `time` (song seconds).
    pub fn reset(&mut self, time: f64) {
        self.apply_all(time);
        for s in &mut self.slots {
            s.fx.reset(time);
            s.dry_lag.reset();
            let (m, o) = (s.mix.target(), s.on.target());
            s.mix.reset(m);
            s.on.reset(o);
            s.asleep = o == 0.0;
        }
    }

    /// Takes over the memory of `old`'s effects of the same layer and kind, so tails ring on.
    pub fn adopt(&mut self, old: &mut FxChain) {
        for s in &mut self.slots {
            let Some(o) = old.slots.iter_mut().find(|o| o.id == s.id && o.kind == s.kind) else { continue };
            s.fx.adopt(&mut o.fx);
            if s.fx.latency() == o.fx.latency() {
                std::mem::swap(&mut s.dry_lag, &mut o.dry_lag);
            }
            let (m, on) = (s.mix.target(), s.on.target());
            s.mix.reset(o.mix.value());
            s.mix.set_target(m);
            s.on.reset(o.on.value());
            s.on.set_target(on);
            s.asleep = o.asleep;
            s.wake_wait = o.wake_wait;
        }
        // The adopted parts take on the new settings.
        let time = old.time;
        self.apply_all(time);
        self.time = time;
    }

    /// Runs one block of the bus's sound in place; returns the most any effect turned it down
    /// (dB, 0 or less). Never allocates.
    pub fn process(&mut self, l: &mut [f32; BLOCK], r: &mut [f32; BLOCK], cx: &FxCx) -> f32 {
        let mut reduction = 0.0f32;
        self.time = cx.time;
        for s in &mut self.slots {
            if !s.keyed.is_empty() {
                s.keyed.at(cx.time);
                s.fx.apply(&s.keyed, cx.tempo);
                s.mix.set_target(s.keyed.get("mix", s.mix_base * 100.0) / 100.0);
            }
            if s.on.target() == 0.0 && s.on.settled() {
                // Bypassed: the sound passes, delayed as the effect would have (so the bus's
                // timing never changes), and the effect rests.
                s.asleep = true;
                s.dry_lag.process(l, r);
                continue;
            }
            if s.asleep {
                s.fx.reset(cx.time);
                s.asleep = false;
                s.wake_wait = s.fx.latency();
            }
            s.dry[0] = *l;
            s.dry[1] = *r;
            let [dl, dr] = &mut s.dry;
            s.dry_lag.process(dl, dr);
            let gr = s.fx.process(l, r, cx, s.id);
            reduction = reduction.min(gr);
            for i in 0..BLOCK {
                let m = s.mix.next();
                let on = if s.wake_wait > 0 {
                    s.wake_wait -= 1;
                    s.on.value()
                } else {
                    s.on.next()
                };
                let w = m * on;
                let (gd, gw) = match s.law {
                    MixLaw::Gain => (1.0 - w, w),
                    MixLaw::Power => {
                        let a = w * std::f32::consts::FRAC_PI_2;
                        // The dry fades out only as far as the effect is on.
                        (1.0 - on + on * a.cos(), a.sin())
                    }
                };
                l[i] = s.dry[0][i] * gd + l[i] * gw;
                r[i] = s.dry[1][i] * gd + r[i] * gw;
            }
        }
        reduction
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{SlotId, Value};

    fn chain_with(kind: &str, set: &[(&str, Value)], enabled: bool) -> FxChain {
        let kit = bpe_kit::kit();
        let reg = &kit.registry;
        let mut scene = bpe_kit::new_studio_scene(reg);
        let id = scene.add_layer_in(reg, ThingId::ROOT, SlotId::Mixer, kind, Some(0)).unwrap();
        scene.set_layer_enabled(reg, ThingId::ROOT, SlotId::Mixer, id, enabled).unwrap();
        let card = scene.card_mut(ThingId::ROOT, SlotId::Mixer).unwrap();
        for (k, v) in set {
            card.params.set(&layered_key(id, k), v.clone());
        }
        FxChain::build(&scene, 0, reg, &TempoMap::default())
    }

    fn run(chain: &mut FxChain, blocks: usize) -> Vec<(f32, f32)> {
        let meters = Meters::default();
        let buses = [[[0.0; BLOCK]; 2]; BUSES];
        let tempo = TempoMap::default();
        let mut out = Vec::new();
        let mut phase = 0.0f32;
        for b in 0..blocks {
            let mut l = [0.0; BLOCK];
            let mut r = [0.0; BLOCK];
            for i in 0..BLOCK {
                phase += 0.05;
                l[i] = phase.sin() * 0.5;
                r[i] = (phase * 1.3).sin() * 0.3;
            }
            let cx = FxCx {
                time: b as f64 * 64.0 / 48_000.0,
                tempo: &tempo,
                hits: &[],
                buses: &buses,
                bus: 0,
                meters: &meters,
            };
            chain.process(&mut l, &mut r, &cx);
            out.extend(l.iter().zip(&r).map(|(a, b)| (*a, *b)));
        }
        out
    }

    #[test]
    fn synced_notes_follow_the_tempo() {
        let t = TempoMap::new(120.0, 4, 4);
        assert!((note_seconds(NoteLength::N8, Feel::Straight, &t) - 0.25).abs() < 1e-12);
        assert!((note_seconds(NoteLength::N8, Feel::Dotted, &t) - 0.375).abs() < 1e-12);
        assert!((note_seconds(NoteLength::N4, Feel::Triplet, &t) - 1.0 / 3.0).abs() < 1e-12);
        let waltz = TempoMap::new(120.0, 3, 4);
        assert!((note_seconds(NoteLength::Bar, Feel::Straight, &waltz) - 1.5).abs() < 1e-12);
    }

    #[test]
    fn a_width_of_zero_makes_the_bus_mono() {
        let mut c = chain_with(fx::WIDTH, &[("width", Value::Number(0.0))], true);
        let out = run(&mut c, 20);
        assert!(out[BLOCK * 4..].iter().all(|(l, r)| (l - r).abs() < 1e-6));
    }

    #[test]
    fn a_bypassed_effect_passes_the_sound() {
        let mut c = chain_with(fx::WIDTH, &[("width", Value::Number(0.0))], false);
        let mut plain = FxChain::default();
        assert_eq!(run(&mut c, 10), run(&mut plain, 10));
        let mut c = chain_with(fx::CHORUS, &[("mix", Value::Number(0.0))], true);
        let a = run(&mut c, 10);
        let b = run(&mut plain, 10);
        assert!(a.iter().zip(&b).all(|(x, y)| (x.0 - y.0).abs() < 1e-6), "Mix 0 is dry");
    }

    #[test]
    fn missing_sidechain_shape_falls_back_to_self() {
        let loud = [("threshold", Value::Number(-40.0)), ("ratio", Value::Number(10.0))];
        let mut own = chain_with(fx::COMPRESSOR, &loud, true);
        let mut gone = chain_with(
            fx::COMPRESSOR,
            &[
                loud[0].clone(),
                loud[1].clone(),
                ("sidechain", Value::Choice("hits".into())),
                ("sc_shape", Value::Target(Target::Thing(ThingId(999)))),
            ],
            true,
        );
        let a = run(&mut own, 40);
        let b = run(&mut gone, 40);
        assert_eq!(a, b, "the deleted shape's compressor listens to itself");
        let mut plain = FxChain::default();
        let dry = run(&mut plain, 40);
        let level = |x: &[(f32, f32)]| x[BLOCK * 20..].iter().fold(0.0f32, |m, v| m.max(v.0.abs()));
        assert!(level(&a) < level(&dry) * 0.5, "and it does compress");
    }

    /// A click at the start, through `chain`, for `blocks` blocks (left channel).
    fn click(chain: &mut FxChain, blocks: usize, tempo: &TempoMap) -> Vec<f32> {
        let meters = Meters::default();
        let buses = [[[0.0; BLOCK]; 2]; BUSES];
        let mut out = Vec::new();
        for b in 0..blocks {
            let mut l = [0.0; BLOCK];
            let mut r = [0.0; BLOCK];
            if b == 0 {
                l[0] = 1.0;
                r[0] = 1.0;
            }
            let time = b as f64 * BLOCK as f64 / 48_000.0;
            let cx = FxCx { time, tempo, hits: &[], buses: &buses, bus: 0, meters: &meters };
            chain.process(&mut l, &mut r, &cx);
            out.extend_from_slice(&l);
        }
        out
    }

    #[test]
    fn a_synced_delay_echoes_on_the_beat() {
        let set = [
            ("sync", Value::Bool(true)),
            ("time_note", Value::Choice("n4".into())),
            ("time_feel", Value::Choice("straight".into())),
            ("feedback", Value::Number(0.0)),
            ("mix", Value::Number(100.0)),
        ];
        let mut c = chain_with(fx::DELAY, &set, true);
        // Built at 120 bpm (the default tempo map): a quarter note is half a second.
        let out = click(&mut c, 48_000 / BLOCK, &TempoMap::default());
        let loudest =
            out.iter().enumerate().fold((0, 0.0f32), |m, (i, v)| if v.abs() > m.1 { (i, v.abs()) } else { m });
        assert!(loudest.0.abs_diff(24_000) <= 2, "the echo lands at {}", loudest.0);
    }

    #[test]
    fn a_reverb_rings_after_the_click() {
        for kind in ["plate", "hall", "spring"] {
            let set =
                [("kind", Value::Choice(kind.into())), ("decay", Value::Number(1.0)), ("mix", Value::Number(100.0))];
            let mut c = chain_with(fx::REVERB, &set, true);
            let out = click(&mut c, 48_000 / BLOCK, &TempoMap::default());
            let tail = out[4_800..9_600].iter().fold(0.0f32, |m, v| m.max(v.abs()));
            let late = out[out.len() - 4_800..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(tail > 1e-3, "{kind} rings: {tail}");
            assert!(late < tail * 0.1, "{kind} dies away: {late} after {tail}");
        }
    }

    #[test]
    fn a_bus_low_pass_turns_the_sine_down() {
        // The left sine is 382 Hz: a 100 Hz 24 dB low pass takes it down by about 30 dB.
        let set = [("cutoff", Value::Number(100.0)), ("slope", Value::Choice("db24".into()))];
        let mut c = chain_with(fx::FILTER, &set, true);
        let out = run(&mut c, 60);
        let peak = out[BLOCK * 40..].iter().fold(0.0f32, |m, v| m.max(v.0.abs()));
        assert!(peak < 0.5 * bpe_dsp::db_to_gain(-20.0), "{peak}");
        // And at Mix 0 it is the dry sound.
        let mut dry = chain_with(fx::FILTER, &[set[0].clone(), ("mix", Value::Number(0.0))], true);
        let mut plain = FxChain::default();
        assert!(run(&mut dry, 10).iter().zip(run(&mut plain, 10)).all(|(a, b)| (a.0 - b.0).abs() < 1e-6));
    }

    #[test]
    fn a_bus_distortion_squares_the_sine_and_counts_its_delay() {
        let set = [("kind", Value::Choice("hard_clip".into())), ("drive", Value::Number(24.0))];
        let mut c = chain_with(fx::DISTORTION, &set, true);
        assert!(c.latency() > 0, "the 4x quality delays the sound");
        let out = run(&mut c, 60);
        let tail = &out[BLOCK * 40..];
        let peak = tail.iter().fold(0.0f32, |m, v| m.max(v.0.abs()));
        // A hard clip spends much of the cycle near its peak; a sine spends little.
        let near = tail.iter().filter(|v| v.0.abs() > 0.9 * peak).count() as f32 / tail.len() as f32;
        assert!(near > 0.5, "{near}");
        let crush = chain_with(fx::DISTORTION, &[("kind", Value::Choice("bitcrush".into()))], true);
        assert_eq!(crush.latency(), 0, "bitcrush runs at the plain rate");
    }

    #[test]
    fn an_eq_bell_cuts_its_band() {
        let mut c = chain_with(
            fx::EQ,
            &[
                ("band1_on", Value::Bool(false)),
                ("band5_on", Value::Bool(true)),
                ("band5_freq", Value::Number(382.0)),
                ("band5_gain", Value::Number(-12.0)),
            ],
            true,
        );
        // The test signal's left channel is a sine at 0.05 rad a sample: 382 Hz.
        let out = run(&mut c, 60);
        let peak = out[BLOCK * 40..].iter().fold(0.0f32, |m, v| m.max(v.0.abs()));
        assert!((peak - 0.5 * bpe_dsp::db_to_gain(-12.0)).abs() < 0.02, "{peak}");
    }
}
