//! The sound settings' schemas: card and layer kinds, registered with the kit. No sound is
//! made here; `bpe-sound` reads these settings.

pub mod adsr;
pub mod chain;
pub mod mixer;
pub mod notes;
pub mod voice;

pub use adsr::{ADSR, AdsrKind, AdsrSettings};
pub use chain::{
    CHAIN_KINDS, DISTORTION, DistKind, DistortionSettings, FILTER, FilterKind, FilterSettings, MAX_CHAIN, MODULATION,
    ModKind, ModulationSettings, Quality, Slope, Source, layer_summary,
};
pub use mixer::{MixerKind, MixerSettings};
pub use notes::{BeatsOrBars, Change, MAX_SLOTS, NOTE_NAMES, Plays, Root, Scale, SlotOrder, StrumDir};
pub use voice::{
    GateMode, GateUnit, NoiseColour, OscSettings, PhaseMode, PlayMode, SILENT_DB, SubOctave, SubWave, VOICE, VoiceKind,
    VoiceSettings, WarpMode, Wave, starter_voice,
};

use bpe_core::Registry;

/// Adds every sound card and layer kind to `reg`.
pub fn register(reg: &mut Registry) {
    reg.add_module(MixerKind);
    reg.add_module(VoiceKind);
    reg.add_layer(AdsrKind);
    reg.add_layer(chain::ModulationKind);
    reg.add_layer(chain::FilterLayerKind);
    reg.add_layer(chain::DistortionKind);
}
