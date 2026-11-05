//! An oscillator's layer chain: Modulation, Filter, Distortion and EQ layers of a shape's sound card,
//! in the oscillator's lane (`Layer.lane` 0 = Osc A, 1 = Osc B), run top to bottom between the
//! oscillator's Gain and its Level. No sound is made here; `bpe-sound` reads these settings.

use super::fx::{EqBand, EqSettings};
use super::voice::VOICE;
use bpe_core::{Choice, Layer, LayerId, LayerKind, LayerSection, Params, SettingDef, Settings};

/// The Modulation layer kind's id.
pub const MODULATION: &str = "chain.modulation";
/// The Filter layer kind's id.
pub const FILTER: &str = "chain.filter";
/// The Distortion layer kind's id.
pub const DISTORTION: &str = "chain.distortion";
/// The EQ layer kind's id: the Mixer's 8-band EQ on one oscillator.
pub const EQ: &str = "chain.eq";
/// Every chain layer kind, in the "+ Add layer" menu's order.
pub const CHAIN_KINDS: [&str; 4] = [MODULATION, FILTER, DISTORTION, EQ];
/// The most chain layers a sound runs, both oscillators together (more are let through
/// untouched).
pub const MAX_CHAIN: usize = 32;

/// How a Modulation layer moves the sound reaching it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum ModKind {
    /// Phase modulation of the sound reaching the layer (a short moving delay).
    #[default]
    Fm,
    /// The source squeezes the wave one way only, as a Casio CZ does.
    PhaseDistortion,
    /// The source moves the level.
    Am,
    /// The sound times the source.
    Ring,
}

impl ModKind {
    pub const ALL: [ModKind; 4] = [ModKind::Fm, ModKind::PhaseDistortion, ModKind::Am, ModKind::Ring];

    pub fn label(self) -> &'static str {
        match self {
            ModKind::Fm => "FM",
            ModKind::PhaseDistortion => "Phase distortion",
            ModKind::Am => "AM",
            ModKind::Ring => "Ring",
        }
    }
}

/// A filter's type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum FilterKind {
    #[default]
    LowPass,
    HighPass,
    BandPass,
    Notch,
    Peak,
    LowShelf,
    HighShelf,
    Ladder,
    Comb,
    Formant,
}

impl FilterKind {
    pub const ALL: [FilterKind; 10] = [
        FilterKind::LowPass,
        FilterKind::HighPass,
        FilterKind::BandPass,
        FilterKind::Notch,
        FilterKind::Peak,
        FilterKind::LowShelf,
        FilterKind::HighShelf,
        FilterKind::Ladder,
        FilterKind::Comb,
        FilterKind::Formant,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FilterKind::LowPass => "Low pass",
            FilterKind::HighPass => "High pass",
            FilterKind::BandPass => "Band pass",
            FilterKind::Notch => "Notch",
            FilterKind::Peak => "Peak",
            FilterKind::LowShelf => "Low shelf",
            FilterKind::HighShelf => "High shelf",
            FilterKind::Ladder => "Ladder",
            FilterKind::Comb => "Comb",
            FilterKind::Formant => "Formant",
        }
    }

    /// Whether it comes in 12 and 24 dB slopes.
    pub fn has_slope(self) -> bool {
        !matches!(self, FilterKind::Ladder | FilterKind::Comb | FilterKind::Formant)
    }

    /// Whether it has a Gain knob (Peak and the shelves).
    pub fn has_gain(self) -> bool {
        matches!(self, FilterKind::Peak | FilterKind::LowShelf | FilterKind::HighShelf)
    }
}

/// How steep a filter is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Slope {
    #[default]
    Db12,
    Db24,
}

/// A distortion's type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum DistKind {
    #[default]
    SoftClip,
    HardClip,
    Tube,
    Tape,
    Diode,
    Overdrive,
    Wavefold,
    Bitcrush,
    Downsample,
    Rectify,
}

impl DistKind {
    pub const ALL: [DistKind; 10] = [
        DistKind::SoftClip,
        DistKind::HardClip,
        DistKind::Tube,
        DistKind::Tape,
        DistKind::Diode,
        DistKind::Overdrive,
        DistKind::Wavefold,
        DistKind::Bitcrush,
        DistKind::Downsample,
        DistKind::Rectify,
    ];

