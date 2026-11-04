//! The On-Hit envelope modulator: a layer of a shape's sound card.

use super::voice::{GateMode, GateUnit, VOICE};
use bpe_core::{LayerKind, LayerSection, SettingDef, Settings};

/// The layer kind's id.
pub const ADSR: &str = "mod.adsr";

/// One envelope: it rises and falls with each note.
#[derive(Clone, Debug, PartialEq, Settings)]
pub struct AdsrSettings {
    /// A wait before the envelope starts.
    #[setting(label = "Delay", range(0, 10000), unit = "ms", keyframe)]
    pub delay: f32,
    /// How long it takes to rise.
    #[setting(label = "Attack", range(0, 10000), unit = "ms", keyframe)]
    pub attack: f32,
    /// How long it stays at the top.
    #[setting(label = "Hold", range(0, 10000), unit = "ms", keyframe)]
    pub hold: f32,
    /// How long it takes to fall to Sustain.
    #[setting(label = "Decay", range(0, 10000), unit = "ms", keyframe)]
    pub decay: f32,
    /// Where it stays while the note is held.
    #[setting(label = "Sustain", range(0, 100), unit = "%", keyframe)]
    pub sustain: f32,
    /// How long it takes to die away once the note is let go.
    #[setting(label = "Release", range(0, 10000), unit = "ms", keyframe)]
    pub release: f32,
    /// How much a soft hit makes it smaller.
    #[setting(label = "Velocity", range(0, 100), unit = "%", keyframe)]
    pub velocity: f32,
    /// Bends the rise (0 is straight).
    #[setting(label = "Attack curve", range(-100, 100), keyframe)]
    pub attack_curve: f32,
    /// Bends the fall to Sustain.
    #[setting(label = "Decay curve", range(-100, 100), keyframe)]
    pub decay_curve: f32,
    /// Bends the dying away.
    #[setting(label = "Release curve", range(-100, 100), keyframe)]
    pub release_curve: f32,
    /// How long a note is held: a set length, or until the next hit on this shape.
    #[setting(label = "Gate")]
    pub gate_mode: GateMode,
    /// A set length in milliseconds or in beats.
    #[setting(label = "Gate unit", when = "gate_mode=fixed")]
    pub gate_unit: GateUnit,
    #[setting(
        label = "Gate length",
        range(1, 10000),
        unit = "ms",
        keyframe,
        when = "gate_mode=fixed and gate_unit=ms"
    )]
    pub gate_ms: f32,
    #[setting(
        label = "Gate beats",
        range(0.0625, 16),
        unit = "beats",
        keyframe,
        when = "gate_mode=fixed and gate_unit=beats"
    )]
    pub gate_beats: f32,
}

impl Default for AdsrSettings {
    fn default() -> Self {
        Self {
            delay: 0.0,
            attack: 2.0,
            hold: 0.0,
            decay: 350.0,
            sustain: 0.0,
            release: 200.0,
            velocity: 50.0,
            attack_curve: -40.0,
            decay_curve: 40.0,
            release_curve: 40.0,
            gate_mode: GateMode::Fixed,
            gate_unit: GateUnit::Ms,
            gate_ms: 100.0,
            gate_beats: 0.25,
        }
    }
}

/// The layer kind.
pub struct AdsrKind;

impl LayerKind for AdsrKind {
    fn id(&self) -> &'static str {
        ADSR
    }
    fn name(&self) -> &'static str {
        "ADSR"
    }
    fn help(&self) -> &'static str {
        "An envelope that rises and falls with every note a hit plays."
    }
    fn settings(&self) -> &'static [SettingDef] {
        AdsrSettings::defs()
    }
    fn section(&self) -> LayerSection {
        LayerSection::Modulator
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
