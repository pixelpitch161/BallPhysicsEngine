//! The sound settings' schemas: card and layer kinds, registered with the kit. No sound is
//! made here; `bpe-sound` reads these settings.

pub mod adsr;
pub mod fx;
pub mod mixer;
pub mod voice;

pub use adsr::{ADSR, AdsrKind, AdsrSettings};
pub use mixer::{BUSES, BusSettings, MIXER, MixerKind, MixerSettings};
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
    fx::register(reg);
}
