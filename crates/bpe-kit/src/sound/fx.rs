//! The bus effects: layers of the scene's `sound.mixer` card in section Effect, each in the lane
//! of its bus (0..2). Every effect has an on switch (the layer's own), and a Mix (dry against
//! wet). The sound engine reads these settings; no sound is made here.

use super::mixer::MIXER;
use super::voice::{full_params, prefixed_defs};
use bpe_core::{
    Choice, GroupOpts, LayerKind, LayerSection, Params, Registry, SettingDef, SettingGroup, Settings, Target,
};

pub const EQ: &str = "fx.eq";
pub const COMPRESSOR: &str = "fx.compressor";
pub const DELAY: &str = "fx.delay";
pub const REVERB: &str = "fx.reverb";
pub const LIMITER: &str = "fx.limiter";
pub const CHORUS: &str = "fx.chorus";
pub const PHASER: &str = "fx.phaser";
pub const FLANGER: &str = "fx.flanger";
pub const WIDTH: &str = "fx.width";

/// Every bus effect kind, in the add menu's order.
pub const FX_KINDS: [&str; 9] = [EQ, COMPRESSOR, DELAY, REVERB, LIMITER, CHORUS, PHASER, FLANGER, WIDTH];

/// A note length a synced time or rate is set in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum NoteLength {
    N64,
    N32,
    N16,
    #[default]
    N8,
    N4,
    N2,
    Bar,
    TwoBars,
}

impl NoteLength {
    pub const ALL: [NoteLength; 8] = [
        NoteLength::N64,
        NoteLength::N32,
        NoteLength::N16,
        NoteLength::N8,
        NoteLength::N4,
        NoteLength::N2,
        NoteLength::Bar,
        NoteLength::TwoBars,
    ];

    pub fn label(self) -> &'static str {
        match self {
            NoteLength::N64 => "1/64",
            NoteLength::N32 => "1/32",
            NoteLength::N16 => "1/16",
            NoteLength::N8 => "1/8",
            NoteLength::N4 => "1/4",
            NoteLength::N2 => "1/2",
            NoteLength::Bar => "1 bar",
            NoteLength::TwoBars => "2 bars",
        }
    }

    /// Its length in whole notes (a 4/4 bar is 1).
    pub fn wholes(self) -> f64 {
        match self {
            NoteLength::N64 => 1.0 / 64.0,
            NoteLength::N32 => 1.0 / 32.0,
            NoteLength::N16 => 1.0 / 16.0,
            NoteLength::N8 => 1.0 / 8.0,
            NoteLength::N4 => 1.0 / 4.0,
            NoteLength::N2 => 1.0 / 2.0,
            NoteLength::Bar => 1.0,
            NoteLength::TwoBars => 2.0,
        }
    }
}

/// Straight, dotted (half as long again) or triplet (two thirds).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Feel {
    #[default]
    Straight,
    Dotted,
    Triplet,
}

impl Feel {
    pub const ALL: [Feel; 3] = [Feel::Straight, Feel::Dotted, Feel::Triplet];

    pub fn label(self) -> &'static str {
        match self {
            Feel::Straight => "Straight",
            Feel::Dotted => "Dotted",
            Feel::Triplet => "Triplet",
        }
    }

    pub fn factor(self) -> f64 {
        match self {
            Feel::Straight => 1.0,
            Feel::Dotted => 1.5,
            Feel::Triplet => 2.0 / 3.0,
        }
    }
}

/// What an EQ's analyser shows behind the curve.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum AnalyserMode {
    /// The sound after the EQ.
    #[default]
    Post,
    /// The sound going in.
    Pre,
    Off,
}

/// An EQ band's shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum BandKind {
    LowCut,
    LowShelf,
    #[default]
    Bell,
    Notch,
    HighShelf,
    HighCut,
}

impl BandKind {
    pub const ALL: [BandKind; 6] =
        [BandKind::LowCut, BandKind::LowShelf, BandKind::Bell, BandKind::Notch, BandKind::HighShelf, BandKind::HighCut];

