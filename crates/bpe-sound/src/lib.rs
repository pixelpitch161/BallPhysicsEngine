//! Ball Studio's sound engine. An [`Engine`] reads a scene, runs its own copy of the physics in
//! step with the audio, places every hit on its exact sample, runs the voices and the master,
//! and renders blocks of [`BLOCK`] stereo samples at [`SAMPLE_RATE`]. The editor and the export
//! call the same [`Engine::render`]; nothing else makes sound.

pub mod chain;
pub mod engine;
pub mod hits;
pub mod master;
pub mod meters;
pub mod modmatrix;
pub mod offline;
pub mod patch;
pub mod tempo;
pub mod voice;
pub mod voices;
mod wav;

pub use bpe_dsp::{BLOCK, SAMPLE_RATE};
pub use engine::{Engine, PRE_ROLL, SAMPLES_PER_BLOCK_TIME};
pub use hits::{BlockHit, HitCursor};
pub use master::{Master, MasterKeys};
pub use meters::{MeterReading, Meters};
pub use offline::{has_sound, render_samples, render_to_wav};
pub use patch::SoundPatch;
pub use tempo::TempoMap;
pub use voice::{BlockCx, VoiceSource};
pub use voices::{ShapeVoice, note_hz};
