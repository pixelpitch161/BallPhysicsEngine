//! The sound settings' schemas: card and layer kinds, registered with the kit. No sound is
//! made here; `bpe-sound` reads these settings.

pub mod adsr;
pub mod chain;
pub mod fx;
pub mod lfo_curve;
pub mod mixer;
pub mod mods;
pub mod notes;
pub mod voice;

pub use adsr::{ADSR, AdsrKind, AdsrSettings};
pub use chain::{
    CHAIN_KINDS, ChainEqSettings, DISTORTION, DistKind, DistortionSettings, EQ, FILTER, FilterKind, FilterSettings,
    MAX_CHAIN, MODULATION, ModKind, ModulationSettings, Quality, Slope, Source, layer_summary,
};
pub use mixer::{BUSES, BusSettings, MIXER, MixerKind, MixerSettings};
pub use mods::{
    DISTANCE, DistanceSettings, Follows, HIT, HitPick, HitSettings, LFO, LIGHT_MOD, LfoPreset, LfoSettings, LfoShape,
    LfoStart, LightFollows, LightModSettings, LightReads, MAX_OF_EACH, MOD_KINDS, Measure, NoteFeel, NoteLength,
    add_modulator, count_of, curve_numbers, kind_name, numbered_name, turn_on_distance,
};
pub use notes::{BeatsOrBars, Change, MAX_SLOTS, NOTE_NAMES, Plays, Root, Scale, SlotOrder, StrumDir};
pub use voice::{
    GateMode, GateUnit, MomentStrength, NoiseColour, OscSettings, PhaseMode, PlayMode, PlayWhen, SILENT_DB, SampleLoop,
    SubOctave, SubWave, VOICE, VoiceKind, VoiceSettings, WarpMode, Wave, hears_own_balls, starter_voice,
};

use bpe_core::Registry;

/// Adds every sound card and layer kind to `reg`.
pub fn register(reg: &mut Registry) {
    reg.add_module(MixerKind);
    reg.add_module(VoiceKind);
    reg.add_layer(AdsrKind);
    fx::register(reg);
    reg.add_layer(mods::DistanceKind);
    reg.add_layer(mods::LightModKind);
    reg.add_layer(mods::LfoKind);
    reg.add_layer(mods::HitKind);
    reg.add_layer(chain::ModulationKind);
    reg.add_layer(chain::FilterLayerKind);
    reg.add_layer(chain::DistortionKind);
    reg.add_layer(chain::EqLayerKind);
}