    pub fn label(self) -> &'static str {
        match self {
            DistKind::SoftClip => "Soft clip",
            DistKind::HardClip => "Hard clip",
            DistKind::Tube => "Tube",
            DistKind::Tape => "Tape",
            DistKind::Diode => "Diode",
            DistKind::Overdrive => "Overdrive",
            DistKind::Wavefold => "Wavefold",
            DistKind::Bitcrush => "Bitcrush",
            DistKind::Downsample => "Downsample",
            DistKind::Rectify => "Rectify",
        }
    }

    /// Whether it runs at a higher rate (Bitcrush and Downsample are meant to alias).
    pub fn oversampled(self) -> bool {
        !matches!(self, DistKind::Bitcrush | DistKind::Downsample)
    }
}

/// How many times the rate a distortion runs at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Choice)]
pub enum Quality {
    X1,
    X2,
    #[default]
    X4,
    X8,
}

impl Quality {
    pub const ALL: [Quality; 4] = [Quality::X1, Quality::X2, Quality::X4, Quality::X8];

    pub fn factor(self) -> u8 {
        match self {
            Quality::X1 => 1,
            Quality::X2 => 2,
            Quality::X4 => 4,
            Quality::X8 => 8,
        }
    }
}

/// Where a Modulation layer's source is: an oscillator as it reaches its first layer, or the
/// sound just after a layer of either chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The other oscillator (the setting left empty: a new layer's source).
    OtherRaw,
    /// Oscillator 0 (A) or 1 (B).
    Raw(u8),
    /// After that layer.
    After(LayerId),
}

impl Source {
    /// Reads the `source` setting: `""`, `"a"`, `"b"` or a layer's id.
    pub fn parse(s: &str) -> Source {
        match s.trim() {
            "" => Source::OtherRaw,
            "a" => Source::Raw(0),
            "b" => Source::Raw(1),
            id => id.parse().map(|n| Source::After(LayerId(n))).unwrap_or(Source::OtherRaw),
        }
    }

    /// The `source` setting's text for this source.
    pub fn text(self) -> String {
        match self {
            Source::OtherRaw => String::new(),
            Source::Raw(0) => "a".into(),
            Source::Raw(_) => "b".into(),
            Source::After(id) => id.0.to_string(),
        }
    }

    /// The source for a layer in `lane`: the other oscillator stands for itself.
    pub fn in_lane(self, lane: u8) -> Source {
        match self {
            Source::OtherRaw => Source::Raw(1 - lane.min(1)),
            s => s,
        }
    }
}

/// Osc A or Osc B.
pub fn osc_name(lane: u8) -> &'static str {
    if lane == 0 { "Osc A" } else { "Osc B" }
}

/// FM, Phase distortion, AM or Ring of the sound reaching the layer.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ModulationSettings {
    #[setting(label = "Type")]
    pub kind: ModKind,
    /// What moves it: an oscillator, or the sound after any layer of either chain.
    #[setting(label = "Source")]
    pub source: String,
    /// How far it moves the sound.
    #[setting(label = "Amount", range(0, 100), unit = "%", keyframe)]
    pub amount: f32,
    /// The level coming out of the layer.
    #[setting(label = "Output", range(-24, 24), unit = "dB", keyframe)]
    pub output: f32,
    /// How much of the layer is heard against the sound reaching it.
    #[setting(label = "Mix", range(0, 100), unit = "%", keyframe)]
    pub mix: f32,
    /// Plays the chain cut at this layer.
    #[setting(label = "Solo")]
    pub solo: bool,
}

impl Default for ModulationSettings {
    fn default() -> Self {
        Self { kind: ModKind::Fm, source: String::new(), amount: 25.0, output: 0.0, mix: 100.0, solo: false }
    }
}

/// A filter of ten types.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct FilterSettings {
    #[setting(label = "Type")]
    pub kind: FilterKind,
    /// 12 or 24 dB an octave.
    #[setting(label = "Slope", when = "kind!=ladder|comb|formant")]
    pub slope: Slope,
    /// Where it cuts (for a Comb, the pitch it rings at).
    #[setting(label = "Cutoff", range(20, 20000), unit = "Hz", keyframe, when = "kind!=formant")]
    pub cutoff: f32,
    /// How much it rings at the cutoff.
    #[setting(label = "Reso", range(0, 100), unit = "%", keyframe)]
    pub reso: f32,
    /// Drives the sound into the filter.
    #[setting(label = "Drive", range(0, 100), unit = "%", keyframe)]
    pub drive: f32,
    /// How far the cutoff follows the note (from C5); a Comb at 100 % plays the note.
    #[setting(label = "Key track", range(0, 100), unit = "%", keyframe, when = "kind!=formant")]
    pub key_track: f32,
    /// How much it lifts or cuts.
    #[setting(label = "Gain", range(-24, 24), unit = "dB", keyframe, when = "kind=peak|low_shelf|high_shelf")]
    pub gain: f32,
    /// The vowel, A E I O U, morphing between them.
    #[setting(label = "Vowel", range(0, 4), keyframe, when = "kind=formant")]
    pub vowel: f32,
    /// Moves the vowel's formants up or down.
    #[setting(label = "Shift", range(-12, 12), unit = "st", keyframe, when = "kind=formant")]
    pub shift: f32,
    #[setting(label = "Mix", range(0, 100), unit = "%", keyframe)]
    pub mix: f32,
    #[setting(label = "Solo")]
    pub solo: bool,
}