    pub fn label(self) -> &'static str {
        match self {
            BandKind::LowCut => "Low cut",
            BandKind::LowShelf => "Low shelf",
            BandKind::Bell => "Bell",
            BandKind::Notch => "Notch",
            BandKind::HighShelf => "High shelf",
            BandKind::HighCut => "High cut",
        }
    }

    /// Whether the band has a Gain (cuts and notches have none).
    pub fn has_gain(self) -> bool {
        matches!(self, BandKind::LowShelf | BandKind::Bell | BandKind::HighShelf)
    }

    /// Whether the band has a Slope.
    pub fn is_cut(self) -> bool {
        matches!(self, BandKind::LowCut | BandKind::HighCut)
    }
}

/// How steep a cut is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Slope {
    Db12,
    #[default]
    Db24,
    Db48,
}

impl Slope {
    pub const ALL: [Slope; 3] = [Slope::Db12, Slope::Db24, Slope::Db48];

    pub fn label(self) -> &'static str {
        match self {
            Slope::Db12 => "12 dB",
            Slope::Db24 => "24 dB",
            Slope::Db48 => "48 dB",
        }
    }

    /// Second-order sections it takes.
    pub fn sections(self) -> usize {
        match self {
            Slope::Db12 => 1,
            Slope::Db24 => 2,
            Slope::Db48 => 4,
        }
    }
}

/// Which part of the stereo sound a band works on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum StereoPlace {
    #[default]
    Both,
    /// The middle: what left and right share.
    Mid,
    /// The sides: how left and right differ.
    Side,
}

impl StereoPlace {
    pub const ALL: [StereoPlace; 3] = [StereoPlace::Both, StereoPlace::Mid, StereoPlace::Side];

    pub fn label(self) -> &'static str {
        match self {
            StereoPlace::Both => "Both",
            StereoPlace::Mid => "Mid",
            StereoPlace::Side => "Side",
        }
    }
}

/// One EQ band. The EQ holds eight, under `band1_`..`band8_`.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct EqBand {
    #[setting(label = "On")]
    pub on: bool,
    #[setting(label = "Shape")]
    pub kind: BandKind,
    #[setting(label = "Frequency", range(20, 20000), unit = "Hz", keyframe)]
    pub freq: f32,
    #[setting(label = "Gain", range(-18, 18), unit = "dB", keyframe)]
    pub gain: f32,
    #[setting(label = "Q", range(0.1, 18), keyframe)]
    pub q: f32,
    #[setting(label = "Slope")]
    pub slope: Slope,
    #[setting(label = "Stereo")]
    pub place: StereoPlace,
}

impl Default for EqBand {
    fn default() -> Self {
        EqBand::bell(1000.0)
    }
}

impl EqBand {
    /// A flat Bell at `freq`, off.
    pub fn bell(freq: f32) -> Self {
        Self { on: false, kind: BandKind::Bell, freq, gain: 0.0, q: 0.71, slope: Slope::Db24, place: StereoPlace::Both }
    }
}

/// The frequencies bands 2..8 start at.
pub const BAND_FREQS: [f32; 8] = [30.0, 60.0, 150.0, 400.0, 1000.0, 2500.0, 6000.0, 12000.0];

impl SettingGroup for EqBand {
    fn group_defs(o: &GroupOpts, default: &Self) -> Vec<SettingDef> {
        prefixed_defs(o, default)
    }

    fn group_read(o: &GroupOpts, default: &Self, p: &Params) -> Self {
        let mut own = full_params(default);
        for (k, v) in p.iter() {
            if let Some(plain) = k.strip_prefix(o.prefix) {
                own.set(plain, v.clone());
            }
        }
        Self::from_params(&own)
    }

    fn group_write(&self, o: &GroupOpts, default: &Self, p: &mut Params) {
        let (mine, base) = (full_params(self), full_params(default));
        for (k, v) in mine.iter() {
            if base.get(k) != Some(v) {
                p.set(&format!("{}{k}", o.prefix), v.clone());
            }
        }
    }
}

