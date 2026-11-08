//! Ball Studio's sound engine. An [`Engine`] reads a scene, runs its own copy of the physics in
//! step with the audio, places every hit on its exact sample, runs the voices and the master,
//! and renders blocks of [`BLOCK`] stereo samples at [`SAMPLE_RATE`]. The editor and the export
//! call the same [`Engine::render`]; nothing else makes sound.

pub mod buses;
pub mod chain;
pub mod chain_graph;
pub mod control;
pub mod engine;
pub mod fx_chain;
pub mod hits;
pub mod lag;
pub mod master;
pub mod meters;
pub mod midi;
pub mod modmatrix;
pub mod mods;
pub mod music;
pub mod offline;
pub mod patch;
pub mod ruled;
pub mod tempo;
pub mod voice;
pub mod voices;
mod wav;

pub use bpe_dsp::{BLOCK, SAMPLE_RATE};
pub use buses::{BusesSpec, ShapeRoute, sends, would_loop};
pub use control::{ControlSampler, FollowSound, follows_sound};
pub use engine::{Engine, PRE_ROLL, SAMPLES_PER_BLOCK_TIME};
pub use hits::{BlockHit, HitCursor};
pub use master::{Master, MasterKeys};
pub use meters::{BusReading, FX_BINS, FX_METERS, FxReading, MeterReading, Meters};
pub use midi::{NoteLog, NoteOn, write_midi};
pub use offline::{Extras, has_sound, midi_path, render_samples, render_sound, render_to_wav, stem_paths};
pub use patch::{SoundPatch, Tables};
pub use ruled::{mixer_rule_keys, voice_rule_keys};
pub use tempo::TempoMap;
pub use voice::{BlockCx, VoiceSource};
pub use voices::{ShapeVoice, note_hz};

/// The kit, its worlds following sound: what the editor and the export run, so a physics
/// setting that follows a shape's sound moves the same everywhere.
pub fn kit() -> bpe_kit::Kit {
    let mut sims = bpe_kit::sims();
    sims.sound = Some(std::sync::Arc::new(FollowSound));
    bpe_kit::Kit { registry: bpe_kit::registry(), sims: std::sync::Arc::new(sims), draws: bpe_kit::draws() }
}