impl Default for FilterSettings {
    fn default() -> Self {
        Self {
            kind: FilterKind::LowPass,
            slope: Slope::Db12,
            cutoff: 2000.0,
            reso: 0.0,
            drive: 0.0,
            key_track: 0.0,
            gain: 0.0,
            vowel: 0.0,
            shift: 0.0,
            mix: 100.0,
            solo: false,
        }
    }
}

/// A distortion of ten types.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct DistortionSettings {
    #[setting(label = "Type")]
    pub kind: DistKind,
    /// How hard the sound is pushed into the curve.
    #[setting(label = "Drive", range(0, 48), unit = "dB", keyframe, when = "kind!=bitcrush|downsample")]
    pub drive: f32,
    /// How many steps the level is cut into.
    #[setting(label = "Bits", range(1, 16), keyframe, when = "kind=bitcrush")]
    pub bits: f32,
    /// The rate the sound is held at.
    #[setting(label = "Rate", range(100, 48000), unit = "Hz", keyframe, when = "kind=downsample")]
    pub rate: f32,
    /// Leans the curve one way (more even harmonics).
    #[setting(label = "Bias", range(-100, 100), unit = "%", keyframe)]
    pub bias: f32,
    /// Darker or brighter after the curve.
    #[setting(label = "Tone", range(-100, 100), keyframe)]
    pub tone: f32,
    #[setting(label = "Output", range(-24, 24), unit = "dB", keyframe)]
    pub output: f32,
    /// How many times the rate the curve runs at: higher folds back less.
    #[setting(label = "Quality", when = "kind!=bitcrush|downsample")]
    pub quality: Quality,
    #[setting(label = "Mix", range(0, 100), unit = "%", keyframe)]
    pub mix: f32,
    #[setting(label = "Solo")]
    pub solo: bool,
}

impl Default for DistortionSettings {
    fn default() -> Self {
        Self {
            kind: DistKind::SoftClip,
            drive: 12.0,
            bits: 8.0,
            rate: 8000.0,
            bias: 0.0,
            tone: 0.0,
            output: 0.0,
            quality: Quality::X4,
            mix: 100.0,
            solo: false,
        }
    }
}

/// The Mixer's 8-band EQ on one oscillator: the same bands (`band1_`..`band8_`, as
/// [`EqSettings`]), with Output, Mix and Solo.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct ChainEqSettings {
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
    #[setting(label = "Output", range(-24, 24), unit = "dB", keyframe)]
    pub output: f32,
    #[setting(label = "Mix", range(0, 100), unit = "%", keyframe)]
    pub mix: f32,
    #[setting(label = "Solo")]
    pub solo: bool,
}

impl Default for ChainEqSettings {
    fn default() -> Self {
        // The bus EQ's bands: a Low cut at 30 Hz that is on, the rest off bells.
        let EqSettings { band1, band2, band3, band4, band5, band6, band7, band8, .. } = EqSettings::default();
        Self { band1, band2, band3, band4, band5, band6, band7, band8, output: 0.0, mix: 100.0, solo: false }
    }
}

impl ChainEqSettings {
    /// The eight bands in order.
    pub fn bands(&self) -> [&EqBand; 8] {
        [&self.band1, &self.band2, &self.band3, &self.band4, &self.band5, &self.band6, &self.band7, &self.band8]
    }

    /// How many bands are on.
    pub fn bands_on(&self) -> usize {
        self.bands().iter().filter(|b| b.on).count()
    }
}