/// The 8-band equalizer.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct EqSettings {
    #[setting(paint, prefix = "band1_", label = "Band 1", group = "Band 1")]
    pub band1: EqBand,
    #[setting(paint, prefix = "band2_", label = "Band 2", group = "Band 2")]
    pub band2: EqBand,
    #[setting(paint, prefix = "band3_", label = "Band 3", group = "Band 3")]
    pub band3: EqBand,
    #[setting(paint, prefix = "band4_", label = "Band 4", group = "Band 4")]
    pub band4: EqBand,
    #[setting(paint, prefix = "band5_", label = "Band 5", group = "Band 5")]
    pub band5: EqBand,
    #[setting(paint, prefix = "band6_", label = "Band 6", group = "Band 6")]
    pub band6: EqBand,
    #[setting(paint, prefix = "band7_", label = "Band 7", group = "Band 7")]
    pub band7: EqBand,
    #[setting(paint, prefix = "band8_", label = "Band 8", group = "Band 8")]
    pub band8: EqBand,
    /// What the analyser behind the curve shows.
    #[setting(label = "Analyser")]
    pub analyser: AnalyserMode,
    /// The level coming out.
    #[setting(label = "Output", range(-24, 24), unit = "dB", keyframe)]
    pub output: f32,
    /// Dry against wet.
    #[setting(label = "Mix", range(0, 100), unit = "%", keyframe)]
    pub mix: f32,
}

impl Default for EqSettings {
    fn default() -> Self {
        let b = |i: usize| EqBand::bell(BAND_FREQS[i]);
        Self {
            band1: EqBand { on: true, kind: BandKind::LowCut, ..b(0) },
            band2: b(1),
            band3: b(2),
            band4: b(3),
            band5: b(4),
            band6: b(5),
            band7: b(6),
            band8: b(7),
            analyser: AnalyserMode::Post,
            output: 0.0,
            mix: 100.0,
        }
    }
}

impl EqSettings {
    /// The eight bands in order.
    pub fn bands(&self) -> [&EqBand; 8] {
        [&self.band1, &self.band2, &self.band3, &self.band4, &self.band5, &self.band6, &self.band7, &self.band8]
    }

    /// The layer key of band `n`'s (1..=8) setting `key` (`band3_freq`).
    pub fn band_key(n: usize, key: &str) -> String {
        format!("band{n}_{key}")
    }
}

/// What a compressor listens to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Sidechain {
    /// The sound it compresses.
    #[default]
    SelfInput,
    /// Another bus's sound.
    Bus,
    /// Each hit on a chosen shape ducks it.
    Hits,
}

impl Sidechain {
    pub const ALL: [Sidechain; 3] = [Sidechain::SelfInput, Sidechain::Bus, Sidechain::Hits];

    pub fn label(self) -> &'static str {
        match self {
            Sidechain::SelfInput => "Self",
            Sidechain::Bus => "Bus",
            Sidechain::Hits => "Hits",
        }
    }
}

/// The compressor.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct CompressorSettings {
    /// Above this the sound is turned down.
    #[setting(label = "Threshold", range(-60, 0), unit = "dB", keyframe)]
    pub threshold: f32,
    /// How hard: 4 lets 1 dB through for every 4 over.
    #[setting(label = "Ratio", range(1, 20), keyframe)]
    pub ratio: f32,
    /// How quickly it turns down.
    #[setting(label = "Attack", range(0.1, 100), unit = "ms", keyframe)]
    pub attack: f32,
    /// How quickly it lets go.
    #[setting(label = "Release", range(5, 2000), unit = "ms", keyframe)]
    pub release: f32,
    /// How gently it starts around the threshold.
    #[setting(label = "Knee", range(0, 24), unit = "dB", keyframe)]
    pub knee: f32,
    /// Turns the result back up.
    #[setting(label = "Makeup", range(0, 24), unit = "dB", keyframe)]
    pub makeup: f32,
    /// Dry against wet.
    #[setting(label = "Mix", range(0, 100), unit = "%", keyframe)]
    pub mix: f32,
    /// What it listens to.
    #[setting(label = "Sidechain")]
    pub sidechain: Sidechain,
    /// The bus it listens to.
    #[setting(label = "Sidechain bus", range(1, 3), when = "sidechain=bus")]
    pub sc_bus: i32,
    /// The shape whose hits duck it.
    #[setting(label = "Sidechain shape", when = "sidechain=hits")]
    pub sc_shape: Target,
    /// Lows below this don't count (all the way down is off).
    #[setting(label = "Sidechain low cut", range(20, 2000), unit = "Hz", keyframe, when = "sidechain!=hits")]
    pub sc_low_cut: f32,
    /// Highs above this don't count (all the way up is off).
    #[setting(label = "Sidechain high cut", range(1000, 20000), unit = "Hz", keyframe, when = "sidechain!=hits")]
    pub sc_high_cut: f32,
}

