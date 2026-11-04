//! The scene's mixer card: the master (and, later, the buses).

use bpe_core::{ModuleKind, SettingDef, Settings, SlotId};

/// The mixer card's kind, on the scene settings only.
pub struct MixerKind;

#[derive(Clone, Debug, PartialEq, Settings)]
pub struct MixerSettings {
    /// Turns the whole mix up or down before the limiter.
    #[setting(label = "Input", range(-24, 24), unit = "dB", keyframe, group = "Master")]
    pub master_input: f32,
    /// Turns the finished sound up or down after the limiter.
    #[setting(label = "Output", range(-24, 24), unit = "dB", keyframe, group = "Master")]
    pub master_output: f32,
    /// The loudest the sound may get, peaks between samples included.
    #[setting(label = "Ceiling", range(-12, 0), unit = "dBTP", group = "Master")]
    pub master_ceiling: f32,
    /// How quickly the limiter lets go after a peak.
    #[setting(label = "Release", range(1, 1000), unit = "ms", group = "Master")]
    pub master_release: f32,
}

impl Default for MixerSettings {
    fn default() -> Self {
        Self { master_input: 0.0, master_output: 0.0, master_ceiling: -1.0, master_release: 60.0 }
    }
}

impl ModuleKind for MixerKind {
    fn id(&self) -> &'static str {
        "sound.mixer"
    }
    fn name(&self) -> &'static str {
        "Mixer"
    }
    fn help(&self) -> &'static str {
        "The scene's sound: the master's limiter and levels."
    }
    fn slot(&self) -> SlotId {
        SlotId::Mixer
    }
    fn settings(&self) -> &'static [SettingDef] {
        MixerSettings::defs()
    }
    fn fits(&self, thing_kind: &str) -> bool {
        thing_kind == "scene"
    }
}