macro_rules! chain_kind {
    ($kind:ident, $id:expr, $name:expr, $help:expr, $settings:ty) => {
        /// The layer kind.
        pub struct $kind;

        impl LayerKind for $kind {
            fn id(&self) -> &'static str {
                $id
            }
            fn name(&self) -> &'static str {
                $name
            }
            fn help(&self) -> &'static str {
                $help
            }
            fn settings(&self) -> &'static [SettingDef] {
                <$settings>::defs()
            }
            fn section(&self) -> LayerSection {
                LayerSection::Chain
            }
            fn repeats(&self) -> bool {
                true
            }
            fn card(&self) -> &'static str {
                VOICE
            }
            fn takes_subs(&self) -> bool {
                false
            }
            fn per_ball(&self) -> bool {
                false
            }
        }
    };
}

chain_kind!(
    ModulationKind,
    MODULATION,
    "Modulation",
    "FM, phase distortion, AM or ring modulation of the sound, from either oscillator or any layer.",
    ModulationSettings
);
chain_kind!(
    FilterLayerKind,
    FILTER,
    "Filter",
    "A filter: low, high, band, notch, peak, shelves, ladder, comb or formant.",
    FilterSettings
);
chain_kind!(
    DistortionKind,
    DISTORTION,
    "Distortion",
    "Bends the wave: clipping, tube, tape, folding, crushing and more.",
    DistortionSettings
);
chain_kind!(EqLayerKind, EQ, "EQ", "An 8-band equaliser: cuts, shelves, bells and notches.", ChainEqSettings);

/// A frequency read aloud: "440 Hz", "1.8 kHz", "12 kHz".
pub fn hz_text(hz: f64) -> String {
    if hz < 1000.0 {
        format!("{hz:.0} Hz")
    } else if hz < 10_000.0 {
        format!("{:.1} kHz", hz / 1000.0)
    } else {
        format!("{:.0} kHz", hz / 1000.0)
    }
}

/// The vowel nearest `v` (0..4).
pub fn vowel_name(v: f64) -> &'static str {
    ["A", "E", "I", "O", "U"][(v.round().clamp(0.0, 4.0)) as usize]
}

/// Points every Modulation layer of `card` whose source is "after" a layer at `map` of that
/// layer; where `map` gives nothing (the layer is gone), at the raw oscillator of the lane the
/// Modulation sits in, as the sound falls back.
pub fn retarget_sources(card: &mut bpe_core::Slot, map: &dyn Fn(LayerId) -> Option<LayerId>) {
    for layer in card.layers.iter().filter(|l| l.kind == MODULATION) {
        let key = bpe_core::layered_key(layer.id, "source");
        let Some(bpe_core::Value::Text(text)) = card.params.get(&key) else { continue };
        if let Source::After(id) = Source::parse(text) {
            let to = map(id).map_or(Source::Raw(layer.lane.unwrap_or(0).min(1)), Source::After);
            card.params.set(&key, bpe_core::Value::Text(to.text()));
        }
    }
}

/// Where a source is, in words: "Osc B", "Osc B · after Filter".
pub fn source_text(source: Source, lane: u8, layers: &[Layer]) -> String {
    match source.in_lane(lane) {
        Source::Raw(o) => osc_name(o).to_owned(),
        Source::After(id) => match layers.iter().find(|l| l.id == id) {
            Some(l) => format!("{} · after {}", osc_name(l.lane.unwrap_or(0)), l.name),
            // Gone: the layer's own oscillator, as the engine falls back.
            None => osc_name(lane).to_owned(),
        },
        Source::OtherRaw => unreachable!("in_lane names the oscillator"),
    }
}