impl Default for CompressorSettings {
    fn default() -> Self {
        Self {
            threshold: -18.0,
            ratio: 4.0,
            attack: 10.0,
            release: 120.0,
            knee: 6.0,
            makeup: 0.0,
            mix: 100.0,
            sidechain: Sidechain::SelfInput,
            sc_bus: 1,
            sc_shape: Target::Anything,
            sc_low_cut: 20.0,
            sc_high_cut: 20000.0,
        }
    }
}

/// How the delay's echoes are laid out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum DelayMode {
    #[default]
    Standard,
    /// Echoes bounce between left and right.
    PingPong,
    /// Four taps, each with its own time, level and pan.
    MultiTap,
}

impl DelayMode {
    pub const ALL: [DelayMode; 3] = [DelayMode::Standard, DelayMode::PingPong, DelayMode::MultiTap];

    pub fn label(self) -> &'static str {
        match self {
            DelayMode::Standard => "Standard",
            DelayMode::PingPong => "Ping-pong",
            DelayMode::MultiTap => "Multi-tap",
        }
    }
}

/// How each repeat sounds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum DelayCharacter {
    /// Clean repeats.
    #[default]
    Digital,
    /// Each repeat a little darker and softly saturated.
    Analog,
    /// Wow, flutter, saturation and a little noise; the pitch bends when the time changes.
    Tape,
}

impl DelayCharacter {
    pub const ALL: [DelayCharacter; 3] = [DelayCharacter::Digital, DelayCharacter::Analog, DelayCharacter::Tape];

    pub fn label(self) -> &'static str {
        match self {
            DelayCharacter::Digital => "Digital",
            DelayCharacter::Analog => "Analog",
            DelayCharacter::Tape => "Tape",
        }
    }
}

/// The delay.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct DelaySettings {
    #[setting(label = "Mode")]
    pub mode: DelayMode,
    #[setting(label = "Character")]
    pub character: DelayCharacter,
    /// Time in note lengths at the scene's tempo, or in milliseconds.
    #[setting(label = "Sync to tempo")]
    pub sync: bool,
    #[setting(label = "Time", range(1, 4000), unit = "ms", keyframe, when = "sync=false")]
    pub time_ms: f32,
    #[setting(label = "Time", when = "sync=true")]
    pub time_note: NoteLength,
    #[setting(label = "Feel", when = "sync=true")]
    pub time_feel: Feel,
    /// How much of each echo comes back again.
    #[setting(label = "Feedback", range(0, 100), unit = "%", keyframe)]
    pub feedback: f32,
    /// Lows below this leave the echoes (all the way down is off).
    #[setting(label = "Low cut", range(20, 2000), unit = "Hz", keyframe)]
    pub low_cut: f32,
    /// Highs above this leave the echoes (all the way up is off).
    #[setting(label = "High cut", range(1000, 20000), unit = "Hz", keyframe)]
    pub high_cut: f32,
    /// A peak at the cuts.
    #[setting(label = "Resonance", range(0, 100), unit = "%", keyframe)]
    pub reso: f32,
    /// How wide the echoes are.
    #[setting(label = "Width", range(0, 100), unit = "%", keyframe)]
    pub width: f32,
    #[setting(label = "Tap 1 time", range(1, 100), unit = "%", keyframe, when = "mode=multi_tap")]
    pub tap1_time: f32,
    #[setting(label = "Tap 1 level", range(-60, 0), unit = "dB", keyframe, when = "mode=multi_tap")]
    pub tap1_level: f32,
    #[setting(label = "Tap 1 pan", range(-100, 100), keyframe, when = "mode=multi_tap")]
    pub tap1_pan: f32,
    #[setting(label = "Tap 2 time", range(1, 100), unit = "%", keyframe, when = "mode=multi_tap")]
    pub tap2_time: f32,
    #[setting(label = "Tap 2 level", range(-60, 0), unit = "dB", keyframe, when = "mode=multi_tap")]
    pub tap2_level: f32,
    #[setting(label = "Tap 2 pan", range(-100, 100), keyframe, when = "mode=multi_tap")]
    pub tap2_pan: f32,
    #[setting(label = "Tap 3 time", range(1, 100), unit = "%", keyframe, when = "mode=multi_tap")]
    pub tap3_time: f32,
    #[setting(label = "Tap 3 level", range(-60, 0), unit = "dB", keyframe, when = "mode=multi_tap")]
    pub tap3_level: f32,
    #[setting(label = "Tap 3 pan", range(-100, 100), keyframe, when = "mode=multi_tap")]
    pub tap3_pan: f32,
    #[setting(label = "Tap 4 time", range(1, 100), unit = "%", keyframe, when = "mode=multi_tap")]
    pub tap4_time: f32,
    #[setting(label = "Tap 4 level", range(-60, 0), unit = "dB", keyframe, when = "mode=multi_tap")]
    pub tap4_level: f32,
    #[setting(label = "Tap 4 pan", range(-100, 100), keyframe, when = "mode=multi_tap")]
    pub tap4_pan: f32,
    /// Dry against wet.
    #[setting(label = "Mix", range(0, 100), unit = "%", keyframe)]
    pub mix: f32,
}

