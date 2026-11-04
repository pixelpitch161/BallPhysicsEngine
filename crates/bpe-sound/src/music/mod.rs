//! Music theory for the Notes: scales and keys, chord names, voice leading and the chord
//! presets. Pure functions, no sound.

pub mod chord_name;
pub mod presets;
pub mod scale;
pub mod voicing;

pub use chord_name::{chord_name, note_name};
pub use presets::{Mood, PRESETS, Preset, chord_labels, realise};
pub use scale::{in_scale, move_into_key, scale_steps};
pub use voicing::{ChordSpec, Voicing, voice_progression};