/// A strip's one-line summary: "Low pass 24 · 1.8 kHz", "Tube · +12 dB · 4x", "FM from Osc B ·
/// after Filter". `params` are the layer's own values by plain key; `layers` the card's layers
/// (to name a source).
pub fn layer_summary(layer: &Layer, params: &Params, layers: &[Layer]) -> String {
    let lane = layer.lane.unwrap_or(0);
    match layer.kind.as_str() {
        MODULATION => {
            let m = ModulationSettings::from_params(params);
            format!("{} from {}", m.kind.label(), source_text(Source::parse(&m.source), lane, layers))
        }
        FILTER => {
            let f = FilterSettings::from_params(params);
            let slope = if f.slope == Slope::Db24 { "24" } else { "12" };
            let cutoff = hz_text(f.cutoff as f64);
            match f.kind {
                FilterKind::Ladder => format!("Ladder · {cutoff}"),
                FilterKind::Comb if f.key_track >= 99.5 => "Comb · plays the note".into(),
                FilterKind::Comb => format!("Comb · {cutoff}"),
                FilterKind::Formant => format!("Formant · {}", vowel_name(f.vowel as f64)),
                k if k.has_gain() => format!("{} {slope} · {cutoff} · {:+.0} dB", k.label(), f.gain),
                k => format!("{} {slope} · {cutoff}", k.label()),
            }
        }
        DISTORTION => {
            let d = DistortionSettings::from_params(params);
            match d.kind {
                DistKind::Bitcrush => format!("Bitcrush · {:.0} bits", d.bits),
                DistKind::Downsample => format!("Downsample · {}", hz_text(d.rate as f64)),
                k => format!("{} · {:+.0} dB · {}x", k.label(), d.drive, d.quality.factor()),
            }
        }
        EQ => match ChainEqSettings::from_params(params).bands_on() {
            0 => "No bands".into(),
            1 => "1 band".into(),
            n => format!("{n} bands"),
        },
        _ => layer.name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpe_core::{Scene, SlotId, ThingId, Value};

    fn sounding() -> (bpe_core::Registry, Scene, ThingId) {
        let reg = crate::registry();
        let mut scene = Scene::empty(&reg);
        let id = scene.add_thing(&reg, "shape", ThingId::ROOT).unwrap();
        let card = super::super::starter_voice(&reg);
        scene.things.get_mut(&id).unwrap().slots.insert(SlotId::Sound, card);
        (reg, scene, id)
    }

    #[test]
    fn chain_layers_register_in_the_chain_section() {
        let (reg, mut scene, id) = sounding();
        for kind in CHAIN_KINDS {
            let k = reg.layer(kind).unwrap_or_else(|| panic!("{kind} registered"));
            assert_eq!(k.section(), LayerSection::Chain);
            assert_eq!(k.card(), VOICE);
            assert!(k.repeats());
        }
        let f = scene.add_layer_in(&reg, id, SlotId::Sound, FILTER, Some(1)).unwrap();
        let d = scene.add_layer_in(&reg, id, SlotId::Sound, DISTORTION, Some(0)).unwrap();
        let m = scene.add_layer_in(&reg, id, SlotId::Sound, MODULATION, Some(0)).unwrap();
        let card = scene.card(id, SlotId::Sound).unwrap();
        let order: Vec<(LayerId, Option<u8>)> = card.layers.iter().map(|l| (l.id, l.lane)).collect();
        // Osc A's chain, then Osc B's, then the modulators.
        assert_eq!(&order[..3], &[(d, Some(0)), (m, Some(0)), (f, Some(1))]);
        assert!(card.layers[3..].iter().all(|l| super::super::MOD_KINDS.contains(&l.kind.as_str())));
        assert_eq!(card.layer(f).unwrap().name, "Filter");
    }

    #[test]
    fn default_modulation_source_is_the_other_osc() {
        let m = ModulationSettings::default();
        assert_eq!(Source::parse(&m.source), Source::OtherRaw);
        assert_eq!(Source::OtherRaw.in_lane(0), Source::Raw(1));
        assert_eq!(Source::OtherRaw.in_lane(1), Source::Raw(0));
        for s in [Source::OtherRaw, Source::Raw(0), Source::Raw(1), Source::After(LayerId(7))] {
            assert_eq!(Source::parse(&s.text()), s);
        }
        let layer = Layer {
            id: LayerId(4),
            kind: MODULATION.into(),
            name: "Modulation".into(),
            lane: Some(0),
            ..Layer::default()
        };
        assert_eq!(layer_summary(&layer, &Params::default(), std::slice::from_ref(&layer)), "FM from Osc B");
        let b = Layer { lane: Some(1), ..layer.clone() };
        assert_eq!(layer_summary(&b, &Params::default(), std::slice::from_ref(&b)), "FM from Osc A");
    }

    #[test]
    fn summaries_read_like_the_mockup() {
        let filter =
            Layer { id: LayerId(2), kind: FILTER.into(), name: "Filter".into(), lane: Some(1), ..Layer::default() };
        let p = |pairs: &[(&str, Value)]| {
            let mut p = Params::default();
            for (k, v) in pairs {
                p.set(k, v.clone());
            }
            p
        };
        let f = p(&[("slope", Value::Choice("db24".into())), ("cutoff", Value::Number(1800.0))]);
        assert_eq!(layer_summary(&filter, &f, &[]), "Low pass 24 · 1.8 kHz");
        let dist = Layer {
            id: LayerId(3),
            kind: DISTORTION.into(),
            name: "Distortion".into(),
            lane: Some(0),
            ..Layer::default()
        };
        let d = p(&[("kind", Value::Choice("tube".into())), ("drive", Value::Number(12.0))]);
        assert_eq!(layer_summary(&dist, &d, &[]), "Tube · +12 dB · 4x");
        let m = Layer {
            id: LayerId(4),
            kind: MODULATION.into(),
            name: "Modulation".into(),
            lane: Some(0),
            ..Layer::default()
        };
        let mp = p(&[("source", Value::Text("2".into()))]);
        assert_eq!(layer_summary(&m, &mp, &[filter.clone(), m.clone()]), "FM from Osc B · after Filter");
        let crush = p(&[("kind", Value::Choice("bitcrush".into())), ("bits", Value::Number(6.0))]);
        assert_eq!(layer_summary(&dist, &crush, &[]), "Bitcrush · 6 bits");
        let peak = p(&[
            ("kind", Value::Choice("peak".into())),
            ("gain", Value::Number(-6.0)),
            ("cutoff", Value::Number(440.0)),
        ]);
        assert_eq!(layer_summary(&filter, &peak, &[]), "Peak 12 · 440 Hz · -6 dB");
        let formant = p(&[("kind", Value::Choice("formant".into())), ("vowel", Value::Number(2.2))]);
        assert_eq!(layer_summary(&filter, &formant, &[]), "Formant · I");
        // A source that went away reads as the layer's own oscillator.
        let gone = p(&[("source", Value::Text("99".into()))]);
        assert_eq!(layer_summary(&m, &gone, std::slice::from_ref(&m)), "FM from Osc A");
    }

    #[test]
    fn chain_eq_has_the_bus_eqs_bands_and_round_trips() {
        let eq = ChainEqSettings::default();
        let bus = EqSettings::default();
        assert_eq!(eq.bands(), bus.bands(), "the same bands as the Mixer's EQ");
        assert_eq!((eq.output, eq.mix, eq.solo), (0.0, 100.0, false));
        assert_eq!(ChainEqSettings::from_params(&Params::default()), eq);
        let defs = ChainEqSettings::defs();
        let def = |k: &str| defs.iter().find(|d| d.key == k).unwrap_or_else(|| panic!("{k}"));
        assert_eq!(def("band1_kind").default, Value::Choice("low_cut".into()));
        assert_eq!(def("band6_freq").default, Value::Number(2500.0));
        for k in ["band3_freq", "band3_gain", "band3_q", "output", "mix"] {
            assert!(def(k).flags.keyframe, "{k} takes keys");
        }
        assert!(defs.iter().all(|d| d.key != "analyser"), "no analyser on a note");
        // Changed values come back as they went in.
        let mut changed = eq.clone();
        changed.band4.on = true;
        changed.band4.kind = super::super::fx::BandKind::HighShelf;
        changed.band4.gain = -7.5;
        changed.band8.q = 3.0;
        changed.output = 2.0;
        changed.solo = true;
        let p = changed.to_params();
        assert_eq!(p.get("band4_gain"), Some(&Value::Number(-7.5)));
        assert_eq!(ChainEqSettings::from_params(&p), changed);
        // Its summary counts the bands that are on.
        let layer = Layer { id: LayerId(3), kind: EQ.into(), name: "EQ".into(), lane: Some(0), ..Layer::default() };
        assert_eq!(layer_summary(&layer, &Params::default(), &[]), "1 band");
        assert_eq!(layer_summary(&layer, &p, &[]), "2 bands");
        let mut none = Params::default();
        none.set("band1_on", Value::Bool(false));
        assert_eq!(layer_summary(&layer, &none, &[]), "No bands");
    }

    #[test]
    fn whens_name_real_choices() {
        let defs = FilterSettings::defs();
        let slope = defs.iter().find(|d| d.key == "slope").unwrap();
        let get = |kind: &'static str| move |k: &str| (k == "kind").then(|| Value::Choice(kind.into()));
        assert!(slope.applies(None, &get("low_pass")));
        assert!(!slope.applies(None, &get("ladder")));
        let gain = defs.iter().find(|d| d.key == "gain").unwrap();
        assert!(gain.applies(None, &get("high_shelf")));
        assert!(!gain.applies(None, &get("notch")));
        let q = DistortionSettings::defs().iter().find(|d| d.key == "quality").unwrap();
        assert!(!q.applies(None, &get("bitcrush")));
        assert!(q.applies(None, &get("tube")));
        assert_eq!(Quality::default().factor(), 4);
        assert_eq!(<Slope as Choice>::name(Slope::Db24), "db24");
    }
}