impl Default for DelaySettings {
    fn default() -> Self {
        Self {
            mode: DelayMode::Standard,
            character: DelayCharacter::Digital,
            sync: true,
            time_ms: 375.0,
            time_note: NoteLength::N8,
            time_feel: Feel::Dotted,
            feedback: 35.0,
            low_cut: 80.0,
            high_cut: 9000.0,
            reso: 0.0,
            width: 100.0,
            tap1_time: 25.0,
            tap1_level: 0.0,
            tap1_pan: -50.0,
            tap2_time: 50.0,
            tap2_level: -3.0,
            tap2_pan: 50.0,
            tap3_time: 75.0,
            tap3_level: -6.0,
            tap3_pan: -100.0,
            tap4_time: 100.0,
            tap4_level: -9.0,
            tap4_pan: 100.0,
            mix: 30.0,
        }
    }
}

/// The reverb's kind of room.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ReverbKind {
    #[default]
    Plate,
    Hall,
    Spring,
}

impl ReverbKind {
    pub const ALL: [ReverbKind; 3] = [ReverbKind::Plate, ReverbKind::Hall, ReverbKind::Spring];

    pub fn label(self) -> &'static str {
        match self {
            ReverbKind::Plate => "Plate",
            ReverbKind::Hall => "Hall",
            ReverbKind::Spring => "Spring",
        }
    }
}

/// How many lines the hall's network has.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum HallLines {
    Eight,
    #[default]
    Sixteen,
}

/// The reverb.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ReverbSettings {
    #[setting(label = "Kind")]
    pub kind: ReverbKind,
    /// A wait before the reverb starts.
    #[setting(label = "Pre-delay", range(0, 500), unit = "ms", keyframe)]
    pub pre_delay: f32,
    /// How long the tail takes to die away by 60 dB.
    #[setting(label = "Decay", range(0.1, 20), unit = "s", keyframe)]
    pub decay: f32,
    /// How big the room is.
    #[setting(label = "Size", range(0, 100), unit = "%", keyframe)]
    pub size: f32,
    /// How quickly the highs die away.
    #[setting(label = "Damping", range(0, 100), unit = "%", keyframe)]
    pub damping: f32,
    /// Lows below this leave the reverb (all the way down is off).
    #[setting(label = "Low cut", range(20, 2000), unit = "Hz", keyframe)]
    pub low_cut: f32,
    /// Highs above this leave the reverb (all the way up is off).
    #[setting(label = "High cut", range(1000, 20000), unit = "Hz", keyframe)]
    pub high_cut: f32,
    /// How wide the tail is.
    #[setting(label = "Width", range(0, 100), unit = "%", keyframe)]
    pub width: f32,
    /// How quickly the echoes blur together.
    #[setting(label = "Diffusion", range(0, 100), unit = "%", keyframe, when = "kind=plate")]
    pub diffusion: f32,
    /// A slow movement in the tail that keeps it from ringing.
    #[setting(label = "Modulation", range(0, 100), unit = "%", keyframe, when = "kind!=spring")]
    pub modulation: f32,
    /// The first reflections off the walls.
    #[setting(label = "Early reflections", range(0, 100), unit = "%", keyframe, when = "kind=hall")]
    pub early: f32,
    #[setting(label = "Lines", when = "kind=hall")]
    pub lines: HallLines,
    #[setting(label = "Springs", range(1, 3), when = "kind=spring")]
    pub springs: i32,
    /// How tight the springs are.
    #[setting(label = "Tension", range(0, 100), unit = "%", keyframe, when = "kind=spring")]
    pub tension: f32,
    /// The springs' drip.
    #[setting(label = "Drip", range(0, 100), unit = "%", keyframe, when = "kind=spring")]
    pub drip: f32,
    /// Holds the tail as it is, for ever.
    #[setting(label = "Freeze")]
    pub freeze: bool,
    /// Dry against wet.
    #[setting(label = "Mix", range(0, 100), unit = "%", keyframe)]
    pub mix: f32,
}

impl Default for ReverbSettings {
    fn default() -> Self {
        Self {
            kind: ReverbKind::Plate,
            pre_delay: 10.0,
            decay: 2.0,
            size: 60.0,
            damping: 40.0,
            low_cut: 100.0,
            high_cut: 12000.0,
            width: 100.0,
            diffusion: 70.0,
            modulation: 30.0,
            early: 40.0,
            lines: HallLines::Sixteen,
            springs: 2,
            tension: 50.0,
            drip: 30.0,
            freeze: false,
            mix: 25.0,
        }
    }
}

/// The limiter (its lookahead is a fixed 1.5 ms).
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct LimiterSettings {
    /// The loudest the bus may get.
    #[setting(label = "Ceiling", range(-24, 0), unit = "dB", keyframe)]
    pub ceiling: f32,
    /// How quickly it lets go after a peak.
    #[setting(label = "Release", range(1, 1000), unit = "ms", keyframe)]
    pub release: f32,
    /// Dry against wet.
    #[setting(label = "Mix", range(0, 100), unit = "%", keyframe)]
    pub mix: f32,
}

impl Default for LimiterSettings {
    fn default() -> Self {
        Self { ceiling: -1.0, release: 60.0, mix: 100.0 }
    }
}

/// Chorus, Phaser and Flanger: a sweep in Hz or in note lengths.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ModFxSettings {
    /// The sweep's speed in note lengths at the scene's tempo, or in Hz.
    #[setting(label = "Sync to tempo")]
    pub sync: bool,
    #[setting(label = "Rate", range(0.01, 10), unit = "Hz", keyframe, when = "sync=false")]
    pub rate: f32,
    #[setting(label = "Rate", when = "sync=true")]
    pub rate_note: NoteLength,
    #[setting(label = "Feel", when = "sync=true")]
    pub rate_feel: Feel,
    /// How far it sweeps.
    #[setting(label = "Depth", range(0, 100), unit = "%", keyframe)]
    pub depth: f32,
    /// How much comes round again (Phaser and Flanger).
    #[setting(label = "Feedback", range(-95, 95), unit = "%", keyframe)]
    pub feedback: f32,
    /// Chorus copies.
    #[setting(label = "Voices", range(2, 4))]
    pub voices: i32,
    /// Phaser stages (each pair makes a notch).
    #[setting(label = "Stages", range(2, 12))]
    pub stages: i32,
    /// How far left and right sweep apart.
    #[setting(label = "Width", range(0, 100), unit = "%", keyframe)]
    pub width: f32,
    /// Dry against wet.
    #[setting(label = "Mix", range(0, 100), unit = "%", keyframe)]
    pub mix: f32,
}

impl ModFxSettings {
    fn chorus() -> Self {
        Self {
            sync: false,
            rate: 0.8,
            rate_note: NoteLength::Bar,
            rate_feel: Feel::Straight,
            depth: 50.0,
            feedback: 0.0,
            voices: 3,
            stages: 6,
            width: 100.0,
            mix: 50.0,
        }
    }
    fn phaser() -> Self {
        Self { rate: 0.3, depth: 70.0, feedback: 40.0, ..Self::chorus() }
    }
    fn flanger() -> Self {
        Self { rate: 0.2, depth: 60.0, feedback: 50.0, ..Self::chorus() }
    }
}

impl Default for ModFxSettings {
    fn default() -> Self {
        Self::chorus()
    }
}

/// Stereo width.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct WidthSettings {
    /// 0 is mono, 100 as it is, 200 twice as wide.
    #[setting(label = "Width", range(0, 200), unit = "%", keyframe)]
    pub width: f32,
    /// Below this the sound is mono (all the way down is off).
    #[setting(label = "Mono below", range(20, 500), unit = "Hz", keyframe)]
    pub mono_below: f32,
    /// Dry against wet.
    #[setting(label = "Mix", range(0, 100), unit = "%", keyframe)]
    pub mix: f32,
}

impl Default for WidthSettings {
    fn default() -> Self {
        Self { width: 100.0, mono_below: 20.0, mix: 100.0 }
    }
}

/// Defs of a settings struct whose defaults are `default` rather than `T::default()` (Phaser and
/// Flanger share Chorus's struct with their own starting values). Leaked once per kind.
fn defs_with<T: Settings>(id: &'static str, default: &T) -> &'static [SettingDef] {
    use std::collections::BTreeMap;
    use std::sync::{Mutex, OnceLock};
    static MADE: OnceLock<Mutex<BTreeMap<&'static str, &'static [SettingDef]>>> = OnceLock::new();
    let made = MADE.get_or_init(Default::default);
    let mut made = made.lock().unwrap_or_else(|e| e.into_inner());
    made.entry(id).or_insert_with(|| {
        let full = full_params(default);
        let defs: Vec<SettingDef> = T::defs()
            .iter()
            .map(|d| SettingDef { default: full.get(d.key).cloned().unwrap_or_else(|| d.default.clone()), ..d.clone() })
            .collect();
        Box::leak(defs.into_boxed_slice())
    })
}

/// The starting values of a Phaser or Flanger, read on top of the saved ones.
pub fn mod_fx_settings(kind: &str, p: &Params) -> ModFxSettings {
    let mut own = match kind {
        PHASER => full_params(&ModFxSettings::phaser()),
        FLANGER => full_params(&ModFxSettings::flanger()),
        _ => full_params(&ModFxSettings::chorus()),
    };
    for (k, v) in p.iter() {
        own.set(k, v.clone());
    }
    ModFxSettings::from_params(&own)
}

/// One bus effect layer kind.
pub struct FxKind {
    id: &'static str,
    name: &'static str,
    help: &'static str,
    defs: fn() -> &'static [SettingDef],
}

impl LayerKind for FxKind {
    fn id(&self) -> &'static str {
        self.id
    }
    fn name(&self) -> &'static str {
        self.name
    }
    fn help(&self) -> &'static str {
        self.help
    }
    fn settings(&self) -> &'static [SettingDef] {
        (self.defs)()
    }
    fn section(&self) -> LayerSection {
        LayerSection::Effect
    }
    fn repeats(&self) -> bool {
        true
    }
    fn card(&self) -> &'static str {
        MIXER
    }
    fn maskable(&self) -> bool {
        false
    }
    fn per_ball(&self) -> bool {
        false
    }
}

/// Every bus effect kind.
pub fn kinds() -> [FxKind; 9] {
    [
        FxKind {
            id: EQ,
            name: "Equalizer",
            help: "Eight bands that lift or cut parts of the sound.",
            defs: EqSettings::defs,
        },
        FxKind {
            id: COMPRESSOR,
            name: "Compressor",
            help: "Turns loud parts down; can duck to another bus or to hits.",
            defs: CompressorSettings::defs,
        },
        FxKind { id: DELAY, name: "Delay", help: "Echoes, clean, analog or tape.", defs: DelaySettings::defs },
        FxKind { id: REVERB, name: "Reverb", help: "A plate, a hall or a spring.", defs: ReverbSettings::defs },
        FxKind { id: LIMITER, name: "Limiter", help: "Keeps the bus under its ceiling.", defs: LimiterSettings::defs },
        FxKind {
            id: CHORUS,
            name: "Chorus",
            help: "Slightly detuned copies spread across the stereo.",
            defs: || defs_with(CHORUS, &ModFxSettings::chorus()),
        },
        FxKind {
            id: PHASER,
            name: "Phaser",
            help: "Sweeping notches.",
            defs: || defs_with(PHASER, &ModFxSettings::phaser()),
        },
        FxKind {
            id: FLANGER,
            name: "Flanger",
            help: "A short sweeping echo.",
            defs: || defs_with(FLANGER, &ModFxSettings::flanger()),
        },
        FxKind { id: WIDTH, name: "Width", help: "Narrower or wider stereo.", defs: WidthSettings::defs },
    ]
}

/// Adds every bus effect kind to `reg`.
pub fn register(reg: &mut Registry) {
    for k in kinds() {
        reg.add_layer(k);
    }
}

/// Whether `kind` is a bus effect.
pub fn is_fx(kind: &str) -> bool {
    kind.starts_with("fx.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{SlotId, ThingId, Value};

    #[test]
    fn eq_starts_with_a_30hz_cut() {
        let eq = EqSettings::from_params(&Params::default());
        let b = eq.bands();
        assert!(b[0].on);
        assert_eq!((b[0].kind, b[0].freq, b[0].slope), (BandKind::LowCut, 30.0, Slope::Db24));
        let rest: Vec<(bool, BandKind, f32, f32)> = b[1..].iter().map(|b| (b.on, b.kind, b.freq, b.gain)).collect();
        let want: Vec<_> = BAND_FREQS[1..].iter().map(|f| (false, BandKind::Bell, *f, 0.0)).collect();
        assert_eq!(rest, want);
        let defs = EqSettings::defs();
        let def = |k: &str| defs.iter().find(|d| d.key == k).unwrap_or_else(|| panic!("{k}"));
        assert_eq!(def("band1_kind").default, Value::Choice("low_cut".into()));
        assert_eq!(def("band5_freq").default, Value::Number(1000.0));
        assert!(def("band8_gain").flags.keyframe);
        assert_eq!(EqSettings::band_key(3, "q"), "band3_q");
    }

    #[test]
    fn fx_layers_register_in_the_effect_section() {
        let reg = crate::registry();
        for id in FX_KINDS {
            let k = reg.layer(id).unwrap_or_else(|| panic!("{id} registered"));
            assert_eq!(k.section(), LayerSection::Effect, "{id}");
            assert_eq!(k.card(), MIXER, "{id}");
            assert!(k.repeats());
            assert!(is_fx(id));
            // Every effect has a Mix and every number takes keys.
            let mix = k.settings().iter().find(|d| d.key == "mix").unwrap_or_else(|| panic!("{id} mix"));
            assert!(mix.flags.keyframe);
            for d in k.settings().iter().filter(|d| matches!(d.default, Value::Number(_))) {
                assert!(d.flags.keyframe, "{id} {}", d.key);
            }
        }
        // Phaser and Flanger start from their own values.
        let phaser = reg.layer(PHASER).unwrap();
        let fb = phaser.settings().iter().find(|d| d.key == "feedback").unwrap();
        assert_eq!(fb.default, Value::Number(40.0));
        assert_eq!(mod_fx_settings(FLANGER, &Params::default()).feedback, 50.0);
        assert_eq!(mod_fx_settings(CHORUS, &Params::default()).feedback, 0.0);
        // A bus effect goes into the mixer card, in its bus's lane.
        let mut scene = bpe_core::Scene::empty(&reg);
        scene.add_slot(&reg, ThingId::ROOT, SlotId::Mixer, MIXER).unwrap();
        let a = scene.add_layer_in(&reg, ThingId::ROOT, SlotId::Mixer, REVERB, Some(1)).unwrap();
        let b = scene.add_layer_in(&reg, ThingId::ROOT, SlotId::Mixer, EQ, Some(0)).unwrap();
        let card = scene.card(ThingId::ROOT, SlotId::Mixer).unwrap();
        let lanes: Vec<_> = card.layers.iter().map(|l| (l.id, l.lane)).collect();
        assert_eq!(lanes, vec![(b, Some(0)), (a, Some(1))], "bus 1's effects first");
    }
}
